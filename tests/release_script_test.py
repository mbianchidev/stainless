"""Checks scripts/release.sh and its helper against synthetic Git, GitHub and build fakes.

Everything runs in a temporary directory with a disposable HOME: the "GitHub" remote is a
local bare repository reached through an SSH shim, and gh, cargo and ./tools/stainless are
fakes. No network, credentials, real builds or real releases are involved.
"""

import fcntl
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import release_helper as helper  # noqa: E402

REPOSITORY = "example/stainless"
REMOTE_URL = "git@github.com:example/stainless.git"
MANIFEST = """{
  "schema_version": 1,
  "chromium": {
    "revision": "cccccccccccccccccccccccccccccccccccccccc",
    "version": "158.0.8127.37"
  },
  "product": {
    "version": "%s",
    "build": %d,
    "channel": "preview",
    "profile_schema": 1
  },
  "patches": []
}
"""

FAKE_STAINLESS = r"""#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
printf 'stainless %s\n' "$*" >>"$FAKE_LOG"
field() { python3 -c 'import json, sys; value = json.load(open(sys.argv[1]))
for key in sys.argv[2:]: value = value[key]
print(value)' "$@"; }
case $1 in
security-status | test)
    if [[ ${FAKE_LEAK:-} == "$1" ]]; then
        python3 -c 'import time; time.sleep(300)' "$FAKE_MARKER-leaked" >/dev/null 2>&1 &
    fi
    [[ ${FAKE_FAIL:-} != "$1" ]]
    ;;
build)
    if [[ ${FAKE_FAIL:-} == slow-build ]]; then
        trap 'printf "build terminated\n" >>"$FAKE_LOG"; exit 143' TERM
        python3 -c 'import time; time.sleep(300)' "$FAKE_MARKER-helper" >/dev/null 2>&1 &
        python3 -c 'import signal, sys, time
signal.signal(signal.SIGTERM, signal.SIG_IGN)
open(sys.argv[1], "a").write("stubborn helper ready\n")
time.sleep(300)' "$FAKE_LOG" "$FAKE_MARKER-stubborn" >/dev/null 2>&1 &
        printf 'build started\n' >>"$FAKE_LOG"
        while :; do sleep 0.1; done
    fi
    [[ ${FAKE_FAIL:-} != build ]]
    ;;
package)
    version=$(field "$root/chromium/manifest.json" product version)
    out="$root/dist/$version"
    [[ ! -e $out ]]
    mkdir -p "$out"
    printf 'synthetic disk image %s\n' "$version" >"$out/Stainless-$version-macos-arm64.dmg"
    cp "$root/LICENSE" "$out/LICENSE"
    printf '{"product_revision": "%s", "dirty": false}\n' "$(git -C "$root" rev-parse HEAD)" \
        >"$out/stainless-build.json"
    printf '{"release_tag": "stainless-preview-v%s", "release": {"version": "%s"}}\n' \
        "$version" "$version" >"$out/stainless-preview.json"
    (cd "$out" && shasum -a 256 * >SHA256SUMS)
    ;;
publish)
    tag=$(field "$2/stainless-preview.json" release_tag)
    gh release create "$tag" --repo "$FAKE_REPOSITORY" --draft --prerelease \
        --target "$(git -C "$root" rev-parse HEAD)" --title Stainless --notes synthetic
    gh release upload "$tag" --repo "$FAKE_REPOSITORY" "$2"/*
    ;;
*) exit 2 ;;
esac
"""

FAKE_SSH = r"""#!/bin/bash
command=${@: -1}
service=${command%% *}
case $service in
git-upload-pack | git-receive-pack) exec git "${service#git-}" "$FAKE_REMOTE" ;;
esac
exit 1
"""

FAKE_CARGO = r"""#!/bin/bash
printf 'cargo %s\n' "$*" >>"$FAKE_LOG"
[[ ${FAKE_FAIL:-} != audit ]]
"""


def write_executable(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    path.chmod(0o755)


class HelperTest(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        self.manifest = self.directory / "manifest.json"
        self.manifest.write_text(MANIFEST % ("0.1.0-preview.8", 8))
        self.releases = self.directory / "releases.tsv"
        self.releases.write_text(
            "stainless-preview-v0.1.0-preview.4\tfalse\nstainless-preview-v0.1.0-preview.3\tfalse\n"
        )

    def tearDown(self):
        shutil.rmtree(self.directory)

    def plan(self, version):
        return helper.plan(version, str(self.manifest), str(self.releases))

    def test_plans_versions_and_warns_about_skipped_values(self):
        cases = [
            ("v0.1.0-preview.9", "1", "9", "skips preview 5-8"),
            ("0.1.0-preview.8", "0", "8", "skips preview 5-7"),
            ("0.2.0-preview.1", "1", "9", None),
            ("0.3.0-preview.1", "1", "9", "skips minor values"),
        ]
        for version, bump, build, warning in cases:
            with self.subTest(version=version), mock.patch("sys.stderr") as stderr:
                result = self.plan(version)
                self.assertEqual((result["BUMP"], result["BUILD"]), (bump, build))
                written = "".join(call.args[0] for call in stderr.write.call_args_list)
                if warning:
                    self.assertIn(warning, written)
                else:
                    self.assertNotIn("warning", written)
        self.assertEqual(self.plan("0.1.0-preview.4")["STATE"], "published")

    def test_rejects_invalid_and_backwards_versions(self):
        for version in ["1.0.0", "0.1.0-preview.01", "0.1.0-preview.9+build.1", " 0.1.0-preview.9",
                        "0.1.0-beta.9", "0.1.0-preview.7", "01.1.0-preview.9"]:
            with self.subTest(version=version), self.assertRaises(helper.ReleaseError):
                self.plan(version)

    def test_bump_changes_only_the_product_version_and_build(self):
        original = self.manifest.read_text()
        helper.bump(str(self.manifest), "0.1.0-preview.9", 9)
        expected = original.replace('"0.1.0-preview.8"', '"0.1.0-preview.9"').replace(
            '"build": 8', '"build": 9'
        )
        self.assertEqual(self.manifest.read_text(), expected)
        with self.assertRaises(helper.ReleaseError):
            helper.bump(str(self.manifest), "0.1.0-preview.8", 10)

    def test_asset_verification_requires_exact_names_and_digests(self):
        package = self.directory / "package"
        package.mkdir()
        (package / "Stainless.dmg").write_bytes(b"synthetic")
        digest = helper.sha256(package / "Stainless.dmg")
        (package / "SHA256SUMS").write_text(f"{digest}  Stainless.dmg\n")
        sums = helper.sha256(package / "SHA256SUMS")
        assets = self.directory / "assets.tsv"
        good = f"Stainless.dmg\t9\tsha256:{digest}\nSHA256SUMS\t80\tsha256:{sums}\n"
        assets.write_text(good)
        helper.verify_assets(str(package / "SHA256SUMS"), str(assets), None)
        for text in [good.replace(digest, "0" * 64), good + "extra.zip\t1\tsha256:00\n",
                     good.splitlines()[0] + "\n"]:
            assets.write_text(text)
            with self.subTest(assets=text), self.assertRaises(helper.ReleaseError):
                helper.verify_assets(str(package / "SHA256SUMS"), str(assets), None)


    def test_prune_keeps_locked_revisions_and_their_temporary_files(self):
        cache, dist = self.directory / "cache", self.directory / "dist"
        for revision in ("a" * 40, "b" * 40, "c" * 40):
            (cache / "chromium" / revision).mkdir(parents=True)
        (cache / ".tmpowned").mkdir()
        dist.mkdir()
        holder = os.open(cache / f".source-{'b' * 40}.lock", os.O_RDWR | os.O_CREAT)
        try:
            fcntl.flock(holder, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with mock.patch("sys.stderr"):
                preview = helper.prune(str(cache), "c" * 40, str(dist), "0.1.0-preview.8", True)
                self.assertEqual(preview["REMOVED"], "3")
                self.assertEqual(len(list((cache / "chromium").iterdir())), 3)
                result = helper.prune(str(cache), "c" * 40, str(dist), "0.1.0-preview.8", False)
        finally:
            os.close(holder)
        self.assertEqual((result["REMOVED"], result["SKIPPED"]), ("1", "2"))
        self.assertEqual(sorted(path.name for path in (cache / "chromium").iterdir()), ["b" * 40, "c" * 40])
        self.assertTrue((cache / ".tmpowned").is_dir())


class ReleaseScriptTest(unittest.TestCase):
    def setUp(self):
        self.base = Path(tempfile.mkdtemp()).resolve()
        self.remote = self.base / "remote.git"
        self.checkout = self.base / "checkout"
        self.log = self.base / "log"
        self.state_file = self.base / "github" / "state.json"
        bin_directory = self.base / "bin"
        write_executable(
            bin_directory / "gh",
            f'#!/bin/bash\nexec python3 "{ROOT / "tests/fixtures/release_gh.py"}" "$@"\n',
        )
        write_executable(bin_directory / "cargo", FAKE_CARGO)
        write_executable(bin_directory / "fake-ssh", FAKE_SSH)
        for directory in ("home", "tmp"):
            (self.base / directory).mkdir()
        self.env = {
            "PATH": f"{bin_directory}:{os.environ['PATH']}",
            "HOME": str(self.base / "home"),
            "XDG_CONFIG_HOME": str(self.base / "home"),
            "TMPDIR": str(self.base / "tmp"),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_SSH_COMMAND": str(bin_directory / "fake-ssh"),
            "GIT_SSH_VARIANT": "ssh",
            "GIT_AUTHOR_NAME": "Synthetic Maintainer",
            "GIT_AUTHOR_EMAIL": "maintainer@example.invalid",
            "GIT_COMMITTER_NAME": "Synthetic Maintainer",
            "GIT_COMMITTER_EMAIL": "maintainer@example.invalid",
            "FAKE_REMOTE": str(self.remote),
            "FAKE_REPOSITORY": REPOSITORY,
            "FAKE_GH_STATE": str(self.state_file),
            "FAKE_LOG": str(self.log),
            "STAINLESS_RELEASE_CHECKS_TIMEOUT": "5",
            "STAINLESS_RELEASE_MERGE_TIMEOUT": "5",
            "STAINLESS_RELEASE_RUN_TIMEOUT": "5",
            "STAINLESS_RELEASE_POLL_INTERVAL": "1",
            "STAINLESS_RELEASE_STOP_GRACE": "2",
            "FAKE_MARKER": f"stainless-release-test-{os.getpid()}-{id(self)}",
        }
        self.state_file.parent.mkdir()
        self.state = {
            "prs": [],
            "releases": {},
            "runs": [],
            "config": {"checks": "pass", "run": "success", "merge": "ok"},
        }
        self.save_state()
        self.log.write_text("")
        seed = self.base / "seed"
        self.git(self.base, "init", "--quiet", "--bare", "-b", "main", str(self.remote))
        self.git(self.base, "init", "--quiet", "-b", "main", str(seed))
        (seed / "chromium").mkdir()
        (seed / "chromium/manifest.json").write_text(MANIFEST % ("0.1.0-preview.8", 8))
        (seed / "components/release/src").mkdir(parents=True)
        (seed / "components/release/src/lib.rs").write_text(
            f'pub const REPOSITORY: &str = "{REPOSITORY}";\n'
        )
        (seed / "LICENSE").write_text("Synthetic license text\n")
        (seed / ".gitignore").write_text("/dist/\n/target/\n")
        write_executable(seed / "tools/stainless", FAKE_STAINLESS)
        (seed / "scripts").mkdir()
        for name in ("release.sh", "release_helper.py"):
            shutil.copy2(ROOT / "scripts" / name, seed / "scripts" / name)
        self.git(seed, "add", ".")
        self.git(seed, "commit", "--quiet", "-m", "Synthetic Stainless source")
        self.git(seed, "push", "--quiet", str(self.remote), "main")
        self.git(self.base, "clone", "--quiet", str(self.remote), str(self.checkout))
        self.git(self.checkout, "remote", "set-url", "origin", REMOTE_URL)

    def tearDown(self):
        for pid in self.marked_processes():
            os.kill(pid, signal.SIGKILL)
        shutil.rmtree(self.base)

    def marked_processes(self):
        result = subprocess.run(["pgrep", "-f", self.env["FAKE_MARKER"]], capture_output=True, text=True)
        return [int(pid) for pid in result.stdout.split()]

    def git(self, cwd, *args):
        return subprocess.run(
            ["git", *args], cwd=cwd, env=self.env, check=True, capture_output=True, text=True
        ).stdout.strip()

    def save_state(self):
        self.state_file.write_text(json.dumps(self.state))

    def load_state(self):
        self.state = json.loads(self.state_file.read_text())
        return self.state

    def configure(self, **values):
        self.load_state()["config"].update(values)
        self.save_state()

    def release(self, *args, **extra):
        return subprocess.run(
            [str(self.checkout / "scripts/release.sh"), "--isolated-host", *args],
            cwd=self.checkout,
            env={**self.env, **extra},
            capture_output=True,
            text=True,
            timeout=120,
        )

    def remote_ref(self, name):
        return subprocess.run(
            ["git", "--git-dir", str(self.remote), "rev-parse", "-q", "--verify", name],
            env=self.env, capture_output=True, text=True,
        ).stdout.strip()

    def logged(self):
        return self.log.read_text().splitlines()

    def assert_clean(self):
        worktrees = self.git(self.checkout, "worktree", "list", "--porcelain")
        self.assertEqual(worktrees.count("worktree "), 1, worktrees)
        self.assertEqual(self.git(self.checkout, "status", "--porcelain"), "")
        self.assertEqual(list((self.base / "tmp").iterdir()), [])
        state = Path(self.git(self.checkout, "rev-parse", "--path-format=absolute", "--git-common-dir"))
        self.assertFalse((state / "stainless-release/lock").exists())

    def test_help_and_invalid_versions_have_no_side_effects(self):
        result = self.release("--help")
        self.assertEqual(result.returncode, 0)
        self.assertIn("Usage: ./scripts/release.sh", result.stdout)
        for version in ["1.0.0", "0.1.0-preview.7", "0.1.0-preview.9+meta"]:
            with self.subTest(version=version):
                result = self.release(version)
                self.assertEqual(result.returncode, 2, result.stderr)
        self.assertFalse([line for line in self.logged() if " create" in line or "push" in line])
        self.assert_clean()

    def test_dry_run_creates_nothing(self):
        main = self.remote_ref("refs/heads/main")
        result = self.release("--dry-run", "v0.1.0-preview.9")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Would open "release: 0.1.0-preview.9"', result.stdout)
        self.assertEqual(self.remote_ref("refs/heads/main"), main)
        self.assertEqual(self.load_state()["prs"], [])
        self.assertEqual(self.state["releases"], {})
        self.assertFalse(any(line.startswith(("stainless", "cargo")) for line in self.logged()))
        self.assert_clean()

    def test_release_merges_bump_builds_merged_commit_and_verifies_publication(self):
        result = self.release("0.1.0-preview.9")
        self.assertEqual(result.returncode, 0, result.stderr)
        state = self.load_state()
        (pull,) = state["prs"]
        self.assertEqual((pull["state"], pull["title"]), ("MERGED", "release: 0.1.0-preview.9"))
        merge = pull["mergeCommit"]["oid"]
        self.assertEqual(self.remote_ref("refs/heads/main"), merge)
        self.assertEqual(self.remote_ref("refs/tags/stainless-preview-v0.1.0-preview.9^{commit}"), merge)
        self.assertEqual(
            self.git(self.base, "--git-dir", str(self.remote), "cat-file", "-t",
                     "refs/tags/stainless-preview-v0.1.0-preview.9"),
            "tag",
        )
        manifest = self.git(self.base, "--git-dir", str(self.remote), "show", f"{merge}:chromium/manifest.json")
        self.assertIn('"version": "0.1.0-preview.9",\n    "build": 9,', manifest)
        release = state["releases"]["stainless-preview-v0.1.0-preview.9"]
        self.assertEqual((release["isDraft"], release["targetCommitish"]), (False, merge))
        steps = [line for line in self.logged() if line.startswith(("stainless", "cargo audit --deny"))]
        self.assertEqual(
            [step.split(" /")[0] for step in steps],
            [
                "stainless security-status", "cargo audit --deny warnings",
                "stainless security-status", "cargo audit --deny warnings", "stainless test",
                "stainless build --jobs 8", "stainless package", "stainless publish",
            ],
        )
        self.assertTrue((self.checkout / "dist/0.1.0-preview.9/SHA256SUMS").is_file())
        self.assertIn("Released stainless-preview-v0.1.0-preview.9", result.stdout)
        self.assert_clean()

        before = len(self.logged())
        result = self.release("0.1.0-preview.9")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("already published and verified", result.stderr)
        self.assertFalse([line for line in self.logged()[before:] if " create" in line or line.startswith("stainless")])

    def test_failed_checks_leave_the_pull_request_open_and_untagged(self):
        self.configure(checks="fail")
        result = self.release("0.1.0-preview.9")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Checks did not all pass", result.stderr)
        self.assertIn("bump PR:", result.stderr)
        self.assertEqual(self.load_state()["prs"][0]["state"], "OPEN")
        self.assertEqual(self.state["releases"], {})
        self.assertEqual(self.remote_ref("refs/tags/stainless-preview-v0.1.0-preview.9"), "")
        self.assertFalse([line for line in self.logged() if line.startswith("stainless build")])
        self.assert_clean()

    def test_refused_merge_stays_recoverable(self):
        self.configure(merge="refuse")
        result = self.release("0.1.0-preview.9")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("could not merge or enable auto-merge", result.stderr)
        self.configure(merge="ok")
        result = self.release("0.1.0-preview.9")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.load_state()["prs"]), 1)
        self.assertIn("Resuming https://github.com/example/stainless/pull/100", result.stderr)

    def test_failed_release_workflow_is_resumed_without_rebuilding(self):
        self.configure(run="failure")
        result = self.release("0.1.0-preview.9")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Release workflow did not succeed", result.stderr)
        self.assertIn("synthetic failure log", result.stderr)
        self.assertTrue(self.load_state()["releases"]["stainless-preview-v0.1.0-preview.9"]["isDraft"])
        self.assert_clean()
        self.configure(run="success")
        before = len(self.logged())
        result = self.release("0.1.0-preview.9")
        self.assertEqual(result.returncode, 0, result.stderr)
        later = self.logged()[before:]
        self.assertFalse([line for line in later if line.startswith("stainless")])
        self.assertTrue([line for line in later if line.startswith("gh run rerun 1000")])
        self.assertEqual(self.load_state()["runs"][0]["attempt"], 2)
        self.assertFalse(self.state["releases"]["stainless-preview-v0.1.0-preview.9"]["isDraft"])

    def test_mismatched_published_digest_fails_verification(self):
        self.configure(tamper="Stainless-0.1.0-preview.9-macos-arm64.dmg")
        result = self.release("0.1.0-preview.9")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Stainless-0.1.0-preview.9-macos-arm64.dmg: published digest", result.stderr)

    def test_version_already_on_base_releases_without_a_bump(self):
        head = self.remote_ref("refs/heads/main")
        result = self.release("0.1.0-preview.8")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.load_state()["prs"], [])
        self.assertEqual(self.remote_ref("refs/tags/stainless-preview-v0.1.0-preview.8^{commit}"), head)

    def test_build_requires_an_isolated_host_before_any_change(self):
        result = subprocess.run(
            [str(self.checkout / "scripts/release.sh"), "0.1.0-preview.9"],
            cwd=self.checkout, env=self.env, capture_output=True, text=True, timeout=120,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("pass --isolated-host", result.stderr)
        self.assertEqual(self.load_state()["prs"], [])
        self.assertFalse([line for line in self.logged() if line.startswith(("stainless", "cargo audit --deny"))])
        self.assert_clean()

    def test_rejects_an_unexpected_push_destination(self):
        self.git(self.checkout, "remote", "set-url", "--push", "origin", "git@github.com:fork/stainless.git")
        result = self.release("--dry-run", "0.1.0-preview.9")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not github.com/example/stainless", result.stderr)

    def test_sigterm_stops_the_whole_build_process_group(self):
        process = subprocess.Popen(
            [str(self.checkout / "scripts/release.sh"), "--isolated-host", "0.1.0-preview.8"],
            cwd=self.checkout,
            env={**self.env, "FAKE_FAIL": "slow-build"},
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        deadline = time.monotonic() + 60
        while not {"build started", "stubborn helper ready"} <= set(self.logged()):
            self.assertLess(time.monotonic(), deadline, "the fake build never started")
            time.sleep(0.1)
        self.assertEqual(len(self.marked_processes()), 2)
        process.send_signal(signal.SIGTERM)
        _, stderr = process.communicate(timeout=60)
        self.assertEqual(process.returncode, 143, stderr)
        self.assertIn("build terminated", self.logged())
        self.assertIn("ignored SIGTERM for 2s; sending SIGKILL", stderr)
        self.assertEqual(self.marked_processes(), [])
        self.assertEqual(self.load_state()["releases"], {})
        self.assert_clean()

    def test_processes_left_by_a_step_are_stopped(self):
        result = self.release("0.1.0-preview.8", FAKE_LEAK="test")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("The step left processes running", result.stderr)
        self.assertIn(f"{self.env['FAKE_MARKER']}-leaked", result.stderr)
        self.assertEqual(self.marked_processes(), [])

    def test_verified_release_prunes_other_caches_and_older_packages(self):
        cache = self.base / "cache"
        current, old = "c" * 40, "a" * 40
        for revision in (current, old):
            (cache / "chromium" / revision / "src").mkdir(parents=True)
            (cache / "chromium" / revision / "src/DEPS").write_text("synthetic\n")
        (cache / f".source-{old}.lock").write_text("")
        (cache / ".tmpinterrupted").mkdir()
        (cache / "stainless-dmg-orphan").mkdir()
        for name in ("0.1.0-preview.4", "0.1.0-preview.99"):
            (self.checkout / "dist" / name).mkdir(parents=True)
            (self.checkout / "dist" / name / "SHA256SUMS").write_text("synthetic\n")
        (self.checkout / "dist/notes").mkdir()
        environment = {"STAINLESS_CACHE_DIR": str(cache)}

        result = self.release("--keep-artifacts", "0.1.0-preview.8", **environment)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--keep-artifacts", result.stderr)
        self.assertTrue((cache / "chromium" / old).is_dir())
        self.assertTrue((self.checkout / "dist/0.1.0-preview.4").is_dir())

        result = self.release("--dry-run", "0.1.0-preview.8", **environment)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("would remove 4 cache/package/temporary directories", result.stdout)
        self.assertTrue((cache / "chromium" / old).is_dir())

        result = self.release("0.1.0-preview.8", **environment)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Removed 4 directories", result.stderr)
        self.assertEqual(sorted(path.name for path in (cache / "chromium").iterdir()), [current])
        self.assertFalse((cache / f".source-{old}.lock").exists())
        self.assertFalse((cache / ".tmpinterrupted").exists() or (cache / "stainless-dmg-orphan").exists())
        self.assertEqual(
            sorted(path.name for path in (self.checkout / "dist").iterdir()),
            ["0.1.0-preview.8", "0.1.0-preview.99", "notes"],
        )
        self.assert_clean()

if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""Version policy, manifest and asset checks for scripts/release.sh.

Subcommands print validated KEY=VALUE lines for the shell entrypoint, warnings on
stderr, and exit 2 for invalid or backwards release input.
"""

import argparse
import fcntl
import hashlib
import json
import os
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

TAG_PREFIX = "stainless-preview-v"
SEMVER = re.compile(
    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-((?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?",
    re.ASCII,
)
PRODUCT_BLOCK = re.compile(r'"product"\s*:\s*\{[^{}]*\}')
COMPONENTS = ("major", "minor", "patch")
REVISION = re.compile(r"[0-9a-f]{40}")
# Temporary directories ./tools/stainless creates in the build cache and dist/.
CACHE_TEMPORARY = re.compile(r"(\.tmp|stainless-dmg-).+")


class ReleaseError(Exception):
    """Invalid or unsafe release input."""


class Version:
    def __init__(self, text):
        match = SEMVER.fullmatch(text)
        if not match:
            raise ReleaseError(f"{text!r} is not a SemVer 2.0.0 version")
        self.text = text
        self.core = tuple(int(part) for part in match.group(1, 2, 3))
        self.pre = tuple(match.group(4).split(".")) if match.group(4) else ()
        self.build = match.group(5) or ""

    def key(self):
        # SemVer precedence: a release outranks its prereleases, numeric identifiers rank
        # below alphanumeric ones, longer identifier lists win ties and build metadata is ignored.
        pre = tuple((0, int(part), "") if part.isdigit() else (1, 0, part) for part in self.pre)
        return self.core, not self.pre, pre

    def __lt__(self, other):
        return self.key() < other.key()

    def __eq__(self, other):
        return self.key() == other.key()

    def __str__(self):
        return self.text


def parse_requested(text):
    if not text or text != text.strip():
        raise ReleaseError("VERSION must not be empty or contain whitespace")
    version = Version(text[1:] if text.startswith("v") else text)
    if version.build:
        raise ReleaseError(
            f"{text}: build metadata is not supported by the Stainless product manifest; "
            "use MAJOR.MINOR.PATCH-preview.N"
        )
    if len(version.pre) != 2 or version.pre[0] != "preview" or not version.pre[1].isdigit():
        raise ReleaseError(
            f"{text}: Stainless releases use the preview channel form MAJOR.MINOR.PATCH-preview.N"
        )
    if int(version.pre[1]) < 1:
        raise ReleaseError(f"{text}: the preview sequence starts at 1")
    return version


def read_manifest(path):
    text = sys.stdin.read() if path == "-" else Path(path).read_text(encoding="utf-8")
    data = json.loads(text)
    product = data.get("product")
    if not isinstance(product, dict):
        raise ReleaseError("The source manifest has no product object")
    version, build = product.get("version"), product.get("build")
    if not isinstance(version, str) or type(build) is not int or build < 1:
        raise ReleaseError("The source manifest product version/build is invalid")
    return text, data, parse_requested(version), build


def read_releases(path):
    published, drafts = [], []
    if not path:
        return published, drafts
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        tag, _, draft = line.partition("\t")
        if not tag.startswith(TAG_PREFIX):
            continue
        try:
            version = parse_requested(tag[len(TAG_PREFIX):])
        except ReleaseError:
            print(f"warning: ignoring unrecognized release tag {tag}", file=sys.stderr)
            continue
        (drafts if draft == "true" else published).append(version)
    return published, drafts


def gap_warnings(previous, chosen):
    for index, name in enumerate(COMPONENTS):
        if chosen.core[index] != previous.core[index]:
            if chosen.core[index] - previous.core[index] > 1:
                return [f"{chosen} skips {name} values after the previous release {previous}"]
            return []
    if previous.pre[:1] == chosen.pre[:1] == ("preview",):
        first, last = int(previous.pre[1]) + 1, int(chosen.pre[1]) - 1
        if last >= first:
            span = str(first) if first == last else f"{first}-{last}"
            return [f"{chosen} skips preview {span} after the previous release {previous}"]
    return []


def plan(requested, manifest, releases):
    chosen = parse_requested(requested)
    _, _, current, current_build = read_manifest(manifest)
    published, drafts = read_releases(releases)
    if any(version == chosen for version in published):
        state = "published"
    elif any(version == chosen for version in drafts):
        state = "draft"
    else:
        state = "new"
    newer = [version for version in published if chosen < version]
    if newer and state == "new":
        raise ReleaseError(
            f"{chosen} is older than the published release {max(newer, key=Version.key)}"
        )
    if chosen < current and state == "new":
        raise ReleaseError(
            f"The release base already targets {current}; choose {current} or newer"
        )
    bump = current < chosen
    build = current_build + 1 if bump else current_build
    older = [version for version in published if version < chosen]
    previous = max(older, key=Version.key) if older else None
    if state == "new" and previous:
        for warning in gap_warnings(previous, chosen):
            print(f"warning: {warning}", file=sys.stderr)
    return {
        "VERSION": str(chosen),
        "TAG": TAG_PREFIX + str(chosen),
        "BUILD": str(build),
        "BUMP": "1" if bump else "0",
        "CURRENT_VERSION": str(current),
        "CURRENT_BUILD": str(current_build),
        "PREVIOUS": str(previous) if previous else "",
        "STATE": state,
    }


def bump(manifest, requested, build):
    path = Path(manifest)
    text, _, current, current_build = read_manifest(manifest)
    chosen = parse_requested(requested)
    if not (current < chosen and build > current_build):
        raise ReleaseError(f"Refusing to move {current} (build {current_build}) backwards")
    blocks = PRODUCT_BLOCK.findall(text)
    if len(blocks) != 1:
        raise ReleaseError("Cannot locate exactly one product block in the manifest")
    block = blocks[0]
    updated, versions = re.subn(
        r'("version"\s*:\s*)"[^"]*"', lambda match: f'{match.group(1)}"{chosen}"', block
    )
    updated, builds = re.subn(
        r'("build"\s*:\s*)[0-9]+', lambda match: f"{match.group(1)}{build}", updated
    )
    if versions != 1 or builds != 1:
        raise ReleaseError("The product block must contain one version and one build")
    result = text.replace(block, updated, 1)
    expected = json.loads(text)
    expected["product"]["version"] = str(chosen)
    expected["product"]["build"] = build
    if json.loads(result) != expected:
        raise ReleaseError("Manifest rewrite changed more than the product version/build")
    handle, temporary = tempfile.mkstemp(dir=path.parent, prefix=".manifest.")
    try:
        with os.fdopen(handle, "w", encoding="utf-8") as output:
            output.write(result)
        os.chmod(temporary, path.stat().st_mode & 0o777)
        os.replace(temporary, path)
    except BaseException:
        Path(temporary).unlink(missing_ok=True)
        raise
    return {"VERSION": str(chosen), "BUILD": str(build)}


def manifest_info(manifest):
    _, data, version, build = read_manifest(manifest)
    revision = data.get("chromium", {}).get("revision")
    if not isinstance(revision, str) or not REVISION.fullmatch(revision):
        raise ReleaseError("The source manifest has no full Chromium revision")
    return {"VERSION": str(version), "BUILD": str(build), "CHROMIUM": revision}


def package_info(directory):
    directory = Path(directory)
    build = json.loads((directory / "stainless-build.json").read_text(encoding="utf-8"))
    feed = json.loads((directory / "stainless-preview.json").read_text(encoding="utf-8"))
    revision = build.get("product_revision")
    if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ReleaseError(f"{directory} has no valid product source revision")
    if build.get("dirty") is not False:
        raise ReleaseError(f"{directory} was built from a dirty checkout")
    return {
        "REVISION": revision,
        "VERSION": str(parse_requested(feed["release"]["version"])),
        "TAG": feed["release_tag"],
    }


def read_sums(path):
    sums = {}
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        digest, separator, name = line.partition("  ")
        if not separator or not re.fullmatch(r"[0-9a-f]{64}", digest) or "/" in name:
            raise ReleaseError(f"Malformed SHA256SUMS entry: {line!r}")
        sums[name] = digest
    if not sums:
        raise ReleaseError("SHA256SUMS is empty")
    return sums


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_assets(sums_path, assets_path, directory):
    """Compare published asset names, sizes and digests with the package's SHA256SUMS."""
    expected = read_sums(sums_path)
    expected["SHA256SUMS"] = sha256(sums_path)
    published = {}
    for line in Path(assets_path).read_text(encoding="utf-8").splitlines():
        if line.strip():
            name, size, digest = (line.split("\t") + ["", ""])[:3]
            published[name] = (int(size), digest)
    problems = [f"unexpected asset {name}" for name in sorted(set(published) - set(expected))]
    for name, digest in sorted(expected.items()):
        if name not in published:
            problems.append(f"missing asset {name}")
            continue
        size, published_digest = published[name]
        if published_digest != f"sha256:{digest}":
            problems.append(
                f"{name}: published digest {published_digest or 'unavailable'}, "
                f"expected sha256:{digest}"
            )
        if directory and (Path(directory) / name).stat().st_size != size:
            problems.append(f"{name}: published size {size} differs from the local package")
    if problems:
        raise ReleaseError("Release assets do not match SHA256SUMS:\n  " + "\n  ".join(problems))
    return {"ASSETS": str(len(expected))}


def lock(path):
    """Take the coordinator's per-revision cache lock without waiting, or return None."""
    handle = os.open(path, os.O_RDWR | os.O_CREAT, 0o644)
    try:
        fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        os.close(handle)
        return None
    return handle


def references():
    """Working directories and command lines of other running processes."""
    own = {os.getpid(), os.getppid()}
    texts = []
    try:
        listing = subprocess.run(["lsof", "-w", "-d", "cwd", "-Fpn"], capture_output=True, text=True)
        processes = subprocess.run(["ps", "-A", "-o", "pid=,command="], capture_output=True, text=True, check=True)
    except (OSError, subprocess.CalledProcessError) as error:
        raise ReleaseError(f"Cannot inspect running processes: {error}") from error
    pid = None
    for line in listing.stdout.splitlines():
        if line.startswith("p"):
            pid = int(line[1:])
        elif line.startswith("n") and pid not in own:
            texts.append(line[1:])
    for line in processes.stdout.splitlines():
        number, _, command = line.strip().partition(" ")
        if number.isdigit() and int(number) not in own:
            texts.append(command)
    mounts = subprocess.run(["mount"], capture_output=True, text=True).stdout
    texts += re.findall(r" on (/.*?) \(", mounts)
    return texts


def in_use(path, texts):
    pattern = re.compile(
        "|".join(re.escape(str(root)) for root in {path, path.resolve()}) + r"(?=$|[/\s'\"])"
    )
    return any(pattern.search(text) for text in texts)


def remove_tree(path):
    def writable(function, name, *_):
        os.chmod(os.path.dirname(name), stat.S_IRWXU)
        if os.path.isdir(name) and not os.path.islink(name):
            os.chmod(name, stat.S_IRWXU)
        function(name)

    if sys.version_info >= (3, 12):
        shutil.rmtree(path, onexc=writable)
    else:
        shutil.rmtree(path, onerror=writable)


def free_space(paths):
    devices = {}
    for path in paths:
        while not path.exists():
            path = path.parent
        result = os.statvfs(path)
        devices[os.stat(path).st_dev] = result.f_bavail * result.f_frsize
    return sum(devices.values())


def prune(cache, keep_revision, dist, keep_version, dry_run):
    """Remove other Chromium revision caches, older packages and orphaned temporary files."""
    if not REVISION.fullmatch(keep_revision):
        raise ReleaseError(f"{keep_revision!r} is not a full Chromium revision")
    keep = parse_requested(keep_version)
    cache, dist = Path(cache), Path(dist)
    if not cache.is_absolute() or not dist.is_absolute():
        raise ReleaseError("The cache and dist paths must be absolute")
    before = free_space([cache, dist])
    texts = references()
    candidates, skipped, handles = [], [], []

    revisions = sorted(
        entry for entry in (cache / "chromium").glob("*")
        if entry.is_dir() and not entry.is_symlink() and REVISION.fullmatch(entry.name)
    ) if (cache / "chromium").is_dir() else []
    lock_names = {f".source-{entry.name}.lock" for entry in revisions} | {f".source-{keep_revision}.lock"}
    lock_names |= {entry.name for entry in cache.glob(".source-*.lock")} if cache.is_dir() else set()
    busy = set()
    try:
        # A dry run takes no locks, so it can never make a concurrent build fail to start.
        if cache.is_dir() and not dry_run:
            for name in sorted(lock_names):
                handle = lock(cache / name)
                if handle is None:
                    busy.add(name)
                else:
                    handles.append(handle)
        for entry in revisions:
            if entry.name == keep_revision:
                continue
            if f".source-{entry.name}.lock" in busy:
                skipped.append((entry, "a Stainless build or package is using it"))
            elif in_use(entry, texts):
                skipped.append((entry, "a running process uses it"))
            else:
                candidates.append((entry, cache / f".source-{entry.name}.lock"))
        temporary = [
            entry for root in (cache, dist) if root.is_dir() for entry in root.iterdir()
            if CACHE_TEMPORARY.fullmatch(entry.name) and entry.is_dir() and not entry.is_symlink()
        ]
        for entry in sorted(temporary):
            if busy:
                skipped.append((entry, "a Stainless build or package may own it"))
            elif in_use(entry, texts):
                skipped.append((entry, "a running process or mounted image uses it"))
            else:
                candidates.append((entry, None))
        for entry in sorted(dist.iterdir()) if dist.is_dir() else []:
            try:
                version = parse_requested(entry.name)
            except ReleaseError:
                continue
            if (version < keep and entry.is_dir() and not entry.is_symlink()
                    and (entry / "SHA256SUMS").is_file()):
                if in_use(entry, texts):
                    skipped.append((entry, "a running process uses it"))
                else:
                    candidates.append((entry, None))
        removed = 0
        for entry, lock_file in candidates:
            if dry_run:
                print(f"  would remove {entry}", file=sys.stderr)
                continue
            print(f"  removing {entry}", file=sys.stderr)
            try:
                remove_tree(entry)
            except OSError as error:
                skipped.append((entry, f"removal failed: {error}"))
                continue
            if lock_file is not None:
                lock_file.unlink(missing_ok=True)
            removed += 1
    finally:
        for handle in handles:
            os.close(handle)
    for entry, reason in skipped:
        print(f"  kept {entry}: {reason}", file=sys.stderr)
    freed = max(free_space([cache, dist]) - before, 0) if removed else 0
    return {
        "REMOVED": str(len(candidates) if dry_run else removed),
        "SKIPPED": str(len(skipped)),
        "FREED_MB": str(freed // (1024 * 1024)),
    }


def exec_group(command):
    """Run COMMAND as its own process group with default interrupt handling.

    Background commands of a non-interactive shell ignore SIGINT; the release script signals
    the whole group instead so helpers started by ./tools/stainless stop with it.
    """
    os.setpgid(0, 0)
    for number in (signal.SIGINT, signal.SIGQUIT):
        signal.signal(number, signal.SIG_DFL)
    try:
        os.execvp(command[0], command)
    except OSError as error:
        print(f"error: cannot run {command[0]}: {error}", file=sys.stderr)
        return 127


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    command = commands.add_parser("plan", help="validate VERSION and plan the release")
    command.add_argument("version")
    command.add_argument("--manifest", required=True)
    command.add_argument("--releases", help="TAG<TAB>isDraft lines")
    command = commands.add_parser("bump", help="set the product version/build in place")
    command.add_argument("--manifest", required=True)
    command.add_argument("--version", required=True)
    command.add_argument("--build", required=True, type=int)
    command = commands.add_parser("manifest", help="print the product version/build")
    command.add_argument("manifest")
    command = commands.add_parser("package", help="print a local package's identity")
    command.add_argument("directory")
    command = commands.add_parser("verify-assets", help="match published assets to SHA256SUMS")
    command.add_argument("--sums", required=True)
    command.add_argument("--assets", required=True, help="NAME<TAB>SIZE<TAB>DIGEST lines")
    command.add_argument("--directory")
    command = commands.add_parser("prune", help="remove other caches and older packages")
    command.add_argument("--cache", required=True)
    command.add_argument("--keep-revision", required=True)
    command.add_argument("--dist", required=True)
    command.add_argument("--keep-version", required=True)
    command.add_argument("--dry-run", action="store_true")
    argv = sys.argv[1:] if argv is None else argv
    if argv[:1] == ["exec-group"] and len(argv) > 1:
        return exec_group(argv[1:])
    args = parser.parse_args(argv)
    try:
        if args.command == "plan":
            result = plan(args.version, args.manifest, args.releases)
        elif args.command == "bump":
            result = bump(args.manifest, args.version, args.build)
        elif args.command == "manifest":
            result = manifest_info(args.manifest)
        elif args.command == "package":
            result = package_info(args.directory)
        elif args.command == "prune":
            result = prune(args.cache, args.keep_revision, args.dist, args.keep_version, args.dry_run)
        else:
            result = verify_assets(args.sums, args.assets, args.directory)
    except (ReleaseError, KeyError, ValueError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2 if isinstance(error, ReleaseError) else 1
    for key, value in result.items():
        print(f"{key}={value}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Synthetic GitHub CLI for tests/release_script_test.py.

Implements only the gh subcommands scripts/release.sh and the fake ./tools/stainless use,
against a JSON state file and a local bare repository. No network access.
"""

import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

STATE = Path(os.environ["FAKE_GH_STATE"])
REMOTE = os.environ["FAKE_REMOTE"]
REPO = os.environ["FAKE_REPOSITORY"]
PREFIX = "stainless-preview-v"
CHECKS = ("macOS source and native checks", "Dependency audit")


def load():
    return json.loads(STATE.read_text())


def save(state):
    STATE.write_text(json.dumps(state, indent=2))


def git(*args):
    return subprocess.run(
        ["git", "--git-dir", REMOTE, *args], check=True, capture_output=True, text=True
    ).stdout.strip()


def ref(name):
    result = subprocess.run(
        ["git", "--git-dir", REMOTE, "rev-parse", "-q", "--verify", f"{name}^{{commit}}"],
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def option(args, name, default=None):
    if name in args:
        return args[args.index(name) + 1]
    for argument in args:
        if argument.startswith(name + "="):
            return argument.split("=", 1)[1]
    return default


def emit(value, args):
    expression = option(args, "--jq")
    text = json.dumps(value)
    if expression is None:
        print(text)
        return
    result = subprocess.run(["jq", "-r", expression], input=text, capture_output=True, text=True)
    sys.stdout.write(result.stdout)
    sys.stderr.write(result.stderr)
    if result.returncode:
        sys.exit(result.returncode)


def release_json(tag, release):
    return {
        "tagName": tag,
        "isDraft": release["isDraft"],
        "isPrerelease": release["isPrerelease"],
        "targetCommitish": release["targetCommitish"],
        "url": f"https://github.com/{REPO}/releases/tag/{tag}",
        "assets": [
            {"name": name, "size": asset["size"], "digest": asset["digest"]}
            for name, asset in sorted(release["assets"].items())
        ],
    }


def pull(state, url):
    for item in state["prs"]:
        if item["url"] == url:
            return item
    print(f"no pull request {url}", file=sys.stderr)
    sys.exit(1)


def evaluate_run(state, run):
    """A release run publishes a complete matching draft, like .github/workflows/release.yml."""
    release = state["releases"].get(run["headBranch"])
    success = (
        state["config"]["run"] == "success"
        and release is not None
        and release["targetCommitish"] == run["headSha"]
    )
    run.update(status="completed", conclusion="success" if success else "failure")
    if success:
        release["isDraft"] = False


def sync_runs(state):
    known = {run["headBranch"] for run in state["runs"]}
    for line in git("for-each-ref", "--format=%(refname:short)", "refs/tags").splitlines():
        if line.startswith(PREFIX) and line not in known:
            number = 1000 + len(state["runs"])
            run = {
                "databaseId": number,
                "headBranch": line,
                "headSha": ref(f"refs/tags/{line}"),
                "attempt": 1,
                "url": f"https://github.com/{REPO}/actions/runs/{number}",
            }
            evaluate_run(state, run)
            state["runs"].append(run)


def run_by_id(state, identifier):
    for run in state["runs"]:
        if str(run["databaseId"]) == identifier:
            return run
    sys.exit(f"no run {identifier}")


def main(args):
    with open(os.environ["FAKE_LOG"], "a", encoding="utf-8") as log:
        log.write("gh " + " ".join(args) + "\n")
    state = load()
    group, command, rest = args[0], args[1] if len(args) > 1 else "", args[2:]
    if group == "auth":
        return 0
    if group == "release":
        releases = state["releases"]
        if command == "list":
            emit([{"tagName": tag, "isDraft": item["isDraft"]} for tag, item in releases.items()], rest)
        elif command == "view":
            if rest[0] not in releases:
                print("release not found", file=sys.stderr)
                return 1
            emit(release_json(rest[0], releases[rest[0]]), rest)
        elif command == "create":
            if rest[0] in releases:
                print("release already exists", file=sys.stderr)
                return 1
            releases[rest[0]] = {
                "isDraft": "--draft" in rest,
                "isPrerelease": "--prerelease" in rest,
                "targetCommitish": option(rest, "--target"),
                "assets": {},
            }
        elif command == "upload":
            storage = STATE.parent / "assets" / rest[0]
            storage.mkdir(parents=True, exist_ok=True)
            for path in rest[3:]:
                name = Path(path).name
                shutil.copyfile(path, storage / name)
                digest = hashlib.sha256(Path(path).read_bytes()).hexdigest()
                if state["config"].get("tamper") == name:
                    digest = "0" * 64
                releases[rest[0]]["assets"][name] = {
                    "size": Path(path).stat().st_size,
                    "digest": f"sha256:{digest}",
                }
        elif command == "download":
            directory = Path(option(rest, "--dir"))
            directory.mkdir(parents=True, exist_ok=True)
            name = option(rest, "--pattern")
            shutil.copyfile(STATE.parent / "assets" / rest[0] / name, directory / name)
        else:
            return 2
    elif group == "pr":
        if command == "list":
            head, base = option(rest, "--head"), option(rest, "--base")
            emit([item for item in state["prs"] if item["headRefName"] == head and item["baseRefName"] == base], rest)
        elif command == "create":
            head = option(rest, "--head")
            url = f"https://github.com/{REPO}/pull/{100 + len(state['prs'])}"
            state["prs"].append({
                "url": url,
                "state": "OPEN",
                "headRefName": head,
                "baseRefName": option(rest, "--base"),
                "headRefOid": ref(f"refs/heads/{head}"),
                "isCrossRepository": False,
                "mergeCommit": None,
                "title": option(rest, "--title"),
            })
            print(url)
        elif command == "checks":
            item = pull(state, rest[0])
            bucket = state["config"]["checks"]
            checks = [
                {"name": name, "bucket": bucket, "link": f"https://github.com/{REPO}/actions/runs/1/job/{index}"}
                for index, name in enumerate(CHECKS)
            ]
            if "--watch" in rest:
                for check in checks:
                    print(f"{check['bucket']}\t{check['name']}\t1m0s\t{check['link']}")
                return 0 if bucket == "pass" else 1
            emit(checks, rest)
        elif command == "view":
            item = pull(state, rest[0])
            emit(item, rest)
        elif command == "merge":
            item = pull(state, rest[0])
            if state["config"]["merge"] != "ok" or option(rest, "--match-head-commit") != item["headRefOid"]:
                print("merge refused", file=sys.stderr)
                return 1
            tree = git("rev-parse", item["headRefOid"] + "^{tree}")
            commit = git("commit-tree", tree, "-p", ref("refs/heads/main"), "-m", option(rest, "--subject"))
            git("update-ref", "refs/heads/main", commit)
            git("update-ref", "-d", f"refs/heads/{item['headRefName']}")
            item.update(state="MERGED", mergeCommit={"oid": commit})
        else:
            return 2
    elif group == "run":
        sync_runs(state)
        if command == "list":
            emit(state["runs"][::-1], rest)
        elif command == "view":
            run = run_by_id(state, rest[0])
            if "--log-failed" in rest:
                print("publish\tVerify\tsynthetic failure log")
            else:
                emit(run, rest)
        elif command == "watch":
            run = run_by_id(state, rest[0])
            print(f"{run['url']} {run['conclusion']}")
            save(state)
            return 0 if run["conclusion"] == "success" else 1
        elif command == "rerun":
            run = run_by_id(state, rest[0])
            run["attempt"] += 1
            evaluate_run(state, run)
        else:
            return 2
    else:
        return 2
    save(state)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

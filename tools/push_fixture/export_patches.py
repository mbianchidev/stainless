#!/usr/bin/env python3
"""Maintainer-only: export intentionally reviewed changes, then review the diff.

Never called by prepare/build/start. Updating these pins does not approve code.
"""

import argparse
import difflib
import hashlib
import json
from pathlib import Path
import tarfile

import fixture


def original_files(archive: Path, prefix: str) -> dict[str, bytes]:
    with tarfile.open(archive) as source:
        return {
            member.name.removeprefix(prefix + "/"): source.extractfile(member).read()
            for member in source.getmembers()
            if member.isfile() and member.name.startswith(prefix + "/")
        }


def export(original: dict[str, bytes], root: Path, destination: Path) -> None:
    changes = []
    for relative, old in sorted(original.items()):
        new = (root / relative).read_bytes()
        if old != new:
            changes.extend(
                difflib.unified_diff(
                    old.decode().splitlines(keepends=True),
                    new.decode().splitlines(keepends=True),
                    fromfile="a/" + relative, tofile="b/" + relative,
                )
            )
    destination.parent.mkdir(exist_ok=True)
    destination.write_text("".join(changes))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    args = parser.parse_args()
    locked = fixture.pins()
    locked["patched_dependencies"] = {
        "a2-0.10.0": {
            "url": "https://static.crates.io/crates/a2/a2-0.10.0.crate",
            "sha256": "f279fc8b1f1a64138f0f4b9cda9be488ae35bc2f8556c7ffe60730f1c07d005a",
            "bytes": 36011, "license": "MIT",
        },
        "again-0.1.2": {
            "url": "https://static.crates.io/crates/again/again-0.1.2.crate",
            "sha256": "05802a5ad4d172eaf796f7047b42d0af9db513585d16d4169660a21613d34b93",
            "bytes": 17395, "license": "MIT",
        },
    }
    locked["build_tools"] = {
        "cmake": {
            "version": "4.5.0", "license": "BSD-3-Clause",
            "url": "https://github.com/Kitware/CMake/releases/download/v4.5.0/cmake-4.5.0-linux-aarch64.tar.gz",
            "sha256": "0c8447e7bba9701467b5ac6155d157aa9e309e71af4981f78bd4b9176107f553",
            "bytes": 53719606,
        },
    }
    patches = []
    archive = args.cache / "downloads" / "autopush-60841760.tar.gz"
    fixture.fetch(archive, locked["source"])
    destination = fixture.HERE / "patches" / "autopush-1.86.0.patch"
    export(original_files(archive, locked["source"]["root"]), args.source, destination)
    patches.append(destination)
    for name, item in locked["patched_dependencies"].items():
        archive = args.cache / "downloads" / f"push-fixture-{name}.crate"
        fixture.fetch(archive, item)
        destination = fixture.HERE / "patches" / (name + ".patch")
        export(original_files(archive, name), args.source / "vendor" / name, destination)
        patches.append(destination)
    locked["patches"] = [
        {"path": str(path.relative_to(fixture.HERE)), "sha256": fixture.digest(path),
         "root": "." if index == 0 else "vendor/" + path.stem}
        for index, path in enumerate(patches)
    ]
    for name, item in locked["build_tools"].items():
        archive = args.cache / "downloads" / f"push-fixture-{name}.tar.gz"
        fixture.fetch(archive, item)
        target = args.source / ".fixture-tools"
        target.mkdir(exist_ok=True)
        import shutil
        shutil.copyfile(archive, target / (name + ".tar.gz"))
    locked["patched_tree_sha256"] = fixture.tree_digest(args.source)
    locked["fresh_schema_sha256"] = fixture.digest(fixture.HERE / "fresh-init.sql")
    fixture.write_json(fixture.HERE / "inputs.json", locked)
    print(json.dumps({"patched_tree_sha256": locked["patched_tree_sha256"],
                      "patches": locked["patches"]}, indent=2))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Prepare pinned build-only local-service inputs outside the checkout."""

import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "chromium/services-inputs.json"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_file(path, entry):
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"Missing regular input: {path}")
    if "bytes" in entry and path.stat().st_size != entry["bytes"]:
        raise ValueError(f"Input size mismatch: {path}")
    if digest(path) != entry["sha256"]:
        raise ValueError(f"Input SHA-256 mismatch: {path}")


def fetch(path, entry):
    if path.is_symlink() or path.parent.is_symlink():
        raise ValueError(f"Refusing a linked input path: {path}")
    if path.exists():
        verify_file(path, entry)
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as stream:
        staged = Path(stream.name)
    try:
        subprocess.run(
            [
                "/usr/bin/curl", "--fail", "--location", "--show-error", "--silent",
                "--connect-timeout", "10", "--max-time", "300", "--proto", "=https",
                "--proto-redir", "=https", "--max-filesize", str(entry["bytes"]),
                "--output", str(staged), entry["url"],
            ],
            check=True,
        )
        verify_file(staged, entry)
        staged.rename(path)
    finally:
        staged.unlink(missing_ok=True)


def verify_tree(archive, destination, entry):
    """Reject changed/extra source files instead of replacing existing edits."""
    expected = set()
    with tarfile.open(archive, "r:gz") as source:
        for member in source:
            relative = Path(member.name)
            if relative.parts[0] != entry["root"]:
                raise ValueError(f"Unexpected archive root: {member.name}")
            if len(relative.parts) == 1:
                continue
            relative = Path(*relative.parts[1:])
            expected.add(relative)
            target = destination / relative
            if member.isdir():
                if not target.is_dir() or target.is_symlink():
                    raise ValueError(f"Changed input directory: {target}")
            elif member.issym():
                if not target.is_symlink() or os.readlink(target) != member.linkname:
                    raise ValueError(f"Changed input link: {target}")
            elif member.isfile():
                if not target.is_file() or target.is_symlink():
                    raise ValueError(f"Missing input file: {target}")
                with source.extractfile(member) as original:
                    original_hash = hashlib.file_digest(original, "sha256").hexdigest()
                if target.stat().st_size != member.size or digest(target) != original_hash:
                    raise ValueError(f"Changed source input; preserve and inspect: {target}")
            else:
                raise ValueError(f"Unsupported archive member: {member.name}")
    for path in destination.rglob("*"):
        if path.relative_to(destination) not in expected:
            raise ValueError(f"Unrecognized source input; preserve and inspect: {path}")


def extract(archive, destination, entry):
    if destination.is_symlink() or destination.parent.is_symlink():
        raise ValueError(f"Refusing a linked source directory: {destination}")
    if destination.exists():
        verify_tree(archive, destination, entry)
        return
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent) as directory:
        staged = Path(directory)
        with tarfile.open(archive, "r:gz") as source:
            for member in source.getmembers():
                parts = Path(member.name).parts
                if not parts or parts[0] != entry["root"] or ".." in parts:
                    raise ValueError(f"Unsafe input archive path: {member.name}")
            source.extractall(staged, filter="data")
        extracted = staged / entry["root"]
        verify_tree(archive, extracted, entry)
        extracted.rename(destination)


def toolchain(manifest, chromium_cache):
    source = chromium_cache / "chromium" / manifest["chromium_revision"] / "src"
    depot = chromium_cache / "depot_tools" / manifest["depot_tools_revision"]
    clang = source / manifest["tools"]["clang"]["path"]
    ninja = source / manifest["tools"]["ninja"]["path"]
    python = depot / manifest["tools"]["python"]["path"]
    # These pinned tool entries can be symlinks within their verified bundles.
    for name, path in (("clang", clang), ("ninja", ninja), ("python", python)):
        if digest(path) != manifest["tools"][name]["sha256"]:
            raise ValueError(f"Pinned {name} differs: {path}")
    if Path(sys.executable).resolve() != python.resolve():
        raise ValueError(f"Run this script with the pinned depot_tools Python: {python}")
    environment = os.environ.copy()
    environment["DEVELOPER_DIR"] = "/Applications/Xcode.app/Contents/Developer"
    xcode = subprocess.check_output(
        ["/usr/bin/xcodebuild", "-version"], env=environment, text=True
    ).strip()
    expected = manifest["tools"]["xcode"]
    if xcode != f"Xcode {expected['version']}\nBuild version {expected['build']}":
        raise ValueError(f"Xcode input differs: {xcode}")
    sdk = subprocess.check_output(
        ["/usr/bin/xcrun", "--show-sdk-version"], env=environment, text=True
    ).strip()
    if sdk != expected["sdk"]:
        raise ValueError(f"SDK input differs: {sdk}")
    environment.update({
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "CC": str(clang),
        "CXX": str(clang.parent / "clang++"),
        "MACOSX_DEPLOYMENT_TARGET": expected["deployment_target"],
        "OMP_NUM_THREADS": "1",
        "VECLIB_MAXIMUM_THREADS": "1",
    })
    return environment, clang, ninja


def check_source_advisories(manifest, cache):
    evidence = {"checked_at_unix": int(time.time()), "source": "https://api.osv.dev/v1/query", "inputs": {}}
    for name, entry in manifest["sources"].items():
        response = subprocess.check_output([
            "/usr/bin/curl", "--fail", "--show-error", "--silent",
            "--connect-timeout", "10", "--max-time", "60", "--proto", "=https",
            "--max-filesize", "16777216", "--header", "Content-Type: application/json",
            "--data", json.dumps({"commit": entry["revision"]}), evidence["source"],
        ], text=True)
        data = json.loads(response)
        if not isinstance(data, dict) or not isinstance(data.get("vulns", []), list):
            raise ValueError(f"Invalid source advisory response for {name}")
        evidence["inputs"][name] = {"revision": entry["revision"], "response": data}
    path = cache / "source-advisories.json"
    path.write_text(json.dumps(evidence, indent=2) + "\n")
    affected = {
        name: [vulnerability["id"] for vulnerability in result["response"].get("vulns", [])]
        for name, result in evidence["inputs"].items()
        if result["response"].get("vulns")
    }
    if affected:
        raise ValueError(f"Source advisory matches block the build; inspect {path}: {affected}")


def prepare(args):
    manifest = json.loads(MANIFEST.read_text())
    cache = args.cache
    if not cache.is_absolute() or ".." in cache.parts:
        raise ValueError("Choose an absolute external build cache without '..'")
    if cache.resolve().is_relative_to(ROOT) or cache.is_symlink():
        raise ValueError("Keep service dependencies outside the checkout; no cache symlink")
    environment, clang, ninja = toolchain(manifest, args.chromium_cache)
    result = {"manifest_sha256": digest(MANIFEST), "sources": {}, "models": {}}
    for name, entry in manifest["sources"].items():
        archive = cache / "downloads" / f"{name}-{entry['revision'][:8]}.tar.gz"
        fetch(archive, entry)
        destination = cache / "sources" / f"{name}-{entry['revision'][:8]}"
        extract(archive, destination, entry)
        result["sources"][name] = str(destination)
    entry = manifest["tools"]["cmake"]
    archive = cache / "downloads" / f"cmake-{entry['version']}-macos-universal.tar.gz"
    fetch(archive, entry)
    destination = cache / "tools" / f"cmake-{entry['version']}"
    extract(archive, destination, entry)
    result["cmake"] = str(destination / "CMake.app/Contents/bin/cmake")
    if digest(Path(result["cmake"])) != entry["binary_sha256"]:
        raise ValueError("Pinned CMake binary differs after extraction")
    result["clang"] = str(clang)
    result["ninja"] = str(ninja)
    if args.build_speech:
        cmake = result["cmake"]
        builds = cache / "build"
        common = [
            "-G", "Ninja", f"-DCMAKE_MAKE_PROGRAM={ninja}",
            f"-DCMAKE_C_COMPILER={clang}", f"-DCMAKE_CXX_COMPILER={clang.parent / 'clang++'}",
            "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_OSX_DEPLOYMENT_TARGET=13.0",
            "-DCMAKE_IGNORE_PREFIX_PATH=/opt/homebrew;/opt/local",
            "-DBUILD_SHARED_LIBS=OFF",
        ]
        check_source_advisories(manifest, cache)
        whisper_build = builds / "speech"
        subprocess.run([
            cmake, "-S", str(ROOT / "tools/local_services"), "-B", str(whisper_build),
            *common, f"-DSTAINLESS_WHISPER_SOURCE={result['sources']['whisper']}",
        ], check=True, env=environment)
        subprocess.run(
            [cmake, "--build", str(whisper_build), "--target", "whisper-cli", "--parallel", "2"],
            check=True, env=environment,
        )
        espeak_build = builds / f"espeak-{manifest['sources']['espeak']['revision'][:8]}"
        subprocess.run([
            cmake, "-S", result["sources"]["espeak"], "-B", str(espeak_build), *common,
            "-DBUILD_TESTING=OFF", "-DUSE_LIBSONIC=OFF", "-DUSE_LIBPCAUDIO=OFF",
            "-DUSE_MBROLA=OFF", "-DUSE_ASYNC=OFF", "-DUSE_SPEECHPLAYER=OFF",
            "-DENABLE_TESTS=OFF",
            f"-DCMAKE_INSTALL_PREFIX={cache / 'tools/espeak'}",
        ], check=True, env=environment)
        subprocess.run(
            [cmake, "--build", str(espeak_build), "--target", "data", "--parallel", "2"],
            check=True, env=environment,
        )
        result["whisper_cli"] = str(whisper_build / "bin/whisper-cli")
        result["espeak"] = str(espeak_build / "src/espeak-ng")
        result["espeak_data"] = str(espeak_build)
    if args.models:
        for name, entry in manifest["models"].items():
            path = cache / "models" / f"{name}.bin"
            fetch(path, entry)
            result["models"][name] = str(path)
    print(json.dumps(result, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--chromium-cache", type=Path, required=True)
    parser.add_argument("--models", action="store_true", help="Fetch approved build-test models")
    parser.add_argument("--build-speech", action="store_true", help="Build CPU-only speech feasibility tools")
    args = parser.parse_args()
    try:
        if not args.cache.is_absolute() or args.cache.resolve().is_relative_to(ROOT):
            raise ValueError("Choose an absolute cache outside the checkout")
        if args.cache.is_symlink() or ".." in args.cache.parts:
            raise ValueError("The build cache cannot be a symlink or contain '..'")
        args.cache.mkdir(parents=True, exist_ok=True)
        descriptor = os.open(args.cache / ".inputs.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        with os.fdopen(descriptor, "r+") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            prepare(args)
    except (OSError, ValueError, tarfile.TarError, subprocess.SubprocessError) as error:
        print(f"Local-service input preparation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

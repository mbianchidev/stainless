#!/usr/bin/env python3
"""Prepare pinned build-only local-service inputs outside the checkout."""

import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import zipfile

from local_services_advisories import (
    check_source_advisories,
    ensure_configurations_verified,
    verify_disabled_advisories,
    version_in_range,
)


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


def verify_model_references(manifest, cache, model_ids):
    result = {}
    for model_id in model_ids:
        for name in manifest["models"][model_id].get("references", []):
            entry = manifest["model_reference_inputs"][name]
            verify_file(cache / "model-notices" / name, entry)
            result[name] = entry
    return result


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
        # A tiny final notice can have a larger HTTP redirect body. The final
        # file must still match its exact reviewed byte count and digest.
        subprocess.run(
            [
                "/usr/bin/curl", "--fail", "--location", "--show-error", "--silent",
                "--connect-timeout", "10", "--max-time", "300", "--proto", "=https",
                "--proto-redir", "=https", "--max-filesize", str(max(entry["bytes"], 4096)),
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


def extract_model_zip(archive, destination, entry):
    if destination.is_symlink() or destination.parent.is_symlink():
        raise ValueError(f"Refusing a linked model source: {destination}")
    expected = entry["members"]
    if any(len(Path(name).parts) != 1 or name in (".", "..") for name in expected):
        raise ValueError("Original model members must be top-level data files")
    if destination.exists():
        actual = {p.relative_to(destination).as_posix() for p in destination.rglob("*")}
        if actual != set(expected):
            raise ValueError(f"Unknown existing model source files: {destination}")
        for name, metadata in expected.items():
            verify_file(destination / name, metadata)
        return
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
        staged = Path(temporary) / "model"
        staged.mkdir()
        with zipfile.ZipFile(archive) as source:
            names = [item.filename for item in source.infolist()]
            if len(names) != len(set(names)) or set(names) != (
                set(expected) | set(entry.get("ignored_source_members", []))
            ):
                raise ValueError("Unexpected or duplicate model archive members")
            for item in source.infolist():
                relative = Path(item.filename)
                if relative.is_absolute() or ".." in relative.parts:
                    raise ValueError(f"Unsafe model archive path: {item.filename}")
                if item.filename not in expected:
                    continue
                mode = item.external_attr >> 16
                if item.is_dir() or stat.S_IFMT(mode) not in (0, stat.S_IFREG) or mode & 0o111:
                    raise ValueError(f"Model member is not regular data: {item.filename}")
                metadata = expected[item.filename]
                if item.file_size != metadata["bytes"] or item.flag_bits & 1:
                    raise ValueError(f"Model archive size/encryption mismatch: {item.filename}")
                target = staged / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                with source.open(item) as original, target.open("xb") as output:
                    shutil.copyfileobj(original, output, length=1024 * 1024)
                verify_file(target, metadata)
        staged.rename(destination)


def tree_digest(directory):
    result = hashlib.sha256()
    for path in sorted(directory.rglob("*")):
        relative = path.relative_to(directory).as_posix()
        if path.is_symlink():
            value = "link:" + os.readlink(path)
        elif path.is_file():
            value = "file:" + digest(path)
        elif path.is_dir():
            value = "directory"
        else:
            raise ValueError(f"Unsupported source node: {path}")
        result.update((relative + "\0" + value + "\n").encode())
    return result.hexdigest()


def cmake_values(path):
    values = {}
    for line in path.read_text().splitlines():
        if line and not line.startswith(("#", "//")) and "=" in line:
            key, value = line.split("=", 1)
            values[key.split(":", 1)[0]] = value
    return values


def verify_runtime_configuration(build, engine, compiler):
    expected = {
        "BUILD_SHARED_LIBS": "OFF",
        "CMAKE_OSX_ARCHITECTURES": "arm64",
        "CMAKE_OSX_DEPLOYMENT_TARGET": "13.0",
        "CMAKE_C_COMPILER": str(compiler),
        "CMAKE_CXX_COMPILER": str(compiler.parent / "clang++"),
    }
    if engine == "translation":
        expected.update({
            "WITH_RUY": "ON", "OPENMP_RUNTIME": "NONE",
            "BUILD_CLI": "OFF", "BUILD_TESTS": "OFF",
        })
        expected.update({
            name: "OFF" for name in (
                "WITH_MKL", "WITH_DNNL", "WITH_ACCELERATE", "WITH_OPENBLAS",
                "WITH_CUDA", "WITH_CUDNN", "WITH_HIP", "WITH_MPS",
                "WITH_TENSOR_PARALLEL", "WITH_FLASH_ATTN", "CUDA_DYNAMIC_LOADING",
            )
        })
    elif engine == "speech":
        expected.update({
            "WHISPER_CURL": "OFF", "WHISPER_COREML": "OFF",
            "WHISPER_BUILD_SERVER": "OFF", "WHISPER_BUILD_EXAMPLES": "OFF",
            "WHISPER_BUILD_TESTS": "OFF", "WHISPER_SDL2": "OFF",
            "GGML_CPU": "ON",
            "GGML_METAL": "OFF", "GGML_ACCELERATE": "OFF",
            "GGML_BLAS": "OFF", "GGML_OPENMP": "OFF", "GGML_RPC": "OFF",
            "GGML_NATIVE": "OFF", "GGML_BACKEND_DL": "OFF",
        })
        expected.update({
            "GGML_" + name: "OFF" for name in (
                "CPU_HBM", "CPU_REPACK", "CPU_KLEIDIAI", "CPU_ALL_VARIANTS",
                "CUDA", "HIP", "MUSA", "VULKAN", "WEBGPU", "ZDNN",
                "VIRTGPU", "VIRTGPU_BACKEND", "SYCL", "OPENVINO", "ET",
                "ET_SYSEMU", "OPENCL", "HEXAGON", "ZENDNN", "LLAMAFILE",
                "OPENMP_FETCH",
            )
        })
    else:
        expected.update({
            "OPENMP_BUILD": "OFF", "GRAPHICS_DISABLED": "ON",
            "DISABLED_LEGACY_ENGINE": "ON", "BUILD_TRAINING_TOOLS": "OFF",
            "BUILD_TESTS": "OFF", "USE_SYSTEM_ICU": "OFF",
            "DISABLE_TIFF": "ON", "DISABLE_ARCHIVE": "ON", "DISABLE_CURL": "ON",
            "ENABLE_NATIVE": "OFF",
        })
    actual = cmake_values(build / "CMakeCache.txt")
    for key, value in expected.items():
        if actual.get(key) != value:
            raise ValueError(f"Native {engine} configuration differs: {key}")
    return expected


def runtime_notices(manifest, sources, cache):
    sections = []
    inventory = []
    for name in ("ctranslate2", "spdlog", "ruy", "cpuinfo", "tesseract", "leptonica", "whisper"):
        root = Path(sources[name])
        paths = [
            path for path in sorted(root.rglob("*"))
            if path.is_file() and not path.is_symlink() and (
                path.name.upper() in (
                    "LICENSE", "LICENSE.TXT", "COPYING", "COPYING.LESSER",
                    "NOTICE", "NOTICE.TXT", "COPYRIGHT",
                ) or path.name.casefold().endswith("-license.txt")
            )
        ]
        if not paths:
            raise ValueError(f"Missing native source license inventory: {name}")
        for path in paths:
            relative = path.relative_to(root).as_posix()
            sections.append(
                f"{name} @ {manifest['sources'][name]['revision']} / {relative}\n"
                + "=" * 72 + "\n" + path.read_text() + "\n"
            )
            inventory.append({
                "component": name, "path": relative, "sha256": digest(path),
            })
    for name, entry in manifest["embedded_sources"].items():
        path = Path(sources[entry["parent"]]) / entry["path"]
        verify_file(path, entry)
        if "notice" in entry:
            notice = cache / "notices" / (name + "-LICENSE")
            fetch(notice, entry["notice"])
            text = notice.read_text()
            notice_digest = digest(notice)
        else:
            text = "".join(path.read_text().splitlines(keepends=True)[
                :entry["notice_prefix_lines"]
            ])
            notice_digest = hashlib.sha256(text.encode()).hexdigest()
        sections.append(
            f"{name} {entry['version']} (vendored by {entry['parent']})\n"
            + "=" * 72 + "\n" + text + "\n"
        )
        inventory.append({
            "component": name, "path": entry["path"],
            "source_sha256": digest(path), "notice_sha256": notice_digest,
        })
    return "\n".join(sections), inventory


def translation_sources(manifest, sources, cache):
    names = ("ctranslate2", "spdlog", "cxxopts", "ruy", "cpuinfo")
    key = hashlib.sha256(json.dumps(
        {name: manifest["sources"][name] for name in names}, sort_keys=True
    ).encode()).hexdigest()[:16]
    destination = cache / "sources" / ("ctranslate2-stainless-" + key)
    if destination.is_symlink():
        raise ValueError(f"Refusing a linked composed source: {destination}")
    with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
        staged = Path(temporary) / "source"
        shutil.copytree(sources["ctranslate2"], staged, symlinks=True)
        for name, relative in (
            ("spdlog", "third_party/spdlog"),
            ("cxxopts", "third_party/cxxopts"),
            ("ruy", "third_party/ruy"),
            ("cpuinfo", "third_party/ruy/third_party/cpuinfo"),
        ):
            target = staged / relative
            if target.is_symlink() or (target.exists() and any(target.iterdir())):
                raise ValueError(f"Unexpected nonempty submodule placeholder: {relative}")
            target.mkdir(parents=True, exist_ok=True)
            shutil.copytree(sources[name], target, symlinks=True, dirs_exist_ok=True)
        for entry in manifest["sources"]["ctranslate2"].get("patches", []):
            patch = ROOT / entry["path"]
            verify_file(patch, entry)
            subprocess.run(
                ["git", "-C", str(staged), "apply", "--check", str(patch)], check=True
            )
            subprocess.run(["git", "-C", str(staged), "apply", str(patch)], check=True)
        expected = tree_digest(staged)
        if destination.exists():
            if tree_digest(destination) != expected:
                raise ValueError(f"Changed composed source; preserve and inspect: {destination}")
        else:
            staged.rename(destination)
    return destination, expected


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


def run_bounded(command, environment, limits):
    started = time.monotonic()
    peak_rss = 0
    maximum_output_bytes = 16 * 1024 * 1024
    with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors, subprocess.Popen(
        command, env=environment, stdout=output, stderr=errors
    ) as process:
        def collect_exit():
            pid, status, usage = os.wait4(process.pid, os.WNOHANG)
            if not pid:
                return None
            process.returncode = os.waitstatus_to_exitcode(status)
            return usage

        try:
            usage = None
            while True:
                usage = collect_exit()
                if usage is not None:
                    break
                if time.monotonic() - started > limits["maximum_seconds_per_case"]:
                    raise TimeoutError("Inference exceeded the approved case deadline")
                if any(os.fstat(stream.fileno()).st_size > maximum_output_bytes
                       for stream in (output, errors)):
                    raise ValueError("Inference exceeded its diagnostic-output limit")
                memory = subprocess.run(
                    ["/bin/ps", "-o", "rss=", "-p", str(process.pid)],
                    capture_output=True, text=True, check=False,
                )
                if memory.returncode == 0 and memory.stdout.strip():
                    peak_rss = max(peak_rss, int(memory.stdout.strip()) * 1024)
                else:
                    usage = collect_exit()
                    if usage is not None:
                        break
                    raise RuntimeError(f"Cannot observe owned inference memory: {memory.stderr}")
                if peak_rss > limits["maximum_rss_bytes"]:
                    raise MemoryError("Inference exceeded the approved RSS limit")
                time.sleep(0.02)
            peak_rss = max(peak_rss, int(usage.ru_maxrss) * (1 if sys.platform == "darwin" else 1024))
            if time.monotonic() - started > limits["maximum_seconds_per_case"]:
                raise TimeoutError("Inference exceeded the approved case deadline")
            if peak_rss > limits["maximum_rss_bytes"]:
                raise MemoryError("Inference exceeded the approved RSS limit")
            if any(os.fstat(stream.fileno()).st_size > maximum_output_bytes
                   for stream in (output, errors)):
                raise ValueError("Inference exceeded its diagnostic-output limit")
        except (OSError, ValueError, TimeoutError, MemoryError, RuntimeError):
            if process.returncode is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            raise
        output.seek(0)
        errors.seek(0)
        stdout = output.read().decode("utf-8")
        stderr = errors.read().decode("utf-8")
        if process.returncode:
            raise RuntimeError(f"Inference exited {process.returncode}: {stderr[-4000:]}")
    return time.monotonic() - started, peak_rss, stdout, stderr


def prepare(args):
    manifest = json.loads(MANIFEST.read_text())
    cache = args.cache
    if not cache.is_absolute() or ".." in cache.parts:
        raise ValueError("Choose an absolute external build cache without '..'")
    if cache.resolve().is_relative_to(ROOT) or cache.is_symlink():
        raise ValueError("Keep service dependencies outside the checkout; no cache symlink")
    environment, clang, ninja = toolchain(manifest, args.chromium_cache)
    result = {
        "manifest_sha256": digest(MANIFEST), "sources": {}, "models": {},
        "advisory_checker_sha256": digest(ROOT / "tools/local_services_advisories.py"),
    }
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
    if (
        args.build_speech or args.build_ocr or args.build_translation or
        args.build_vision or args.build_runtimes
    ):
        cmake = result["cmake"]
        builds = cache / "build"
        common = [
            "-G", "Ninja", f"-DCMAKE_MAKE_PROGRAM={ninja}",
            f"-DCMAKE_C_COMPILER={clang}", f"-DCMAKE_CXX_COMPILER={clang.parent / 'clang++'}",
            "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_OSX_DEPLOYMENT_TARGET=13.0",
            "-DCMAKE_IGNORE_PREFIX_PATH=/opt/homebrew;/opt/local",
            "-DBUILD_SHARED_LIBS=OFF",
        ]
        upstream = set()
        if args.build_ocr:
            upstream.update(("tesseract", "leptonica"))
        if args.build_translation or args.build_runtimes:
            upstream.update(("ctranslate2", "spdlog", "ruy", "cpuinfo", "nlohmann-json"))
        if args.build_runtimes:
            upstream.add("whisper")
        if args.build_speech:
            upstream.update(("whisper", "espeak"))
        if args.build_vision:
            upstream.add("llama")
        result["source_advisories"] = check_source_advisories(manifest, cache, upstream)
    if args.build_speech:
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
    if args.build_ocr:
        fingerprint = hashlib.sha256(
            (digest(MANIFEST) + digest(Path(__file__))).encode()
        ).hexdigest()[:16]
        ocr_build = builds / f"ocr-{fingerprint}"
        prefix = ocr_build / "sysroot"
        leptonica = ocr_build / "leptonica"
        isolated = [
            "-DCMAKE_INSTALL_LIBDIR=lib",
            "-DCMAKE_DISABLE_FIND_PACKAGE_PkgConfig=ON",
            "-DCMAKE_FIND_USE_PACKAGE_REGISTRY=OFF",
            "-DCMAKE_FIND_USE_SYSTEM_PACKAGE_REGISTRY=OFF",
        ]
        ocr_environment = environment.copy()
        for name in (
            "HOMEBREW_PREFIX", "PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR",
            "CMAKE_PREFIX_PATH", "CPATH", "C_INCLUDE_PATH",
            "CPLUS_INCLUDE_PATH", "LIBRARY_PATH",
        ):
            ocr_environment.pop(name, None)
        subprocess.run([
            cmake, "-S", result["sources"]["leptonica"], "-B", str(leptonica),
            *common, *isolated, f"-DCMAKE_INSTALL_PREFIX={prefix}",
            "-DBUILD_PROG=OFF", "-DSW_BUILD=OFF", "-DSYM_LINK=OFF",
            "-DENABLE_ZLIB=OFF", "-DENABLE_PNG=OFF", "-DENABLE_GIF=OFF",
            "-DENABLE_JPEG=OFF", "-DENABLE_TIFF=OFF", "-DENABLE_WEBP=OFF",
            "-DENABLE_OPENJPEG=OFF",
        ], check=True, env=ocr_environment)
        subprocess.run([
            cmake, "--build", str(leptonica), "--parallel", "2",
        ], check=True, env=ocr_environment)
        subprocess.run([
            cmake, "--install", str(leptonica),
        ], check=True, env=ocr_environment)
        tesseract = ocr_build / "tesseract"
        subprocess.run([
            cmake, "-S", result["sources"]["tesseract"], "-B", str(tesseract),
            *common, *isolated,
            f"-DLeptonica_DIR={prefix / 'lib/cmake/leptonica'}",
            "-DOPENMP_BUILD=OFF", "-DGRAPHICS_DISABLED=ON",
            "-DDISABLED_LEGACY_ENGINE=ON", "-DBUILD_TRAINING_TOOLS=OFF",
            "-DBUILD_TESTS=OFF", "-DUSE_SYSTEM_ICU=OFF", "-DDISABLE_TIFF=ON",
            "-DDISABLE_ARCHIVE=ON", "-DDISABLE_CURL=ON", "-DENABLE_NATIVE=OFF",
            "-DENABLE_LTO=OFF", "-DENABLE_CCACHE=OFF",
            "-DENABLE_PRECOMPILED_HEADERS=OFF", "-DINSTALL_CONFIGS=OFF",
        ], check=True, env=ocr_environment)
        subprocess.run([
            cmake, "--build", str(tesseract), "--target", "tesseract",
            "--parallel", "2",
        ], check=True, env=ocr_environment)
        executable = tesseract / "bin/tesseract"
        result["ocr_cli"] = str(executable)
        result["ocr_cli_sha256"] = digest(executable)
        result["ocr_sysroot"] = str(prefix)
        result["ocr_build"] = str(ocr_build)
        (ocr_build / "stainless-inputs.json").write_text(
            json.dumps(result, indent=2) + "\n"
        )
    if args.build_translation:
        composed, source_digest = translation_sources(
            manifest, result["sources"], cache
        )
        key = hashlib.sha256(
            (digest(MANIFEST) + digest(Path(__file__))).encode()
        ).hexdigest()[:16]
        build = builds / ("translation-" + key)
        subprocess.run([
            cmake, "-S", str(composed), "-B", str(build), *common,
            "-DCMAKE_POLICY_VERSION_MINIMUM=3.5",
            "-DCMAKE_OSX_ARCHITECTURES=arm64",
            "-DCMAKE_FIND_USE_PACKAGE_REGISTRY=OFF",
            "-DCMAKE_FIND_USE_SYSTEM_PACKAGE_REGISTRY=OFF",
            "-DWITH_MKL=OFF", "-DWITH_DNNL=OFF", "-DWITH_ACCELERATE=OFF",
            "-DWITH_OPENBLAS=OFF", "-DWITH_RUY=ON", "-DWITH_CUDA=OFF",
            "-DWITH_CUDNN=OFF", "-DWITH_HIP=OFF", "-DWITH_MPS=OFF",
            "-DWITH_TENSOR_PARALLEL=OFF", "-DWITH_FLASH_ATTN=OFF",
            "-DCUDA_DYNAMIC_LOADING=OFF", "-DOPENMP_RUNTIME=NONE",
            "-DENABLE_CPU_DISPATCH=OFF", "-DBUILD_CLI=ON", "-DBUILD_TESTS=OFF",
            "-DRUY_MINIMAL_BUILD=ON", "-DRUY_PROFILER=OFF",
            "-DRUY_ENABLE_INSTALL=OFF", "-DCPUINFO_BUILD_TOOLS=OFF",
            "-DCPUINFO_BUILD_UNIT_TESTS=OFF", "-DCPUINFO_BUILD_MOCK_TESTS=OFF",
            "-DCPUINFO_BUILD_BENCHMARKS=OFF",
        ], check=True, env=environment)
        subprocess.run([
            cmake, "--build", str(build), "--target", "translator", "--parallel", "2",
        ], check=True, env=environment)
        executable = build / "cli/ct2-translator"
        result["translation_cli"] = str(executable)
        result["translation_cli_sha256"] = digest(executable)
        result["translation_build"] = str(build)
        result["translation_sources"] = str(composed)
        result["translation_source_tree_sha256"] = source_digest
        (build / "stainless-inputs.json").write_text(
            json.dumps(result, indent=2) + "\n"
        )
    if args.build_vision:
        key = hashlib.sha256(
            (digest(MANIFEST) + digest(Path(__file__))).encode()
        ).hexdigest()[:16]
        build = builds / ("vision-" + key)
        vision_environment = environment.copy()
        for name in (
            "HOMEBREW_PREFIX", "PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR",
            "CMAKE_PREFIX_PATH", "CPATH", "C_INCLUDE_PATH",
            "CPLUS_INCLUDE_PATH", "LIBRARY_PATH",
        ):
            vision_environment.pop(name, None)
        subprocess.run([
            cmake, "-S", result["sources"]["llama"], "-B", str(build), *common,
            "-DCMAKE_OSX_ARCHITECTURES=arm64",
            "-DCMAKE_FIND_USE_PACKAGE_REGISTRY=OFF",
            "-DCMAKE_FIND_USE_SYSTEM_PACKAGE_REGISTRY=OFF",
            "-DFETCHCONTENT_FULLY_DISCONNECTED=ON",
            "-DLLAMA_BUILD_IS_DEV=OFF", "-DLLAMA_BUILD_COMMON=ON",
            "-DLLAMA_BUILD_TOOLS=ON", "-DLLAMA_BUILD_TESTS=OFF",
            "-DLLAMA_BUILD_EXAMPLES=OFF", "-DLLAMA_BUILD_SERVER=OFF",
            "-DLLAMA_BUILD_APP=OFF", "-DLLAMA_BUILD_UI=OFF",
            "-DLLAMA_USE_PREBUILT_UI=OFF", "-DLLAMA_CONNECT=OFF",
            "-DLLAMA_OPENSSL=OFF", "-DLLAMA_SUBPROCESS=OFF",
            "-DLLAMA_LLGUIDANCE=OFF", "-DLLAMA_USE_SYSTEM_GGML=OFF",
            "-DLLAMA_TOOLS_INSTALL=OFF", "-DLLAMA_TESTS_INSTALL=OFF",
            "-DMTMD_VIDEO=OFF", "-DGGML_NATIVE=OFF", "-DGGML_CPU=ON",
            "-DGGML_CPU_KLEIDIAI=OFF", "-DGGML_CPU_REPACK=OFF",
            "-DGGML_OPENMP=OFF", "-DGGML_ACCELERATE=OFF", "-DGGML_BLAS=OFF",
            "-DGGML_METAL=OFF", "-DGGML_CUDA=OFF", "-DGGML_VULKAN=OFF",
            "-DGGML_WEBGPU=OFF", "-DGGML_SYCL=OFF", "-DGGML_RPC=OFF",
            "-DGGML_LLAMAFILE=OFF", "-DGGML_BACKEND_DL=OFF",
            "-DGGML_CCACHE=OFF", "-DGGML_CPU_ALL_VARIANTS=OFF",
        ], check=True, env=vision_environment)
        verify_disabled_advisories(
            result["source_advisories"], cmake_values(build / "CMakeCache.txt")
        )
        subprocess.run([
            cmake, "--build", str(build), "--target", "llama-mtmd-cli",
            "--parallel", "2",
        ], check=True, env=vision_environment)
        executable = build / "bin/llama-mtmd-cli"
        result["vision_cli"] = str(executable)
        result["vision_cli_sha256"] = digest(executable)
        result["vision_build"] = str(build)
        (build / "stainless-inputs.json").write_text(
            json.dumps(result, indent=2) + "\n"
        )
    if args.build_runtimes:
        runtime_source = ROOT / "tools/local_services/runtime"
        key = hashlib.sha256(
            (digest(MANIFEST) + digest(Path(__file__)) + tree_digest(runtime_source)).encode()
        ).hexdigest()[:16]
        runtime_root = builds / ("runtime-" + key)
        composed, composed_digest = translation_sources(
            manifest, result["sources"], cache
        )
        runtime_environment = environment.copy()
        for name in (
            "HOMEBREW_PREFIX", "PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR",
            "CMAKE_PREFIX_PATH", "CPATH", "C_INCLUDE_PATH",
            "CPLUS_INCLUDE_PATH", "LIBRARY_PATH",
        ):
            runtime_environment.pop(name, None)
        result["runtimes"] = {}
        for engine, source in (
            ("translation", composed),
            ("ocr", Path(result["sources"]["tesseract"])),
            ("speech", Path(result["sources"]["whisper"])),
        ):
            build = runtime_root / engine
            command = [
                cmake, "-S", str(runtime_source), "-B", str(build), *common,
                f"-DSTAINLESS_ENGINE={engine}",
                f"-DSTAINLESS_ENGINE_SOURCE={source}",
            ]
            if engine == "ocr":
                command.append(
                    f"-DLeptonica_DIR={Path(result['ocr_sysroot']) / 'lib/cmake/leptonica'}"
                )
            subprocess.run(command, check=True, env=runtime_environment)
            configuration = verify_runtime_configuration(build, engine, clang)
            subprocess.run([
                cmake, "--build", str(build), "--target", "stainless_" + engine,
                "--parallel", "2",
            ], check=True, env=runtime_environment)
            library = build / ("libstainless_" + engine + ".dylib")
            symbols = subprocess.check_output(
                ["/usr/bin/nm", "-gjU", str(library)], text=True
            ).splitlines()
            expected_symbols = (runtime_source / (engine + ".exports")).read_text().splitlines()
            if set(symbols) != set(expected_symbols):
                raise ValueError(f"Native {engine} exports more than its fixed C ABI")
            dependencies = subprocess.check_output(
                ["/usr/bin/otool", "-L", str(library)], text=True
            )
            for line in dependencies.splitlines()[1:]:
                name = line.strip().split(" (", 1)[0]
                if name == "@rpath/" + library.name:
                    continue
                if not name.startswith(("/usr/lib/", "/System/Library/")):
                    raise ValueError(f"Unexpected native runtime dependency: {name}")
            subprocess.run([
                "/usr/bin/codesign", "--force", "--sign", "-", "--timestamp=none",
                str(library),
            ], check=True)
            subprocess.run([
                "/usr/bin/codesign", "--verify", "--strict", str(library),
            ], check=True)
            result["runtimes"][engine] = {
                "library": str(library), "sha256": digest(library),
                "configuration_sha256": digest(build / "CMakeCache.txt"),
                "verified_configuration": configuration,
                "exports": sorted(symbols), "dependencies": dependencies,
            }
        notices, inventory = runtime_notices(manifest, result["sources"], cache)
        notices_path = runtime_root / "StainlessLocalServicesNotices.txt"
        notices_path.write_text(notices)
        result["runtime_notices"] = {
            "path": str(notices_path), "sha256": digest(notices_path),
            "sources": inventory,
        }
        result["runtime_source_sha256"] = tree_digest(runtime_source)
        result["runtime_header_sha256"] = digest(
            runtime_source / "stainless_local_runtime.h"
        )
        result["runtime_translation_sources_sha256"] = composed_digest
        (runtime_root / "stainless-inputs.json").write_text(
            json.dumps(result, indent=2) + "\n"
        )
        if args.runtime_report:
            report = args.runtime_report
            if (
                not report.is_absolute() or ".." in report.parts or
                report.is_symlink() or report.parent.is_symlink() or
                report.resolve().is_relative_to(ROOT)
            ):
                raise ValueError("Choose an unlinked external runtime report path")
            report.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(
                mode="w", dir=report.parent, delete=False
            ) as output:
                staged = Path(output.name)
                json.dump(result, output, indent=2)
                output.write("\n")
            try:
                staged.replace(report)
            finally:
                staged.unlink(missing_ok=True)
    if "source_advisories" in result:
        ensure_configurations_verified(result["source_advisories"])
        (cache / "source-advisories.json").write_text(
            json.dumps(result["source_advisories"], indent=2) + "\n"
        )
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
    parser.add_argument("--build-ocr", action="store_true", help="Build CPU-only OCR with image codecs/network/training disabled")
    parser.add_argument("--build-translation", action="store_true", help="Build pinned CPU-only CTranslate2 and exact math/CLI submodules")
    parser.add_argument("--build-vision", action="store_true", help="Build pinned CPU-only llama.cpp multimodal feasibility CLI")
    parser.add_argument("--build-runtimes", action="store_true", help="Build hidden-symbol FD-only translation/OCR/speech C ABI libraries")
    parser.add_argument("--runtime-report", type=Path, help="Write native runtime provenance for the browser build")
    args = parser.parse_args()
    if args.runtime_report and not args.build_runtimes:
        parser.error("--runtime-report requires --build-runtimes")
    if args.build_runtimes:
        args.build_ocr = True
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

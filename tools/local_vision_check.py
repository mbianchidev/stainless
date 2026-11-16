#!/usr/bin/env python3
"""Evaluate original frozen raster scenes, not browser/AX/sandbox acceptance."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import unicodedata

from local_services import MANIFEST, ROOT, digest, run_bounded, toolchain, verify_file
from local_services import verify_model_references


CORPUS = ROOT / "tests/fixtures/local_vision.json"
CORPUS_SHA256 = "e8fcbb40b9bfe868abe9903b164b9c435f5d279c0127e23128b2345037505793"
MARKER = "STAINLESS_VISION_RESPONSE_BOUNDARY"


def raster(case, settings, path):
    width, height = settings["width"], settings["height"]
    if not 0 < width <= 4096 or not 0 < height <= 4096 or width * height > 16777216:
        raise ValueError("Synthetic image dimensions exceed the declared boundary")
    if case["shape"] not in ("circle", "square", "none"):
        raise ValueError("Unknown frozen scene shape")
    foreground = bytes(case["color"])
    background = bytes(settings["background"])
    if len(foreground) != 3 or len(background) != 3:
        raise ValueError("Expected three RGB components")
    center_x, center_y = settings["shape_center"]
    radius = settings["circle_radius"]
    half = settings["square_half_width"]
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            inside = (
                case["shape"] == "circle" and
                (x - center_x) ** 2 + (y - center_y) ** 2 <= radius ** 2
            ) or (
                case["shape"] == "square" and
                center_x - half <= x < center_x + half and
                center_y - half <= y < center_y + half
            )
            pixels.extend(foreground if inside else background)
    path.write_bytes(f"P6\n{width} {height}\n255\n".encode() + pixels)
    return digest(path)


def parse_response(stdout):
    if stdout.count(MARKER) != 1:
        raise ValueError("Vision CLI did not expose exactly one generated response")
    response = stdout.split(MARKER, 1)[1]
    response = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", response).strip()
    if not response or len(response.encode()) > 32768:
        raise ValueError("Vision CLI returned empty or oversized generated text")
    return response


def assess(text, case):
    words = set(re.findall(r"[^\W_]+", unicodedata.normalize("NFC", text).casefold()))
    required = [
        [word for word in alternatives if word in words]
        for alternatives in case["required_concepts"]
    ]
    forbidden = [word for word in case["forbidden_concepts"] if word in words]
    return {
        "matched_concepts": required, "contradictions": forbidden,
        "passed": all(required) and not forbidden,
    }


def configuration(build):
    path = build / "CMakeCache.txt"
    values = dict(re.findall(
        r"^([A-Z0-9_]+):[^=]+=(.*)$", path.read_text(), re.MULTILINE
    ))
    expected = {
        "LLAMA_BUILD_IS_DEV": "OFF", "LLAMA_OPENSSL": "OFF",
        "LLAMA_SUBPROCESS": "OFF", "LLAMA_CONNECT": "OFF",
        "LLAMA_BUILD_SERVER": "OFF", "LLAMA_BUILD_APP": "OFF",
        "LLAMA_BUILD_UI": "OFF", "LLAMA_USE_SYSTEM_GGML": "OFF",
        "LLAMA_LLGUIDANCE": "OFF", "MTMD_VIDEO": "OFF",
        "GGML_NATIVE": "OFF", "GGML_CPU_KLEIDIAI": "OFF",
        "GGML_OPENMP": "OFF", "GGML_ACCELERATE": "OFF", "GGML_BLAS": "OFF",
        "GGML_METAL": "OFF", "GGML_CUDA": "OFF", "GGML_VULKAN": "OFF",
        "GGML_WEBGPU": "OFF", "GGML_SYCL": "OFF", "GGML_RPC": "OFF",
        "GGML_LLAMAFILE": "OFF", "GGML_BACKEND_DL": "OFF",
        "GGML_CCACHE": "OFF", "BUILD_SHARED_LIBS": "OFF",
        "CMAKE_OSX_DEPLOYMENT_TARGET": "13.0",
        "CMAKE_OSX_ARCHITECTURES": "arm64",
    }
    for key, value in expected.items():
        if values.get(key) != value:
            raise ValueError(f"Native vision configuration differs: {key}")
    return {"cache_sha256": digest(path), "verified_options": expected}


def evaluate(args):
    if digest(CORPUS) != CORPUS_SHA256:
        raise ValueError("The frozen independent vision oracle changed")
    manifest = json.loads(MANIFEST.read_text())
    corpus = json.loads(CORPUS.read_text())
    environment, _, _ = toolchain(manifest, args.chromium_cache)
    if (
        not args.cache.is_absolute() or args.cache.is_symlink() or
        ".." in args.cache.parts or args.cache.resolve().is_relative_to(ROOT)
    ):
        raise ValueError("Use an absolute, unlinked external input cache")
    key = hashlib.sha256(
        (digest(MANIFEST) + digest(ROOT / "tools/local_services.py")).encode()
    ).hexdigest()[:16]
    build = args.cache / "build" / f"vision-{key}"
    metadata = json.loads((build / "stainless-inputs.json").read_text())
    executable = build / "bin/llama-mtmd-cli"
    if metadata["manifest_sha256"] != digest(MANIFEST) or metadata["vision_cli"] != str(executable):
        raise ValueError("Native vision build differs; rerun pinned preparation")
    verify_file(executable, {"sha256": metadata["vision_cli_sha256"]})
    native_configuration = configuration(build)
    composition = None
    if args.measured_color:
        composition = (
            args.chromium_cache / "chromium" / manifest["chromium_revision"] /
            "src/out/Stainless/stainless_image_description_probe"
        )
        if not composition.is_file() or composition.is_symlink():
            raise ValueError("Build the exact normal-GN native image-description probe first")
    dependencies = subprocess.check_output(
        ["/usr/bin/otool", "-L", str(executable)], env=environment, text=True
    )
    for line in dependencies.splitlines()[1:]:
        name = line.strip().split(" (", 1)[0]
        if not name.startswith(("/usr/lib/", "/System/Library/")):
            raise ValueError(f"Build-host native vision dependency: {name}")
    prefix = {
        "256m": "vlm",
        "500m": "vlm-500",
        "qwen35-08b": "vlm-qwen35",
    }[args.candidate]
    model_ids = (prefix + "-text", prefix + "-projector")
    model = args.cache / "models" / (model_ids[0] + ".bin")
    projector = args.cache / "models" / (model_ids[1] + ".bin")
    verify_file(model, manifest["models"][model_ids[0]])
    verify_file(projector, manifest["models"][model_ids[1]])
    for name in list(environment):
        if name.startswith(("LLAMA_ARG_", "MTMD_", "GGML_")):
            environment.pop(name)
    environment["MTMD_TEST_RESPONSE_MARKER"] = MARKER
    report = {
        "checked_at_unix": int(time.time()),
        "scope": corpus["scope"],
        "candidate": args.candidate,
        "manifest_sha256": digest(MANIFEST),
        "corpus_sha256": digest(CORPUS),
        "native_cli_sha256": digest(executable),
        "native_runtime_revision": manifest["sources"]["llama"]["revision"],
        "configuration": native_configuration,
        "macho_dependencies": dependencies,
        "models": {name: manifest["models"][name] for name in model_ids},
        "reviewed_model_references": verify_model_references(manifest, args.cache, model_ids),
        "generation": corpus["generation"],
        "description_pipeline": "native-measured-uniform-color" if composition else "raw-model",
        "native_composition_binary_sha256": digest(composition) if composition else None,
        "cases": [],
        "worker_handle_patch_cabi_browser_ax_installed_verified": False,
    }
    with tempfile.TemporaryDirectory(prefix="vision-probe-", dir=args.cache) as temporary:
        temporary = Path(temporary)
        for case in corpus["cases"]:
            image = temporary / (case["id"] + ".ppm")
            image_sha = raster(case, corpus["raster"], image)
            command = [
                str(executable), "-m", str(model), "--mmproj", str(projector),
                "--no-mmproj-auto", "--image", str(image), "-p", corpus["prompt"],
                "--threads", str(corpus["limits"]["compute_threads"]),
                "--threads-batch", str(corpus["limits"]["compute_threads"]),
                "--ctx-size", str(corpus["generation"]["context_tokens"]),
                "--n-predict", str(corpus["limits"]["maximum_generated_tokens"]),
                "--gpu-layers", "0", "--no-mmproj-offload",
                "--seed", str(corpus["generation"]["seed"]),
                "--temp", str(corpus["generation"]["temperature"]),
            ]
            started = time.monotonic()
            seconds, rss, stdout, stderr = run_bounded(command, environment, corpus["limits"])
            model_seconds = seconds
            raw = parse_response(stdout)
            actual = raw
            uniform = None
            if composition:
                description = temporary / (case["id"] + ".description")
                description.write_text(raw)
                remaining = corpus["limits"]["maximum_seconds_per_case"] - (
                    time.monotonic() - started
                )
                if remaining <= 0:
                    raise TimeoutError("Image description exceeded its complete-case deadline")
                _, measured_rss, composed, composition_errors = run_bounded(
                    [str(composition), str(image), str(description)], environment,
                    dict(corpus["limits"], maximum_seconds_per_case=remaining),
                )
                rss = max(rss, measured_rss)
                result = json.loads(composed)
                if (
                    not isinstance(result, dict) or set(result) != {"text", "uniform"} or
                    not isinstance(result["text"], str) or not result["text"] or
                    not isinstance(result["uniform"], bool)
                ):
                    raise ValueError("Native description composition returned an invalid result")
                actual = result["text"]
                uniform = result["uniform"]
                stderr += composition_errors
            seconds = time.monotonic() - started
            if seconds > corpus["limits"]["maximum_seconds_per_case"]:
                raise TimeoutError("Image description exceeded its complete-case deadline")
            report["cases"].append({
                "id": case["id"], "actual": actual, **assess(actual, case),
                "raw_actual": raw, "raw_passed": assess(raw, case)["passed"],
                "measured_uniform": uniform,
                "image_sha256": image_sha, "seconds": seconds,
                "model_seconds": model_seconds,
                "peak_rss_bytes": rss, "stderr": stderr,
            })
        missing = [
            str(executable), "-m", str(temporary / "absent-model.gguf"),
            "--mmproj", str(projector), "--no-mmproj-auto",
            "--image", str(image), "-p", corpus["prompt"],
            "--threads", "2", "--threads-batch", "2",
            "--ctx-size", str(corpus["generation"]["context_tokens"]),
            "--n-predict", str(corpus["limits"]["maximum_generated_tokens"]),
            "--gpu-layers", "0", "--no-mmproj-offload",
        ]
        try:
            run_bounded(missing, environment, corpus["limits"])
        except RuntimeError as error:
            if "absent-model.gguf" not in str(error):
                raise
            report["missing_model_rejected"] = True
        else:
            report["missing_model_rejected"] = False
    report["quality_passed"] = (
        len(report["cases"]) == len(corpus["cases"]) and
        all(case["passed"] for case in report["cases"]) and
        report["missing_model_rejected"]
    )
    report["all_rss_observed"] = all(case["peak_rss_bytes"] > 0 for case in report["cases"])
    report["raw_model_quality_passed"] = all(case["raw_passed"] for case in report["cases"])
    if args.output.exists() or args.output.is_symlink():
        raise ValueError("Preserve the existing vision report")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps({
        "quality_passed": report["quality_passed"],
        "all_rss_observed": report["all_rss_observed"],
        "cases": [{key: case[key] for key in ("id", "passed", "actual")}
                  for case in report["cases"]],
        "report": str(args.output),
    }, ensure_ascii=False))
    return 0 if report["quality_passed"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chromium-cache", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--candidate", choices=["256m", "500m", "qwen35-08b"], default="256m")
    parser.add_argument("--measured-color", action="store_true", help="Run the reviewed native uniform-color final-description composition, preserving raw evidence")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        return evaluate(args)
    except (OSError, ValueError, RuntimeError, TimeoutError, MemoryError,
            subprocess.SubprocessError) as error:
        print(f"Vision feasibility failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Evaluate frozen synthetic OCR pixels, not browser/PDF/accessibility acceptance."""

import argparse
import csv
import hashlib
import io
import json
import math
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import unicodedata

from local_services import MANIFEST, ROOT, digest, run_bounded, toolchain, verify_file


CORPUS = ROOT / "tests/fixtures/local_ocr.json"
GENERATOR = ROOT / "tools/local_services/ocr_fixture.swift"


def words(text):
    return re.findall(r"[^\W_]+", unicodedata.normalize("NFC", text).casefold())


def cache_values(path):
    values = {}
    for line in path.read_text().splitlines():
        if not line or line.startswith(("#", "//")) or "=" not in line:
            continue
        key, value = line.split("=", 1)
        values[key.split(":", 1)[0]] = value
    return values


def verify_configuration(build):
    tesseract = cache_values(build / "tesseract/CMakeCache.txt")
    leptonica = cache_values(build / "leptonica/CMakeCache.txt")
    expected = {
        "OPENMP_BUILD": "OFF", "BUILD_TRAINING_TOOLS": "OFF",
        "BUILD_TESTS": "OFF", "GRAPHICS_DISABLED": "ON",
        "DISABLED_LEGACY_ENGINE": "ON", "DISABLE_CURL": "ON",
        "DISABLE_ARCHIVE": "ON", "DISABLE_TIFF": "ON",
        "ENABLE_NATIVE": "OFF", "BUILD_SHARED_LIBS": "OFF",
        "CMAKE_OSX_DEPLOYMENT_TARGET": "13.0",
    }
    for key, value in expected.items():
        if tesseract.get(key) != value:
            raise ValueError(f"OCR runtime configuration differs: {key}")
    for codec in ("ZLIB", "PNG", "GIF", "JPEG", "TIFF", "WEBP", "OPENJPEG"):
        if leptonica.get("ENABLE_" + codec) != "OFF":
            raise ValueError(f"Unexpected host codec enabled: {codec}")
    return {
        "tesseract_cache_sha256": digest(build / "tesseract/CMakeCache.txt"),
        "leptonica_cache_sha256": digest(build / "leptonica/CMakeCache.txt"),
        "verified_options": expected,
        "native_compute": "OpenMP disabled; single-threaded engine within the two-thread limit",
    }


def parse_tsv(text, width, height):
    rows = []
    reader = csv.DictReader(io.StringIO(text), delimiter="\t")
    expected = [
        "level", "page_num", "block_num", "par_num", "line_num", "word_num",
        "left", "top", "width", "height", "conf", "text",
    ]
    if reader.fieldnames != expected:
        raise ValueError("OCR did not return the expected TSV header")
    for row in reader:
        if None in row or any(row[name] is None for name in expected):
            raise ValueError("OCR returned a malformed TSV row")
        token = row["text"].strip()
        if not token:
            continue
        if row["level"] != "5":
            raise ValueError("OCR returned text without a word-level box")
        confidence = float(row["conf"])
        box = {name: int(row[name]) for name in ("left", "top", "width", "height")}
        if not math.isfinite(confidence) or not 0 <= confidence <= 100:
            raise ValueError("Invalid OCR word confidence")
        if (
            box["left"] < 0 or box["top"] < 0 or
            box["width"] <= 0 or box["height"] <= 0 or
            box["left"] + box["width"] > width or
            box["top"] + box["height"] > height
        ):
            raise ValueError("OCR returned a word box outside the input pixels")
        rows.append({"text": token, "confidence": confidence, "box": box})
    return rows


def evaluate(args):
    manifest = json.loads(MANIFEST.read_text())
    corpus = json.loads(CORPUS.read_text())
    environment, _, _ = toolchain(manifest, args.chromium_cache)
    if not args.cache.is_absolute() or args.cache.is_symlink() or ".." in args.cache.parts:
        raise ValueError("Use an absolute, unlinked owned external input cache")
    key = hashlib.sha256(
        (digest(MANIFEST) + digest(ROOT / "tools/local_services.py")).encode()
    ).hexdigest()[:16]
    build = args.cache / "build" / f"ocr-{key}"
    metadata = json.loads((build / "stainless-inputs.json").read_text())
    executable = build / "tesseract/bin/tesseract"
    if metadata["manifest_sha256"] != digest(MANIFEST) or metadata["ocr_cli"] != str(executable):
        raise ValueError("OCR build inputs differ; rerun the pinned preparation")
    verify_file(executable, {"sha256": metadata["ocr_cli_sha256"]})
    configuration = verify_configuration(build)
    dependencies = subprocess.check_output(
        ["/usr/bin/otool", "-L", str(executable)], text=True, env=environment
    )
    for line in dependencies.splitlines()[1:]:
        dependency = line.strip().split(" (", 1)[0]
        if not dependency.startswith(("/usr/lib/", "/System/Library/")):
            raise ValueError(f"OCR tool has a build-host or unresolved dependency: {dependency}")
    fixture_build = args.cache / "build" / ("ocr-fixture-" + digest(GENERATOR)[:16])
    if fixture_build.is_symlink():
        raise ValueError("Refusing a linked fixture build")
    fixture_build.mkdir(parents=True, exist_ok=True)
    generator = fixture_build / "ocr-fixture"
    subprocess.run([
        "/usr/bin/xcrun", "swiftc", "-O", "-module-cache-path",
        str(fixture_build / "module-cache"), "-o", str(generator), str(GENERATOR),
    ], check=True, env=environment)
    report = {
        "checked_at_unix": int(time.time()),
        "scope": corpus["scope"],
        "manifest_sha256": digest(MANIFEST),
        "corpus_sha256": digest(CORPUS),
        "generator_source_sha256": digest(GENERATOR),
        "generator_binary_sha256": digest(generator),
        "ocr_binary_sha256": digest(executable),
        "configuration": configuration,
        "macho_dependencies": dependencies,
        "models": {},
        "cases": [],
        "browser_pdf_ax_installed_verified": False,
    }
    limits = dict(corpus["limits"])
    limits["maximum_seconds_per_case"] = limits["maximum_case_seconds"]
    environment.update({"OMP_NUM_THREADS": "1", "OMP_THREAD_LIMIT": "1"})
    with tempfile.TemporaryDirectory(prefix="ocr-probe-", dir=args.cache) as temporary:
        temporary = Path(temporary)
        data = temporary / "tessdata"
        data.mkdir()
        for language in ("eng", "ita", "fra", "deu", "spa"):
            name = "ocr-" + language
            source = args.cache / "models" / (name + ".bin")
            verify_file(source, manifest["models"][name])
            shutil.copyfile(source, data / (language + ".traineddata"))
            report["models"][language] = manifest["models"][name]["sha256"]
        generated = subprocess.check_output(
            [str(generator), str(CORPUS), str(temporary)], env=environment, text=True
        )
        report["raster"] = json.loads(generated)
        for index, case in enumerate(corpus["cases"]):
            image = temporary / f"case-{index}.pgm"
            command = [
                str(executable), str(image), "stdout", "--tessdata-dir", str(data),
                "--psm", "6", "--oem", "1", "-l", case["language"],
                "-c", "tessedit_create_tsv=1",
            ]
            seconds, rss, stdout, stderr = run_bounded(command, environment, limits)
            rows = parse_tsv(stdout, corpus["generator"]["width"], corpus["generator"]["height"])
            actual = " ".join(row["text"] for row in rows)
            report["cases"].append({
                "language": case["language"], "expected": case["text"],
                "actual": actual, "passed": words(actual) == words(case["text"]),
                "words": rows, "seconds": seconds,
                "peak_rss_bytes": rss,
                "rss_observed": rss > 0, "stderr": stderr,
                "image_sha256": digest(image),
            })
        missing = [
            str(executable), str(temporary / "case-0.pgm"), "stdout",
            "--tessdata-dir", str(data), "--psm", "6", "--oem", "1",
            "-l", "synthetic_missing_model",
        ]
        try:
            run_bounded(missing, environment, limits)
        except RuntimeError as error:
            if "Failed loading language" not in str(error):
                raise
            report["missing_model_rejected"] = True
        else:
            report["missing_model_rejected"] = False
    report["quality_passed"] = (
        all(case["passed"] for case in report["cases"]) and
        report["missing_model_rejected"]
    )
    report["rss_measurement"] = "Live PID watchdog plus wait4 kernel peak RSS"
    report["all_rss_observed"] = all(
        case["rss_observed"] for case in report["cases"]
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.output.is_symlink():
        raise ValueError("Refusing a linked report path")
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps({
        "quality_passed": report["quality_passed"],
        "all_rss_observed": report["all_rss_observed"],
        "cases": [{key: case[key] for key in ("language", "passed", "actual")}
                  for case in report["cases"]],
        "report": str(args.output),
    }, ensure_ascii=False))
    return 0 if report["quality_passed"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chromium-cache", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        return evaluate(args)
    except (OSError, ValueError, RuntimeError, TimeoutError, MemoryError,
            subprocess.SubprocessError) as error:
        print(f"OCR feasibility failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

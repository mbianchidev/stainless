#!/usr/bin/env python3
"""Prepare only pinned synthetic OCR inputs for the actual utility-worker test."""

import argparse
import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import struct
import sys
import zlib

from local_services import MANIFEST, ROOT, digest, fetch, toolchain, verify_file


def prepare(args):
    manifest = json.loads(MANIFEST.read_text())
    corpus = ROOT / "tests/fixtures/local_ocr.json"
    specification = json.loads(corpus.read_text())
    environment, _, _ = toolchain(manifest, args.chromium_cache)
    if (
        not args.output.is_absolute() or ".." in args.output.parts or
        args.output.exists() or args.output.is_symlink() or
        args.output.parent.is_symlink() or
        args.output.resolve().is_relative_to(ROOT)
    ):
        raise ValueError("Choose a new, absolute external fixture directory")
    if (
        not args.cache.is_absolute() or ".." in args.cache.parts or
        args.cache.is_symlink() or args.cache.resolve().is_relative_to(ROOT)
    ):
        raise ValueError("Use the existing dedicated external service-input cache")
    args.output.mkdir(mode=0o700, parents=True)
    generator = args.output / "ocr-fixture"
    source = ROOT / "tools/local_services/ocr_fixture.swift"
    subprocess.run([
        "/usr/bin/xcrun", "swiftc", "-O", "-module-cache-path",
        str(args.output / "module-cache"), "-o", str(generator), str(source),
    ], check=True, env=environment)
    raster = json.loads(subprocess.check_output(
        [str(generator), str(corpus), str(args.output)],
        env=environment, text=True,
    ))
    models = {}
    for language in ("eng", "ita", "fra", "deu", "spa"):
        entry = manifest["models"]["ocr-" + language]
        original = args.cache / "models" / ("ocr-" + language + ".bin")
        fetch(original, entry)
        target = args.output / (language + ".traineddata")
        shutil.copyfile(original, target)
        verify_file(target, entry)
        models[language] = {
            "file": target.name, "bytes": target.stat().st_size,
            "sha256": digest(target),
        }
    width = specification["generator"]["width"]
    height = specification["generator"]["height"]
    cases = []
    for index, case in enumerate(specification["cases"]):
        pgm = args.output / f"case-{index}.pgm"
        header, dimensions, maximum, pixels = pgm.read_bytes().split(b"\n", 3)
        if (
            (header, dimensions, maximum) != (
                b"P5", f"{width} {height}".encode(), b"255"
            ) or len(pixels) != width * height
        ):
            raise ValueError("Frozen offscreen generator returned unexpected pixels")
        rgba = bytearray(width * height * 4)
        for offset, pixel in enumerate(pixels):
            rgba[offset * 4:offset * 4 + 4] = bytes((pixel, pixel, pixel, 255))
        target = args.output / f"case-{index}.rgba"
        with target.open("xb") as stream:
            stream.write(rgba)
        def chunk(name, data):
            return (
                struct.pack(">I", len(data)) + name + data +
                struct.pack(">I", zlib.crc32(name + data))
            )
        png = args.output / f"case-{index}.png"
        scanlines = b"".join(
            b"\0" + pixels[row * width:(row + 1) * width]
            for row in range(height)
        )
        with png.open("xb") as stream:
            stream.write(
                b"\x89PNG\r\n\x1a\n" +
                chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0)) +
                chunk(b"IDAT", zlib.compress(scanlines)) + chunk(b"IEND", b"")
            )
        cases.append({
            "language": case["language"], "expected": case["text"],
            "file": target.name, "sha256": digest(target),
            "encoded_file": png.name, "encoded_sha256": digest(png),
        })
    result = {
        "schema_version": 1, "manifest_sha256": digest(MANIFEST),
        "corpus_sha256": digest(corpus), "generator_sha256": digest(source),
        "width": width, "height": height, "models": models, "cases": cases,
        "raster": raster,
    }
    (args.output / "fixture.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"fixture": str(args.output), "cases": len(cases)}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--chromium-cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if (
            not args.cache.is_absolute() or ".." in args.cache.parts or
            args.cache.is_symlink() or
            args.cache.resolve().is_relative_to(ROOT)
        ):
            raise ValueError("Use the existing dedicated external service-input cache")
        descriptor = os.open(
            args.cache / ".inputs.lock", os.O_RDWR | os.O_NOFOLLOW
        )
        with os.fdopen(descriptor, "r+") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            prepare(args)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Native OCR fixture preparation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

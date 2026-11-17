#!/usr/bin/env python3
"""Prepare pinned synthetic inputs for actual local utility-worker tests."""

import argparse
from array import array
import fcntl
import json
import math
import os
from pathlib import Path
import random
import shutil
import subprocess
import struct
import sys
import wave
import zlib

from local_services import MANIFEST, ROOT, digest, fetch, toolchain, verify_file


def write_png(path, pixels, width, height, channels):
    if channels not in (1, 3) or len(pixels) != width * height * channels:
        raise ValueError("Invalid bounded synthetic PNG pixels")

    def chunk(name, data):
        return (
            struct.pack(">I", len(data)) + name + data +
            struct.pack(">I", zlib.crc32(name + data))
        )

    stride = width * channels
    scanlines = b"".join(
        b"\0" + pixels[row * stride:(row + 1) * stride]
        for row in range(height)
    )
    with path.open("xb") as stream:
        stream.write(
            b"\x89PNG\r\n\x1a\n" +
            chunk(b"IHDR", struct.pack(
                ">IIBBBBB", width, height, 8, 0 if channels == 1 else 2, 0, 0, 0
            )) +
            chunk(b"IDAT", zlib.compress(scanlines)) + chunk(b"IEND", b"")
        )


def write_pdf(path, pixels, width, height, rotation=0, existing_text=""):
    if (
        width <= 0 or height <= 0 or len(pixels) != width * height or
        rotation not in (0, 90, 180, 270) or not existing_text.isascii()
    ):
        raise ValueError("Invalid synthetic grayscale PDF fixture")
    text = existing_text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
    content = b"q 800 0 0 120 0 0 cm /Im0 Do Q\n"
    if text:
        content += f"BT /F1 12 Tf 12 50 Td ({text}) Tj ET\n".encode("ascii")
    compressed = zlib.compress(pixels)
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 800 120] "
            f"/Rotate {rotation} /Contents 4 0 R /Resources << "
            f"/XObject << /Im0 5 0 R >> /Font << /F1 6 0 R >> >> >>"
        ).encode("ascii"),
        f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"endstream",
        (
            f"<< /Type /XObject /Subtype /Image /Width {width} /Height {height} "
            f"/ColorSpace /DeviceGray /BitsPerComponent 8 "
            f"/Filter /FlateDecode /Length {len(compressed)} >>\nstream\n"
        ).encode("ascii") + compressed + b"\nendstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    document = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for index, value in enumerate(objects, 1):
        offsets.append(len(document))
        document.extend(f"{index} 0 obj\n".encode() + value + b"\nendobj\n")
    xref = len(document)
    document.extend(
        f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    )
    for offset in offsets:
        document.extend(f"{offset:010d} 00000 n \n".encode())
    document.extend(
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref}\n%%EOF\n".encode()
    )
    with path.open("xb") as stream:
        stream.write(document)


def write_ocr_bounds(output, pixels, width, height):
    axis = 4096
    if width > axis or height > axis or len(pixels) != width * height:
        raise ValueError("Frozen OCR raster does not fit the boundary fixture")
    padded = bytearray(b"\xff") * (axis * axis)
    for row in range(height):
        padded[row * axis:row * axis + width] = pixels[row * width:(row + 1) * width]
    write_png(output / "at-limit.png", padded, axis, axis, 1)
    write_png(output / "too-wide.png", b"\xff" * (axis + 1), axis + 1, 1, 1)


def write_ocr_encoded_limit(path):
    pixels = random.Random(0x53544149).randbytes(4096 * 4096 * 3)
    write_png(path, pixels, 4096, 4096, 3)
    if path.stat().st_size <= 16777216:
        raise ValueError("Synthetic noisy PNG did not exceed the encoded limit")


def prepare_vision(args, manifest):
    from local_vision_check import CORPUS, raster
    from local_services import verify_model_references

    specification = json.loads(CORPUS.read_text())
    selection = manifest["runtime_selection"]["image_description"]
    models = {}
    identifiers = [selection["model"], selection["projector"]]
    references = verify_model_references(manifest, args.cache, identifiers)
    for kind, identifier in zip(("model", "projector"), identifiers):
        entry = manifest["models"][identifier]
        original = args.cache / "models" / (identifier + ".bin")
        fetch(original, entry)
        name = kind + ".gguf"
        target = args.output / name
        shutil.copyfile(original, target)
        verify_file(target, entry)
        models[name] = {
            "id": identifier, "file": name, "bytes": entry["bytes"],
            "sha256": entry["sha256"],
        }
    width = specification["raster"]["width"]
    height = specification["raster"]["height"]
    cases = []
    for case in specification["cases"]:
        ppm = args.output / (case["id"] + ".ppm")
        raster(case, specification["raster"], ppm)
        header, dimensions, maximum, rgb = ppm.read_bytes().split(b"\n", 3)
        if (
            (header, dimensions, maximum) != (
                b"P6", f"{width} {height}".encode(), b"255"
            ) or len(rgb) != width * height * 3
        ):
            raise ValueError("Frozen image-description raster changed")
        rgba = bytearray(width * height * 4)
        for index in range(width * height):
            rgba[index * 4:index * 4 + 4] = rgb[index * 3:index * 3 + 3] + b"\xff"
        target = ppm.with_suffix(".rgba")
        with target.open("xb") as stream:
            stream.write(rgba)
        png = ppm.with_suffix(".png")
        write_png(png, rgb, width, height, 3)
        cases.append({
            "id": case["id"], "file": target.name, "sha256": digest(target),
            "encoded_file": png.name, "encoded_sha256": digest(png),
            "required_concepts": case["required_concepts"],
            "forbidden_concepts": case["forbidden_concepts"],
            "uniform": case["shape"] == "none",
        })
    result = {
        "schema_version": 1, "kind": "vision", "manifest_sha256": digest(MANIFEST),
        "corpus_sha256": digest(CORPUS), "models": models,
        "width": width, "height": height, "cases": cases,
        "reviewed_model_references": references,
    }
    (args.output / "fixture.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"fixture": str(args.output), "cases": len(cases)}))


def negative_speech_pcm(specification):
    rate = specification["sample_rate_hz"]
    seconds = specification["seconds"]
    if rate != 16000 or not 0 < seconds <= 30:
        raise ValueError("Frozen non-speech input exceeds the 16 kHz PCM window")
    count = rate * seconds
    random_source = random.Random(specification["noise"]["seed"])
    noise_peak = specification["noise"]["peak"]
    tone = specification["tones"]
    frequencies = tone["frequencies_hz"]
    if (
        not 0 < noise_peak <= 32767 or not 0 < tone["peak"] <= 32767 or
        not frequencies or any(not 0 < value < rate / 2 for value in frequencies)
    ):
        raise ValueError("Invalid frozen non-speech amplitude or frequency")
    result = {
        "silence": array("h", [0]) * count,
        "noise": array("h", (
            random_source.randint(-noise_peak, noise_peak) for _ in range(count)
        )),
        "tones": array("h", (
            round(tone["peak"] * sum(
                math.sin(2 * math.pi * frequency * index / rate)
                for frequency in frequencies
            ) / len(frequencies)) for index in range(count)
        )),
        "silence_after_speech": array("h", [0]) * (
            rate * specification["silence_after_speech"]["seconds"]
        ),
    }
    encoded = {}
    for name, samples in result.items():
        if not samples or len(samples) > 480000:
            raise ValueError("Frozen non-speech input exceeds the PCM window")
        if sys.byteorder != "little":
            samples.byteswap()
        encoded[name] = samples.tobytes()
    return encoded


def prepare_speech(args, manifest, environment):
    from local_speech_check import CORPUS, NEURAL, prepare_neural, synthesize_neural

    corpus = json.loads(CORPUS.read_text())
    recipe = prepare_neural(args.cache)
    generator = args.cache / "build" / (
        "espeak-" + manifest["sources"]["espeak"]["revision"][:8]
    )
    espeak = generator / "src/espeak-ng"
    if not espeak.is_file():
        raise ValueError("Build the pinned synthetic speech generator first")
    environment["ESPEAK_DATA_PATH"] = str(generator)
    cases = []
    for case in corpus["cases"]:
        wav = args.output / (case["id"] + ".wav")
        wav_sha = synthesize_neural(
            espeak, environment, case, recipe, args.cache, wav
        )
        with wave.open(str(wav), "rb") as source:
            if (
                source.getnchannels() != 1 or source.getsampwidth() != 2 or
                source.getframerate() != 16000 or source.getnframes() > 480000
            ):
                raise ValueError("Invalid native speech fixture PCM")
            pcm = source.readframes(source.getnframes())
        target = wav.with_suffix(".pcm")
        with target.open("xb") as stream:
            stream.write(pcm)
        cases.append({
            "id": case["id"], "language": case["language"],
            "content_words": case["content_words"], "file": target.name,
            "sha256": digest(target), "wav_sha256": wav_sha, "speech": True,
        })
    for name, pcm in negative_speech_pcm(corpus["negative_cases"]).items():
        target = args.output / (name + ".pcm")
        with target.open("xb") as stream:
            stream.write(pcm)
        cases.append({
            "id": name, "language": "en", "file": target.name,
            "sha256": digest(target), "speech": False,
        })
    model_id = manifest["runtime_selection"]["speech"]["model"]
    entry = manifest["models"][model_id]
    original = args.cache / "models" / (model_id + ".bin")
    fetch(original, entry)
    target = args.output / "model.bin"
    shutil.copyfile(original, target)
    verify_file(target, entry)
    result = {
        "schema_version": 1, "kind": "speech",
        "manifest_sha256": digest(MANIFEST), "corpus_sha256": digest(CORPUS),
        "neural_recipe_sha256": digest(NEURAL), "generator_sha256": digest(espeak),
        "minimum_content_words": corpus["minimum_content_words"],
        "model": {
            "id": model_id, "file": target.name, "bytes": entry["bytes"],
            "sha256": entry["sha256"],
        },
        "cases": cases,
    }
    (args.output / "fixture.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"fixture": str(args.output), "cases": len(cases)}))


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
    if args.kind == "speech":
        prepare_speech(args, manifest, environment)
        return
    if args.kind == "vision":
        prepare_vision(args, manifest)
        return
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
        png = args.output / f"case-{index}.png"
        write_png(png, pixels, width, height, 1)
        pdf = args.output / f"case-{index}.pdf"
        write_pdf(pdf, pixels, width, height)
        if index == 0:
            write_ocr_bounds(args.output, pixels, width, height)
            write_ocr_encoded_limit(args.output / "encoded-too-large.png")
            write_pdf(args.output / "rotated.pdf", pixels, width, height, rotation=90)
            write_pdf(
                args.output / "existing-text.pdf", pixels, width, height,
                existing_text="Existing author text stays intact.",
            )
        cases.append({
            "language": case["language"], "expected": case["text"],
            "file": target.name, "sha256": digest(target),
            "encoded_file": png.name, "encoded_sha256": digest(png),
            "pdf_file": pdf.name, "pdf_sha256": digest(pdf),
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
    parser.add_argument("--kind", choices=("ocr", "speech", "vision"), default="ocr")
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
        print(f"Native {args.kind} fixture preparation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

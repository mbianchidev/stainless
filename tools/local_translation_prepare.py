#!/usr/bin/env python3
"""Convert pinned original Marian NPZ weights; no browser/catalog/publication claim."""

import argparse
import fcntl
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import zipfile

from local_services import MANIFEST, ROOT, digest, extract_model_zip, fetch
from local_services import run_bounded, toolchain, verify_file


LOCK = ROOT / "tools/local_services/translation-requirements.txt"
VERSIONS = {
    "ctranslate2": "4.9.0",
    "numpy": "2.4.6",
    "PyYAML": "6.0.3",
}
CONVERTER_FILES = {
    "model.bin", "config.json", "source_vocabulary.json",
    "target_vocabulary.json", "shared_vocabulary.json", "vocabulary.json",
}
PACK_FILES = CONVERTER_FILES | {
    "source.spm", "target.spm", "ORIGINAL_MODEL_LICENSE.txt",
    "ORIGINAL_MODEL_README.md", "NOTICES.txt",
}


def has_vocabulary(names):
    return (
        "shared_vocabulary.json" in names or "vocabulary.json" in names or
        {"source_vocabulary.json", "target_vocabulary.json"} <= names
    )


def conversion_recipe(entry):
    return hashlib.sha256(json.dumps({
        "entry": entry, "versions": VERSIONS, "lock": digest(LOCK),
        "script": digest(Path(__file__)),
        "input_helper": digest(ROOT / "tools/local_services.py"),
        "quantization": "int8",
    }, sort_keys=True).encode()).hexdigest()


def verify_output(directory, recipe):
    if directory.is_symlink():
        raise ValueError(f"Refusing a linked converted model: {directory}")
    record = directory / "provenance.json"
    if not record.is_file() or record.is_symlink():
        raise ValueError("Missing regular converted-model provenance")
    provenance = json.loads(record.read_text())
    if not isinstance(provenance, dict) or provenance.get("recipe_sha256") != recipe:
        raise ValueError("Existing converted model has different provenance")
    files = provenance.get("files")
    if not isinstance(files, dict) or not set(files) <= PACK_FILES:
        raise ValueError("Invalid converted-model file records")
    required = PACK_FILES - CONVERTER_FILES | {"model.bin", "config.json"}
    if not required <= set(files) or not has_vocabulary(set(files)):
        raise ValueError("Incomplete converted-model file records")
    expected = set(files) | {"provenance.json"}
    if {p.name for p in directory.iterdir()} != expected:
        raise ValueError("Preserve unknown files in the converted model")
    for name, metadata in files.items():
        if not isinstance(metadata, dict) or not {"bytes", "sha256"} <= metadata.keys():
            raise ValueError(f"Invalid converted-model hash record: {name}")
        verify_file(directory / name, metadata)
    return provenance


def prepare(args):
    manifest = json.loads(MANIFEST.read_text())
    entry = manifest["translation_models"][args.pair]
    environment, _, _ = toolchain(manifest, args.chromium_cache)
    packages = args.cache / "translation-packages"
    for name, version in VERSIONS.items():
        distribution = importlib.metadata.distribution(name)
        if distribution.version != version or not Path(
            distribution.locate_file("")
        ).resolve().is_relative_to(packages.resolve()):
            raise ValueError(f"Use only the hash-locked conversion environment: {name}")
    recipe = conversion_recipe(entry)
    destination = args.cache / "converted" / f"opus-{args.pair}-ct2-int8-{recipe[:16]}"
    if destination.exists():
        result = verify_output(destination, recipe)
        print(json.dumps({"directory": str(destination), "provenance": result}, indent=2))
        return
    archive = args.cache / "downloads" / f"opus-{args.pair}-{entry['sha256'][:16]}.zip"
    fetch(archive, entry)
    source = args.cache / "sources" / f"opus-{args.pair}-{entry['sha256'][:16]}"
    extract_model_zip(archive, source, entry)
    destination.parent.mkdir(parents=True, exist_ok=True)
    environment.update({
        "PYTHONPATH": str(packages),
        "PYTHONNOUSERSITE": "1",
        "OMP_NUM_THREADS": "1",
        "VECLIB_MAXIMUM_THREADS": "1",
    })
    with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
        staged = Path(temporary) / "model"
        command = [
            sys.executable, "-c",
            "from ctranslate2.converters.marian import main; main()",
            "--model_path", str(source / entry["model_file"]),
            "--vocab_paths", str(source / entry["vocab_file"]),
            str(source / entry["vocab_file"]),
            "--output_dir", str(staged), "--quantization", "int8",
        ]
        seconds, rss, stdout, stderr = run_bounded(
            command, environment,
            {"maximum_seconds_per_case": 300, "maximum_rss_bytes": 2147483648},
        )
        actual = {p.name for p in staged.iterdir()}
        if (
            not {"model.bin", "config.json"} <= actual or
            not actual <= CONVERTER_FILES or not has_vocabulary(actual)
        ):
            raise ValueError("Converter returned an unexpected model layout")
        for path in staged.iterdir():
            if not path.is_file() or path.is_symlink():
                raise ValueError("Converter returned a non-data model member")
        shutil.copyfile(source / entry["source_tokenizer"], staged / "source.spm")
        shutil.copyfile(source / entry["target_tokenizer"], staged / "target.spm")
        shutil.copyfile(source / "LICENSE", staged / "ORIGINAL_MODEL_LICENSE.txt")
        shutil.copyfile(source / "README.md", staged / "ORIGINAL_MODEL_README.md")
        reference = entry["reference"]
        declaration = reference["declared_license"] or "not stated in this reference"
        (staged / "NOTICES.txt").write_text(
            f"Original pretrained model: OPUS-MT {args.pair}\n"
            f"Attribution: {entry['attribution']}\n"
            f"Original source: {entry['url']}\n"
            f"Original license: {entry['original_license']} "
            f"({entry['original_license_uri']})\n"
            "Modification: original Marian NPZ weights converted and quantized "
            "to CTranslate2 int8 with version 4.9.0.\n"
            f"Reference ({reference['kind']}): {reference['url']}\n"
            f"Reference license declaration: {declaration}; "
            "does not replace the original pretrained-weight attribution/license.\n"
            "No independent licensor confirmation or rights clearance is asserted.\n"
        )
        records = {
            p.name: {"bytes": p.stat().st_size, "sha256": digest(p)}
            for p in staged.iterdir()
        }
        provenance = {
            "recipe_sha256": recipe,
            "original_archive_sha256": entry["sha256"],
            "original_license": entry["original_license"],
            "reference": reference,
            "converter_versions": VERSIONS,
            "converter_lock_sha256": digest(LOCK),
            "converter_source_sha256": digest(Path(__file__)),
            "input_helper_sha256": digest(ROOT / "tools/local_services.py"),
            "weight_loading": "Native Marian NPZ; NumPy default allow_pickle=False and YAML safe_load; no remote code",
            "quantization": "int8",
            "files": records,
            "scope": "Build-only conversion, not native translation or browser acceptance",
        }
        (staged / "provenance.json").write_text(
            json.dumps(provenance, indent=2, sort_keys=True) + "\n"
        )
        staged.rename(destination)
    verify_output(destination, recipe)
    print(json.dumps({
        "directory": str(destination), "provenance": provenance,
        "conversion": {
            "seconds": seconds, "peak_rss_bytes": rss,
            "stdout": stdout, "stderr": stderr,
        },
    }, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chromium-cache", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument(
        "--pair", choices=json.loads(MANIFEST.read_text()).get("translation_models", {}),
        required=True,
    )
    args = parser.parse_args()
    try:
        if (
            not args.cache.is_absolute() or args.cache.is_symlink() or
            ".." in args.cache.parts or args.cache.resolve().is_relative_to(ROOT)
        ):
            raise ValueError("Use an absolute, unlinked external build cache")
        args.cache.mkdir(parents=True, exist_ok=True)
        descriptor = os.open(
            args.cache / ".translation-inputs.lock",
            os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600,
        )
        with os.fdopen(descriptor, "r+") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            prepare(args)
    except (OSError, ValueError, RuntimeError, TimeoutError, MemoryError,
            zipfile.BadZipFile, importlib.metadata.PackageNotFoundError,
            subprocess.SubprocessError) as error:
        print(f"Translation input preparation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

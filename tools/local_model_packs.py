#!/usr/bin/env python3
"""Prepare deterministic reviewed model data, not a release or browser install."""

import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
import tempfile
import zipfile

from local_services import MANIFEST, ROOT, digest, extract, fetch, runtime_notices
from local_translation_prepare import conversion_recipe, verify_output


MODEL_TAG = "stainless-models-v1"
ASSET_BASE = (
    "https://github.com/stainless-browser/stainless/releases/download/" + MODEL_TAG + "/"
)
LANGUAGES = {
    "en": "English", "it": "Italian", "fr": "French",
    "de": "German", "es": "Spanish",
}


def encoded(value):
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode()


def member_name(name):
    return (
        isinstance(name, str) and name not in (".", "..") and
        re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,95}", name) is not None and
        Path(name).suffix in (".bin", ".gguf", ".json", ".spm", ".txt", ".md", ".traineddata")
    )


def file_metadata(path):
    info = path.lstat()
    if (
        not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or
        info.st_uid != os.getuid() or info.st_mode & 0o111
    ):
        raise ValueError(f"Expected unlinked regular model data: {path.name}")
    return {"bytes": info.st_size, "sha256": digest(path)}


def write_pack(destination, members):
    if destination.exists() or destination.is_symlink() or not members:
        raise ValueError("Choose a new model archive with a strict nonempty member set")
    if not all(member_name(name) for name in members):
        raise ValueError("Model archive member is not a single reviewed data filename")
    records = {}
    with tempfile.NamedTemporaryFile(
        prefix=destination.name + ".", suffix=".partial",
        dir=destination.parent, delete=False,
    ) as temporary:
        staged = Path(temporary.name)
    try:
        with zipfile.ZipFile(staged, "w", compression=zipfile.ZIP_STORED) as archive:
            for name in sorted(members):
                source = members[name]
                metadata = (
                    file_metadata(source) if isinstance(source, Path) else
                    {"bytes": len(source), "sha256": hashlib.sha256(source).hexdigest()}
                )
                entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
                entry.create_system = 3
                entry.external_attr = 0o100400 << 16
                entry.compress_type = zipfile.ZIP_STORED
                entry.file_size = metadata["bytes"]
                actual = hashlib.sha256()
                count = 0
                with archive.open(entry, "w") as target:
                    if isinstance(source, Path):
                        descriptor = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
                        with os.fdopen(descriptor, "rb") as stream:
                            before = os.fstat(stream.fileno())
                            if (
                                not stat.S_ISREG(before.st_mode) or
                                before.st_nlink != 1 or before.st_uid != os.getuid() or
                                before.st_mode & 0o111
                            ):
                                raise ValueError("Model member changed before archive reading")
                            while chunk := stream.read(1048576):
                                actual.update(chunk)
                                count += len(chunk)
                                if count > metadata["bytes"]:
                                    raise ValueError("Model member grew while preparing its pack")
                                target.write(chunk)
                            after = os.fstat(stream.fileno())
                            identity = lambda value: (
                                value.st_dev, value.st_ino, value.st_size,
                                value.st_mode, value.st_nlink, value.st_uid,
                                value.st_mtime_ns, value.st_ctime_ns,
                            )
                            if identity(after) != identity(before):
                                raise ValueError("Model member changed while preparing its pack")
                    else:
                        target.write(source)
                        actual.update(source)
                        count = len(source)
                if count != metadata["bytes"] or actual.hexdigest() != metadata["sha256"]:
                    raise ValueError("Model member bytes differ from their reviewed input")
                records[name] = metadata
        archive_record = file_metadata(staged)
        staged.rename(destination)
        return {
            "asset": destination.name, "archive_bytes": archive_record["bytes"],
            "archive_sha256": archive_record["sha256"],
            "unpacked_bytes": sum(item["bytes"] for item in records.values()),
            "members": records,
        }
    finally:
        staged.unlink(missing_ok=True)


def model_sources(manifest, cache):
    result = {}
    for name, entry in manifest["sources"].items():
        archive = cache / "downloads" / f"{name}-{entry['revision'][:8]}.tar.gz"
        source = cache / "sources" / f"{name}-{entry['revision'][:8]}"
        fetch(archive, entry)
        extract(archive, source, entry)
        result[name] = str(source)
    return result


def reference(manifest, cache, name):
    entry = manifest["model_reference_inputs"][name]
    path = cache / "model-notices" / name
    fetch(path, entry)
    return path


def model(manifest, cache, name):
    entry = manifest["models"][name]
    path = cache / "models" / (name + ".bin")
    fetch(path, entry)
    return path


def speech_languages(sources):
    source = Path(sources["whisper"]) / "src/whisper.cpp"
    text = source.read_text()
    match = re.search(
        r"static const std::map<std::string, std::pair<int, std::string>> g_lang = \{(.*?)\n\};",
        text, re.S,
    )
    if not match:
        raise ValueError("Pinned Whisper language table is unavailable")
    entries = re.findall(r'\{\s*"([a-z]{2,3})",\s*\{\s*(\d+),', match[1])
    if len(entries) != 100 or {int(index) for _, index in entries} != set(range(100)):
        raise ValueError("Pinned Whisper language table differs from the reviewed finite set")
    return [name for name, _ in sorted(entries, key=lambda value: int(value[1]))]


def emit_cpp(catalog):
    lines = [
        '#include "chrome/browser/stainless_local/model_catalog.h"',
        '#include "base/no_destructor.h"',
        "",
        "namespace stainless_local {",
        "const std::vector<ModelPack>& ModelCatalog() {",
        "  static const base::NoDestructor<std::vector<ModelPack>> packs(std::vector<ModelPack>{",
    ]
    for pack in catalog["packs"]:
        quote = lambda value: json.dumps(value, ensure_ascii=True)
        lines.extend([
            "    {",
            f"      {quote(pack['id'])}, {quote(pack['feature'])},",
            f"      {quote(pack['name'])}, {quote(ASSET_BASE + pack['asset'])},",
            f"      {quote(pack['archive_sha256'])}, {pack['archive_bytes']}ULL,",
            f"      {pack['unpacked_bytes']}ULL, {quote(pack['license'])},",
            "      {" + ", ".join(quote(value) for value in pack["languages"]) + "},",
            "      {",
        ])
        for name, metadata in sorted(pack["members"].items()):
            lines.append(
                f"        {{{quote(name)}, {quote(metadata['sha256'])}, "
                f"{metadata['bytes']}ULL}},"
            )
        lines.extend(["      },", "    },"])
    lines.extend(["  });", "  return *packs;", "}", "}  // namespace stainless_local", ""])
    return "\n".join(lines)


def prepare(args):
    manifest = json.loads(MANIFEST.read_text())
    sources = model_sources(manifest, args.cache)
    notices, notice_records = runtime_notices(manifest, sources, args.cache)
    common = {"RUNTIME_NOTICES.txt": notices.encode()}
    language_set = speech_languages(sources)
    packs = []
    for pair, entry in manifest["translation_models"].items():
        recipe = conversion_recipe(entry)
        directory = args.cache / "converted" / f"opus-{pair}-ct2-int8-{recipe[:16]}"
        provenance = verify_output(directory, recipe)
        members = {name: directory / name for name in provenance["files"]}
        members["provenance.json"] = directory / "provenance.json"
        members.update(common)
        source, target = pair.split("-")
        pack = write_pack(
            args.output / f"stainless-model-opus-mt-{pair}-ct2-int8-v1.zip", members
        )
        pack.update({
            "id": "translation-" + pair, "feature": "translation",
            "name": f"OPUS-MT {LANGUAGES[source]} to {LANGUAGES[target]}",
            "languages": [source, target],
            "license": "Original weights CC-BY-4.0; preserve original and reference notices",
        })
        packs.append(pack)
    for language, name in (
        ("eng", "English"), ("ita", "Italian"), ("fra", "French"),
        ("deu", "German"), ("spa", "Spanish"),
    ):
        identifier = "ocr-" + language
        entry = manifest["models"][identifier]
        members = {
            **common, language + ".traineddata": model(manifest, args.cache, identifier),
            "LICENSE.txt": reference(manifest, args.cache, "tessdata-fast-license"),
            "MODEL_CARD.md": reference(manifest, args.cache, "tessdata-fast-card"),
            "MODEL_PROVENANCE.json": encoded({
                "model": entry, "modification": "No weight modification",
                "runtime": manifest["sources"]["tesseract"],
            }),
        }
        pack = write_pack(
            args.output / f"stainless-model-tesseract-fast-{language}-v1.zip", members
        )
        pack.update({
            "id": identifier, "feature": "ocr", "name": "Tesseract fast " + name,
            "languages": [language], "license": "Apache-2.0",
        })
        packs.append(pack)
    speech = manifest["runtime_selection"]["speech"]
    members = {
        **common, "model.bin": model(manifest, args.cache, speech["model"]),
        "ORIGINAL_MODEL_LICENSE.txt": reference(manifest, args.cache, "whisper-original-license"),
        "MODEL_CARD.md": reference(manifest, args.cache, "whisper-model-card"),
        "MODEL_PROVENANCE.json": encoded({
            "model": manifest["models"][speech["model"]], "selection": speech,
            "languages": language_set,
            "modification": "Publisher conversion to GGML and Q5_0 quantization; no new weight conversion",
            "runtime": manifest["sources"]["whisper"],
        }),
    }
    pack = write_pack(args.output / speech["asset"], members)
    pack.update({
        "id": "speech-multilingual", "feature": "speech",
        "name": "Whisper large-v3 Turbo Q5 multilingual",
        "languages": language_set, "license": "MIT",
    })
    packs.append(pack)
    vision = manifest["runtime_selection"]["image_description"]
    members = {
        **common, "model.gguf": model(manifest, args.cache, vision["model"]),
        "projector.gguf": model(manifest, args.cache, vision["projector"]),
        "LICENSE.txt": reference(manifest, args.cache, "qwen35-original-license"),
        "ORIGINAL_MODEL_CARD.md": reference(manifest, args.cache, "qwen35-original-card"),
        "PUBLISHER_MODEL_CARD.md": reference(manifest, args.cache, "qwen35-gguf-card"),
        "MODEL_PROVENANCE.json": encoded({
            "model": manifest["models"][vision["model"]],
            "projector": manifest["models"][vision["projector"]],
            "selection": vision, "runtime": manifest["sources"]["llama"],
            "modification": "Publisher Q8_0 GGUF and F16 projector; measured uniform-color completion is runtime code, not a weight change",
            "limitations": "Publisher card does not prove exact original tensor revision or independent rights clearance",
        }),
    }
    pack = write_pack(args.output / vision["asset"], members)
    pack.update({
        "id": "image-description", "feature": "image_description",
        "name": "Qwen3.5-0.8B Q8_0 with F16 projector",
        "languages": ["en"], "license": "Apache-2.0; retain publisher/original provenance",
    })
    packs.append(pack)
    catalog = {
        "schema_version": 1, "model_release_tag": MODEL_TAG,
        "inputs_sha256": digest(MANIFEST), "builder_sha256": digest(Path(__file__)),
        "native_notice_records": notice_records,
        "packs": sorted(packs, key=lambda value: value["id"]),
    }
    if len(packs) != 15 or len({pack["id"] for pack in packs}) != 15:
        raise ValueError("The reviewed catalog must contain exactly fifteen distinct data packs")
    (args.output / "model_catalog.json").write_bytes(encoded(catalog))
    (args.output / "model_catalog.cc").write_text(emit_cpp(catalog))
    (args.output / "SHA256SUMS").write_text("".join(
        f"{pack['archive_sha256']}  {pack['asset']}\n" for pack in catalog["packs"]
    ))
    print(json.dumps({
        "packs": len(packs),
        "archive_bytes": sum(pack["archive_bytes"] for pack in packs),
        "unpacked_bytes": sum(pack["unpacked_bytes"] for pack in packs),
        "catalog_sha256": digest(args.output / "model_catalog.json"),
        "output": str(args.output), "published": False,
    }))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        for path in (args.cache, args.output):
            if (
                not path.is_absolute() or ".." in path.parts or path.is_symlink() or
                path.resolve().is_relative_to(ROOT) or path.parent.is_symlink()
            ):
                raise ValueError("Use exact unlinked absolute paths outside the checkout")
        if not args.cache.is_dir() or args.output.exists():
            raise ValueError("Use the existing dedicated cache and a new output directory")
        args.output.mkdir(mode=0o700)
        descriptor = os.open(
            args.cache / ".model-packs.lock",
            os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600,
        )
        with os.fdopen(descriptor, "r+") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            prepare(args)
    except (OSError, ValueError, zipfile.BadZipFile) as error:
        print(f"Model data-pack preparation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

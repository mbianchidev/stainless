#!/usr/bin/env python3
"""Evaluate frozen native translation routes, not browser/worker acceptance."""

import argparse
import hashlib
import itertools
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import unicodedata

from local_services import MANIFEST, ROOT, digest, run_bounded, toolchain
from local_services import tree_digest, verify_file
from local_translation_prepare import conversion_recipe, verify_output


CORPUS = ROOT / "tests/fixtures/local_translation.json"
CORPUS_SHA256 = "97ec3acedd0e5ad948ad7f25e911f76ecca4492f14a5ff0f449b7bb17a496009"


def assess(text, concepts):
    words = set(re.findall(r"[^\W_]+", unicodedata.normalize("NFC", text).casefold()))
    matches = [
        [alternative for alternative in alternatives if alternative in words]
        for alternatives in concepts
    ]
    return {"matched_concepts": matches, "passed": all(matches)}


def configuration(build):
    cache = build / "CMakeCache.txt"
    values = dict(re.findall(
        r"^([A-Z0-9_]+):[^=]+=(.*)$", cache.read_text(), re.MULTILINE
    ))
    expected = {
        "WITH_MKL": "OFF", "WITH_DNNL": "OFF", "WITH_ACCELERATE": "OFF",
        "WITH_OPENBLAS": "OFF", "WITH_RUY": "ON", "WITH_CUDA": "OFF",
        "WITH_CUDNN": "OFF", "WITH_HIP": "OFF", "WITH_MPS": "OFF",
        "WITH_TENSOR_PARALLEL": "OFF", "WITH_FLASH_ATTN": "OFF",
        "CUDA_DYNAMIC_LOADING": "OFF", "OPENMP_RUNTIME": "NONE",
        "ENABLE_CPU_DISPATCH": "OFF", "BUILD_SHARED_LIBS": "OFF",
        "CMAKE_OSX_DEPLOYMENT_TARGET": "13.0",
        "CMAKE_OSX_ARCHITECTURES": "arm64",
    }
    for key, value in expected.items():
        if values.get(key) != value:
            raise ValueError(f"Translation runtime configuration differs: {key}")
    return {"cache_sha256": digest(cache), "verified_options": expected}


def evaluate(args):
    if digest(CORPUS) != CORPUS_SHA256:
        raise ValueError("The frozen translation oracle changed")
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
    build = args.cache / "build" / f"translation-{key}"
    metadata = json.loads((build / "stainless-inputs.json").read_text())
    executable = build / "cli/ct2-translator"
    if (
        metadata["manifest_sha256"] != digest(MANIFEST) or
        metadata["translation_cli"] != str(executable)
    ):
        raise ValueError("Translation build inputs differ; rerun pinned preparation")
    verify_file(executable, {"sha256": metadata["translation_cli_sha256"]})
    native_configuration = configuration(build)
    if tree_digest(Path(metadata["translation_sources"])) != metadata["translation_source_tree_sha256"]:
        raise ValueError("The composed native translation source changed")
    source = args.browser_source
    tokenizer = source / "out/Stainless/stainless_sentencepiece_probe"
    if not tokenizer.is_file() or tokenizer.is_symlink():
        raise ValueError("Build the exact Chromium GN tokenizer probe first")
    tokenizer_root = source / "third_party/sentencepiece"
    readme = (tokenizer_root / "README.chromium").read_text()
    if "Revision: b9e2e4f6ebc73900e061044b54f68a95090e23ff\n" not in readme:
        raise ValueError("Unreviewed native tokenizer revision")
    dependencies = subprocess.check_output(
        ["/usr/bin/otool", "-L", str(executable)], env=environment, text=True
    )
    for line in dependencies.splitlines()[1:]:
        name = line.strip().split(" (", 1)[0]
        if not name.startswith(("/usr/lib/", "/System/Library/")):
            raise ValueError(f"Build-host translation dependency: {name}")
    languages = tuple(corpus["sentences"])
    required = {f"{a}-{b}" for a in languages for b in languages
                if a != b and "en" in (a, b)}
    if set(manifest["translation_models"]) != required:
        raise ValueError("The five-language eight-model input set is incomplete")
    models = {}
    records = {}
    for pair, entry in manifest["translation_models"].items():
        recipe = conversion_recipe(entry)
        directory = args.cache / "converted" / f"opus-{pair}-ct2-int8-{recipe[:16]}"
        records[pair] = verify_output(directory, recipe)
        models[pair] = directory
    report = {
        "checked_at_unix": int(time.time()),
        "scope": corpus["scope"],
        "manifest_sha256": digest(MANIFEST),
        "corpus_sha256": digest(CORPUS),
        "runtime_sha256": digest(executable),
        "configuration": native_configuration,
        "macho_dependencies": dependencies,
        "tokenizer_binary_sha256": digest(tokenizer),
        "tokenizer_gn_sha256": digest(tokenizer_root / "BUILD.gn"),
        "tokenizer_source_sha256": tree_digest(tokenizer_root / "src/src"),
        "tokenizer_patches": {
            path.name: digest(path) for path in sorted((tokenizer_root / "patches").glob("*.patch"))
        },
        "models": records,
        "limits": corpus["limits"],
        "cases": [],
        "browser_worker_installed_verified": False,
    }
    with tempfile.TemporaryDirectory(prefix="translation-probe-", dir=args.cache) as temporary:
        temporary = Path(temporary)
        for origin, target in itertools.permutations(languages, 2):
            started = time.monotonic()
            observations = []

            def execute(command):
                limits = dict(corpus["limits"])
                limits["maximum_seconds_per_case"] -= time.monotonic() - started
                if limits["maximum_seconds_per_case"] <= 0:
                    raise TimeoutError("Translation route exceeded its deadline")
                seconds, rss, stdout, stderr = run_bounded(command, environment, limits)
                observations.append({
                    "seconds": seconds, "peak_rss_bytes": rss, "stderr": stderr,
                })
                return stdout

            def tokenize(model, operation, value):
                request = temporary / "request.json"
                request.write_text(json.dumps(value))
                stdout = execute([
                    str(tokenizer), f"--model={model}",
                    f"--operation={operation}", f"--input={request}",
                ])
                result = json.loads(stdout)
                name = "pieces" if operation == "encode" else "text"
                if not isinstance(result, dict) or set(result) != {name}:
                    raise ValueError("Unexpected native tokenizer response")
                value = result[name]
                if operation == "encode":
                    if not isinstance(value, list) or not all(
                        isinstance(piece, str) and piece and not any(c.isspace() for c in piece)
                        for piece in value
                    ):
                        raise ValueError("Native tokenizer returned invalid piece strings")
                elif not isinstance(value, str):
                    raise ValueError("Native tokenizer returned non-text output")
                return value

            text = corpus["sentences"][origin]
            route = [origin, target] if "en" in (origin, target) else [origin, "en", target]
            hops = []
            for left, right in zip(route, route[1:]):
                model = models[f"{left}-{right}"]
                pieces = tokenize(model / "source.spm", "encode", {"text": text})
                src = temporary / "source.txt"
                out = temporary / "target.txt"
                src.write_text(" ".join(pieces) + "\n")
                out.unlink(missing_ok=True)
                stdout = execute([
                    str(executable), "--model", str(model), "--device", "cpu",
                    "--compute_type", "int8_float32", "--inter_threads", "1",
                    "--intra_threads", str(corpus["limits"]["compute_threads"]),
                    "--batch_size", "1", "--read_batch_size", "1",
                    "--max_queued_batches", "1", "--max_input_length", "0",
                    "--beam_size", "6", "--max_decoding_length",
                    str(corpus["limits"]["maximum_generated_tokens"]),
                    "--src", str(src), "--out", str(out),
                ])
                if stdout or not out.is_file() or out.stat().st_size > 65536:
                    raise ValueError("Unexpected native translation output")
                lines = out.read_text().splitlines()
                if len(lines) != 1:
                    raise ValueError("Expected exactly one native translated hypothesis")
                pieces = lines[0].split()
                if len(pieces) >= corpus["limits"]["maximum_generated_tokens"]:
                    raise ValueError("Native decoder reached its generation boundary")
                text = tokenize(model / "target.spm", "decode", {"pieces": pieces})
                hops.append({"pair": f"{left}-{right}", "actual": text})
            report["cases"].append({
                "pair": f"{origin}-{target}", "route": route,
                "input": corpus["sentences"][origin], "actual": text,
                **assess(text, corpus["destination_concepts"][target]),
                "hops": hops, "seconds": time.monotonic() - started,
                "peak_rss_bytes": max(item["peak_rss_bytes"] for item in observations),
                "observations": observations,
            })
        try:
            run_bounded([
                str(executable), "--model", str(temporary / "absent-model"),
                "--device", "cpu", "--intra_threads", "2",
                "--src", str(temporary / "source.txt"),
            ], environment, corpus["limits"])
        except RuntimeError as error:
            if "model.bin" not in str(error):
                raise
            report["missing_model_rejected"] = True
        else:
            report["missing_model_rejected"] = False
    report["quality_passed"] = (
        len(report["cases"]) == 20 and all(case["passed"] for case in report["cases"])
        and report["missing_model_rejected"]
    )
    report["all_rss_observed"] = all(case["peak_rss_bytes"] > 0 for case in report["cases"])
    if args.output.exists() or args.output.is_symlink():
        raise ValueError("Preserve the existing translation report")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps({
        "quality_passed": report["quality_passed"],
        "all_rss_observed": report["all_rss_observed"],
        "cases": [{key: case[key] for key in ("pair", "passed", "actual")}
                  for case in report["cases"]],
        "report": str(args.output),
    }, ensure_ascii=False))
    return 0 if report["quality_passed"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chromium-cache", type=Path, required=True)
    parser.add_argument("--browser-source", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        return evaluate(args)
    except (OSError, ValueError, RuntimeError, TimeoutError, MemoryError,
            subprocess.SubprocessError) as error:
        print(f"Translation feasibility failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

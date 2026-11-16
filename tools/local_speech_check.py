#!/usr/bin/env python3
"""Run the frozen speech-model positive gate, not a browser/service acceptance test."""

import argparse
import audioop
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unicodedata
import wave

from local_services import MANIFEST, ROOT, digest, run_bounded, toolchain, verify_file
from local_services import fetch


CORPUS = ROOT / "tests/fixtures/local_speech.json"
NEURAL = ROOT / "tests/fixtures/local_speech_neural.json"


def words(text):
    normalized = unicodedata.normalize("NFKD", text).lower()
    normalized = "".join(c for c in normalized if not unicodedata.combining(c))
    return set(re.findall(r"[^\W_]+", normalized))


def synthesize(espeak, environment, case, config, destination):
    generated = destination.with_suffix(".source.wav")
    subprocess.run([
        str(espeak), "-v", case["language"], "-s", str(config["speed"]),
        "-p", str(config["pitch"]), "-a", str(config["amplitude"]),
        "-w", str(generated), "--", case["text"],
    ], check=True, env=environment, capture_output=True, timeout=30)
    with wave.open(str(generated), "rb") as source:
        if source.getnchannels() != 1 or source.getsampwidth() != 2:
            raise ValueError("Generator did not produce mono 16-bit PCM")
        rate = source.getframerate()
        samples = source.readframes(source.getnframes())
    if rate != 16000:
        samples, _ = audioop.ratecv(samples, 2, 1, rate, 16000, None)
    if not samples or len(samples) > 16000 * 2 * 30:
        raise ValueError("Synthetic utterance is empty or exceeds the PCM window")
    with wave.open(str(destination), "wb") as target:
        target.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
        target.writeframes(samples)
    return digest(destination)


def prepare_neural(cache):
    recipe = json.loads(NEURAL.read_text())
    if recipe["corpus_sha256"] != digest(CORPUS):
        raise ValueError("The approved neural fixture oracle changed")
    base = f"https://huggingface.co/rhasspy/piper-voices/resolve/{recipe['model_revision']}"
    for voice in recipe["voices"].values():
        directory = cache / "fixture-voices" / voice["name"]
        for kind, filename in (
            ("model", voice["name"] + ".onnx"),
            ("config", voice["name"] + ".onnx.json"),
            ("card", "MODEL_CARD"),
        ):
            entry = {**voice[kind], "url": f"{base}/{voice['directory']}/{filename}"}
            fetch(directory / filename, entry)
    return recipe


def synthesize_neural(espeak, environment, case, recipe, cache, destination):
    import numpy as np
    import onnxruntime as ort

    voice = recipe["voices"][case["language"]]
    directory = cache / "fixture-voices" / voice["name"]
    config = json.loads((directory / (voice["name"] + ".onnx.json")).read_text())
    phonemes = subprocess.check_output([
        str(espeak), "-q", "--ipa=3", "-v", config["espeak"]["voice"], "--", case["text"],
    ], env=environment, text=True, timeout=30)
    phonemes = re.sub(r"\s+", " ", phonemes.strip().replace("\u200d", ""))
    if case["text"][-1] in ".!?":
        phonemes += case["text"][-1]
    mapping = config["phoneme_id_map"]
    symbols = sorted((key for key in mapping if key not in {"_", "^", "$"}), key=len, reverse=True)
    ids = list(mapping["^"]) + list(mapping["_"])
    position = 0
    while position < len(phonemes):
        symbol = next((symbol for symbol in symbols if phonemes.startswith(symbol, position)), None)
        if symbol is None:
            raise ValueError(f"Unmapped fixture phoneme at {position}: {phonemes[position:position + 8]!r}")
        ids.extend(mapping[symbol])
        ids.extend(mapping["_"])
        position += len(symbol)
    ids.extend(mapping["$"])
    options = ort.SessionOptions()
    options.intra_op_num_threads = recipe["inference"]["compute_threads"]
    options.inter_op_num_threads = 1
    options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    ort.set_seed(recipe["inference"]["seed"])
    session = ort.InferenceSession(
        str(directory / (voice["name"] + ".onnx")), options, providers=["CPUExecutionProvider"]
    )
    inputs = {
        "input": np.array([ids], dtype=np.int64),
        "input_lengths": np.array([len(ids)], dtype=np.int64),
        "scales": np.array([
            config["inference"]["noise_scale"], config["inference"]["length_scale"],
            config["inference"]["noise_w"],
        ], dtype=np.float32),
    }
    if config["num_speakers"] > 1:
        inputs["sid"] = np.array([recipe["inference"]["speaker_id"]], dtype=np.int64)
    if {entry.name for entry in session.get_inputs()} != set(inputs):
        raise ValueError("Neural voice inputs do not match the reviewed Piper graph contract")
    audio = session.run(None, inputs)[0].reshape(-1)
    if not np.all(np.isfinite(audio)) or len(audio) > 30 * config["audio"]["sample_rate"]:
        raise ValueError("Neural voice returned invalid or overlong audio")
    pcm = (np.clip(audio, -1.0, 1.0) * 32767).astype("<i2").tobytes()
    rate = config["audio"]["sample_rate"]
    if rate != 16000:
        pcm, _ = audioop.ratecv(pcm, 2, 1, rate, 16000, None)
    if not pcm:
        raise ValueError("Neural fixture is empty")
    with wave.open(str(destination), "wb") as target:
        target.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
        target.writeframes(pcm)
    return digest(destination)


def evaluate(args):
    manifest = json.loads(MANIFEST.read_text())
    corpus = json.loads(CORPUS.read_text())
    environment, _, _ = toolchain(manifest, args.chromium_cache)
    cache = args.cache
    generator_revision = manifest["sources"]["espeak"]["revision"][:8]
    generator_build = cache / "build" / f"espeak-{generator_revision}"
    espeak = generator_build / "src/espeak-ng"
    whisper = cache / "build/speech/bin/whisper-cli"
    if not espeak.is_file() or not whisper.is_file():
        raise ValueError("Build the pinned feasibility tools with tools/local_services.py first")
    environment["ESPEAK_DATA_PATH"] = str(generator_build)
    neural = prepare_neural(cache) if args.generator == "neural" else None
    report = {
        "schema_version": 1,
        "boundary": "Pinned upstream CLI positive-model feasibility only; no native VAD, browser, sandbox, API or negative-audio acceptance claim.",
        "corpus_sha256": digest(CORPUS),
        "inputs_sha256": digest(MANIFEST),
        "generator_revision": manifest["sources"]["espeak"]["revision"],
        "generator_sha256": digest(espeak),
        "whisper_cli_sha256": digest(whisper),
        "fixture_generator": args.generator,
        "neural_recipe_sha256": digest(NEURAL) if neural else None,
        "harness_sha256": digest(Path(__file__)),
        "decoder": {"beam_size": 5, "best_of": 5, "temperature_fallback": True},
        "models": [],
    }
    fixtures = cache / "fixtures"
    fixtures.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="speech-", dir=fixtures) as directory:
        work = Path(directory)
        paths = {}
        for case in corpus["cases"]:
            path = work / f"{case['id']}.wav"
            if neural:
                audio_hash = synthesize_neural(espeak, environment, case, neural, cache, path)
            else:
                audio_hash = synthesize(espeak, environment, case, corpus["generator"], path)
            paths[case["id"]] = (path, audio_hash)
        for name in args.models:
            entry = manifest["models"][name]
            model = cache / "models" / f"{name}.bin"
            verify_file(model, entry)
            result = {"id": name, "sha256": entry["sha256"], "cases": []}
            for case in corpus["cases"]:
                audio, audio_hash = paths[case["id"]]
                output = work / f"{name}-{case['id']}"
                command = [
                    str(whisper), "-m", str(model), "-f", str(audio),
                    "-l", case["language"], "-t", "2", "-p", "1",
                    "-ng", "-bs", "5", "-bo", "5",
                    "-lpt", "-1.0", "-nth", "0.6",
                    "-ojf", "-of", str(output),
                ]
                seconds, rss, _, _ = run_bounded(command, environment, corpus["limits"])
                data = json.loads(output.with_suffix(".json").read_text())
                text = "".join(segment["text"] for segment in data["transcription"])
                actual = words(text)
                matched = [any(words(option) <= actual for option in group) for group in case["content_words"]]
                result["cases"].append({
                    "id": case["id"], "wav_sha256": audio_hash, "transcript": text,
                    "matched_content_words": sum(matched), "required": corpus["minimum_content_words"],
                    "passed": sum(matched) >= corpus["minimum_content_words"],
                    "seconds": seconds, "peak_rss_bytes": rss,
                })
            result["all_positive_cases_passed"] = all(case["passed"] for case in result["cases"])
            report["models"].append(result)
    report["any_model_passed_positive_gate"] = any(
        model["all_positive_cases_passed"] for model in report["models"]
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0 if report["any_model_passed_positive_gate"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--chromium-cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--generator", choices=["formant", "neural"], default="formant")
    parser.add_argument("--models", nargs="+",
                        choices=["whisper-tiny", "whisper-base", "whisper-small", "whisper-medium-q5", "whisper-turbo-q5"],
                        default=["whisper-tiny", "whisper-base"])
    args = parser.parse_args()
    try:
        return evaluate(args)
    except (OSError, ValueError, KeyError, ImportError, TimeoutError, MemoryError, RuntimeError,
            subprocess.SubprocessError) as error:
        print(f"Speech feasibility check failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

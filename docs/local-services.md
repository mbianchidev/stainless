# Local-service build inputs and feasibility

The preview.8 service replacements are **not available in the browser yet**.
The existing Google translation, speech/model-delivery and messaging restrictions
remain in effect. Preparing dependencies or passing a model probe does not restore
Web Speech, captions, translation, OCR, image descriptions or Web Push.
The work remains tracked in [#40](https://github.com/mbianchidev/stainless/issues/40)
and [#41](https://github.com/mbianchidev/stainless/issues/41).

## Reproducible speech-model probe

`chromium/services-inputs.json` is a separate, experimental build-input manifest.
It does not change the browser/product/schema pins or install anything into an
existing app. It locks native source archives, CMake, the Chromium compiler/Ninja,
depot_tools Python, Xcode/SDK and the explicitly reviewed model candidates.
This snapshot requires Xcode/SDK 27.2 and the existing pinned depot_tools Python
3.11.8. Those are probe prerequisites, not new installed-browser requirements.

Use an absolute, dedicated cache outside the checkout. The commands below run
only command-line tools and generate fictional audio without playing it:

```sh
chromium_cache="${STAINLESS_CACHE_DIR:-$HOME/Library/Caches/StainlessBuild}"
services_cache="$HOME/Library/Caches/StainlessServices"
depot_revision="$(plutil -extract depot_tools.revision raw -o - chromium/manifest.json)"
python="$chromium_cache/depot_tools/$depot_revision/python-bin/python3"

"$python" tools/local_services.py \
  --chromium-cache "$chromium_cache" --cache "$services_cache" \
  --models --build-speech

"$python" tools/local_speech_check.py \
  --chromium-cache "$chromium_cache" --cache "$services_cache" \
  --models whisper-tiny whisper-base --output target/speech-feasibility.json
```

Preparation verifies hashes before extraction, rejects changed or extra source
files instead of resetting them, and checks current exact-revision OSV matches
before compiling. Missing advisory evidence or a reported affected revision
blocks that build. An empty query is not exhaustive vulnerability proof:
native/tool/submodule and Python advisory coverage still needs reviewing before
an eventual browser candidate or publication.

The build disables GPU/Metal, OpenMP, Accelerate, remote model downloading,
MBROLA, host audio output and optional audio libraries. Native builds use two
jobs. The manifest's maintained eSpeak-NG commit replaces the older 1.52.0
release after advisory matches were found; it is a build-only fixture generator.
Neither it nor the upstream Whisper CLI is bundled into Stainless.

The probe checks the frozen fictional phrases in
`tests/fixtures/local_speech.json`, requiring at least four of five predefined
content words in **each** of the five languages. It uses two compute threads,
bounded process execution and sampled RSS. It writes input/tool/model/audio
hashes, actual transcripts, elapsed time and observed memory to its JSON report.
Generated WAVs and upstream CLI outputs are removed from its owned temporary
directory, including on failure. Exit status 1 means no evaluated candidate
passed, not permission to enable a feature or loosen its assertions.

The probe uses Python 3.11's deprecated `audioop` resampler, so that interpreter
version is intentional. It must not be run with an arbitrary newer Python that
has removed the module.

## Optional neural fixtures

The separate, reviewed `tests/fixtures/local_speech_neural.json` recipe uses
Piper ONNX voices with pinned model/config/license-card hashes. It keeps the
same fictional sentences and word thresholds. Voice-specific cards record
public-domain, CC0 and attribution-license provenance, including the applicable
Hi-Fi TTS base attribution. No source recordings, sample MP3s, generated audio
or voice models are redistributed in this repository or shipped in the app.

The generator uses the maintained native eSpeak phonemizer and a small,
hash-locked CPU ONNX environment. It does not install Piper's older bundled
native phonemizer. The Python wheels are build-only, currently require
Apple Silicon macOS 14+, and must remain outside the normal Python environment:

```sh
"$python" -m pip --isolated install \
  --index-url https://pypi.org/simple --require-hashes --only-binary=:all: \
  --no-compile --no-warn-script-location \
  --cache-dir "$services_cache/pip-cache" \
  --target "$services_cache/fixture-packages" \
  -r tools/local_services/fixture-requirements.txt

PYTHONPATH="$services_cache/fixture-packages" \
  "$python" tools/local_speech_check.py \
  --chromium-cache "$chromium_cache" --cache "$services_cache" \
  --generator neural --models whisper-small whisper-medium-q5 \
  --output target/speech-neural-feasibility.json
```

Use a new empty package target for the initial install; do not overwrite
unrecognized existing environments. Neural inference uses the verified
publisher's voice settings and a fixed seed. Unknown phonemes, mismatched graph
inputs, invalid output and oversized audio fail explicitly.

The latest bounded probe did **not** pass: even quantized medium recovered only
three of five required words in the Italian and German cases. English, French
and Spanish recovered five. Sampled model RSS remained below 2 GiB and each
case below 60 seconds; those resource observations do not fix the quality gap.
The original German ASCII transliteration also changes the generator's vowel
pronunciation, so fixture validity remains part of the unresolved investigation.
These results do not establish how the models perform on all other speech.

No candidate has been selected for a browser model catalog. Classic VAD,
negative-audio behavior, browser permission/API/caption integration, sandboxed
worker loading, lifecycle and actual installed behavior are separate unpassed
gates. The probe deliberately does not claim them.

## Tests and release boundaries

`./tools/stainless test` includes focused source-input integrity fixtures without
downloading models or running inference. Those tests protect corrupted-download
rejection, preservation of unknown source edits and archive containment. They
are not replacement-feature acceptance or real-server delivery evidence.

Optional production model downloads will require separate user consent and
reviewed release assets. No model assets have been published by this work.
Keep the [public-license gate](https://github.com/mbianchidev/stainless/issues/27),
[release evidence](phase-one.md#release-evidence), installed native workflows
and [separate-client update verification](update-verification.md) open.

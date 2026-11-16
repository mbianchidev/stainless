# Local-service build inputs and feasibility

The preview.8 local-model replacements are **not available in the browser yet**.
Experimental source adds opt-in native Web Push and
`stainless://settings/localServices`. Eight actual headless browser journeys
pass against a controlled TLS/WebSocket peer, including repeat subscriptions,
delivery/replay, native authorization, backpressure and Settings keyboard/error
flows. A separate actual patched-Autopush/Postgres/TLS journey also passes
independent VAPID/encrypted and payload-less delivery, retained-state server
restart and unsubscribe. The broader persistence/clone/denial matrix and
installed notification gates remain open.
Positive delivery uses explicit synthetic-profile notification grants:
Chromium refuses permission prompts headlessly, and that denial is tested
without weakening it. Physical prompt acceptance/presentation is not verified.
The existing Google translation, speech/model-delivery
and GCM/FCM restrictions remain in effect. Preparing dependencies or passing a
model/protocol probe does not restore Web Speech, captions, translation, OCR,
image descriptions or installed Push.
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
python_relative="$(plutil -extract tools.python.path raw -o - chromium/services-inputs.json)"
python="$chromium_cache/depot_tools/$depot_revision/$python_relative"

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
bounded process execution, a live owned-PID memory watchdog and kernel peak RSS.
It writes input/tool/model/audio
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
A separately approved correction changed only German `traegt/gruene` to
`trägt/grüne`; all word aliases, the Italian sentence, voices, model and limits
stayed unchanged. Its bounded rerun still recovered only three words in Italian
and German, with the other languages at five. The new kernel-RSS report peaked
at 1,167,376,384 bytes and 10.558 seconds; original failed/sampled reports remain
separate. Correct spelling did not solve the quality gap. These results do not
establish how the models perform on all other speech.
The subsequent approved quantized v3 Turbo trial recovered five words in
English, Italian, French and Spanish, but German remained at three. Its gate
also **failed**, with peak kernel RSS 828,293,120 bytes and maximum case time
19.650 seconds. Four passing languages are not a five-language pass; no further
model/voice/corpus escalation or browser catalog selection follows automatically.

No candidate has been selected for a browser model catalog. Classic VAD,
negative-audio behavior, browser permission/API/caption integration, sandboxed
worker loading, lifecycle and actual installed behavior are separate unpassed
gates. The probe deliberately does not claim them.

## OCR feasibility

The separate OCR inputs pin official Tesseract 5.5.4, Leptonica 1.87.0 and
immutable Apache-2.0 `tessdata_fast` models for English, Italian, French, German
and Spanish. The five model files total 11,764,636 bytes. Their pinned publisher
README requires Tesseract's LSTM engine; no legacy or orientation-detection
model is selected.

```sh
"$python" tools/local_services.py \
  --chromium-cache "$chromium_cache" --cache "$services_cache" \
  --models --build-ocr

"$python" tools/local_ocr_check.py \
  --chromium-cache "$chromium_cache" --cache "$services_cache" \
  --output target/ocr-feasibility.json
```

The static CPU tool disables image codecs, curl, archives, training, graphics,
OpenMP, host-specific optimization and ambient package discovery. It consumes
generated PGM pixels, not arbitrary URLs. The verified executable links only
the system C/C++ runtime. A single compute thread stays within the approved
two-thread budget; builds use two jobs. These are standalone build tools, not
libraries already bundled into the browser.

`tests/fixtures/local_ocr.json` freezes the fictional five-language sentences,
blank-image case, geometry and independent acceptance rules before OCR output.
The Swift generator draws only into an offscreen bitmap. It requires the named
font and never opens a window. The probe records input-raster/tool/model hashes,
the actual font name, recognized text, actual confidence, word boxes, time and memory.
Every nonempty word must have a finite confidence and a positive in-bounds box;
all Unicode words, including accents, must match. A missing pack must fail.

The six upright-raster cases and missing-pack rejection passed on the build Mac.
The unchanged corpus also passed after replacing an inadequate fast-process RSS
sampler with `wait4` kernel peak measurement plus the live watchdog: peak RSS
was **21,512,192 bytes**, with a maximum case time of **0.172 seconds**.
The earlier incomplete sampling report remains separate evidence. Historical
speech reports still describe their original sampled measurements.

This is not PDF text insertion, selectable/searchable scanned PDFs, an actual
accessibility tree, a sandboxed worker, browser packaging, older-macOS support
or installed evidence. Those integrations and gates remain outstanding.

## Native translation feasibility

The translation inputs pin CTranslate2 4.9.0, its exact CPU/CLI source dependencies
and eight original OPUS-MT Marian NPZ models. The two unavailable 2020 Spanish
archives were replaced, with explicit approval, by the publisher's
`opus-2021-02-19` English/Spanish pair. The other six model choices are unchanged.
Every archive, selected data member and original CC-BY-4.0 license is hash-locked.
Relevant Hugging Face conversion-card declarations are recorded separately;
they do not replace the original pretrained-weight license. The Spanish
replacement references its actual original publisher recipe, not the old
Hugging Face weights.

The build-only converter uses hash-locked CTranslate2, NumPy and PyYAML wheels.
It loads native NPZ with NumPy's default pickle prohibition and uses YAML's safe
loader. It does not install PyTorch/Transformers, execute publisher scripts or
load remote model code. Use a new empty package target for the initial install:

```sh
"$python" -m pip --isolated install \
  --index-url https://pypi.org/simple --require-hashes --only-binary=:all: \
  --no-compile --no-warn-script-location \
  --cache-dir "$services_cache/pip-cache" \
  --target "$services_cache/translation-packages" \
  -r tools/local_services/translation-requirements.txt

(
  set -e
  for pair in en-it it-en en-fr fr-en en-de de-en en-es es-en; do
    PYTHONNOUSERSITE=1 PYTHONPATH="$services_cache/translation-packages" \
      "$python" tools/local_translation_prepare.py \
      --chromium-cache "$chromium_cache" --cache "$services_cache" --pair "$pair"
  done
)

"$python" tools/local_services.py \
  --chromium-cache "$chromium_cache" --cache "$services_cache" --build-translation
```

Conversion excludes publisher logs/scripts and retains original notices,
tokenizers, exact input hashes and deterministic recipe/output records.
Volatile timing, memory and diagnostics stay outside the model provenance.
Existing altered inputs or converted files fail instead of being overwritten.
Source composition also preserves and rejects edited nested dependencies.

After following the [Chromium source/build runbook](build-macos.md) for the
current patch stack, build its **test-only** `stainless_sentencepiece_probe`
target with the pinned `depot_tools/autoninja`, not raw Ninja. This links the
normal `//third_party/sentencepiece` target: upstream revision
`b9e2e4f6ebc73900e061044b54f68a95090e23ff` plus seven Chromium patches.
Do not install or substitute a system/Python SentencePiece build.
With `browser_source` pointing to that prepared Chromium checkout:

```sh
"$python" tools/local_translation_check.py \
  --chromium-cache "$chromium_cache" --browser-source "$browser_source" \
  --cache "$services_cache" --output target/translation-feasibility.json
```

The native engine disables GPU/MPS, OpenMP, Accelerate, MKL, DNNL, OpenBLAS,
tensor parallelism and native CPU dispatch. The observed Mach-O depends only
on system C/C++ runtimes and declares macOS 13; that is not an older-OS runtime
result. Pinned CMake requires the documented external policy floor of 3.5 for
old nested cpuinfo/clog build files. Their two upstream CMake deprecation
warnings remain visible, not suppressed.

The frozen synthetic oracle in `tests/fixtures/local_translation.json` passed
all **20 directed routes**, including sequential English pivots. Each result
contained all five predefined concepts; maximum route time was **0.427 seconds**
and kernel peak RSS **195,264,512 bytes**, under two compute threads and one job.
Missing-model rejection, five Unicode tokenizer roundtrips and malformed
tokenizer-request rejection also passed. Some carry/hold verbs became
wear/contain: this bounded concept check is not full semantic or grammar
validation, and language quality remains experimental.

These are standalone command-line results, not the native C ABI, sandboxed
worker/model broker, document-bound page translation/restore, download consent,
prepared release assets or installed browser support. Those gates remain open.

## Image-description feasibility

The approved llama.cpp 1.0.0 annotated tag peels to
`b1b770d90d94cad9e9ea5b993bdbc383a522fb28`. The tag's actual object, source
archive SHA-256 and model/projector hashes are pinned separately; the release
page's `targetCommitish` is not substituted for the tag's commit.
The standalone CPU tool disables GPU backends, OpenMP, Accelerate/BLAS,
OpenSSL, subprocess/video support, optional downloaded dependencies,
host GGML and dynamic backend loading. Its Mach-O links only system C/C++
runtimes. This is not the eventual sandboxed handle-loading bridge.
The source-archive CLI build reports unavailable Git metadata, an unused
FetchContent guard, SDK `char_traits<unsigned char>` deprecations and the
prebuilt compiler's missing `libLTO.dylib` linker warning. These diagnostics
were retained, not suppressed; exact source/binary hashes are recorded
independently. A warning-clean bundled runtime/older-OS result is not claimed.

`tests/fixtures/local_vision.json` freezes three original 512-pixel RGB scenes
before inference: a red circle, blue square and blank white image. The inputs
were visually reviewed before model output. Generated descriptions are parsed
only after the upstream CLI's explicit test-response marker, not from metadata,
the prompt or diagnostics.

The original approved 256M Q8 model described both colored shapes correctly but
invented black text on the blank image. Its gate therefore **failed**; maximum
kernel RSS was 401,293,312 bytes and maximum case time 2.505 seconds.
The failed report remains separate evidence. A subsequent, explicitly approved
bounded round evaluates quantized Whisper v3 Turbo and SmolVLM 500M with their
exact source/card/weight hashes and the **same frozen corpora, assertions and
resource limits**. Neither trial is automatically selected for a browser
catalog or silently substituted at runtime.
The 500M trial also failed the blank-image case, inventing a black label on a
white box; its two colored-scene cases passed. Kernel peak RSS was
705,773,568 bytes and maximum case time 3.473 seconds. Increasing that model
within this approved round did not resolve the quality gap.

The standalone vision probe, after pinned preparation, is:

```sh
"$python" tools/local_services.py \
  --chromium-cache "$chromium_cache" --cache "$services_cache" \
  --models --build-vision

"$python" tools/local_vision_check.py \
  --chromium-cache "$chromium_cache" --cache "$services_cache" \
  --candidate 256m --output target/vision-feasibility.json
```

Use `--candidate 500m` only for the separately approved trial and a distinct
report path. No image descriptions, OCR, translation, speech or captions have
been enabled in the browser by these probes.

## Tests and release boundaries

`./tools/stainless test` includes focused source-input integrity fixtures without
downloading models or running inference. Those tests protect corrupted-download
rejection, preservation of unknown source edits, archive containment,
owned-process deadlines/kernel memory limits, malformed OCR output, exact
translation concepts and converted-model integrity. They
also exercise the self-hosted fixture's owned-resource and failure-safety helpers
without starting containers. They are not replacement-feature acceptance or
real-server delivery evidence.

The separate [self-hosted Web Push fixture](web-push-self-hosting.md) pins
Autopush 1.86.0 plus reviewed security/integrity patches, Postgres and a
loopback-only TLS proxy. Its real protocol, independent sender, restart,
key-rotation and reset observations do not prove browser permission,
service-worker dispatch, native notification presentation or installed behavior.
Postgres support remains enterprise-beta. Follow the guide's isolated ownership,
audit and cleanup requirements; no default hosted provider or account is chosen.

Optional production model downloads will require separate user consent and
reviewed release assets. No model assets have been published by this work.
Keep the [public-license gate](https://github.com/mbianchidev/stainless/issues/27),
[release evidence](phase-one.md#release-evidence), installed native workflows
and [separate-client update verification](update-verification.md) open.

# Local-service runtimes and model status

The preview.8 local-service integration is **experimental and not released**.
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

## Native runtime integration

Source now contains fixed translation/OCR/speech/vision C ABI libraries, a browser-only Mojo
worker in the unchanged generic utility sandbox, and a document-bound browser
broker. Native page/selection translation and safe original-text restoration are
now connected, with eight actual headless native browser journeys passing on
clean source `2ea91bf`.
Web Speech, caption, PDF and AX adapters remain unfinished.
The source has a compiled fifteen-pack catalog, root-bound storage and explicit
anonymous model setup in Local Services Settings. The model assets have not
been published; a missing release asset reports an error, not a successful setup.
The bounded speech and composed image-description probes below now pass;
that does not establish their browser feature adapters.

### Native translation controls

After enabling **Local translation** in
`stainless://settings/localServices`, the native **View** and browser app menus
provide **Translate Page Locally**, **Translate Selection Locally** and
**Restore Original Text**. The dialog confirms source and target languages,
shows actual progress/errors, and offers Stop and Restore original.
Its setup button opens Settings without enabling a feature or downloading data.
The last confirmed target language is profile-local. Unknown, conflicting or
mixed source detection requires a choice; supported author language hints and
the bundled CLD3 detector never trigger a model download.

All twenty directed routes use the eight reviewed English-direction packs.
A non-English pair needs both explicit English-pivot packs; missing packs fail
visibly. The adapter processes supported HTTP/HTTPS/file HTML documents and
their ordinary active frames, not opaque/fenced/guest documents or shadow trees.
Form values, passwords, editable content, scripts, styles and inherited
`translate="no"` text are excluded. Results replace text only, not HTML.
Changed or moved nodes are preserved, including during restoration. Closing the
dialog stops pending work; originals for applied text remain in that document
until restore or document destruction, never in persistent storage.

The aggregate page/selection input limit is 100,000 Unicode scalar values across
participating frames, before chunking. Larger input fails without translation.
The five-minute operation deadline and sixty-second broker requests retain
partial/restore status. Additional output guards reject a node above 1 MiB or
an operation above 4 MiB rather than truncate it.
Native fixtures include Unicode/frame boundaries, excluded text, source choice,
mode/consent denial, navigation and actual model-backed translation/restore.
The model-backed cases require the prepared reviewed packs through the
test-only `--stainless-model-pack-fixture` argument; no production model path
override is added. On clean source `2ea91bf`, all eight adapter journeys passed:
exact 100,000/100,001 cross-origin Unicode boundaries, form/editable/translate-no
exclusion, mixed-source choice and missing-pack failure, default-off/private
denial, navigation, real page translation with menu-based restore after closing,
HTML-like real model output remaining text, and selection/pivot translation with
in-flight author edits preserved. These are native browser results, not physical
screen-reader, published-pack or installed-browser evidence.

The first native run exposed synchronous first-use cache canonicalization on
the browser UI thread. Resolution, inspection and model acquisition now run on
the blocking storage sequence without a production blocking allowance. The
follow-up passed all eight adapter cases and the existing actual anonymous
download/offline-acquisition journey; it also exposed a Settings keyboard race
while location verification was pending. Feature-consent controls are now
independent of that storage check; the follow-up regression run remains pending.

### Sandboxed worker evidence

The libraries consume read-only regular file descriptors. Translation, speech
and OCR hash the exact copied bytes before parsing. Large GGUF inputs use a
bounded streaming SHA-256 pass, a borrowed-FILE parser, no mmap/lazy loading,
and nanosecond identity/change checks before and after parsing, avoiding a
second model-sized snapshot. The projector's four-file source-compatible
FILE-reader patch and composed source tree are independently hash-recorded;
legacy filename entry points remain available for compatibility checks.
No C++ object, allocator or exception crosses the
Chromium/system-libc++ boundary. Only four C functions per library are exported.
Translation uses the pinned CPU backend and at most two compute threads; OCR
has OpenMP and its optional file/network/image-codec dependencies disabled.
Speech uses the fixed CPU-only Whisper backend with two compute threads,
classic WebRTC VAD and the frozen no-speech/confidence thresholds. Its native
library build, four exports, system-only dependencies and actual disabled
GPU/network configuration have passed. On clean source `ddf2401`, all five
frozen positive language cases, all four non-speech cases and wrong-model-hash
denial passed through the actual broker/utility. Translation/OCR regressions
and all three shared native image-composition cases passed in that same run.
This is not Web Speech, microphone permission, caption or installed evidence.
The fixed vision library builds with two CPU threads, no GPU/network/dynamic
backend/RPC/subprocess/video support and four C exports. On clean source
`08216ae`, all three unchanged scenes passed through actual verified-handle
utility inference as both RGBA and encoded PNG, preserving raw evidence,
at most 256 generated tokens and the real measured-color composition.
Wrong-projector-hash denial and all existing local-runtime cases also passed:
12 native cases, 176 seconds total; the six-image VLM case took 40.829 seconds.
The preserved legacy filename API separately passed the same three composed
scenes against the identical patched native source. Its raw blank-color gate
still fails, as recorded separately. This is not browser AX integration,
an RSS-cutoff stress result or installed evidence.
Additional OCR model-file lookup is rejected. Encoded images are decoded only
in the utility worker, with dimensions checked before bitmap allocation.
The OCR result bridge carries native word and text-line bounds/confidences,
not inferred confidence or a semantic-layout claim.

The broker admits one job globally, queues at most 16 descriptors, and requests
model handles/pixels only after admission. Requests carry a weak document
identity, are cancelled on navigation, destruction or feature disable, and
include queue time in their 60-second deadline. A separate blocking sequence
targets 200 ms RSS samples for the 250 ms maximum, requires an initial valid measurement before
inference, and terminates its creation-time-verified process above 2 GiB.
Workers are not reused across jobs. Source/build presence is not yet a passed
real-browser watchdog, sandbox, model-loading or output-quality result.
The broker now retains admitted model leases and blocks the next admission
until that exact service instance reports its actual exit, including
cancellation during process launch. An unconfirmed shutdown blocks further
inference rather than releasing leases or starting overlapping work.
Model handles and leases are destroyed on a blocking I/O sequence, not the
browser UI. Next admission waits for that cleanup acknowledgement after the
exact worker exit; stale/rejected prepared inputs use the same ownership path.
Result measurements are replaced with browser-owned RSS/sample timing data;
the worker cannot attest its own resource usage.
The actual 13-case `3dac2e6` run also passed lease retention through worker
exit and cancellation during launch. Its nine speech jobs peaked at
1,135,673,344 sampled bytes with a 205,101 microsecond maximum sample gap;
six vision jobs peaked at 1,283,571,712 bytes with a 204,941 microsecond gap.
These are observed positive workloads, not a passed over-limit termination test.

The normal source build now prepares, checks and bundles these fixed libraries
and their native/embedded dependency notices without downloading model weights.
It validates native CMake options, exact input hashes, OSV commits and published
upstream advisory ranges, records their provenance in `stainless-build.json`,
and verifies dependencies/signatures before browser checks. The fixed native
tool inputs currently require Xcode/SDK 27.2; this is a build-host requirement,
not evidence of an older installed macOS runtime.

To build only the native libraries, without fetching model candidates:

```sh
"$python" tools/local_services.py \
  --chromium-cache "$chromium_cache" --cache "$services_cache" \
  --build-runtimes --runtime-report "$services_cache/native-runtimes.json"
```

Define `python`, `chromium_cache` and `services_cache` as in the next section.
`tools/local_runtime_fixture.py` defaults to the five reviewed OCR packs
and six frozen offscreen rasters for the manual native worker case. It does
not fetch unrelated speech/vision candidates. The case requires `--run-manual`,
`--gtest_filter=StainlessLocalWorkerBrowserTest.MANUAL_FrozenOcrThroughActualBoundedUtility`
and `--stainless-native-ocr-fixture=/exact/owned/fixture` on the native fixture
target, after finalizing both frameworks as described in the build guide.
The separate manual native translation case takes
`--stainless-native-translation-fixture=/exact/verified/converted-model`,
uses Chromium's exact SentencePiece target and read-only verified model handles,
and retains the frozen English-to-Italian concept oracle.
The helper's explicit `--kind speech` prepares only the reviewed Turbo model,
the five approved neural utterances and frozen silence/noise/tone inputs.
Its native case uses
`--gtest_filter=StainlessLocalWorkerBrowserTest.MANUAL_FrozenSpeechAndNonSpeechThroughActualBoundedUtility`
and `--stainless-native-speech-fixture=/exact/owned/fixture`.
These manual fixtures are source-only preparation, not model-import interfaces.
Disposable restored/generated language inputs, outputs, environments and build
leaves must be removed after their final consumers. Earlier trial artifacts
were deleted at the user's request; the results below remain historical
recorded outcomes, not a claim that those model bytes or raw reports remain.

## Model data and cache groundwork

After preparing all eight selected translation directions with the conversion
procedure below, the data-only builder creates the fifteen reviewed ZIPs,
their exact member inventories, checksums and compiled catalog input:

```sh
"$python" tools/local_model_packs.py \
  --cache "$services_cache" --output /exact/new/owned/model-pack-output
```

The output must be a new absolute directory outside the checkout. This prepares
model data only; it neither packages a browser installer nor uploads a release.
ZIP order, timestamps, permissions and storage method are normalized. The
prepared catalog contains 2,291,944,032 archive bytes and 2,291,931,092 unpacked
bytes, including model/runtime/original/publisher notices. Catalog hashes and
member sizes are source-controlled; prepared weights/archives remain disposable
after their final authorized consumers.

The internal storage backend resolves the real macOS caches directory, or an
absolute `--disk-cache-dir`, then uses a canonical user-data-root binding.
Path computation and absent-cache inspection create nothing. Explicit setup
must separately pass regular-profile/consent checks before directory creation.
Private-profile overlap, relative overrides, links and foreign markers are
rejected. Read-only model leases block removal until worker exit. Installed,
reserved, archive and staging bytes share a 4 GiB budget; no pack is evicted
automatically. Old versions and interrupted reservations stay accounted for
and require explicit owned removal.
The source now adds independent feature consent and named-pack download/removal
dialogs at `stainless://settings/localServices`. Setup names exact bytes,
licenses, fixed GitHub/CDN destinations and exposed network metadata before
confirmation. Downloading is anonymous, streams under the same reservation,
omits cookies/referrers/authentication, bypasses the HTTP cache and refuses
unreviewed redirects. Cancellation waits for the owned writer/parser to stop
before cleanup; a pack committed before cancellation remains installed and is
reported explicitly for user-directed removal.
Production Settings lint/type checks and native handler compilation pass.
On clean source `2280de5`, three actual native Settings journeys passed:
keyboard feature/pack consent and Escape/focus return, no-cache/no-network
denial before consent, OTR provider denial, explicit missing-asset and foreign
redirect failures, and the actual compiled OCR ZIP through a controlled
canonical-GitHub response followed by offline verified acquisition.
The intercepted request omits credentials/auth/cookies/referrer and disables
the HTTP cache. This is native source/controlled-transport evidence, not proof
that public model assets exist, physical screen-reader verification or an
installed feature. Allowed-CDN redirect, broader cancellation/restart/clone,
mode and backup/restore/browser-adapter gates remain separate. Data setup
does not make the unfinished browser actions available.

## Reproducible speech-model probe

`chromium/services-inputs.json` is a separate, experimental build-input manifest.
It does not change the browser/product/schema pins or install anything into an
existing app. It locks native source archives, CMake, the Chromium compiler/Ninja,
depot_tools Python, Xcode/SDK and the explicitly reviewed model candidates.
This snapshot requires Xcode/SDK 27.2 and the existing pinned depot_tools Python
3.11.8. Those are native build/probe prerequisites, not new installed-browser requirements.

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

Native advisory checks do not compare llama.cpp's `bNNNN`/Git labels as
SemVer against its `1.0.0` release name. They query the official fixed-reference
ancestry for the exact pinned commit and record full resolved fix SHAs.
Build-host GitHub metadata uses an already authenticated `gh` CLI when
available, otherwise anonymous HTTPS. Exhausted/unavailable evidence still
blocks the build; it never falls back to stale results. This authorization is
for public build-time advisory reads only, not installed model downloads,
browser traffic or private-updater credentials.
The published RPC-only `GHSA-j8rj-fmpv-wcxw` has no stated fixed version:
its reviewed scope is hash-bound, and the build must independently verify
`GGML_RPC`, dynamic backend loading, server and connection support are all
actually disabled in CMake before compilation. This is an uncompiled-feature
disposition, not a claim that upstream RPC is safe or a vulnerability waiver.
Missing/changed scope, unknown ranges, failed ancestry or unavailable evidence
still block the build.

The bundled vision inventory includes complete embedded JSON, miniaudio,
stb-image and conditionally excluded subprocess notices. Miniaudio's exact
vendored bytes match its official immutable commit. The vendored JSON/stb
headers differ from the bounded official references checked: their exact bytes
remain locked by the llama parent commit, not a falsely claimed standalone Git
revision. Fresh official advisory ranges are still required for both, and that
standalone OSV coverage gap remains explicit rather than an empty success.

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

Source comparison with the frozen official Piper reference found a fixture
implementation mismatch: its default synthesis peak-normalizes the float
waveform before PCM conversion, but this helper only clipped/scaled it. The
helper now retains that reference normalization and rejects empty/non-finite
or effectively silent positive utterances. Voices, model candidates, scales,
seed, corpus, aliases and thresholds are unchanged. This fix does not establish
why German previously failed or make that quality gate pass on its own.

The corrected same-candidate Turbo run also failed: English/Italian/French/
Spanish each recovered five words, while German remained at three. Maximum
kernel RSS was 827,441,152 bytes and case time 17.607 seconds. The fixture
correction was real, but did not solve that quality failure.

The user then explicitly authorized **one bounded replacement per failing
speech/vision family**, with unchanged languages, corpora/prompts, acceptance
thresholds, two threads, 2 GiB and per-case deadlines. The locked trial inputs
are Whisper large-v3 Q5_0 and Qwen3.5-0.8B Q8_0 with its matching F16 projector.
Qwen runs through the already pinned llama.cpp implementation; no Python model
code is executed. Its original/publisher cards and Apache-2.0 notice are fixed
by hash, including the publisher's modification provenance and the limitation
that a card reference is not exact source-tensor/independent rights clearance.
Both are trials, not selected browser catalog models or passing quality results.

The bounded large-v3 replacement also recovered only three German content words
(the other languages remained five), peaking at 2,070,216,704 bytes and 21.807
seconds. Qwen's colored scenes passed; it correctly called the white image
completely blank but omitted the frozen required color word. Its raw gate failed,
with peak RSS 1,269,104,640 bytes and maximum time 4.440 seconds. These outcomes
are not being reclassified as passes.

The user subsequently approved changing **only the German synthetic voice**
and adding a general measured-uniform-image-color property to the final
description. The replacement voice is the publisher's CC0-documented
Thorsten-emotional medium **neutral speaker 4**, inheriting the reviewed
Thorsten-medium/CC0-rdh base. The other four voices, German sentence/required
words, ASR candidates, prompts and quality/resource limits remain unchanged.
Native measured-color composition must retain the raw model evidence separately;
it is not a weakened oracle or a claim that the raw model named that color.
The shared native final-description helper checks every bounded decoded RGBA
pixel, composites alpha over white, and adds a separately labeled measured color
only for an exactly uniform raster. One differing visible pixel prevents that
label; no scene-specific detector, average-color guess or fabricated object is
added. The normal-GN probe exercises that same production helper. A composed
probe result is distinct from full VLM worker/AX/installed integration.

The approved neutral-voice rerun of the same Turbo model recovered all five
content words in every language, with peak kernel RSS **827,424,768 bytes**
and maximum case time **18.496 seconds**. The other four voice definitions
and generated WAV hashes remained unchanged.
The approved Qwen plus shared native measured-color composition passed all
three frozen scenes, with peak RSS **1,265,418,240 bytes** and maximum complete
case time **4.535 seconds**. The raw-model blank-color gate remains failed:
the measured property, not the model, supplies the separately labeled white.

The user approved selecting **Whisper Turbo Q5** and the **Qwen3.5-0.8B
measured-color pipeline** for the forthcoming model catalog. The exact choices,
speech/VAD thresholds and accurate asset names are recorded in
`chromium/services-inputs.json`; the image asset is named for Qwen, not SmolVLM.
Catalog construction, the VLM handle bridge, browser permission/API/caption/AX
integration, lifecycle and installed behavior still have their own gates.
Approval of the model set does not classify unfinished integration as available.

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
two-thread budget; builds use two jobs. These CLI results are separate from the new C ABI libraries and do not verify
their browser integration.

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

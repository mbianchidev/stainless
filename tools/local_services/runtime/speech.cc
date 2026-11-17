#include <algorithm>
#include <cmath>
#include <cstring>
#include <memory>
#include <new>
#include <stdexcept>
#include <string>
#include <vector>

#include "runtime_input.h"
#include "stainless_local_runtime.h"
#include "whisper.h"

struct StainlessSpeech {
  std::unique_ptr<whisper_context, decltype(&whisper_free)> context{
      nullptr, whisper_free};
  uint32_t threads = 2;
};

uint32_t stainless_speech_abi_version() noexcept { return 1; }

StainlessSpeech* stainless_speech_create(
    const StainlessModelFile* model, uint32_t compute_threads,
    StainlessRuntimeError* error) noexcept {
  using namespace stainless_runtime;
  ClearError(error);
  if (!model || compute_threads < 1 || compute_threads > 2) {
    Error(error, STAINLESS_RUNTIME_INVALID_INPUT,
          "Invalid local speech model or thread boundary");
    return nullptr;
  }
  try {
    auto bytes =
        ReadModel(model->descriptor, model->size, 1610612736, model->sha256);
    auto params = whisper_context_default_params();
    params.use_gpu = false;
    params.flash_attn = false;
    params.dtw_token_timestamps = false;
    auto engine = std::make_unique<StainlessSpeech>();
    engine->threads = compute_threads;
    engine->context.reset(whisper_init_from_buffer_with_params(
        bytes.data(), bytes.size(), params));
    if (!engine->context) {
      Error(error, STAINLESS_RUNTIME_INVALID_MODEL,
            "Verified local speech model could not be initialized");
      return nullptr;
    }
    return engine.release();
  } catch (const std::bad_alloc&) {
    Error(error, STAINLESS_RUNTIME_RESOURCE_LIMIT,
          "Local speech model allocation failed");
  } catch (const std::exception&) {
    Error(error, STAINLESS_RUNTIME_INVALID_MODEL,
          "Local speech model could not be loaded from a verified handle");
  }
  return nullptr;
}

int32_t stainless_speech_run(StainlessSpeech* engine, const int16_t* pcm,
                             size_t sample_count, StainlessText language,
                             StainlessEmitToken emit, void* context,
                             StainlessSpeechMetadata* metadata,
                             StainlessRuntimeError* error) noexcept {
  using namespace stainless_runtime;
  ClearError(error);
  if (metadata) {
    *metadata = {1, 0, 0, 0};
  }
  if (!engine || !pcm || !sample_count || sample_count > 480000 ||
      !BoundedText(language, 2) || language.size != 2 || !emit || !metadata) {
    return Error(error, STAINLESS_RUNTIME_INVALID_INPUT,
                 "Invalid bounded 16 kHz mono speech input");
  }
  try {
    const std::string name(language.data, language.size);
    if (name != "en" && name != "it" && name != "fr" && name != "de" &&
        name != "es") {
      return Error(error, STAINLESS_RUNTIME_INVALID_INPUT,
                   "Unsupported local speech language");
    }
    std::vector<float> samples;
    samples.reserve(sample_count);
    for (size_t index = 0; index < sample_count; ++index) {
      samples.push_back(static_cast<float>(pcm[index]) / 32768.0f);
    }
    auto params = whisper_full_default_params(WHISPER_SAMPLING_BEAM_SEARCH);
    params.n_threads = static_cast<int>(engine->threads);
    params.no_context = true;
    params.no_timestamps = true;
    params.translate = false;
    params.language = name.c_str();
    params.detect_language = false;
    params.print_special = false;
    params.print_progress = false;
    params.print_realtime = false;
    params.print_timestamps = false;
    params.debug_mode = false;
    params.greedy.best_of = 5;
    params.beam_search.beam_size = 5;
    params.logprob_thold = -1.0f;
    params.no_speech_thold = 0.6f;
    params.vad = false;
    if (whisper_full(engine->context.get(), params, samples.data(),
                     static_cast<int>(samples.size())) != 0) {
      return Error(error, STAINLESS_RUNTIME_FAILED,
                   "Local speech decoding failed");
    }
    std::string transcript;
    double log_probability_sum = 0;
    double maximum_no_speech = 0;
    size_t tokens = 0;
    const int segments = whisper_full_n_segments(engine->context.get());
    if (segments < 0 || segments > 128) {
      return Error(error, STAINLESS_RUNTIME_RESOURCE_LIMIT,
                   "Local speech returned too many segments");
    }
    for (int segment = 0; segment < segments; ++segment) {
      const auto probability = whisper_full_get_segment_no_speech_prob(
          engine->context.get(), segment);
      if (!std::isfinite(probability) || probability < 0 || probability > 1) {
        return Error(error, STAINLESS_RUNTIME_FAILED,
                     "Local speech returned invalid probability data");
      }
      maximum_no_speech = std::max<double>(maximum_no_speech, probability);
      const int count = whisper_full_n_tokens(engine->context.get(), segment);
      if (count < 0 || count > 4096) {
        return Error(error, STAINLESS_RUNTIME_RESOURCE_LIMIT,
                     "Local speech token boundary exceeded");
      }
      for (int index = 0; index < count; ++index) {
        const auto token =
            whisper_full_get_token_data(engine->context.get(), segment, index);
        if (token.id >= whisper_token_eot(engine->context.get())) {
          continue;
        }
        if (!std::isfinite(token.plog)) {
          return Error(error, STAINLESS_RUNTIME_FAILED,
                       "Local speech returned invalid confidence data");
        }
        log_probability_sum += token.plog;
        ++tokens;
      }
      const char* text =
          whisper_full_get_segment_text(engine->context.get(), segment);
      if (!text || std::strlen(text) > 32768 - transcript.size()) {
        return Error(error, STAINLESS_RUNTIME_RESOURCE_LIMIT,
                     "Local speech transcript boundary exceeded");
      }
      transcript += text;
    }
    if (!tokens || transcript.empty()) {
      return STAINLESS_RUNTIME_OK;
    }
    metadata->has_metrics = 1;
    metadata->average_log_probability = log_probability_sum / tokens;
    metadata->no_speech_probability = maximum_no_speech;
    if (metadata->average_log_probability < -1.0 || maximum_no_speech >= 0.6) {
      return STAINLESS_RUNTIME_OK;
    }
    metadata->no_speech = 0;
    if (!emit(context, {transcript.data(), transcript.size()})) {
      return Error(error, STAINLESS_RUNTIME_OUTPUT_REJECTED,
                   "Local speech output was cancelled or rejected");
    }
    return STAINLESS_RUNTIME_OK;
  } catch (const std::bad_alloc&) {
    return Error(error, STAINLESS_RUNTIME_RESOURCE_LIMIT,
                 "Local speech request allocation failed");
  } catch (const std::exception&) {
    return Error(error, STAINLESS_RUNTIME_FAILED,
                 "Local speech inference failed");
  }
}

void stainless_speech_destroy(StainlessSpeech* engine) noexcept {
  delete engine;
}

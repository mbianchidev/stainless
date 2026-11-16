#include <memory>
#include <new>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

#include "ctranslate2/models/model.h"
#include "ctranslate2/models/model_reader.h"
#include "ctranslate2/translator.h"
#include "runtime_input.h"
#include "stainless_local_runtime.h"

struct StainlessTranslator {
  std::unique_ptr<ctranslate2::Translator> translator;
};

uint32_t stainless_translation_abi_version() noexcept { return 1; }

StainlessTranslator* stainless_translation_create(
    const StainlessModelFile* files, size_t count, uint32_t compute_threads,
    StainlessRuntimeError* error) noexcept {
  using namespace stainless_runtime;
  ClearError(error);
  if (!files || count < 3 || count > 4 || compute_threads < 1 ||
      compute_threads > 2) {
    Error(error, STAINLESS_RUNTIME_INVALID_INPUT,
          "Invalid translation model bundle or thread limit");
    return nullptr;
  }
  try {
    ctranslate2::models::ModelMemoryReader reader("stainless-verified-model");
    const std::set<std::string> allowed = {"config.json",
                                           "model.bin",
                                           "shared_vocabulary.json",
                                           "vocabulary.json",
                                           "source_vocabulary.json",
                                           "target_vocabulary.json"};
    std::set<std::string> members;
    for (size_t index = 0; index < count; ++index) {
      const auto& file = files[index];
      if (!BoundedText(file.name, 64)) {
        throw std::invalid_argument("Invalid model name");
      }
      const std::string name(file.name.data, file.name.size);
      if (allowed.count(name) != 1 || !members.insert(name).second) {
        throw std::invalid_argument("Unexpected model member");
      }
      reader.register_file(
          name,
          ReadModel(file.descriptor, file.size,
                    name == "model.bin" ? 536870912 : 16777216, file.sha256));
    }
    if (!members.count("config.json") || !members.count("model.bin") ||
        !((count == 3 && (members.count("shared_vocabulary.json") ||
                          members.count("vocabulary.json"))) ||
          (count == 4 && members.count("source_vocabulary.json") &&
           members.count("target_vocabulary.json")))) {
      throw std::invalid_argument("Incomplete model vocabulary");
    }
    auto model = ctranslate2::models::Model::load(
        reader, ctranslate2::Device::CPU, 0,
        ctranslate2::ComputeType::INT8_FLOAT32, false, false);
    ctranslate2::ReplicaPoolConfig config;
    config.num_threads_per_replica = compute_threads;
    config.max_queued_batches = 1;
    auto engine = std::make_unique<StainlessTranslator>();
    engine->translator =
        std::make_unique<ctranslate2::Translator>(model, config);
    if (engine->translator->num_replicas() != 1) {
      throw std::runtime_error("Unexpected model replicas");
    }
    return engine.release();
  } catch (const std::bad_alloc&) {
    Error(error, STAINLESS_RUNTIME_RESOURCE_LIMIT,
          "Translation model allocation failed");
  } catch (const std::exception&) {
    Error(error, STAINLESS_RUNTIME_INVALID_MODEL,
          "Translation model could not be loaded from verified handles");
  }
  return nullptr;
}

int32_t stainless_translation_run(StainlessTranslator* engine,
                                  const StainlessText* source, size_t count,
                                  uint32_t maximum_tokens,
                                  StainlessEmitToken emit, void* context,
                                  StainlessRuntimeError* error) noexcept {
  using namespace stainless_runtime;
  ClearError(error);
  if (!engine || !source || !count || count > 1024 || !maximum_tokens ||
      maximum_tokens > 128 || !emit) {
    return Error(error, STAINLESS_RUNTIME_INVALID_INPUT,
                 "Invalid bounded translation request");
  }
  try {
    std::vector<std::string> tokens;
    size_t bytes = 0;
    for (size_t index = 0; index < count; ++index) {
      if (!BoundedText(source[index], 8192) ||
          source[index].size > 8192 - bytes) {
        return Error(error, STAINLESS_RUNTIME_INVALID_INPUT,
                     "Translation input exceeds its byte boundary");
      }
      bytes += source[index].size;
      tokens.emplace_back(source[index].data, source[index].size);
    }
    ctranslate2::TranslationOptions options;
    options.beam_size = 6;
    options.max_input_length = 0;
    options.max_decoding_length = maximum_tokens;
    options.num_hypotheses = 1;
    const auto results = engine->translator->translate_batch({tokens}, options);
    if (results.size() != 1 || results.front().hypotheses.size() != 1 ||
        results.front().hypotheses.front().size() >= maximum_tokens) {
      return Error(error, STAINLESS_RUNTIME_RESOURCE_LIMIT,
                   "Translation did not complete within its token boundary");
    }
    bytes = 0;
    for (const auto& token : results.front().hypotheses.front()) {
      if (token.size() > 65536 - bytes) {
        return Error(error, STAINLESS_RUNTIME_RESOURCE_LIMIT,
                     "Translation output exceeds its byte boundary");
      }
      bytes += token.size();
      if (!emit(context, {token.data(), token.size()})) {
        return Error(error, STAINLESS_RUNTIME_OUTPUT_REJECTED,
                     "Translation output was cancelled or rejected");
      }
    }
    engine->translator->clear_cache();
    return STAINLESS_RUNTIME_OK;
  } catch (const std::bad_alloc&) {
    return Error(error, STAINLESS_RUNTIME_RESOURCE_LIMIT,
                 "Translation request allocation failed");
  } catch (const std::exception&) {
    return Error(error, STAINLESS_RUNTIME_FAILED,
                 "Local translation inference failed");
  }
}

void stainless_translation_destroy(StainlessTranslator* engine) noexcept {
  delete engine;
}

#ifndef STAINLESS_LOCAL_RUNTIME_H_
#define STAINLESS_LOCAL_RUNTIME_H_

#include <stddef.h>
#include <stdint.h>

#if defined(__cplusplus)
#define STAINLESS_NOEXCEPT noexcept
extern "C" {
#else
#define STAINLESS_NOEXCEPT
#endif

#define STAINLESS_RUNTIME_EXPORT __attribute__((visibility("default")))

typedef enum StainlessRuntimeStatus {
  STAINLESS_RUNTIME_OK = 0,
  STAINLESS_RUNTIME_INVALID_INPUT = 1,
  STAINLESS_RUNTIME_INVALID_MODEL = 2,
  STAINLESS_RUNTIME_FAILED = 3,
  STAINLESS_RUNTIME_RESOURCE_LIMIT = 4,
  STAINLESS_RUNTIME_OUTPUT_REJECTED = 5
} StainlessRuntimeStatus;

typedef struct StainlessRuntimeError {
  int32_t code;
  char message[192];
} StainlessRuntimeError;

typedef struct StainlessText {
  const char* data;
  size_t size;
} StainlessText;

typedef struct StainlessOcrWord {
  StainlessText text;
  double confidence;
  int32_t left;
  int32_t top;
  int32_t width;
  int32_t height;
  uint32_t line_index;
  int32_t line_left;
  int32_t line_top;
  int32_t line_width;
  int32_t line_height;
  double line_confidence;
} StainlessOcrWord;

typedef struct StainlessModelFile {
  StainlessText name;
  int32_t descriptor;
  uint64_t size;
  uint8_t sha256[32];
} StainlessModelFile;

// Handles belong to their creating library. Output slices are borrowed only
// during the callback; the receiver must copy anything it retains.
typedef struct StainlessTranslator StainlessTranslator;
typedef struct StainlessOcr StainlessOcr;
typedef struct StainlessSpeech StainlessSpeech;

typedef struct StainlessSpeechMetadata {
  uint32_t no_speech;
  uint32_t has_metrics;
  double average_log_probability;
  double no_speech_probability;
} StainlessSpeechMetadata;

typedef int32_t (*StainlessEmitToken)(void* context, StainlessText token);
typedef int32_t (*StainlessEmitWord)(void* context,
                                     const StainlessOcrWord* word);

STAINLESS_RUNTIME_EXPORT uint32_t stainless_translation_abi_version(void)
    STAINLESS_NOEXCEPT;
STAINLESS_RUNTIME_EXPORT StainlessTranslator* stainless_translation_create(
    const StainlessModelFile* files, size_t count, uint32_t compute_threads,
    StainlessRuntimeError* error) STAINLESS_NOEXCEPT;
STAINLESS_RUNTIME_EXPORT int32_t stainless_translation_run(
    StainlessTranslator* engine, const StainlessText* source, size_t count,
    uint32_t maximum_tokens, StainlessEmitToken emit, void* context,
    StainlessRuntimeError* error) STAINLESS_NOEXCEPT;
STAINLESS_RUNTIME_EXPORT void stainless_translation_destroy(
    StainlessTranslator* engine) STAINLESS_NOEXCEPT;

STAINLESS_RUNTIME_EXPORT uint32_t stainless_ocr_abi_version(void)
    STAINLESS_NOEXCEPT;
STAINLESS_RUNTIME_EXPORT StainlessOcr* stainless_ocr_create(
    int32_t model_descriptor, uint64_t model_size, const uint8_t* model_sha256,
    StainlessText language, StainlessRuntimeError* error) STAINLESS_NOEXCEPT;
STAINLESS_RUNTIME_EXPORT int32_t stainless_ocr_run(
    StainlessOcr* engine, const uint8_t* pixels, size_t pixel_bytes,
    uint32_t width, uint32_t height, uint32_t stride, StainlessEmitWord emit,
    void* context, StainlessRuntimeError* error) STAINLESS_NOEXCEPT;
STAINLESS_RUNTIME_EXPORT void stainless_ocr_destroy(StainlessOcr* engine)
    STAINLESS_NOEXCEPT;

STAINLESS_RUNTIME_EXPORT uint32_t stainless_speech_abi_version(void)
    STAINLESS_NOEXCEPT;
STAINLESS_RUNTIME_EXPORT StainlessSpeech* stainless_speech_create(
    const StainlessModelFile* model, uint32_t compute_threads,
    StainlessRuntimeError* error) STAINLESS_NOEXCEPT;
STAINLESS_RUNTIME_EXPORT int32_t stainless_speech_run(
    StainlessSpeech* engine, const int16_t* pcm, size_t sample_count,
    StainlessText language, StainlessEmitToken emit, void* context,
    StainlessSpeechMetadata* metadata,
    StainlessRuntimeError* error) STAINLESS_NOEXCEPT;
STAINLESS_RUNTIME_EXPORT void stainless_speech_destroy(StainlessSpeech* engine)
    STAINLESS_NOEXCEPT;

#if defined(__cplusplus)
}
#endif

#endif

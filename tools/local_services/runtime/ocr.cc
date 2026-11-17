#include <cmath>
#include <cstring>
#include <memory>
#include <new>
#include <stdexcept>
#include <string>
#include <vector>

#include "runtime_input.h"
#include "stainless_local_runtime.h"
#include "tesseract/baseapi.h"
#include "tesseract/resultiterator.h"

struct StainlessOcr {
  std::string model;
  tesseract::TessBaseAPI api;
};

namespace {

bool RejectAdditionalModelFile(const char*, std::vector<char>*) {
  return false;
}

}  // namespace

uint32_t stainless_ocr_abi_version() noexcept { return 2; }

StainlessOcr* stainless_ocr_create(int32_t model_descriptor,
                                   uint64_t model_size,
                                   const uint8_t* model_sha256,
                                   StainlessText language,
                                   StainlessRuntimeError* error) noexcept {
  using namespace stainless_runtime;
  ClearError(error);
  if (!BoundedText(language, 3) || language.size != 3) {
    Error(error, STAINLESS_RUNTIME_INVALID_INPUT, "Invalid local OCR language");
    return nullptr;
  }
  try {
    const std::string name(language.data, language.size);
    if (name != "eng" && name != "ita" && name != "fra" && name != "deu" &&
        name != "spa") {
      Error(error, STAINLESS_RUNTIME_INVALID_INPUT,
            "Unsupported local OCR language");
      return nullptr;
    }
    auto engine = std::make_unique<StainlessOcr>();
    engine->model =
        ReadModel(model_descriptor, model_size, 67108864, model_sha256);
    if (engine->api.Init(engine->model.data(),
                         static_cast<int>(engine->model.size()), name.c_str(),
                         tesseract::OEM_LSTM_ONLY, nullptr, 0, nullptr, nullptr,
                         false, RejectAdditionalModelFile) != 0) {
      throw std::runtime_error("Model initialization failed");
    }
    engine->api.SetPageSegMode(tesseract::PSM_SINGLE_BLOCK);
    return engine.release();
  } catch (const std::bad_alloc&) {
    Error(error, STAINLESS_RUNTIME_RESOURCE_LIMIT,
          "OCR model allocation failed");
  } catch (const std::exception&) {
    Error(error, STAINLESS_RUNTIME_INVALID_MODEL,
          "OCR model could not be loaded from a verified handle");
  }
  return nullptr;
}

int32_t stainless_ocr_run(StainlessOcr* engine, const uint8_t* pixels,
                          size_t pixel_bytes, uint32_t width, uint32_t height,
                          uint32_t stride, StainlessEmitWord emit,
                          void* context,
                          StainlessRuntimeError* error) noexcept {
  using namespace stainless_runtime;
  ClearError(error);
  if (!engine || !pixels || !emit || !width || width > 4096 || !height ||
      height > 4096 || static_cast<uint64_t>(width) * height > 16777216 ||
      stride != width * 4 ||
      pixel_bytes != static_cast<uint64_t>(stride) * height) {
    return Error(error, STAINLESS_RUNTIME_INVALID_INPUT,
                 "Invalid bounded RGBA OCR image");
  }
  try {
    // The worker provides unpremultiplied RGBA. Composite transparent pixels
    // over white without granting the runtime an encoded-image/path parser.
    std::string rgb(static_cast<size_t>(width) * height * 3, '\0');
    for (size_t index = 0; index < static_cast<size_t>(width) * height;
         ++index) {
      const auto alpha = pixels[index * 4 + 3];
      for (size_t channel = 0; channel < 3; ++channel) {
        rgb[index * 3 + channel] = static_cast<char>(
            (pixels[index * 4 + channel] * alpha + 255 * (255 - alpha) + 127) /
            255);
      }
    }
    engine->api.SetImage(reinterpret_cast<const unsigned char*>(rgb.data()),
                         static_cast<int>(width), static_cast<int>(height), 3,
                         static_cast<int>(width * 3));
    if (engine->api.Recognize(nullptr) != 0) {
      engine->api.Clear();
      return Error(error, STAINLESS_RUNTIME_FAILED,
                   "Local OCR recognition failed");
    }
    std::unique_ptr<tesseract::ResultIterator> iterator(
        engine->api.GetIterator());
    size_t output_bytes = 0;
    uint32_t line_index = 0;
    if (iterator) {
      do {
        std::unique_ptr<char[]> text(
            iterator->GetUTF8Text(tesseract::RIL_WORD));
        if (!text || !text[0]) {
          continue;
        }
        const auto size = std::strlen(text.get());
        const double confidence = iterator->Confidence(tesseract::RIL_WORD);
        const double line_confidence =
            iterator->Confidence(tesseract::RIL_TEXTLINE);
        if (iterator->IsAtBeginningOf(tesseract::RIL_TEXTLINE)) {
          ++line_index;
        }
        int left, top, right, bottom;
        int line_left, line_top, line_right, line_bottom;
        if (size > 1048576 - output_bytes || !std::isfinite(confidence) ||
            confidence < 0 || confidence > 100 ||
            !iterator->BoundingBox(tesseract::RIL_WORD, &left, &top, &right,
                                   &bottom) ||
            left < 0 || top < 0 || right <= left || bottom <= top ||
            static_cast<uint32_t>(right) > width ||
            static_cast<uint32_t>(bottom) > height ||
            !std::isfinite(line_confidence) || line_confidence < 0 ||
            line_confidence > 100 ||
            !iterator->BoundingBox(tesseract::RIL_TEXTLINE, &line_left,
                                   &line_top, &line_right, &line_bottom) ||
            line_left < 0 || line_top < 0 || line_right <= line_left ||
            line_bottom <= line_top ||
            static_cast<uint32_t>(line_right) > width ||
            static_cast<uint32_t>(line_bottom) > height) {
          iterator.reset();
          engine->api.Clear();
          return Error(error, STAINLESS_RUNTIME_FAILED,
                       "Local OCR returned invalid bounded word or line data");
        }
        output_bytes += size;
        const StainlessOcrWord word = {{text.get(), size},
                                       confidence,
                                       left,
                                       top,
                                       right - left,
                                       bottom - top,
                                       line_index,
                                       line_left,
                                       line_top,
                                       line_right - line_left,
                                       line_bottom - line_top,
                                       line_confidence};
        if (!emit(context, &word)) {
          iterator.reset();
          engine->api.Clear();
          return Error(error, STAINLESS_RUNTIME_OUTPUT_REJECTED,
                       "OCR output was cancelled or rejected");
        }
      } while (iterator->Next(tesseract::RIL_WORD));
    }
    iterator.reset();
    engine->api.Clear();
    return STAINLESS_RUNTIME_OK;
  } catch (const std::bad_alloc&) {
    engine->api.Clear();
    return Error(error, STAINLESS_RUNTIME_RESOURCE_LIMIT,
                 "OCR image allocation failed");
  } catch (const std::exception&) {
    engine->api.Clear();
    return Error(error, STAINLESS_RUNTIME_FAILED, "Local OCR inference failed");
  }
}

void stainless_ocr_destroy(StainlessOcr* engine) noexcept { delete engine; }

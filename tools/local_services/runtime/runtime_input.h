#ifndef STAINLESS_RUNTIME_INPUT_H_
#define STAINLESS_RUNTIME_INPUT_H_

#include <CommonCrypto/CommonDigest.h>
#include <errno.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

#include <algorithm>
#include <array>
#include <cstdio>
#include <cstring>
#include <limits>
#include <memory>
#include <stdexcept>
#include <string>

#include "stainless_local_runtime.h"

namespace stainless_runtime {

inline int32_t Error(StainlessRuntimeError* error, StainlessRuntimeStatus code,
                     const char* message) {
  if (error) {
    error->code = code;
    std::memset(error->message, 0, sizeof(error->message));
    const auto length =
        std::min(std::strlen(message), sizeof(error->message) - 1);
    std::memcpy(error->message, message, length);
  }
  return code;
}

inline void ClearError(StainlessRuntimeError* error) {
  Error(error, STAINLESS_RUNTIME_OK, "");
}

inline struct stat ModelIdentity(int descriptor, uint64_t size,
                                 uint64_t maximum_size,
                                 const uint8_t* expected_sha256) {
  struct stat before;
  const int flags = fcntl(descriptor, F_GETFL);
  if (flags == -1 || (flags & O_ACCMODE) != O_RDONLY ||
      fstat(descriptor, &before) != 0 || !S_ISREG(before.st_mode) ||
      before.st_nlink != 1 || before.st_size < 0 ||
      static_cast<uint64_t>(before.st_size) != size || !size ||
      size > maximum_size || !expected_sha256 ||
      size > std::numeric_limits<CC_LONG>::max() ||
      size > std::numeric_limits<size_t>::max()) {
    throw std::invalid_argument("Invalid read-only model handle");
  }
  return before;
}

inline void CheckModelIdentity(int descriptor, const struct stat& before) {
  struct stat after;
  if (fstat(descriptor, &after) != 0 || before.st_dev != after.st_dev ||
      before.st_ino != after.st_ino || before.st_size != after.st_size ||
      before.st_mtime != after.st_mtime) {
    throw std::invalid_argument("Changed model handle");
  }
#if defined(__APPLE__)
  if (before.st_mtimespec.tv_nsec != after.st_mtimespec.tv_nsec ||
      before.st_ctimespec.tv_sec != after.st_ctimespec.tv_sec ||
      before.st_ctimespec.tv_nsec != after.st_ctimespec.tv_nsec) {
    throw std::invalid_argument("Changed model handle");
  }
#else
  if (before.st_mtim.tv_nsec != after.st_mtim.tv_nsec ||
      before.st_ctim.tv_sec != after.st_ctim.tv_sec ||
      before.st_ctim.tv_nsec != after.st_ctim.tv_nsec) {
    throw std::invalid_argument("Changed model handle");
  }
#endif
}

inline void CheckModelDigest(const uint8_t* actual_sha256,
                             const uint8_t* expected_sha256) {
  uint8_t difference = 0;
  for (size_t index = 0; index < CC_SHA256_DIGEST_LENGTH; ++index) {
    difference |= actual_sha256[index] ^ expected_sha256[index];
  }
  if (difference) {
    throw std::invalid_argument("Model digest does not match verified catalog");
  }
}

inline std::string ReadModel(int descriptor, uint64_t size,
                             uint64_t maximum_size,
                             const uint8_t* expected_sha256) {
  const auto before =
      ModelIdentity(descriptor, size, maximum_size, expected_sha256);
  std::string bytes(static_cast<size_t>(size), '\0');
  size_t position = 0;
  while (position < bytes.size()) {
    const auto read = pread(descriptor, bytes.data() + position,
                            std::min<size_t>(bytes.size() - position, 1048576),
                            static_cast<off_t>(position));
    if (read < 0 && errno == EINTR) {
      continue;
    }
    if (read <= 0) {
      throw std::invalid_argument("Incomplete model handle");
    }
    position += static_cast<size_t>(read);
  }
  CheckModelIdentity(descriptor, before);
  uint8_t actual_sha256[CC_SHA256_DIGEST_LENGTH];
  CC_SHA256(bytes.data(), static_cast<CC_LONG>(bytes.size()), actual_sha256);
  CheckModelDigest(actual_sha256, expected_sha256);
  return bytes;
}

// Streaming parsers use an owned duplicate, never a path or a second
// model-sized snapshot. Recheck identity after parsing, with mmap disabled in
// the consumer.
class VerifiedModelFile {
 public:
  VerifiedModelFile(int descriptor, uint64_t size, uint64_t maximum_size,
                    const uint8_t* expected_sha256)
      : descriptor_(descriptor),
        identity_(
            ModelIdentity(descriptor, size, maximum_size, expected_sha256)) {
    CC_SHA256_CTX hash;
    if (CC_SHA256_Init(&hash) != 1) {
      throw std::runtime_error("Model digest initialization failed");
    }
    std::array<uint8_t, 65536> bytes;
    uint64_t position = 0;
    while (position < size) {
      const auto count =
          pread(descriptor, bytes.data(),
                std::min<uint64_t>(size - position, bytes.size()),
                static_cast<off_t>(position));
      if (count < 0 && errno == EINTR) {
        continue;
      }
      if (count <= 0) {
        throw std::invalid_argument("Incomplete streaming model handle");
      }
      if (CC_SHA256_Update(&hash, bytes.data(), static_cast<CC_LONG>(count)) !=
          1) {
        throw std::runtime_error("Model digest update failed");
      }
      position += static_cast<uint64_t>(count);
    }
    CheckUnchanged();
    uint8_t actual_sha256[CC_SHA256_DIGEST_LENGTH];
    if (CC_SHA256_Final(actual_sha256, &hash) != 1) {
      throw std::runtime_error("Model digest finalization failed");
    }
    CheckModelDigest(actual_sha256, expected_sha256);
    const int copy = fcntl(descriptor, F_DUPFD_CLOEXEC, 0);
    if (copy < 0) {
      throw std::runtime_error("Read-only model handle duplication failed");
    }
    FILE* stream = fdopen(copy, "rb");
    if (!stream) {
      close(copy);
      throw std::runtime_error("Read-only model stream initialization failed");
    }
    stream_.reset(stream);
    if (std::fseek(stream_.get(), 0, SEEK_SET) != 0) {
      throw std::runtime_error("Read-only model stream rewind failed");
    }
  }

  FILE* get() const { return stream_.get(); }
  void CheckUnchanged() const { CheckModelIdentity(descriptor_, identity_); }

 private:
  int descriptor_;
  struct stat identity_;
  std::unique_ptr<FILE, decltype(&std::fclose)> stream_{nullptr, std::fclose};
};

inline bool BoundedText(StainlessText text, size_t maximum) {
  return text.data && text.size && text.size <= maximum &&
         std::memchr(text.data, '\0', text.size) == nullptr;
}

}  // namespace stainless_runtime

#endif

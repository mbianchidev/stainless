#ifndef STAINLESS_RUNTIME_INPUT_H_
#define STAINLESS_RUNTIME_INPUT_H_

#include <CommonCrypto/CommonDigest.h>
#include <errno.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

#include <algorithm>
#include <cstring>
#include <limits>
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

inline std::string ReadModel(int descriptor, uint64_t size,
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
  unsigned char actual_sha256[CC_SHA256_DIGEST_LENGTH];
  CC_SHA256(bytes.data(), static_cast<CC_LONG>(bytes.size()), actual_sha256);
  uint8_t difference = 0;
  for (size_t index = 0; index < CC_SHA256_DIGEST_LENGTH; ++index) {
    difference |= actual_sha256[index] ^ expected_sha256[index];
  }
  if (difference) {
    throw std::invalid_argument("Model digest does not match verified catalog");
  }
  return bytes;
}

inline bool BoundedText(StainlessText text, size_t maximum) {
  return text.data && text.size && text.size <= maximum &&
         std::memchr(text.data, '\0', text.size) == nullptr;
}

}  // namespace stainless_runtime

#endif

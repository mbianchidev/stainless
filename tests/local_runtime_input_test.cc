#include <fcntl.h>
#include <unistd.h>

#include <iostream>
#include <stdexcept>
#include <string>

#include "../tools/local_services/runtime/runtime_input.h"

namespace {

unsigned char expected_sha256[CC_SHA256_DIGEST_LENGTH];

bool Rejected(int descriptor, uint64_t size, uint64_t maximum) {
  try {
    stainless_runtime::ReadModel(descriptor, size, maximum, expected_sha256);
    return false;
  } catch (const std::invalid_argument&) {
    return true;
  }
}

bool Check(bool condition, const char* message) {
  if (!condition) {
    std::cerr << "Native model-handle test failed: " << message << '\n';
  }
  return condition;
}

}  // namespace

int main(int argc, char** argv) {
  if (argc != 2) {
    return 2;
  }
  CC_SHA256("synthetic", 9, expected_sha256);
  const std::string path = std::string(argv[1]) + "/synthetic-model";
  const std::string link = path + "-linked";
  const int writable = open(path.c_str(), O_CREAT | O_EXCL | O_RDWR, 0600);
  if (!Check(writable >= 0, "create owned fixture")) {
    return 1;
  }
  if (!Check(write(writable, "synthetic", 9) == 9, "write fixture")) {
    close(writable);
    return 1;
  }
  const int readable = open(path.c_str(), O_RDONLY);
  bool passed = Check(readable >= 0, "open read-only fixture");
  if (readable >= 0) {
    passed &= Check(stainless_runtime::ReadModel(
                        readable, 9, 9, expected_sha256) == "synthetic",
                    "read exact approved bytes");
    expected_sha256[0] ^= 1;
    passed &= Check(Rejected(readable, 9, 9), "reject changed model bytes");
    expected_sha256[0] ^= 1;
    passed &= Check(Rejected(readable, 8, 9), "reject forged size");
    passed &= Check(Rejected(readable, 9, 8), "reject oversize handle");
    passed &= Check(Rejected(writable, 9, 9), "reject writable model");
    passed &= Check(Rejected(-1, 9, 9), "reject invalid descriptor");
    passed &= Check(::link(path.c_str(), link.c_str()) == 0,
                    "create owned hard-link fixture");
    passed &= Check(Rejected(readable, 9, 9), "reject hard-linked model");
    passed &= Check(unlink(link.c_str()) == 0, "remove owned link");
    passed &=
        Check(fcntl(readable, F_GETFD) != -1, "borrowed descriptor stays open");
    close(readable);
  }
  close(writable);
  passed &= Check(unlink(path.c_str()) == 0, "remove owned fixture");
  return passed ? 0 : 1;
}

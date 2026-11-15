#!/bin/bash
set -euo pipefail

if [[ "$(/usr/bin/uname -s)" != Darwin || "$#" -eq 0 ]]; then
  echo "Usage on macOS: strip_rust.sh MACH_O_BINARY [...]" >&2
  exit 2
fi

temporary=""
temporary_binary=""
cleanup() {
  result=$?
  trap - EXIT
  if [[ -n "$temporary" ]]; then
    /bin/rm -f "$temporary_binary" || { [[ "$result" -ne 0 ]] || result=1; }
    /bin/rmdir "$temporary" || { [[ "$result" -ne 0 ]] || result=1; }
  fi
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

for binary in "$@"; do
  if [[ ! -f "$binary" || -L "$binary" ]]; then
    echo "Refusing to strip a missing, non-regular or symlinked binary: $binary" >&2
    exit 1
  fi
  case "$(/usr/bin/file -b "$binary")" in
    Mach-O*) ;;
    *)
      echo "Refusing to strip a non-Mach-O input: $binary" >&2
      exit 1
      ;;
  esac
  temporary=$(/usr/bin/mktemp -d "${binary}.native-strip.XXXXXX")
  temporary_binary="$temporary/${binary##*/}"
  /bin/cp -p "$binary" "$temporary_binary"
  /usr/bin/codesign --remove-signature "$temporary_binary"
  /usr/bin/xcrun strip -S -x "$temporary_binary"
  /usr/bin/codesign --force --sign - --timestamp=none "$temporary_binary"
  /usr/bin/codesign --verify --strict "$temporary_binary"
  /usr/bin/touch -r "$binary" "$temporary_binary"
  /bin/mv -f "$temporary_binary" "$binary"
  /bin/rmdir "$temporary"
  temporary=""
  temporary_binary=""
  echo "Native Apple strip and ad-hoc signature verified: $binary"
done

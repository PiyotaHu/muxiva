#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build=${TMPDIR:-/tmp}/voxa-stage8-build
mkdir -p "$build"
cxx_bin=${CXX:-c++}
cxx_system=
if [ "$(uname -s)" = Darwin ]; then
  sdk=$(xcrun --show-sdk-path)
  cxx_system="-isystem $sdk/usr/include/c++/v1"
fi
cargo build --offline -p voxa-ffi --manifest-path "$repo/Cargo.toml"
"$cxx_bin" -std=c++17 -Wall -Wextra -Wpedantic -Werror -pthread \
  -fsanitize=address,undefined -fno-omit-frame-pointer \
  $cxx_system -I"$repo/cpp/include" -I"$repo/cpp/adapters/include" \
  "$repo/cpp/adapters/src/in_memory_mock_rtc.cc" "$repo/cpp/adapters/src/mock_rtc_adapter.cc" \
  "$repo/cpp/adapters/tests/mock_rtc_adapter_test.cc" -L"$repo/target/debug" -lvoxa_ffi \
  -Wl,-rpath,"$repo/target/debug" -o "$build/mock_rtc_adapter_asan"
"$build/mock_rtc_adapter_asan"

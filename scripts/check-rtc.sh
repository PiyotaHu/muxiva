#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build=${TMPDIR:-/tmp}/muxiva-stage8-build
mkdir -p "$build"
cc_bin=${CC:-cc}
cxx_bin=${CXX:-c++}
cxx_system=
if [ "$(uname -s)" = Darwin ]; then
  sdk=$(xcrun --show-sdk-path)
  cxx_system="-isystem $sdk/usr/include/c++/v1"
fi
cargo build --offline -p muxiva-ffi --manifest-path "$repo/Cargo.toml"
"$cc_bin" -std=c11 -Wall -Wextra -Werror -I"$repo/cpp/include" -I"$repo/cpp/adapters/include" \
  "$repo/cpp/adapters/tests/abi_smoke.c" -o "$build/rtc_abi_smoke"
"$cxx_bin" -std=c++17 -Wall -Wextra -Wpedantic -Werror -pthread $cxx_system \
  -I"$repo/cpp/include" -I"$repo/cpp/adapters/include" \
  "$repo/cpp/adapters/src/in_memory_mock_rtc.cc" "$repo/cpp/adapters/src/mock_rtc_adapter.cc" \
  "$repo/cpp/adapters/tests/mock_rtc_adapter_test.cc" -L"$repo/target/debug" -lmuxiva_ffi \
  -Wl,-rpath,"$repo/target/debug" -o "$build/mock_rtc_adapter_test"
"$cxx_bin" -std=c++17 -Wall -Wextra -Wpedantic -Werror -pthread $cxx_system \
  -I"$repo/cpp/include" -I"$repo/providers/transport/agora/cpp/include" \
  "$repo/providers/transport/agora/cpp/src/agora_rtc_adapter.cc" \
  "$repo/providers/transport/agora/cpp/tests/agora_rtc_adapter_test.cc" -L"$repo/target/debug" -lmuxiva_ffi \
  -Wl,-rpath,"$repo/target/debug" -o "$build/agora_rtc_adapter_test"
"$build/rtc_abi_smoke"
"$build/mock_rtc_adapter_test"
"$build/agora_rtc_adapter_test"

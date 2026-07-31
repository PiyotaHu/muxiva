#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build=${TMPDIR:-/tmp}/voxa-stage8-build
mkdir -p "$build"
sdk=$(xcrun --show-sdk-path)
cargo build --offline -p voxa-ffi --manifest-path "$repo/Cargo.toml"
cc -std=c11 -Wall -Wextra -Werror -I"$repo/cpp/include" -I"$repo/cpp/adapters/include" \
  "$repo/cpp/adapters/tests/abi_smoke.c" -o "$build/rtc_abi_smoke"
clang++ -std=c++17 -Wall -Wextra -Wpedantic -Werror -pthread \
  -isystem "$sdk/usr/include/c++/v1" -I"$repo/cpp/include" -I"$repo/cpp/adapters/include" \
  "$repo/cpp/adapters/src/in_memory_mock_rtc.cc" "$repo/cpp/adapters/src/mock_rtc_adapter.cc" \
  "$repo/cpp/adapters/tests/mock_rtc_adapter_test.cc" -L"$repo/target/debug" -lvoxa_ffi \
  -Wl,-rpath,"$repo/target/debug" -o "$build/mock_rtc_adapter_test"
"$build/rtc_abi_smoke"
"$build/mock_rtc_adapter_test"

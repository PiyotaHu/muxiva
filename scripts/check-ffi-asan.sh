#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cxx_bin=${CXX:-c++}
cxx_system=
if [ "$(uname -s)" = Darwin ]; then
  sdk=$(xcrun --show-sdk-path)
  cxx_system="-isystem $sdk/usr/include/c++/v1"
fi
cargo build --offline -p muxiva-ffi --manifest-path "$repo/Cargo.toml"
"$cxx_bin" -std=c++17 -Wall -Wextra -Werror $cxx_system -fsanitize=address,undefined \
  -fno-omit-frame-pointer -I"$repo/cpp/include" \
  "$repo/cpp/examples/uppercase_transform.cpp" -L"$repo/target/debug" -lmuxiva_ffi \
  -Wl,-rpath,"$repo/target/debug" -o "$repo/target/uppercase_transform_asan"
"$repo/target/uppercase_transform_asan"

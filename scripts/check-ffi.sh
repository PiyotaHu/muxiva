#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cc_bin=${CC:-cc}
cxx_bin=${CXX:-c++}
cxx_system=
if [ "$(uname -s)" = Darwin ]; then
  sdk=$(xcrun --show-sdk-path)
  cxx_system="-isystem $sdk/usr/include/c++/v1"
fi
cargo build --offline -p voxa-ffi --manifest-path "$repo/Cargo.toml"
"$cc_bin" -std=c11 -Wall -Wextra -Werror -I"$repo/cpp/include" \
  "$repo/cpp/tests/header_smoke.c" -L"$repo/target/debug" -lvoxa_ffi \
  -Wl,-rpath,"$repo/target/debug" -o "$repo/target/header_smoke"
"$cxx_bin" -std=c++17 -Wall -Wextra -Werror $cxx_system -I"$repo/cpp/include" \
  "$repo/cpp/examples/uppercase_transform.cpp" -L"$repo/target/debug" -lvoxa_ffi \
  -Wl,-rpath,"$repo/target/debug" -o "$repo/target/uppercase_transform"
"$cxx_bin" -std=c++17 -Wall -Wextra -Werror $cxx_system -I"$repo/cpp/include" \
  "$repo/cpp/examples/multimodal_graph.cpp" -L"$repo/target/debug" -lvoxa_ffi \
  -Wl,-rpath,"$repo/target/debug" -o "$repo/target/multimodal_graph"
"$repo/target/header_smoke"
"$repo/target/uppercase_transform"
"$repo/target/multimodal_graph"
"$repo/scripts/check-cpp-consumer.sh"

#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if ! command -v cmake >/dev/null 2>&1; then
  echo "SKIP C++ CMake consumer gate: cmake is required"
  exit 0
fi

build_dir="$repo/target/cmake-muxiva-sdk"
prefix_dir="$repo/target/cmake-muxiva-prefix"
consumer_dir="$repo/target/cmake-cpp-consumer"
rm -rf "$build_dir" "$prefix_dir" "$consumer_dir"

consumer_cxx_flags=
if [ "$(uname -s)" = Darwin ]; then
  sdk=$(xcrun --show-sdk-path)
  consumer_cxx_flags="-isystem $sdk/usr/include/c++/v1"
fi

cmake -S "$repo" -B "$build_dir" -DCMAKE_INSTALL_PREFIX="$prefix_dir"
cmake --build "$build_dir" --parallel
cmake --install "$build_dir"
cmake -S "$repo/examples/cpp/uppercase-node" -B "$consumer_dir" \
  -DCMAKE_PREFIX_PATH="$prefix_dir" \
  -DCMAKE_CXX_FLAGS="$consumer_cxx_flags"
cmake --build "$consumer_dir" --parallel
"$consumer_dir/muxiva_cpp_uppercase"

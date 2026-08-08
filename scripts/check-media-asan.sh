#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build=${TMPDIR:-/tmp}/muxiva-media-asan-build
mkdir -p "$build"
cxx_bin=${CXX:-clang++}
cxx_system=
if [ "$(uname -s)" = Darwin ]; then
  sdk=$(xcrun --show-sdk-path)
  cxx_system="-isystem $sdk/usr/include/c++/v1"
fi
"$cxx_bin" -std=c++17 -Wall -Wextra -Wpedantic -Werror -pthread \
  -fsanitize=address,undefined -fno-omit-frame-pointer $cxx_system \
  -I"$repo/cpp/media/include" "$repo/cpp/media/src/media_pipeline.cc" \
  "$repo/cpp/media/tests/media_pipeline_test.cc" \
  -o "$build/media_pipeline_asan"
"$build/media_pipeline_asan"

ffmpeg_flags=
pkg_path=${MUXIVA_FFMPEG_PKG_CONFIG_PATH:-}
if command -v pkg-config >/dev/null 2>&1; then
  if [ -z "$pkg_path" ] && command -v brew >/dev/null 2>&1 && brew --prefix ffmpeg >/dev/null 2>&1; then
    pkg_path="$(brew --prefix ffmpeg)/lib/pkgconfig"
  fi
  if PKG_CONFIG_PATH="$pkg_path${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
    pkg-config --exists libavutil libswresample libswscale 2>/dev/null; then
    ffmpeg_flags=$(PKG_CONFIG_PATH="$pkg_path${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
      pkg-config --cflags --libs libavutil libswresample libswscale)
  fi
elif command -v brew >/dev/null 2>&1 && brew --prefix ffmpeg >/dev/null 2>&1; then
  ffmpeg_root=$(brew --prefix ffmpeg)
  ffmpeg_flags="-I$ffmpeg_root/include -L$ffmpeg_root/lib -lavutil -lswresample -lswscale"
fi
if [ -z "$ffmpeg_flags" ]; then
  echo "SKIP FFmpeg ASan backend: development libraries are unavailable"
  exit 0
fi
"$cxx_bin" -std=c++17 -Wall -Wextra -Wpedantic -Werror -pthread \
  -fsanitize=address,undefined -fno-omit-frame-pointer $cxx_system \
  -DMUXIVA_ENABLE_FFMPEG=1 -I"$repo/cpp/media/include" \
  "$repo/cpp/media/src/media_pipeline.cc" "$repo/cpp/media/src/ffmpeg_backend.cc" \
  "$repo/cpp/media/tests/ffmpeg_backend_test.cc" $ffmpeg_flags \
  -o "$build/ffmpeg_backend_asan"
"$build/ffmpeg_backend_asan"

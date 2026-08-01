#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build_directory="${TMPDIR:-/tmp}/voxa-voice-node-pack-check"
cxx="${CXX:-c++}"
mkdir -p "$build_directory"

cxx_system=()
if [[ "$(uname -s)" == "Darwin" ]]; then
  sdk_path="$(xcrun --show-sdk-path)"
  cxx_system=(-isystem "$sdk_path/usr/include/c++/v1")
fi

python3 -m unittest discover \
  -s "$repository_root/examples/voice-agent/tests" -v

for package in agora_audio_source agora_audio_sink; do
  "$cxx" -std=c++17 -Wall -Wextra -Wpedantic -Werror \
    "${cxx_system[@]}" \
    -I"$repository_root/cpp/include" \
    -I"$repository_root/providers/agora/cpp/include" \
    -c "$repository_root/examples/voice-agent/.voxa/nodes/$package/node.cpp" \
    -o "$build_directory/$package.o"
done

cmake -S "$repository_root/providers/agora/cpp" \
  -B "$build_directory/agora-provider" \
  -DVOXA_ENABLE_AGORA=OFF \
  -DVOXA_SOURCE_ROOT="$repository_root"
cmake --build "$build_directory/agora-provider" --target voxa_agora

echo "Voice Node Pack validation passed: Qwen=Python; Agora=C++."

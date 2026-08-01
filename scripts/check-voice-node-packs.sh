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

cmake -S "$repository_root/examples/voice-agent" \
  -B "$build_directory/voice-agent" \
  -DVOXA_ENABLE_AGORA=OFF
cmake --build "$build_directory/voice-agent"

VOXA_VOICE_FIXTURE_GRAPH="$repository_root/examples/voice-agent/graph.json" \
  cargo test -p voxa-studio compiled_project_cpp_node_packs_load_through_the_real_abi
VOXA_VOICE_FIXTURE_GRAPH="$repository_root/examples/voice-agent/graph.json" \
  cargo test -p voxa-studio installed_voice_templates_compile_against_the_real_project_registry

echo "Voice Node Pack validation passed: Qwen=Python; Agora=C++; native ABI=loaded."

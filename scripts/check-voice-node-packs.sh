#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build_directory="$(mktemp -d "${TMPDIR:-/tmp}/muxiva-voice-node-pack-check.XXXXXX")"
trap 'rm -rf "$build_directory"' EXIT
cxx="${CXX:-c++}"

bash -n \
  "$repository_root/providers/transport/agora/cpp/download-macos-sdk.sh" \
  "$repository_root/examples/voice-agent/setup.sh" \
  "$repository_root/examples/voice-agent/run.sh"

studio_command="$(MUXIVA_BIN=/bin/echo "$repository_root/examples/voice-agent/run.sh" --studio --port 5678 --no-open | tail -n 1)"
headless_command="$(MUXIVA_BIN=/bin/echo "$repository_root/examples/voice-agent/run.sh" --headless --port 18080 | tail -n 1)"
test "$studio_command" = "studio $repository_root/examples/voice-agent/graph.json --port 5678 --no-open"
test "$headless_command" = "serve $repository_root/examples/voice-agent/graph.json --port 18080"

"$repository_root/scripts/check-agent-typescript.sh"

cxx_system=()
if [[ "$(uname -s)" == "Darwin" ]]; then
  sdk_path="$(xcrun --show-sdk-path)"
  cxx_system=(-isystem "$sdk_path/usr/include/c++/v1")
fi

python3 -m unittest discover \
  -s "$repository_root/providers/algorithm/qwen/python/tests" -v
python3 -m unittest discover \
  -s "$repository_root/examples/voice-agent/tests" -v

for package in agora_audio_source agora_audio_sink agora_data_source agora_data_sink; do
  "$cxx" -std=c++17 -Wall -Wextra -Wpedantic -Werror \
    "${cxx_system[@]}" \
    -I"$repository_root/cpp/include" \
    -I"$repository_root/providers/transport/agora/cpp/include" \
    -c "$repository_root/providers/transport/agora/cpp/nodes/$package/node.cpp" \
    -o "$build_directory/$package.o"
done

cmake -S "$repository_root/providers/transport/agora/cpp" \
  -B "$build_directory/agora-provider" \
  -DMUXIVA_ENABLE_AGORA=OFF \
  -DMUXIVA_SOURCE_ROOT="$repository_root"
cmake --build "$build_directory/agora-provider" --target muxiva_agora

cmake -S "$repository_root/providers/transport/agora/cpp" \
  -B "$build_directory/agora-node-packs" \
  -DMUXIVA_ENABLE_AGORA=OFF \
  -DMUXIVA_SOURCE_ROOT="$repository_root" \
  -DMUXIVA_NODE_PACK_OUTPUT_ROOT="$build_directory/voice-node-packs"
cmake --build "$build_directory/agora-node-packs"
for package in agora_audio_source agora_audio_sink agora_data_source agora_data_sink; do
  test "$(cat "$build_directory/voice-node-packs/$package/provider-mode")" = "offline-stub"
done

MUXIVA_VOICE_FIXTURE_GRAPH="$repository_root/examples/voice-agent/graph.json" \
MUXIVA_NATIVE_NODE_ROOT="$build_directory/voice-node-packs" \
  cargo test -p muxiva-studio compiled_project_cpp_node_packs_load_through_the_real_abi
MUXIVA_VOICE_FIXTURE_GRAPH="$repository_root/examples/voice-agent/graph.json" \
MUXIVA_NATIVE_NODE_ROOT="$build_directory/voice-node-packs" \
  cargo test -p muxiva-studio installed_voice_templates_compile_against_the_real_project_registry

echo "Voice Node Pack validation passed: Qwen=Python; Agora=C++; native ABI=loaded."

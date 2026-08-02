#!/usr/bin/env bash
set -euo pipefail

application_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$application_root/../.." && pwd)"
qwen_provider_root="$repository_root/providers/qwen/python"
agora_provider_root="$repository_root/providers/agora/cpp"
sdk_root="${1:-${VOXA_AGORA_SDK_ROOT:-}}"

if [[ -z "$sdk_root" ]]; then
  echo "Usage: ./examples/voice-agent/setup.sh /absolute/path/to/agora-native-sdk" >&2
  exit 2
fi
if [[ ! -d "$sdk_root" ]]; then
  echo "Agora SDK directory does not exist: $sdk_root" >&2
  exit 2
fi

echo "[VOXA][SETUP] Installing Python Node Pack dependencies"
python3 -m pip install -r "$qwen_provider_root/requirements.txt"

echo "[VOXA][SETUP] Building trusted C++ Agora Node Packs"
cmake -S "$agora_provider_root" \
  -B "$repository_root/build/voice-agent" \
  -DVOXA_ENABLE_AGORA=ON \
  -DVOXA_AGORA_SDK_ROOT="$sdk_root" \
  -DVOXA_NODE_PACK_OUTPUT_ROOT="$application_root/.voxa/native"
cmake --build "$repository_root/build/voice-agent" --config Release

echo "[VOXA][READY] Native and Python Node Packs are installed."
echo "[VOXA][NEXT]  ./examples/voice-agent/run.sh"

#!/usr/bin/env bash
set -euo pipefail

application_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$application_root/../.." && pwd)"
qwen_provider_root="$repository_root/providers/algorithm/qwen/python"
agora_provider_root="$repository_root/providers/transport/agora/cpp"
sdk_root="${1:-${VOXA_AGORA_SDK_ROOT:-}}"

if [[ "$sdk_root" == "--help" || "$sdk_root" == "-h" ]]; then
  cat <<'EOF'
Usage:
  ./examples/voice-agent/setup.sh
      macOS: download the pinned official Agora SDK, verify it, then build.

  ./examples/voice-agent/setup.sh /absolute/path/to/agora-sdk
      Use an SDK that you downloaded manually.

Qwen does not require a vendor SDK download. This command creates a project
Python virtual environment and installs the websocket dependency automatically.
EOF
  exit 0
fi

if [[ -z "$sdk_root" ]]; then
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "[VOXA][ERROR] Automatic Agora SDK download currently supports macOS only." >&2
    echo "[VOXA][HELP]  Download your platform SDK from https://docs.agora.io/en/api-reference/sdks?product=voice" >&2
    echo "[VOXA][NEXT]  ./examples/voice-agent/setup.sh /absolute/path/to/agora-sdk" >&2
    exit 2
  fi
  sdk_root="$repository_root/build/vendor/agora-macos-4.6.2"
  "$agora_provider_root/download-macos-sdk.sh" "$sdk_root"
fi
if [[ ! -d "$sdk_root" ]]; then
  echo "Agora SDK directory does not exist: $sdk_root" >&2
  exit 2
fi

echo "[VOXA][SETUP] Creating isolated Python environment"
python3 -m venv "$application_root/.voxa/venv"
"$application_root/.voxa/venv/bin/python" -m pip install \
  --disable-pip-version-check -r "$qwen_provider_root/requirements.txt"

echo "[VOXA][SETUP] Building trusted C++ Agora Node Packs"
cmake -S "$agora_provider_root" \
  -B "$repository_root/build/voice-agent-provider-v1" \
  -DVOXA_ENABLE_AGORA=ON \
  -DVOXA_AGORA_SDK_ROOT="$sdk_root" \
  -DVOXA_SOURCE_ROOT="$repository_root" \
  -DVOXA_NODE_PACK_OUTPUT_ROOT="$application_root/.voxa/native"
cmake --build "$repository_root/build/voice-agent-provider-v1" --config Release

echo "[VOXA][READY] Native and Python Node Packs are installed."
echo "[VOXA][AGORA] sdk=$sdk_root"
echo "[VOXA][QWEN]  python=$application_root/.voxa/venv/bin/python (no Qwen SDK download required)"
echo "[VOXA][NEXT]  ./examples/voice-agent/run.sh"

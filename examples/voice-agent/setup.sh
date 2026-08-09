#!/usr/bin/env bash
set -euo pipefail

application_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$application_root/../.." && pwd)"
qwen_provider_root="$repository_root/providers/algorithm/qwen/python"
agora_provider_root="$repository_root/providers/transport/agora/cpp"
sdk_root="${1:-${MUXIVA_AGORA_SDK_ROOT:-}}"
bootstrap_python="${MUXIVA_BOOTSTRAP_PYTHON:-}"
node_command="${MUXIVA_NODE:-node}"
npm_command="${MUXIVA_NPM:-npm}"

if [[ "$sdk_root" == "--help" || "$sdk_root" == "-h" ]]; then
  cat <<'EOF'
Usage:
  ./examples/voice-agent/setup.sh
      macOS: download the pinned official Agora SDK, verify it, then build.

  ./examples/voice-agent/setup.sh /absolute/path/to/agora-sdk
      Use an SDK that you downloaded manually.

Qwen does not require a vendor SDK download. This command creates a project
Python virtual environment and installs the websocket dependency automatically.
Demo 2 uses Pi as a TypeScript Agent Node. This command also installs its
locked npm dependencies. Node.js 22.19 or newer is required.

Set MUXIVA_BOOTSTRAP_PYTHON=/absolute/path/to/python3 to override automatic
Python selection. Muxiva requires Python 3.10 or newer for this demo.
EOF
  exit 0
fi

if ! command -v "$node_command" >/dev/null 2>&1; then
  echo '[MUXIVA][ERROR] Node.js 22.19 or newer is required by the Pi Agent Node.' >&2
  echo '[MUXIVA][HELP]  Install an active Node.js release from https://nodejs.org/en/download' >&2
  exit 2
fi
if ! "$node_command" --eval 'const [major, minor] = process.versions.node.split(".").map(Number); process.exit(major > 22 || (major === 22 && minor >= 19) ? 0 : 1)'; then
  echo "[MUXIVA][ERROR] Node.js 22.19 or newer is required; found $($node_command --version)." >&2
  echo '[MUXIVA][HELP]  Upgrade from https://nodejs.org/en/download' >&2
  exit 2
fi
if ! command -v "$npm_command" >/dev/null 2>&1; then
  echo '[MUXIVA][ERROR] npm is required to install the locked Pi Agent dependencies.' >&2
  echo '[MUXIVA][HELP]  Install the official Node.js distribution, which includes npm.' >&2
  exit 2
fi

if [[ -z "$sdk_root" ]]; then
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "[MUXIVA][ERROR] Automatic Agora SDK download currently supports macOS only." >&2
    echo "[MUXIVA][HELP]  Download your platform SDK from https://docs.agora.io/en/api-reference/sdks?product=voice" >&2
    echo "[MUXIVA][NEXT]  ./examples/voice-agent/setup.sh /absolute/path/to/agora-sdk" >&2
    exit 2
  fi
  sdk_root="$repository_root/build/vendor/agora-macos-4.6.2"
  "$agora_provider_root/download-macos-sdk.sh" "$sdk_root"
fi
if [[ ! -d "$sdk_root" ]]; then
  echo "Agora SDK directory does not exist: $sdk_root" >&2
  exit 2
fi

if [[ -z "$bootstrap_python" ]]; then
  for candidate in python3.13 python3.12 python3.11 python3.10 python3; do
    if command -v "$candidate" >/dev/null 2>&1 && "$candidate" -c 'import sys; raise SystemExit(sys.version_info < (3, 10))' 2>/dev/null; then
      bootstrap_python="$(command -v "$candidate")"
      break
    fi
  done
fi
if [[ -z "$bootstrap_python" ]] || ! "$bootstrap_python" -c 'import sys; raise SystemExit(sys.version_info < (3, 10))' 2>/dev/null; then
  echo '[MUXIVA][ERROR] Python 3.10 or newer is required to create the Qwen environment.' >&2
  echo '[MUXIVA][HELP]  Set MUXIVA_BOOTSTRAP_PYTHON=/absolute/path/to/python3 and rerun setup.sh.' >&2
  exit 2
fi

echo "[MUXIVA][SETUP] Python bootstrap=$bootstrap_python version=$($bootstrap_python --version 2>&1)"
echo "[MUXIVA][SETUP] Creating isolated Python environment"
"$bootstrap_python" -m venv "$application_root/.muxiva/venv"
"$application_root/.muxiva/venv/bin/python" -m pip install \
  --disable-pip-version-check -r "$qwen_provider_root/requirements.txt"

echo "[MUXIVA][SETUP] Installing locked Pi TypeScript Agent dependencies with $($node_command --version)"
"$npm_command" ci --ignore-scripts \
  --cache "$application_root/.muxiva/npm-cache" \
  --prefix "$application_root"
"$npm_command" run --prefix "$application_root" check:typescript

echo "[MUXIVA][SETUP] Building trusted C++ Agora Node Packs"
cmake -S "$agora_provider_root" \
  -B "$repository_root/build/voice-agent-provider-v1" \
  -DMUXIVA_ENABLE_AGORA=ON \
  -DMUXIVA_AGORA_SDK_ROOT="$sdk_root" \
  -DMUXIVA_SOURCE_ROOT="$repository_root" \
  -DMUXIVA_NODE_PACK_OUTPUT_ROOT="$application_root/.muxiva/native"
cmake --build "$repository_root/build/voice-agent-provider-v1" --config Release

echo "[MUXIVA][READY] Native, Python, and TypeScript Agent Node Packs are installed."
echo "[MUXIVA][AGORA] sdk=$sdk_root"
echo "[MUXIVA][QWEN]  python=$application_root/.muxiva/venv/bin/python (no Qwen SDK download required)"
echo "[MUXIVA][PI]    node=$node_command version=$($node_command --version) package=@earendil-works/pi-agent-core@0.84.1"
echo "[MUXIVA][NEXT]  ./examples/voice-agent/run.sh"

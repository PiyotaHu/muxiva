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
cargo_command="${MUXIVA_CARGO:-cargo}"
pi_agent_repository="${MUXIVA_PI_AGENT_REPOSITORY:-https://github.com/PiyotaHu/muxiva-pi-agent.git}"
pi_agent_ref="${MUXIVA_PI_AGENT_REF:-v0.2.1}"
pi_agent_root="$application_root/.muxiva/agents/muxiva-pi-agent"

if [[ "$sdk_root" == "--help" || "$sdk_root" == "-h" ]]; then
  cat <<'EOF'
Usage:
  ./examples/voice-agent/setup.sh
      macOS / Linux: download the pinned official Agora SDK, verify it, then build.

  ./examples/voice-agent/setup.sh /absolute/path/to/agora-sdk
      Use an SDK that you downloaded manually.

Qwen does not require a vendor SDK download. This command creates a project
Python virtual environment and installs the websocket dependency automatically.
Demo 2 integrates the independently versioned PiyotaHu/muxiva-pi-agent
repository as a TypeScript Agent Node. This command checks out the locked
v0.2.1 release and installs its dependencies. Node.js 22.19 or newer is
required.

Advanced application integration:
  MUXIVA_PI_AGENT_REPOSITORY=https://github.com/your-org/your-agent.git \
  MUXIVA_PI_AGENT_REF=v1.2.3 ./examples/voice-agent/setup.sh

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
if ! command -v git >/dev/null 2>&1; then
  echo '[MUXIVA][ERROR] Git is required to install the external Pi Agent repository.' >&2
  echo '[MUXIVA][HELP]  Install Git, or see https://piyotahu.github.io/muxiva/nodes/agent-integration/' >&2
  exit 2
fi
if ! command -v "$cargo_command" >/dev/null 2>&1; then
  echo '[MUXIVA][ERROR] Cargo is required to build the Studio/CLI from this source checkout.' >&2
  echo '[MUXIVA][HELP]  Install Rust from https://rustup.rs and rerun setup.sh.' >&2
  exit 2
fi

if [[ -z "$sdk_root" ]]; then
  if [[ "$(uname -s)" == "Darwin" ]]; then
    sdk_root="$repository_root/build/vendor/agora-macos-4.6.2"
    "$agora_provider_root/download-macos-sdk.sh" "$sdk_root"
  elif [[ "$(uname -s)" == "Linux" ]]; then
    sdk_root="$repository_root/build/vendor/agora-linux-4.4.32"
    "$agora_provider_root/download-linux-sdk.sh" "$sdk_root"
  else
    echo "[MUXIVA][ERROR] Automatic Agora SDK download supports macOS and Linux only." >&2
    echo "[MUXIVA][HELP]  Download your platform SDK from https://docs.agora.io/en/api-reference/sdks?product=voice" >&2
    echo "[MUXIVA][NEXT]  ./examples/voice-agent/setup.sh /absolute/path/to/agora-sdk" >&2
    exit 2
  fi
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
echo '[MUXIVA][SETUP] Building the repository Studio/CLI so embedded assets match this checkout'
"$cargo_command" build --locked --manifest-path "$repository_root/Cargo.toml" -p muxiva-cli
echo "[MUXIVA][SETUP] Creating isolated Python environment"
"$bootstrap_python" -m venv "$application_root/.muxiva/venv"
"$application_root/.muxiva/venv/bin/python" -m pip install \
  --disable-pip-version-check -r "$qwen_provider_root/requirements.txt"

echo "[MUXIVA][SETUP] Installing application-owned Pi Agent repository ref=$pi_agent_ref"
if [[ -e "$pi_agent_root" && ! -d "$pi_agent_root/.git" ]]; then
  echo "[MUXIVA][ERROR] Refusing to overwrite non-Git Agent directory: $pi_agent_root" >&2
  echo '[MUXIVA][NEXT]  Move that directory aside, or set MUXIVA_PI_AGENT_REPOSITORY and MUXIVA_PI_AGENT_REF for your own reviewed Agent.' >&2
  exit 2
fi
if [[ -d "$pi_agent_root/.git" ]]; then
  if [[ -n "$(git -C "$pi_agent_root" status --porcelain)" ]]; then
    echo "[MUXIVA][ERROR] Refusing to replace local changes in Agent repository: $pi_agent_root" >&2
    echo '[MUXIVA][NEXT]  Commit or move those changes before rerunning setup.' >&2
    exit 2
  fi
  installed_remote="$(git -C "$pi_agent_root" remote get-url origin 2>/dev/null || true)"
  if [[ "$installed_remote" != "$pi_agent_repository" ]]; then
    echo "[MUXIVA][ERROR] Installed Agent remote does not match: $installed_remote" >&2
    echo "[MUXIVA][NEXT]  Expected $pi_agent_repository; move $pi_agent_root aside and rerun setup." >&2
    exit 2
  fi
  git -C "$pi_agent_root" fetch --depth 1 origin "refs/tags/$pi_agent_ref:refs/tags/$pi_agent_ref" 2>/dev/null || \
    git -C "$pi_agent_root" fetch --depth 1 origin "$pi_agent_ref"
  git -C "$pi_agent_root" checkout --detach "$pi_agent_ref"
  echo "[MUXIVA][AGENT][REUSE] repository=$pi_agent_repository ref=$pi_agent_ref"
else
  mkdir -p "$(dirname "$pi_agent_root")"
  git clone --depth 1 --branch "$pi_agent_ref" "$pi_agent_repository" "$pi_agent_root"
fi
if [[ ! -f "$pi_agent_root/package.json" || ! -f "$pi_agent_root/src/index.ts" ]]; then
  echo '[MUXIVA][ERROR] Agent repository does not expose the expected TypeScript package.' >&2
  echo '[MUXIVA][HELP]  Follow https://piyotahu.github.io/muxiva/nodes/agent-integration/' >&2
  exit 2
fi
pi_agent_commit="$(git -C "$pi_agent_root" rev-parse HEAD)"
mkdir -p "$application_root/.muxiva/workspaces/pi-agent"

echo "[MUXIVA][SETUP] Installing locked TypeScript dependencies with $($node_command --version)"
if "$npm_command" ls --all --prefix "$application_root" >/dev/null 2>&1 &&
   "$npm_command" run --prefix "$application_root" check:typescript >/dev/null 2>&1; then
  echo '[MUXIVA][AGENT][REUSE] Locked TypeScript dependencies are already installed and valid; npm ci skipped.'
else
  "$npm_command" ci --ignore-scripts \
    --cache "$application_root/.muxiva/npm-cache" \
    --prefix "$application_root"
fi
"$npm_command" run --prefix "$application_root" check:typescript
"$npm_command" run --prefix "$pi_agent_root" check
"$npm_command" test --prefix "$pi_agent_root"

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
echo "[MUXIVA][AGENT] repository=$pi_agent_repository ref=$pi_agent_ref commit=$pi_agent_commit"
echo "[MUXIVA][AGENT] workspace=$application_root/.muxiva/workspaces/pi-agent permissions=list,read,search,create,replace,web-search"
echo "[MUXIVA][NEXT]  ./examples/voice-agent/run.sh"

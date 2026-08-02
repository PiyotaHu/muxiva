#!/usr/bin/env bash
set -euo pipefail

application_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$application_root/../.." && pwd)"

if command -v voxa >/dev/null 2>&1; then
  voxa_binary="$(command -v voxa)"
elif [[ -x "$repository_root/target/release/voxa" ]]; then
  voxa_binary="$repository_root/target/release/voxa"
elif [[ -x "$repository_root/target/debug/voxa" ]]; then
  voxa_binary="$repository_root/target/debug/voxa"
else
  echo "The voxa binary is not installed. Install a release binary or run: cargo install --path crates/voxa-cli --locked" >&2
  exit 2
fi

if [[ -x "$application_root/.voxa/venv/bin/python" ]]; then
  export VOXA_PYTHON="$application_root/.voxa/venv/bin/python"
fi

echo '[VOXA][WELCOME] Real voice setup requires two connection cards: Agora RTC and Alibaba Cloud Model Studio.'
echo '[VOXA][STEP 1] Studio opens now. Click Connections in the top toolbar.'
echo '[VOXA][STEP 2] Fill missing Required fields once and click Save connections. Studio persists them in this project .env.'
echo '[VOXA][STEP 3] Do not click Run or Voice Room until both cards show Ready.'
echo '[VOXA][HELP]  https://piyotahu.github.io/Voxa/voice-demo/'
echo "[VOXA][LOG]   $application_root/.voxa/runtime.log"

studio_args=("$application_root/graph.json" "$@")
if [[ "$(uname -s)" == "Linux" && -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
  has_no_open=false
  for argument in "$@"; do
    if [[ "$argument" == "--no-open" ]]; then
      has_no_open=true
      break
    fi
  done
  if [[ "$has_no_open" == false ]]; then
    studio_args+=("--no-open")
  fi
  echo '[VOXA][HEADLESS] No desktop session detected; browser auto-open is disabled.'
  echo '[VOXA][HEADLESS] For SSH access, restart with --port 5678 and forward 127.0.0.1:5678 from your laptop.'
  echo '[VOXA][HEADLESS] Guide: https://piyotahu.github.io/Voxa/remote-studio/'
fi

mkdir -p "$application_root/.voxa"
set +e
"$voxa_binary" studio "${studio_args[@]}" 2>&1 \
  | tee "$application_root/.voxa/runtime.log"
status=${PIPESTATUS[0]}
set -e
exit "$status"

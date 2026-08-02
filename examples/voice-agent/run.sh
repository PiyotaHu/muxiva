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
echo '[VOXA][STEP 2] Fill every Required field and click Save connections.'
echo '[VOXA][STEP 3] Do not click Run or Voice Room until both cards show Ready.'
echo '[VOXA][HELP]  https://piyotahu.github.io/Voxa/voice-demo/'

exec "$voxa_binary" studio "$application_root/graph.json"

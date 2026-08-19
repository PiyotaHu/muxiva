#!/usr/bin/env bash
# Start the Xiaozhi voice agent headlessly.
#
# The Xiaozhi WebSocket transport lives inside the graph (xiaozhi.audio_source),
# so this single command starts the whole pipeline: device -> VAD+ASR -> LLM ->
# TTS -> device. Point the ESP32 firmware OTA/websocket URL at
# ws://<raspberry-pi-ip>:8888.
set -euo pipefail

cd "$(dirname "$0")"

if [ ! -f .env ]; then
  echo "[run] missing .env; copy .env.example to .env and fill your keys" >&2
  exit 1
fi

set -a
# shellcheck disable=SC1091
source .env
set +a

exec cargo run -p muxiva-cli -- serve graph.json --host 0.0.0.0 --port 8080

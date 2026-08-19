#!/usr/bin/env bash
# One-time Raspberry Pi (and general Debian/Ubuntu) setup for the Xiaozhi agent.
set -euo pipefail

cd "$(dirname "$0")"

echo "[setup] install system Opus codec"
sudo apt-get update -y
sudo apt-get install -y libopus0 libopus-dev

echo "[setup] install Python WebSocket dependency"
python3 -m pip install --user -r ../providers/transport/xiaozhi/python/requirements.txt 2>/dev/null \
  || python3 -m pip install -r ../providers/transport/xiaozhi/python/requirements.txt

echo "[setup] install provider Python dependencies"
python3 -m pip install --user -r ../providers/algorithm/qwen/python/requirements.txt 2>/dev/null \
  || python3 -m pip install -r ../providers/algorithm/qwen/python/requirements.txt

if [ ! -f .env ]; then
  cp .env.example .env
  echo "[setup] created .env — edit it with your DeepSeek and DashScope keys"
else
  echo "[setup] .env already exists, leaving it untouched"
fi

echo "[setup] done. Next: edit .env, then run ./run.sh"

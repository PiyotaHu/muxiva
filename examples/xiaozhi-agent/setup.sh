#!/usr/bin/env bash
# One-time Raspberry Pi (and general Debian/Ubuntu) setup for the Xiaozhi agent.
set -euo pipefail

cd "$(dirname "$0")"

repo_root="$(cd ../.. && pwd)"
python_bin="$repo_root/.venv/bin/python"

echo "[setup] install system Opus codec and Python virtualenv support"
sudo apt-get update -y
sudo apt-get install -y libopus0 libopus-dev python3-venv

if [ ! -x "$python_bin" ]; then
  echo "[setup] create repository Python environment"
  python3 -m venv "$repo_root/.venv"
fi

echo "[setup] install transport, speech, and artwork dependencies"
"$python_bin" -m pip install \
  -r "$repo_root/providers/transport/xiaozhi/python/requirements.txt" \
  -r "$repo_root/providers/algorithm/qwen/python/requirements.txt" \
  -r "$repo_root/examples/xiaozhi-agent/requirements.txt"

"$python_bin" -c 'from PIL import Image; print("[setup] artwork image pipeline ready")'

if [ ! -f .env ]; then
  cp .env.example .env
  echo "[setup] created .env — edit it with your DeepSeek and DashScope keys"
else
  echo "[setup] .env already exists, leaving it untouched"
fi

echo "[setup] done. Next: edit .env, then run ./run.sh"

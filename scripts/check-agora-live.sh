#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
python_bin=${VOXA_AGORA_PYTHON:-"$repo/target/agora-python-probe/bin/python"}
if [ -z "${VOXA_AGORA_APP_ID:-}" ] || [ -z "${VOXA_AGORA_CHANNEL:-}" ]; then
  echo "SKIP Agora live soak: set VOXA_AGORA_APP_ID and VOXA_AGORA_CHANNEL"
  exit 0
fi
if [ ! -x "$python_bin" ]; then
  echo "Agora live soak requires VOXA_AGORA_PYTHON with CPython 3.9" >&2
  exit 1
fi
export VOXA_AGORA_SOAK_SECONDS=${VOXA_AGORA_SOAK_SECONDS:-60}
exec "$python_bin" "$repo/examples/python/agora_soak.py"

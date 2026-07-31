#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
python_bin=${VOXA_PYTHON:-$(command -v python3)}
wheel_dir="$repo/target/stage9-wheels"
unpack_dir="$repo/target/stage9-python-unpacked"
mkdir -p "$wheel_dir" "$unpack_dir"
PYO3_PYTHON="$python_bin" "$python_bin" -m maturin build \
  --manifest-path "$repo/crates/voxa-python/Cargo.toml" \
  --interpreter "$python_bin" --out "$wheel_dir"
wheel=$(find "$wheel_dir" -type f -name 'voxa-*.whl' | sort | tail -n 1)
test -n "$wheel"
rm -rf "$unpack_dir"
mkdir -p "$unpack_dir"
"$python_bin" -m zipfile -e "$wheel" "$unpack_dir"
PYTHONPATH="$unpack_dir" "$python_bin" -m pytest -q "$repo/crates/voxa-python/tests"

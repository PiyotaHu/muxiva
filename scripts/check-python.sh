#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -n "${VOXA_PYTHON:-}" ]; then
  python_bin=$VOXA_PYTHON
elif [ -x /Users/private-user/.pyenv/versions/3.13.12/bin/python3.13 ] && /Users/private-user/.pyenv/versions/3.13.12/bin/python3.13 -m maturin --version >/dev/null 2>&1; then
  python_bin=/Users/private-user/.pyenv/versions/3.13.12/bin/python3.13
else
  python_bin=$(command -v python3)
fi
if ! "$python_bin" -m maturin --version >/dev/null 2>&1; then
  echo "SKIP Python binding gate: $python_bin has no maturin module"
  exit 0
fi
echo "Python binding interpreter: $python_bin"
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

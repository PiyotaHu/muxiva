#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
python_bin=${VOXA_PYTHON:-/Users/private-user/.pyenv/versions/3.13.12/bin/python3.13}
PYO3_PYTHON="$python_bin" cargo test --offline --workspace --manifest-path "$repo/Cargo.toml"
"$repo/scripts/check-python.sh"
"$repo/scripts/check-node.sh"

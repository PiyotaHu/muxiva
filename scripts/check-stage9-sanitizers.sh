#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
python_bin=${MUXIVA_PYTHON:-$(command -v python3)}
PYO3_PYTHON="$python_bin" cargo test --offline --workspace --manifest-path "$repo/Cargo.toml"
"$repo/scripts/check-python.sh"
"$repo/scripts/check-node.sh"

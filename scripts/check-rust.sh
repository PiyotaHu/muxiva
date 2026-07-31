#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

if [[ -z "${PYO3_PYTHON:-}" ]]; then
  if [[ -x /Users/private-user/.pyenv/versions/3.13.12/bin/python3.13 ]]; then
    export PYO3_PYTHON=/Users/private-user/.pyenv/versions/3.13.12/bin/python3.13
  else
    export PYO3_PYTHON="$(command -v python3)"
  fi
fi
echo "PyO3 interpreter: $PYO3_PYTHON"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets

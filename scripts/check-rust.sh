#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

if [[ -z "${PYO3_PYTHON:-}" ]]; then
  export PYO3_PYTHON="$(command -v python3)"
fi
echo "PyO3 interpreter: $PYO3_PYTHON"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets

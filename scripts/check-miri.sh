#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

if ! rustup toolchain list | grep -q '^nightly'; then
  echo "SKIP Miri: no nightly toolchain is installed; CI provisions a pinned nightly"
  exit 0
fi

if ! cargo +nightly miri --version >/dev/null 2>&1; then
  echo "SKIP Miri: the nightly miri component is unavailable"
  exit 0
fi

cargo +nightly miri test -p voxa-types --lib


#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

toolchain="${VOXA_MIRI_TOOLCHAIN:-nightly-2026-07-15}"

if ! rustup toolchain list | grep -q "^${toolchain}"; then
  echo "SKIP Miri: pinned toolchain ${toolchain} is not installed; CI provisions it"
  exit 0
fi

if ! cargo "+${toolchain}" miri --version >/dev/null 2>&1; then
  echo "SKIP Miri: the miri component is unavailable for ${toolchain}"
  exit 0
fi

export CARGO_NET_OFFLINE=true
cargo "+${toolchain}" miri test -p voxa-types --lib

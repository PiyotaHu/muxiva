#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

./scripts/check-rust.sh
./scripts/check-ffi.sh
./scripts/check-rtc.sh
./scripts/check-media.sh

if [[ -x ./scripts/check-python.sh ]]; then
  ./scripts/check-python.sh
else
  echo "SKIP Python binding gate: Stage 9 check script is not present"
fi

if [[ -x ./scripts/check-node.sh ]]; then
  ./scripts/check-node.sh
else
  echo "SKIP Node binding gate: Stage 9 check script is not present"
fi

./scripts/check-provider-boundaries.sh
./scripts/check-voice-node-packs.sh

#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

if ! cargo fuzz --help >/dev/null 2>&1; then
  echo "SKIP fuzz: cargo-fuzz is unavailable; scheduled CI provisions it"
  exit 0
fi

for target in frame_construction signal_event_value graph_json; do
  cargo fuzz run "$target" -- -runs="${VOXA_FUZZ_RUNS:-1000}"
done


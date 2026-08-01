#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

toolchain="${VOXA_FUZZ_TOOLCHAIN:-nightly-2026-07-15}"

if ! rustup toolchain list | grep -q "^${toolchain}"; then
  echo "SKIP fuzz: pinned toolchain ${toolchain} is not installed; CI provisions it"
  exit 0
fi

if ! cargo "+${toolchain}" fuzz --help >/dev/null 2>&1; then
  echo "SKIP fuzz: cargo-fuzz is unavailable; scheduled CI provisions it"
  exit 0
fi

export CARGO_NET_OFFLINE=true
for target in frame_construction signal_event_value ffi_frame_view graph_json edge_policy_config; do
  cargo "+${toolchain}" fuzz run "$target" -- \
    -runs="${VOXA_FUZZ_RUNS:-1000}" \
    -max_len="${VOXA_FUZZ_MAX_LEN:-65536}"
done

# Checked-in minimized failures are ordinary deterministic regressions.
cargo test --offline --workspace --all-targets

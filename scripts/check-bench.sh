#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

report_dir="${MUXIVA_BENCH_REPORT_DIR:-$repository_root/tests/reports/benchmarks}"
mkdir -p "$report_dir"
commit="$(git rev-parse HEAD 2>/dev/null || printf unknown)"
toolchain="$(rustc --version --verbose | tr '\n' ';')"
export MUXIVA_BENCH_COMMIT="$commit" MUXIVA_BENCH_TOOLCHAIN="$toolchain"

for scenario in queue flow frame-copy managed-stream stop; do
  cargo run --locked --offline --release -p muxiva-bench -- "$scenario" >"$report_dir/$scenario.json"
done

printf 'benchmark reports (measurements only; no regression gate): %s\n' "$report_dir"

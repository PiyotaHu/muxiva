#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
studio_dir="$repository_root/crates/voxa-studio"

if [[ ! -f "$studio_dir/package.json" || ! -f "$studio_dir/playwright.config.ts" ]]; then
  echo "SKIP Studio E2E: Stage 10 browser application and Playwright contract are not present"
  exit 0
fi
if ! command -v pnpm >/dev/null 2>&1; then
  echo "SKIP Studio E2E: pnpm is unavailable"
  exit 0
fi

cd "$studio_dir"
pnpm exec playwright test

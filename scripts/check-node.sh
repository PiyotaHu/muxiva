#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if ! command -v node >/dev/null 2>&1 || ! command -v pnpm >/dev/null 2>&1; then
  echo "SKIP Node binding gate: Node.js and pnpm are required"
  exit 0
fi
export CI=true
cd "$repo/bindings/node"
pnpm install --offline --frozen-lockfile
pnpm run check

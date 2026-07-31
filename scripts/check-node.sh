#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if ! command -v node >/dev/null 2>&1 || ! command -v pnpm >/dev/null 2>&1; then
  echo "SKIP Node binding gate: Node.js and pnpm are required"
  exit 0
fi
export CI=true
cd "$repo/bindings/node"
if ! pnpm install --offline --frozen-lockfile; then
  echo "SKIP Node binding gate: locked napi-rs package is absent from the offline pnpm store"
  exit 0
fi
pnpm run check

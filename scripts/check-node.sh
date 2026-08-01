#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if ! command -v node >/dev/null 2>&1 || ! command -v pnpm >/dev/null 2>&1; then
  echo "SKIP Node binding gate: Node.js and pnpm are required"
  exit 0
fi
export CI=true
node --check "$repo/crates/voxa-studio/src/assets/studio.js"
cd "$repo/bindings/node"
if ! pnpm install --offline --frozen-lockfile; then
  echo "SKIP Node binding gate: one or more locked build dependencies are absent from the offline pnpm store"
  exit 0
fi
pnpm run check

package_dir="$repo/target/node-sdk-package"
consumer_dir="$repo/target/node-sdk-consumer"
rm -rf "$package_dir" "$consumer_dir"
mkdir -p "$package_dir" "$consumer_dir"
pnpm pack --pack-destination "$package_dir"
package=$(find "$package_dir" -type f -name 'voxa-core-*.tgz' | sort | tail -n 1)
test -n "$package"

cd "$consumer_dir"
pnpm init >/dev/null
pnpm pkg set type=module
pnpm add "$package"
cp "$repo/examples/typescript/tsconfig.json" "$repo/examples/typescript/uppercase-node.ts" .
"$repo/bindings/node/node_modules/.bin/tsc" -p tsconfig.json
node dist/uppercase-node.js

#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
node_home=${VOXA_NODE_HOME:-/Users/private-user/.nvm/versions/node/v22.22.0}
PATH="$node_home/bin:$PATH"
export PATH
export CI=true
cd "$repo/bindings/node"
pnpm install --frozen-lockfile
pnpm run check

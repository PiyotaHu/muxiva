#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
voice_root="$repository_root/examples/voice-agent"

if ! command -v node >/dev/null 2>&1; then
  echo 'SKIP TypeScript Agent gate: Node.js 22.19 or newer is required'
  exit 0
fi
if ! node --eval 'const [major, minor] = process.versions.node.split(".").map(Number); process.exit(major > 22 || (major === 22 && minor >= 19) ? 0 : 1)'; then
  echo "SKIP TypeScript Agent gate: Node.js 22.19 or newer is required; found $(node --version)"
  exit 0
fi

npm --prefix "$repository_root/bindings/agent" test

if [[ ! -f "$voice_root/node_modules/@piyotahu/muxiva-pi-agent/package.json" ||
      ! -f "$voice_root/.muxiva/agents/muxiva-pi-agent/src/index.ts" ]]; then
  echo 'SKIP Pi Agent compile gate: run examples/voice-agent/setup.sh to install the locked external Agent'
  exit 0
fi

npm --prefix "$voice_root" run check:typescript
npm --prefix "$voice_root" run test:agent
npm --prefix "$voice_root/.muxiva/agents/muxiva-pi-agent" run check
npm --prefix "$voice_root/.muxiva/agents/muxiva-pi-agent" test

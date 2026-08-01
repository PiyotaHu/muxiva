#!/usr/bin/env bash
set -euo pipefail

application_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$application_root/../.." && pwd)"

if command -v voxa >/dev/null 2>&1; then
  voxa_binary="$(command -v voxa)"
elif [[ -x "$repository_root/target/release/voxa" ]]; then
  voxa_binary="$repository_root/target/release/voxa"
elif [[ -x "$repository_root/target/debug/voxa" ]]; then
  voxa_binary="$repository_root/target/debug/voxa"
else
  echo "The voxa binary is not installed. Install a release binary or run: cargo install --path crates/voxa-cli --locked" >&2
  exit 2
fi

exec "$voxa_binary" studio "$application_root/graph.json"

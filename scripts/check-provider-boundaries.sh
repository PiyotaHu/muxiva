#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

if find crates -maxdepth 1 -type d -name 'muxiva-provider-*' | grep -q .; then
  echo "Provider crates must not be compiled into the Muxiva framework workspace" >&2
  exit 1
fi

if rg -ni 'qwen|dashscope|agora' \
  Cargo.toml CMakeLists.txt cmake \
  crates/muxiva-core/src crates/muxiva-graph-json/src/builtins.rs; then
  echo "Vendor-specific code leaked into the framework workspace, build, Core, or built-in Nodes" >&2
  exit 1
fi

# Graph parsing and Studio keep explicit read-only aliases for names released
# before the official Node namespaces were simplified. These aliases migrate
# persisted user Graphs; they do not register or implement vendor Nodes.

python3 - <<'PY'
import json
from pathlib import Path

root = Path("examples/voice-agent")
manifests = [json.loads(path.read_text()) for path in Path("providers").rglob("muxiva.node.json")]
for manifest in manifests:
    node_type = manifest["node_type"]
    language = manifest["language"]
    if node_type.startswith("qwen.") and language != "python":
        raise SystemExit(f"{node_type} must be implemented in Python, found {language}")
    if node_type.startswith("agora.") and language != "cpp":
        raise SystemExit(f"{node_type} must be implemented in C++, found {language}")

for path in root.glob(".muxiva/templates/*.json"):
    graph = json.loads(path.read_text())["graph"]
    for node in graph["nodes"]:
        node_type, language = node["node_type"], node["language"]
        if node_type.startswith("qwen.") and language != "python":
            raise SystemExit(f"template {path} couples Qwen to {language}")
        if node_type.startswith("agora.") and language != "cpp":
            raise SystemExit(f"template {path} couples Agora to {language}")
PY

if find providers/transport/agora \
  -type f \( -name '*.rs' -o -name '*.py' -o -name '*.ts' \) | grep -q .; then
  echo "Agora provider implementation must remain C++-only" >&2
  exit 1
fi

echo "Node boundary validation passed: framework is vendor-neutral; Qwen=Python; Agora=C++."

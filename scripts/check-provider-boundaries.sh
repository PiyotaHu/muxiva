#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

if find crates -maxdepth 1 -type d -name 'voxa-provider-*' | grep -q .; then
  echo "Provider crates must not be compiled into the Voxa framework workspace" >&2
  exit 1
fi

if rg -ni 'qwen|dashscope|agora' \
  Cargo.toml CMakeLists.txt cmake \
  crates/voxa-core/src crates/voxa-graph-json/src crates/voxa-studio/src; then
  echo "Vendor-specific code leaked into the framework workspace, build, Core, Graph builtins, or Studio" >&2
  exit 1
fi

python3 - <<'PY'
import json
from pathlib import Path

root = Path("examples/voice-agent")
manifests = [json.loads(path.read_text()) for path in root.glob(".voxa/nodes/*/voxa.node.json")]
for manifest in manifests:
    node_type = manifest["node_type"]
    language = manifest["language"]
    if node_type.startswith("provider.qwen.") and language != "python":
        raise SystemExit(f"{node_type} must be implemented in Python, found {language}")
    if node_type.startswith("provider.agora.") and language != "cpp":
        raise SystemExit(f"{node_type} must be implemented in C++, found {language}")

for path in root.glob(".voxa/templates/*.json"):
    graph = json.loads(path.read_text())["graph"]
    for node in graph["nodes"]:
        node_type, language = node["node_type"], node["language"]
        if node_type.startswith("provider.qwen.") and language != "python":
            raise SystemExit(f"template {path} couples Qwen to {language}")
        if node_type.startswith("provider.agora.") and language != "cpp":
            raise SystemExit(f"template {path} couples Agora to {language}")
PY

if find providers/agora examples/voice-agent/.voxa/nodes/agora_* \
  -type f \( -name '*.rs' -o -name '*.py' -o -name '*.ts' \) | grep -q .; then
  echo "Agora provider implementation must remain C++-only" >&2
  exit 1
fi

echo "Provider boundary validation passed: framework is vendor-neutral; Qwen=Python; Agora=C++."

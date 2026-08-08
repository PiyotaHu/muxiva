#!/usr/bin/env python3
"""Validate Provider and Node manifests as one discoverable catalog."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROVIDERS = ROOT / "providers"
CATEGORIES = {"transport", "algorithm", "media", "control", "utility"}
CAPABILITY = re.compile(r"^[a-z0-9_-]+(?:\.[a-z0-9_-]+)*$")


def fail(message: str) -> None:
    print(f"PROVIDER CATALOG ERROR: {message}", file=sys.stderr)


def nearest_provider(path: Path) -> Path | None:
    for parent in (path.parent, *path.parents):
        candidate = parent / "muxiva.provider.json"
        if candidate.is_file():
            return candidate
        if parent == PROVIDERS:
            break
    return None


def main() -> int:
    errors = 0
    identities: set[tuple[str, str, str]] = set()
    packages: set[str] = set()
    providers: dict[Path, dict] = {}

    for path in sorted(PROVIDERS.rglob("muxiva.provider.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        providers[path] = data
        if data.get("format") != "muxiva.provider/v1":
            fail(f"{path.relative_to(ROOT)} has an unsupported format")
            errors += 1
        if data.get("category") not in CATEGORIES:
            fail(f"{path.relative_to(ROOT)} has an invalid category")
            errors += 1
        connection_ids = [item.get("id") for item in data.get("connections", [])]
        if len(connection_ids) != len(set(connection_ids)):
            fail(f"{path.relative_to(ROOT)} repeats a connection ID")
            errors += 1

    for path in sorted(PROVIDERS.rglob("muxiva.node.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        provider_path = nearest_provider(path)
        if provider_path is None:
            fail(f"{path.relative_to(ROOT)} is not owned by a Provider Manifest")
            errors += 1
            continue
        provider = providers[provider_path]
        if data.get("category") not in CATEGORIES:
            fail(f"{path.relative_to(ROOT)} has an invalid category")
            errors += 1
        if not CAPABILITY.fullmatch(data.get("capability", "")):
            fail(f"{path.relative_to(ROOT)} has an invalid capability")
            errors += 1
        connection_id = data.get("connection_id")
        declared = {item.get("id") for item in provider.get("connections", [])}
        if connection_id and connection_id not in declared:
            fail(f"{path.relative_to(ROOT)} references unknown connection {connection_id}")
            errors += 1
        identity = (data.get("node_type", ""), data.get("language", ""), data.get("factory_version", ""))
        if identity in identities:
            fail(f"duplicate Factory identity {identity}")
            errors += 1
        identities.add(identity)
        package_id = data.get("package_id", "")
        if package_id in packages:
            fail(f"duplicate package_id {package_id}")
            errors += 1
        packages.add(package_id)
        for port in data.get("ports", []):
            if not isinstance(port.get("schema", {}), dict):
                fail(f"{path.relative_to(ROOT)} port {port.get('name')} schema is not an object")
                errors += 1

    if errors:
        print(f"Provider catalog validation failed with {errors} error(s).", file=sys.stderr)
        return 1
    print(f"Provider catalog validation passed: {len(providers)} providers, {len(identities)} Nodes.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

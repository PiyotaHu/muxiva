#!/usr/bin/env python3
"""Validate release names and require explicitly confirmed publishing owners."""

from __future__ import annotations

import argparse
import json
import pathlib
import sys


ROOT = pathlib.Path(__file__).resolve().parents[2]
IDENTITY_PATH = ROOT / "release" / "identity.json"


def load_json(path: pathlib.Path) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read {path.relative_to(ROOT)}: {error}") from error


def metadata_errors(identity: dict) -> list[str]:
    errors: list[str] = []
    if identity.get("schema_version") != 1:
        errors.append("release/identity.json schema_version must be 1")

    repository = identity.get("github", {}).get("repository")
    pypi_project = identity.get("pypi", {}).get("project")
    npm_scope = identity.get("npm", {}).get("scope")
    tap_repository = identity.get("homebrew", {}).get("tap_repository")

    if not isinstance(repository, str) or repository.count("/") != 1:
        errors.append("github.repository must be an owner/repository name")
    if not isinstance(tap_repository, str) or tap_repository.count("/") != 1:
        errors.append("homebrew.tap_repository must be an owner/repository name")
    if not isinstance(npm_scope, str) or not npm_scope.startswith("@"):
        errors.append("npm.scope must start with @")

    pyproject = (ROOT / "crates" / "muxiva-python" / "pyproject.toml").read_text(
        encoding="utf-8"
    )
    if f'name = "{pypi_project}"' not in pyproject:
        errors.append(
            f"Python project metadata does not declare release identity {pypi_project!r}"
        )

    for package_path in sorted((ROOT / "bindings").glob("*/package.json")):
        package = load_json(package_path)
        name = package.get("name", "")
        if not name.startswith(f"{npm_scope}/"):
            errors.append(
                f"{package_path.relative_to(ROOT)} package {name!r} is outside {npm_scope}"
            )
        repository_url = package.get("repository", {}).get("url", "")
        if repository and repository not in repository_url:
            errors.append(
                f"{package_path.relative_to(ROOT)} repository does not match {repository}"
            )
    return errors


CHANNELS = {
    "cli": ("github", "homebrew"),
    "python": ("github", "pypi"),
    "npm": ("github", "npm"),
    "all": ("github", "homebrew", "pypi", "npm"),
}


def ownership_errors(identity: dict, channel: str) -> list[str]:
    errors = []
    for publisher in CHANNELS[channel]:
        record = identity.get(publisher, {})
        if record.get("confirmed") is not True:
            evidence = record.get("evidence", "no evidence recorded")
            errors.append(f"{publisher} ownership is unconfirmed: {evidence}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--channel",
        choices=sorted(CHANNELS),
        help="also require confirmed ownership for this publishing channel",
    )
    args = parser.parse_args()

    identity = load_json(IDENTITY_PATH)
    errors = metadata_errors(identity)
    if args.channel:
        errors.extend(ownership_errors(identity, args.channel))

    if errors:
        for error in errors:
            print(f"release preflight: ERROR: {error}", file=sys.stderr)
        return 1

    suffix = f" and {args.channel} ownership" if args.channel else ""
    print(f"release preflight: metadata{suffix} verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

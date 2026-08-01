#!/usr/bin/env python3
"""Validate public documentation language isolation and page parity."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs" / "site"
CJK = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]")


def markdown_files(locale: str) -> set[Path]:
    base = DOCS / locale
    return {path.relative_to(base) for path in base.rglob("*.md")}


def fail(message: str) -> None:
    print(f"DOCS I18N ERROR: {message}", file=sys.stderr)


def main() -> int:
    english = markdown_files("en")
    chinese = markdown_files("zh")
    errors = 0

    for missing in sorted(english - chinese):
        fail(f"missing Chinese page for {missing}")
        errors += 1
    for missing in sorted(chinese - english):
        fail(f"missing English page for {missing}")
        errors += 1

    for relative in sorted(english):
        source = DOCS / "en" / relative
        for line_number, line in enumerate(source.read_text(encoding="utf-8").splitlines(), 1):
            if CJK.search(line):
                fail(f"Chinese text in English source {relative}:{line_number}")
                errors += 1

    for relative in sorted(chinese):
        source = DOCS / "zh" / relative
        if not CJK.search(source.read_text(encoding="utf-8")):
            fail(f"Chinese source contains no Chinese prose: {relative}")
            errors += 1

    if errors:
        print(f"Documentation i18n validation failed with {errors} error(s).", file=sys.stderr)
        return 1

    print(f"Documentation i18n validation passed: {len(english)} paired pages.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

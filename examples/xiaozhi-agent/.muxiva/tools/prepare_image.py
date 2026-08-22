#!/usr/bin/env python3
"""Download and dither an Agent-generated image for the 400x300 RLCD."""

from __future__ import annotations

import io
import pathlib
import sys
import urllib.request

from PIL import Image, ImageEnhance, ImageOps


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: prepare_image.py URL OUTPUT")
    url, output_name = sys.argv[1:]
    request = urllib.request.Request(
        url, headers={"User-Agent": "Muxiva-ESP32-Image/1.0"}
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        payload = response.read(12 * 1024 * 1024 + 1)
    if len(payload) > 12 * 1024 * 1024:
        raise RuntimeError("generated image exceeds 12 MiB")
    with Image.open(io.BytesIO(payload)) as source:
        image = ImageOps.exif_transpose(source).convert("RGB")
        image = ImageOps.fit(image, (400, 300), method=Image.Resampling.LANCZOS)
        image = ImageEnhance.Contrast(image.convert("L")).enhance(1.35)
        image = image.convert("1", dither=Image.Dither.FLOYDSTEINBERG)
        output = pathlib.Path(output_name)
        output.parent.mkdir(parents=True, exist_ok=True)
        temporary = output.with_suffix(".tmp.png")
        image.save(temporary, format="PNG", optimize=True)
        temporary.replace(output)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Build a compact 400x300 monochrome contact sheet for saved artworks."""

from __future__ import annotations

import json
import pathlib
import sys

from PIL import Image, ImageDraw, ImageOps


def main() -> None:
    if len(sys.argv) != 4:
        raise SystemExit("usage: build_gallery.py GALLERY_JSON IMAGE_DIR OUTPUT")
    gallery_path = pathlib.Path(sys.argv[1])
    image_dir = pathlib.Path(sys.argv[2])
    output_path = pathlib.Path(sys.argv[3])
    entries = (
        json.loads(gallery_path.read_text(encoding="utf-8"))
        if gallery_path.exists()
        else []
    )
    entries = entries[-6:]

    canvas = Image.new("L", (400, 300), 255)
    draw = ImageDraw.Draw(canvas)
    draw.text((8, 5), "MY ARTWORKS", fill=0)
    draw.line((8, 20, 392, 20), fill=0, width=2)
    for slot, entry in enumerate(entries):
        column, row = slot % 3, slot // 3
        x, y = 7 + column * 131, 27 + row * 134
        draw.rectangle((x, y, x + 122, y + 124), outline=0, width=2)
        path = image_dir / str(entry.get("file", ""))
        if path.is_file():
            with Image.open(path) as source:
                preview = ImageOps.fit(
                    source.convert("L"), (116, 101), method=Image.Resampling.LANCZOS
                )
                preview = preview.point(
                    lambda value: 255 if value >= 160 else 0, mode="1"
                )
                canvas.paste(preview, (x + 3, y + 3))
        number = entry.get("number", slot + 1)
        draw.rectangle((x + 3, y + 106, x + 119, y + 121), fill=255)
        draw.text((x + 7, y + 107), f"#{number}", fill=0)

    if not entries:
        draw.text((145, 140), "NO ARTWORKS", fill=0)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    canvas.convert("1").save(output_path, format="PNG", optimize=True)


if __name__ == "__main__":
    main()

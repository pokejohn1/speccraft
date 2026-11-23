#!/usr/bin/env python3
"""Draws the Speccraft app icon (original artwork, MIT OR Apache-2.0).

A spec page on a deep blue tile: numbered outline lines with the sequence-number boxes of the
editing view (filled = switched on), and a green margin bar for a rule-linked paragraph.
Writes assets/app-icon/speccraft-*.png and speccraft.ico. Needs Pillow.
"""
import sys
from pathlib import Path
from PIL import Image, ImageDraw

S = 1024  # drawn at 1024 and scaled down


def draw(size: int) -> Image.Image:
    k = 4  # supersampling
    W = S * k
    im = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    u = lambda v: int(v * k)
    d.rounded_rectangle([0, 0, W - 1, W - 1], radius=u(224), fill=(24, 62, 112, 255))
    # The page, with a folded corner.
    page = [(u(232), u(148)), (u(676), u(148)), (u(800), u(272)), (u(800), u(876)), (u(232), u(876))]
    d.polygon(page, fill=(246, 243, 236, 255))
    d.polygon([(u(676), u(148)), (u(676), u(272)), (u(800), u(272))], fill=(205, 214, 228, 255))
    # Outline rows: (indent, line length, box on?, green bar?)
    rows = [(0, 380, True, False), (48, 300, True, True), (96, 236, False, False), (48, 300, True, False), (96, 236, True, False)]
    y = 322
    for indent, length, on, bar in rows:
        x = 300 + indent
        box = [u(x), u(y - 26), u(x + 56), u(y + 26)]
        if on:
            d.rounded_rectangle(box, radius=u(10), fill=(43, 108, 210, 255))
        else:
            d.rounded_rectangle(box, radius=u(10), outline=(150, 160, 175, 255), width=u(8))
        if bar:
            d.rectangle([u(x + 64), u(y - 26), u(x + 74), u(y + 26)], fill=(46, 160, 90, 255))
        d.rounded_rectangle([u(x + 92), u(y - 12), u(x + 92 + length), u(y + 12)], radius=u(12), fill=(40, 48, 62, 255))
        y += 112
    return im.resize((size, size), Image.LANCZOS)


def main() -> None:
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "assets/app-icon")
    out.mkdir(parents=True, exist_ok=True)
    for n in (1024, 512, 256, 128):
        draw(n).save(out / f"speccraft-{n}.png")
    draw(256).save(out / "speccraft.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])


if __name__ == "__main__":
    main()

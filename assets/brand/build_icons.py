#!/usr/bin/env python3
"""Rebuild cutemarkdown icon assets from the SVG masters.

    pip install cairosvg pillow
    python assets/brand/build_icons.py

Outputs (next to this script):
    png/logo-<size>.png   16, 24, 32 (logo-small.svg) and 48, 64, 128, 256, 512 (logo.svg)
    app.ico               16, 20, 24, 32, 40, 48, 64, 128, 256
                          (<=32 from logo-small.svg; 32-bit BMP entries, PNG for 256)
    preview.png           16/24/32/48/256 on white and on #1b1b1f
"""
import io
import struct
from pathlib import Path

import cairosvg
from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
MASTER = (HERE / "logo.svg").read_bytes()
SMALL = (HERE / "logo-small.svg").read_bytes()

SMALL_MAX = 32  # sizes <= this use the small-size variant
PNG_SIZES = [16, 24, 32, 48, 64, 128, 256, 512]
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]


def render(size: int) -> Image.Image:
    svg = SMALL if size <= SMALL_MAX else MASTER
    png = cairosvg.svg2png(bytestring=svg, output_width=size, output_height=size)
    return Image.open(io.BytesIO(png)).convert("RGBA")


def bmp_entry(im: Image.Image) -> bytes:
    """32-bit BGRA DIB with an all-zero AND mask, as Windows expects in an .ico."""
    w, h = im.size
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    xor = im.transpose(Image.FLIP_TOP_BOTTOM).tobytes("raw", "BGRA")
    mask_row = ((w + 31) // 32) * 4
    return header + xor + bytes(mask_row * h)


def png_entry(im: Image.Image) -> bytes:
    buf = io.BytesIO()
    im.save(buf, format="PNG", optimize=True)
    return buf.getvalue()


def write_ico(path: Path, sizes) -> None:
    blobs = [(s, png_entry(render(s)) if s >= 256 else bmp_entry(render(s))) for s in sizes]
    offset = 6 + 16 * len(blobs)
    out = struct.pack("<HHH", 0, 1, len(blobs))
    for s, data in blobs:
        dim = 0 if s >= 256 else s  # 0 means 256 in the ICONDIRENTRY
        out += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    path.write_bytes(out + b"".join(d for _, d in blobs))


def write_preview(path: Path, sizes=(16, 24, 32, 48, 256)) -> None:
    pad, label_h = 28, 22
    panel_w = sum(sizes) + pad * (len(sizes) + 1)
    panel_h = max(sizes) + pad * 2 + label_h
    sheet = Image.new("RGBA", (panel_w, panel_h * 2), (255, 255, 255, 255))
    draw = ImageDraw.Draw(sheet)
    for row, (bg, ink) in enumerate((((255, 255, 255, 255), (110, 104, 120)),
                                     ((0x1B, 0x1B, 0x1F, 255), (160, 154, 170)))):
        y0 = row * panel_h
        draw.rectangle([0, y0, panel_w, y0 + panel_h], fill=bg)
        x = pad
        for s in sizes:
            top = y0 + pad + max(sizes) - s
            sheet.alpha_composite(render(s), (x, top))
            label = f"{s}px"
            tw = draw.textlength(label)
            draw.text((x + (s - tw) / 2, y0 + pad + max(sizes) + 8), label, fill=ink)
            x += s + pad
    sheet.convert("RGB").save(path, optimize=True)


def main() -> None:
    out = HERE / "png"
    out.mkdir(exist_ok=True)
    for s in PNG_SIZES:
        render(s).save(out / f"logo-{s}.png", optimize=True)
    write_ico(HERE / "app.ico", ICO_SIZES)
    write_preview(HERE / "preview.png")
    print(f"wrote {len(PNG_SIZES)} PNGs, app.ico ({len(ICO_SIZES)} sizes), preview.png")


if __name__ == "__main__":
    main()

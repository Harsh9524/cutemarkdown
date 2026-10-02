#!/usr/bin/env python3
"""Generate the installer artwork from the brand SVGs.

    pip install cairosvg pillow
    python installer/make_images.py

Outputs (next to this script, committed so CI does not need Python):
    welcome.bmp   164x314  Welcome / Finish page side image (MUI_WELCOMEFINISHPAGE_BITMAP)
    header.bmp    150x57   page header image (MUI_HEADERIMAGE_BITMAP)

Both are plain 24-bit uncompressed BMPs, which is what NSIS wants. Everything is
drawn at 4x and downsampled for clean edges. Only assets/brand/logo.svg is used
as input, plus one system font for the wordmark (Segoe UI / DejaVu Sans / Liberation
Sans, whichever exists); if no font is found the wordmark is skipped.
"""
import io
import math
from pathlib import Path

import cairosvg
from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = Path(__file__).resolve().parent
LOGO = HERE.parent / "assets" / "brand" / "logo.svg"

SS = 4  # supersampling factor

# Brand palette (assets/brand/README.md)
ROSE = (0xE8, 0x55, 0x8C)
VIOLET = (0x6F, 0x5F, 0xEA)
PLUM = (0x3A, 0x12, 0x68)
BLUSH = (0xFF, 0xF3, 0xF9)
LAVENDER = (0xE4, 0xDC, 0xFF)

FONT_CANDIDATES = [
    "C:/Windows/Fonts/segoeuib.ttf",
    "C:/Windows/Fonts/seguisb.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
    "/Library/Fonts/Arial Bold.ttf",
]
FONT_REGULAR_CANDIDATES = [
    "C:/Windows/Fonts/segoeui.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/Library/Fonts/Arial.ttf",
]


def font(candidates, px):
    for c in candidates:
        if Path(c).exists():
            return ImageFont.truetype(c, px)
    return None


def logo(px: int) -> Image.Image:
    png = cairosvg.svg2png(url=str(LOGO), output_width=px, output_height=px)
    return Image.open(io.BytesIO(png)).convert("RGBA")


def diagonal_gradient(w, h, c0, c1) -> Image.Image:
    """Top-left c0 -> bottom-right c1."""
    g = Image.linear_gradient("L")  # 256x256, black (top) -> white (bottom)
    # Diagonal mask = blend of a left->right ramp and a top->bottom ramp (mostly vertical).
    horiz = g.rotate(90).resize((w, h))  # counter-clockwise: black left -> white right
    vert = g.resize((w, h))
    mask = Image.blend(horiz, vert, 0.75)
    return Image.composite(Image.new("RGB", (w, h), c1), Image.new("RGB", (w, h), c0), mask)


def soft_circle(canvas: Image.Image, cx, cy, r, color, alpha):
    layer = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    ImageDraw.Draw(layer).ellipse([cx - r, cy - r, cx + r, cy + r], fill=color + (int(255 * alpha),))
    canvas.alpha_composite(layer)


def shadowed(canvas: Image.Image, im: Image.Image, x, y, dy, blur, alpha):
    """Paste `im` at (x, y) with a soft plum drop shadow."""
    shadow = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    a = im.getchannel("A").point(lambda v: int(v * alpha))
    tint = Image.new("RGBA", im.size, PLUM + (255,))
    tint.putalpha(a)
    shadow.alpha_composite(tint, (x, y + dy))
    canvas.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(blur)))
    canvas.alpha_composite(im, (x, y))


def centered_text(draw, cx, y, text, fnt, fill, tracking=0.0):
    if fnt is None:
        return
    widths = [draw.textlength(ch, font=fnt) for ch in text]
    total = sum(widths) + tracking * (len(text) - 1)
    x = cx - total / 2
    for ch, w in zip(text, widths):
        draw.text((x, y), ch, font=fnt, fill=fill)
        x += w + tracking


def welcome() -> Image.Image:
    W, H = 164, 314
    w, h = W * SS, H * SS
    canvas = diagonal_gradient(w, h, BLUSH, LAVENDER).convert("RGBA")

    # soft decorative blobs
    soft_circle(canvas, 150 * SS, 22 * SS, 78 * SS, ROSE, 0.13)
    soft_circle(canvas, 12 * SS, 296 * SS, 96 * SS, VIOLET, 0.13)
    soft_circle(canvas, 128 * SS, 250 * SS, 26 * SS, ROSE, 0.10)
    soft_circle(canvas, 26 * SS, 140 * SS, 9 * SS, VIOLET, 0.12)

    # logo
    size = 104
    x = (W - size) // 2 * SS
    y = 68 * SS
    shadowed(canvas, logo(size * SS), x, y, dy=5 * SS, blur=7 * SS, alpha=0.30)

    # wordmark + tagline
    d = ImageDraw.Draw(canvas)
    centered_text(d, w / 2, 194 * SS, "cutemarkdown", font(FONT_CANDIDATES, 15 * SS), PLUM)
    centered_text(
        d, w / 2, 218 * SS, "a cute Markdown reader", font(FONT_REGULAR_CANDIDATES, 9 * SS),
        (0x6B, 0x5B, 0x9A),
    )

    # a tiny row of dots as a divider
    for i, c in enumerate((ROSE, (0xAC, 0x5A, 0xBB), VIOLET)):
        cx = (W / 2 + (i - 1) * 9) * SS
        cy = 246 * SS
        r = 2.2 * SS
        d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=c)

    return canvas.convert("RGB").resize((W, H), Image.LANCZOS)


def header() -> Image.Image:
    W, H = 150, 57
    w, h = W * SS, H * SS
    canvas = Image.new("RGBA", (w, h), (255, 255, 255, 255))  # MUI header background is white
    size = 42
    x = (W - size - 12) * SS
    y = (H - size) // 2 * SS
    shadowed(canvas, logo(size * SS), x, y, dy=2 * SS, blur=3 * SS, alpha=0.22)
    return canvas.convert("RGB").resize((W, H), Image.LANCZOS)


def save_bmp(im: Image.Image, name: str):
    path = HERE / name
    im.save(path, format="BMP")  # RGB -> 24-bit uncompressed
    print(f"wrote {path.relative_to(HERE.parent)} {im.size[0]}x{im.size[1]}")


def main():
    save_bmp(welcome(), "welcome.bmp")
    save_bmp(header(), "header.bmp")


if __name__ == "__main__":
    main()

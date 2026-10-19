"""Draws the Nyra mark (assets/logo.svg: a rounded square with a violet-to-cyan gradient and a white N)
as assets/nyra.ico (16, 24, 32, 48, 64, 128, 256 px; tools/windows/install-file-icon.ps1 installs it) and
editors/vscode/icons/nyra-128.png (the VS Code extension's icon).

    python tools/gen_icon.py

Needs Pillow. Each size is drawn on its own at 8x and scaled down, with the stroke and the corner radius
tuned per size so the N stays crisp at 16 px (the SVG's proportions would make a 1.7 px stroke there).
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
VIOLET = (0x7C, 0x5C, 0xFF)
CYAN = (0x22, 0xB8, 0xE8)
SIZES = [16, 24, 32, 48, 64, 128, 256]
SS = 8  # supersampling factor
# hand-placed N for the sizes where a fraction of a pixel shows: (left stem x, right stem x, top y, bottom y, stroke),
# in output pixels; an even stroke centers on a pixel edge, an odd one on a pixel center
PIXEL_N = {16: (5, 11, 4.5, 11.5, 2), 24: (7.5, 16.5, 7, 17, 3), 32: (10, 22, 9, 23, 4)}


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def mark(size: int) -> Image.Image:
    big = size * SS
    # proportions of the SVG mark (96 units: rx 26, N from 30 to 66, stroke 10), adjusted for small sizes
    if size <= 16:
        margin, radius, stroke, inset = 0.0, 0.25, 0.17, 0.27
    elif size <= 24:
        margin, radius, stroke, inset = 0.0, 0.25, 0.15, 0.285
    elif size <= 48:
        margin, radius, stroke, inset = (0.0 if size <= 32 else 1 / size), 0.26, 0.13, 0.30
    else:
        margin, radius, stroke, inset = 0.03, 26 / 96, 10 / 96, 30 / 96

    # diagonal gradient, top-left violet to bottom-right cyan
    n = 256  # a linear gradient scales without loss
    grad = Image.new("RGB", (n, n))
    px = grad.load()
    for y in range(n):
        for x in range(n):
            px[x, y] = lerp(VIOLET, CYAN, (x + y) / (2 * (n - 1)))
    grad = grad.resize((big, big), Image.BILINEAR)

    m = round(margin * big)
    side = big - 2 * m
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle([m, m, m + side - 1, m + side - 1], radius=round(radius * side), fill=255)

    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    img.paste(grad, (0, 0), mask)

    # the N: M30 68 V28 L66 68 V28 in a 96 box, round caps and joins
    if size in PIXEL_N:
        # small sizes: the strokes sit on whole pixels (x of the two stem centers, top and bottom, stroke width)
        xl, xr, top, bottom, wpx = (v * SS for v in PIXEL_N[size])
        w = round(wpx)
    else:
        w = max(SS, round(stroke * side))
        xl = m + inset * side + w / 2
        xr = m + side - inset * side - w / 2
        top = m + (28 / 96) * side
        bottom = m + (68 / 96) * side
    pts = [(xl, bottom), (xl, top), (xr, bottom), (xr, top)]
    d = ImageDraw.Draw(img)
    d.line(pts, fill=(255, 255, 255, 255), width=w, joint="curve")
    r = w / 2
    for x, y in (pts[0], pts[3]):
        d.ellipse([x - r, y - r, x + r, y + r], fill=(255, 255, 255, 255))
    # the inner joints: round as well
    for x, y in (pts[1], pts[2]):
        d.ellipse([x - r, y - r, x + r, y + r], fill=(255, 255, 255, 255))
    # keep the N inside the rounded square
    alpha = img.getchannel("A")
    img.putalpha(Image.composite(alpha, Image.new("L", (big, big), 0), mask))
    return img.resize((size, size), Image.LANCZOS)


def main():
    images = {s: mark(s) for s in SIZES}
    assets = ROOT / "assets"
    images[256].save(assets / "nyra.ico", format="ICO", sizes=[(s, s) for s in SIZES], append_images=[images[s] for s in SIZES[:-1]])
    vs = ROOT / "editors" / "vscode" / "icons"
    vs.mkdir(exist_ok=True)
    images[128].save(vs / "nyra-128.png")  # the extension's own icon; its file icon is icons/nyra.svg
    return images


if __name__ == "__main__":
    main()

"""Writes the WebP lossless (VP8L) test fixtures of src/web/img/vp8l_dec.rs.

Each fixture is a tiny image defined by a formula (the Rust tests compute the same formula and
compare every pixel), saved by Pillow's libwebp with `lossless=True, exact=True` at several
methods, so the decoder sees the transforms, color cache, LZ77 and prefix codes libwebp makes.
`vp8l-alph.webp` is a lossy image whose alpha is an ALPH chunk compressed with VP8L.

Run from the repository root: python tools/img_fixtures_vp8l.py
"""

import os

from PIL import Image

OUT = os.path.join("src", "web", "img", "testdata")


def lcg_bytes(n):
    s = 12345
    out = []
    for _ in range(n):
        s = (s * 1103515245 + 12345) % (1 << 31)
        out.append((s >> 16) & 255)
    return out


def pixel(name, x, y):
    if name == "grad":
        return (x * 7 % 256, y * 11 % 256, (x + y) * 3 % 256, 255)
    if name == "alpha":
        return (x * 12 % 256, y * 13 % 256, 128, x * y * 5 % 256)
    if name == "pal12":
        i = (x // 3 + (y // 2) * 5) % 12
        return (i * 20, 255 - i * 15, i * i * 7 % 256, 255)
    if name == "pal200":
        i = (x * 7 + y * 13) % 200
        return (i, i * 3 % 256, 255 - i, 255 if i % 5 else 128)
    if name == "pal2":
        return (255, 0, 0, 255) if (x + y) % 3 == 0 else (0, 0, 255, 255)
    if name == "one":
        return (10, 20, 30, 40)
    if name == "tall":
        return (x * 80 % 256, y * 30 % 256, x * y * 10 % 256, 255)
    if name == "rep":
        return ((x // 10) * 25 % 256, (x % 10) * 20, y * 60 % 256, 255)
    if name == "alph":
        return (x * 9 % 256, 100, y * 15 % 256, (x * 10 + y * 7) % 256)
    raise ValueError(name)


SIZES = {
    "grad": (37, 23), "alpha": (21, 19), "pal12": (33, 17), "pal200": (40, 30), "pal2": (29, 5),
    "noise": (16, 16), "one": (1, 1), "tall": (3, 7), "rep": (300, 4), "alph": (24, 16),
}


def image(name):
    w, h = SIZES[name]
    img = Image.new("RGBA", (w, h))
    if name == "noise":
        b = lcg_bytes(w * h * 4)
        img.putdata([tuple(b[i * 4:i * 4 + 4]) for i in range(w * h)])
    else:
        img.putdata([pixel(name, x, y) for y in range(h) for x in range(w)])
    return img


# (name, method, quality)
LOSSLESS = [
    ("grad", 0, 0), ("grad", 4, 75), ("grad", 6, 100), ("alpha", 4, 75), ("alpha", 6, 100),
    ("pal12", 4, 75), ("pal200", 6, 100), ("pal2", 4, 75), ("noise", 4, 75), ("one", 4, 75),
    ("tall", 6, 100), ("rep", 6, 100),
]


def main():
    os.makedirs(OUT, exist_ok=True)
    total = 0
    for name, method, quality in LOSSLESS:
        path = os.path.join(OUT, f"vp8l-{name}-m{method}.webp")
        image(name).save(path, lossless=True, exact=True, method=method, quality=quality)
        total += os.path.getsize(path)
    path = os.path.join(OUT, "vp8l-alph.webp")
    image("alph").save(path, lossless=False, quality=50, alpha_quality=100, method=4)
    total += os.path.getsize(path)
    print(f"wrote {len(LOSSLESS) + 1} fixtures, {total} bytes")


if __name__ == "__main__":
    main()

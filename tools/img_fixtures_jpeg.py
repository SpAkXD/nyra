"""Make the JPEG fixtures of the image codec's tests (src/web/img/testdata/jpeg-*).

    python tools/img_fixtures_jpeg.py

Each fixture is a small JPEG and, next to it, the RGB that Pillow (libjpeg-turbo) decodes from it
(`.rgb`: width * height * 3 bytes, the size is the JPEG's). The decoder's tests compare with those.
Most fixtures are written by Pillow. The sampling layouts and scan structures Pillow cannot
write (4:4:0, 4:1:1, a factor of 3, one scan per component, a restart interval with separate
scans) come from the small baseline encoder below; Pillow still decodes them for the expected
pixels. Needs Pillow and numpy. Deterministic: the same files on every run.
"""
import io
import struct
from pathlib import Path

import numpy as np
from PIL import Image, ImageCms, ImageOps

OUT = Path(__file__).resolve().parent.parent / "src" / "web" / "img" / "testdata"


def picture(w, h, seed=1):
    """Gradients, a disc, a bar and a little noise: smooth parts and edges."""
    rng = np.random.default_rng(seed)
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    r = 255 * x / max(w - 1, 1)
    g = 255 * y / max(h - 1, 1)
    b = 128 + 100 * np.sin(x / 3.0 + y / 5.0)
    disc = (x - w * 0.6) ** 2 + (y - h * 0.4) ** 2 < (min(w, h) * 0.3) ** 2
    r[disc], g[disc], b[disc] = 250, 40, 60
    bar = (x > w * 0.15) & (x < w * 0.3)
    r[bar], g[bar], b[bar] = 20, 30, 200
    px = np.stack([r, g, b], -1) + rng.normal(0, 6, (h, w, 3))
    return Image.fromarray(np.clip(px, 0, 255).astype(np.uint8), "RGB")


def save(name, data, expected):
    (OUT / f"{name}.jpg").write_bytes(data)
    (OUT / f"{name}.rgb").write_bytes(expected.convert("RGB").tobytes())


def pillow(name, img, **opts):
    buf = io.BytesIO()
    img.save(buf, "JPEG", optimize=True, **opts)
    data = buf.getvalue()
    save(name, data, Image.open(io.BytesIO(data)))


# ---------------------------------------------------------------------------------------------
# A small baseline encoder for layouts Pillow cannot make. Codes are fixed-length per table
# (valid, not small), so the files only test decoding.

ZIGZAG = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
]
QT = np.array([
    [16, 11, 10, 16, 24, 40, 51, 61], [12, 12, 14, 19, 26, 58, 60, 55], [14, 13, 16, 24, 40, 57, 69, 56],
    [14, 17, 22, 29, 51, 87, 80, 62], [18, 22, 37, 56, 68, 109, 103, 77], [24, 35, 55, 64, 81, 104, 113, 92],
    [49, 64, 78, 87, 103, 121, 120, 101], [72, 92, 95, 98, 112, 100, 103, 99],
]) // 2 + 1
C = np.array([[(np.sqrt(0.5) if u == 0 else 1.0) / 2 * np.cos((2 * x + 1) * u * np.pi / 16) for x in range(8)] for u in range(8)])


def category(v):
    return int(abs(v)).bit_length()


def tiny_encoder(img, samp, separate=False, restart=0):
    """A baseline JPEG of `img` with sampling factors `samp` (one (h, v) per component), all in one
    scan or one scan per component, with an optional restart interval (in MCUs)."""
    a = np.asarray(img, dtype=np.float64)
    h, w = a.shape[:2]
    r, g, b = a[..., 0], a[..., 1], a[..., 2]
    ycc = [0.299 * r + 0.587 * g + 0.114 * b, -0.168736 * r - 0.331264 * g + 0.5 * b + 128, 0.5 * r - 0.418688 * g - 0.081312 * b + 128]
    hmax, vmax = max(s[0] for s in samp), max(s[1] for s in samp)
    mcux, mcuy = -(-w // (8 * hmax)), -(-h // (8 * vmax))
    full = np.pad(np.stack(ycc), ((0, 0), (0, mcuy * 8 * vmax - h), (0, mcux * 8 * hmax - w)), mode="edge")
    planes = []
    for c, (sh, sv) in enumerate(samp):
        fx, fy = hmax // sh, vmax // sv
        p = full[c].reshape(full.shape[1] // fy, fy, full.shape[2] // fx, fx).mean(axis=(1, 3))
        blocks = {}
        for by in range(p.shape[0] // 8):
            for bx in range(p.shape[1] // 8):
                d = C @ (p[by * 8:by * 8 + 8, bx * 8:bx * 8 + 8] - 128) @ C.T
                blocks[bx, by] = [int(v) for v in np.round(d / QT).astype(int).flatten()]
        planes.append(blocks)

    def block_symbols(blk, pred):
        out = [("dc", category(blk[0] - pred), blk[0] - pred)]
        run = 0
        for k in range(1, 64):
            v = blk[ZIGZAG[k]]
            if v == 0:
                run += 1
                continue
            while run > 15:
                out.append(("ac", 0xF0, 0))
                run -= 16
            out.append(("ac", run << 4 | category(v), v))
            run = 0
        if run:
            out.append(("ac", 0, 0))
        return out

    # the scans: lists of MCUs, each a list of (component, block)
    scans = []
    comps = [[c] for c in range(3)] if separate else [[0, 1, 2]]
    for sc in comps:
        mcus = []
        if len(sc) == 1:
            c = sc[0]
            sh, sv = samp[c]
            cw, ch = -(-w * sh // hmax), -(-h * sv // vmax)
            for by in range(-(-ch // 8)):
                for bx in range(-(-cw // 8)):
                    mcus.append([(c, (bx, by))])
        else:
            for my in range(mcuy):
                for mx in range(mcux):
                    mcus.append([(c, (mx * samp[c][0] + x, my * samp[c][1] + y)) for c in sc for y in range(samp[c][1]) for x in range(samp[c][0])])
        scans.append((sc, mcus))

    # symbols per scan, restart intervals marked
    coded = []
    for sc, mcus in scans:
        items = []
        pred = [0, 0, 0]
        for i, mcu in enumerate(mcus):
            if restart and i and i % restart == 0:
                items.append(("rst", (i // restart - 1) % 8))
                pred = [0, 0, 0]
            for c, pos in mcu:
                blk = planes[c][pos]
                items += [(kind, sym, v, c) for kind, sym, v in block_symbols(blk, pred[c])]
                pred[c] = blk[0]
        coded.append((sc, items))

    tables = {}
    for _, items in coded:
        for it in items:
            if it[0] != "rst":
                tables.setdefault((it[0], min(it[3], 1)), set()).add(it[1])
    codes = {}
    dht = b""
    for (kind, t), syms in sorted(tables.items()):
        syms = sorted(syms)
        n = max(1, (len(syms) + 1 - 1).bit_length())
        while (1 << n) - 1 < len(syms):
            n += 1
        counts = [0] * 16
        counts[n - 1] = len(syms)
        codes[kind, t] = {s: (i, n) for i, s in enumerate(syms)}
        dht += bytes([(kind == "ac") << 4 | t]) + bytes(counts) + bytes(syms)

    def seg(m, payload):
        return bytes([0xFF, m]) + struct.pack(">H", len(payload) + 2) + payload

    q = bytes([0] + [int(QT.flatten()[z]) for z in ZIGZAG])
    q += bytes([1] + [int(QT.flatten()[z]) for z in ZIGZAG])
    sof = struct.pack(">BHHB", 8, h, w, 3) + b"".join(bytes([c + 1, sh << 4 | sv, min(c, 1)]) for c, (sh, sv) in enumerate(samp))
    out = b"\xff\xd8" + seg(0xE0, b"JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00") + seg(0xDB, q) + seg(0xC0, sof) + seg(0xC4, dht)
    if restart:
        out += seg(0xDD, struct.pack(">H", restart))
    for sc, items in coded:
        out += seg(0xDA, bytes([len(sc)]) + b"".join(bytes([c + 1, min(c, 1) << 4 | min(c, 1)]) for c in sc) + bytes([0, 63, 0]))
        bits = ""
        data = b""

        def flush(bits):
            bits += "1" * (-len(bits) % 8)
            b = bytes(int(bits[i:i + 8], 2) for i in range(0, len(bits), 8))
            return b.replace(b"\xff", b"\xff\x00")

        for it in items:
            if it[0] == "rst":
                data += flush(bits) + bytes([0xFF, 0xD0 + it[1]])
                bits = ""
                continue
            kind, sym, v, c = it
            code, n = codes[kind, min(c, 1)][sym]
            bits += format(code, f"0{n}b")
            size = sym & 15
            if size:
                bits += format(v if v > 0 else v + (1 << size) - 1, f"0{size}b")
        out += data + flush(bits)
    return out + b"\xff\xd9"


def tiny(name, img, samp, **kw):
    data = tiny_encoder(img, samp, **kw)
    save(name, data, Image.open(io.BytesIO(data)))


def exif_with_orientation(o):
    ex = Image.Exif()
    ex[274] = o
    return ex.tobytes()


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("jpeg-*"):
        old.unlink()
    small, wide = picture(17, 11), picture(21, 17, seed=2)
    pillow("jpeg-444", small, quality=80, subsampling=0)
    pillow("jpeg-422", wide, quality=80, subsampling=1)
    pillow("jpeg-420", wide, quality=80, subsampling=2)
    pillow("jpeg-prog", wide, quality=80, subsampling=2, progressive=True)
    pillow("jpeg-prog444", small, quality=90, subsampling=0, progressive=True)
    pillow("jpeg-rst", wide, quality=80, subsampling=2, restart_marker_blocks=1)
    pillow("jpeg-prog-rst", wide, quality=80, subsampling=2, progressive=True, restart_marker_rows=1)
    pillow("jpeg-gray", small.convert("L"), quality=80)
    pillow("jpeg-gray-prog", wide.convert("L"), quality=80, progressive=True)
    pillow("jpeg-cmyk", small.convert("CMYK"), quality=85)
    pillow("jpeg-rgb", small, quality=85, keep_rgb=True)
    tiny("jpeg-440", wide, [(1, 2), (1, 1), (1, 1)])
    tiny("jpeg-411", wide, [(4, 1), (1, 1), (1, 1)])
    tiny("jpeg-h3", wide, [(3, 1), (1, 1), (1, 1)])
    tiny("jpeg-mixed", wide, [(2, 2), (1, 1), (1, 2)])
    tiny("jpeg-scans", wide, [(2, 2), (1, 1), (1, 1)], separate=True, restart=3)
    # the same 9x5 picture with each EXIF orientation; expected: Pillow's exif_transpose
    tile = picture(9, 5, seed=3)
    for o in range(1, 9):
        buf = io.BytesIO()
        tile.save(buf, "JPEG", quality=90, subsampling=0, optimize=True, exif=exif_with_orientation(o))
        data = buf.getvalue()
        save(f"jpeg-exif{o}", data, ImageOps.exif_transpose(Image.open(io.BytesIO(data))))
    icc = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
    pillow("jpeg-icc", picture(9, 5, seed=4), quality=90, icc_profile=icc)
    # DCT-scaled decodes (Pillow's draft mode is libjpeg's scale_denom): 1/2, 1/4, 1/8 one after another
    for name in ["420", "422", "411", "prog"]:
        scaled = b""
        for s in (2, 4, 8):
            im = Image.open(OUT / f"jpeg-{name}.jpg")
            im.draft("RGB", (im.width // s, im.height // s))
            assert im.size == (-(-21 // s), -(-17 // s))
            scaled += im.convert("RGB").tobytes()
        (OUT / f"jpeg-{name}.scaled.rgb").write_bytes(scaled)
    total = sum(p.stat().st_size for p in OUT.glob("jpeg-*"))
    print(f"{len(list(OUT.glob('jpeg-*.jpg')))} fixtures, {total} bytes")


if __name__ == "__main__":
    main()

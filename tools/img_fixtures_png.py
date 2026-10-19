"""Writes the test fixtures of the PNG, GIF, BMP and inflate decoders into src/web/img/testdata/.

    python tools/img_fixtures_png.py

The PNG and GIF files are written by hand here (zlib + struct, and a small LZW encoder), so every
color type, bit depth, filter and LZW edge case is covered; a few come from Pillow, as real-world
files. Pixels follow formulas the Rust tests repeat (`samp`, `pal` below), and each file whose
pixels Pillow decodes the same way is checked against Pillow. GIF and BMP fixtures get an
`.rgba` file next to them with the expected pixels (RGBA8, rows top to bottom).
"""

import io
import os
import struct
import zlib

from PIL import Image

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src", "web", "img", "testdata")
CHECK_FAILURES = []
LZW_FULL_RUNS = [0]  # the most codes the GIF encoder wrote with a full table


def write(name, data):
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(data)


def pillow_check(name, w, h, expected):
    """Compares Pillow's decode of `name` with the expected RGBA bytes (any color of a fully
    transparent pixel counts as the same)."""
    im = Image.open(os.path.join(OUT, name))
    got, expected = bytearray(im.convert("RGBA").tobytes()), bytearray(expected)
    for px in (got, expected):
        for i in range(0, len(px) - 3, 4):
            if px[i + 3] == 0:
                px[i : i + 3] = b"\x00\x00\x00"
    if im.size != (w, h) or got != expected:
        CHECK_FAILURES.append(name)


# ---------------------------------------------------------------- the formulas the tests repeat


def s16(x, y, c):
    return (x * 7919 + y * 104729 + c * 31337 + x * y * 1543 + 12345) & 0xFFFF


def samp(x, y, c, d):
    """Sample of channel `c` at (x, y) for bit depth `d`: the top `d` bits of s16."""
    return s16(x, y, c) >> (16 - d)


def pal(i):
    return ((i * 29 + 7) % 256, (i * 71 + 3) % 256, (i * 113 + 5) % 256)


def pal_len(d):
    return 200 if d == 8 else 1 << d


def pindex(x, y, d):
    return samp(x, y, 0, d) % pal_len(d)


def pal_alpha(i):
    return (i * 67 + 1) % 256


def to8(v, d):
    if d == 16:
        return (v * 255 + 32767) // 65535
    if d == 8:
        return v
    return v * (255 // ((1 << d) - 1))


# ---------------------------------------------------------------- PNG

CH = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}
ADAM7 = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]


def raw_row(ct, d, xs, y):
    vals = []
    for x in xs:
        if ct == 3:
            vals.append(pindex(x, y, d))
        else:
            vals += [samp(x, y, c, d) for c in range(CH[ct])]
    if d == 16:
        return b"".join(struct.pack(">H", v) for v in vals)
    if d == 8:
        return bytes(vals)
    per = 8 // d
    out = bytearray()
    for i in range(0, len(vals), per):
        b = 0
        for k in range(per):
            v = vals[i + k] if i + k < len(vals) else 0
            b |= v << (8 - d * (k + 1))
        out.append(b)
    return bytes(out)


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def filt(ft, row, prev, bpp):
    out = bytearray(len(row))
    for i, x in enumerate(row):
        a = row[i - bpp] if i >= bpp else 0
        b = prev[i]
        c = prev[i - bpp] if i >= bpp else 0
        pred = [0, a, b, (a + b) // 2, paeth(a, b, c)][ft]
        out[i] = (x - pred) & 255
    return bytes(out)


def chunk(ty, body):
    return struct.pack(">I", len(body)) + ty + body + struct.pack(">I", zlib.crc32(ty + body) & 0xFFFFFFFF)


def gray_key(d):
    return samp(1, 1, 0, d)


def rgb_key(d):
    return [samp(2, 1, c, d) for c in range(3)]


def expect_png(ct, d, w, h, trns):
    px = bytearray()
    for y in range(h):
        for x in range(w):
            if ct == 3:
                i = pindex(x, y, d)
                r, g, b = pal(i)
                a = pal_alpha(i) if trns and i < pal_len(d) - 3 else 255
                px += bytes((r, g, b, a))
            elif ct == 0:
                v = samp(x, y, 0, d)
                g = to8(v, d)
                px += bytes((g, g, g, 0 if trns and v == gray_key(d) else 255))
            elif ct == 2:
                vs = [samp(x, y, c, d) for c in range(3)]
                px += bytes([to8(v, d) for v in vs] + [0 if trns and vs == rgb_key(d) else 255])
            elif ct == 4:
                g = to8(samp(x, y, 0, d), d)
                px += bytes((g, g, g, to8(samp(x, y, 1, d), d)))
            else:
                px += bytes(to8(samp(x, y, c, d), d) for c in range(4))
    return px


ICC = bytes((i * 7 + 3) % 256 for i in range(300))


def png(name, ct, d, w, h, interlace=False, trns=False, iccp=False, split=None, iend=True, check=True):
    passes = ADAM7 if interlace else [(0, 0, 1, 1)]
    bpp = max(1, CH[ct] * d // 8)
    raw = bytearray()
    fi = 0
    for x0, y0, dx, dy in passes:
        xs, ys = list(range(x0, w, dx)), list(range(y0, h, dy))
        if not xs or not ys:
            continue
        prev = bytes(len(raw_row(ct, d, xs, 0)))
        for y in ys:
            r = raw_row(ct, d, xs, y)
            ft = fi % 5
            fi += 1
            raw.append(ft)
            raw += filt(ft, r, prev, bpp)
            prev = r
    z = zlib.compress(bytes(raw), 9)
    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, d, ct, 0, 0, int(interlace)))
    if iccp:
        out += chunk(b"iCCP", b"test profile\x00\x00" + zlib.compress(ICC))
    out += chunk(b"gAMA", struct.pack(">I", 45455))
    if ct == 3:
        out += chunk(b"PLTE", b"".join(bytes(pal(i)) for i in range(pal_len(d))))
    if trns:
        if ct == 3:
            out += chunk(b"tRNS", bytes(pal_alpha(i) for i in range(pal_len(d) - 3)))
        elif ct == 0:
            out += chunk(b"tRNS", struct.pack(">H", gray_key(d)))
        else:
            out += chunk(b"tRNS", struct.pack(">HHH", *rgb_key(d)))
    out += chunk(b"tEXt", b"Comment\x00a text chunk")
    if split:
        for i in range(0, len(z), split):
            out += chunk(b"IDAT", z[i : i + split])
    else:
        out += chunk(b"IDAT", z)
    if iend:
        out += chunk(b"IEND", b"")
    write(name, out)
    # Pillow does not reduce 16-bit samples the way PNG asks (it truncates), so only <= 8 bits.
    if check and d <= 8:
        pillow_check(name, w, h, expect_png(ct, d, w, h, trns))


def png_fixtures():
    for d in (1, 2, 4):
        png(f"png-g{d}.png", 0, d, 13, 9)
    png("png-g8.png", 0, 8, 7, 5)
    png("png-g16.png", 0, 16, 7, 5)
    png("png-rgb8.png", 2, 8, 7, 5)
    png("png-rgb16.png", 2, 16, 7, 5)
    png("png-p1.png", 3, 1, 13, 9)
    png("png-p2-trns.png", 3, 2, 13, 9, trns=True)
    png("png-p4.png", 3, 4, 13, 9)
    png("png-p8-trns.png", 3, 8, 13, 9, trns=True)
    png("png-ga8.png", 4, 8, 7, 5)
    png("png-ga16.png", 4, 16, 7, 5)
    png("png-rgba8.png", 6, 8, 7, 5)
    png("png-rgba16.png", 6, 16, 7, 5)
    png("png-g2-key.png", 0, 2, 7, 5, trns=True)
    png("png-g8-key.png", 0, 8, 7, 5, trns=True)
    png("png-g16-key.png", 0, 16, 7, 5, trns=True)
    png("png-rgb8-key.png", 2, 8, 7, 5, trns=True)
    png("png-rgb16-key.png", 2, 16, 7, 5, trns=True)
    png("png-i-rgba8-1x1.png", 6, 8, 1, 1, interlace=True)
    png("png-i-rgba8-3x2.png", 6, 8, 3, 2, interlace=True)
    png("png-i-rgba8-13x9.png", 6, 8, 13, 9, interlace=True)
    png("png-i-g1-13x9.png", 0, 1, 13, 9, interlace=True)
    png("png-i-p4-7x5.png", 3, 4, 7, 5, interlace=True)
    png("png-i-rgb16-7x5.png", 2, 16, 7, 5, interlace=True)
    png("png-i-ga8-9x10.png", 4, 8, 9, 10, interlace=True)
    png("png-multi-idat.png", 6, 8, 13, 9, split=10)
    png("png-iccp.png", 2, 8, 7, 5, iccp=True)
    png("png-noiend.png", 2, 8, 7, 5, iend=False)

    # Pillow's own files: an optimized RGB PNG and an APNG whose default image is frame 1.
    rgb = Image.frombytes("RGB", (13, 9), bytes(expect_png(2, 8, 13, 9, False)[i] for i in range(13 * 9 * 4) if i % 4 != 3))
    buf = io.BytesIO()
    rgb.save(buf, "PNG", optimize=True)
    write("png-pillow.png", buf.getvalue())
    first = Image.frombytes("RGBA", (7, 5), bytes(expect_png(6, 8, 7, 5, False)))
    second = Image.new("RGBA", (7, 5), (255, 0, 0, 255))
    buf = io.BytesIO()
    first.save(buf, "PNG", save_all=True, append_images=[second], duration=100, loop=0)
    write("png-apng.png", buf.getvalue())


# ---------------------------------------------------------------- inflate


def zdata(n):
    """The bytes the inflate tests compress: an LCG mixed with runs and repeated phrases."""
    out = bytearray()
    s = 12345
    i = 0
    while len(out) < n:
        s = (s * 1103515245 + 12345) & 0x7FFFFFFF
        kind = (s >> 16) % 4
        if kind == 0:
            out.append((s >> 8) & 255)
        elif kind == 1:
            out += bytes([(s >> 4) & 255]) * ((s >> 20) % 40 + 1)
        elif kind == 2:
            out += b"the quick brown fox %d " % (i % 97)
        else:
            out += bytes((i + k) & 255 for k in range((s >> 12) % 300))
        i += 1
    return bytes(out[:n])


def inflate_fixtures():
    write("png-zlib-0.z", zlib.compress(zdata(3000), 0))
    write("png-zlib-1.z", zlib.compress(zdata(20000), 1))
    write("png-zlib-9.z", zlib.compress(zdata(20000), 9))
    c = zlib.compressobj(6, zlib.DEFLATED, 15, 9, zlib.Z_FIXED)
    write("png-zlib-fixed.z", c.compress(zdata(5000)) + c.flush())


# ---------------------------------------------------------------- GIF


def lzw(indices, min_size, full_codes_before_clear=None):
    """GIF LZW. When the table is full the encoder keeps going without a clear code for
    `full_codes_before_clear` codes (None: clear at once), then clears."""
    clear, eoi = 1 << min_size, (1 << min_size) + 1
    out = bytearray()
    acc = nbits = 0

    def emit(code, size):
        nonlocal acc, nbits
        acc |= code << nbits
        nbits += size
        while nbits >= 8:
            out.append(acc & 255)
            acc >>= 8
            nbits -= 8

    def reset():
        return {bytes([i]): i for i in range(clear)}, clear + 2, min_size + 1

    table, nxt, size = reset()
    emit(clear, size)
    full_run = 0
    w = bytes([indices[0]])
    for k in indices[1:]:
        wk = w + bytes([k])
        if wk in table:
            w = wk
            continue
        emit(table[w], size)
        if nxt < 4096:
            table[wk] = nxt
            nxt += 1
            if nxt > (1 << size) and size < 12:
                size += 1
        else:
            full_run += 1
            LZW_FULL_RUNS[0] = max(LZW_FULL_RUNS[0], full_run)
            if full_codes_before_clear is None or full_run > full_codes_before_clear:
                emit(clear, size)
                table, nxt, size = reset()
                full_run = 0
        w = bytes([k])
    emit(table[w], size)
    emit(eoi, size)
    if nbits:
        out.append(acc & 255)
    return bytes(out)


def sub_blocks(data):
    out = bytearray()
    for i in range(0, len(data), 255):
        part = data[i : i + 255]
        out.append(len(part))
        out += part
    return bytes(out) + b"\x00"


def table_bits(n):
    bits = 0
    while (2 << bits) < n:
        bits += 1
    return bits


def color_table(colors):
    bits = table_bits(len(colors))
    return b"".join(bytes(c) for c in colors) + b"\x00\x00\x00" * ((2 << bits) - len(colors)), bits


def gif(name, sw, sh, gpal, frame, check=True):
    """One frame: (left, top, fw, fh, idx(x, y), local palette or None, interlaced, transparent index or None, min code size, lzw option)."""
    left, top, fw, fh, idx, lpal, inter, ti, mcs, full = frame
    out = bytearray(b"GIF89a")
    flags = 0
    gtab = b""
    if gpal:
        gtab, bits = color_table(gpal)
        flags = 0x80 | 0x70 | bits
    out += struct.pack("<HHBBB", sw, sh, flags, 0, 0) + gtab
    out += b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x00\x00\x00"
    out += b"\x21\xfe" + sub_blocks(b"a comment block")
    out += b"\x21\xf9\x04" + bytes([1 if ti is not None else 0]) + b"\x0a\x00" + bytes([ti or 0]) + b"\x00"
    iflags = 0x40 if inter else 0
    ltab = b""
    if lpal:
        ltab, bits = color_table(lpal)
        iflags |= 0x80 | bits
    out += b"\x2c" + struct.pack("<HHHHB", left, top, fw, fh, iflags) + ltab
    rows = list(range(fh))
    if inter:
        rows = list(range(0, fh, 8)) + list(range(4, fh, 8)) + list(range(2, fh, 4)) + list(range(1, fh, 2))
    indices = [idx(x, y) for y in rows for x in range(fw)]
    out += bytes([mcs]) + sub_blocks(lzw(indices, mcs, full))
    out += b"\x3b"
    write(name, bytes(out))
    colors = lpal or gpal
    px = bytearray(sw * sh * 4)
    for y in range(fh):
        for x in range(fw):
            i = idx(x, y)
            cx, cy = left + x, top + y
            if i != ti and cx < sw and cy < sh:
                px[(cy * sw + cx) * 4 : (cy * sw + cx) * 4 + 4] = bytes(colors[i]) + b"\xff"
    write(name.replace(".gif", ".rgba"), bytes(px))
    if check:
        pillow_check(name, sw, sh, px)


def gif_fixtures():
    p8 = [pal(i) for i in range(8)]
    gif("gif-trans.gif", 7, 5, p8, (1, 1, 5, 3, lambda x, y: (x * 3 + y * 5 + x * y) % 8, None, False, 3, 3, None))
    gif("gif-plain.gif", 7, 5, p8, (0, 0, 7, 5, lambda x, y: (x * 3 + y * 5 + x * y) % 8, None, False, 3, 3, None))
    p16 = [pal(i * 3 + 1) for i in range(16)]
    gif("gif-interlace.gif", 13, 11, None, (0, 0, 13, 11, lambda x, y: (x * 7 + y * 3 + x * y) % 16, p16, True, None, 4, None))

    # Enough pseudo-random pixels to fill the 4096-entry table: 600 codes with a full table and
    # no clear code, then a clear and a new table.
    def rnd(x, y):
        v = ((y * 128 + x) * 0x9E3779B1) & 0xFFFFFFFF
        v ^= v >> 15
        v = (v * 0x85EBCA77) & 0xFFFFFFFF
        v ^= v >> 13
        return v & 15

    gif("gif-lzw12.gif", 128, 96, p16, (0, 0, 128, 96, rnd, None, False, None, 4, 600))
    assert LZW_FULL_RUNS[0] >= 600, "the LZW table of gif-lzw12.gif never filled"
    os.remove(os.path.join(OUT, "gif-lzw12.rgba"))  # the Rust test computes these pixels

    # Pillow's own GIF writer, with a transparent color.
    im = Image.new("P", (9, 7))
    im.putpalette([v for i in range(16) for v in pal(i * 5 + 2)])
    im.putdata([(x * 5 + y * 3) % 16 for y in range(7) for x in range(9)])
    buf = io.BytesIO()
    im.save(buf, "GIF", transparency=4, optimize=False)
    write("gif-pillow.gif", buf.getvalue())
    write("gif-pillow.rgba", Image.open(io.BytesIO(buf.getvalue())).convert("RGBA").tobytes())


# ---------------------------------------------------------------- BMP


def rgb_px(x, y):
    return ((x * 37 + y * 11) % 256, (x * 13 + y * 59) % 256, (x * y * 7 + 3) % 256, (x * 51 + y * 31) % 256)


def bmp(name, w, h, bpp, rows, palette=None, comp=0, hs=40, masks=None, topdown=False, core=False, expected=None):
    """A BMP from `rows` (the pixel bytes of each row top to bottom, unpadded)."""
    stride = (w * bpp + 31) // 32 * 4
    order = rows if topdown else rows[::-1]
    if comp in (1, 2):
        pixels = b"".join(order)
    else:
        pixels = b"".join(r + b"\x00" * (stride - len(r)) for r in order)
    pal_bytes = b""
    if palette:
        pal_bytes = b"".join(bytes((b, g, r)) + (b"" if core else b"\x00") for r, g, b in palette)
    if core:
        header = struct.pack("<IHHHH", 12, w, h, 1, bpp)
    else:
        header = struct.pack("<IiiHHIIiiII", hs, w, -h if topdown else h, 1, bpp, comp, len(pixels), 2835, 2835,
                             len(palette) if palette else 0, 0)
        if hs == 40 and masks:
            header += struct.pack("<III", *masks[:3])
        elif hs >= 108:
            m = list(masks or (0, 0, 0, 0)) + [0] * 4
            header += struct.pack("<IIII", *m[:4]) + b"BGRs" + b"\x00" * (hs - 40 - 20)
    off = 14 + len(header) + len(pal_bytes)
    data = b"BM" + struct.pack("<IHHI", off + len(pixels), 0, 0, off) + header + pal_bytes + pixels
    write(name, data)
    write(name.replace(".bmp", ".rgba"), bytes(expected))


def bmp_fixtures():
    w, h = 7, 5
    exp24 = [v for y in range(h) for x in range(w) for v in rgb_px(x, y)[:3] + (255,)]
    # Pillow: 24-bit, 32-bit with alpha, 8-bit palette, 1-bit.
    im = Image.frombytes("RGB", (w, h), bytes(v for y in range(h) for x in range(w) for v in rgb_px(x, y)[:3]))
    buf = io.BytesIO()
    im.save(buf, "BMP")
    write("bmp-rgb24.bmp", buf.getvalue())
    write("bmp-rgb24.rgba", bytes(exp24))
    exp32 = [v for y in range(h) for x in range(w) for v in rgb_px(x, y)]
    im = Image.frombytes("RGBA", (w, h), bytes(exp32))
    buf = io.BytesIO()
    im.save(buf, "BMP")
    write("bmp-rgba32.bmp", buf.getvalue())
    write("bmp-rgba32.rgba", bytes(exp32))
    pw, ph = 13, 9
    pidx = lambda x, y: (x * 3 + y * 5 + x * y) % 200
    im = Image.new("P", (pw, ph))
    im.putpalette([v for i in range(256) for v in pal(i)])
    im.putdata([pidx(x, y) for y in range(ph) for x in range(pw)])
    buf = io.BytesIO()
    im.save(buf, "BMP")
    write("bmp-p8.bmp", buf.getvalue())
    write("bmp-p8.rgba", bytes(v for y in range(ph) for x in range(pw) for v in pal(pidx(x, y)) + (255,)))
    im = Image.new("1", (pw, ph))
    im.putdata([255 if (x + y * 2) % 3 == 0 else 0 for y in range(ph) for x in range(pw)])
    buf = io.BytesIO()
    im.save(buf, "BMP")
    write("bmp-p1.bmp", buf.getvalue())
    write("bmp-p1.rgba", bytes(v for y in range(ph) for x in range(pw) for v in ((255,) * 4 if (x + y * 2) % 3 == 0 else (0, 0, 0, 255))))

    # Hand-written: 4-bit palette, top-down 24-bit, 16-bit 565 and 555, V5 32-bit with an alpha mask, RLE8, RLE4, OS/2.
    p16 = [pal(i * 3 + 1) for i in range(16)]
    idx4 = lambda x, y: (x * 7 + y * 3 + x * y) % 16

    def pack4(vals):
        vals = vals + [0] * (len(vals) % 2)
        return bytes(vals[i] << 4 | vals[i + 1] for i in range(0, len(vals), 2))

    bmp("bmp-p4.bmp", pw, ph, 4, [pack4([idx4(x, y) for x in range(pw)]) for y in range(ph)], palette=p16,
        expected=[v for y in range(ph) for x in range(pw) for v in p16[idx4(x, y)] + (255,)])
    bmp("bmp-topdown24.bmp", w, h, 24, [bytes(v for x in range(w) for v in rgb_px(x, y)[2::-1]) for y in range(h)], topdown=True,
        expected=exp24)

    def c5(x, y, k):
        return (x * 5 + y * 9 + k * 11 + x * y) % 32

    def c6(x, y):
        return (x * 9 + y * 5 + x * y * 3) % 64

    s5 = lambda v: (v * 255 + 15) // 31
    s6 = lambda v: (v * 255 + 31) // 63
    rows565 = [b"".join(struct.pack("<H", c5(x, y, 0) << 11 | c6(x, y) << 5 | c5(x, y, 2)) for x in range(w)) for y in range(h)]
    bmp("bmp-565.bmp", w, h, 16, rows565, comp=3, masks=(0xF800, 0x07E0, 0x001F),
        expected=[v for y in range(h) for x in range(w) for v in (s5(c5(x, y, 0)), s6(c6(x, y)), s5(c5(x, y, 2)), 255)])
    rows555 = [b"".join(struct.pack("<H", c5(x, y, 0) << 10 | c5(x, y, 1) << 5 | c5(x, y, 2)) for x in range(w)) for y in range(h)]
    bmp("bmp-555.bmp", w, h, 16, rows555,
        expected=[v for y in range(h) for x in range(w) for v in (s5(c5(x, y, 0)), s5(c5(x, y, 1)), s5(c5(x, y, 2)), 255)])
    # V5, ARGB with the masks in the header, top-down.
    rows_argb = [b"".join(struct.pack("<I", a << 24 | r << 16 | g << 8 | b) for r, g, b, a in (rgb_px(x, y) for x in range(w))) for y in range(h)]
    bmp("bmp-v5-argb.bmp", w, h, 32, rows_argb, comp=3, hs=124, masks=(0xFF0000, 0xFF00, 0xFF, 0xFF000000), topdown=True, expected=exp32)

    # RLE8, 9x6: runs, an absolute run (odd length, padded), a delta, end of line, end of bitmap.
    # Rows are listed bottom-up, as the file stores them; pixels never written stay transparent.
    rw, rh = 9, 6
    p8 = [pal(i) for i in range(16)]
    n = None
    rle = bytes([4, 1, 5, 2]) + b"\x00\x00"  # file row 0: run of four 1s, run of five 2s, end of line
    rle += b"\x00\x03" + bytes([3, 4, 5, 0]) + bytes([2, 6]) + b"\x00\x00"  # row 1: absolute 3 4 5 (padded), two 6s
    rle += bytes([2, 7]) + b"\x00\x02" + bytes([3, 1]) + bytes([2, 8]) + b"\x00\x00"  # row 2: two 7s, delta (+3, +1), two 8s
    rle += bytes([9, 9]) + b"\x00\x00"  # row 4: nine 9s
    rle += b"\x00\x04" + bytes([10, 11, 12, 13]) + b"\x00\x01"  # row 5: absolute 10 11 12 13, end of bitmap
    canvas = [  # file rows, bottom-up
        [1, 1, 1, 1, 2, 2, 2, 2, 2],
        [3, 4, 5, 6, 6, n, n, n, n],
        [7, 7, n, n, n, n, n, n, n],
        [n, n, n, n, n, 8, 8, n, n],
        [9] * 9,
        [10, 11, 12, 13, n, n, n, n, n],
    ]
    exp = []
    for y in range(rh):
        for x in range(rw):
            i = canvas[rh - 1 - y][x]
            exp += list(p8[i]) + [255] if i is not None else [0, 0, 0, 0]
    bmp_rle("bmp-rle8.bmp", rw, rh, 8, 1, bytes(rle), p8, exp)

    # RLE4, 7x3: a run of alternating colors, an absolute run, end of line, end of bitmap.
    rle4 = bytes([5, 0x12]) + b"\x00\x00"  # row 0: 1 2 1 2 1
    rle4 += b"\x00\x05" + bytes([0x34, 0x56, 0x70]) + b"\x00" + b"\x00\x00"  # row 1: 3 4 5 6 7 (padded to a word)
    rle4 += bytes([7, 0xAB]) + b"\x00\x01"  # row 2: A B A B A B A
    rows4 = [[1, 2, 1, 2, 1, None, None], [3, 4, 5, 6, 7, None, None], [10, 11, 10, 11, 10, 11, 10]]
    exp = []
    for y in range(3):
        for i in rows4[2 - y]:
            exp += list(p16[i]) + [255] if i is not None else [0, 0, 0, 0]
    bmp_rle("bmp-rle4.bmp", 7, 3, 4, 2, rle4, p16, exp)

    # OS/2 core header, 8-bit palette with 3-byte entries.
    cidx = lambda x, y: (x + y * 3) % 16
    bmp("bmp-core8.bmp", w, h, 8, [bytes(cidx(x, y) for x in range(w)) for y in range(h)], palette=p8, core=True,
        expected=[v for y in range(h) for x in range(w) for v in p8[cidx(x, y)] + (255,)])


def bmp_rle(name, w, h, bpp, comp, data, palette, expected):
    pal_bytes = b"".join(bytes((b, g, r, 0)) for r, g, b in palette)
    header = struct.pack("<IiiHHIIiiII", 40, w, h, 1, bpp, comp, len(data), 2835, 2835, len(palette), 0)
    off = 14 + len(header) + len(pal_bytes)
    write(name, b"BM" + struct.pack("<IHHI", off + len(data), 0, 0, off) + header + pal_bytes + data)
    write(name.replace(".bmp", ".rgba"), bytes(expected))


def main():
    os.makedirs(OUT, exist_ok=True)
    png_fixtures()
    inflate_fixtures()
    gif_fixtures()
    bmp_fixtures()
    # BMP: Pillow fills pixels RLE skips with color 0 and reads a BI_RGB 32-bit file as opaque, so
    # only the plain ones are compared.
    for n, size in [("bmp-rgb24", (7, 5)), ("bmp-p8", (13, 9)), ("bmp-p1", (13, 9)), ("bmp-p4", (13, 9)),
                    ("bmp-topdown24", (7, 5)), ("bmp-565", (7, 5)), ("bmp-555", (7, 5)), ("bmp-v5-argb", (7, 5)),
                    ("bmp-core8", (7, 5))]:
        with open(os.path.join(OUT, n + ".rgba"), "rb") as f:
            pillow_check(n + ".bmp", *size, f.read())
    total = sum(os.path.getsize(os.path.join(OUT, f)) for f in os.listdir(OUT) if f.split("-")[0] in ("png", "gif", "bmp"))
    print(f"wrote the fixtures ({total} bytes)")
    # Known Pillow differences: it drops the tRNS key of 2-bit gray, and it scales 5/6-bit BMP
    # channels by bit replication (v << 3 | v >> 2) instead of rounding v * 255 / 31.
    known = {"png-g2-key.png", "bmp-565.bmp", "bmp-555.bmp"}
    unexpected = [n for n in CHECK_FAILURES if n not in known]
    if unexpected:
        print("Pillow decodes these differently:", ", ".join(unexpected))


if __name__ == "__main__":
    main()

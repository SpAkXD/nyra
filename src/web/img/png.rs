//! PNG decoder (zero crates): every color type (gray, RGB, palette, gray + alpha, RGBA) at every
//! bit depth (1, 2, 4, 8, 16), palette transparency and the tRNS color key of gray and RGB
//! images, Adam7 interlacing, the five row filters, image data split over many IDAT chunks, and
//! the iCCP color profile. The result is RGBA8; 16-bit samples are rounded to 8 bits.
//!
//! An APNG decodes to its default image (the IDAT data). gAMA, cHRM, sRGB and the other chunks
//! are ignored. Like browsers it is lenient where the pixels are all there: chunk CRCs and the
//! Adler-32 of the image data are not checked (a damaged byte almost always breaks the
//! decompression anyway), and a file may end without IEND or with extra image data. Image data
//! that stops short is an error ("truncated PNG data").
//!
//! Memory: the decompressed scanlines and the RGBA output, nothing more; the output is allocated
//! only once the scanlines are complete, so a small hostile file cannot claim a huge buffer.

use super::common::{check_size, Image, Res};
use super::inflate;

const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const TRUNCATED: &str = "truncated PNG data";
/// The largest ICC profile kept (a real one is a few KB).
const ICC_LIMIT: usize = 16 << 20;

/// The Adam7 passes: first column, first row, column step, row step.
const ADAM7: [(usize, usize, usize, usize); 7] =
    [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];
const NOT_INTERLACED: [(usize, usize, usize, usize); 1] = [(0, 0, 1, 1)];

/// Decodes a PNG file to RGBA8.
pub fn decode(data: &[u8]) -> Res<Image> {
    if data.len() < 8 || data[..8] != SIGNATURE {
        return Err("not a PNG file".into());
    }
    let mut pos = 8;
    let mut header: Option<Header> = None;
    let mut plte: &[u8] = &[];
    let mut trns: Option<&[u8]> = None;
    let mut icc = None;
    let mut idat: Vec<&[u8]> = Vec::new();
    while pos + 8 <= data.len() {
        let len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let ty = &data[pos + 4..pos + 8];
        if len > 0x7FFF_FFFF {
            return Err("corrupt PNG data (a chunk length over 2^31)".into());
        }
        let start = pos + 8;
        let end = start.saturating_add(len);
        let body = match data.get(start..end) {
            Some(b) => b,
            None if ty == b"IDAT" => &data[start..],
            None if !idat.is_empty() => break,
            None => return Err(TRUNCATED.into()),
        };
        if header.is_none() && ty != b"IHDR" {
            return Err("corrupt PNG data (the first chunk is not IHDR)".into());
        }
        if !idat.is_empty() && ty != b"IDAT" {
            break; // the image data is complete; nothing after it matters
        }
        match ty {
            b"IHDR" if header.is_none() => header = Some(Header::parse(body)?),
            b"PLTE" => plte = body,
            b"tRNS" => trns = Some(body),
            b"iCCP" => icc = iccp(body),
            b"IDAT" => idat.push(body),
            b"IEND" => break,
            _ => {}
        }
        pos = end.saturating_add(4); // the CRC, not checked
    }
    let Some(h) = header else { return Err(TRUNCATED.into()) };
    if idat.is_empty() {
        return Err(TRUNCATED.into());
    }
    let joined;
    let z: &[u8] = if idat.len() == 1 {
        idat[0]
    } else {
        joined = idat.concat();
        &joined
    };
    let conv = Converter::new(&h, plte, trns)?;
    let passes: &[(usize, usize, usize, usize)] = if h.interlaced { &ADAM7 } else { &NOT_INTERLACED };

    // The size of the filtered scanlines of all passes.
    let mut raw_len = 0usize;
    for &(x0, y0, dx, dy) in passes {
        let (pw, ph) = (pass_len(h.w, x0, dx), pass_len(h.h, y0, dy));
        if pw > 0 && ph > 0 {
            let rb = h.row_bytes(pw)?;
            raw_len = (rb + 1).checked_mul(ph).and_then(|n| n.checked_add(raw_len)).ok_or("the PNG image is too large")?;
        }
    }
    // DEFLATE cannot expand by more than 1032:1, so a short stream gets a small buffer.
    let mut raw = Vec::with_capacity(raw_len.min(z.len().saturating_mul(1032).saturating_add(1024)));
    match inflate::zlib_to(z, &mut raw, raw_len, false) {
        _ if raw.len() == raw_len => {}
        Ok(()) => return Err(TRUNCATED.into()),
        Err(e) if e.contains("truncated") => return Err(TRUNCATED.into()),
        Err(e) => return Err(format!("corrupt PNG image data: {e}")),
    }

    let mut px = vec![0u8; h.w * h.h * 4];
    let bpp = (h.bits_per_pixel() / 8).max(1);
    let zero = vec![0u8; h.row_bytes(h.w)?];
    let mut row_px = if h.interlaced { vec![0u8; h.w * 4] } else { Vec::new() };
    let mut off = 0;
    for &(x0, y0, dx, dy) in passes {
        let (pw, ph) = (pass_len(h.w, x0, dx), pass_len(h.h, y0, dy));
        if pw == 0 || ph == 0 {
            continue;
        }
        let rb = h.row_bytes(pw)?;
        let stride = rb + 1;
        let pass = &mut raw[off..off + stride * ph];
        off += stride * ph;
        for j in 0..ph {
            let (before, rest) = pass.split_at_mut(j * stride);
            let prev = if j == 0 { &zero[..rb] } else { &before[(j - 1) * stride + 1..] };
            let ft = rest[0];
            let row = &mut rest[1..stride];
            unfilter(ft, row, prev, bpp)?;
            let y = y0 + j * dy;
            if dx == 1 {
                conv.expand(row, &mut px[y * h.w * 4..(y + 1) * h.w * 4]);
            } else {
                let out = &mut row_px[..pw * 4];
                conv.expand(row, out);
                for (i, p) in out.as_chunks::<4>().0.iter().enumerate() {
                    let at = (y * h.w + x0 + i * dx) * 4;
                    px[at..at + 4].copy_from_slice(p);
                }
            }
        }
    }
    Ok(Image { w: h.w, h: h.h, px, icc })
}

/// The number of pixels (or rows) of an Adam7 pass along a side of `n`.
fn pass_len(n: usize, first: usize, step: usize) -> usize {
    if n > first {
        (n - first).div_ceil(step)
    } else {
        0
    }
}

struct Header {
    w: usize,
    h: usize,
    depth: u8,
    color: u8,
    interlaced: bool,
}

impl Header {
    fn parse(b: &[u8]) -> Res<Header> {
        if b.len() < 13 {
            return Err("corrupt PNG data (a short IHDR chunk)".into());
        }
        let w = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
        let h = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
        let (depth, color) = (b[8], b[9]);
        check_size(w, h)?;
        let depths: &[u8] = match color {
            0 => &[1, 2, 4, 8, 16],
            3 => &[1, 2, 4, 8],
            2 | 4 | 6 => &[8, 16],
            _ => return Err(format!("unsupported PNG color type {color}")),
        };
        if !depths.contains(&depth) {
            return Err(format!("unsupported PNG bit depth {depth} (color type {color})"));
        }
        if b[10] != 0 || b[11] != 0 {
            return Err("unsupported PNG compression or filter method".into());
        }
        if b[12] > 1 {
            return Err(format!("unsupported PNG interlace method {}", b[12]));
        }
        Ok(Header { w, h, depth, color, interlaced: b[12] == 1 })
    }

    fn bits_per_pixel(&self) -> usize {
        let channels = match self.color {
            0 | 3 => 1,
            4 => 2,
            2 => 3,
            _ => 4,
        };
        channels * self.depth as usize
    }

    /// The bytes of a row of `w` pixels, without the filter byte.
    fn row_bytes(&self, w: usize) -> Res<usize> {
        Ok(w.checked_mul(self.bits_per_pixel()).ok_or("the PNG image is too large")?.div_ceil(8))
    }
}

/// The ICC profile of an iCCP chunk: a name, a zero byte, the method (0) and zlib data. A
/// damaged profile is dropped, as browsers do.
fn iccp(b: &[u8]) -> Option<Vec<u8>> {
    let nul = b.iter().take(80).position(|&c| c == 0)?;
    if b.get(nul + 1) != Some(&0) {
        return None;
    }
    inflate::zlib(&b[nul + 2..], ICC_LIMIT).ok()
}

/// Undoes the filter of one row in place. `prev` is the row above (zeros for the first row of a
/// pass), `bpp` the bytes per whole pixel (at least 1).
fn unfilter(ft: u8, row: &mut [u8], prev: &[u8], bpp: usize) -> Res<()> {
    match ft {
        0 => {}
        2 => row.iter_mut().zip(prev).for_each(|(r, &p)| *r = r.wrapping_add(p)),
        1 | 3 | 4 => match bpp {
            1 => unfilter_n::<1>(ft, row, prev),
            2 => unfilter_n::<2>(ft, row, prev),
            3 => unfilter_n::<3>(ft, row, prev),
            4 => unfilter_n::<4>(ft, row, prev),
            6 => unfilter_n::<6>(ft, row, prev),
            _ => unfilter_n::<8>(ft, row, prev),
        },
        _ => return Err(format!("corrupt PNG data (unknown filter type {ft})")),
    }
    Ok(())
}

/// Sub (1), Average (3) and Paeth (4) for `N` bytes per pixel, a pixel at a time.
#[inline(always)]
fn unfilter_n<const N: usize>(ft: u8, row: &mut [u8], prev: &[u8]) {
    let (mut a, mut c) = ([0u8; N], [0u8; N]);
    let rows = row.as_chunks_mut::<N>().0.iter_mut().zip(prev.as_chunks::<N>().0);
    match ft {
        1 => {
            for (r, _) in rows {
                for k in 0..N {
                    r[k] = r[k].wrapping_add(a[k]);
                    a[k] = r[k];
                }
            }
        }
        3 => {
            for (r, p) in rows {
                for k in 0..N {
                    r[k] = r[k].wrapping_add(((u16::from(a[k]) + u16::from(p[k])) / 2) as u8);
                    a[k] = r[k];
                }
            }
        }
        _ => {
            for (r, p) in rows {
                for k in 0..N {
                    r[k] = r[k].wrapping_add(paeth(a[k], p[k], c[k]));
                    a[k] = r[k];
                    c[k] = p[k];
                }
            }
        }
    }
}

/// The Paeth predictor: of left (a), above (b) and upper left (c), the one closest to
/// a + b - c, preferring a, then b. Written as two selects, without branches.
#[inline(always)]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a, b, c) = (i16::from(a), i16::from(b), i16::from(c));
    let pa = (b - c).abs();
    let pb = (a - c).abs();
    let pc = (a + b - 2 * c).abs();
    let (bc, pbc) = if pc < pb { (c, pc) } else { (b, pb) };
    (if pbc < pa { bc } else { a }) as u8
}

/// A 16-bit sample to 8 bits, rounded.
#[inline(always)]
fn to8(v: u16) -> u8 {
    ((u32::from(v) * 255 + 32767) / 65535) as u8
}

/// How the samples of a row become RGBA8.
enum Kind {
    /// Palette, or gray at 1-8 bits: each sample looks up its color (bits per sample given).
    Lut(u8, Box<[[u8; 4]; 256]>),
    Gray16(Option<u16>),
    Rgb8(Option<[u16; 3]>),
    Rgb16(Option<[u16; 3]>),
    GrayAlpha8,
    GrayAlpha16,
    Rgba8,
    Rgba16,
}

struct Converter {
    kind: Kind,
}

impl Converter {
    fn new(h: &Header, plte: &[u8], trns: Option<&[u8]>) -> Res<Converter> {
        let be16 = |b: &[u8], i: usize| b.get(i..i + 2).map(|s| u16::from_be_bytes([s[0], s[1]]));
        let key3 = || trns.and_then(|t| Some([be16(t, 0)?, be16(t, 2)?, be16(t, 4)?]));
        let kind = match (h.color, h.depth) {
            (3, d) => {
                if plte.len() < 3 {
                    return Err("corrupt PNG data (the palette is missing)".into());
                }
                // Indices past the palette are opaque black, as in browsers.
                let mut lut = Box::new([[0, 0, 0, 255]; 256]);
                for (e, &[r, g, b]) in lut.iter_mut().zip(plte.as_chunks::<3>().0) {
                    *e = [r, g, b, 255];
                }
                for (e, &a) in lut.iter_mut().zip(trns.unwrap_or(&[])) {
                    e[3] = a;
                }
                Kind::Lut(d, lut)
            }
            (0, 16) => Kind::Gray16(trns.and_then(|t| be16(t, 0))),
            (0, d) => {
                let max = (1u16 << d) - 1;
                let mut lut = Box::new([[0, 0, 0, 255]; 256]);
                for (v, e) in lut.iter_mut().enumerate().take(max as usize + 1) {
                    let g = (v * 255 / max as usize) as u8;
                    *e = [g, g, g, 255];
                }
                if let Some(k) = trns.and_then(|t| be16(t, 0)).filter(|&k| k <= max) {
                    lut[k as usize][3] = 0;
                }
                Kind::Lut(d, lut)
            }
            (2, 8) => Kind::Rgb8(key3()),
            (2, _) => Kind::Rgb16(key3()),
            (4, 8) => Kind::GrayAlpha8,
            (4, _) => Kind::GrayAlpha16,
            (6, 8) => Kind::Rgba8,
            _ => Kind::Rgba16,
        };
        Ok(Converter { kind })
    }

    /// Converts the unfiltered samples of a row into `out` (RGBA8, `out.len() / 4` pixels).
    fn expand(&self, src: &[u8], out: &mut [u8]) {
        let px = out.as_chunks_mut::<4>().0.iter_mut();
        let be = |s: &[u8], k: usize| u16::from_be_bytes([s[2 * k], s[2 * k + 1]]);
        match &self.kind {
            Kind::Lut(8, lut) => px.zip(src).for_each(|(o, &s)| *o = lut[s as usize]),
            Kind::Lut(d, lut) => {
                let d = *d as usize;
                let (per, mask) = (8 / d, (1u8 << d) - 1);
                for (i, o) in px.enumerate() {
                    let shift = 8 - d * (i % per + 1);
                    *o = lut[((src[i / per] >> shift) & mask) as usize];
                }
            }
            Kind::Gray16(key) => {
                for (o, s) in px.zip(src.as_chunks::<2>().0) {
                    let v = u16::from_be_bytes(*s);
                    let g = to8(v);
                    *o = [g, g, g, if *key == Some(v) { 0 } else { 255 }];
                }
            }
            Kind::Rgb8(None) => px.zip(src.as_chunks::<3>().0).for_each(|(o, &[r, g, b])| *o = [r, g, b, 255]),
            Kind::Rgb8(Some(key)) => {
                for (o, &[r, g, b]) in px.zip(src.as_chunks::<3>().0) {
                    *o = [r, g, b, if [r, g, b].map(u16::from) == *key { 0 } else { 255 }];
                }
            }
            Kind::Rgb16(key) => {
                for (o, s) in px.zip(src.as_chunks::<6>().0) {
                    let v = [be(s, 0), be(s, 1), be(s, 2)];
                    *o = [to8(v[0]), to8(v[1]), to8(v[2]), if *key == Some(v) { 0 } else { 255 }];
                }
            }
            Kind::GrayAlpha8 => px.zip(src.as_chunks::<2>().0).for_each(|(o, &[g, a])| *o = [g, g, g, a]),
            Kind::GrayAlpha16 => {
                for (o, s) in px.zip(src.as_chunks::<4>().0) {
                    let g = to8(be(s, 0));
                    *o = [g, g, g, to8(be(s, 1))];
                }
            }
            Kind::Rgba8 => out.copy_from_slice(&src[..out.len()]),
            Kind::Rgba16 => px.zip(src.as_chunks::<8>().0).for_each(|(o, s)| *o = [0, 1, 2, 3].map(|k| to8(be(s, k)))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The formulas of tools/img_fixtures_png.py.
    fn s16(x: usize, y: usize, c: usize) -> usize {
        (x * 7919 + y * 104729 + c * 31337 + x * y * 1543 + 12345) & 0xFFFF
    }
    fn samp(x: usize, y: usize, c: usize, d: u8) -> usize {
        s16(x, y, c) >> (16 - d)
    }
    fn pal(i: usize) -> [u8; 3] {
        [((i * 29 + 7) % 256) as u8, ((i * 71 + 3) % 256) as u8, ((i * 113 + 5) % 256) as u8]
    }
    fn pal_len(d: u8) -> usize {
        if d == 8 {
            200
        } else {
            1 << d
        }
    }
    fn exp8(v: usize, d: u8) -> u8 {
        match d {
            16 => ((v * 255 + 32767) / 65535) as u8,
            8 => v as u8,
            _ => (v * (255 / ((1 << d) - 1))) as u8,
        }
    }

    /// The pixel the fixture of color type `ct`, depth `d` has at (x, y).
    fn expected(ct: u8, d: u8, trns: bool, x: usize, y: usize) -> [u8; 4] {
        match ct {
            3 => {
                let i = samp(x, y, 0, d) % pal_len(d);
                let [r, g, b] = pal(i);
                let a = if trns && i < pal_len(d) - 3 { ((i * 67 + 1) % 256) as u8 } else { 255 };
                [r, g, b, a]
            }
            0 => {
                let v = samp(x, y, 0, d);
                let g = exp8(v, d);
                [g, g, g, if trns && v == samp(1, 1, 0, d) { 0 } else { 255 }]
            }
            2 => {
                let v = [0, 1, 2].map(|c| samp(x, y, c, d));
                let key = [0, 1, 2].map(|c| samp(2, 1, c, d));
                [exp8(v[0], d), exp8(v[1], d), exp8(v[2], d), if trns && v == key { 0 } else { 255 }]
            }
            4 => {
                let g = exp8(samp(x, y, 0, d), d);
                [g, g, g, exp8(samp(x, y, 1, d), d)]
            }
            _ => [0, 1, 2, 3].map(|c| exp8(samp(x, y, c, d), d)),
        }
    }

    /// Name, file, color type, bit depth, tRNS, width, height.
    type Fixture = (&'static str, &'static [u8], u8, u8, bool, usize, usize);

    const FIXTURES: &[Fixture] = &[
        ("g1", include_bytes!("testdata/png-g1.png"), 0, 1, false, 13, 9),
        ("g2", include_bytes!("testdata/png-g2.png"), 0, 2, false, 13, 9),
        ("g4", include_bytes!("testdata/png-g4.png"), 0, 4, false, 13, 9),
        ("g8", include_bytes!("testdata/png-g8.png"), 0, 8, false, 7, 5),
        ("g16", include_bytes!("testdata/png-g16.png"), 0, 16, false, 7, 5),
        ("rgb8", include_bytes!("testdata/png-rgb8.png"), 2, 8, false, 7, 5),
        ("rgb16", include_bytes!("testdata/png-rgb16.png"), 2, 16, false, 7, 5),
        ("p1", include_bytes!("testdata/png-p1.png"), 3, 1, false, 13, 9),
        ("p2-trns", include_bytes!("testdata/png-p2-trns.png"), 3, 2, true, 13, 9),
        ("p4", include_bytes!("testdata/png-p4.png"), 3, 4, false, 13, 9),
        ("p8-trns", include_bytes!("testdata/png-p8-trns.png"), 3, 8, true, 13, 9),
        ("ga8", include_bytes!("testdata/png-ga8.png"), 4, 8, false, 7, 5),
        ("ga16", include_bytes!("testdata/png-ga16.png"), 4, 16, false, 7, 5),
        ("rgba8", include_bytes!("testdata/png-rgba8.png"), 6, 8, false, 7, 5),
        ("rgba16", include_bytes!("testdata/png-rgba16.png"), 6, 16, false, 7, 5),
        ("g2-key", include_bytes!("testdata/png-g2-key.png"), 0, 2, true, 7, 5),
        ("g8-key", include_bytes!("testdata/png-g8-key.png"), 0, 8, true, 7, 5),
        ("g16-key", include_bytes!("testdata/png-g16-key.png"), 0, 16, true, 7, 5),
        ("rgb8-key", include_bytes!("testdata/png-rgb8-key.png"), 2, 8, true, 7, 5),
        ("rgb16-key", include_bytes!("testdata/png-rgb16-key.png"), 2, 16, true, 7, 5),
        ("i-rgba8-1x1", include_bytes!("testdata/png-i-rgba8-1x1.png"), 6, 8, false, 1, 1),
        ("i-rgba8-3x2", include_bytes!("testdata/png-i-rgba8-3x2.png"), 6, 8, false, 3, 2),
        ("i-rgba8-13x9", include_bytes!("testdata/png-i-rgba8-13x9.png"), 6, 8, false, 13, 9),
        ("i-g1-13x9", include_bytes!("testdata/png-i-g1-13x9.png"), 0, 1, false, 13, 9),
        ("i-p4-7x5", include_bytes!("testdata/png-i-p4-7x5.png"), 3, 4, false, 7, 5),
        ("i-rgb16-7x5", include_bytes!("testdata/png-i-rgb16-7x5.png"), 2, 16, false, 7, 5),
        ("i-ga8-9x10", include_bytes!("testdata/png-i-ga8-9x10.png"), 4, 8, false, 9, 10),
        ("multi-idat", include_bytes!("testdata/png-multi-idat.png"), 6, 8, false, 13, 9),
        ("iccp", include_bytes!("testdata/png-iccp.png"), 2, 8, false, 7, 5),
        ("noiend", include_bytes!("testdata/png-noiend.png"), 2, 8, false, 7, 5),
        ("pillow", include_bytes!("testdata/png-pillow.png"), 2, 8, false, 13, 9),
        ("apng", include_bytes!("testdata/png-apng.png"), 6, 8, false, 7, 5),
    ];

    #[test]
    fn every_fixture_decodes_to_its_formula() {
        for &(name, data, ct, d, trns, w, h) in FIXTURES {
            let img = decode(data).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!((img.w, img.h, img.px.len()), (w, h, w * h * 4), "{name}");
            for y in 0..h {
                for x in 0..w {
                    let at = (y * w + x) * 4;
                    assert_eq!(img.px[at..at + 4], expected(ct, d, trns, x, y), "{name} at ({x}, {y})");
                }
            }
            let icc: Option<Vec<u8>> = (name == "iccp").then(|| (0..300).map(|i| ((i * 7 + 3) % 256) as u8).collect());
            assert_eq!(img.icc, icc, "{name}");
        }
    }

    #[test]
    fn errors_are_plain_sentences() {
        assert_eq!(decode(b"GIF89a").unwrap_err(), "not a PNG file");
        let rgb = FIXTURES[5].1;
        // Bit depth 3.
        let mut bad = rgb.to_vec();
        bad[24] = 3;
        assert_eq!(decode(&bad).unwrap_err(), "unsupported PNG bit depth 3 (color type 2)");
        // A width of 0, then a size over the pixel limit.
        let mut bad = rgb.to_vec();
        bad[16..20].copy_from_slice(&[0, 0, 0, 0]);
        assert!(decode(&bad).unwrap_err().contains("no pixels"));
        bad[16..24].copy_from_slice(&[0, 1, 0, 0, 0, 1, 0, 0]);
        assert!(decode(&bad).unwrap_err().contains("too large"));
        // Cut in the image data.
        let idat = rgb.windows(4).position(|w| w == b"IDAT").unwrap();
        assert_eq!(decode(&rgb[..idat + 20]).unwrap_err(), TRUNCATED);
        // A claimed size much larger than the data: an error, without a large allocation.
        let mut big = rgb.to_vec();
        big[16..24].copy_from_slice(&[0, 0, 0x27, 0x10, 0, 0, 0x27, 0x10]);
        assert_eq!(decode(&big).unwrap_err(), TRUNCATED);
    }

    /// Every prefix and a few hundred one-byte changes of every fixture: Ok or Err, never a panic.
    #[test]
    fn truncated_and_corrupt_files_never_panic() {
        let mut s = 7u32;
        for &(name, data, ..) in FIXTURES {
            for cut in 0..data.len() {
                let r = std::panic::catch_unwind(|| decode(&data[..cut]));
                assert!(r.is_ok(), "{name}: panic when cut at {cut}");
            }
            for _ in 0..300 {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                let mut bad = data.to_vec();
                let at = (s >> 8) as usize % bad.len();
                bad[at] ^= (s >> 24) as u8 | 1;
                let r = std::panic::catch_unwind(|| decode(&bad));
                assert!(r.is_ok(), "{name}: panic with byte {at} changed");
            }
        }
    }
}

//! BMP decoder (zero crates): the Windows headers (BITMAPINFOHEADER and its V2 to V5 versions)
//! and the OS/2 1.x core header; 1, 4 and 8-bit palette images, 16-bit (5-5-5, or any masks
//! with BI_BITFIELDS), 24-bit, 32-bit, RLE8 and RLE4; rows bottom-up or top-down. Channels
//! narrower than 8 bits are scaled with rounding (5 bits: v * 255 / 31).
//!
//! Alpha: an alpha mask (V3 and later headers, or BI_ALPHABITFIELDS) gives alpha, and so does
//! the fourth byte of a plain (BI_RGB) 32-bit image, but only if some pixel has a nonzero one:
//! many writers leave that byte 0, and such files are opaque (the rule Chromium uses). Pixels
//! an RLE image skips (with a delta or an early end of line) are transparent, as in browsers.
//! Palette indices past the palette are black. JPEG/PNG-in-BMP and OS/2 2.x compression are
//! not supported.

use super::common::{check_size, Image, Res};

const TRUNCATED: &str = "truncated BMP data";

/// Decodes a BMP file to RGBA8.
pub fn decode(data: &[u8]) -> Res<Image> {
    if data.len() < 2 || &data[..2] != b"BM" {
        return Err("not a BMP file".into());
    }
    if data.len() < 18 {
        return Err(TRUNCATED.into());
    }
    let off = le32(data, 10) as usize;
    let hs = le32(data, 14) as usize;
    let header_end = 14usize.checked_add(hs).filter(|&e| e <= data.len()).ok_or(TRUNCATED)?;
    let core = hs == 12;
    let (w, h, bpp, comp, ncolors) = if core {
        (i64::from(le16(data, 18)), i64::from(le16(data, 20)), le16(data, 24), 0, 0)
    } else if hs >= 40 {
        let w = i64::from(le32(data, 18) as i32);
        let h = i64::from(le32(data, 22) as i32);
        (w, h, le16(data, 28), le32(data, 30), le32(data, 46) as usize)
    } else {
        return Err(format!("unsupported BMP header size {hs}"));
    };
    match (comp, bpp) {
        (0, 1 | 4 | 8 | 16 | 24 | 32) | (1, 8) | (2, 4) | (3 | 6, 16 | 32) if hs != 64 || comp < 3 => {}
        (0..=6, _) => return Err(format!("unsupported BMP format ({bpp} bits per pixel, compression {comp})")),
        _ => return Err(format!("unsupported BMP compression {comp}")),
    }
    if w <= 0 || h == 0 || h == i64::from(i32::MIN) {
        return Err(format!("corrupt BMP data (a size of {w} x {h})"));
    }
    let top_down = h < 0;
    let (w, h) = (w as usize, h.unsigned_abs() as usize);
    check_size(w, h)?;
    if top_down && (comp == 1 || comp == 2) {
        return Err("corrupt BMP data (a top-down RLE image)".into());
    }

    // The channel masks: BI_BITFIELDS puts them right after a 40-byte header, or in it (V2+),
    // at the same place in the file either way.
    let mut masks = match bpp {
        16 => [0x7C00, 0x03E0, 0x001F, 0],
        _ => [0xFF_0000, 0xFF00, 0xFF, 0xFF00_0000],
    };
    let mut pal_start = header_end;
    if comp == 3 || comp == 6 {
        let n = if comp == 6 || hs >= 56 { 4 } else { 3 };
        let m = data.get(54..54 + 4 * n).ok_or(TRUNCATED)?;
        masks = [le32(m, 0), le32(m, 4), le32(m, 8), if n == 4 { le32(m, 12) } else { 0 }];
        if hs == 40 {
            pal_start += 4 * n;
        }
    }
    let alpha = (bpp == 16 || bpp == 32) && masks[3] != 0;

    let mut lut = [[0u8, 0, 0, 255]; 256];
    if bpp <= 8 {
        let max = 1usize << bpp;
        let n = if ncolors == 0 || ncolors > max { max } else { ncolors };
        let entry = if core { 3 } else { 4 };
        let pal = data.get(pal_start..).unwrap_or(&[]);
        for (e, c) in lut.iter_mut().zip(pal.chunks_exact(entry).take(n)) {
            *e = [c[2], c[1], c[0], 255];
        }
    }
    if off < header_end {
        return Err("corrupt BMP data (the pixels start inside the header)".into());
    }
    if off > data.len() {
        return Err(TRUNCATED.into());
    }
    let mut px = if comp == 1 || comp == 2 {
        rle(data, off, w, h, comp == 1, &lut)?
    } else {
        pixels(data, off, w, h, bpp as usize, top_down, &lut, &masks)?
    };
    // A 32-bit image whose alpha is 0 everywhere has no alpha.
    if alpha && px.iter().skip(3).step_by(4).all(|&a| a == 0) {
        px.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
    }
    Ok(Image { w, h, px, icc: None })
}

fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// A channel of a 16 or 32-bit pixel: where its mask starts and how many bits it has (the run
/// of ones from the lowest set bit; 0 for no mask).
#[derive(Clone, Copy)]
struct Channel {
    shift: u32,
    bits: u32,
}

impl Channel {
    fn new(mask: u32) -> Channel {
        if mask == 0 {
            return Channel { shift: 0, bits: 0 };
        }
        let shift = mask.trailing_zeros();
        Channel { shift, bits: (mask >> shift).trailing_ones() }
    }

    /// The channel of pixel `v` scaled to 8 bits (`none` when there is no mask).
    #[inline]
    fn get(self, v: u32, none: u8) -> u8 {
        if self.bits == 0 {
            return none;
        }
        let max = (1u64 << self.bits) - 1;
        let x = u64::from(v >> self.shift) & max;
        if self.bits == 8 {
            x as u8
        } else {
            ((x * 255 + max / 2) / max) as u8
        }
    }
}

/// The pixels of an uncompressed image: rows padded to 4 bytes, from `off`.
#[allow(clippy::too_many_arguments)]
fn pixels(
    data: &[u8],
    off: usize,
    w: usize,
    h: usize,
    bpp: usize,
    top_down: bool,
    lut: &[[u8; 4]; 256],
    masks: &[u32; 4],
) -> Res<Vec<u8>> {
    let row_bytes = w.checked_mul(bpp).ok_or(TRUNCATED)?.div_ceil(8);
    let stride = row_bytes.div_ceil(4) * 4;
    // The last row's padding may be missing.
    let need = stride.checked_mul(h - 1).and_then(|n| n.checked_add(row_bytes)).ok_or(TRUNCATED)?;
    if data.len() - off < need {
        return Err(TRUNCATED.into());
    }
    let ch = masks.map(Channel::new);
    let mut px = vec![0u8; w * h * 4];
    for r in 0..h {
        let src = &data[off + r * stride..off + r * stride + row_bytes];
        let y = if top_down { r } else { h - 1 - r };
        let dst = &mut px[y * w * 4..(y + 1) * w * 4];
        let dst = dst.as_chunks_mut::<4>().0.iter_mut();
        let rgba = |v: u32| [ch[0].get(v, 0), ch[1].get(v, 0), ch[2].get(v, 0), ch[3].get(v, 255)];
        match bpp {
            1 | 4 | 8 => {
                let (per, mask) = (8 / bpp, ((1u16 << bpp) - 1) as u8);
                for (i, o) in dst.enumerate() {
                    let shift = 8 - bpp * (i % per + 1);
                    *o = lut[((src[i / per] >> shift) & mask) as usize];
                }
            }
            24 => dst.zip(src.as_chunks::<3>().0).for_each(|(o, &[b, g, r])| *o = [r, g, b, 255]),
            16 => dst.zip(src.as_chunks::<2>().0).for_each(|(o, &s)| *o = rgba(u32::from(u16::from_le_bytes(s)))),
            _ => dst.zip(src.as_chunks::<4>().0).for_each(|(o, &s)| *o = rgba(u32::from_le_bytes(s))),
        }
    }
    Ok(px)
}

/// The pixels of an RLE8 or RLE4 image (always bottom-up). Pixels it never sets stay transparent.
fn rle(data: &[u8], mut p: usize, w: usize, h: usize, rle8: bool, lut: &[[u8; 4]; 256]) -> Res<Vec<u8>> {
    let mut px = vec![0u8; w * h * 4];
    // `y` counts rows from the bottom.
    let (mut x, mut y) = (0usize, 0usize);
    let mut put = |x: usize, y: usize, index: u8| {
        if x < w && y < h {
            let at = ((h - 1 - y) * w + x) * 4;
            px[at..at + 4].copy_from_slice(&lut[index as usize]);
        }
    };
    let nibble = |b: u8, i: usize| if i.is_multiple_of(2) { b >> 4 } else { b & 15 };
    while y < h {
        let (Some(&n), Some(&b)) = (data.get(p), data.get(p + 1)) else {
            // No end-of-bitmap code: fine if the last row is complete.
            if y == h - 1 && x >= w {
                break;
            }
            return Err(TRUNCATED.into());
        };
        p += 2;
        if n > 0 {
            for i in 0..n as usize {
                put(x + i, y, if rle8 { b } else { nibble(b, i) });
            }
            x += n as usize;
            continue;
        }
        match b {
            0 => (x, y) = (0, y + 1),
            1 => break,
            2 => {
                let d = data.get(p..p + 2).ok_or(TRUNCATED)?;
                (x, y) = (x + d[0] as usize, y + d[1] as usize);
                p += 2;
            }
            n => {
                let n = n as usize;
                let bytes = if rle8 { n } else { n.div_ceil(2) };
                let src = data.get(p..p + bytes).ok_or(TRUNCATED)?;
                for i in 0..n {
                    put(x + i, y, if rle8 { src[i] } else { nibble(src[i / 2], i) });
                }
                x += n;
                p += bytes + (bytes & 1); // runs are padded to 16 bits
            }
        }
    }
    Ok(px)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Name, file, expected RGBA, width, height.
    type Fixture = (&'static str, &'static [u8], &'static [u8], usize, usize);

    const FIXTURES: &[Fixture] = &[
        ("rgb24", include_bytes!("testdata/bmp-rgb24.bmp"), include_bytes!("testdata/bmp-rgb24.rgba"), 7, 5),
        ("rgba32", include_bytes!("testdata/bmp-rgba32.bmp"), include_bytes!("testdata/bmp-rgba32.rgba"), 7, 5),
        ("p8", include_bytes!("testdata/bmp-p8.bmp"), include_bytes!("testdata/bmp-p8.rgba"), 13, 9),
        ("p1", include_bytes!("testdata/bmp-p1.bmp"), include_bytes!("testdata/bmp-p1.rgba"), 13, 9),
        ("p4", include_bytes!("testdata/bmp-p4.bmp"), include_bytes!("testdata/bmp-p4.rgba"), 13, 9),
        ("topdown24", include_bytes!("testdata/bmp-topdown24.bmp"), include_bytes!("testdata/bmp-topdown24.rgba"), 7, 5),
        ("565", include_bytes!("testdata/bmp-565.bmp"), include_bytes!("testdata/bmp-565.rgba"), 7, 5),
        ("555", include_bytes!("testdata/bmp-555.bmp"), include_bytes!("testdata/bmp-555.rgba"), 7, 5),
        ("v5-argb", include_bytes!("testdata/bmp-v5-argb.bmp"), include_bytes!("testdata/bmp-v5-argb.rgba"), 7, 5),
        ("rle8", include_bytes!("testdata/bmp-rle8.bmp"), include_bytes!("testdata/bmp-rle8.rgba"), 9, 6),
        ("rle4", include_bytes!("testdata/bmp-rle4.bmp"), include_bytes!("testdata/bmp-rle4.rgba"), 7, 3),
        ("core8", include_bytes!("testdata/bmp-core8.bmp"), include_bytes!("testdata/bmp-core8.rgba"), 7, 5),
    ];

    #[test]
    fn every_fixture_decodes() {
        for &(name, data, want, w, h) in FIXTURES {
            let img = decode(data).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!((img.w, img.h), (w, h), "{name}");
            assert!(img.px == want, "{name}");
        }
    }

    #[test]
    fn errors_and_alpha() {
        assert_eq!(decode(b"GIF89a").unwrap_err(), "not a BMP file");
        let rgb24 = FIXTURES[0].1;
        assert_eq!(decode(&rgb24[..rgb24.len() - 4]).unwrap_err(), TRUNCATED);
        // The last row's padding may be missing (7 x 3 bytes of a 24-byte row: 3 bytes).
        assert!(decode(&rgb24[..rgb24.len() - 3]).is_ok());
        let mut bad = rgb24.to_vec();
        bad[18..22].copy_from_slice(&(-7i32).to_le_bytes());
        assert!(decode(&bad).unwrap_err().contains("size of -7 x 5"));
        bad[18..26].copy_from_slice(&[0, 0, 1, 0, 0, 0, 1, 0]);
        assert!(decode(&bad).unwrap_err().contains("too large"));
        let mut bad = rgb24.to_vec();
        bad[28] = 2;
        assert!(decode(&bad).unwrap_err().contains("unsupported BMP format (2 bits"));
        // A 32-bit BI_RGB file with every alpha byte 0 is opaque.
        let mut zero = FIXTURES[1].1.to_vec();
        let off = le32(&zero, 10) as usize;
        for i in (off + 3..zero.len()).step_by(4) {
            zero[i] = 0;
        }
        assert!(decode(&zero).unwrap().px.chunks(4).all(|p| p[3] == 255));
    }

    #[test]
    fn truncated_and_corrupt_files_never_panic() {
        let mut s = 3u32;
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

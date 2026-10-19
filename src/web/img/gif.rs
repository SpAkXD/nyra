//! GIF decoder (zero crates): the first frame, drawn on a transparent canvas the size of the
//! logical screen (or of the frame, when the screen size is 0). Global and local color tables,
//! the transparent color of a Graphic Control Extension, interlaced frames, and LZW with code
//! sizes growing to 12 bits, clear codes anywhere and a full table that is not cleared. Other
//! extensions (comments, NETSCAPE looping, plain text) are skipped.
//!
//! Lenient like browsers: an LZW stream that ends early, cleanly, leaves the rest of the frame
//! transparent, and data after the last pixel is ignored. A file cut off inside the frame data
//! is an error ("truncated GIF data") unless every pixel was already decoded. Pixels whose
//! color index is past the color table are opaque black; parts of a frame outside the screen
//! are clipped.

use super::common::{check_size, Image, Res};

const TRUNCATED: &str = "truncated GIF data";

/// Decodes the first frame of a GIF file to RGBA8.
pub fn decode(data: &[u8]) -> Res<Image> {
    if data.len() < 6 || (&data[..6] != b"GIF87a" && &data[..6] != b"GIF89a") {
        return Err("not a GIF file".into());
    }
    let d = data.get(6..13).ok_or(TRUNCATED)?;
    let (sw, sh, flags) = (le16(d, 0), le16(d, 2), d[4]);
    let mut pos = 13;
    let mut global: &[u8] = &[];
    if flags & 0x80 != 0 {
        let n = 3 << ((flags & 7) + 1);
        global = data.get(pos..pos + n).ok_or(TRUNCATED)?;
        pos += n;
    }
    let mut transparent = None;
    loop {
        let &block = data.get(pos).ok_or(TRUNCATED)?;
        pos += 1;
        match block {
            0x21 => {
                let &label = data.get(pos).ok_or(TRUNCATED)?;
                pos += 1;
                if label == 0xF9 {
                    // Graphic Control Extension: size 4, flags, delay (2), transparent index.
                    if let Some(g) = data.get(pos..pos + 5).filter(|g| g[0] >= 4) {
                        transparent = (g[1] & 1 != 0).then_some(g[4]);
                    }
                }
                pos = skip_sub_blocks(data, pos)?;
            }
            0x2C => return frame(data, pos, sw, sh, global, transparent),
            0x3B => return Err("the GIF has no image".into()),
            _ => return Err(format!("corrupt GIF data (unknown block type 0x{block:02x})")),
        }
    }
}

fn le16(b: &[u8], i: usize) -> usize {
    usize::from(b[i]) | usize::from(b[i + 1]) << 8
}

/// The position after a run of data sub-blocks (each a length byte and that many bytes, ending
/// with a zero length).
fn skip_sub_blocks(data: &[u8], mut pos: usize) -> Res<usize> {
    loop {
        let &n = data.get(pos).ok_or(TRUNCATED)?;
        pos += 1 + n as usize;
        if n == 0 {
            return Ok(pos);
        }
        if pos > data.len() {
            return Err(TRUNCATED.into());
        }
    }
}

/// Decodes the frame whose image descriptor starts at `pos` and draws it on the canvas.
fn frame(data: &[u8], mut pos: usize, sw: usize, sh: usize, global: &[u8], transparent: Option<u8>) -> Res<Image> {
    let d = data.get(pos..pos + 9).ok_or(TRUNCATED)?;
    let (left, top, fw, fh, flags) = (le16(d, 0), le16(d, 2), le16(d, 4), le16(d, 6), d[8]);
    pos += 9;
    let mut colors = global;
    if flags & 0x80 != 0 {
        let n = 3 << ((flags & 7) + 1);
        colors = data.get(pos..pos + n).ok_or(TRUNCATED)?;
        pos += n;
    }
    if colors.is_empty() {
        return Err("the GIF has no color table".into());
    }
    let (w, h) = if sw == 0 || sh == 0 { (left + fw, top + fh) } else { (sw, sh) };
    check_size(w, h)?;
    let &min_size = data.get(pos).ok_or(TRUNCATED)?;
    pos += 1;
    if !(2..=8).contains(&min_size) {
        return Err(format!("unsupported GIF LZW code size {min_size}"));
    }
    // Join the sub-blocks; a file cut off inside them keeps what is there.
    let mut lzw = Vec::new();
    let mut cut = false;
    loop {
        let Some(&n) = data.get(pos) else {
            cut = true;
            break;
        };
        if n == 0 {
            break;
        }
        let end = (pos + 1 + n as usize).min(data.len());
        lzw.extend_from_slice(&data[pos + 1..end]);
        pos = end;
    }

    let mut px = vec![0u8; w * h * 4];
    let mut canvas =
        Canvas { px: &mut px, w, h, left, top, fw, colors, transparent, x: 0, row: 0, y: 0, fh, interlaced: flags & 0x40 != 0 };
    let total = fw * fh;
    let done = if total == 0 { 0 } else { lzw_decode(&lzw, min_size, total, &mut canvas)? };
    if done < total && cut {
        return Err(TRUNCATED.into());
    }
    Ok(Image { w, h, px, icc: None })
}

/// Where the decoded color indices go: frame pixels in order, drawn on the canvas.
struct Canvas<'a> {
    px: &'a mut [u8],
    w: usize,
    h: usize,
    left: usize,
    top: usize,
    fw: usize,
    fh: usize,
    colors: &'a [u8],
    transparent: Option<u8>,
    interlaced: bool,
    /// The next pixel: column, decoded row (in file order) and its canvas row in the frame.
    x: usize,
    row: usize,
    y: usize,
}

impl Canvas<'_> {
    #[inline]
    fn put(&mut self, index: u8) {
        if Some(index) != self.transparent {
            let (cx, cy) = (self.left + self.x, self.top + self.y);
            if cx < self.w && cy < self.h {
                let i = index as usize * 3;
                let rgb = self.colors.get(i..i + 3).unwrap_or(&[0, 0, 0]);
                let at = (cy * self.w + cx) * 4;
                self.px[at..at + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        self.x += 1;
        if self.x == self.fw {
            self.x = 0;
            self.row += 1;
            self.y = if self.interlaced { interlaced_row(self.row, self.fh) } else { self.row };
        }
    }
}

/// The frame row that the `r`-th stored row of an interlaced frame of height `h` belongs to:
/// every 8th row from 0, every 8th from 4, every 4th from 2, then every 2nd from 1.
fn interlaced_row(mut r: usize, h: usize) -> usize {
    for (first, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
        let n = if h > first { (h - first).div_ceil(step) } else { 0 };
        if r < n {
            return first + r * step;
        }
        r -= n;
    }
    h // past the last row
}

/// Decodes the LZW codes of `data` into `canvas`, up to `total` pixels; returns the pixel count.
fn lzw_decode(data: &[u8], min_size: u8, total: usize, canvas: &mut Canvas) -> Res<usize> {
    let clear = 1usize << min_size;
    let end = clear + 1;
    let mut prefix = [0u16; 4096];
    let mut suffix = [0u8; 4096];
    let mut first = [0u8; 4096];
    let mut len = [0u16; 4096];
    for i in 0..clear {
        (suffix[i], first[i], len[i]) = (i as u8, i as u8, 1);
    }
    let mut size = u32::from(min_size) + 1;
    let mut next = clear + 2;
    let mut prev: Option<usize> = None;
    let (mut acc, mut nbits, mut pos) = (0u32, 0u32, 0usize);
    let mut stack = [0u8; 4096];
    let mut done = 0;
    while done < total {
        while nbits < size {
            let Some(&b) = data.get(pos) else { return Ok(done) };
            acc |= u32::from(b) << nbits;
            nbits += 8;
            pos += 1;
        }
        let code = (acc & ((1 << size) - 1)) as usize;
        acc >>= size;
        nbits -= size;
        if code == clear {
            size = u32::from(min_size) + 1;
            next = clear + 2;
            prev = None;
            continue;
        }
        if code == end {
            return Ok(done);
        }
        let out_code = match prev {
            None if code < clear => code,
            None => return Err("corrupt GIF data (bad LZW code)".into()),
            Some(p) => {
                if code > next {
                    return Err("corrupt GIF data (bad LZW code)".into());
                }
                if next < 4096 {
                    // The new entry is the previous string plus the first byte of this one
                    // (of the previous one, when this code is the entry being made).
                    let f = if code == next { first[p] } else { first[code] };
                    (prefix[next], suffix[next], first[next], len[next]) = (p as u16, f, first[p], len[p] + 1);
                    next += 1;
                    if next == 1 << size && size < 12 {
                        size += 1;
                    }
                }
                code
            }
        };
        prev = Some(out_code);
        // Write the string backwards into the stack, then out in order.
        let n = len[out_code] as usize;
        let mut c = out_code;
        for i in (0..n).rev() {
            stack[i] = suffix[c];
            c = prefix[c] as usize;
        }
        for &index in &stack[..n.min(total - done)] {
            canvas.put(index);
        }
        done += n.min(total - done);
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &[(&str, &[u8], &[u8])] = &[
        ("trans", include_bytes!("testdata/gif-trans.gif"), include_bytes!("testdata/gif-trans.rgba")),
        ("plain", include_bytes!("testdata/gif-plain.gif"), include_bytes!("testdata/gif-plain.rgba")),
        ("interlace", include_bytes!("testdata/gif-interlace.gif"), include_bytes!("testdata/gif-interlace.rgba")),
        ("pillow", include_bytes!("testdata/gif-pillow.gif"), include_bytes!("testdata/gif-pillow.rgba")),
    ];
    const LZW12: &[u8] = include_bytes!("testdata/gif-lzw12.gif");

    /// The pixels of gif-lzw12.gif (`rnd` and `pal` in tools/img_fixtures_png.py).
    fn lzw12_pixels() -> Vec<u8> {
        let mut px = Vec::new();
        for i in 0..128 * 96u32 {
            let mut v = i.wrapping_mul(0x9E37_79B1);
            v ^= v >> 15;
            v = v.wrapping_mul(0x85EB_CA77);
            v ^= v >> 13;
            let c = (v & 15) as usize * 3 + 1;
            px.extend_from_slice(&[((c * 29 + 7) % 256) as u8, ((c * 71 + 3) % 256) as u8, ((c * 113 + 5) % 256) as u8, 255]);
        }
        px
    }

    /// Fully transparent pixels compare equal whatever their color.
    fn same(a: &[u8], b: &[u8]) -> bool {
        a.len() == b.len() && a.chunks(4).zip(b.chunks(4)).all(|(p, q)| p == q || (p[3] == 0 && q[3] == 0))
    }

    #[test]
    fn every_fixture_decodes() {
        let sizes = [(7, 5), (7, 5), (13, 11), (9, 7)];
        for (&(name, data, want), (w, h)) in FIXTURES.iter().zip(sizes) {
            let img = decode(data).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!((img.w, img.h), (w, h), "{name}");
            assert!(same(&img.px, want), "{name}");
        }
        // A full 4096-entry table used for 600 codes without a clear code, then a clear.
        let img = decode(LZW12).unwrap();
        assert_eq!((img.w, img.h), (128, 96));
        assert!(img.px == lzw12_pixels());
    }

    #[test]
    fn errors_and_leniency() {
        assert_eq!(decode(b"\x89PNG").unwrap_err(), "not a GIF file");
        assert_eq!(decode(b"GIF89a\x01\x00").unwrap_err(), TRUNCATED);
        assert_eq!(decode(b"GIF89a\x01\x00\x01\x00\x00\x00\x00\x3b").unwrap_err(), "the GIF has no image");
        // Cut in the frame data: an error. Without the trailer: fine.
        let plain = FIXTURES[1].1;
        assert_eq!(decode(&plain[..plain.len() - 10]).unwrap_err(), TRUNCATED);
        assert!(same(&decode(&plain[..plain.len() - 1]).unwrap().px, FIXTURES[1].2));
        // An LZW stream that ends at once (a clear code, then the end code): all transparent.
        let mut gif = b"GIF89a\x02\x00\x02\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff".to_vec();
        gif.extend_from_slice(b"\x2c\x00\x00\x00\x00\x02\x00\x02\x00\x00\x02\x01\x2c\x00\x3b");
        let img = decode(&gif).unwrap();
        assert!(img.px.iter().all(|&v| v == 0));
        // The interlaced row order.
        let rows: Vec<usize> = (0..11).map(|r| interlaced_row(r, 11)).collect();
        assert_eq!(rows, [0, 8, 4, 2, 6, 10, 1, 3, 5, 7, 9]);
    }

    #[test]
    fn truncated_and_corrupt_files_never_panic() {
        let mut s = 11u32;
        let all = FIXTURES.iter().map(|f| (f.0, f.1)).chain([("lzw12", LZW12)]);
        for (name, data) in all {
            let step = if data.len() > 1000 { 7 } else { 1 };
            for cut in (0..data.len()).step_by(step) {
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

//! JPEG decoding (zero crates): baseline, extended sequential (8-bit Huffman) and progressive
//! files, any chroma subsampling with integer ratios (4:4:4, 4:2:2, 4:2:0, 4:4:0, 4:1:1, ...),
//! restart markers, the EXIF orientation, grayscale, Adobe RGB, CMYK and YCCK files, and the ICC
//! profile of the APP2 chunks.
//!
//! [`decode`] can scale by 1/2, 1/4 or 1/8 inside the IDCT, like libjpeg's `scale_denom`, so a
//! 50-megapixel photo that is needed 2000 pixels wide never exists at full size in memory. The
//! output follows libjpeg-turbo's defaults, and so Pillow's, within a level or two: the accurate
//! integer IDCT (islow), the reduced IDCTs of `jidctred.c` for scaled decodes, fancy (triangle)
//! upsampling of 2x chroma, replication for other ratios, and the same fixed-point YCbCr to RGB.
//!
//! Memory: a baseline file whose first scan holds all components is decoded as a stream of MCU
//! rows, and the only large allocation is the output image. Progressive and multi-scan files
//! keep their coefficients (16 bits each; only the DC term of a component decoded at 1/8).
//!
//! Damaged files: an error in the headers before the first scan is an `Err`. When the
//! entropy-coded data ends early, has a bad Huffman code or loses a restart interval, the decoded
//! part is kept and the missing blocks come out flat gray, the way a browser shows a partly loaded
//! JPEG; a damaged restart marker is resynchronized like libjpeg does. A broken header after the
//! first scan (a truncated progressive file) ends the decode with what is there. Arithmetic
//! coding, 12-bit samples, lossless and hierarchical JPEG are refused with an error.

use super::common::{check_size, Image, Res, MAX_PIXELS};

/// Where the k-th coefficient of the zigzag order sits in a row-major 8x8 block.
pub(super) const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49,
    56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// At most this many scans in one file (a progressive JPEG has about 10), so a file of many
/// tiny scans cannot keep the decoder busy for minutes.
const MAX_SCANS: usize = 1000;
/// The coefficient memory a progressive or multi-scan decode may take, in bytes.
const MAX_COEF_BYTES: usize = MAX_PIXELS * 4;
/// Huffman codes up to this many bits are decoded with one table lookup.
const FAST: usize = 9;
const TRUNCATED: &str = "truncated JPEG header";

/// What [`info`] reads from the headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    /// The width as displayed, after the EXIF orientation.
    pub w: usize,
    /// The height as displayed, after the EXIF orientation.
    pub h: usize,
    /// The EXIF orientation, 1..=8 (1 when the file has none).
    pub orientation: u8,
    /// True for a progressive JPEG.
    pub progressive: bool,
    /// 1 (gray), 3 (YCbCr or RGB) or 4 (CMYK or YCCK).
    pub components: u8,
    /// True if the file has a complete ICC profile.
    pub icc: bool,
}

impl Info {
    /// The scale [`decode`] uses for a wanted size: the largest of 8, 4, 2 and 1 that keeps the
    /// displayed size at least `want_w` x `want_h` (a 0 does not constrain; both 0 means 1).
    pub fn scale_for(&self, want_w: usize, want_h: usize) -> usize {
        if want_w == 0 && want_h == 0 {
            return 1;
        }
        [8, 4, 2].into_iter().find(|&s| self.w.div_ceil(s) >= want_w && self.h.div_ceil(s) >= want_h).unwrap_or(1)
    }

    /// The displayed size of a decode at 1/`s`: each side divided by `s`, rounded up.
    pub fn scaled_size(&self, s: usize) -> (usize, usize) {
        (self.w.div_ceil(s), self.h.div_ceil(s))
    }
}

/// Reads the headers (up to the first scan) without decoding the image.
pub fn info(data: &[u8]) -> Res<Info> {
    let mut r = Reader::new(data)?;
    let mut h = Headers::default();
    while let Some(m) = r.marker() {
        match m {
            0xDA | 0xD9 => break,
            0xD8 | 0x01 => {}
            _ => {
                let s = r.segment()?;
                h.segment(m, s)?;
            }
        }
    }
    h.info()
}

/// Decodes a JPEG to RGBA, turned upright by its EXIF orientation. With a wanted size it decodes
/// at 1/2, 1/4 or 1/8 instead: the smallest of those whose displayed size is still at least
/// `want_w` x `want_h` ([`Info::scale_for`]); `0, 0` decodes at full size.
pub fn decode(data: &[u8], want_w: usize, want_h: usize) -> Res<Image> {
    let mut d = Dec { r: Reader::new(data)?, h: Headers::default(), want: (want_w, want_h), st: None, scans: 0 };
    while let Some(m) = d.r.marker() {
        let step = match m {
            0xD9 => break,
            0xD8 | 0x01 => Ok(false),
            0xDA => d.r.segment().and_then(|s| d.scan(s)),
            _ => d.r.segment().and_then(|s| d.h.segment(m, s).map(|_| false)),
        };
        match step {
            Ok(false) => {}
            Ok(true) => break,
            // after the first scan, a broken header ends the image (a truncated progressive file)
            Err(_) if d.st.is_some() => break,
            Err(e) => return Err(e),
        }
    }
    d.finish()
}

// ---------------------------------------------------------------------------------------------
// Headers

struct Comp {
    id: u8,
    /// Sampling factors.
    h: usize,
    v: usize,
    /// Quantization table number.
    tq: usize,
    /// Blocks per row and per column, padded to whole MCUs.
    bw: usize,
    bh: usize,
    /// Samples per row and per column (full size).
    cw: usize,
    ch: usize,
}

struct Frame {
    w: usize,
    h: usize,
    progressive: bool,
    comps: Vec<Comp>,
    hmax: usize,
    vmax: usize,
    mcux: usize,
    mcuy: usize,
}

#[derive(Default)]
struct Headers<'a> {
    frame: Option<Frame>,
    qt: [Option<[i32; 64]>; 4],
    dc: [Option<Box<Huff>>; 4],
    ac: [Option<Box<Huff>>; 4],
    restart: usize,
    /// 0 when the file has no EXIF orientation.
    orientation: u8,
    adobe: Option<u8>,
    jfif: bool,
    /// ICC chunks: (sequence number, count, bytes).
    icc: Vec<(u8, u8, &'a [u8])>,
}

fn be16(s: &[u8], i: usize) -> usize {
    (s[i] as usize) << 8 | s[i + 1] as usize
}

impl<'a> Headers<'a> {
    fn segment(&mut self, m: u8, s: &'a [u8]) -> Res<()> {
        match m {
            0xC0..=0xC2 => self.frame(m, s),
            0xC3 => Err("lossless JPEG is not supported".into()),
            0xC5..=0xC7 | 0xDE | 0xDF => Err("hierarchical JPEG is not supported".into()),
            0xC9..=0xCF => Err("arithmetic-coded JPEG is not supported".into()),
            0xC4 => self.dht(s),
            0xDB => self.dqt(s),
            0xDD => {
                if s.len() < 2 {
                    return Err(TRUNCATED.into());
                }
                self.restart = be16(s, 0);
                Ok(())
            }
            0xE0 => {
                self.jfif |= s.starts_with(b"JFIF\0");
                Ok(())
            }
            0xE1 => {
                if let Some(x) = s.strip_prefix(b"Exif\0\0") {
                    if self.orientation == 0 {
                        self.orientation = exif_orientation(x).unwrap_or(0);
                    }
                }
                Ok(())
            }
            0xE2 => {
                if s.len() >= 14 && s.starts_with(b"ICC_PROFILE\0") {
                    self.icc.push((s[12], s[13], &s[14..]));
                }
                Ok(())
            }
            0xEE => {
                if s.len() >= 12 && s.starts_with(b"Adobe") {
                    self.adobe = Some(s[11]);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn frame(&mut self, m: u8, s: &[u8]) -> Res<()> {
        if self.frame.is_some() {
            return Err("the JPEG has more than one frame".into());
        }
        if s.len() < 6 {
            return Err(TRUNCATED.into());
        }
        if s[0] != 8 {
            return Err(format!("{}-bit JPEG is not supported (only 8-bit)", s[0]));
        }
        let (h, w, n) = (be16(s, 1), be16(s, 3), s[5] as usize);
        if h == 0 {
            return Err("a JPEG whose height is set by a DNL marker is not supported".into());
        }
        if w == 0 {
            return Err("the JPEG has a width of 0".into());
        }
        if !matches!(n, 1 | 3 | 4) {
            return Err(format!("a JPEG with {n} components is not supported"));
        }
        if s.len() < 6 + 3 * n {
            return Err(TRUNCATED.into());
        }
        let mut comps = Vec::with_capacity(n);
        for i in 0..n {
            let c = &s[6 + 3 * i..9 + 3 * i];
            let (ch, cv, tq) = ((c[1] >> 4) as usize, (c[1] & 15) as usize, c[2] as usize);
            if !(1..=4).contains(&ch) || !(1..=4).contains(&cv) || tq > 3 {
                return Err("bad JPEG frame header (sampling factors or table number)".into());
            }
            // one component: the sampling factors do not matter (the scan is not interleaved)
            let (ch, cv) = if n == 1 { (1, 1) } else { (ch, cv) };
            comps.push(Comp { id: c[0], h: ch, v: cv, tq, bw: 0, bh: 0, cw: 0, ch: 0 });
        }
        let hmax = comps.iter().map(|c| c.h).max().unwrap_or(1);
        let vmax = comps.iter().map(|c| c.v).max().unwrap_or(1);
        let (mcux, mcuy) = (w.div_ceil(8 * hmax), h.div_ceil(8 * vmax));
        for c in &mut comps {
            c.bw = mcux * c.h;
            c.bh = mcuy * c.v;
            c.cw = (w * c.h).div_ceil(hmax);
            c.ch = (h * c.v).div_ceil(vmax);
        }
        self.frame = Some(Frame { w, h, progressive: m == 0xC2, comps, hmax, vmax, mcux, mcuy });
        Ok(())
    }

    fn dht(&mut self, mut s: &[u8]) -> Res<()> {
        while !s.is_empty() {
            if s.len() < 17 {
                return Err(TRUNCATED.into());
            }
            let (tc, th) = ((s[0] >> 4) as usize, (s[0] & 15) as usize);
            let n: usize = s[1..17].iter().map(|&c| c as usize).sum();
            if tc > 1 || th > 3 || n > 256 {
                return Err("bad JPEG Huffman table".into());
            }
            if s.len() < 17 + n {
                return Err(TRUNCATED.into());
            }
            let t = Some(Box::new(Huff::new(&s[1..17], &s[17..17 + n])?));
            if tc == 0 {
                self.dc[th] = t;
            } else {
                self.ac[th] = t;
            }
            s = &s[17 + n..];
        }
        Ok(())
    }

    fn dqt(&mut self, mut s: &[u8]) -> Res<()> {
        while !s.is_empty() {
            let (pq, tq) = ((s[0] >> 4) as usize, (s[0] & 15) as usize);
            if pq > 1 || tq > 3 {
                return Err("bad JPEG quantization table".into());
            }
            if s.len() < 1 + (64 << pq) {
                return Err(TRUNCATED.into());
            }
            let mut t = [0i32; 64];
            for (k, &z) in ZIGZAG.iter().enumerate() {
                t[z] = if pq == 0 { s[1 + k] as i32 } else { be16(s, 1 + 2 * k) as i32 };
            }
            self.qt[tq] = Some(t);
            s = &s[1 + (64 << pq)..];
        }
        Ok(())
    }

    /// The ICC profile, if all its chunks are there.
    fn icc(&self) -> Option<Vec<u8>> {
        let count = self.icc.first()?.1;
        if count == 0 || self.icc.len() != count as usize {
            return None;
        }
        let mut v = self.icc.clone();
        v.sort_by_key(|c| c.0);
        if v.iter().enumerate().any(|(i, c)| c.0 as usize != i + 1 || c.1 != count) {
            return None;
        }
        Some(v.iter().flat_map(|c| c.2.iter().copied()).collect())
    }

    fn orientation(&self) -> u8 {
        self.orientation.max(1)
    }

    fn info(&self) -> Res<Info> {
        let f = self.frame.as_ref().ok_or("the JPEG has no frame header (SOF)")?;
        let o = self.orientation();
        let (w, h) = if o >= 5 { (f.h, f.w) } else { (f.w, f.h) };
        let icc = self.icc().is_some();
        Ok(Info { w, h, orientation: o, progressive: f.progressive, components: f.comps.len() as u8, icc })
    }

    /// How the components become RGB, like libjpeg's default for the file's markers.
    fn color(&self) -> Color {
        let Some(f) = &self.frame else { return Color::Gray };
        match f.comps.len() {
            1 => Color::Gray,
            3 => {
                let rgb_ids = f.comps[0].id == b'R' && f.comps[1].id == b'G' && f.comps[2].id == b'B';
                if self.jfif {
                    Color::YCbCr
                } else if let Some(t) = self.adobe {
                    if t == 0 {
                        Color::Rgb
                    } else {
                        Color::YCbCr
                    }
                } else if rgb_ids {
                    Color::Rgb
                } else {
                    Color::YCbCr
                }
            }
            _ => match self.adobe {
                None | Some(0) => Color::Cmyk,
                _ => Color::Ycck,
            },
        }
    }
}

/// The orientation (tag 274) of IFD0 of an EXIF block (the TIFF data after "Exif\0\0").
fn exif_orientation(d: &[u8]) -> Option<u8> {
    let le = match d.get(0..4)? {
        b"II*\0" => true,
        b"MM\0*" => false,
        _ => return None,
    };
    let u16_at = |o: usize| -> Option<usize> {
        let b = d.get(o..o.checked_add(2)?)?;
        Some(if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) } as usize)
    };
    let u32_at = |o: usize| -> Option<usize> {
        let b: [u8; 4] = d.get(o..o.checked_add(4)?)?.try_into().ok()?;
        Some(if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) } as usize)
    };
    let ifd = u32_at(4)?;
    for i in 0..u16_at(ifd)? {
        let e = ifd.checked_add(2 + 12 * i)?;
        if u16_at(e)? == 0x112 {
            let v = match u16_at(e + 2)? {
                3 => u16_at(e + 8)?,
                4 => u32_at(e + 8)?,
                _ => return None,
            };
            return (1..=8).contains(&v).then_some(v as u8);
        }
    }
    None
}

/// Walks the markers of the file.
struct Reader<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(d: &'a [u8]) -> Res<Reader<'a>> {
        if !d.starts_with(&[0xFF, 0xD8]) {
            return Err("not a JPEG file (no SOI marker)".into());
        }
        Ok(Reader { d, pos: 2 })
    }

    /// The next marker, skipping whatever is not one (leftover entropy-coded data, fill bytes,
    /// stuffed zeros, restart markers). None at the end of the data.
    fn marker(&mut self) -> Option<u8> {
        loop {
            let i = self.pos + self.d.get(self.pos..)?.iter().position(|&b| b == 0xFF)?;
            let mut j = i + 1;
            while self.d.get(j) == Some(&0xFF) {
                j += 1;
            }
            let m = *self.d.get(j)?;
            self.pos = j + 1;
            if m != 0 && !(0xD0..=0xD7).contains(&m) {
                return Some(m);
            }
        }
    }

    /// The payload of the segment that starts here (after its length).
    fn segment(&mut self) -> Res<&'a [u8]> {
        let p = self.pos;
        if p + 2 > self.d.len() {
            return Err(TRUNCATED.into());
        }
        let n = be16(self.d, p);
        if n < 2 {
            return Err("bad JPEG segment length".into());
        }
        if p + n > self.d.len() {
            return Err(TRUNCATED.into());
        }
        self.pos = p + n;
        Ok(&self.d[p + 2..p + n])
    }
}

// ---------------------------------------------------------------------------------------------
// Huffman decoding

struct Huff {
    /// For the first FAST bits: code length << 8 | symbol, or 0 for a longer code.
    fast: [u16; 1 << FAST],
    /// For AC tables, when the code and its extra bits fit in the first FAST bits and the value
    /// in -128..=127: value << 8 | run << 4 | bits used. Otherwise 0.
    fast_ac: [i16; 1 << FAST],
    /// The largest code of each length (-1 if none).
    maxcode: [i32; 17],
    /// Index of a code's symbol in `vals` minus the code, per length.
    valoff: [i32; 17],
    vals: [u8; 256],
}

impl Huff {
    fn new(counts: &[u8], vals: &[u8]) -> Res<Huff> {
        let mut h = Huff::none();
        h.vals[..vals.len()].copy_from_slice(vals);
        let (mut code, mut k) = (0usize, 0usize);
        for l in 1..=16 {
            let n = counts[l - 1] as usize;
            if code + n > 1 << l {
                return Err("bad JPEG Huffman table".into());
            }
            h.valoff[l] = k as i32 - code as i32;
            if n > 0 {
                h.maxcode[l] = (code + n - 1) as i32;
            }
            if l <= FAST {
                for i in 0..n {
                    let first = (code + i) << (FAST - l);
                    h.fast[first..first + (1 << (FAST - l))].fill((l << 8 | vals[k + i] as usize) as u16);
                }
            }
            code = (code + n) << 1;
            k += n;
        }
        for (i, &e) in h.fast.iter().enumerate() {
            let (l, r, s) = ((e >> 8) as usize, (e >> 4) as usize & 15, e as usize & 15);
            if e != 0 && s != 0 && l + s <= FAST {
                let bits = (i >> (FAST - l - s)) as i32 & ((1 << s) - 1);
                let v = if bits < 1 << (s - 1) { bits - (1 << s) + 1 } else { bits };
                if (-128..=127).contains(&v) {
                    h.fast_ac[i] = (v << 8 | (r << 4 | (l + s)) as i32) as i16;
                }
            }
        }
        Ok(h)
    }

    /// A table that decodes nothing (every read is a bad code).
    fn none() -> Huff {
        Huff { fast: [0; 1 << FAST], fast_ac: [0; 1 << FAST], maxcode: [-1; 17], valoff: [0; 17], vals: [0; 256] }
    }
}

/// The entropy-coded data of a scan, read MSB first through a 64-bit buffer.
struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    buf: u64,
    /// Valid bits in `buf` (left-aligned).
    n: u32,
    /// Zero bits added at the tail of `buf` past the end of the data (saturating).
    fake: u32,
    /// The data has ended: `pos` is at a marker or at the end of the file.
    stop: bool,
    /// A bad Huffman code or value was read.
    bad: bool,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8], pos: usize) -> Bits<'a> {
        Bits { d, pos, buf: 0, n: 0, fake: 0, stop: false, bad: false }
    }

    #[inline]
    fn refill(&mut self) {
        // fast path: the next 8 bytes hold no 0xFF, so take as many whole bytes as fit (the bits
        // of a partly taken byte are already right where it will go later)
        if self.n > 56 {
            return;
        }
        if !self.stop {
            if let Some(b) = self.d.get(self.pos..self.pos + 8) {
                let w = u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
                if (!w).wrapping_sub(0x0101_0101_0101_0101) & w & 0x8080_8080_8080_8080 == 0 {
                    let take = (64 - self.n) / 8;
                    self.buf |= w >> self.n;
                    self.pos += take as usize;
                    self.n += 8 * take;
                    return;
                }
            }
        }
        while self.n <= 56 {
            let mut b = 0u8;
            if !self.stop {
                match self.d.get(self.pos) {
                    Some(&0xFF) => {
                        if self.d.get(self.pos + 1) == Some(&0) {
                            b = 0xFF;
                            self.pos += 2;
                        } else {
                            self.stop = true;
                        }
                    }
                    Some(&x) => {
                        b = x;
                        self.pos += 1;
                    }
                    None => self.stop = true,
                }
            }
            if self.stop {
                self.fake = (self.fake + 8).min(256);
            }
            self.buf |= (b as u64) << (56 - self.n);
            self.n += 8;
        }
    }

    /// No real bits are left before the end of the data.
    fn empty(&mut self) -> bool {
        self.refill();
        self.fake >= self.n
    }

    /// The decoding read past the end of the data or found a bad code.
    fn overrun(&self) -> bool {
        self.fake > self.n || self.bad
    }

    #[inline]
    fn huff(&mut self, h: &Huff) -> u8 {
        if self.n < 16 {
            self.refill();
        }
        let e = h.fast[(self.buf >> (64 - FAST)) as usize];
        if e != 0 {
            let l = (e >> 8) as u32;
            self.buf <<= l;
            self.n -= l;
            return e as u8;
        }
        let p = (self.buf >> 48) as i32;
        for l in FAST + 1..=16 {
            let code = p >> (16 - l);
            if code <= h.maxcode[l] {
                self.buf <<= l;
                self.n -= l as u32;
                let i = h.valoff[l] + code;
                return if (0..256).contains(&i) { h.vals[i as usize] } else { 0 };
            }
        }
        self.bad = true;
        0
    }

    /// `s` bits (1..=16) as an unsigned number.
    #[inline]
    fn bits(&mut self, s: u32) -> i32 {
        if self.n < s {
            self.refill();
        }
        let v = (self.buf >> (64 - s)) as i32;
        self.buf <<= s;
        self.n -= s;
        v
    }

    /// A coefficient of magnitude category `s` (its `s` extra bits, sign-extended).
    #[inline]
    fn extend(&mut self, s: u8) -> i32 {
        if s == 0 {
            return 0;
        }
        if s > 15 {
            self.bad = true;
            return 0;
        }
        let v = self.bits(s as u32);
        if v < 1 << (s - 1) {
            v - (1 << s) + 1
        } else {
            v
        }
    }

    /// A sequential block. `b` is the block, or only its DC term when the AC is not needed.
    fn block(&mut self, b: &mut [i16], dc: &Huff, ac: &Huff, pred: &mut i32) {
        let t = self.huff(dc);
        *pred = pred.wrapping_add(self.extend(t));
        b[0] = *pred as i16;
        let full = b.len() == 64;
        let mut k = 1;
        while k < 64 {
            if self.n < 16 {
                self.refill();
            }
            let c = ac.fast_ac[(self.buf >> (64 - FAST)) as usize];
            if c != 0 {
                k += (c >> 4) as usize & 15;
                self.buf <<= c & 15;
                self.n -= (c & 15) as u32;
                if k > 63 {
                    self.bad = true;
                    break;
                }
                if full {
                    b[ZIGZAG[k]] = c >> 8;
                }
                k += 1;
                continue;
            }
            let rs = self.huff(ac);
            let (r, s) = ((rs >> 4) as usize, rs & 15);
            if s == 0 {
                if r != 15 {
                    break;
                }
                k += 16;
                continue;
            }
            k += r;
            let v = self.extend(s);
            if k > 63 {
                self.bad = true;
                break;
            }
            if full {
                b[ZIGZAG[k]] = v as i16;
            }
            k += 1;
        }
    }

    fn dc_first(&mut self, b: &mut [i16], dc: &Huff, pred: &mut i32, al: u32) {
        let t = self.huff(dc);
        *pred = pred.wrapping_add(self.extend(t));
        b[0] = pred.wrapping_shl(al) as i16;
    }

    fn dc_refine(&mut self, b: &mut [i16], al: u32) {
        if self.bits(1) != 0 {
            b[0] |= 1 << al;
        }
    }

    fn ac_first(&mut self, b: &mut [i16], ac: &Huff, ss: usize, se: usize, al: u32, eobrun: &mut u32) {
        if *eobrun > 0 {
            *eobrun -= 1;
            return;
        }
        let mut k = ss;
        while k <= se {
            if self.n < 16 {
                self.refill();
            }
            let c = ac.fast_ac[(self.buf >> (64 - FAST)) as usize];
            if c != 0 {
                k += (c >> 4) as usize & 15;
                self.buf <<= c & 15;
                self.n -= (c & 15) as u32;
                if k > se {
                    self.bad = true;
                    break;
                }
                b[ZIGZAG[k]] = ((c >> 8) as i32).wrapping_shl(al) as i16;
                k += 1;
                continue;
            }
            let rs = self.huff(ac);
            let (r, s) = ((rs >> 4) as u32, rs & 15);
            if s == 0 {
                if r < 15 {
                    *eobrun = (1 << r) - 1;
                    if r > 0 {
                        *eobrun += self.bits(r) as u32;
                    }
                    break;
                }
                k += 16;
                continue;
            }
            k += r as usize;
            let v = self.extend(s);
            if k > se {
                self.bad = true;
                break;
            }
            b[ZIGZAG[k]] = v.wrapping_shl(al) as i16;
            k += 1;
        }
    }

    fn ac_refine(&mut self, b: &mut [i16], ac: &Huff, ss: usize, se: usize, al: u32, eobrun: &mut u32) {
        let (p1, m1) = (1i16 << al, -1i16 << al);
        let mut k = ss;
        if *eobrun == 0 {
            while k <= se {
                let rs = self.huff(ac);
                let (mut r, s) = ((rs >> 4) as u32, rs & 15);
                let mut val = 0i16;
                if s != 0 {
                    val = if self.bits(1) != 0 { p1 } else { m1 };
                } else if r != 15 {
                    *eobrun = 1 << r;
                    if r > 0 {
                        *eobrun += self.bits(r) as u32;
                    }
                    break;
                }
                // skip r zero coefficients (and the new one's position), refining the nonzero ones passed
                while k <= se {
                    let c = &mut b[ZIGZAG[k]];
                    if *c != 0 {
                        if self.bits(1) != 0 && *c & p1 == 0 {
                            *c = c.wrapping_add(if *c >= 0 { p1 } else { m1 });
                        }
                    } else {
                        if r == 0 {
                            break;
                        }
                        r -= 1;
                    }
                    k += 1;
                }
                if val != 0 && k <= se {
                    b[ZIGZAG[k]] = val;
                }
                k += 1;
            }
        }
        if *eobrun > 0 {
            while k <= se {
                let c = &mut b[ZIGZAG[k]];
                if *c != 0 && self.bits(1) != 0 && *c & p1 == 0 {
                    *c = c.wrapping_add(if *c >= 0 { p1 } else { m1 });
                }
                k += 1;
            }
            *eobrun -= 1;
        }
    }

    /// Moves `pos` to the next marker (FF xx, xx not 00 or FF) and returns xx.
    fn seek_marker(&mut self) -> Option<u8> {
        let mut i = self.pos;
        while i + 1 < self.d.len() {
            if self.d[i] == 0xFF && self.d[i + 1] != 0 && self.d[i + 1] != 0xFF {
                self.pos = i;
                return Some(self.d[i + 1]);
            }
            i += 1;
        }
        self.pos = self.d.len();
        None
    }

    /// At a restart point: reads RSTn (n = `want`), resynchronizing like libjpeg when it is
    /// missing or damaged. True if the next interval has data.
    fn restart(&mut self, want: u8) -> bool {
        (self.buf, self.n, self.fake, self.bad, self.stop) = (0, 0, 0, false, false);
        loop {
            let Some(m) = self.seek_marker() else {
                self.stop = true;
                return false;
            };
            if m == 0xD0 + want {
                self.pos += 2;
                return true;
            }
            let act = if (0xD0..=0xD7).contains(&m) {
                let d = m - 0xD0;
                if d == (want + 1) & 7 || d == (want + 2) & 7 {
                    3 // one of the next two: this interval is lost, keep the marker for later
                } else if d == (want + 7) & 7 || d == (want + 6) & 7 {
                    2 // an earlier one: skip it and look further
                } else {
                    1 // too far off to tell: take it as the one wanted
                }
            } else if m < 0xC0 {
                2
            } else {
                3
            };
            match act {
                1 => {
                    self.pos += 2;
                    return true;
                }
                2 => self.pos += 2,
                _ => {
                    self.stop = true;
                    return false;
                }
            }
        }
    }

    /// The data stopped at a restart marker, so a later interval can still be decoded.
    fn at_restart(&self) -> bool {
        let mut i = self.pos;
        while self.d.get(i) == Some(&0xFF) {
            i += 1;
        }
        self.stop && i > self.pos && self.d.get(i).is_some_and(|m| (0xD0..=0xD7).contains(m))
    }
}

// ---------------------------------------------------------------------------------------------
// Decoding

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Seq,
    DcFirst,
    DcRefine,
    AcFirst,
    AcRefine,
}

struct State {
    out: Out,
    /// Per component: all its coefficients, or (streaming) those of one MCU row.
    coefs: Vec<Vec<i16>>,
    /// Coefficients kept per block: 64, or 1 for a component decoded at 1/8 (DC only).
    cstride: Vec<usize>,
    streaming: bool,
    latched: Vec<bool>,
}

struct Dec<'a> {
    r: Reader<'a>,
    h: Headers<'a>,
    want: (usize, usize),
    st: Option<State>,
    scans: usize,
}

impl Dec<'_> {
    /// Sets up the output at the first scan. `all` is true if the scan holds every component.
    fn setup(&mut self, all: bool) -> Res<()> {
        let info = self.h.info()?;
        let color = self.h.color();
        let o = info.orientation;
        let f = self.h.frame.as_ref().ok_or("the JPEG has no frame header (SOF)")?;
        let s = info.scale_for(self.want.0, self.want.1);
        let m = 8 / s;
        let (w, h) = (f.w.div_ceil(s), f.h.div_ceil(s));
        let (iw, ih) = info.scaled_size(s);
        check_size(iw, ih)?;
        // every pixel is written, so zeroed memory (cheap to get) will do
        let img = Image { w: iw, h: ih, px: vec![0; iw * ih * 4], icc: None };
        let streaming = !f.progressive && all;
        let mut planes = Vec::with_capacity(f.comps.len());
        let (mut coefs, mut cstride) = (Vec::new(), Vec::new());
        let mut total = 0usize;
        for c in &f.comps {
            // like libjpeg: let the IDCT scale a subsampled component up instead of upsampling
            let mut cs = m;
            while cs < 8 && (f.hmax * m) % (c.h * cs * 2) == 0 && (f.vmax * m) % (c.v * cs * 2) == 0 {
                cs *= 2;
            }
            if (f.hmax * m) % (c.h * cs) != 0 || (f.vmax * m) % (c.v * cs) != 0 {
                return Err("JPEG with fractional chroma subsampling is not supported".into());
            }
            let (fx, fy) = (f.hmax * m / (c.h * cs), f.vmax * m / (c.v * cs));
            let dw = (f.w * c.h * cs).div_ceil(f.hmax * 8);
            let dh = (f.h * c.v * cs).div_ceil(f.vmax * 8);
            let up = match (fx, fy) {
                (1, 1) => Up::Copy,
                _ if m == 1 => Up::Rep,
                (2, 1) if dw > 2 => Up::H2,
                (1, 2) => Up::V2,
                (2, 2) if dw > 2 => Up::HV2,
                _ => Up::Rep,
            };
            let rows = c.v * cs;
            let stride = c.bw * cs;
            planes.push(Plane {
                cs,
                rows,
                stride,
                dw,
                dh,
                fx,
                fy,
                up,
                strip: vec![0; (2 * rows + 1) * stride],
                base: 0,
                line: vec![0; (dw * fx).max(w)],
                sums: vec![0; if up == Up::HV2 { dw } else { 0 }],
                q: [0; 64],
            });
            let cst = if cs == 1 { 1 } else { 64 };
            let n = if streaming { c.v * c.bw * cst } else { c.bw * c.bh * cst };
            total = total.saturating_add(n.saturating_mul(2));
            if total > MAX_COEF_BYTES {
                return Err("the JPEG is too large to decode (progressive or multi-scan)".into());
            }
            coefs.push(n);
            cstride.push(cst);
        }
        let coefs = coefs.into_iter().map(|n| vec![0i16; n]).collect();
        let out = Out { color, w, h, orient: o, mcu_rows: f.vmax * m, planes, rgba: vec![0; w * 4], img };
        let n = f.comps.len();
        self.st = Some(State { out, coefs, cstride, streaming, latched: vec![false; n] });
        Ok(())
    }

    /// Decodes one scan. True when the image is complete (a streamed baseline scan).
    fn scan(&mut self, s: &[u8]) -> Res<bool> {
        self.scans += 1;
        if self.scans > MAX_SCANS {
            return Err("the JPEG has too many scans".into());
        }
        let (comps, tabs, kind, ss, se, al, all) = self.scan_header(s)?;
        if self.st.is_none() {
            self.setup(all)?;
        }
        let Dec { r, h, st, .. } = self;
        let (Some(f), Some(st)) = (h.frame.as_ref(), st.as_mut()) else { return Err("the JPEG has no frame header (SOF)".into()) };
        let ns = comps.len();
        for &ci in &comps {
            if !st.latched[ci] {
                st.out.planes[ci].q = h.qt[f.comps[ci].tq].ok_or("the JPEG lacks a quantization table")?;
                st.latched[ci] = true;
            }
        }
        if matches!(kind, Kind::AcFirst | Kind::AcRefine) && st.cstride[comps[0]] == 1 {
            return Ok(false); // AC terms that a 1/8 decode does not use
        }
        let none = Huff::none();
        let tables: Vec<(&Huff, &Huff)> = tabs
            .iter()
            .map(|&(td, ta)| {
                (h.dc.get(td).and_then(|t| t.as_deref()).unwrap_or(&none), h.ac.get(ta).and_then(|t| t.as_deref()).unwrap_or(&none))
            })
            .collect();
        let mut bits = Bits::new(r.d, r.pos);
        let single = ns == 1;
        let (nx, ny) = if single {
            let c = &f.comps[comps[0]];
            (c.cw.div_ceil(8), c.ch.div_ceil(8))
        } else {
            (f.mcux, f.mcuy)
        };
        let ri = h.restart;
        let (mut preds, mut eobrun, mut rst, mut live, mut n) = ([0i32; 4], 0u32, 0u8, true, 0usize);
        for my in 0..ny {
            for mx in 0..nx {
                if ri > 0 && n > 0 && n % ri == 0 {
                    live = bits.restart(rst);
                    rst = (rst + 1) & 7;
                    preds = [0; 4];
                    eobrun = 0;
                }
                n += 1;
                if live && bits.empty() {
                    live = false;
                }
                for (k, &ci) in comps.iter().enumerate() {
                    let c = &f.comps[ci];
                    let (bh, bv) = if single { (1, 1) } else { (c.h, c.v) };
                    let cs = st.cstride[ci];
                    let row0 = if st.streaming { my * bv } else { 0 };
                    let (dc, ac) = tables[k];
                    for by in 0..bv {
                        for bx in 0..bh {
                            let (x, y) = (mx * bh + bx, my * bv + by);
                            let i = ((y - row0) * c.bw + x) * cs;
                            let b = &mut st.coefs[ci][i..i + cs];
                            if st.streaming {
                                b.fill(0);
                            }
                            if !live {
                                continue;
                            }
                            match kind {
                                Kind::Seq => bits.block(b, dc, ac, &mut preds[k]),
                                Kind::DcFirst => bits.dc_first(b, dc, &mut preds[k], al),
                                Kind::DcRefine => bits.dc_refine(b, al),
                                Kind::AcFirst => bits.ac_first(b, ac, ss, se, al, &mut eobrun),
                                Kind::AcRefine => bits.ac_refine(b, ac, ss, se, al, &mut eobrun),
                            }
                        }
                    }
                }
                if live && bits.overrun() {
                    live = false;
                }
            }
            if st.streaming {
                for (ci, c) in f.comps.iter().enumerate() {
                    st.out.load(ci, &st.coefs[ci], st.cstride[ci], c.bw, 0, c.v, my);
                }
                st.out.rows_done(my);
            } else if !live && !(ri > 0 && bits.at_restart()) {
                break; // nothing more of this scan can be decoded
            }
        }
        r.pos = bits.pos;
        if st.streaming {
            st.out.emit(ny - 1);
            return Ok(true);
        }
        Ok(false)
    }

    /// Parses a scan header: the components (frame indexes), their (DC, AC) tables, the kind of
    /// scan, Ss, Se, Al, and whether it holds every component.
    #[allow(clippy::type_complexity)]
    fn scan_header(&self, s: &[u8]) -> Res<(Vec<usize>, Vec<(usize, usize)>, Kind, usize, usize, u32, bool)> {
        let f = self.h.frame.as_ref().ok_or("the JPEG has no frame header (SOF)")?;
        let ns = *s.first().ok_or(TRUNCATED)? as usize;
        if ns == 0 || ns > 4 || s.len() < 4 + 2 * ns {
            return Err("bad JPEG scan header".into());
        }
        let (mut comps, mut tabs) = (Vec::with_capacity(ns), Vec::with_capacity(ns));
        for i in 0..ns {
            let id = s[1 + 2 * i];
            let ci = f.comps.iter().position(|c| c.id == id).ok_or("a JPEG scan names an unknown component")?;
            if comps.contains(&ci) {
                return Err("a JPEG scan names a component twice".into());
            }
            comps.push(ci);
            tabs.push(((s[2 + 2 * i] >> 4) as usize, (s[2 + 2 * i] & 15) as usize));
        }
        let p = 1 + 2 * ns;
        let (ss, se, ah, al) = (s[p] as usize, s[p + 1] as usize, (s[p + 2] >> 4) as u32, (s[p + 2] & 15) as u32);
        let kind = if !f.progressive {
            Kind::Seq
        } else {
            if ss > se || se > 63 || (ss == 0 && se != 0) || (ss > 0 && ns != 1) || al > 13 || ah > 13 {
                return Err("bad progressive JPEG scan".into());
            }
            match (ss == 0, ah == 0) {
                (true, true) => Kind::DcFirst,
                (true, false) => Kind::DcRefine,
                (false, true) => Kind::AcFirst,
                (false, false) => Kind::AcRefine,
            }
        };
        if ns > 1 && comps.iter().map(|&c| f.comps[c].h * f.comps[c].v).sum::<usize>() > 10 {
            return Err("bad JPEG scan (more than 10 blocks per MCU)".into());
        }
        for &(td, ta) in &tabs {
            let need_dc = matches!(kind, Kind::Seq | Kind::DcFirst);
            let need_ac = matches!(kind, Kind::Seq | Kind::AcFirst | Kind::AcRefine);
            if (need_dc && self.h.dc.get(td).is_none_or(|t| t.is_none())) || (need_ac && self.h.ac.get(ta).is_none_or(|t| t.is_none()))
            {
                return Err("the JPEG lacks a Huffman table".into());
            }
        }
        Ok((comps, tabs, kind, ss, se, al, ns == f.comps.len()))
    }

    fn finish(self) -> Res<Image> {
        let icc = self.h.icc();
        let (Some(f), Some(mut st)) = (self.h.frame, self.st) else { return Err("the JPEG has no image data (no scan)".into()) };
        if !st.streaming {
            for k in 0..f.mcuy {
                for (ci, c) in f.comps.iter().enumerate() {
                    st.out.load(ci, &st.coefs[ci], st.cstride[ci], c.bw, k * c.v, c.v, k);
                }
                st.out.rows_done(k);
            }
            st.out.emit(f.mcuy - 1);
        }
        let mut img = st.out.img;
        img.icc = icc;
        Ok(img)
    }
}

// ---------------------------------------------------------------------------------------------
// Output: IDCT, upsampling, color conversion, orientation

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Gray,
    YCbCr,
    Rgb,
    Cmyk,
    Ycck,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Up {
    /// Same size as the output.
    Copy,
    /// Fancy (triangle) upsampling 2x horizontally, 2x vertically, or both.
    H2,
    V2,
    HV2,
    /// Replication by (fx, fy).
    Rep,
}

/// One component on its way to the output: a strip of its samples (the rows of the current and
/// the previous MCU row, and the row above them), upsampled one output row at a time.
struct Plane {
    /// IDCT size: 8, 4, 2 or 1.
    cs: usize,
    /// Sample rows per MCU row.
    rows: usize,
    stride: usize,
    /// Valid samples per row and rows in the image (at the decode scale).
    dw: usize,
    dh: usize,
    fx: usize,
    fy: usize,
    up: Up,
    strip: Vec<u8>,
    /// The sample row held in strip row 0.
    base: usize,
    line: Vec<u8>,
    sums: Vec<i32>,
    q: [i32; 64],
}

struct Out {
    color: Color,
    /// The decoded size before the orientation is applied.
    w: usize,
    h: usize,
    orient: u8,
    /// Output rows per MCU row.
    mcu_rows: usize,
    planes: Vec<Plane>,
    rgba: Vec<u8>,
    img: Image,
}

impl Out {
    /// Runs the IDCT of `nb` block rows (from block row `brow0` of `coefs`) of component `ci`
    /// into its strip as MCU row `k`.
    #[allow(clippy::too_many_arguments)]
    fn load(&mut self, ci: usize, coefs: &[i16], cstride: usize, bw: usize, brow0: usize, nb: usize, k: usize) {
        let p = &mut self.planes[ci];
        let cs = p.cs;
        let nbx = p.dw.div_ceil(cs).min(bw);
        for br in 0..nb {
            let r = k * p.rows + br * cs;
            if r >= p.dh {
                break;
            }
            let at0 = (r - p.base) * p.stride;
            for bx in 0..nbx {
                let i = ((brow0 + br) * bw + bx) * cstride;
                let c = &coefs[i..i + cstride];
                let o = &mut p.strip[at0 + bx * cs..];
                match cs {
                    8 => idct8(c, &p.q, o, p.stride),
                    4 => idct4(c, &p.q, o, p.stride),
                    2 => idct2(c, &p.q, o, p.stride),
                    _ => o[0] = clamp(((deq(c[0], p.q[0]) + 4) >> 3) + 128),
                }
            }
        }
    }

    /// MCU row `k` is loaded: emits the output rows of MCU row k - 1 and slides the strips.
    fn rows_done(&mut self, k: usize) {
        if k == 0 {
            return;
        }
        self.emit(k - 1);
        for p in &mut self.planes {
            let nb = k * p.rows - 1;
            let from = nb - p.base;
            p.strip.copy_within(from * p.stride..(from + p.rows + 1) * p.stride, 0);
            p.base = nb;
        }
    }

    /// Converts the output rows of MCU row `j` and writes them into the image.
    fn emit(&mut self, j: usize) {
        let y1 = ((j + 1) * self.mcu_rows).min(self.h);
        for y in j * self.mcu_rows..y1 {
            for p in &mut self.planes {
                p.upsample(y, self.w);
            }
            self.convert();
            self.put(y);
        }
    }

    fn convert(&mut self) {
        let Out { color, w, planes, rgba, .. } = self;
        let px = rgba[..*w * 4].as_chunks_mut::<4>().0.iter_mut();
        let l = |i: usize| &planes[i].line[..*w];
        match color {
            Color::Gray => px.zip(l(0)).for_each(|(o, &g)| o.copy_from_slice(&[g, g, g, 255])),
            Color::Rgb => {
                for (x, o) in px.enumerate() {
                    o.copy_from_slice(&[l(0)[x], l(1)[x], l(2)[x], 255]);
                }
            }
            Color::YCbCr => {
                for (((o, &y), &cb), &cr) in px.zip(l(0)).zip(l(1)).zip(l(2)) {
                    o.copy_from_slice(&ycc_rgb(y, cb, cr));
                }
            }
            // Adobe CMYK is stored inverted; RGB as Pillow makes it: (255 - C) * (255 - K) / 255
            Color::Cmyk => {
                for (x, o) in px.enumerate() {
                    let nk = l(3)[x] as u32;
                    for (i, d) in o[..3].iter_mut().enumerate() {
                        *d = (nk - muldiv255(255 - l(i)[x] as u32, nk)) as u8;
                    }
                    o[3] = 255;
                }
            }
            // YCC holds 255 - C, 255 - M, 255 - Y as YCbCr; K is stored inverted as in CMYK
            Color::Ycck => {
                let (y, cb, cr) = (l(0), l(1), l(2));
                for (x, o) in px.enumerate() {
                    let rgb = ycc_rgb(y[x], cb[x], cr[x]);
                    let nk = l(3)[x] as u32;
                    for (d, &c) in o[..3].iter_mut().zip(&rgb) {
                        *d = (nk - muldiv255(c as u32, nk)) as u8;
                    }
                    o[3] = 255;
                }
            }
        }
    }

    /// Writes converted row `y` into the image, turned by the EXIF orientation.
    fn put(&mut self, y: usize) {
        let (w, h, iw) = (self.w, self.h, self.img.w);
        let px = &mut self.img.px;
        let src = &self.rgba[..w * 4];
        if self.orient == 1 {
            px[y * w * 4..(y + 1) * w * 4].copy_from_slice(src);
            return;
        }
        for (x, s) in src.as_chunks::<4>().0.iter().enumerate() {
            let (dx, dy) = match self.orient {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                _ => (y, w - 1 - x),
            };
            let d = (dy * iw + dx) * 4;
            px[d..d + 4].copy_from_slice(s);
        }
    }
}

impl Plane {
    /// The strip index of sample row `r` (clamped to the image).
    fn row(&self, r: usize) -> usize {
        (r.min(self.dh - 1) - self.base) * self.stride
    }

    /// Fills `line` with output row `y` of this component.
    fn upsample(&mut self, y: usize, w: usize) {
        match self.up {
            Up::Copy => {
                let a = self.row(y);
                self.line[..w].copy_from_slice(&self.strip[a..a + w]);
            }
            Up::Rep => {
                let (a, fx) = (self.row(y / self.fy), self.fx);
                let src = &self.strip[a..a + self.dw];
                for (x, d) in self.line[..w].iter_mut().enumerate() {
                    *d = src[x / fx];
                }
            }
            Up::H2 => {
                let a = self.row(y);
                h2v1(&self.strip[a..a + self.dw], &mut self.line);
            }
            Up::V2 | Up::HV2 => {
                // the nearer sample row weighs 3/4, the one above (even y) or below (odd y) 1/4
                let near = y / 2;
                let far = if y.is_multiple_of(2) { near.saturating_sub(1) } else { near + 1 };
                let (a, b) = (self.row(near), self.row(far));
                let n = if self.up == Up::V2 { w } else { self.dw };
                let (s0, s1) = (&self.strip[a..a + n], &self.strip[b..b + n]);
                if self.up == Up::V2 {
                    let bias = 1 + (y % 2) as u32;
                    for ((d, &p), &q) in self.line.iter_mut().zip(s0).zip(s1) {
                        *d = ((p as u32 * 3 + q as u32 + bias) >> 2) as u8;
                    }
                } else {
                    for ((d, &p), &q) in self.sums.iter_mut().zip(s0).zip(s1) {
                        *d = p as i32 * 3 + q as i32;
                    }
                    h2v2(&self.sums, &mut self.line);
                }
            }
        }
    }
}

/// libjpeg's h2v1 fancy upsampling of a row of more than 2 samples into `out` (twice as long).
fn h2v1(s: &[u8], out: &mut [u8]) {
    let n = s.len();
    let v = |i: usize| s[i] as u32;
    out[0] = s[0];
    out[1] = ((v(0) * 3 + v(1) + 2) >> 2) as u8;
    for (o, w) in out[2..2 * n - 2].as_chunks_mut::<2>().0.iter_mut().zip(s.windows(3)) {
        let m = w[1] as u32 * 3;
        o[0] = ((m + w[0] as u32 + 1) >> 2) as u8;
        o[1] = ((m + w[2] as u32 + 2) >> 2) as u8;
    }
    out[2 * n - 2] = ((v(n - 1) * 3 + v(n - 2) + 1) >> 2) as u8;
    out[2 * n - 1] = s[n - 1];
}

/// The horizontal half of libjpeg's h2v2 fancy upsampling, on column sums (3 * near + far).
fn h2v2(s: &[i32], out: &mut [u8]) {
    let n = s.len();
    out[0] = ((s[0] * 4 + 8) >> 4) as u8;
    out[1] = ((s[0] * 3 + s[1] + 7) >> 4) as u8;
    for (o, w) in out[2..2 * n - 2].as_chunks_mut::<2>().0.iter_mut().zip(s.windows(3)) {
        let m = w[1] * 3;
        o[0] = ((m + w[0] + 8) >> 4) as u8;
        o[1] = ((m + w[2] + 7) >> 4) as u8;
    }
    out[2 * n - 2] = ((s[n - 1] * 3 + s[n - 2] + 8) >> 4) as u8;
    out[2 * n - 1] = ((s[n - 1] * 4 + 7) >> 4) as u8;
}

fn muldiv255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    ((t >> 8) + t) >> 8
}

/// libjpeg's fixed-point YCbCr to RGB (16 fraction bits; the same numbers as its tables).
#[inline(always)]
fn ycc_rgb(y: u8, cb: u8, cr: u8) -> [u8; 4] {
    let (y, cb, cr) = (y as i32, cb as i32 - 128, cr as i32 - 128);
    let r = y + ((91881 * cr + 32768) >> 16);
    let g = y + ((-22554 * cb - 46802 * cr + 32768) >> 16);
    let b = y + ((116130 * cb + 32768) >> 16);
    [clamp(r), clamp(g), clamp(b), 255]
}

#[inline]
fn clamp(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// A dequantized coefficient, limited to a range no valid 8-bit JPEG exceeds, so the IDCTs
/// below cannot overflow 32 bits on damaged data.
#[inline]
fn deq(c: i16, q: i32) -> i32 {
    (c as i32 * q).clamp(-8192, 8191)
}

/// The same bound for the values between the two IDCT passes.
#[inline]
fn mid(v: i32) -> i32 {
    v.clamp(-8192, 8191)
}

/// libjpeg's accurate integer 8-point IDCT (jidctint.c, 13 fraction bits) on 8 lanes at once
/// (`v[k][lane]`, which the compiler vectorizes), descaled by `shift`.
#[inline(always)]
fn idct8_lanes(v: &[[i32; 8]; 8], shift: u32, out: &mut [[i32; 8]; 8]) {
    let r = 1 << (shift - 1);
    for l in 0..8 {
        let z1 = (v[2][l] + v[6][l]) * 4433;
        let t2 = z1 - v[6][l] * 15137;
        let t3 = z1 + v[2][l] * 6270;
        let t0 = (v[0][l] + v[4][l]) << 13;
        let t1 = (v[0][l] - v[4][l]) << 13;
        let (t10, t13, t11, t12) = (t0 + t3 + r, t0 - t3 + r, t1 + t2 + r, t1 - t2 + r);
        let (a0, a1, a2, a3) = (v[7][l], v[5][l], v[3][l], v[1][l]);
        let (z1, z2, z3, z4) = (a0 + a3, a1 + a2, a0 + a2, a1 + a3);
        let z5 = (z3 + z4) * 9633;
        let (z1, z2, z3, z4) = (z1 * -7373, z2 * -20995, z3 * -16069 + z5, z4 * -3196 + z5);
        let b0 = a0 * 2446 + z1 + z3;
        let b1 = a1 * 16819 + z2 + z4;
        let b2 = a2 * 25172 + z2 + z3;
        let b3 = a3 * 12299 + z1 + z4;
        out[0][l] = (t10 + b3) >> shift;
        out[7][l] = (t10 - b3) >> shift;
        out[1][l] = (t11 + b2) >> shift;
        out[6][l] = (t11 - b2) >> shift;
        out[2][l] = (t12 + b1) >> shift;
        out[5][l] = (t12 - b1) >> shift;
        out[3][l] = (t13 + b0) >> shift;
        out[4][l] = (t13 - b0) >> shift;
    }
}

/// The full 8x8 IDCT of a block into `out` (rows `stride` apart).
fn idct8(c: &[i16], q: &[i32; 64], out: &mut [u8], stride: usize) {
    let Ok(c) = <&[i16; 64]>::try_from(c) else { return };
    if c[1..].iter().all(|&v| v == 0) {
        let v = clamp((((deq(c[0], q[0]) << 2) + 16) >> 5) + 128);
        for y in 0..8 {
            if let Some(o) = out.get_mut(y * stride..y * stride + 8) {
                o.fill(v);
            }
        }
        return;
    }
    let mut v = [[0i32; 8]; 8];
    for k in 0..8 {
        for x in 0..8 {
            v[k][x] = deq(c[k * 8 + x], q[k * 8 + x]);
        }
    }
    // columns, then (transposed) rows
    let mut ws = [[0i32; 8]; 8];
    idct8_lanes(&v, 11, &mut ws);
    for (y, row) in ws.iter().enumerate() {
        for (x, &val) in row.iter().enumerate() {
            v[x][y] = mid(val);
        }
    }
    idct8_lanes(&v, 18, &mut ws);
    for (y, o) in out.chunks_mut(stride).take(8).enumerate() {
        for (x, d) in o.iter_mut().take(8).enumerate() {
            *d = clamp(ws[x][y] + 128);
        }
    }
}

/// libjpeg's reduced 4-point IDCT (jidctred.c) of one 1-D input (column 4 / row 4 unused).
#[inline(always)]
fn idct4_1d(v: [i32; 8], shift: u32) -> [i32; 4] {
    let t0 = v[0] << 14;
    let t2 = v[2] * 15137 - v[6] * 6270;
    let (t10, t12) = (t0 + t2, t0 - t2);
    let (z1, z2, z3, z4) = (v[7], v[5], v[3], v[1]);
    let o0 = z1 * -1730 + z2 * 11893 + z3 * -17799 + z4 * 8697;
    let o2 = z1 * -4176 + z2 * -4926 + z3 * 7373 + z4 * 20995;
    let r = 1 << (shift - 1);
    [(t10 + o2 + r) >> shift, (t12 + o0 + r) >> shift, (t12 - o0 + r) >> shift, (t10 - o2 + r) >> shift]
}

/// A block scaled to 4x4 in the IDCT.
fn idct4(c: &[i16], q: &[i32; 64], out: &mut [u8], stride: usize) {
    let mut ws = [0i32; 32];
    for x in [0, 1, 2, 3, 5, 6, 7] {
        if [1, 2, 3, 5, 6, 7].iter().all(|&y| c[y * 8 + x] == 0) {
            let dc = deq(c[x], q[x]) << 2;
            for y in 0..4 {
                ws[y * 8 + x] = dc;
            }
            continue;
        }
        let r = idct4_1d(std::array::from_fn(|y| if y == 4 { 0 } else { deq(c[y * 8 + x], q[y * 8 + x]) }), 12);
        for y in 0..4 {
            ws[y * 8 + x] = mid(r[y]);
        }
    }
    for (y, w) in ws.as_chunks::<8>().0.iter().enumerate() {
        let o = &mut out[y * stride..y * stride + 4];
        if [1, 2, 3, 5, 6, 7].iter().all(|&i| w[i] == 0) {
            o.fill(clamp(((w[0] + 16) >> 5) + 128));
            continue;
        }
        let r = idct4_1d(std::array::from_fn(|i| w[i]), 19);
        for (d, v) in o.iter_mut().zip(r) {
            *d = clamp(v + 128);
        }
    }
}

/// libjpeg's reduced 2-point IDCT (jidctred.c) of one 1-D input (only terms 0, 1, 3, 5, 7).
#[inline(always)]
fn idct2_1d(v: [i32; 8], shift: u32) -> [i32; 2] {
    let t10 = v[0] << 15;
    let t0 = v[7] * -5906 + v[5] * 6967 + v[3] * -10426 + v[1] * 29692;
    let r = 1 << (shift - 1);
    [(t10 + t0 + r) >> shift, (t10 - t0 + r) >> shift]
}

/// A block scaled to 2x2 in the IDCT.
fn idct2(c: &[i16], q: &[i32; 64], out: &mut [u8], stride: usize) {
    let mut ws = [0i32; 16];
    for x in [0, 1, 3, 5, 7] {
        if [1, 3, 5, 7].iter().all(|&y| c[y * 8 + x] == 0) {
            let dc = deq(c[x], q[x]) << 2;
            ws[x] = dc;
            ws[8 + x] = dc;
            continue;
        }
        let r = idct2_1d(std::array::from_fn(|y| if y % 2 == 1 || y == 0 { deq(c[y * 8 + x], q[y * 8 + x]) } else { 0 }), 13);
        ws[x] = mid(r[0]);
        ws[8 + x] = mid(r[1]);
    }
    for (y, w) in ws.as_chunks::<8>().0.iter().enumerate() {
        let o = &mut out[y * stride..y * stride + 2];
        if [1, 3, 5, 7].iter().all(|&i| w[i] == 0) {
            o.fill(clamp(((w[0] + 16) >> 5) + 128));
            continue;
        }
        let r = idct2_1d(std::array::from_fn(|i| w[i]), 20);
        for (d, v) in o.iter_mut().zip(r) {
            *d = clamp(v + 128);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture of testdata/: the JPEG and the RGB Pillow decodes from it.
    macro_rules! fixture {
        ($n:literal) => {
            (
                $n,
                include_bytes!(concat!("testdata/jpeg-", $n, ".jpg")) as &[u8],
                include_bytes!(concat!("testdata/jpeg-", $n, ".rgb")) as &[u8],
            )
        };
    }

    type Fixture = (&'static str, &'static [u8], &'static [u8]);

    const FIXTURES: [Fixture; 17] = [
        fixture!("444"),
        fixture!("422"),
        fixture!("420"),
        fixture!("440"),
        fixture!("411"),
        fixture!("h3"),
        fixture!("mixed"),
        fixture!("scans"),
        fixture!("prog"),
        fixture!("prog444"),
        fixture!("rst"),
        fixture!("prog-rst"),
        fixture!("gray"),
        fixture!("gray-prog"),
        fixture!("cmyk"),
        fixture!("rgb"),
        fixture!("icc"),
    ];

    const EXIF: [Fixture; 8] = [
        fixture!("exif1"),
        fixture!("exif2"),
        fixture!("exif3"),
        fixture!("exif4"),
        fixture!("exif5"),
        fixture!("exif6"),
        fixture!("exif7"),
        fixture!("exif8"),
    ];

    fn find(name: &str) -> Fixture {
        FIXTURES.into_iter().find(|f| f.0 == name).unwrap()
    }

    fn rgb(img: &Image) -> Vec<u8> {
        img.px.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect()
    }

    /// Mean and largest absolute difference of two equally long byte strings.
    fn diff(a: &[u8], b: &[u8]) -> (f64, u8) {
        assert_eq!(a.len(), b.len());
        let d = a.iter().zip(b).map(|(&x, &y)| x.abs_diff(y));
        (d.clone().map(f64::from).sum::<f64>() / a.len() as f64, d.max().unwrap_or(0))
    }

    #[test]
    fn decodes_like_pillow() {
        for (name, jpg, expected) in FIXTURES {
            let img = decode(jpg, 0, 0).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(img.px.len(), img.w * img.h * 4);
            let (mean, max) = diff(&rgb(&img), expected);
            // the same integer IDCT, upsampling and color conversion as libjpeg-turbo: exact in practice
            assert!(mean < 0.1 && max <= 2, "{name}: mean {mean} max {max}");
            assert!(img.px.as_chunks::<4>().0.iter().all(|p| p[3] == 255), "{name}");
        }
    }

    #[test]
    fn applies_the_exif_orientation() {
        for (i, (name, jpg, expected)) in EXIF.into_iter().enumerate() {
            let o = i as u8 + 1;
            let info = info(jpg).unwrap();
            assert_eq!(info.orientation, o, "{name}");
            let img = decode(jpg, 0, 0).unwrap();
            let size = if o >= 5 { (5, 9) } else { (9, 5) };
            assert_eq!((img.w, img.h), size, "{name}");
            assert_eq!((info.w, info.h), size, "{name}");
            assert_eq!(diff(&rgb(&img), expected).1, 0, "{name}");
        }
    }

    #[test]
    fn reads_the_headers() {
        let i = info(find("prog").1).unwrap();
        assert_eq!((i.w, i.h, i.orientation, i.progressive, i.components, i.icc), (21, 17, 1, true, 3, false));
        let i = info(find("420").1).unwrap();
        assert!(!i.progressive);
        assert_eq!(info(find("gray").1).unwrap().components, 1);
        assert_eq!(info(find("cmyk").1).unwrap().components, 4);
        assert!(info(find("icc").1).unwrap().icc);
        assert!(info(b"GIF89a").is_err());
        assert!(info(&[0xFF, 0xD8, 0xFF, 0xD9]).is_err());
    }

    #[test]
    fn joins_the_icc_profile() {
        let img = decode(find("icc").1, 0, 0).unwrap();
        let icc = img.icc.unwrap();
        assert_eq!(u32::from_be_bytes(icc[..4].try_into().unwrap()) as usize, icc.len());
        assert_eq!(&icc[36..40], b"acsp");
        assert_eq!(decode(find("444").1, 0, 0).unwrap().icc, None);
    }

    #[test]
    fn picks_the_scale() {
        let i = info(find("prog").1).unwrap(); // 21 x 17
        assert_eq!(i.scale_for(0, 0), 1);
        assert_eq!(i.scale_for(11, 0), 2);
        assert_eq!(i.scale_for(12, 0), 1);
        assert_eq!(i.scale_for(6, 5), 4);
        assert_eq!(i.scale_for(6, 6), 2);
        assert_eq!(i.scale_for(1, 1), 8);
        assert_eq!(i.scale_for(0, 3), 8);
        assert_eq!(i.scaled_size(8), (3, 3));
        let i = info(EXIF[5].1).unwrap(); // 9 x 5 stored, 5 x 9 shown
        assert_eq!(i.scale_for(3, 0), 2);
        assert_eq!(i.scale_for(0, 3), 4);
    }

    /// Box-filters an RGB image of w x h down by s (partial boxes at the edges).
    fn shrink(px: &[u8], w: usize, h: usize, s: usize) -> Vec<u8> {
        let (ow, oh) = (w.div_ceil(s), h.div_ceil(s));
        let mut out = Vec::with_capacity(ow * oh * 3);
        for y in 0..oh {
            for x in 0..ow {
                for c in 0..3 {
                    let (mut sum, mut n) = (0usize, 0usize);
                    for yy in y * s..((y + 1) * s).min(h) {
                        for xx in x * s..((x + 1) * s).min(w) {
                            sum += px[(yy * w + xx) * 3 + c] as usize;
                            n += 1;
                        }
                    }
                    out.push(((sum + n / 2) / n) as u8);
                }
            }
        }
        out
    }

    #[test]
    fn scales_like_libjpeg() {
        let scaled = [
            ("420", include_bytes!("testdata/jpeg-420.scaled.rgb") as &[u8]),
            ("422", include_bytes!("testdata/jpeg-422.scaled.rgb")),
            ("411", include_bytes!("testdata/jpeg-411.scaled.rgb")),
            ("prog", include_bytes!("testdata/jpeg-prog.scaled.rgb")),
        ];
        for (name, mut expected) in scaled {
            for s in [2, 4, 8] {
                let img = decode(find(name).1, 21usize.div_ceil(s), 0).unwrap();
                assert_eq!((img.w, img.h), (21usize.div_ceil(s), 17usize.div_ceil(s)), "{name} 1/{s}");
                let (now, rest) = expected.split_at(img.w * img.h * 3);
                assert_eq!(diff(&rgb(&img), now).1, 0, "{name} 1/{s}");
                expected = rest;
            }
        }
    }

    #[test]
    fn scaled_is_close_to_a_box_filter() {
        // a smooth picture (the reduced IDCTs are not box filters, so hard edges would differ more)
        let (w, h) = (83, 61);
        let mut img = Image::new(w, h, [0, 0, 0, 255]).unwrap();
        for (i, p) in img.px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = ((i % w) as f64, (i / w) as f64);
            p[0] = (128.0 + 100.0 * (x / 13.0).sin()) as u8;
            p[1] = (40.0 + 2.5 * y) as u8;
            p[2] = (128.0 + 90.0 * ((x + y) / 17.0).cos()) as u8;
        }
        let jpg = super::super::jpeg_enc::encode(&img, 95);
        let full = decode(&jpg, 0, 0).unwrap();
        for s in [2, 4, 8] {
            let img = decode(&jpg, w.div_ceil(s), h.div_ceil(s)).unwrap();
            assert_eq!((img.w, img.h), (w.div_ceil(s), h.div_ceil(s)));
            let (mean, max) = diff(&rgb(&img), &shrink(&rgb(&full), w, h, s));
            assert!(mean < 2.0 && max < 40, "1/{s}: mean {mean} max {max}");
        }
        // one side wanted larger than half: full size
        let img = decode(find("420").1, 12, 0).unwrap();
        assert_eq!((img.w, img.h), (21, 17));
    }

    #[test]
    fn survives_damaged_restart_markers() {
        let (_, jpg, expected) = find("rst");
        let rst: Vec<usize> = (0..jpg.len() - 1).filter(|&i| jpg[i] == 0xFF && (0xD0..=0xD7).contains(&jpg[i + 1])).collect();
        assert_eq!(rst.len(), 3);
        // a wrong number, a missing marker, garbage before a marker
        let mut wrong = jpg.to_vec();
        wrong[rst[0] + 1] = 0xD5;
        let mut missing = jpg.to_vec();
        missing.drain(rst[1]..rst[1] + 2);
        let mut garbage = jpg.to_vec();
        garbage.splice(rst[2]..rst[2], [0x12, 0x34, 0x56]);
        for (what, data) in [("wrong", wrong), ("missing", missing), ("garbage", garbage)] {
            let img = decode(&data, 0, 0).unwrap_or_else(|e| panic!("{what}: {e}"));
            let (mean, _) = diff(&rgb(&img), expected);
            // one interval (a quarter of the image) at most is lost
            assert!(mean < 25.0, "{what}: mean {mean}");
        }
    }

    #[test]
    fn a_truncated_file_ends_in_gray() {
        for name in ["444", "gray", "420", "prog"] {
            let jpg = find(name).1;
            // cut a few bytes into the first scan's data
            let sos = (0..jpg.len() - 1).find(|&i| jpg[i] == 0xFF && jpg[i + 1] == 0xDA).unwrap();
            let img = decode(&jpg[..sos + 2 + be16(jpg, sos + 2) + 4], 0, 0).unwrap();
            let last = &img.px[(img.h - 1) * img.w * 4..];
            if name == "444" || name == "gray" {
                assert!(last.as_chunks::<4>().0.iter().all(|p| p[..3] == [128, 128, 128]), "{name}");
            } else {
                // 2x chroma is interpolated from the decoded row above
                let off: usize = last.as_chunks::<4>().0.iter().map(|p| p[1].abs_diff(128) as usize).sum();
                assert!(off < 20 * img.w, "{name}: {off}");
            }
        }
        assert!(decode(&find("420").1[..100], 0, 0).is_err()); // no scan at all
    }

    #[test]
    fn refuses_what_it_does_not_decode() {
        let jpg = find("444").1;
        let sof = (0..jpg.len() - 1).find(|&i| jpg[i] == 0xFF && jpg[i + 1] == 0xC0).unwrap();
        let with = |at: usize, b: u8| {
            let mut d = jpg.to_vec();
            d[at] = b;
            decode(&d, 0, 0).unwrap_err()
        };
        assert!(with(sof + 1, 0xC9).contains("arithmetic"));
        assert!(with(sof + 1, 0xC3).contains("lossless"));
        assert!(with(sof + 1, 0xC5).contains("hierarchical"));
        assert!(with(sof + 4, 12).contains("12-bit"));
        assert!(decode(b"\x89PNG\r\n", 0, 0).is_err());
    }

    /// Every truncation and a few hundred corruptions of some fixtures: Ok or Err, never a panic.
    #[test]
    fn damaged_files_do_not_panic() {
        let start = std::time::Instant::now();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for name in ["420", "prog", "rst", "prog-rst", "scans", "mixed", "cmyk"] {
            let jpg = find(name).1;
            let mut cases: Vec<Vec<u8>> = (0..jpg.len()).map(|n| jpg[..n].to_vec()).collect();
            for _ in 0..300 {
                let mut d = jpg.to_vec();
                for _ in 0..1 + rnd() % 4 {
                    let i = rnd() as usize % d.len();
                    d[i] = match rnd() % 4 {
                        0 => 0xFF,
                        1 => 0,
                        _ => rnd() as u8,
                    };
                }
                cases.push(d);
            }
            for d in cases {
                for want in [(0, 0), (3, 3)] {
                    let r = std::panic::catch_unwind(|| decode(&d, want.0, want.1).map(|i| i.px.len()));
                    assert!(r.is_ok(), "{name}: panic on {} bytes", d.len());
                }
            }
        }
        assert!(start.elapsed().as_secs() < 60);
    }
}

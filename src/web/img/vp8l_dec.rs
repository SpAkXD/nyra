//! A WebP lossless (VP8L) decoder, zero crates. It reads the payload of a `VP8L` chunk ([`decode`])
//! and the compressed data of an `ALPH` chunk with compression method 1 ([`decode_alpha`]).
//!
//! Supported: the whole VP8L format of the WebP lossless bitstream specification (RFC 9649): all
//! four transforms (predictor with its 14 modes and the first-row and first-column rules,
//! cross-color, subtract green, color indexing with pixel bundling), the color cache, meta prefix
//! codes (an entropy image selecting prefix-code groups), simple and normal prefix codes, and LZ77
//! backward references with the 120 two-dimensional distance codes. Prefix codes are decoded with
//! two-level lookup tables.
//!
//! Corrupt or truncated data gives an `Err`, never a panic or an endless loop: every prefix code
//! must be complete (or have exactly one symbol), every copy must stay inside the image, and the
//! image may hold at most [`MAX_PIXELS`] pixels, so the memory a file can claim is bounded.
//!
//! The constants the encoder (`vp8l.rs`) shares with the decoder live here.

use super::common::{check_size, Res, MAX_PIXELS};

/// The order the code lengths of the code-length code are stored in.
pub(super) const CODE_LENGTH_ORDER: [usize; 19] = [17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// The 120 short distance codes: code `i + 1` means `dx + dy * width` pixels back, with `(dx, dy)`
/// the entry `i` (a positive `dx` is to the left).
pub(super) const DIST_MAP: [(i8, i8); 120] = [
    (0, 1),
    (1, 0),
    (1, 1),
    (-1, 1),
    (0, 2),
    (2, 0),
    (1, 2),
    (-1, 2),
    (2, 1),
    (-2, 1),
    (2, 2),
    (-2, 2),
    (0, 3),
    (3, 0),
    (1, 3),
    (-1, 3),
    (3, 1),
    (-3, 1),
    (2, 3),
    (-2, 3),
    (3, 2),
    (-3, 2),
    (0, 4),
    (4, 0),
    (1, 4),
    (-1, 4),
    (4, 1),
    (-4, 1),
    (3, 3),
    (-3, 3),
    (2, 4),
    (-2, 4),
    (4, 2),
    (-4, 2),
    (0, 5),
    (3, 4),
    (-3, 4),
    (4, 3),
    (-4, 3),
    (5, 0),
    (1, 5),
    (-1, 5),
    (5, 1),
    (-5, 1),
    (2, 5),
    (-2, 5),
    (5, 2),
    (-5, 2),
    (4, 4),
    (-4, 4),
    (3, 5),
    (-3, 5),
    (5, 3),
    (-5, 3),
    (0, 6),
    (6, 0),
    (1, 6),
    (-1, 6),
    (6, 1),
    (-6, 1),
    (2, 6),
    (-2, 6),
    (6, 2),
    (-6, 2),
    (4, 5),
    (-4, 5),
    (5, 4),
    (-5, 4),
    (3, 6),
    (-3, 6),
    (6, 3),
    (-6, 3),
    (0, 7),
    (7, 0),
    (1, 7),
    (-1, 7),
    (5, 5),
    (-5, 5),
    (7, 1),
    (-7, 1),
    (4, 6),
    (-4, 6),
    (6, 4),
    (-6, 4),
    (2, 7),
    (-2, 7),
    (7, 2),
    (-7, 2),
    (3, 7),
    (-3, 7),
    (7, 3),
    (-7, 3),
    (5, 6),
    (-5, 6),
    (6, 5),
    (-6, 5),
    (8, 0),
    (4, 7),
    (-4, 7),
    (7, 4),
    (-7, 4),
    (8, 1),
    (8, 2),
    (6, 6),
    (-6, 6),
    (8, 3),
    (5, 7),
    (-5, 7),
    (7, 5),
    (-7, 5),
    (8, 4),
    (6, 7),
    (-6, 7),
    (7, 6),
    (-7, 6),
    (8, 5),
    (7, 7),
    (-7, 7),
    (8, 6),
    (8, 7),
];

/// Literal symbols of the green alphabet (the other three channels have 256 too).
pub(super) const NUM_LITERALS: usize = 256;
/// Length prefix codes, the green alphabet's symbols 256..280.
pub(super) const NUM_LENGTH_CODES: usize = 24;
/// Distance prefix codes.
pub(super) const NUM_DIST_CODES: usize = 40;
/// The longest prefix code.
pub(super) const MAX_CODE_LENGTH: usize = 15;
/// The longest code of the code-length code.
pub(super) const MAX_CL_LENGTH: usize = 7;
/// The largest color cache a file may declare (the encoder uses at most 10 bits, like libwebp).
pub(super) const MAX_CACHE_BITS: u32 = 11;

/// The color cache slot of `argb` in a cache of `1 << bits` entries (`bits` in 1..=11).
#[inline]
pub(super) fn cache_slot(argb: u32, bits: u32) -> usize {
    (argb.wrapping_mul(0x1e35_a7bd) >> (32 - bits)) as usize
}

/// The size of the green alphabet with a color cache of `cache_bits` (0 = no cache).
pub(super) fn green_alphabet(cache_bits: u32) -> usize {
    NUM_LITERALS + NUM_LENGTH_CODES + if cache_bits > 0 { 1 << cache_bits } else { 0 }
}

/// `ceil(size / 2^bits)`: the width or height of a sub-image of tiles of `2^bits` pixels.
#[inline]
pub(super) fn subsample(size: usize, bits: u32) -> usize {
    (size + (1 << bits) - 1) >> bits
}

/// Channel-wise sum modulo 256 of two ARGB pixels.
#[inline]
pub(super) fn add_pixels(a: u32, b: u32) -> u32 {
    let ag = (a & 0xff00_ff00).wrapping_add(b & 0xff00_ff00) & 0xff00_ff00;
    let rb = (a & 0x00ff_00ff).wrapping_add(b & 0x00ff_00ff) & 0x00ff_00ff;
    ag | rb
}

/// Channel-wise difference modulo 256 of two ARGB pixels.
#[inline]
pub(super) fn sub_pixels(a: u32, b: u32) -> u32 {
    let ag = (0x0080_0080u32 | (a & 0xff00_ff00)).wrapping_sub(b & 0xff00_ff00) & 0xff00_ff00;
    let rb = (0xff00_ff00u32 | (a & 0x00ff_00ff)).wrapping_sub(b & 0x00ff_00ff) & 0x00ff_00ff;
    ag | rb
}

#[inline]
fn average2(a: u32, b: u32) -> u32 {
    (((a ^ b) & 0xfefe_fefe) >> 1) + (a & b)
}

#[inline]
fn channel(p: u32, shift: u32) -> i32 {
    ((p >> shift) & 0xff) as i32
}

#[inline]
fn clamp255(v: i32) -> u32 {
    v.clamp(0, 255) as u32
}

fn select(l: u32, t: u32, tl: u32) -> u32 {
    let mut pa_minus_pb = 0;
    for s in [24, 16, 8, 0] {
        pa_minus_pb += (channel(l, s) - channel(tl, s)).abs() - (channel(t, s) - channel(tl, s)).abs();
    }
    // The spec's pL (distance of the estimate to L) minus pT (to T) is -pa_minus_pb.
    if pa_minus_pb <= 0 {
        t
    } else {
        l
    }
}

fn clamp_add_subtract_full(a: u32, b: u32, c: u32) -> u32 {
    let mut out = 0;
    for s in [24, 16, 8, 0] {
        out |= clamp255(channel(a, s) + channel(b, s) - channel(c, s)) << s;
    }
    out
}

fn clamp_add_subtract_half(a: u32, b: u32) -> u32 {
    let mut out = 0;
    for s in [24, 16, 8, 0] {
        let (x, y) = (channel(a, s), channel(b, s));
        out |= clamp255(x + (x - y) / 2) << s;
    }
    out
}

/// The prediction of predictor `mode` (0..=13; 14 and 15 act like 0, as in libwebp) from the
/// left, top, top-left and top-right pixels.
#[inline]
pub(super) fn predict(mode: u32, l: u32, t: u32, tl: u32, tr: u32) -> u32 {
    match mode {
        1 => l,
        2 => t,
        3 => tr,
        4 => tl,
        5 => average2(average2(l, tr), t),
        6 => average2(l, tl),
        7 => average2(l, t),
        8 => average2(tl, t),
        9 => average2(t, tr),
        10 => average2(average2(l, tl), average2(t, tr)),
        11 => select(l, t, tl),
        12 => clamp_add_subtract_full(l, t, tl),
        13 => clamp_add_subtract_half(average2(l, t), tl),
        _ => 0xff00_0000,
    }
}

/// `(t * c) >> 5` with both as signed bytes: one term of the cross-color transform.
#[inline]
pub(super) fn color_delta(t: u8, c: u8) -> i32 {
    (t as i8 as i32 * c as i8 as i32) >> 5
}

/// The value of a length or distance prefix code (`code` < 40) and its extra bits.
#[inline]
fn prefix_value(br: &mut BitReader, code: u32) -> usize {
    if code < 4 {
        return code as usize + 1;
    }
    let extra = (code - 2) >> 1;
    let offset = (2 + (code & 1) as usize) << extra;
    offset + br.read(extra) as usize + 1
}

/// The distance in pixels of distance code `code` (>= 1) in an image `width` pixels wide.
#[inline]
pub(super) fn code_to_distance(code: usize, width: usize) -> usize {
    if code > 120 {
        return code - 120;
    }
    let (dx, dy) = DIST_MAP[code - 1];
    let d = dx as i64 + dy as i64 * width as i64;
    d.max(1) as usize
}

/// Reads bits least significant first, from a 64-bit window. Reading past the end gives zeros and
/// is reported by [`BitReader::overrun`].
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    nbits: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        let mut br = BitReader { data, pos: 0, buf: 0, nbits: 0 };
        br.fill();
        br
    }

    /// Tops the window up to at least 57 bits.
    #[inline]
    fn fill(&mut self) {
        if self.nbits > 56 {
            return;
        }
        if self.pos + 8 <= self.data.len() {
            let mut b = [0u8; 8];
            b.copy_from_slice(&self.data[self.pos..self.pos + 8]);
            self.buf |= u64::from_le_bytes(b) << self.nbits;
            let k = (63 - self.nbits) >> 3;
            self.pos += k as usize;
            self.nbits += k * 8;
        } else {
            while self.nbits <= 56 {
                let byte = self.data.get(self.pos).copied().unwrap_or(0);
                self.buf |= (byte as u64) << self.nbits;
                self.pos += 1;
                self.nbits += 8;
            }
        }
    }

    #[inline]
    fn peek(&self, n: u32) -> u32 {
        (self.buf & ((1u64 << n) - 1)) as u32
    }

    #[inline]
    fn skip(&mut self, n: u32) {
        self.buf >>= n;
        self.nbits -= n;
    }

    /// The next `n` bits (`n` <= 32).
    #[inline]
    fn read(&mut self, n: u32) -> u32 {
        self.fill();
        let v = self.peek(n);
        self.skip(n);
        v
    }

    /// True once more bits were consumed than the data has.
    #[inline]
    fn overrun(&self) -> bool {
        self.pos * 8 - self.nbits as usize > self.data.len() * 8
    }
}

const LINK: u32 = 1 << 31;
const ROOT_BITS: u32 = 8;

/// A prefix code as a two-level lookup table. A direct entry is `len << 16 | symbol`; a root entry
/// with [`LINK`] set points to a second-level table (`sub_bits << 16 | offset`).
struct Huffman {
    table: Vec<u32>,
    root: u32,
}

impl Huffman {
    /// The code with these code lengths (each <= 15). An error unless the code is complete or has
    /// exactly one symbol (which then takes zero bits).
    fn new(lengths: &[u8]) -> Res<Huffman> {
        let mut count = [0u32; MAX_CODE_LENGTH + 1];
        for &l in lengths {
            count[l as usize] += 1;
        }
        let used = lengths.len() - count[0] as usize;
        if used == 0 {
            return Err("a prefix code has no symbols".into());
        }
        if used == 1 {
            let sym = lengths.iter().position(|&l| l != 0).unwrap_or(0) as u32;
            return Ok(Huffman { table: vec![sym], root: 0 });
        }
        let mut left: i64 = 1;
        for &c in &count[1..] {
            left = (left << 1) - c as i64;
            if left < 0 {
                return Err("an over-subscribed prefix code".into());
            }
        }
        if left != 0 {
            return Err("an incomplete prefix code".into());
        }
        let max_len = (1..=MAX_CODE_LENGTH).rev().find(|&l| count[l] > 0).unwrap_or(1) as u32;
        let root = max_len.min(ROOT_BITS);
        // Canonical codes: the first code of each length, then (symbol, length, reversed code).
        let mut first = [0u32; MAX_CODE_LENGTH + 1];
        let mut c = 0u32;
        for l in 2..=MAX_CODE_LENGTH {
            c = (c + count[l - 1]) << 1;
            first[l] = c;
        }
        let mut list = Vec::with_capacity(used);
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                let code = first[l as usize];
                first[l as usize] += 1;
                list.push((sym as u32, l as u32, reverse_bits(code, l as u32)));
            }
        }
        let mut table = vec![0u32; 1 << root];
        if max_len > root {
            let mut sub = vec![0u32; 1 << root];
            for &(_, l, rev) in &list {
                if l > root {
                    let p = (rev & ((1 << root) - 1)) as usize;
                    sub[p] = sub[p].max(l - root);
                }
            }
            for (p, &sb) in sub.iter().enumerate() {
                if sb > 0 {
                    table[p] = LINK | (sb << 16) | table.len() as u32;
                    table.resize(table.len() + (1 << sb), 0);
                }
            }
        }
        for &(sym, l, rev) in &list {
            if l <= root {
                let mut k = rev as usize;
                while k < 1 << root {
                    table[k] = (l << 16) | sym;
                    k += 1 << l;
                }
            } else {
                let link = table[(rev & ((1 << root) - 1)) as usize];
                let sb = (link >> 16) & 0x7fff;
                let off = (link & 0xffff) as usize;
                let mut k = (rev >> root) as usize;
                while k < 1 << sb {
                    table[off + k] = ((l - root) << 16) | sym;
                    k += 1 << (l - root);
                }
            }
        }
        Ok(Huffman { table, root })
    }

    #[inline]
    fn read(&self, br: &mut BitReader) -> u32 {
        if br.nbits < 32 {
            br.fill();
        }
        let e = self.table[br.peek(self.root) as usize];
        if e & LINK == 0 {
            br.skip(e >> 16);
            return e & 0xffff;
        }
        br.skip(self.root);
        let sb = (e >> 16) & 0x7fff;
        let e2 = self.table[(e & 0xffff) as usize + br.peek(sb) as usize];
        br.skip(e2 >> 16);
        e2 & 0xffff
    }
}

/// `code` with its lowest `len` bits in reverse order.
pub(super) fn reverse_bits(code: u32, len: u32) -> u32 {
    if len == 0 {
        0
    } else {
        code.reverse_bits() >> (32 - len)
    }
}

/// Reads one prefix code of an alphabet of `size` symbols, using `lengths` as scratch.
fn read_code(br: &mut BitReader, size: usize, lengths: &mut Vec<u8>) -> Res<Huffman> {
    lengths.clear();
    lengths.resize(size, 0);
    if br.read(1) == 1 {
        // A simple code: one or two symbols.
        let two = br.read(1) == 1;
        let first_bits = if br.read(1) == 1 { 8 } else { 1 };
        let s0 = br.read(first_bits) as usize;
        if s0 >= size {
            return Err("a prefix code symbol is out of range".into());
        }
        lengths[s0] = 1;
        if two {
            let s1 = br.read(8) as usize;
            if s1 >= size {
                return Err("a prefix code symbol is out of range".into());
            }
            lengths[s1] = 1;
        }
    } else {
        let mut cl = [0u8; 19];
        let n = br.read(4) as usize + 4;
        for &i in &CODE_LENGTH_ORDER[..n] {
            cl[i] = br.read(3) as u8;
        }
        let cl_code = Huffman::new(&cl)?;
        let mut max_tokens = size;
        if br.read(1) == 1 {
            let nbits = 2 + 2 * br.read(3);
            max_tokens = 2 + br.read(nbits) as usize;
            if max_tokens > size {
                return Err("a prefix code has too many code lengths".into());
            }
        }
        let mut sym = 0;
        let mut prev = 8u8;
        while sym < size && max_tokens > 0 {
            max_tokens -= 1;
            let c = cl_code.read(br);
            if c < 16 {
                lengths[sym] = c as u8;
                sym += 1;
                if c != 0 {
                    prev = c as u8;
                }
            } else {
                let (extra, base, value) = match c {
                    16 => (2, 3, prev),
                    17 => (3, 3, 0),
                    _ => (7, 11, 0),
                };
                let repeat = br.read(extra) as usize + base;
                if sym + repeat > size {
                    return Err("a prefix code's code lengths run past its alphabet".into());
                }
                lengths[sym..sym + repeat].fill(value);
                sym += repeat;
            }
            if br.overrun() {
                return Err("truncated VP8L data".into());
            }
        }
    }
    if br.overrun() {
        return Err("truncated VP8L data".into());
    }
    Huffman::new(lengths)
}

/// The five prefix codes of one group: green (with lengths and cache), red, blue, alpha, distance.
struct Group {
    codes: [Huffman; 5],
}

fn read_group(br: &mut BitReader, cache_bits: u32, lengths: &mut Vec<u8>) -> Res<Group> {
    let g = read_code(br, green_alphabet(cache_bits), lengths)?;
    let r = read_code(br, NUM_LITERALS, lengths)?;
    let b = read_code(br, NUM_LITERALS, lengths)?;
    let a = read_code(br, NUM_LITERALS, lengths)?;
    let d = read_code(br, NUM_DIST_CODES, lengths)?;
    Ok(Group { codes: [g, r, b, a, d] })
}

enum Transform {
    Predictor { bits: u32, width: usize, data: Vec<u32> },
    CrossColor { bits: u32, width: usize, data: Vec<u32> },
    SubtractGreen,
    ColorIndexing { width: usize, palette: Vec<u32>, width_bits: u32 },
}

/// Decodes a VP8L chunk payload (from its 0x2f signature byte): the width, the height, and the
/// pixels as `0xAARRGGBB`, rows top to bottom. The `alpha_is_used` hint is ignored: the alpha
/// values are returned as coded.
pub fn decode(data: &[u8]) -> Res<(usize, usize, Vec<u32>)> {
    if data.len() < 5 {
        return Err("truncated VP8L header".into());
    }
    if data[0] != 0x2f {
        return Err("not a VP8L image (bad signature byte)".into());
    }
    let v = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
    let w = (v & 0x3fff) as usize + 1;
    let h = ((v >> 14) & 0x3fff) as usize + 1;
    if v >> 29 != 0 {
        return Err("unknown VP8L version".into());
    }
    check_size(w, h)?;
    let mut br = BitReader::new(&data[5..]);
    let px = decode_stream(&mut br, w, h, true)?;
    Ok((w, h, px))
}

/// Decodes the compressed data of an `ALPH` chunk with compression method 1 (the bytes after the
/// ALPH header byte): a VP8L image stream without the 5-byte header, alpha in the green channel.
/// Returns `w * h` alpha values before ALPH filtering is undone (the caller does that).
pub fn decode_alpha(data: &[u8], w: usize, h: usize) -> Res<Vec<u8>> {
    check_size(w, h)?;
    let mut br = BitReader::new(data);
    let px = decode_stream(&mut br, w, h, true)?;
    Ok(px.iter().map(|&p| (p >> 8) as u8).collect())
}

/// Decodes an image stream of `w` x `h`: with transforms and meta prefix codes when `top`, else
/// an entropy-coded sub-image.
fn decode_stream(br: &mut BitReader, w: usize, h: usize, top: bool) -> Res<Vec<u32>> {
    let mut transforms = Vec::new();
    let mut xsize = w;
    if top {
        let mut seen = 0u32;
        while br.read(1) == 1 {
            let kind = br.read(2);
            if seen & (1 << kind) != 0 {
                return Err("a VP8L transform is used twice".into());
            }
            seen |= 1 << kind;
            transforms.push(match kind {
                0 | 1 => {
                    let bits = br.read(3) + 2;
                    let data = decode_stream(br, subsample(xsize, bits), subsample(h, bits), false)?;
                    if kind == 0 {
                        Transform::Predictor { bits, width: xsize, data }
                    } else {
                        Transform::CrossColor { bits, width: xsize, data }
                    }
                }
                2 => Transform::SubtractGreen,
                _ => {
                    let n = br.read(8) as usize + 1;
                    let mut palette = decode_stream(br, n, 1, false)?;
                    for i in 1..n {
                        palette[i] = add_pixels(palette[i], palette[i - 1]);
                    }
                    let width_bits = match n {
                        1..=2 => 3,
                        3..=4 => 2,
                        5..=16 => 1,
                        _ => 0,
                    };
                    let t = Transform::ColorIndexing { width: xsize, palette, width_bits };
                    xsize = subsample(xsize, width_bits);
                    t
                }
            });
        }
    }
    let cache_bits = if br.read(1) == 1 {
        let b = br.read(4);
        if !(1..=MAX_CACHE_BITS).contains(&b) {
            return Err("bad VP8L color cache size".into());
        }
        b
    } else {
        0
    };
    // The entropy image, mapped to indices into the groups actually used.
    let mut meta: Option<(u32, usize, Vec<u32>)> = None;
    let mut lengths = Vec::new();
    let groups = if top && br.read(1) == 1 {
        let bits = br.read(3) + 2;
        let ew = subsample(xsize, bits);
        let mut ent = decode_stream(br, ew, subsample(h, bits), false)?;
        let num = ent.iter().map(|&p| (p >> 8) & 0xffff).max().unwrap_or(0) as usize + 1;
        let mut slot = vec![u32::MAX; num];
        let mut used = 0u32;
        for p in ent.iter_mut() {
            let g = ((*p >> 8) & 0xffff) as usize;
            if slot[g] == u32::MAX {
                slot[g] = used;
                used += 1;
            }
            *p = slot[g];
        }
        // Groups are stored in index order; unused ones are read (and checked) and dropped.
        let mut by_slot: Vec<Option<Group>> = (0..used).map(|_| None).collect();
        for &s in &slot {
            let group = read_group(br, cache_bits, &mut lengths)?;
            if s != u32::MAX {
                by_slot[s as usize] = Some(group);
            }
        }
        meta = Some((bits, ew, ent));
        by_slot.into_iter().map(|g| g.ok_or_else(|| "a missing prefix-code group".to_string())).collect::<Res<Vec<_>>>()?
    } else {
        vec![read_group(br, cache_bits, &mut lengths)?]
    };
    let mut px = decode_pixels(br, xsize, h, &groups, meta.as_ref(), cache_bits)?;
    for t in transforms.iter().rev() {
        px = match t {
            Transform::Predictor { bits, width, data } => {
                inverse_predictor(&mut px, *width, h, *bits, data);
                px
            }
            Transform::CrossColor { bits, width, data } => {
                inverse_cross_color(&mut px, *width, *bits, data);
                px
            }
            Transform::SubtractGreen => {
                for p in px.iter_mut() {
                    let g = (*p >> 8) & 0xff;
                    *p = add_pixels(*p, (g << 16) | g);
                }
                px
            }
            Transform::ColorIndexing { width, palette, width_bits } => inverse_palette(&px, *width, h, palette, *width_bits),
        };
    }
    Ok(px)
}

/// Decodes the LZ77-coded pixels of a `w` x `h` image.
fn decode_pixels(
    br: &mut BitReader,
    w: usize,
    h: usize,
    groups: &[Group],
    meta: Option<&(u32, usize, Vec<u32>)>,
    cache_bits: u32,
) -> Res<Vec<u32>> {
    let total = w * h;
    if total > MAX_PIXELS {
        return Err("the image is too large".into());
    }
    let mut out = vec![0u32; total];
    let mut cache = vec![0u32; if cache_bits > 0 { 1 << cache_bits } else { 0 }];
    let mut cached = 0; // pixels before this index are in the cache
    let (mut i, mut x, mut y) = (0usize, 0usize, 0usize);
    let mut group = &groups[0];
    while i < total {
        if let Some((bits, ew, ent)) = meta {
            group = &groups[ent[(y >> bits) * ew + (x >> bits)] as usize];
        }
        let [gc, rc, bc, ac, dc] = &group.codes;
        let s = gc.read(br);
        if s < NUM_LITERALS as u32 {
            let r = rc.read(br);
            let b = bc.read(br);
            let a = ac.read(br);
            out[i] = (a << 24) | (r << 16) | (s << 8) | b;
            i += 1;
            x += 1;
            if x == w {
                x = 0;
                y += 1;
            }
        } else if s < (NUM_LITERALS + NUM_LENGTH_CODES) as u32 {
            let len = prefix_value(br, s - NUM_LITERALS as u32);
            let dsym = dc.read(br);
            let dist = code_to_distance(prefix_value(br, dsym), w);
            if dist > i || len > total - i {
                return Err("a VP8L backward reference is out of bounds".into());
            }
            if dist >= len {
                out.copy_within(i - dist..i - dist + len, i);
            } else {
                for k in i..i + len {
                    out[k] = out[k - dist];
                }
            }
            i += len;
            x += len;
            if x >= w {
                y += x / w;
                x %= w;
            }
        } else {
            let slot = (s - (NUM_LITERALS + NUM_LENGTH_CODES) as u32) as usize;
            let bits = cache_bits;
            while cached < i {
                let p = out[cached];
                cache[cache_slot(p, bits)] = p;
                cached += 1;
            }
            out[i] = cache[slot];
            i += 1;
            x += 1;
            if x == w {
                x = 0;
                y += 1;
            }
        }
        if br.overrun() {
            return Err("truncated VP8L data".into());
        }
    }
    Ok(out)
}

fn inverse_predictor(px: &mut [u32], w: usize, h: usize, bits: u32, modes: &[u32]) {
    let mw = subsample(w, bits);
    px[0] = add_pixels(px[0], 0xff00_0000);
    for x in 1..w {
        px[x] = add_pixels(px[x], px[x - 1]);
    }
    for y in 1..h {
        let row = y * w;
        px[row] = add_pixels(px[row], px[row - w]);
        let mrow = &modes[(y >> bits) * mw..];
        let mut x = 1;
        while x < w {
            let mode = (mrow[x >> bits] >> 8) & 0xf;
            let end = (((x >> bits) + 1) << bits).min(w);
            for i in row + x..row + end {
                let p = predict(mode, px[i - 1], px[i - w], px[i - w - 1], px[i - w + 1]);
                px[i] = add_pixels(px[i], p);
            }
            x = end;
        }
    }
}

fn inverse_cross_color(px: &mut [u32], w: usize, bits: u32, data: &[u32]) {
    let mw = subsample(w, bits);
    for (i, p) in px.iter_mut().enumerate() {
        let (x, y) = (i % w, i / w);
        let m = data[(y >> bits) * mw + (x >> bits)];
        let (g2r, g2b, r2b) = (m as u8, (m >> 8) as u8, (m >> 16) as u8);
        let g = (*p >> 8) as u8;
        let r = ((*p >> 16) as i32 + color_delta(g2r, g)) as u8;
        let b = ((*p & 0xff) as i32 + color_delta(g2b, g) + color_delta(r2b, r)) as u8;
        *p = (*p & 0xff00_ff00) | ((r as u32) << 16) | b as u32;
    }
}

fn inverse_palette(px: &[u32], w: usize, h: usize, palette: &[u32], width_bits: u32) -> Vec<u32> {
    let pw = subsample(w, width_bits);
    let bpp = 8 >> width_bits;
    let mask = (1u32 << bpp) - 1;
    let per = (1usize << width_bits) - 1;
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        let row = &px[y * pw..(y + 1) * pw];
        for x in 0..w {
            let idx = ((row[x >> width_bits] >> 8) >> ((x & per) as u32 * bpp)) & mask;
            out.push(palette.get(idx as usize).copied().unwrap_or(0));
        }
    }
    out
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// The payload of the first chunk `id` of a RIFF WebP file.
    pub(in super::super) fn riff_chunk<'a>(file: &'a [u8], id: &[u8; 4]) -> &'a [u8] {
        let mut p = 12;
        while p + 8 <= file.len() {
            let size = u32::from_le_bytes([file[p + 4], file[p + 5], file[p + 6], file[p + 7]]) as usize;
            if &file[p..p + 4] == id {
                return &file[p + 8..p + 8 + size];
            }
            p += 8 + size + (size & 1);
        }
        panic!("no {} chunk", String::from_utf8_lossy(id));
    }

    fn argb(r: usize, g: usize, b: usize, a: usize) -> u32 {
        ((a as u32 & 255) << 24) | ((r as u32 & 255) << 16) | ((g as u32 & 255) << 8) | (b as u32 & 255)
    }

    /// The pixels of fixture `name`, as `tools/img_fixtures_vp8l.py` defines them.
    fn expected(name: &str, w: usize, h: usize) -> Vec<u32> {
        if name == "noise" {
            let mut s: u64 = 12345;
            let mut b = Vec::new();
            for _ in 0..w * h * 4 {
                s = (s * 1103515245 + 12345) % (1 << 31);
                b.push(((s >> 16) & 255) as usize);
            }
            return b.chunks(4).map(|c| argb(c[0], c[1], c[2], c[3])).collect();
        }
        let mut out = Vec::new();
        for y in 0..h {
            for x in 0..w {
                out.push(match name {
                    "grad" => argb(x * 7, y * 11, (x + y) * 3, 255),
                    "alpha" => argb(x * 12, y * 13, 128, x * y * 5),
                    "pal12" => {
                        let i = (x / 3 + (y / 2) * 5) % 12;
                        argb(i * 20, 255 - i * 15, i * i * 7, 255)
                    }
                    "pal200" => {
                        let i = (x * 7 + y * 13) % 200;
                        argb(i, i * 3, 255 - i, if i % 5 != 0 { 255 } else { 128 })
                    }
                    "pal2" => {
                        if (x + y) % 3 == 0 {
                            argb(255, 0, 0, 255)
                        } else {
                            argb(0, 0, 255, 255)
                        }
                    }
                    "one" => argb(10, 20, 30, 40),
                    "tall" => argb(x * 80, y * 30, x * y * 10, 255),
                    "rep" => argb((x / 10) * 25, (x % 10) * 20, y * 60, 255),
                    "alph" => argb(x * 9, 100, y * 15, x * 10 + y * 7),
                    _ => unreachable!(),
                });
            }
        }
        out
    }

    pub(in super::super) const FIXTURES: [(&str, &str, usize, usize, &[u8]); 12] = [
        ("grad", "m0", 37, 23, include_bytes!("testdata/vp8l-grad-m0.webp")),
        ("grad", "m4", 37, 23, include_bytes!("testdata/vp8l-grad-m4.webp")),
        ("grad", "m6", 37, 23, include_bytes!("testdata/vp8l-grad-m6.webp")),
        ("alpha", "m4", 21, 19, include_bytes!("testdata/vp8l-alpha-m4.webp")),
        ("alpha", "m6", 21, 19, include_bytes!("testdata/vp8l-alpha-m6.webp")),
        ("pal12", "m4", 33, 17, include_bytes!("testdata/vp8l-pal12-m4.webp")),
        ("pal200", "m6", 40, 30, include_bytes!("testdata/vp8l-pal200-m6.webp")),
        ("pal2", "m4", 29, 5, include_bytes!("testdata/vp8l-pal2-m4.webp")),
        ("noise", "m4", 16, 16, include_bytes!("testdata/vp8l-noise-m4.webp")),
        ("one", "m4", 1, 1, include_bytes!("testdata/vp8l-one-m4.webp")),
        ("tall", "m6", 3, 7, include_bytes!("testdata/vp8l-tall-m6.webp")),
        ("rep", "m6", 300, 4, include_bytes!("testdata/vp8l-rep-m6.webp")),
    ];

    #[test]
    fn pillow_fixtures_decode_exactly() {
        for (name, method, w, h, file) in FIXTURES {
            let (dw, dh, px) = decode(riff_chunk(file, b"VP8L")).unwrap_or_else(|e| panic!("{name}-{method}: {e}"));
            assert_eq!((dw, dh), (w, h), "{name}-{method}");
            let want = expected(name, w, h);
            for (i, (&got, &want)) in px.iter().zip(&want).enumerate() {
                assert_eq!(got, want, "{name}-{method}: pixel ({}, {}): {got:08x} != {want:08x}", i % w, i / w);
            }
        }
    }

    /// Undoes ALPH filtering (method 1 horizontal, 2 vertical, 3 gradient) in place.
    fn unfilter(a: &mut [u8], w: usize, h: usize, method: u8) {
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let pred = match (method, x, y) {
                    (0, _, _) | (_, 0, 0) => 0,
                    (_, _, 0) => a[i - 1],
                    (_, 0, _) => a[i - w],
                    (1, _, _) => a[i - 1],
                    (2, _, _) => a[i - w],
                    _ => (a[i - 1] as i32 + a[i - w] as i32 - a[i - w - 1] as i32).clamp(0, 255) as u8,
                };
                a[i] = a[i].wrapping_add(pred);
            }
        }
    }

    #[test]
    fn pillow_alph_chunk_decodes_exactly() {
        let alph = riff_chunk(include_bytes!("testdata/vp8l-alph.webp"), b"ALPH");
        assert_eq!(alph[0] & 3, 1, "the fixture's alpha is not VP8L-compressed");
        let (w, h) = (24, 16);
        let mut a = decode_alpha(&alph[1..], w, h).unwrap();
        unfilter(&mut a, w, h, (alph[0] >> 2) & 3);
        let want: Vec<u8> = expected("alph", w, h).iter().map(|&p| (p >> 24) as u8).collect();
        assert_eq!(a, want);
    }

    #[test]
    fn corrupt_and_truncated_data_never_panic() {
        let mut checked = 0;
        for (name, _, _, _, file) in FIXTURES {
            let data = riff_chunk(file, b"VP8L");
            for n in 0..data.len() {
                let r = std::panic::catch_unwind(|| decode(&data[..n]).is_ok());
                assert!(r.is_ok(), "{name}: a truncation to {n} bytes panicked");
                checked += 1;
            }
            let mut s: u32 = 7;
            for _ in 0..60 {
                let mut d = data.to_vec();
                for _ in 0..1 + (s >> 29) {
                    s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let at = 5 + (s >> 8) as usize % (d.len() - 5).max(1);
                    if at < d.len() {
                        d[at] ^= 1 << ((s >> 5) & 7);
                    }
                }
                let r = std::panic::catch_unwind(|| decode(&d).is_ok());
                assert!(r.is_ok(), "{name}: a corruption panicked");
                checked += 1;
            }
        }
        assert!(checked > 1000);
    }

    #[test]
    fn hostile_headers_are_refused() {
        assert!(decode(&[]).is_err());
        assert!(decode(&[0x2f, 0, 0, 0]).is_err());
        assert!(decode(&[0x2e, 0, 0, 0, 0, 0]).is_err());
        // 16384 x 16384 is over MAX_PIXELS.
        assert!(decode(&[0x2f, 0xff, 0xff, 0xff, 0x0f, 0, 0, 0]).is_err());
        // Version 1.
        assert!(decode(&[0x2f, 0, 0, 0, 0x20, 0, 0, 0]).is_err());
    }
}

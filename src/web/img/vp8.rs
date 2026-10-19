//! The lossy WebP encoder: one VP8 key frame (RFC 6386), the payload of a `VP8 ` chunk.
//!
//! How it works:
//! - RGB is converted to YUV 4:2:0 (BT.601, studio range, as decoders expect); chroma is averaged
//!   over 2x2 pixels weighted by alpha; fully transparent pixels do not count as distortion and
//!   fully transparent blocks are made flat, so they cost almost nothing.
//! - Each macroblock is classified by how busy it is and put in one of up to 4 segments; busy
//!   segments get a coarser quantizer than smooth ones, where errors show more (like libwebp's
//!   "spatial noise shaping").
//! - Each macroblock picks its prediction (16x16 or sixteen 4x4 blocks, and the chroma mode) by
//!   rate-distortion: the squared error (optionally plus a term that keeps texture energy) plus a
//!   lambda times the bits the boolean coder will spend, estimated from the token probabilities.
//!   The reconstruction is bit-exact with libwebp's decoder (checked against Pillow).
//! - Coefficients are quantized with a dead zone, then the token probabilities are optimized for
//!   the image and sent in the frame header when that saves bits.
//! - The loop filter level of each segment is chosen by trying levels on the reconstruction.

use super::vp8_dsp::{self as dsp, Ctx, FilterParams, CS, S};
use super::vp8_tables::*;

/// Encoder settings.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// 0..=100, roughly libwebp's quality scale.
    pub quality: f32,
    /// Spatial noise shaping, 0..=100: how much coarser busy segments are quantized.
    pub sns: f32,
    /// Segments, 1..=4.
    pub segments: usize,
    /// The rate-distortion lambda as a multiple of the squared AC quantizer step.
    pub lambda: f32,
    /// Weight of the texture-energy term in the distortion (0 disables it).
    pub texture: f32,
    /// Weight of chroma distortion relative to luma.
    pub chroma_weight: f32,
    /// Rounding bias of the quantizer for AC coefficients (in 1/256 of a step; 128 rounds to nearest).
    pub bias_ac: u32,
    /// The same for DC coefficients.
    pub bias_dc: u32,
    /// Choose the loop filter level by trying levels (else a level from the quantizer).
    pub search_filter: bool,
    /// Passes: 2 decides the modes again with the token probabilities of the first pass.
    pub passes: usize,
    /// How many 4x4 modes (of 10, ranked by a quick estimate) get the full rate-distortion test.
    pub i4_candidates: usize,
}

impl Options {
    pub fn new(quality: f32) -> Options {
        Options {
            quality,
            sns: 50.0,
            segments: 4,
            lambda: 0.02,
            texture: 0.0,
            chroma_weight: 1.0,
            bias_ac: 110,
            bias_dc: 96,
            search_filter: true,
            passes: 1,
            i4_candidates: 10,
        }
    }
}

// ---- YUV planes ------------------------------------------------------------------------------------

/// An image as VP8 codes it: Y, U and V planes padded to whole macroblocks.
pub struct Yuv {
    pub w: usize,
    pub h: usize,
    pub mbw: usize,
    pub mbh: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    /// Per luma pixel: 0 where the pixel is fully transparent (its color does not matter), else 1.
    pub mask: Option<Vec<u8>>,
    /// The same per chroma sample (0 where all four pixels are transparent).
    pub cmask: Option<Vec<u8>>,
}

impl Yuv {
    /// Converts RGBA (straight alpha) to YUV 4:2:0, edges replicated into the padding.
    pub fn from_rgba(w: usize, h: usize, px: &[u8]) -> Yuv {
        let (mbw, mbh) = (w.div_ceil(16), h.div_ceil(16));
        let (ys, cs) = (mbw * 16, mbw * 8);
        let mut y = vec![0u8; ys * mbh * 16];
        let mut u = vec![0u8; cs * mbh * 8];
        let mut v = vec![0u8; cs * mbh * 8];
        let has_alpha = px.as_chunks::<4>().0.iter().any(|p| p[3] != 255);
        let mut mask = has_alpha.then(|| vec![1u8; ys * mbh * 16]);
        let mut cmask = has_alpha.then(|| vec![1u8; cs * mbh * 8]);
        for r in 0..mbh * 16 {
            let sr = r.min(h - 1);
            for c in 0..ys {
                let sc = c.min(w - 1);
                let p = &px[(sr * w + sc) * 4..];
                let luma = 16839 * p[0] as i32 + 33059 * p[1] as i32 + 6420 * p[2] as i32;
                y[r * ys + c] = ((luma + (1 << 15) + (16 << 16)) >> 16) as u8;
                if let Some(m) = mask.as_mut() {
                    m[r * ys + c] = (p[3] != 0) as u8;
                }
            }
        }
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        for r in 0..mbh * 8 {
            let cr = r.min(ch - 1);
            for c in 0..cs {
                let cc = c.min(cw - 1);
                // the 2x2 pixels, weighted by alpha
                let (mut sr, mut sg, mut sb, mut sa, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
                let (mut pr, mut pg, mut pb) = (0u32, 0u32, 0u32);
                for dy in 0..2 {
                    for dx in 0..2 {
                        let (yy, xx) = ((cr * 2 + dy).min(h - 1), (cc * 2 + dx).min(w - 1));
                        let p = &px[(yy * w + xx) * 4..];
                        let a = p[3] as u32;
                        sr += p[0] as u32 * a;
                        sg += p[1] as u32 * a;
                        sb += p[2] as u32 * a;
                        sa += a;
                        pr += p[0] as u32;
                        pg += p[1] as u32;
                        pb += p[2] as u32;
                        n += 1;
                    }
                }
                let (rr, gg, bb) = if sa > 0 {
                    (sr as f32 / sa as f32, sg as f32 / sa as f32, sb as f32 / sa as f32)
                } else {
                    (pr as f32 / n as f32, pg as f32 / n as f32, pb as f32 / n as f32)
                };
                let uu = (-9719.0 * rr - 19081.0 * gg + 28800.0 * bb) / 65536.0 + 128.0;
                let vv = (28800.0 * rr - 24116.0 * gg - 4684.0 * bb) / 65536.0 + 128.0;
                u[r * cs + c] = uu.round().clamp(0.0, 255.0) as u8;
                v[r * cs + c] = vv.round().clamp(0.0, 255.0) as u8;
                if let Some(m) = cmask.as_mut() {
                    m[r * cs + c] = (sa > 0) as u8;
                }
            }
        }
        // fully transparent blocks become flat, continuing their left (or upper) neighbor, so
        // they cost almost nothing (their color is invisible)
        if let (Some(m), Some(cm)) = (&mask, &cmask) {
            flatten(&mut y, m, ys, 8);
            flatten(&mut u, cm, cs, 4);
            flatten(&mut v, cm, cs, 4);
        }
        Yuv { w, h, mbw, mbh, y, u, v, mask, cmask }
    }
}

/// Sets each `n` x `n` block whose mask is all 0 to one value: the mean of the pixels just left
/// of it, else just above it, else 128.
fn flatten(p: &mut [u8], mask: &[u8], stride: usize, n: usize) {
    let rows = p.len() / stride;
    for by in (0..rows).step_by(n) {
        for bx in (0..stride).step_by(n) {
            if (0..n).any(|r| mask[(by + r) * stride + bx..][..n].iter().any(|&k| k != 0)) {
                continue;
            }
            let v = if bx > 0 {
                (0..n).map(|r| p[(by + r) * stride + bx - 1] as u32).sum::<u32>() / n as u32
            } else if by > 0 {
                p[(by - 1) * stride + bx..][..n].iter().map(|&x| x as u32).sum::<u32>() / n as u32
            } else {
                128
            };
            for r in 0..n {
                p[(by + r) * stride + bx..][..n].fill(v as u8);
            }
        }
    }
}

// ---- the boolean coder -------------------------------------------------------------------------------

/// The boolean entropy coder of RFC 6386 section 7.
pub struct BoolWriter {
    pub out: Vec<u8>,
    range: u32,
    bottom: u32,
    bit_count: i32,
}

impl BoolWriter {
    pub fn new() -> BoolWriter {
        BoolWriter { out: Vec::new(), range: 255, bottom: 0, bit_count: 24 }
    }

    fn carry(&mut self) {
        for b in self.out.iter_mut().rev() {
            if *b == 255 {
                *b = 0;
            } else {
                *b += 1;
                return;
            }
        }
    }

    #[inline]
    pub fn put(&mut self, bit: bool, prob: u8) {
        let split = 1 + (((self.range - 1) * prob as u32) >> 8);
        if bit {
            self.bottom = self.bottom.wrapping_add(split);
            self.range -= split;
        } else {
            self.range = split;
        }
        while self.range < 128 {
            self.range <<= 1;
            if self.bottom & (1 << 31) != 0 {
                self.carry();
            }
            self.bottom <<= 1;
            self.bit_count -= 1;
            if self.bit_count == 0 {
                self.out.push((self.bottom >> 24) as u8);
                self.bottom &= (1 << 24) - 1;
                self.bit_count = 8;
            }
        }
    }

    /// Puts `bit` with probability `prob` and returns it.
    pub fn put_ret(&mut self, bit: bool, prob: u8) -> bool {
        self.put(bit, prob);
        bit
    }

    pub fn flag(&mut self, bit: bool) -> bool {
        self.put(bit, 128);
        bit
    }

    /// `n` bits of `v`, most significant first, with probability 1/2.
    pub fn literal(&mut self, v: u32, n: u32) {
        for i in (0..n).rev() {
            self.put((v >> i) & 1 != 0, 128);
        }
    }

    /// A flag, then (when `v != 0`) the magnitude in `n` bits and the sign.
    pub fn signed(&mut self, v: i32, n: u32) {
        if self.flag(v != 0) {
            self.literal(v.unsigned_abs(), n);
            self.flag(v < 0);
        }
    }

    pub fn finish(mut self) -> Vec<u8> {
        let mut c = self.bit_count;
        let mut v = self.bottom;
        if c < 32 && v & (1u32 << (32 - c)) != 0 {
            self.carry();
        }
        v = v.wrapping_shl((c & 7) as u32);
        c >>= 3;
        while c > 0 {
            v = v.wrapping_shl(8);
            c -= 1;
        }
        for _ in 0..4 {
            self.out.push((v >> 24) as u8);
            v = v.wrapping_shl(8);
        }
        self.out
    }
}

impl Default for BoolWriter {
    fn default() -> Self {
        BoolWriter::new()
    }
}

/// The cost of coding a `0` with probability `p` (of a zero), in 1/256 bit.
fn bit_cost_table() -> [u16; 256] {
    std::array::from_fn(|p| if p == 0 { 2048 } else { (-(p as f64 / 256.0).log2() * 256.0).round() as u16 })
}

#[inline]
fn cost(t: &[u16; 256], p: u8, bit: bool) -> u32 {
    if bit {
        t[(256 - p as usize).min(255)] as u32
    } else {
        t[p as usize] as u32
    }
}

/// The leaves of a tree with the path to each: (value, [(node index, bit)]).
fn tree_paths(tree: &[i8]) -> Vec<(u8, Vec<(usize, bool)>)> {
    fn walk(tree: &[i8], at: usize, path: &mut Vec<(usize, bool)>, out: &mut Vec<(u8, Vec<(usize, bool)>)>) {
        for bit in [false, true] {
            let next = tree[at + bit as usize];
            path.push((at / 2, bit));
            if next <= 0 {
                out.push(((-next) as u8, path.clone()));
            } else {
                walk(tree, next as usize, path, out);
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(tree, 0, &mut Vec::new(), &mut out);
    out
}

fn put_tree(bw: &mut BoolWriter, paths: &[(u8, Vec<(usize, bool)>)], probs: &[u8], v: u8) {
    let path = &paths.iter().find(|p| p.0 == v).expect("a leaf of the tree").1;
    for &(node, bit) in path {
        bw.put(bit, probs[node]);
    }
}

fn tree_cost(t: &[u16; 256], paths: &[(u8, Vec<(usize, bool)>)], probs: &[u8], v: u8) -> u32 {
    let path = &paths.iter().find(|p| p.0 == v).expect("a leaf of the tree").1;
    path.iter().map(|&(node, bit)| cost(t, probs[node], bit)).sum()
}

// ---- token costs ----------------------------------------------------------------------------------

type Probs = [[[[u8; 11]; 3]; 8]; 4];

/// The largest coefficient level.
const MAX_LEVEL: i32 = 2048 + 66;

/// The bit costs of the coefficient tokens for a set of probabilities.
struct Costs {
    /// [type][band][ctx]: the cost of the end-of-block token.
    eob: [[[u32; 3]; 8]; 4],
    /// The cost of "not end of block".
    more: [[[u32; 3]; 8]; 4],
    /// The cost of a zero token (after "not end of block").
    zero: [[[u32; 3]; 8]; 4],
    /// The cost of a level 1..=67 (index), with the sign; level 67 and up: the cost of category 6
    /// without its extra bits.
    level: Box<[[[[u32; 68]; 3]; 8]; 4]>,
    /// The extra bits of category 6, by `level - 67`.
    cat6: Vec<u32>,
}

/// The token of a level: the tree branch bits after "nonzero", and the extra bits.
fn level_bits(v: i32) -> (&'static [(usize, bool)], u32, &'static [u8]) {
    // (node index, bit) pairs from node 2 on, then (extra value, extra probabilities)
    match v {
        1 => (&[(2, false)], 0, &[]),
        2 => (&[(2, true), (3, false), (4, false)], 0, &[]),
        3 => (&[(2, true), (3, false), (4, true), (5, false)], 0, &[]),
        4 => (&[(2, true), (3, false), (4, true), (5, true)], 0, &[]),
        5..=6 => (&[(2, true), (3, true), (6, false), (7, false)], (v - 5) as u32, &[159]),
        7..=10 => (&[(2, true), (3, true), (6, false), (7, true)], (v - 7) as u32, &[165, 145]),
        11..=18 => (&[(2, true), (3, true), (6, true), (8, false), (9, false)], (v - 11) as u32, &CAT3),
        19..=34 => (&[(2, true), (3, true), (6, true), (8, false), (9, true)], (v - 19) as u32, &CAT4),
        35..=66 => (&[(2, true), (3, true), (6, true), (8, true), (10, false)], (v - 35) as u32, &CAT5),
        _ => (&[(2, true), (3, true), (6, true), (8, true), (10, true)], (v - 67) as u32, &CAT6),
    }
}

impl Costs {
    fn new(p: &Probs, t: &[u16; 256]) -> Costs {
        let mut c = Costs {
            eob: [[[0; 3]; 8]; 4],
            more: [[[0; 3]; 8]; 4],
            zero: [[[0; 3]; 8]; 4],
            level: Box::new([[[[0; 68]; 3]; 8]; 4]),
            cat6: Vec::with_capacity(2048),
        };
        for ty in 0..4 {
            for b in 0..8 {
                for x in 0..3 {
                    let pr = &p[ty][b][x];
                    c.eob[ty][b][x] = cost(t, pr[0], false);
                    c.more[ty][b][x] = cost(t, pr[0], true);
                    c.zero[ty][b][x] = cost(t, pr[1], false);
                    for v in 1..=67 {
                        let (path, _, extra) = level_bits(v);
                        let mut sum = cost(t, pr[1], true) + 256; // nonzero, sign
                        for &(node, bit) in path {
                            sum += cost(t, pr[node], bit);
                        }
                        if v < 67 {
                            let (_, e, _) = level_bits(v);
                            for (k, &ep) in extra.iter().enumerate() {
                                sum += cost(t, ep, (e >> (extra.len() - 1 - k)) & 1 != 0);
                            }
                        }
                        c.level[ty][b][x][v as usize] = sum;
                    }
                }
            }
        }
        for e in 0..2048u32 {
            c.cat6.push(CAT6.iter().enumerate().map(|(k, &ep)| cost(t, ep, (e >> (10 - k)) & 1 != 0)).sum());
        }
        c
    }

    #[inline]
    fn level(&self, ty: usize, b: usize, x: usize, v: i32) -> u32 {
        if v < 67 {
            self.level[ty][b][x][v as usize]
        } else {
            self.level[ty][b][x][67] + self.cat6[((v - 67) as usize).min(2047)]
        }
    }

    /// The bits of a block of levels (coding order) from position `first`, in 1/256 bit.
    fn block(&self, levels: &[i16; 16], first: usize, ty: usize, ctx: usize) -> u32 {
        let last = match (first..16).rev().find(|&i| levels[i] != 0) {
            Some(l) => l,
            None => return self.eob[ty][BANDS[first]][ctx],
        };
        let mut x = ctx;
        let mut sum = 0;
        let mut prev_zero = false;
        for (i, &lv) in levels.iter().enumerate().take(last + 1).skip(first) {
            let b = BANDS[i];
            if !prev_zero {
                sum += self.more[ty][b][x];
            }
            let v = (lv as i32).abs();
            if v == 0 {
                sum += self.zero[ty][b][x];
                prev_zero = true;
                x = 0;
            } else {
                sum += self.level(ty, b, x, v);
                prev_zero = false;
                x = if v == 1 { 1 } else { 2 };
            }
        }
        if last < 15 {
            sum += self.eob[ty][BANDS[last + 1]][x];
        }
        sum
    }
}

/// Codes a block of levels; `stats` (when given) counts the bits at each tree node instead.
fn put_block(bw: &mut BoolWriter, p: &Probs, levels: &[i16; 16], first: usize, ty: usize, ctx: usize) {
    let mut x = ctx;
    let last = (first..16).rev().find(|&i| levels[i] != 0);
    let Some(last) = last else {
        bw.put(false, p[ty][BANDS[first]][ctx][0]);
        return;
    };
    let mut prev_zero = false;
    for (i, &lv) in levels.iter().enumerate().take(last + 1).skip(first) {
        let pr = &p[ty][BANDS[i]][x];
        if !prev_zero {
            bw.put(true, pr[0]);
        }
        let v = (lv as i32).abs();
        if v == 0 {
            bw.put(false, pr[1]);
            prev_zero = true;
            x = 0;
            continue;
        }
        bw.put(true, pr[1]);
        let (path, e, extra) = level_bits(v.min(MAX_LEVEL));
        for &(node, bit) in path {
            bw.put(bit, pr[node]);
        }
        for (k, &ep) in extra.iter().enumerate() {
            bw.put((e >> (extra.len() - 1 - k)) & 1 != 0, ep);
        }
        bw.put(lv < 0, 128);
        prev_zero = false;
        x = if v == 1 { 1 } else { 2 };
    }
    if last < 15 {
        bw.put(false, p[ty][BANDS[last + 1]][x][0]);
    }
}

/// Counts, for each probability, how often its node codes a 0 and a 1.
type Stats = [[[[[u32; 2]; 11]; 3]; 8]; 4];

fn count_block(st: &mut Stats, levels: &[i16; 16], first: usize, ty: usize, ctx: usize) {
    let mut x = ctx;
    let Some(last) = (first..16).rev().find(|&i| levels[i] != 0) else {
        st[ty][BANDS[first]][ctx][0][0] += 1;
        return;
    };
    let mut prev_zero = false;
    for (i, &lv) in levels.iter().enumerate().take(last + 1).skip(first) {
        let s = &mut st[ty][BANDS[i]][x];
        if !prev_zero {
            s[0][1] += 1;
        }
        let v = (lv as i32).abs();
        if v == 0 {
            s[1][0] += 1;
            prev_zero = true;
            x = 0;
            continue;
        }
        s[1][1] += 1;
        let (path, _, _) = level_bits(v.min(MAX_LEVEL));
        for &(node, bit) in path {
            s[node][bit as usize] += 1;
        }
        prev_zero = false;
        x = if v == 1 { 1 } else { 2 };
    }
    if last < 15 {
        st[ty][BANDS[last + 1]][x][0][0] += 1;
    }
}

// ---- quantization ---------------------------------------------------------------------------------

/// A quantizer for one kind of block: steps for DC and AC, rounding biases, sharpening.
#[derive(Clone, Copy, Debug)]
struct QMat {
    q: [u32; 2],
    iq: [u32; 2],
    bias: [u32; 2],
    sharpen: [u32; 16],
}

const QFIX: u32 = 17;

impl QMat {
    fn new(dc: u32, ac: u32, bias_dc: u32, bias_ac: u32, sharpen: bool) -> QMat {
        const FREQ_SHARPENING: [u32; 16] = [0, 30, 60, 90, 30, 60, 90, 90, 60, 90, 90, 90, 90, 90, 90, 90];
        let q = [dc, ac];
        QMat {
            q,
            iq: [(1 << QFIX) / dc, (1 << QFIX) / ac],
            bias: [bias_dc << (QFIX - 8), bias_ac << (QFIX - 8)],
            sharpen: std::array::from_fn(|i| if sharpen { (FREQ_SHARPENING[i] * q[(i > 0) as usize]) >> 11 } else { 0 }),
        }
    }

    /// Quantizes `coef` (raster order) from coding position `first`: the levels in coding order and
    /// the dequantized coefficients (raster order). True if a level is not zero.
    fn quantize(&self, coef: &[i16; 16], first: usize, levels: &mut [i16; 16], deq: &mut [i16; 16]) -> bool {
        let mut nz = false;
        for n in 0..16 {
            let j = ZIGZAG[n];
            if n < first {
                levels[n] = 0;
                deq[j] = 0;
                continue;
            }
            let k = (n > 0) as usize;
            let c = coef[j] as i32;
            let a = c.unsigned_abs() + self.sharpen[j];
            let mut lv = ((a * self.iq[k] + self.bias[k]) >> QFIX) as i32;
            lv = lv.min(MAX_LEVEL);
            if c < 0 {
                lv = -lv;
            }
            levels[n] = lv as i16;
            deq[j] = (lv * self.q[k] as i32) as i16;
            nz |= lv != 0;
        }
        nz
    }
}

/// The quantizers and the rate-distortion lambda of a segment.
#[derive(Clone, Debug)]
struct Segment {
    /// The quantizer index 0..=127.
    q: i32,
    y1: QMat,
    y2: QMat,
    uv: QMat,
    /// Score = 256 * distortion + lambda * bits(1/256).
    lambda: u64,
    /// For the quick estimates: about the square root of `lambda` (in 1/16).
    sqrt_lambda: u64,
    /// The weight of the texture term (in 1/256).
    tlambda: u32,
    filter: i32,
}

fn clip_q(q: i32) -> usize {
    q.clamp(0, 127) as usize
}

/// The quantizer index for a quality and an exponent (libwebp's mapping).
fn quality_to_q(quality: f32, expn: f64) -> i32 {
    let c = (quality as f64 / 100.0).clamp(0.0, 1.0);
    let linear = if c < 0.75 { c * (2.0 / 3.0) } else { 2.0 * c - 1.0 };
    let base = linear.powf(1.0 / 3.0);
    let cc = base.powf(expn);
    (127.0 * (1.0 - cc)) as i32
}

// ---- the encoder ----------------------------------------------------------------------------------

/// What was decided for one macroblock.
#[derive(Clone)]
struct Mb {
    segment: u8,
    ymode: u8,
    bmodes: [u8; 16],
    uvmode: u8,
    /// Levels in coding order: 16 luma blocks, 4 U, 4 V, the Y2 block.
    levels: Box<[[i16; 16]; 25]>,
    /// No level is nonzero.
    skip: bool,
}

/// The nonzero flags along an edge: 4 luma, 2 U, 2 V, Y2.
#[derive(Clone, Copy, Default)]
struct Nz {
    y: [u8; 4],
    u: [u8; 2],
    v: [u8; 2],
    dc: u8,
}

/// The modes along an edge, for the 4x4 mode contexts.
type EdgeModes = [u8; 4];

fn implied_bmode(ymode: u8) -> u8 {
    match ymode {
        V_PRED => B_VE,
        H_PRED => B_HE,
        TM_PRED => B_TM,
        _ => B_DC,
    }
}

struct Trees {
    ymode: Vec<(u8, Vec<(usize, bool)>)>,
    uvmode: Vec<(u8, Vec<(usize, bool)>)>,
    bmode: Vec<(u8, Vec<(usize, bool)>)>,
    bits: [u16; 256],
    ymode_cost: [u32; 5],
    uv_cost: [u32; 4],
    /// [above][left][mode]
    bmode_cost: Box<[[[u32; 10]; 10]; 10]>,
}

impl Trees {
    fn new() -> Trees {
        let bits = bit_cost_table();
        let ymode = tree_paths(&KF_YMODE_TREE);
        let uvmode = tree_paths(&UV_MODE_TREE);
        let bmode = tree_paths(&BMODE_TREE);
        let ymode_cost = std::array::from_fn(|m| tree_cost(&bits, &ymode, &KF_YMODE_PROBS, m as u8));
        let uv_cost = std::array::from_fn(|m| tree_cost(&bits, &uvmode, &KF_UV_MODE_PROBS, m as u8));
        let mut bmode_cost = Box::new([[[0u32; 10]; 10]; 10]);
        for a in 0..10 {
            for l in 0..10 {
                for m in 0..10 {
                    bmode_cost[a][l][m] = tree_cost(&bits, &bmode, &BMODE_PROBS[a][l], m as u8);
                }
            }
        }
        Trees { ymode, uvmode, bmode, bits, ymode_cost, uv_cost, bmode_cost }
    }
}

fn sse(a: &[u8], b: &[u8], mask: Option<&[u8]>) -> u64 {
    match mask {
        None => a.iter().zip(b).map(|(&x, &y)| ((x as i32 - y as i32) * (x as i32 - y as i32)) as u64).sum(),
        Some(m) => {
            a.iter().zip(b).zip(m).map(|((&x, &y), &k)| (((x as i32 - y as i32) * (x as i32 - y as i32)) as u64) * k as u64).sum()
        }
    }
}

/// The weighted sum of absolute Hadamard coefficients of a 4x4 block (libwebp's `TTransform`): a
/// measure of its texture energy.
fn texture(px: &[u8; 16]) -> i32 {
    const W: [i32; 16] = [38, 32, 20, 9, 32, 28, 17, 7, 20, 17, 10, 4, 9, 7, 4, 2];
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let r: [i32; 4] = std::array::from_fn(|k| px[i * 4 + k] as i32);
        let a0 = r[0] + r[2];
        let a1 = r[1] + r[3];
        let a2 = r[1] - r[3];
        let a3 = r[0] - r[2];
        tmp[i * 4] = a0 + a1;
        tmp[i * 4 + 1] = a3 + a2;
        tmp[i * 4 + 2] = a3 - a2;
        tmp[i * 4 + 3] = a0 - a1;
    }
    let mut sum = 0;
    for i in 0..4 {
        let a0 = tmp[i] + tmp[8 + i];
        let a1 = tmp[4 + i] + tmp[12 + i];
        let a2 = tmp[4 + i] - tmp[12 + i];
        let a3 = tmp[i] - tmp[8 + i];
        let b = [a0 + a1, a3 + a2, a3 - a2, a0 - a1];
        for k in 0..4 {
            sum += W[i + k * 4] * b[k].abs();
        }
    }
    sum
}

fn tdisto(a: &[u8; 16], b: &[u8; 16]) -> u64 {
    ((texture(a) - texture(b)).abs() >> 5) as u64
}

fn block4(buf: &[u8], stride: usize, bx: usize, by: usize) -> [u8; 16] {
    std::array::from_fn(|k| buf[(by * 4 + k / 4) * stride + bx * 4 + k % 4])
}

struct Enc<'a> {
    yuv: &'a Yuv,
    opts: Options,
    segs: Vec<Segment>,
    trees: Trees,
    costs: Costs,
    /// The segment of each macroblock.
    seg_map: Vec<u8>,
    /// The reconstruction (unfiltered).
    ry: Vec<u8>,
    ru: Vec<u8>,
    rv: Vec<u8>,
}

/// The chosen chroma mode with its levels, reconstruction and nonzero flags.
struct UvTry {
    score: u64,
    mode: u8,
    levels: [[i16; 16]; 8],
    ru: [u8; 64],
    rv: [u8; 64],
    top: Nz,
    left: Nz,
    any: bool,
}

/// The best 4x4 mode so far: score, mode, levels, reconstruction, a level is not zero.
type Best4 = (u64, u8, [i16; 16], [u8; 16], bool);

/// One candidate's result for the luma of a macroblock.
struct LumaTry {
    score: u64,
    mode: u8,
    bmodes: [u8; 16],
    levels: [[i16; 16]; 16],
    y2: [i16; 16],
    recon: [u8; 256],
    nz_bottom: [u8; 4],
    nz_right: [u8; 4],
    nz_dc: u8,
    any: bool,
}

impl<'a> Enc<'a> {
    fn mb_src(&self, mx: usize, my: usize) -> ([u8; 256], [u8; 64], [u8; 64], Option<[u8; 256]>) {
        let yuv = self.yuv;
        let (ys, cs) = (yuv.mbw * 16, yuv.mbw * 8);
        let y = std::array::from_fn(|k| yuv.y[(my * 16 + k / 16) * ys + mx * 16 + k % 16]);
        let u = std::array::from_fn(|k| yuv.u[(my * 8 + k / 8) * cs + mx * 8 + k % 8]);
        let v = std::array::from_fn(|k| yuv.v[(my * 8 + k / 8) * cs + mx * 8 + k % 8]);
        let m = yuv.mask.as_ref().map(|m| std::array::from_fn(|k| m[(my * 16 + k / 16) * ys + mx * 16 + k % 16]));
        (y, u, v, m)
    }

    /// The chroma mask of a macroblock (when the image has transparency).
    fn mb_cmask(&self, mx: usize, my: usize) -> Option<[u8; 64]> {
        let cs = self.yuv.mbw * 8;
        self.yuv.cmask.as_ref().map(|m| std::array::from_fn(|k| m[(my * 8 + k / 8) * cs + mx * 8 + k % 8]))
    }

    /// The best 16x16 luma prediction.
    #[allow(clippy::too_many_arguments)]
    fn try_i16(
        &self,
        seg: &Segment,
        ctx: &Ctx,
        src: &[u8; 256],
        mask: Option<&[u8; 256]>,
        top: &Nz,
        left: &Nz,
        mx: usize,
        my: usize,
    ) -> LumaTry {
        let mut best: Option<LumaTry> = None;
        for mode in [DC_PRED, V_PRED, H_PRED, TM_PRED] {
            if (mode == V_PRED && my == 0) || (mode == H_PRED && mx == 0) {
                // (allowed by the format, but the fixed border values predict nothing useful)
                continue;
            }
            let mut pred = [0u8; 256];
            dsp::pred_block(mode, &ctx.y, S, 16, my > 0, mx > 0, &mut pred);
            let mut coefs = [[0i16; 16]; 16];
            let mut dc = [0i16; 16];
            for b in 0..16 {
                let (bx, by) = (b % 4, b / 4);
                let s4 = block4(src, 16, bx, by);
                let p4 = block4(&pred, 16, bx, by);
                coefs[b] = dsp::fdct(&s4, &p4);
                dc[b] = coefs[b][0];
            }
            let wht = dsp::fwht(&dc);
            let mut y2 = [0i16; 16];
            let mut y2deq = [0i16; 16];
            let mut any_dc = seg.y2.quantize(&wht, 0, &mut y2, &mut y2deq);
            let dcs = dsp::iwht(&y2deq);
            if any_dc && dcs.iter().all(|&d| d == 0) {
                // decoders disagree on whether such a block is "coded"; it changes nothing, drop it
                y2 = [0; 16];
                any_dc = false;
            }
            let mut rate = self.trees.ymode_cost[mode as usize] + self.costs.block(&y2, 0, 1, (top.dc + left.dc) as usize);
            let mut levels = [[0i16; 16]; 16];
            let mut recon = [0u8; 256];
            let mut nz_top = top.y;
            let mut nz_left = left.y;
            let mut any = any_dc;
            let mut sd = 0u64;
            for b in 0..16 {
                let (bx, by) = (b % 4, b / 4);
                let mut deq = [0i16; 16];
                let nz = seg.y1.quantize(&coefs[b], 1, &mut levels[b], &mut deq);
                deq[0] = dcs[b];
                rate += self.costs.block(&levels[b], 1, 0, (nz_top[bx] + nz_left[by]) as usize);
                nz_top[bx] = nz as u8;
                nz_left[by] = nz as u8;
                any |= nz;
                let p4 = block4(&pred, 16, bx, by);
                let mut r4 = [0u8; 16];
                dsp::idct_add(&deq, &p4, &mut r4);
                if seg.tlambda > 0 {
                    sd += tdisto(&block4(src, 16, bx, by), &r4);
                }
                for k in 0..16 {
                    recon[(by * 4 + k / 4) * 16 + bx * 4 + k % 4] = r4[k];
                }
            }
            let d = sse(src, &recon, mask.map(|m| &m[..]));
            let score = 256 * (d + ((sd * seg.tlambda as u64) >> 8)) + seg.lambda * rate as u64;
            if best.as_ref().is_none_or(|b| score < b.score) {
                best = Some(LumaTry {
                    score,
                    mode,
                    bmodes: [implied_bmode(mode); 16],
                    levels,
                    y2,
                    recon,
                    nz_bottom: nz_top,
                    nz_right: nz_left,
                    nz_dc: any_dc as u8,
                    any,
                });
            }
        }
        best.expect("DC prediction is always tried")
    }

    /// The best sixteen 4x4 predictions; `None` when it cannot beat `bound`.
    #[allow(clippy::too_many_arguments)]
    fn try_i4(
        &self,
        seg: &Segment,
        ctx: &Ctx,
        src: &[u8; 256],
        mask: Option<&[u8; 256]>,
        top: &Nz,
        left: &Nz,
        top_modes: &EdgeModes,
        left_modes: &EdgeModes,
        bound: u64,
    ) -> Option<LumaTry> {
        let mut ctx = ctx.clone();
        let mut t = LumaTry {
            score: seg.lambda * self.trees.ymode_cost[B_PRED as usize] as u64,
            mode: B_PRED,
            bmodes: [0; 16],
            levels: [[0; 16]; 16],
            y2: [0; 16],
            recon: [0; 256],
            nz_bottom: top.y,
            nz_right: left.y,
            nz_dc: left.dc, // unchanged by a 4x4 macroblock (the caller keeps the edge's value)
            any: false,
        };
        for b in 0..16 {
            let (bx, by) = (b % 4, b / 4);
            let above = if by == 0 { top_modes[bx] } else { t.bmodes[b - 4] };
            let leftm = if bx == 0 { left_modes[by] } else { t.bmodes[b - 1] };
            let s4 = block4(src, 16, bx, by);
            let m4: Option<[u8; 16]> = mask.map(|m| block4(m, 16, bx, by));
            let ctxn = (t.nz_bottom[bx] + t.nz_right[by]) as usize;
            let mut best: Option<Best4> = None;
            // rank the modes by a quick estimate (the transformed residual's magnitude plus the
            // mode's bits), then decide among the best few by the real rate and distortion
            let mut cand: [(u64, u8, [u8; 16], [i16; 16]); 10] = [(0, 0, [0; 16], [0; 16]); 10];
            for mode in 0..10u8 {
                let mut p4 = [0u8; 16];
                dsp::pred4(mode, &ctx.y, b, &mut p4);
                let coef = dsp::fdct(&s4, &p4);
                let satd: u64 = coef.iter().map(|&c| c.unsigned_abs() as u64).sum();
                let bits = self.trees.bmode_cost[above as usize][leftm as usize][mode as usize] as u64;
                cand[mode as usize] = (satd * 256 + seg.sqrt_lambda * bits, mode, p4, coef);
            }
            let keep = self.opts.i4_candidates.clamp(1, 10);
            if keep < 10 {
                cand.sort_unstable_by_key(|c| c.0);
            }
            for &(_, mode, p4, coef) in &cand[..keep] {
                let mut lv = [0i16; 16];
                let mut deq = [0i16; 16];
                let nz = seg.y1.quantize(&coef, 0, &mut lv, &mut deq);
                let mut r4 = [0u8; 16];
                dsp::idct_add(&deq, &p4, &mut r4);
                let rate = self.trees.bmode_cost[above as usize][leftm as usize][mode as usize] + self.costs.block(&lv, 0, 3, ctxn);
                let d = sse(&s4, &r4, m4.as_ref().map(|m| &m[..]));
                let sd = if seg.tlambda > 0 { (tdisto(&s4, &r4) * seg.tlambda as u64) >> 8 } else { 0 };
                let score = 256 * (d + sd) + seg.lambda * rate as u64;
                if best.as_ref().is_none_or(|b| score < b.0) {
                    best = Some((score, mode, lv, r4, nz));
                }
            }
            let (score, mode, lv, r4, nz) = best.expect("ten modes were tried");
            t.score += score;
            if t.score >= bound {
                return None;
            }
            t.bmodes[b] = mode;
            t.levels[b] = lv;
            t.nz_bottom[bx] = nz as u8;
            t.nz_right[by] = nz as u8;
            t.any |= nz;
            ctx.put4(b, &r4);
            for k in 0..16 {
                t.recon[(by * 4 + k / 4) * 16 + bx * 4 + k % 4] = r4[k];
            }
        }
        Some(t)
    }

    /// The best chroma mode: (score, mode, levels of the 8 blocks, recon u, recon v, nz flags).
    #[allow(clippy::too_many_arguments)]
    fn try_uv(&self, seg: &Segment, ctx: &Ctx, su: &[u8; 64], sv: &[u8; 64], top: &Nz, left: &Nz, mx: usize, my: usize) -> UvTry {
        let mut best: Option<UvTry> = None;
        let cmask = self.mb_cmask(mx, my);
        for mode in [DC_PRED, V_PRED, H_PRED, TM_PRED] {
            if (mode == V_PRED && my == 0) || (mode == H_PRED && mx == 0) {
                continue;
            }
            let mut levels = [[0i16; 16]; 8];
            let mut rec = [[0u8; 64]; 2];
            let mut nz_top = Nz::default();
            let mut nz_left = Nz::default();
            let mut rate = self.trees.uv_cost[mode as usize];
            let mut any = false;
            let mut d = 0u64;
            for (pi, (buf, src)) in [(&ctx.u, su), (&ctx.v, sv)].into_iter().enumerate() {
                let mut pred = [0u8; 64];
                dsp::pred_block(mode, buf, CS, 8, my > 0, mx > 0, &mut pred);
                let (mut nt, mut nl) = if pi == 0 { (top.u, left.u) } else { (top.v, left.v) };
                for b in 0..4 {
                    let (bx, by) = (b % 2, b / 2);
                    let s4 = block4(src, 8, bx, by);
                    let p4 = block4(&pred, 8, bx, by);
                    let coef = dsp::fdct(&s4, &p4);
                    let mut deq = [0i16; 16];
                    let lv = &mut levels[pi * 4 + b];
                    let nz = seg.uv.quantize(&coef, 0, lv, &mut deq);
                    rate += self.costs.block(lv, 0, 2, (nt[bx] + nl[by]) as usize);
                    nt[bx] = nz as u8;
                    nl[by] = nz as u8;
                    any |= nz;
                    let mut r4 = [0u8; 16];
                    dsp::idct_add(&deq, &p4, &mut r4);
                    for k in 0..16 {
                        rec[pi][(by * 4 + k / 4) * 8 + bx * 4 + k % 4] = r4[k];
                    }
                }
                d += sse(src, &rec[pi], cmask.as_ref().map(|m| &m[..]));
                if pi == 0 {
                    nz_top.u = nt;
                    nz_left.u = nl;
                } else {
                    nz_top.v = nt;
                    nz_left.v = nl;
                }
            }
            let d = (d as f32 * self.opts.chroma_weight) as u64;
            let score = 256 * d + seg.lambda * rate as u64;
            if best.as_ref().is_none_or(|b| score < b.score) {
                best = Some(UvTry { score, mode, levels, ru: rec[0], rv: rec[1], top: nz_top, left: nz_left, any });
            }
        }
        best.expect("DC prediction is always tried")
    }

    /// Decides every macroblock; fills the reconstruction.
    fn encode_frame(&mut self) -> Vec<Mb> {
        let (mbw, mbh) = (self.yuv.mbw, self.yuv.mbh);
        let (ys, cs) = (mbw * 16, mbw * 8);
        let mut mbs = Vec::with_capacity(mbw * mbh);
        let mut top_nz = vec![Nz::default(); mbw];
        let mut top_modes = vec![[B_DC; 4]; mbw];
        let mut ctx = Ctx::new();
        for my in 0..mbh {
            let mut left_nz = Nz::default();
            let mut left_modes = [B_DC; 4];
            for mx in 0..mbw {
                let segment = self.seg_map[my * mbw + mx];
                let seg = &self.segs[segment as usize];
                ctx.load_edges(&self.ry, ys, &self.ru, &self.rv, cs, mx, my, mbw);
                let (sy, su, sv, mask) = self.mb_src(mx, my);
                let tn = top_nz[mx];
                let i16 = self.try_i16(seg, &ctx, &sy, mask.as_ref(), &tn, &left_nz, mx, my);
                let luma = match self.try_i4(seg, &ctx, &sy, mask.as_ref(), &tn, &left_nz, &top_modes[mx], &left_modes, i16.score) {
                    Some(i4) => i4,
                    None => i16,
                };
                let UvTry { mode: uvmode, levels: uvl, ru: rec_u, rv: rec_v, top: uvt, left: uvl_nz, any: uv_any, .. } =
                    self.try_uv(seg, &ctx, &su, &sv, &tn, &left_nz, mx, my);
                // commit
                let mut levels = Box::new([[0i16; 16]; 25]);
                levels[..16].copy_from_slice(&luma.levels);
                levels[16..24].copy_from_slice(&uvl);
                levels[24] = luma.y2;
                let skip = !luma.any && !uv_any;
                for r in 0..16 {
                    self.ry[(my * 16 + r) * ys + mx * 16..][..16].copy_from_slice(&luma.recon[r * 16..r * 16 + 16]);
                }
                for r in 0..8 {
                    self.ru[(my * 8 + r) * cs + mx * 8..][..8].copy_from_slice(&rec_u[r * 8..r * 8 + 8]);
                    self.rv[(my * 8 + r) * cs + mx * 8..][..8].copy_from_slice(&rec_v[r * 8..r * 8 + 8]);
                }
                // the contexts the next macroblocks see
                let is4 = luma.mode == B_PRED;
                let mut t = Nz { y: luma.nz_bottom, u: uvt.u, v: uvt.v, dc: if is4 { tn.dc } else { luma.nz_dc } };
                let mut l = Nz { y: luma.nz_right, u: uvl_nz.u, v: uvl_nz.v, dc: if is4 { left_nz.dc } else { luma.nz_dc } };
                if skip {
                    // a skipped macroblock resets the flags (and the Y2 flag unless it is 4x4)
                    t = Nz { dc: t.dc, ..Nz::default() };
                    l = Nz { dc: l.dc, ..Nz::default() };
                    if !is4 {
                        t.dc = 0;
                        l.dc = 0;
                    }
                }
                top_nz[mx] = t;
                left_nz = l;
                top_modes[mx] = [luma.bmodes[12], luma.bmodes[13], luma.bmodes[14], luma.bmodes[15]];
                left_modes = [luma.bmodes[3], luma.bmodes[7], luma.bmodes[11], luma.bmodes[15]];
                mbs.push(Mb { segment, ymode: luma.mode, bmodes: luma.bmodes, uvmode, levels, skip });
            }
        }
        mbs
    }
}

/// Walks the coded blocks of all macroblocks in coding order with their contexts.
fn walk_tokens(mbs: &[Mb], mbw: usize, mut f: impl FnMut(&[i16; 16], usize, usize, usize)) {
    let mut top = vec![Nz::default(); mbw];
    for row in mbs.chunks(mbw) {
        let mut left = Nz::default();
        for (mx, mb) in row.iter().enumerate() {
            let t = &mut top[mx];
            let is4 = mb.ymode == B_PRED;
            if mb.skip {
                t.y = [0; 4];
                t.u = [0; 2];
                t.v = [0; 2];
                left.y = [0; 4];
                left.u = [0; 2];
                left.v = [0; 2];
                if !is4 {
                    t.dc = 0;
                    left.dc = 0;
                }
                continue;
            }
            let (first, ty) = if is4 {
                (0, 3)
            } else {
                let nz = mb.levels[24].iter().any(|&l| l != 0) as u8;
                f(&mb.levels[24], 0, 1, (t.dc + left.dc) as usize);
                t.dc = nz;
                left.dc = nz;
                (1, 0)
            };
            for b in 0..16 {
                let (bx, by) = (b % 4, b / 4);
                f(&mb.levels[b], first, ty, (t.y[bx] + left.y[by]) as usize);
                let nz = mb.levels[b][first..].iter().any(|&l| l != 0) as u8;
                t.y[bx] = nz;
                left.y[by] = nz;
            }
            for b in 0..8 {
                let (bx, by) = (b % 2, (b % 4) / 2);
                let (tt, ll) = if b < 4 { (&mut t.u, &mut left.u) } else { (&mut t.v, &mut left.v) };
                f(&mb.levels[16 + b], 0, 2, (tt[bx] + ll[by]) as usize);
                let nz = mb.levels[16 + b].iter().any(|&l| l != 0) as u8;
                tt[bx] = nz;
                ll[by] = nz;
            }
        }
    }
}

/// The probabilities that code `stats` best, keeping a default where an update does not pay.
fn optimize_probs(stats: &Stats, bits: &[u16; 256]) -> Probs {
    let mut p = COEFF_PROBS;
    for t in 0..4 {
        for b in 0..8 {
            for c in 0..3 {
                for n in 0..11 {
                    let [n0, n1] = stats[t][b][c][n];
                    let total = n0 + n1;
                    if total == 0 {
                        continue;
                    }
                    let newp = ((n0 as u64 * 256 + total as u64 / 2) / total as u64).clamp(1, 255) as u8;
                    let old = COEFF_PROBS[t][b][c][n];
                    let up = COEFF_UPDATE_PROBS[t][b][c][n];
                    let cost_with = |p: u8| n0 as u64 * cost(bits, p, false) as u64 + n1 as u64 * cost(bits, p, true) as u64;
                    let keep = cost_with(old) + cost(bits, up, false) as u64;
                    let change = cost_with(newp) + cost(bits, up, true) as u64 + 8 * 256;
                    if change < keep {
                        p[t][b][c][n] = newp;
                    }
                }
            }
        }
    }
    p
}

/// Assigns each macroblock a segment by how busy it is, and sets each segment's quantizers.
fn segments(yuv: &Yuv, opts: &Options) -> (Vec<Segment>, Vec<u8>) {
    let (mbw, mbh) = (yuv.mbw, yuv.mbh);
    let ys = mbw * 16;
    // busyness: the log of the AC energy of the macroblock's 4x4 blocks
    let mut act = Vec::with_capacity(mbw * mbh);
    for my in 0..mbh {
        for mx in 0..mbw {
            let mut e = 0f64;
            for b in 0..16 {
                let (bx, by) = (b % 4, b / 4);
                let s4: [u8; 16] = std::array::from_fn(|k| yuv.y[(my * 16 + by * 4 + k / 4) * ys + mx * 16 + bx * 4 + k % 4]);
                let c = dsp::fdct(&s4, &[0; 16]);
                for (k, &v) in c.iter().enumerate().skip(1) {
                    let _ = k;
                    e += (v as f64).abs();
                }
            }
            act.push((1.0 + e / 16.0).ln());
        }
    }
    let n = opts.segments.clamp(1, 4).min(act.len());
    let (lo, hi) = act.iter().fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    let mut centers: Vec<f64> = (0..n).map(|i| lo + (hi - lo) * (i as f64 + 0.5) / n as f64).collect();
    let mut map = vec![0u8; act.len()];
    for _ in 0..8 {
        for (i, &a) in act.iter().enumerate() {
            map[i] = (0..n).min_by(|&x, &y| (a - centers[x]).abs().total_cmp(&(a - centers[y]).abs())).unwrap_or(0) as u8;
        }
        for (s, c) in centers.iter_mut().enumerate() {
            let (sum, cnt) =
                act.iter().zip(&map).filter(|(_, &m)| m as usize == s).fold((0.0, 0usize), |(a, k), (&x, _)| (a + x, k + 1));
            if cnt > 0 {
                *c = sum / cnt as f64;
            }
        }
    }
    let mean = act.iter().sum::<f64>() / act.len().max(1) as f64;
    let range = (hi - lo).max(1e-6);
    let amp = 0.9 * opts.sns as f64 / 100.0;
    let segs = centers
        .iter()
        .map(|&c| {
            // -1 (smooth) .. 1 (busy) around the image's mean
            let alpha = ((c - mean) / range * 2.0).clamp(-1.0, 1.0);
            let expn = 1.0 + amp * alpha * 1.5;
            let q = quality_to_q(opts.quality, expn).clamp(0, 127);
            make_segment(q, opts)
        })
        .collect();
    (segs, map)
}

fn make_segment(q: i32, opts: &Options) -> Segment {
    let y1 = QMat::new(DC_Q[clip_q(q)] as u32, AC_Q[clip_q(q)] as u32, opts.bias_dc, opts.bias_ac, true);
    let y2 =
        QMat::new((DC_Q[clip_q(q)] * 2) as u32, ((AC_Q[clip_q(q)] as u32 * 101581) >> 16).max(8), opts.bias_dc, opts.bias_ac, false);
    let uv = QMat::new(DC_Q[clip_q(q + UV_DC_DELTA).min(117)] as u32, AC_Q[clip_q(q)] as u32, opts.bias_dc, opts.bias_ac, false);
    let qac = AC_Q[clip_q(q)] as f64;
    let lambda = (opts.lambda as f64 * qac * qac).max(1.0) as u64;
    let tlambda = (opts.texture as f64 * qac) as u32;
    let sqrt_lambda = ((lambda as f64).sqrt() * 16.0) as u64;
    Segment { q, y1, y2, uv, lambda, sqrt_lambda, tlambda, filter: 0 }
}

/// The chroma DC quantizer is a little finer than luma's (libwebp's default does the same).
const UV_DC_DELTA: i32 = -2;

/// The SSE of the luma of `rec` against the source, macroblock by macroblock.
fn mb_sse(a: &[u8], b: &[u8], ys: usize, mbw: usize, mbh: usize, mask: Option<&[u8]>) -> Vec<u64> {
    let mut out = vec![0u64; mbw * mbh];
    for r in 0..mbh * 16 {
        let row = r * ys;
        for mx in 0..mbw {
            let at = row + mx * 16;
            let (x, y) = (&a[at..at + 16], &b[at..at + 16]);
            let s: u32 = match mask {
                None => x.iter().zip(y).map(|(&p, &q)| ((p as i32 - q as i32) * (p as i32 - q as i32)) as u32).sum(),
                Some(m) => x
                    .iter()
                    .zip(y)
                    .zip(&m[at..at + 16])
                    .map(|((&p, &q), &k)| ((p as i32 - q as i32) * (p as i32 - q as i32)) as u32 * k as u32)
                    .sum(),
            };
            out[(r / 16) * mbw + mx] += s as u64;
        }
    }
    out
}

/// Encodes the image as a VP8 key frame; returns the `VP8 ` chunk payload.
pub fn encode(yuv: &Yuv, opts: &Options) -> Vec<u8> {
    let (mbw, mbh) = (yuv.mbw, yuv.mbh);
    let ys = mbw * 16;
    let (segs, seg_map) = segments(yuv, opts);
    let trees = Trees::new();
    let costs = Costs::new(&COEFF_PROBS, &trees.bits);
    let mut enc = Enc {
        yuv,
        opts: *opts,
        segs,
        trees,
        costs,
        seg_map,
        ry: vec![0; yuv.y.len()],
        ru: vec![0; yuv.u.len()],
        rv: vec![0; yuv.v.len()],
    };
    let mut mbs = enc.encode_frame();
    let mut stats: Box<Stats> = Box::new([[[[[0; 2]; 11]; 3]; 8]; 4]);
    walk_tokens(&mbs, mbw, |lv, first, ty, ctx| count_block(&mut stats, lv, first, ty, ctx));
    let mut probs = optimize_probs(&stats, &enc.trees.bits);
    for _ in 1..opts.passes {
        enc.costs = Costs::new(&probs, &enc.trees.bits);
        mbs = enc.encode_frame();
        *stats = [[[[[0; 2]; 11]; 3]; 8]; 4];
        walk_tokens(&mbs, mbw, |lv, first, ty, ctx| count_block(&mut stats, lv, first, ty, ctx));
        probs = optimize_probs(&stats, &enc.trees.bits);
    }

    // the loop filter: per segment, the level that brings the reconstruction closest to the source
    let nseg = enc.segs.len();
    let mut levels = vec![0i32; nseg];
    let sharpness = 0u8;
    let inner: Vec<bool> = mbs.iter().map(|m| m.ymode == B_PRED || !m.skip).collect();
    if opts.search_filter {
        // each pass filters the luma with one level per segment and measures each segment's error
        // (filtering is local, so the segments barely interact)
        let try_levels = |lv: &[i32]| -> Vec<u64> {
            let params: Vec<FilterParams> =
                inner.iter().zip(&enc.seg_map).map(|(&i, &s)| FilterParams::new(lv[s as usize], sharpness, i)).collect();
            let mut fy = enc.ry.clone();
            dsp::filter_frame(&mut fy, ys, None, mbw, mbh, &params, false);
            let e = mb_sse(&fy, &yuv.y, ys, mbw, mbh, yuv.mask.as_deref());
            let mut per = vec![0u64; nseg];
            for (x, &s) in e.iter().zip(&enc.seg_map) {
                per[s as usize] += x;
            }
            per
        };
        let mut best: Vec<(u64, i32)> = vec![(u64::MAX, 0); nseg];
        let consider = |lv: &[i32], best: &mut Vec<(u64, i32)>| {
            let per = try_levels(lv);
            for s in 0..nseg {
                if per[s] < best[s].0 {
                    best[s] = (per[s], lv[s]);
                }
            }
        };
        for l in [0, 12, 24, 36, 50, 63] {
            consider(&vec![l; nseg], &mut best);
        }
        for step in [6, 3] {
            let centers: Vec<i32> = best.iter().map(|b| b.1).collect();
            for d in [-step, step] {
                let lv: Vec<i32> = centers.iter().map(|&c| (c + d).clamp(0, 63)).collect();
                consider(&lv, &mut best);
            }
        }
        for (s, lv) in levels.iter_mut().enumerate() {
            *lv = best[s].1;
        }
    } else {
        for (s, lv) in levels.iter_mut().enumerate() {
            let q = AC_Q[clip_q(enc.segs[s].q)] as i32 >> 2;
            *lv = (q * 6 / 5).clamp(0, 63);
        }
    }
    for (s, seg) in enc.segs.iter_mut().enumerate() {
        seg.filter = levels[s];
    }

    write_frame(yuv, &enc.segs, &enc.seg_map, &mbs, &probs, sharpness, &enc.trees)
}

#[allow(clippy::too_many_arguments)]
fn write_frame(yuv: &Yuv, segs: &[Segment], seg_map: &[u8], mbs: &[Mb], probs: &Probs, sharpness: u8, trees: &Trees) -> Vec<u8> {
    let mbw = yuv.mbw;
    let mut bw = BoolWriter::new();
    bw.flag(false); // color space
    bw.flag(false); // clamping required
    let use_segments = segs.len() > 1;
    // segment tree probabilities from the counts
    let mut seg_probs = [255u8; 3];
    if use_segments {
        let mut cnt = [0u64; 4];
        for &s in seg_map {
            cnt[s as usize] += 1;
        }
        let pr = |a: u64, b: u64| (a * 256 + (a + b) / 2).checked_div(a + b).map_or(255, |p| p.clamp(1, 255) as u8);
        seg_probs = [pr(cnt[0] + cnt[1], cnt[2] + cnt[3]), pr(cnt[0], cnt[1]), pr(cnt[2], cnt[3])];
    }
    if bw.flag(use_segments) {
        bw.flag(true); // update the map
        bw.flag(true); // update the data
        bw.flag(true); // absolute values
        for s in 0..4 {
            bw.signed(segs.get(s).map_or(0, |g| g.q), 7);
        }
        for s in 0..4 {
            bw.signed(segs.get(s).map_or(0, |g| g.filter), 6);
        }
        for p in seg_probs {
            if bw.flag(p != 255) {
                bw.literal(p as u32, 8);
            }
        }
    }
    let level = if use_segments { segs.iter().map(|s| s.filter).max().unwrap_or(0) } else { segs[0].filter };
    bw.flag(false); // the normal loop filter
    bw.literal(level as u32, 6);
    bw.literal(sharpness as u32, 3);
    bw.flag(false); // no loop filter deltas
    bw.literal(0, 2); // one token partition
    bw.literal(segs[0].q as u32, 7);
    bw.signed(0, 4); // y1 dc
    bw.signed(0, 4); // y2 dc
    bw.signed(0, 4); // y2 ac
    bw.signed(UV_DC_DELTA, 4); // uv dc
    bw.signed(0, 4); // uv ac
    bw.flag(false); // refresh entropy probabilities
    for t in 0..4 {
        for b in 0..8 {
            for c in 0..3 {
                for n in 0..11 {
                    let p = probs[t][b][c][n];
                    if bw.put_ret(p != COEFF_PROBS[t][b][c][n], COEFF_UPDATE_PROBS[t][b][c][n]) {
                        bw.literal(p as u32, 8);
                    }
                }
            }
        }
    }
    // the skip flag
    let skips = mbs.iter().filter(|m| m.skip).count() as u64;
    let total = mbs.len() as u64;
    let skip_prob = (((total - skips) * 256 + total / 2) / total.max(1)).clamp(1, 255) as u8;
    let use_skip = skips > 0;
    bw.flag(use_skip);
    if use_skip {
        bw.literal(skip_prob as u32, 8);
    }
    // the modes
    let mut top_modes = vec![[B_DC; 4]; mbw];
    for row in mbs.chunks(mbw) {
        let mut left_modes = [B_DC; 4];
        for (mx, mb) in row.iter().enumerate() {
            if use_segments {
                let s = mb.segment;
                bw.put(s >= 2, seg_probs[0]);
                if s < 2 {
                    bw.put(s == 1, seg_probs[1]);
                } else {
                    bw.put(s == 3, seg_probs[2]);
                }
            }
            if use_skip {
                bw.put(mb.skip, skip_prob);
            }
            put_tree(&mut bw, &trees.ymode, &KF_YMODE_PROBS, mb.ymode);
            if mb.ymode == B_PRED {
                for b in 0..16 {
                    let (bx, by) = (b % 4, b / 4);
                    let above = if by == 0 { top_modes[mx][bx] } else { mb.bmodes[b - 4] };
                    let left = if bx == 0 { left_modes[by] } else { mb.bmodes[b - 1] };
                    put_tree(&mut bw, &trees.bmode, &BMODE_PROBS[above as usize][left as usize], mb.bmodes[b]);
                }
            }
            put_tree(&mut bw, &trees.uvmode, &KF_UV_MODE_PROBS, mb.uvmode);
            top_modes[mx] = [mb.bmodes[12], mb.bmodes[13], mb.bmodes[14], mb.bmodes[15]];
            left_modes = [mb.bmodes[3], mb.bmodes[7], mb.bmodes[11], mb.bmodes[15]];
        }
    }
    let part0 = bw.finish();
    let mut tw = BoolWriter::new();
    walk_tokens(mbs, mbw, |lv, first, ty, ctx| put_block(&mut tw, probs, lv, first, ty, ctx));
    let part1 = tw.finish();

    let mut out = Vec::with_capacity(10 + part0.len() + part1.len());
    // bit 0: a key frame (0), bits 1-3: version 0, bit 4: shown, then the first partition's size
    let tag = (1u32 << 4) | ((part0.len() as u32) << 5);
    out.extend_from_slice(&tag.to_le_bytes()[..3]);
    out.extend_from_slice(&[0x9d, 0x01, 0x2a]);
    out.extend_from_slice(&(yuv.w as u16).to_le_bytes());
    out.extend_from_slice(&(yuv.h as u16).to_le_bytes());
    out.extend_from_slice(&part0);
    out.extend_from_slice(&part1);
    out
}

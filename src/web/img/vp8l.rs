//! A WebP lossless (VP8L) encoder, zero crates. [`encode`] makes the payload of a `VP8L` chunk and
//! [`encode_alpha`] the compressed data of an `ALPH` chunk with compression method 1.
//!
//! [`encode_alpha_filtered`] also tries the ALPH prefilters and says which one it used.
//!
//! What it does (on a mixed set of photos, screenshots and graphics the files come out about 2%
//! smaller than libwebp's lossless `-m 4`, about as small as `-m 6`):
//! - An entropy analysis (like libwebp's) ranks the ways to code the image: color indexing when
//!   it has at most 256 colors (always, with at most 16), with or without subtract green, with
//!   or without the predictor transform. Photo-like images take the best; on images where many
//!   pixels repeat a neighbour (screenshots, graphics) the best of each kind, plus a predictor
//!   used for every tile (left or top), is prepared and judged by a quick greedy LZ77 estimate,
//!   because the analysis does not see LZ77. The winner is encoded, sometimes with a close
//!   second, and on small images both subtract-green choices; the smallest stream is kept.
//! - Color indexing sorts the palette, delta-codes it and bundles 2, 4 or 8 pixels per byte when
//!   there are at most 16 colors.
//! - The predictor transform picks one of the 14 predictors per 4x4 tile (8x8 above 4 megapixels)
//!   by the cost of its residuals under the statistics so far; the cross-color transform then
//!   picks green-to-red, green-to-blue and red-to-blue per 16x16 tile (32x32) the same way.
//! - LZ77: libwebp's hash chain gives the longest match per pixel (the pixel above and the pixel
//!   to the left tried first; fewer candidates on large images); an optimal parse by dynamic
//!   programming then picks literals, color cache hits and copies (the chain's match and the runs
//!   at distance 1 and at the pixel above) by their cost in bits, with the 2D distance codes for
//!   short distances.
//! - The color cache size (0..=10 bits) is chosen by an entropy estimate for the greedy parse and
//!   for literals only; when they differ much, both are parsed and the cheaper kept.
//! - Meta prefix codes: tile histograms are clustered (entropy bins, random then greedy merges,
//!   tiles moved to their cheapest cluster), the image is parsed again with each tile's costs
//!   (up to a megapixel), and the result is kept when smaller than one group.
//! - Prefix codes are canonical Huffman codes of at most 15 bits, stored as simple codes (one or two
//!   symbols) or with the run-length coded code-length code; trailing zeros are trimmed.
//!
//! Fully transparent pixels (alpha 0) of [`encode`] may come back with other RGB values (libwebp's
//! `exact = false`): they are set to whatever the predictor makes cheapest. Every other pixel is
//! kept exactly.

use super::vp8l_dec::{
    cache_slot, color_delta, green_alphabet, predict, reverse_bits, sub_pixels, subsample, CODE_LENGTH_ORDER, DIST_MAP, MAX_CL_LENGTH,
    MAX_CODE_LENGTH, NUM_DIST_CODES, NUM_LENGTH_CODES, NUM_LITERALS,
};
use std::collections::HashMap;

/// The longest copy (libwebp's limit; the format allows 4096).
const MAX_LENGTH: usize = 4095;
/// The farthest copy: distance code 40 with all extra bits set, minus the 120 plane codes.
const WINDOW: usize = (1 << 20) - 120;
const HASH_BITS: u32 = 18;
/// The largest color cache the encoder tries.
const MAX_CACHE_BITS: u32 = 10;
/// Hash chain candidates per pixel for the main image and for sub-images.
const CHAIN_ITERS: u32 = 50;
const SUB_CHAIN_ITERS: u32 = 16;

/// Encodes `w` x `h` pixels (`0xAARRGGBB`, rows top to bottom) as a VP8L chunk payload: the 0x2f
/// signature, the 14-bit width and height minus one, the `alpha_is_used` bit (set when some
/// pixel is not opaque), version 0 and the image stream. Fully transparent pixels may change
/// their RGB (see the module docs).
///
/// Panics if `w` or `h` is 0 or above 16384, or `argb.len() != w * h`.
pub fn encode(w: usize, h: usize, argb: &[u32]) -> Vec<u8> {
    check_dims(w, h, argb.len());
    let has_alpha = argb.iter().any(|&p| p < 0xff00_0000);
    let mut px = argb.to_vec();
    if has_alpha {
        for p in px.iter_mut().filter(|p| **p >> 24 == 0) {
            *p = 0;
        }
    }
    let v = (w as u32 - 1) | ((h as u32 - 1) << 14) | ((has_alpha as u32) << 28);
    let mut out = vec![0x2f];
    out.extend_from_slice(&v.to_le_bytes());
    out.extend(encode_stream(w, h, &px, has_alpha));
    out
}

/// Encodes an alpha plane (`w * h` values, rows top to bottom) as the compressed data of an `ALPH`
/// chunk with compression method 1: a VP8L image stream without the 5-byte header, the alpha
/// values in the green channel. The caller writes the ALPH header byte: compression 1, filtering
/// method 0, pre-processing 0 (header byte 1). [`encode_alpha_filtered`] is often smaller.
///
/// Panics if `w` or `h` is 0 or above 16384, or `alpha.len() != w * h`.
#[cfg(test)]
pub fn encode_alpha(w: usize, h: usize, alpha: &[u8]) -> Vec<u8> {
    check_dims(w, h, alpha.len());
    let px: Vec<u32> = alpha.iter().map(|&a| 0xff00_0000 | (a as u32) << 8).collect();
    encode_stream(w, h, &px, false)
}

/// Like [`encode_alpha`], but may first apply an ALPH filter (1 horizontal, 2 vertical,
/// 3 gradient): the filter whose output LZ77 codes best (by a quick estimate) is tried against no
/// filter, and the smaller stream is kept. Returns the filtering method for bits 2-3 of the ALPH
/// header byte (0 = none) and the compressed data. On smooth alpha (a blurred shape) a filter
/// often halves the size, because the filtered rows repeat each other.
pub fn encode_alpha_filtered(w: usize, h: usize, alpha: &[u8]) -> (u8, Vec<u8>) {
    check_dims(w, h, alpha.len());
    let plane = |a: &[u8]| -> Vec<u32> { a.iter().map(|&v| 0xff00_0000 | (v as u32) << 8).collect() };
    let (method, filtered) = (1..=3)
        .map(|m| {
            let f = alpha_filter(w, h, alpha, m);
            (quick_estimate(&plane(&f), w, h), m, f)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, m, f)| (m, f))
        .unwrap_or_default();
    let plain = encode_stream(w, h, &plane(alpha), false);
    let data = encode_stream(w, h, &plane(&filtered), false);
    if data.len() < plain.len() {
        (method, data)
    } else {
        (0, plain)
    }
}

/// `alpha` with ALPH filter `method` (1 horizontal, 2 vertical, 3 gradient) applied: each value
/// minus its prediction, modulo 256 (the WebP container spec's filtering).
fn alpha_filter(w: usize, h: usize, a: &[u8], method: u8) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let pred = match (x, y) {
                (0, 0) => 0,
                (_, 0) => a[i - 1],
                (0, _) => a[i - w],
                _ => match method {
                    1 => a[i - 1],
                    2 => a[i - w],
                    _ => (a[i - 1] as i32 + a[i - w] as i32 - a[i - w - 1] as i32).clamp(0, 255) as u8,
                },
            };
            out[i] = a[i].wrapping_sub(pred);
        }
    }
    out
}

fn check_dims(w: usize, h: usize, len: usize) {
    assert!((1..=16384).contains(&w) && (1..=16384).contains(&h), "a VP8L image is 1 to 16384 pixels wide and high");
    assert_eq!(len, w * h, "the pixel count does not match the size");
}

/// Writes bits least significant first.
struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter { out: Vec::new(), acc: 0, n: 0 }
    }

    /// Writes the low `bits` bits of `v` (`bits` <= 32, the other bits of `v` zero).
    #[inline]
    fn put(&mut self, v: u32, bits: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += bits;
        if self.n >= 32 {
            self.out.extend_from_slice(&(self.acc as u32).to_le_bytes());
            self.acc >>= 32;
            self.n -= 32;
        }
    }

    fn bits(&self) -> usize {
        self.out.len() * 8 + self.n as usize
    }

    /// Writes all the bits of `other`.
    fn append(&mut self, other: BitWriter) {
        if self.n == 0 {
            self.out.extend_from_slice(&other.out);
        } else {
            for &b in &other.out {
                self.put(b as u32, 8);
            }
        }
        self.put(other.acc as u32, other.n);
    }

    fn finish(mut self) -> Vec<u8> {
        while self.n > 0 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n = self.n.saturating_sub(8);
        }
        self.out
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    /// Color indexing.
    Palette,
    /// No predictor, optionally subtract green.
    Direct { green: bool },
    /// Predictor and cross-color, optionally subtract green; the predictor chosen per tile, or
    /// one for all tiles (which keeps rows that repeat each other repeating, for LZ77).
    Spatial { green: bool, uniform: Option<u32> },
}

/// The image stream for `px`: tries the most promising ways and keeps the smallest.
fn encode_stream(w: usize, h: usize, px: &[u32], free_rgb: bool) -> Vec<u8> {
    let palette = find_palette(px);
    let (modes, repeats) = analyze(px, w, h, palette.as_deref());
    // The analysis does not see LZ77 (a screenshot often codes best without the predictor), so
    // when many pixels repeat a neighbour, the best mode of each kind is prepared and judged by a
    // quick LZ77 estimate.
    let mut picks: Vec<Mode> = Vec::new();
    for &(mode, _) in &modes {
        if !picks.iter().any(|p| std::mem::discriminant(p) == std::mem::discriminant(&mode)) {
            picks.push(mode);
        }
    }
    if repeats < 0.25 && w * h > 1 << 16 {
        picks.truncate(1);
    } else if let Some(&Mode::Spatial { green, .. }) =
        picks.iter().find(|m| matches!(m, Mode::Spatial { .. })).filter(|_| w * h <= 1 << 20)
    {
        // Smooth graphics up to a megapixel: the left or the top predictor everywhere.
        picks.push(Mode::Spatial { green, uniform: Some(1) });
        picks.push(Mode::Spatial { green, uniform: Some(2) });
    }
    let single = picks.len() == 1;
    let mut cands: Vec<(f64, Prepared)> = picks
        .into_iter()
        .map(|mode| {
            let p = prepare(w, h, px, mode, palette.as_deref(), free_rgb);
            let est = if single { 0.0 } else { p.bw.bits() as f64 + quick_estimate(&p.img, p.w, h) };
            (est, p)
        })
        .collect();
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    // A close second is encoded too on smaller images.
    let tries = if cands.len() > 1 && w * h <= 1 << 20 && cands[1].0 < cands[0].0 * 1.05 { 2 } else { 1 };
    // A uniform predictor is a long shot: when it wins the estimate, the best other way is
    // encoded too.
    let uniform = |p: &Prepared| matches!(p.mode, Mode::Spatial { uniform: Some(_), .. });
    let other = if uniform(&cands[0].1) { cands.iter().position(|c| !uniform(&c.1)).filter(|&i| i >= tries) } else { None };
    let mut full: Vec<Prepared> = Vec::new();
    for (i, c) in cands.into_iter().enumerate() {
        if i < tries || Some(i) == other {
            full.push(c.1);
        }
    }
    // On small graphics the analysis is not sharp enough to decide subtract green: try both.
    if w * h <= 1 << 16 || (w * h <= 1 << 19 && repeats >= 0.25) {
        let flip = match full[0].mode {
            Mode::Direct { green } => Some(Mode::Direct { green: !green }),
            Mode::Spatial { green, uniform } => Some(Mode::Spatial { green: !green, uniform }),
            Mode::Palette => None,
        };
        if let Some(mode) = flip.filter(|m| full.iter().all(|p| p.mode != *m)) {
            full.push(prepare(w, h, px, mode, palette.as_deref(), free_rgb));
        }
    }
    let mut best: Option<Vec<u8>> = None;
    for p in full {
        let mut bw = p.bw;
        bw.put(0, 1);
        write_image_data(&mut bw, p.w, h, &p.img, true);
        let out = bw.finish();
        if best.as_ref().is_none_or(|b| out.len() < b.len()) {
            best = Some(out);
        }
    }
    best.unwrap_or_default()
}

/// A way to code the image: its transforms written, and the image left to code (`w` wide).
struct Prepared {
    mode: Mode,
    bw: BitWriter,
    img: Vec<u32>,
    w: usize,
}

/// Estimated bits of the image data of `img` (`w` x `h`): a greedy parse with a short hash
/// chain, on bands of rows adding up to about a quarter megapixel.
fn quick_estimate(img: &[u32], w: usize, h: usize) -> f64 {
    const BAND: usize = 32;
    let rows = ((1usize << 18) / w).max(1);
    let sample: Vec<u32>;
    let (px, sh) = if h <= rows {
        (img, h)
    } else {
        let bands = (rows / BAND).max(1);
        let stride = h / bands;
        let mut s = Vec::with_capacity(bands * BAND * w);
        for b in 0..bands {
            let y0 = b * stride;
            let y1 = (y0 + BAND).min(h);
            s.extend_from_slice(&img[y0 * w..y1 * w]);
        }
        sample = s;
        (&sample[..], sample.len() / w)
    };
    let chain = hash_chain(px, w, 8, WINDOW.min(w << 6));
    let mut refs = greedy_refs(px, &chain, &DistCodes::new(w));
    let bits = best_cache_bits(px, &refs).0;
    apply_cache(px, &mut refs, bits);
    refs_cost(&refs, bits) * h as f64 / sh as f64
}

/// Writes the transforms of `mode` and applies them.
fn prepare(w: usize, h: usize, px: &[u32], mode: Mode, palette: Option<&[u32]>, free_rgb: bool) -> Prepared {
    let mut bw = BitWriter::new();
    let mut img: Vec<u32>;
    let mut iw = w;
    match (mode, palette) {
        (Mode::Palette, Some(pal)) => {
            bw.put(1, 1);
            bw.put(3, 2);
            bw.put(pal.len() as u32 - 1, 8);
            let delta: Vec<u32> = (0..pal.len()).map(|i| if i == 0 { pal[0] } else { sub_pixels(pal[i], pal[i - 1]) }).collect();
            write_image_data(&mut bw, pal.len(), 1, &delta, false);
            (img, iw) = index_image(px, w, h, pal);
        }
        (Mode::Direct { green }, _) | (Mode::Spatial { green, .. }, _) => {
            img = px.to_vec();
            if green {
                bw.put(1, 1);
                bw.put(2, 2);
                for p in img.iter_mut() {
                    let g = (*p >> 8) & 0xff;
                    *p = sub_pixels(*p, (g << 16) | g);
                }
            }
            if let Mode::Spatial { uniform, .. } = mode {
                // Small predictor tiles pay for their sub-image; larger ones on large images
                // save time; one predictor for all needs only the largest.
                let large = w * h > 1 << 22;
                let bits = if uniform.is_some() {
                    9
                } else if large {
                    3
                } else {
                    2
                };
                let modes = predictor_transform(&mut img, w, h, bits, free_rgb, uniform);
                bw.put(1, 1);
                bw.put(0, 2);
                bw.put(bits - 2, 3);
                write_image_data(&mut bw, subsample(w, bits), subsample(h, bits), &modes, false);
                if img.iter().any(|&p| p & 0x00ff_00ff != 0) {
                    let bits = if large { 5 } else { 4 };
                    let cc = cross_color_transform(&mut img, w, h, bits);
                    bw.put(1, 1);
                    bw.put(1, 2);
                    bw.put(bits - 2, 3);
                    write_image_data(&mut bw, subsample(w, bits), subsample(h, bits), &cc, false);
                }
            }
        }
        (Mode::Palette, None) => unreachable!("palette mode without a palette"),
    }
    Prepared { mode, bw, img, w: iw }
}

// ---------------------------------------------------------------------------------------------
// Analysis and the palette

/// The distinct colors of `px`, sorted, if there are at most 256.
fn find_palette(px: &[u32]) -> Option<Vec<u32>> {
    let mut seen = std::collections::HashSet::new();
    let mut last = !px[0];
    for &p in px {
        if p != last {
            last = p;
            if seen.insert(p) && seen.len() > 256 {
                return None;
            }
        }
    }
    let mut pal: Vec<u32> = seen.into_iter().collect();
    pal.sort_unstable();
    Some(pal)
}

/// Bits per index: 2 colors pack 8 pixels per byte, 4 colors 4, 16 colors 2.
fn palette_width_bits(n: usize) -> u32 {
    match n {
        0..=2 => 3,
        3..=4 => 2,
        5..=16 => 1,
        _ => 0,
    }
}

/// The color-indexed image (bundled when the palette is small) and its width.
fn index_image(px: &[u32], w: usize, h: usize, pal: &[u32]) -> (Vec<u32>, usize) {
    let map: HashMap<u32, u32> = pal.iter().enumerate().map(|(i, &c)| (c, i as u32)).collect();
    let wb = palette_width_bits(pal.len());
    let pw = subsample(w, wb);
    let bpp = 8 >> wb;
    let mut out = vec![0xff00_0000u32; pw * h];
    let (mut last, mut last_idx) = (pal[0], 0);
    for y in 0..h {
        for x in 0..w {
            let p = px[y * w + x];
            if p != last {
                last = p;
                last_idx = map[&p];
            }
            out[y * pw + (x >> wb)] |= last_idx << (8 + (x & ((1 << wb) - 1)) as u32 * bpp);
        }
    }
    (out, pw)
}

/// Shannon entropy in bits of a histogram.
fn entropy(h: &[u32]) -> f64 {
    let total: u64 = h.iter().map(|&c| c as u64).sum();
    if total == 0 {
        return 0.0;
    }
    let t = total as f64;
    h.iter().filter(|&&c| c > 0).map(|&c| c as f64 * (t / c as f64).log2()).sum()
}

/// Ranks the ways to code `px` by an entropy estimate, best first (like libwebp's analysis): the
/// pixel values and their differences to the left neighbour, per channel, with and without
/// subtract green, and the palette indices.
fn analyze(px: &[u32], w: usize, h: usize, palette: Option<&[u32]>) -> (Vec<(Mode, f64)>, f64) {
    let mut hs = vec![[0u32; 256]; 13];
    let map: HashMap<u32, u32> = palette.unwrap_or(&[]).iter().enumerate().map(|(i, &c)| (c, i as u32)).collect();
    let mut repeats = 0;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let p = px[i];
            if (x > 0 && p == px[i - 1]) || (y > 0 && p == px[i - w]) {
                repeats += 1;
                continue;
            }
            let left = if x > 0 {
                px[i - 1]
            } else if y > 0 {
                px[i - w]
            } else {
                0xff00_0000
            };
            let d = sub_pixels(p, left);
            for (k, v) in [p, d].into_iter().enumerate() {
                let (a, r, g, b) = (v >> 24, (v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff);
                hs[k][a as usize] += 1;
                hs[2 + k][g as usize] += 1;
                hs[4 + k][r as usize] += 1;
                hs[6 + k][b as usize] += 1;
                hs[8 + k][(r.wrapping_sub(g) & 0xff) as usize] += 1;
                hs[10 + k][(b.wrapping_sub(g) & 0xff) as usize] += 1;
            }
            if let Some(&idx) = map.get(&p) {
                hs[12][idx as usize] += 1;
            }
        }
    }
    let e: Vec<f64> = hs.iter().map(|h| entropy(h)).collect();
    // The cost of the predictor and cross-color sub-images, roughly.
    let tiles = (subsample(w, 4) * subsample(h, 4)) as f64;
    let mut out = vec![
        (Mode::Direct { green: false }, e[0] + e[2] + e[4] + e[6]),
        (Mode::Spatial { green: false, uniform: None }, e[1] + e[3] + e[5] + e[7] + tiles * 3.8),
        (Mode::Direct { green: true }, e[0] + e[2] + e[8] + e[10]),
        (Mode::Spatial { green: true, uniform: None }, e[1] + e[3] + e[9] + e[11] + tiles * 3.8),
    ];
    let repeats = repeats as f64 / (w * h) as f64;
    if let Some(pal) = palette {
        if pal.len() <= 16 {
            // Bundled indices beat everything else on images this simple.
            return (vec![(Mode::Palette, 0.0)], repeats);
        }
        out.push((Mode::Palette, e[12] + pal.len() as f64 * 8.0));
    }
    out.sort_by(|a, b| a.1.total_cmp(&b.1));
    (out, repeats)
}

// ---------------------------------------------------------------------------------------------
// The predictor and cross-color transforms

/// A cost in bits per value of each channel (alpha, red, green, blue), from histograms of the
/// values chosen so far plus a prior that favours values near 0.
fn cost_tables(hist: &[[u32; 256]], out: &mut [[u16; 256]]) {
    const PRIOR: f64 = 24.0;
    let prior: Vec<f64> = (0..256).map(|v| 0.6f64.powi((v as i8 as i32).abs())).collect();
    let z: f64 = prior.iter().sum();
    for (h, o) in hist.iter().zip(out.iter_mut()) {
        let total: f64 = h.iter().map(|&c| c as f64).sum::<f64>() + PRIOR;
        for v in 0..256 {
            o[v] = ((total / (h[v] as f64 + PRIOR * prior[v] / z)).log2() * COST_SCALE).round().min(65535.0) as u16;
        }
    }
}

/// Fixed-point scale of the transforms' integer costs (1/16 bit).
const COST_SCALE: f64 = 16.0;

/// A pixel and its neighbours (pixel, left, top, top-left, top-right), and how many pixels of a
/// tile have exactly these.
type Context = ([u32; 5], u32);

/// The cost of the residuals of predictor `M` for these pixels.
fn mode_cost<const M: u32>(ctx: &[Context], costs: &[[u16; 256]; 4]) -> u32 {
    let mut c = 0u32;
    for &([p, l, t, tl, tr], n) in ctx {
        let r = sub_pixels(p, predict(M, l, t, tl, tr));
        c += n
            * (costs[0][(r >> 24) as usize] as u32
                + costs[1][((r >> 16) & 0xff) as usize] as u32
                + costs[2][((r >> 8) & 0xff) as usize] as u32
                + costs[3][(r & 0xff) as usize] as u32);
    }
    c
}

/// The contexts of the pixels in rows `y0..y1` (every `ystep`-th) and columns `x0..x1` (none on
/// the first row or column), runs of equal contexts merged.
#[allow(clippy::too_many_arguments)]
fn tile_contexts(px: &[u32], w: usize, x0: usize, x1: usize, y0: usize, y1: usize, ystep: usize, out: &mut Vec<Context>) {
    out.clear();
    for y in (y0..y1).step_by(ystep) {
        let (up, cur) = px[(y - 1) * w..(y + 1) * w].split_at(w);
        for x in x0..x1 {
            // The top-right pixel of the last column is the first of the current row.
            let tr = if x + 1 < w { up[x + 1] } else { cur[0] };
            let c = [cur[x], cur[x - 1], up[x], up[x - 1], tr];
            match out.last_mut() {
                Some((last, n)) if *last == c => *n += 1,
                _ => out.push((c, 1)),
            }
        }
    }
}

type ModeCost = fn(&[Context], &[[u16; 256]; 4]) -> u32;
const MODE_COSTS: [ModeCost; 14] = [
    mode_cost::<0>,
    mode_cost::<1>,
    mode_cost::<2>,
    mode_cost::<3>,
    mode_cost::<4>,
    mode_cost::<5>,
    mode_cost::<6>,
    mode_cost::<7>,
    mode_cost::<8>,
    mode_cost::<9>,
    mode_cost::<10>,
    mode_cost::<11>,
    mode_cost::<12>,
    mode_cost::<13>,
];

/// Applies the predictor transform with tiles of `2^bits` pixels: chooses a predictor per tile and
/// replaces `px` by the residuals. Returns the sub-image of predictors. With `free_rgb`, fully
/// transparent pixels get the RGB of their prediction. `uniform` uses that predictor everywhere.
fn predictor_transform(px: &mut Vec<u32>, w: usize, h: usize, bits: u32, free_rgb: bool, uniform: Option<u32>) -> Vec<u32> {
    let (tw, th) = (subsample(w, bits), subsample(h, bits));
    let mut modes = vec![uniform.unwrap_or(0); tw * th];
    let mut hist = [[0u32; 256]; 4];
    let mut costs = [[0u16; 256]; 4];
    cost_tables(&hist, &mut costs);
    // Large tiles and large images are judged by every other row.
    let ystep = if bits >= 5 || w * h > 1 << 22 { 2 } else { 1 };
    let mut ctx = Vec::new();
    for ty in 0..if uniform.is_some() { 0 } else { th } {
        for tx in 0..tw {
            let (x0, x1) = ((tx << bits).max(1), ((tx + 1) << bits).min(w));
            let (y0, y1) = ((ty << bits).max(1), ((ty + 1) << bits).min(h));
            if x0 >= x1 || y0 >= y1 {
                // On the first row or column the predictor is fixed.
                modes[ty * tw + tx] = if tx > 0 { modes[ty * tw + tx - 1] } else { 0 };
                continue;
            }
            tile_contexts(px, w, x0, x1, y0, y1, ystep, &mut ctx);
            // Ties go to the left tile's predictor, which keeps the predictor image compact.
            let left = if tx > 0 { modes[ty * tw + tx - 1] } else { u32::MAX };
            let mut best = (u32::MAX, 0);
            for (mode, f) in MODE_COSTS.iter().enumerate() {
                let c = f(&ctx, &costs);
                if c < best.0 || (c == best.0 && mode as u32 == left) {
                    best = (c, mode as u32);
                }
            }
            let mode = best.1;
            modes[ty * tw + tx] = mode;
            for y in y0..y1 {
                let row = y * w;
                for i in row + x0..row + x1 {
                    let r = sub_pixels(px[i], predict(mode, px[i - 1], px[i - w], px[i - w - 1], px[i - w + 1]));
                    for (c, s) in [24, 16, 8, 0].into_iter().enumerate() {
                        hist[c][((r >> s) & 0xff) as usize] += 1;
                    }
                }
            }
        }
        cost_tables(&hist, &mut costs);
    }
    let mut res = vec![0u32; w * h];
    for y in 0..h {
        let mrow = &modes[(y >> bits) * tw..];
        for x in 0..w {
            let i = y * w + x;
            let pred = if y == 0 {
                if x == 0 {
                    0xff00_0000
                } else {
                    px[i - 1]
                }
            } else if x == 0 {
                px[i - w]
            } else {
                predict(mrow[x >> bits], px[i - 1], px[i - w], px[i - w - 1], px[i - w + 1])
            };
            if free_rgb && px[i] >> 24 == 0 {
                px[i] = pred & 0x00ff_ffff;
            }
            res[i] = sub_pixels(px[i], pred);
        }
    }
    *px = res;
    modes.iter().map(|&m| 0xff00_0000 | m << 8).collect()
}

/// Applies the cross-color transform with tiles of `2^bits` pixels; returns its sub-image.
fn cross_color_transform(px: &mut [u32], w: usize, h: usize, bits: u32) -> Vec<u32> {
    let (tw, th) = (subsample(w, bits), subsample(h, bits));
    let mut out = vec![0xff00_0000u32; tw * th];
    let mut hist = [[0u32; 256]; 2];
    let mut costs = [[0u16; 256]; 2];
    // The tile's (green, red, blue) values, sorted and counted.
    let mut keys: Vec<u32> = Vec::new();
    let mut uniq: Vec<(u8, u8, u8, u32)> = Vec::new();
    // Large tiles are judged by every other pixel of every other row.
    let step = if bits >= 5 || (w * h > 1 << 21 && bits >= 4) { 2 } else { 1 };
    let scale = (step * step) as i64;
    for ty in 0..th {
        cost_tables(&hist, &mut costs);
        for tx in 0..tw {
            let (x0, x1) = (tx << bits, ((tx + 1) << bits).min(w));
            let (y0, y1) = (ty << bits, ((ty + 1) << bits).min(h));
            keys.clear();
            for y in (y0..y1).step_by(step) {
                keys.extend(px[y * w + x0..y * w + x1].iter().step_by(step).map(|&p| p & 0x00ff_ffff));
            }
            keys.sort_unstable();
            uniq.clear();
            for &k in &keys {
                match uniq.last_mut() {
                    Some(u) if (u.0, u.1, u.2) == ((k >> 8) as u8, (k >> 16) as u8, k as u8) => u.3 += 1,
                    _ => uniq.push(((k >> 8) as u8, (k >> 16) as u8, k as u8, 1)),
                }
            }
            let left = if tx > 0 { out[ty * tw + tx - 1] } else { 0 };
            let up = if ty > 0 { out[(ty - 1) * tw + tx] } else { 0 };
            // Green to red; a bonus of 3 bits for the value of a neighbour tile or 0.
            const BONUS: i64 = 3 * COST_SCALE as i64;
            let cost_r = |g2r: i32| -> i64 {
                let t = g2r as u8;
                let sum: u32 =
                    uniq.iter().map(|&(g, r, _, n)| n * costs[0][(r as i32 - color_delta(t, g)) as u8 as usize] as u32).sum();
                let mut c = sum as i64 * scale;
                for n in [left, up] {
                    if n as u8 == t {
                        c -= BONUS;
                    }
                }
                if t == 0 {
                    c -= BONUS;
                }
                c
            };
            let mut g2r = 0;
            let mut best = cost_r(0);
            for delta in [32, 16, 8, 4, 2, 1] {
                for cand in [g2r - delta, g2r + delta] {
                    if (-128..=127).contains(&cand) {
                        let c = cost_r(cand);
                        if c < best {
                            best = c;
                            g2r = cand;
                        }
                    }
                }
            }
            // Green to blue and red to blue.
            let cost_b = |g2b: i32, r2b: i32| -> i64 {
                let (tg, tr) = (g2b as u8, r2b as u8);
                let sum: u32 = uniq
                    .iter()
                    .map(|&(g, r, b, n)| n * costs[1][(b as i32 - color_delta(tg, g) - color_delta(tr, r)) as u8 as usize] as u32)
                    .sum();
                let mut c = sum as i64 * scale;
                for n in [left, up] {
                    if (n >> 8) as u8 == tg && (n >> 16) as u8 == tr {
                        c -= BONUS;
                    }
                }
                if tg == 0 && tr == 0 {
                    c -= BONUS;
                }
                c
            };
            let (mut g2b, mut r2b) = (0, 0);
            let mut best = cost_b(0, 0);
            for delta in [16, 16, 8, 4, 2, 2, 2] {
                let (bg, br) = (g2b, r2b);
                for (dg, dr) in [(0, -1), (0, 1), (-1, 0), (1, 0), (-1, -1), (-1, 1), (1, -1), (1, 1)] {
                    let (cg, cr) = (bg + dg * delta, br + dr * delta);
                    if (-128..=127).contains(&cg) && (-128..=127).contains(&cr) {
                        let c = cost_b(cg, cr);
                        if c < best {
                            best = c;
                            (g2b, r2b) = (cg, cr);
                        }
                    }
                }
            }
            let (t_gr, t_gb, t_rb) = (g2r as u8, g2b as u8, r2b as u8);
            out[ty * tw + tx] = 0xff00_0000 | (t_rb as u32) << 16 | (t_gb as u32) << 8 | t_gr as u32;
            for y in y0..y1 {
                for p in &mut px[y * w + x0..y * w + x1] {
                    let (g, r, b) = ((*p >> 8) as u8, (*p >> 16) as u8, *p as u8);
                    let nr = (r as i32 - color_delta(t_gr, g)) as u8;
                    let nb = (b as i32 - color_delta(t_gb, g) - color_delta(t_rb, r)) as u8;
                    hist[0][nr as usize] += 1;
                    hist[1][nb as usize] += 1;
                    *p = (*p & 0xff00_ff00) | (nr as u32) << 16 | nb as u32;
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// LZ77

/// One coded symbol: a literal pixel, a color cache hit, or a copy (`code` is the distance code).
#[derive(Clone, Copy, Debug)]
enum Ref {
    Lit(u32),
    Cache(u32),
    Copy { len: u32, code: u32 },
}

impl Ref {
    fn len(&self) -> usize {
        match *self {
            Ref::Copy { len, .. } => len as usize,
            _ => 1,
        }
    }
}

/// The distance codes of an image `w` pixels wide.
struct DistCodes {
    w: usize,
    /// `plane[dy * 16 + 8 - dx]` = the code of (dx, dy), or 0.
    plane: [u8; 128],
}

impl DistCodes {
    fn new(w: usize) -> Self {
        let mut plane = [0u8; 128];
        for (i, &(dx, dy)) in DIST_MAP.iter().enumerate() {
            plane[(dy as i32 * 16 + 8 - dx as i32) as usize] = i as u8 + 1;
        }
        DistCodes { w, plane }
    }

    /// The distance code of a copy `d` pixels back (libwebp's mapping).
    fn code(&self, d: usize) -> u32 {
        let (yo, xo) = (d / self.w, d % self.w);
        if xo <= 8 && yo < 8 {
            let c = self.plane[yo * 16 + 8 - xo];
            if c != 0 {
                return c as u32;
            }
        } else if xo + 8 > self.w && yo < 7 {
            let c = self.plane[(yo + 1) * 16 + 8 + (self.w - xo)];
            if c != 0 {
                return c as u32;
            }
        }
        (d + 120) as u32
    }
}

/// The prefix symbol, extra bit count and extra bits of a length or distance code `v` (>= 1).
#[inline]
fn prefix(v: u32) -> (usize, u32, u32) {
    let d = v - 1;
    if d < 4 {
        return (d as usize, 0, 0);
    }
    let hb = 31 - d.leading_zeros();
    let nbits = hb - 1;
    ((2 * hb + ((d >> nbits) & 1)) as usize, nbits, d & ((1 << nbits) - 1))
}

#[inline]
fn pair_hash(a: u32, b: u32, bits: u32) -> usize {
    ((b.wrapping_mul(0xc6a4_a793) ^ a.wrapping_mul(0x5bd1_e996)) >> (32 - bits)) as usize
}

#[inline]
fn match_len(px: &[u32], a: usize, b: usize, max: usize) -> usize {
    px[a..a + max].iter().zip(&px[b..b + max]).take_while(|(x, y)| x == y).count()
}

/// The longest match found for every pixel, as `distance << 12 | length` (0 = none); libwebp's
/// hash chain, filled from the end so a match extends to the pixels before it for free.
fn hash_chain(px: &[u32], w: usize, iters: u32, window: usize) -> Vec<u32> {
    let n = px.len();
    let mut out = vec![0u32; n];
    if n < 3 {
        return out;
    }
    let mut chain = vec![-1i32; n];
    // A hash table about as large as the image (small sub-images need no big one).
    let hb = (usize::BITS - n.leading_zeros()).clamp(8, HASH_BITS);
    let mut head = vec![-1i32; 1 << hb];
    let mut same = px[0] == px[1];
    let mut pos = 0;
    while pos < n - 2 {
        let same_next = px[pos + 1] == px[pos + 2];
        if same && same_next {
            // A run: hash the color with the run length left, so equal runs link up.
            let mut len = 1;
            while pos + len + 2 < n && px[pos + len + 2] == px[pos] {
                len += 1;
            }
            if len > MAX_LENGTH {
                pos += len - MAX_LENGTH;
                len = MAX_LENGTH;
            }
            while len > 0 {
                let hh = pair_hash(px[pos], len as u32, hb);
                chain[pos] = head[hh];
                head[hh] = pos as i32;
                pos += 1;
                len -= 1;
            }
            same = false;
        } else {
            let hh = pair_hash(px[pos], px[pos + 1], hb);
            chain[pos] = head[hh];
            head[hh] = pos as i32;
            pos += 1;
            same = same_next;
        }
    }
    chain[pos] = head[pair_hash(px[pos], px[pos + 1], hb)];
    drop(head);

    let mut base = n - 2;
    while base > 0 {
        let max_len = (n - 1 - base).min(MAX_LENGTH);
        let mut iter = iters as i32;
        let (mut best_len, mut best_dist) = (0, 0);
        let min_pos = base.saturating_sub(window) as i32;
        let length_max = max_len.min(256);
        let mut pos = chain[base];
        if base >= w {
            let l = match_len(px, base - w, base, max_len);
            if l > best_len {
                (best_len, best_dist) = (l, w);
            }
            iter -= 1;
        }
        let l = match_len(px, base - 1, base, max_len);
        if l > best_len {
            (best_len, best_dist) = (l, 1);
        }
        iter -= 1;
        if best_len == MAX_LENGTH {
            pos = -1;
        }
        let mut best_px = px[base + best_len];
        while pos >= min_pos && pos >= 0 {
            iter -= 1;
            if iter <= 0 {
                break;
            }
            let p = pos as usize;
            pos = chain[p];
            if px[p + best_len] != best_px {
                continue;
            }
            let l = match_len(px, p, base, max_len);
            if l > best_len {
                (best_len, best_dist) = (l, base - p);
                best_px = px[base + best_len];
                if best_len >= length_max {
                    break;
                }
            }
        }
        // Extend the match to the pixels before while they match too.
        let mut max_base = base;
        loop {
            out[base] = ((best_dist as u32) << 12) | best_len as u32;
            base -= 1;
            if best_dist == 0 || base == 0 || base < best_dist || px[base - best_dist] != px[base] {
                break;
            }
            if best_len == MAX_LENGTH && best_dist != 1 && base + MAX_LENGTH < max_base {
                break;
            }
            if best_len < MAX_LENGTH {
                best_len += 1;
                max_base = base;
            }
        }
    }
    out
}

/// Hash chain candidates per pixel and the farthest distance searched: less on large images.
fn chain_effort(w: usize, h: usize, top: bool) -> (u32, usize) {
    let n = w * h;
    if !top {
        (SUB_CHAIN_ITERS, WINDOW)
    } else if n <= 1 << 20 {
        (CHAIN_ITERS, WINDOW)
    } else if n <= 1 << 22 {
        (CHAIN_ITERS / 2, WINDOW.min(w << 8))
    } else {
        (CHAIN_ITERS / 10, WINDOW.min(w << 6))
    }
}

/// A first, greedy parse (libwebp's: copies of 4 or more pixels, shortened when the match at a
/// later pixel reaches farther).
fn greedy_refs(px: &[u32], chain: &[u32], dc: &DistCodes) -> Vec<Ref> {
    const MIN_LENGTH: usize = 4;
    let n = px.len();
    let mut refs = Vec::new();
    let mut i = 0;
    let mut last_check = 0;
    while i < n {
        let (dist, len0) = ((chain[i] >> 12) as usize, (chain[i] & 0xfff) as usize);
        let mut len = 1;
        if len0 >= MIN_LENGTH {
            len = len0;
            let mut max_reach = 0;
            let j_max = (i + len0).min(n - 1);
            last_check = last_check.max(i);
            for (j, &cj) in chain.iter().enumerate().take(j_max + 1).skip(last_check + 1) {
                let lj = (cj & 0xfff) as usize;
                let reach = j + if lj >= MIN_LENGTH { lj } else { 1 };
                if reach > max_reach {
                    len = j - i;
                    max_reach = reach;
                    if max_reach >= n {
                        break;
                    }
                }
            }
            last_check = last_check.max(j_max);
        }
        if len == 1 {
            refs.push(Ref::Lit(px[i]));
        } else {
            refs.push(Ref::Copy { len: len as u32, code: dc.code(dist) });
        }
        i += len;
    }
    refs
}

/// Turns literals that are in the color cache into cache hits.
fn apply_cache(px: &[u32], refs: &mut [Ref], bits: u32) {
    if bits == 0 {
        return;
    }
    let mut cache = vec![0u32; 1 << bits];
    let mut i = 0;
    for r in refs.iter_mut() {
        if let Ref::Lit(p) = *r {
            let s = cache_slot(p, bits);
            if cache[s] == p {
                *r = Ref::Cache(s as u32);
            }
        }
        for &p in &px[i..i + r.len()] {
            cache[cache_slot(p, bits)] = p;
        }
        i += r.len();
    }
}

/// The color cache sizes (0..=10 bits) whose literals have the smallest entropy: for the parse
/// `refs`, and for every pixel a literal, in one pass.
fn best_cache_bits(px: &[u32], refs: &[Ref]) -> (u32, u32) {
    // No cache larger than the image.
    let max = MAX_CACHE_BITS.min(usize::BITS - px.len().leading_zeros()) as usize;
    let mut caches: Vec<Vec<u32>> = (1..=max).map(|b| vec![0u32; 1 << b]).collect();
    // Per parse and cache size: green (with cache symbols), red, blue, alpha.
    let new_hists = || -> Vec<[Vec<u32>; 4]> {
        (0..=max as u32).map(|b| [vec![0u32; green_alphabet(b)], vec![0u32; 256], vec![0u32; 256], vec![0u32; 256]]).collect()
    };
    let mut hists = [new_hists(), new_hists()];
    let mut lengths = vec![0u32; NUM_LENGTH_CODES];
    let sample = (px.len() >> 18).max(1);
    let mut refs = refs.iter();
    let mut next = 0;
    for (i, &p) in px.iter().enumerate() {
        let mut literal = false;
        if i == next {
            match refs.next() {
                Some(&Ref::Copy { len, .. }) => {
                    if (i >> 16) % sample == 0 {
                        lengths[prefix(len).0] += 1;
                    }
                    next += len as usize;
                }
                _ => {
                    literal = true;
                    next += 1;
                }
            }
        }
        // Large images are judged by every n-th chunk of 64K pixels (the chunk before each warms
        // the caches up).
        let chunk = i >> 16;
        if chunk % sample != 0 && (chunk + 1) % sample != 0 {
            continue;
        }
        let key = p.wrapping_mul(0x1e35_a7bd);
        if chunk % sample == 0 {
            for b in 0..=max {
                let hit = b > 0 && caches[b - 1][(key >> (32 - b)) as usize] == p;
                for (k, h) in hists.iter_mut().enumerate() {
                    if k == 0 && !literal {
                        continue;
                    }
                    let h = &mut h[b];
                    if hit {
                        h[0][NUM_LITERALS + NUM_LENGTH_CODES + (key >> (32 - b)) as usize] += 1;
                    } else {
                        h[0][((p >> 8) & 0xff) as usize] += 1;
                        h[1][((p >> 16) & 0xff) as usize] += 1;
                        h[2][(p & 0xff) as usize] += 1;
                        h[3][(p >> 24) as usize] += 1;
                    }
                }
            }
        }
        for b in 1..=max {
            caches[b - 1][(key >> (32 - b)) as usize] = p;
        }
    }
    let mut best = [(f64::MAX, 0); 2];
    for (k, hs) in hists.iter_mut().enumerate() {
        for (b, h) in hs.iter_mut().enumerate() {
            if k == 0 {
                h[0][NUM_LITERALS..NUM_LITERALS + NUM_LENGTH_CODES].copy_from_slice(&lengths);
            }
            let cost: f64 = h.iter().map(|x| entropy(x) + header_estimate(x)).sum();
            if cost < best[k].0 {
                best[k] = (cost, b as u32);
            }
        }
    }
    (best[0].1, best[1].1)
}

/// Estimated bits of coding `refs` with one group of prefix codes.
fn refs_cost(refs: &[Ref], cache_bits: u32) -> f64 {
    let extra: u64 = refs
        .iter()
        .map(|r| match *r {
            Ref::Copy { len, code } => (prefix(len).1 + prefix(code).1) as u64,
            _ => 0,
        })
        .sum();
    histo_cost(&Histo::of(refs, cache_bits)) + extra as f64
}

/// A rough size in bits of the prefix code for histogram `h`.
fn header_estimate(h: &[u32]) -> f64 {
    let used = h.iter().filter(|&&c| c > 0).count();
    if used <= 2 {
        return 12.0;
    }
    let mut runs = 0;
    let mut prev_zero = false;
    for &c in h {
        if c == 0 && !prev_zero {
            runs += 1;
        }
        prev_zero = c == 0;
    }
    40.0 + used as f64 * 3.0 + runs as f64 * 4.0
}

/// Costs in bits of every symbol, from the histogram of a previous parse.
struct CostModel {
    green: Vec<f32>,
    red: Vec<f32>,
    blue: Vec<f32>,
    alpha: Vec<f32>,
    dist: Vec<f32>,
}

fn symbol_costs(counts: &[u32]) -> Vec<f32> {
    let total: u64 = counts.iter().map(|&c| c as u64).sum();
    let used = counts.iter().filter(|&&c| c > 0).count();
    if used <= 1 {
        return vec![0.0; counts.len()];
    }
    let lt = (total as f64).log2();
    counts.iter().map(|&c| if c == 0 { lt as f32 + 1.0 } else { (lt - (c as f64).log2()) as f32 }).collect()
}

impl CostModel {
    fn new(h: &Histo) -> Self {
        CostModel {
            green: symbol_costs(h.green()),
            red: symbol_costs(h.red()),
            blue: symbol_costs(h.blue()),
            alpha: symbol_costs(h.alpha()),
            dist: symbol_costs(h.dist()),
        }
    }

    #[inline]
    fn literal(&self, p: u32) -> f32 {
        self.green[((p >> 8) & 0xff) as usize]
            + self.red[((p >> 16) & 0xff) as usize]
            + self.blue[(p & 0xff) as usize]
            + self.alpha[(p >> 24) as usize]
    }
}

/// The parse of least cost under `model`: dynamic programming over the pixels, with literals,
/// cache hits and copies of the longest match found at each pixel (every length up to 32, then
/// the longest of each length prefix code).
fn optimal_refs(px: &[u32], chain: &[u32], dc: &DistCodes, cache_bits: u32, models: &[CostModel], tiles: Option<&Tiles>) -> Vec<Ref> {
    const RING: usize = 4096;
    let n = px.len();
    let len_costs: Vec<Vec<f32>> = models
        .iter()
        .map(|m| {
            (0..=MAX_LENGTH)
                .map(|k| {
                    let (s, nb, _) = prefix(k.max(1) as u32);
                    m.green[NUM_LITERALS + s] + nb as f32
                })
                .collect()
        })
        .collect();
    let (mut x, mut y) = (0, 0);
    let ends: Vec<usize> = (0..NUM_LENGTH_CODES as u32)
        .map(|p| if p < 4 { p as usize + 1 } else { ((2 + (p & 1) as usize) << ((p - 2) >> 1)) + (1 << ((p - 2) >> 1)) })
        .filter(|&e| e <= MAX_LENGTH)
        .collect();
    let mut cost = vec![f64::INFINITY; RING];
    cost[0] = 0.0;
    let mut step = vec![0u16; n + 1];
    let mut cache = vec![0u32; if cache_bits > 0 { 1 << cache_bits } else { 0 }];
    let cache_base = NUM_LITERALS + NUM_LENGTH_CODES;
    let mut runs = [0usize; 2];
    let mut use_cache = vec![false; if cache_bits > 0 { n } else { 0 }];
    // The distance and length of each candidate at the previous pixel, and its cost.
    let mut prev = [(0usize, 0usize); 3];
    let mut prev_c = f64::INFINITY;
    for i in 0..n {
        let c = cost[i % RING];
        cost[i % RING] = f64::INFINITY;
        let m = tiles.map_or(0, |t| t.group(x, y));
        (x, y) = if x + 1 == dc.w { (0, y + 1) } else { (x + 1, y) };
        let (model, len_cost) = (&models[m], &len_costs[m]);
        let p = px[i];
        let mut lit = model.literal(p);
        if cache_bits > 0 {
            let s = cache_slot(p, cache_bits);
            if cache[s] == p && model.green[cache_base + s] <= lit {
                lit = model.green[cache_base + s];
                use_cache[i] = true;
            }
            cache[s] = p;
        }
        let j = (i + 1) % RING;
        if c + (lit as f64) < cost[j] {
            cost[j] = c + lit as f64;
            step[i + 1] = 0;
        }
        // Candidates: the hash chain's match, and runs at distance 1 and at the pixel above.
        let (dist, len) = ((chain[i] >> 12) as usize, (chain[i] & 0xfff) as usize);
        for (kind, d) in [(0, dist), (1, 1), (2, dc.w)] {
            let len = if kind == 0 {
                len
            } else {
                let run = &mut runs[kind - 1];
                if *run > 1 {
                    *run -= 1;
                } else if i >= d && px[i - d] == px[i] {
                    *run = match_len(px, i - d, i, (n - i).min(MAX_LENGTH));
                } else {
                    *run = 0;
                }
                *run
            };
            // A match that continues the previous pixel's, from a pixel no cheaper to reach,
            // reaches next to nothing the previous pixel's did not (a bit at length code edges,
            // a pixel when the length is capped).
            let cont = d == prev[kind].0 && len <= prev[kind].1;
            prev[kind] = (d, len);
            if len == 0 || (cont && c >= prev_c) || (kind > 0 && (d == dist || i < d)) {
                continue;
            }
            let (ds, nb, _) = prefix(dc.code(d));
            let base = c + (model.dist[ds] + nb as f32) as f64;
            let mut relax = |k: usize| {
                let v = base + len_cost[k] as f64;
                let j = (i + k) % RING;
                if v < cost[j] {
                    cost[j] = v;
                    step[i + k] = (kind << 12 | k) as u16;
                }
            };
            // Every short length; for a long match (the pixels after it start long matches of
            // their own) only the longest length of each prefix code.
            let short = if len > 64 { 0 } else { len.min(32) };
            for k in 1..=short {
                relax(k);
            }
            let lo = if len > 64 { 32 } else { short };
            for &e in ends.iter().filter(|&&e| e > lo).take_while(|&&e| e < len) {
                relax(e);
            }
            if len > short {
                relax(len);
            }
        }
        prev_c = c;
    }
    let mut out = Vec::new();
    let mut j = n;
    while j > 0 {
        let (kind, k) = ((step[j] >> 12) as usize, (step[j] & 0xfff) as usize);
        if k == 0 {
            out.push(Ref::Lit(px[j - 1]));
            j -= 1;
        } else {
            let d = [(chain[j - k] >> 12) as usize, 1, dc.w][kind];
            out.push(Ref::Copy { len: k as u32, code: dc.code(d) });
            j -= k;
        }
    }
    out.reverse();
    if cache_bits > 0 {
        // Use a cache hit where the cost model preferred it.
        let mut i = 0;
        for r in out.iter_mut() {
            if let Ref::Lit(p) = *r {
                if use_cache[i] {
                    *r = Ref::Cache(cache_slot(p, cache_bits) as u32);
                }
            }
            i += r.len();
        }
    }
    out
}

/// The tiles of meta prefix codes: `2^bits` pixels square, `tw` per row, a group each.
struct Tiles {
    bits: u32,
    tw: usize,
    group: Vec<u32>,
}

impl Tiles {
    #[inline]
    fn group(&self, x: usize, y: usize) -> usize {
        self.group[(y >> self.bits) * self.tw + (x >> self.bits)] as usize
    }
}

// ---------------------------------------------------------------------------------------------
// Histograms, prefix codes and writing

/// The five histograms of a prefix-code group in one array: green (literals, lengths, cache),
/// red, blue, alpha, distance.
#[derive(Clone)]
struct Histo {
    h: Vec<u32>,
    ga: usize,
}

impl Histo {
    fn new(cache_bits: u32) -> Self {
        let ga = green_alphabet(cache_bits);
        Histo { h: vec![0; ga + 3 * 256 + NUM_DIST_CODES], ga }
    }

    fn green(&self) -> &[u32] {
        &self.h[..self.ga]
    }
    fn red(&self) -> &[u32] {
        &self.h[self.ga..self.ga + 256]
    }
    fn blue(&self) -> &[u32] {
        &self.h[self.ga + 256..self.ga + 512]
    }
    fn alpha(&self) -> &[u32] {
        &self.h[self.ga + 512..self.ga + 768]
    }
    fn dist(&self) -> &[u32] {
        &self.h[self.ga + 768..]
    }
    fn parts(&self) -> [&[u32]; 5] {
        [self.green(), self.red(), self.blue(), self.alpha(), self.dist()]
    }

    #[inline]
    fn add(&mut self, r: &Ref) {
        let ga = self.ga;
        match *r {
            Ref::Lit(p) => {
                self.h[((p >> 8) & 0xff) as usize] += 1;
                self.h[ga + ((p >> 16) & 0xff) as usize] += 1;
                self.h[ga + 256 + (p & 0xff) as usize] += 1;
                self.h[ga + 512 + (p >> 24) as usize] += 1;
            }
            Ref::Cache(s) => self.h[NUM_LITERALS + NUM_LENGTH_CODES + s as usize] += 1,
            Ref::Copy { len, code } => {
                self.h[NUM_LITERALS + prefix(len).0] += 1;
                self.h[ga + 768 + prefix(code).0] += 1;
            }
        }
    }

    fn of(refs: &[Ref], cache_bits: u32) -> Self {
        let mut h = Histo::new(cache_bits);
        for r in refs {
            h.add(r);
        }
        h
    }
}

/// Code lengths of a Huffman code for `counts`, at most `limit` bits (libwebp's way: if the tree
/// is too deep, small counts are raised and it is built again). One used symbol gets length 1.
fn huffman_lengths(counts: &[u32], limit: u32) -> Vec<u8> {
    let mut lens = vec![0u8; counts.len()];
    let syms: Vec<usize> = (0..counts.len()).filter(|&i| counts[i] > 0).collect();
    match syms.len() {
        0 => return lens,
        1 => {
            lens[syms[0]] = 1;
            return lens;
        }
        _ => {}
    }
    let mut floor = 1u64;
    loop {
        let mut leaves: Vec<(u64, usize)> = syms.iter().map(|&s| ((counts[s] as u64).max(floor), s)).collect();
        leaves.sort_unstable();
        let m = leaves.len();
        let mut weight: Vec<u64> = leaves.iter().map(|l| l.0).collect();
        let mut parent = vec![0usize; 2 * m - 1];
        let (mut a, mut b) = (0, m);
        for k in m..2 * m - 1 {
            let mut pick = [0usize; 2];
            for slot in &mut pick {
                if a < m && (b >= k || weight[a] <= weight[b]) {
                    *slot = a;
                    a += 1;
                } else {
                    *slot = b;
                    b += 1;
                }
            }
            weight.push(weight[pick[0]] + weight[pick[1]]);
            parent[pick[0]] = k;
            parent[pick[1]] = k;
        }
        let mut depth = vec![0u32; 2 * m - 1];
        for k in (0..2 * m - 2).rev() {
            depth[k] = depth[parent[k]] + 1;
        }
        if depth[..m].iter().all(|&d| d <= limit) {
            for (i, &(_, s)) in leaves.iter().enumerate() {
                lens[s] = depth[i] as u8;
            }
            return lens;
        }
        floor *= 2;
    }
}

/// A prefix code ready for writing: the bits per symbol (0 when the code has a single symbol,
/// which takes no bits) and the bit-reversed canonical codes.
struct Code {
    len: Vec<u8>,
    code: Vec<u16>,
}

impl Code {
    fn new(lens: &[u8]) -> Self {
        let used = lens.iter().filter(|&&l| l > 0).count();
        if used <= 1 {
            return Code { len: vec![0; lens.len()], code: vec![0; lens.len()] };
        }
        let mut count = [0u32; MAX_CODE_LENGTH + 1];
        for &l in lens {
            count[l as usize] += 1;
        }
        let mut next = [0u32; MAX_CODE_LENGTH + 1];
        let mut c = 0;
        for l in 2..=MAX_CODE_LENGTH {
            c = (c + count[l - 1]) << 1;
            next[l] = c;
        }
        let mut code = vec![0u16; lens.len()];
        for (s, &l) in lens.iter().enumerate() {
            if l > 0 {
                code[s] = reverse_bits(next[l as usize], l as u32) as u16;
                next[l as usize] += 1;
            }
        }
        Code { len: lens.to_vec(), code }
    }

    #[inline]
    fn put(&self, bw: &mut BitWriter, s: usize) {
        bw.put(self.code[s] as u32, self.len[s] as u32);
    }
}

/// The run-length tokens of code lengths: (symbol 0..=18, extra bits).
fn length_tokens(lens: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    let mut prev = 8u8;
    let mut i = 0;
    while i < lens.len() {
        let v = lens[i];
        let mut run = lens[i..].iter().take_while(|&&l| l == v).count();
        i += run;
        if v == 0 {
            while run > 0 {
                if run < 3 {
                    out.extend(std::iter::repeat_n((0, 0), run));
                    run = 0;
                } else if run <= 10 {
                    out.push((17, run as u8 - 3));
                    run = 0;
                } else {
                    let r = run.min(138);
                    out.push((18, (r - 11) as u8));
                    run -= r;
                }
            }
        } else {
            if v != prev {
                out.push((v, 0));
                run -= 1;
                prev = v;
            }
            while run > 0 {
                if run < 3 {
                    out.extend(std::iter::repeat_n((v, 0), run));
                    run = 0;
                } else {
                    let r = run.min(6);
                    out.push((16, (r - 3) as u8));
                    run -= r;
                }
            }
        }
    }
    out
}

fn token_extra_bits(t: u8) -> u32 {
    match t {
        16 => 2,
        17 => 3,
        18 => 7,
        _ => 0,
    }
}

/// Writes the prefix code for `counts` and returns it.
fn write_code(bw: &mut BitWriter, counts: &[u32]) -> Code {
    let lens = huffman_lengths(counts, MAX_CODE_LENGTH as u32);
    let used: Vec<usize> = (0..lens.len()).filter(|&i| lens[i] > 0).collect();
    let code = Code::new(&lens);
    if used.len() <= 2 && used.iter().all(|&s| s < 256) {
        // A simple code; no symbol at all is written as the single symbol 0.
        let s0 = used.first().copied().unwrap_or(0) as u32;
        bw.put(1, 1);
        bw.put(used.len().max(1) as u32 - 1, 1);
        if s0 < 2 {
            bw.put(0, 1);
            bw.put(s0, 1);
        } else {
            bw.put(1, 1);
            bw.put(s0, 8);
        }
        if used.len() == 2 {
            bw.put(used[1] as u32, 8);
        }
        return code;
    }
    bw.put(0, 1);
    let tokens = length_tokens(&lens);
    let mut th = [0u32; 19];
    for t in &tokens {
        th[t.0 as usize] += 1;
    }
    let cl = huffman_lengths(&th, MAX_CL_LENGTH as u32);
    let cl_code = Code::new(&cl);
    let mut ncl = 19;
    while ncl > 4 && cl[CODE_LENGTH_ORDER[ncl - 1]] == 0 {
        ncl -= 1;
    }
    bw.put(ncl as u32 - 4, 4);
    for &i in &CODE_LENGTH_ORDER[..ncl] {
        bw.put(cl[i] as u32, 3);
    }
    let mut trimmed = tokens.len();
    let mut zero_bits = 0;
    while trimmed > 0 && matches!(tokens[trimmed - 1].0, 0 | 17 | 18) {
        let t = tokens[trimmed - 1].0;
        zero_bits += cl[t as usize] as u32 + token_extra_bits(t);
        trimmed -= 1;
    }
    let trim = trimmed > 1 && zero_bits > 12;
    bw.put(trim as u32, 1);
    let count = if trim {
        let v = (trimmed - 2) as u32;
        let nbits = if v == 0 { 0 } else { 31 - v.leading_zeros() };
        let pairs = nbits / 2 + 1;
        bw.put(pairs - 1, 3);
        bw.put(v, pairs * 2);
        trimmed
    } else {
        tokens.len()
    };
    for &(t, extra) in &tokens[..count] {
        cl_code.put(bw, t as usize);
        bw.put(extra as u32, token_extra_bits(t));
    }
    code
}

/// Writes the five codes of a group.
fn write_codes(bw: &mut BitWriter, h: &Histo) -> [Code; 5] {
    h.parts().map(|p| write_code(bw, p))
}

/// Writes the symbols of `refs`; `group` gives the group of the pixel at (x, y).
fn write_refs(bw: &mut BitWriter, refs: &[Ref], w: usize, codes: &[[Code; 5]], group: impl Fn(usize, usize) -> usize) {
    let (mut x, mut y) = (0, 0);
    for r in refs {
        let [g, red, b, a, d] = &codes[group(x, y)];
        match *r {
            Ref::Lit(p) => {
                g.put(bw, ((p >> 8) & 0xff) as usize);
                red.put(bw, ((p >> 16) & 0xff) as usize);
                b.put(bw, (p & 0xff) as usize);
                a.put(bw, (p >> 24) as usize);
            }
            Ref::Cache(s) => g.put(bw, NUM_LITERALS + NUM_LENGTH_CODES + s as usize),
            Ref::Copy { len, code } => {
                let (s, nb, extra) = prefix(len);
                g.put(bw, NUM_LITERALS + s);
                bw.put(extra, nb);
                let (s, nb, extra) = prefix(code);
                d.put(bw, s);
                bw.put(extra, nb);
            }
        }
        x += r.len();
        if x >= w {
            y += x / w;
            x %= w;
        }
    }
}

/// Writes an entropy-coded image: the color cache bits, (for the main image `top`) the meta prefix
/// codes, the prefix codes and the LZ77-coded pixels.
fn write_image_data(bw: &mut BitWriter, w: usize, h: usize, px: &[u32], top: bool) {
    let dc = DistCodes::new(w);
    let (iters, window) = chain_effort(w, h, top);
    let chain = hash_chain(px, w, iters, window);
    let greedy = greedy_refs(px, &chain, &dc);
    // The cache size that suits the greedy parse, and the one that suits literals only: the
    // optimal parse may well prefer cache hits to the greedy parse's copies.
    let (parsed, lit) = best_cache_bits(px, &greedy);
    let mut sizes = vec![parsed];
    if lit.abs_diff(parsed) >= 3 && w * h <= 1 << 20 {
        sizes.push(lit);
    }
    let mut best: Option<(f64, u32, Vec<Ref>)> = None;
    for &bits in &sizes {
        let mut first = greedy.clone();
        apply_cache(px, &mut first, bits);
        let model = CostModel::new(&Histo::of(&first, bits));
        let refs = optimal_refs(px, &chain, &dc, bits, std::slice::from_ref(&model), None);
        let cost = refs_cost(&refs, bits);
        if best.as_ref().is_none_or(|b| cost < b.0) {
            best = Some((cost, bits, refs));
        }
    }
    let Some((_, cache_bits, refs)) = best else { unreachable!() };
    if cache_bits > 0 {
        bw.put(1, 1);
        bw.put(cache_bits, 4);
    } else {
        bw.put(0, 1);
    }
    let mut single = BitWriter::new();
    if top {
        single.put(0, 1);
    }
    let codes = write_codes(&mut single, &Histo::of(&refs, cache_bits));
    write_refs(&mut single, &refs, w, std::slice::from_ref(&codes), |_, _| 0);
    if top && w * h >= 1024 {
        if let Some(multi) = write_meta_codes(px, w, h, &chain, &dc, &refs, cache_bits) {
            if multi.bits() < single.bits() {
                bw.append(multi);
                return;
            }
        }
    }
    bw.append(single);
}

// ---------------------------------------------------------------------------------------------
// Meta prefix codes

/// `n * log2(n)`.
#[inline]
fn nlogn(n: u64) -> f64 {
    if n < 2 {
        0.0
    } else {
        n as f64 * (n as f64).log2()
    }
}

/// Estimated bits of coding counts with their own prefix code: entropy plus the code's size.
fn part_cost(counts: impl Iterator<Item = u32>) -> f64 {
    let (mut total, mut sum, mut used, mut runs, mut zero) = (0u64, 0f64, 0usize, 0usize, false);
    for c in counts {
        if c > 0 {
            total += c as u64;
            sum += nlogn(c as u64);
            used += 1;
            zero = false;
        } else {
            if !zero {
                runs += 1;
            }
            zero = true;
        }
    }
    let header = if used <= 2 { 12.0 } else { 40.0 + used as f64 * 3.0 + runs as f64 * 4.0 };
    nlogn(total) - sum + header
}

/// The five part ranges of a [`Histo`] with green alphabet `ga`.
fn part_ranges(ga: usize) -> [std::ops::Range<usize>; 5] {
    [0..ga, ga..ga + 256, ga + 256..ga + 512, ga + 512..ga + 768, ga + 768..ga + 768 + NUM_DIST_CODES]
}

fn histo_cost(h: &Histo) -> f64 {
    part_ranges(h.ga).into_iter().map(|r| part_cost(h.h[r].iter().copied())).sum()
}

fn merged_cost(a: &Histo, b: &Histo) -> f64 {
    part_ranges(a.ga).into_iter().map(|r| part_cost(a.h[r.clone()].iter().zip(&b.h[r]).map(|(x, y)| x + y))).sum()
}

struct Cluster {
    h: Histo,
    cost: f64,
}

impl Cluster {
    fn merge(&mut self, other: &Cluster, cost: f64) {
        for (x, y) in self.h.h.iter_mut().zip(&other.h.h) {
            *x += y;
        }
        self.cost = cost;
    }
}

/// Clusters the tile histograms (libwebp's way in short: entropy bins, then random pairs while
/// there are many, then the best pairs greedily, then each tile moved to its cheapest cluster).
/// Returns the cluster of each tile and the clusters' histograms.
fn cluster_tiles(tiles: &[Histo]) -> (Vec<u32>, Vec<Histo>) {
    const GREEDY_MAX: usize = 64;
    let nonempty: Vec<usize> = (0..tiles.len()).filter(|&t| tiles[t].h.iter().any(|&c| c > 0)).collect();
    // 1. Entropy bins: tiles with similar green, red and blue entropy are merged when it pays.
    let feats: Vec<[f64; 3]> = nonempty
        .iter()
        .map(|&t| {
            let r = part_ranges(tiles[t].ga);
            [0, 1, 2].map(|k| part_cost(tiles[t].h[r[k].clone()].iter().copied()))
        })
        .collect();
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for f in &feats {
        for k in 0..3 {
            lo[k] = lo[k].min(f[k]);
            hi[k] = hi[k].max(f[k]);
        }
    }
    let mut clusters: Vec<Cluster> = Vec::new();
    let mut bins: HashMap<usize, usize> = HashMap::new();
    for (&t, f) in nonempty.iter().zip(&feats) {
        let bin = (0..3).fold(0, |acc, k| {
            let q = if hi[k] > lo[k] { (((f[k] - lo[k]) / (hi[k] - lo[k])) * 4.0).min(3.0) as usize } else { 0 };
            acc * 4 + q
        });
        let c = Cluster { h: tiles[t].clone(), cost: histo_cost(&tiles[t]) };
        if let Some(&i) = bins.get(&bin) {
            let m = merged_cost(&clusters[i].h, &c.h);
            if m < clusters[i].cost + c.cost {
                clusters[i].merge(&c, m);
                continue;
            }
        } else {
            bins.insert(bin, clusters.len());
        }
        clusters.push(c);
    }
    // 2. Random pairs, the best of a sample merged, while there are too many clusters.
    let mut seed = 0x9e37_79b9u32;
    let mut rnd = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize % n
    };
    while clusters.len() > GREEDY_MAX {
        let mut best = (f64::MIN, 0, 0, 0.0);
        for _ in 0..16 {
            let a = rnd(clusters.len());
            let b = rnd(clusters.len() - 1);
            let b = if b >= a { b + 1 } else { b };
            let m = merged_cost(&clusters[a].h, &clusters[b].h);
            let gain = clusters[a].cost + clusters[b].cost - m;
            if gain > best.0 {
                best = (gain, a, b, m);
            }
        }
        let (_, a, b, m) = best;
        let other = clusters.swap_remove(b);
        let a = if a == clusters.len() { b } else { a };
        clusters[a].merge(&other, m);
    }
    let mut clusters = merge_greedily(clusters);
    // 4. Each tile to the cluster that codes it cheapest, a few rounds, then merge again.
    let mut assign = vec![0u32; tiles.len()];
    for round in 0..4 {
        assign = remap(tiles, &clusters);
        let fresh = rebuild(tiles, &mut assign);
        if round == 3 || fresh.len() == clusters.len() && fresh.iter().zip(&clusters).all(|(a, b)| a.h == b.h.h) {
            clusters = fresh.into_iter().map(|h| Cluster { cost: histo_cost(&h), h }).collect();
            break;
        }
        clusters = fresh.into_iter().map(|h| Cluster { cost: histo_cost(&h), h }).collect();
    }
    let n = clusters.len();
    let merged = merge_greedily(clusters);
    if merged.len() < n {
        let merged: Vec<Cluster> = merged;
        assign = remap(tiles, &merged);
        let out = rebuild(tiles, &mut assign);
        return (assign, out);
    }
    let out = merged.into_iter().map(|c| c.h).collect();
    (assign, out)
}

/// Merges the pair of clusters that saves the most bits while some pair saves bits.
fn merge_greedily(mut clusters: Vec<Cluster>) -> Vec<Cluster> {
    let k = clusters.len();
    let mut gain = vec![vec![f64::MIN; k]; k];
    let mut merged = vec![vec![0f64; k]; k];
    for a in 0..k {
        for b in a + 1..k {
            let m = merged_cost(&clusters[a].h, &clusters[b].h);
            gain[a][b] = clusters[a].cost + clusters[b].cost - m;
            merged[a][b] = m;
        }
    }
    let mut alive = vec![true; k];
    loop {
        let mut best = (0.0, 0, 0);
        for a in 0..k {
            if alive[a] {
                for b in a + 1..k {
                    if alive[b] && gain[a][b] > best.0 {
                        best = (gain[a][b], a, b);
                    }
                }
            }
        }
        if best.0 <= 0.0 {
            break;
        }
        let (_, a, b) = best;
        let m = merged[a][b];
        let (left, right) = clusters.split_at_mut(b);
        left[a].merge(&right[0], m);
        alive[b] = false;
        for (c, &live) in alive.iter().enumerate() {
            if live && c != a {
                let (x, y) = (a.min(c), a.max(c));
                let m = merged_cost(&clusters[x].h, &clusters[y].h);
                gain[x][y] = clusters[x].cost + clusters[y].cost - m;
                merged[x][y] = m;
            }
        }
    }
    clusters.into_iter().zip(alive).filter(|c| c.1).map(|c| c.0).collect()
}

/// The cluster that codes each tile cheapest (an empty tile takes the one before it).
fn remap(tiles: &[Histo], clusters: &[Cluster]) -> Vec<u32> {
    let costs: Vec<Vec<f32>> =
        clusters.iter().map(|c| part_ranges(c.h.ga).into_iter().flat_map(|r| symbol_costs(&c.h.h[r])).collect()).collect();
    let mut assign = vec![0u32; tiles.len()];
    let mut last = 0;
    let mut used: Vec<(usize, u32)> = Vec::new();
    for (t, tile) in tiles.iter().enumerate() {
        used.clear();
        used.extend(tile.h.iter().enumerate().filter(|c| *c.1 > 0).map(|(i, &c)| (i, c)));
        if !used.is_empty() {
            let cost = |c: &Vec<f32>| used.iter().map(|&(i, n)| c[i] * n as f32).sum::<f32>();
            last = (0..costs.len()).min_by(|&a, &b| cost(&costs[a]).total_cmp(&cost(&costs[b]))).unwrap_or(0) as u32;
        }
        assign[t] = last;
    }
    assign
}

/// The histograms of the clusters as assigned, renumbered without the unused ones.
fn rebuild(tiles: &[Histo], assign: &mut [u32]) -> Vec<Histo> {
    let ga = tiles[0].ga;
    let mut out: Vec<Histo> = Vec::new();
    let mut renum = vec![u32::MAX; assign.iter().max().map_or(0, |&m| m as usize + 1)];
    for (t, a) in assign.iter_mut().enumerate() {
        if renum[*a as usize] == u32::MAX {
            renum[*a as usize] = out.len() as u32;
            out.push(Histo { h: vec![0; tiles[0].h.len()], ga });
        }
        *a = renum[*a as usize];
        for (x, y) in out[*a as usize].h.iter_mut().zip(&tiles[t].h) {
            *x += y;
        }
    }
    out
}

/// The meta prefix codes, prefix codes and pixels of the main image with clustered tile
/// histograms, or `None` when one group is best.
/// The histogram of the symbols starting in each tile.
fn tile_histos(refs: &[Ref], w: usize, h: usize, bits: u32, cache_bits: u32) -> Vec<Histo> {
    let tw = subsample(w, bits);
    let mut tiles = vec![Histo::new(cache_bits); tw * subsample(h, bits)];
    let (mut x, mut y) = (0, 0);
    for r in refs {
        tiles[(y >> bits) * tw + (x >> bits)].add(r);
        x += r.len();
        if x >= w {
            y += x / w;
            x %= w;
        }
    }
    tiles
}

/// The main image with meta prefix codes: tile histograms of `refs` are clustered, the image is
/// parsed again with each tile's cluster costs, clustered again and written. `None` when one
/// group is best.
fn write_meta_codes(
    px: &[u32],
    w: usize,
    h: usize,
    chain: &[u32],
    dc: &DistCodes,
    refs: &[Ref],
    cache_bits: u32,
) -> Option<BitWriter> {
    let mut bits = 3;
    while bits < 9 && subsample(w, bits) * subsample(h, bits) > 2600 {
        bits += 1;
    }
    let (tw, th) = (subsample(w, bits), subsample(h, bits));
    let (mut group, mut groups) = cluster_tiles(&tile_histos(refs, w, h, bits, cache_bits));
    if groups.len() < 2 {
        return None;
    }
    let reparsed;
    let mut refs = refs;
    if w * h <= 1 << 20 {
        // Parse again with each tile's cluster costs, and cluster again (large images skip it).
        let models: Vec<CostModel> = groups.iter().map(CostModel::new).collect();
        reparsed = optimal_refs(px, chain, dc, cache_bits, &models, Some(&Tiles { bits, tw, group }));
        refs = &reparsed;
        (group, groups) = cluster_tiles(&tile_histos(refs, w, h, bits, cache_bits));
        if groups.len() < 2 {
            return None;
        }
    }
    let mut bw = BitWriter::new();
    bw.put(1, 1);
    bw.put(bits - 2, 3);
    let ent: Vec<u32> = group.iter().map(|&g| 0xff00_0000 | g << 8).collect();
    write_image_data(&mut bw, tw, th, &ent, false);
    let codes: Vec<[Code; 5]> = groups.iter().map(|g| write_codes(&mut bw, g)).collect();
    let tiles = Tiles { bits, tw, group };
    write_refs(&mut bw, refs, w, &codes, |x, y| tiles.group(x, y));
    Some(bw)
}

#[cfg(test)]
mod tests {
    use super::super::vp8l_dec::{decode, decode_alpha};
    use super::*;

    /// A deterministic pseudo-random sequence.
    pub(super) struct Rng(u64);
    impl Rng {
        pub(super) fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) as u32
        }
    }

    /// `px` with fully transparent pixels made 0 (their RGB is not kept).
    fn visible(px: &[u32]) -> Vec<u32> {
        px.iter().map(|&p| if p >> 24 == 0 { 0 } else { p }).collect()
    }

    fn round_trip(name: &str, w: usize, h: usize, px: &[u32]) -> usize {
        let data = encode(w, h, px);
        let (dw, dh, out) = decode(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((dw, dh), (w, h), "{name}");
        let (want, got) = (visible(px), visible(&out));
        if let Some(i) = (0..want.len()).find(|&i| want[i] != got[i]) {
            panic!("{name} {w}x{h}: pixel ({}, {}) is {:08x}, not {:08x}", i % w, i / w, got[i], want[i]);
        }
        assert_eq!(data[4] >> 4 & 1, px.iter().any(|&p| p >> 24 != 255) as u8, "{name}: alpha_is_used");
        data.len()
    }

    fn images() -> Vec<(String, usize, usize, Vec<u32>)> {
        let mut rng = Rng(1);
        let mut out = Vec::new();
        for &(w, h) in &[(1, 1), (1, 9), (9, 1), (2, 2), (3, 7), (37, 23), (64, 64), (100, 37), (257, 3)] {
            let noise: Vec<u32> = (0..w * h).map(|_| rng.next()).collect();
            out.push(("noise-alpha".to_string(), w, h, noise.clone()));
            out.push(("noise".to_string(), w, h, noise.iter().map(|p| p | 0xff00_0000).collect()));
            let grad = (0..w * h)
                .map(|i| 0xff00_0000 | ((i % w * 5) as u32 & 255) << 16 | ((i / w * 3) as u32 & 255) << 8 | (i as u32 & 255))
                .collect();
            out.push(("gradient".to_string(), w, h, grad));
            // 8, 4, 2 and 1 pixels per byte, and no palette.
            for colors in [2, 4, 16, 17, 256, 300] {
                let pal: Vec<u32> = (0..colors).map(|_| rng.next() | 0xff00_0000).collect();
                let px = (0..w * h).map(|i| pal[(i / 3 + i / w) % colors]).collect();
                out.push((format!("{colors}-colors"), w, h, px));
            }
            let alpha = (0..w * h).map(|i| (((i * 7 % 256) as u32) << 24) | (0x123456 * (i as u32 % 3))).collect();
            out.push(("alpha".to_string(), w, h, alpha));
            let flat = vec![0xff33_6699; w * h];
            out.push(("flat".to_string(), w, h, flat));
        }
        out
    }

    #[test]
    fn round_trips_are_exact() {
        for (name, w, h, px) in images() {
            round_trip(&name, w, h, &px);
        }
    }

    #[test]
    fn very_wide_and_tall_images_round_trip() {
        let w = 16383;
        let px: Vec<u32> = (0..w).map(|i| 0xff00_0000 | (i as u32 * 77) & 0xffffff).collect();
        round_trip("wide", w, 1, &px);
        let px: Vec<u32> = (0..2 * w).map(|i| 0xff00_0000 | (i as u32 / 100 * 0x010203)).collect();
        round_trip("wide2", w, 2, &px);
        round_trip("tall", 1, w, &px[..w]);
    }

    #[test]
    fn alpha_planes_round_trip() {
        let mut rng = Rng(9);
        for &(w, h) in &[(1, 1), (5, 3), (40, 30), (128, 7)] {
            let planes: Vec<Vec<u8>> = vec![
                (0..w * h).map(|i| if (i % w) * 2 < w { 0 } else { 255 }).collect(),
                (0..w * h).map(|i| (i % w * 255 / w) as u8).collect(),
                (0..w * h).map(|_| rng.next() as u8).collect(),
                vec![200; w * h],
            ];
            for a in planes {
                let data = encode_alpha(w, h, &a);
                assert_eq!(decode_alpha(&data, w, h).unwrap(), a);
            }
        }
    }

    #[test]
    fn filtered_alpha_planes_round_trip() {
        // A soft-edged disc (smooth alpha suits a filter) and a few other planes.
        let (w, h) = (90, 70);
        let disc: Vec<u8> = (0..w * h)
            .map(|i| {
                let (dx, dy) = ((i % w) as f64 - 45.0, (i / w) as f64 - 35.0);
                ((30.0 - (dx * dx + dy * dy).sqrt()) * 12.0).clamp(0.0, 255.0) as u8
            })
            .collect();
        let mut rng = Rng(4);
        let noise: Vec<u8> = (0..w * h).map(|_| rng.next() as u8).collect();
        let ramp: Vec<u8> = (0..w * h).map(|i| (i % w + i / w) as u8).collect();
        for a in [disc, noise, ramp] {
            let (filter, data) = encode_alpha_filtered(w, h, &a);
            assert!(data.len() <= encode_alpha(w, h, &a).len());
            let mut back = decode_alpha(&data, w, h).unwrap();
            // Undo the ALPH filter as the container spec does.
            for i in 0..w * h {
                let (x, y) = (i % w, i / w);
                let pred = match (filter, x, y) {
                    (0, _, _) | (_, 0, 0) => 0,
                    (_, _, 0) => back[i - 1],
                    (_, 0, _) => back[i - w],
                    (1, _, _) => back[i - 1],
                    (2, _, _) => back[i - w],
                    _ => (back[i - 1] as i32 + back[i - w] as i32 - back[i - w - 1] as i32).clamp(0, 255) as u8,
                };
                back[i] = back[i].wrapping_add(pred);
            }
            assert_eq!(back, a, "filter {filter}");
        }
    }

    #[test]
    fn every_predictor_round_trips() {
        let (w, h) = (48, 40);
        let mut rng = Rng(3);
        let px: Vec<u32> = (0..w * h).map(|i| (rng.next() & 0x0f0f_0f0f).wrapping_add((i as u32 % w as u32) * 0x0101_0101)).collect();
        for mode in 0..16u32 {
            let mut img = px.clone();
            // A forced predictor sub-image with all 16 modes in tiles of 4.
            let bits = 2;
            let (tw, th) = (subsample(w, bits), subsample(h, bits));
            let modes: Vec<u32> = (0..tw * th).map(|t| 0xff00_0000 | ((t as u32 + mode) % 16) << 8).collect();
            let mut res = vec![0; w * h];
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    let pred = match (x, y) {
                        (0, 0) => 0xff00_0000,
                        (_, 0) => img[i - 1],
                        (0, _) => img[i - w],
                        _ => predict(
                            (modes[(y >> bits) * tw + (x >> bits)] >> 8) & 15,
                            img[i - 1],
                            img[i - w],
                            img[i - w - 1],
                            img[i - w + 1],
                        ),
                    };
                    res[i] = sub_pixels(img[i], pred);
                }
            }
            img = res;
            let mut bw = BitWriter::new();
            bw.put(0x2f, 8);
            bw.put((w as u32 - 1) | (h as u32 - 1) << 14 | 1 << 28, 32);
            bw.put(1, 1);
            bw.put(0, 2);
            bw.put(bits - 2, 3);
            write_image_data(&mut bw, tw, th, &modes, false);
            bw.put(0, 1);
            write_image_data(&mut bw, w, h, &img, true);
            let (_, _, out) = decode(&bw.finish()).unwrap();
            assert_eq!(out, px, "mode offset {mode}");
        }
    }

    #[test]
    fn meta_prefix_codes_round_trip() {
        let (w, h) = (96, 80);
        let mut rng = Rng(5);
        let px: Vec<u32> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if (x / 24 + y / 20) % 2 == 0 {
                    0xff00_0000 | (rng.next() & 0x0f0f0f)
                } else {
                    0xff00_0000 | (x * 0x030201 + y * 0x010000) as u32
                }
            })
            .collect();
        let chain = hash_chain(&px, w, CHAIN_ITERS, WINDOW);
        let dc = DistCodes::new(w);
        let refs = greedy_refs(&px, &chain, &dc);
        for cache_bits in [0, 4] {
            let mut refs = refs.clone();
            apply_cache(&px, &mut refs, cache_bits);
            let multi = write_meta_codes(&px, w, h, &chain, &dc, &refs, cache_bits).expect("several groups");
            let mut bw = BitWriter::new();
            bw.put(0x2f, 8);
            bw.put((w as u32 - 1) | (h as u32 - 1) << 14, 32);
            bw.put(0, 1);
            if cache_bits > 0 {
                bw.put(1, 1);
                bw.put(cache_bits, 4);
            } else {
                bw.put(0, 1);
            }
            bw.append(multi);
            let (_, _, out) = decode(&bw.finish()).unwrap();
            assert_eq!(out, px, "cache bits {cache_bits}");
        }
    }

    #[test]
    fn prefix_codes_match_the_decoder() {
        for v in 1..5000u32 {
            let (s, nb, extra) = prefix(v);
            let s = s as u32;
            let back = if s < 4 { s + 1 } else { ((2 + (s & 1)) << ((s - 2) >> 1)) + extra + 1 };
            assert_eq!(back, v);
            assert_eq!(nb, if s < 4 { 0 } else { (s - 2) >> 1 });
        }
        for w in [1, 2, 3, 7, 8, 9, 16, 100] {
            let dc = DistCodes::new(w);
            for d in 1..2000 {
                assert_eq!(super::super::vp8l_dec::code_to_distance(dc.code(d) as usize, w), d, "w {w} d {d}");
            }
        }
    }
}

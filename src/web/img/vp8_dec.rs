//! The lossy WebP decoder: a VP8 key frame (RFC 6386) to YUV planes and then RGB, for WebP input
//! files and to check the encoder. Supports everything a key frame can use: segments, the normal
//! and simple loop filters with level deltas, up to 8 token partitions, probability updates.
//! Chroma is upsampled with the usual 9-3-3-1 filter.

use super::common::{check_size, Res};
use super::vp8_dsp::{self as dsp, Ctx, FilterParams, CS, S};
use super::vp8_tables::*;

/// The boolean decoder of RFC 6386 section 7; reading past the end yields zeros (and is noted).
struct BoolReader<'a> {
    data: &'a [u8],
    at: usize,
    /// A 16-bit window: the top 8 bits compare with the split.
    value: u32,
    range: u32,
    bit_count: u32,
    eof: bool,
}

impl<'a> BoolReader<'a> {
    fn new(data: &'a [u8]) -> BoolReader<'a> {
        let mut r = BoolReader { data, at: 0, value: 0, range: 255, bit_count: 0, eof: false };
        r.value = (r.byte() << 8) | r.byte();
        r
    }

    #[inline]
    fn byte(&mut self) -> u32 {
        let b = self.data.get(self.at).copied().unwrap_or(0) as u32;
        self.at += 1;
        if self.at > self.data.len() + 2 {
            self.eof = true;
        }
        b
    }

    #[inline]
    fn get(&mut self, prob: u8) -> bool {
        let split = 1 + (((self.range - 1) * prob as u32) >> 8);
        let big = split << 8;
        let bit = if self.value >= big {
            self.range -= split;
            self.value -= big;
            true
        } else {
            self.range = split;
            false
        };
        if self.range < 128 {
            let shift = self.range.leading_zeros() - 24;
            self.range <<= shift;
            self.value <<= shift;
            self.bit_count += shift;
            if self.bit_count >= 8 {
                self.bit_count -= 8;
                let b = self.byte();
                self.value |= b << self.bit_count;
            }
        }
        bit
    }
    fn flag(&mut self) -> bool {
        self.get(128)
    }

    fn literal(&mut self, n: u32) -> u32 {
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | self.flag() as u32;
        }
        v
    }

    fn signed(&mut self, n: u32) -> i32 {
        if !self.flag() {
            return 0;
        }
        let v = self.literal(n) as i32;
        if self.flag() {
            -v
        } else {
            v
        }
    }

    fn tree(&mut self, tree: &[i8], probs: &[u8]) -> u8 {
        let mut at = 0usize;
        loop {
            let next = tree[at + self.get(probs[at / 2]) as usize];
            if next <= 0 {
                return (-next) as u8;
            }
            at = next as usize;
        }
    }
}

/// A decoded frame: YUV planes padded to whole macroblocks.
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub mbw: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

impl Frame {
    /// The frame as RGBA (alpha 255), chroma upsampled with the 9-3-3-1 filter.
    pub fn to_rgba(&self) -> Vec<u8> {
        let (ys, cs) = (self.mbw * 16, self.mbw * 8);
        let (cw, ch) = (self.w.div_ceil(2), self.h.div_ceil(2));
        let mut out = vec![0u8; self.w * self.h * 4];
        for y in 0..self.h {
            let cy = y / 2;
            let fy = if y % 2 == 1 { (cy + 1).min(ch - 1) } else { cy.saturating_sub(1) };
            for x in 0..self.w {
                let cx = x / 2;
                let fx = if x % 2 == 1 { (cx + 1).min(cw - 1) } else { cx.saturating_sub(1) };
                let up = |p: &[u8]| {
                    (9 * p[cy * cs + cx] as u32 + 3 * p[cy * cs + fx] as u32 + 3 * p[fy * cs + cx] as u32 + p[fy * cs + fx] as u32 + 8)
                        >> 4
                };
                let (u, v) = (up(&self.u) as i32, up(&self.v) as i32);
                let yy = self.y[y * ys + x] as i32;
                let o = &mut out[(y * self.w + x) * 4..(y * self.w + x) * 4 + 4];
                o.copy_from_slice(&yuv_to_rgb(yy, u, v));
            }
        }
        out
    }
}

/// libwebp's YUV to RGB (BT.601, studio range, 14-bit fixed point).
fn yuv_to_rgb(y: i32, u: i32, v: i32) -> [u8; 4] {
    let mh = |a: i32, c: i32| (a * c) >> 8;
    let clip = |x: i32| (x >> 6).clamp(0, 255) as u8;
    let yy = mh(y, 19077);
    [clip(yy + mh(v, 26149) - 14234), clip(yy - mh(u, 6419) - mh(v, 13320) + 8708), clip(yy + mh(u, 33050) - 17685), 255]
}

type Probs = [[[[u8; 11]; 3]; 8]; 4];

/// Reads the levels of one block into `out` (raster order, dequantized). Returns the position after
/// the last coded coefficient (`first` when the block is empty), like libwebp's `GetCoeffs`.
fn read_block(br: &mut BoolReader, p: &Probs, ty: usize, ctx: usize, first: usize, dq: [i32; 2], out: &mut [i16; 16]) -> usize {
    let mut x = ctx;
    let mut n = first;
    while n < 16 {
        let pr = &p[ty][BANDS[n]][x];
        if !br.get(pr[0]) {
            return n;
        }
        let mut pr = pr;
        while !br.get(pr[1]) {
            n += 1;
            if n == 16 {
                return 16;
            }
            pr = &p[ty][BANDS[n]][0];
        }
        let v: i32 = if !br.get(pr[2]) {
            x = 1;
            1
        } else {
            x = 2;
            if !br.get(pr[3]) {
                if !br.get(pr[4]) {
                    2
                } else {
                    3 + br.get(pr[5]) as i32
                }
            } else if !br.get(pr[6]) {
                if !br.get(pr[7]) {
                    5 + br.get(159) as i32
                } else {
                    7 + 2 * br.get(165) as i32 + br.get(145) as i32
                }
            } else {
                let (base, extra): (i32, &[u8]) = if !br.get(pr[8]) {
                    if !br.get(pr[9]) {
                        (11, &CAT3)
                    } else {
                        (19, &CAT4)
                    }
                } else if !br.get(pr[10]) {
                    (35, &CAT5)
                } else {
                    (67, &CAT6)
                };
                let mut e = 0;
                for &ep in extra {
                    e = (e << 1) | br.get(ep) as i32;
                }
                base + e
            }
        };
        let v = if br.get(128) { -v } else { v };
        out[ZIGZAG[n]] = (v * dq[(n > 0) as usize]).clamp(-32768, 32767) as i16;
        n += 1;
    }
    16
}

#[derive(Clone, Copy, Default)]
struct Nz {
    y: [u8; 4],
    u: [u8; 2],
    v: [u8; 2],
    dc: u8,
}

/// Decodes the payload of a `VP8 ` chunk.
pub fn decode(data: &[u8]) -> Res<Frame> {
    if data.len() < 10 {
        return Err("truncated VP8 data".into());
    }
    let tag = data[0] as u32 | (data[1] as u32) << 8 | (data[2] as u32) << 16;
    if tag & 1 != 0 {
        return Err("a VP8 inter frame where a key frame is needed".into());
    }
    if tag >> 1 & 7 > 3 {
        return Err("unknown VP8 version".into());
    }
    let part0_len = (tag >> 5) as usize;
    if data[3..6] != [0x9d, 0x01, 0x2a] {
        return Err("bad VP8 start code".into());
    }
    let w = (u16::from_le_bytes([data[6], data[7]]) & 0x3fff) as usize;
    let h = (u16::from_le_bytes([data[8], data[9]]) & 0x3fff) as usize;
    check_size(w, h)?;
    let rest = &data[10..];
    if part0_len > rest.len() {
        return Err("truncated VP8 data".into());
    }
    let mut br = BoolReader::new(&rest[..part0_len]);
    let _color_space = br.flag();
    let _clamp = br.flag();
    // segments
    let mut seg_q = [0i32; 4];
    let mut seg_f = [0i32; 4];
    let mut seg_probs = [255u8; 3];
    let mut absolute = false;
    let use_segments = br.flag();
    let mut update_map = false;
    if use_segments {
        update_map = br.flag();
        if br.flag() {
            absolute = br.flag();
            for q in seg_q.iter_mut() {
                *q = br.signed(7);
            }
            for f in seg_f.iter_mut() {
                *f = br.signed(6);
            }
        }
        if update_map {
            for p in seg_probs.iter_mut() {
                *p = if br.flag() { br.literal(8) as u8 } else { 255 };
            }
        }
    }
    let simple = br.flag();
    let level = br.literal(6) as i32;
    let sharpness = br.literal(3) as u8;
    let mut ref_delta = 0;
    let mut mode_delta = 0;
    let use_lf_delta = br.flag();
    if use_lf_delta && br.flag() {
        for i in 0..4 {
            if br.flag() {
                let v = br.signed(6);
                if i == 0 {
                    ref_delta = v;
                }
            }
        }
        for i in 0..4 {
            if br.flag() {
                let v = br.signed(6);
                if i == 0 {
                    mode_delta = v;
                }
            }
        }
    }
    let nparts = 1usize << br.literal(2);
    let base_q = br.literal(7) as i32;
    let dq: [i32; 5] = std::array::from_fn(|_| br.signed(4));
    let _refresh = br.flag();
    let mut probs = COEFF_PROBS;
    for t in 0..4 {
        for b in 0..8 {
            for c in 0..3 {
                for n in 0..11 {
                    if br.get(COEFF_UPDATE_PROBS[t][b][c][n]) {
                        probs[t][b][c][n] = br.literal(8) as u8;
                    }
                }
            }
        }
    }
    let use_skip = br.flag();
    let skip_prob = if use_skip { br.literal(8) as u8 } else { 0 };

    // the token partitions
    let sizes_len = 3 * (nparts - 1);
    let after = &rest[part0_len..];
    if after.len() < sizes_len {
        return Err("truncated VP8 data".into());
    }
    let mut parts = Vec::with_capacity(nparts);
    let mut at = sizes_len;
    for p in 0..nparts {
        let len = if p + 1 < nparts {
            let s = &after[3 * p..3 * p + 3];
            s[0] as usize | (s[1] as usize) << 8 | (s[2] as usize) << 16
        } else {
            after.len().saturating_sub(at)
        };
        let end = (at + len).min(after.len());
        parts.push(BoolReader::new(&after[at.min(end)..end]));
        at = end;
    }

    // the dequantization factors of each segment: [y1, y2, uv] x [dc, ac]
    let qs: Vec<[[i32; 2]; 3]> = (0..4)
        .map(|s| {
            let q = if use_segments {
                if absolute {
                    seg_q[s]
                } else {
                    seg_q[s] + base_q
                }
            } else {
                base_q
            };
            let c = |d: i32| (q + d).clamp(0, 127) as usize;
            [
                [DC_Q[c(dq[0])] as i32, AC_Q[c(0)] as i32],
                [DC_Q[c(dq[1])] as i32 * 2, ((AC_Q[c(dq[2])] as i32 * 101581) >> 16).max(8)],
                [DC_Q[c(dq[3]).min(117)] as i32, AC_Q[c(dq[4])] as i32],
            ]
        })
        .collect();
    let filters: Vec<[FilterParams; 2]> = (0..4)
        .map(|s| {
            let base = if use_segments {
                if absolute {
                    seg_f[s]
                } else {
                    seg_f[s] + level
                }
            } else {
                level
            };
            std::array::from_fn(|i4| {
                let mut l = base;
                if use_lf_delta {
                    l += ref_delta;
                    if i4 == 1 {
                        l += mode_delta;
                    }
                }
                FilterParams::new(l, sharpness, i4 == 1)
            })
        })
        .collect();

    let (mbw, mbh) = (w.div_ceil(16), h.div_ceil(16));
    let (ys, cs) = (mbw * 16, mbw * 8);
    let mut fy = vec![0u8; ys * mbh * 16];
    let mut fu = vec![0u8; cs * mbh * 8];
    let mut fv = vec![0u8; cs * mbh * 8];
    let mut fparams = vec![FilterParams::default(); mbw * mbh];
    let mut top_nz = vec![Nz::default(); mbw];
    let mut top_modes = vec![[B_DC; 4]; mbw];
    let mut ctx = Ctx::new();
    for my in 0..mbh {
        let mut left_nz = Nz::default();
        let mut left_modes = [B_DC; 4];
        let tr = &mut parts[my % nparts];
        for mx in 0..mbw {
            // the header of the macroblock (first partition)
            let segment = if update_map {
                if !br.get(seg_probs[0]) {
                    br.get(seg_probs[1]) as usize
                } else {
                    2 + br.get(seg_probs[2]) as usize
                }
            } else {
                0
            };
            let skip = use_skip && br.get(skip_prob);
            let ymode = br.tree(&KF_YMODE_TREE, &KF_YMODE_PROBS);
            let mut bmodes = [0u8; 16];
            if ymode == B_PRED {
                for b in 0..16 {
                    let (bx, by) = (b % 4, b / 4);
                    let above = if by == 0 { top_modes[mx][bx] } else { bmodes[b - 4] };
                    let left = if bx == 0 { left_modes[by] } else { bmodes[b - 1] };
                    bmodes[b] = br.tree(&BMODE_TREE, &BMODE_PROBS[above as usize][left as usize]);
                }
            } else {
                bmodes = [match ymode {
                    V_PRED => B_VE,
                    H_PRED => B_HE,
                    TM_PRED => B_TM,
                    _ => B_DC,
                }; 16];
            }
            let uvmode = br.tree(&UV_MODE_TREE, &KF_UV_MODE_PROBS);
            top_modes[mx] = [bmodes[12], bmodes[13], bmodes[14], bmodes[15]];
            left_modes = [bmodes[3], bmodes[7], bmodes[11], bmodes[15]];

            // the coefficients
            let q = &qs[segment];
            let mut coefs = [[0i16; 16]; 24];
            let is4 = ymode == B_PRED;
            let t = &mut top_nz[mx];
            let mut coded = false;
            if !skip {
                let first = if is4 {
                    0
                } else {
                    let mut dc = [0i16; 16];
                    let n = read_block(tr, &probs, 1, (t.dc + left_nz.dc) as usize, 0, q[1], &mut dc);
                    let nz = (n > 0) as u8;
                    t.dc = nz;
                    left_nz.dc = nz;
                    let dcs = dsp::iwht(&dc);
                    for b in 0..16 {
                        coefs[b][0] = dcs[b];
                        coded |= dcs[b] != 0;
                    }
                    1
                };
                let ty = if is4 { 3 } else { 0 };
                for b in 0..16 {
                    let (bx, by) = (b % 4, b / 4);
                    let n = read_block(tr, &probs, ty, (t.y[bx] + left_nz.y[by]) as usize, first, q[0], &mut coefs[b]);
                    let nz = (n > first) as u8;
                    t.y[bx] = nz;
                    left_nz.y[by] = nz;
                    coded |= n > first;
                }
                for b in 0..8 {
                    let (bx, by) = (b % 2, (b % 4) / 2);
                    let (tt, ll) = if b < 4 { (&mut t.u, &mut left_nz.u) } else { (&mut t.v, &mut left_nz.v) };
                    let n = read_block(tr, &probs, 2, (tt[bx] + ll[by]) as usize, 0, q[2], &mut coefs[16 + b]);
                    let nz = (n > 0) as u8;
                    tt[bx] = nz;
                    ll[by] = nz;
                    coded |= n > 0;
                }
            } else {
                t.y = [0; 4];
                t.u = [0; 2];
                t.v = [0; 2];
                left_nz.y = [0; 4];
                left_nz.u = [0; 2];
                left_nz.v = [0; 2];
                if !is4 {
                    t.dc = 0;
                    left_nz.dc = 0;
                }
            }
            let mut f = filters[segment][is4 as usize];
            f.inner |= coded;
            fparams[my * mbw + mx] = f;

            // prediction and reconstruction
            ctx.load_edges(&fy, ys, &fu, &fv, cs, mx, my, mbw);
            let mut out = [0u8; 256];
            if is4 {
                for b in 0..16 {
                    let mut p4 = [0u8; 16];
                    dsp::pred4(bmodes[b], &ctx.y, b, &mut p4);
                    let mut r4 = [0u8; 16];
                    dsp::idct_add(&coefs[b], &p4, &mut r4);
                    ctx.put4(b, &r4);
                    for k in 0..16 {
                        out[((b / 4) * 4 + k / 4) * 16 + (b % 4) * 4 + k % 4] = r4[k];
                    }
                }
            } else {
                let mut pred = [0u8; 256];
                dsp::pred_block(ymode, &ctx.y, S, 16, my > 0, mx > 0, &mut pred);
                for b in 0..16 {
                    let (bx, by) = (b % 4, b / 4);
                    let p4: [u8; 16] = std::array::from_fn(|k| pred[(by * 4 + k / 4) * 16 + bx * 4 + k % 4]);
                    let mut r4 = [0u8; 16];
                    dsp::idct_add(&coefs[b], &p4, &mut r4);
                    for k in 0..16 {
                        out[(by * 4 + k / 4) * 16 + bx * 4 + k % 4] = r4[k];
                    }
                }
            }
            for r in 0..16 {
                fy[(my * 16 + r) * ys + mx * 16..][..16].copy_from_slice(&out[r * 16..r * 16 + 16]);
            }
            for (pi, (buf, plane)) in [(&ctx.u, &mut fu), (&ctx.v, &mut fv)].into_iter().enumerate() {
                let mut pred = [0u8; 64];
                dsp::pred_block(uvmode, buf, CS, 8, my > 0, mx > 0, &mut pred);
                for b in 0..4 {
                    let (bx, by) = (b % 2, b / 2);
                    let p4: [u8; 16] = std::array::from_fn(|k| pred[(by * 4 + k / 4) * 8 + bx * 4 + k % 4]);
                    let mut r4 = [0u8; 16];
                    dsp::idct_add(&coefs[16 + pi * 4 + b], &p4, &mut r4);
                    for k in 0..4 {
                        plane[(my * 8 + by * 4 + k) * cs + mx * 8 + bx * 4..][..4].copy_from_slice(&r4[k * 4..k * 4 + 4]);
                    }
                }
            }
        }
        if br.eof || parts[my % nparts].eof {
            return Err("truncated VP8 data".into());
        }
    }
    if level > 0 {
        dsp::filter_frame(&mut fy, ys, Some((&mut fu, &mut fv, cs)), mbw, mbh, &fparams, simple);
    }
    Ok(Frame { w, h, mbw, y: fy, u: fu, v: fv })
}

#[cfg(test)]
mod tests {
    use super::super::vp8::{self, BoolWriter};
    use super::*;

    #[test]
    fn the_boolean_coder_round_trips() {
        let mut w = BoolWriter::new();
        let mut seed = 99u32;
        let mut bits = Vec::new();
        for i in 0..5000u32 {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let p = ((seed >> 16) % 255 + 1) as u8;
            let b = (seed >> 8) % 256 < p as u32 / (1 + i % 3);
            w.put(b, p);
            bits.push((b, p));
        }
        w.literal(0x2a5, 10);
        let data = w.finish();
        let mut r = BoolReader::new(&data);
        for (b, p) in bits {
            assert_eq!(r.get(p), b);
        }
        assert_eq!(r.literal(10), 0x2a5);
        assert!(!r.eof);
    }

    #[test]
    fn decodes_what_the_encoder_wrote() {
        // gradients with a sharp edge, an odd size, low to top quality
        let (w, h) = (45, 29);
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let edge = if x > 20 { 120 } else { 0 };
                px.extend_from_slice(&[(x * 5 + edge) as u8, (y * 8) as u8, ((x + y) * 3) as u8, 255]);
            }
        }
        for q in [5.0, 50.0, 90.0, 100.0] {
            let yuv = vp8::Yuv::from_rgba(w, h, &px);
            let frame = decode(&vp8::encode(&yuv, &vp8::Options::new(q))).unwrap();
            assert_eq!((frame.w, frame.h), (w, h));
            let rgba = frame.to_rgba();
            let mse: f64 = px.iter().zip(&rgba).map(|(a, b)| (*a as f64 - *b as f64).powi(2)).sum::<f64>() / px.len() as f64;
            let psnr = 10.0 * (255.0f64 * 255.0 / mse.max(1e-9)).log10();
            assert!(psnr > if q > 80.0 { 28.0 } else { 20.0 }, "q{q}: PSNR {psnr}");
        }
    }
}

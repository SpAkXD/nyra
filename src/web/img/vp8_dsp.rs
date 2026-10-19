//! The pixel work of VP8 that the encoder and the decoder must do bit for bit alike: intra
//! prediction, the 4x4 transforms (DCT and Walsh-Hadamard) and the loop filter (RFC 6386,
//! sections 12, 14 and 15; the formulas follow libvpx and libwebp).
//!
//! A macroblock is predicted from a work buffer (`Ctx`) that holds its 16x16 luma pixels with the
//! row above (plus 4 pixels above-right) and the column to the left, and the two 8x8 chroma blocks
//! the same way. The frame's borders use the format's fixed values: 127 above the first row, 129
//! left of the first column.

use super::vp8_tables::*;

/// The stride of the luma work buffer: the left column, 16 pixels, 4 pixels above-right.
pub const S: usize = 21;
/// The stride of a chroma work buffer: the left column and 8 pixels.
pub const CS: usize = 9;

/// The work buffer of one macroblock: luma at `y[(row + 1) * S + col + 1]` (row -1 is the row
/// above, col -1 the column to the left), chroma alike in `u` and `v` with stride `CS`.
#[derive(Clone)]
pub struct Ctx {
    pub y: [u8; S * 17],
    pub u: [u8; CS * 9],
    pub v: [u8; CS * 9],
}

impl Ctx {
    pub fn new() -> Ctx {
        Ctx { y: [0; S * 17], u: [0; CS * 9], v: [0; CS * 9] }
    }

    /// Fills the edges of the macroblock at (`mx`, `my`) from the reconstructed (unfiltered) planes:
    /// the row above (and above-right), the column to the left, the corner, with the format's
    /// values outside the frame. `mbw` is the frame's width in macroblocks.
    #[allow(clippy::too_many_arguments)]
    pub fn load_edges(&mut self, y: &[u8], ys: usize, u: &[u8], v: &[u8], cs: usize, mx: usize, my: usize, mbw: usize) {
        let (x0, y0) = (mx * 16, my * 16);
        // luma: above and above-right
        if my == 0 {
            self.y[..S].fill(127);
        } else {
            let above = &y[(y0 - 1) * ys..];
            self.y[0] = if mx == 0 { 129 } else { above[x0 - 1] };
            self.y[1..17].copy_from_slice(&above[x0..x0 + 16]);
            if mx + 1 < mbw {
                self.y[17..21].copy_from_slice(&above[x0 + 16..x0 + 20]);
            } else {
                let last = above[x0 + 15];
                self.y[17..21].fill(last);
            }
        }
        // the above-right pixels repeat for the 4x4 blocks of the right column below the first row
        for r in [4, 8, 12] {
            let (top, rest) = self.y.split_at_mut(r * S);
            rest[17..21].copy_from_slice(&top[17..21]);
        }
        for r in 0..16 {
            self.y[(r + 1) * S] = if mx == 0 { 129 } else { y[(y0 + r) * ys + x0 - 1] };
        }
        // chroma
        let (cx0, cy0) = (mx * 8, my * 8);
        for (plane, buf) in [(u, &mut self.u), (v, &mut self.v)] {
            if my == 0 {
                buf[..CS].fill(127);
            } else {
                let above = &plane[(cy0 - 1) * cs..];
                buf[0] = if mx == 0 { 129 } else { above[cx0 - 1] };
                buf[1..9].copy_from_slice(&above[cx0..cx0 + 8]);
            }
            for r in 0..8 {
                buf[(r + 1) * CS] = if mx == 0 { 129 } else { plane[(cy0 + r) * cs + cx0 - 1] };
            }
        }
    }

    /// Writes the 4x4 block `px` (luma block `b`, raster order in the macroblock) into the buffer,
    /// where the next blocks' predictions read it.
    pub fn put4(&mut self, b: usize, px: &[u8; 16]) {
        let (bx, by) = (b % 4, b / 4);
        for r in 0..4 {
            let at = (by * 4 + r + 1) * S + bx * 4 + 1;
            self.y[at..at + 4].copy_from_slice(&px[r * 4..r * 4 + 4]);
        }
    }
}

impl Default for Ctx {
    fn default() -> Self {
        Ctx::new()
    }
}

#[inline]
fn avg3(a: u8, b: u8, c: u8) -> u8 {
    ((a as u32 + 2 * b as u32 + c as u32 + 2) >> 2) as u8
}

#[inline]
fn avg2(a: u8, b: u8) -> u8 {
    ((a as u32 + b as u32 + 1) >> 1) as u8
}

#[inline]
pub fn clip8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// The 16x16 luma prediction (or 8x8 chroma, `n = 8`) of `mode` from a work buffer with stride
/// `s`. `top`/`left`: whether the macroblock has neighbors above / to the left (for DC).
pub fn pred_block(mode: u8, buf: &[u8], s: usize, n: usize, top: bool, left: bool, out: &mut [u8]) {
    let above = &buf[1..1 + n];
    let tl = buf[0] as i32;
    let l = |r: usize| buf[(r + 1) * s];
    match mode {
        DC_PRED => {
            let shift = if n == 16 { 4 } else { 3 };
            let dc = match (top, left) {
                (true, true) => {
                    let sum: u32 = above.iter().map(|&p| p as u32).sum::<u32>() + (0..n).map(|r| l(r) as u32).sum::<u32>();
                    (sum + n as u32) >> (shift + 1)
                }
                (true, false) => (above.iter().map(|&p| p as u32).sum::<u32>() + (n as u32 >> 1)) >> shift,
                (false, true) => ((0..n).map(|r| l(r) as u32).sum::<u32>() + (n as u32 >> 1)) >> shift,
                (false, false) => 128,
            };
            out[..n * n].fill(dc as u8);
        }
        V_PRED => {
            for r in 0..n {
                out[r * n..r * n + n].copy_from_slice(above);
            }
        }
        H_PRED => {
            for r in 0..n {
                out[r * n..r * n + n].fill(l(r));
            }
        }
        _ => {
            for r in 0..n {
                let d = l(r) as i32 - tl;
                for c in 0..n {
                    out[r * n + c] = clip8(above[c] as i32 + d);
                }
            }
        }
    }
}

/// The prediction of the 4x4 luma block `b` (raster order) with 4x4 mode `mode`.
pub fn pred4(mode: u8, y: &[u8; S * 17], b: usize, out: &mut [u8; 16]) {
    let (bx, by) = (b % 4, b / 4);
    let base = by * 4 * S + bx * 4; // the top-left corner pixel
    let top = |i: usize| y[base + 1 + i]; // A.. H (0..8)
    let left = |i: usize| y[base + (i + 1) * S]; // I J K L
    let x = y[base];
    let mut set = |c: usize, r: usize, v: u8| out[r * 4 + c] = v;
    match mode {
        B_DC => {
            let mut dc = 4u32;
            for i in 0..4 {
                dc += top(i) as u32 + left(i) as u32;
            }
            out.fill((dc >> 3) as u8);
        }
        B_TM => {
            for r in 0..4 {
                let d = left(r) as i32 - x as i32;
                for c in 0..4 {
                    set(c, r, clip8(top(c) as i32 + d));
                }
            }
        }
        B_VE => {
            let vals = [avg3(x, top(0), top(1)), avg3(top(0), top(1), top(2)), avg3(top(1), top(2), top(3)), avg3(top(2), top(3), top(4))];
            for r in 0..4 {
                for c in 0..4 {
                    set(c, r, vals[c]);
                }
            }
        }
        B_HE => {
            let (a, b2, c2, d, e) = (x, left(0), left(1), left(2), left(3));
            let rows = [avg3(a, b2, c2), avg3(b2, c2, d), avg3(c2, d, e), avg3(d, e, e)];
            for r in 0..4 {
                for c in 0..4 {
                    set(c, r, rows[r]);
                }
            }
        }
        B_RD => {
            let (i, j, k, l) = (left(0), left(1), left(2), left(3));
            let (a, b2, c2, d) = (top(0), top(1), top(2), top(3));
            set(0, 3, avg3(j, k, l));
            let v = avg3(i, j, k);
            set(1, 3, v);
            set(0, 2, v);
            let v = avg3(x, i, j);
            set(2, 3, v);
            set(1, 2, v);
            set(0, 1, v);
            let v = avg3(a, x, i);
            set(3, 3, v);
            set(2, 2, v);
            set(1, 1, v);
            set(0, 0, v);
            let v = avg3(b2, a, x);
            set(3, 2, v);
            set(2, 1, v);
            set(1, 0, v);
            let v = avg3(c2, b2, a);
            set(3, 1, v);
            set(2, 0, v);
            set(3, 0, avg3(d, c2, b2));
        }
        B_LD => {
            let t: [u8; 8] = std::array::from_fn(top);
            set(0, 0, avg3(t[0], t[1], t[2]));
            let v = avg3(t[1], t[2], t[3]);
            set(1, 0, v);
            set(0, 1, v);
            let v = avg3(t[2], t[3], t[4]);
            set(2, 0, v);
            set(1, 1, v);
            set(0, 2, v);
            let v = avg3(t[3], t[4], t[5]);
            set(3, 0, v);
            set(2, 1, v);
            set(1, 2, v);
            set(0, 3, v);
            let v = avg3(t[4], t[5], t[6]);
            set(3, 1, v);
            set(2, 2, v);
            set(1, 3, v);
            let v = avg3(t[5], t[6], t[7]);
            set(3, 2, v);
            set(2, 3, v);
            set(3, 3, avg3(t[6], t[7], t[7]));
        }
        B_VR => {
            let (i, j, k) = (left(0), left(1), left(2));
            let (a, b2, c2, d) = (top(0), top(1), top(2), top(3));
            let v = avg2(x, a);
            set(0, 0, v);
            set(1, 2, v);
            let v = avg2(a, b2);
            set(1, 0, v);
            set(2, 2, v);
            let v = avg2(b2, c2);
            set(2, 0, v);
            set(3, 2, v);
            set(3, 0, avg2(c2, d));
            set(0, 3, avg3(k, j, i));
            set(0, 2, avg3(j, i, x));
            let v = avg3(i, x, a);
            set(0, 1, v);
            set(1, 3, v);
            let v = avg3(x, a, b2);
            set(1, 1, v);
            set(2, 3, v);
            let v = avg3(a, b2, c2);
            set(2, 1, v);
            set(3, 3, v);
            set(3, 1, avg3(b2, c2, d));
        }
        B_VL => {
            let t: [u8; 8] = std::array::from_fn(top);
            set(0, 0, avg2(t[0], t[1]));
            let v = avg2(t[1], t[2]);
            set(1, 0, v);
            set(0, 2, v);
            let v = avg2(t[2], t[3]);
            set(2, 0, v);
            set(1, 2, v);
            let v = avg2(t[3], t[4]);
            set(3, 0, v);
            set(2, 2, v);
            set(0, 1, avg3(t[0], t[1], t[2]));
            let v = avg3(t[1], t[2], t[3]);
            set(1, 1, v);
            set(0, 3, v);
            let v = avg3(t[2], t[3], t[4]);
            set(2, 1, v);
            set(1, 3, v);
            let v = avg3(t[3], t[4], t[5]);
            set(3, 1, v);
            set(2, 3, v);
            set(3, 2, avg3(t[4], t[5], t[6]));
            set(3, 3, avg3(t[5], t[6], t[7]));
        }
        B_HD => {
            let (i, j, k, l) = (left(0), left(1), left(2), left(3));
            let (a, b2, c2) = (top(0), top(1), top(2));
            let v = avg2(i, x);
            set(0, 0, v);
            set(2, 1, v);
            let v = avg2(j, i);
            set(0, 1, v);
            set(2, 2, v);
            let v = avg2(k, j);
            set(0, 2, v);
            set(2, 3, v);
            set(0, 3, avg2(l, k));
            set(3, 0, avg3(a, b2, c2));
            set(2, 0, avg3(x, a, b2));
            let v = avg3(i, x, a);
            set(1, 0, v);
            set(3, 1, v);
            let v = avg3(j, i, x);
            set(1, 1, v);
            set(3, 2, v);
            let v = avg3(k, j, i);
            set(1, 2, v);
            set(3, 3, v);
            set(1, 3, avg3(l, k, j));
        }
        _ => {
            // B_HU
            let (i, j, k, l) = (left(0), left(1), left(2), left(3));
            set(0, 0, avg2(i, j));
            let v = avg2(j, k);
            set(2, 0, v);
            set(0, 1, v);
            let v = avg2(k, l);
            set(2, 1, v);
            set(0, 2, v);
            set(1, 0, avg3(i, j, k));
            let v = avg3(j, k, l);
            set(3, 0, v);
            set(1, 1, v);
            let v = avg3(k, l, l);
            set(3, 1, v);
            set(1, 2, v);
            for (c, r) in [(3, 2), (2, 2), (0, 3), (1, 3), (2, 3), (3, 3)] {
                set(c, r, l);
            }
        }
    }
}

// ---- transforms ----------------------------------------------------------------------------------

/// The forward DCT of `src - pred` (both 4x4, row-major), coefficients in raster order. This is
/// libwebp's integer transform, which the inverse below undoes up to rounding.
pub fn fdct(src: &[u8; 16], pred: &[u8; 16]) -> [i16; 16] {
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let d: [i32; 4] = std::array::from_fn(|k| src[i * 4 + k] as i32 - pred[i * 4 + k] as i32);
        let a0 = d[0] + d[3];
        let a1 = d[1] + d[2];
        let a2 = d[1] - d[2];
        let a3 = d[0] - d[3];
        tmp[i * 4] = (a0 + a1) * 8;
        tmp[1 + i * 4] = (a2 * 2217 + a3 * 5352 + 1812) >> 9;
        tmp[2 + i * 4] = (a0 - a1) * 8;
        tmp[3 + i * 4] = (a3 * 2217 - a2 * 5352 + 937) >> 9;
    }
    let mut out = [0i16; 16];
    for i in 0..4 {
        let a0 = tmp[i] + tmp[12 + i];
        let a1 = tmp[4 + i] + tmp[8 + i];
        let a2 = tmp[4 + i] - tmp[8 + i];
        let a3 = tmp[i] - tmp[12 + i];
        out[i] = ((a0 + a1 + 7) >> 4) as i16;
        out[4 + i] = (((a2 * 2217 + a3 * 5352 + 12000) >> 16) + (a3 != 0) as i32) as i16;
        out[8 + i] = ((a0 - a1 + 7) >> 4) as i16;
        out[12 + i] = ((a3 * 2217 - a2 * 5352 + 51000) >> 16) as i16;
    }
    out
}

const C1: i32 = 20091 + (1 << 16);
const C2: i32 = 35468;

#[inline]
fn mul(a: i32, b: i32) -> i32 {
    (a * b) >> 16
}

/// The inverse DCT of the (dequantized) coefficients added to `pred` (RFC 6386 section 14.3).
pub fn idct_add(coef: &[i16; 16], pred: &[u8; 16], out: &mut [u8; 16]) {
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let (i0, i4, i8, i12) = (coef[i] as i32, coef[4 + i] as i32, coef[8 + i] as i32, coef[12 + i] as i32);
        let a = i0 + i8;
        let b = i0 - i8;
        let c = mul(i4, C2) - mul(i12, C1);
        let d = mul(i4, C1) + mul(i12, C2);
        tmp[i * 4] = a + d;
        tmp[i * 4 + 1] = b + c;
        tmp[i * 4 + 2] = b - c;
        tmp[i * 4 + 3] = a - d;
    }
    // tmp holds the columns: tmp[col * 4 + row]
    for r in 0..4 {
        let dc = tmp[r] + 4;
        let a = dc + tmp[8 + r];
        let b = dc - tmp[8 + r];
        let c = mul(tmp[4 + r], C2) - mul(tmp[12 + r], C1);
        let d = mul(tmp[4 + r], C1) + mul(tmp[12 + r], C2);
        let row = [a + d, b + c, b - c, a - d];
        for k in 0..4 {
            out[r * 4 + k] = clip8(pred[r * 4 + k] as i32 + (row[k] >> 3));
        }
    }
}

/// The forward Walsh-Hadamard transform of the 16 DC coefficients of a 16x16 macroblock
/// (in the raster order of its 4x4 blocks).
pub fn fwht(dc: &[i16; 16]) -> [i16; 16] {
    let mut tmp = [0i32; 16];
    for r in 0..4 {
        let i: [i32; 4] = std::array::from_fn(|k| dc[r * 4 + k] as i32);
        let a0 = i[0] + i[2];
        let a1 = i[1] + i[3];
        let a2 = i[1] - i[3];
        let a3 = i[0] - i[2];
        tmp[r * 4] = a0 + a1;
        tmp[r * 4 + 1] = a3 + a2;
        tmp[r * 4 + 2] = a3 - a2;
        tmp[r * 4 + 3] = a0 - a1;
    }
    let mut out = [0i16; 16];
    for c in 0..4 {
        let a0 = tmp[c] + tmp[8 + c];
        let a1 = tmp[4 + c] + tmp[12 + c];
        let a2 = tmp[4 + c] - tmp[12 + c];
        let a3 = tmp[c] - tmp[8 + c];
        out[c] = ((a0 + a1) >> 1) as i16;
        out[4 + c] = ((a3 + a2) >> 1) as i16;
        out[8 + c] = ((a3 - a2) >> 1) as i16;
        out[12 + c] = ((a0 - a1) >> 1) as i16;
    }
    out
}

/// The inverse Walsh-Hadamard transform (RFC 6386 section 14.3): the DC of each 4x4 block.
pub fn iwht(coef: &[i16; 16]) -> [i16; 16] {
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let a0 = coef[i] as i32 + coef[12 + i] as i32;
        let a1 = coef[4 + i] as i32 + coef[8 + i] as i32;
        let a2 = coef[4 + i] as i32 - coef[8 + i] as i32;
        let a3 = coef[i] as i32 - coef[12 + i] as i32;
        tmp[i] = a0 + a1;
        tmp[8 + i] = a0 - a1;
        tmp[4 + i] = a3 + a2;
        tmp[12 + i] = a3 - a2;
    }
    let mut out = [0i16; 16];
    for r in 0..4 {
        let dc = tmp[r * 4] + 3;
        let a0 = dc + tmp[r * 4 + 3];
        let a1 = tmp[r * 4 + 1] + tmp[r * 4 + 2];
        let a2 = tmp[r * 4 + 1] - tmp[r * 4 + 2];
        let a3 = dc - tmp[r * 4 + 3];
        out[r * 4] = ((a0 + a1) >> 3) as i16;
        out[r * 4 + 1] = ((a3 + a2) >> 3) as i16;
        out[r * 4 + 2] = ((a0 - a1) >> 3) as i16;
        out[r * 4 + 3] = ((a3 - a2) >> 3) as i16;
    }
    out
}

// ---- the loop filter -------------------------------------------------------------------------------

/// How one macroblock is filtered (computed from the frame's and segment's filter level).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct FilterParams {
    /// The edge limit (0: no filtering).
    pub limit: u8,
    pub ilevel: u8,
    pub hev: u8,
    /// Filter the edges inside the macroblock too.
    pub inner: bool,
}

impl FilterParams {
    /// The parameters for a filter `level` (0..=63) and `sharpness` (0..=7).
    pub fn new(level: i32, sharpness: u8, inner: bool) -> FilterParams {
        let level = level.clamp(0, 63);
        if level == 0 {
            return FilterParams::default();
        }
        let mut ilevel = level;
        if sharpness > 0 {
            ilevel >>= if sharpness > 4 { 2 } else { 1 };
            ilevel = ilevel.min(9 - sharpness as i32);
        }
        let ilevel = ilevel.max(1);
        FilterParams {
            limit: (2 * level + ilevel) as u8,
            ilevel: ilevel as u8,
            hev: if level >= 40 { 2 } else if level >= 15 { 1 } else { 0 },
            inner,
        }
    }
}

#[inline]
fn sclip1(v: i32) -> i32 {
    v.clamp(-128, 127)
}

#[inline]
fn sclip2(v: i32) -> i32 {
    v.clamp(-16, 15)
}

/// The 8 pixels across an edge, p3 p2 p1 p0 | q0 q1 q2 q3, from `base` (the p3 pixel) in steps.
#[inline]
fn load8(p: &[u8], base: usize, step: usize) -> [i32; 8] {
    std::array::from_fn(|k| p[base + k * step] as i32)
}

#[inline]
fn store(p: &mut [u8], base: usize, step: usize, k: usize, v: i32) {
    p[base + k * step] = clip8(v);
}

/// The 2-pixel filter (p0, q0); `v` are the pixels from p3.
#[inline]
fn filter2(p: &mut [u8], base: usize, step: usize, v: &[i32; 8]) {
    let (p1, p0, q0, q1) = (v[2], v[3], v[4], v[5]);
    let a = 3 * (q0 - p0) + sclip1(p1 - q1);
    let a1 = sclip2((a + 4) >> 3);
    let a2 = sclip2((a + 3) >> 3);
    store(p, base, step, 3, p0 + a2);
    store(p, base, step, 4, q0 - a1);
}

#[inline]
fn filter4(p: &mut [u8], base: usize, step: usize, v: &[i32; 8]) {
    let (p1, p0, q0, q1) = (v[2], v[3], v[4], v[5]);
    let a = 3 * (q0 - p0);
    let a1 = sclip2((a + 4) >> 3);
    let a2 = sclip2((a + 3) >> 3);
    let a3 = (a1 + 1) >> 1;
    store(p, base, step, 2, p1 + a3);
    store(p, base, step, 3, p0 + a2);
    store(p, base, step, 4, q0 - a1);
    store(p, base, step, 5, q1 - a3);
}

#[inline]
fn filter6(p: &mut [u8], base: usize, step: usize, v: &[i32; 8]) {
    let (p2, p1, p0, q0, q1, q2) = (v[1], v[2], v[3], v[4], v[5], v[6]);
    let a = sclip1(3 * (q0 - p0) + sclip1(p1 - q1));
    let a1 = (27 * a + 63) >> 7;
    let a2 = (18 * a + 63) >> 7;
    let a3 = (9 * a + 63) >> 7;
    store(p, base, step, 1, p2 + a3);
    store(p, base, step, 2, p1 + a2);
    store(p, base, step, 3, p0 + a1);
    store(p, base, step, 4, q0 - a1);
    store(p, base, step, 5, q1 - a2);
    store(p, base, step, 6, q2 - a3);
}

/// Filters `n` pixels along an edge: `at` is the first pixel after the edge, `step` crosses the
/// edge, `along` moves along it. `mb_edge`: the 6-tap macroblock-edge filter, else the inner one.
fn filter_edge(p: &mut [u8], at: usize, step: usize, along: usize, n: usize, f: &FilterParams, mb_edge: bool) {
    let limit = f.limit as i32 + if mb_edge { 4 } else { 0 };
    let t = 2 * limit + 1;
    let (it, ht) = (f.ilevel as i32, f.hev as i32);
    let mut base = at - 4 * step;
    for _ in 0..n {
        let v = load8(p, base, step);
        let (p3, p2, p1, p0, q0, q1, q2, q3) = (v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]);
        if 4 * (p0 - q0).abs() + (p1 - q1).abs() <= t
            && (p1 - p0).abs() <= it
            && (q1 - q0).abs() <= it
            && (p3 - p2).abs() <= it
            && (p2 - p1).abs() <= it
            && (q3 - q2).abs() <= it
            && (q2 - q1).abs() <= it
        {
            if (p1 - p0).abs() > ht || (q1 - q0).abs() > ht {
                filter2(p, base, step, &v);
            } else if mb_edge {
                filter6(p, base, step, &v);
            } else {
                filter4(p, base, step, &v);
            }
        }
        base += along;
    }
}

fn simple_edge(p: &mut [u8], at: usize, step: usize, along: usize, n: usize, limit: i32) {
    let t = 2 * limit + 1;
    // the simple filter reads only p1..q1; start at p1 and pretend it is p3's slot shifted by 2
    let mut base = at - 4 * step;
    for _ in 0..n {
        let v: [i32; 8] = std::array::from_fn(|k| if (2..6).contains(&k) { p[base + k * step] as i32 } else { 0 });
        if 4 * (v[3] - v[4]).abs() + (v[2] - v[5]).abs() <= t {
            filter2(p, base, step, &v);
        }
        base += along;
    }
}
/// Applies the loop filter to a whole frame in macroblock order (each macroblock: its left edge,
/// inner vertical edges, top edge, inner horizontal edges), as a decoder does. `params` has one
/// entry per macroblock; `simple` selects the simple filter (luma only). Without `chroma` (the U
/// and V planes and their stride) only luma is filtered.
pub fn filter_frame(y: &mut [u8], ys: usize, mut chroma: Option<(&mut [u8], &mut [u8], usize)>, mbw: usize, mbh: usize, params: &[FilterParams], simple: bool) {
    for my in 0..mbh {
        for mx in 0..mbw {
            let f = &params[my * mbw + mx];
            if f.limit == 0 {
                continue;
            }
            let yat = my * 16 * ys + mx * 16;
            if simple {
                let l = f.limit as i32;
                if mx > 0 {
                    simple_edge(y, yat, 1, ys, 16, l + 4);
                }
                if f.inner {
                    for k in 1..4 {
                        simple_edge(y, yat + 4 * k, 1, ys, 16, l);
                    }
                }
                if my > 0 {
                    simple_edge(y, yat, ys, 1, 16, l + 4);
                }
                if f.inner {
                    for k in 1..4 {
                        simple_edge(y, yat + 4 * k * ys, ys, 1, 16, l);
                    }
                }
                continue;
            }
            let mut uv = chroma.as_mut().map(|(u, v, cs)| (&mut **u, &mut **v, *cs, my * 8 * *cs + mx * 8));
            if mx > 0 {
                filter_edge(y, yat, 1, ys, 16, f, true);
                if let Some((u, v, cs, cat)) = uv.as_mut() {
                    filter_edge(u, *cat, 1, *cs, 8, f, true);
                    filter_edge(v, *cat, 1, *cs, 8, f, true);
                }
            }
            if f.inner {
                for k in 1..4 {
                    filter_edge(y, yat + 4 * k, 1, ys, 16, f, false);
                }
                if let Some((u, v, cs, cat)) = uv.as_mut() {
                    filter_edge(u, *cat + 4, 1, *cs, 8, f, false);
                    filter_edge(v, *cat + 4, 1, *cs, 8, f, false);
                }
            }
            if my > 0 {
                filter_edge(y, yat, ys, 1, 16, f, true);
                if let Some((u, v, cs, cat)) = uv.as_mut() {
                    filter_edge(u, *cat, *cs, 1, 8, f, true);
                    filter_edge(v, *cat, *cs, 1, 8, f, true);
                }
            }
            if f.inner {
                for k in 1..4 {
                    filter_edge(y, yat + 4 * k * ys, ys, 1, 16, f, false);
                }
                if let Some((u, v, cs, cat)) = uv.as_mut() {
                    filter_edge(u, *cat + 4 * *cs, *cs, 1, 8, f, false);
                    filter_edge(v, *cat + 4 * *cs, *cs, 1, 8, f, false);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_round_trip() {
        // the inverse DCT undoes the forward one up to rounding
        let mut seed = 12345u32;
        for _ in 0..200 {
            let mut src = [0u8; 16];
            let mut pred = [0u8; 16];
            for k in 0..16 {
                seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                src[k] = (seed >> 16) as u8;
                seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                pred[k] = (seed >> 16) as u8;
            }
            let c = fdct(&src, &pred);
            let mut out = [0u8; 16];
            idct_add(&c, &pred, &mut out);
            for k in 0..16 {
                assert!((out[k] as i32 - src[k] as i32).abs() <= 1, "{src:?} {out:?}");
            }
        }
        let dc: [i16; 16] = std::array::from_fn(|k| (k as i16 * 37 - 200) * 8);
        let back = iwht(&fwht(&dc));
        for k in 0..16 {
            assert!((back[k] - dc[k]).abs() <= 1, "{dc:?} {back:?}");
        }
    }

    #[test]
    fn filter_params_follow_the_format() {
        let f = FilterParams::new(20, 0, true);
        assert_eq!((f.limit, f.ilevel, f.hev), (60, 20, 1));
        let f = FilterParams::new(40, 5, false);
        assert_eq!((f.limit, f.ilevel, f.hev), (84, 4, 2));
        assert_eq!(FilterParams::new(0, 0, true).limit, 0);
    }
}

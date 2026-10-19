//! Downscaling: a separable Lanczos3 filter in linear light with premultiplied alpha, so edges
//! stay sharp, colors do not darken where they mix, and transparent pixels do not bleed their
//! (invisible) color into the visible ones. Large reductions first halve the image with a 2x2 box
//! filter (also in linear light) until it is at most twice the wanted size, then filter.
//!
//! The filter streams: it keeps only the source rows its window needs, so the memory is the
//! source plus a few rows.

use super::common::Image;

/// sRGB (0..=255) to linear light (0..=1).
fn to_linear_table() -> [f32; 256] {
    std::array::from_fn(|i| {
        let c = i as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    })
}

/// Linear light (0..=1, in 14-bit steps) back to sRGB.
const OUT_BITS: usize = 14;

fn to_srgb_table() -> Vec<u8> {
    let n = 1 << OUT_BITS;
    (0..n)
        .map(|i| {
            let l = i as f32 / (n - 1) as f32;
            let c = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
            (c * 255.0 + 0.5).clamp(0.0, 255.0) as u8
        })
        .collect()
}

#[inline]
fn to_srgb(t: &[u8], l: f32) -> u8 {
    let i = (l * ((1 << OUT_BITS) - 1) as f32 + 0.5) as i32;
    t[i.clamp(0, (1 << OUT_BITS) - 1) as usize]
}

fn lanczos3(x: f32) -> f32 {
    let x = x.abs();
    if x < 1e-6 {
        return 1.0;
    }
    if x >= 3.0 {
        return 0.0;
    }
    let px = std::f32::consts::PI * x;
    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
}

/// For each output pixel along one axis: the first source index and the weights (summing to 1,
/// the edge pixel repeated outside the image).
fn weights(src: usize, dst: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = src as f32 / dst as f32;
    let fscale = scale.max(1.0);
    let support = 3.0 * fscale;
    (0..dst)
        .map(|i| {
            let center = (i as f32 + 0.5) * scale - 0.5;
            let lo = (center - support).floor() as isize + 1;
            let hi = (center + support).ceil() as isize - 1;
            let first = lo.max(0) as usize;
            let last = (hi.max(0) as usize).min(src - 1);
            let mut w = vec![0f32; last - first + 1];
            for j in lo..=hi {
                let k = lanczos3((j as f32 - center) / fscale);
                let at = (j.clamp(0, src as isize - 1) as usize).clamp(first, last) - first;
                w[at] += k;
            }
            let sum: f32 = w.iter().sum();
            if sum.abs() > 1e-6 {
                w.iter_mut().for_each(|x| *x /= sum);
            }
            (first, w)
        })
        .collect()
}

/// One row of the source as premultiplied linear RGBA.
fn linear_row(img: &Image, y: usize, lin: &[f32; 256], out: &mut [f32]) {
    let row = &img.px[y * img.w * 4..(y + 1) * img.w * 4];
    for (o, p) in out.as_chunks_mut::<4>().0.iter_mut().zip(row.as_chunks::<4>().0) {
        let a = p[3] as f32 / 255.0;
        o[0] = lin[p[0] as usize] * a;
        o[1] = lin[p[1] as usize] * a;
        o[2] = lin[p[2] as usize] * a;
        o[3] = a;
    }
}

/// Premultiplied linear RGBA back to straight sRGB RGBA8.
fn store(px: &[f32], srgb: &[u8], out: &mut [u8]) {
    for (o, p) in out.as_chunks_mut::<4>().0.iter_mut().zip(px.as_chunks::<4>().0) {
        let a = p[3].clamp(0.0, 1.0);
        if a < 1.0 / 512.0 {
            *o = [0, 0, 0, 0];
            continue;
        }
        let inv = 1.0 / a;
        o[0] = to_srgb(srgb, p[0] * inv);
        o[1] = to_srgb(srgb, p[1] * inv);
        o[2] = to_srgb(srgb, p[2] * inv);
        o[3] = (a * 255.0 + 0.5) as u8;
    }
}

/// The image at `dw` x `dh` (each at least 1) with Lanczos3 in linear light.
fn lanczos(src: &Image, dw: usize, dh: usize) -> Image {
    let (sw, sh) = (src.w, src.h);
    let lin = to_linear_table();
    let srgb = to_srgb_table();
    let xw = weights(sw, dw);
    let yw = weights(sh, dh);
    let cap = yw.iter().map(|w| w.1.len()).max().unwrap_or(1) + 1;
    let mut ring = vec![0f32; cap * dw * 4];
    let mut ring_row = vec![usize::MAX; cap];
    let mut row = vec![0f32; sw * 4];
    let mut acc = vec![0f32; dw * 4];
    let mut out = vec![0u8; dw * dh * 4];
    for (y, (start, ws)) in yw.iter().enumerate() {
        for k in 0..ws.len() {
            let sy = start + k;
            let slot = sy % cap;
            if ring_row[slot] != sy {
                linear_row(src, sy, &lin, &mut row);
                let dst = &mut ring[slot * dw * 4..(slot + 1) * dw * 4];
                for (x, (xs, xws)) in xw.iter().enumerate() {
                    let mut s = [0f32; 4];
                    for (j, &w) in xws.iter().enumerate() {
                        let p = &row[(xs + j) * 4..(xs + j) * 4 + 4];
                        s[0] += w * p[0];
                        s[1] += w * p[1];
                        s[2] += w * p[2];
                        s[3] += w * p[3];
                    }
                    dst[x * 4..x * 4 + 4].copy_from_slice(&s);
                }
                ring_row[slot] = sy;
            }
        }
        acc.fill(0.0);
        for (k, &w) in ws.iter().enumerate() {
            let slot = (start + k) % cap;
            let r = &ring[slot * dw * 4..(slot + 1) * dw * 4];
            for (a, &v) in acc.iter_mut().zip(r) {
                *a += w * v;
            }
        }
        store(&acc, &srgb, &mut out[y * dw * 4..(y + 1) * dw * 4]);
    }
    Image { w: dw, h: dh, px: out, icc: src.icc.clone() }
}

/// The image halved in both directions (a 2x2 box filter in linear light; an odd last row or
/// column is averaged with itself).
pub fn half(src: &Image) -> Image {
    let lin = to_linear_table();
    let srgb = to_srgb_table();
    let (dw, dh) = (src.w.div_ceil(2), src.h.div_ceil(2));
    let mut out = vec![0u8; dw * dh * 4];
    let mut acc = vec![0f32; dw * 4];
    for y in 0..dh {
        acc.fill(0.0);
        for sy in [2 * y, (2 * y + 1).min(src.h - 1)] {
            let row = &src.px[sy * src.w * 4..(sy + 1) * src.w * 4];
            for x in 0..dw {
                for sx in [2 * x, (2 * x + 1).min(src.w - 1)] {
                    let p = &row[sx * 4..sx * 4 + 4];
                    let a = p[3] as f32 / 255.0;
                    let o = &mut acc[x * 4..x * 4 + 4];
                    o[0] += lin[p[0] as usize] * a;
                    o[1] += lin[p[1] as usize] * a;
                    o[2] += lin[p[2] as usize] * a;
                    o[3] += a;
                }
            }
        }
        acc.iter_mut().for_each(|v| *v *= 0.25);
        store(&acc, &srgb, &mut out[y * dw * 4..(y + 1) * dw * 4]);
    }
    Image { w: dw, h: dh, px: out, icc: src.icc.clone() }
}

/// The image scaled to `dw` x `dh` (never up: a larger size returns a copy).
pub fn resize(src: &Image, dw: usize, dh: usize) -> Image {
    let (dw, dh) = (dw.max(1), dh.max(1));
    if dw >= src.w && dh >= src.h {
        return src.clone();
    }
    lanczos(src, dw.min(src.w), dh.min(src.h))
}

/// Halvings of an image, made when first needed, so many sizes can be made from one source
/// quickly: each size is filtered from the smallest level still at least twice as large.
pub struct Pyramid {
    levels: Vec<Image>,
}

impl Pyramid {
    pub fn new(src: Image) -> Pyramid {
        Pyramid { levels: vec![src] }
    }

    /// The image `dw` px wide (the height keeps the aspect ratio; at least 1).
    pub fn width(&mut self, dw: usize) -> Image {
        loop {
            let last = self.levels.last().expect("the source");
            if last.w < 4 * dw || last.h < 4 {
                break;
            }
            let next = half(last);
            self.levels.push(next);
        }
        let src = self.levels.iter().rev().find(|l| l.w >= 2 * dw).unwrap_or(&self.levels[0]);
        let s = &self.levels[0];
        let dh = ((s.h as f64 * dw as f64 / s.w as f64).round() as usize).max(1);
        resize(src, dw, dh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Image {
        let mut px = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&f(x, y));
            }
        }
        Image { w, h, px, icc: None }
    }

    #[test]
    fn keeps_flat_colors_and_sizes() {
        let src = img(37, 23, |_, _| [200, 100, 50, 255]);
        let out = resize(&src, 10, 6);
        assert_eq!((out.w, out.h), (10, 6));
        for p in out.px.as_chunks::<4>().0 {
            assert!(p.iter().zip([200u8, 100, 50, 255]).all(|(a, b)| (*a as i32 - b as i32).abs() <= 1), "{p:?}");
        }
        // never up
        assert_eq!(resize(&src, 100, 100).w, 37);
    }

    #[test]
    fn mixes_in_linear_light_and_ignores_transparent_color() {
        // black and white columns average to sRGB 188 (linear 0.5), not 128
        let src = img(64, 4, |x, _| if x % 2 == 0 { [0, 0, 0, 255] } else { [255, 255, 255, 255] });
        let out = resize(&src, 8, 1);
        // (the edges repeat the outermost column, so look inside)
        assert!(out.px.as_chunks::<4>().0.iter().skip(2).take(4).all(|p| (p[0] as i32 - 188).abs() <= 2), "{:?}", out.px);
        // a transparent red half does not tint the opaque green half
        let src = img(64, 4, |x, _| if x < 32 { [255, 0, 0, 0] } else { [0, 255, 0, 255] });
        let out = half(&half(&src));
        let p = &out.px[(8 + 1) * 4..(8 + 1) * 4 + 4];
        assert_eq!(&p[..3], &[0, 255, 0]);
    }

    #[test]
    fn pyramid_makes_each_width() {
        let src = img(1000, 600, |x, y| [(x % 256) as u8, (y % 256) as u8, 7, 255]);
        let mut p = Pyramid::new(src);
        for w in [480, 200, 100, 31] {
            let o = p.width(w);
            assert_eq!(o.w, w);
            assert_eq!(o.h, (600.0 * w as f64 / 1000.0).round() as usize);
        }
    }
}

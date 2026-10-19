//! Baseline JPEG encoding (zero crates) for the responsive images of `nyra build`: YCbCr with 2x2
//! chroma subsampling below quality 95 and full chroma from 95 (one component when the image is
//! gray), JFIF, the ICC profile as APP2
//! chunks, an accurate float DCT, and Huffman tables made for the image (two passes over the
//! blocks, like `cjpeg -optimize`). Alpha is ignored: the caller flattens the image first.
//!
//! Quantization: luma uses the table mozjpeg uses by default (N. Robidoux's, from ImageMagick),
//! scaled so that a quality gives about the luma SSIM that libjpeg gives at the same quality;
//! chroma uses libjpeg's own table and scaling. AC terms are rounded with a small dead zone (a
//! remainder of 0.6 rounds up, not 0.5). On photos this makes files about 5% smaller than
//! libjpeg-turbo's (Pillow, `optimize=True`) at equal SSIM.

use super::common::Image;
use super::jpeg::ZIGZAG;

/// Robidoux's luma table (mozjpeg's default, `quant_tbl_master_idx` 3), row-major.
const LUMA: [u16; 64] = [
    16, 16, 16, 18, 25, 37, 56, 85, 16, 17, 20, 27, 34, 40, 53, 75, 16, 20, 24, 31, 43, 62, 91, 135, 18, 27, 31, 40, 53, 74, 106, 156,
    25, 34, 43, 53, 69, 94, 131, 189, 37, 40, 62, 74, 94, 124, 169, 238, 56, 53, 91, 106, 131, 169, 226, 311, 85, 75, 135, 156, 189,
    238, 311, 418,
];
/// The chrominance table of the JPEG standard (Annex K), as libjpeg uses it.
const CHROMA: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];
/// The luma table's scale relative to libjpeg's, in percent: measured on photos, Robidoux's table
/// at 77% of libjpeg's scale gives the luma SSIM of libjpeg's own table at the same quality.
const LUMA_SCALE: u32 = 77;
/// How much less than half a step an AC remainder must exceed to round up.
const DEAD_ZONE: f32 = 0.1;

/// From this quality on, chroma keeps full resolution (4:4:4): about 15-20% more bytes than 4:2:0
/// for the same luma, so only for near-lossless requests, where 2x2 chroma would be the main loss.
const FULL_CHROMA_FROM: u32 = 95;

/// The most bytes of ICC profile in one APP2 chunk (65,535 minus the length and the header).
const ICC_CHUNK: usize = 65519;

/// Encodes `img` as a baseline JPEG at `quality` 1..=100 (on libjpeg's scale: about the same
/// SSIM as libjpeg at that quality; 75 to 90 suits photos). The output depends only on the input.
/// An image wider or taller than 65,535 pixels (JPEG's limit) gives an empty Vec.
pub fn encode(img: &Image, quality: u8) -> Vec<u8> {
    let (w, h) = (img.w, img.h);
    if w == 0 || h == 0 || w > 65535 || h > 65535 || img.px.len() < w * h * 4 {
        return Vec::new();
    }
    let gray = img.px.as_chunks::<4>().0.iter().all(|p| p[0] == p[1] && p[1] == p[2]);
    let q = quality.clamp(1, 100) as u32;
    let s = if q < 50 { 5000 / q } else { 200 - 2 * q };
    let qt = [
        LUMA.map(|b| ((b as u32 * s * LUMA_SCALE + 5000) / 10000).clamp(1, 255) as u16),
        CHROMA.map(|b| ((b as u32 * s + 50) / 100).clamp(1, 255) as u16),
    ];
    let sub = if gray || q >= FULL_CHROMA_FROM { 1 } else { 2 };
    let e = Enc { img, gray, sub, qt, dct: dct_matrix() };
    let ntab = if gray { 1 } else { 2 };

    // pass 1: the symbol statistics, for Huffman tables made for this image
    let mut freq = [[0u64; 257]; 4];
    let mut pred = [0i32; 3];
    e.blocks(|c, b| {
        let t = 2 * (c > 0) as usize;
        let (dc, ac) = freq[t..t + 2].split_at_mut(1);
        count(b, &mut pred[c], &mut dc[0], &mut ac[0]);
    });
    let tabs: Vec<Table> = freq[..2 * ntab].iter().map(Table::optimal).collect();

    let mut out = Vec::with_capacity(w * h / 4 + 1024);
    out.extend_from_slice(&[0xFF, 0xD8]);
    segment(&mut out, 0xE0, &[b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0]);
    if let Some(icc) = &img.icc {
        let n = icc.len().div_ceil(ICC_CHUNK);
        if (1..=255).contains(&n) {
            for (i, chunk) in icc.chunks(ICC_CHUNK).enumerate() {
                let mut p = b"ICC_PROFILE\0".to_vec();
                p.extend_from_slice(&[i as u8 + 1, n as u8]);
                p.extend_from_slice(chunk);
                segment(&mut out, 0xE2, &p);
            }
        }
    }
    let mut p = Vec::new();
    for (i, t) in qt[..ntab].iter().enumerate() {
        p.push(i as u8);
        p.extend(ZIGZAG.iter().map(|&z| t[z] as u8));
    }
    segment(&mut out, 0xDB, &p);
    let mut p = vec![8, (h >> 8) as u8, h as u8, (w >> 8) as u8, w as u8, if gray { 1 } else { 3 }];
    p.extend_from_slice(&[1, (sub as u8) << 4 | sub as u8, 0]);
    if !gray {
        p.extend_from_slice(&[2, 0x11, 1, 3, 0x11, 1]);
    }
    segment(&mut out, 0xC0, &p);
    let mut p = Vec::new();
    for (i, t) in tabs.iter().enumerate() {
        p.push((((i % 2) << 4) | (i / 2)) as u8);
        p.extend_from_slice(&t.bits);
        p.extend_from_slice(&t.vals);
    }
    segment(&mut out, 0xC4, &p);
    let sos: &[u8] = if gray { &[1, 1, 0x00, 0, 63, 0] } else { &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0] };
    segment(&mut out, 0xDA, sos);

    // pass 2: the entropy-coded data
    let mut bw = BitWriter { out, acc: 0, n: 0 };
    let mut pred = [0i32; 3];
    e.blocks(|c, b| {
        let t = 2 * (c > 0) as usize;
        write(b, &mut pred[c], &tabs[t], &tabs[t + 1], &mut bw);
    });
    let mut out = bw.finish();
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

fn segment(out: &mut Vec<u8>, m: u8, payload: &[u8]) {
    let n = payload.len() + 2;
    out.extend_from_slice(&[0xFF, m, (n >> 8) as u8, n as u8]);
    out.extend_from_slice(payload);
}

/// `m[u][x] = c(u) / 2 * cos((2x + 1) u pi / 16)`: the 1-D DCT of the JPEG standard.
fn dct_matrix() -> [[f32; 8]; 8] {
    let mut m = [[0f32; 8]; 8];
    for (u, row) in m.iter_mut().enumerate() {
        let c = if u == 0 { std::f64::consts::FRAC_1_SQRT_2 } else { 1.0 };
        for (x, v) in row.iter_mut().enumerate() {
            *v = (c / 2.0 * ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 16.0).cos()) as f32;
        }
    }
    m
}

struct Enc<'a> {
    img: &'a Image,
    gray: bool,
    /// The luma sampling factor: 2 (chroma 2x2 subsampled) or 1.
    sub: usize,
    qt: [[u16; 64]; 2],
    dct: [[f32; 8]; 8],
}

impl Enc<'_> {
    /// Calls `f(component, quantized block)` for every block in the order of the scan: MCU by
    /// MCU, the luma blocks of an MCU and then its Cb and Cr. The image is converted to YCbCr one
    /// MCU row at a time (unrounded), its edges extended by repeating the last column and row;
    /// subsampled chroma is the mean of 2x2 samples.
    fn blocks(&self, mut f: impl FnMut(usize, &[i32; 64])) {
        let (w, h, px) = (self.img.w, self.img.h, &self.img.px);
        let (sub, mcu) = (self.sub, 8 * self.sub);
        let (mx, my) = (w.div_ceil(mcu), h.div_ceil(mcu));
        let pw = mx * mcu;
        let nc = if self.gray { 1 } else { 3 };
        let mut planes = vec![vec![0f32; pw * mcu]; nc];
        let cw = pw / sub;
        let mut chroma = vec![vec![0f32; cw * 8]; if sub == 2 { nc - 1 } else { 0 }];
        let mut blk = [0f32; 64];
        let mut q = [0i32; 64];
        for row in 0..my {
            for y in 0..mcu {
                let sy = (row * mcu + y).min(h - 1);
                let src = &px[sy * w * 4..(sy + 1) * w * 4];
                for x in 0..pw {
                    let p = &src[x.min(w - 1) * 4..][..3];
                    let (r, g, b) = (p[0] as f32, p[1] as f32, p[2] as f32);
                    planes[0][y * pw + x] = 0.299 * r + 0.587 * g + 0.114 * b - 128.0;
                    if nc == 3 {
                        planes[1][y * pw + x] = -0.168_735_9 * r - 0.331_264_1 * g + 0.5 * b;
                        planes[2][y * pw + x] = 0.5 * r - 0.418_687_6 * g - 0.081_312_4 * b;
                    }
                }
            }
            for (c, dst) in chroma.iter_mut().enumerate() {
                let src = &planes[c + 1];
                for y in 0..8 {
                    for x in 0..cw {
                        let (a, b) = ((2 * y) * pw + 2 * x, (2 * y + 1) * pw + 2 * x);
                        dst[y * cw + x] = (src[a] + src[a + 1] + src[b] + src[b + 1]) * 0.25;
                    }
                }
            }
            for mcx in 0..mx {
                for by in 0..sub {
                    for bx in 0..sub {
                        gather(&planes[0], pw, mcx * mcu + bx * 8, by * 8, &mut blk);
                        self.fdct_quant(&blk, 0, &mut q);
                        f(0, &q);
                    }
                }
                for c in 1..nc {
                    let src = if sub == 2 { &chroma[c - 1] } else { &planes[c] };
                    gather(src, cw, mcx * 8, 0, &mut blk);
                    self.fdct_quant(&blk, 1, &mut q);
                    f(c, &q);
                }
            }
        }
    }

    /// The DCT of a level-shifted block, divided by quantization table `t` and rounded (AC terms
    /// with the dead zone).
    fn fdct_quant(&self, b: &[f32; 64], t: usize, out: &mut [i32; 64]) {
        let m = &self.dct;
        let mut tmp = [0f32; 64];
        for y in 0..8 {
            let row = &b[y * 8..y * 8 + 8];
            for u in 0..8 {
                tmp[y * 8 + u] = (0..8).map(|x| m[u][x] * row[x]).sum();
            }
        }
        for u in 0..8 {
            for (v, mv) in m.iter().enumerate() {
                let c: f32 = (0..8).map(|y| mv[y] * tmp[y * 8 + u]).sum();
                let i = v * 8 + u;
                let x = c / self.qt[t][i] as f32;
                let r = if i == 0 { x.round() } else { x.signum() * (x.abs() + 0.5 - DEAD_ZONE).floor() };
                let lim = if i == 0 { 1024 } else { 1023 };
                out[i] = (r as i32).clamp(-lim, lim);
            }
        }
    }
}

fn gather(src: &[f32], stride: usize, x0: usize, y0: usize, b: &mut [f32; 64]) {
    for y in 0..8 {
        b[y * 8..y * 8 + 8].copy_from_slice(&src[(y0 + y) * stride + x0..][..8]);
    }
}

/// The magnitude category of a value: its bit length.
fn cat(v: i32) -> usize {
    (32 - v.unsigned_abs().leading_zeros()) as usize
}

fn count(b: &[i32; 64], pred: &mut i32, dc: &mut [u64; 257], ac: &mut [u64; 257]) {
    dc[cat(b[0] - *pred)] += 1;
    *pred = b[0];
    let mut r = 0;
    for &z in &ZIGZAG[1..] {
        let v = b[z];
        if v == 0 {
            r += 1;
            continue;
        }
        while r > 15 {
            ac[0xF0] += 1;
            r -= 16;
        }
        ac[r << 4 | cat(v)] += 1;
        r = 0;
    }
    if r > 0 {
        ac[0] += 1;
    }
}

fn write(b: &[i32; 64], pred: &mut i32, dc: &Table, ac: &Table, w: &mut BitWriter) {
    let d = b[0] - *pred;
    *pred = b[0];
    let c = cat(d);
    w.put(dc.code[c], dc.size[c]);
    w.put(low_bits(d, c), c as u32);
    let mut r = 0;
    for &z in &ZIGZAG[1..] {
        let v = b[z];
        if v == 0 {
            r += 1;
            continue;
        }
        while r > 15 {
            w.put(ac.code[0xF0], ac.size[0xF0]);
            r -= 16;
        }
        let c = cat(v);
        let s = r << 4 | c;
        w.put(ac.code[s], ac.size[s]);
        w.put(low_bits(v, c), c as u32);
        r = 0;
    }
    if r > 0 {
        w.put(ac.code[0], ac.size[0]);
    }
}

/// The `c` extra bits of a value: itself, or for a negative one its one's complement.
fn low_bits(v: i32, c: usize) -> u32 {
    (if v < 0 { v - 1 } else { v }) as u32 & ((1u32 << c) - 1)
}

/// A Huffman table: the DHT form (counts per length, symbols) and the codes.
struct Table {
    bits: [u8; 16],
    vals: Vec<u8>,
    code: [u32; 256],
    size: [u32; 256],
}

impl Table {
    /// Code lengths from symbol frequencies, limited to 16 bits and without the all-ones code,
    /// as libjpeg's `jpeg_gen_optimal_table` makes them.
    fn optimal(freq: &[u64; 257]) -> Table {
        let mut freq = *freq;
        if freq[..256].iter().all(|&f| f == 0) {
            freq[0] = 1;
        }
        freq[256] = 1; // a reserved code, so no real code is all ones
        let mut size = [0usize; 257];
        let mut next = [usize::MAX; 257];
        loop {
            // the two least frequent (ties: the larger symbol)
            let mut c1 = 256;
            let mut v = u64::MAX;
            for (i, &f) in freq.iter().enumerate() {
                if f != 0 && f <= v {
                    (v, c1) = (f, i);
                }
            }
            let mut c2 = usize::MAX;
            v = u64::MAX;
            for (i, &f) in freq.iter().enumerate() {
                if f != 0 && f <= v && i != c1 {
                    (v, c2) = (f, i);
                }
            }
            if c2 == usize::MAX {
                break;
            }
            freq[c1] += freq[c2];
            freq[c2] = 0;
            let mut c = c1;
            size[c] += 1;
            while next[c] != usize::MAX {
                c = next[c];
                size[c] += 1;
            }
            next[c] = c2;
            let mut c = c2;
            size[c] += 1;
            while next[c] != usize::MAX {
                c = next[c];
                size[c] += 1;
            }
        }
        let mut bits = [0usize; 260];
        for &s in &size {
            if s > 0 {
                bits[s] += 1;
            }
        }
        for i in (17..260).rev() {
            while bits[i] > 0 {
                let mut j = i - 2;
                while bits[j] == 0 {
                    j -= 1;
                }
                bits[i] -= 2;
                bits[i - 1] += 1;
                bits[j + 1] += 2;
                bits[j] -= 1;
            }
        }
        let mut i = 16;
        while bits[i] == 0 {
            i -= 1;
        }
        bits[i] -= 1;
        let mut vals = Vec::new();
        for len in 1..260 {
            vals.extend((0..256).filter(|&s| size[s] == len).map(|s| s as u8));
        }
        let mut t = Table { bits: [0; 16], vals, code: [0; 256], size: [0; 256] };
        let (mut code, mut k) = (0u32, 0usize);
        for (len, &n) in bits.iter().enumerate().take(17).skip(1) {
            t.bits[len - 1] = n as u8;
            for _ in 0..n {
                let s = t.vals[k] as usize;
                (t.code[s], t.size[s]) = (code, len as u32);
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        t.vals.truncate(k);
        t
    }
}

struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    fn put(&mut self, code: u32, len: u32) {
        self.acc = self.acc << len | code as u64;
        self.n += len;
        while self.n >= 8 {
            self.n -= 8;
            let b = (self.acc >> self.n) as u8;
            self.out.push(b);
            if b == 0xFF {
                self.out.push(0);
            }
        }
    }

    /// Pads the last byte with one bits.
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            let k = 8 - self.n;
            self.put((1 << k) - 1, k);
        }
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::super::jpeg;
    use super::*;

    /// A test picture: gradients, a sharp-edged disc, a little pseudo-random noise.
    fn picture(w: usize, h: usize) -> Image {
        let mut img = Image::new(w, h, [0, 0, 0, 255]).unwrap();
        let mut seed = 7u32;
        for (i, p) in img.px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = ((i % w) as f64, (i / w) as f64);
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let n = (seed >> 28) as f64 - 8.0;
            let disc = (x - w as f64 * 0.6).powi(2) + (y - h as f64 * 0.4).powi(2) < (w.min(h) as f64 * 0.25).powi(2);
            let rgb =
                if disc { [230.0, 40.0, 50.0] } else { [255.0 * x / w as f64, 255.0 * y / h as f64, 128.0 + 60.0 * (x / 7.0).sin()] };
            for c in 0..3 {
                p[c] = (rgb[c] + n).clamp(0.0, 255.0) as u8;
            }
        }
        img
    }

    fn psnr(a: &Image, b: &Image) -> f64 {
        let se: f64 = a.px.iter().zip(&b.px).map(|(&x, &y)| (x as f64 - y as f64).powi(2)).sum();
        10.0 * (255.0f64.powi(2) / (se / (a.w * a.h * 3) as f64)).log10()
    }

    #[test]
    fn round_trips() {
        let img = picture(67, 45);
        for (q, min) in [(50, 26.0), (75, 28.0), (90, 31.0), (100, 38.0)] {
            let jpg = encode(&img, q);
            let back = jpeg::decode(&jpg, 0, 0).unwrap();
            assert_eq!((back.w, back.h), (67, 45));
            let p = psnr(&img, &back);
            assert!(p > min, "q{q}: {p:.1} dB");
            let info = jpeg::info(&jpg).unwrap();
            assert_eq!((info.components, info.progressive), (3, false));
        }
    }

    #[test]
    fn sizes_follow_quality() {
        let img = picture(120, 80);
        let sizes: Vec<usize> = [30, 60, 80, 94, 95, 98].iter().map(|&q| encode(&img, q).len()).collect();
        assert!(sizes.windows(2).all(|w| w[0] < w[1]), "{sizes:?}");
        // well under the raw size at the usual qualities
        assert!(sizes[2] < 120 * 80 * 3 / 5, "{sizes:?}");
    }

    #[test]
    fn is_deterministic() {
        let img = picture(41, 29);
        assert_eq!(encode(&img, 82), encode(&img, 82));
    }

    #[test]
    fn gray_images_have_one_component() {
        let mut img = picture(30, 20);
        for p in img.px.as_chunks_mut::<4>().0 {
            let g = p[1];
            p[..3].copy_from_slice(&[g, g, g]);
        }
        let jpg = encode(&img, 85);
        assert_eq!(jpeg::info(&jpg).unwrap().components, 1);
        let back = jpeg::decode(&jpg, 0, 0).unwrap();
        assert!(back.px.as_chunks::<4>().0.iter().all(|p| p[0] == p[1] && p[1] == p[2]));
        assert!(psnr(&img, &back) > 33.0);
    }

    #[test]
    fn small_and_odd_sizes() {
        for (w, h) in [(1, 1), (1, 17), (17, 1), (8, 8), (9, 9), (16, 16), (17, 33)] {
            let mut img = Image::new(w, h, [200, 60, 30, 255]).unwrap();
            if w * h > 1 {
                img.px[0] = 10;
            }
            for q in [75, 95] {
                let back = jpeg::decode(&encode(&img, q), 0, 0).unwrap();
                assert_eq!((back.w, back.h), (w, h));
                // the flat color survives away from the one odd pixel
                let p = &back.px[back.px.len() - 4..];
                assert!(p[0].abs_diff(200) < 12 && p[1].abs_diff(60) < 12 && p[2].abs_diff(30) < 12, "{w}x{h} q{q}: {p:?}");
            }
        }
        assert!(encode(&Image { w: 70000, h: 1, px: vec![0; 280000], icc: None }, 80).is_empty());
    }

    #[test]
    fn keeps_the_icc_profile() {
        let mut img = picture(20, 10);
        let icc: Vec<u8> = (0..150_000u32).map(|i| (i * 7 % 251) as u8).collect();
        img.icc = Some(icc.clone());
        let jpg = encode(&img, 80);
        assert_eq!(jpeg::decode(&jpg, 0, 0).unwrap().icc, Some(icc));
        assert!(jpeg::info(&jpg).unwrap().icc);
    }

    #[test]
    fn huffman_codes_fit_in_16_bits() {
        // Fibonacci frequencies make the deepest unlimited Huffman tree
        let mut freq = [0u64; 257];
        let (mut a, mut b) = (1u64, 1u64);
        for f in freq.iter_mut().take(40) {
            *f = a;
            (a, b) = (b, a + b);
        }
        let t = Table::optimal(&freq);
        assert_eq!(t.vals.len(), 40);
        assert!(t.size[..40].iter().all(|&s| (1..=16).contains(&s)));
        // a prefix code with room left for the reserved all-ones code
        let kraft: f64 = t.size[..40].iter().map(|&s| 0.5f64.powi(s as i32)).sum();
        assert!(kraft < 1.0, "{kraft}");
    }
}

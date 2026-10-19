//! Nyra's own image codec, with no crates: `nyra build` of a `use web` site decodes the images it
//! finds and writes resized WebP variants (and JPEG copies where WebP does not fit, like
//! `og:image`) without any outside tool.
//!
//! - Decoders: JPEG (`jpeg.rs`: baseline and progressive, any subsampling, EXIF orientation,
//!   CMYK, scaled decoding in the DCT), PNG (`png.rs` with `inflate.rs`), GIF (`gif.rs`, the first
//!   frame), BMP (`bmp.rs`) and WebP (`vp8_dec.rs` lossy, `vp8l_dec.rs` lossless, alpha).
//! - Resizing: `resize.rs` (Lanczos3 in linear light, premultiplied alpha).
//! - Encoders: lossy WebP (`vp8.rs`), lossless WebP (`vp8l.rs`, also for the alpha of lossy files),
//!   baseline JPEG (`jpeg_enc.rs`); `webp.rs` is the RIFF container.
//!
//! This file picks the decoder by the file's first bytes and chooses between lossy and lossless
//! WebP for each image.

// Codec code indexes blocks and planes by position; the index loops read like the format's spec.
#![allow(clippy::needless_range_loop)]

pub mod bmp;
pub mod common;
pub mod gif;
pub mod inflate;
pub mod jpeg;
pub mod jpeg_enc;
pub mod png;
pub mod resize;
pub mod vp8;
pub mod vp8_dec;
pub mod vp8_dsp;
pub mod vp8_tables;
pub mod vp8l;
pub mod vp8l_dec;
pub mod webp;

pub use common::{Image, Res};

/// The kind of an image file, by its first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Jpeg,
    Png,
    Gif,
    Bmp,
    Webp,
    Other,
}

pub fn kind(b: &[u8]) -> Kind {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Kind::Jpeg
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Kind::Png
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Kind::Gif
    } else if b.starts_with(b"BM") && b.len() > 26 {
        Kind::Bmp
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Kind::Webp
    } else {
        Kind::Other
    }
}

/// The size an image is shown at (for a JPEG with the EXIF orientation applied), from its headers.
pub fn size(b: &[u8]) -> Option<(usize, usize)> {
    match kind(b) {
        Kind::Jpeg => jpeg::info(b).ok().map(|i| (i.w, i.h)),
        _ => None,
    }
}

/// True for an animation (a GIF with several frames, a WebP with an `ANIM` chunk): the build keeps
/// such a file as it is.
pub fn animated(b: &[u8]) -> bool {
    match kind(b) {
        Kind::Gif => gif_frames(b) > 1,
        Kind::Webp => webp::chunks(b).is_ok_and(|c| c.iter().any(|c| &c.id == b"ANIM")),
        _ => false,
    }
}

/// The number of frames of a GIF (counted up to 2).
fn gif_frames(b: &[u8]) -> usize {
    let mut at = 13;
    if b.len() < 13 {
        return 0;
    }
    if b[10] & 0x80 != 0 {
        at += 3 << ((b[10] & 7) + 1);
    }
    let skip_blocks = |mut at: usize| -> Option<usize> {
        loop {
            let n = *b.get(at)? as usize;
            at += 1 + n;
            if n == 0 {
                return Some(at);
            }
        }
    };
    let mut frames = 0;
    while let Some(&t) = b.get(at) {
        match t {
            0x21 => match skip_blocks(at + 2) {
                Some(next) => at = next,
                None => break,
            },
            0x2C => {
                frames += 1;
                if frames > 1 {
                    break;
                }
                let Some(&flags) = b.get(at + 9) else { break };
                at += 10;
                if flags & 0x80 != 0 {
                    at += 3 << ((flags & 7) + 1);
                }
                match skip_blocks(at + 1) {
                    Some(next) => at = next,
                    None => break,
                }
            }
            _ => break,
        }
    }
    frames
}

/// Decodes an image. A JPEG is decoded at a reduced scale when the result is still at least
/// `want_w` wide (0: full size).
pub fn decode(b: &[u8], want_w: usize) -> Res<Image> {
    match kind(b) {
        Kind::Jpeg => {
            let want_h = match jpeg::info(b) {
                Ok(i) if want_w > 0 => (i.h as u64 * want_w as u64).div_ceil(i.w.max(1) as u64) as usize,
                _ => 0,
            };
            jpeg::decode(b, want_w, want_h)
        }
        Kind::Png => png::decode(b),
        Kind::Gif => gif::decode(b),
        Kind::Bmp => bmp::decode(b),
        Kind::Webp => decode_webp(b),
        Kind::Other => Err("not an image format the build reads (JPEG, PNG, GIF, BMP or WebP)".into()),
    }
}

fn decode_webp(b: &[u8]) -> Res<Image> {
    let chunks = webp::chunks(b)?;
    let find = |id: &[u8; 4]| chunks.iter().find(|c| &c.id == id).map(|c| c.data);
    if find(b"ANIM").is_some() {
        return Err("an animated WebP (it is kept as it is)".into());
    }
    let icc = find(b"ICCP").map(<[u8]>::to_vec);
    if let Some(d) = find(b"VP8L") {
        let (w, h, argb) = vp8l_dec::decode(d)?;
        let mut px = Vec::with_capacity(w * h * 4);
        for p in argb {
            px.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8]);
        }
        return Ok(Image { w, h, px, icc });
    }
    let d = find(b"VP8 ").ok_or("a WebP file without image data")?;
    let frame = vp8_dec::decode(d)?;
    let mut px = frame.to_rgba();
    if let Some(a) = find(b"ALPH") {
        let alpha = decode_alph(a, frame.w, frame.h)?;
        for (p, a) in px.as_chunks_mut::<4>().0.iter_mut().zip(alpha) {
            p[3] = a;
        }
    }
    Ok(Image { w: frame.w, h: frame.h, px, icc })
}

/// The alpha plane of an `ALPH` chunk: uncompressed or VP8L-compressed, then unfiltered.
fn decode_alph(d: &[u8], w: usize, h: usize) -> Res<Vec<u8>> {
    let (&head, data) = d.split_first().ok_or("an empty WebP alpha chunk")?;
    let mut a = match head & 3 {
        0 => data.get(..w * h).ok_or("truncated WebP alpha data")?.to_vec(),
        1 => vp8l_dec::decode_alpha(data, w, h)?,
        _ => return Err("unknown WebP alpha compression".into()),
    };
    let filter = (head >> 2) & 3;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let left = if x > 0 { Some(a[i - 1]) } else { None };
            let up = if y > 0 { Some(a[i - w]) } else { None };
            let pred = match (filter, left, up) {
                (0, _, _) => 0,
                (_, None, None) => 0,
                (1, Some(l), _) | (2, Some(l), None) | (3, Some(l), None) => l,
                (1, None, Some(u)) | (2, _, Some(u)) | (3, None, Some(u)) => u,
                (_, Some(l), Some(u)) => {
                    let c = a[i - w - 1] as i32;
                    (l as i32 + u as i32 - c).clamp(0, 255) as u8
                }
                _ => 0,
            };
            a[i] = a[i].wrapping_add(pred);
        }
    }
    Ok(a)
}

/// True if an image looks like a graphic (few colors or large flat areas), where lossless WebP
/// is worth trying.
fn graphic(img: &Image) -> bool {
    let px = img.px.as_chunks::<4>().0;
    let mut colors = std::collections::HashSet::new();
    let mut same = 0usize;
    for (i, p) in px.iter().enumerate() {
        if colors.len() <= 256 {
            colors.insert(*p);
        }
        if i > 0 && px[i - 1] == *p {
            same += 1;
        }
    }
    colors.len() <= 256 || same * 2 > px.len()
}

/// The image as a WebP: lossy (with a lossless alpha plane when it has transparency), or lossless
/// when it is a graphic and lossless is about as small (no smoothing of text and edges), or
/// smaller in any case for a small image. `quality` is 0..=100.
pub fn webp(img: &Image, quality: f32) -> Vec<u8> {
    let alpha = img.has_alpha();
    let yuv = vp8::Yuv::from_rgba(img.w, img.h, &img.px);
    let frame = vp8::encode(&yuv, &vp8::Options::new(quality));
    let alph = alpha.then(|| {
        let a: Vec<u8> = img.px.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
        let (filter, data) = vp8l::encode_alpha_filtered(img.w, img.h, &a);
        // VP8L compression (1), the filter in bits 2-3, no pre-processing
        let mut d = vec![1 | filter << 2];
        d.extend(data);
        d
    });
    let icc = img.icc.as_deref().filter(|p| !srgb_profile(p));
    let lossy = webp::lossy(img.w, img.h, &frame, alph.as_deref(), icc);
    let small = img.w * img.h <= 64 * 64;
    let graphic = graphic(img);
    if !(small || graphic) {
        return lossy;
    }
    let argb: Vec<u32> = img
        .px
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| (p[3] as u32) << 24 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32)
        .collect();
    let ll = webp::lossless(img.w, img.h, &vp8l::encode(img.w, img.h, &argb), alpha, icc);
    let limit = if graphic { lossy.len() + lossy.len() / 4 } else { lossy.len() };
    if ll.len() <= limit {
        ll
    } else {
        lossy
    }
}

/// The image as a baseline JPEG (transparency flattened on white). `quality` is 0..=100.
pub fn jpeg(img: &Image, quality: u8) -> Vec<u8> {
    if !img.has_alpha() {
        return jpeg_enc::encode(img, quality);
    }
    let mut flat = img.clone();
    for p in flat.px.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        for c in &mut p[..3] {
            *c = ((*c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        }
        p[3] = 255;
    }
    jpeg_enc::encode(&flat, quality)
}

/// A tiny blurred stand-in for an image (at most 24 px on its longer side, a lossy WebP of about
/// 100 to 300 bytes) to show while the image loads.
pub fn preview(img: &Image) -> Vec<u8> {
    let long = img.w.max(img.h).max(1);
    let (w, h) = (((img.w * 24).div_ceil(long)).max(1), ((img.h * 24).div_ceil(long)).max(1));
    let small = resize::resize(img, w, h);
    let yuv = vp8::Yuv::from_rgba(small.w, small.h, &small.px);
    let mut o = vp8::Options::new(40.0);
    o.segments = 1;
    webp::lossy(small.w, small.h, &vp8::encode(&yuv, &o), None, None)
}

/// True for an sRGB profile, which browsers assume anyway (it is not copied into the variants).
fn srgb_profile(p: &[u8]) -> bool {
    // the profile description tag is near the start; "sRGB" in the first 1 KB is enough
    p.len() < 64 || p[..p.len().min(1024)].windows(4).any(|w| w == b"sRGB")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo_like(w: usize, h: usize, alpha: bool) -> Image {
        let mut px = Vec::with_capacity(w * h * 4);
        let mut s = 1u32;
        for y in 0..h {
            for x in 0..w {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                let n = (s >> 28) as usize;
                let a = if alpha { ((x * 255) / w.max(1)) as u8 } else { 255 };
                px.extend_from_slice(&[(x * 3 + n) as u8, (y * 2 + n) as u8, ((x + y) + n) as u8, a]);
            }
        }
        Image { w, h, px, icc: None }
    }

    #[test]
    fn writes_webp_that_decodes_back() {
        for (w, h, alpha) in [(1, 1, false), (37, 23, false), (90, 70, true), (200, 120, false)] {
            let img = photo_like(w, h, alpha);
            let out = webp(&img, 80.0);
            assert_eq!(kind(&out), Kind::Webp);
            let back = decode(&out, 0).unwrap();
            assert_eq!((back.w, back.h), (w, h));
            let err: f64 = img.px.iter().zip(&back.px).map(|(a, b)| (*a as f64 - *b as f64).abs()).sum::<f64>() / img.px.len() as f64;
            assert!(err < 12.0, "{w}x{h}: mean error {err}");
            if alpha {
                // the alpha plane is lossless
                assert!(img.px.as_chunks::<4>().0.iter().zip(back.px.as_chunks::<4>().0).all(|(a, b)| a[3] == b[3]));
            }
        }
    }

    #[test]
    fn flat_graphics_become_lossless() {
        let mut img = Image::new(120, 80, [255, 255, 255, 255]).unwrap();
        for y in 20..60 {
            for x in 30..90 {
                img.px[(y * 120 + x) * 4..(y * 120 + x) * 4 + 4].copy_from_slice(&[200, 30, 40, 255]);
            }
        }
        let out = webp(&img, 80.0);
        assert!(webp::chunks(&out).unwrap().iter().any(|c| &c.id == b"VP8L"));
        assert_eq!(decode(&out, 0).unwrap().px, img.px);
    }

    #[test]
    fn writes_jpeg_and_previews() {
        let img = photo_like(64, 48, true);
        let j = jpeg(&img, 85);
        assert_eq!(kind(&j), Kind::Jpeg);
        let back = decode(&j, 0).unwrap();
        assert_eq!((back.w, back.h), (64, 48));
        let p = preview(&img);
        let back = decode(&p, 0).unwrap();
        assert_eq!((back.w, back.h), (24, 18));
        assert!(p.len() < 400, "{}", p.len());
    }

    #[test]
    fn finds_animations() {
        assert!(!animated(include_bytes!("testdata/gif-plain.gif")));
        let mut two = include_bytes!("testdata/gif-plain.gif").to_vec();
        // a second frame: the same image descriptor and data again before the trailer
        let start = two.iter().position(|&b| b == 0x2C).unwrap();
        let frame = two[start..two.len() - 1].to_vec();
        two.pop();
        two.extend_from_slice(&frame);
        two.push(0x3B);
        assert!(animated(&two));
    }

    #[test]
    fn corrupt_files_are_errors() {
        let img = photo_like(40, 30, true);
        for data in [webp(&img, 70.0), jpeg(&img, 70)] {
            for cut in (0..data.len()).step_by(7) {
                let _ = decode(&data[..cut], 0);
            }
            let mut d = data.clone();
            for i in (20..d.len()).step_by(11) {
                d[i] ^= 0x5a;
                let _ = decode(&d, 0);
            }
        }
        assert!(decode(b"hello", 0).is_err());
    }
}

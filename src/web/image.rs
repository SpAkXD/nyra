//! Images of a site: their width and height read from the file header (PNG, JPEG, GIF, BMP, WebP,
//! AVIF, SVG), and the WebP variants for `srcset`, made by the compiler's own codec (`img/`, see
//! [`builtin`]) or by an image tool the build is told to use (`cwebp`, `magick` or `vips`).

use std::path::Path;
use std::process::{Command, Stdio};

/// The widths of the WebP variants (only those smaller than the image are made).
pub const WIDTHS: [u32; 4] = [480, 800, 1200, 1600];

/// The kind of an image file, by its extension.
pub fn is_image(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "svg" | "ico")
}

/// A raster image that a tool can turn into WebP variants (not a vector, an icon or an animation).
pub fn resizable(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    matches!(ext, "png" | "jpg" | "jpeg" | "webp")
}

/// A raster image the built-in codec turns into WebP variants (it also reads BMP and the first
/// frame of a GIF; an animated GIF or WebP is kept as it is).
pub fn decodable(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    matches!(ext, "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
}

/// Width and height from the first bytes of an image file.
pub fn size(bytes: &[u8]) -> Option<(u32, u32)> {
    let be16 = |i: usize| -> Option<u32> { Some(u16::from_be_bytes([*bytes.get(i)?, *bytes.get(i + 1)?]) as u32) };
    let le16 = |i: usize| -> Option<u32> { Some(u16::from_le_bytes([*bytes.get(i)?, *bytes.get(i + 1)?]) as u32) };
    let be32 = |i: usize| -> Option<u32> { Some(u32::from_be_bytes(bytes.get(i..i + 4)?.try_into().ok()?)) };
    let le24 = |i: usize| -> Option<u32> { Some(u32::from_le_bytes([*bytes.get(i)?, *bytes.get(i + 1)?, *bytes.get(i + 2)?, 0])) };
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(16)?, be32(20)?));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some((le16(6)?, le16(8)?));
    }
    if bytes.starts_with(b"BM") && bytes.len() > 26 {
        let le32 = |i: usize| -> Option<i32> { Some(i32::from_le_bytes(bytes.get(i..i + 4)?.try_into().ok()?)) };
        return match le32(14)? {
            12 => Some((le16(18)?, le16(20)?)),
            _ => Some((le32(18)?.unsigned_abs(), le32(22)?.unsigned_abs())),
        };
    }
    #[cfg(not(target_arch = "wasm32"))]
    if bytes.starts_with(&[0xFF, 0xD8]) {
        // JPEG: the size as shown, after the EXIF orientation
        if let Some((w, h)) = super::img::size(bytes) {
            return Some((w as u32, h as u32));
        }
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        // JPEG: walk the segments to the first start-of-frame
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if marker == 0xFF {
                i += 1;
                continue;
            }
            let len = be16(i + 2)? as usize;
            let sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
            if sof {
                return Some((be16(i + 7)?, be16(i + 5)?));
            }
            i += 2 + len;
        }
        return None;
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return match bytes.get(12..16)? {
            b"VP8 " => Some((le16(26)? & 0x3FFF, le16(28)? & 0x3FFF)),
            b"VP8L" => {
                let b = |i: usize| bytes.get(i).map(|&x| x as u32);
                let (b1, b2, b3, b4) = (b(21)?, b(22)?, b(23)?, b(24)?);
                Some((1 + (((b2 & 0x3F) << 8) | b1), 1 + (((b4 & 0xF) << 10) | (b3 << 2) | ((b2 & 0xC0) >> 6))))
            }
            b"VP8X" => Some((1 + le24(24)?, 1 + le24(27)?)),
            _ => None,
        };
    }
    if bytes.get(4..8) == Some(b"ftyp") && bytes.get(8..12).is_some_and(|b| b == b"avif" || b == b"avis") {
        // the `ispe` box holds the size
        let at = bytes.windows(4).position(|w| w == b"ispe")?;
        return Some((be32(at + 8)?, be32(at + 12)?));
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    if let Some(at) = head.find("<svg") {
        return svg_size(&head[at..]);
    }
    None
}

/// `width` and `height` of the `<svg>` tag (in px or plain numbers), else its `viewBox`.
fn svg_size(text: &str) -> Option<(u32, u32)> {
    let end = text.find('>')?;
    let tag = super::html::tags(&text[..=end]).into_iter().next()?;
    let num = |v: &str| -> Option<u32> {
        let v = v.trim().trim_end_matches("px");
        v.parse::<f64>().ok().filter(|n| *n > 0.0).map(|n| n.round() as u32)
    };
    if let (Some(w), Some(h)) = (tag.get("width").and_then(num), tag.get("height").and_then(num)) {
        return Some((w, h));
    }
    let vb: Vec<f64> = tag.get("viewbox")?.split([' ', ',']).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect();
    match vb.as_slice() {
        [_, _, w, h] if *w > 0.0 && *h > 0.0 => Some((w.round() as u32, h.round() as u32)),
        _ => None,
    }
}

/// The outside image tools the build can use instead of its own codec (`--image-tool`).
pub const TOOLS: [&str; 3] = ["cwebp", "magick", "vips"];

/// How the build makes image variants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tool {
    /// The compiler's own codec (the default).
    Builtin,
    /// An outside tool: its name or path.
    External(String),
}

/// The tool for `--image-tool` (`none`: no variants; `builtin` or nothing: the built-in codec).
pub fn find_tool(choice: Option<&str>) -> Option<Tool> {
    match choice {
        Some("none") => None,
        None | Some("builtin") => Some(Tool::Builtin),
        Some(name) => Some(Tool::External(name.to_string())),
    }
}

// ---- the built-in codec ------------------------------------------------------------------------------

/// The widest variant (and the widest a replaced original may be).
pub const MAX_WIDTH: usize = 2560;
/// The quality of the WebP variants (libwebp's scale) and of JPEG copies.
const WEBP_QUALITY: f32 = 80.0;
const JPEG_QUALITY: u8 = 85;
/// Part of each cache file's name: a new one when the codec's output changes.
const CODEC_VERSION: &str = "c1";

/// What the built-in codec makes of one image.
#[derive(Default, Debug)]
pub struct Made {
    /// WebP variants (width, bytes), narrowest first.
    pub variants: Vec<(u32, Vec<u8>)>,
    /// The original file stays the widest `srcset` entry (a WebP of its size was not smaller).
    pub original_top: bool,
    /// The original is not copied (a BMP, or wider than [`MAX_WIDTH`]): the widest variant takes
    /// its place.
    pub replace: bool,
    /// A JPEG of at most 1200 px for `og:image`, when the original is not a JPEG or PNG of a fitting
    /// size.
    pub og: Option<Vec<u8>>,
    /// A tiny WebP (at most 24 px) shown blurred while the image loads; none for an image with
    /// transparency.
    pub preview: Option<Vec<u8>>,
}

/// The cache of encoded files, by the content of the source, so a rebuild does not encode again.
#[cfg(not(target_arch = "wasm32"))]
fn cache_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("nyra").join("web-img")
}

/// The variants of an image made by the built-in codec (read from the cache when it has them).
/// `for_meta`: the image is also an `og:image`. An animation gives no variants (it is kept as it
/// is); an error says why the file could not be read.
#[cfg(not(target_arch = "wasm32"))]
pub fn builtin(bytes: &[u8], for_meta: bool) -> Result<Made, String> {
    use super::img::{self, Kind};
    if img::animated(bytes) {
        return Ok(Made::default());
    }
    let kind = img::kind(bytes);
    if kind == Kind::Other {
        return Err("not a JPEG, PNG, GIF, BMP or WebP file".into());
    }
    let (w, _) = size(bytes).ok_or("its header has no size")?;
    let w = w as usize;
    let top = w.min(MAX_WIDTH);
    // the standard widths below the widest, skipping one that is almost as wide
    let mut widths: Vec<usize> = WIDTHS.iter().map(|&x| x as usize).chain([2400]).filter(|&x| x < top && x * 100 <= top * 85).collect();
    widths.push(top);
    let replace = kind == Kind::Bmp || w > MAX_WIDTH;
    let og_needed = for_meta && (!matches!(kind, Kind::Jpeg | Kind::Png) || w > 2400 || bytes.len() > 1_500_000);

    let key = super::hash(bytes);
    let dir = cache_dir();
    let names: Vec<String> = widths.iter().map(|vw| format!("{key}-{vw}-{CODEC_VERSION}.webp")).collect();
    let og_name = format!("{key}-og-{CODEC_VERSION}.jpg");
    let ph_name = format!("{key}-ph-{CODEC_VERSION}.webp");
    let mut outs: Vec<Option<Vec<u8>>> = names.iter().map(|n| std::fs::read(dir.join(n)).ok()).collect();
    let mut og = if og_needed { std::fs::read(dir.join(&og_name)).ok() } else { None };
    // (an empty cached preview: the image has transparency and gets none)
    let mut preview = std::fs::read(dir.join(&ph_name)).ok();
    if outs.iter().any(Option::is_none) || (og_needed && og.is_none()) || preview.is_none() {
        let src = img::decode(bytes, top)?;
        let opaque = !src.has_alpha();
        let (sw, sh) = (src.w, src.h);
        let mut pyramid = img::resize::Pyramid::new(src);
        if preview.is_none() {
            let data = if opaque { img::preview(&pyramid.width((sw * 24).div_ceil(sw.max(sh)).max(1))) } else { Vec::new() };
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join(&ph_name), &data);
            preview = Some(data);
        }
        // the sizes to encode, widest first; then each is encoded on its own thread
        let mut jobs: Vec<(Option<usize>, img::Image)> = Vec::new();
        for (i, &vw) in widths.iter().enumerate().rev() {
            if outs[i].is_none() {
                jobs.push((Some(i), pyramid.width(vw)));
            }
        }
        if og_needed && og.is_none() {
            jobs.push((None, pyramid.width(w.min(1200))));
        }
        drop(pyramid);
        let done: Vec<(Option<usize>, Vec<u8>)> = std::thread::scope(|s| {
            let handles: Vec<_> = jobs
                .iter()
                .map(|(i, im)| s.spawn(move || (*i, if i.is_some() { img::webp(im, WEBP_QUALITY) } else { img::jpeg(im, JPEG_QUALITY) })))
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        let _ = std::fs::create_dir_all(&dir);
        for (i, data) in done {
            let name = match i {
                Some(i) => &names[i],
                None => &og_name,
            };
            // write then rename, so a parallel build never reads half a file
            let part = dir.join(format!("{name}.{}.partial", std::process::id()));
            if std::fs::write(&part, &data).is_ok() && std::fs::rename(&part, dir.join(name)).is_err() {
                let _ = std::fs::remove_file(&part);
            }
            match i {
                Some(i) => outs[i] = Some(data),
                None => og = Some(data),
            }
        }
    }
    let mut all: Vec<(u32, Vec<u8>)> = Vec::with_capacity(widths.len());
    for (vw, data) in widths.iter().zip(outs) {
        all.push((*vw as u32, data.ok_or("the encoder stopped")?));
    }
    // the original stays the widest entry when its WebP is not smaller
    let mut original_top = false;
    if !replace && all.last().is_some_and(|t| t.1.len() >= bytes.len()) {
        all.pop();
        original_top = true;
    }
    // a variant that is not smaller than the next wider one is left out
    let mut limit = if original_top { bytes.len() } else { usize::MAX };
    let mut variants = Vec::with_capacity(all.len());
    for v in all.into_iter().rev() {
        if v.1.len() < limit {
            limit = v.1.len();
            variants.push(v);
        }
    }
    variants.reverse();
    Ok(Made { variants, original_top, replace, og, preview: preview.filter(|p| !p.is_empty()) })
}

/// The WebAssembly build (the playground) does not convert images.
#[cfg(target_arch = "wasm32")]
pub fn builtin(_: &[u8], _: bool) -> Result<Made, String> {
    Err("the playground does not convert images".into())
}

/// Writes a WebP of `src` scaled to `width` px to `out` with `tool`. False if the tool failed.
pub fn webp(tool: &str, src: &Path, width: u32, out: &Path) -> bool {
    let mut cmd = Command::new(tool);
    let name = Path::new(tool).file_stem().and_then(|s| s.to_str()).unwrap_or(tool);
    match name {
        "cwebp" => {
            cmd.args(["-quiet", "-q", "80", "-resize", &width.to_string(), "0"]).arg(src).arg("-o").arg(out);
        }
        "magick" => {
            cmd.arg(src).args(["-resize", &format!("{width}x"), "-quality", "80"]).arg(out);
        }
        "vips" => {
            cmd.arg("thumbnail").arg(src).arg(format!("{}[Q=80]", out.display())).arg(width.to_string());
        }
        _ => return false,
    }
    let ok = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    ok && out.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sizes_from_headers() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(size(&png), Some((640, 480)));
        assert_eq!(size(b"GIF89a\x20\x03\x58\x02rest"), Some((800, 600)));
        // JPEG: SOI, an APP0 segment, then SOF0 with height 300 and width 400
        let jpeg =
            [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0x2C, 0x01, 0x90, 0x03, 0, 0, 0, 0];
        assert_eq!(size(&jpeg), Some((400, 300)));
        let mut vp8x = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
        vp8x.extend_from_slice(&[0x1F, 0x03, 0x00, 0x57, 0x02, 0x00]);
        assert_eq!(size(&vp8x), Some((800, 600)));
        assert_eq!(size(b"<?xml version=\"1.0\"?><svg xmlns=\"x\" width=\"24px\" height=\"12\">"), Some((24, 12)));
        assert_eq!(size(b"<svg viewBox=\"0 0 100 50\">"), Some((100, 50)));
        assert_eq!(size(b"not an image"), None);
    }

    #[test]
    fn knows_image_kinds() {
        assert!(is_image("a/B.JPG") && is_image("x.svg") && !is_image("a.css"));
        assert!(resizable("a.png") && !resizable("a.svg") && !resizable("a.gif"));
    }
}

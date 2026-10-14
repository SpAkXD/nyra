//! Images of a site: their width and height read from the file header (PNG, JPEG, GIF, WebP,
//! AVIF, SVG; the compiler has no crates and decodes nothing), and WebP variants for `srcset` made
//! by an image tool on the PATH (`cwebp`, `magick` or `vips`), when there is one.

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

/// The image tools the build knows, in the order it tries them.
pub const TOOLS: [&str; 3] = ["cwebp", "magick", "vips"];

/// The image tool to use: `choice` (`--image-tool`: a name or `none`), else the first one on the PATH.
pub fn find_tool(choice: Option<&str>) -> Option<String> {
    match choice {
        Some("none") => None,
        Some(name) => Some(name.to_string()),
        None => TOOLS.iter().find(|t| runs(t)).map(|t| t.to_string()),
    }
}

fn runs(tool: &str) -> bool {
    let arg = match tool {
        "cwebp" => "-version",
        "magick" => "-version",
        _ => "--version",
    };
    Command::new(tool).arg(arg).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
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

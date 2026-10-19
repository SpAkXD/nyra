//! The WebP container (RIFF): writing lossy (`VP8 `, with an `ALPH` chunk for transparency) and
//! lossless (`VP8L`) files with an optional ICC profile, and reading the chunks of a file.

use super::common::Res;

/// One RIFF chunk: its four-character code and payload.
pub struct Chunk<'a> {
    pub id: [u8; 4],
    pub data: &'a [u8],
}

fn push_chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}

fn riff(body: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 12);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(body.len() as u32 + 4).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    out
}

/// The `VP8X` chunk payload: the feature flags and the canvas size.
fn vp8x(w: usize, h: usize, icc: bool, alpha: bool) -> [u8; 10] {
    let mut x = [0u8; 10];
    x[0] = if icc { 0x20 } else { 0 } | if alpha { 0x10 } else { 0 };
    x[4..7].copy_from_slice(&((w - 1) as u32).to_le_bytes()[..3]);
    x[7..10].copy_from_slice(&((h - 1) as u32).to_le_bytes()[..3]);
    x
}

/// A lossy WebP: the VP8 frame, the alpha plane (an `ALPH` payload, header byte included) and the
/// ICC profile when there are any.
pub fn lossy(w: usize, h: usize, vp8: &[u8], alph: Option<&[u8]>, icc: Option<&[u8]>) -> Vec<u8> {
    let mut body = Vec::with_capacity(vp8.len() + alph.map_or(0, |a| a.len()) + 64);
    if alph.is_some() || icc.is_some() {
        push_chunk(&mut body, b"VP8X", &vp8x(w, h, icc.is_some(), alph.is_some()));
        if let Some(p) = icc {
            push_chunk(&mut body, b"ICCP", p);
        }
        if let Some(a) = alph {
            push_chunk(&mut body, b"ALPH", a);
        }
    }
    push_chunk(&mut body, b"VP8 ", vp8);
    riff(body)
}

/// A lossless WebP of a `VP8L` payload.
pub fn lossless(w: usize, h: usize, vp8l: &[u8], alpha: bool, icc: Option<&[u8]>) -> Vec<u8> {
    let mut body = Vec::with_capacity(vp8l.len() + 64);
    if let Some(p) = icc {
        push_chunk(&mut body, b"VP8X", &vp8x(w, h, true, alpha));
        push_chunk(&mut body, b"ICCP", p);
    }
    push_chunk(&mut body, b"VP8L", vp8l);
    riff(body)
}

/// The chunks of a WebP file, after checking the RIFF header.
pub fn chunks(data: &[u8]) -> Res<Vec<Chunk<'_>>> {
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return Err("not a WebP file".into());
    }
    let size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let end = size.saturating_add(8).min(data.len());
    let mut out = Vec::new();
    let mut at = 12;
    while at + 8 <= end {
        let id = [data[at], data[at + 1], data[at + 2], data[at + 3]];
        let len = u32::from_le_bytes([data[at + 4], data[at + 5], data[at + 6], data[at + 7]]) as usize;
        let start = at + 8;
        let stop = start.checked_add(len).filter(|&s| s <= data.len()).ok_or("truncated WebP chunk")?;
        out.push(Chunk { id, data: &data[start..stop] });
        at = stop + (len & 1);
    }
    if out.is_empty() {
        return Err("a WebP file without chunks".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_chunks() {
        let f = lossy(3, 2, &[1, 2, 3], Some(&[9]), Some(&[7, 7]));
        let c = chunks(&f).unwrap();
        let ids: Vec<&[u8; 4]> = c.iter().map(|c| &c.id).collect();
        assert_eq!(ids, [b"VP8X", b"ICCP", b"ALPH", b"VP8 "]);
        assert_eq!(c[0].data, &[0x30, 0, 0, 0, 2, 0, 0, 1, 0, 0]);
        assert_eq!(c[3].data, &[1, 2, 3]);
        assert_eq!(f.len() % 2, 0);
        assert!(chunks(&f[..f.len() - 3]).is_err());
        assert!(chunks(b"RIFF\0\0\0\0WEBQ").is_err());
    }
}

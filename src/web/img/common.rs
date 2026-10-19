//! What the parts of the image codec share: the decoded image and the error type.

/// An error of a decoder: a sentence for the build's warning ("truncated PNG data").
pub type Res<T> = Result<T, String>;

/// The most pixels a decoder allocates for one image (about 480 MB of RGBA). A file that claims
/// more is refused with an error, so a hostile header cannot exhaust the memory.
pub const MAX_PIXELS: usize = 120_000_000;

/// A decoded image: RGBA, 8 bits per channel, straight (not premultiplied) alpha, rows top to
/// bottom, `px.len() == w * h * 4`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
    /// The file's ICC color profile, when it has one (for JPEG: the APP2 chunks joined).
    pub icc: Option<Vec<u8>>,
}

impl Image {
    /// A new image of `w` x `h` pixels, all `rgba`. An error over [`MAX_PIXELS`] or for a zero size.
    #[cfg(test)]
    pub fn new(w: usize, h: usize, rgba: [u8; 4]) -> Res<Image> {
        check_size(w, h)?;
        Ok(Image { w, h, px: rgba.repeat(w * h), icc: None })
    }

    /// True if some pixel is not fully opaque.
    pub fn has_alpha(&self) -> bool {
        self.px.as_chunks::<4>().0.iter().any(|p| p[3] != 255)
    }
}

/// An error if `w` x `h` is empty or more than [`MAX_PIXELS`].
pub fn check_size(w: usize, h: usize) -> Res<()> {
    if w == 0 || h == 0 {
        return Err("the image has no pixels (a width or height of 0)".into());
    }
    if w.checked_mul(h).is_none_or(|n| n > MAX_PIXELS) {
        return Err(format!("the image is too large ({w} x {h} pixels; the limit is {} million)", MAX_PIXELS / 1_000_000));
    }
    Ok(())
}

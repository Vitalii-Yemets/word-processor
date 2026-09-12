//! Decoding the image formats a document can carry.
//!
//! A `.docx` embeds its pictures as ordinary files inside the package, most
//! often PNG or JPEG and, in anything old enough, a Windows bitmap, a GIF, a
//! scan in TIFF, or a drawing recorded as a metafile.
//! None can be shown without being decoded, so the decoders are here — written
//! against the specifications rather than taken from a library, like everything
//! else in this project.
//!
//! # What comes out
//!
//! Always eight-bit RGBA, whatever went in. A drawing surface should not have
//! to know that one picture was greyscale, another palettised and a third
//! subsampled: those are facts about how a file was written, not about what it
//! shows.

#![forbid(unsafe_code)]

pub mod bmp;
pub mod emf;
pub mod fax;
pub mod gif;
pub mod jpeg;
pub mod metafile;
pub mod png;
pub mod tiff;
pub mod wmf;

/// A decoded picture: eight-bit RGBA, top row first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// Four bytes per pixel, `width * height * 4` in all.
    pub pixels: Vec<u8>,
}

impl Image {
    /// A picture of a given size, filled with transparent black.
    #[must_use]
    pub fn empty(width: usize, height: usize) -> Self {
        Self { width, height, pixels: vec![0; width * height * 4] }
    }

    /// Whether the picture has no pixels at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Why a picture could not be decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The bytes are not a picture in any format this program reads.
    UnknownFormat,
    /// The file ends before the picture does.
    Truncated,
    /// The file says something the format does not allow.
    Malformed(&'static str),
    /// A valid picture using something this decoder does not implement.
    Unsupported(&'static str),
    /// A picture too large to be a picture: almost certainly a damaged header.
    TooLarge,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownFormat => f.write_str("not a picture format this program reads"),
            Self::Truncated => f.write_str("the file ends before the picture does"),
            Self::Malformed(what) => write!(f, "the picture is malformed: {what}"),
            Self::Unsupported(what) => write!(f, "the picture uses {what}, which is not read yet"),
            Self::TooLarge => f.write_str("the picture claims a size beyond any real picture"),
        }
    }
}

impl std::error::Error for Error {}

/// The largest picture that will be decoded, in pixels.
///
/// A header is four bytes of width and four of height, and a damaged one can
/// ask for a hundred million pixels as easily as a hundred. The limit is far
/// past any picture in a document and far short of exhausting memory.
pub const MAX_PIXELS: usize = 64 * 1024 * 1024;

/// Which format a file is in, from the bytes it starts with.
///
/// Read from the content rather than from the file name: a document names its
/// parts however it likes, and a picture that says `.png` and holds a JPEG is
/// something a real package does contain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Bmp,
    Gif,
    Tiff,
    /// The two metafile formats, which are recordings of drawing rather than
    /// pictures. See [`metafile`].
    Emf,
    Wmf,
}

impl Format {
    /// Recognises a format from the start of a file.
    #[must_use]
    pub fn detect(data: &[u8]) -> Option<Self> {
        if data.starts_with(&png::SIGNATURE) {
            return Some(Self::Png);
        }
        // Every JPEG begins with the start-of-image marker.
        if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(Self::Jpeg);
        }
        if data.starts_with(&gif::SIGNATURE_87) || data.starts_with(&gif::SIGNATURE_89) {
            return Some(Self::Gif);
        }
        // Two letters saying which way round the numbers are, and then the
        // number forty-two either way round.
        if data.starts_with(&tiff::LITTLE) || data.starts_with(&tiff::BIG) {
            return Some(Self::Tiff);
        }
        // Two letters and nothing else, which is as much as a bitmap says
        // about itself. The header after them is checked by the decoder.
        if data.starts_with(&bmp::SIGNATURE) {
            return Some(Self::Bmp);
        }
        // The metafiles say what they are further in than the others do: one
        // through a signature forty bytes along, the other through a header
        // that has to be recognised by its shape. Both are asked last, so that
        // nothing which says outright what it is can be taken for one.
        if emf::is_emf(data) {
            return Some(Self::Emf);
        }
        if wmf::is_wmf(data) {
            return Some(Self::Wmf);
        }
        None
    }
}

/// Decodes a picture, whichever of the formats it is in.
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    match Format::detect(data) {
        Some(Format::Png) => png::decode(data),
        Some(Format::Jpeg) => jpeg::decode(data),
        Some(Format::Bmp) => bmp::decode(data),
        Some(Format::Gif) => gif::decode(data),
        Some(Format::Tiff) => tiff::decode(data),
        Some(Format::Emf) => emf::decode(data),
        Some(Format::Wmf) => wmf::decode(data),
        None => Err(Error::UnknownFormat),
    }
}

/// Checks a size before anything is allocated for it.
pub(crate) fn check_size(width: usize, height: usize) -> Result<(), Error> {
    if width == 0 || height == 0 {
        return Err(Error::Malformed("a picture with no width or no height"));
    }
    if width.checked_mul(height).is_none_or(|pixels| pixels > MAX_PIXELS) {
        return Err(Error::TooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_recognised_in_nothing() {
        assert_eq!(Format::detect(&[]), None);
        assert_eq!(Format::detect(b"not a picture"), None);
    }

    #[test]
    fn a_png_is_recognised_by_its_signature() {
        assert_eq!(Format::detect(&png::SIGNATURE), Some(Format::Png));
    }

    #[test]
    fn a_jpeg_is_recognised_by_its_first_marker() {
        assert_eq!(Format::detect(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(Format::Jpeg));
    }

    #[test]
    fn a_bitmap_is_recognised_by_its_two_letters() {
        assert_eq!(Format::detect(b"BM and whatever follows"), Some(Format::Bmp));
        assert_eq!(Format::detect(b"MB"), None);
    }

    #[test]
    fn both_versions_of_gif_are_recognised() {
        assert_eq!(Format::detect(b"GIF87a and the rest"), Some(Format::Gif));
        assert_eq!(Format::detect(b"GIF89a and the rest"), Some(Format::Gif));
        assert_eq!(Format::detect(b"GIF99a"), None);
    }

    #[test]
    fn a_metafile_is_recognised_by_what_it_says_further_in() {
        // A metafile says what it is forty bytes along rather than at the
        // front, so a file that is one is only known after the others have
        // been asked.
        let mut emf = vec![0u8; 88];
        emf[0..4].copy_from_slice(&1u32.to_le_bytes());
        emf[40..44].copy_from_slice(&emf::SIGNATURE);
        assert_eq!(Format::detect(&emf), Some(Format::Emf));

        let mut wmf = vec![0u8; 32];
        wmf[0..4].copy_from_slice(&wmf::PLACEABLE);
        assert_eq!(Format::detect(&wmf), Some(Format::Wmf));
    }

    #[test]
    fn a_tiff_is_recognised_either_way_round() {
        assert_eq!(Format::detect(b"II\x2A\x00 and the rest"), Some(Format::Tiff));
        assert_eq!(Format::detect(b"MM\x00\x2A and the rest"), Some(Format::Tiff));
        // The letters without the forty-two after them are not a picture.
        assert_eq!(Format::detect(b"II\x00\x00"), None);
    }

    #[test]
    fn a_size_of_nothing_is_refused() {
        assert!(check_size(0, 10).is_err());
        assert!(check_size(10, 0).is_err());
    }

    #[test]
    fn an_impossible_size_is_refused_before_anything_is_allocated() {
        assert_eq!(check_size(usize::MAX, 2), Err(Error::TooLarge));
        assert_eq!(check_size(100_000, 100_000), Err(Error::TooLarge));
    }

    #[test]
    fn an_ordinary_size_is_accepted() {
        assert!(check_size(1920, 1080).is_ok());
    }
}

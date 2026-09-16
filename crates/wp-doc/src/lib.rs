//! The binary Word document: `.doc`, as Word wrote it from 1997 to 2003 and
//! can still write today.
//!
//! # Why it is worth reading
//!
//! Because a twenty-year-old document is still a document. Every office has
//! a drawer of them, and a word processor that cannot open them is not a
//! word processor. The format is [MS-DOC] over [MS-CFB]: a file system in
//! a file, a table of contents at the start of its main stream, text in
//! pieces, formatting in pages of sprms, and everything else in tables of
//! its own. [`wp_ole`] reads the file system, [`fib`] the table of contents,
//! [`sprm`] the modifiers, and [`read`] puts the document together.
//!
//! # What is here
//!
//! Reading: the text however it was saved, one byte or two a character, in
//! pieces or not; paragraphs with their formatting and their styles; runs
//! with theirs; tables; bulleted and numbered lists; links from fields;
//! pictures — bitmaps and metafiles alike — from the data stream and the
//! drawing store; the page size and margins. Not writing: a Word 97-2003
//! file this program wrote could only be checked by Word, and is named in
//! the roadmap as its own item.

#![forbid(unsafe_code)]

pub mod fib;
mod read;
pub mod sprm;

pub use read::{read, LinkFound, PictureFound, Reading, PICTURE_MARK};

use wp_docx::{Document, TextPosition};

/// Why a file could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a compound file at all.
    NotCompound,
    /// A compound file, but not a Word document.
    NotWord,
    /// A document that is encrypted, which this program does not open.
    Encrypted,
    /// The file ends before the structure named does.
    Truncated(&'static str),
    /// The structure named is not as the format says.
    Malformed(&'static str),
    /// The document could not be built from what was read.
    Document(String),
}

impl From<wp_ole::Error> for Error {
    /// A compound file that will not open is a document that will not open,
    /// and the three things that can be wrong with one are the same three.
    fn from(error: wp_ole::Error) -> Self {
        match error {
            wp_ole::Error::NotCompound => Self::NotCompound,
            wp_ole::Error::Truncated(what) => Self::Truncated(what),
            wp_ole::Error::Malformed(what) => Self::Malformed(what),
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotCompound => write!(f, "not a compound file"),
            Self::NotWord => write!(f, "not a Word document"),
            Self::Encrypted => write!(f, "the document is encrypted"),
            Self::Truncated(what) => write!(f, "the file ends inside its {what}"),
            Self::Malformed(what) => write!(f, "the file's {what} is not as the format says"),
            Self::Document(what) => write!(f, "{what}"),
        }
    }
}

impl std::error::Error for Error {}

/// Opens a binary document: everything [`read`] found, with the pictures
/// put in where their marks were, the links laid over their text, and the
/// page set up as the first section says.
pub fn open(bytes: &[u8]) -> Result<Document, Error> {
    let reading = read(bytes)?;
    let mut document =
        Document::create(&reading.body).map_err(|error| Error::Document(error.to_string()))?;

    let mut pictures = reading.pictures;
    pictures.sort_by_key(|one| std::cmp::Reverse((one.paragraph, one.offset)));
    let mut links = reading.links;
    for picture in pictures {
        let start = TextPosition::new(picture.paragraph, picture.offset);
        let end = TextPosition::new(picture.paragraph, picture.offset + PICTURE_MARK.len_utf8());
        let before = document.paragraph_text(picture.paragraph).map_or(0, |text| text.len());
        document.set_caret(start);
        document.extend_selection_to(end);
        document.delete_selection();
        document.set_caret(start);
        let _ = document.insert_picture(
            &picture.bytes,
            picture.extension,
            picture.width_emu,
            picture.height_emu,
        );
        let after = document.paragraph_text(picture.paragraph).map_or(0, |text| text.len());
        for link in &mut links {
            if link.paragraph == picture.paragraph && link.start >= picture.offset {
                link.start = (link.start + after).saturating_sub(before);
                link.end = (link.end + after).saturating_sub(before);
            }
        }
    }
    for link in links {
        if link.end <= link.start {
            continue;
        }
        document.set_caret(TextPosition::new(link.paragraph, link.start));
        document.extend_selection_to(TextPosition::new(link.paragraph, link.end));
        document.add_hyperlink(&link.address, "");
    }
    if let Some((width, height, [top, right, bottom, left])) = reading.page {
        if width > 0 && height > 0 {
            document.set_page_size(width, height);
            document.set_page_margins(top, right, bottom, left);
        }
    }

    document.set_caret(TextPosition::default());
    document.clear_selection();
    let _ = document.mark_saved();
    Ok(document)
}

/// Whether bytes are a compound file, which is what a `.doc` is — and what
/// some files with other names are too.
#[must_use]
pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
}

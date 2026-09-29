//! The binary Word document: `.doc`, as Word wrote it from 1993 to 2003 and
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
//! pieces or not; paragraphs with their formatting — lines round them and
//! colour behind them, tab stops, direction — and their styles; runs with
//! theirs; tables, with cells merged either way, ruled and shaded, and tables
//! inside them; bulleted and numbered lists; links, and every other field
//! with its instruction; bookmarks; comments, with what they cover and who
//! wrote them; footnotes and endnotes; tracked insertions, deletions and
//! changes of formatting; pictures — bitmaps and metafiles alike — in the
//! line and floating; shapes and text boxes; the sections, each with its
//! page, its columns and its own headers and footers; the font table; and
//! what the document says about itself. Word 6 and Word 95 files as well as
//! Word 97's and after, and files encrypted with a password — Word 97's
//! RC4, Word 2002's RC4 through the system's cryptography, and Word 95's
//! exclusive-or.
//!
//! Writing: [`save`], a Word 97-2003 file of what the model holds that the
//! format has a place for — see [`write`] for what that is and is not.
//!
//! # What is not
//!
//! Groups of drawings, WordArt and freeform drawings, and Word 6's drawing
//! objects; pictures in text boxes; Word 6's outline numbering of headings;
//! tracked changes to paragraph formatting and to paragraph marks, and what
//! a run's formatting was before a tracked change to it, which the format
//! does not keep; the ranges of Word 6's comments; a field whose result runs
//! over several paragraphs, which keeps its result as text; notes marked
//! with something other than their number; frames; the properties of a file
//! that encrypts them with the rest; the styles as styles, which are laid
//! into the formatting of what uses them; and Word 2 and older, which are not
//! compound files at all. Each is named in the roadmap rather than half read
//! here.

#![forbid(unsafe_code)]

pub mod fib;
mod format;
mod old;
mod plc;
mod read;
mod sections;
mod shapes;
pub mod sprm;
mod summary;
mod tables;
mod text;
pub mod write;

pub use read::{
    read, read_with_password, BookmarkFound, CommentFound, FurnitureFound, LinkFound, NoteFound,
    PictureFound, Reading, SectionFound, PICTURE_MARK,
};
pub use sections::PageSetup;
pub use write::write as save;

use wp_docx::notes::Kind;
use wp_docx::{Document, TextPosition};

/// Why a file could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a compound file at all.
    NotCompound,
    /// A compound file, but not a Word document.
    NotWord,
    /// A document that is encrypted, and no password was given.
    Encrypted,
    /// The password given is not the document's.
    WrongPassword,
    /// A document written in a way this program does not read.
    Unsupported(String),
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

impl From<wp_crypt::Error> for Error {
    fn from(error: wp_crypt::Error) -> Self {
        match error {
            wp_crypt::Error::WrongPassword => Self::WrongPassword,
            wp_crypt::Error::Unsupported(how) => Self::Unsupported(how),
            wp_crypt::Error::Damaged(what) => Self::Malformed(what),
            wp_crypt::Error::NotEncrypted | wp_crypt::Error::Tampered => {
                Self::Malformed("encryption")
            }
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotCompound => write!(f, "not a compound file"),
            Self::NotWord => write!(f, "not a Word document"),
            Self::Encrypted => write!(f, "the document is encrypted"),
            Self::WrongPassword => write!(f, "that is not the password"),
            Self::Unsupported(what) => write!(f, "the document is written with {what}"),
            Self::Truncated(what) => write!(f, "the file ends inside its {what}"),
            Self::Malformed(what) => write!(f, "the file's {what} is not as the format says"),
            Self::Document(what) => write!(f, "{what}"),
        }
    }
}

impl std::error::Error for Error {}

/// Whether a binary document is encrypted, and needs a password to open.
#[must_use]
pub fn is_encrypted(bytes: &[u8]) -> bool {
    wp_ole::CompoundFile::open(bytes.to_vec())
        .ok()
        .and_then(|file| file.stream("WordDocument"))
        .and_then(|word| fib::Base::parse(&word).ok())
        .is_some_and(|base| base.encrypted)
}

/// Opens a binary document: everything [`read`] found, put where it belongs
/// — the pictures in where their marks were, the links, bookmarks and
/// comments laid over their text, the notes given their words, and the
/// sections made with their pages and their headers and footers.
pub fn open(bytes: &[u8]) -> Result<Document, Error> {
    open_with_password(bytes, None)
}

/// The same, for a document that may be encrypted: deciphered with the
/// password, which the document then keeps, so that saving it writes it
/// back encrypted — as a `.docx`, since this program writes no weaker
/// encryption than that.
pub fn open_with_password(bytes: &[u8], password: Option<&str>) -> Result<Document, Error> {
    let reading = read_with_password(bytes, password)?;
    let encrypted = is_encrypted(bytes);
    let failed = |error: wp_docx::Error| Error::Document(error.to_string());
    let mut document = Document::create(&reading.body).map_err(failed)?;
    if !reading.fonts.is_empty() {
        document.set_font_table(&reading.fonts).map_err(failed)?;
    }

    put_pictures(&mut document, &reading.pictures, 0);
    for link in &reading.links {
        if link.end <= link.start {
            continue;
        }
        document.set_caret(TextPosition::new(link.paragraph, link.start));
        document.extend_selection_to(TextPosition::new(link.paragraph, link.end));
        document.add_hyperlink(&link.address, "");
    }
    for bookmark in &reading.bookmarks {
        document.set_caret(bookmark.start);
        document.extend_selection_to(bookmark.end);
        document.add_bookmark(&bookmark.name);
    }
    // The comments, and then their pictures, in the part they are all in:
    // one after another, each as many paragraphs long as its words.
    let mut written = Vec::new();
    for comment in &reading.comments {
        document.set_caret(comment.start);
        document.extend_selection_to(comment.end);
        let text = comment.body.plain_text();
        if let Ok(id) = document.add_comment(text.trim(), &comment.author, &comment.date) {
            if document.set_comment_body(id, &comment.body) {
                written.push(comment);
            }
        }
    }
    document.clear_selection();
    if let Some(part) = document.comments_part() {
        put_pictures_in_entries(
            &mut document,
            &part,
            0,
            written.iter().map(|comment| (&comment.body, &comment.pictures)),
        );
    }
    for note in &reading.notes {
        let kind = if note.endnote { Kind::Endnote } else { Kind::Footnote };
        document.put_note(kind, note.id, &note.body).map_err(failed)?;
    }
    for kind in [Kind::Footnote, Kind::Endnote] {
        let Some(part) = document.notes_part(kind) else { continue };
        // After the two notes that are not notes: the separator line and
        // the one a note carried over to the next page is set under.
        let notes = reading.notes.iter().filter(|note| note.endnote == (kind == Kind::Endnote));
        put_pictures_in_entries(
            &mut document,
            &part,
            2,
            notes.map(|note| (&note.body, &note.pictures)),
        );
    }

    set_up_sections(&mut document, &reading.sections).map_err(failed)?;
    if reading.facing_pages {
        document.set_different_odd_and_even(true);
    }
    if !reading.properties.is_empty() {
        document.set_properties(&reading.properties).map_err(failed)?;
    }
    if let (true, Some(password)) = (encrypted, password) {
        document.set_password(Some(password));
    }

    document.set_caret(TextPosition::default());
    document.clear_selection();
    // None of that was anything a person did.
    document.forget_history();
    let _ = document.mark_saved();
    Ok(document)
}

/// Makes the sections: the breaks first, each on the paragraph that ends its
/// section, and then each section's page and its headers and footers, with
/// the caret in it — which is what says which section a change is to.
fn set_up_sections(
    document: &mut Document,
    sections: &[SectionFound],
) -> Result<(), wp_docx::Error> {
    for (index, section) in sections.iter().enumerate() {
        let (Some(last), Some(next)) = (section.last_paragraph, sections.get(index + 1)) else {
            continue;
        };
        document.end_section_at(last, next.page.start);
    }
    let mut first = 0;
    for section in sections {
        document.set_caret(TextPosition::new(first, 0));
        let page = &section.page;
        let turned = page.landscape && page.width < page.height;
        let (width, height) =
            if turned { (page.height, page.width) } else { (page.width, page.height) };
        if width > 0 && height > 0 {
            document.set_page_size(width, height);
        }
        let [top, right, bottom, left] = page.margins;
        document.set_page_margins(top, right, bottom, left);
        document.set_furniture_distances(page.header_distance, page.footer_distance);
        if page.columns > 1 {
            document.set_columns(page.columns, page.column_gap);
        }
        if page.title_page {
            document.set_different_first_page(true);
        }
        if let Some(numbering) = page.numbering {
            document.set_page_numbering(numbering);
        }
        for furniture in &section.furniture {
            document.set_furniture_body(furniture.kind, furniture.which, &furniture.body)?;
        }
        first = section.last_paragraph.map_or(first, |last| last + 1);
    }
    // The pictures in the headers and footers go into each one's own part,
    // which is where the parts they need are named.
    for (index, section) in sections.iter().enumerate() {
        for furniture in section.furniture.iter().filter(|furniture| !furniture.pictures.is_empty())
        {
            let Some(part) = document.furniture_part_for(furniture.kind, index, furniture.which)
            else {
                continue;
            };
            if document.enter_part(&part) {
                put_pictures(document, &furniture.pictures, 0);
                document.leave_part();
            }
        }
    }
    Ok(())
}

/// Puts pictures in where their marks are, first to last, in whichever part
/// is being edited, `shift` paragraphs further on than they were counted.
///
/// Each mark is longer than the picture that takes its place, and the places
/// of everything after it were counted with the picture, so each is where it
/// should be once the ones before it are in.
fn put_pictures(document: &mut Document, pictures: &[PictureFound], shift: usize) {
    for picture in pictures {
        let paragraph = picture.paragraph + shift;
        let start = TextPosition::new(paragraph, picture.offset);
        let end = TextPosition::new(paragraph, picture.offset + PICTURE_MARK.len_utf8());
        document.set_caret(start);
        document.extend_selection_to(end);
        document.delete_selection();
        document.set_caret(start);
        let put = document.insert_picture(
            &picture.bytes,
            picture.extension,
            picture.width_emu,
            picture.height_emu,
        );
        if let (Ok(true), Some(anchor)) = (put, &picture.anchor) {
            document.set_anchor_at(start, Some(anchor));
        }
    }
    document.clear_selection();
}

/// The same for a part of entries one after another — the comments, or the
/// notes of one kind — whose paragraphs are counted through the whole part:
/// each entry's pictures are shifted past the paragraphs of the ones before
/// it, and past `before` paragraphs that are not entries' at all.
fn put_pictures_in_entries<'a>(
    document: &mut Document,
    part: &str,
    before: usize,
    entries: impl Iterator<Item = (&'a wp_docx::model::Body, &'a Vec<PictureFound>)>,
) {
    let entries: Vec<_> = entries.collect();
    if entries.iter().all(|(_, pictures)| pictures.is_empty()) || !document.enter_part(part) {
        return;
    }
    let mut shift = before;
    for (body, pictures) in entries {
        put_pictures(document, pictures, shift);
        shift += body.paragraphs().len();
    }
    document.leave_part();
}

/// Whether bytes are a compound file, which is what a `.doc` is — and what
/// some files with other names are too.
#[must_use]
pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
}

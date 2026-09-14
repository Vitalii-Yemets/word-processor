//! Reading a PDF back into a document: Word's "PDF Reflow".
//!
//! A PDF is a description of pages, not of a document — it says where
//! each glyph and picture is drawn and nothing of paragraphs, so opening
//! one in a word processor means working the document back out. The
//! pieces: [`file`] finds the objects, [`filters`] unpacks their streams,
//! [`font`] and [`cmap`] turn shown bytes into characters, [`content`]
//! runs each page for where its glyphs land, [`images`] takes the
//! pictures out, and [`reflow`] turns the glyphs into lines, paragraphs,
//! lists, headings and tables.

pub mod cmap;
pub mod content;
pub mod file;
pub mod filters;
pub mod font;
pub mod images;
pub mod object;
pub mod reflow;

use wp_docx::{Document, TextPosition};

use file::File;
use object::Object;
use reflow::{PageDrawn, PICTURE_MARK};

/// Why a file could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// No `%PDF` header.
    NotPdf,
    /// The file is encrypted, which this reader does not undo.
    Encrypted,
    /// No pages could be found.
    NoPages,
    /// The document could not be built from what was read.
    Document(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotPdf => write!(f, "not a PDF file"),
            Self::Encrypted => write!(f, "the PDF is encrypted"),
            Self::NoPages => write!(f, "the PDF has no pages"),
            Self::Document(what) => write!(f, "{what}"),
        }
    }
}

impl std::error::Error for Error {}

/// Opens a PDF as a document: its text reflowed into paragraphs, with the
/// formatting, lists, headings, tables, pictures and links that can be
/// told from the page.
pub fn open(bytes: &[u8]) -> Result<Document, Error> {
    let file = File::open(bytes)?;
    let pages = file.pages();
    if pages.is_empty() {
        return Err(Error::NoPages);
    }
    let mut drawn_pages = Vec::with_capacity(pages.len());
    for page in &pages {
        let drawn = content::Interpreter::run_page(&file, page);
        let [left, bottom, right, top] = page.media_box;
        let (width, height) = if page.rotate == 90 || page.rotate == 270 {
            (top - bottom, right - left)
        } else {
            (right - left, top - bottom)
        };
        let links = links_of(&file, page);
        drawn_pages.push(PageDrawn { drawn, width, height, links });
    }
    let reading = reflow::reflow(drawn_pages);

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
            picture.width_emu.max(1),
            picture.height_emu.max(1),
        );
        // A picture is one byte of the paragraph's text where its mark was
        // three: the links after it move up.
        let after = document.paragraph_text(picture.paragraph).map_or(0, |text| text.len());
        for link in &mut links {
            if link.paragraph == picture.paragraph && link.start >= picture.offset {
                link.start = (link.start + after).saturating_sub(before);
                link.end = (link.end + after).saturating_sub(before);
            }
        }
    }
    for link in links {
        if link.end <= link.start || link.address.is_empty() {
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
    let info = file.info();
    if let Object::String(title) = file.get(&info, "Title") {
        let title = object::text_of(&title);
        if !title.trim().is_empty() {
            let mut properties = document.properties();
            properties.title = title.trim().to_owned();
            let _ = document.set_properties(&properties);
        }
    }
    document.set_caret(TextPosition::default());
    document.clear_selection();
    let _ = document.mark_saved();
    Ok(document)
}

/// The links laid over a page: link annotations that go to an address.
fn links_of(file: &File<'_>, page: &file::PageInfo) -> Vec<([f64; 4], String)> {
    let mut links = Vec::new();
    let annotations = file.get(&page.dictionary, "Annots");
    let Some(annotations) = annotations.as_array() else { return links };
    let [left, bottom, _, _] = page.media_box;
    for annotation in annotations {
        let annotation = file.resolve(annotation);
        let Some(annotation) = annotation.as_dictionary() else { continue };
        if file.get(annotation, "Subtype").as_name() != Some("Link") {
            continue;
        }
        let Some(rect) = annotation.get("Rect").and_then(|r| file.rectangle(r)) else { continue };
        let action = file.get(annotation, "A");
        let Some(action) = action.as_dictionary() else { continue };
        if file.get(action, "S").as_name() != Some("URI") {
            continue;
        }
        let Object::String(uri) = file.get(action, "URI") else { continue };
        let address = String::from_utf8_lossy(&uri).into_owned();
        let rect = [
            rect[0].min(rect[2]) - left,
            rect[1].min(rect[3]) - bottom,
            rect[0].max(rect[2]) - left,
            rect[1].max(rect[3]) - bottom,
        ];
        links.push((rect, address));
    }
    links
}

/// Whether bytes look like a PDF.
#[must_use]
pub fn looks_like_pdf(bytes: &[u8]) -> bool {
    object::find(&bytes[..bytes.len().min(1024)], b"%PDF").is_some()
}

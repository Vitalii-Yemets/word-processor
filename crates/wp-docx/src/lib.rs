//! WordprocessingML documents: reading and creating a `.docx`.
//!
//! # What this stage does and does not do
//!
//! A document that is **opened** can be read and saved again, and saving
//! reproduces the original file byte for byte. That is the property everything
//! else will be built on: nothing is lost, because nothing is regenerated.
//!
//! A document that is **created** here is generated from the model in
//! [`model`], which covers paragraphs, runs, character formatting and tables.
//!
//! Editing the body of an opened document is deliberately not offered yet. The
//! model does not represent everything a real document contains, so writing an
//! opened document back out from it would quietly discard the rest. Doing that
//! properly needs the full document model, which is the next stage of the
//! project.
//!
//! # Example
//!
//! ```
//! use wp_docx::{Document, model::{Block, Body, Paragraph}};
//!
//! let mut body = Body::default();
//! body.blocks.push(Block::Paragraph(Paragraph::text("Hello")));
//!
//! let document = Document::create(&body)?;
//! let bytes = document.save()?;
//!
//! // Read it back.
//! let reopened = Document::open(&bytes)?;
//! assert_eq!(reopened.body()?.plain_text(), "Hello");
//! # Ok::<(), wp_docx::Error>(())
//! ```

#![forbid(unsafe_code)]

pub mod model;
mod read;
mod write;

use wp_opc::{Package, Relationships, TargetMode};
use wp_xml::Reader;

pub use model::Body;
pub use read::W as WORDPROCESSING_NAMESPACE;

/// Content type of the styles part.
const STYLES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";

/// Relationship type of the styles part.
const STYLES_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

/// Why a document could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The package layer could not read or write the file.
    Package(wp_opc::Error),
    /// The main document part is not valid XML.
    Xml { part: String, source: wp_xml::Error },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Package(error) => write!(f, "{error}"),
            Self::Xml { part, source } => write!(f, "part {part:?} is not valid XML: {source}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<wp_opc::Error> for Error {
    fn from(error: wp_opc::Error) -> Self {
        Self::Package(error)
    }
}

/// An open document.
#[derive(Clone, Debug)]
pub struct Document {
    package: Package,
    main_part: String,
}

impl Document {
    /// Opens a `.docx` from its bytes.
    pub fn open(bytes: &[u8]) -> Result<Self, Error> {
        let package = Package::open(bytes)?;
        let main_part = package.main_document_part()?;
        Ok(Self { package, main_part })
    }

    /// Builds a new document containing the given body.
    pub fn create(body: &Body) -> Result<Self, Error> {
        let mut package = Package::empty();

        let document_xml = write::document_xml(body).map_err(|source| Error::Xml {
            part: "word/document.xml".to_owned(),
            source,
        })?;

        package.add_part(
            "word/document.xml",
            wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
            document_xml.into_bytes(),
        );
        package.add_part("word/styles.xml", STYLES_CONTENT_TYPE, default_styles().into_bytes());

        // A package is navigated by relationships, not by filenames, so the
        // main document has to be pointed at from the package root.
        let mut root = Relationships::new("");
        root.add(wp_opc::OFFICE_DOCUMENT_RELATIONSHIP, "word/document.xml", TargetMode::Internal);
        package.set_relationships(&root)?;

        let mut document_relationships = Relationships::new("word/document.xml");
        document_relationships.add(STYLES_RELATIONSHIP, "styles.xml", TargetMode::Internal);
        package.set_relationships(&document_relationships)?;

        Ok(Self { package, main_part: "word/document.xml".to_owned() })
    }

    /// The package behind the document, for inspecting its parts.
    #[must_use]
    pub fn package(&self) -> &Package {
        &self.package
    }

    /// The name of the main document part.
    #[must_use]
    pub fn main_part(&self) -> &str {
        &self.main_part
    }

    /// Reads the body.
    ///
    /// Parsed on demand rather than at open time: opening a document should not
    /// pay for work the caller may not need.
    pub fn body(&self) -> Result<Body, Error> {
        let text = self
            .package
            .xml_part(&self.main_part)
            .ok_or_else(|| wp_opc::Error::MissingPart(self.main_part.clone()))??;

        let events = Reader::new(&text).into_events().map_err(|source| Error::Xml {
            part: self.main_part.clone(),
            source,
        })?;

        Ok(read::parse_body(&events))
    }

    /// The whole document's text, with formatting removed.
    pub fn plain_text(&self) -> Result<String, Error> {
        Ok(self.body()?.plain_text())
    }

    /// Writes the document back out.
    ///
    /// For a document that was opened and not modified, this reproduces the
    /// original file exactly.
    pub fn save(&self) -> Result<Vec<u8>, Error> {
        Ok(self.package.save()?)
    }
}

/// A small stylesheet, so that documents created here have the styles their
/// paragraphs refer to.
///
/// Without it a `w:pStyle` naming `Heading1` would resolve to nothing and the
/// heading would render as body text.
fn default_styles() -> String {
    let w = read::W;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="{w}">
<w:docDefaults><w:rPrDefault><w:rPr>
<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:cs="Calibri" w:eastAsia="Calibri"/>
<w:sz w:val="22"/><w:szCs w:val="22"/>
</w:rPr></w:rPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal">
<w:name w:val="Normal"/><w:qFormat/>
</w:style>
<w:style w:type="paragraph" w:styleId="Title">
<w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:qFormat/>
<w:pPr><w:spacing w:before="240" w:after="240"/></w:pPr>
<w:rPr><w:b/><w:bCs/><w:sz w:val="56"/><w:szCs w:val="56"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading1">
<w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:qFormat/>
<w:pPr><w:outlineLvl w:val="0"/><w:spacing w:before="240" w:after="120"/></w:pPr>
<w:rPr><w:b/><w:bCs/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading2">
<w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:qFormat/>
<w:pPr><w:outlineLvl w:val="1"/><w:spacing w:before="200" w:after="100"/></w:pPr>
<w:rPr><w:b/><w:bCs/><w:sz w:val="26"/><w:szCs w:val="26"/></w:rPr>
</w:style>
</w:styles>"#
    )
}

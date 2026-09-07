//! WordprocessingML documents: reading, creating and editing a `.docx`.
//!
//! # How editing stays safe
//!
//! An opened document is held as an element tree that keeps everything it was
//! given — every element, attribute and comment, understood or not. An edit
//! changes the nodes it must and leaves the rest alone, so saving writes back a
//! document that differs only where the user changed it.
//!
//! This matters more than it sounds. A real `.docx` carries a macro project, an
//! embedded font, a chart, a content control, somebody else's tracked changes. A
//! model that understood only what it knew about would throw the rest away the
//! moment the user pressed save.
//!
//! A document that is opened and saved without being edited comes back byte for
//! byte identical, because nothing is re-serialized at all.
//!
//! # Example
//!
//! ```
//! use wp_docx::{Document, model::{Block, Body, Paragraph}};
//!
//! let mut body = Body::default();
//! body.blocks.push(Block::Paragraph(Paragraph::text("Hello, world")));
//! let bytes = Document::create(&body)?.save()?;
//!
//! // Reopen it and change one word.
//! let mut document = Document::open(&bytes)?;
//! assert_eq!(document.replace_text("world", "everyone"), 1);
//! assert_eq!(document.plain_text(), "Hello, everyone");
//! # Ok::<(), wp_docx::Error>(())
//! ```

#![forbid(unsafe_code)]

pub mod edit;
pub mod model;
pub mod position;
mod read;
pub mod styles;

use wp_opc::{Package, Relationships, TargetMode};
use wp_xml::tree::{Element, XmlTree};

pub use model::Body;
pub use read::W as WORDPROCESSING_NAMESPACE;
pub use position::TextPosition;
pub use styles::{Style, StyleKind, Styles};

use model::{
    Alignment, Block, Paragraph, ResolvedParagraphProperties, ResolvedRunProperties, Run,
};

/// Content type of the styles part.
const STYLES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";

/// Relationship type of the styles part.
const STYLES_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

/// Content type of the settings part.
const SETTINGS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";

/// Relationship type of the settings part.
const SETTINGS_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";

/// Page width of A4 in twentieths of a point, the unit the format uses.
const A4_WIDTH_TWIPS: &str = "11906";
/// Page height of A4 in the same unit.
const A4_HEIGHT_TWIPS: &str = "16838";
/// One inch of margin, in the same unit.
const MARGIN_TWIPS: &str = "1440";

/// Why a document could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The package layer could not read or write the file.
    Package(wp_opc::Error),
    /// A part is not valid XML.
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
    tree: XmlTree,
    /// The document's style definitions, read once when it is opened.
    styles: Styles,
    /// Whether the tree has been changed since it was read.
    ///
    /// While it is false, saving writes the original bytes straight back, which
    /// is what makes an untouched document come out identical.
    modified: bool,
}

impl Document {
    /// Opens a `.docx` from its bytes.
    pub fn open(bytes: &[u8]) -> Result<Self, Error> {
        let package = Package::open(bytes)?;
        let main_part = package.main_document_part()?;

        let text = package
            .xml_part(&main_part)
            .ok_or_else(|| wp_opc::Error::MissingPart(main_part.clone()))??;
        let tree = XmlTree::parse(&text).map_err(|source| Error::Xml {
            part: main_part.clone(),
            source,
        })?;

        let styles = read_styles(&package, &main_part);

        Ok(Self { package, main_part, tree, styles, modified: false })
    }

    /// Builds a new document containing the given body.
    pub fn create(body: &Body) -> Result<Self, Error> {
        let tree = build_document(body);
        let xml = tree.to_xml().map_err(|source| Error::Xml {
            part: "word/document.xml".to_owned(),
            source,
        })?;

        let mut package = Package::empty();
        package.add_part(
            "word/document.xml",
            wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
            xml.into_bytes(),
        );
        package.add_part("word/styles.xml", STYLES_CONTENT_TYPE, default_styles().into_bytes());
        package.add_part(
            "word/settings.xml",
            SETTINGS_CONTENT_TYPE,
            default_settings().into_bytes(),
        );

        // A package is navigated by relationships, not by filenames, so the main
        // document has to be pointed at from the package root.
        let mut root = Relationships::new("");
        root.add(wp_opc::OFFICE_DOCUMENT_RELATIONSHIP, "word/document.xml", TargetMode::Internal);
        package.set_relationships(&root)?;

        let mut document_relationships = Relationships::new("word/document.xml");
        document_relationships.add(STYLES_RELATIONSHIP, "styles.xml", TargetMode::Internal);
        document_relationships.add(SETTINGS_RELATIONSHIP, "settings.xml", TargetMode::Internal);
        package.set_relationships(&document_relationships)?;

        let styles = read_styles(&package, "word/document.xml");

        Ok(Self {
            package,
            main_part: "word/document.xml".to_owned(),
            tree,
            styles,
            modified: false,
        })
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

    /// The element tree of the main document part.
    #[must_use]
    pub fn tree(&self) -> &XmlTree {
        &self.tree
    }

    /// The element tree, for edits this crate does not offer directly.
    ///
    /// Taking this marks the document as changed, since there is no way to know
    /// afterwards whether it was.
    pub fn tree_mut(&mut self) -> &mut XmlTree {
        self.modified = true;
        &mut self.tree
    }

    /// Whether the document has been changed since it was opened.
    #[must_use]
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Reads the body.
    #[must_use]
    pub fn body(&self) -> Body {
        read::read_document(&self.tree.root)
    }

    /// The whole document's text, with formatting removed.
    #[must_use]
    pub fn plain_text(&self) -> String {
        self.body().plain_text()
    }

    /// The document's style definitions.
    #[must_use]
    pub fn styles(&self) -> &Styles {
        &self.styles
    }

    /// What a paragraph's formatting actually is, once its style chain and the
    /// document defaults have been applied.
    #[must_use]
    pub fn resolve_paragraph(&self, paragraph: &Paragraph) -> ResolvedParagraphProperties {
        self.styles.resolve_paragraph(&paragraph.properties)
    }

    /// What a run's formatting actually is.
    ///
    /// The paragraph is needed as well as the run: a run inside a heading is
    /// bold because the *paragraph* style says so, not because the run does.
    #[must_use]
    pub fn resolve_run(&self, paragraph: &Paragraph, run: &Run) -> ResolvedRunProperties {
        self.styles.resolve_run(paragraph.style(), &run.properties)
    }

    /// The prefix this document uses for the WordprocessingML namespace.
    fn prefix(&self) -> Option<String> {
        edit::prefix_for(&self.tree.root, WORDPROCESSING_NAMESPACE)
    }

    /// Replaces every occurrence of a string, returning how many were changed.
    ///
    /// The search works across run boundaries, which it has to: Word splits a
    /// paragraph's text between runs wherever formatting changes, so a word can
    /// easily be stored in two pieces.
    pub fn replace_text(&mut self, needle: &str, replacement: &str) -> usize {
        let replaced = edit::replace_text(&mut self.tree.root, needle, replacement);
        if replaced > 0 {
            self.modified = true;
        }
        replaced
    }

    /// How many paragraphs the document has, in reading order.
    #[must_use]
    pub fn paragraph_count(&self) -> usize {
        position::paragraph_count(&self.tree.root)
    }

    /// The text of one paragraph, measured the way a [`TextPosition`] is.
    #[must_use]
    pub fn paragraph_text(&self, index: usize) -> Option<String> {
        position::text_of(&self.tree.root, index)
    }

    /// Inserts text at a position, as typing does.
    pub fn insert_text(&mut self, at: TextPosition, text: &str) -> bool {
        let prefix = self.prefix();
        let changed = position::insert_text(&mut self.tree.root, at, text, prefix.as_deref());
        self.modified |= changed;
        changed
    }

    /// Removes a stretch of text from one paragraph.
    pub fn delete_range(&mut self, paragraph: usize, start: usize, end: usize) -> bool {
        let changed = position::delete_range(&mut self.tree.root, paragraph, start, end);
        self.modified |= changed;
        changed
    }

    /// Splits a paragraph in two, as pressing Enter does.
    pub fn split_paragraph(&mut self, at: TextPosition) -> bool {
        let prefix = self.prefix();
        let changed = position::split_paragraph(&mut self.tree.root, at, prefix.as_deref());
        self.modified |= changed;
        changed
    }

    /// Joins a paragraph onto the one before it, as Backspace at its start does.
    pub fn merge_with_previous(&mut self, paragraph: usize) -> bool {
        let changed = position::merge_with_previous(&mut self.tree.root, paragraph);
        self.modified |= changed;
        changed
    }

    /// Appends a paragraph to the end of the document.
    pub fn append_paragraph(&mut self, paragraph: &Paragraph) -> bool {
        self.append_block(&Block::Paragraph(paragraph.clone()))
    }

    /// Appends a block to the end of the document, before the section properties.
    pub fn append_block(&mut self, block: &Block) -> bool {
        let prefix = self.prefix();
        let Some(body) = read::find_body_mut(&mut self.tree.root) else {
            return false;
        };
        edit::append_block(body, block, prefix.as_deref());
        self.modified = true;
        true
    }

    /// Sets the style of the paragraph at a given index, or clears it.
    pub fn set_paragraph_style(&mut self, index: usize, style: Option<&str>) -> bool {
        let prefix = self.prefix();
        let Some(body) = read::find_body_mut(&mut self.tree.root) else {
            return false;
        };
        let changed = edit::set_paragraph_style(body, index, style, prefix.as_deref());
        self.modified |= changed;
        changed
    }

    /// Sets the alignment of the paragraph at a given index.
    pub fn set_paragraph_alignment(&mut self, index: usize, alignment: Alignment) -> bool {
        let prefix = self.prefix();
        let Some(body) = read::find_body_mut(&mut self.tree.root) else {
            return false;
        };
        let changed = edit::set_paragraph_alignment(body, index, alignment, prefix.as_deref());
        self.modified |= changed;
        changed
    }

    /// Records that the bytes from [`Self::save`] have actually been stored.
    ///
    /// This commits the edited tree into the package and clears the modified
    /// flag. Clearing the flag alone would be a quiet corruption: the package
    /// would still hold the *old* main part, so the next save would write the
    /// document as it was before the edits.
    pub fn mark_saved(&mut self) -> Result<(), Error> {
        if !self.modified {
            return Ok(());
        }

        let xml = self.tree.to_xml().map_err(|source| Error::Xml {
            part: self.main_part.clone(),
            source,
        })?;
        self.package.set_part(&self.main_part, xml.into_bytes());
        self.modified = false;
        Ok(())
    }

    /// Writes the document back out.
    ///
    /// An unmodified document is written from its original bytes, so it comes
    /// out identical. A modified one has only its main part re-serialized;
    /// every other part is still written back exactly as it arrived.
    pub fn save(&self) -> Result<Vec<u8>, Error> {
        if !self.modified {
            return Ok(self.package.save()?);
        }

        let xml = self.tree.to_xml().map_err(|source| Error::Xml {
            part: self.main_part.clone(),
            source,
        })?;

        let mut package = self.package.clone();
        package.set_part(&self.main_part, xml.into_bytes());
        Ok(package.save()?)
    }
}

/// Reads the style definitions belonging to a document part.
///
/// The part is found by following the styles relationship rather than by
/// guessing at a filename, which is how a package is meant to be navigated. A
/// document with no styles part simply has none: that is unusual but valid, and
/// everything then falls back to the built-in defaults.
fn read_styles(package: &Package, main_part: &str) -> Styles {
    let target = package
        .relationships(main_part)
        .ok()
        .and_then(|relationships| {
            let relationship = relationships.single_by_type(STYLES_RELATIONSHIP)?;
            relationship.resolved_target(main_part)?.ok()
        })
        .unwrap_or_else(|| "word/styles.xml".to_owned());

    let Some(Ok(text)) = package.xml_part(&target) else {
        return Styles::default();
    };
    match XmlTree::parse(&text) {
        Ok(tree) => Styles::parse(&tree.root),
        // A damaged styles part should not stop the document opening; the text
        // is still readable, it just renders with the defaults.
        Err(_) => Styles::default(),
    }
}

/// Builds the tree of a brand new `document.xml`.
fn build_document(body: &Body) -> XmlTree {
    let namespace = WORDPROCESSING_NAMESPACE;

    let mut root = Element::new("w:document", Some(namespace));
    root.declarations.push((Some("w".to_owned()), namespace.to_owned()));

    let mut body_element = Element::new("w:body", Some(namespace));
    for block in &body.blocks {
        edit::append_block(&mut body_element, block, Some("w"));
    }
    body_element.push_element(section_properties());
    root.push_element(body_element);

    XmlTree {
        standalone: Some(true),
        has_declaration: true,
        doctype: None,
        before_root: Vec::new(),
        root,
        after_root: Vec::new(),
    }
}

/// Page size and margins, which must be the last child of the body.
fn section_properties() -> Element {
    let namespace = WORDPROCESSING_NAMESPACE;
    let mut section = Element::new("w:sectPr", Some(namespace));

    let mut size = Element::new("w:pgSz", Some(namespace));
    size.set_namespaced_attribute("w:w", namespace, A4_WIDTH_TWIPS);
    size.set_namespaced_attribute("w:h", namespace, A4_HEIGHT_TWIPS);
    section.push_element(size);

    let mut margins = Element::new("w:pgMar", Some(namespace));
    for (name, value) in [
        ("w:top", MARGIN_TWIPS),
        ("w:right", MARGIN_TWIPS),
        ("w:bottom", MARGIN_TWIPS),
        ("w:left", MARGIN_TWIPS),
        ("w:header", "708"),
        ("w:footer", "708"),
        ("w:gutter", "0"),
    ] {
        margins.set_namespaced_attribute(name, namespace, value);
    }
    section.push_element(margins);

    section
}

/// Document settings.
///
/// The only thing in here is the compatibility mode, and it earns its place.
/// Without it Word assumes a document was written for Word 2007 and opens it in
/// compatibility mode: the title bar says so, newer features are disabled, and
/// the user is invited to convert a file that never needed converting. The
/// declaration is what says which version's rules the document was written to.
fn default_settings() -> String {
    let w = WORDPROCESSING_NAMESPACE;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings xmlns:w="{w}">
<w:compat>
<w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/>
</w:compat>
</w:settings>"#
    )
}

/// A small stylesheet, so that documents created here have the styles their
/// paragraphs refer to.
///
/// Without it a `w:pStyle` naming `Heading1` would resolve to nothing and the
/// heading would render as body text.
///
/// The document defaults state the paragraph spacing explicitly. Leaving it
/// unstated does not mean zero: it means each program applies its own idea of a
/// default, and Word's is eight points after every paragraph. The same document
/// then came out one page here and two in Word. Saying it outright leaves
/// nothing to anyone's discretion.
fn default_styles() -> String {
    let w = WORDPROCESSING_NAMESPACE;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="{w}">
<w:docDefaults>
<w:rPrDefault><w:rPr>
<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:cs="Calibri" w:eastAsia="Calibri"/>
<w:sz w:val="22"/><w:szCs w:val="22"/>
</w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr>
<w:spacing w:after="160" w:line="259" w:lineRule="auto"/>
</w:pPr></w:pPrDefault>
</w:docDefaults>
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

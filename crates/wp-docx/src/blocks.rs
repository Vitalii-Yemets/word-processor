//! Building blocks: pieces of a document saved by name and put back later.
//!
//! # What a building block is
//!
//! A piece of a document — a paragraph, a table, a whole cover page — kept
//! under a name so that it can be dropped into another document without
//! being typed again. Word's Quick Parts, its AutoText, its cover pages and
//! its headers and footers galleries are all the same thing: a list of these
//! with a different gallery name on each.
//!
//! # Where they live
//!
//! Not in the document that uses them. A block is kept in a *template*, and
//! the template a person's own blocks live in is `Normal.dotm` — the one Word
//! makes when it first runs and writes everything personal into. So a block
//! saved in one document is there in the next, which is the whole point of
//! saving one.
//!
//! # How the format writes them
//!
//! In a second document inside the package: a part related as the glossary
//! document, holding `w:docPart` entries. Each has a name, a gallery and a
//! category — which together are how Word decides which menu it appears on —
//! and a body, which is ordinary document content.
//!
//! ```text
//! word/glossary/document.xml
//!   <w:glossaryDocument><w:docParts>
//!     <w:docPart>
//!       <w:docPartPr><w:name w:val="Signature"/>
//!         <w:category><w:name w:val="General"/><w:gallery w:val="quickParts"/></w:category>
//!       </w:docPartPr>
//!       <w:docPartBody><w:p><w:r><w:t>Yours faithfully,</w:t></w:r></w:p></w:docPartBody>
//!     </w:docPart>
//!   </w:docParts></w:glossaryDocument>
//! ```
//!
//! # Word's own galleries, and this program's
//!
//! Word fills some of these galleries itself — its cover pages, its page
//! numbers, its watermarks — with content shipped inside Word. That content
//! is not here and cannot be: a gallery offering "Accent Bar 2" and drawing
//! something else would be lying about what a person was picking.
//!
//! What is here is the names of those galleries, so that two things work.
//! A block a person saves into one goes in under the name Word reads, and so
//! turns up in Word's own gallery rather than nowhere. And a document written
//! by Word, whose glossary holds Word's blocks, shows them on the menu they
//! belong on. The designs this program offers alongside them are its own, and
//! are drawn in the program rather than kept here.

use wp_xml::tree::{Element, XmlTree};

use crate::model::Body;
use crate::{edit, read, Document, Error};

/// Where the glossary document lives inside the package.
const PART: &str = "word/glossary/document.xml";

/// What the package calls a relationship to it.
const RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/glossaryDocument";

/// And what kind of part it is.
const CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.glossary+xml";

/// The gallery a block a person saves goes into, unless they say otherwise.
pub const QUICK_PARTS: &str = "quickParts";

/// The other gallery a person fills themselves, which Word puts on a menu of
/// its own.
pub const AUTO_TEXT: &str = "autoText";

/// And the category a block goes in inside its gallery.
pub const GENERAL: &str = "General";

/// The gallery Word's cover pages are in.
pub const COVER_PAGES: &str = "coverPg";

/// Its page numbers, which are four galleries rather than one: Word asks
/// where the number goes before it asks what it looks like, and what looks
/// right at the head of a page does not look right in the margin.
pub const PAGE_NUMBERS: &str = "pgNum";
pub const PAGE_NUMBERS_TOP: &str = "pgNumT";
pub const PAGE_NUMBERS_BOTTOM: &str = "pgNumB";
pub const PAGE_NUMBERS_MARGINS: &str = "pgNumMargins";

/// Its watermarks.
pub const WATERMARKS: &str = "watermarks";

/// Its headers and its footers, which are galleries of their own and not the
/// same as the page-number ones: a header design is a whole header, where a
/// page-number design is the number by itself.
pub const HEADERS: &str = "hdrs";
pub const FOOTERS: &str = "ftrs";

/// Its tables, its equations, its text boxes, its tables of contents and its
/// bibliographies, which are named so that a block saved into one of them is
/// not quietly refiled under something else.
pub const TABLES: &str = "tbls";
pub const EQUATIONS: &str = "eq";
pub const TEXT_BOXES: &str = "txtBox";
pub const CONTENTS: &str = "tblOfContents";
pub const BIBLIOGRAPHIES: &str = "bib";

/// One saved piece of a document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildingBlock {
    /// What it is called, which is what a person picks off a menu.
    pub name: String,
    /// Which menu that is: [`QUICK_PARTS`], [`AUTO_TEXT`] or one of Word's
    /// own.
    pub gallery: String,
    /// The heading it sits under inside that menu.
    pub category: String,
    /// What it is for, shown in the organiser.
    pub description: String,
}

impl BuildingBlock {
    /// A block in the gallery a person's own pieces go to.
    #[must_use]
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            gallery: QUICK_PARTS.to_owned(),
            category: GENERAL.to_owned(),
            description: String::new(),
        }
    }

    /// The same in another gallery.
    #[must_use]
    pub fn in_gallery(mut self, gallery: &str) -> Self {
        self.gallery = gallery.to_owned();
        self
    }
}

impl Document {
    /// Every block the document carries, in the order they were saved.
    #[must_use]
    pub fn building_blocks(&self) -> Vec<BuildingBlock> {
        let Some(root) = self.glossary_root() else { return Vec::new() };
        let Some(parts) = root.child(Some(read::W), "docParts") else { return Vec::new() };
        parts
            .child_elements()
            .filter(|child| child.is(Some(read::W), "docPart"))
            .map(read_block)
            .collect()
    }

    /// The blocks of one gallery.
    #[must_use]
    pub fn blocks_in(&self, gallery: &str) -> Vec<BuildingBlock> {
        self.building_blocks().into_iter().filter(|block| block.gallery == gallery).collect()
    }

    /// What one of them holds.
    #[must_use]
    pub fn building_block_body(&self, name: &str) -> Option<Body> {
        let root = self.glossary_root()?;
        let parts = root.child(Some(read::W), "docParts")?;
        let found = parts
            .child_elements()
            .filter(|child| child.is(Some(read::W), "docPart"))
            .find(|child| read_block(child).name == name)?;
        let body = found.child(Some(read::W), "docPartBody")?;
        Some(read::read_part(body))
    }

    /// Saves a piece of a document under a name.
    ///
    /// A block of the same name is written over, which is what Word asks
    /// about and then does: two blocks with one name is a menu where one of
    /// them can never be picked.
    pub fn add_building_block(&mut self, block: &BuildingBlock, body: &Body) -> bool {
        if block.name.trim().is_empty() {
            return false;
        }
        let prefix = self.prefix();
        let mut root = self.glossary_root().unwrap_or_else(|| new_glossary(prefix.as_deref()));
        let name = |local: &str| edit::name_with(prefix.as_deref(), local);

        // The list of them, made if this is the first.
        if root.child(Some(read::W), "docParts").is_none() {
            root.push_element(Element::new(&name("docParts"), Some(read::W)));
        }
        let Some(parts) = root.child_mut(Some(read::W), "docParts") else { return false };
        parts.children.retain(|node| {
            node.as_element().is_none_or(|child| {
                !child.is(Some(read::W), "docPart") || read_block(child).name != block.name
            })
        });
        parts.push_element(write_block(block, body, prefix.as_deref()));

        self.save_glossary_root(root)
    }

    /// Changes what a block is called and where it is filed, leaving what is
    /// in it alone.
    ///
    /// Word's organiser has a Modify button for exactly this: a piece saved
    /// in a hurry under "Block 1" in the wrong gallery is a piece nobody will
    /// find again, and having to save it afresh means having the document it
    /// came from still open.
    ///
    /// The content is not touched, which is the point: it is the same block,
    /// filed differently.
    pub fn edit_building_block(&mut self, name: &str, wanted: &BuildingBlock) -> bool {
        if wanted.name.trim().is_empty() {
            return false;
        }
        let prefix = self.prefix();
        let Some(mut root) = self.glossary_root() else { return false };
        let Some(parts) = root.child_mut(Some(read::W), "docParts") else { return false };

        // A block already called what this one is to be called would be two
        // of one name, which is a menu where one can never be picked.
        if !wanted.name.eq_ignore_ascii_case(name)
            && parts.child_elements().any(|child| {
                child.is(Some(read::W), "docPart") && read_block(child).name == wanted.name
            })
        {
            return false;
        }

        let Some(part) = parts
            .child_elements_mut()
            .find(|child| child.is(Some(read::W), "docPart") && read_block(child).name == name)
        else {
            return false;
        };
        let Some(properties) = part.child_mut(Some(read::W), "docPartPr") else { return false };
        write_block_properties(properties, wanted, prefix.as_deref());

        self.save_glossary_root(root)
    }

    /// Takes one away. Says whether there was one.
    pub fn remove_building_block(&mut self, name: &str) -> bool {
        let Some(mut root) = self.glossary_root() else { return false };
        let Some(parts) = root.child_mut(Some(read::W), "docParts") else { return false };
        let before = parts.children.len();
        parts.children.retain(|node| {
            node.as_element().is_none_or(|child| {
                !child.is(Some(read::W), "docPart") || read_block(child).name != name
            })
        });
        if parts.children.len() == before {
            return false;
        }
        self.save_glossary_root(root)
    }

    /// Puts a block's content into the document at the caret.
    ///
    /// Returns whether there was a block of that name to put in.
    pub fn insert_building_block(&mut self, from: &Document, name: &str) -> bool {
        let Some(body) = from.building_block_body(name) else { return false };
        if body.blocks.is_empty() {
            return false;
        }
        self.paste_blocks(&body.blocks)
    }

    /// The glossary document, read afresh.
    fn glossary_root(&self) -> Option<Element> {
        let text = self.package().xml_part(PART)?.ok()?;
        XmlTree::parse(&text).ok().map(|tree| tree.root)
    }

    /// Writes it back, making the part and its relationship if this is the
    /// first block.
    fn save_glossary_root(&mut self, root: Element) -> bool {
        let tree = XmlTree {
            standalone: Some(true),
            has_declaration: true,
            doctype: None,
            before_root: Vec::new(),
            root,
            after_root: Vec::new(),
        };
        let Ok(xml) = tree.to_xml() else { return false };

        let fresh = self.package().part(PART).is_none();
        self.package_mut().set_part(PART, xml.into_bytes());
        if fresh {
            let mut types = self.package().content_types().clone();
            types.set_override(&format!("/{PART}"), CONTENT_TYPE);
            self.package_mut().set_content_types(types);

            let main = self.main_part().to_owned();
            if let Ok(mut relationships) = self.package().relationships(&main) {
                relationships.add(
                    RELATIONSHIP,
                    "glossary/document.xml",
                    wp_opc::TargetMode::Internal,
                );
                let _ = self.package_mut().set_relationships(&relationships);
            }
        }
        self.note_change();
        true
    }
}

/// An empty glossary document.
fn new_glossary(prefix: Option<&str>) -> Element {
    let mut root = Element::new(&edit::name_with(prefix, "glossaryDocument"), Some(read::W));
    root.declarations.push((prefix.map(str::to_owned), read::W.to_owned()));
    root
}

/// Reads what a `w:docPart` says about itself.
fn read_block(element: &Element) -> BuildingBlock {
    let properties = element.child(Some(read::W), "docPartPr");
    let said = |parent: Option<&Element>, local: &str| {
        parent
            .and_then(|parent| parent.child(Some(read::W), local))
            .and_then(|child| child.attribute(Some(read::W), "val"))
            .unwrap_or_default()
            .to_owned()
    };
    let category = properties.and_then(|properties| properties.child(Some(read::W), "category"));
    BuildingBlock {
        name: said(properties, "name"),
        gallery: said(category, "gallery"),
        category: said(category, "name"),
        description: said(properties, "description"),
    }
}

/// Writes one.
fn write_block(block: &BuildingBlock, body: &Body, prefix: Option<&str>) -> Element {
    let name = |local: &str| edit::name_with(prefix, local);
    let mut properties = Element::new(&name("docPartPr"), Some(read::W));
    write_block_properties(&mut properties, block, prefix);

    let mut content = Element::new(&name("docPartBody"), Some(read::W));
    for block in &body.blocks {
        content.push_element(edit::block_element(block, prefix));
    }

    let mut part = Element::new(&name("docPart"), Some(read::W));
    part.push_element(properties);
    part.push_element(content);
    part
}

/// What a block's properties say: its name, where it is filed, what sort of
/// thing it is and what it is for.
///
/// Written into properties that may already have some — which is what
/// changing a block's name means — so everything this writes is taken out
/// first. What is left is whatever the properties said that this program does
/// not model, which stays where it was.
fn write_block_properties(properties: &mut Element, block: &BuildingBlock, prefix: Option<&str>) {
    let name = |local: &str| edit::name_with(prefix, local);
    let valued = |local: &str, value: &str| {
        let mut element = Element::new(&name(local), Some(read::W));
        element.set_namespaced_attribute(&name("val"), read::W, value);
        element
    };

    for local in ["name", "category", "types", "behaviors", "description"] {
        properties.remove_children_named(Some(read::W), local);
    }

    let mut category = Element::new(&name("category"), Some(read::W));
    category.push_element(valued("name", &block.category));
    category.push_element(valued("gallery", &block.gallery));

    properties.insert_element(0, valued("name", &block.name));
    properties.push_element(category);
    // What sort of thing it is: a piece of content that goes where the caret
    // is, which is what every block a person saves is.
    let mut types = Element::new(&name("types"), Some(read::W));
    types.push_element(valued("type", "bbPlcHdr"));
    properties.push_element(types);
    let mut behaviors = Element::new(&name("behaviors"), Some(read::W));
    behaviors.push_element(valued("behavior", "content"));
    properties.push_element(behaviors);
    if !block.description.is_empty() {
        properties.push_element(valued("description", &block.description));
    }
}

/// Makes a template with nothing in it but a place for blocks to live.
///
/// What `Normal.dotm` is before anything has been saved into it: a document
/// with one empty paragraph, which is what a new document made from it comes
/// out as.
pub fn empty_template() -> Result<Document, Error> {
    let mut body = Body::default();
    body.blocks.push(crate::model::Block::Paragraph(crate::model::Paragraph::default()));
    Document::create(&body)
}

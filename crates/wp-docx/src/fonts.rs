//! The document's font table: what it says about each font it names.
//!
//! # What it is for
//!
//! A document names its fonts by family, and a family is only a name: the
//! machine it is opened on may not have it. `word/fontTable.xml` says more
//! about each one — another name it goes by, what kind of letter it has (with
//! serifs or without, every letter the same width or not), its PANOSE numbers
//! — and that is what a program that lacks the font goes on when it chooses
//! one to stand in for it. Word does, and a web page Word writes carries the
//! same table as its `@font-face` rules so that it can be rebuilt.

use wp_opc::TargetMode;
use wp_xml::tree::{Element, XmlTree};

use crate::{edit, read, Document, Error};

/// Content type of the font table.
const CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml";

/// Relationship type of that part.
const RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable";

/// What kind of letter a font has, as far as choosing a stand-in goes:
/// `w:family`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontClass {
    /// With serifs, like Times.
    Roman,
    /// Without, like Arial.
    Swiss,
    /// Every letter the same width, like Courier.
    Modern,
    /// Written by hand.
    Script,
    /// For show.
    Decorative,
    /// Nothing said.
    #[default]
    Auto,
}

impl FontClass {
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "roman" => Self::Roman,
            "swiss" => Self::Swiss,
            "modern" => Self::Modern,
            "script" => Self::Script,
            "decorative" => Self::Decorative,
            _ => Self::Auto,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Roman => "roman",
            Self::Swiss => "swiss",
            Self::Modern => "modern",
            Self::Script => "script",
            Self::Decorative => "decorative",
            Self::Auto => "auto",
        }
    }
}

/// One font of the table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontEntry {
    pub name: String,
    /// Another name the same font goes by, tried when this one is not on the
    /// machine: `w:altName`.
    pub alt_name: Option<String>,
    /// The ten PANOSE numbers, as twenty hex digits.
    pub panose: Option<String>,
    /// The character set it was made for, as Windows numbers them.
    pub charset: Option<u8>,
    pub class: FontClass,
    /// Whether every letter is the same width: `Some(true)` for fixed,
    /// `Some(false)` for variable.
    pub fixed_pitch: Option<bool>,
}

impl Document {
    /// The fonts the document's font table describes, in its order.
    #[must_use]
    pub fn font_table(&self) -> Vec<FontEntry> {
        let Some(tree) = crate::related_tree(
            self.package(),
            &self.document_part,
            RELATIONSHIP,
            "word/fontTable.xml",
        ) else {
            return Vec::new();
        };
        tree.root.children_named(Some(read::W), "font").filter_map(read_font).collect()
    }

    /// Writes the font table, in place of whatever the document had.
    pub fn set_font_table(&mut self, fonts: &[FontEntry]) -> Result<(), Error> {
        let mut root = Element::new("w:fonts", Some(read::W));
        root.declarations.push((Some("w".to_owned()), read::W.to_owned()));
        for font in fonts {
            root.push_element(font_element(font));
        }
        let tree = XmlTree {
            standalone: Some(true),
            has_declaration: true,
            doctype: None,
            before_root: Vec::new(),
            root,
            after_root: Vec::new(),
        };
        let xml = tree
            .to_xml()
            .map_err(|source| Error::Xml { part: "word/fontTable.xml".to_owned(), source })?;

        let owner = self.document_part.clone();
        let mut relationships = self
            .package()
            .relationships(&owner)
            .unwrap_or_else(|_| wp_opc::Relationships::new(&owner));
        let part = relationships
            .single_by_type(RELATIONSHIP)
            .and_then(|found| found.resolved_target(&owner)?.ok())
            .unwrap_or_else(|| "word/fontTable.xml".to_owned());
        self.package_mut().add_part(&part, CONTENT_TYPE, xml.into_bytes());
        if relationships.single_by_type(RELATIONSHIP).is_none() {
            let target = part.strip_prefix("word/").unwrap_or(&part).to_owned();
            relationships.add(RELATIONSHIP, &target, TargetMode::Internal);
            self.package_mut().set_relationships(&relationships)?;
        }
        self.note_change();
        Ok(())
    }
}

fn read_font(element: &Element) -> Option<FontEntry> {
    let name = element.attribute(Some(read::W), "name")?.trim().to_owned();
    if name.is_empty() {
        return None;
    }
    let value = |local: &str| {
        element
            .child(Some(read::W), local)
            .and_then(|child| child.attribute(Some(read::W), "val"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    Some(FontEntry {
        name,
        // Word writes several names in one, separated by commas; the first
        // is the one to try.
        alt_name: value("altName")
            .and_then(|names| names.split(',').next())
            .map(|first| first.trim().to_owned())
            .filter(|first| !first.is_empty()),
        panose: value("panose1").map(str::to_owned),
        charset: value("charset").and_then(|hex| u8::from_str_radix(hex, 16).ok()),
        class: value("family").map_or(FontClass::Auto, FontClass::from_word),
        fixed_pitch: value("pitch").and_then(|pitch| match pitch {
            "fixed" => Some(true),
            "variable" => Some(false),
            _ => None,
        }),
    })
}

/// One `w:font`, its children in the schema's order.
fn font_element(font: &FontEntry) -> Element {
    let mut element = Element::new("w:font", Some(read::W));
    element.set_namespaced_attribute("w:name", read::W, &font.name);
    let valued = |local: &str, value: &str| {
        let mut child = Element::new(&edit::name_with(Some("w"), local), Some(read::W));
        child.set_namespaced_attribute("w:val", read::W, value);
        child
    };
    if let Some(alt) = &font.alt_name {
        element.push_element(valued("altName", alt));
    }
    if let Some(panose) = &font.panose {
        element.push_element(valued("panose1", panose));
    }
    if let Some(charset) = font.charset {
        element.push_element(valued("charset", &format!("{charset:02X}")));
    }
    element.push_element(valued("family", font.class.word()));
    if let Some(fixed) = font.fixed_pitch {
        element.push_element(valued("pitch", if fixed { "fixed" } else { "variable" }));
    }
    element
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, Body, Paragraph};

    #[test]
    fn the_table_is_written_and_read_back() {
        let body = Body { blocks: vec![Block::Paragraph(Paragraph::text("x"))] };
        let mut document = Document::create(&body).expect("a document");
        assert!(document.font_table().is_empty());
        let fonts = vec![
            FontEntry {
                name: "Calibri Light".to_owned(),
                alt_name: Some("Calibri".to_owned()),
                panose: Some("020F0302020204030204".to_owned()),
                charset: Some(0),
                class: FontClass::Swiss,
                fixed_pitch: Some(false),
            },
            FontEntry {
                name: "Cambria".to_owned(),
                class: FontClass::Roman,
                ..FontEntry::default()
            },
        ];
        document.set_font_table(&fonts).expect("written");
        let saved = Document::open(&document.save().expect("saved")).expect("reopened");
        assert_eq!(saved.font_table(), fonts);
    }
}

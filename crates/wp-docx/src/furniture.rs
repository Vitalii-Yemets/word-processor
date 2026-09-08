//! Headers and footers: the parts of a document that are not the document.
//!
//! # Why they are separate parts
//!
//! A header is not text in the body — it is its own part of the package, with
//! its own relationship, pointed at from `w:sectPr`. That is what lets the same
//! header appear on every page without being stored on every page, and it is
//! why adding one means writing a part, a relationship, a content type and a
//! reference, in that order. Miss any of the four and Word opens the file and
//! quietly shows no header.
//!
//! # What a page number really is
//!
//! It is a *field*: the instruction `PAGE`, wrapped round a run holding the
//! last answer somebody worked out. The number in the file is a cache. Anything
//! that lays the document out has to work it out again, which is why the runs
//! inside a field carry their instruction — see [`crate::model::Run::field`].

use wp_opc::TargetMode;
use wp_xml::tree::{Element, XmlTree};

use crate::history::EditKind;
use crate::model::{Alignment, Block, Body, Paragraph, ParagraphProperties, Run};
use crate::{edit, read, Document, Error};

/// Content type of a header part.
const HEADER_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
/// And of a footer part.
const FOOTER_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";

/// Relationship type of a header part.
const HEADER_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
/// And of a footer.
const FOOTER_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";

/// Which of the two a call is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Furniture {
    Header,
    Footer,
}

impl Furniture {
    fn content_type(self) -> &'static str {
        match self {
            Self::Header => HEADER_CONTENT_TYPE,
            Self::Footer => FOOTER_CONTENT_TYPE,
        }
    }

    fn relationship(self) -> &'static str {
        match self {
            Self::Header => HEADER_RELATIONSHIP,
            Self::Footer => FOOTER_RELATIONSHIP,
        }
    }

    /// The `w:sectPr` child that points at the part.
    fn reference(self) -> &'static str {
        match self {
            Self::Header => "headerReference",
            Self::Footer => "footerReference",
        }
    }

    /// The root element of the part itself.
    fn root(self) -> &'static str {
        match self {
            Self::Header => "hdr",
            Self::Footer => "ftr",
        }
    }

    fn part_stem(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Footer => "footer",
        }
    }
}

/// What a ready-made header or footer holds.
///
/// Word offers a gallery of these; these are the four that are useful without
/// a designer, and the fifth that takes one away again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// One empty paragraph, which is what Word's "Blank" is.
    Blank,
    /// The page number on its own.
    PageNumber,
    /// The page number as "Page 1 of 4".
    PageOfTotal,
    /// A caption the caller supplies, such as the document's name.
    Text,
    /// No header or footer at all.
    None,
}

impl Document {
    /// The header or footer of the caret's section, if there is one.
    #[must_use]
    pub fn furniture(&self, which: Furniture) -> Option<Body> {
        self.furniture_of(which, self.section_here())
    }

    /// The header or footer of one section.
    #[must_use]
    pub fn furniture_of(&self, which: Furniture, section: usize) -> Option<Body> {
        let part = self.furniture_part_of(which, section)?;
        let text = self.package().xml_part(&part)?.ok()?;
        let tree = XmlTree::parse(&text).ok()?;
        Some(read::read_part(&tree.root))
    }

    /// The name of the package part holding the caret's section's one.
    #[must_use]
    pub fn furniture_part(&self, which: Furniture) -> Option<String> {
        self.furniture_part_of(which, self.section_here())
    }

    /// The same for one section.
    #[must_use]
    pub fn furniture_part_of(&self, which: Furniture, section: usize) -> Option<String> {
        let id = self.furniture_reference(which, section)?;
        let relationships = self.package().relationships(self.main_part()).ok()?;
        let relationship = relationships.by_id(&id)?;
        relationship.resolved_target(self.main_part())?.ok()
    }

    /// Which relationship a section's header or footer is behind.
    ///
    /// A section that names none of its own uses the one before it, which is
    /// what Word's "Link to Previous" means and what the format assumes when a
    /// section leaves the reference out.
    fn furniture_reference(&self, which: Furniture, section: usize) -> Option<String> {
        let mut index = section;
        loop {
            if let Some(id) = self.own_reference(which, index) {
                return Some(id);
            }
            index = index.checked_sub(1)?;
        }
    }

    /// The reference a section writes itself, without following the ones before
    /// it — which is what says whether it has a header of its own at all.
    fn own_reference(&self, which: Furniture, section: usize) -> Option<String> {
        crate::sections::properties_of(&self.tree().root, section).and_then(|properties| {
            properties
                .children_named(Some(read::W), which.reference())
                // Only the default one: Word also allows a different header on
                // the first page and on even pages, which this does not offer
                // yet.
                .find(|element| {
                    element.attribute(Some(read::W), "type").is_none_or(|kind| kind == "default")
                })
                .and_then(|element| element.attribute(Some(read::RELATIONSHIPS), "id"))
                .map(str::to_owned)
        })
    }

    /// The part a section names itself, if it names one.
    ///
    /// What a change writes into: a section that has been using the section
    /// before it gets a part of its own rather than writing over the header the
    /// rest of the document is printing.
    fn own_part(&self, which: Furniture, section: usize) -> Option<String> {
        let id = self.own_reference(which, section)?;
        let relationships = self.package().relationships(self.main_part()).ok()?;
        let relationship = relationships.by_id(&id)?;
        relationship.resolved_target(self.main_part())?.ok()
    }

    /// Puts a header or footer on the document, replacing whatever was there.
    ///
    /// `caption` is used only by [`Preset::Text`].
    pub fn set_furniture(
        &mut self,
        which: Furniture,
        preset: Preset,
        alignment: Alignment,
        caption: &str,
    ) -> Result<bool, Error> {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        if preset == Preset::None {
            return Ok(self.remove_furniture(which));
        }

        let body = preset_body(preset, alignment, caption);
        let xml = part_xml(which, &body)?;

        // Re-use the part if this section has one of its own, so a header
        // changed twice does not leave an orphan behind in the package. A
        // section that has only been following the one before it gets a part of
        // its own instead of writing over the header the rest of the document
        // is printing.
        let part = match self.own_part(which, self.section_here()) {
            Some(existing) => existing,
            None => self.unused_part_name(which),
        };
        self.package_mut().add_part(&part, which.content_type(), xml.into_bytes());

        let target = part.strip_prefix("word/").unwrap_or(&part).to_owned();
        let main_part = self.main_part().to_owned();
        let mut relationships = self
            .package()
            .relationships(&main_part)
            .unwrap_or_else(|_| wp_opc::Relationships::new(&main_part));

        // A part that is already pointed at keeps its relationship; a new one
        // gets a new id.
        let id = match relationships.all().iter().find(|entry| entry.target == target) {
            Some(entry) => entry.id.clone(),
            None => {
                relationships.add(which.relationship(), &target, TargetMode::Internal).id.clone()
            }
        };
        self.package_mut().set_relationships(&relationships)?;

        self.write_reference(which, &id);
        self.mark_modified();
        Ok(true)
    }

    /// Takes the header or footer off, leaving the part behind unreferenced.
    fn remove_furniture(&mut self, which: Furniture) -> bool {
        let prefix = self.prefix();
        let Some(section) = self.section_properties_mut(prefix.as_deref()) else { return false };
        if section.child(Some(read::W), which.reference()).is_none() {
            return false;
        }
        section.remove_children_named(Some(read::W), which.reference());
        self.mark_modified();
        true
    }

    /// Points `w:sectPr` at the part.
    fn write_reference(&mut self, which: Furniture, id: &str) {
        let prefix = self.prefix();
        let Some(section) = self.section_properties_mut(prefix.as_deref()) else { return };
        section.remove_children_named(Some(read::W), which.reference());

        let mut reference =
            Element::new(&edit::name_with(prefix.as_deref(), which.reference()), Some(read::W));
        reference.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "type"),
            read::W,
            "default",
        );
        reference.set_namespaced_attribute("r:id", read::RELATIONSHIPS, id);
        // The declaration goes on the element itself when the document has not
        // bound the relationships prefix anywhere the section can see.
        reference.declarations.push((Some("r".to_owned()), read::RELATIONSHIPS.to_owned()));

        // Both references come before everything else the schema allows in a
        // section, so the front is the right place.
        section.insert_element(0, reference);
    }

    /// A part name nothing in the package is using.
    fn unused_part_name(&self, which: Furniture) -> String {
        let mut index = 1usize;
        loop {
            let candidate = format!("word/{}{index}.xml", which.part_stem());
            if self.package().part(&candidate).is_none() {
                return candidate;
            }
            index += 1;
        }
    }

    /// How far the header sits from the top of the page and the footer from the
    /// bottom, in twentieths of a point, for the caret's section.
    #[must_use]
    pub fn furniture_distances(&self) -> (i32, i32) {
        self.furniture_distances_of(self.section_here())
    }

    /// The same for one section.
    #[must_use]
    pub fn furniture_distances_of(&self, section: usize) -> (i32, i32) {
        let margins = crate::sections::properties_of(&self.tree().root, section)
            .and_then(|properties| properties.child(Some(read::W), "pgMar"));
        let read_one = |name: &str| {
            margins
                .and_then(|element| element.attribute(Some(read::W), name))
                .and_then(|text| text.parse().ok())
                // Half an inch, which is what Word uses when nothing says.
                .unwrap_or(720)
        };
        (read_one("header"), read_one("footer"))
    }
}

/// The body of one of the ready-made headers and footers.
fn preset_body(preset: Preset, alignment: Alignment, caption: &str) -> Body {
    let mut paragraph = Paragraph {
        properties: ParagraphProperties {
            alignment: Some(alignment),
            ..ParagraphProperties::default()
        },
        runs: Vec::new(),
    };

    match preset {
        Preset::Blank | Preset::None => {}
        Preset::PageNumber => paragraph.runs.push(Run::field("PAGE", "1")),
        Preset::PageOfTotal => {
            paragraph.runs.push(Run::text("Page "));
            paragraph.runs.push(Run::field("PAGE", "1"));
            paragraph.runs.push(Run::text(" of "));
            paragraph.runs.push(Run::field("NUMPAGES", "1"));
        }
        Preset::Text => paragraph.runs.push(Run::text(caption)),
    }

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(paragraph));
    body
}

/// The XML of a header or footer part.
fn part_xml(which: Furniture, body: &Body) -> Result<String, Error> {
    let mut root = Element::new(&format!("w:{}", which.root()), Some(read::W));
    root.declarations.push((Some("w".to_owned()), read::W.to_owned()));
    root.declarations.push((Some("r".to_owned()), read::RELATIONSHIPS.to_owned()));

    for block in &body.blocks {
        root.push_element(edit::block_element(block, Some("w")));
    }

    let tree = XmlTree {
        standalone: Some(true),
        has_declaration: true,
        doctype: None,
        before_root: Vec::new(),
        root,
        after_root: Vec::new(),
    };
    tree.to_xml().map_err(|source| Error::Xml { part: which.part_stem().to_owned(), source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_preset_is_one_empty_paragraph() {
        let body = preset_body(Preset::Blank, Alignment::Center, "");
        assert_eq!(body.blocks.len(), 1);
        assert_eq!(body.plain_text(), "");
    }

    #[test]
    fn the_page_number_preset_is_a_field_and_not_the_digit_one() {
        let body = preset_body(Preset::PageNumber, Alignment::Center, "");
        let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!("a paragraph") };
        assert_eq!(paragraph.runs[0].field.as_deref(), Some("PAGE"));
    }

    #[test]
    fn page_of_total_asks_for_both_numbers() {
        let body = preset_body(Preset::PageOfTotal, Alignment::Center, "");
        let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!("a paragraph") };
        let fields: Vec<&str> =
            paragraph.runs.iter().filter_map(|run| run.field.as_deref()).collect();
        assert_eq!(fields, ["PAGE", "NUMPAGES"]);
    }

    #[test]
    fn a_text_preset_holds_what_it_was_given() {
        let body = preset_body(Preset::Text, Alignment::Start, "Quarterly report");
        assert_eq!(body.plain_text(), "Quarterly report");
    }
}

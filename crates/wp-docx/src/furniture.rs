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

/// Which of a section's three headers or footers a call is about.
///
/// # Why there are three
///
/// Because a book is not printed the way a letter is. The first page of a
/// chapter carries the chapter's title and no running head; the left-hand and
/// right-hand pages carry different ones, so that the reader always sees the
/// book's title on one side and the chapter's on the other. Word offers both as
/// switches — "Different First Page" and "Different Odd & Even Pages" — and a
/// document written with either of them says so, whether or not the program
/// reading it knows about them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Which {
    /// Every page the other two do not claim.
    #[default]
    Default,
    /// The first page of the section, when the section asks for one.
    First,
    /// The even-numbered pages, when the document asks for them.
    Even,
}

impl Which {
    /// What the format calls it, in the `w:type` of a reference.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::First => "first",
            Self::Even => "even",
        }
    }

    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "first" => Self::First,
            "even" => Self::Even,
            _ => Self::Default,
        }
    }
}

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

    /// The header or footer a page should be printed with.
    ///
    /// Which of a section's three it is depends on where the page falls and on
    /// what the document has asked for: the first page of the section gets the
    /// first-page one where the section asks for a different first page, an
    /// even-numbered page gets the even one where the document asks for
    /// different odd and even pages, and everything else gets the ordinary one.
    ///
    /// A section that asks for a different first page and names no first-page
    /// header has none on that page — it is not given the ordinary one. That is
    /// what the format says and what Word does: ticking the box and typing
    /// nothing leaves the first page bare, which is exactly what somebody
    /// ticking it usually wants.
    #[must_use]
    pub fn furniture_for_page(
        &self,
        kind: Furniture,
        section: usize,
        first_of_section: bool,
        page_number: usize,
    ) -> Option<Body> {
        self.furniture_of_page(
            kind,
            section,
            self.which_for_page(section, first_of_section, page_number),
        )
    }

    /// Which of the three a page falls under.
    #[must_use]
    pub fn which_for_page(
        &self,
        section: usize,
        first_of_section: bool,
        page_number: usize,
    ) -> Which {
        if first_of_section && self.different_first_page(section) {
            return Which::First;
        }
        if page_number % 2 == 0 && self.different_odd_and_even() {
            return Which::Even;
        }
        Which::Default
    }

    /// One of a section's three headers or footers.
    #[must_use]
    pub fn furniture_of_page(&self, kind: Furniture, section: usize, which: Which) -> Option<Body> {
        let part = self.furniture_part_for(kind, section, which)?;
        let text = self.package().xml_part(&part)?.ok()?;
        let tree = XmlTree::parse(&text).ok()?;
        Some(read::read_part(&tree.root))
    }

    /// The part behind one of a section's three.
    #[must_use]
    pub fn furniture_part_for(
        &self,
        kind: Furniture,
        section: usize,
        which: Which,
    ) -> Option<String> {
        let id = self.furniture_reference_for(kind, section, which)?;
        let relationships = self.package().relationships(self.main_part()).ok()?;
        let relationship = relationships.by_id(&id)?;
        relationship.resolved_target(self.main_part())?.ok()
    }

    /// Whether the section's first page has a header and footer of its own.
    #[must_use]
    pub fn different_first_page(&self, section: usize) -> bool {
        crate::sections::properties_of(&self.tree().root, section)
            .and_then(|properties| properties.child(Some(read::W), "titlePg"))
            .is_some_and(read::on_off)
    }

    /// Asks for one, or stops asking, in the caret's section.
    pub fn set_different_first_page(&mut self, on: bool) -> bool {
        if self.different_first_page(self.section_here()) == on {
            return false;
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let Some(section) = self.section_properties_mut(prefix.as_deref()) else { return false };

        section.remove_children_named(Some(read::W), "titlePg");
        if on {
            crate::page::section_child(section, prefix.as_deref(), "titlePg");
        }
        self.mark_modified();
        true
    }

    /// Whether left-hand and right-hand pages carry different ones.
    ///
    /// A property of the whole document rather than of a section, because a
    /// book is printed one way throughout.
    #[must_use]
    pub fn different_odd_and_even(&self) -> bool {
        self.setting_is_on("evenAndOddHeaders")
    }

    /// Asks for that, or stops asking.
    pub fn set_different_odd_and_even(&mut self, on: bool) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        if !self.set_setting_flag("evenAndOddHeaders", on) {
            return false;
        }
        self.mark_modified();
        true
    }

    /// Which relationship one of a section's headers or footers is behind.
    ///
    /// A section that names none of its own uses the one before it, which is
    /// what Word's "Link to Previous" means and what the format assumes when a
    /// section leaves the reference out. Each of the three is followed back on
    /// its own: a section can have its own first-page header and inherit the
    /// ordinary one.
    fn furniture_reference_for(
        &self,
        kind: Furniture,
        section: usize,
        which: Which,
    ) -> Option<String> {
        let mut index = section;
        loop {
            if let Some(id) = self.own_reference_for(kind, index, which) {
                return Some(id);
            }
            index = index.checked_sub(1)?;
        }
    }

    /// The ordinary one, which is what everything but a page asks for.
    fn furniture_reference(&self, kind: Furniture, section: usize) -> Option<String> {
        self.furniture_reference_for(kind, section, Which::Default)
    }

    /// The reference a section writes itself, without following the ones before
    /// it — which is what says whether it has a header of its own at all.
    fn own_reference_for(&self, kind: Furniture, section: usize, which: Which) -> Option<String> {
        crate::sections::properties_of(&self.tree().root, section).and_then(|properties| {
            properties
                .children_named(Some(read::W), kind.reference())
                .find(|element| {
                    // A reference that says nothing is the ordinary one.
                    element
                        .attribute(Some(read::W), "type")
                        .map_or(which == Which::Default, |named| Which::from_word(named) == which)
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
    fn own_part_for(&self, kind: Furniture, section: usize, which: Which) -> Option<String> {
        let id = self.own_reference_for(kind, section, which)?;
        let relationships = self.package().relationships(self.main_part()).ok()?;
        let relationship = relationships.by_id(&id)?;
        relationship.resolved_target(self.main_part())?.ok()
    }

    /// Whether a section names one of its own rather than following the section
    /// before it.
    ///
    /// What Word's "Link to Previous" shows: a section that names none of its
    /// own is linked, and the button is pressed in.
    #[must_use]
    pub fn has_own_furniture(&self, kind: Furniture, section: usize, which: Which) -> bool {
        self.own_reference_for(kind, section, which).is_some()
    }

    /// Puts a body of somebody else's making in as one of a section's three.
    ///
    /// What breaking a link needs: the section keeps showing what it was
    /// showing, but from a part of its own, so that changing it no longer
    /// changes the section before it.
    pub fn set_furniture_body(
        &mut self,
        kind: Furniture,
        which: Which,
        body: &Body,
    ) -> Result<bool, Error> {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let xml = part_xml(kind, body)?;

        let part = match self.own_part_for(kind, self.section_here(), which) {
            Some(existing) => existing,
            None => self.unused_part_name(kind),
        };
        self.package_mut().add_part(&part, kind.content_type(), xml.into_bytes());

        let target = part.strip_prefix("word/").unwrap_or(&part).to_owned();
        let main_part = self.main_part().to_owned();
        let mut relationships = self
            .package()
            .relationships(&main_part)
            .unwrap_or_else(|_| wp_opc::Relationships::new(&main_part));
        let id = match relationships.all().iter().find(|entry| entry.target == target) {
            Some(entry) => entry.id.clone(),
            None => {
                relationships.add(kind.relationship(), &target, TargetMode::Internal).id.clone()
            }
        };
        self.package_mut().set_relationships(&relationships)?;

        self.write_reference(kind, which, &id);
        self.mark_modified();
        Ok(true)
    }

    /// Takes a section's own reference away, so it follows the one before it.
    pub fn unset_furniture(&mut self, kind: Furniture, which: Which) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        self.remove_furniture(kind, which)
    }

    /// Puts a header or footer on the document, replacing whatever was there.
    ///
    /// `caption` is used only by [`Preset::Text`].
    pub fn set_furniture(
        &mut self,
        kind: Furniture,
        preset: Preset,
        alignment: Alignment,
        caption: &str,
    ) -> Result<bool, Error> {
        self.set_furniture_for(kind, Which::Default, preset, alignment, caption)
    }

    /// The same for one of the three a section can have.
    pub fn set_furniture_for(
        &mut self,
        kind: Furniture,
        which: Which,
        preset: Preset,
        alignment: Alignment,
        caption: &str,
    ) -> Result<bool, Error> {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        if preset == Preset::None {
            return Ok(self.remove_furniture(kind, which));
        }

        let body = preset_body(preset, alignment, caption);
        let xml = part_xml(kind, &body)?;

        // Re-use the part if this section has one of its own, so a header
        // changed twice does not leave an orphan behind in the package. A
        // section that has only been following the one before it gets a part of
        // its own instead of writing over the header the rest of the document
        // is printing.
        let part = match self.own_part_for(kind, self.section_here(), which) {
            Some(existing) => existing,
            None => self.unused_part_name(kind),
        };
        self.package_mut().add_part(&part, kind.content_type(), xml.into_bytes());

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
                relationships.add(kind.relationship(), &target, TargetMode::Internal).id.clone()
            }
        };
        self.package_mut().set_relationships(&relationships)?;

        self.write_reference(kind, which, &id);
        self.mark_modified();
        Ok(true)
    }

    /// Takes one of them off, leaving the part behind unreferenced.
    fn remove_furniture(&mut self, kind: Furniture, which: Which) -> bool {
        let prefix = self.prefix();
        let Some(section) = self.section_properties_mut(prefix.as_deref()) else { return false };
        let before = section.children_named(Some(read::W), kind.reference()).count();
        // Only the one of that type: taking the first-page header off must
        // leave the ordinary one alone.
        retain_references(section, kind.reference(), |named| named != which);
        if section.children_named(Some(read::W), kind.reference()).count() == before {
            return false;
        }
        self.mark_modified();
        true
    }

    /// Points `w:sectPr` at the part.
    fn write_reference(&mut self, kind: Furniture, which: Which, id: &str) {
        let prefix = self.prefix();
        let Some(section) = self.section_properties_mut(prefix.as_deref()) else { return };
        retain_references(section, kind.reference(), |named| named != which);

        let mut reference =
            Element::new(&edit::name_with(prefix.as_deref(), kind.reference()), Some(read::W));
        reference.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "type"),
            read::W,
            which.word(),
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

    /// How far the header sits from the top of the paper and the footer from
    /// the bottom, in twentieths of a point.
    ///
    /// Word's Header from Top and Footer from Bottom. They are not margins:
    /// the margin says where the text starts, and these say where the furniture
    /// sits in the space above and below it. A header pushed past the top
    /// margin overlaps the text, which is Word's behaviour too and is what a
    /// person asking for a deep header is asking for.
    pub fn set_furniture_distances(&mut self, header: i32, footer: i32) -> bool {
        if self.furniture_distances() == (header.max(0), footer.max(0)) {
            return false;
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let Some(section) = self.section_properties_mut(prefix.as_deref()) else { return false };

        let margins = crate::page::section_child(section, prefix.as_deref(), "pgMar");
        let name = |local: &str| edit::name_with(prefix.as_deref(), local);
        margins.set_namespaced_attribute(&name("header"), read::W, &header.max(0).to_string());
        margins.set_namespaced_attribute(&name("footer"), read::W, &footer.max(0).to_string());
        self.mark_modified();
        true
    }
}

/// Keeps only the references a test agrees to, by the type each one names.
///
/// The three references of one kind live side by side in the section, told
/// apart only by their `w:type`, so anything that changes one has to leave the
/// other two exactly where they were.
fn retain_references(section: &mut Element, local: &str, keep: impl Fn(Which) -> bool) {
    section.children.retain(|node| {
        let Some(element) = node.as_element() else { return true };
        if !element.is(Some(read::W), local) {
            return true;
        }
        let named =
            element.attribute(Some(read::W), "type").map_or(Which::Default, Which::from_word);
        keep(named)
    });
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

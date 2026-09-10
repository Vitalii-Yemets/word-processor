//! Sections: the stretches of a document that are printed on different paper.
//!
//! # What a section is
//!
//! Everything about the page rather than about the words: how big the paper is,
//! which way round it goes, how wide the margins are, how many columns the text
//! runs down, which header and footer are printed on it. A document with a
//! landscape table in the middle of a portrait report has three sections, and
//! so does one where every chapter has its own header.
//!
//! # How the format writes them
//!
//! Backwards from how anybody would guess. A section's properties are not
//! written where it begins: they are written on the *last* paragraph of it, in
//! that paragraph's own properties. The final section has nowhere to put them —
//! there is no paragraph after it — so its properties go at the end of the
//! body instead. A document with no section breaks is therefore a document with
//! one `w:sectPr`, at the end, which is why a reader that only looks there gets
//! the common case right and every other case wrong.
//!
//! # What is here
//!
//! Which blocks belong to which section and what paper each is printed on,
//! which is what the layout needs to put a landscape page in the middle of a
//! portrait document.

use wp_xml::tree::Element;

use crate::read::W;
use crate::Document;

/// What happens where one section ends and the next begins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Start {
    /// The next section starts on a new page, which is the usual break.
    #[default]
    NextPage,
    /// It carries on down the same page, which is how a document changes the
    /// number of columns partway down.
    Continuous,
    /// On the next even or the next odd page, for a book printed both sides.
    EvenPage,
    OddPage,
    /// In the next column of the same page.
    NextColumn,
}

impl Start {
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "continuous" => Self::Continuous,
            "evenPage" => Self::EvenPage,
            "oddPage" => Self::OddPage,
            "nextColumn" => Self::NextColumn,
            _ => Self::NextPage,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::NextPage => "nextPage",
            Self::Continuous => "continuous",
            Self::EvenPage => "evenPage",
            Self::OddPage => "oddPage",
            Self::NextColumn => "nextColumn",
        }
    }

    /// Whether the section begins on a page of its own.
    #[must_use]
    pub fn on_a_new_page(self) -> bool {
        !matches!(self, Self::Continuous | Self::NextColumn)
    }
}

/// The paper one section is printed on, in twentieths of a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    pub width: i32,
    pub height: i32,
    pub margin_top: i32,
    pub margin_right: i32,
    pub margin_bottom: i32,
    pub margin_left: i32,
    /// How many columns the text runs down, and the gap between them.
    pub columns: usize,
    pub column_gap: i32,
    /// How this section begins.
    pub start: Start,
}

impl Default for Setup {
    /// A4 with an inch of margin, which is what a document that says nothing
    /// is printed on.
    fn default() -> Self {
        Self {
            width: 11_906,
            height: 16_838,
            margin_top: 1440,
            margin_right: 1440,
            margin_bottom: 1440,
            margin_left: 1440,
            columns: 1,
            column_gap: 720,
            start: Start::NextPage,
        }
    }
}

impl Setup {
    /// Reads it out of a `w:sectPr`.
    #[must_use]
    pub fn read(properties: &Element) -> Self {
        let mut setup = Self::default();
        let number = |element: &Element, name: &str| -> Option<i32> {
            element.attribute(Some(W), name).and_then(|text| text.trim().parse().ok())
        };

        if let Some(size) = properties.child(Some(W), "pgSz") {
            if let Some(width) = number(size, "w") {
                setup.width = width;
            }
            if let Some(height) = number(size, "h") {
                setup.height = height;
            }
        }
        if let Some(margins) = properties.child(Some(W), "pgMar") {
            if let Some(value) = number(margins, "top") {
                setup.margin_top = value;
            }
            if let Some(value) = number(margins, "right") {
                setup.margin_right = value;
            }
            if let Some(value) = number(margins, "bottom") {
                setup.margin_bottom = value;
            }
            if let Some(value) = number(margins, "left") {
                setup.margin_left = value;
            }
        }
        if let Some(columns) = properties.child(Some(W), "cols") {
            if let Some(count) = number(columns, "num") {
                setup.columns = (count.max(1) as usize).clamp(1, 12);
            }
            if let Some(space) = number(columns, "space") {
                setup.column_gap = space;
            }
        }
        if let Some(kind) =
            properties.child(Some(W), "type").and_then(|element| element.attribute(Some(W), "val"))
        {
            setup.start = Start::from_word(kind);
        }
        setup
    }

    /// Whether the paper is wider than it is tall.
    #[must_use]
    pub fn is_landscape(&self) -> bool {
        self.width > self.height
    }
}

/// How a section's pages are numbered.
///
/// # Why a section decides this
///
/// Because a book's front matter is numbered i, ii, iii and its body starts
/// again at 1, and the two are different sections. Word keeps both facts in the
/// section's own properties: where the numbering starts again, and what the
/// numbers look like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageNumbering {
    /// The number the section's first page carries. Nothing means the numbering
    /// runs on from the section before it.
    pub start: Option<i32>,
    pub format: NumberFormat,
}

/// What a page number looks like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NumberFormat {
    #[default]
    Decimal,
    UpperRoman,
    LowerRoman,
    UpperLetter,
    LowerLetter,
}

impl NumberFormat {
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "upperRoman" => Self::UpperRoman,
            "lowerRoman" => Self::LowerRoman,
            "upperLetter" => Self::UpperLetter,
            "lowerLetter" => Self::LowerLetter,
            _ => Self::Decimal,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Decimal => "decimal",
            Self::UpperRoman => "upperRoman",
            Self::LowerRoman => "lowerRoman",
            Self::UpperLetter => "upperLetter",
            Self::LowerLetter => "lowerLetter",
        }
    }

    /// What the reader is shown for a page.
    ///
    /// Nothing sensible can be made of a number below one, and Word shows the
    /// figures themselves rather than nothing, so it falls back to them.
    #[must_use]
    pub fn of(self, number: usize) -> String {
        if number == 0 {
            return number.to_string();
        }
        match self {
            Self::Decimal => number.to_string(),
            Self::UpperRoman => roman(number),
            Self::LowerRoman => roman(number).to_lowercase(),
            Self::UpperLetter => letters(number),
            Self::LowerLetter => letters(number).to_lowercase(),
        }
    }
}

/// A number in Roman figures.
///
/// The ordinary subtractive kind, which is what a page number is written in:
/// four is IV and nine is IX, not IIII and VIIII.
#[must_use]
fn roman(number: usize) -> String {
    const FIGURES: &[(usize, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];

    let mut left = number;
    let mut out = String::new();
    for (value, figure) in FIGURES {
        while left >= *value {
            out.push_str(figure);
            left -= value;
        }
    }
    out
}

/// A number as letters: A, B, ... Z, AA, BB, and so on.
///
/// Word's sequence rather than a base-26 count: the twenty-seventh page is AA
/// and the twenty-eighth BB, which looks wrong written down and is what Word
/// prints.
#[must_use]
fn letters(number: usize) -> String {
    let count = (number - 1) / 26 + 1;
    let letter = char::from(b'A' + ((number - 1) % 26) as u8);
    core::iter::repeat_n(letter, count).collect()
}

/// One section: the blocks it covers and the paper they are printed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    /// The blocks of the body it covers, as a half-open range.
    pub first_block: usize,
    pub end_block: usize,
    pub setup: Setup,
}

impl Document {
    /// Every section of the document, in order.
    ///
    /// Always at least one: a document with no section breaks is one section
    /// covering everything.
    #[must_use]
    pub fn sections(&self) -> Vec<Section> {
        let root = &self.tree().root;
        let Some(body) = crate::read::find_body(root) else {
            return vec![Section { first_block: 0, end_block: 0, setup: Setup::default() }];
        };

        // Where each section ends, and what it says about itself.
        let mut ends: Vec<(usize, Setup)> = Vec::new();
        let mut blocks = 0usize;
        collect_ends(body, &mut blocks, &mut ends);

        // The last section's properties are at the end of the body, because
        // there is no paragraph after it to write them on.
        let last = body.child(Some(W), "sectPr").map_or_else(Setup::default, Setup::read);
        ends.push((blocks, last));

        let mut out = Vec::with_capacity(ends.len());
        let mut first = 0usize;
        for (end, setup) in ends {
            // A section ending where it began holds nothing, which happens when
            // two breaks sit together. It is kept: the paper it names is what
            // the pages between the two breaks are printed on.
            out.push(Section { first_block: first, end_block: end, setup });
            first = end;
        }
        out
    }

    /// The section a block belongs to.
    #[must_use]
    pub fn section_of_block(&self, block: usize) -> Section {
        let sections = self.sections();
        sections
            .iter()
            .find(|section| block >= section.first_block && block < section.end_block)
            .copied()
            .or_else(|| sections.last().copied())
            .unwrap_or(Section { first_block: 0, end_block: 0, setup: Setup::default() })
    }

    /// The paper the caret's section is printed on.
    #[must_use]
    pub fn setup_here(&self) -> Setup {
        self.section_of_block(self.caret().paragraph).setup
    }

    /// Which section a paragraph is in, counted from zero.
    ///
    /// The properties of a section are written on its last paragraph, so the
    /// section a paragraph belongs to is the one whose break comes first at or
    /// after it. Past the last break it is the final section, whose properties
    /// are at the end of the body.
    #[must_use]
    pub fn section_index_of(&self, paragraph: usize) -> usize {
        let breaks = breaks(&self.tree().root);
        breaks.iter().position(|(end, _)| *end >= paragraph).unwrap_or(breaks.len())
    }

    /// Which section the caret is in.
    #[must_use]
    pub fn section_here(&self) -> usize {
        self.section_index_of(self.caret().paragraph)
    }

    /// How many sections the document has.
    #[must_use]
    pub fn section_count(&self) -> usize {
        breaks(&self.tree().root).len() + 1
    }

    /// How a section numbers its pages.
    #[must_use]
    pub fn page_numbering(&self, section: usize) -> PageNumbering {
        let Some(element) = properties_of(&self.tree().root, section)
            .and_then(|properties| properties.child(Some(W), "pgNumType"))
        else {
            return PageNumbering::default();
        };
        PageNumbering {
            start: element.attribute(Some(W), "start").and_then(|text| text.trim().parse().ok()),
            format: element
                .attribute(Some(W), "fmt")
                .map_or(NumberFormat::Decimal, NumberFormat::from_word),
        }
    }

    /// Sets how the caret's section numbers its pages.
    pub fn set_page_numbering(&mut self, numbering: PageNumbering) -> bool {
        if self.page_numbering(self.section_here()) == numbering {
            return false;
        }
        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let Some(section) = self.section_properties_mut(prefix.as_deref()) else { return false };

        section.remove_children_named(Some(W), "pgNumType");
        if numbering != PageNumbering::default() {
            let element = crate::page::section_child(section, prefix.as_deref(), "pgNumType");
            if let Some(start) = numbering.start {
                element.set_namespaced_attribute(
                    &crate::edit::name_with(prefix.as_deref(), "start"),
                    W,
                    &start.to_string(),
                );
            }
            if numbering.format != NumberFormat::Decimal {
                element.set_namespaced_attribute(
                    &crate::edit::name_with(prefix.as_deref(), "fmt"),
                    W,
                    numbering.format.word(),
                );
            }
        }
        self.mark_modified();
        true
    }

    /// What each page of the document is numbered, and in what figures.
    ///
    /// Walked from the front, because a page's number depends on every page
    /// before it: the numbering runs on from section to section unless a
    /// section says to start again.
    #[must_use]
    pub fn page_numbers(&self, sections_by_page: &[usize]) -> Vec<(usize, NumberFormat)> {
        // What each section says about its page numbers, asked once per
        // section. Finding a section means finding the breaks, and finding
        // those means walking the whole document — so asking per page would
        // walk the document once for every page in it.
        let count = sections_by_page.iter().copied().max().map_or(0, |last| last + 1);
        let numbering: Vec<PageNumbering> =
            (0..count).map(|section| self.page_numbering(section)).collect();

        let mut out = Vec::with_capacity(sections_by_page.len());
        let mut number = 0usize;
        let mut previous: Option<usize> = None;

        for section in sections_by_page {
            let Some(numbering) = numbering.get(*section) else { continue };
            let first_of_section = previous != Some(*section);
            number = match numbering.start {
                Some(start) if first_of_section => start.max(0) as usize,
                _ => number + 1,
            };
            previous = Some(*section);
            out.push((number, numbering.format));
        }
        out
    }

    /// Inserts a section break at the caret, the way Layout ▸ Breaks does.
    ///
    /// What is on the far side of the caret becomes a section of its own, and
    /// `start` is how that new section begins — on a new page, straight on down
    /// the same one, or on the next even or odd page.
    ///
    /// The properties the caret's section had are written onto the paragraph the
    /// caret was in, because that paragraph now ends the first of the two; the
    /// second keeps the properties that were governing before, which is where
    /// the new break type is written. Both halves are therefore printed on the
    /// paper the one section was, which is what Word does: a break changes where
    /// the text goes, not how the page looks, until somebody changes the page.
    pub fn insert_section_break(&mut self, start: Start) -> bool {
        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);
        let prefix = self.prefix();

        // The text after the caret begins the new section, so the paragraph is
        // split first — the same split pressing Enter makes.
        if !crate::position::split_paragraph(&mut self.tree_mut().root, caret, prefix.as_deref()) {
            return false;
        }
        let ending = caret.paragraph;
        self.set_caret(crate::TextPosition::new(ending + 1, 0));

        // Whatever governs the caret now governs the second section: it keeps
        // the properties, and takes the break type just asked for.
        let Some(governing) = self.section_properties_mut(prefix.as_deref()) else {
            return false;
        };
        let kept = governing.clone();
        if start == Start::NextPage {
            // The default, which Word leaves unsaid.
            governing.remove_children_named(Some(W), "type");
        } else {
            let element = crate::page::section_child(governing, prefix.as_deref(), "type");
            element.set_namespaced_attribute(
                &crate::edit::name_with(prefix.as_deref(), "val"),
                W,
                start.word(),
            );
        }

        // And the copy goes on the paragraph that now ends the first section.
        let Some(path) = crate::position::paragraph_path(&self.tree().root, ending) else {
            return false;
        };
        let Some(paragraph) = crate::edit::element_at_path_mut(&mut self.tree_mut().root, &path)
        else {
            return false;
        };
        let properties = crate::edit::paragraph_properties_of(paragraph, prefix.as_deref());
        properties.remove_children_named(Some(W), "sectPr");
        crate::edit::insert_ordered(properties, kept, crate::edit::PARAGRAPH_PROPERTY_ORDER);
        self.mark_modified();
        true
    }
}

/// Finds every paragraph that ends a section, counting blocks as it goes.
///
/// The count has to follow exactly the same rules as reading the blocks does,
/// or a section would cover the wrong ones — so the two walks are the same
/// walk, over the same children, skipping the same wrappers.
fn collect_ends(parent: &Element, blocks: &mut usize, ends: &mut Vec<(usize, Setup)>) {
    for child in parent.child_elements() {
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        match child.local_name() {
            "p" => {
                *blocks += 1;
                // A paragraph that carries section properties is the last one
                // of its section.
                if let Some(properties) = child
                    .child(Some(W), "pPr")
                    .and_then(|properties| properties.child(Some(W), "sectPr"))
                {
                    ends.push((*blocks, Setup::read(properties)));
                }
            }
            "tbl" => *blocks += 1,
            _ if is_transparent(child) => collect_ends(child, blocks, ends),
            _ => {}
        }
    }
}

/// The wrappers that hold blocks without being one.
fn is_transparent(element: &Element) -> bool {
    element.namespace.as_deref() == Some(W)
        && matches!(element.local_name(), "sdt" | "sdtContent" | "ins" | "moveTo")
}

/// Where the section breaks are written: for each one, the paragraph it sits on
/// counted in reading order, and the path to that paragraph.
///
/// Paths rather than references, for the reason [`crate::position::paragraph_path`]
/// gives: a caller that wants to change the properties cannot hold a borrow of
/// the whole document while it looks for them.
///
/// Paragraphs rather than blocks, because that is what a caret is counted in.
/// The two only agree in a document without tables, and it is the caret that
/// has to land in the right section.
#[must_use]
pub(crate) fn breaks(root: &Element) -> Vec<(usize, Vec<usize>)> {
    let mut found = Vec::new();
    let mut path = Vec::new();
    let mut counter = 0usize;
    walk_breaks(root, &mut path, &mut counter, &mut found);
    found
}

/// The properties of one section, counted from zero.
///
/// `None` where the last section has none written at the end of the body, which
/// is a document that says nothing about its own page.
#[must_use]
pub(crate) fn properties_of(root: &Element, section: usize) -> Option<&Element> {
    let breaks = breaks(root);
    match breaks.get(section) {
        Some((_, path)) => crate::edit::element_at_path(root, path)
            .and_then(|paragraph| paragraph.child(Some(W), "pPr"))
            .and_then(|properties| properties.child(Some(W), "sectPr")),
        // Past the last break is the final section, at the end of the body.
        None => crate::read::find_body(root).and_then(|body| body.child(Some(W), "sectPr")),
    }
}

/// The same, to be written into.
///
/// What "apply to the whole document" needs: every section's properties, one
/// after another, rather than only the one the caret is in.
pub(crate) fn properties_of_mut(root: &mut Element, section: usize) -> Option<&mut Element> {
    let path = breaks(root).get(section).map(|(_, path)| path.clone());
    match path {
        Some(path) => crate::edit::element_at_path_mut(root, &path)
            .and_then(|paragraph| paragraph.child_mut(Some(W), "pPr"))
            .and_then(|properties| properties.child_mut(Some(W), "sectPr")),
        None => crate::read::find_body_mut(root).and_then(|body| body.child_mut(Some(W), "sectPr")),
    }
}

fn walk_breaks(
    element: &Element,
    path: &mut Vec<usize>,
    counter: &mut usize,
    found: &mut Vec<(usize, Vec<usize>)>,
) {
    for (index, node) in element.children.iter().enumerate() {
        let Some(child) = node.as_element() else { continue };
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        path.push(index);
        if child.is(Some(W), "p") {
            if child
                .child(Some(W), "pPr")
                .and_then(|properties| properties.child(Some(W), "sectPr"))
                .is_some()
            {
                found.push((*counter, path.clone()));
            }
            *counter += 1;
        } else {
            walk_breaks(child, path, counter, found);
        }
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_of_start_survives_being_named_and_read_back() {
        for start in
            [Start::NextPage, Start::Continuous, Start::EvenPage, Start::OddPage, Start::NextColumn]
        {
            assert_eq!(Start::from_word(start.word()), start);
        }
    }

    #[test]
    fn a_start_nobody_here_knows_begins_a_page() {
        assert_eq!(Start::from_word("something else"), Start::NextPage);
    }

    #[test]
    fn only_a_continuous_break_stays_on_the_page() {
        assert!(Start::NextPage.on_a_new_page());
        assert!(Start::EvenPage.on_a_new_page());
        assert!(!Start::Continuous.on_a_new_page());
        assert!(!Start::NextColumn.on_a_new_page());
    }

    #[test]
    fn a_setup_that_says_nothing_is_a_four_with_inch_margins() {
        let setup = Setup::default();
        assert_eq!((setup.width, setup.height), (11_906, 16_838));
        assert_eq!(setup.margin_top, 1440);
        assert_eq!(setup.columns, 1);
        assert!(!setup.is_landscape());
    }

    #[test]
    fn paper_wider_than_it_is_tall_is_landscape() {
        let setup = Setup { width: 16_838, height: 11_906, ..Setup::default() };
        assert!(setup.is_landscape());
    }

    #[test]
    fn a_setup_is_read_out_of_the_properties() {
        let mut properties = Element::new("w:sectPr", Some(W));
        let mut size = Element::new("w:pgSz", Some(W));
        size.set_namespaced_attribute("w:w", W, "16838");
        size.set_namespaced_attribute("w:h", W, "11906");
        properties.push_element(size);
        let mut margins = Element::new("w:pgMar", Some(W));
        margins.set_namespaced_attribute("w:top", W, "720");
        properties.push_element(margins);
        let mut columns = Element::new("w:cols", Some(W));
        columns.set_namespaced_attribute("w:num", W, "3");
        properties.push_element(columns);
        let mut kind = Element::new("w:type", Some(W));
        kind.set_namespaced_attribute("w:val", W, "continuous");
        properties.push_element(kind);

        let setup = Setup::read(&properties);
        assert_eq!(setup.width, 16_838);
        assert_eq!(setup.margin_top, 720);
        assert_eq!(setup.columns, 3);
        assert_eq!(setup.start, Start::Continuous);
        assert!(setup.is_landscape());
    }

    #[test]
    fn properties_that_say_nothing_leave_the_defaults() {
        let setup = Setup::read(&Element::new("w:sectPr", Some(W)));
        assert_eq!(setup, Setup::default());
    }
}

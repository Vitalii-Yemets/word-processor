//! Page setup, and the change-case command.
//!
//! # Why these are together
//!
//! They are not, really — but both are edits that reach past a run and past a
//! paragraph, and both are small. Page setup writes `w:sectPr`, the properties
//! of the section the caret is in — the paper it is printed on, its margins and
//! its columns; changing case rewrites the selected text and nothing else.

use wp_xml::tree::Element;

use crate::history::EditKind;
use crate::{edit, read, Document};

/// The named page sizes Word offers, in twentieths of a point.
///
/// Only the ones people pick: the rest of Word's list is envelope sizes.
pub const PAGE_SIZES: &[(&str, i32, i32)] = &[
    ("A4", 11906, 16838),
    ("A5", 8391, 11906),
    ("A3", 16838, 23811),
    ("Letter", 12240, 15840),
    ("Legal", 12240, 20160),
    ("Tabloid", 15840, 24480),
];

/// The margin settings Word offers by name, as top, right, bottom, left.
pub const MARGIN_PRESETS: &[(&str, i32, i32, i32, i32)] = &[
    ("Normal", 1440, 1440, 1440, 1440),
    ("Narrow", 720, 720, 720, 720),
    ("Moderate", 1440, 1080, 1440, 1080),
    ("Wide", 1440, 2880, 1440, 2880),
];

/// The order the schema requires for the children of `w:sectPr`.
const SECTION_PROPERTY_ORDER: &[&str] = &[
    "footnotePr",
    "endnotePr",
    "type",
    "pgSz",
    "pgMar",
    "paperSrc",
    "pgBorders",
    "lnNumType",
    "pgNumType",
    "cols",
    "formProt",
    "vAlign",
    "noEndnote",
    "titlePg",
    "textDirection",
    "bidi",
    "rtlGutter",
    "docGrid",
];

impl Document {
    /// What the paper is called, when it is one of the sizes with a name.
    ///
    /// Turned round or not: A4 on its side is still A4, which is what the
    /// orientation setting beside it says.
    #[must_use]
    pub fn page_size_name(&self) -> Option<&'static str> {
        let (width, height) = self.page_size();
        let (width, height) = if width > height { (height, width) } else { (width, height) };
        PAGE_SIZES
            .iter()
            .find(|(_, named_width, named_height)| {
                (width - named_width).abs() <= 2 && (height - named_height).abs() <= 2
            })
            .map(|(name, ..)| *name)
    }

    /// The paper written out, which is what Word puts under the name.
    ///
    /// In inches, because that is what the rulers of this program are marked
    /// in; following the user's own measurement units is a later piece of work.
    #[must_use]
    pub fn page_size_note(&self) -> String {
        let (width, height) = self.page_size();
        let inches = |twips: i32| f64::from(twips) / 1440.0;
        format!("{:.2}\" x {:.2}\"", inches(width), inches(height))
    }

    /// What the margins are called, when they are one of the sets with a name.
    #[must_use]
    pub fn margin_preset_name(&self) -> Option<&'static str> {
        let here = self.page_margins();
        MARGIN_PRESETS
            .iter()
            .find(|(_, top, right, bottom, left)| (*top, *right, *bottom, *left) == here)
            .map(|(name, ..)| *name)
    }

    /// The page's width and height in twentieths of a point.
    #[must_use]
    pub fn page_size(&self) -> (i32, i32) {
        let size =
            self.section_properties().and_then(|section| section.child(Some(read::W), "pgSz"));
        let read_one = |name: &str, fallback: i32| {
            size.and_then(|element| element.attribute(Some(read::W), name))
                .and_then(|text| text.parse().ok())
                .unwrap_or(fallback)
        };
        (read_one("w", 11906), read_one("h", 16838))
    }

    /// Whether the page is wider than it is tall.
    #[must_use]
    pub fn is_landscape(&self) -> bool {
        let (width, height) = self.page_size();
        width > height
    }

    /// Sets the paper size, keeping whichever way round the page is.
    ///
    /// The two numbers are always given portrait-way-round, because that is how
    /// a paper size is quoted; turning them over is what landscape means.
    pub fn set_page_size(&mut self, width: i32, height: i32) -> bool {
        let (width, height) = if self.is_landscape() { (height, width) } else { (width, height) };
        self.write_page_size(width, height)
    }

    /// Turns the page on its side, or back. Returns whether anything moved.
    pub fn set_landscape(&mut self, landscape: bool) -> bool {
        let (width, height) = self.page_size();
        if landscape == (width > height) {
            return false;
        }
        self.write_page_size(height, width)
    }

    fn write_page_size(&mut self, width: i32, height: i32) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let Some(section) = self.section_properties_mut(prefix.as_deref()) else { return false };

        let element = section_child(section, prefix.as_deref(), "pgSz");
        for (name, value) in [("w", width.to_string()), ("h", height.to_string())] {
            element.set_namespaced_attribute(
                &edit::name_with(prefix.as_deref(), name),
                read::W,
                &value,
            );
        }
        // Word wants telling which way round it is as well as the numbers, or
        // it re-derives the orientation and can end up disagreeing with itself.
        element.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "orient"),
            read::W,
            if width > height { "landscape" } else { "portrait" },
        );
        self.mark_modified();
        true
    }

    /// The page margins in twentieths of a point, as top, right, bottom, left.
    #[must_use]
    pub fn page_margins(&self) -> (i32, i32, i32, i32) {
        let margins =
            self.section_properties().and_then(|section| section.child(Some(read::W), "pgMar"));
        let read_one = |name: &str| {
            margins
                .and_then(|element| element.attribute(Some(read::W), name))
                .and_then(|text| text.parse().ok())
                .unwrap_or(1440)
        };
        (read_one("top"), read_one("right"), read_one("bottom"), read_one("left"))
    }

    /// Sets the page margins.
    pub fn set_page_margins(&mut self, top: i32, right: i32, bottom: i32, left: i32) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let Some(section) = self.section_properties_mut(prefix.as_deref()) else { return false };

        let element = section_child(section, prefix.as_deref(), "pgMar");
        for (name, value) in [("top", top), ("right", right), ("bottom", bottom), ("left", left)] {
            element.set_namespaced_attribute(
                &edit::name_with(prefix.as_deref(), name),
                read::W,
                &value.to_string(),
            );
        }
        self.mark_modified();
        true
    }

    /// The `w:sectPr` that governs the caret, which is where the page setup for
    /// the section the caret is in lives.
    ///
    /// A section's properties are written on its last paragraph, so the ones
    /// that govern a paragraph are the first written at or after it. Past the
    /// last break they are the body's own, at the end of it — which is where a
    /// document with no breaks keeps its only set.
    fn section_properties(&self) -> Option<&Element> {
        let root = &self.tree().root;
        if let Some(path) = self.section_break_path() {
            return edit::element_at_path(root, &path)
                .and_then(|paragraph| paragraph.child(Some(read::W), "pPr"))
                .and_then(|properties| properties.child(Some(read::W), "sectPr"));
        }
        body_of(root).and_then(|body| body.child(Some(read::W), "sectPr"))
    }

    /// The path to the paragraph carrying the caret's section properties, when
    /// they are written on one rather than at the end of the body.
    fn section_break_path(&self) -> Option<Vec<usize>> {
        let caret = self.caret().paragraph;
        crate::sections::breaks(&self.tree().root)
            .into_iter()
            .find(|(paragraph, _)| *paragraph >= caret)
            .map(|(_, path)| path)
    }

    /// The same, made if it is not there.
    ///
    /// Only the body's can be missing: a break exists because somebody wrote
    /// the properties that make it one.
    pub(crate) fn section_properties_mut(&mut self, prefix: Option<&str>) -> Option<&mut Element> {
        if let Some(path) = self.section_break_path() {
            return edit::element_at_path_mut(&mut self.tree_mut().root, &path)
                .and_then(|paragraph| paragraph.child_mut(Some(read::W), "pPr"))
                .and_then(|properties| properties.child_mut(Some(read::W), "sectPr"));
        }
        let body = body_of_mut(&mut self.tree_mut().root)?;
        if body.child(Some(read::W), "sectPr").is_none() {
            // It goes last in the body, which is where the schema puts it.
            body.push_element(Element::new(&edit::name_with(prefix, "sectPr"), Some(read::W)));
        }
        body.child_mut(Some(read::W), "sectPr")
    }

    /// How many columns the text flows down, and the gap between them in
    /// twentieths of a point.
    #[must_use]
    pub fn columns(&self) -> (usize, i32) {
        let columns =
            self.section_properties().and_then(|section| section.child(Some(read::W), "cols"));
        let count = columns
            .and_then(|element| element.attribute(Some(read::W), "num"))
            .and_then(|text| text.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, 12);
        let gap = columns
            .and_then(|element| element.attribute(Some(read::W), "space"))
            .and_then(|text| text.parse().ok())
            .unwrap_or(720);
        (count, gap)
    }

    /// Sets how many columns the text flows down.
    pub fn set_columns(&mut self, count: usize, gap: i32) -> bool {
        let count = count.clamp(1, 12);
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let Some(section) = self.section_properties_mut(prefix.as_deref()) else { return false };

        let element = section_child(section, prefix.as_deref(), "cols");
        element.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "num"),
            read::W,
            &count.to_string(),
        );
        element.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "space"),
            read::W,
            &gap.to_string(),
        );
        // Word writes this whenever the columns are all the same width, and
        // leaves the individual widths out. Saying so keeps it from inventing
        // its own.
        element.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "equalWidth"),
            read::W,
            "1",
        );
        self.mark_modified();
        true
    }

    /// Changes the case of the selection, the way Word's `Aa` button does.
    pub fn change_case(&mut self, wanted: CaseChange) -> bool {
        let text = self.selected_text();
        if text.is_empty() {
            return false;
        }
        let changed = wanted.applied_to(&text);
        if changed == text {
            return false;
        }
        self.paste(&changed)
    }
}

/// The `w:body`, wherever it is under the root.
fn body_of(root: &Element) -> Option<&Element> {
    if root.is(Some(read::W), "body") {
        return Some(root);
    }
    root.child_elements().find_map(body_of)
}

fn body_of_mut(root: &mut Element) -> Option<&mut Element> {
    if root.is(Some(read::W), "body") {
        return Some(root);
    }
    root.child_elements_mut().find_map(body_of_mut)
}

/// The named child of a `w:sectPr`, made in the right place if it is missing.
pub(crate) fn section_child<'a>(
    section: &'a mut Element,
    prefix: Option<&str>,
    local: &str,
) -> &'a mut Element {
    if section.child(Some(read::W), local).is_none() {
        let new = Element::new(&edit::name_with(prefix, local), Some(read::W));
        edit::insert_ordered(section, new, SECTION_PROPERTY_ORDER);
    }
    section.child_mut(Some(read::W), local).expect("just inserted, or already there")
}

/// What the change-case button does next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaseChange {
    #[default]
    Sentence,
    Lower,
    Upper,
    Capitalize,
    Toggle,
}

impl CaseChange {
    /// The list the button cycles through, in Word's order.
    pub const ALL: &'static [CaseChange] = &[
        CaseChange::Sentence,
        CaseChange::Lower,
        CaseChange::Upper,
        CaseChange::Capitalize,
        CaseChange::Toggle,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sentence => "Sentence case",
            Self::Lower => "lowercase",
            Self::Upper => "UPPERCASE",
            Self::Capitalize => "Capitalize Each Word",
            Self::Toggle => "tOGGLE cASE",
        }
    }

    /// The next one round, so one button can offer all five.
    #[must_use]
    pub fn next(self) -> Self {
        let position = Self::ALL.iter().position(|value| *value == self).unwrap_or(0);
        Self::ALL[(position + 1) % Self::ALL.len()]
    }

    /// The text with this case applied.
    #[must_use]
    pub fn applied_to(self, text: &str) -> String {
        match self {
            Self::Lower => text.to_lowercase(),
            Self::Upper => text.to_uppercase(),
            // Whichever case each letter is in, the other one. A character with
            // no case of its own is left alone.
            Self::Toggle => text
                .chars()
                .flat_map(|character| {
                    if character.is_uppercase() {
                        character.to_lowercase().collect::<Vec<_>>()
                    } else {
                        character.to_uppercase().collect::<Vec<_>>()
                    }
                })
                .collect(),
            Self::Sentence => cased(text, |character| matches!(character, '.' | '!' | '?')),
            Self::Capitalize => {
                cased(text, |character| character.is_whitespace() || character == '-')
            }
        }
    }
}

/// Lower-cases everything and puts a capital after each break.
fn cased(text: &str, is_break: impl Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut starting = true;
    for character in text.chars() {
        if starting && character.is_alphabetic() {
            out.extend(character.to_uppercase());
            starting = false;
        } else {
            out.extend(character.to_lowercase());
        }
        if is_break(character) {
            starting = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upper_and_lower_are_what_they_say() {
        assert_eq!(CaseChange::Upper.applied_to("one Two"), "ONE TWO");
        assert_eq!(CaseChange::Lower.applied_to("One TWO"), "one two");
    }

    #[test]
    fn sentence_case_capitalizes_after_a_full_stop() {
        assert_eq!(
            CaseChange::Sentence.applied_to("one two. THREE four! five"),
            "One two. Three four! Five"
        );
    }

    #[test]
    fn capitalize_each_word_does_exactly_that() {
        assert_eq!(CaseChange::Capitalize.applied_to("the quick-brown fox"), "The Quick-Brown Fox");
    }

    #[test]
    fn toggle_case_swaps_every_letter() {
        assert_eq!(CaseChange::Toggle.applied_to("Hello World"), "hELLO wORLD");
    }

    #[test]
    fn a_character_with_no_case_is_left_alone() {
        assert_eq!(CaseChange::Toggle.applied_to("a1б!"), "A1Б!");
        assert_eq!(CaseChange::Upper.applied_to("привет"), "ПРИВЕТ");
    }

    #[test]
    fn the_cycle_comes_back_round_to_where_it_started() {
        let mut wanted = CaseChange::Sentence;
        for _ in 0..CaseChange::ALL.len() {
            wanted = wanted.next();
        }
        assert_eq!(wanted, CaseChange::Sentence);
    }
}

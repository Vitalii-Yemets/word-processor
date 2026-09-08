//! Line numbers, hyphenation, the colour of the page, and who may edit it.
//!
//! # Four unrelated things in one place
//!
//! They have nothing in common to a reader. They have everything in common to
//! this program: each is one switch, written once, that changes how the whole
//! document behaves — and each of them lives in a different part of the file.
//! Line numbers are section properties; hyphenation and protection are
//! settings; the page colour is a child of the document itself and a setting
//! saying to honour it.
//!
//! Keeping them together is what keeps four one-line features from becoming
//! four modules.

use wp_xml::tree::Element;

use crate::history::EditKind;
use crate::{edit, page, read, Document};

/// Where the numbering starts again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Restart {
    /// Once through the document, never starting again.
    #[default]
    Continuous,
    NewPage,
    NewSection,
}

impl Restart {
    #[must_use]
    fn word(self) -> &'static str {
        match self {
            Self::Continuous => "continuous",
            Self::NewPage => "newPage",
            Self::NewSection => "newSection",
        }
    }

    #[must_use]
    fn from_word(word: &str) -> Self {
        match word {
            "newPage" => Self::NewPage,
            "newSection" => Self::NewSection,
            _ => Self::Continuous,
        }
    }

    /// What a person is shown when picking one.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Continuous => "Continuous",
            Self::NewPage => "Restart Each Page",
            Self::NewSection => "Restart Each Section",
        }
    }
}

/// How lines are numbered down the margin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineNumbers {
    /// Print every nth number — 1 numbers every line, 5 every fifth.
    pub count_by: u32,
    /// The number the first line gets.
    pub start: u32,
    pub restart: Restart,
    /// How far the number sits from the text, in twentieths of a point.
    pub distance: Option<i32>,
}

impl Default for LineNumbers {
    fn default() -> Self {
        Self { count_by: 1, start: 1, restart: Restart::Continuous, distance: None }
    }
}

/// What a reader is allowed to do to a protected document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditMode {
    /// Nothing at all.
    #[default]
    ReadOnly,
    /// Only leave comments.
    Comments,
    /// Edit, but every change is recorded.
    TrackedChanges,
    /// Only fill in form fields.
    Forms,
}

impl EditMode {
    #[must_use]
    fn word(self) -> &'static str {
        match self {
            Self::ReadOnly => "readOnly",
            Self::Comments => "comments",
            Self::TrackedChanges => "trackedChanges",
            Self::Forms => "forms",
        }
    }

    #[must_use]
    fn from_word(word: &str) -> Option<Self> {
        match word {
            "readOnly" => Some(Self::ReadOnly),
            "comments" => Some(Self::Comments),
            "trackedChanges" => Some(Self::TrackedChanges),
            "forms" => Some(Self::Forms),
            _ => None,
        }
    }

    /// What a person is shown when picking one.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "No changes (Read only)",
            Self::Comments => "Comments",
            Self::TrackedChanges => "Tracked changes",
            Self::Forms => "Filling in forms",
        }
    }

    /// Every one that can be picked, in Word's order.
    pub const ALL: &'static [Self] =
        &[Self::ReadOnly, Self::Comments, Self::TrackedChanges, Self::Forms];
}

impl Document {
    // --- Line numbers ---------------------------------------------------------

    /// How the lines are numbered, if they are.
    #[must_use]
    pub fn line_numbers(&self) -> Option<LineNumbers> {
        let section = self.section_element()?;
        let element = section.child(Some(read::W), "lnNumType")?;
        let number = |name: &str, fallback: u32| {
            element
                .attribute(Some(read::W), name)
                .and_then(|text| text.parse().ok())
                .unwrap_or(fallback)
        };
        Some(LineNumbers {
            count_by: number("countBy", 1).max(1),
            start: number("start", 1),
            restart: Restart::from_word(
                element.attribute(Some(read::W), "restart").unwrap_or_default(),
            ),
            distance: element.attribute(Some(read::W), "distance").and_then(|t| t.parse().ok()),
        })
    }

    /// Numbers the lines, or stops numbering them.
    pub fn set_line_numbers(&mut self, wanted: Option<LineNumbers>) -> bool {
        if self.line_numbers() == wanted {
            return false;
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(section) = self.section_properties_mut(prefix.as_deref()) else { return false };
        let Some(wanted) = wanted else {
            section.remove_children_named(Some(read::W), "lnNumType");
            self.mark_modified();
            return true;
        };

        let element = page::section_child(section, prefix.as_deref(), "lnNumType");
        let name = |local: &str| edit::name_with(prefix.as_deref(), local);
        element.set_namespaced_attribute(&name("countBy"), read::W, &wanted.count_by.to_string());
        element.set_namespaced_attribute(&name("start"), read::W, &wanted.start.to_string());
        element.set_namespaced_attribute(&name("restart"), read::W, wanted.restart.word());
        match wanted.distance {
            Some(distance) => {
                element.set_namespaced_attribute(&name("distance"), read::W, &distance.to_string())
            }
            None => element.remove_attribute(&name("distance")),
        }

        self.mark_modified();
        true
    }

    // --- Hyphenation ----------------------------------------------------------

    /// Whether long words are broken across lines.
    #[must_use]
    pub fn automatic_hyphenation(&self) -> bool {
        self.setting_is_on("autoHyphenation")
    }

    /// Turns hyphenation on or off.
    pub fn set_automatic_hyphenation(&mut self, on: bool) -> bool {
        self.set_setting_flag("autoHyphenation", on)
    }

    /// Whether words in capitals are left alone.
    #[must_use]
    pub fn hyphenate_capitals(&self) -> bool {
        !self.setting_is_on("doNotHyphenateCaps")
    }

    /// Leaves words in capitals alone, or stops doing so.
    pub fn set_hyphenate_capitals(&mut self, on: bool) -> bool {
        self.set_setting_flag("doNotHyphenateCaps", !on)
    }

    /// How close to the margin a word has to come before it is broken, in
    /// twentieths of a point.
    #[must_use]
    pub fn hyphenation_zone(&self) -> Option<i32> {
        self.setting_value("hyphenationZone").and_then(|value| value.parse().ok())
    }

    /// Sets that distance.
    pub fn set_hyphenation_zone(&mut self, twips: Option<i32>) -> bool {
        self.set_setting_value("hyphenationZone", twips.map(|value| value.to_string()).as_deref())
    }

    // --- The colour of the page -----------------------------------------------

    /// The colour behind the text, as six hex digits.
    #[must_use]
    pub fn page_color(&self) -> Option<String> {
        let background = self.tree().root.child(Some(read::W), "background")?;
        let color = background.attribute(Some(read::W), "color")?;
        if color.eq_ignore_ascii_case("auto") || color.is_empty() {
            return None;
        }
        Some(color.to_owned())
    }

    /// Colours the page, or takes the colour off again.
    ///
    /// Two things are written, not one: the colour on the document, and the
    /// setting that says to show it. Word writes both, and without the second
    /// it shows a white page and no sign of anything wrong.
    pub fn set_page_color(&mut self, color: Option<&str>) -> bool {
        let wanted = color.map(str::to_owned);
        if self.page_color() == wanted {
            return false;
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let root = &mut self.tree_mut().root;
        root.remove_children_named(Some(read::W), "background");
        if let Some(color) = &wanted {
            let mut background =
                Element::new(&edit::name_with(prefix.as_deref(), "background"), Some(read::W));
            background.set_namespaced_attribute(
                &edit::name_with(prefix.as_deref(), "color"),
                read::W,
                color,
            );
            // The schema puts it before the body, and Word will not read one
            // written after it.
            root.insert_element(0, background);
        }

        self.set_setting_flag("displayBackgroundShape", wanted.is_some());
        self.mark_modified();
        true
    }

    // --- Who may edit it ------------------------------------------------------

    /// What a reader is allowed to do, if the document says.
    #[must_use]
    pub fn protection(&self) -> Option<EditMode> {
        let root = self.settings_root()?;
        let element = root.child(Some(read::W), "documentProtection")?;
        // Written but not enforced means Word ignores it, and so does this.
        if matches!(
            element.attribute(Some(read::W), "enforcement"),
            None | Some("0" | "false" | "off")
        ) {
            return None;
        }
        EditMode::from_word(element.attribute(Some(read::W), "edit").unwrap_or_default())
    }

    /// Restricts editing, or lifts the restriction.
    ///
    /// There is no password. A password on a `.docx` protects nothing — the
    /// file says so itself, in plain text, and any program may ignore it —
    /// so offering one would be claiming a safety this cannot give.
    pub fn set_protection(&mut self, wanted: Option<EditMode>) -> bool {
        if self.protection() == wanted {
            return false;
        }
        let Some(mut root) = self.settings_root() else { return false };
        root.remove_children_named(Some(read::W), "documentProtection");

        if let Some(mode) = wanted {
            let prefix = self.prefix();
            let name = |local: &str| edit::name_with(prefix.as_deref(), local);
            let mut element = Element::new(&name("documentProtection"), Some(read::W));
            element.set_namespaced_attribute(&name("edit"), read::W, mode.word());
            element.set_namespaced_attribute(&name("enforcement"), read::W, "1");
            edit::insert_ordered(&mut root, element, crate::settings::SETTINGS_ORDER);
        }

        if !self.save_settings_root(root) {
            return false;
        }
        self.mark_modified();
        true
    }

    /// The body's `w:sectPr`, read-only.
    fn section_element(&self) -> Option<&Element> {
        fn body_of(element: &Element) -> Option<&Element> {
            if element.is(Some(read::W), "body") {
                return Some(element);
            }
            element.child_elements().find_map(body_of)
        }
        body_of(&self.tree().root)?.child(Some(read::W), "sectPr")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_restart_survives_being_written_and_read_back() {
        for restart in [Restart::Continuous, Restart::NewPage, Restart::NewSection] {
            assert_eq!(Restart::from_word(restart.word()), restart);
        }
    }

    #[test]
    fn every_edit_mode_survives_being_written_and_read_back() {
        for mode in EditMode::ALL {
            assert_eq!(EditMode::from_word(mode.word()), Some(*mode));
        }
    }

    #[test]
    fn an_unknown_word_is_not_a_mode() {
        assert_eq!(EditMode::from_word("something else"), None);
        assert_eq!(Restart::from_word("something else"), Restart::Continuous);
    }

    #[test]
    fn numbering_starts_at_one_and_counts_every_line() {
        let numbers = LineNumbers::default();
        assert_eq!(numbers.count_by, 1);
        assert_eq!(numbers.start, 1);
    }
}

//! Word's Asian Layout: a run set across a vertical line, or as two lines in
//! one.
//!
//! `w:eastAsianLayout` on a run. Two things are said on it, and each is a
//! feature Word's Asian Layout menu offers by name:
//!
//! * **Horizontal in Vertical** — `w:vert`. In a line that runs down the
//!   page a number lies on its side like every Latin letter, and a
//!   two-figure year or a page number reads badly that way. This sets the run
//!   upright and across, inside the column, so the reader sees "12" the way
//!   up they would in a horizontal line. `w:vertCompress` squeezes it to the
//!   width of one square when it is wider, which is what keeps the column
//!   the same width as its neighbours.
//! * **Two Lines in One** — `w:combine`. The run is set as two half-height
//!   lines stacked inside the height of one, the way a gloss is fitted into
//!   a line of a classical text, with the brackets `w:combineBrackets` names
//!   round the pair.
//!
//! Neither changes the text: the characters are what they were, and a
//! search finds them. What changes is how the layout places them — see the
//! layout crate, which reads this off the resolved run.

use wp_xml::tree::Element;

use crate::edit::name_with;
use crate::read::{on_off_value, W};

/// The brackets set round a run of two lines in one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CombineBrackets {
    #[default]
    None,
    /// `round`: ( and ).
    Round,
    /// `square`: [ and ].
    Square,
    /// `angle`: < and >.
    Angle,
    /// `curly`: { and }.
    Curly,
}

impl CombineBrackets {
    /// What the file calls it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Round => "round",
            Self::Square => "square",
            Self::Angle => "angle",
            Self::Curly => "curly",
        }
    }

    /// And back.
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "round" => Self::Round,
            "square" => Self::Square,
            "angle" => Self::Angle,
            "curly" => Self::Curly,
            _ => Self::None,
        }
    }

    /// The two characters set either side of the pair of lines, which are
    /// the fullwidth ones: a bracket the height of the line, not of a Latin
    /// letter.
    #[must_use]
    pub fn characters(self) -> Option<(char, char)> {
        match self {
            Self::None => None,
            Self::Round => Some(('（', '）')),
            Self::Square => Some(('［', '］')),
            Self::Angle => Some(('〈', '〉')),
            Self::Curly => Some(('｛', '｝')),
        }
    }

    /// What the dialog calls it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Round => "( )",
            Self::Square => "[ ]",
            Self::Angle => "< >",
            Self::Curly => "{ }",
        }
    }

    pub const ALL: &'static [Self] =
        &[Self::None, Self::Round, Self::Square, Self::Angle, Self::Curly];
}

/// What `w:eastAsianLayout` says about a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EastAsianLayout {
    /// Horizontal in Vertical: the run stands upright and reads across
    /// inside a line that runs down the page.
    pub horizontal_in_vertical: bool,
    /// And is squeezed to the width of one square when it is wider.
    pub fit_in_line: bool,
    /// Two Lines in One: the run is set as two half-height lines inside the
    /// height of one.
    pub two_lines_in_one: bool,
    /// The brackets round those two lines.
    pub brackets: CombineBrackets,
}

impl EastAsianLayout {
    /// Whether nothing is asked for, so nothing need be written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Reads the element.
    #[must_use]
    pub fn read(element: &Element) -> Self {
        let flag = |name: &str| {
            element.attribute(Some(W), name).is_some_and(|value| on_off_value(value.trim()))
        };
        Self {
            horizontal_in_vertical: flag("vert"),
            fit_in_line: flag("vertCompress"),
            two_lines_in_one: flag("combine"),
            brackets: element
                .attribute(Some(W), "combineBrackets")
                .map(CombineBrackets::from_word)
                .unwrap_or_default(),
        }
    }

    /// Writes it, with only what is asked for on it: the format's own
    /// default for every attribute is off, and Word writes the on ones.
    #[must_use]
    pub fn element(&self, prefix: Option<&str>) -> Element {
        let mut element = Element::new(&name_with(prefix, "eastAsianLayout"), Some(W));
        let mut set = |name: &str, value: &str| {
            element.set_namespaced_attribute(&name_with(prefix, name), W, value);
        };
        if self.two_lines_in_one {
            set("combine", "1");
            if self.brackets != CombineBrackets::None {
                set("combineBrackets", self.brackets.word());
            }
        }
        if self.horizontal_in_vertical {
            set("vert", "1");
            if self.fit_in_line {
                set("vertCompress", "1");
            }
        }
        element
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_is_what_is_read() {
        let layouts = [
            EastAsianLayout { horizontal_in_vertical: true, ..EastAsianLayout::default() },
            EastAsianLayout {
                horizontal_in_vertical: true,
                fit_in_line: true,
                ..EastAsianLayout::default()
            },
            EastAsianLayout {
                two_lines_in_one: true,
                brackets: CombineBrackets::Square,
                ..EastAsianLayout::default()
            },
        ];
        for layout in layouts {
            let element = layout.element(Some("w"));
            assert_eq!(EastAsianLayout::read(&element), layout, "{layout:?}");
        }
        assert!(EastAsianLayout::default().is_empty());
    }

    #[test]
    fn the_attributes_are_spelt_the_way_word_spells_them() {
        let layout = EastAsianLayout {
            two_lines_in_one: true,
            brackets: CombineBrackets::Round,
            horizontal_in_vertical: true,
            fit_in_line: true,
        };
        let element = layout.element(Some("w"));
        assert_eq!(element.attribute(Some(W), "combine"), Some("1"));
        assert_eq!(element.attribute(Some(W), "combineBrackets"), Some("round"));
        assert_eq!(element.attribute(Some(W), "vert"), Some("1"));
        assert_eq!(element.attribute(Some(W), "vertCompress"), Some("1"));
        // Off is said by leaving the attribute out.
        let plain = EastAsianLayout { two_lines_in_one: true, ..EastAsianLayout::default() };
        let element = plain.element(Some("w"));
        assert_eq!(element.attribute(Some(W), "combineBrackets"), None);
        assert_eq!(element.attribute(Some(W), "vert"), None);
    }

    #[test]
    fn a_file_that_says_off_is_read_as_off() {
        let mut element = Element::new("w:eastAsianLayout", Some(W));
        element.set_namespaced_attribute("w:vert", W, "0");
        element.set_namespaced_attribute("w:combine", W, "true");
        let read = EastAsianLayout::read(&element);
        assert!(!read.horizontal_in_vertical);
        assert!(read.two_lines_in_one);
    }
}

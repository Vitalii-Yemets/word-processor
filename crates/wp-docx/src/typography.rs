//! The character formatting behind Word's Font dialog that is not simply on or
//! off: how wide the letters are drawn, how far apart, how far off the line,
//! and which of the font's own alternate forms are asked for.
//!
//! # Two namespaces, for one dialog
//!
//! Word's Font dialog has two tabs, and the split between them is very nearly
//! the split between two eras of the format. The Font tab sets properties that
//! have been in WordprocessingML since it was written down: `w:caps`,
//! `w:smallCaps`, `w:vanish`, `w:dstrike`. The Advanced tab sets two kinds of
//! thing: character spacing — `w:w`, `w:spacing`, `w:position`, `w:kern`, also
//! standard — and OpenType features, which are newer than the standard and so
//! live in Microsoft's `w14` namespace beside the text effects. See
//! [`crate::effects`] for why that is allowed and how it is declared.
//!
//! # What the numbers are measured in
//!
//! Every one of them is measured differently, and the format gives no hint:
//!
//! | Property   | Element      | Unit                | Word shows           |
//! |------------|--------------|---------------------|----------------------|
//! | Scale      | `w:w`        | per cent            | per cent             |
//! | Spacing    | `w:spacing`  | twentieths of point | points, to 2 places  |
//! | Position   | `w:position` | half-points         | points, to 1 place   |
//! | Kerning    | `w:kern`     | half-points         | points               |
//!
//! Spacing and position are signed: text can be condensed as well as expanded,
//! and lowered as well as raised. Kerning is a threshold rather than an amount
//! — Word kerns text at or above that size and leaves smaller text alone,
//! because kerning small text costs more than it is worth — and zero means off.

use wp_xml::tree::Element;

use crate::effects::{W14, W14_PREFIX};

/// One hundred per cent: letters at the width the font drew them.
pub const NORMAL_SCALE: u32 = 100;

/// Which ligatures the font is asked for.
///
/// Word offers four of the five the format allows; the fifth, contextual
/// alternates, it puts under a tick box of its own, because it is not a
/// ligature at all — it is one letter chosen to suit its neighbours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ligatures {
    /// None at all: every letter as it is.
    #[default]
    None,
    /// The ones a typeface expects to be used — `fi`, `fl` and their like.
    Standard,
    /// Those, and the ones that only apply next to particular letters.
    StandardContextual,
    /// Those, and the ornamental ones a font offers but does not assume.
    HistoricalDiscretional,
    /// Everything the font has.
    All,
}

impl Ligatures {
    /// What Word's list offers, in Word's order.
    pub const CHOICES: &'static [Self] =
        &[Self::None, Self::Standard, Self::StandardContextual, Self::HistoricalDiscretional];

    /// What the list calls it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Standard => "Standard only",
            Self::StandardContextual => "Standard and Contextual",
            Self::HistoricalDiscretional => "Historical and Discretionary",
            Self::All => "All",
        }
    }

    /// What the format writes.
    #[must_use]
    pub fn to_attribute(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Standard => "standard",
            Self::StandardContextual => "standardContextual",
            Self::HistoricalDiscretional => "historicalDiscretional",
            Self::All => "all",
        }
    }

    #[must_use]
    pub fn from_attribute(value: &str) -> Self {
        match value {
            "standard" => Self::Standard,
            "standardContextual" => Self::StandardContextual,
            "historicalDiscretional" => Self::HistoricalDiscretional,
            "all" => Self::All,
            _ => Self::None,
        }
    }

    /// The OpenType features this asks the font for, as their four-letter tags.
    ///
    /// `rlig` is left out: a required ligature is required, and is applied
    /// whatever this says. See [`crate::model::RunProperties`].
    #[must_use]
    pub fn features(self) -> &'static [&'static [u8; 4]] {
        match self {
            Self::None => &[],
            Self::Standard => &[b"liga"],
            Self::StandardContextual => &[b"liga", b"clig"],
            Self::HistoricalDiscretional => &[b"liga", b"clig", b"hlig", b"dlig"],
            Self::All => &[b"liga", b"clig", b"hlig", b"dlig"],
        }
    }
}

/// Whether digits are all one width, or each its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NumberSpacing {
    /// Whatever the font was built to do.
    #[default]
    Default,
    /// Each digit as wide as it wants to be, which reads better in a sentence.
    Proportional,
    /// Every digit the same width, which is the only way a column of figures
    /// lines up.
    Tabular,
}

impl NumberSpacing {
    pub const CHOICES: &'static [Self] = &[Self::Default, Self::Proportional, Self::Tabular];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Proportional => "Proportional",
            Self::Tabular => "Tabular",
        }
    }

    #[must_use]
    pub fn to_attribute(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Proportional => "proportional",
            Self::Tabular => "tabular",
        }
    }

    #[must_use]
    pub fn from_attribute(value: &str) -> Self {
        match value {
            "proportional" => Self::Proportional,
            "tabular" => Self::Tabular,
            _ => Self::Default,
        }
    }

    #[must_use]
    pub fn feature(self) -> Option<&'static [u8; 4]> {
        match self {
            Self::Default => None,
            Self::Proportional => Some(b"pnum"),
            Self::Tabular => Some(b"tnum"),
        }
    }
}

/// Whether digits all stand at cap height, or hang below the line like letters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NumberForms {
    #[default]
    Default,
    /// All the same height, which is what most people expect of a number.
    Lining,
    /// Some tall, some short, some below the line — a number set to read as
    /// part of a sentence rather than as a figure.
    OldStyle,
}

impl NumberForms {
    pub const CHOICES: &'static [Self] = &[Self::Default, Self::Lining, Self::OldStyle];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Lining => "Lining",
            Self::OldStyle => "Old-style",
        }
    }

    #[must_use]
    pub fn to_attribute(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Lining => "lining",
            Self::OldStyle => "oldStyle",
        }
    }

    #[must_use]
    pub fn from_attribute(value: &str) -> Self {
        match value {
            "lining" => Self::Lining,
            "oldStyle" => Self::OldStyle,
            _ => Self::Default,
        }
    }

    #[must_use]
    pub fn feature(self) -> Option<&'static [u8; 4]> {
        match self {
            Self::Default => None,
            Self::Lining => Some(b"lnum"),
            Self::OldStyle => Some(b"onum"),
        }
    }
}

/// Everything on the Advanced tab that is asked of the font rather than of the
/// layout: which of its alternate glyphs to use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenType {
    pub ligatures: Ligatures,
    pub number_spacing: NumberSpacing,
    pub number_forms: NumberForms,
    /// A font may carry up to twenty sets of alternate letters, numbered. Word
    /// offers them as a list and applies one; the format allows several, so
    /// several are carried.
    pub stylistic_sets: Vec<u8>,
    /// One letter chosen to suit the letters beside it, which is a different
    /// thing from a ligature: nothing is joined, one glyph is swapped.
    pub contextual_alternates: bool,
}

impl OpenType {
    /// Whether nothing is asked for, so nothing need be written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Every feature tag this asks the font for, in the order they are applied.
    ///
    /// Order matters: a ligature is made of glyphs, so the glyphs have to have
    /// been chosen first. Word applies the number features before the ligature
    /// ones for the same reason.
    #[must_use]
    pub fn features(&self) -> Vec<[u8; 4]> {
        let mut out: Vec<[u8; 4]> = Vec::new();
        if let Some(tag) = self.number_forms.feature() {
            out.push(*tag);
        }
        if let Some(tag) = self.number_spacing.feature() {
            out.push(*tag);
        }
        for set in &self.stylistic_sets {
            // ss01 to ss20, written out as the tag the font is asked for.
            if (1..=20).contains(set) {
                let tens = b'0' + set / 10;
                let units = b'0' + set % 10;
                out.push([b's', b's', tens, units]);
            }
        }
        if self.contextual_alternates {
            out.push(*b"calt");
        }
        for tag in self.ligatures.features() {
            out.push(**tag);
        }
        out
    }
}

/// Reads whatever OpenType features a `w:rPr` asks for.
#[must_use]
pub fn read_open_type(properties: &Element) -> Option<OpenType> {
    let mut found = OpenType::default();
    let mut any = false;

    for property in properties.child_elements() {
        if property.namespace.as_deref() != Some(W14) {
            continue;
        }
        let value = property.attribute(Some(W14), "val");
        match property.local_name() {
            "ligatures" => {
                found.ligatures = Ligatures::from_attribute(value.unwrap_or("none"));
                any = true;
            }
            "numSpacing" => {
                found.number_spacing = NumberSpacing::from_attribute(value.unwrap_or("default"));
                any = true;
            }
            "numForm" => {
                found.number_forms = NumberForms::from_attribute(value.unwrap_or("default"));
                any = true;
            }
            "cntxtAlts" => {
                found.contextual_alternates = value.is_none_or(is_on);
                any = true;
            }
            "stylisticSets" => {
                for set in property.child_elements() {
                    if set.local_name() != "styleSet" {
                        continue;
                    }
                    if let Some(id) = set.attribute(Some(W14), "id").and_then(|id| id.parse().ok())
                    {
                        found.stylistic_sets.push(id);
                        any = true;
                    }
                }
            }
            _ => {}
        }
    }

    any.then_some(found)
}

/// Takes whatever OpenType features a `w:rPr` asks for back off it.
///
/// Written separately from [`write_open_type`] because a change that asks for
/// nothing still has to take away what was asked for before: "no ligatures" is
/// an answer, and it is spelt by the absence of the element.
pub fn remove_open_type(properties: &mut Element) {
    for local in ["ligatures", "numSpacing", "numForm", "cntxtAlts", "stylisticSets"] {
        properties.remove_children_named(Some(W14), local);
    }
}

/// Writes the OpenType features into a `w:rPr`, in the namespace of their own.
pub fn write_open_type(properties: &mut Element, wanted: &OpenType) {
    if wanted.is_empty() {
        return;
    }
    let mut valued = |local: &str, value: &str| {
        let mut element = Element::new(&format!("{W14_PREFIX}:{local}"), Some(W14));
        element.set_namespaced_attribute(&format!("{W14_PREFIX}:val"), W14, value);
        properties.push_element(element);
    };

    if wanted.ligatures != Ligatures::default() {
        valued("ligatures", wanted.ligatures.to_attribute());
    }
    if wanted.number_spacing != NumberSpacing::default() {
        valued("numSpacing", wanted.number_spacing.to_attribute());
    }
    if wanted.number_forms != NumberForms::default() {
        valued("numForm", wanted.number_forms.to_attribute());
    }
    if wanted.contextual_alternates {
        valued("cntxtAlts", "1");
    }
    if !wanted.stylistic_sets.is_empty() {
        let mut sets = Element::new(&format!("{W14_PREFIX}:stylisticSets"), Some(W14));
        for id in &wanted.stylistic_sets {
            let mut one = Element::new(&format!("{W14_PREFIX}:styleSet"), Some(W14));
            one.set_namespaced_attribute(&format!("{W14_PREFIX}:id"), W14, &id.to_string());
            sets.push_element(one);
        }
        properties.push_element(sets);
    }
}

/// Whether a `w:val` that stands for on or off says on.
///
/// The format allows four spellings of each, and a missing attribute means on
/// — which is the trap: `<w14:cntxtAlts/>` turns them on rather than off.
fn is_on(value: &str) -> bool {
    !matches!(value.trim(), "0" | "false" | "off")
}

/// Declares what a document using these needs on its root, so that it still
/// opens in a reader that has never heard of them.
///
/// The same declaration [`crate::effects`] needs, and for the same reason: both
/// live under `w14`, and both are declared ignorable so an older reader skips
/// what it does not know instead of refusing the file.
pub(crate) fn declare_namespace(root: &mut Element) {
    crate::effects::declare_namespace(root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_xml::tree::Element;

    fn properties() -> Element {
        Element::new("w:rPr", Some("http://schemas.openxmlformats.org/wordprocessingml/2006/main"))
    }

    #[test]
    fn a_run_that_asks_for_nothing_reads_as_nothing() {
        assert_eq!(read_open_type(&properties()), None);
    }

    #[test]
    fn what_is_written_is_what_is_read_back() {
        let wanted = OpenType {
            ligatures: Ligatures::StandardContextual,
            number_spacing: NumberSpacing::Tabular,
            number_forms: NumberForms::OldStyle,
            stylistic_sets: vec![1, 7],
            contextual_alternates: true,
        };
        let mut element = properties();
        write_open_type(&mut element, &wanted);
        assert_eq!(read_open_type(&element), Some(wanted));
    }

    #[test]
    fn nothing_is_written_when_nothing_is_asked_for() {
        let mut element = properties();
        write_open_type(&mut element, &OpenType::default());
        assert_eq!(element.child_elements().count(), 0);
    }

    #[test]
    fn a_bare_element_turns_contextual_alternates_on() {
        // `<w14:cntxtAlts/>` with no value means on, which is the way the
        // format spells every on-or-off property and the easiest to get wrong.
        let mut element = properties();
        element.push_element(Element::new("w14:cntxtAlts", Some(W14)));
        assert_eq!(read_open_type(&element).map(|found| found.contextual_alternates), Some(true));
    }

    #[test]
    fn a_stylistic_set_becomes_the_tag_the_font_knows() {
        let wanted = OpenType { stylistic_sets: vec![1, 12], ..OpenType::default() };
        assert_eq!(wanted.features(), vec![*b"ss01", *b"ss12"]);
    }

    #[test]
    fn a_set_outside_the_twenty_the_format_allows_is_left_out() {
        let wanted = OpenType { stylistic_sets: vec![0, 21, 3], ..OpenType::default() };
        assert_eq!(wanted.features(), vec![*b"ss03"]);
    }

    #[test]
    fn the_features_go_in_the_order_they_are_applied() {
        // The glyphs a ligature is made of have to have been chosen before the
        // ligature can be: numbers first, then alternates, then ligatures.
        let wanted = OpenType {
            ligatures: Ligatures::Standard,
            number_forms: NumberForms::Lining,
            number_spacing: NumberSpacing::Tabular,
            contextual_alternates: true,
            stylistic_sets: Vec::new(),
        };
        assert_eq!(wanted.features(), vec![*b"lnum", *b"tnum", *b"calt", *b"liga"]);
    }

    #[test]
    fn every_choice_word_offers_survives_the_file() {
        for ligatures in Ligatures::CHOICES {
            assert_eq!(Ligatures::from_attribute(ligatures.to_attribute()), *ligatures);
        }
        for spacing in NumberSpacing::CHOICES {
            assert_eq!(NumberSpacing::from_attribute(spacing.to_attribute()), *spacing);
        }
        for forms in NumberForms::CHOICES {
            assert_eq!(NumberForms::from_attribute(forms.to_attribute()), *forms);
        }
    }
}

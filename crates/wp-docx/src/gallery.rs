//! The themes, colour schemes and font pairings a document can be given.
//!
//! # Why this list and not Word's
//!
//! Word ships around thirty named themes, and each one is a set of exact
//! values — `4472C4` and not "a blue". Writing a theme called "Facet" whose
//! colours are a guess at Facet's would produce a document that says one thing
//! and looks like another, which is worse than not offering it.
//!
//! So what is here is the Office theme, whose values are known exactly because
//! every document Word makes carries them, and a handful of schemes named for
//! what they are rather than after Word's. A person picking "Grayscale" gets
//! grey; a person picking "Facet" in Word gets Facet, and would not have got it
//! here.
//!
//! The font pairings are Word's own, because a font pairing is two font names
//! and there is nothing to guess.

use crate::theme::{Slot, Theme};

/// A named set of the twelve colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorScheme {
    pub name: &'static str,
    /// The twelve, in [`Slot::ALL`] order.
    pub colors: [&'static str; 12],
}

/// A named pair of fonts: one for headings, one for the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontPair {
    pub name: &'static str,
    pub major: &'static str,
    pub minor: &'static str,
}

impl FontPair {
    /// What the list shows, which is the pairing rather than a made-up name.
    #[must_use]
    pub fn label(&self) -> String {
        if self.major == self.minor {
            return self.major.to_owned();
        }
        format!("{} / {}", self.major, self.minor)
    }
}

/// The colour schemes offered.
///
/// The first is Office, exactly as Word writes it. The rest are named for what
/// they are.
pub const COLOR_SCHEMES: &[ColorScheme] = &[
    ColorScheme {
        name: "Office",
        colors: [
            "000000", "FFFFFF", "44546A", "E7E6E6", "4472C4", "ED7D31", "A5A5A5", "FFC000",
            "5B9BD5", "70AD47", "0563C1", "954F72",
        ],
    },
    ColorScheme {
        name: "Grayscale",
        colors: [
            "000000", "FFFFFF", "3B3B3B", "E9E9E9", "585858", "757575", "8D8D8D", "A4A4A4",
            "BFBFBF", "D9D9D9", "5A5A5A", "7F7F7F",
        ],
    },
    ColorScheme {
        name: "Blue",
        colors: [
            "000000", "FFFFFF", "1F3864", "DEEAF6", "2E74B5", "1F4E79", "5B9BD5", "9DC3E6",
            "4472C4", "8FAADC", "0563C1", "954F72",
        ],
    },
    ColorScheme {
        name: "Red",
        colors: [
            "000000", "FFFFFF", "5B1A1A", "F8E3E3", "C00000", "E36C0A", "D99694", "E5B8B7",
            "953735", "FF6600", "0563C1", "954F72",
        ],
    },
    ColorScheme {
        name: "Green",
        colors: [
            "000000", "FFFFFF", "1E4620", "E2EFDA", "548235", "70AD47", "A9D08E", "C6E0B4",
            "375623", "92D050", "0563C1", "954F72",
        ],
    },
    ColorScheme {
        name: "Warm",
        colors: [
            "000000", "FFFFFF", "5A3B1E", "FBE9D9", "C55A11", "ED7D31", "F4B183", "FFC000",
            "BF8F00", "997300", "0563C1", "954F72",
        ],
    },
];

/// The font pairings offered, which are Word's own.
pub const FONT_PAIRS: &[FontPair] = &[
    FontPair { name: "Office", major: "Calibri Light", minor: "Calibri" },
    FontPair { name: "Office Classic", major: "Cambria", minor: "Calibri" },
    FontPair { name: "Arial", major: "Arial", minor: "Arial" },
    FontPair { name: "Georgia", major: "Georgia", minor: "Georgia" },
    FontPair { name: "Times New Roman", major: "Times New Roman", minor: "Times New Roman" },
    FontPair { name: "Trebuchet MS", major: "Trebuchet MS", minor: "Trebuchet MS" },
    FontPair { name: "Verdana", major: "Verdana", minor: "Verdana" },
    FontPair { name: "Corbel", major: "Corbel", minor: "Corbel" },
];

impl ColorScheme {
    /// The whole theme this scheme makes, with the Office fonts.
    #[must_use]
    pub fn theme(&self) -> Theme {
        Theme {
            name: self.name.to_owned(),
            colors: self.colors.iter().map(|value| (*value).to_owned()).collect(),
            ..Theme::default()
        }
    }

    /// One of its colours by slot, for a swatch to be drawn in.
    #[must_use]
    pub fn color(&self, slot: Slot) -> &'static str {
        let at = Slot::ALL.iter().position(|entry| *entry == slot).unwrap_or(0);
        self.colors.get(at).copied().unwrap_or("000000")
    }
}

impl Theme {
    /// The same theme with a different set of colours.
    #[must_use]
    pub fn with_colors(&self, scheme: &ColorScheme) -> Self {
        Self {
            name: scheme.name.to_owned(),
            colors: scheme.colors.iter().map(|value| (*value).to_owned()).collect(),
            major_font: self.major_font.clone(),
            minor_font: self.minor_font.clone(),
            effect: self.effect,
        }
    }

    /// The same theme with a different pair of fonts.
    #[must_use]
    pub fn with_fonts(&self, pair: &FontPair) -> Self {
        Self {
            name: self.name.clone(),
            colors: self.colors.clone(),
            major_font: pair.major.to_owned(),
            minor_font: pair.minor.to_owned(),
            effect: self.effect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scheme_names_twelve_colours() {
        for scheme in COLOR_SCHEMES {
            assert_eq!(scheme.colors.len(), Slot::ALL.len(), "{}", scheme.name);
        }
    }

    #[test]
    fn every_colour_is_six_hex_digits() {
        for scheme in COLOR_SCHEMES {
            for colour in scheme.colors {
                assert_eq!(colour.len(), 6, "{} has {colour}", scheme.name);
                assert!(
                    colour.chars().all(|c| c.is_ascii_hexdigit()),
                    "{} has {colour}",
                    scheme.name
                );
            }
        }
    }

    #[test]
    fn the_first_scheme_is_the_office_one_word_writes() {
        let office = COLOR_SCHEMES[0];
        assert_eq!(office.name, "Office");
        assert_eq!(office.theme(), Theme::default());
    }

    #[test]
    fn no_two_schemes_share_a_name() {
        for (at, scheme) in COLOR_SCHEMES.iter().enumerate() {
            assert!(
                !COLOR_SCHEMES[..at].iter().any(|earlier| earlier.name == scheme.name),
                "{} is named twice",
                scheme.name
            );
        }
    }

    #[test]
    fn changing_the_colours_leaves_the_fonts_alone() {
        let theme = Theme { major_font: "Georgia".to_owned(), ..Theme::default() };
        let changed = theme.with_colors(&COLOR_SCHEMES[1]);
        assert_eq!(changed.major_font, "Georgia");
        assert_eq!(changed.color(Slot::Accent1), COLOR_SCHEMES[1].color(Slot::Accent1));
    }

    #[test]
    fn changing_the_fonts_leaves_the_colours_alone() {
        let theme = Theme::default().with_colors(&COLOR_SCHEMES[1]);
        let changed = theme.with_fonts(&FONT_PAIRS[3]);
        assert_eq!(changed.major_font, "Georgia");
        assert_eq!(changed.color(Slot::Accent1), COLOR_SCHEMES[1].color(Slot::Accent1));
    }

    #[test]
    fn a_pairing_of_one_font_is_shown_as_one_name() {
        let same = FontPair { name: "Test", major: "Georgia", minor: "Georgia" };
        assert_eq!(same.label(), "Georgia");

        let different = FontPair { name: "Test", major: "Cambria", minor: "Calibri" };
        assert_eq!(different.label(), "Cambria / Calibri");
    }
}

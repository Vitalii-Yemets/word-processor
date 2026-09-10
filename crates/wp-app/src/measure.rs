//! How a measurement is shown and how one that is typed is read.
//!
//! # Why this is one place
//!
//! The file stores every length in twentieths of a point, and nobody types
//! twentieths of a point. Every dialog that holds a measurement therefore
//! converts, and each of them was converting on its own — the same two
//! functions written three times, all of them assuming inches.
//!
//! Word does not assume inches. Its Options has "Show measurements in units
//! of", and a person who set that to centimetres expects every box in the
//! program to be in centimetres. That cannot be true if each dialog decides for
//! itself, so the conversion lives here and the unit is asked for once.
//!
//! # What is not converted
//!
//! Points, where Word uses points regardless: the spacing above and below a
//! paragraph, the size of type, the position of text off its line. A person who
//! asked for centimetres did not ask for their type size in centimetres.

/// The units Word's Options offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Unit {
    #[default]
    Inches,
    Centimetres,
    Millimetres,
    Points,
    Picas,
}

impl Unit {
    /// Every one, in Word's order.
    pub const ALL: &'static [Self] =
        &[Self::Inches, Self::Centimetres, Self::Millimetres, Self::Points, Self::Picas];

    /// What Word's list calls it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Inches => "Inches",
            Self::Centimetres => "Centimeters",
            Self::Millimetres => "Millimeters",
            Self::Points => "Points",
            Self::Picas => "Picas",
        }
    }

    /// The mark that goes after a number in this unit.
    ///
    /// An inch is a double prime with nothing before it; the rest are words
    /// with a space. See `with_unit` in the dialog machinery.
    #[must_use]
    pub fn mark(self) -> &'static str {
        match self {
            Self::Inches => "\"",
            Self::Centimetres => "cm",
            Self::Millimetres => "mm",
            Self::Points => "pt",
            Self::Picas => "pi",
        }
    }

    /// How many twentieths of a point make one of these.
    ///
    /// An inch is 1440 of them by definition; the rest follow from that and
    /// from the definitions of the units themselves — a point is a
    /// seventy-second of an inch, a pica twelve points, an inch 25.4
    /// millimetres.
    #[must_use]
    pub fn twips(self) -> f64 {
        match self {
            Self::Inches => 1440.0,
            Self::Centimetres => 1440.0 / 2.54,
            Self::Millimetres => 1440.0 / 25.4,
            Self::Points => 20.0,
            Self::Picas => 240.0,
        }
    }

    /// How many places after the point are worth showing.
    ///
    /// Enough to say what was typed and no more: a hundredth of an inch is a
    /// quarter of a millimetre, and nobody sets a margin to a thousandth.
    #[must_use]
    pub fn places(self) -> usize {
        match self {
            Self::Inches | Self::Centimetres | Self::Picas => 2,
            Self::Millimetres | Self::Points => 1,
        }
    }

    /// What the settings file writes for it.
    #[must_use]
    pub fn to_name(self) -> &'static str {
        match self {
            Self::Inches => "inches",
            Self::Centimetres => "centimetres",
            Self::Millimetres => "millimetres",
            Self::Points => "points",
            Self::Picas => "picas",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Self {
        Self::ALL
            .iter()
            .copied()
            .find(|unit| unit.to_name().eq_ignore_ascii_case(name.trim()))
            .unwrap_or_default()
    }
}

/// A length from the file, as a person would type it.
#[must_use]
pub fn format(twips: i32, unit: Unit) -> String {
    format!("{:.*}", unit.places(), f64::from(twips) / unit.twips())
}

/// A length as it was typed, in the twentieths of a point the file stores.
///
/// A comma for a decimal point is what half the world types, and a box that
/// refuses it looks broken. Anything that is not a number at all comes back as
/// nothing, so the caller can tell that from a typed zero.
#[must_use]
pub fn parse(said: &str, unit: Unit) -> Option<i32> {
    let typed: f64 = said.trim().replace(',', ".").parse().ok()?;
    // Word's own limit: twenty-two inches, which is longer than any paper it
    // will print on.
    let twips = (typed * unit.twips()).round();
    Some(twips.clamp(-22.0 * 1440.0, 22.0 * 1440.0) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inch_is_what_the_format_says_it_is() {
        assert_eq!(parse("1", Unit::Inches), Some(1440));
        assert_eq!(format(1440, Unit::Inches), "1.00");
    }

    #[test]
    fn every_unit_comes_back_as_itself() {
        for unit in Unit::ALL {
            let there = parse("2", *unit).expect("two of anything is a number");
            let back = format(there, *unit);
            assert!(back.starts_with('2'), "two {} came back as {back}", unit.label());
        }
    }

    #[test]
    fn the_units_are_the_sizes_they_are_defined_as() {
        // A point is a seventy-second of an inch, a pica twelve points, an inch
        // 25.4 millimetres. Getting one of these wrong is a margin that is not
        // where it was asked to be.
        assert_eq!(parse("72", Unit::Points), Some(1440));
        assert_eq!(parse("6", Unit::Picas), Some(1440));
        assert_eq!(parse("2.54", Unit::Centimetres), Some(1440));
        assert_eq!(parse("25.4", Unit::Millimetres), Some(1440));
    }

    #[test]
    fn a_comma_for_a_decimal_point_is_still_a_number() {
        assert_eq!(parse("1,5", Unit::Inches), parse("1.5", Unit::Inches));
    }

    #[test]
    fn what_is_not_a_number_is_nothing_rather_than_a_zero() {
        // The caller has to be able to tell an empty box from a typed nought:
        // one is Word's "auto" and the other is a real measurement.
        assert_eq!(parse("", Unit::Inches), None);
        assert_eq!(parse("nonsense", Unit::Inches), None);
        assert_eq!(parse("0", Unit::Inches), Some(0));
    }

    #[test]
    fn a_measurement_beyond_any_paper_is_brought_back_inside() {
        assert_eq!(parse("99", Unit::Inches), Some(22 * 1440));
        assert_eq!(parse("-99", Unit::Inches), Some(-22 * 1440));
    }

    #[test]
    fn every_unit_word_offers_survives_the_settings_file() {
        for unit in Unit::ALL {
            assert_eq!(Unit::from_name(unit.to_name()), *unit);
        }
        // And a file from a later version, or a damaged one, falls back rather
        // than refusing to open.
        assert_eq!(Unit::from_name("furlongs"), Unit::Inches);
    }
}

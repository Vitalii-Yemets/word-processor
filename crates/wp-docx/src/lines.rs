//! What is drawn at the ends of a line: `a:headEnd` and `a:tailEnd`.
//!
//! # Why these are not part of the shape
//!
//! An arrowhead is not a shape the way a rectangle is. It belongs to the
//! *line*, which is why the format puts it inside `a:ln` beside the colour and
//! the width, and why the same connector is an arrow or is not depending on
//! nothing but this. Word's gallery offers "Line", "Line Arrow" and "Line
//! Arrow Double" as three things to insert, and all three insert the same
//! shape.
//!
//! # Which end is which
//!
//! The head is where the shape's outline starts and the tail is where it ends,
//! which for a connector is the end it was drawn away from and the end it was
//! drawn towards. Word's own gallery puts the arrow on the tail, so a line
//! drawn left to right has its point on the right.

use wp_xml::tree::Element;

/// What is drawn at one end of a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineEnd {
    pub kind: EndKind,
    /// How wide it is across the line, and how far it reaches back along it.
    /// Both are said as one of three sizes rather than as a measurement,
    /// because both are reckoned from the width of the line itself.
    pub width: EndSize,
    pub length: EndSize,
}

impl LineEnd {
    /// Whether anything is drawn there at all.
    #[must_use]
    pub fn is_nothing(self) -> bool {
        self.kind == EndKind::None
    }
}

/// The six things the format can draw at the end of a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EndKind {
    /// Nothing, which is what a plain line has.
    #[default]
    None,
    Triangle,
    /// A triangle with the back of it cut in, which is the arrow Word calls
    /// stealth.
    Stealth,
    Diamond,
    Oval,
    /// An open V rather than a filled head: two strokes and no inside.
    Arrow,
}

impl EndKind {
    /// The name the format knows it by.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Triangle => "triangle",
            Self::Stealth => "stealth",
            Self::Diamond => "diamond",
            Self::Oval => "oval",
            Self::Arrow => "arrow",
        }
    }

    /// And reading one back. A kind nobody knows is nothing at all: an end
    /// drawn as the wrong shape says something the document did not.
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "triangle" => Self::Triangle,
            "stealth" => Self::Stealth,
            "diamond" => Self::Diamond,
            "oval" => Self::Oval,
            "arrow" => Self::Arrow,
            _ => Self::None,
        }
    }
}

/// How big an end is, across the line or along it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EndSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl EndSize {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Small => "sm",
            Self::Medium => "med",
            Self::Large => "lg",
        }
    }

    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "sm" => Self::Small,
            "lg" => Self::Large,
            _ => Self::Medium,
        }
    }

    /// How many times the width of the line it is.
    ///
    /// The format says these as three names rather than as measurements
    /// because they are reckoned from the line: a thick line gets a big
    /// arrowhead without anybody asking for one.
    #[must_use]
    pub fn times(self) -> f32 {
        match self {
            Self::Small => 2.0,
            Self::Medium => 3.0,
            Self::Large => 5.0,
        }
    }
}

/// One end read out of a line, by the name the format gives that end.
#[must_use]
pub fn read_end(line: &Element, local: &str) -> LineEnd {
    let Some(end) = line.child_elements().find(|child| child.local_name() == local) else {
        return LineEnd::default();
    };
    LineEnd {
        kind: EndKind::from_word(end.attribute_by_name("type").unwrap_or_default()),
        width: EndSize::from_word(end.attribute_by_name("w").unwrap_or_default()),
        length: EndSize::from_word(end.attribute_by_name("len").unwrap_or_default()),
    }
}

/// And one written back, or nothing when there is nothing to draw there.
///
/// An end saying `type="none"` and one left out are the same thing to every
/// reader, and leaving it out is what Word does.
#[must_use]
pub fn end_element(name: &str, end: LineEnd) -> Option<Element> {
    if end.is_nothing() {
        return None;
    }
    let mut element = Element::new(name, Some(crate::shapes::A));
    element.set_attribute("type", end.kind.word());
    element.set_attribute("w", end.width.word());
    element.set_attribute("len", end.length.word());
    Some(element)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_survives_being_written_and_read_back() {
        for kind in [
            EndKind::None,
            EndKind::Triangle,
            EndKind::Stealth,
            EndKind::Diamond,
            EndKind::Oval,
            EndKind::Arrow,
        ] {
            assert_eq!(EndKind::from_word(kind.word()), kind);
        }
    }

    #[test]
    fn a_size_survives_being_written_and_read_back() {
        for size in [EndSize::Small, EndSize::Medium, EndSize::Large] {
            assert_eq!(EndSize::from_word(size.word()), size);
        }
    }

    #[test]
    fn a_kind_nobody_knows_draws_nothing() {
        // Rather than drawing the wrong head, which would say something about
        // the line that the document did not.
        assert_eq!(EndKind::from_word("triangleWithBells"), EndKind::None);
    }

    #[test]
    fn a_size_nobody_states_is_the_middle_one() {
        // Which is what the format falls back on, so a line saying only that it
        // has an arrow gets the arrow Word draws for it.
        assert_eq!(EndSize::from_word(""), EndSize::Medium);
    }

    #[test]
    fn nothing_at_the_end_is_written_as_nothing_at_all() {
        assert!(end_element("a:headEnd", LineEnd::default()).is_none());
    }
}

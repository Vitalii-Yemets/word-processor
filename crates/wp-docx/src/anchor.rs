//! Where a drawing that floats sits, and what the text does about it.
//!
//! # Two ways a drawing can be in a document
//!
//! In the line, where it is a very large letter: the text before it and the
//! text after it are on the same line, and moving the text moves it. That is
//! `wp:inline`, and it is what a picture pasted into a document starts as.
//!
//! Or anchored: fixed to a place on the page, with the text flowing round it.
//! That is `wp:anchor`, and it carries three things the inline kind does not —
//! where it sits across the page, where it sits down the page, and what the
//! text is to do when it meets it.
//!
//! # Why the position is not simply two numbers
//!
//! Because "two inches from the left" is meaningless without saying two inches
//! from the left *of what*. The margin, the page, the column and the paragraph
//! are all answers, and a document uses whichever one keeps the drawing where
//! its author meant it when the page size changes.

use wp_xml::tree::Element;

/// What the text does when it meets a floating drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wrap {
    /// Nothing: the drawing and the text are drawn over one another.
    None,
    /// The text keeps out of the drawing's box.
    #[default]
    Square,
    /// The text follows the drawing's own outline rather than its box.
    Tight,
    /// The same, and into any hole the drawing has.
    Through,
    /// The text stops above the drawing and starts again below it.
    TopAndBottom,
}

impl Wrap {
    /// The element the format writes it as.
    #[must_use]
    pub fn element(self) -> &'static str {
        match self {
            Self::None => "wrapNone",
            Self::Square => "wrapSquare",
            Self::Tight => "wrapTight",
            Self::Through => "wrapThrough",
            Self::TopAndBottom => "wrapTopAndBottom",
        }
    }

    /// Reads one back out of a document.
    #[must_use]
    pub fn from_element(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|wrap| wrap.element() == name)
    }

    /// What a person is shown when picking one.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Behind Text",
            Self::Square => "Square",
            Self::Tight => "Tight",
            Self::Through => "Through",
            Self::TopAndBottom => "Top and Bottom",
        }
    }

    /// Whether the text has to keep out of the drawing's way at all.
    #[must_use]
    pub fn reserves_room(self) -> bool {
        self != Self::None
    }

    /// Every one that can be picked, in Word's order.
    pub const ALL: &'static [Self] =
        &[Self::Square, Self::Tight, Self::Through, Self::TopAndBottom, Self::None];
}

/// What a position is measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Relative {
    /// The margin, which is what keeps a drawing beside the text when the page
    /// size changes.
    #[default]
    Margin,
    Page,
    /// The column the text is in, which is the same as the margin in a document
    /// of one column.
    Column,
    /// The paragraph the anchor is in — only meaningful downwards.
    Paragraph,
    /// The line it is in.
    Line,
    /// The character it is beside — only meaningful across.
    Character,
}

impl Relative {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Margin => "margin",
            Self::Page => "page",
            Self::Column => "column",
            Self::Paragraph => "paragraph",
            Self::Line => "line",
            Self::Character => "character",
        }
    }

    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "page" => Self::Page,
            "column" => Self::Column,
            "paragraph" => Self::Paragraph,
            "line" => Self::Line,
            "character" => Self::Character,
            // The many others — the inside margin, the outside margin, the top
            // margin — all measure from an edge of the page or of the text, and
            // the margin is the nearer of the two.
            _ => Self::Margin,
        }
    }
}

/// Where along one axis a drawing sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Lined up with an edge or the middle: `left`, `center`, `right` across,
    /// `top`, `center`, `bottom` down.
    Aligned(String),
    /// A distance, in English Metric Units.
    Offset(i64),
}

impl Default for Placement {
    fn default() -> Self {
        Self::Offset(0)
    }
}

/// Everything about where a floating drawing sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub wrap: Wrap,
    /// Whether the drawing is drawn under the text rather than over it.
    pub behind_text: bool,
    pub horizontal_from: Relative,
    pub horizontal: Placement,
    pub vertical_from: Relative,
    pub vertical: Placement,
    /// How much room to leave round it, in EMU: left, right, top, bottom.
    pub distance: (i64, i64, i64, i64),
}

impl Default for Anchor {
    fn default() -> Self {
        Self {
            wrap: Wrap::Square,
            behind_text: false,
            horizontal_from: Relative::Column,
            horizontal: Placement::Offset(0),
            vertical_from: Relative::Paragraph,
            vertical: Placement::Offset(0),
            // A tenth of an inch either side, which is what Word leaves.
            distance: (114_300, 114_300, 0, 0),
        }
    }
}

impl Anchor {
    /// A drawing lined up with an edge of the text, wrapped square.
    #[must_use]
    pub fn aligned(across: &str, wrap: Wrap) -> Self {
        Self { wrap, horizontal: Placement::Aligned(across.to_owned()), ..Self::default() }
    }

    /// Whether the text has to keep out of its way.
    #[must_use]
    pub fn reserves_room(&self) -> bool {
        self.wrap.reserves_room()
    }
}

/// Reads the anchor out of a `w:drawing`, if the drawing floats.
#[must_use]
pub fn read_anchor(drawing: &Element) -> Option<Anchor> {
    let anchor = find(drawing, "anchor")?;
    let mut result = Anchor { wrap: Wrap::None, ..Anchor::default() };

    result.behind_text = matches!(anchor.attribute_by_name("behindDoc"), Some("1" | "true"));
    result.distance = (
        number(anchor, "distL"),
        number(anchor, "distR"),
        number(anchor, "distT"),
        number(anchor, "distB"),
    );

    // The wrap is whichever of the five elements is there. None of them is
    // `wrapNone` with nothing said, which is why the default above is None and
    // not Square: a drawing that says nothing does not push the text about.
    for child in anchor.child_elements() {
        if let Some(wrap) = Wrap::from_element(child.local_name()) {
            result.wrap = wrap;
            break;
        }
    }

    if let Some(across) = child(anchor, "positionH") {
        result.horizontal_from =
            Relative::from_word(across.attribute_by_name("relativeFrom").unwrap_or_default());
        result.horizontal = placement_of(across);
    }
    if let Some(down) = child(anchor, "positionV") {
        result.vertical_from =
            Relative::from_word(down.attribute_by_name("relativeFrom").unwrap_or_default());
        result.vertical = placement_of(down);
    }
    Some(result)
}

/// Writes the attributes and children an anchor adds to a drawing.
///
/// The extent, the graphic and the rest are the same as an inline drawing's, so
/// they are not here: this is only the difference between floating and not.
pub fn write_anchor(anchor: &Anchor, element: &mut Element, wp: &str) {
    let (left, right, top, bottom) = anchor.distance;
    element.set_attribute("distL", &left.to_string());
    element.set_attribute("distR", &right.to_string());
    element.set_attribute("distT", &top.to_string());
    element.set_attribute("distB", &bottom.to_string());
    element.set_attribute("simplePos", "0");
    // Which drawing is over which when two overlap. One number for all of them
    // is enough while nothing here can reorder them.
    element.set_attribute("relativeHeight", "251658240");
    element.set_attribute("behindDoc", if anchor.behind_text { "1" } else { "0" });
    element.set_attribute("locked", "0");
    element.set_attribute("layoutInCell", "1");
    element.set_attribute("allowOverlap", "1");

    let mut simple = Element::new("wp:simplePos", Some(wp));
    simple.set_attribute("x", "0");
    simple.set_attribute("y", "0");
    element.push_element(simple);

    element.push_element(position("positionH", anchor.horizontal_from, &anchor.horizontal, wp));
    element.push_element(position("positionV", anchor.vertical_from, &anchor.vertical, wp));
}

/// The wrap element, which goes after the extent rather than before it.
#[must_use]
pub fn wrap_element(anchor: &Anchor, wp: &str) -> Element {
    let mut element = Element::new(&format!("wp:{}", anchor.wrap.element()), Some(wp));
    if matches!(anchor.wrap, Wrap::Square | Wrap::Tight | Wrap::Through) {
        // Which sides the text may run down. Both is the ordinary answer and
        // the only one this program lays out.
        element.set_attribute("wrapText", "bothSides");
    }
    element
}

/// One of the two position elements.
fn position(name: &str, from: Relative, placement: &Placement, wp: &str) -> Element {
    let mut element = Element::new(&format!("wp:{name}"), Some(wp));
    element.set_attribute("relativeFrom", from.word());
    match placement {
        Placement::Aligned(edge) => {
            let mut align = Element::new("wp:align", Some(wp));
            align.set_text(edge);
            element.push_element(align);
        }
        Placement::Offset(distance) => {
            let mut offset = Element::new("wp:posOffset", Some(wp));
            offset.set_text(&distance.to_string());
            element.push_element(offset);
        }
    }
    element
}

/// Reads whichever of the two ways a position was written.
fn placement_of(element: &Element) -> Placement {
    if let Some(align) = child(element, "align") {
        let edge = align.text_content();
        if !edge.trim().is_empty() {
            return Placement::Aligned(edge.trim().to_owned());
        }
    }
    if let Some(offset) = child(element, "posOffset") {
        if let Ok(distance) = offset.text_content().trim().parse() {
            return Placement::Offset(distance);
        }
    }
    Placement::Offset(0)
}

fn number(element: &Element, name: &str) -> i64 {
    element.attribute_by_name(name).and_then(|value| value.parse().ok()).unwrap_or(0)
}

fn child<'a>(parent: &'a Element, local: &str) -> Option<&'a Element> {
    parent.child_elements().find(|child| child.local_name() == local)
}

fn find<'a>(root: &'a Element, local: &str) -> Option<&'a Element> {
    if root.local_name() == local {
        return Some(root);
    }
    root.child_elements().find_map(|child| find(child, local))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";

    /// A drawing carrying an anchor, written the way this module writes one.
    fn drawing_with(anchor: &Anchor) -> Element {
        let mut drawing = Element::new("w:drawing", Some("w"));
        let mut element = Element::new("wp:anchor", Some(WP));
        write_anchor(anchor, &mut element, WP);
        element.push_element(wrap_element(anchor, WP));
        drawing.push_element(element);
        drawing
    }

    #[test]
    fn every_wrap_survives_being_written_and_read_back() {
        for wrap in Wrap::ALL {
            let anchor = Anchor { wrap: *wrap, ..Anchor::default() };
            let read = read_anchor(&drawing_with(&anchor)).expect("an anchor");
            assert_eq!(read.wrap, *wrap, "{}", wrap.label());
        }
    }

    #[test]
    fn a_position_lined_up_with_an_edge_reads_back_as_that_edge() {
        let anchor = Anchor::aligned("right", Wrap::Square);
        let read = read_anchor(&drawing_with(&anchor)).expect("an anchor");
        assert_eq!(read.horizontal, Placement::Aligned("right".to_owned()));
    }

    #[test]
    fn a_position_given_as_a_distance_reads_back_as_that_distance() {
        let anchor = Anchor {
            horizontal: Placement::Offset(635_000),
            vertical: Placement::Offset(-12_700),
            ..Anchor::default()
        };
        let read = read_anchor(&drawing_with(&anchor)).expect("an anchor");
        assert_eq!(read.horizontal, Placement::Offset(635_000));
        assert_eq!(read.vertical, Placement::Offset(-12_700), "a drawing may sit above its line");
    }

    #[test]
    fn what_a_position_is_measured_from_survives() {
        let anchor = Anchor {
            horizontal_from: Relative::Page,
            vertical_from: Relative::Line,
            ..Anchor::default()
        };
        let read = read_anchor(&drawing_with(&anchor)).expect("an anchor");
        assert_eq!(read.horizontal_from, Relative::Page);
        assert_eq!(read.vertical_from, Relative::Line);
    }

    #[test]
    fn a_relative_nobody_here_names_becomes_the_margin() {
        assert_eq!(Relative::from_word("outsideMargin"), Relative::Margin);
        assert_eq!(Relative::from_word(""), Relative::Margin);
    }

    #[test]
    fn a_drawing_behind_the_text_says_so_and_reads_back() {
        let anchor = Anchor { behind_text: true, wrap: Wrap::None, ..Anchor::default() };
        let read = read_anchor(&drawing_with(&anchor)).expect("an anchor");
        assert!(read.behind_text);
    }

    #[test]
    fn the_room_left_round_a_drawing_survives() {
        let anchor = Anchor { distance: (1, 2, 3, 4), ..Anchor::default() };
        let read = read_anchor(&drawing_with(&anchor)).expect("an anchor");
        assert_eq!(read.distance, (1, 2, 3, 4));
    }

    #[test]
    fn a_drawing_that_does_not_float_has_no_anchor() {
        let mut drawing = Element::new("w:drawing", Some("w"));
        drawing.push_element(Element::new("wp:inline", Some(WP)));
        assert!(read_anchor(&drawing).is_none());
    }

    #[test]
    fn only_a_wrap_of_none_leaves_the_text_alone() {
        assert!(!Wrap::None.reserves_room());
        for wrap in [Wrap::Square, Wrap::Tight, Wrap::Through, Wrap::TopAndBottom] {
            assert!(wrap.reserves_room(), "{}", wrap.label());
        }
    }
}

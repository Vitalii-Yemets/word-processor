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

/// Which side of a drawing the text runs down.
///
/// `wrapSquare/@wrapText`, and Word's Wrap text in the Layout dialog. Only
/// meaningful for the three wraps that let text beside the drawing at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WrapSide {
    /// Both, which is what Word writes unless told otherwise: a line beside
    /// the drawing is broken into a piece each side of it.
    #[default]
    BothSides,
    /// The text keeps to the left of the drawing and the room to its right is
    /// left empty.
    Left,
    /// And the other way about.
    Right,
    /// Whichever side has more room, which is one piece of line rather than
    /// two.
    Largest,
}

impl WrapSide {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::BothSides => "bothSides",
            Self::Left => "left",
            Self::Right => "right",
            Self::Largest => "largest",
        }
    }

    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "left" => Self::Left,
            "right" => Self::Right,
            "largest" => Self::Largest,
            _ => Self::BothSides,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::BothSides => "Both sides",
            Self::Left => "Left only",
            Self::Right => "Right only",
            Self::Largest => "Largest only",
        }
    }

    pub const ALL: &'static [Self] = &[Self::BothSides, Self::Left, Self::Right, Self::Largest];
}

/// Word's 2010 extension to the drawing markup.
///
/// Two of the things a drawing can say live only here: its size as a percentage
/// of a frame, and its position as one. Word writes the absolute values beside
/// them, so a reader that skips the extension still draws the picture in the
/// right place — which is what makes an extension an extension. See
/// [`crate::edit::declare_extension`].
pub const WP14: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing";

/// And the prefix Word writes it under.
pub const WP14_PREFIX: &str = "wp14";

/// A measurement stated as a percentage of a frame.
///
/// `wp14:sizeRelH` and `wp14:sizeRelV`: what Word writes when a drawing is
/// sized in per cent rather than in inches — a picture at half the page width
/// stays half of it when the paper changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Relatively {
    /// What the percentage is of.
    pub from: Relative,
    /// Thousandths of a per cent, which is the unit the format counts in:
    /// 50000 is half.
    pub thousandths: i32,
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
    /// One of the four margins, as a band of its own.
    ///
    /// Not the same thing as [`Relative::Margin`], which means the text area
    /// the margins surround. These are the empty bands themselves, and they
    /// are the only way to put something *in* a margin rather than against
    /// the edge of the text — which is what a page number down the side of a
    /// page is.
    LeftMargin,
    RightMargin,
    TopMargin,
    BottomMargin,
    /// The margin nearer the binding, and the one further from it.
    ///
    /// Which side either lands on changes with the page in a document printed
    /// on both sides. See [`crate::page`] for the switch that mirrors them.
    InsideMargin,
    OutsideMargin,
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
            Self::LeftMargin => "leftMargin",
            Self::RightMargin => "rightMargin",
            Self::TopMargin => "topMargin",
            Self::BottomMargin => "bottomMargin",
            Self::InsideMargin => "insideMargin",
            Self::OutsideMargin => "outsideMargin",
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
            "leftMargin" => Self::LeftMargin,
            "rightMargin" => Self::RightMargin,
            "topMargin" => Self::TopMargin,
            "bottomMargin" => Self::BottomMargin,
            "insideMargin" => Self::InsideMargin,
            "outsideMargin" => Self::OutsideMargin,
            // Anything else measures from the text area, which is what the
            // format falls back on and what nearly every drawing uses.
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
    /// A distance as a percentage of the frame, in thousandths of a per cent.
    ///
    /// `wp14:pctPosHOffset` and its twin: the same extension the relative size
    /// uses, and the same reason — a drawing a third of the way across the page
    /// is still a third of the way across a wider page. See [`WP14`].
    Percent(i32),
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
    /// Which side of it the text runs down. See [`WrapSide`].
    pub side: WrapSide,
    /// Its width as a percentage of a frame, where it is stated that way.
    /// The absolute width is written beside it and is what a reader that skips
    /// the extension uses. See [`Relatively`].
    pub width_of: Option<Relatively>,
    pub height_of: Option<Relatively>,
    /// Whether the drawing is drawn under the text rather than over it.
    pub behind_text: bool,
    /// Whether it may lie over another floating drawing.
    ///
    /// `allowOverlap`, and Word's Allow overlap. With it off, a drawing that
    /// would land on top of one already placed is pushed down until it does
    /// not — which is what makes two pictures dropped in the same place end up
    /// one above the other rather than one on top of the other.
    pub allow_overlap: bool,
    /// Whether the anchor may be moved to another paragraph.
    ///
    /// `locked`, and Word's Lock anchor. Kept so that it survives being read
    /// and written; what it guards against — Word moving an anchor to the
    /// paragraph a dragged drawing lands in — is not something this program
    /// does yet, so there is nothing here for it to stop. See **C47** in the
    /// roadmap.
    pub locked: bool,
    pub horizontal_from: Relative,
    pub horizontal: Placement,
    pub vertical_from: Relative,
    pub vertical: Placement,
    /// How much room to leave round it, in EMU: left, right, top, bottom.
    pub distance: (i64, i64, i64, i64),
    /// Which drawing is over which where two of them overlap.
    ///
    /// `wp:anchor/@relativeHeight`, and bigger is nearer the reader. Word
    /// starts at [`USUAL_DEPTH`] and counts up as drawings are added, which is
    /// why a document's numbers are all within a few of each other and nothing
    /// is lost by writing them back as they came.
    ///
    /// It orders the drawings within a layer and not across one: a drawing
    /// behind the text is behind every drawing in front of it whatever number
    /// either carries. See [`Anchor::behind_text`].
    pub depth: u32,
}

/// The number Word gives the first floating drawing in a document.
///
/// 0x0F000000. There is nothing special about it beyond being Word's, and
/// matching it means a document saved here and opened there has the numbers
/// Word would have written.
pub const USUAL_DEPTH: u32 = 251_658_240;

impl Default for Anchor {
    fn default() -> Self {
        Self {
            wrap: Wrap::Square,
            side: WrapSide::default(),
            width_of: None,
            height_of: None,
            // Word's own defaults: a drawing may overlap, and its anchor is
            // free to move.
            allow_overlap: true,
            locked: false,
            behind_text: false,
            horizontal_from: Relative::Column,
            horizontal: Placement::Offset(0),
            vertical_from: Relative::Paragraph,
            vertical: Placement::Offset(0),
            // A tenth of an inch either side, which is what Word leaves.
            distance: (114_300, 114_300, 0, 0),
            depth: USUAL_DEPTH,
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
    // Both default to what Word writes when it says nothing, which is not the
    // same answer for the two of them.
    result.allow_overlap = !matches!(anchor.attribute_by_name("allowOverlap"), Some("0" | "false"));
    result.locked = matches!(anchor.attribute_by_name("locked"), Some("1" | "true"));
    result.depth = anchor
        .attribute_by_name("relativeHeight")
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(USUAL_DEPTH);
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
            // And which side of the drawing the text runs down, which only
            // the wraps that let text beside it at all can say.
            result.side =
                WrapSide::from_word(child.attribute_by_name("wrapText").unwrap_or_default());
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
    result.width_of = relatively(anchor, "sizeRelH", "pctWidth");
    result.height_of = relatively(anchor, "sizeRelV", "pctHeight");
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
    // Which drawing is over which when two overlap.
    element.set_attribute("relativeHeight", &anchor.depth.to_string());
    element.set_attribute("behindDoc", if anchor.behind_text { "1" } else { "0" });
    element.set_attribute("locked", if anchor.locked { "1" } else { "0" });
    element.set_attribute("layoutInCell", "1");
    element.set_attribute("allowOverlap", if anchor.allow_overlap { "1" } else { "0" });

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
        // Which side the text may run down. See [`WrapSide`].
        element.set_attribute("wrapText", anchor.side.word());
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
        // The extension, in place of the offset. Word writes it this way round
        // — a percentage instead of a distance, not beside one — and the
        // relative size is the other way about. See [`WP14`].
        Placement::Percent(thousandths) => {
            let local = if name == "positionH" { "pctPosHOffset" } else { "pctPosVOffset" };
            let mut offset = Element::new(&format!("{WP14_PREFIX}:{local}"), Some(WP14));
            offset.set_text(&thousandths.to_string());
            element.push_element(offset);
        }
    }
    element
}

/// The two elements that state a size as a percentage of a frame.
///
/// They go last among the anchor's children, after the wrap, which is where
/// Word writes them.
#[must_use]
pub fn relative_size_elements(anchor: &Anchor) -> Vec<Element> {
    let mut out = Vec::new();
    for (relatively, local, inner) in
        [(anchor.width_of, "sizeRelH", "pctWidth"), (anchor.height_of, "sizeRelV", "pctHeight")]
    {
        let Some(relatively) = relatively else { continue };
        let mut element = Element::new(&format!("{WP14_PREFIX}:{local}"), Some(WP14));
        element.set_attribute("relativeFrom", relatively.from.word());
        let mut value = Element::new(&format!("{WP14_PREFIX}:{inner}"), Some(WP14));
        value.set_text(&relatively.thousandths.to_string());
        element.push_element(value);
        out.push(element);
    }
    out
}

/// Whether an anchor says anything that needs the 2010 extension declared.
#[must_use]
pub fn needs_extension(anchor: &Anchor) -> bool {
    anchor.width_of.is_some()
        || anchor.height_of.is_some()
        || matches!(anchor.horizontal, Placement::Percent(_))
        || matches!(anchor.vertical, Placement::Percent(_))
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
    // The extension, which Word writes *instead* of an offset when the position
    // is a percentage. Looked for by local name: the prefix is the document's
    // business. See [`WP14`].
    for local in ["pctPosHOffset", "pctPosVOffset"] {
        if let Some(offset) = child(element, local) {
            if let Ok(thousandths) = offset.text_content().trim().parse() {
                return Placement::Percent(thousandths);
            }
        }
    }
    Placement::Offset(0)
}

/// A size stated as a percentage, read out of `wp14:sizeRelH` or its twin.
fn relatively(anchor: &Element, local: &str, inner: &str) -> Option<Relatively> {
    let element = child(anchor, local)?;
    let thousandths = child(element, inner)?.text_content().trim().parse().ok()?;
    Some(Relatively {
        from: Relative::from_word(element.attribute_by_name("relativeFrom").unwrap_or_default()),
        thousandths,
    })
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
        assert_eq!(Relative::from_word("somethingNobodyWrote"), Relative::Margin);
        assert_eq!(Relative::from_word(""), Relative::Margin);
    }

    #[test]
    fn each_margin_is_a_band_of_its_own_and_comes_back_as_itself() {
        // The text area and the four bands round it are different frames, and
        // reading one as the other is the difference between a page number in
        // the margin and one against the edge of the text.
        for word in [
            "leftMargin",
            "rightMargin",
            "topMargin",
            "bottomMargin",
            "insideMargin",
            "outsideMargin",
        ] {
            let read = Relative::from_word(word);
            assert_ne!(read, Relative::Margin, "{word} was read as the text area");
            assert_eq!(read.word(), word, "{word} did not come back as itself");
        }
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
    fn which_drawing_is_over_which_survives() {
        let anchor = Anchor { depth: USUAL_DEPTH + 7, ..Anchor::default() };
        let read = read_anchor(&drawing_with(&anchor)).expect("an anchor");
        assert_eq!(read.depth, USUAL_DEPTH + 7);
    }

    #[test]
    fn a_drawing_that_says_nothing_about_it_gets_words_own_number() {
        // An anchor written by something that left the attribute out. It has to
        // come back as a number, because a drawing with no place in the pile
        // has no place at all.
        let mut anchor = Element::new("wp:anchor", Some(WP));
        anchor.set_attribute("behindDoc", "0");
        let mut drawing = Element::new("w:drawing", Some("w"));
        drawing.push_element(anchor);
        assert_eq!(read_anchor(&drawing).expect("an anchor").depth, USUAL_DEPTH);
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

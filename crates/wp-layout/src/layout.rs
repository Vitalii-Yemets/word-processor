//! Turning a document into positioned glyphs on pages.
//!
//! # What this stage does
//!
//! Text is measured, broken into lines at the page width, and each glyph is
//! given a position. Paragraph spacing, indents and alignment are honoured, and
//! a line that will not fit starts a new page.
//!
//! # What it does not do yet
//!
//! This is a first layout engine, and it is deliberately honest about its
//! limits rather than approximating quietly:
//!
//! * **No bracket pairing in the bidirectional algorithm.** A bracket takes the
//!   direction of what is around it rather than of what it encloses, which
//!   needs the pairings from the character database. See [`wp_bidi`].
//! * **Kerning only from the old `kern` table.** Modern fonts put it in `GPOS`,
//!   which arrives with shaping.
//! * **A table row does not split across a page.** A row that will not fit
//!   starts a new page whole; one taller than a page overflows it.

use std::collections::HashMap;
use std::rc::Rc;

use wp_docx::model::{
    Alignment, Block, Body, Border, BreakKind, LineRule, Paragraph, ResolvedRunProperties, Run,
    RunContent, TabAlignment, TabLeader, TabStop, Table, TableBorders, TableCell, TableRow,
    TextDirection, VerticalAlignment,
};
use wp_docx::sections::{NumberFormat, Start};
use wp_docx::styles::Conditional;
use wp_docx::{Document, ListCounters, TextPosition};
use wp_font::{Font, GlyphId};
use wp_image::Image;
use wp_raster::Color;

use wp_docx::cells::CellEdge;

use crate::borders::Side;
use crate::library::FontLibrary;

/// Laying the body out again from where it changed.
mod again;

/// Twentieths of a point, the unit the format measures almost everything in.
pub(crate) const TWIPS_PER_POINT: f32 = 20.0;
/// Points per inch, which is what makes a point a point.
const POINTS_PER_INCH: f32 = 72.0;

/// How far one level of an outline is indented past the one above it, in
/// points. A quarter of an inch, which is what Word steps by.
///
/// Public because the marks beside the paragraphs of an outline are drawn
/// by the window, a step to the left of where each level begins.
pub const OUTLINE_STEP: f32 = 18.0;

/// The level that means "everything": nine headings and the body text under
/// them. What Word offers as All Levels.
const OUTLINE_ALL: u8 = 10;

/// How far past the last letter a selected paragraph break is shown, in pixels.
const BREAK_HIGHLIGHT_WIDTH: f32 = 5.0;

/// The gap between default tab stops, in twentieths of a point.
///
/// Half an inch, which is what a document falls back to when it defines no tab
/// stops of its own — and what Word uses.
const DEFAULT_TAB_TWIPS: i32 = 720;

/// A4, the default when a document does not say.
const DEFAULT_PAGE_WIDTH_TWIPS: f32 = 11906.0;
const DEFAULT_PAGE_HEIGHT_TWIPS: f32 = 16838.0;
const DEFAULT_MARGIN_TWIPS: f32 = 1440.0;

/// Page size and margins, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageMetrics {
    pub width: f32,
    pub height: f32,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    /// How many columns the text flows down, and the gap between them in
    /// points. One column is the ordinary case and means no gap is used.
    pub columns: usize,
    pub column_gap: f32,
    /// Which way the text runs on the paper: across, or down it. See
    /// [`Self::boxed`] for what that does to everything else here.
    pub direction: TextDirection,
}

impl Default for PageMetrics {
    fn default() -> Self {
        Self {
            width: DEFAULT_PAGE_WIDTH_TWIPS / TWIPS_PER_POINT,
            height: DEFAULT_PAGE_HEIGHT_TWIPS / TWIPS_PER_POINT,
            margin_top: DEFAULT_MARGIN_TWIPS / TWIPS_PER_POINT,
            margin_right: DEFAULT_MARGIN_TWIPS / TWIPS_PER_POINT,
            margin_bottom: DEFAULT_MARGIN_TWIPS / TWIPS_PER_POINT,
            margin_left: DEFAULT_MARGIN_TWIPS / TWIPS_PER_POINT,
            columns: 1,
            // Word's default gap between columns is half an inch.
            column_gap: 720.0 / TWIPS_PER_POINT,
            direction: TextDirection::Horizontal,
        }
    }
}

impl PageMetrics {
    /// The width text may occupy.
    #[must_use]
    pub fn text_width(&self) -> f32 {
        (self.width - self.margin_left - self.margin_right).max(1.0)
    }

    /// The width of one column, once the gaps between them are taken out.
    #[must_use]
    pub fn column_width(&self) -> f32 {
        let columns = self.columns.max(1) as f32;
        ((self.text_width() - self.column_gap * (columns - 1.0)) / columns).max(1.0)
    }

    /// The height text may occupy.
    #[must_use]
    pub fn text_height(&self) -> f32 {
        (self.height - self.margin_top - self.margin_bottom).max(1.0)
    }

    /// The paper one section of a document is printed on.
    #[must_use]
    pub fn from_setup(setup: &wp_docx::sections::Setup) -> Self {
        let points = |twips: i32| twips as f32 / TWIPS_PER_POINT;
        Self {
            width: points(setup.width),
            height: points(setup.height),
            margin_top: points(setup.margin_top),
            margin_right: points(setup.margin_right),
            margin_bottom: points(setup.margin_bottom),
            margin_left: points(setup.margin_left),
            columns: setup.columns.max(1),
            column_gap: points(setup.column_gap),
            // Word takes no notice of text reading upwards on a section,
            // whatever the file says, and neither does this.
            direction: match setup.direction {
                TextDirection::Up => TextDirection::Horizontal,
                other => other,
            },
        }
    }

    /// Whether the text runs down the paper rather than across it, so that
    /// the body is laid out into a box and turned.
    #[must_use]
    pub fn is_turned(&self) -> bool {
        self.direction.is_turned()
    }

    /// The paper as the text sees it: the box the body of a turned section is
    /// laid out into, before the box is turned onto the page.
    ///
    /// # What turns with the paper
    ///
    /// Everything. Text that runs down the page has its lines going from
    /// right to left, so the top margin is where the lines begin and the
    /// right margin is where the first of them stands; the columns divide the
    /// page's height into bands rather than its width into strips, which is
    /// what Word does with columns in vertical text; a paragraph's left indent
    /// is measured from the top. So the box is the paper with its sides
    /// swapped round the corner the turn is about, and laying the body out
    /// into it as though it were paper is what gives all of that at once —
    /// see [`Frame`].
    #[must_use]
    pub fn boxed(&self) -> Self {
        if !self.is_turned() {
            return *self;
        }
        let (top, right, bottom, left) = match self.direction {
            // Reading down from the top right, the lines going leftwards: the
            // box's left is the page's top and its top is the page's right.
            TextDirection::Down | TextDirection::TurnedDown => {
                (self.margin_right, self.margin_bottom, self.margin_left, self.margin_top)
            }
            // Reading down from the top left, the lines going rightwards.
            TextDirection::DownLeftToRight => {
                (self.margin_left, self.margin_bottom, self.margin_right, self.margin_top)
            }
            // Reading up from the bottom left, the lines going rightwards.
            TextDirection::Up => {
                (self.margin_left, self.margin_top, self.margin_right, self.margin_bottom)
            }
            TextDirection::Horizontal | TextDirection::RotatedAsian => unreachable!(),
        };
        Self {
            width: self.height,
            height: self.width,
            margin_top: top,
            margin_right: right,
            margin_bottom: bottom,
            margin_left: left,
            direction: TextDirection::Horizontal,
            ..*self
        }
    }

    /// The frame that puts the box of [`Self::boxed`] onto a page this many
    /// pixels across and down.
    #[must_use]
    pub fn frame_on(&self, page_width: f32, page_height: f32) -> Frame {
        match self.direction {
            TextDirection::Down | TextDirection::TurnedDown => {
                Frame::turned(Turn::Down, page_width, 0.0)
            }
            TextDirection::DownLeftToRight => {
                Frame { turn: Turn::Down, flipped: true, x: 0.0, y: 0.0 }
            }
            TextDirection::Up => Frame::turned(Turn::Up, 0.0, page_height),
            TextDirection::Horizontal | TextDirection::RotatedAsian => Frame::default(),
        }
    }

    #[must_use]
    /// Reads the last `w:sectPr` out of a document, falling back to A4.
    ///
    /// The last one, which is the whole story for a document of one section
    /// and the fallback for one of several — see [`Self::from_setup`].
    pub fn from_document(document: &Document) -> Self {
        let mut metrics = Self::default();
        let namespace = Some(wp_docx::WORDPROCESSING_NAMESPACE);

        let Some(body) = find_element(&document.tree().root, "body") else {
            return metrics;
        };
        let Some(section) = body.child(namespace, "sectPr") else {
            return metrics;
        };

        if let Some(size) = section.child(namespace, "pgSz") {
            if let Some(width) = twips(size, "w") {
                metrics.width = width;
            }
            if let Some(height) = twips(size, "h") {
                metrics.height = height;
            }
        }
        if let Some(columns) = section.child(namespace, "cols") {
            if let Some(count) =
                columns.attribute(namespace, "num").and_then(|text| text.parse::<usize>().ok())
            {
                metrics.columns = count.clamp(1, 12);
            }
            if let Some(space) = twips(columns, "space") {
                metrics.column_gap = space;
            }
        }
        if let Some(direction) = section
            .child(namespace, "textDirection")
            .and_then(|element| element.attribute(namespace, "val"))
        {
            metrics.direction = match TextDirection::from_word(direction) {
                TextDirection::Up => TextDirection::Horizontal,
                other => other,
            };
        }
        if let Some(margins) = section.child(namespace, "pgMar") {
            if let Some(value) = twips(margins, "top") {
                metrics.margin_top = value;
            }
            if let Some(value) = twips(margins, "right") {
                metrics.margin_right = value;
            }
            if let Some(value) = twips(margins, "bottom") {
                metrics.margin_bottom = value;
            }
            if let Some(value) = twips(margins, "left") {
                metrics.margin_left = value;
            }
        }

        metrics
    }
}

fn find_element<'a>(
    element: &'a wp_xml::tree::Element,
    local: &str,
) -> Option<&'a wp_xml::tree::Element> {
    if element.is(Some(wp_docx::WORDPROCESSING_NAMESPACE), local) {
        return Some(element);
    }
    element.child_elements().find_map(|child| find_element(child, local))
}

/// Reads a measurement attribute, converting twentieths of a point to points.
fn twips(element: &wp_xml::tree::Element, name: &str) -> Option<f32> {
    element
        .attribute(Some(wp_docx::WORDPROCESSING_NAMESPACE), name)
        .and_then(|text| text.parse::<f32>().ok())
        .map(|value| value / TWIPS_PER_POINT)
}

/// How the letters of a run are decorated, once the colour has been settled.
///
/// The document says the effect and names a colour for it; by the time a glyph
/// is placed both have been worked out, and this is what drawing needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphEffect {
    pub kind: wp_docx::effects::Effect,
    pub color: Color,
}

/// Whether a run's letters are drawn as capitals.
///
/// The text itself is not changed — that is what separates this from Change
/// Case, and why turning it off gives the small letters back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Caps {
    #[default]
    None,
    /// Every letter a capital, all the same size.
    All,
    /// Every letter a capital, but the ones that were small drawn smaller —
    /// which is what makes small capitals read as small capitals rather than as
    /// shouting.
    Small,
}

/// How tall a small capital is beside a full one.
///
/// Word synthesises small capitals by shrinking the capitals rather than by
/// asking the font for its own, and this is the proportion it shrinks them to.
const SMALL_CAPS_RATIO: f32 = 0.8;
/// A glyph with a place on the page, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedGlyph {
    /// Index of the face in the font library.
    pub face: usize,
    pub glyph: GlyphId,
    /// Where the glyph's origin sits.
    pub x: f32,
    pub baseline: f32,
    /// How far the pen moves after it, which is what makes a caret land between
    /// two letters rather than on one.
    pub advance: f32,
    /// Height of one em in pixels, which is what the outline is scaled by.
    pub size: f32,
    /// How wide the glyph is drawn beside how tall: one for a letter at its own
    /// proportions, which is nearly every letter. Word's Scale sets this, and
    /// it stretches the outline rather than only the room after it — a letter
    /// at 150 per cent is a wide letter, not a normal letter with a gap.
    pub stretch: f32,
    pub color: Color,
    /// Where in the document this glyph came from.
    ///
    /// Without this a page is a picture: something to look at but not to click
    /// in. Carrying the position through is what lets a click become a caret.
    pub source: TextPosition,
    /// Byte length of the character it draws, so a caret can step past it.
    pub source_length: usize,
    /// The shadow, outline, glow or reflection this glyph is drawn with.
    pub effect: Option<GlyphEffect>,
    /// A character that takes up room but draws nothing: a tab, a line break.
    ///
    /// Recorded rather than left out so that the caret, a click and a selection
    /// band all treat a tab like any other character. Leaving it out would put
    /// the caret on the wrong side of one.
    pub invisible: bool,
    /// Where the glyph is drawn from where the pen stands, on the page.
    ///
    /// Nothing for nearly every glyph: the pen's place is the glyph's. A
    /// letter that stands upright in a line running down the page is the
    /// exception — the pen walks down the turned baseline and the letter is
    /// drawn beside it, centred on the column — and so is a run set across
    /// such a line. The pen's place is what a caret and a click are measured
    /// against; this is only where the drawing lands. See [`Turn::Upright`].
    pub shift_x: f32,
    pub shift_y: f32,
}

/// Which way a run of text is turned on the page.
///
/// Word's Text Direction in a table cell or a text box, or a section set
/// down the page rather than across it. See [`Frame`], which is how a turn
/// becomes coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Turn {
    #[default]
    None,
    /// A right angle clockwise: the text reads downwards.
    Down,
    /// A right angle the other way: the text reads upwards.
    Up,
    /// Two right angles: the text reads leftwards, upside down. Never asked
    /// for by a document; what two clockwise turns come to, one inside the
    /// other.
    Over,
    /// Not a turn of the line but of one letter against it: the letter stands
    /// upright in a line that reads downwards, which is how every ideograph
    /// and every kana is set in vertical writing. Only ever on a glyph, never
    /// on a [`Frame`]; and it stays what it is however the line it is in is
    /// turned again, because there is no drawing an upright letter other than
    /// upright.
    Upright,
}

impl Turn {
    /// How many right angles clockwise.
    const fn quarters(self) -> u8 {
        match self {
            Self::None | Self::Upright => 0,
            Self::Down => 1,
            Self::Over => 2,
            Self::Up => 3,
        }
    }

    const fn from_quarters(quarters: u8) -> Self {
        match quarters % 4 {
            0 => Self::None,
            1 => Self::Down,
            2 => Self::Over,
            _ => Self::Up,
        }
    }

    /// This turn, and then another on top of it: what a letter turned with
    /// its cell is turned when the page the cell is on is turned too.
    #[must_use]
    pub fn then(self, outer: Self) -> Self {
        if self == Self::Upright || outer == Self::Upright {
            return Self::Upright;
        }
        Self::from_quarters(self.quarters() + outer.quarters())
    }

    /// The turn that undoes this one.
    #[must_use]
    pub fn inverse(self) -> Self {
        match self {
            Self::Upright => Self::Upright,
            other => Self::from_quarters(4 - other.quarters()),
        }
    }

    /// The turn in radians, clockwise on a canvas, for a letter that is
    /// turned at all.
    #[must_use]
    pub fn radians(self) -> Option<f32> {
        match self {
            Self::None | Self::Upright => None,
            Self::Down => Some(core::f32::consts::FRAC_PI_2),
            Self::Over => Some(core::f32::consts::PI),
            Self::Up => Some(-core::f32::consts::FRAC_PI_2),
        }
    }
}

/// Where a line's own coordinates sit on the page.
///
/// # Why the text is laid out straight and turned afterwards
///
/// Because breaking a line, spacing it, aligning it and numbering it are the
/// same work whichever way up the text is. Turned text is laid out into a box
/// as long as the cell is tall — or as long as the page is, for a section
/// written down the page — and this is what maps that box onto the page: the
/// layout never learns about angles, and only the drawing does.
///
/// The origin is the corner the box's own origin lands on — the cell's top
/// right for text reading downwards, its bottom left for text reading upwards —
/// so that the first letter of the first line is where a reader would start.
///
/// A frame is a turn and a place, and once in a while a mirror as well:
/// Mongolian reads downwards like Japanese but its lines go across to the
/// right rather than the left, which no turn alone gives. The mirror is a
/// flip of the box's own down axis before the turn, and it changes nothing
/// about which way up a letter is drawn — only where the next line goes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub turn: Turn,
    /// Whether the lines go the other way from where the turn alone would
    /// stack them.
    pub flipped: bool,
    pub x: f32,
    pub y: f32,
}

impl Frame {
    /// A frame that turns about a corner, with the lines stacking the way
    /// the turn stacks them.
    #[must_use]
    pub const fn turned(turn: Turn, x: f32, y: f32) -> Self {
        Self { turn, flipped: false, x, y }
    }

    /// The turn's own mapping of a direction, with the flip before it: what
    /// the box's axes become on the page, without the move.
    fn linear(self, x: f32, y: f32) -> (f32, f32) {
        let y = if self.flipped { -y } else { y };
        match self.turn {
            Turn::None | Turn::Upright => (x, y),
            Turn::Down => (-y, x),
            Turn::Over => (-x, -y),
            Turn::Up => (y, -x),
        }
    }

    /// And the same mapping undone.
    fn linear_back(self, x: f32, y: f32) -> (f32, f32) {
        let (x, y) = match self.turn {
            Turn::None | Turn::Upright => (x, y),
            Turn::Down => (y, -x),
            Turn::Over => (-x, -y),
            Turn::Up => (-y, x),
        };
        if self.flipped {
            (x, -y)
        } else {
            (x, y)
        }
    }

    /// A point of the box, as a point of the page.
    #[must_use]
    pub fn on_page(self, x: f32, y: f32) -> (f32, f32) {
        let (x, y) = self.linear(x, y);
        (self.x + x, self.y + y)
    }

    /// And back, which is what a click on the page asks.
    #[must_use]
    pub fn in_frame(self, x: f32, y: f32) -> (f32, f32) {
        self.linear_back(x - self.x, y - self.y)
    }

    /// A direction of the box — a move, with no place to it — as a direction
    /// of the page. What a glyph's shift, which is a move and not a place,
    /// goes through.
    #[must_use]
    pub fn direction_on_page(self, x: f32, y: f32) -> (f32, f32) {
        self.linear(x, y)
    }

    /// A rectangle of the box, as a rectangle of the page.
    ///
    /// A right angle turns a rectangle into a rectangle, which is why a
    /// selection band and a caret need nothing cleverer than this.
    #[must_use]
    pub fn rect(&self, x: f32, y: f32, width: f32, height: f32) -> (f32, f32, f32, f32) {
        if !self.is_turned() {
            return (self.x + x, self.y + y, width, height);
        }
        // The far corner of the rectangle is the near one after a turn, so
        // both corners are mapped and the lesser of each is taken.
        let (one_x, one_y) = self.on_page(x, y);
        let (other_x, other_y) = self.on_page(x + width, y + height);
        (one_x.min(other_x), one_y.min(other_y), (other_x - one_x).abs(), (other_y - one_y).abs())
    }

    /// Whether the text is turned at all.
    #[must_use]
    pub fn is_turned(&self) -> bool {
        self.turn != Turn::None || self.flipped
    }

    /// Whether a right angle is in it, so that the box's across is the page's
    /// down: what says a line's band is a column.
    #[must_use]
    pub fn is_sideways(&self) -> bool {
        matches!(self.turn, Turn::Down | Turn::Up)
    }

    /// This frame inside another: where the box of a turned cell lands when
    /// the page it was laid out on is itself turned.
    ///
    /// A point goes through this frame and then the outer one, so the result
    /// is the frame that does both at once. The turns add; a flip in the outer
    /// frame turns the inner turn the other way round, which is what a mirror
    /// does to a clock.
    #[must_use]
    pub fn then(self, outer: Self) -> Self {
        let (x, y) = outer.on_page(self.x, self.y);
        let turn = if outer.flipped {
            Turn::from_quarters(outer.turn.quarters() + 4 - self.turn.quarters())
        } else {
            Turn::from_quarters(outer.turn.quarters() + self.turn.quarters())
        };
        Self { turn, flipped: self.flipped != outer.flipped, x, y }
    }

    /// The frame that undoes this one: a page turned and then turned by this
    /// is the page as it was.
    #[must_use]
    pub fn inverse(self) -> Self {
        let turn = if self.flipped { self.turn } else { self.turn.inverse() };
        let back = Self { turn, flipped: self.flipped, x: 0.0, y: 0.0 };
        let (x, y) = back.on_page(-self.x, -self.y);
        Self { x, y, ..back }
    }

    /// The same mapping, as a transform — for the things that are drawn from
    /// outlines rather than from rectangles.
    #[must_use]
    pub fn transform(&self) -> wp_raster::Transform {
        // The transform's columns are where the box's axes land: x' = a·x +
        // c·y + e, y' = b·x + d·y + f.
        let (a, b) = self.linear(1.0, 0.0);
        let (c, d) = self.linear(0.0, 1.0);
        wp_raster::Transform { a, b, c, d, e: self.x, f: self.y }
    }
}

/// Whether a line belongs to a cell, by where the two of them are.
///
/// Geometry rather than a recorded owner, because a line does not know which
/// cell it is in — and does not need to: the page holds the cell rectangles and
/// the lines, and where a line sits is what says which cell holds it. The
/// middle of the line is what is asked about, so that a word too long for its
/// cell, which is drawn hanging over the edge, still belongs to the cell it was
/// typed in.
fn in_cell(line: &PageLine, cell: &PlacedCell) -> bool {
    let (x, y, width, height) = line.band(line.left, line.right);
    let middle_x = x + width / 2.0;
    let middle_y = y + height / 2.0;
    middle_x >= cell.x
        && middle_x <= cell.x + cell.width
        && middle_y >= cell.y
        && middle_y <= cell.y + cell.height
}

/// One line of text on a page.
///
/// Kept because a caret and a mouse click are line-shaped questions: which line
/// is this point on, and where in the document does that line begin and end.
#[derive(Clone, Debug, PartialEq)]
pub struct PageLine {
    pub baseline: f32,
    pub ascent: f32,
    pub descent: f32,
    /// Where the line's text starts and ends horizontally.
    pub left: f32,
    pub right: f32,
    /// Which glyphs of the page belong to it.
    pub glyphs: core::ops::Range<usize>,
    pub paragraph: usize,
    /// The stretch of the paragraph's text this line covers.
    pub start_offset: usize,
    pub end_offset: usize,
    /// Where the numbers above sit on the page.
    ///
    /// They are the line's own, measured along the text and across it, which
    /// for a line in a turned cell is not the same as along the page and down
    /// it. Everything that asks a line a question about the page — a click, a
    /// caret, a selection band — goes through here. See [`Frame`].
    pub frame: Frame,
}

impl PageLine {
    /// The vertical band the line occupies, in the line's own coordinates.
    #[must_use]
    pub fn top(&self) -> f32 {
        self.baseline - self.ascent
    }

    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.baseline + self.descent
    }

    /// A stretch of the line, as a rectangle of the page.
    ///
    /// `from` and `to` are along the text, which is across the page for an
    /// ordinary line and down it for a turned one.
    #[must_use]
    pub fn band(&self, from: f32, to: f32) -> (f32, f32, f32, f32) {
        self.frame.rect(from, self.top(), (to - from).max(0.0), self.ascent + self.descent)
    }
}

/// How a line of interface text should look.
///
/// Only what a toolbar needs: the document's own formatting comes from its runs
/// and its styles, not from here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

/// A picture, decoded once and placed on a page.
///
/// The pixels are shared rather than copied: the same picture can appear many
/// times in a document, and laying the document out again must not decode it
/// again.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedImage {
    pub x: f32,
    /// The top edge, not the baseline.
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub image: Rc<Image>,
    /// Which drawing is over which where two overlap, and whether this one is
    /// over the text. See [`Page::drawings_under`].
    pub depth: u32,
    pub over_text: bool,
    /// Where in the document it is, so a press on it can say which picture was
    /// pressed. A shape carries the same. See [`PlacedShape::at`].
    pub at: Option<TextPosition>,
    /// How far round it is turned as it is drawn, in radians, and whether it is
    /// drawn as its own mirror image. See [`wp_docx::floating::Turned`].
    pub turn: f32,
    pub flipped_across: bool,
    pub flipped_down: bool,
    /// What it is called in a list of the document's drawings: the name the
    /// file gives it, and "Picture" for one that carries none.
    pub name: String,
    /// Whether this is the frame of a video, which is drawn with the play sign
    /// over it that says so.
    pub video: bool,
}

/// One cell of a table, as the page holds it.
///
/// Only where it is. What is in it is text like any other text, and is on the
/// page already; this is the rectangle round it, which nothing else records.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedCell {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Somewhere in the document inside the cell, which is how anything acting
    /// on the cell says which cell it means.
    pub at: TextPosition,
}

impl PlacedCell {
    /// Which of the cell's four edges a point is nearest, and how far away it
    /// is.
    ///
    /// `None` when the point is not near any of them. Near is measured against
    /// `reach`, and a point outside the cell altogether is not near its edges:
    /// a pen held over the next cell is over the next cell.
    #[must_use]
    pub fn edge_near(&self, x: f32, y: f32, reach: f32) -> Option<(CellEdge, f32)> {
        if x < self.x - reach
            || x > self.x + self.width + reach
            || y < self.y - reach
            || y > self.y + self.height + reach
        {
            return None;
        }
        [
            (CellEdge::Top, (y - self.y).abs()),
            (CellEdge::Bottom, (y - (self.y + self.height)).abs()),
            (CellEdge::Start, (x - self.x).abs()),
            (CellEdge::End, (x - (self.x + self.width)).abs()),
        ]
        .into_iter()
        .filter(|(_, away)| *away <= reach)
        .min_by(|(_, one), (_, two)| one.total_cmp(two))
    }
}

/// A shape drawn on the page, with whatever is written inside it.
///
/// The text is a page of its own, already laid out and already positioned: a
/// text box is a small document, and laying one out is laying out a document
/// into a smaller box.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedShape {
    pub x: f32,
    /// The top edge, not the baseline.
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The geometry to draw, worked out from the shape's preset name.
    pub preset: crate::geometry::Preset,
    /// The values behind its yellow handles, which the geometry needs along
    /// with the preset: a rounded rectangle is not one shape but a family of
    /// them. See [`crate::geometry::Adjusts`].
    pub adjusts: crate::geometry::Adjusts,
    /// What is inside it: nothing, one colour, a gradient or a hatching.
    pub fill: crate::paint::Paint,
    pub outline: Option<Color>,
    /// How thick the line round it is, in pixels.
    pub outline_weight: f32,
    /// What is drawn at the two ends of that line: the arrowheads of a
    /// connector. See [`wp_docx::lines`].
    pub head_end: wp_docx::lines::LineEnd,
    pub tail_end: wp_docx::lines::LineEnd,
    /// The shadow under it, from the document theme, and how far it falls in
    /// pixels. See [`wp_docx::theme::Effect`].
    pub shadow: Option<(Color, f32)>,
    /// What it is drawn with besides its fill and its line, with every
    /// measurement already in pixels: the shadows, the glow, the soft edge and
    /// the reflection. See [`Effects`].
    pub effects: Effects,
    /// What makes it solid rather than flat, in pixels. See [`Solid`].
    pub solid: Solid,
    /// The glyphs of the text inside, already placed relative to the page.
    pub text: Vec<PositionedGlyph>,
    /// Which of them are turned, and which way: the words of a text box that
    /// run down it rather than across. See [`Page::turned`], which is the
    /// same thing for the page's own text.
    pub text_turned: Vec<(core::ops::Range<usize>, Turn)>,
    /// What the drawing is called, which is what a list of them shows.
    pub name: String,
    /// The number the file knows it by, which is what a connector names.
    pub id: u32,
    /// Which shapes the two ends of this one are fastened to, for a connector.
    /// See [`wp_docx::joins`].
    pub joins: wp_docx::joins::Joins,
    /// The way a joined connector was routed round the shapes it joins, when it
    /// had to be routed at all. Nothing for every other shape, and for a
    /// connector that goes straight from one point to the other: what is drawn
    /// then comes from the preset and the box, the same as any shape.
    /// See [`crate::connectors::route`].
    pub route: Option<wp_raster::Path>,
    /// Where in the document the drawing is, so a press on it can put the
    /// caret beside it and the commands that act on it can find it.
    pub at: Option<TextPosition>,
    /// Which drawing is over which where two overlap, and whether this one is
    /// over the text. See [`Page::drawings_under`].
    pub depth: u32,
    pub over_text: bool,
    /// The shape this was placed from, kept until its text has been laid out.
    ///
    /// The text inside a shape is a document of its own, and laying one out
    /// while the outer one is being laid out would be the engine calling
    /// itself. So it is done in a pass of its own afterwards, and this is what
    /// that pass works from.
    pub(crate) source: Option<Box<wp_docx::shapes::Shape>>,
    /// How far round it is turned as it is drawn, in radians, and whether it is
    /// drawn as its own mirror image. See [`wp_docx::floating::Turned`].
    pub turn: f32,
    pub flipped_across: bool,
    pub flipped_down: bool,
}

impl PlacedShape {
    /// Which way one of the glyphs of the text inside is turned.
    #[must_use]
    pub fn text_turn_of(&self, glyph: usize) -> Turn {
        self.text_turned
            .iter()
            .filter(|(span, _)| span.contains(&glyph))
            .fold(Turn::None, |so_far, (_, turn)| so_far.then(*turn))
    }
}

/// What a shape is drawn with besides its fill and its line, in pixels.
///
/// The document states these in English metric units and in
/// hundred-thousandths, because that is what the file says and what a file
/// saved again has to say. Drawing them wants pixels and colours, and the
/// turning of the one into the other belongs here, where how big a pixel is
/// happens to be known. See [`wp_docx::shapeeffects`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Effects {
    pub outer_shadow: Option<Shadow>,
    pub inner_shadow: Option<Shadow>,
    pub glow: Option<Glow>,
    /// How far in from its edge the shape fades away.
    pub soft_edge: f32,
    pub reflection: Option<Reflection>,
}

/// A shadow, inside the shape or outside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub colour: Color,
    pub blur: f32,
    /// How far it falls, already worked out into across and down.
    pub across: f32,
    pub down: f32,
}

/// A colour spreading out of the shape's edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    pub colour: Color,
    pub reach: f32,
}

/// The shape again, upside down and fading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reflection {
    pub blur: f32,
    /// How solid it is where it starts, and how far down it has faded away.
    pub start: u8,
    pub fades_by: f32,
    /// How far below the shape it begins.
    pub below: f32,
}

impl Effects {
    /// The document's own numbers, in pixels and colours.
    #[must_use]
    pub(crate) fn of(
        effects: &wp_docx::shapeeffects::Effects,
        scale: f32,
        theme: &wp_docx::theme::Theme,
    ) -> Self {
        // A pixel per English metric unit, by way of the point.
        let pixels = |emu: i64| emu as f32 / wp_docx::shapes::EMU_PER_POINT as f32 * scale;
        let shadow = |shadow: &wp_docx::shapeeffects::Shadow| {
            // The direction is measured clockwise from three o'clock, in
            // sixtieths of a degree, which is the same way round as a canvas
            // counts: down the page is a quarter turn on.
            let angle = (shadow.direction as f32 / 60_000.0).to_radians();
            let distance = pixels(shadow.distance_emu);
            Shadow {
                colour: tinted(&shadow.colour, shadow.alpha, theme),
                blur: pixels(shadow.blur_emu),
                across: distance * angle.cos(),
                down: distance * angle.sin(),
            }
        };
        Self {
            outer_shadow: effects.outer_shadow.as_ref().map(&shadow),
            inner_shadow: effects.inner_shadow.as_ref().map(&shadow),
            glow: effects.glow.as_ref().map(|glow| Glow {
                colour: tinted(&glow.colour, glow.alpha, theme),
                reach: pixels(glow.radius_emu),
            }),
            soft_edge: pixels(effects.soft_edge_emu),
            reflection: effects.reflection.as_ref().map(|reflection| Reflection {
                blur: pixels(reflection.blur_emu),
                start: (reflection.start_alpha.clamp(0, 100_000) * 255 / 100_000) as u8,
                // How far down it has faded to nothing, as a share of the
                // shape's own height.
                fades_by: reflection.end_at.max(1) as f32 / 100_000.0,
                below: pixels(reflection.distance_emu),
            }),
        }
    }

    /// Whether there is anything here at all, which is the usual answer.
    #[must_use]
    pub fn is_nothing(&self) -> bool {
        self.outer_shadow.is_none()
            && self.inner_shadow.is_none()
            && self.glow.is_none()
            && self.soft_edge <= 0.0
            && self.reflection.is_none()
    }

    /// How far outside its own box the shape reaches because of these.
    ///
    /// A shadow falls outside the shape and a glow spreads out of it, and what
    /// is drawn outside the box is what a redraw of that box would leave
    /// behind.
    #[must_use]
    pub fn reach(&self) -> f32 {
        let shadow = self
            .outer_shadow
            .map_or(0.0, |shadow| shadow.blur + shadow.across.abs().max(shadow.down.abs()));
        let glow = self.glow.map_or(0.0, |glow| glow.reach);
        shadow.max(glow)
    }
}

/// A colour with an amount of it, as the format says one.
fn tinted(colour: &wp_docx::colour::Colour, alpha: i32, theme: &wp_docx::theme::Theme) -> Color {
    let solid = Color::from_hex(&colour.resolve(theme)).unwrap_or(Color::rgb(0, 0, 0));
    let alpha = (alpha.clamp(0, 100_000) * 255 / 100_000) as u8;
    Color::rgba(solid.red, solid.green, solid.blue, alpha)
}

/// What makes a shape solid rather than flat, in pixels.
///
/// # How a solid is drawn without drawing in three dimensions
///
/// The depth is a run of copies of the shape stepped back along the way the
/// scene is turned, in the colour of its sides, with the face laid over them:
/// that is what a solid looks like from the front, and it is all that an
/// orthographic camera — the one nearly every document uses — can show. The
/// bevel is a band round the edge of the face, light on the side the light
/// comes from and dark on the other.
///
/// What this does not do is turn the face itself. A shape turned right round in
/// Word is a shape seen at an angle, and drawing that means drawing a shape
/// nobody asked this program to draw yet. See the roadmap.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Solid {
    /// How far back the shape goes, and which way that is on the screen.
    pub depth: f32,
    pub across: f32,
    pub down: f32,
    /// The colour of the sides, or nothing to take the shape's own fill and
    /// darken it.
    pub sides: Option<Color>,
    /// How wide the bevel round the face is, and how hard its light is.
    pub bevel: f32,
    pub shine: f32,
}

impl Solid {
    /// The document's own numbers, in pixels.
    #[must_use]
    pub(crate) fn of(
        depth: &wp_docx::depth::Depth,
        scene: &wp_docx::depth::Scene,
        scale: f32,
        theme: &wp_docx::theme::Theme,
    ) -> Self {
        if depth.is_flat() {
            return Self::default();
        }
        let pixels = |emu: i64| emu as f32 / wp_docx::shapes::EMU_PER_POINT as f32 * scale;
        // Which way the depth goes on the screen: the scene's own turn, seen
        // flat on. Turned round to the right, the far end of the shape is to
        // the left of the near one; tipped forwards, it is above.
        let longitude = (scene.longitude as f32 / 60_000.0).to_radians();
        let latitude = (scene.latitude as f32 / 60_000.0).to_radians();
        let depth_pixels = pixels(depth.extrusion_emu);
        // A solid with no turn at all is looked at straight on, and a depth
        // straight back is a depth nobody can see. Word draws it that way too,
        // and the bevel is what shows instead.
        let (across, down) = (-longitude.sin() * depth_pixels, latitude.sin() * depth_pixels);

        let bevel = depth
            .bevel_top
            .as_ref()
            .map_or(0.0, |bevel| pixels(bevel.width_emu.max(bevel.height_emu)));
        Self {
            depth: depth_pixels,
            across,
            down,
            sides: depth
                .extrusion_colour
                .as_ref()
                .and_then(|colour| Color::from_hex(&colour.resolve(theme))),
            bevel,
            // What it is made of, which is what the light does on it: metal
            // takes a hard edge and matte hardly shows one.
            shine: match depth.material.as_str() {
                "metal" => 1.0,
                "matte" | "dkEdge" | "softEdge" => 0.45,
                _ => 0.7,
            },
        }
    }

    /// Whether there is anything solid about it, which for most shapes there is
    /// not.
    #[must_use]
    pub fn is_flat(&self) -> bool {
        self.depth <= 0.0 && self.bevel <= 0.0
    }
}

/// A shape drawn on a page that is not a rectangle.
///
/// The slices of a pie and the line of a line chart: already in page
/// coordinates, and filled in one colour.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedPath {
    pub path: wp_raster::Path,
    pub color: Color,
}

/// A filled rectangle: an underline, a strikethrough, a rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decoration {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: Color,
}

/// One page, ready to draw.
#[derive(Clone, Debug, Default)]
pub struct Page {
    /// Page size in pixels.
    pub width: f32,
    pub height: f32,
    pub glyphs: Vec<PositionedGlyph>,
    pub images: Vec<PlacedImage>,
    pub shapes: Vec<PlacedShape>,
    pub decorations: Vec<Decoration>,
    /// The cells of every table on the page, as rectangles.
    ///
    /// Which edge of which cell the pointer is nearest cannot be worked out
    /// from the text: a cell's edges are nowhere in it. Word's Border Painter
    /// is a pen dragged along an edge, so it needs the edges.
    pub cells: Vec<PlacedCell>,
    /// Shapes that are not rectangles: the slices of a pie, the line of a line
    /// chart. Already in page coordinates, and drawn over the decorations.
    pub paths: Vec<PlacedPath>,
    /// Somebody's strokes, each lot where its drawing put it.
    pub inks: Vec<PlacedInk>,
    pub lines: Vec<PageLine>,
    /// Which glyphs of the page are turned, and which way.
    ///
    /// Kept as spans rather than on every glyph because a document is mostly
    /// text the ordinary way up, and a word of it should not have to carry a
    /// field that says so a hundred thousand times. See [`Page::turn_of`].
    pub turned: Vec<(core::ops::Range<usize>, Turn)>,
    /// How the page's own text was turned as a whole, for a section written
    /// down the page rather than across it.
    ///
    /// The body is laid out into a box as long as the page is tall and
    /// turned onto the page afterwards — the same way a turned cell is, see
    /// [`Frame`] — and everything the box held is already where it lands.
    /// What is kept here is the turn itself, because the first `turned_glyphs`
    /// glyphs are turned by it on top of whatever their own spans say, and
    /// because a page given back to the engine has to be turned back into its
    /// box before the engine can go on placing text into it. Nothing put on
    /// the page after the turn — a header, a footer, the line numbers — is
    /// turned with it, which is why the count is kept.
    pub frame: Frame,
    pub turned_glyphs: usize,
    /// Which pass of which engine drew it.
    ///
    /// Bookkeeping, not content: two pages that draw the same are the same
    /// page whichever pass drew them, so the stamp is left out of equality.
    /// It is how an engine given pages back tells its own last pass's from
    /// any others — see [`again`]. Public because a page built anywhere
    /// else with `..Page::default()` has to be able to leave it at nought.
    pub stamp: u64,
}

impl PartialEq for Page {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.glyphs == other.glyphs
            && self.images == other.images
            && self.shapes == other.shapes
            && self.decorations == other.decorations
            && self.cells == other.cells
            && self.paths == other.paths
            && self.inks == other.inks
            && self.lines == other.lines
            && self.turned == other.turned
            && self.frame == other.frame
            && self.turned_glyphs == other.turned_glyphs
    }
}

impl Page {
    /// How far the drawn contents reach across, from the leftmost edge to
    /// the rightmost: the glyphs, the lines under them, the pictures and
    /// the shapes.
    ///
    /// Nothing for a page with nothing on it. Used where a page has to be
    /// moved as one piece — a line of text in a window read right to left
    /// moves; it does not have its letters reversed.
    #[must_use]
    pub fn horizontal_extent(&self) -> Option<(f32, f32)> {
        let mut extent: Option<(f32, f32)> = None;
        let mut widen = |from: f32, to: f32| {
            extent = Some(match extent {
                Some((left, right)) => (left.min(from), right.max(to)),
                None => (from, to),
            });
        };
        for glyph in &self.glyphs {
            widen(glyph.x, glyph.x + glyph.advance);
        }
        for decoration in &self.decorations {
            widen(decoration.x, decoration.x + decoration.width);
        }
        for image in &self.images {
            widen(image.x, image.x + image.width);
        }
        for shape in &self.shapes {
            widen(shape.x, shape.x + shape.width);
        }
        for path in &self.paths {
            if let Some((left, _, right, _)) = wp_raster::bounds_of(&path.path) {
                widen(left, right);
            }
        }
        extent
    }
}

/// One drawing on a page, whatever kind it is.
///
/// The two kinds are kept in lists of their own because almost everything that
/// touches them wants one kind or the other. Drawing them is the exception: two
/// that overlap are ordered by the number their anchors carry and not by which
/// kind they are, so the pass that draws them needs them in one sequence.
#[derive(Clone, Copy, Debug)]
pub enum Drawing<'a> {
    Picture(&'a PlacedImage),
    Shape(&'a PlacedShape),
    Ink(&'a PlacedInk),
}

/// Ink on a page: somebody's strokes, laid out and put where they go.
///
/// A drawing like a picture is: it has a box, a place in the text, a depth
/// among the other drawings, and a name. The strokes are already where the
/// box is.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedInk {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The strokes, in page coordinates.
    pub drawing: crate::inking::InkDrawing,
    /// Which drawing is over which where two overlap, and whether this one is
    /// over the text. See [`Page::drawings_under`].
    pub depth: u32,
    pub over_text: bool,
    /// Where in the document it is, so a press on it can say which ink was
    /// pressed. See [`PlacedShape::at`].
    pub at: Option<TextPosition>,
    pub name: String,
}

impl Page {
    /// Which way one of the page's glyphs is turned.
    #[must_use]
    pub fn turn_of(&self, glyph: usize) -> Turn {
        // A page has one span per turned cell and most pages have none, so this
        // is a walk over nothing at all in the ordinary case. A glyph in a
        // turned cell on a page turned as a whole is in two spans at once, and
        // is turned by both.
        let own = self
            .turned
            .iter()
            .filter(|(span, _)| span.contains(&glyph))
            .fold(Turn::None, |so_far, (_, turn)| so_far.then(*turn));
        if glyph < self.turned_glyphs {
            own.then(self.frame.turn)
        } else {
            own
        }
    }

    /// The drawings that go under the text, in the order they are drawn.
    #[must_use]
    pub fn drawings_under(&self) -> Vec<Drawing<'_>> {
        self.drawings_where(false)
    }

    /// And the ones that go over it: Word's "in front of text", which is every
    /// floating drawing that does not say `behindDoc`.
    #[must_use]
    pub fn drawings_over(&self) -> Vec<Drawing<'_>> {
        self.drawings_where(true)
    }

    /// One layer's drawings, nearest the reader last.
    ///
    /// Ordered by the anchor's number, and where two carry the same number —
    /// which is what every drawing in the line of text does — by where they
    /// stand in the document. That is Word's tie-break too.
    fn drawings_where(&self, over: bool) -> Vec<Drawing<'_>> {
        let mut out: Vec<(u32, u8, usize, Drawing<'_>)> = Vec::new();
        for (at, picture) in self.images.iter().enumerate() {
            if picture.over_text == over {
                out.push((picture.depth, 0, at, Drawing::Picture(picture)));
            }
        }
        for (at, shape) in self.shapes.iter().enumerate() {
            if shape.over_text == over {
                out.push((shape.depth, 1, at, Drawing::Shape(shape)));
            }
        }
        for (at, ink) in self.inks.iter().enumerate() {
            if ink.over_text == over {
                out.push((ink.depth, 2, at, Drawing::Ink(ink)));
            }
        }
        out.sort_by_key(|(depth, kind, at, _)| (*depth, *kind, *at));
        out.into_iter().map(|(_, _, _, drawing)| drawing).collect()
    }

    /// The place in the document a point on the page corresponds to.
    ///
    /// Used to turn a click into a caret. A point below the last line lands at
    /// the end of the page rather than nowhere, which is what a person means
    /// when they click in the empty space under the text.
    #[must_use]
    pub fn position_at(&self, x: f32, y: f32) -> Option<TextPosition> {
        if self.lines.is_empty() {
            return None;
        }

        // A point inside a cell is answered by that cell's own lines, and by
        // nothing else.
        //
        // # Why the cell has to come first
        //
        // Because the cells of a row are side by side: their lines all sit at
        // the same height, and asking which line a point is level with says
        // only which row it is in. Without the cell, every click in a row
        // landed in whichever of its cells came first — a table nobody could
        // put the caret in, which is most of what a table is for.
        let cell = self.cell_at(x, y);
        let line = self.nearest_line(x, y, cell).or_else(|| self.nearest_line(x, y, None))?;

        let (along, _) = line.frame.in_frame(x, y);

        // Before the first glyph or after the last, the answer is one end.
        if along <= line.left {
            return Some(TextPosition::new(line.paragraph, line.start_offset));
        }
        if along >= line.right {
            return Some(TextPosition::new(line.paragraph, line.end_offset));
        }

        for index in line.glyphs.clone() {
            let glyph = &self.glyphs[index];
            let at = self.along_line(line, glyph);
            if along < at + glyph.advance {
                // Past the middle of a letter means the caret goes after it,
                // which is what makes clicking feel like it lands where aimed.
                let offset = if along > at + glyph.advance / 2.0 {
                    glyph.source.offset + glyph.source_length
                } else {
                    glyph.source.offset
                };
                return Some(TextPosition::new(glyph.source.paragraph, offset));
            }
        }

        Some(TextPosition::new(line.paragraph, line.end_offset))
    }

    /// The cell a point is in, innermost first.
    ///
    /// Innermost, because a table inside a cell is inside that cell: the point
    /// is in both, and the one that decides where a click goes is the smallest
    /// that holds it.
    #[must_use]
    pub fn cell_at(&self, x: f32, y: f32) -> Option<&PlacedCell> {
        self.cells
            .iter()
            .filter(|cell| {
                x >= cell.x && x <= cell.x + cell.width && y >= cell.y && y <= cell.y + cell.height
            })
            .min_by(|one, other| (one.width * one.height).total_cmp(&(other.width * other.height)))
    }

    /// The line a point is nearest, out of a cell's lines or out of all of them.
    ///
    /// Nearest across the text first and along it second: a point level with a
    /// line belongs to that line wherever along it the point is, which is what
    /// makes clicking in the margin beside a line put the caret on that line.
    /// The second measure only settles which of several lines at one height is
    /// meant — the case a table makes, and a text box beside text.
    fn nearest_line(&self, x: f32, y: f32, cell: Option<&PlacedCell>) -> Option<&PageLine> {
        let across = |line: &PageLine| {
            let across = line.frame.in_frame(x, y).1;
            if across < line.top() {
                line.top() - across
            } else if across > line.bottom() {
                across - line.bottom()
            } else {
                0.0
            }
        };
        let along = |line: &PageLine| {
            let along = line.frame.in_frame(x, y).0;
            if along < line.left {
                line.left - along
            } else if along > line.right {
                along - line.right
            } else {
                0.0
            }
        };

        self.lines.iter().filter(|line| cell.is_none_or(|cell| in_cell(line, cell))).min_by(
            |first, second| {
                across(first)
                    .total_cmp(&across(second))
                    .then_with(|| along(first).total_cmp(&along(second)))
            },
        )
    }

    /// Where a caret at a position should be drawn, as a rectangle.
    ///
    /// A rectangle rather than a left edge and a height, because a caret in a
    /// cell whose text is turned lies the other way: it is as long as the line
    /// is tall and as thick as `thickness`, and only the page knows which way
    /// round that comes out. See [`Frame`].
    #[must_use]
    pub fn caret_at(&self, position: TextPosition, thickness: f32) -> Option<(f32, f32, f32, f32)> {
        let line = self.line_of(position)?;

        let mut along = line.left;
        for index in line.glyphs.clone() {
            let glyph = &self.glyphs[index];
            let at = self.along_line(line, glyph);
            if glyph.source.offset >= position.offset {
                along = at;
                break;
            }
            along = at + glyph.advance;
        }
        if position.offset >= line.end_offset {
            along = line.right;
        }

        Some(line.band(along, along + thickness.max(1.0)))
    }

    /// The line a position falls on.
    #[must_use]
    pub fn line_of(&self, position: TextPosition) -> Option<&PageLine> {
        self.lines
            .iter()
            .find(|line| {
                line.paragraph == position.paragraph
                    && position.offset >= line.start_offset
                    && position.offset <= line.end_offset
            })
            .or_else(|| self.lines.iter().find(|line| line.paragraph == position.paragraph))
    }

    /// Whether any of this page's text comes from a given paragraph.
    #[must_use]
    pub fn holds_paragraph(&self, paragraph: usize) -> bool {
        self.lines.iter().any(|line| line.paragraph == paragraph)
    }

    /// The bands that highlight a selection on this page, as `(x, y, w, h)`.
    ///
    /// A selection is two positions, but showing one means a band per line: the
    /// tail of the first line, whole lines between, the head of the last. Only
    /// the part that falls on this page is returned, so a selection running
    /// across a page break is asked of each page in turn.
    ///
    /// A line whose paragraph break is inside the selection gets a short band
    /// past its last letter, so the break itself looks selected. Without it
    /// there is nothing on screen to say that pressing Delete will join the
    /// paragraphs.
    #[must_use]
    pub fn selection_rects(
        &self,
        start: TextPosition,
        end: TextPosition,
    ) -> Vec<(f32, f32, f32, f32)> {
        let mut rects = Vec::new();
        if start >= end {
            return rects;
        }

        for (index, line) in self.lines.iter().enumerate() {
            if line.paragraph < start.paragraph || line.paragraph > end.paragraph {
                continue;
            }

            let from = if line.paragraph == start.paragraph {
                start.offset.max(line.start_offset)
            } else {
                line.start_offset
            };
            let to = if line.paragraph == end.paragraph {
                end.offset.min(line.end_offset)
            } else {
                line.end_offset
            };
            if from > to {
                continue;
            }

            let left = self.offset_x(line, from);
            let mut right = self.offset_x(line, to);

            // The last line of a paragraph that is not the last in the
            // selection carries a break, and the break is selected too.
            let ends_paragraph =
                self.lines.get(index + 1).is_none_or(|next| next.paragraph != line.paragraph);
            if ends_paragraph && line.paragraph < end.paragraph && to >= line.end_offset {
                right = right.max(line.right) + BREAK_HIGHLIGHT_WIDTH;
            }

            if right <= left {
                continue;
            }
            rects.push(line.band(left, right));
        }

        rects
    }

    /// Where an offset sits along one line.
    ///
    /// Along the text rather than across the page: for a line in a turned cell
    /// the two are not the same, and every answer a line gives is in the line's
    /// own coordinates. See [`Frame`].
    fn offset_x(&self, line: &PageLine, offset: usize) -> f32 {
        if offset >= line.end_offset {
            return line.right;
        }
        let mut x = line.left;
        for index in line.glyphs.clone() {
            let glyph = &self.glyphs[index];
            let along = self.along_line(line, glyph);
            if glyph.source.offset >= offset {
                return along;
            }
            x = along + glyph.advance;
        }
        x
    }

    /// How far along its line a glyph's origin is.
    ///
    /// A glyph carries where it is *on the page*, because that is what drawing
    /// it needs. A line is asked questions in its own coordinates, so the one
    /// is turned into the other here — which for an untouched line is a
    /// subtraction and nothing more.
    fn along_line(&self, line: &PageLine, glyph: &PositionedGlyph) -> f32 {
        if !line.frame.is_turned() {
            return glyph.x;
        }
        line.frame.in_frame(glyph.x, glyph.baseline).0
    }
}

/// How a run should look, reduced to what drawing needs.
#[derive(Clone, Debug)]
pub(crate) struct RunStyle {
    face: usize,
    size: f32,
    color: Color,
    /// The colour drawn behind the text, when the run asks for one.
    highlight: Option<Color>,
    underline: bool,
    /// The colour of the underline, when the run asks for one of its own.
    underline_color: Option<Color>,
    strike: bool,
    /// A second line through the text, half a line's width above the first.
    double_strike: bool,
    right_to_left: bool,
    /// Whether the letters are drawn as capitals, and whether the ones that
    /// were already small are drawn smaller. See [`Caps`].
    caps: Caps,
    /// Which language's rules the capitals of this run follow. The capital of
    /// a Turkish i is not the capital of an English one. See
    /// [`wp_docx::casing`].
    casing: wp_docx::casing::Tailoring,
    /// Where the words of this run's language may be broken, when the
    /// machine has the patterns for it. See [`Hyphenation`].
    hyphenation: Option<Rc<wp_dict::hyphenation::Patterns>>,
    /// How wide the letters are drawn, as a fraction of their own width.
    /// One for text at its natural width, which is nearly all of it.
    stretch: f32,
    /// Room added after every glyph, in pixels. Negative pulls them together.
    letter_spacing: f32,
    /// Whether the font's own kerning is used. Word turns it off below a size
    /// the document names, because kerning small text costs more than it is
    /// worth.
    kern: bool,
    /// Text that is in the document but not shown. Word draws it only while the
    /// formatting marks are showing, and gives it a dotted underline so it can
    /// be told apart from text that will be printed.
    hidden: bool,
    /// The OpenType features the run asks the font for, as their tags. Empty
    /// for the overwhelming majority of runs, which is what keeps the ordinary
    /// path as fast as it was.
    features: Rc<Vec<[u8; 4]>>,
    /// The shadow, outline, glow or reflection the run asks for.
    effect: Option<GlyphEffect>,
    /// How far off the line the run rides, positive upwards. Zero for text on
    /// the line, which is nearly all of it.
    raise: f32,
    ascent: f32,
    descent: f32,
    pub(crate) line_height: f32,
    /// The run set across a vertical line, or as two lines in one: Word's
    /// Asian Layout. See [`wp_docx::eastasian`].
    east_asian: wp_docx::eastasian::EastAsianLayout,
}

/// What one stored character is drawn as, and at what size.
///
/// Nearly always itself at the run's size, which is why the common answer costs
/// one comparison and no allocation. A run set in capitals draws the capital
/// instead; a run set in small capitals draws it smaller when the letter it
/// came from was small, which is the whole of what small capitals are.
///
/// Several characters can come back for one, because in a few languages a
/// capital is longer than the letter it capitalises — ß becomes SS, ﬁ becomes
/// FI. They are drawn one after another and all point back at the one
/// character, which is what keeps a click landing where the text says.
fn drawn_as(character: char, style: &RunStyle) -> SmallCaps {
    match style.caps {
        Caps::None => SmallCaps::One(character, style.size),
        Caps::All => SmallCaps::of(character, style.size, style.casing),
        Caps::Small => {
            // A letter that was already a capital — or is not a letter at all —
            // stays the size it was. Only what was small is drawn small.
            let size =
                if character.is_lowercase() { style.size * SMALL_CAPS_RATIO } else { style.size };
            SmallCaps::of(character, size, style.casing)
        }
    }
}

/// The characters one character is drawn as, without an allocation for the
/// overwhelmingly common case of exactly one.
enum SmallCaps {
    One(char, f32),
    /// A capital that runs to several characters — ß is SS — or to none at
    /// all, which is what happens to an accent a language drops in capitals.
    Several(std::vec::IntoIter<char>, f32),
    Done,
}

impl SmallCaps {
    /// The capitals of a character at a given size, in the language it is
    /// written in.
    fn of(character: char, size: f32, casing: wp_docx::casing::Tailoring) -> Self {
        let mut capitals = wp_docx::casing::upper_char(character, casing);
        match capitals.len() {
            1 => Self::One(capitals.remove(0), size),
            _ => Self::Several(capitals.into_iter(), size),
        }
    }
}

impl Iterator for SmallCaps {
    type Item = (char, f32);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::One(character, size) => {
                let answer = (*character, *size);
                *self = Self::Done;
                Some(answer)
            }
            Self::Several(capitals, size) => {
                let size = *size;
                capitals.next().map(|character| (character, size))
            }
            Self::Done => None,
        }
    }
}

impl RunStyle {
    /// Plain text of a given face, size and colour.
    ///
    /// Everything a document's own runs can ask for is off: this is for the
    /// text the program itself puts on screen — a button's name, a label on a
    /// ruler, a letter inside an equation — none of which is formatted by the
    /// document and all of which would otherwise have to name a dozen fields
    /// only to say "no" to each of them.
    fn plain(face: usize, size: f32, color: Color) -> Self {
        Self {
            face,
            size,
            color,
            casing: wp_docx::casing::Tailoring::Default,
            hyphenation: None,
            highlight: None,
            effect: None,
            underline: false,
            underline_color: None,
            strike: false,
            double_strike: false,
            right_to_left: false,
            caps: Caps::None,
            stretch: 1.0,
            letter_spacing: 0.0,
            kern: true,
            hidden: false,
            features: Rc::new(Vec::new()),
            raise: 0.0,
            ascent: size * 0.8,
            descent: size * 0.2,
            east_asian: wp_docx::eastasian::EastAsianLayout::default(),
            line_height: size * 1.2,
        }
    }
}

/// A glyph that has been chosen but not yet placed.
#[derive(Clone, Copy, Debug)]
struct ShapedGlyph {
    face: usize,
    glyph: GlyphId,
    advance: f32,
    /// Where it is drawn from where the pen stands, which is nothing for
    /// nearly every glyph and is the whole of where a mark goes. Right and up
    /// are positive, as the font says them. See [`wp_shape::Placement`].
    x_offset: f32,
    y_offset: f32,
    /// Whether it takes its place in the text and draws nothing.
    ///
    /// The optional hyphen is the one that does: it is a mark the writer put
    /// inside a word to say the word may be broken there, and it is drawn only
    /// if the line is broken there — by the line, not by the glyph.
    invisible: bool,
    /// Byte offset of the character it draws, within the paragraph text.
    offset: usize,
    /// Byte length of that character.
    length: usize,
    /// The character it was chosen for, kept because a bracket in a line that
    /// reads right to left is drawn as the other end of its pair and the glyph
    /// has to be chosen again.
    character: char,
    /// How tall this glyph is drawn, which is the run's size for all but one
    /// case: a small capital is a capital drawn smaller than the capitals
    /// beside it, and so carries a size of its own.
    size: f32,
    /// Where the glyph is drawn from where the pen stands, when that is not
    /// the same place: `Some` for a letter standing upright in a line that
    /// runs down the page, or set across such a line, or set as one of two
    /// lines in one. In the line's own coordinates; see
    /// [`PositionedGlyph::shift_x`].
    upright: Option<(f32, f32)>,
    /// How much narrower the glyph is drawn than its own width, on top of the
    /// run's stretch: one for nearly every glyph, and less for a run squeezed
    /// into a square across a vertical line.
    squeeze: f32,
}

/// What was worked out for one paragraph last time it was laid out.
///
/// Typing a letter changes one paragraph. Everything the layout knows about
/// the other eleven thousand is still true, and measuring them again — asking
/// the font for every glyph, its width and its kerning — is most of what a
/// keystroke costs. So the answer is kept, with the paragraph it was worked out
/// from, and used again when that paragraph has not changed.
///
/// Only paragraphs of plain text are kept. A field says a different thing when
/// the page it is on changes, a note carries a number that is worked out while
/// laying out, and a picture is a shared handle rather than something to copy —
/// so those are measured afresh every time, which is right and costs little
/// because there are few of them.
#[derive(Clone, Debug)]
struct Measured {
    /// The paragraph this was worked out from, to tell whether it still holds.
    paragraph: Paragraph,
    items: Vec<Item>,
    styles: Vec<RunStyle>,
    /// The direction of each item, which the text and the styles decide.
    levels: Vec<u8>,
    /// Whether it was measured for a line running down the page, where the
    /// letters of the East Asian scripts stand upright and take the room the
    /// font gives them that way round.
    vertical: bool,
}

/// Whether a paragraph is one whose measurements may be kept.
fn is_plain(paragraph: &Paragraph) -> bool {
    paragraph.runs.iter().all(|run| {
        run.field.is_none()
            && run.revision.is_none()
            && run
                .content
                .iter()
                .all(|content| matches!(content, RunContent::Text(_) | RunContent::Tab))
    })
}

/// The smallest thing a line can be broken between.
///
/// Shared with [`crate::tablefit`], which measures a cell by building its items
/// and never breaking them into lines: how wide the widest of them is, and how
/// wide all of them together are, is what a column has to be to hold it.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    glyphs: Vec<ShapedGlyph>,
    pub(crate) width: f32,
    /// Whitespace collapses at the end of a line rather than being drawn.
    pub(crate) is_space: bool,
    /// Whether a line may end just before this item. Almost everything may
    /// begin one; a full stop that a change of formatting left in a run of its
    /// own may not.
    breaks_before: bool,
    /// A tab, whose width is not known until the line is being placed: it
    /// reaches to the next stop, which depends on where the line has got to.
    is_tab: bool,
    /// Where an alignment tab goes, when the tab is one.
    ///
    /// An ordinary tab looks its target up among the stops; this one is told.
    aligned_tab: Option<wp_docx::model::TabAlignment>,
    /// A picture, with the height it takes up.
    ///
    /// Drawn in the line unless it carries an anchor, in which case the item
    /// holds its place in the text and the drawing goes where the anchor says.
    picture: Option<(Rc<Image>, f32)>,
    /// Where the picture floats, when it does. A shape carries its own inside
    /// the shape; a picture has nowhere else to put it.
    picture_anchor: Option<wp_docx::anchor::Anchor>,
    /// What the picture is called, for a list of the drawings in a document.
    /// The same reason `picture_anchor` is here: a shape carries its name and a
    /// picture has nowhere else to put one.
    picture_name: Option<String>,
    /// How far round the picture is turned, and whether it is mirrored. A
    /// shape carries its own; this is where a picture's lives.
    picture_turn: wp_docx::floating::Turned,
    /// Whether the picture is the frame of a video kept somewhere else.
    picture_video: bool,
    /// An equation drawn in the line, with the height it takes up.
    math: Option<(Box<crate::math::MathBox>, f32)>,
    /// A chart drawn in the line, with the height it takes up.
    chart: Option<(Box<crate::charting::ChartDrawing>, f32)>,
    /// Strokes somebody drew, with the height they take up.
    ink: Option<(Box<crate::inking::InkDrawing>, f32)>,
    /// A word with its reading set over it, and the height it takes up.
    ruby: Option<(Box<crate::ruby::RubyBox>, f32)>,
    /// How wide the hyphen is that would be drawn if the line ended here.
    ///
    /// Nothing for almost every item. An item that ends with an optional
    /// hyphen — the mark a writer puts inside a word to say it may be broken
    /// there — carries the width of the hyphen that is then drawn, which is
    /// what makes the room for it before the line is settled rather than
    /// after. So does a piece of a word the patterns of its language say may
    /// be broken there, when the document hyphenates on its own.
    hyphen: f32,
    /// Whether this is the rest of a word after a place the language's
    /// patterns allow a break: a line may end before it, but only when the
    /// document's hyphenation rules let it — see [`Breaking`].
    auto_hyphen: bool,
    /// A shape drawn in the line, with the height it takes up.
    shape: Option<(Box<wp_docx::shapes::Shape>, f32)>,
    /// A group of drawings in the line, with the height it takes up.
    group: Option<(Box<wp_docx::group::Group>, f32)>,
    /// Forces the rest of the paragraph onto a new line, or a new page.
    hard_break: Option<BreakKind>,
    pub(crate) style: usize,
    /// Where this item sits in the paragraph text, so a line knows the stretch
    /// of the document it covers.
    start_offset: usize,
    end_offset: usize,
}

/// Lays documents out with a given font library and resolution.
#[derive(Debug)]
pub struct LayoutEngine<'a> {
    library: &'a FontLibrary,
    /// Dots per inch. 96 is what Windows calls 100% scaling.
    dpi: f32,
    /// Parsed faces, kept so a font is not re-parsed for every run.
    fonts: HashMap<usize, Font<'a>>,
    /// Decoded pictures, kept so one is not decoded twice.
    ///
    /// A picture that would not decode is remembered as `None`, so a damaged
    /// one is not attempted again on every keystroke either.
    pictures: HashMap<String, Option<Rc<Image>>>,
    /// What text is drawn in when the document says nothing about its colour.
    automatic_color: Color,
    /// The colour a table line takes when the document names none.
    automatic_line: Color,
    /// Which page is being laid out, while the furniture is drawn.
    ///
    /// A `PAGE` field in a footer has to say which page it is on, and that is
    /// only known once the body has been laid out and the pages counted — so
    /// the footer is laid out afterwards, once per page, with this set.
    field_page: Option<(usize, usize, NumberFormat)>,
    /// How far a floating drawing being laid out now will be moved afterwards.
    ///
    /// Nought everywhere but in a running head. A head is laid out on a page
    /// of its own and slid into place when its height is known, and a drawing
    /// it carries is not in that flow — it is anchored to the paper. So what
    /// the slide will add is taken off while the drawing is placed, and the
    /// two cancel. See [`merge_page`].
    drawing_shift: f32,
    /// Whether insertions and deletions are shown as changes rather than as
    /// the text they would leave behind.
    show_markup: bool,
    /// And whether a run whose formatting somebody changed is marked as such.
    ///
    /// Apart from the one above because Word switches them apart: its Show
    /// Markup menu has a line for each. See [`wp_docx::model::FormatChange`].
    show_formatting: bool,
    /// Whether the boundaries of a table with no lines of its own are drawn.
    ///
    /// Word's View Gridlines. Never printed: `wp-pdf` and the printer both lay
    /// the document out with an engine of their own, and neither turns this on.
    table_gridlines: bool,
    /// Whether the formatting marks are showing, which is the only time hidden
    /// text is drawn.
    show_marks: bool,
    /// How much of each page is kept for the footnotes printed at its foot.
    reserved: Vec<f32>,
    /// What each paragraph of the main body measured to last time.
    ///
    /// Indexed by the paragraph's place in the document. A paragraph that has
    /// not changed since is not measured again — see [`Measured`].
    measured: Vec<Option<Measured>>,
    /// Whether the body being laid out is the document's own, which is the only
    /// one the measurements are kept for: a header and the text inside a shape
    /// are counted from zero as well, and would be taken for the body's.
    keeping: bool,
    /// The drawings floating on each page, which narrow the lines beside them.
    floats: Vec<Float>,
    /// The recipient whose values the merge fields show, when one is being
    /// previewed. Empty means the fields show their own names.
    merge_record: Vec<(String, String)>,
    /// What number each `SEQ` field shows, by where it sits.
    ///
    /// Worked out once per layout by walking the document in order. Counting
    /// them as they are met would go wrong the moment a table row was laid out
    /// twice to measure it, which is exactly what happens.
    sequence_numbers: HashMap<(usize, usize), usize>,
    /// Which page each bookmark starts on, for `PAGEREF` to answer with.
    bookmark_pages: HashMap<String, usize>,
    /// What number each note mark shows, by its id.
    ///
    /// Worked out once per layout from where the marks fall in the text: the
    /// number in the file is only an identifier, and a document edited for
    /// years has them in no order at all.
    note_numbers: HashMap<(bool, i32), usize>,
    /// Where each list has got to.
    ///
    /// The third item of a list is only the third because of the two before it,
    /// so counting belongs to the pass over the document rather than to any one
    /// paragraph. Cleared at the start of every layout, so laying the same
    /// document out twice gives the same numbers both times.
    counters: ListCounters,
    /// The deepest heading level drawn, when the document is being shown as an
    /// outline. Nothing means it is being shown as itself.
    ///
    /// Ten means everything, headings and body text alike, which is what Word
    /// calls All Levels.
    outline: Option<u8>,
    /// The level of the last heading passed, so the body text under it can be
    /// indented one step further in.
    outline_heading: u8,
    /// The paragraphs of an outline folded away under a heading, as runs of
    /// their numbers: Word's collapsed headings. See
    /// [`LayoutEngine::set_outline_folded`].
    outline_folded: Vec<core::ops::Range<usize>>,
    /// Whether an outline shows only the first line of each paragraph of
    /// body text, as Word's Show First Line Only does.
    outline_first_line: bool,
    /// Whether an outline is drawn in one plain font, as Word's is with Show
    /// Text Formatting turned off.
    outline_plain: bool,
    /// Whether the paragraph just given to be placed was left out of an
    /// outline, which put nothing on the page — and so did not begin one,
    /// whatever the page's count of lines says. See [`Self::place_blocks`].
    left_out: bool,
    /// How many tables deep the paragraph being placed is. An outline places
    /// a table where body text goes and leaves what is in its cells as it
    /// is: a cell's paragraph is not indented to a level inside its cell.
    in_table: usize,
    /// Whether the document is being shown as a browser would show it: see
    /// [`LayoutEngine::set_web`].
    web: bool,
    /// The document's font table, by family in lower case.
    font_table: HashMap<String, wp_docx::fonts::FontEntry>,
    /// Word's automatic hyphenation, as the document has it set.
    hyphenation: Hyphenation,
    /// Whether the paragraph whose items are being built may have its words
    /// broken by the patterns: the document hyphenates, and the paragraph
    /// does not say to leave its words alone.
    hyphenating: bool,
    /// Whether the text being placed reads down the page, laid out into a box
    /// that is turned afterwards: the letters of the East Asian scripts then
    /// stand upright in the line, and a run may be set across it. See
    /// [`Frame`] and [`Turn::Upright`].
    vertical: bool,
    /// The patterns for each language asked for so far, and none for a
    /// language the machine has no patterns for, so that it is not looked
    /// for again for every run.
    patterns: HashMap<String, Option<Rc<wp_dict::hyphenation::Patterns>>>,
    /// The document's theme: the shadow it puts under a shape, and the
    /// colours and the fills a shape names from it.
    ///
    /// Read once when the document starts being laid out, because a theme is a
    /// property of the document and asking the package for it per shape would
    /// re-parse the theme part for every drawing on the page.
    theme: wp_docx::theme::Theme,
    /// How far apart the stops a tab falls back to are, in twentieths of a
    /// point.
    ///
    /// Read from the document when it is laid out, because every document
    /// says in its settings and half of them say something other than the
    /// half inch the format assumes.
    default_tab: i32,
    /// The style of every paragraph of the document, gathered once per
    /// layout, for the paragraphs that ask about their neighbours.
    paragraph_styles: Vec<Option<String>>,
    /// Which section each page of the document belongs to.
    ///
    /// Kept because a page cannot be asked: it is a picture of paper, and the
    /// header printed on it is the one belonging to the section whose text it
    /// holds. Filled in as the sections are laid out, and only for the document
    /// itself — a header or a footnote is laid out through the same code and
    /// has no sections of its own.
    page_sections: Vec<usize>,
    /// What the last pass over the document's body left behind, for the
    /// next one to take up from. See [`again`].
    remembered: Option<again::Remembered>,
    /// This engine's number, and how many passes it has made: together, the
    /// stamp on the pages of a pass.
    engine: u64,
    passes: u64,
    /// How many writes the document's package had seen when it was last
    /// laid out. A write is a style edited, a picture replaced, a chart's
    /// figures changed — something that alters how a paragraph looks
    /// without altering the paragraph, so everything kept about the
    /// paragraphs is let go when the count moves.
    generation: u64,
    /// How many blocks of the body the last layout of a document placed.
    placed: usize,
    /// Which way a line of interface text reads, where the interface says:
    /// see [`LayoutEngine::set_interface_direction`].
    interface_direction: Option<wp_bidi::Direction>,
}

impl<'a> LayoutEngine<'a> {
    #[must_use]
    pub fn new(library: &'a FontLibrary) -> Self {
        Self {
            library,
            dpi: 96.0,
            fonts: HashMap::new(),
            pictures: HashMap::new(),
            automatic_color: Color::BLACK,
            automatic_line: Color::rgb(0x40, 0x40, 0x40),
            counters: ListCounters::new(),
            field_page: None,
            drawing_shift: 0.0,
            show_markup: true,
            show_formatting: true,
            table_gridlines: false,
            show_marks: false,
            note_numbers: HashMap::new(),
            sequence_numbers: HashMap::new(),
            bookmark_pages: HashMap::new(),
            reserved: Vec::new(),
            measured: Vec::new(),
            keeping: false,
            floats: Vec::new(),
            merge_record: Vec::new(),
            outline: None,
            outline_heading: 0,
            outline_folded: Vec::new(),
            outline_first_line: false,
            outline_plain: false,
            left_out: false,
            in_table: 0,
            web: false,
            font_table: HashMap::new(),
            hyphenation: Hyphenation::default(),
            hyphenating: false,
            vertical: false,
            patterns: HashMap::new(),
            theme: wp_docx::theme::Theme::default(),
            default_tab: DEFAULT_TAB_TWIPS,
            paragraph_styles: Vec::new(),
            page_sections: Vec::new(),
            remembered: None,
            engine: again::engine_number(),
            passes: 0,
            generation: 0,
            placed: 0,
            interface_direction: None,
        }
    }

    /// How many blocks of the body the last layout of a document placed.
    ///
    /// A keystroke into a long document should place a handful, however
    /// long the document is: the pages that did not move are kept. See
    /// [`again`].
    #[must_use]
    pub fn blocks_placed(&self) -> usize {
        self.placed
    }

    /// Sets whether tracked changes are drawn as changes.
    ///
    /// With this off the document is laid out as it would read once every
    /// change had been accepted, which is Word's "No Markup" view.
    #[must_use]
    pub fn with_markup(mut self, shown: bool) -> Self {
        self.set_markup(shown);
        self
    }

    /// The fonts this engine draws with.
    #[must_use]
    pub fn library(&self) -> &FontLibrary {
        self.library
    }

    /// The same, on an engine that is being kept and used again.
    ///
    /// Anything that changes how a paragraph is measured throws away what was
    /// measured before it changed — see [`Measured`].
    pub fn set_table_gridlines(&mut self, shown: bool) {
        self.table_gridlines = shown;
    }

    /// Whether insertions and deletions are shown as changes.
    pub fn set_markup(&mut self, shown: bool) {
        if self.show_markup != shown {
            self.show_markup = shown;
            self.measured.clear();
        }
    }

    /// And whether changed formatting is marked.
    ///
    /// What was measured is thrown away, as above. The mark is only a colour
    /// and a colour cannot move a line break, but the colour is decided while a
    /// paragraph is measured and kept with the measurement — so a paragraph
    /// measured with the mark on would keep it after the switch.
    pub fn set_formatting_markup(&mut self, shown: bool) {
        if self.show_formatting != shown {
            self.show_formatting = shown;
            self.measured.clear();
        }
    }

    /// Whether the formatting marks are showing.
    ///
    /// It reaches the layout rather than only the drawing because of one
    /// property: text marked hidden is drawn when they are showing and not at
    /// all when they are not, and so takes up room only sometimes.
    pub fn set_marks(&mut self, shown: bool) {
        if self.show_marks != shown {
            self.show_marks = shown;
            self.measured.clear();
        }
    }

    /// Sets what colour text is drawn in when the document names none.
    ///
    /// Not a preference but a fact about the paper: on dark paper, automatic
    /// text is light. A colour the document does name is left alone.
    #[must_use]
    pub fn with_automatic_colors(mut self, text: Color, line: Color) -> Self {
        self.set_automatic_colors(text, line);
        self
    }

    /// The same, on an engine that is being kept and used again.
    pub fn set_automatic_colors(&mut self, text: Color, line: Color) {
        if self.automatic_color != text || self.automatic_line != line {
            self.automatic_color = text;
            self.automatic_line = line;
            self.measured.clear();
        }
    }

    /// Shows the document as an outline: every paragraph indented to the depth
    /// of its heading, and anything deeper than `depth` left out.
    ///
    /// `Some(10)` is every level, headings and body text alike; `Some(1)` is
    /// the top headings alone. `None` shows the document as it is.
    #[must_use]
    pub fn with_outline(mut self, depth: Option<u8>) -> Self {
        self.set_outline(depth);
        self
    }

    /// Lays the document out as a browser would show it: Word's Web Layout.
    ///
    /// One sheet as wide as the paper it is given and as long as the text,
    /// in one column, whatever the sections say — a page on the web has no
    /// pages, so no page breaks, no sections on paper of their own, no
    /// headers or footers, no borders round the paper and no numbers down
    /// its margin. The notes come after the text, where a page saved from the
    /// document has them.
    pub fn set_web(&mut self, on: bool) {
        if self.web != on {
            self.web = on;
            self.measured.clear();
        }
    }

    /// Says which way the interface reads, for the lines of it drawn here —
    /// see [`Self::styled_line`] — or `None` for each line to go the way its
    /// own first letter says.
    ///
    /// A message of a window read right to left reads right to left
    /// whatever it starts with: "{0} נשמר" with an English name in it is
    /// the name and then the word, read from the right, and a line that
    /// took its direction from the name would put the word first. Windows
    /// does the same with a window's reading order.
    pub fn set_interface_direction(&mut self, direction: Option<wp_bidi::Direction>) {
        self.interface_direction = direction;
    }

    /// The same, on an engine that is being kept and used again.
    ///
    /// An outline shows no footnotes and no endnotes, as Word's does not:
    /// it is the document's headings and its text, and a note is neither.
    pub fn set_outline(&mut self, depth: Option<u8>) {
        if self.outline != depth {
            self.outline = depth;
            self.measured.clear();
        }
    }

    /// Leaves out of an outline the paragraphs folded away under a heading:
    /// runs of paragraph numbers, a table left out whole when every
    /// paragraph of it is in one. Nothing outside an outline.
    ///
    /// Which they are is the window's to say. A heading is folded in the
    /// view, not in the document, and what is under it is worked out there
    /// from where each paragraph stands — see [`wp_docx::outlining`].
    pub fn set_outline_folded(&mut self, folded: Vec<core::ops::Range<usize>>) {
        self.outline_folded = folded;
    }

    /// Shows only the first line of each paragraph of body text in an
    /// outline, with an ellipsis after it where there was more: Word's Show
    /// First Line Only.
    pub fn set_outline_first_line(&mut self, on: bool) {
        self.outline_first_line = on;
    }

    /// Draws an outline in one plain font, the document's own at its own
    /// size, with none of the formatting its text carries: Word's Show Text
    /// Formatting turned off.
    pub fn set_outline_plain(&mut self, on: bool) {
        if self.outline_plain != on {
            self.outline_plain = on;
            self.measured.clear();
        }
    }

    /// Whether a paragraph of an outline is folded away under a heading.
    fn folded_away(&self, paragraph: usize) -> bool {
        self.outline.is_some()
            && self.outline_folded.iter().any(|folded| folded.contains(&paragraph))
    }

    /// How deep the outline goes, when what is being placed is a paragraph
    /// of it: in an outline, and not in a table's cell.
    fn outlining(&self) -> Option<u8> {
        self.outline.filter(|_| self.in_table == 0)
    }

    /// Whether an outline leaves a table out: while it shows no body text at
    /// all, which a table is, or when every paragraph of the table is folded
    /// away under a heading.
    fn outline_leaves_out(&self, first: usize, count: usize) -> bool {
        match self.outlining() {
            Some(depth) => {
                depth < OUTLINE_ALL
                    || (count > 0 && (first..first + count).all(|at| self.folded_away(at)))
            }
            None => false,
        }
    }

    /// Whether the text of the document is being drawn plain: in an outline
    /// with its formatting turned off.
    fn plain(&self) -> bool {
        self.outline.is_some() && self.outline_plain
    }
    /// An engine that lays out for a device: a screen, or a printer.
    ///
    /// The only difference the device makes is how fine the drawing is. Where
    /// the words go is worked out in points and must come out the same on
    /// both — see `tests/resolution.rs`, which holds it to that.
    #[must_use]
    pub fn for_device(library: &'a FontLibrary, device: crate::Device) -> Self {
        Self::new(library).with_dpi(device.dpi)
    }

    /// Sets the resolution. Larger values render the same page bigger.
    #[must_use]
    pub fn with_dpi(mut self, dpi: f32) -> Self {
        self.set_dpi(dpi);
        self
    }

    /// The same, on an engine that is being kept and used again.
    pub fn set_dpi(&mut self, dpi: f32) {
        let wanted = dpi.clamp(24.0, 1200.0);
        if (self.dpi - wanted).abs() > f32::EPSILON {
            self.dpi = wanted;
            self.measured.clear();
        }
    }

    #[must_use]
    pub fn dpi(&self) -> f32 {
        self.dpi
    }

    /// Pixels per point at this resolution.
    fn pixels_per_point(&self) -> f32 {
        self.dpi / POINTS_PER_INCH
    }

    /// The gap between default tab stops, in pixels.
    fn default_tab_width(&self) -> f32 {
        self.default_tab as f32 / TWIPS_PER_POINT * self.pixels_per_point()
    }

    /// Lays out a single line of plain text, for interface elements.
    ///
    /// The status strip and any other text the program shows go through the same
    /// engine as the document, so there is one way to put text on screen rather
    /// than two that drift apart.
    pub fn simple_line(
        &mut self,
        text: &str,
        x: f32,
        baseline: f32,
        size_points: f32,
        color: Color,
    ) -> Page {
        self.styled_line(text, x, baseline, size_points, color, TextStyle::default())
    }

    /// The same, with formatting — so a toolbar button that applies bold can be
    /// drawn in bold, which says what it does better than any label.
    pub fn styled_line(
        &mut self,
        text: &str,
        x: f32,
        baseline: f32,
        size_points: f32,
        color: Color,
        wanted: TextStyle,
    ) -> Page {
        let mut page = Page::default();
        let size = size_points * self.pixels_per_point();

        let Some(face) = self.library.default_face(wanted.bold, wanted.italic) else {
            return page;
        };
        let style = RunStyle {
            underline: wanted.underline,
            strike: wanted.strike,
            ascent: size,
            descent: size * 0.25,
            line_height: size * 1.25,
            ..RunStyle::plain(face, size, color)
        };

        // A line of interface text reads the way the interface does, where
        // it says; else the way its own first letter does.
        let direction =
            self.interface_direction.unwrap_or_else(|| wp_bidi::Direction::from_text(text));
        let glyphs = self.shape(text, &style, 0);
        let glyphs = self.in_drawing_order(text, glyphs, &style, direction);

        let mut pen = x;
        for glyph in glyphs {
            page.glyphs.push(PositionedGlyph {
                face: glyph.face,
                glyph: glyph.glyph,
                x: pen,
                baseline,
                advance: glyph.advance,
                size,
                stretch: 1.0,
                color,
                effect: style.effect,
                source: TextPosition::default(),
                source_length: glyph.length,
                invisible: false,
                shift_x: 0.0,
                shift_y: 0.0,
            });
            pen += glyph.advance;
        }

        if style.underline && pen > x {
            page.decorations.push(Decoration {
                x,
                y: baseline + size * 0.12,
                width: pen - x,
                height: (size * 0.06).max(1.0),
                color,
            });
        }
        if style.strike && pen > x {
            page.decorations.push(Decoration {
                x,
                y: baseline - size * 0.28,
                width: pen - x,
                height: (size * 0.06).max(1.0),
                color,
            });
        }

        page.width = pen;
        page.height = baseline + style.descent;
        page
    }

    /// One line of text drawn exactly as a run of the document carrying this
    /// formatting would be drawn.
    ///
    /// This is what a preview is for. Word's Font dialog shows a sample, and a
    /// sample is only worth showing if it is the truth: the font that will
    /// actually be found, the small capitals as they will actually be
    /// synthesised, the letters at the width and spacing they will actually
    /// have. Anything drawn a second way would drift from the first, and a
    /// preview that lies is worse than no preview.
    ///
    /// So it goes through the same [`Self::style_for`] and the same shaping as
    /// the document, and differs only in that there is no line to break and no
    /// page to fill.
    #[must_use]
    pub fn sample_line(
        &mut self,
        text: &str,
        properties: &ResolvedRunProperties,
        x: f32,
        baseline: f32,
    ) -> Page {
        let mut page = Page::default();
        let Some(style) = self.style_for(properties) else { return page };

        // The document's own text, so the way its own first letter says.
        let glyphs = self.shape(text, &style, 0);
        let glyphs =
            self.in_drawing_order(text, glyphs, &style, wp_bidi::Direction::from_text(text));

        let mut pen = x;
        let baseline = baseline - style.raise;
        for glyph in glyphs {
            page.glyphs.push(PositionedGlyph {
                face: glyph.face,
                glyph: glyph.glyph,
                x: pen,
                baseline,
                advance: glyph.advance,
                size: glyph.size,
                stretch: style.stretch,
                color: style.color,
                effect: style.effect,
                source: TextPosition::default(),
                source_length: glyph.length,
                invisible: style.hidden,
                shift_x: 0.0,
                shift_y: 0.0,
            });
            pen += glyph.advance;
        }

        let width = pen - x;
        let thickness = (style.size * 0.06).max(1.0);
        if let Some(colour) = style.highlight.filter(|_| width > 0.0) {
            page.decorations.push(Decoration {
                x,
                y: baseline - style.ascent,
                width,
                height: style.ascent + style.descent,
                color: colour,
            });
        }
        if style.underline && width > 0.0 {
            page.decorations.push(Decoration {
                x,
                y: baseline + style.size * 0.12,
                width,
                height: thickness,
                color: style.underline_color.unwrap_or(style.color),
            });
        }
        if (style.strike || style.double_strike) && width > 0.0 {
            let middle = baseline - style.size * 0.28;
            let offsets: &[f32] =
                if style.double_strike { &[-thickness, thickness] } else { &[0.0] };
            for offset in offsets {
                page.decorations.push(Decoration {
                    x,
                    y: middle + offset,
                    width,
                    height: thickness,
                    color: style.color,
                });
            }
        }

        page.width = pen;
        page.height = baseline + style.descent;
        page
    }

    /// Lays a whole document out into pages.
    /// Shows one recipient's values in place of the merge fields' names.
    ///
    /// What previewing a mail merge is: the letter is not changed, only what
    /// its fields are worked out to.
    #[must_use]
    pub fn with_merge_record(mut self, record: Vec<(String, String)>) -> Self {
        self.set_merge_record(record);
        self
    }

    /// The same, on an engine that is being kept and used again.
    pub fn set_merge_record(&mut self, record: Vec<(String, String)>) {
        if self.merge_record != record {
            self.merge_record = record;
            self.measured.clear();
        }
    }

    pub fn layout_document(&mut self, document: &Document) -> Vec<Page> {
        self.layout_document_with(document, PageMetrics::from_document(document))
    }

    /// The same, on paper of a size the caller chooses.
    ///
    /// What a view mode is: draft and web layout are the same document on
    /// different paper, not a different way of laying one out.
    pub fn layout_document_with(&mut self, document: &Document, metrics: PageMetrics) -> Vec<Page> {
        self.layout_document_again(document, metrics, Vec::new())
    }

    /// The same, given back the pages this engine gave last time.
    ///
    /// Those pages, untouched, are what lets a keystroke cost what it costs
    /// on a short document however long this one is: the pages before the
    /// change are kept, the text is laid out again from where it changed,
    /// and the pages after are kept once the pagination lands where it
    /// landed before. See [`again`]. Pages that are not this
    /// engine's last are laid out afresh, so nothing is lost by giving the
    /// wrong ones back — only time.
    pub fn layout_document_again(
        &mut self,
        document: &Document,
        metrics: PageMetrics,
        previous: Vec<Page>,
    ) -> Vec<Page> {
        self.theme = document.theme();
        self.hyphenation = Hyphenation::of(document, self.pixels_per_point());
        // Where the tabs fall back to when a paragraph names no stops of its
        // own, which every document says for itself.
        self.default_tab = document.default_tab_width();
        self.paragraph_styles = document.paragraph_styles();
        self.number_notes(document);
        // A part of the package written since — a style, a picture, a theme
        // — changes what a paragraph resolves to without changing the
        // paragraph, so nothing measured before it can be trusted after.
        let generation = document.package().generation();
        if self.generation != generation {
            self.generation = generation;
            self.measured.clear();
            // What the document says about its fonts, for the ones the
            // machine does not have.
            self.font_table = document
                .font_table()
                .into_iter()
                .map(|entry| (entry.name.to_lowercase(), entry))
                .collect();
        }
        self.placed = 0;

        // Footnotes take room away from the text on the page they belong to,
        // and which page a mark lands on depends on how much room the text
        // has — so the two are worked out together. A `PAGEREF` can only be
        // answered once there are pages to count, so it is the same again.
        // The body is laid out, the room and the pages are read off it, and
        // if either differs from what the body was laid out with it is laid
        // out again with the new answers: twice more at most. A page number
        // that moves the text that moves the page number is a document
        // nobody can typeset, and Word gives up at the same point.
        //
        // What it is laid out with the first time is what the last layout
        // ended with, when there are pages back from it: on a keystroke the
        // answers are almost always still right, and then one pass is all
        // it takes. Without pages back it starts, as a new engine does, from
        // no room and no pages.
        // An outline shows no notes, as Word's does not: see
        // [`Self::set_outline`].
        let footnotes = if self.outline.is_some() {
            Vec::new()
        } else {
            document.notes(wp_docx::notes::Kind::Footnote)
        };
        let body = document.body();
        let metrics = if self.web { PageMetrics { columns: 1, ..metrics } } else { metrics };
        // The pages of a section written down the page were turned before
        // they were given out, and the engine places text into the box they
        // were laid out in: so back into the box they go, until the end.
        let mut previous = previous;
        for page in &mut previous {
            page.unturn();
        }
        // Counted over the body that is about to be laid out, so that it is
        // built once.
        self.number_sequences(&body);
        if previous.is_empty() {
            self.reserved = Vec::new();
        }
        // From here to the end of the body, the paragraphs are the document's
        // own and their measurements are worth keeping. A header or the text
        // inside a shape is counted from zero as well, so it must not be
        // mistaken for the body — see [`Measured`].
        self.keeping = true;
        let mut pages = self.layout_body_from(&body, document, metrics, previous);
        if self.web {
            // The notes are not the document's own paragraphs, whose
            // measurements are kept.
            self.keeping = false;
            self.finish_web_page(&mut pages, document, metrics);
            Self::rejoin_connectors(&mut pages);
            self.fill_shapes(&mut pages, document);
            mark_videos(&mut pages);
            return pages;
        }
        for _ in 0..2 {
            let reserved = if footnotes.is_empty() {
                Vec::new()
            } else {
                self.measure_footnotes(&pages, &footnotes, document, metrics)
            };
            let bookmarks = Self::locate_bookmarks(&pages, document);
            if reserved == self.reserved && bookmarks == self.bookmark_pages {
                break;
            }
            self.reserved = reserved;
            self.bookmark_pages = bookmarks;
            let previous = core::mem::take(&mut pages);
            pages = self.layout_body_from(&body, document, metrics, previous);
        }
        if !footnotes.is_empty() {
            self.place_footnotes(&mut pages, &footnotes, document, metrics);
        }

        // The body and its footnotes are on the pages; a section written
        // down the page has them turned onto the paper now, and everything
        // after this goes onto the paper the ordinary way up.
        self.turn_pages(&mut pages, document, metrics);

        // The numbers down the margin, where a section asks for them.
        // Everything after this is a header, a footer or the text inside a
        // shape, and those are counted from zero as the body is.
        self.keeping = false;
        self.number_lines(&mut pages, document);

        // The border round the paper, which is about the sheet rather than
        // about anything on it.
        self.draw_page_borders(&mut pages, document);

        // The header and the footer go on afterwards, once there are pages to
        // put them on and a total for a page number to count towards.
        self.place_all_furniture(&mut pages, document, metrics);

        // And the text inside every shape, which is a document of its own and
        // so is laid out once the outer one has stopped moving.
        Self::rejoin_connectors(&mut pages);
        self.fill_shapes(&mut pages, document);
        mark_videos(&mut pages);
        pages
    }

    /// Turns the pages of every section written down the page onto their
    /// paper. See [`Page::turn`].
    fn turn_pages(&self, pages: &mut [Page], document: &Document, metrics: PageMetrics) {
        for (index, page) in pages.iter_mut().enumerate() {
            let paper = self.paper_of_page(index, document, metrics);
            if paper.is_turned() {
                let (width, height) = (page.height, page.width);
                page.turn(paper.frame_on(width, height));
            }
        }
    }

    /// The paper one of the document's pages is printed on: its section's,
    /// or the document's where the page belongs to no section it knows.
    fn paper_of_page(&self, page: usize, document: &Document, metrics: PageMetrics) -> PageMetrics {
        let sections = document.sections();
        if sections.len() <= 1 || self.web {
            return metrics;
        }
        self.page_sections
            .get(page)
            .and_then(|section| sections.get(*section))
            .map_or(metrics, |section| PageMetrics::from_setup(&section.setup))
    }

    /// Lays a body out into pages.
    ///
    /// # Why one body can need several kinds of paper
    ///
    /// Because a document is made of sections, and each of them says what it is
    /// printed on: a landscape table in the middle of a portrait report is a
    /// section of its own. The blocks are laid out section by section, each on
    /// its own paper, and a section that begins a new page begins one — see
    /// [`wp_docx::sections`].
    ///
    /// The `metrics` given here are what a section that says nothing falls back
    /// to, and what a body with no sections of its own — a header, a footnote,
    /// the text inside a shape — is laid out on.
    pub fn layout_body(
        &mut self,
        body: &Body,
        document: &Document,
        metrics: PageMetrics,
    ) -> Vec<Page> {
        self.layout_body_from(body, document, metrics, Vec::new())
    }

    /// The same, given back the pages of the last pass over the document's
    /// body, so that it is taken up from where the body changed and left off
    /// where the pagination lands as it did. See [`again`].
    ///
    /// Only the document's own body is remembered from one pass to the
    /// next; any other body is laid out whole, and the pages given back are
    /// let go.
    fn layout_body_from(
        &mut self,
        body: &Body,
        document: &Document,
        metrics: PageMetrics,
        previous: Vec<Page>,
    ) -> Vec<Page> {
        // Every pass starts with no floating drawings: they are found again as
        // the text is placed, and keeping the last pass's would narrow the
        // lines twice over.
        self.floats.clear();
        // And with no heading passed yet, so an outline indents the same way
        // every time the same document is laid out.
        self.outline_heading = 0;
        // Lists count from the beginning every time the document is laid out.
        self.counters = ListCounters::new();

        let (stretches, of_the_document) = self.sections_for(body, document, metrics);
        // Which section each page turns out to belong to, filled in below.
        let mut belongs: Vec<usize> = Vec::new();
        let mut pages: Vec<Page> = Vec::new();
        let mut index = 0usize;
        let mut y = 0.0f32;
        let mut column = 0usize;

        // Where the last pass over this body left off, and where this one
        // takes up from.
        let mut trail = self.keeping.then(|| again::Trail::on(metrics));
        let mut resumed = None;
        if let Some(trail) = &mut trail {
            match self.take_up(body, &stretches, previous, trail) {
                again::Taking::Afresh => {}
                again::Taking::Unchanged(pages) => return pages,
                again::Taking::From(mut from) => {
                    pages = core::mem::take(&mut from.pages);
                    belongs = core::mem::take(&mut from.belongs);
                    y = from.y;
                    column = from.column;
                    index = from.index;
                    resumed = Some(from);
                }
            }
        }

        for (which, stretch) in stretches.iter().enumerate() {
            let Stretch { blocks: range, metrics: paper, start, section } = stretch;
            // A section written down the page is laid out into the paper's
            // box, which is the paper with its sides swapped, and turned
            // onto the paper once everything is on it. See
            // [`PageMetrics::boxed`].
            let metrics = paper.boxed();
            self.vertical = paper.direction.is_vertical_writing();
            let scale = self.pixels_per_point();
            let page_width = metrics.width * scale;
            let page_height = metrics.height * scale;
            let left = metrics.margin_left * scale;
            let top = metrics.margin_top * scale;
            let bottom_limit = page_height - metrics.margin_bottom * scale;
            // The area is one column wide; the flow moves along to the next
            // column when it runs out of page, and only then to a new page.
            let text_width = metrics.column_width() * scale;
            let column_gap = metrics.column_gap * scale;
            // A change of direction is a change of paper: the box the text
            // goes into is a different shape, so it cannot share a page with
            // what came before.
            let turned_differently = which > 0
                && stretches.get(which - 1).is_some_and(|before| {
                    before.metrics.direction.is_turned() != paper.direction.is_turned()
                        || (paper.direction.is_turned()
                            && before.metrics.direction != paper.direction)
                });

            let mut from = range.start;
            match &resumed {
                // The stretches before the one taken up in are done, and that
                // one is entered at the block taken up at, on the page as it
                // stood.
                Some(taken) if which < taken.stretch => continue,
                Some(taken) if which == taken.stretch => from = taken.block,
                _ => {
                    // A section on paper of its own has to begin a page of its
                    // own, because a page is one size all the way down.
                    let first = pages.is_empty();
                    if first || start.on_a_new_page() || turned_differently {
                        pages.push(Page {
                            width: page_width,
                            height: page_height,
                            ..Page::default()
                        });
                        // One that has to begin on an even or an odd page
                        // takes a blank page in front of it when the count
                        // comes out wrong, which is how a chapter always opens
                        // on the same side of the paper.
                        let wrong = match start {
                            Start::EvenPage => pages.len() % 2 == 1,
                            Start::OddPage => pages.len() % 2 == 0,
                            _ => false,
                        };
                        if wrong && !first {
                            pages.push(Page {
                                width: page_width,
                                height: page_height,
                                ..Page::default()
                            });
                        }
                        y = top;
                        column = 0;
                    }
                }
            }

            let Some(blocks) = body.blocks.get(from..range.end) else { continue };
            if let Some(trail) = &mut trail {
                trail.stretch = which;
                trail.offset = from;
            }
            self.place_blocks(
                blocks,
                &mut index,
                document,
                &mut pages,
                &mut y,
                &mut column,
                Placement {
                    left,
                    top,
                    bottom_limit,
                    text_width,
                    page_width,
                    page_height,
                    columns: metrics.columns.max(1),
                    column_gap,
                    keeping: true,
                },
                trail.as_mut(),
            );
            // Everything pushed while this section was placed is that section's.
            belongs.resize(pages.len(), *section);

            // Landed where the last pass landed: the rest is the last pass's.
            if let (Some(trail), Some(taken)) = (&trail, &mut resumed) {
                if trail.settled.is_some() {
                    self.settle(trail, taken, &mut pages, &mut belongs);
                    break;
                }
            }
        }

        self.vertical = false;
        if pages.is_empty() {
            let scale = self.pixels_per_point();
            let metrics = metrics.boxed();
            pages.push(Page {
                width: metrics.width * scale,
                height: metrics.height * scale,
                ..Page::default()
            });
            belongs.resize(1, 0);
        }
        if of_the_document {
            self.page_sections.clone_from(&belongs);
        }
        if let Some(trail) = trail {
            self.placed += trail.placed;
            self.remember(trail, resumed, &mut pages, belongs, body, &stretches);
        }
        pages
    }

    /// The stretches of a body that are laid out on one kind of paper each.
    ///
    /// A body that is not the document — a header, a footnote, the inside of a
    /// shape — has no sections of its own and is one stretch on the paper it
    /// was given.
    fn sections_for(
        &self,
        body: &Body,
        document: &Document,
        metrics: PageMetrics,
    ) -> (Vec<Stretch>, bool) {
        let sections = document.sections();
        let is_the_document = sections
            .last()
            .is_some_and(|last| last.end_block == body.blocks.len() && sections.len() > 1);
        // A page on the web is one stretch of text, whatever its sections.
        if !is_the_document || self.web {
            let whole = Stretch {
                blocks: 0..body.blocks.len(),
                metrics,
                start: Start::NextPage,
                section: 0,
            };
            return (vec![whole], false);
        }

        // Numbered before the empty ones are dropped, because the number is
        // what says whose header is printed on the pages.
        let stretches = sections
            .iter()
            .enumerate()
            .filter(|(_, section)| section.end_block > section.first_block)
            .map(|(index, section)| Stretch {
                blocks: section.first_block..section.end_block.min(body.blocks.len()),
                metrics: PageMetrics::from_setup(&section.setup),
                start: section.setup.start,
                section: index,
            })
            .collect();
        (stretches, true)
    }

    /// Lays out a run of blocks, keeping the paragraph numbering in step.
    ///
    /// The index counts paragraphs in reading order, table cells included,
    /// because that is what a caret position means. Walking the blocks here
    /// rather than flattening them first is what lets a table be laid out as a
    /// grid instead of as a list of its paragraphs.
    ///
    /// Given a trail, it leaves a checkpoint at every block the placement
    /// could be taken up from — one outside any run of paragraphs kept
    /// together, and not one a run was moved back to — and stops the moment
    /// a checkpoint matches the last pass's. See [`again`].
    #[allow(clippy::too_many_arguments)]
    fn place_blocks(
        &mut self,
        blocks: &[Block],
        index: &mut usize,
        document: &Document,
        pages: &mut Vec<Page>,
        y: &mut f32,
        column: &mut usize,
        area: Placement,
        mut trail: Option<&mut again::Trail>,
    ) {
        // The run of paragraphs that have asked to stay with the one after
        // them, and where that run began: a heading followed by two more
        // headings goes over to the next page as a whole.
        let mut kept: Option<(usize, usize, Mark)> = None;
        // Which block has already been moved for this rule, so a paragraph that
        // cannot be kept with the next one however far it is moved is laid out
        // rather than moved for ever.
        let mut moved_at: Option<usize> = None;

        let mut position = 0usize;
        while position < blocks.len() {
            if let Some(trail) = trail.as_deref_mut() {
                if kept.is_none() && moved_at.is_none_or(|moved| position > moved) {
                    let point = again::Checkpoint {
                        block: trail.offset + position,
                        index: *index,
                        stretch: trail.stretch,
                        page: pages.len().saturating_sub(1),
                        held: pages.last().map(again::Extent::of).unwrap_or_default(),
                        y: *y,
                        column: *column,
                        counters: self.counters.clone(),
                        floats: self.floats.len(),
                        outline_heading: self.outline_heading,
                    };
                    let settled = self.settles(trail, &point);
                    trail.checkpoints.push(point);
                    if settled.is_some() {
                        trail.settled = settled;
                        return;
                    }
                }
            }

            if let Some(trail) = trail.as_deref_mut() {
                trail.placed += 1;
            }
            let before = Mark::here(pages, *y, *column, 0, 0);
            let counted = *index;

            match &blocks[position] {
                Block::Paragraph(paragraph) => {
                    let resolved = document.resolve_paragraph(paragraph);
                    self.place_paragraph(*index, paragraph, document, pages, y, column, area);
                    *index += 1;

                    // One an outline left out put nothing on the page, which
                    // is not the same as beginning a page: taken for one, the
                    // heading kept with it was moved to a page of its own —
                    // on the outline's one sheet, a quarter of the largest
                    // float down.
                    if core::mem::take(&mut self.left_out) {
                        position += 1;
                        continue;
                    }

                    // Nothing of this paragraph landed on the page it began
                    // on, so it started a page — and whatever asked to stay
                    // with it has been left behind.
                    let started_a_page =
                        pages.get(before.page).is_some_and(|page| page.lines.len() == before.lines);
                    let asked_for = resolved.page_break_before;
                    let already = moved_at == Some(position);

                    if area.keeping && started_a_page && !asked_for && !already {
                        if let Some((block, at, mark)) =
                            kept.filter(|(_, _, mark)| mark.page == before.page)
                        {
                            moved_at = Some(position);
                            mark.take_back(pages);
                            *y = mark.y;
                            *column = mark.column;
                            self.start_page(pages, y, column, area);
                            *index = at;
                            position = block;
                            kept = None;
                            continue;
                        }
                    }

                    // A paragraph that asks to stay with the next one joins the
                    // run, or begins it; one that does not ends it.
                    kept = resolved.keep_next.then(|| kept.unwrap_or((position, counted, before)));
                }
                Block::Table(table) => {
                    let count = blocks[position].paragraph_count();
                    if self.outline_leaves_out(*index, count) {
                        *index += count;
                    } else {
                        // An outline shows a table where body text goes: one
                        // step in from the heading above it.
                        let area = match self.outlining() {
                            Some(_) => {
                                let step = self.outline_step(None) * self.pixels_per_point();
                                Placement {
                                    left: area.left + step,
                                    text_width: (area.text_width - step).max(1.0),
                                    ..area
                                }
                            }
                            None => area,
                        };
                        self.in_table += 1;
                        self.place_table(table, index, document, pages, y, column, area);
                        self.in_table -= 1;
                    }
                    kept = None;
                }
            }
            position += 1;
        }
    }

    /// Where a paragraph may be drawn, in pixels.
    ///
    /// The index is the paragraph's place in reading order, which is what a
    /// caret and a click are expressed in.
    #[allow(clippy::too_many_arguments)]
    fn place_paragraph(
        &mut self,
        index: usize,
        paragraph: &Paragraph,
        document: &Document,
        pages: &mut Vec<Page>,
        y: &mut f32,
        column: &mut usize,
        area: Placement,
    ) {
        let resolved = document.resolve_paragraph(paragraph);
        let scale = self.pixels_per_point();
        // Whether the words of this paragraph may be broken by the patterns:
        // the document hyphenates, and the paragraph does not say not to.
        self.hyphenating = self.hyphenation.automatic && !resolved.no_hyphenation;

        // Shown as an outline, a paragraph sits at the depth of its heading and
        // one deeper than the level being shown is not drawn at all. Body text
        // counts as one step past the heading above it, which is where a reader
        // expects to find it.
        let mut outline_indent = 0.0;
        // Whether this is body text in an outline, which Show First Line Only
        // cuts down to its first line.
        let mut outline_body = false;
        if let Some(depth) = self.outlining() {
            // Level nine is Word's way of writing "body text" down, and is
            // no heading.
            let heading = resolved.outline_level.filter(|level| *level < 9).map(|level| level + 1);
            if let Some(level) = heading {
                self.outline_heading = level;
            }
            // A heading is shown down to its own level; body text only when
            // every level is being shown, which is what Word's list calls All
            // Levels and what nine headings plus one comes to. And nothing a
            // heading above it has been folded over.
            if heading.unwrap_or(OUTLINE_ALL) > depth || self.folded_away(index) {
                self.left_out = true;
                return;
            }
            outline_indent = self.outline_step(heading) * scale;
            outline_body = heading.is_none();
        }

        // The mark a list paragraph carries, and the indents its level asks
        // for. Counting has to happen here even when the mark is not drawn, so
        // that a list continues across a page.
        let (mark, level_indent_start, level_indent_hanging) = self.list_mark(&resolved, document);

        // Spacing is in twentieths of a point, like almost everything else.
        //
        // Word's "Don't add space between paragraphs of the same style" drops
        // it where one such paragraph meets another of the same style. It is
        // what makes a bulleted list read as a list instead of as a column of
        // paragraphs with gaps between them, and it is asked of the neighbours
        // rather than of this paragraph alone.
        // Asked of the styles gathered once for the whole document, because
        // asking the document for each neighbour walks the whole tree for
        // each paragraph, and that made a keystroke on a thousand pages
        // cost seconds.
        let same_as = |other: usize| {
            resolved.contextual_spacing
                && self.paragraph_styles.get(other) == self.paragraph_styles.get(index)
                && other < self.paragraph_styles.len()
        };
        let after_its_own_kind = index > 0 && same_as(index - 1);
        let before_its_own_kind = index + 1 < self.paragraph_styles.len() && same_as(index + 1);

        let space_before = if after_its_own_kind {
            0.0
        } else {
            resolved.space_before as f32 / TWIPS_PER_POINT * scale
        };
        let space_after = if before_its_own_kind {
            0.0
        } else {
            resolved.space_after as f32 / TWIPS_PER_POINT * scale
        };
        // A list level's indents apply only where nothing else set one: a
        // paragraph that says where it sits has already been believed.
        //
        // An outline shows none of a paragraph's own indents, as Word's does
        // not: where a paragraph sits there is its level's place and nothing
        // else. A list keeps its mark's room.
        let authored_indent = self.outlining().is_none()
            && (resolved.indent_start != 0 || resolved.indent_first_line != 0);
        let (indent_twips, first_twips) = if authored_indent {
            (resolved.indent_start, resolved.indent_first_line)
        } else {
            (level_indent_start, -level_indent_hanging)
        };
        let indent_start = indent_twips as f32 / TWIPS_PER_POINT * scale + outline_indent;
        let indent_end = if self.outlining().is_some() {
            0.0
        } else {
            resolved.indent_end as f32 / TWIPS_PER_POINT * scale
        };
        // Nor how it is aligned: every paragraph of an outline begins at its
        // level's place, which is what makes the levels readable. The start
        // is the paragraph's own, so text written right to left still runs
        // from the right.
        let alignment =
            if self.outlining().is_some() { Alignment::Start } else { resolved.alignment };
        let indent_first = first_twips as f32 / TWIPS_PER_POINT * scale;

        if resolved.page_break_before
            && !self.web
            && !pages.last().is_some_and(|page| page.glyphs.is_empty())
        {
            self.start_page(pages, y, column, area);
        }

        *y += space_before;

        // Measured last time, if this paragraph is one whose measurements are
        // kept and it has not changed since. That is what makes typing into a
        // long document cost what typing into a short one costs: the eleven
        // thousand paragraphs nobody touched are not measured again.
        let kept = self.keeping && is_plain(paragraph);
        let ready = kept
            .then(|| self.measured.get(index))
            .flatten()
            .and_then(Option::as_ref)
            .filter(|measured| {
                measured.paragraph == *paragraph && measured.vertical == self.vertical
            })
            .cloned();

        let (mut items, styles, mut item_levels) = match ready {
            Some(measured) => (measured.items, measured.styles, measured.levels),
            None => {
                let mut styles = Vec::new();
                let mut text = String::new();
                let mut items =
                    self.build_items(paragraph, document, &mut styles, index, &mut text);

                // Which direction each piece of the paragraph is drawn in.
                //
                // Not a property of the run it came from: a Hebrew phrase inside
                // an English sentence, or a price inside a Hebrew one, is a run
                // of its own whatever the document says about the paragraph, and
                // finding those runs needs the whole paragraph at once. See
                // [`wp_bidi`].
                let direction = if resolved.right_to_left {
                    wp_bidi::Direction::RightToLeft
                } else {
                    wp_bidi::Direction::LeftToRight
                };
                let bytes = wp_bidi::levels(&text, direction);
                let base = u8::from(resolved.right_to_left);
                let item_levels: Vec<u8> = items
                    .iter()
                    .map(|item| {
                        let level = bytes.get(item.start_offset).copied().unwrap_or(base);
                        // A run the document marks right-to-left reads that way
                        // whatever its characters are, which is what `w:rtl`
                        // means.
                        if styles.get(item.style).is_some_and(|style| style.right_to_left) {
                            level | 1
                        } else {
                            level
                        }
                    })
                    .collect();
                for (item, level) in items.iter_mut().zip(&item_levels) {
                    // A piece that reads right to left is drawn from its
                    // right-hand end, so its glyphs go down in the other order.
                    if level % 2 == 1 {
                        item.glyphs.reverse();
                    }
                }
                self.mirror_glyphs(&mut items, &item_levels, &styles);

                if kept {
                    if self.measured.len() <= index {
                        self.measured.resize(index + 1, None);
                    }
                    self.measured[index] = Some(Measured {
                        paragraph: paragraph.clone(),
                        items: items.clone(),
                        styles: styles.clone(),
                        levels: item_levels.clone(),
                        vertical: self.vertical,
                    });
                }
                (items, styles, item_levels)
            }
        };

        if items.is_empty() {
            // An empty paragraph still takes up a line's worth of height, and
            // still needs a line recorded: a caret has to be able to sit in it.
            //
            // Its line spacing counts, exactly as it does for a line with
            // letters on it. Without that, an empty paragraph was a shade
            // shorter than a full one — and a row of a table grew by a pixel
            // and a half the moment anything was typed into it, which is the
            // sort of jump that makes a table look broken.
            let natural = self.empty_line_height(paragraph, document);
            let height = line_height(natural, &resolved, scale);
            if *y + height > self.limit(pages.len().saturating_sub(1), area)
                && !pages.last().is_some_and(|page| page.glyphs.is_empty())
            {
                self.start_page(pages, y, column, area);
            }
            let ascent = height * 0.8;
            let column_left = area.in_column(*column).left;
            if let Some(page) = pages.last_mut() {
                page.lines.push(PageLine {
                    baseline: *y + ascent,
                    ascent,
                    descent: height - ascent,
                    left: column_left + indent_start,
                    right: column_left + indent_start,
                    glyphs: page.glyphs.len()..page.glyphs.len(),
                    paragraph: index,
                    start_offset: 0,
                    end_offset: 0,
                    frame: Frame::default(),
                });
            }
            *y += height;
            *y += space_after;
            return;
        }

        let full_width = (area.text_width - indent_start - indent_end).max(1.0);

        // Where the paragraph sits on each page it touches, so its shading and
        // its borders can be drawn round the band it occupies. A paragraph that
        // crosses a page break has one band per page, which is what Word draws.
        let mut bands: Vec<(usize, f32, f32, f32)> = Vec::new();

        // Lines are broken one at a time rather than all at once, because how
        // wide a line may be depends on where it lands: a drawing floating on
        // the page narrows the lines beside it and no others. Breaking the
        // whole paragraph first would mean deciding the widths before knowing
        // the heights that put the lines where they are.
        let mut cursor = 0usize;
        let mut number = 0usize;
        // What the page held before this paragraph, and before the line last
        // put on it: where the keeping rules put things back to.
        let began = Mark::here(pages, *y, *column, 0, 0);
        let mut before_line = began;
        // Each rule moves things at most once, so a paragraph too tall for a
        // page still gets laid out rather than moved for ever.
        let mut kept_together = false;
        let mut widow_moved = false;
        // How many lines in a row have ended with a hyphen, for the limit on
        // them.
        let mut hyphens_in_a_row = 0u32;
        while cursor < items.len() || number == 0 {
            let extra_first = if number == 0 { indent_first.max(0.0) } else { 0.0 };
            let rules = Breaking {
                allowed: self.hyphenation.limit == 0 || hyphens_in_a_row < self.hyphenation.limit,
                zone: self.hyphenation.zone,
            };
            let page_index = pages.len().saturating_sub(1);
            let here = area.in_column(*column);

            // A first guess at the room, made against the top of the line
            // before its height is known.
            let (mut line_left, mut line_width) = self.usable_span(
                page_index,
                here.left + indent_start + extra_first,
                (full_width - extra_first).max(1.0),
                *y,
                *y + 1.0,
            );
            let mut line = break_line(&mut items, &mut item_levels, cursor, line_width, rules);
            let (mut ascent, mut descent, mut natural_height) =
                line_metrics(&line, &items, &styles);
            let mut height = line_height(natural_height, &resolved, scale);

            // Now that the height is known the room can be asked for again,
            // against the band the line really covers. Once: a line that is
            // narrowed twice over is one nobody could follow.
            let (settled_left, settled_width) = self.usable_span(
                page_index,
                here.left + indent_start + extra_first,
                (full_width - extra_first).max(1.0),
                *y,
                *y + height,
            );
            if settled_width < line_width {
                line_left = settled_left;
                line_width = settled_width;
                line = break_line(&mut items, &mut item_levels, cursor, line_width, rules);
                let measured = line_metrics(&line, &items, &styles);
                ascent = measured.0;
                descent = measured.1;
                natural_height = measured.2;
                height = line_height(natural_height, &resolved, scale);
            }

            if *y + height > self.limit(page_index, area)
                && !pages.last().is_some_and(|p| p.glyphs.is_empty())
            {
                // The rules about where a paragraph may be broken, which are
                // the reason a heading is never left alone at the foot of a
                // page and a paragraph never leaves one line behind.
                //
                // A paragraph that must not be split at all, and a break that
                // would leave a single first line behind, both move the whole
                // paragraph to the next page. It is put back off the page and
                // laid out again there, once — a paragraph taller than a page
                // has to be broken somewhere.
                // Moving it is only worth anything if there is something above
                // it to move away from: a paragraph that begins a page and
                // still does not fit has nowhere better to go.
                let alone_on_the_page = began.lines == 0;
                let orphan = resolved.widow_control && number == 1;
                let must_not_break = resolved.keep_lines && number > 0;
                if (orphan || must_not_break) && !kept_together && !alone_on_the_page {
                    kept_together = true;
                    began.take_back(pages);
                    *y = began.y;
                    *column = began.column;
                    self.start_page(pages, y, column, area);
                    bands.clear();
                    before_line = Mark::here(pages, *y, *column, 0, 0);
                    cursor = 0;
                    number = 0;
                    continue;
                }

                // And a break that would leave the last line alone at the top
                // of the next page takes the line before it along, so that two
                // go over rather than one. Word's rule, and the reason it is
                // called widow control.
                if resolved.widow_control
                    && !widow_moved
                    && number >= 2
                    && is_one_line_left(&items, cursor, line_width, rules)
                {
                    widow_moved = true;
                    before_line.take_back(pages);
                    *y = before_line.y;
                    *column = before_line.column;
                    bands.retain(|(page, ..)| *page <= before_line.page);
                    self.start_page(pages, y, column, area);
                    cursor = before_line.cursor;
                    number = before_line.number;
                    continue;
                }

                self.start_page(pages, y, column, area);
                // A new page has its own floats and its own room, so the line
                // is broken again against what is actually there.
                let page_index = pages.len().saturating_sub(1);
                let here = area.in_column(*column);
                let (fresh_left, fresh_width) = self.usable_span(
                    page_index,
                    here.left + indent_start + extra_first,
                    (full_width - extra_first).max(1.0),
                    *y,
                    *y + height,
                );
                line_left = fresh_left;
                line_width = fresh_width;
                line = break_line(&mut items, &mut item_levels, cursor, line_width, rules);
                let measured = line_metrics(&line, &items, &styles);
                ascent = measured.0;
                descent = measured.1;
                height = line_height(measured.2, &resolved, scale);
            }

            let page_index = pages.len().saturating_sub(1);
            let here = area.in_column(*column);
            match bands.last_mut() {
                Some((index, left, _, bottom)) if *index == page_index && *left == here.left => {
                    *bottom = *y + height;
                }
                _ => bands.push((page_index, here.left, *y, *y + height)),
            }

            // Remembered before the line goes down, so that a widow can put
            // this one on the next page along with the one after it.
            before_line = Mark::here(pages, *y, *column, cursor, number);

            // The pieces this line is laid out in: one for each stretch of
            // room beside the drawings at this height, filled left to right.
            // Nearly always one — a drawing in the middle of the text is what
            // makes it two, which is what Word's `bothSides` wrapping is.
            //
            // The stretches are asked for against the height settled above; a
            // piece whose own text is taller than the first piece's would like
            // a taller band than was asked for, which is rare enough to leave.
            let spans = self.free_spans(
                page_index,
                here.left + indent_start + extra_first,
                (full_width - extra_first).max(1.0),
                *y,
                *y + height,
            );
            let mut pieces: Vec<(Line, f32, f32)> = Vec::new();
            let mut at = cursor;
            for (which, (span_left, span_width)) in spans.iter().copied().enumerate() {
                if at >= items.len() {
                    break;
                }
                let last_span = which + 1 == spans.len();
                let piece = if last_span {
                    // Nowhere to pass a word on to, so this one takes it
                    // whatever its width — and cuts it if it is wider than the
                    // stretch itself.
                    break_line(&mut items, &mut item_levels, at, span_width, rules)
                } else {
                    break_next_line_fitting(&items, at, span_width, true, rules)
                };
                if piece.items.end <= at {
                    // Nothing fits in this stretch. The word goes in the next.
                    continue;
                }
                at = piece.items.end;
                pieces.push((piece, span_left, span_width));
            }
            if pieces.is_empty() {
                pieces.push((line.clone(), line_left, line_width));
                at = line.items.end;
            }

            // The line is as tall as the tallest of its pieces.
            for (piece, ..) in &pieces {
                let (piece_ascent, piece_descent, piece_natural) =
                    line_metrics(piece, &items, &styles);
                ascent = ascent.max(piece_ascent);
                descent = descent.max(piece_descent);
                natural_height = natural_height.max(piece_natural);
            }
            height = line_height(natural_height, &resolved, scale).max(height);

            let baseline = *y + ascent;
            let next = at;
            let is_last = next >= items.len();

            // The mark goes on the first line only, and before the glyphs of
            // that line are recorded — so it lies outside the line's range and
            // a click or a selection never lands on it. It is not text.
            if number == 0 {
                if let Some(mark) = &mark {
                    self.place_list_mark(
                        mark,
                        paragraph,
                        document,
                        pages.last_mut().expect("there is always a page"),
                        pieces.first().map_or(line_left, |(_, left, _)| *left),
                        indent_first,
                        baseline,
                    );
                }
            }

            let count = pieces.len();
            for (which, (piece, piece_left, piece_width)) in pieces.iter().enumerate() {
                self.place_line(
                    piece,
                    &items,
                    &styles,
                    pages.last_mut().expect("there is always a page"),
                    LinePlacement {
                        left: *piece_left,
                        origin: here.left,
                        width: *piece_width,
                        baseline,
                        ascent,
                        descent,
                        alignment,
                        paragraph_rtl: resolved.right_to_left,
                        // Only the piece that ends the paragraph is a last
                        // line: a justified line broken round a drawing is
                        // stretched in every piece but that one.
                        is_last_line: is_last && which + 1 == count,
                        paragraph: index,
                        page: page_index,
                        area,
                    },
                    Composition { stops: &resolved.tab_stops, levels: &item_levels },
                );
            }

            *y += height;
            // A drawing wrapped above and below pushes the text past its foot,
            // which is the whole of what that wrapping means.
            *y = self.past_top_and_bottom_floats(page_index, *y);

            // A line that ends with a page break sends what follows to the top
            // of a new page, and one that ends with a column break to the top
            // of the next column. Only in the document's own flow: a cell, a
            // header or a note has no page to break, and a page on the web
            // has none at all.
            let broken = next
                .checked_sub(1)
                .and_then(|last| items.get(last))
                .and_then(|item| item.hard_break.filter(|kind| *kind != BreakKind::Line));
            if let Some(kind) = broken.filter(|_| !self.web && area.bottom_limit.is_finite()) {
                if kind == BreakKind::Page {
                    *column = area.columns.saturating_sub(1);
                }
                self.start_page(pages, y, column, area);
            }

            // Whether this line ended with a hyphen: it went on past its
            // last word, and that word carried one.
            let ended_with_hyphen = next < items.len()
                && items[cursor..next]
                    .iter()
                    .rev()
                    .find(|item| !item.is_space)
                    .is_some_and(|item| item.hyphen > 0.0);
            hyphens_in_a_row = if ended_with_hyphen { hyphens_in_a_row + 1 } else { 0 };

            number += 1;
            if next <= cursor && number > 0 && cursor < items.len() {
                // Nothing was consumed, which would loop forever. One item goes
                // on the line whatever its width, which is what the breaker
                // does for an item wider than the page.
                cursor += 1;
            } else {
                cursor = next;
            }

            // Word's Show First Line Only: body text in an outline is its
            // first line and an ellipsis, where there was more to it.
            if outline_body && self.outline_first_line && cursor < items.len() {
                let style =
                    next.checked_sub(1).and_then(|last| items.get(last)).map(|item| item.style);
                if let (Some(style), Some(page)) =
                    (style.and_then(|style| styles.get(style)), pages.get_mut(page_index))
                {
                    let style = style.clone();
                    let end = page.lines.last().map_or(0.0, |line| line.right);
                    self.place_ellipsis(&style, page, end, baseline);
                }
                break;
            }
        }

        // The shading and the borders go on last, because the band they cover is
        // only known once every line has been placed. They are decorations, and
        // decorations are drawn before the text, so the shading lands behind it.
        if resolved.shading.is_some() || !resolved.borders.is_empty() {
            for (page_index, column_left, top, bottom) in bands {
                let left = column_left + indent_start;
                let right = column_left + area.text_width - indent_end;
                let Some(page) = pages.get_mut(page_index) else { continue };
                Self::decorate_paragraph(
                    page,
                    &resolved,
                    left,
                    right,
                    top,
                    bottom,
                    self.automatic_line,
                    scale,
                );
            }
        }

        *y += space_after;
    }

    /// Draws the border round the pages of every section that asks for one.
    ///
    /// # Why it is not drawn with the paragraphs
    ///
    /// Because it belongs to the sheet. It is the same on every page of the
    /// section whatever is on them, it is there on a page with no text at all,
    /// and it is measured from the edge of the paper rather than from anything
    /// that was laid out. See [`wp_docx::pageborders`].
    fn draw_page_borders(&mut self, pages: &mut [Page], document: &Document) {
        let sections = document.sections();
        let borders: Vec<wp_docx::pageborders::PageBorders> =
            (0..sections.len()).map(|section| document.page_borders_of(section)).collect();
        if borders.iter().all(wp_docx::pageborders::PageBorders::is_empty) {
            return;
        }

        let scale = self.pixels_per_point();
        let automatic = self.automatic_line;
        // Which page of its own section each page is, because a border can be
        // asked for on the first page of a section or on all but the first.
        let mut seen: Vec<usize> = vec![0; sections.len()];

        for (index, page) in pages.iter_mut().enumerate() {
            let section = self.page_sections.get(index).copied().unwrap_or(0);
            let within = seen.get(section).copied().unwrap_or(0);
            if let Some(count) = seen.get_mut(section) {
                *count += 1;
            }

            let Some(border) = borders.get(section) else { continue };
            if border.is_empty() || !border.display.covers(within) {
                continue;
            }

            // The distance is in whole points from the edge of the paper, or
            // from the text, which is the margin less the distance.
            let inset = border.distance as f32 * scale;
            let setup = sections.get(section).map(|section| &section.setup);
            let (left, top, right, bottom) = if border.from_text {
                let metrics = setup.map(PageMetrics::from_setup).unwrap_or_default();
                (
                    (metrics.margin_left - border.distance as f32).max(0.0) * scale,
                    (metrics.margin_top - border.distance as f32).max(0.0) * scale,
                    page.width - (metrics.margin_right - border.distance as f32).max(0.0) * scale,
                    page.height - (metrics.margin_bottom - border.distance as f32).max(0.0) * scale,
                )
            } else {
                (inset, inset, page.width - inset, page.height - inset)
            };

            let width = (right - left).max(1.0);
            let height = (bottom - top).max(1.0);
            let edges = [
                (&border.top, Side::Top, left, top, width),
                (&border.bottom, Side::Bottom, left, bottom, width),
                (&border.start, Side::Start, left, top, height),
                (&border.end, Side::End, right, top, height),
            ];

            for (edge, side, x, y, run) in edges {
                let Some(edge) = edge.as_ref().filter(|edge| edge.is_visible()) else { continue };
                let thickness = (edge.width_points() * scale).max(1.0);
                let colour = edge.color.as_deref().and_then(Color::from_hex).unwrap_or(automatic);
                crate::borders::draw_edge(page, edge, side, x, y, run, thickness, colour);
            }
        }
    }

    /// Draws one paragraph's shading and borders round the band it fills.
    #[allow(clippy::too_many_arguments)]
    fn decorate_paragraph(
        page: &mut Page,
        resolved: &wp_docx::model::ResolvedParagraphProperties,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
        automatic: Color,
        scale: f32,
    ) {
        // A little room either side, which is what the `w:space` of a border
        // asks for and what stops a box sitting on the letters.
        let pad = 2.0 * scale;
        let (left, right) = (left - pad, right + pad);
        let (top, bottom) = (top - pad * 0.5, bottom + pad * 0.5);
        let width = (right - left).max(1.0);
        let height = (bottom - top).max(1.0);

        if let Some(fill) = resolved.shading.as_deref().and_then(Color::from_hex) {
            page.decorations.push(Decoration { x: left, y: top, width, height, color: fill });
        }

        // Which style an edge is drawn in is the same question here as round a
        // page, and is answered in one place for both.
        let edges = [
            (&resolved.borders.top, Side::Top, left, top, width),
            (&resolved.borders.bottom, Side::Bottom, left, bottom, width),
            (&resolved.borders.start, Side::Start, left, top, height),
            (&resolved.borders.end, Side::End, right, top, height),
        ];
        for (border, side, x, y, run) in edges {
            let Some(border) = border.as_ref().filter(|border| border.is_visible()) else {
                continue;
            };
            let thickness = (border.width_points() * scale).max(1.0);
            let colour = border.color.as_deref().and_then(Color::from_hex).unwrap_or(automatic);
            crate::borders::draw_edge(page, border, side, x, y, run, thickness, colour);
        }
    }

    /// Lays a table out as a grid of cells.
    ///
    /// # How a row is measured
    ///
    /// A row is as tall as its tallest cell, and how tall a cell is cannot be
    /// known before its paragraphs have been broken into lines. So each row is
    /// laid out twice: once onto a scratch page to measure it, and then, once
    /// the page it belongs on is decided, for real. The counting state is put
    /// back between the two passes, or a numbered list inside a table would
    /// count every item twice.
    #[allow(clippy::too_many_arguments)]
    fn place_table(
        &mut self,
        table: &Table,
        index: &mut usize,
        document: &Document,
        pages: &mut Vec<Page>,
        y: &mut f32,
        column: &mut usize,
        area: Placement,
    ) {
        let scale = self.pixels_per_point();
        let columns = self.column_widths(table, document, area.text_width, scale);
        if columns.is_empty() {
            return;
        }

        let borders = document
            .styles()
            .resolve_table_borders(table.style.as_deref())
            .overlaid_with(&table.borders);

        // Which parts of the table its style is allowed to treat specially.
        // The style says what a header row looks like; the table says whether
        // it has one. See `wp_docx::table_properties::TableLook`.
        let look = table.look;

        // Half the room between cells goes on each side of every one of them,
        // so a cell sits inside its slot of the grid rather than filling it.
        let spacing = cell_spacing(table, scale);
        let half = spacing / 2.0;
        // A table sits in whichever column the flow has reached.
        let table_left =
            area.in_column(*column).left + table.indent as f32 / TWIPS_PER_POINT * scale;

        for (row_number, row) in table.rows.iter().enumerate() {
            let spans = cell_spans(row, &columns, table_left, half);

            // Measured first, on a page of its own that is then thrown away.
            let saved_counters = self.counters.clone();
            let mut scratch =
                vec![Page { width: area.page_width, height: area.page_height, ..Page::default() }];
            let mut scratch_index = *index;
            let mut height = 0.0f32;
            // How tall each cell's own content came out, so that a cell whose
            // text sits in the middle or at the foot of the row can be told how
            // much room is left over. The row is as tall as its tallest cell,
            // and every other cell has some.
            let mut measured: Vec<f32> = Vec::with_capacity(row.cells.len());
            for (cell, (left, width)) in row.cells.iter().zip(&spans) {
                let margins = margins_of_cell(cell, table, scale);
                let across = (*width - margins.across()).max(1.0);
                if cell.direction.is_turned() {
                    // The tallest this row could be: what is left of the page
                    // under it. A cell whose text is turned is as long as the
                    // row is tall, so this is the longest its line can be.
                    let room = (self.limit(pages.len().saturating_sub(1), area)
                        - *y
                        - spacing
                        - margins.down())
                    .max(1.0);
                    // Turned text is measured the other way round: what it
                    // needs is how *long* it came out, because that is what the
                    // row has to be tall enough for.
                    let laid =
                        self.turned_cell(cell, &mut scratch_index, document, area, room, across);
                    let length = text_length(&laid);
                    // A turned cell fills the length it asked for, so it has no
                    // room left over and nowhere to be moved to.
                    measured.push(f32::INFINITY);
                    height = height.max(length + margins.down());
                    continue;
                }
                let mut cell_y = 0.0f32;
                self.place_cell(
                    cell,
                    &mut scratch_index,
                    document,
                    &mut scratch,
                    &mut cell_y,
                    area,
                    *left + margins.start,
                    across,
                );
                measured.push(cell_y);
                height = height.max(cell_y + margins.down());
            }
            self.counters = saved_counters;

            // The room between the cells is part of the row: half of it above
            // them and half below.
            height += spacing;
            if let Some(wanted) = row.height {
                let wanted = wanted as f32 / TWIPS_PER_POINT * scale;
                // Word's two rules, and the difference between them is the
                // whole point of having both: a minimum is pushed taller by
                // what is in the row, and an exact height is not — what does
                // not fit is cut off at the line the row ends on.
                height = if row.height_exact { wanted + spacing } else { height.max(wanted) };
            }

            // A row that will not fit starts a page, unless it would not fit on
            // an empty one either — in which case it has to overflow somewhere.
            if *y + height > self.limit(pages.len().saturating_sub(1), area)
                && !pages.last().is_some_and(|p| p.glyphs.is_empty())
            {
                self.start_page(pages, y, column, area);
            }

            // The colour behind each cell, which is what a banded table is
            // made of. It goes on before the text, because a decoration is
            // drawn under the glyphs. The band is the cells' own, inside the
            // room left round them: a table with spacing shows the paper
            // between one cell and the next, not the colour of either.
            let cells_top = *y + half;
            let cells_bottom = *y + height - half;
            if let Some(page) = pages.last_mut() {
                for (number, (cell, (left, width))) in row.cells.iter().zip(&spans).enumerate() {
                    let parts =
                        parts_of(look, row_number, table.rows.len(), number, row.cells.len());
                    let from_style = document
                        .styles()
                        .resolve_table_cell(table.style.as_deref(), &parts)
                        .shading;
                    // What the cell itself says wins over what its style says,
                    // the way direct formatting always does.
                    let fill =
                        cell.shading.as_deref().or(from_style.as_deref()).and_then(Color::from_hex);
                    let Some(fill) = fill else { continue };
                    page.decorations.push(Decoration {
                        x: *left,
                        y: cells_top,
                        width: *width,
                        height: (cells_bottom - cells_top).max(0.0),
                        color: fill,
                    });
                }
            }

            let row_top = *y;
            // Where each cell began in the document, taken before it is laid
            // out: the running paragraph number is the cell's first paragraph
            // until the cell has been walked.
            let mut placed: Vec<(TextPosition, f32, f32)> = Vec::with_capacity(row.cells.len());
            for (at, (cell, (left, width))) in row.cells.iter().zip(&spans).enumerate() {
                placed.push((TextPosition::new(*index, 0), *left, *width));
                let margins = margins_of_cell(cell, table, scale);
                let across = (*width - margins.across()).max(1.0);

                if cell.direction.is_turned() {
                    // Laid out straight into a box as long as the row is tall,
                    // and then turned onto the page. The first line goes against
                    // the edge the reader starts from: the right for text that
                    // reads downwards, the left for text that reads upwards.
                    let length = (height - spacing - margins.down()).max(1.0);
                    let laid = self.turned_cell(cell, index, document, area, length, across);
                    let frame = match cell.direction {
                        wp_docx::model::TextDirection::Up => Frame::turned(
                            Turn::Up,
                            *left + margins.start,
                            cells_bottom - margins.bottom,
                        ),
                        // Mongolian's way: reading downwards with the lines
                        // going across to the right, which is the clockwise
                        // turn with its lines mirrored.
                        wp_docx::model::TextDirection::DownLeftToRight => Frame {
                            turn: Turn::Down,
                            flipped: true,
                            x: *left + margins.start,
                            y: cells_top + margins.top,
                        },
                        _ => Frame::turned(
                            Turn::Down,
                            *left + *width - margins.end,
                            cells_top + margins.top,
                        ),
                    };
                    if let Some(page) = pages.last_mut() {
                        lay_turned(page, laid, frame);
                    }
                    continue;
                }

                // Where the text sits down the cell: at the top, in the middle
                // of what is left over, or at the foot of it. The room left
                // over is what the row gained from a taller cell beside this
                // one, and a cell that fills the row has none.
                let room = (height - spacing - margins.down()).max(0.0);
                let spare = (room - measured.get(at).copied().unwrap_or(room)).max(0.0);
                let down = match cell.vertical {
                    wp_docx::table_properties::CellAlignment::Top => 0.0,
                    wp_docx::table_properties::CellAlignment::Middle => spare / 2.0,
                    wp_docx::table_properties::CellAlignment::Bottom => spare,
                };

                let mut cell_y = cells_top + margins.top + down;
                // What the page held before this cell, so that what the cell
                // adds can be cut back to the room the cell has. Only an exact
                // height leaves less room than the content needs.
                let already = pages.last().map_or((0, 0, 0), |page| {
                    (page.glyphs.len(), page.lines.len(), page.decorations.len())
                });
                self.place_cell(
                    cell,
                    index,
                    document,
                    pages,
                    &mut cell_y,
                    area,
                    *left + margins.start,
                    across,
                );
                if row.height_exact {
                    if let Some(page) = pages.last_mut() {
                        trim_cell(page, already, cells_bottom - margins.bottom);
                    }
                }
            }

            let row_bottom = row_top + height;
            if let Some(page) = pages.last_mut() {
                for (at, left, width) in placed {
                    page.cells.push(PlacedCell {
                        x: left,
                        y: cells_top,
                        width,
                        height: (cells_bottom - cells_top).max(0.0),
                        at,
                    });
                }
            }
            if let Some(page) = pages.last_mut() {
                draw_row_borders(
                    page,
                    &borders,
                    row,
                    &spans,
                    cells_top,
                    cells_bottom,
                    row_number,
                    table.rows.len(),
                    scale,
                    self.automatic_line,
                    self.table_gridlines,
                );
            }
            *y = row_bottom;
        }
    }

    /// Lays a cell whose text is turned out straight, onto a page of its own.
    ///
    /// `length` is how long a line may be, which for turned text is how tall
    /// the row is; `across` is how much room there is for the lines to stack
    /// into, which is the column's width. The page that comes back is in the
    /// cell's own coordinates and is turned onto the real one by [`lay_turned`].
    ///
    /// Nothing here knows about angles, which is the point: breaking a line,
    /// aligning it, numbering it and measuring it are the same work whichever
    /// way up the text ends up.
    fn turned_cell(
        &mut self,
        cell: &TableCell,
        index: &mut usize,
        document: &Document,
        area: Placement,
        length: f32,
        across: f32,
    ) -> Page {
        let mut pages = vec![Page { width: length, height: across, ..Page::default() }];
        let mut y = 0.0f32;
        // Word's Vertical is writing, not a turn: the letters of the East
        // Asian scripts stand upright in it. The other directions turn the
        // whole line.
        let was = core::mem::replace(&mut self.vertical, cell.direction.is_vertical_writing());
        self.place_cell(cell, index, document, &mut pages, &mut y, area, 0.0, length);
        self.vertical = was;
        // A cell has no bottom limit of its own, so it is never broken across
        // pages and there is exactly one page to take back.
        pages.remove(0)
    }

    /// Lays one cell's blocks into its own column.
    ///
    /// The bottom limit is lifted for the length of a cell: where the row goes
    /// has already been decided, and a paragraph inside a cell must not start a
    /// page of its own halfway through one.
    #[allow(clippy::too_many_arguments)]
    fn place_cell(
        &mut self,
        cell: &TableCell,
        index: &mut usize,
        document: &Document,
        pages: &mut Vec<Page>,
        y: &mut f32,
        area: Placement,
        left: f32,
        width: f32,
    ) {
        // A cell continuing the one above holds no content of its own, but its
        // paragraphs are still counted: they exist in the document.
        // A cell has no columns of its own: the text inside it fills the cell.
        let cell_area = Placement { left, text_width: width, bottom_limit: f32::INFINITY, ..area }
            .without_columns();
        let mut cell_column = 0usize;
        self.place_blocks(
            &cell.blocks,
            index,
            document,
            pages,
            y,
            &mut cell_column,
            cell_area,
            None,
        );
    }

    /// Moves the flow on: to the next column, or to a new page after the last.
    ///
    /// This is what makes columns columns. Text that runs out of room does not
    /// go straight to a new page — it goes to the top of the next column, and
    /// only the last column overflows onto paper.
    fn start_page(&self, pages: &mut Vec<Page>, y: &mut f32, column: &mut usize, area: Placement) {
        if *column + 1 < area.columns {
            *column += 1;
        } else {
            *column = 0;
            pages.push(Page {
                width: area.page_width,
                height: area.page_height,
                ..Page::default()
            });
        }
        *y = area.top;
    }

    /// Decodes an embedded picture, once per document.
    ///
    /// A picture can appear many times, and a document is laid out again after
    /// every keystroke; decoding a photograph each time would make typing
    /// visibly slow. A picture that will not decode is remembered as such, so
    /// a damaged one is not attempted over and over either.
    fn picture(
        &mut self,
        picture: &wp_docx::model::Picture,
        document: &Document,
    ) -> Option<Rc<Image>> {
        if let Some(known) = self.pictures.get(&picture.relationship) {
            return known.clone();
        }

        // The fonts of this machine go with the bytes: a metafile draws words
        // as well as shapes, and the letters have to come from somewhere. See
        // [`wp_image::metafile::Faces`].
        let decoded = document
            .embedded_part(&picture.relationship)
            .and_then(|bytes| wp_image::decode_with(bytes, self.library).ok())
            .filter(|image| !image.is_empty())
            .map(Rc::new);

        self.pictures.insert(picture.relationship.clone(), decoded.clone());
        decoded
    }

    /// A picture already decoded, by the relationship it is embedded through.
    ///
    /// Placing a page has no document to hand, so anything that needs a
    /// picture there has to have asked for it while one was. See
    /// [`Self::decode_group`].
    fn decoded(&self, relationship: &str) -> Option<Rc<Image>> {
        self.pictures.get(relationship).cloned().flatten()
    }

    /// Decodes every picture in a group, and in the groups inside it.
    ///
    /// Done while the item is built, because that is the last place the
    /// document is to hand: a group is placed long afterwards, when all that
    /// is left is the page.
    fn decode_group(&mut self, group: &wp_docx::group::Group, document: &Document) {
        for member in &group.members {
            match &member.what {
                wp_docx::group::Inside::Picture(picture) => {
                    self.picture(picture, document);
                }
                wp_docx::group::Inside::Group(inner) => self.decode_group(inner, document),
                wp_docx::group::Inside::Shape(_) => {}
            }
        }
    }

    /// The mark a list paragraph carries, and what its level asks for.
    ///
    /// Returns the mark, the level's indent and its hanging indent, all in
    /// twentieths of a point. A paragraph that is not in a list gets nothing,
    /// and neither does one in a list the document never defined.
    fn list_mark(
        &mut self,
        resolved: &wp_docx::model::ResolvedParagraphProperties,
        document: &Document,
    ) -> (Option<String>, i32, i32) {
        let Some(reference) = resolved.numbering else {
            return (None, 0, 0);
        };
        let numbering = document.numbering();
        let Some(level) = numbering.level(reference.id, reference.level) else {
            return (None, 0, 0);
        };

        let indent_start = level.indent_start.unwrap_or(0);
        let hanging = level.indent_hanging.unwrap_or(0);
        let mark = self.counters.advance(numbering, reference.id, reference.level);

        // An empty mark is a level that counts without showing anything.
        (mark.filter(|text| !text.is_empty()), indent_start, hanging)
    }

    /// Draws a list mark beside the first line of its paragraph.
    #[allow(clippy::too_many_arguments)]
    fn place_list_mark(
        &mut self,
        mark: &str,
        paragraph: &Paragraph,
        document: &Document,
        page: &mut Page,
        line_left: f32,
        indent_first: f32,
        baseline: f32,
    ) {
        let properties = document.styles().resolve_run(paragraph.style(), &Default::default());
        let Some(style) = self.style_for(&properties) else { return };

        let glyphs = self.shape(mark, &style, 0);
        let width: f32 = glyphs.iter().map(|glyph| glyph.advance).sum();

        // With a hanging indent the mark sits where the first line would have
        // begun. Without one there is no room reserved for it, so it goes just
        // outside the text instead of on top of it.
        let mut pen = if indent_first < 0.0 {
            line_left + indent_first
        } else {
            line_left - width - style.size * 0.4
        };

        for glyph in glyphs {
            page.glyphs.push(PositionedGlyph {
                face: glyph.face,
                glyph: glyph.glyph,
                x: pen,
                baseline,
                advance: glyph.advance,
                size: style.size,
                stretch: 1.0,
                color: style.color,
                effect: style.effect,
                // The mark is not part of the document's text, so it points at
                // nothing a caret could reach.
                source: TextPosition::default(),
                source_length: 0,
                invisible: false,
                shift_x: 0.0,
                shift_y: 0.0,
            });
            pen += glyph.advance;
        }
    }

    /// The ellipsis after the first line of a paragraph an outline shows only
    /// the first line of, in the style the line ends in.
    ///
    /// Put down after the line is, so that it lies outside every line's range
    /// of glyphs, as a list mark does: it is not text, and a click or a
    /// selection never lands on it.
    fn place_ellipsis(&mut self, style: &RunStyle, page: &mut Page, x: f32, baseline: f32) {
        let mut pen = x;
        for glyph in self.shape("…", style, 0) {
            page.glyphs.push(PositionedGlyph {
                face: glyph.face,
                glyph: glyph.glyph,
                x: pen,
                baseline,
                advance: glyph.advance,
                size: style.size,
                stretch: 1.0,
                color: style.color,
                effect: style.effect,
                source: TextPosition::default(),
                source_length: 0,
                invisible: false,
                shift_x: 0.0,
                shift_y: 0.0,
            });
            pen += glyph.advance;
        }
    }

    /// How far in an outline puts a paragraph, in points: a heading one step
    /// in for each level above its own, and body text one step in from the
    /// heading above it — or from a heading at the top level, before there
    /// is any, which is where Word shows a title.
    ///
    /// `heading` is the paragraph's level counted from one, if it is a
    /// heading.
    fn outline_step(&self, heading: Option<u8>) -> f32 {
        let level = heading.unwrap_or_else(|| self.outline_heading.max(1).saturating_add(1));
        f32::from(level.saturating_sub(1)) * OUTLINE_STEP
    }

    /// The height an empty paragraph occupies.
    pub(crate) fn empty_line_height(&mut self, paragraph: &Paragraph, document: &Document) -> f32 {
        let mut properties = document.styles().resolve_run(paragraph.style(), &Default::default());
        if self.plain() {
            properties = plain_run(document, properties);
        }
        match self.style_for(&properties) {
            Some(style) => style.line_height,
            None => 0.0,
        }
    }
    /// Turns a paragraph into measured, breakable items.
    ///
    /// The paragraph's own text comes back with them, built as the runs are
    /// walked. It could be asked of the document instead — but that means
    /// finding the paragraph in the element tree, and finding it means walking
    /// the tree from the top, once per paragraph. A thousand-page document laid
    /// out that way spends most of its time counting paragraphs it has already
    /// counted.
    pub(crate) fn build_items(
        &mut self,
        paragraph: &Paragraph,
        document: &Document,
        styles: &mut Vec<RunStyle>,
        paragraph_index: usize,
        text: &mut String,
    ) -> Vec<Item> {
        let mut items = Vec::new();
        // Which field of this paragraph is being built, so a `SEQ` can be told
        // from the one before it.
        let mut field_number = 0usize;
        // Byte offset within the paragraph's text, counted the same way the
        // editing layer counts it, so a glyph and a caret mean the same thing.
        let mut offset = 0usize;
        // The character the run before ended with. A line may be broken between
        // two runs only where it could be broken inside one, or a full stop
        // somebody made bold would be free to begin a line.
        let mut previous_character: Option<char> = None;

        for run in &paragraph.runs {
            // Text somebody deleted is only drawn while the markup is showing;
            // otherwise the document reads as it would once every change was
            // accepted, which is what most people want most of the time.
            let deleted = run
                .revision
                .as_ref()
                .is_some_and(|change| change.kind == wp_docx::model::RevisionKind::Deleted);
            if deleted && !self.show_markup {
                continue;
            }

            let mut resolved = document.resolve_run(paragraph, run);
            if self.plain() {
                resolved = plain_run(document, resolved);
            }
            let Some(mut style) = self.style_for(&resolved) else {
                continue;
            };
            if let Some(change) = &run.revision {
                // Word marks a change by its author's colour, underlining what
                // was added and striking through what was taken out.
                style.color = author_color(&change.author);
                match change.kind {
                    wp_docx::model::RevisionKind::Inserted => style.underline = true,
                    wp_docx::model::RevisionKind::Deleted => style.strike = true,
                }
            }
            // Text somebody reformatted is marked in their colour and nothing
            // else: the words are unchanged, only the way they are set. Word
            // says what changed in a balloon down the margin, which this has
            // nowhere to put yet.
            if self.show_formatting && run.revision.is_none() {
                if let Some(change) = &run.format_change {
                    style.color = author_color(&change.author);
                }
            }
            let style_index = styles.len();
            styles.push(style.clone());

            let before = offset;
            let first = items.len();
            self.build_run_items(
                run,
                document,
                &style,
                style_index,
                &mut items,
                &mut offset,
                text,
                paragraph_index,
                field_number,
            );
            if run.field.is_some() {
                field_number += 1;
            }

            // Whether this run's first item may begin a line depends on what
            // the run before it ended with.
            let (opens, closes) = run_edges(run);
            if items.len() > first {
                if let (Some(before), Some(after)) = (previous_character, opens) {
                    if !wp_break::may_break(before, after) {
                        items[first].breaks_before = false;
                    }
                }
                previous_character = closes;
            }

            // Deleted text takes up no room in the document's own text, so it
            // takes up no caret positions either: every item it made points at
            // the one place it sits between.
            if deleted {
                for item in &mut items[first..] {
                    item.start_offset = before;
                    item.end_offset = before;
                }
                offset = before;
                // The text goes back with them: a deleted run is not part of
                // what the document says, and the offsets after it count as
                // though it were not there.
                text.truncate(before);
            }
        }

        items
    }

    #[allow(clippy::too_many_arguments)]
    fn build_run_items(
        &mut self,
        run: &Run,
        document: &Document,
        style: &RunStyle,
        style_index: usize,
        items: &mut Vec<Item>,
        offset: &mut usize,
        paragraph_text: &mut String,
        paragraph_index: usize,
        field_number: usize,
    ) {
        for content in &run.content {
            // A copy is laid out as what it holds: printing a selection and
            // pasting as a picture lay out what was copied.
            match content.bare() {
                RunContent::Text(text) => {
                    // A run inside a field shows what the field works out, not
                    paragraph_text.push_str(text);
                    // the answer somebody cached in the file. The cached text
                    // is still what the caret moves through, so the offsets
                    // advance by its length either way.
                    let shown = self
                        .field_value(run)
                        .or_else(|| {
                            let instruction = run.field.as_deref()?;
                            self.document_field_value(
                                instruction,
                                document,
                                paragraph_index,
                                field_number,
                            )
                        })
                        .unwrap_or_else(|| text.clone());
                    // A run set across a vertical line, or as two lines in
                    // one, is one piece that no line breaks inside.
                    if self.east_asian_run(&shown, text, style, style_index, items, offset) {
                        continue;
                    }
                    let chunks = segment(&shown);
                    // The words are cut where the language's patterns allow,
                    // when the document hyphenates on its own and the text is
                    // the document's — a field's answer is not cut, since its
                    // pieces would not be the document's bytes.
                    let patterns = if self.hyphenating && shown == *text {
                        style.hyphenation.clone()
                    } else {
                        None
                    };
                    // However long the shown text is, the chunks between them
                    // take up exactly the bytes the document holds, so the
                    // caret still lands where the text really is.
                    let mut left = text.len();
                    for (number, chunk) in chunks.iter().enumerate() {
                        let start = *offset;
                        let glyphs = self.shape(&chunk.text, style, start);
                        // A chunk ends with an optional hyphen only where the
                        // writer put one, and a break is allowed after every
                        // one of them — so such a chunk is a place a line may
                        // end, and the hyphen is what is drawn if it does.
                        let drawn_hyphen = if chunk.text.ends_with('\u{00AD}') {
                            self.hyphen_width(style)
                        } else {
                            0.0
                        };
                        let taken = if number + 1 == chunks.len() {
                            left
                        } else {
                            chunk.text.len().min(left)
                        };
                        left -= taken;
                        *offset += taken;
                        let end = *offset;

                        // Where the word may be broken, as byte offsets into
                        // the chunk, from the patterns; nothing for a space,
                        // for a word with anything but letters in it, and for
                        // a word in capitals when those are to be left alone.
                        let cuts = match &patterns {
                            Some(patterns) if !chunk.is_space && taken == chunk.text.len() => {
                                word_cuts(&chunk.text, patterns, self.hyphenation.capitals)
                            }
                            _ => Vec::new(),
                        };
                        let hyphen_width =
                            if cuts.is_empty() { 0.0 } else { self.hyphen_width(style) };
                        let pieces = cut_glyphs(glyphs, start, &cuts);
                        let count = pieces.len();
                        for (which, (glyphs, piece_start, piece_end)) in
                            pieces.into_iter().enumerate()
                        {
                            let last = which + 1 == count;
                            let width = glyphs.iter().map(|glyph| glyph.advance).sum();
                            items.push(Item {
                                glyphs,
                                width,
                                is_space: chunk.is_space,
                                breaks_before: which == 0,
                                is_tab: false,
                                aligned_tab: None,
                                picture: None,
                                picture_anchor: None,
                                picture_name: None,
                                picture_turn: wp_docx::floating::Turned::default(),
                                picture_video: false,
                                group: None,
                                shape: None,
                                math: None,
                                chart: None,
                                ink: None,
                                ruby: None,
                                hyphen: if last { drawn_hyphen } else { hyphen_width },
                                auto_hyphen: which > 0,
                                hard_break: None,
                                style: style_index,
                                start_offset: piece_start,
                                end_offset: if last { end } else { piece_end },
                            });
                        }
                    }
                }
                // An alignment tab is a tab that goes to the middle of the line
                // or to its far end whatever the stops say — which is why it
                // carries where it is going rather than looking it up.
                RunContent::PositionTab(alignment) => {
                    paragraph_text.push('\t');
                    let start = *offset;
                    *offset += 1;
                    items.push(Item {
                        width: self.default_tab_width(),
                        glyphs: Vec::new(),
                        is_space: false,
                        breaks_before: true,
                        is_tab: true,
                        aligned_tab: Some(*alignment),
                        picture: None,
                        picture_anchor: None,
                        picture_name: None,
                        picture_turn: wp_docx::floating::Turned::default(),
                        picture_video: false,
                        group: None,
                        math: None,
                        chart: None,
                        ink: None,
                        ruby: None,
                        hyphen: 0.0,
                        auto_hyphen: false,
                        shape: None,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Tab => {
                    paragraph_text.push('\t');
                    // A tab is one character to the caret, so it takes one byte
                    // of the paragraph's text — the same byte the editor counts.
                    let start = *offset;
                    *offset += 1;
                    items.push(Item {
                        // Only an estimate: how far a tab really reaches depends
                        // on where the line has got to, which is not known until
                        // the line is placed. Breaking uses the default width
                        // and placing uses the true stop.
                        width: self.default_tab_width(),
                        glyphs: Vec::new(),
                        is_space: false,
                        breaks_before: true,
                        is_tab: true,
                        aligned_tab: None,
                        picture: None,
                        picture_anchor: None,
                        picture_name: None,
                        picture_turn: wp_docx::floating::Turned::default(),
                        picture_video: false,
                        group: None,
                        math: None,
                        chart: None,
                        ink: None,
                        ruby: None,
                        hyphen: 0.0,
                        auto_hyphen: false,
                        shape: None,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                // The mark that points at a note is drawn as its number, worked
                // out from where the marks fall in reading order rather than
                // from the number in the file.
                RunContent::NoteReference { id, endnote } => {
                    paragraph_text.push(' ');
                    let start = *offset;
                    *offset += 1;
                    let shown = self.note_number(*id, *endnote).to_string();
                    let glyphs = self.shape(&shown, style, start);
                    let width = glyphs.iter().map(|glyph| glyph.advance).sum();
                    items.push(Item {
                        glyphs,
                        width,
                        is_space: false,
                        breaks_before: true,
                        is_tab: false,
                        aligned_tab: None,
                        picture: None,
                        picture_anchor: None,
                        picture_name: None,
                        picture_turn: wp_docx::floating::Turned::default(),
                        picture_video: false,
                        group: None,
                        math: None,
                        chart: None,
                        ink: None,
                        ruby: None,
                        hyphen: 0.0,
                        auto_hyphen: false,
                        shape: None,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Chart(reference) => {
                    paragraph_text.push(' ');
                    // A chart takes one character of the paragraph, exactly as
                    // a picture does. It is drawn from the numbers in its own
                    // part, which is why the document is needed here.
                    let start = *offset;
                    *offset += 1;

                    let scale = self.pixels_per_point();
                    let width = (reference.width_points() as f32 * scale).max(1.0);
                    let height = (reference.height_points() as f32 * scale).max(1.0);
                    let drawn = self.chart_drawing(reference, document, width, height);

                    items.push(Item {
                        glyphs: Vec::new(),
                        width,
                        is_space: false,
                        breaks_before: true,
                        is_tab: false,
                        aligned_tab: None,
                        picture: None,
                        picture_anchor: None,
                        picture_name: None,
                        picture_turn: wp_docx::floating::Turned::default(),
                        picture_video: false,
                        group: None,
                        math: None,
                        chart: drawn.map(|drawing| (Box::new(drawing), height)),
                        ink: None,
                        ruby: None,
                        hyphen: 0.0,
                        auto_hyphen: false,
                        shape: None,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                // A drawing nothing here models is not drawn and takes no room
                // in the line: what it is, is the file's business, and what is
                // wanted of this program is that it does not lose it. See
                // [`wp_docx::model::RunContent::Carried`].
                RunContent::Carried(_) => {}
                RunContent::Ink(reference) => {
                    paragraph_text.push(' ');
                    // Ink takes one character of the paragraph, the same as
                    // every other drawing: it is one thing the caret can stand
                    // either side of. The strokes are in a part of their own,
                    // which is why the document is needed here.
                    let start = *offset;
                    *offset += 1;

                    let scale = self.pixels_per_point();
                    let drawn = document.ink(&reference.relationship);
                    // How big the ink is drawn: what the run says, and when it
                    // says nothing, how big what was drawn is.
                    let (width_emu, height_emu) = match (reference.width_emu, reference.height_emu)
                    {
                        (width, height) if width > 0 && height > 0 => (width, height),
                        _ => drawn
                            .as_ref()
                            .and_then(wp_docx::ink::Ink::bounds)
                            .map(|(left, top, right, bottom)| (right - left, bottom - top))
                            .unwrap_or((0, 0)),
                    };
                    let points = |emu: i64| emu as f32 / 914_400.0 * 72.0 * scale;
                    let width = points(width_emu).max(1.0);
                    let height = points(height_emu).max(1.0);
                    let drawing = drawn
                        .filter(|ink| !ink.is_empty())
                        .map(|ink| crate::inking::draw(&ink, width, height));
                    // Ink that floats takes no room in the line, the same as a
                    // picture that floats: the anchor says where it goes, and
                    // the line leaves a floating item out — see [`Item::floats`].
                    let floats = reference.anchor.is_some();
                    let name = if reference.name.is_empty() {
                        "Ink".to_owned()
                    } else {
                        reference.name.clone()
                    };

                    items.push(Item {
                        glyphs: Vec::new(),
                        width,
                        is_space: false,
                        breaks_before: !floats,
                        is_tab: false,
                        aligned_tab: None,
                        picture: None,
                        picture_anchor: reference.anchor.clone(),
                        picture_name: Some(name),
                        picture_turn: wp_docx::floating::Turned::default(),
                        picture_video: false,
                        group: None,
                        math: None,
                        chart: None,
                        ink: drawing.map(|drawing| (Box::new(drawing), height)),
                        ruby: None,
                        hyphen: 0.0,
                        auto_hyphen: false,
                        shape: None,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Diagram(reference) => {
                    paragraph_text.push(' ');
                    // A diagram takes one character of the paragraph, exactly
                    // as a chart does. What is drawn is the shapes it was last
                    // laid out into, which live in parts of their own — so the
                    // document is needed here, and what comes back is a group,
                    // because a group is what a drawing of several shapes is.
                    let start = *offset;
                    *offset += 1;

                    let scale = self.pixels_per_point();
                    let width = (reference.width_points() as f32 * scale).max(1.0);
                    let height = (reference.height_points() as f32 * scale).max(1.0);
                    let drawn = document.diagram(reference).and_then(|diagram| diagram.drawing);
                    if let Some(group) = &drawn {
                        self.decode_group(group, document);
                    }

                    items.push(Item {
                        glyphs: Vec::new(),
                        width,
                        is_space: false,
                        breaks_before: true,
                        is_tab: false,
                        aligned_tab: None,
                        picture: None,
                        picture_anchor: None,
                        picture_name: None,
                        picture_turn: wp_docx::floating::Turned::default(),
                        picture_video: false,
                        group: drawn.map(|group| (Box::new(group), height)),
                        math: None,
                        chart: None,
                        ink: None,
                        ruby: None,
                        hyphen: 0.0,
                        auto_hyphen: false,
                        shape: None,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Ruby(ruby) => {
                    // The word under the reading is the text: its characters
                    // are the document's and the caret walks through them. The
                    // reading's are not counted at all — it is an annotation
                    // about the word rather than part of the sentence.
                    let word = ruby.plain_text();
                    paragraph_text.push_str(&word);
                    let start = *offset;
                    *offset += word.len();

                    let laid = self.ruby_box(ruby, style, start, paragraph_index);
                    let width = laid.width;
                    let height = laid.ascent + laid.descent;
                    items.push(Item {
                        glyphs: Vec::new(),
                        width,
                        is_space: false,
                        breaks_before: true,
                        is_tab: false,
                        aligned_tab: None,
                        picture: None,
                        picture_anchor: None,
                        picture_name: None,
                        picture_turn: wp_docx::floating::Turned::default(),
                        picture_video: false,
                        group: None,
                        shape: None,
                        math: None,
                        chart: None,
                        ink: None,
                        ruby: Some((Box::new(laid), height)),
                        hyphen: 0.0,
                        auto_hyphen: false,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Math(math) => {
                    paragraph_text.push(' ');
                    // An equation takes one character of the paragraph, exactly
                    // as a picture does, so the caret can stand either side of
                    // it. It is measured whole here and drawn whole later: an
                    // equation is never broken across lines.
                    let start = *offset;
                    *offset += 1;

                    let laid = self.math_box(math, style);
                    let width = laid.width;
                    let height = laid.ascent + laid.descent;
                    items.push(Item {
                        glyphs: Vec::new(),
                        width,
                        is_space: false,
                        breaks_before: true,
                        is_tab: false,
                        aligned_tab: None,
                        picture: None,
                        picture_anchor: None,
                        picture_name: None,
                        picture_turn: wp_docx::floating::Turned::default(),
                        picture_video: false,
                        group: None,
                        shape: None,
                        math: Some((Box::new(laid), height)),
                        chart: None,
                        ink: None,
                        ruby: None,
                        hyphen: 0.0,
                        auto_hyphen: false,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Shape(shape) => {
                    paragraph_text.push(' ');
                    // A shape takes one character, exactly as a picture does,
                    // so the caret can stand either side of it.
                    let start = *offset;
                    *offset += 1;

                    let scale = self.pixels_per_point();
                    // A drawing that floats takes no room on the line: the
                    // text goes round it, not past it.
                    let width = if shape.anchor.is_some() {
                        0.0
                    } else {
                        shape.width_points() as f32 * scale
                    };
                    let height = shape.height_points() as f32 * scale;

                    items.push(Item {
                        glyphs: Vec::new(),
                        width,
                        is_space: false,
                        breaks_before: true,
                        is_tab: false,
                        aligned_tab: None,
                        picture: None,
                        picture_anchor: None,
                        picture_name: None,
                        picture_turn: wp_docx::floating::Turned::default(),
                        picture_video: false,
                        group: None,
                        math: None,
                        chart: None,
                        ink: None,
                        ruby: None,
                        hyphen: 0.0,
                        auto_hyphen: false,
                        shape: Some((shape.clone(), height)),
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Group(group) => {
                    paragraph_text.push(' ');
                    // A group takes one character, the same as the drawings
                    // inside it would have taken one each: it is one drawing
                    // as far as the text is concerned.
                    let start = *offset;
                    *offset += 1;

                    let scale = self.pixels_per_point();
                    let width = if group.anchor.is_some() {
                        0.0
                    } else {
                        group.width_points() as f32 * scale
                    };
                    let height = group.height_points() as f32 * scale;
                    self.decode_group(group, document);

                    items.push(Item {
                        glyphs: Vec::new(),
                        width,
                        is_space: false,
                        breaks_before: true,
                        is_tab: false,
                        aligned_tab: None,
                        picture: None,
                        picture_anchor: None,
                        picture_name: None,
                        picture_turn: wp_docx::floating::Turned::default(),
                        picture_video: false,
                        group: Some((Box::new(group.clone()), height)),
                        math: None,
                        chart: None,
                        ink: None,
                        ruby: None,
                        hyphen: 0.0,
                        auto_hyphen: false,
                        shape: None,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Picture(picture) => {
                    paragraph_text.push(' ');
                    // A picture takes one character of the paragraph's text, so
                    // the caret can stand either side of it and Backspace can
                    // reach it — the same rule a tab follows.
                    let start = *offset;
                    *offset += 1;

                    let scale = self.pixels_per_point();
                    let mut width = picture.width_points() as f32 * scale;
                    let mut height = picture.height_points() as f32 * scale;
                    let decoded = self.picture(picture, document);

                    // A drawing with no stated size is drawn at its own, which
                    // is what a picture pasted straight into a document has.
                    if width <= 0.0 || height <= 0.0 {
                        let (natural_width, natural_height) = decoded
                            .as_ref()
                            .map_or((0.0, 0.0), |image| (image.width as f32, image.height as f32));
                        width = natural_width;
                        height = natural_height;
                    }

                    items.push(Item {
                        glyphs: Vec::new(),
                        width,
                        is_space: false,
                        breaks_before: true,
                        is_tab: false,
                        aligned_tab: None,
                        picture: decoded.map(|image| (image, height)),
                        picture_anchor: picture.anchor.clone(),
                        picture_name: picture.description.clone(),
                        picture_turn: picture.turned,
                        picture_video: picture.video,
                        group: None,
                        shape: None,
                        math: None,
                        chart: None,
                        ink: None,
                        ruby: None,
                        hyphen: 0.0,
                        auto_hyphen: false,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Break(kind) => {
                    paragraph_text.push('\n');
                    let start = *offset;
                    *offset += 1;
                    items.push(Item {
                        glyphs: Vec::new(),
                        width: 0.0,
                        is_space: false,
                        breaks_before: true,
                        is_tab: false,
                        aligned_tab: None,
                        picture: None,
                        picture_anchor: None,
                        picture_name: None,
                        picture_turn: wp_docx::floating::Turned::default(),
                        picture_video: false,
                        group: None,
                        math: None,
                        chart: None,
                        ink: None,
                        ruby: None,
                        hyphen: 0.0,
                        auto_hyphen: false,
                        shape: None,
                        hard_break: Some(*kind),
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                // `bare` has taken a copy's wrapping off already.
                RunContent::Copied(_) => {}
            }
        }
    }

    /// The widest stretch of room a line has, once the drawings floating
    /// beside it are out of the way.
    ///
    /// What asks is everything that needs one number rather than a line's
    /// worth of them: the first guess at a line's height, and the widow rule
    /// looking ahead. The line itself is laid out across every stretch — see
    /// [`Self::free_spans`].
    fn usable_span(&self, page: usize, left: f32, width: f32, top: f32, bottom: f32) -> (f32, f32) {
        let spans = self.free_spans(page, left, width, top, bottom);
        // The widest. A line has to be somewhere, so a drawing that covers
        // everything leaves the line where it was rather than nowhere.
        spans.into_iter().max_by(|one, other| (one.1).total_cmp(&other.1)).unwrap_or((left, width))
    }

    /// Every stretch of room a line has, left to right, once the drawings
    /// floating beside it are out of the way.
    ///
    /// A drawing in the middle of the text leaves room on both sides of it, and
    /// a line beside it is a piece of line each side — which is what Word's
    /// `bothSides` wrapping means and what it writes unless told otherwise.
    /// Each stretch is where it starts and how wide it is.
    fn free_spans(
        &self,
        page: usize,
        left: f32,
        width: f32,
        top: f32,
        bottom: f32,
    ) -> Vec<(f32, f32)> {
        if self.floats.is_empty() {
            return vec![(left, width)];
        }
        // Whether any drawing beside this line asks for the wider side alone.
        let mut largest_only = false;

        // Every stretch across the line that no drawing covers, in order.
        let mut free = vec![(left, left + width)];
        for float in &self.floats {
            if float.page != page || float.wrap == wp_docx::anchor::Wrap::None {
                continue;
            }
            // Top-and-bottom takes the whole width of anything it touches, and
            // is dealt with by pushing the text past it rather than by
            // narrowing it — so it takes no room here.
            if float.wrap == wp_docx::anchor::Wrap::TopAndBottom {
                continue;
            }
            if float.bottom <= top || float.top >= bottom {
                continue;
            }

            // Tight and through wrapping run the text up to the shape itself
            // rather than to the box round it: beside the point of a triangle a
            // line gets nearly the whole width, and beside its base none.
            let (blocked_left, blocked_right) = match (float.wrap, float.outline) {
                (wp_docx::anchor::Wrap::Tight | wp_docx::anchor::Wrap::Through, Some(outline)) => {
                    match crate::geometry::span_between(
                        outline.preset,
                        &outline.adjusts,
                        outline.x,
                        outline.y,
                        outline.width,
                        outline.height,
                        top,
                        bottom,
                    ) {
                        // The room asked for round the drawing is kept either side
                        // of the outline, the same as it is either side of the box.
                        Some((reaches_left, reaches_right)) => (
                            reaches_left - (outline.x - float.left),
                            reaches_right + (float.right - (outline.x + outline.width)),
                        ),
                        // The band is beside the box but not beside the shape: a
                        // line there is not blocked at all.
                        None => continue,
                    }
                }
                _ => (float.left, float.right),
            };

            // Which side the text is allowed down. Left means the text keeps
            // to the left of the drawing, so everything from the drawing's
            // left edge onwards is out of bounds however much room is beyond
            // it — that is what asking for one side means.
            let (blocked_left, blocked_right) = match float.side {
                wp_docx::anchor::WrapSide::Left => (blocked_left, left + width),
                wp_docx::anchor::WrapSide::Right => (left, blocked_right),
                wp_docx::anchor::WrapSide::Largest => {
                    largest_only = true;
                    (blocked_left, blocked_right)
                }
                wp_docx::anchor::WrapSide::BothSides => (blocked_left, blocked_right),
            };

            let mut narrowed = Vec::new();
            for (start, end) in free {
                if blocked_right <= start || blocked_left >= end {
                    narrowed.push((start, end));
                    continue;
                }
                if blocked_left > start {
                    narrowed.push((start, blocked_left));
                }
                if blocked_right < end {
                    narrowed.push((blocked_right, end));
                }
            }
            free = narrowed;
        }

        // What is left, as a place and a width each. Anything narrower than a
        // line is tall is not room for text at all: a four-pixel gap beside a
        // picture would otherwise take one letter per line and read as a
        // column of nonsense, and Word leaves such a gap empty too.
        let least = ROOM_FOR_TEXT;
        let mut spans: Vec<(f32, f32)> = free
            .into_iter()
            .filter(|(start, end)| end - start >= least)
            .map(|(start, end)| (start, end - start))
            .collect();
        spans.sort_by(|one, other| one.0.total_cmp(&other.0));

        // A drawing that asks for the wider side alone gets it.
        if largest_only && spans.len() > 1 {
            let widest = spans
                .iter()
                .copied()
                .max_by(|one, other| (one.1).total_cmp(&other.1))
                .expect("more than one");
            spans = vec![widest];
        }
        if spans.is_empty() {
            return vec![(left, width.max(1.0))];
        }
        spans
    }

    /// Pushes a position past any drawing wrapped above and below it.
    fn past_top_and_bottom_floats(&self, page: usize, y: f32) -> f32 {
        let mut y = y;
        for float in &self.floats {
            if float.page != page || float.wrap != wp_docx::anchor::Wrap::TopAndBottom {
                continue;
            }
            if float.top <= y && float.bottom > y {
                y = float.bottom;
            }
        }
        y
    }

    /// Works out where a floating drawing sits and records it.
    ///
    /// `line_top` is where the line carrying the anchor begins, which is what
    /// a position measured from the paragraph or the line is measured from.
    #[allow(clippy::too_many_arguments, reason = "a float has a place, a size and a page")]
    fn place_float(
        &mut self,
        anchor: &wp_docx::anchor::Anchor,
        page: &mut Page,
        page_index: usize,
        area: &Placement,
        line_top: f32,
        width: f32,
        height: f32,
        shape: &wp_docx::shapes::Shape,
        at: Option<TextPosition>,
    ) {
        let scale = self.pixels_per_point();
        let turned = wp_docx::floating::Turned::of_shape(shape);
        let (x, y) = self.float_box(
            anchor,
            page_index,
            area,
            line_top,
            width,
            height,
            Some((
                crate::geometry::Preset::from_word(&shape.preset),
                crate::geometry::Adjusts::from_pairs(&shape.adjusts),
            )),
        );

        page.shapes.push(PlacedShape {
            x,
            y,
            width,
            height,
            preset: crate::geometry::Preset::from_word(&shape.preset),
            adjusts: crate::geometry::Adjusts::from_pairs(&shape.adjusts),
            head_end: shape.head_end,
            id: shape.id,
            joins: shape.joins,
            route: None,
            effects: Effects::of(&shape.effects, scale, &self.theme),
            solid: Solid::of(&shape.depth, &shape.scene, scale, &self.theme),
            tail_end: shape.tail_end,
            fill: crate::paint::Paint::of(&shape.fill, &self.theme),
            outline: self.outline_colour(shape),
            outline_weight: (shape.outline_points_in(&self.theme) * scale).max(1.0),
            shadow: self.shape_shadow(scale),
            text: Vec::new(),
            text_turned: Vec::new(),
            name: shape.name.clone(),
            at,
            depth: anchor.depth,
            // Word's rule, and the only thing `behindDoc` decides: a floating
            // drawing is drawn over the text unless it says otherwise. Where
            // the wrapping keeps the text out of its way it makes no visible
            // difference; where it does not — a drawing laid over a table —
            // it is the difference between reading the words and not.
            over_text: !anchor.behind_text,
            source: Some(Box::new(shape.clone())),
            turn: turned.radians(),
            flipped_across: turned.flipped_across,
            flipped_down: turned.flipped_down,
        });
    }

    /// Puts everything a group holds onto the page.
    ///
    /// The group itself draws nothing: it is a rectangle with a world inside
    /// it, and what is drawn is the drawings in that world, each at its own
    /// fraction of the rectangle. A group inside a group is the same thing
    /// again, one rectangle further in.
    ///
    /// Everything in a group shares the group's place in the pile: they were
    /// made one drawing, and one drawing is at one depth.
    #[allow(
        clippy::too_many_arguments,
        reason = "a group has a place, a size, a depth and a where"
    )]
    fn place_group(
        &mut self,
        page: &mut Page,
        group: &wp_docx::group::Group,
        rect: (f32, f32, f32, f32),
        at: Option<TextPosition>,
        depth: u32,
        over_text: bool,
    ) {
        self.place_group_turned(
            page,
            group,
            rect,
            at,
            depth,
            over_text,
            wp_docx::floating::Turned::default(),
        );
    }

    /// The same, inside a group that is itself turned.
    ///
    /// `outer` is the turn already in force round this one, which the group's
    /// own turn is composed with. At the top there is none.
    #[allow(
        clippy::too_many_arguments,
        reason = "a group has a place, a size, a depth and a where"
    )]
    fn place_group_turned(
        &mut self,
        page: &mut Page,
        group: &wp_docx::group::Group,
        rect: (f32, f32, f32, f32),
        at: Option<TextPosition>,
        depth: u32,
        over_text: bool,
        outer: wp_docx::floating::Turned,
    ) {
        let (left, top, width, height) = rect;
        // The whole group's turn: its own, seen from inside whatever is round
        // it. Every member is turned by this as well as by its own.
        let turned = group.turned.inside(outer);
        let (middle_x, middle_y) = (left + width / 2.0, top + height / 2.0);
        let scale = self.pixels_per_point();

        for member in &group.members {
            let (fraction_x, fraction_y, fraction_width, fraction_height) = group.fractions(member);
            let member_width = fraction_width * width;
            let member_height = fraction_height * height;
            // Where the member's own middle lands once the group is turned: a
            // turn moves the members as well as turning each of them.
            let (from_x, from_y) = (
                left + fraction_x * width + member_width / 2.0 - middle_x,
                top + fraction_y * height + member_height / 2.0 - middle_y,
            );
            let (to_x, to_y) = turned.moves(from_x, from_y);
            let member_left = middle_x + to_x - member_width / 2.0;
            let member_top = middle_y + to_y - member_height / 2.0;
            let member_rect = (member_left, member_top, member_width, member_height);

            match &member.what {
                wp_docx::group::Inside::Shape(shape) => {
                    let inside = wp_docx::floating::Turned::of_shape(shape).inside(turned);
                    page.shapes.push(PlacedShape {
                        x: member_left,
                        y: member_top,
                        width: member_width,
                        height: member_height,
                        preset: crate::geometry::Preset::from_word(&shape.preset),
                        adjusts: crate::geometry::Adjusts::from_pairs(&shape.adjusts),
                        head_end: shape.head_end,
                        id: shape.id,
                        joins: shape.joins,
                        route: None,
                        effects: Effects::of(&shape.effects, scale, &self.theme),
                        solid: Solid::of(&shape.depth, &shape.scene, scale, &self.theme),
                        tail_end: shape.tail_end,
                        fill: crate::paint::Paint::of(&shape.fill, &self.theme),
                        outline: self.outline_colour(shape),
                        outline_weight: (shape.outline_points_in(&self.theme) * scale).max(1.0),
                        shadow: self.shape_shadow(scale),
                        text: Vec::new(),
                        text_turned: Vec::new(),
                        name: shape.name.clone(),
                        // The group's place and not the member's: a member has
                        // no place of its own in the text, and a press on one
                        // takes hold of the group, which is what a group is for.
                        at,
                        depth,
                        over_text,
                        source: Some(shape.clone()),
                        turn: inside.radians(),
                        flipped_across: inside.flipped_across,
                        flipped_down: inside.flipped_down,
                    });
                }
                wp_docx::group::Inside::Picture(picture) => {
                    // Decoded when the item was built, where the document was
                    // to hand; here it is only fetched. A picture that would
                    // not decode is drawn as nothing, as one in the line is.
                    let Some(image) = self.decoded(&picture.relationship) else { continue };
                    let inside = picture.turned.inside(turned);
                    page.images.push(PlacedImage {
                        x: member_left,
                        y: member_top,
                        width: member_width,
                        height: member_height,
                        image,
                        depth,
                        over_text,
                        at,
                        turn: inside.radians(),
                        flipped_across: inside.flipped_across,
                        flipped_down: inside.flipped_down,
                        name: if picture.description.as_deref().unwrap_or_default().is_empty() {
                            "Picture".to_owned()
                        } else {
                            picture.description.clone().unwrap_or_default()
                        },
                        video: picture.video,
                    });
                }
                wp_docx::group::Inside::Group(inner) => {
                    self.place_group_turned(page, inner, member_rect, at, depth, over_text, turned);
                }
            }
        }
    }

    /// How big a floating drawing is, once a size stated as a percentage has
    /// had its say.
    ///
    /// A picture at half the page width is half of whatever the page is, which
    /// is the whole point of stating it that way: the absolute size beside it
    /// was right for the paper the document was last written on.
    fn float_size(
        &self,
        anchor: &wp_docx::anchor::Anchor,
        area: &Placement,
        stated: (f32, f32),
    ) -> (f32, f32) {
        use wp_docx::anchor::Relative;

        let (width, height) = stated;

        let across = |from: Relative| match from {
            Relative::Page => area.page_width,
            _ => area.text_width,
        };
        let down = |from: Relative| match from {
            Relative::Page => area.page_height,
            _ => (area.bottom_limit - area.top).max(1.0),
        };

        let width = match anchor.width_of {
            Some(relatively) => across(relatively.from) * share(relatively.thousandths),
            None => width,
        };
        let height = match anchor.height_of {
            Some(relatively) => down(relatively.from) * share(relatively.thousandths),
            None => height,
        };
        (width.max(1.0), height.max(1.0))
    }

    /// Works out where a floating drawing sits and reserves the room round it.
    ///
    /// Everything a shape and a picture have in common, which is all of it but
    /// the drawing itself: where a drawing goes does not depend on what it is a
    /// drawing of. `outline` is the shape to follow for tight wrapping, and is
    /// nothing for a picture — a picture is the box it fills.
    #[allow(clippy::too_many_arguments)]
    fn float_box(
        &mut self,
        anchor: &wp_docx::anchor::Anchor,
        page_index: usize,
        area: &Placement,
        line_top: f32,
        width: f32,
        height: f32,
        outline: Option<(crate::geometry::Preset, crate::geometry::Adjusts)>,
    ) -> (f32, f32) {
        use wp_docx::anchor::{Placement as Where, Relative};

        let scale = self.pixels_per_point();
        let emu = |value: i64| value as f32 / wp_docx::shapes::EMU_PER_POINT as f32 * scale;

        // Across the page. The text area is what "margin" and "column" both
        // mean here: a document of one column has them in the same place, and
        // one of several has the column as the nearer answer.
        //
        // The margins themselves are bands of their own, and the only way to
        // put something *in* a margin rather than against the edge of the
        // text — which is what a page number down the side of a page is.
        let text_right = area.left + area.text_width;
        let (band_left, band_width) = match anchor.horizontal_from {
            Relative::Page => (0.0, area.page_width),
            // Inside and outside are the binding side and the other one, and
            // which is which changes with the page in a document printed on
            // both sides. Read as left and right, which is right for a
            // document printed on one side and for the odd pages of one
            // printed on both.
            Relative::LeftMargin | Relative::InsideMargin => (0.0, area.left),
            Relative::RightMargin | Relative::OutsideMargin => {
                (text_right, (area.page_width - text_right).max(0.0))
            }
            Relative::TopMargin | Relative::BottomMargin => (area.left, area.text_width),
            _ => (area.left, area.text_width),
        };
        let x = match &anchor.horizontal {
            Where::Aligned(edge) => match edge.as_str() {
                "center" => band_left + (band_width - width) / 2.0,
                "right" | "outside" => band_left + band_width - width,
                _ => band_left,
            },
            Where::Offset(distance) => band_left + emu(*distance),
            // A share of the frame rather than a distance into it, which is
            // what keeps a drawing a third of the way across a page of any
            // width. See [`wp_docx::anchor::Placement::Percent`].
            Where::Percent(thousandths) => band_left + band_width * share(*thousandths),
        };

        // Down the page.
        let y = match anchor.vertical_from {
            Relative::Page => {
                self.drawing_shift
                    + match &anchor.vertical {
                        Where::Aligned(edge) => match edge.as_str() {
                            "center" => (area.page_height - height) / 2.0,
                            "bottom" => area.page_height - height,
                            _ => 0.0,
                        },
                        Where::Offset(distance) => emu(*distance),
                        Where::Percent(thousandths) => area.page_height * share(*thousandths),
                    }
            }
            Relative::Margin
            | Relative::LeftMargin
            | Relative::RightMargin
            | Relative::InsideMargin
            | Relative::OutsideMargin => match &anchor.vertical {
                Where::Aligned(edge) => match edge.as_str() {
                    "center" => area.top + (area.bottom_limit - area.top - height) / 2.0,
                    "bottom" => area.bottom_limit - height,
                    _ => area.top,
                },
                Where::Offset(distance) => area.top + emu(*distance),
                Where::Percent(thousandths) => {
                    area.top + (area.bottom_limit - area.top) * share(*thousandths)
                }
            },
            // The bands above and below the text, measured from the top of the
            // page and from the foot of the text respectively.
            Relative::TopMargin => match &anchor.vertical {
                Where::Aligned(edge) => match edge.as_str() {
                    "center" => (area.top - height) / 2.0,
                    "bottom" => area.top - height,
                    _ => 0.0,
                },
                Where::Offset(distance) => emu(*distance),
                Where::Percent(thousandths) => area.top * share(*thousandths),
            },
            Relative::BottomMargin => match &anchor.vertical {
                Where::Aligned(edge) => match edge.as_str() {
                    "center" => {
                        area.bottom_limit + (area.page_height - area.bottom_limit - height) / 2.0
                    }
                    "bottom" => area.page_height - height,
                    _ => area.bottom_limit,
                },
                Where::Offset(distance) => area.bottom_limit + emu(*distance),
                Where::Percent(thousandths) => {
                    area.bottom_limit + (area.page_height - area.bottom_limit) * share(*thousandths)
                }
            },
            // Paragraph and line both mean "from where the text is now", which
            // is what anchors a drawing to the words it belongs with.
            _ => match &anchor.vertical {
                Where::Offset(distance) => line_top + emu(*distance),
                Where::Percent(thousandths) => {
                    line_top + (area.bottom_limit - line_top).max(0.0) * share(*thousandths)
                }
                Where::Aligned(_) => line_top,
            },
        };

        let (left_room, right_room, top_room, bottom_room) = anchor.distance;

        // A drawing that may not overlap is pushed down until it lies clear of
        // the ones already placed. Word's Allow overlap, unticked: two pictures
        // dropped in the same place end up one above the other rather than one
        // on top of the other.
        //
        // Down and not sideways, because down is where a page has room: moving
        // it across would take it out of the text it belongs beside. Each push
        // can uncover a new neighbour, so it is done until nothing is in the
        // way — and counted, because a page crowded with drawings must not
        // become a loop.
        let mut y = y;
        if !anchor.allow_overlap {
            for _ in 0..self.floats.len().min(64) {
                let (top, bottom) = (y - emu(top_room), y + height + emu(bottom_room));
                let (left, right) = (x - emu(left_room), x + width + emu(right_room));
                let Some(under) = self
                    .floats
                    .iter()
                    .filter(|float| float.page == page_index)
                    .filter(|float| float.right > left && float.left < right)
                    .filter(|float| float.bottom > top && float.top < bottom)
                    .map(|float| float.bottom)
                    .fold(None, |most: Option<f32>, bottom| {
                        Some(most.map_or(bottom, |most| most.max(bottom)))
                    })
                else {
                    break;
                };
                y = under + emu(top_room);
            }
        }

        self.floats.push(Float {
            page: page_index,
            left: x - emu(left_room),
            top: y - emu(top_room),
            right: x + width + emu(right_room),
            bottom: y + height + emu(bottom_room),
            wrap: anchor.wrap,
            side: anchor.side,
            // The shape itself, so that tight wrapping can follow its outline
            // rather than the box round it.
            outline: outline.map(|(preset, adjusts)| Outline {
                preset,
                adjusts,
                x,
                y,
                width,
                height,
            }),
        });

        (x, y)
    }

    /// The shadow the theme puts under a shape, in pixels.
    ///
    /// The colour of the line round a shape, against the document's theme.
    fn outline_colour(&self, shape: &wp_docx::shapes::Shape) -> Option<Color> {
        shape.outline.as_ref().and_then(|colour| Color::from_hex(&colour.resolve(&self.theme)))
    }

    /// Word's Design ▸ Effects, which is a property of the document rather than
    /// of any one shape — so it is asked for here and applied to every shape
    /// alike. See [`wp_docx::theme::Effect`].
    fn shape_shadow(&self, scale: f32) -> Option<(Color, f32)> {
        let (distance, alpha) = self.theme.effect.shadow()?;
        // The theme writes it in English metric units; a point is 12,700 of
        // them, and the second effect style is the one a plain shape uses.
        let points = distance as f32 / 12_700.0 * 2.0;
        let share = (alpha as f32 / 100_000.0 * 255.0).clamp(0.0, 255.0) as u8;
        Some((Color::rgba(0, 0, 0, share), (points * scale).max(1.0)))
    }
    /// Puts every joined connector where the shapes it is fastened to are.
    ///
    /// A pass of its own, after everything is placed: a connector may be laid
    /// out before the shapes it joins, and where it goes depends on where they
    /// went. The box it was saved with is only the answer from the last time
    /// anybody worked this out, which is why it is worked out again rather than
    /// believed.
    ///
    /// # Which way round it is drawn
    ///
    /// A connector is drawn from one corner of its box to the opposite one, so
    /// the box alone cannot say which corner is the start. That is what the
    /// flips are for: an end fastened to a shape on the left is drawn from the
    /// left, and one fastened to a shape on the right is the same connector
    /// mirrored.
    fn rejoin_connectors(pages: &mut [Page]) {
        for page in pages.iter_mut() {
            // Where every shape on the page is, by the number the file knows it
            // by. Gathered first because a connector may name a shape that
            // comes after it.
            let boxes: Vec<(u32, f32, f32, f32, f32)> = page
                .shapes
                .iter()
                .filter(|shape| shape.id != 0)
                .map(|shape| (shape.id, shape.x, shape.y, shape.width, shape.height))
                .collect();
            let site_of = |join: wp_docx::joins::Join| {
                boxes.iter().find(|(id, ..)| *id == join.shape).map(|(_, x, y, width, height)| {
                    crate::connectors::connection_site(join.site, *x, *y, *width, *height)
                })
            };

            let box_of = |join: wp_docx::joins::Join| {
                boxes
                    .iter()
                    .find(|(id, ..)| *id == join.shape)
                    .map(|(_, x, y, width, height)| (*x, *y, *x + *width, *y + *height))
            };

            for shape in &mut page.shapes {
                if shape.joins.is_nothing() {
                    continue;
                }
                // Used up here: a page kept from one layout to the next has
                // its connectors joined already, and joining one again from
                // where the first joining left it would move it.
                let joins = core::mem::take(&mut shape.joins);
                // An end fastened to nothing stays where the connector was
                // drawn; one fastened to a shape goes to that shape.
                let corner = wp_raster::Point::new(shape.x, shape.y);
                let far = wp_raster::Point::new(shape.x + shape.width, shape.y + shape.height);
                let start = joins.start.and_then(site_of).unwrap_or(corner);
                let end = joins.end.and_then(site_of).unwrap_or(far);

                shape.x = start.x.min(end.x);
                shape.y = start.y.min(end.y);
                shape.width = (end.x - start.x).abs();
                shape.height = (end.y - start.y).abs();
                shape.flipped_across = end.x < start.x;
                shape.flipped_down = end.y < start.y;

                // And how it gets from the one to the other. A straight
                // connector goes straight, whatever is in the way; an elbow or
                // a curve is laid out leg by leg so that it leaves each shape
                // by the side it is fastened to and crosses neither.
                let bent = !matches!(
                    shape.preset,
                    crate::geometry::Preset::Line | crate::geometry::Preset::StraightConnector
                );
                let (Some(one), Some(two)) = (joins.start, joins.end) else {
                    continue;
                };
                let (Some(from_box), Some(to_box)) = (box_of(one), box_of(two)) else {
                    continue;
                };
                if !bent {
                    continue;
                }
                let stand_off = shape.outline_weight.max(1.0) * 6.0;
                let places = crate::connectors::route(
                    crate::connectors::Place {
                        at: start,
                        faces: crate::connectors::Facing::of_site(one.site),
                        shape: from_box,
                    },
                    crate::connectors::Place {
                        at: end,
                        faces: crate::connectors::Facing::of_site(two.site),
                        shape: to_box,
                    },
                    stand_off,
                );
                let curved = matches!(
                    shape.preset,
                    crate::geometry::Preset::CurvedConnector2
                        | crate::geometry::Preset::CurvedConnector3
                        | crate::geometry::Preset::CurvedConnector4
                        | crate::geometry::Preset::CurvedConnector5
                );
                shape.route = Some(crate::connectors::route_path(&places, curved));
                // The flips say which corner of the box a preset starts from,
                // and a route says where every corner of it goes: a route drawn
                // mirrored would be drawn somewhere neither shape is. They stay
                // on the shape in the file, where a preset is still what Word
                // draws it from.
                shape.flipped_across = false;
                shape.flipped_down = false;
                // A route may go outside the two points it joins, and the box
                // is what the rest of the program believes about where a
                // drawing is. Widened to hold the whole of it, or half a
                // connector would be outside what anybody could take hold of.
                for at in &places {
                    let right = (shape.x + shape.width).max(at.x);
                    let bottom = (shape.y + shape.height).max(at.y);
                    shape.x = shape.x.min(at.x);
                    shape.y = shape.y.min(at.y);
                    shape.width = right - shape.x;
                    shape.height = bottom - shape.y;
                }
            }
        }
    }

    /// Lays out the text inside every shape on every page.
    ///
    /// A pass of its own, after the body: the text in a shape is a document in
    /// a smaller box, and laying one out while the outer one is still being
    /// laid out would be the engine calling itself.
    fn fill_shapes(&mut self, pages: &mut [Page], document: &Document) {
        // A shape's text is inset from its edges by this much, which is the
        // margin Word leaves inside a text box: a tenth of an inch at the
        // sides and half that above and below.
        const INSET_X: f32 = 7.2;
        const INSET_Y: f32 = 3.6;

        for page in pages.iter_mut() {
            for shape in &mut page.shapes {
                let Some(source) = shape.source.take() else { continue };
                if !source.has_text() {
                    continue;
                }

                let scale = self.pixels_per_point();
                let inset_x = INSET_X * scale;
                let inset_y = INSET_Y * scale;
                let width = (shape.width / scale - INSET_X * 2.0).max(1.0);
                let height = (shape.height / scale - INSET_Y * 2.0).max(1.0);
                // Tall enough that the text never runs off the end of it: what
                // does not fit in a shape is hidden by the shape's edge, not
                // carried onto a second one. A text box whose words run down
                // it is the exception: its box is laid out on its side and
                // turned, the way a section written down the page is, and
                // what runs off its end is what runs off its edge.
                let direction = source.direction;
                let metrics = PageMetrics {
                    width,
                    height: if direction.is_turned() { height } else { f32::MAX / 4.0 },
                    margin_top: 0.0,
                    margin_right: 0.0,
                    margin_bottom: 0.0,
                    margin_left: 0.0,
                    columns: 1,
                    column_gap: 0.0,
                    direction,
                };

                // The words that name no colour of their own take the one the
                // shape names for them — white on a gallery shape — which is
                // the automatic colour while they are laid out.
                let automatic = self.automatic_color;
                if let Some(ink) =
                    source.ink.as_ref().and_then(|ink| Color::from_hex(&ink.resolve(&self.theme)))
                {
                    self.automatic_color = ink;
                }
                let mut inner = self.layout_body(&source.body(), document, metrics);
                self.automatic_color = automatic;
                if inner.is_empty() {
                    continue;
                }
                let first = &mut inner[0];
                if direction.is_turned() {
                    let (turned_width, turned_height) = (first.height, first.width);
                    first.turn(metrics.frame_on(turned_width, turned_height));
                }
                shape.text_turned = spans_of((0..first.glyphs.len()).map(|at| first.turn_of(at)));
                shape.text = first
                    .glyphs
                    .iter()
                    .map(|glyph| PositionedGlyph {
                        x: glyph.x + shape.x + inset_x,
                        baseline: glyph.baseline + shape.y + inset_y,
                        ..*glyph
                    })
                    .collect();
            }
        }
    }
    /// Chooses a face for a family, bold or not, italic or not.
    ///
    /// A family the machine does not have is looked up in the document's
    /// font table: the other name it goes by is tried, and failing that a
    /// face of the same kind stands in — one with serifs for one with serifs,
    /// a typewriter's for a typewriter's. See [`wp_docx::fonts`].
    fn choose_face(&self, family: Option<&str>, bold: bool, italic: bool) -> Option<usize> {
        use wp_docx::fonts::FontClass;
        let entry = family
            .filter(|wanted| !self.library.has_family(wanted))
            .and_then(|wanted| self.font_table.get(&wanted.to_lowercase()));
        let Some(entry) = entry else {
            return self.library.select(family, bold, italic);
        };
        if let Some(alt) = entry.alt_name.as_deref().filter(|alt| self.library.has_family(alt)) {
            return self.library.select(Some(alt), bold, italic);
        }
        let like = match (entry.class, entry.fixed_pitch) {
            (_, Some(true)) | (FontClass::Modern, _) => crate::library::Likeness::Mono,
            (FontClass::Roman, _) => crate::library::Likeness::Serif,
            _ => crate::library::Likeness::Sans,
        };
        self.library.default_face_like(like, bold, italic)
    }

    /// Chooses a face and works out the metrics for one set of run properties.
    fn style_for(&mut self, properties: &ResolvedRunProperties) -> Option<RunStyle> {
        let face =
            self.choose_face(properties.font.as_deref(), properties.bold, properties.italic)?;

        // A superscript is set smaller and lifted, a subscript smaller and
        // dropped. Word uses about two thirds of the size and a third of the
        // height, and those proportions are what make it look like typesetting
        // rather than a shrunken letter parked next to the line.
        let full_size = properties.size_points() as f32 * self.pixels_per_point();
        let (size, raise_fraction) = match properties.vertical_align {
            VerticalAlignment::Baseline => (full_size, 0.0),
            VerticalAlignment::Superscript => (full_size * 0.66, 0.34),
            VerticalAlignment::Subscript => (full_size * 0.66, -0.16),
        };
        let font = self.font(face)?;
        let units = f32::from(font.units_per_em());
        let metrics = font.vertical_metrics();

        let ascent = f32::from(metrics.ascender) * size / units;
        let descent = -f32::from(metrics.descender) * size / units;
        let line_height = metrics.line_height() as f32 * size / units;

        // A document that names no colour means "whatever reads against the
        // paper", which is not always black — in a dark theme the paper is dark
        // and so the automatic colour is light.
        let color =
            properties.color.as_deref().and_then(Color::from_hex).unwrap_or(self.automatic_color);
        // A reflection is the text turned over, so it takes the text's colour;
        // the others have one of their own.
        let effect = properties.effect.as_ref().map(|wanted| GlyphEffect {
            kind: wanted.effect,
            color: wanted.color.as_deref().and_then(Color::from_hex).unwrap_or(color),
        });

        // The Advanced tab of Word's Font dialog, in the units the layout works
        // in. Each is stored differently; see [`wp_docx::typography`].
        let stretch = properties.scale as f32 / 100.0;
        let letter_spacing =
            properties.spacing_twentieths as f32 / 20.0 * self.pixels_per_point() * stretch;
        // Raised or lowered without being made smaller, which is what separates
        // this from a superscript. It rides on top of whatever the vertical
        // alignment already did.
        let lifted = properties.position_half_points as f32 / 2.0 * self.pixels_per_point();
        // A threshold rather than a switch: kerning is used at or above the
        // size the document names. A document that names none gets kerning,
        // because that is what this has always drawn.
        let kern = properties
            .kerning_half_points
            .is_none_or(|from| from > 0 && properties.size_half_points >= from);

        Some(RunStyle {
            face,
            size,
            color,
            effect,
            highlight: properties.highlight.as_deref().and_then(highlight_color),
            underline: properties.underline.is_visible(),
            underline_color: properties.underline_color.as_deref().and_then(Color::from_hex),
            strike: properties.strike,
            double_strike: properties.double_strike,
            right_to_left: properties.right_to_left,
            // Which language's rules its capitals follow, taken from the same
            // tag the proofing tools read.
            casing: wp_docx::casing::Tailoring::of(
                properties.language.as_deref().unwrap_or_default(),
            ),
            hyphenation: self.patterns_for(
                properties.language.as_deref().unwrap_or(wp_docx::languages::DEFAULT_TAG),
            ),
            caps: if properties.small_caps {
                Caps::Small
            } else if properties.caps {
                Caps::All
            } else {
                Caps::None
            },
            stretch,
            letter_spacing,
            kern,
            hidden: properties.hidden && !self.show_marks,
            features: Rc::new(properties.open_type.features()),
            raise: full_size * raise_fraction + lifted,
            // The line keeps the height of full-sized text, so a superscript
            // does not make its line shorter than the ones around it.
            ascent: ascent.max(full_size * 0.8),
            descent,
            line_height: line_height.max(full_size * 1.2),
            east_asian: properties.east_asian_layout,
        })
    }

    /// The patterns for a language, looked for on the machine once and kept.
    fn patterns_for(&mut self, language: &str) -> Option<Rc<wp_dict::hyphenation::Patterns>> {
        if !self.hyphenation.automatic {
            return None;
        }
        if let Some(found) = self.patterns.get(language) {
            return found.clone();
        }
        let found = wp_dict::hyphenation::for_language(language).map(Rc::new);
        self.patterns.insert(language.to_owned(), found.clone());
        found
    }

    /// Lays a word out with its reading over it.
    ///
    /// Both halves are shaped through the same machinery as any other text —
    /// the same fonts, the same rules, the same marks — and only then put one
    /// over the other. A reading shaped a second way would drift from the
    /// words beside it.
    fn ruby_box(
        &mut self,
        ruby: &wp_docx::ruby::Ruby,
        style: &RunStyle,
        base_offset: usize,
        paragraph: usize,
    ) -> crate::ruby::RubyBox {
        let scale = self.pixels_per_point();

        // Each half is set at the size its own runs ask for, and at what the
        // ruby's properties say when they ask for nothing: the properties
        // describe the pair and the runs are what is drawn.
        let mut lower = style.clone();
        if let Some(half_points) = own_size(&ruby.base).or(ruby.properties.base_size_half_points) {
            lower.size = half_points as f32 / 2.0 * scale;
        }
        let word = self.ruby_half(&ruby.plain_text(), &lower, base_offset, paragraph);

        // The reading is set smaller: at the size it asks for, and at half the
        // word's when it asks for nothing, which is the proportion Word uses.
        let mut smaller = style.clone();
        smaller.size = match own_size(&ruby.annotation).or(ruby.properties.size_half_points) {
            Some(half_points) => half_points as f32 / 2.0 * scale,
            None => lower.size / 2.0,
        };
        // Every glyph of the reading points at the word it belongs to, so a
        // press on the reading puts the caret beside the word rather than
        // inside something the document does not have.
        let reading = self.ruby_half(&ruby.reading(), &smaller, base_offset, paragraph);

        // How far above the line it sits. The file says it in half-points from
        // the baseline; a file that says nothing gets the word's own height,
        // which is where a reading clears the letters under it.
        let raise = match ruby.properties.raise_half_points {
            Some(half_points) => (half_points as f32 / 2.0 * scale).max(word.ascent),
            None => word.ascent,
        };
        crate::ruby::lay_out(word, reading, ruby.properties.align, raise)
    }

    /// One half of a ruby, shaped and measured from nothing.
    fn ruby_half(
        &mut self,
        text: &str,
        style: &RunStyle,
        base_offset: usize,
        paragraph: usize,
    ) -> crate::ruby::Half {
        let shaped = self.shape(text, style, base_offset);
        let (ascent, descent) = self.text_extents(style.face, style.size);

        let mut glyphs = Vec::with_capacity(shaped.len());
        let mut x = 0.0f32;
        for glyph in &shaped {
            glyphs.push(PositionedGlyph {
                face: glyph.face,
                glyph: glyph.glyph,
                x: x + glyph.x_offset,
                baseline: -glyph.y_offset,
                advance: glyph.advance,
                size: glyph.size,
                stretch: style.stretch,
                color: style.color,
                effect: style.effect,
                source: TextPosition::new(paragraph, glyph.offset),
                source_length: glyph.length,
                invisible: style.hidden || glyph.invisible,
                shift_x: 0.0,
                shift_y: 0.0,
            });
            x += glyph.advance;
        }
        crate::ruby::Half { glyphs, width: x, ascent, descent }
    }

    /// Lays an equation out at the size of the run it sits in.
    fn math_box(&mut self, math: &wp_docx::math::Math, style: &RunStyle) -> crate::math::MathBox {
        let mut shaper = EngineShaper { engine: self, face: style.face };
        crate::math::layout(&mut shaper, math, style.size, style.color)
    }

    /// Lays out the chart a drawing points at, if the document has one.
    fn chart_drawing(
        &mut self,
        reference: &wp_docx::model::ChartReference,
        document: &Document,
        width: f32,
        height: f32,
    ) -> Option<crate::charting::ChartDrawing> {
        let chart = document.chart(&reference.relationship)?;
        let palette = self.chart_palette(document);
        let face = self.library.default_face(false, false)?;
        let mut shaper = LabelShaper { engine: self, face };
        Some(crate::charting::draw(&mut shaper, &chart, 0.0, 0.0, width, height, &palette))
    }

    /// The colours a chart is drawn in: the document's theme accents, and the
    /// colour text reads in against this paper.
    fn chart_palette(&self, document: &Document) -> crate::charting::Palette {
        use wp_docx::theme::Slot;

        let theme = document.theme();
        let accents = [
            Slot::Accent1,
            Slot::Accent2,
            Slot::Accent3,
            Slot::Accent4,
            Slot::Accent5,
            Slot::Accent6,
        ]
        .iter()
        .filter_map(|slot| Color::from_hex(&theme.color(*slot)))
        .collect();

        crate::charting::Palette { accents, line: self.automatic_line, text: self.automatic_color }
    }
}

/// Lets the chart drawing ask the engine to place a label.
struct LabelShaper<'engine, 'library> {
    engine: &'engine mut LayoutEngine<'library>,
    face: usize,
}

impl crate::charting::ChartShaper for LabelShaper<'_, '_> {
    fn shape_label(
        &mut self,
        text: &str,
        x: f32,
        baseline: f32,
        size: f32,
        color: Color,
    ) -> (Vec<PositionedGlyph>, f32) {
        let style = RunStyle::plain(self.face, size, color);

        let shaped = self.engine.shape(text, &style, 0);
        let mut glyphs = Vec::with_capacity(shaped.len());
        let mut at = x;
        for glyph in &shaped {
            glyphs.push(PositionedGlyph {
                face: glyph.face,
                glyph: glyph.glyph,
                x: at,
                baseline,
                advance: glyph.advance,
                size,
                stretch: 1.0,
                color,
                effect: None,
                // A chart is one character of the document, so nothing inside
                // it points at a place of its own.
                source: wp_docx::TextPosition::new(0, 0),
                source_length: 0,
                invisible: false,
                shift_x: 0.0,
                shift_y: 0.0,
            });
            at += glyph.advance;
        }
        (glyphs, at - x)
    }
}

impl<'a> LayoutEngine<'a> {
    /// How far letters of a size reach above and below the baseline.
    fn text_extents(&mut self, face: usize, size: f32) -> (f32, f32) {
        let Some(font) = self.font(face) else { return (size * 0.8, size * 0.2) };
        let units = f32::from(font.units_per_em());
        let metrics = font.vertical_metrics();
        let ascent = f32::from(metrics.ascender) * size / units;
        let descent = -f32::from(metrics.descender) * size / units;
        (ascent.max(size * 0.6), descent.max(size * 0.1))
    }
}

/// Lets the equation layout ask the engine to shape a piece of text.
///
/// The engine owns the fonts, and the equation layout owns the arithmetic. This
/// is the seam between them, kept narrow on purpose: everything the one needs
/// from the other is a run of letters turned into glyphs.
struct EngineShaper<'engine, 'library> {
    engine: &'engine mut LayoutEngine<'library>,
    /// The face the surrounding text is set in, which is the face an equation
    /// is set in too — Word uses Cambria Math, and a document that does not
    /// have it falls back to the text font, which is what happens here always.
    face: usize,
}

impl crate::math::MathShaper for EngineShaper<'_, '_> {
    fn shape_math(
        &mut self,
        text: &str,
        size: f32,
        color: Color,
    ) -> (Vec<PositionedGlyph>, f32, f32, f32) {
        // A style of the right size, built here rather than passed in: every
        // level of an equation is set smaller than the one above it.
        let style = RunStyle::plain(self.face, size, color);

        let shaped = self.engine.shape(text, &style, 0);
        let mut glyphs = Vec::with_capacity(shaped.len());
        let mut x = 0.0;
        for glyph in &shaped {
            glyphs.push(PositionedGlyph {
                face: glyph.face,
                glyph: glyph.glyph,
                x,
                baseline: 0.0,
                advance: glyph.advance,
                size,
                stretch: 1.0,
                color,
                effect: None,
                // An equation is one character of the document however many
                // letters are drawn for it, so no glyph inside it points at a
                // place of its own.
                source: wp_docx::TextPosition::new(0, 0),
                source_length: 0,
                invisible: false,
                shift_x: 0.0,
                shift_y: 0.0,
            });
            x += glyph.advance;
        }

        // Measured from the font rather than guessed, so a tall letter and a
        // short one both sit right.
        let (ascent, descent) = self.engine.text_extents(self.face, size);
        (glyphs, x, ascent, descent)
    }
}

impl<'a> LayoutEngine<'a> {
    /// Parses a face, keeping it for later.
    fn font(&mut self, face: usize) -> Option<&Font<'a>> {
        if !self.fonts.contains_key(&face) {
            let parsed = self.library.face(face)?.font()?;
            self.fonts.insert(face, parsed);
        }
        self.fonts.get(&face)
    }

    /// Rule L4: the characters that are drawn mirrored where the line reads
    /// right to left.
    ///
    /// A bracket is a role, not a shape. The document holds the bracket that
    /// opens the phrase, and in Hebrew or Arabic the one that opens it is drawn
    /// as what a Latin reader calls a closing bracket. The same goes for the
    /// comparisons and the guillemets. Only the drawing changes: the document
    /// still holds what was typed, so the caret and a copy are unaffected.
    /// The glyphs of a line of text standing on its own, in the order they
    /// are drawn from the left.
    ///
    /// By the whole of the bidirectional algorithm, where the line had been
    /// turned round whenever it began in a script read from the right. A
    /// line of Hebrew with a number in it, or a word of English, or a
    /// bracket, is not that line backwards: the number keeps its digits in
    /// order and the word its letters — page 12 of 40 does not say 21 and
    /// 04 — and a bracket in a stretch read from the right is drawn as the
    /// other end of its pair, as on the page. See [`Self::mirror_glyphs`].
    fn in_drawing_order(
        &mut self,
        text: &str,
        mut glyphs: Vec<ShapedGlyph>,
        style: &RunStyle,
        direction: wp_bidi::Direction,
    ) -> Vec<ShapedGlyph> {
        let levels = wp_bidi::levels(text, direction);
        let base = u8::from(direction.is_right_to_left());
        let glyph_levels: Vec<u8> =
            glyphs.iter().map(|glyph| levels.get(glyph.offset).copied().unwrap_or(base)).collect();
        for (glyph, level) in glyphs.iter_mut().zip(&glyph_levels) {
            if level % 2 == 0 {
                continue;
            }
            let Some(mirror) = wp_bidi::mirrored(glyph.character) else { continue };
            let mut buffer = [0u8; 4];
            let drawn = self.shape(mirror.encode_utf8(&mut buffer), style, glyph.offset);
            if let Some(chosen) = drawn.first() {
                *glyph = ShapedGlyph {
                    face: chosen.face,
                    glyph: chosen.glyph,
                    advance: chosen.advance,
                    ..*glyph
                };
            }
        }
        wp_bidi::reorder(&glyph_levels).into_iter().map(|at| glyphs[at]).collect()
    }

    fn mirror_glyphs(&mut self, items: &mut [Item], levels: &[u8], styles: &[RunStyle]) {
        for (item, level) in items.iter_mut().zip(levels) {
            if level % 2 == 0 {
                continue;
            }
            let Some(style) = styles.get(item.style).cloned() else { continue };
            for index in 0..item.glyphs.len() {
                let glyph = item.glyphs[index];
                let Some(mirror) = wp_bidi::mirrored(glyph.character) else { continue };

                let mut buffer = [0u8; 4];
                let drawn = self.shape(mirror.encode_utf8(&mut buffer), &style, glyph.offset);
                let Some(chosen) = drawn.first() else { continue };

                // The pair are usually the same width, but nothing promises it,
                // and a line that has been measured must stay measured.
                item.width += chosen.advance - glyph.advance;
                item.glyphs[index] =
                    ShapedGlyph { face: chosen.face, glyph: chosen.glyph, ..glyph };
                item.glyphs[index].advance = chosen.advance;
            }
        }
    }

    /// Where a run changes between text and emoji.
    ///
    /// An emoji is taken from a font that draws it in colour whatever the run
    /// asks for, so a run holding one is really several runs: shaping is a
    /// question about one font, and this says where one font's stretch ends.
    /// Empty when the run holds no emoji at all, which is nearly every run.
    fn emoji_pieces(&self, text: &str) -> Vec<(core::ops::Range<usize>, bool)> {
        let mut pieces: Vec<(core::ops::Range<usize>, bool)> = Vec::new();
        let mut any = false;

        for (at, character) in text.char_indices() {
            // A variation selector belongs to the character before it, and says
            // which of its two faces the writer meant.
            let as_emoji = if matches!(character, '\u{FE0F}' | '\u{FE0E}') {
                pieces.last().is_some_and(|(_, held)| *held)
            } else {
                let after = text[at + character.len_utf8()..].chars().next();
                let wanted = match after {
                    Some('\u{FE0F}') => true,
                    Some('\u{FE0E}') => false,
                    _ => wp_segment::drawn_as_emoji(character),
                };
                wanted && self.library.colour_face_for(character).is_some()
            };
            any |= as_emoji;

            match pieces.last_mut() {
                Some((range, held)) if *held == as_emoji => range.end = at + character.len_utf8(),
                _ => pieces.push((at..at + character.len_utf8(), as_emoji)),
            }
        }

        // Nothing to say where a run holds no emoji at all, which is nearly
        // every run. One piece is still worth saying where that piece is the
        // emoji: it is drawn from another font than the one the run asked for.
        if !any {
            return Vec::new();
        }
        pieces
    }

    /// A stretch of emoji, taken from whichever font draws them in colour.
    ///
    /// Nothing is shaped: an emoji is one character and one picture, and the
    /// variation selector that asked for the picture draws nothing itself.
    fn shape_emoji(
        &mut self,
        text: &str,
        style: &RunStyle,
        base_offset: usize,
    ) -> Vec<ShapedGlyph> {
        let mut glyphs = Vec::new();
        for (at, character) in text.char_indices() {
            let selector = matches!(character, '\u{FE0F}' | '\u{FE0E}');
            let found = if selector {
                None
            } else {
                self.library.colour_face_for(character).and_then(|(face, glyph)| {
                    let font = self.font(face)?;
                    let units = f32::from(font.units_per_em());
                    let advance = f32::from(font.advance(glyph)) * style.size / units;
                    Some((face, glyph, advance))
                })
            };
            let (face, glyph, advance) = found.unwrap_or((style.face, GlyphId(0), 0.0));

            glyphs.push(ShapedGlyph {
                face,
                glyph,
                advance,
                x_offset: 0.0,
                y_offset: 0.0,
                invisible: selector,
                offset: base_offset + at,
                length: character.len_utf8(),
                character,
                size: style.size,
                upright: None,
                squeeze: 1.0,
            });
        }
        glyphs
    }

    /// Turns text into glyphs, falling back to another font per character when
    /// the chosen one has no glyph for it.
    fn shape(&mut self, text: &str, style: &RunStyle, base_offset: usize) -> Vec<ShapedGlyph> {
        // An emoji comes from a colour font whatever the run asks for, so a
        // run holding one is shaped in pieces: the emoji, and the text either
        // side of them. A piece with no emoji in it splits no further.
        let pieces = self.emoji_pieces(text);
        if !pieces.is_empty() {
            let mut out = Vec::new();
            for (range, as_emoji) in pieces {
                let piece = &text[range.clone()];
                let at = base_offset + range.start;
                out.extend(if as_emoji {
                    self.shape_emoji(piece, style, at)
                } else {
                    self.shape(piece, style, at)
                });
            }
            return out;
        }

        // Text in a script that is written joined has to be shaped as a whole:
        // which glyph a letter takes depends on its neighbours, so it cannot be
        // decided a character at a time. So does a run that asked the font for
        // one of its own alternate forms — a ligature is two letters becoming
        // one, and one letter at a time cannot see the second.
        //
        // Not a run drawn in capitals, though: what is drawn there is not what
        // is stored, and a ligature made of what is drawn would point at the
        // wrong characters.
        // And so does a run with a mark in it, and one in a script that is not
        // drawn in the order it is written: where an accent goes, and which
        // letter of a syllable comes first, are both invisible a character at
        // a time. See [`wp_shape::needs_shaping`].
        let joined = text.chars().any(wp_shape::is_joining_script);
        let whole = wp_shape::needs_shaping(text);
        if whole || (!style.features.is_empty() && style.caps == Caps::None) {
            if let Some(glyphs) = self.shape_joined(text, style, base_offset, joined) {
                return hide_optional_hyphens(glyphs);
            }
        }

        let mut glyphs = Vec::with_capacity(text.len());
        let mut previous: Option<GlyphId> = None;

        for (local, source) in text.char_indices() {
            // What is drawn is not always what is stored: `w:caps` and
            // `w:smallCaps` draw a capital where the document holds a small
            // letter, and leave the document alone. A letter whose capital is
            // several characters — the German ß is SS — draws several glyphs,
            // and every one of them points back at the one character it came
            // from, so the caret still lands where the text says it should.
            // A variation selector after a character says which of its two
            // faces the writer meant: U+FE0F the coloured picture, U+FE0E the
            // letter. Without one, the character's own default decides — a
            // rocket is a picture and a bare heart is punctuation.
            let after = text[local + source.len_utf8()..].chars().next();
            let as_emoji = match after {
                Some('\u{FE0F}') => true,
                Some('\u{FE0E}') => false,
                _ => wp_segment::drawn_as_emoji(source),
            };

            for (character, size) in drawn_as(source, style) {
                let mut chosen = None;

                // An emoji is drawn from a font that draws it in colour
                // wherever the machine has one, whatever face the run asks
                // for: the Latin fonts hold monochrome outlines for many of
                // them, and a black rocket is not what anybody meant.
                if as_emoji {
                    if let Some((face, glyph)) = self.library.colour_face_for(character) {
                        if let Some(font) = self.font(face) {
                            let units = f32::from(font.units_per_em());
                            let advance = f32::from(font.advance(glyph)) * size / units;
                            chosen = Some((face, glyph, advance));
                        }
                    }
                }

                if chosen.is_none() {
                    if let Some(font) = self.font(style.face) {
                        if let Some(glyph) = font.glyph_for(character) {
                            let units = f32::from(font.units_per_em());
                            let mut advance = f32::from(font.advance(glyph)) * size / units;
                            if let Some(previous) = previous.filter(|_| style.kern) {
                                // The positioning table first and the old one
                                // only if it says nothing: a font that carries
                                // both means the same thing twice.
                                let by = wp_shape::kerning_between(
                                    font,
                                    &wp_shape::script_of(text),
                                    previous,
                                    glyph,
                                );
                                advance += by as f32 * size / units;
                            }
                            chosen = Some((style.face, glyph, advance));
                        }
                    }
                }

                if chosen.is_none() {
                    // The face cannot draw this character. Another one may be
                    // able to, and an empty box helps nobody.
                    if let Some((face, glyph)) = self.library.fallback_for(character, false, false)
                    {
                        if let Some(font) = self.font(face) {
                            let units = f32::from(font.units_per_em());
                            let advance = f32::from(font.advance(glyph)) * size / units;
                            chosen = Some((face, glyph, advance));
                        }
                    }
                }

                match chosen {
                    Some((face, glyph, advance)) => {
                        glyphs.push(ShapedGlyph {
                            face,
                            glyph,
                            x_offset: 0.0,
                            y_offset: 0.0,
                            invisible: false,
                            // Letters drawn wider take up more room, and the
                            // room asked for between them is the same width
                            // wherever it is measured. Hidden text takes up
                            // none: it is not on the page at all.
                            advance: if style.hidden {
                                0.0
                            } else {
                                advance * style.stretch + style.letter_spacing
                            },
                            offset: base_offset + local,
                            character,
                            length: source.len_utf8(),
                            size,
                            upright: None,
                            squeeze: 1.0,
                        });
                        previous = Some(glyph);
                    }
                    None => previous = None,
                }
            }
        }

        // Which way round the glyphs go is not decided here: it belongs to the
        // paragraph, not the run, and the bidirectional algorithm settles it
        // once the whole paragraph is known. See [`wp_bidi`].
        let mut glyphs = hide_optional_hyphens(glyphs);
        if self.vertical {
            self.stand_upright(&mut glyphs, style);
        }
        glyphs
    }

    /// Sets the letters that stand upright in a line running down the page
    /// upright: the ideographs, the kana, hangul, and the marks that have a
    /// form of their own for that way round.
    ///
    /// Three things change for such a glyph, and only for such a glyph — a
    /// Latin word in the same line lies on its side and is left exactly as it
    /// was. The font is asked for the glyph's vertical form, which is where a
    /// full stop moves to the top right of its square and a bracket turns to
    /// open downwards. The room it takes along the line is what the font
    /// gives it downwards rather than across, which is its square. And where
    /// it is drawn is worked out from where the pen stands: the pen walks
    /// down what will be the turned baseline, and the letter has to stand
    /// centred on the column beside it. See [`Turn::Upright`] for the shift.
    fn stand_upright(&mut self, glyphs: &mut [ShapedGlyph], style: &RunStyle) {
        use wp_shape::vertical::Orientation;
        for glyph in glyphs.iter_mut() {
            let orientation = Orientation::of(glyph.character);
            if orientation == Orientation::Rotated || glyph.invisible {
                continue;
            }
            let Some(font) = self.font(glyph.face) else { continue };
            let units = f32::from(font.units_per_em());
            let scale = glyph.size / units;

            let mut stands = orientation.is_upright();
            if orientation.wants_vertical_form() {
                let script = wp_shape::script_of(&glyph.character.to_string());
                if let Some(form) = wp_shape::vertical_form(font, &script, glyph.glyph) {
                    glyph.glyph = form;
                    stands = true;
                }
            }
            if !stands {
                continue;
            }

            // The square, and where the pen's baseline runs through it.
            let (top, bottom) = font.ideographic_extent();
            let (top, bottom) = (f32::from(top) * scale, -f32::from(bottom) * scale);
            let origin = f32::from(font.vertical_origin(glyph.glyph)) * scale;
            let width = f32::from(font.advance(glyph.glyph)) * scale * style.stretch;
            let down = f32::from(font.vertical_advance(glyph.glyph)) * scale;

            glyph.advance = if style.hidden { 0.0 } else { down + style.letter_spacing };
            glyph.upright = Some((origin, width / 2.0 - (top - bottom) / 2.0));
            glyph.x_offset = 0.0;
            glyph.y_offset = 0.0;
        }
    }

    /// Shapes text in a joined script, if a face can be found that knows how.
    ///
    /// `None` when no face on this machine carries the rules — the caller then
    /// falls back to a glyph per character, which is wrong for Arabic but is
    /// still the letters in the right order.
    fn shape_joined(
        &mut self,
        text: &str,
        style: &RunStyle,
        base_offset: usize,
        joined: bool,
    ) -> Option<Vec<ShapedGlyph>> {
        // The run's own face first: a document that asks for a font gets it.
        // Failing that, whichever face can draw the first letter, because a
        // face without the letters cannot have the rules for joining them.
        let face = self.face_for_joining(text, style)?;
        let font = self.font(face)?;
        let units = f32::from(font.units_per_em());

        let shaped = if joined {
            wp_shape::shape(font, text)
        } else {
            // A Latin run gets only what the document asked the font for, and
            // nothing else: a person who turned on tabular figures did not ask
            // for ligatures as well.
            wp_shape::shape_with(font, text, &style.features)
        };
        if shaped.is_empty() {
            return Some(Vec::new());
        }

        let mut glyphs = Vec::with_capacity(shaped.len());
        for (index, entry) in shaped.iter().enumerate() {
            if entry.glyph.0 == 0 {
                // The font has nothing for this character; the fallback path
                // will do better than an empty box.
                return None;
            }
            // The glyph's own width, and whatever the font's positioning rules
            // added to it: kerning arrives here.
            let scale = style.size / units;
            let advance = (f32::from(font.advance(entry.glyph)) + entry.x_advance as f32) * scale;
            // A glyph reaches to wherever the next one starts, so a ligature
            // covers every character that went into it.
            let length = shaped
                .get(index + 1)
                .map_or(text.len(), |next| next.cluster)
                .saturating_sub(entry.cluster);

            glyphs.push(ShapedGlyph {
                face,
                glyph: entry.glyph,
                advance: advance * style.stretch + style.letter_spacing,
                x_offset: entry.x_offset as f32 * scale * style.stretch,
                y_offset: entry.y_offset as f32 * scale,
                invisible: false,
                offset: base_offset + entry.cluster,
                character: text[entry.cluster..].chars().next().unwrap_or('\u{FFFD}'),
                length: length.max(1),
                size: style.size,
                upright: None,
                squeeze: 1.0,
            });
        }

        // In the order they are stored, not the order they are drawn: which way
        // round a piece of text goes is settled once for the whole paragraph,
        // by the bidirectional algorithm. See [`wp_bidi`].
        Some(glyphs)
    }

    /// Word's Asian Layout for one run: the run set across a line that runs
    /// down the page, or set as two lines in one. Either makes the run one
    /// item that no line breaks inside, with each glyph told where it is
    /// drawn from where the pen stands. See [`wp_docx::eastasian`].
    ///
    /// Whether anything was made: nothing when the run asks for neither, and
    /// the caller then lays it out as words.
    fn east_asian_run(
        &mut self,
        shown: &str,
        stored: &str,
        style: &RunStyle,
        style_index: usize,
        items: &mut Vec<Item>,
        offset: &mut usize,
    ) -> bool {
        let layout = style.east_asian;
        let across = layout.horizontal_in_vertical && self.vertical;
        if (!across && !layout.two_lines_in_one) || shown.is_empty() {
            return false;
        }
        let start = *offset;
        *offset += stored.len();
        let glyphs = if across {
            self.across_the_line(shown, style, start)
        } else {
            self.two_lines_in_one(shown, style, start)
        };
        let width = glyphs.iter().map(|glyph| glyph.advance).sum();
        items.push(Item {
            glyphs,
            width,
            is_space: false,
            breaks_before: true,
            is_tab: false,
            aligned_tab: None,
            picture: None,
            picture_anchor: None,
            picture_name: None,
            picture_turn: wp_docx::floating::Turned::default(),
            picture_video: false,
            group: None,
            shape: None,
            math: None,
            chart: None,
            ink: None,
            ruby: None,
            hyphen: 0.0,
            auto_hyphen: false,
            hard_break: None,
            style: style_index,
            start_offset: start,
            end_offset: *offset,
        });
        true
    }

    /// Horizontal in Vertical: the run stands upright and reads across,
    /// inside a line that runs down the page.
    ///
    /// The run is shaped as the horizontal run it is, and then set on its
    /// side in the box — which is upright on the page — centred on the
    /// column, in the room of one square along the line. Squeezed to that
    /// square across as well when the run asks to fit in the line and is
    /// wider than it. The pen walks the square in equal steps, one per
    /// glyph, so that the caret has somewhere to stand between them.
    fn across_the_line(&mut self, text: &str, style: &RunStyle, start: usize) -> Vec<ShapedGlyph> {
        // Shaped as a horizontal run, which is what it is: the pass that
        // stands letters upright is not for it.
        let was = core::mem::replace(&mut self.vertical, false);
        let mut glyphs = self.shape(text, style, start);
        self.vertical = was;
        let Some(first) = glyphs.first().copied() else { return glyphs };
        let Some(font) = self.font(first.face) else { return glyphs };
        let scale = style.size / f32::from(font.units_per_em());
        let (top, bottom) = font.ideographic_extent();
        let (top, bottom) = (f32::from(top) * scale, -f32::from(bottom) * scale);
        let origin = f32::from(font.vertical_origin(first.glyph)) * scale;
        let square = top + bottom;

        let total: f32 = glyphs.iter().map(|glyph| glyph.advance).sum();
        let squeeze = if style.east_asian.fit_in_line && total > square && total > 0.0 {
            square / total
        } else {
            1.0
        };
        let shown = total * squeeze;
        let slot = square / glyphs.len() as f32;
        let mut running = 0.0;
        for (index, glyph) in glyphs.iter_mut().enumerate() {
            let own = glyph.advance * squeeze;
            glyph.upright =
                Some((origin - index as f32 * slot, shown / 2.0 - (top - bottom) / 2.0 - running));
            glyph.squeeze = squeeze;
            glyph.advance = slot;
            running += own;
        }
        glyphs
    }

    /// Two Lines in One: the run set as two half-height lines stacked inside
    /// the height of one, with brackets round the pair when it asks for them.
    ///
    /// The first half of the characters goes on the upper line and the rest
    /// on the lower, each centred in the width of the wider. The pen walks
    /// the upper line's glyphs in equal steps across that width and stands
    /// still for the lower line's, so that the caret can be put between the
    /// characters of the upper line and lands after the pair for the lower.
    /// In a line that runs down the page the two lines are two columns side
    /// by side, and the letters stand upright in each — the shift that
    /// stands a letter upright and the one that puts it on its line add.
    fn two_lines_in_one(&mut self, text: &str, style: &RunStyle, start: usize) -> Vec<ShapedGlyph> {
        let characters: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
        let half = characters.len().div_ceil(2);
        let split = characters.get(half).copied().unwrap_or(text.len());
        let (upper, lower) = text.split_at(split);

        let small = RunStyle { size: style.size / 2.0, ..style.clone() };
        let mut upper_glyphs = self.shape(upper, &small, start);
        let mut lower_glyphs = self.shape(lower, &small, start + split);
        let width_upper: f32 = upper_glyphs.iter().map(|glyph| glyph.advance).sum();
        let width_lower: f32 = lower_glyphs.iter().map(|glyph| glyph.advance).sum();
        let wide = width_upper.max(width_lower);

        let place = |glyphs: &mut Vec<ShapedGlyph>, width: f32, down: f32, stepping: bool| {
            let step = if stepping { wide / glyphs.len().max(1) as f32 } else { 0.0 };
            // The pen has walked the whole width by the time the lower line
            // is placed, so its glyphs are drawn that far back.
            let back = if stepping { 0.0 } else { wide };
            let mut running = 0.0;
            for (index, glyph) in glyphs.iter_mut().enumerate() {
                let (up_x, up_y) = glyph.upright.unwrap_or((0.0, 0.0));
                glyph.upright = Some((
                    up_x + (wide - width) / 2.0 + running - index as f32 * step - back,
                    up_y + down,
                ));
                running += glyph.advance;
                glyph.advance = step;
            }
        };
        place(&mut upper_glyphs, width_upper, -style.ascent / 2.0, true);
        place(&mut lower_glyphs, width_lower, style.descent / 2.0, false);

        let mut glyphs = Vec::new();
        let brackets = style.east_asian.brackets.characters();
        let bracket = |this: &mut Self, character: char, at: usize| {
            let mut shaped = this.shape(&character.to_string(), style, at);
            for glyph in &mut shaped {
                // The bracket is drawn and is nobody's character: the caret
                // does not stop inside it.
                glyph.length = 0;
            }
            shaped
        };
        if let Some((open, _)) = brackets {
            glyphs.extend(bracket(self, open, start));
        }
        glyphs.extend(upper_glyphs);
        glyphs.extend(lower_glyphs);
        if let Some((_, close)) = brackets {
            glyphs.extend(bracket(self, close, start + text.len()));
        }
        glyphs
    }

    /// How wide the hyphen is that ends a broken word, in this style.
    ///
    /// Measured through the same shaping as everything else, so a hyphen in a
    /// stretched or letter-spaced run is as wide there as it is anywhere.
    fn hyphen_width(&mut self, style: &RunStyle) -> f32 {
        self.shape("-", style, 0).iter().map(|glyph| glyph.advance).sum()
    }

    /// The hyphen itself, ready to be put at the end of a broken line.
    fn hyphen_glyph(&mut self, style: &RunStyle, offset: usize) -> Option<ShapedGlyph> {
        self.shape("-", style, offset).into_iter().next()
    }

    /// A face that can both draw a joined script and shape it.
    fn face_for_joining(&mut self, text: &str, style: &RunStyle) -> Option<usize> {
        let first = text
            .chars()
            .find(|character| {
                wp_shape::is_joining_script(*character) || wp_shape::reorders(*character)
            })
            // A run that is neither joined nor reordered is shaped for the
            // sake of a mark in it, and what has to be drawable is the letter
            // the mark sits on, which is the first character of it.
            .or_else(|| text.chars().next())?;

        // A face without the letters cannot have the rules for joining them.
        let usable = self.font(style.face).is_some_and(|font| {
            font.glyph_for(first).is_some() && font.substitution_table().is_some()
        });
        if usable {
            return Some(style.face);
        }

        let (face, _) = self.library.fallback_for(first, false, false)?;
        Some(face)
    }
}

/// Whether what is left of a paragraph after a break is a single line.
///
/// Asked before a break is taken, because a break that would leave one line
/// alone on the next page is the thing widow control exists to prevent. The
/// width used is the one the line being broken had: near enough, since the two
/// lines are on the same page and the same column.
fn is_one_line_left(items: &[Item], cursor: usize, width: f32, rules: Breaking) -> bool {
    if cursor >= items.len() {
        return false;
    }
    let line = break_next_line(items, cursor, width, rules);
    line.items.end >= items.len()
}

/// What a page held before something was put on it.
///
/// # Why anything is ever taken back off a page
///
/// Because the rules about where a paragraph may be broken are only known to
/// have been broken after the breaking. A paragraph that must not be split is
/// laid out line by line like any other, and it is only when the page runs out
/// halfway through that the rule bites. Putting the lines back — truncating the
/// page to what it held before them — and starting again on the next page is
/// exactly what Word does, and it is far simpler than predicting the break
/// before any line has been measured.
#[derive(Clone, Copy, Debug)]
struct Mark {
    page: usize,
    glyphs: usize,
    images: usize,
    shapes: usize,
    decorations: usize,
    paths: usize,
    lines: usize,
    /// Where the text had got to down the page, and which column it was in.
    y: f32,
    column: usize,
    /// How far through the paragraph's items the line breaker had got, and how
    /// many lines of the paragraph were already down.
    cursor: usize,
    number: usize,
}

impl Mark {
    /// What the page holds now.
    fn here(pages: &[Page], y: f32, column: usize, cursor: usize, number: usize) -> Self {
        let page = pages.len().saturating_sub(1);
        let last = pages.last();
        Self {
            page,
            glyphs: last.map_or(0, |page| page.glyphs.len()),
            images: last.map_or(0, |page| page.images.len()),
            shapes: last.map_or(0, |page| page.shapes.len()),
            decorations: last.map_or(0, |page| page.decorations.len()),
            paths: last.map_or(0, |page| page.paths.len()),
            lines: last.map_or(0, |page| page.lines.len()),
            y,
            column,
            cursor,
            number,
        }
    }

    /// Puts the pages back the way they were.
    fn take_back(self, pages: &mut Vec<Page>) {
        pages.truncate(self.page + 1);
        let Some(page) = pages.last_mut() else { return };
        page.glyphs.truncate(self.glyphs);
        page.images.truncate(self.images);
        page.shapes.truncate(self.shapes);
        page.decorations.truncate(self.decorations);
        page.paths.truncate(self.paths);
        page.lines.truncate(self.lines);
    }
}

/// What a paragraph says about the pieces of one of its lines, beyond the
/// pieces themselves: where its tabs stop, and which way round each piece
/// reads.
#[derive(Clone, Copy, Debug)]
struct Composition<'a> {
    stops: &'a [TabStop],
    /// One level per item of the paragraph, from the bidirectional algorithm.
    levels: &'a [u8],
}

/// The stretch of a line that follows a tab, which is what its stop places.
///
/// Everything up to the next tab or the end of the line: a centred stop centres
/// that stretch on itself and a right-hand one ends it there, so both have to
/// be able to measure it.
#[derive(Clone, Copy)]
struct Following<'a> {
    from: usize,
    end: usize,
    items: &'a [Item],
    styles: &'a [RunStyle],
}

impl LayoutEngine<'_> {
    /// Where a tab reaches from where the line has got to, and what fills the
    /// space it jumped.
    ///
    /// # Why the text after it has to be measured
    ///
    /// Because a stop says where the text *ends* as often as where it begins.
    /// A right-hand stop is how a page number is put against the margin and a
    /// centred one is how a heading is centred over a column: both mean the tab
    /// has to know how wide what follows it is before it knows how far to jump.
    ///
    /// A tab never moves backwards. Where what follows is too wide to fit
    /// before the stop, the text simply carries on from where it was, which is
    /// what Word does rather than letting one column run into another.
    fn tab_reach(
        &mut self,
        x: f32,
        following: Following<'_>,
        placement: &LinePlacement,
        stops: &[TabStop],
    ) -> (f32, TabLeader) {
        let per_twip = self.pixels_per_point() / TWIPS_PER_POINT;
        // A bar draws a line rather than catching a tab, and a clear cancels a
        // stop a style put there: neither is a place to jump to.
        let found = stops
            .iter()
            .filter(|stop| !matches!(stop.alignment, TabAlignment::Bar | TabAlignment::Clear))
            .map(|stop| (placement.origin + stop.position as f32 * per_twip, *stop))
            .find(|(at, _)| *at > x + 0.01);

        let Some((at, stop)) = found else {
            // Past the last stop the default grid takes over, which is what
            // Word does and what makes a tab in an ordinary paragraph work at
            // all.
            return (next_tab_stop(x, placement.origin, self.default_tab_width()), TabLeader::None);
        };

        let target = match stop.alignment {
            TabAlignment::Center => at - segment_width(following) / 2.0,
            TabAlignment::End => at - segment_width(following),
            TabAlignment::Decimal => {
                let (before, found_point) = self.width_before_point(following);
                // A column of figures with nothing to align on is put against
                // the stop, the way a right-hand one would be.
                if found_point {
                    at - before
                } else {
                    at - segment_width(following)
                }
            }
            _ => at,
        };
        (target.max(x), stop.leader)
    }

    /// How wide the stretch after a tab is up to its first decimal point, and
    /// whether there was one.
    ///
    /// The point is looked for by its glyph rather than by its character: the
    /// items hold glyphs that have already been chosen, and shaping the two
    /// separators in the same style says which glyphs to watch for. Both the
    /// point and the comma count, because half the world writes a decimal
    /// comma and Word aligns on whichever one the text uses.
    fn width_before_point(&mut self, following: Following<'_>) -> (f32, bool) {
        let mut width = 0.0;
        for item in following.items[following.from..following.end].iter() {
            if item.is_tab {
                break;
            }
            let style = following.styles[item.style].clone();
            let separators = self.decimal_glyphs(&style);
            for glyph in &item.glyphs {
                if separators.contains(&(glyph.face, glyph.glyph)) {
                    return (width, true);
                }
                width += glyph.advance;
            }
            if item.glyphs.is_empty() {
                width += item.width;
            }
        }
        (width, false)
    }

    /// The glyphs a decimal point and a decimal comma come out as in one style.
    fn decimal_glyphs(&mut self, style: &RunStyle) -> Vec<(usize, GlyphId)> {
        [".", ","]
            .iter()
            .filter_map(|mark| self.shape(mark, style, 0).first().map(|one| (one.face, one.glyph)))
            .collect()
    }

    /// Fills the space a tab jumped with the character its stop asks for.
    ///
    /// Counted back from the far end so that the last dot sits against the text
    /// it leads to: a table of contents whose dots stopped short of the page
    /// numbers would look like a mistake, and in Word they never do.
    fn place_leader(
        &mut self,
        page: &mut Page,
        character: char,
        style: &RunStyle,
        span: (f32, f32),
        baseline: f32,
        source: TextPosition,
    ) {
        let (from, to) = span;
        let mut text = [0u8; 4];
        let shaped = self.shape(character.encode_utf8(&mut text), style, 0);
        let Some(one) = shaped.first().copied() else { return };
        if one.advance <= 0.0 || to <= from {
            return;
        }

        let count = ((to - from) / one.advance).floor() as usize;
        let mut x = to - count as f32 * one.advance;
        for _ in 0..count {
            page.glyphs.push(PositionedGlyph {
                face: one.face,
                glyph: one.glyph,
                x,
                baseline,
                advance: one.advance,
                size: style.size,
                stretch: 1.0,
                color: style.color,
                effect: style.effect,
                // The dots are not text: they all point at the tab that made
                // them, so a click among them lands on the tab.
                source,
                source_length: 0,
                invisible: false,
                shift_x: 0.0,
                shift_y: 0.0,
            });
            x += one.advance;
        }
    }
}

/// What size a ruby's runs ask to be set at, if they ask.
fn own_size(runs: &[wp_docx::model::Run]) -> Option<i32> {
    runs.iter().find_map(|run| run.properties.size_half_points).map(|size| size as i32)
}

/// Makes the optional hyphens of a run take no room and draw nothing.
///
/// The character is a mark about the word rather than a letter of it: Word
/// shows it only when the marks are shown, and draws a hyphen for it only
/// where a line is broken at it. It keeps its place in the run so that the
/// caret can still be moved over it and a click still lands beside it.
fn hide_optional_hyphens(mut glyphs: Vec<ShapedGlyph>) -> Vec<ShapedGlyph> {
    for glyph in &mut glyphs {
        if glyph.character == '\u{00AD}' {
            glyph.advance = 0.0;
            glyph.invisible = true;
        }
    }
    glyphs
}

/// How wide the stretch after a tab is.
fn segment_width(following: Following<'_>) -> f32 {
    following.items[following.from..following.end]
        .iter()
        .take_while(|item| !item.is_tab)
        .map(|item| item.width)
        .sum()
}

impl LayoutEngine<'_> {
    /// Places one line's glyphs, applying alignment, and records the line.
    fn place_line(
        &mut self,
        line: &Line,
        items: &[Item],
        styles: &[RunStyle],
        page: &mut Page,
        placement: LinePlacement,
        composition: Composition<'_>,
    ) {
        let Composition { stops, levels } = composition;
        // Trailing whitespace hangs into the margin rather than being counted,
        // which is what keeps a centred line actually centred.
        let content_width: f32 = line
            .items
            .clone()
            .filter(|index| !(items[*index].is_space && *index >= line.last_visible))
            .map(|index| items[index].width)
            .sum();

        let slack = (placement.width - content_width).max(0.0);
        let mut justify_extra = 0.0;

        // In a right-to-left paragraph the start edge is the right one.
        let effective = match (placement.alignment, placement.paragraph_rtl) {
            (Alignment::Start, false) | (Alignment::End, true) => Alignment::Start,
            (Alignment::End, false) | (Alignment::Start, true) => Alignment::End,
            (other, _) => other,
        };

        let mut x = match effective {
            Alignment::Start => placement.left,
            Alignment::Center => placement.left + slack / 2.0,
            Alignment::End => placement.left + slack,
            Alignment::Both => {
                // The last line of a justified paragraph is not stretched.
                if !placement.is_last_line {
                    let gaps = line
                        .items
                        .clone()
                        .filter(|index| items[*index].is_space && *index < line.last_visible)
                        .count();
                    if gaps > 0 {
                        justify_extra = slack / gaps as f32;
                    }
                }
                placement.left
            }
        };

        let line_left = x;
        let first_glyph = page.glyphs.len();
        let baseline = placement.baseline;
        let line_end = line.items.end;

        // A bar stop is a line down the page rather than a place a tab reaches:
        // it is drawn on every line of the paragraph, whether or not anything
        // was tabbed to it. See [`wp_docx::model::TabAlignment::Bar`].
        let per_twip = self.pixels_per_point() / TWIPS_PER_POINT;
        for stop in stops.iter().filter(|stop| stop.alignment == TabAlignment::Bar) {
            let at = placement.origin + stop.position as f32 * per_twip;
            page.decorations.push(Decoration {
                x: at,
                y: baseline - placement.ascent,
                width: 1.0,
                height: placement.ascent + placement.descent,
                color: styles.first().map_or(self.automatic_color, |style| style.color),
            });
        }

        // The order the pieces are drawn in, which is not the order they are
        // stored in wherever the line holds both directions at once. Everything
        // that follows walks the line in this order and leaves the logical
        // index alone, because the caret, the selection and the trailing space
        // are all still counted the way the text is stored. See [`wp_bidi`].
        let line_levels: Vec<u8> =
            line.items.clone().map(|index| levels.get(index).copied().unwrap_or(0)).collect();
        let visual: Vec<usize> = wp_bidi::reorder(&line_levels)
            .into_iter()
            .map(|position| line.items.start + position)
            .collect();

        // Where the hyphen goes, if this line broke a word in half: the place
        // the last word ends, and the style it was written in.
        let mut broken_at: Option<(f32, usize, usize, bool)> = None;

        for index in visual {
            let item = &items[index];
            let style = &styles[item.style];

            if item.is_space && index >= line.last_visible {
                // A space at the end of a line is not drawn: it would hang past
                // the last word, and in justified text it would be stretched
                // along with the others. But it is still a character, and the
                // caret has to be able to stand after it — without this,
                // pressing the space bar at the end of a line moves nothing
                // until the next letter is typed.
                let advance: f32 = item.glyphs.iter().map(|glyph| glyph.advance).sum();
                page.glyphs.push(PositionedGlyph {
                    face: style.face,
                    glyph: GlyphId(0),
                    x,
                    baseline,
                    advance,
                    size: style.size,
                    stretch: 1.0,
                    color: style.color,
                    effect: style.effect,
                    source: TextPosition::new(placement.paragraph, item.start_offset),
                    source_length: item.end_offset - item.start_offset,
                    invisible: true,
                    shift_x: 0.0,
                    shift_y: 0.0,
                });
                x += advance;
                continue;
            }

            let start_x = x;
            // A superscript or subscript sits off the line rather than on it.
            let glyph_baseline = baseline - style.raise;
            for glyph in &item.glyphs {
                let (shift_x, shift_y) = glyph.upright.unwrap_or((0.0, 0.0));
                // A letter standing upright in a line that will be turned is
                // marked as such, so that the turn leaves it be. The marks
                // are spans, and a run of upright letters is one span.
                if glyph.upright.is_some() && self.vertical {
                    let at = page.glyphs.len();
                    match page.turned.last_mut() {
                        Some((span, Turn::Upright)) if span.end == at => span.end = at + 1,
                        _ => page.turned.push((at..at + 1, Turn::Upright)),
                    }
                }
                page.glyphs.push(PositionedGlyph {
                    face: glyph.face,
                    glyph: glyph.glyph,
                    // Where the pen stands, moved by what the font said about
                    // this glyph: down the page is positive here and up is
                    // positive in a font, so the one is taken from the other.
                    x: x + glyph.x_offset,
                    baseline: glyph_baseline - glyph.y_offset,
                    advance: glyph.advance,
                    // The glyph's own size rather than the run's: a small
                    // capital is drawn smaller than the capitals beside it.
                    size: glyph.size,
                    stretch: style.stretch * glyph.squeeze,
                    color: style.color,
                    effect: style.effect,
                    source: TextPosition::new(placement.paragraph, glyph.offset),
                    source_length: glyph.length,
                    // Hidden text is recorded where it sits and drawn nowhere,
                    // so the caret can still be moved through it and a click
                    // still lands in the right place — which is what happens in
                    // Word when the marks are turned back on.
                    invisible: style.hidden || glyph.invisible,
                    shift_x,
                    shift_y,
                });
                x += glyph.advance;
            }

            // A word broken at an optional hyphen is drawn with the hyphen the
            // writer asked for — and only then. The last line of a paragraph
            // ends where the words end, and an optional hyphen there is
            // nothing at all. A word the patterns broke is drawn with a
            // hyphen the same way; there the piece ends with a letter and
            // not with a mark.
            if item.hyphen > 0.0 && index + 1 == line.last_visible && line.items.end < items.len() {
                let marked = item.glyphs.last().is_some_and(|glyph| glyph.invisible);
                broken_at = Some((x, item.style, item.end_offset, marked));
            }
            if let Some((shape, height)) = &item.shape {
                // A drawing that floats is not on the line at all: it is put
                // where its anchor says and the text keeps out of its way.
                if let Some(anchor) = shape.anchor.clone() {
                    let line_top = baseline - placement.ascent;
                    let shape = shape.as_ref().clone();
                    // A size stated as a percentage of a frame wins over the
                    // one written beside it. See [`Self::float_size`].
                    let (width, height) = self.float_size(
                        &anchor,
                        &placement.area,
                        (
                            item.width.max(shape.width_points() as f32 * self.pixels_per_point()),
                            *height,
                        ),
                    );
                    self.place_float(
                        &anchor,
                        page,
                        placement.page,
                        &placement.area,
                        line_top,
                        width,
                        height,
                        &shape,
                        Some(TextPosition::new(placement.paragraph, item.start_offset)),
                    );
                    continue;
                }
                // A shape sits on the baseline, as a picture does.
                page.shapes.push(PlacedShape {
                    x,
                    y: baseline - height,
                    width: item.width,
                    height: *height,
                    preset: crate::geometry::Preset::from_word(&shape.preset),
                    adjusts: crate::geometry::Adjusts::from_pairs(&shape.adjusts),
                    head_end: shape.head_end,
                    id: shape.id,
                    joins: shape.joins,
                    route: None,
                    effects: Effects::of(&shape.effects, self.pixels_per_point(), &self.theme),
                    solid: Solid::of(
                        &shape.depth,
                        &shape.scene,
                        self.pixels_per_point(),
                        &self.theme,
                    ),
                    tail_end: shape.tail_end,
                    fill: crate::paint::Paint::of(&shape.fill, &self.theme),
                    outline: self.outline_colour(shape),
                    outline_weight: (shape.outline_points_in(&self.theme)
                        * self.pixels_per_point())
                    .max(1.0),
                    shadow: self.shape_shadow(self.pixels_per_point()),
                    text: Vec::new(),
                    text_turned: Vec::new(),
                    name: shape.name.clone(),
                    at: Some(TextPosition::new(placement.paragraph, item.start_offset)),
                    // In the line of text, which is under everything that
                    // floats and is drawn before the words of its own line.
                    depth: 0,
                    over_text: false,
                    source: Some(shape.clone()),
                    turn: wp_docx::floating::Turned::of_shape(shape).radians(),
                    flipped_across: shape.flipped_across,
                    flipped_down: shape.flipped_down,
                });
                x += item.width;
            } else if let Some((group, height)) = &item.group {
                let at = Some(TextPosition::new(placement.paragraph, item.start_offset));
                // A group that floats goes where its anchor says, exactly as
                // one drawing would: a group is one drawing.
                if let Some(anchor) = group.anchor.clone() {
                    let line_top = baseline - placement.ascent;
                    let (width, height) = self.float_size(
                        &anchor,
                        &placement.area,
                        (
                            item.width.max(group.width_points() as f32 * self.pixels_per_point()),
                            *height,
                        ),
                    );
                    let height = &height;
                    let (at_x, at_y) = self.float_box(
                        &anchor,
                        placement.page,
                        &placement.area,
                        line_top,
                        width,
                        *height,
                        // The box and not the outline: what a group's edge is
                        // for tight wrapping is the edge of everything in it,
                        // which is the box it covers.
                        None,
                    );
                    let group = group.as_ref().clone();
                    self.place_group(
                        page,
                        &group,
                        (at_x, at_y, width, *height),
                        at,
                        anchor.depth,
                        !anchor.behind_text,
                    );
                    continue;
                }
                let group = group.as_ref().clone();
                self.place_group(
                    page,
                    &group,
                    (x, baseline - height, item.width, *height),
                    at,
                    0,
                    false,
                );
                x += item.width;
            } else if let Some((drawing, height)) = &item.ink {
                let at = Some(TextPosition::new(placement.paragraph, item.start_offset));
                // Ink that floats is put where its anchor says, the way a
                // picture that floats is; the strokes are laid out already in
                // a box of their own, so that box is moved to where it goes.
                if let Some(anchor) = item.picture_anchor.clone() {
                    let line_top = baseline - placement.ascent;
                    let (width, height) =
                        self.float_size(&anchor, &placement.area, (item.width, *height));
                    let (at_x, at_y) = self.float_box(
                        &anchor,
                        placement.page,
                        &placement.area,
                        line_top,
                        width,
                        height,
                        None,
                    );
                    page.inks.push(PlacedInk {
                        x: at_x,
                        y: at_y,
                        width,
                        height,
                        drawing: drawing.translated(at_x, at_y),
                        depth: anchor.depth,
                        over_text: !anchor.behind_text,
                        at,
                        name: picture_name(item),
                    });
                    continue;
                }
                // In the line it hangs from the baseline the way a picture
                // does.
                let top = baseline - item_height(item);
                page.inks.push(PlacedInk {
                    x,
                    y: top,
                    width: item.width,
                    height: *height,
                    drawing: drawing.translated(x, top),
                    depth: 0,
                    over_text: false,
                    at,
                    name: picture_name(item),
                });
                x += item.width;
            } else if let Some((drawing, _)) = &item.chart {
                // A chart is laid out already, in a box of its own; putting it
                // on the line is moving that box to where the line is. It hangs
                // from the baseline the way a picture does.
                let moved = drawing.translated(x, baseline - item_height(item));
                page.decorations.extend(moved.rules);
                page.paths.extend(
                    moved.paths.into_iter().map(|(path, color)| PlacedPath { path, color }),
                );
                page.glyphs.extend(moved.glyphs);
                x += item.width;
            } else if let Some((laid, _)) = &item.ruby {
                // Laid out already, against a baseline of its own: putting it
                // on the line is moving it to this one.
                page.glyphs.extend(laid.placed(x, baseline));
                x += item.width;
            } else if let Some((laid, _)) = &item.math {
                // An equation is laid out already, against a baseline of its
                // own; putting it on the line is moving it to this one.
                let (glyphs, rules) = laid.placed(x, baseline);
                page.glyphs.extend(glyphs);
                page.decorations.extend(rules);
                x += item.width;
            } else if let Some((image, height)) = &item.picture {
                // A picture that floats is not on the line at all: it is put
                // where its anchor says and the text keeps out of its way, the
                // same as a shape that floats.
                if let Some(anchor) = item.picture_anchor.clone() {
                    let line_top = baseline - placement.ascent;
                    let (width, height) =
                        self.float_size(&anchor, &placement.area, (item.width, *height));
                    let (at_x, at_y) = self.float_box(
                        &anchor,
                        placement.page,
                        &placement.area,
                        line_top,
                        width,
                        height,
                        // A picture is the box it fills: there is no outline for
                        // tight wrapping to follow.
                        None,
                    );
                    page.images.push(PlacedImage {
                        x: at_x,
                        y: at_y,
                        width,
                        height,
                        image: Rc::clone(image),
                        depth: anchor.depth,
                        over_text: !anchor.behind_text,
                        at: Some(TextPosition::new(placement.paragraph, item.start_offset)),
                        turn: item.picture_turn.radians(),
                        flipped_across: item.picture_turn.flipped_across,
                        flipped_down: item.picture_turn.flipped_down,
                        name: picture_name(item),
                        video: item.picture_video,
                    });
                    continue;
                }
                // One in the line sits on the baseline, like a very tall letter.
                page.images.push(PlacedImage {
                    x,
                    y: baseline - height,
                    width: item.width,
                    height: *height,
                    image: Rc::clone(image),
                    depth: 0,
                    over_text: false,
                    at: Some(TextPosition::new(placement.paragraph, item.start_offset)),
                    turn: item.picture_turn.radians(),
                    flipped_across: item.picture_turn.flipped_across,
                    flipped_down: item.picture_turn.flipped_down,
                    name: picture_name(item),
                    video: item.picture_video,
                });
                x += item.width;
            } else if item.is_tab {
                // A tab is a distance to a place, not a distance to travel.
                let following = Following { from: index + 1, end: line_end, items, styles };
                let (target, leader) = match item.aligned_tab {
                    // An alignment tab is told where it is going. The middle
                    // and the far end of the text, and what follows it is
                    // pulled back so that it ends there rather than starting
                    // there — which is what makes a page number sit against
                    // the right margin.
                    Some(alignment) => {
                        let following = Following { from: index + 1, end: line_end, items, styles };
                        let rest = segment_width(following);
                        let right = placement.left + placement.width;
                        let target = match alignment {
                            wp_docx::model::TabAlignment::Center => {
                                placement.left + (placement.width - rest) / 2.0
                            }
                            wp_docx::model::TabAlignment::End => right - rest,
                            _ => x,
                        };
                        (target.max(x), TabLeader::None)
                    }
                    None => self.tab_reach(x, following, &placement, stops),
                };
                if let Some(character) = leader.character() {
                    let style = styles[item.style].clone();
                    let source = TextPosition::new(placement.paragraph, item.start_offset);
                    self.place_leader(page, character, &style, (x, target), baseline, source);
                }
                x = target;
            } else if item.glyphs.is_empty() {
                x += item.width;
            }

            // A tab and a break take up a character each, so they are recorded
            // where they sit even though nothing is drawn for them. Without
            // this the caret would land on the wrong side of a tab.
            if item.end_offset > item.start_offset && item.glyphs.is_empty() {
                page.glyphs.push(PositionedGlyph {
                    face: style.face,
                    glyph: GlyphId(0),
                    x: start_x,
                    baseline,
                    advance: x - start_x,
                    size: style.size,
                    stretch: 1.0,
                    color: style.color,
                    effect: style.effect,
                    source: TextPosition::new(placement.paragraph, item.start_offset),
                    source_length: item.end_offset - item.start_offset,
                    invisible: true,
                    shift_x: 0.0,
                    shift_y: 0.0,
                });
            }
            if item.is_space {
                x += justify_extra;
            }

            let drawn_width = x - start_x;
            // The band behind highlighted text. Pushed before the underline so
            // that it is drawn first and the underline shows on top of it.
            if let Some(colour) = style.highlight {
                if drawn_width > 0.0 {
                    page.decorations.push(Decoration {
                        x: start_x,
                        y: baseline - placement.ascent,
                        width: drawn_width,
                        height: placement.ascent + placement.descent,
                        color: colour,
                    });
                }
            }
            let thickness = (style.size * 0.05).max(1.0);
            if style.underline && drawn_width > 0.0 {
                page.decorations.push(Decoration {
                    x: start_x,
                    // Just below the baseline, scaled so it stays proportional.
                    y: baseline + placement.descent * 0.25,
                    width: drawn_width,
                    height: thickness,
                    // Word lets the line be a different colour from the letters
                    // it is under, and most of the time it is not.
                    color: style.underline_color.unwrap_or(style.color),
                });
            }
            // One line through the middle, or two straddling it. The format
            // keeps them apart rather than counting lines, and so does Word's
            // dialog: they are two tick boxes, and each unticks the other.
            if (style.strike || style.double_strike) && drawn_width > 0.0 {
                let middle = baseline - style.ascent * 0.3;
                let offsets: &[f32] =
                    if style.double_strike { &[-thickness, thickness] } else { &[0.0] };
                for offset in offsets {
                    page.decorations.push(Decoration {
                        x: start_x,
                        y: middle + offset,
                        width: drawn_width,
                        height: thickness,
                        color: style.color,
                    });
                }
            }
        }

        // The hyphen that says a word was broken. It belongs to the line
        // rather than to any item of it — the writer wrote a mark, not a
        // hyphen, and what turns one into the other is the line ending there.
        if let Some((at, style_index, offset, marked)) = broken_at {
            if let Some(style) = styles.get(style_index).cloned() {
                if let Some(glyph) = self.hyphen_glyph(&style, offset) {
                    page.glyphs.push(PositionedGlyph {
                        face: glyph.face,
                        glyph: glyph.glyph,
                        x: at,
                        baseline: baseline - style.raise,
                        advance: glyph.advance,
                        size: glyph.size,
                        stretch: style.stretch,
                        color: style.color,
                        effect: style.effect,
                        // It points at the mark it was drawn for — or, for a
                        // word the patterns broke, at the place the word was
                        // broken — and covers none of the text: a click on
                        // the hyphen lands beside it rather than inside
                        // anything.
                        source: TextPosition::new(
                            placement.paragraph,
                            if marked {
                                offset.saturating_sub('\u{00AD}'.len_utf8())
                            } else {
                                offset
                            },
                        ),
                        source_length: 0,
                        invisible: false,
                        shift_x: 0.0,
                        shift_y: 0.0,
                    });
                    x += glyph.advance;
                }
            }
        }

        // The stretch of the paragraph this line covers, so a caret and a click
        // can be resolved against it later.
        let start_offset =
            line.items.clone().map(|index| items[index].start_offset).min().unwrap_or(0);
        let end_offset =
            line.items.clone().map(|index| items[index].end_offset).max().unwrap_or(start_offset);

        page.lines.push(PageLine {
            baseline,
            ascent: placement.ascent,
            descent: placement.descent,
            left: line_left,
            right: x,
            glyphs: first_glyph..page.glyphs.len(),
            paragraph: placement.paragraph,
            start_offset,
            end_offset,
            frame: Frame::default(),
        });
    }
}

/// Where one line goes, and how it should be aligned.
#[derive(Clone, Copy, Debug)]
struct LinePlacement {
    left: f32,
    /// The left edge of the text area, which is where tab stops are counted
    /// from. Not the same as `left`, which an indent moves.
    origin: f32,
    width: f32,
    baseline: f32,
    ascent: f32,
    descent: f32,
    alignment: Alignment,
    paragraph_rtl: bool,
    is_last_line: bool,
    /// Which paragraph the line belongs to, in reading order.
    paragraph: usize,
    /// Which page it landed on, so a drawing anchored in it knows where it is.
    page: usize,
    /// The area the text is being laid into, which is what a floating drawing
    /// measures its position from.
    area: Placement,
}

/// One stretch of a body laid out on paper of its own: a section, or the whole
/// of a body that has no sections.
#[derive(Clone, Debug)]
pub(crate) struct Stretch {
    /// The blocks it covers.
    pub(crate) blocks: core::ops::Range<usize>,
    /// The paper they are printed on.
    pub(crate) metrics: PageMetrics,
    /// How the stretch begins.
    pub(crate) start: Start,
    /// Which section of the document it is, for the header printed on it.
    pub(crate) section: usize,
}

/// Which pages a stretch of header or footer is being printed on.
///
/// A `PAGE` field says which page of the whole document it is on, not which
/// page of the section — so the numbering has to be told where the stretch
/// begins and how many pages there are altogether. The section is here too,
/// because how far the header sits from the edge of the paper is a property of
/// the section it belongs to.
#[derive(Clone, Copy, Debug)]
struct Numbering {
    total: usize,
    section: usize,
    /// What the page is numbered and in what figures, which is not the same
    /// as where it falls in the document: a section can start its numbering
    /// again, and can ask for Roman figures.
    printed: usize,
    format: NumberFormat,
}

/// The rectangle text is placed within, in pixels.
#[derive(Clone, Copy, Debug)]
struct Placement {
    /// The left edge of the first column, and the width of one column.
    left: f32,
    top: f32,
    bottom_limit: f32,
    text_width: f32,
    page_width: f32,
    page_height: f32,
    /// How many columns the text flows down, and the gap between them.
    columns: usize,
    column_gap: f32,
    /// Whether the rules about keeping paragraphs together apply.
    ///
    /// They are about page breaks, so they mean nothing where a page break
    /// means nothing — inside a header, a footnote or a shape, each of which
    /// is laid out onto a page of its own that is never turned.
    keeping: bool,
}

impl Placement {
    /// The same area, moved along to one of its columns.
    ///
    /// Every column is the same width, so only the left edge moves — which is
    /// what keeps line breaking identical from one column to the next.
    #[must_use]
    fn in_column(self, column: usize) -> Self {
        if self.columns <= 1 {
            return self;
        }
        let step = self.text_width + self.column_gap;
        Self { left: self.left + column as f32 * step, ..self }
    }

    /// The area a table cell is laid out in, which never has columns of its own.
    #[must_use]
    fn without_columns(self) -> Self {
        Self { columns: 1, ..self }
    }
}

impl Item {
    /// Whether the drawing in this item floats on the page rather than sitting
    /// in the line.
    ///
    /// A floating drawing takes no room on the line it is anchored in: not
    /// across it — the width is already left out when the item is built — and
    /// not down it either. Counting its height made the anchor's line as tall
    /// as the whole drawing, so the text after it began below the drawing and
    /// nothing ever came out beside one. Square wrapping wrapped nothing.
    fn floats(&self) -> bool {
        self.picture_anchor.is_some()
            || self.shape.as_ref().is_some_and(|(shape, _)| shape.anchor.is_some())
            || self.group.as_ref().is_some_and(|(group, _)| group.anchor.is_some())
    }
}

/// One line: a span of items, and where the drawable part of it ends.
#[derive(Clone, Debug)]
struct Line {
    items: std::ops::Range<usize>,
    /// Index after the last item that is actually drawn, so trailing spaces can
    /// be skipped without losing them from the range.
    last_visible: usize,
}

/// The width of every column of a table, in pixels.
///
/// The grid is what decides the geometry: a cell says how many columns it
/// covers, not how wide it is. A table with no grid — which a hand-written
/// document may well be — has one made for it from the widest row, so its cells
/// still line up with each other.
/// What to call a picture in a list of the document's drawings.
///
/// The name the file gives it, which is Word's `wp:docPr` — the same place its
/// description comes from. A picture carrying neither is called "Picture",
/// because a row with nothing in it would be a row nobody could read.
fn picture_name(item: &Item) -> String {
    match &item.picture_name {
        Some(name) if !name.is_empty() => name.clone(),
        _ => "Picture".to_owned(),
    }
}

/// Which parts of the table one cell is in.
///
/// A cell can be in several at once — the first cell of the first row of a
/// banded table is in three — and which of them the style may use at all is the
/// table's own decision, carried in its `w:tblLook`. See
/// [`wp_docx::model::TableLook`].
///
/// The bands count from the first row that is not the header, because Word
/// counts them that way: a table with a header row has its first band on the
/// row under it, not on the header.
fn parts_of(
    look: wp_docx::model::TableLook,
    row: usize,
    rows: usize,
    column: usize,
    columns: usize,
) -> Vec<Conditional> {
    let mut parts = Vec::new();

    if look.first_row && row == 0 {
        parts.push(Conditional::FirstRow);
    }
    if look.last_row && rows > 1 && row + 1 == rows {
        parts.push(Conditional::LastRow);
    }
    if look.first_column && column == 0 {
        parts.push(Conditional::FirstColumn);
    }
    if look.last_column && columns > 1 && column + 1 == columns {
        parts.push(Conditional::LastColumn);
    }

    if look.banded_rows {
        let first_banded = usize::from(look.first_row);
        if row >= first_banded {
            let band = row - first_banded;
            parts.push(if band % 2 == 0 {
                Conditional::Band1Horizontal
            } else {
                Conditional::Band2Horizontal
            });
        }
    }
    if look.banded_columns {
        let first_banded = usize::from(look.first_column);
        if column >= first_banded {
            let band = column - first_banded;
            parts.push(if band % 2 == 0 {
                Conditional::Band1Vertical
            } else {
                Conditional::Band2Vertical
            });
        }
    }
    parts
}

impl LayoutEngine<'_> {
    /// Puts the list counters aside, for a pass that measures rather than
    /// places.
    ///
    /// Measuring a numbered paragraph must not use up the number it is given
    /// when it is really laid out, or a list inside a table would count every
    /// item twice. See [`crate::tablefit`].
    pub(crate) fn saved_counters(&mut self) -> ListCounters {
        self.counters.clone()
    }

    /// And puts them back.
    pub(crate) fn restore_counters(&mut self, counters: ListCounters) {
        self.counters = counters;
    }
}

/// How long the text on a page came out: the end of its longest line.
///
/// What a turned cell needs is a row tall enough for this, which is the one
/// question a cell laid out straight cannot answer about itself.
///
/// A whisker more than the text measured, because a row exactly as long as its
/// text is a row the line breaker may decide the last word does not fit in —
/// and the word would go round onto a second line nobody asked for.
fn text_length(page: &Page) -> f32 {
    page.lines.iter().map(|line| line.right).fold(0.0f32, f32::max) + 1.0
}

/// Puts a cell laid out straight onto the page, turned a right angle.
///
/// Everything the cell produced is moved: its letters, its lines, the bands and
/// rules that were drawn behind them, and whatever was placed inside it. A
/// right angle turns a rectangle into a rectangle, so nothing here needs more
/// than [`Frame::rect`] — except a path, which is turned properly, and a
/// picture, which is put in the right place and drawn the way up it was.
fn lay_turned(page: &mut Page, laid: Page, frame: Frame) {
    let first = page.glyphs.len();
    for mut glyph in laid.glyphs {
        let (x, y) = frame.on_page(glyph.x, glyph.baseline);
        glyph.x = x;
        glyph.baseline = y;
        let (shift_x, shift_y) = frame.direction_on_page(glyph.shift_x, glyph.shift_y);
        glyph.shift_x = shift_x;
        glyph.shift_y = shift_y;
        page.glyphs.push(glyph);
    }
    // The cell's own spans first — a letter standing upright in it, a cell
    // turned inside it — and then the turn of the whole cell, which is on
    // top of them. See [`Page::turn_of`].
    for (span, turn) in laid.turned {
        page.turned.push(((span.start + first)..(span.end + first), turn));
    }
    if page.glyphs.len() > first {
        page.turned.push((first..page.glyphs.len(), frame.turn));
    }

    for mut line in laid.lines {
        // The lines keep their own numbers and are told where they sit, which
        // is what makes a click and a caret land in the right place.
        line.glyphs = (line.glyphs.start + first)..(line.glyphs.end + first);
        line.frame = line.frame.then(frame);
        page.lines.push(line);
    }

    for decoration in laid.decorations {
        let (x, y, width, height) =
            frame.rect(decoration.x, decoration.y, decoration.width, decoration.height);
        page.decorations.push(Decoration { x, y, width, height, color: decoration.color });
    }

    for cell in laid.cells {
        let (x, y, width, height) = frame.rect(cell.x, cell.y, cell.width, cell.height);
        page.cells.push(PlacedCell { x, y, width, height, at: cell.at });
    }

    for mut picture in laid.images {
        let (x, y, width, height) = frame.rect(picture.x, picture.y, picture.width, picture.height);
        picture.x = x;
        picture.y = y;
        picture.width = width;
        picture.height = height;
        page.images.push(picture);
    }

    for mut shape in laid.shapes {
        let (x, y, width, height) = frame.rect(shape.x, shape.y, shape.width, shape.height);
        shape.x = x;
        shape.y = y;
        shape.width = width;
        shape.height = height;
        for glyph in &mut shape.text {
            let (gx, gy) = frame.on_page(glyph.x, glyph.baseline);
            glyph.x = gx;
            glyph.baseline = gy;
        }
        page.shapes.push(shape);
    }

    for mut path in laid.paths {
        path.path = path.path.transformed(&frame.transform());
        page.paths.push(path);
    }

    for mut ink in laid.inks {
        let (x, y, width, height) = frame.rect(ink.x, ink.y, ink.width, ink.height);
        ink.x = x;
        ink.y = y;
        ink.width = width;
        ink.height = height;
        ink.drawing = ink.drawing.transformed(&frame.transform());
        page.inks.push(ink);
    }
}

/// The turns of a row of glyphs, as spans: one per run of glyphs turned the
/// same way, and none for the ordinary way up.
fn spans_of(turns: impl Iterator<Item = Turn>) -> Vec<(core::ops::Range<usize>, Turn)> {
    let mut spans: Vec<(core::ops::Range<usize>, Turn)> = Vec::new();
    for (at, turn) in turns.enumerate() {
        if turn == Turn::None {
            continue;
        }
        match spans.last_mut() {
            Some((span, last)) if *last == turn && span.end == at => span.end = at + 1,
            _ => spans.push((at..at + 1, turn)),
        }
    }
    spans
}

/// Turns a page laid out into a box onto the paper — or, given the inverse
/// frame, back into its box.
///
/// What [`lay_turned`] does for a cell, done to a whole page in place: for a
/// section written down the page, whose body is laid out into a box as
/// long as the page is tall and turned afterwards. Everything on the page
/// moves — see [`Page::frame`] for why the turn is kept on the page rather
/// than only its effect.
///
/// Every step here is a swap of coordinates and a subtraction from the
/// page's edge, and the subtraction is the one that rounds: a page turned,
/// turned back and turned again lands exactly where it landed the first
/// time, because the second subtraction is of two numbers a whisker apart,
/// which is exact. That is what lets the engine be given its pages back —
/// see [`again`].
fn turn_page(page: &mut Page, frame: Frame) {
    for glyph in &mut page.glyphs {
        let (x, y) = frame.on_page(glyph.x, glyph.baseline);
        glyph.x = x;
        glyph.baseline = y;
        let (shift_x, shift_y) = frame.direction_on_page(glyph.shift_x, glyph.shift_y);
        glyph.shift_x = shift_x;
        glyph.shift_y = shift_y;
    }
    for line in &mut page.lines {
        line.frame = line.frame.then(frame);
    }
    for decoration in &mut page.decorations {
        let (x, y, width, height) =
            frame.rect(decoration.x, decoration.y, decoration.width, decoration.height);
        decoration.x = x;
        decoration.y = y;
        decoration.width = width;
        decoration.height = height;
    }
    for cell in &mut page.cells {
        let (x, y, width, height) = frame.rect(cell.x, cell.y, cell.width, cell.height);
        cell.x = x;
        cell.y = y;
        cell.width = width;
        cell.height = height;
    }
    for picture in &mut page.images {
        let (x, y, width, height) = frame.rect(picture.x, picture.y, picture.width, picture.height);
        picture.x = x;
        picture.y = y;
        picture.width = width;
        picture.height = height;
    }
    for shape in &mut page.shapes {
        let (x, y, width, height) = frame.rect(shape.x, shape.y, shape.width, shape.height);
        shape.x = x;
        shape.y = y;
        shape.width = width;
        shape.height = height;
        for glyph in &mut shape.text {
            let (gx, gy) = frame.on_page(glyph.x, glyph.baseline);
            glyph.x = gx;
            glyph.baseline = gy;
        }
        if let Some(route) = &mut shape.route {
            *route = route.transformed(&frame.transform());
        }
    }
    for path in &mut page.paths {
        path.path = path.path.transformed(&frame.transform());
    }
    for ink in &mut page.inks {
        let (x, y, width, height) = frame.rect(ink.x, ink.y, ink.width, ink.height);
        ink.x = x;
        ink.y = y;
        ink.width = width;
        ink.height = height;
        ink.drawing = ink.drawing.transformed(&frame.transform());
    }
    if frame.is_sideways() {
        core::mem::swap(&mut page.width, &mut page.height);
    }
}

impl Page {
    /// Turns the page's body onto the paper, for a section written down it.
    /// The turn is remembered on the page, and the glyphs on it now are the
    /// ones it covers.
    fn turn(&mut self, frame: Frame) {
        if !frame.is_turned() || self.frame.is_turned() {
            return;
        }
        turn_page(self, frame);
        self.frame = frame;
        self.turned_glyphs = self.glyphs.len();
    }

    /// And back into its box, for the engine to go on placing text in it.
    /// Whatever was put on the page after the turn is turned with it, which
    /// does no harm: the engine takes all of that off again before it places
    /// anything. See [`again`].
    fn unturn(&mut self) {
        if !self.frame.is_turned() {
            return;
        }
        let frame = self.frame;
        turn_page(self, frame.inverse());
        self.frame = Frame::default();
        self.turned_glyphs = 0;
    }
}

/// How much room is kept clear inside one cell, in pixels.
///
/// What the cell states, then what its table states, then Word's own defaults —
/// which are a little at each side and nothing above or below.
pub(crate) fn margins_of_cell(cell: &TableCell, table: &Table, scale: f32) -> Margins {
    Margins::of(cell.margins.over(table.cell_margins), scale)
}

/// Room kept clear on the four sides of a cell, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Margins {
    pub top: f32,
    pub start: f32,
    pub bottom: f32,
    pub end: f32,
}

impl Margins {
    fn of(margins: wp_docx::model::CellMargins, scale: f32) -> Self {
        let (top, start, bottom, end) = margins.or_usual();
        let pixels = |twips: i32| twips.max(0) as f32 / TWIPS_PER_POINT * scale;
        Self { top: pixels(top), start: pixels(start), bottom: pixels(bottom), end: pixels(end) }
    }

    /// How much of a cell's width the two side margins take.
    fn across(&self) -> f32 {
        self.start + self.end
    }

    /// And how much of its height.
    fn down(&self) -> f32 {
        self.top + self.bottom
    }
}

/// How much room a table leaves between one cell and the next, in pixels.
///
/// Half of it goes on each side of every cell, so the gap between two of them
/// is the whole of it and the gap between a cell and the edge of the table is
/// half — which is what the number means and what keeps the grid the table's
/// geometry rather than something the spacing has moved.
pub(crate) fn cell_spacing(table: &Table, scale: f32) -> f32 {
    table.cell_spacing.unwrap_or(0).max(0) as f32 / TWIPS_PER_POINT * scale
}

/// Where each cell of a row sits, as a left edge and a width.
///
/// A cell covering several columns takes their widths together, which is what
/// makes a merged heading line up with the columns under it.
fn cell_spans(row: &TableRow, columns: &[f32], left: f32, inset: f32) -> Vec<(f32, f32)> {
    let mut spans = Vec::with_capacity(row.cells.len());
    let mut x = left;
    let mut column = 0usize;

    for cell in &row.cells {
        let span = cell.span.max(1) as usize;
        let width: f32 = columns.iter().skip(column).take(span).sum();
        // A row with more cells than the grid has columns still has to put them
        // somewhere, so anything past the end gets the last column's width.
        let width = if width > 0.0 { width } else { columns.last().copied().unwrap_or(0.0) };
        // The cell sits inside its slot by half the room the table leaves
        // between cells, which is none at all unless it asks for some.
        spans.push((x + inset, (width - inset * 2.0).max(1.0)));
        x += width;
        column += span;
    }

    spans
}

/// Draws the lines around and between the cells of one row.
#[allow(clippy::too_many_arguments)]
fn draw_row_borders(
    page: &mut Page,
    borders: &TableBorders,
    row: &TableRow,
    spans: &[(f32, f32)],
    top: f32,
    bottom: f32,
    row_number: usize,
    row_count: usize,
    scale: f32,
    automatic: Color,
    gridlines: bool,
) {
    // A table with no lines of its own is invisible, and a person editing one
    // has to see where the cells are. Word draws faint boundaries for that and
    // calls them gridlines; they are on the screen only and never printed,
    // which is what makes them gridlines rather than borders.
    let gridline = gridlines
        .then(|| (1.0f32, Color::rgba(automatic.red, automatic.green, automatic.blue, 70)));

    // What one line of the grid comes to: the border that asked for it, how
    // thick it is and what colour. The border itself is carried because the
    // style is drawn rather than only measured — a double line is two lines,
    // and a table's are drawn by the same code that draws a paragraph's.
    let line = |border: &Option<Border>| -> Option<(Option<Border>, f32, Color)> {
        let Some(border) = border.as_ref().filter(|border| border.is_visible()) else {
            // A gridline has no border behind it and no style: it is the faint
            // mark that says where a cell is, and nothing asked for it.
            return gridline.map(|(thickness, colour)| (None, thickness, colour));
        };
        let color = border.color.as_deref().and_then(Color::from_hex).unwrap_or(automatic);
        Some((Some(border.clone()), (border.width_points() * scale).max(1.0), color))
    };

    // A line inside the table comes from the table's inside borders and from
    // nowhere else. An absent inside border is a table that asks for no line
    // between its rows, which is what Word draws for one: falling back to the
    // border round the outside made Borders > Outside draw a full grid, and
    // every table whose frame was its only border came out ruled.
    let first_row = row_number == 0;
    let last_row = row_number + 1 == row_count;
    let horizontal = if first_row { line(&borders.top) } else { line(&borders.inside_horizontal) };
    // A line between two rows belongs to the row under it and is drawn once.
    // Drawing it from above as well put two lines in the same place — twice the
    // ink, and a pair of cells joined down the table still had a line between
    // them: the upper one drew it whatever the lower one said. Below the last
    // row there is no row under it, so that line is the table's own edge; a
    // cell that asks for a bottom border of its own still gets one, which is
    // what the border painter paints.
    let below = if last_row { line(&borders.bottom) } else { None };

    let Some((left_edge, _)) = spans.first() else { return };
    let right_edge = spans.last().map_or(*left_edge, |(x, width)| x + width);

    for (index, (x, width)) in spans.iter().enumerate() {
        let cell = row.cells.get(index);
        // A cell of its own says what it wants; otherwise the table decides.
        let cell_borders = cell.map(|cell| &cell.borders);
        // A border the cell itself asks for, and nothing else.
        //
        // Not `line`, which answers an absent border with a gridline: a
        // gridline is the faint mark a table with no borders draws to show
        // where its cells are, and a cell that says nothing about an edge is
        // not asking for one of its own — the table decides that edge. Asking
        // `line` here meant every cell drew all four of its own edges as
        // gridlines, over whatever the table had drawn: two lines in every
        // place there should be one, and a pair of cells joined down the table
        // still had a line between them because the gridline outranked the
        // join.
        let cell_line = |pick: fn(&TableBorders) -> &Option<Border>| {
            cell_borders
                .and_then(|own| pick(own).as_ref())
                .filter(|border| border.is_visible())
                .map(|border| {
                    let colour =
                        border.color.as_deref().and_then(Color::from_hex).unwrap_or(automatic);
                    (Some(border.clone()), (border.width_points() * scale).max(1.0), colour)
                })
        };

        // A cell continuing the one above has no line between the two: that is
        // what makes them look like one cell.
        let continues = cell.is_some_and(|cell| cell.merged_upwards);
        let wanted = if continues { None } else { horizontal.clone() };
        if let Some(ruled) = cell_line(|borders| &borders.top).or(wanted) {
            rule(page, &ruled, Side::Top, *x, top, *width);
        }
        if let Some(ruled) = cell_line(|borders| &borders.bottom).or(below.clone()) {
            let (_, thickness, _) = ruled;
            rule(page, &ruled, Side::Bottom, *x, bottom - thickness, *width);
        }

        let vertical =
            if index == 0 { line(&borders.start) } else { line(&borders.inside_vertical) };
        if let Some(ruled) = cell_line(|borders| &borders.start).or(vertical) {
            rule(page, &ruled, Side::Start, *x, top, bottom - top);
        }
    }

    if let Some(ruled) = line(&borders.end) {
        let (_, thickness, _) = ruled;
        rule(page, &ruled, Side::End, right_edge - thickness, top, bottom - top);
    }
}

/// Draws one line of a table's grid.
///
/// The corner given is the one the line starts at, as the old code had it: the
/// band is centred half a thickness in from it, so a line sits inside the cell
/// rather than straddling its boundary. Which is what Word draws, and what the
/// eye expects of a ruled table.
fn rule(
    page: &mut Page,
    ruled: &(Option<Border>, f32, Color),
    side: Side,
    x: f32,
    y: f32,
    run: f32,
) {
    let (border, thickness, colour) = ruled;
    let Some(border) = border else {
        // A gridline, which is not a border and has no style of its own.
        let (width, height) = match side {
            Side::Top | Side::Bottom => (run, *thickness),
            Side::Start | Side::End => (*thickness, run),
        };
        page.decorations.push(Decoration { x, y, width, height, color: *colour });
        return;
    };

    let (at_x, at_y) = match side {
        Side::Top | Side::Bottom => (x, y + thickness / 2.0),
        Side::Start | Side::End => (x + thickness / 2.0, y),
    };
    crate::borders::draw_edge(page, border, side, at_x, at_y, run, *thickness, *colour);
}

/// Where a tab reaches from a given position.
///
/// A tab is a distance to a place, not a distance to travel: it advances to the
/// next stop, and always advances, so a tab sitting exactly on a stop moves to
/// the one after it. Stops are counted from the left edge of the text area, not
/// from the line, so an indented paragraph lines up with an unindented one.
fn next_tab_stop(x: f32, origin: f32, gap: f32) -> f32 {
    if gap <= 0.0 {
        return x;
    }
    // How far along the line already is, counted from where the stops start.
    let along = x - origin;
    let next = (along / gap).floor() + 1.0;
    origin + next * gap
}

/// The ascent, descent and natural height of a line, from the tallest run in it.
fn line_metrics(line: &Line, items: &[Item], styles: &[RunStyle]) -> (f32, f32, f32) {
    let mut ascent = 0.0f32;
    let mut descent = 0.0f32;
    let mut height = 0.0f32;

    for index in line.items.clone() {
        if let Some(style) = styles.get(items[index].style) {
            ascent = ascent.max(style.ascent);
            descent = descent.max(style.descent);
            height = height.max(style.line_height);
        }
        // A drawing that floats is not part of the line. See [`Item::floats`].
        if items[index].floats() {
            continue;
        }
        // A picture stands on the baseline, so the line has to be tall enough
        // to hold it or it would be drawn over the paragraph above.
        // An equation stands on the baseline too, and reaches further above it
        // than any letter does.
        // A chart stands on the baseline too.
        if let Some((_, chart_height)) = &items[index].chart {
            ascent = ascent.max(*chart_height);
            height = height.max(*chart_height + descent);
        }
        // And so does ink, which is a drawing like any other as far as the
        // line is concerned.
        if let Some((_, ink_height)) = &items[index].ink {
            ascent = ascent.max(*ink_height);
            height = height.max(*ink_height + descent);
        }
        if let Some((laid, _)) = &items[index].math {
            ascent = ascent.max(laid.ascent);
            descent = descent.max(laid.descent);
            height = height.max(laid.ascent + laid.descent);
        }
        // A reading sits above the line, so the line has to be tall enough for
        // it — or it is drawn over the words of the line before.
        if let Some((laid, _)) = &items[index].ruby {
            ascent = ascent.max(laid.ascent);
            descent = descent.max(laid.descent);
            height = height.max(laid.ascent + laid.descent);
        }
        if let Some((_, picture_height)) = &items[index].picture {
            ascent = ascent.max(*picture_height);
            height = height.max(*picture_height + descent);
        }
        // A shape and a group stand on the baseline too, and a group is as
        // tall as the box it covers however much or little is drawn in it.
        for standing in [
            &items[index].shape.as_ref().map(|(_, height)| *height),
            &items[index].group.as_ref().map(|(_, height)| *height),
        ] {
            let Some(standing) = standing else { continue };
            ascent = ascent.max(*standing);
            height = height.max(*standing + descent);
        }
    }

    if height == 0.0 {
        // An empty line still occupies space, taken from whatever style is at
        // hand rather than collapsing to nothing.
        if let Some(style) = styles.first() {
            ascent = style.ascent;
            descent = style.descent;
            height = style.line_height;
        }
    }

    (ascent, descent, height)
}

/// A piece of text that a line may be broken around.
struct Chunk {
    text: String,
    is_space: bool,
}

/// Splits text into the pieces a line may be broken between.
///
/// Where a break is allowed is [`wp_break`]'s business: it knows that a line
/// may end after a hyphen but never before one, that a non-breaking space is
/// not a place to break, and that Chinese and Japanese are written without
/// spaces and so wrap between the characters themselves.
///
/// The spaces are then split off into pieces of their own, because a line drops
/// the spaces that fall at its end and justification widens the ones that do
/// not.
fn segment(text: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut start = 0;
    for end in wp_break::opportunities(text).into_iter().chain([text.len()]) {
        push_piece(&mut chunks, &text[start..end]);
        start = end;
    }
    chunks
}

/// Adds one unbreakable piece, with the spaces it ends with split off.
fn push_piece(chunks: &mut Vec<Chunk>, piece: &str) {
    let head = piece.trim_end_matches(collapses_at_a_line_end);
    if !head.is_empty() {
        chunks.push(Chunk { text: head.to_owned(), is_space: false });
    }
    if head.len() < piece.len() {
        chunks.push(Chunk { text: piece[head.len()..].to_owned(), is_space: true });
    }
}

/// Whether a space is one that disappears at the end of a line.
///
/// A non-breaking space is not: it is drawn wherever it falls, which is the
/// whole point of it.
fn collapses_at_a_line_end(character: char) -> bool {
    character.is_whitespace() && wp_break::class_of(character) != wp_break::Class::GL
}

/// The first and last characters of a run's text, for deciding whether a line
/// may be broken between two runs.
///
/// A run that begins or ends with something that is not text — a tab, a
/// picture, a break — has no character there, and a break beside it is always
/// allowed.
fn run_edges(run: &Run) -> (Option<char>, Option<char>) {
    fn text(content: Option<&RunContent>) -> Option<&str> {
        match content {
            Some(RunContent::Text(text)) => Some(text.as_str()),
            _ => None,
        }
    }
    let first = text(run.content.first()).and_then(|text| text.chars().next());
    let last = text(run.content.last()).and_then(|text| text.chars().last());
    (first, last)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page holding one picture, which may or may not be a video.
    fn page_with_picture(video: bool) -> Page {
        let mut page = Page { width: 600.0, height: 800.0, ..Page::default() };
        page.images.push(PlacedImage {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 120.0,
            image: Rc::new(wp_image::Image::empty(2, 2)),
            depth: 0,
            over_text: false,
            at: None,
            turn: 0.0,
            flipped_across: false,
            flipped_down: false,
            name: "Video 1".to_owned(),
            video,
        });
        page
    }

    #[test]
    fn a_video_is_drawn_with_the_play_sign_over_it() {
        let mut pages = [page_with_picture(true)];
        mark_videos(&mut pages);
        // The circle and the triangle in it.
        assert_eq!(pages[0].paths.len(), 2);
        assert_eq!(pages[0].paths[1].color, Color::WHITE);
        assert!(pages[0].paths[0].color.alpha < 255, "the sign hides what is under it");
    }

    #[test]
    fn the_play_sign_sits_in_the_middle_of_the_frame() {
        let mut pages = [page_with_picture(true)];
        mark_videos(&mut pages);

        let mut bounds = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (path, _) in pages[0].paths.iter().map(|placed| (&placed.path, placed.color)) {
            for command in &path.commands {
                let points = match command {
                    wp_raster::Command::MoveTo(point) | wp_raster::Command::LineTo(point) => {
                        vec![*point]
                    }
                    wp_raster::Command::QuadTo(one, two) => vec![*one, *two],
                    wp_raster::Command::CubicTo(one, two, three) => vec![*one, *two, *three],
                    wp_raster::Command::Close => Vec::new(),
                };
                for point in points {
                    bounds.0 = bounds.0.min(point.x);
                    bounds.1 = bounds.1.min(point.y);
                    bounds.2 = bounds.2.max(point.x);
                    bounds.3 = bounds.3.max(point.y);
                }
            }
        }
        let (middle_x, middle_y) = ((bounds.0 + bounds.2) / 2.0, (bounds.1 + bounds.3) / 2.0);
        assert!((middle_x - 200.0).abs() < 2.0, "the sign is at {middle_x} across");
        assert!((middle_y - 160.0).abs() < 2.0, "the sign is at {middle_y} down");
        // And inside the picture, which is what makes it a sign on the video
        // rather than a mark beside it.
        assert!(bounds.0 >= 100.0 && bounds.2 <= 300.0);
    }

    #[test]
    fn an_ordinary_picture_is_left_alone() {
        let mut pages = [page_with_picture(false)];
        mark_videos(&mut pages);
        assert!(pages[0].paths.is_empty());
    }

    #[test]
    fn text_splits_into_words_and_spaces() {
        let chunks = segment("hello world");
        let texts: Vec<&str> = chunks.iter().map(|chunk| chunk.text.as_str()).collect();
        assert_eq!(texts, ["hello", " ", "world"]);
        assert!(!chunks[0].is_space);
        assert!(chunks[1].is_space);
    }

    #[test]
    fn runs_of_spaces_stay_together() {
        let chunks = segment("a   b");
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[1].text, "   ");
    }

    #[test]
    fn cjk_characters_break_individually() {
        // Written without spaces, so a line may end between almost any two.
        let chunks = segment("永和九年");
        assert_eq!(chunks.len(), 4, "each ideograph should be its own break point");
    }

    #[test]
    fn latin_and_cjk_mix_correctly() {
        let chunks = segment("year 永和 done");
        let texts: Vec<&str> = chunks.iter().map(|chunk| chunk.text.as_str()).collect();
        assert_eq!(texts, ["year", " ", "永", "和", " ", "done"]);
    }

    #[test]
    fn page_metrics_default_to_a4() {
        let metrics = PageMetrics::default();
        // A4 is 595 by 842 points.
        assert!((metrics.width - 595.3).abs() < 1.0);
        assert!((metrics.height - 841.9).abs() < 1.0);
        assert!(metrics.text_width() > 0.0);
    }

    /// A page of plain lines, ten pixels to the letter, built without fonts.
    ///
    /// Selection geometry is arithmetic on positions, so it can be tested on a
    /// page laid out by hand — and then the answers are exact rather than
    /// whatever the machine's fonts happen to measure.
    fn ruled_page(paragraphs: &[(usize, &str)]) -> Page {
        let mut page = Page { width: 400.0, height: 300.0, ..Page::default() };

        for (index, (paragraph, text)) in paragraphs.iter().enumerate() {
            let baseline = 20.0 + index as f32 * 20.0;
            let first = page.glyphs.len();
            let mut offset = 0usize;

            for character in text.chars() {
                page.glyphs.push(PositionedGlyph {
                    face: 0,
                    glyph: GlyphId(0),
                    x: 10.0 + offset as f32 * 10.0,
                    baseline,
                    advance: 10.0,
                    size: 12.0,
                    stretch: 1.0,
                    color: Color::BLACK,
                    effect: None,
                    source: TextPosition::new(*paragraph, offset),
                    source_length: character.len_utf8(),
                    invisible: false,
                    shift_x: 0.0,
                    shift_y: 0.0,
                });
                offset += character.len_utf8();
            }

            page.lines.push(PageLine {
                baseline,
                ascent: 12.0,
                descent: 4.0,
                left: 10.0,
                right: 10.0 + offset as f32 * 10.0,
                glyphs: first..page.glyphs.len(),
                paragraph: *paragraph,
                start_offset: 0,
                end_offset: offset,
                frame: Frame::default(),
            });
        }

        page
    }

    #[test]
    fn nothing_is_highlighted_without_a_selection() {
        let page = ruled_page(&[(0, "abcdef")]);
        let at = TextPosition::new(0, 3);
        assert!(page.selection_rects(at, at).is_empty(), "a caret is not a selection");
    }

    #[test]
    fn a_selection_inside_one_line_is_one_band() {
        let page = ruled_page(&[(0, "abcdef")]);
        let rects = page.selection_rects(TextPosition::new(0, 1), TextPosition::new(0, 4));

        assert_eq!(rects.len(), 1);
        let (x, _, width, height) = rects[0];
        assert!((x - 20.0).abs() < 0.01, "should start at the second letter");
        assert!((width - 30.0).abs() < 0.01, "three letters wide");
        assert!((height - 16.0).abs() < 0.01);
    }

    #[test]
    fn a_selection_to_the_end_of_a_line_reaches_its_right_edge() {
        let page = ruled_page(&[(0, "abcdef")]);
        let rects = page.selection_rects(TextPosition::new(0, 4), TextPosition::new(0, 6));

        assert_eq!(rects.len(), 1);
        let (x, _, width, _) = rects[0];
        assert!((x + width - 70.0).abs() < 0.01, "should end where the text does");
    }

    #[test]
    fn a_selection_across_paragraphs_gives_a_band_per_line() {
        let page = ruled_page(&[(0, "first"), (1, "middle"), (2, "last")]);
        let rects = page.selection_rects(TextPosition::new(0, 2), TextPosition::new(2, 2));

        assert_eq!(rects.len(), 3);
        // The middle paragraph is covered end to end.
        assert!((rects[1].0 - 10.0).abs() < 0.01);
        assert!((rects[1].2 - 65.0).abs() < 0.01, "six letters plus the break");
    }

    #[test]
    fn a_selected_paragraph_break_is_shown_past_the_last_letter() {
        // Otherwise nothing on screen says that Delete will join the two.
        let page = ruled_page(&[(0, "ab"), (1, "cd")]);
        let rects = page.selection_rects(TextPosition::new(0, 2), TextPosition::new(1, 1));

        assert_eq!(rects.len(), 2);
        assert!(rects[0].2 > 0.0, "the break itself should be highlighted");
        assert!((rects[0].2 - BREAK_HIGHLIGHT_WIDTH).abs() < 0.01);
    }

    #[test]
    fn a_paragraph_outside_the_selection_is_left_alone() {
        let page = ruled_page(&[(0, "one"), (1, "two"), (2, "three")]);
        let rects = page.selection_rects(TextPosition::new(1, 0), TextPosition::new(1, 3));

        assert_eq!(rects.len(), 1);
        assert!((rects[0].1 - 28.0).abs() < 0.01, "should be the second line");
    }

    #[test]
    fn a_wrapped_paragraph_gets_no_break_band_on_its_first_line() {
        // Both lines belong to paragraph 0, so only the last one carries the
        // break — a band after the first would look like a stray space.
        let mut page = ruled_page(&[(0, "abcd"), (0, "efgh"), (1, "next")]);
        page.lines[1].start_offset = 4;
        page.lines[1].end_offset = 8;
        for index in page.lines[1].glyphs.clone() {
            page.glyphs[index].source.offset += 4;
        }

        let rects = page.selection_rects(TextPosition::new(0, 0), TextPosition::new(1, 2));
        assert_eq!(rects.len(), 3);
        assert!((rects[0].2 - 40.0).abs() < 0.01, "the first line stops at its text");
        assert!(rects[1].2 > 40.0, "the second line carries the break");
    }
}

#[cfg(test)]
mod tab_tests {
    use super::*;

    #[test]
    fn a_tab_reaches_the_next_stop() {
        // Half an inch at 96 dpi is 48 pixels.
        assert!((next_tab_stop(0.0, 0.0, 48.0) - 48.0).abs() < 0.01);
        assert!((next_tab_stop(10.0, 0.0, 48.0) - 48.0).abs() < 0.01);
        assert!((next_tab_stop(47.9, 0.0, 48.0) - 48.0).abs() < 0.01);
    }

    #[test]
    fn a_tab_sitting_on_a_stop_moves_to_the_next_one() {
        // Otherwise pressing Tab twice would do nothing the second time.
        assert!((next_tab_stop(48.0, 0.0, 48.0) - 96.0).abs() < 0.01);
        assert!((next_tab_stop(96.0, 0.0, 48.0) - 144.0).abs() < 0.01);
    }

    #[test]
    fn stops_are_counted_from_the_text_area_not_the_line() {
        // A margin of 96 pixels: the first stop is 48 past it, not 48 from zero.
        assert!((next_tab_stop(96.0, 96.0, 48.0) - 144.0).abs() < 0.01);
        assert!((next_tab_stop(100.0, 96.0, 48.0) - 144.0).abs() < 0.01);
    }

    #[test]
    fn a_tab_always_advances() {
        // A stop that could return the same position would hang a line.
        let mut x = 0.0f32;
        for _ in 0..5 {
            let next = next_tab_stop(x, 0.0, 48.0);
            assert!(next > x, "a tab must move the pen");
            x = next;
        }
    }

    #[test]
    fn a_gap_of_nothing_is_refused_rather_than_looping() {
        assert!((next_tab_stop(20.0, 0.0, 0.0) - 20.0).abs() < 0.01);
    }
}

/// The colour behind text that a highlight name stands for.
///
/// The format does not store a colour here: it stores one of a fixed list of
/// names, which is why Word's highlighter offers exactly fifteen colours and no
/// picker. These are the values Word draws them in.
#[must_use]
fn highlight_color(name: &str) -> Option<Color> {
    Some(match name {
        "black" => Color::rgb(0x00, 0x00, 0x00),
        "blue" => Color::rgb(0x00, 0x00, 0xFF),
        "cyan" => Color::rgb(0x00, 0xFF, 0xFF),
        "green" => Color::rgb(0x00, 0xFF, 0x00),
        "magenta" => Color::rgb(0xFF, 0x00, 0xFF),
        "red" => Color::rgb(0xFF, 0x00, 0x00),
        "yellow" => Color::rgb(0xFF, 0xFF, 0x00),
        "white" => Color::rgb(0xFF, 0xFF, 0xFF),
        "darkBlue" => Color::rgb(0x00, 0x00, 0x80),
        "darkCyan" => Color::rgb(0x00, 0x80, 0x80),
        "darkGreen" => Color::rgb(0x00, 0x80, 0x00),
        "darkMagenta" => Color::rgb(0x80, 0x00, 0x80),
        "darkRed" => Color::rgb(0x80, 0x00, 0x00),
        "darkYellow" => Color::rgb(0x80, 0x80, 0x00),
        "darkGray" => Color::rgb(0x80, 0x80, 0x80),
        "lightGray" => Color::rgb(0xC0, 0xC0, 0xC0),
        // "none", and anything a future version of the format adds.
        _ => return None,
    })
}

impl LayoutEngine<'_> {
    /// What a field shows, when the run is inside one this engine understands.
    ///
    /// `None` means "show what the file cached", which is the right answer for
    /// every field whose instruction this does not know — a cross-reference, a
    /// table of contents, a mail-merge field. Showing a stale answer is much
    /// better than showing nothing.
    fn field_value(&self, run: &Run) -> Option<String> {
        let instruction = run.field.as_deref()?;
        let (page, pages, format) = self.field_page?;

        // The instruction is a little language: a name, then switches. Only the
        // name matters here.
        let name = instruction.split_whitespace().next()?.to_ascii_uppercase();
        match name.as_str() {
            // In the figures the section asks for: a book's front matter is
            // numbered i, ii, iii and its body starts again at 1.
            "PAGE" => Some(format.of(page)),
            "NUMPAGES" => Some(pages.to_string()),
            _ => None,
        }
    }

    /// Numbers the lines down the margin, where a section asks for it.
    ///
    /// # What is counted
    ///
    /// The lines of the text, and only those. Word leaves out the lines inside
    /// a table, and so does this: a numbered contract does not number the rows
    /// of its own schedule. A paragraph can also ask to be left out, which is
    /// what a heading in such a document usually does.
    ///
    /// The count runs on through the document, or starts again on every page,
    /// or on every section, as the section asks. Which numbers are printed is a
    /// separate question: counting by five prints every fifth, and counts the
    /// four in between just the same.
    fn number_lines(&mut self, pages: &mut [Page], document: &Document) {
        use wp_docx::appearance::Restart;

        let scale = self.pixels_per_point();
        let mut counted: u32 = 0;
        let mut last_section: Option<usize> = None;

        // What each section says about numbering its lines, and how wide its
        // margin is. Both mean finding the section breaks, and finding those
        // means walking the whole document — so they are asked once per section
        // rather than once per page.
        let sections = document.sections();
        let rules_of: Vec<Option<wp_docx::appearance::LineNumbers>> =
            (0..sections.len()).map(|section| document.line_numbers_of(section)).collect();

        for (index, page) in pages.iter_mut().enumerate() {
            let section = self.page_sections.get(index).copied().unwrap_or(0);
            let Some(rules) = rules_of.get(section).copied().flatten() else {
                last_section = Some(section);
                continue;
            };

            // Where the numbers hang: to the left of the text, by the distance
            // the section asks for, or a quarter of an inch when it says
            // nothing — which is what Word calls automatic.
            let metrics = PageMetrics::from_setup(&sections[section].setup);
            let text_left = metrics.margin_left * scale;
            let away = rules.distance.unwrap_or(360) as f32 / TWIPS_PER_POINT * scale;

            let starting = last_section.is_none_or(|last| match rules.restart {
                Restart::Continuous => false,
                Restart::NewSection => last != section,
                Restart::NewPage => true,
            });
            if starting || rules.restart == Restart::NewPage {
                counted = 0;
            }
            last_section = Some(section);

            // Gathered first: the numbers are drawn through the same shaping as
            // the text, which needs the engine while the page is borrowed.
            let mut wanted: Vec<(f32, String)> = Vec::new();
            for line in &page.lines {
                if document.paragraph_in_table(line.paragraph)
                    || document.suppresses_line_numbers(line.paragraph)
                {
                    continue;
                }
                let number = rules.start + counted;
                counted += 1;
                if number % rules.count_by != 0 {
                    continue;
                }
                wanted.push((line.baseline, number.to_string()));
            }

            // All at one size, the document's own: Word draws them through a
            // character style of its own rather than at the size of the line
            // they stand beside, so a heading does not get a giant number.
            let size = document.styles().resolve_run(None, &Default::default()).size_half_points
                as f32
                / 2.0;
            for (baseline, text) in wanted {
                let drawn = self.simple_line(&text, 0.0, baseline, size, self.automatic_color);
                let width = drawn.width;
                let at = text_left - away - width;
                let glyphs: Vec<PositionedGlyph> = drawn
                    .glyphs
                    .into_iter()
                    .map(|glyph| PositionedGlyph {
                        x: glyph.x + at,
                        // The numbers are not text: a click among them lands
                        // where a click in the margin lands, which is nowhere.
                        source_length: 0,
                        ..glyph
                    })
                    .collect();
                page.glyphs.extend(glyphs);
            }
        }
    }

    /// Puts each page's header and footer on it.
    ///
    /// Which one a page gets depends on the section it belongs to and on where
    /// it falls within that section: the first page of a section can have one
    /// of its own, and left-hand and right-hand pages can differ, both of which
    /// a document asks for and this only obeys. A document that asks for
    /// neither — which is nearly all of them — comes out of this the same as it
    /// always did: one header, on every page.
    fn place_all_furniture(
        &mut self,
        pages: &mut [Page],
        document: &Document,
        metrics: PageMetrics,
    ) {
        use wp_docx::furniture::{Furniture, Which};

        let total = pages.len();
        let belongs: Vec<usize> =
            (0..total).map(|page| self.page_sections.get(page).copied().unwrap_or(0)).collect();

        // What each page is numbered: not where it falls in the document, which
        // is what a header printing a page number would otherwise say.
        let numbers = document.page_numbers(&belongs);

        // Which of the three headers a page takes depends on the section it is
        // in, and asking the document that means finding the section breaks —
        // which means walking the document. Asked once per section here rather
        // than once per page: a thousand-page document asked it a thousand
        // times, and walked the whole document each time.
        let sections = belongs.iter().copied().max().unwrap_or(0) + 1;
        let first_differs: Vec<bool> =
            (0..sections).map(|section| document.different_first_page(section)).collect();
        let odd_and_even = document.different_odd_and_even();

        // Each header is read out of the package and parsed, so each one is
        // read once and kept: a hundred-page document would otherwise parse the
        // same header a hundred times over.
        let mut kept: HashMap<(usize, bool, Which), Option<Body>> = HashMap::new();

        for page in 0..total {
            let section = belongs[page];
            let first_of_section = page == 0 || belongs[page - 1] != section;
            let which = if first_of_section && first_differs[section] {
                Which::First
            } else if (page + 1) % 2 == 0 && odd_and_even {
                Which::Even
            } else {
                Which::Default
            };

            for (kind, is_footer) in [(Furniture::Header, false), (Furniture::Footer, true)] {
                let key = (section, is_footer, which);
                let found = kept
                    .entry(key)
                    .or_insert_with(|| document.furniture_of_page(kind, section, which));
                let Some(body) = found.as_ref() else { continue };
                self.place_furniture(
                    &mut pages[page..=page],
                    body,
                    document,
                    metrics,
                    is_footer,
                    Numbering { total, section, printed: numbers[page].0, format: numbers[page].1 },
                );
            }
        }
    }

    /// Lays a header or a footer onto a stretch of pages.
    ///
    /// Each page gets its own pass, because a page number is different on every
    /// one of them. What comes back is merged into the page rather than kept
    /// apart: it is drawn with the page and it is not text the caret can reach,
    /// so its lines are dropped.
    fn place_furniture(
        &mut self,
        pages: &mut [Page],
        body: &Body,
        document: &Document,
        metrics: PageMetrics,
        footer: bool,
        numbering: Numbering,
    ) {
        let scale = self.pixels_per_point();
        let (header_distance, footer_distance) = document.furniture_distances_of(numbering.section);

        for page in pages.iter_mut() {
            self.field_page = Some((numbering.printed, numbering.total, numbering.format));

            // Laid out at the top of a scratch page first, because how tall it
            // turns out decides where the footer starts.
            let area = Placement {
                left: metrics.margin_left * scale,
                top: 0.0,
                bottom_limit: f32::INFINITY,
                text_width: metrics.text_width() * scale,
                page_width: page.width,
                page_height: page.height,
                columns: 1,
                column_gap: 0.0,
                keeping: false,
            };

            // Counting starts afresh so a numbered list in the body is not
            // advanced by anything in the furniture.
            let saved = core::mem::replace(&mut self.counters, ListCounters::new());

            let lay_out = |engine: &mut Self| {
                let mut scratch =
                    vec![Page { width: page.width, height: page.height, ..Page::default() }];
                let mut y = 0.0f32;
                let mut column = 0usize;
                let mut counted = 0usize;
                engine.place_blocks(
                    &body.blocks,
                    &mut counted,
                    document,
                    &mut scratch,
                    &mut y,
                    &mut column,
                    area,
                    None,
                );
                (scratch.remove(0), y)
            };

            // A head's offset is known before anything is laid out. A foot's
            // is not — it depends on how tall the foot turns out — so the foot
            // is laid out once to learn that and again to place whatever it
            // carries against the paper. Twice over a few lines is cheaper
            // than a drawing in the wrong place.
            let (laid, offset) = if footer {
                let (_, height) = lay_out(self);
                // The footer's bottom edge sits the footer distance up from the
                // bottom of the page, which is what `w:footer` measures.
                let offset =
                    page.height - footer_distance as f32 / TWIPS_PER_POINT * scale - height;
                self.drawing_shift = -offset;
                let (laid, _) = lay_out(self);
                (laid, offset)
            } else {
                let offset = header_distance as f32 / TWIPS_PER_POINT * scale;
                self.drawing_shift = -offset;
                let (laid, _) = lay_out(self);
                (laid, offset)
            };
            self.drawing_shift = 0.0;
            self.counters = saved;

            merge_page(page, laid, offset);
        }

        self.field_page = None;
    }
}

/// Copies everything drawable from one page onto another, moved down by an
/// offset.
///
/// The lines are deliberately not copied: a header is drawn on the page but is
/// not part of the text, so a click in it must not put the caret there.
/// How far down a page what is on it reaches: its lines, its pictures, its
/// drawings, its cells and the rules and bands drawn with them.
fn content_bottom(page: &Page) -> f32 {
    let lines = page.lines.iter().map(|line| line.baseline + line.descent);
    let images = page.images.iter().map(|image| image.y + image.height);
    let shapes = page.shapes.iter().map(|shape| shape.y + shape.height);
    let cells = page.cells.iter().map(|cell| cell.y + cell.height);
    let decorations = page.decorations.iter().map(|decoration| decoration.y + decoration.height);
    lines.chain(images).chain(shapes).chain(cells).chain(decorations).fold(0.0, f32::max)
}

/// What a run is drawn as in an outline with its formatting turned off: the
/// document's own font at its own size, upright, unmarked and in the
/// automatic colour — Word's Show Text Formatting unticked.
///
/// What says what the text is rather than how it looks stays: which way it
/// reads, what language it is in, whether it is hidden, and whether it is
/// raised as a footnote's mark is.
fn plain_run(document: &Document, run: ResolvedRunProperties) -> ResolvedRunProperties {
    ResolvedRunProperties {
        right_to_left: run.right_to_left,
        language: run.language,
        hidden: run.hidden,
        no_proof: run.no_proof,
        vertical_align: run.vertical_align,
        ..document.styles().resolve_run(None, &Default::default())
    }
}

fn merge_page(page: &mut Page, from: Page, offset: f32) {
    let first = page.glyphs.len();
    for mut glyph in from.glyphs {
        glyph.baseline += offset;
        page.glyphs.push(glyph);
    }
    // Which of them are turned — a heading down a cell of a table in the
    // running head, a note's ideographs standing upright — comes with them.
    for (span, turn) in from.turned {
        page.turned.push(((span.start + first)..(span.end + first), turn));
    }
    for mut decoration in from.decorations {
        decoration.y += offset;
        page.decorations.push(decoration);
    }
    for mut image in from.images {
        image.y += offset;
        page.images.push(image);
    }
    // A running head can carry a floating drawing — a page number standing in
    // the margin is one — and until now it was laid out and then thrown away
    // here. It moves by the same offset as the words, because it was placed
    // with that offset already taken off: see [`LayoutEngine::drawing_shift`].
    for mut shape in from.shapes {
        shape.y += offset;
        page.shapes.push(shape);
    }
    // A path is already in page coordinates and carries no origin to move,
    // so it comes over as it is.
    page.paths.extend(from.paths);
}

/// The colour one author's changes are drawn in.
///
/// Word gives each reviewer a colour so two people's edits can be told apart at
/// a glance, and keeps it the same for that name across sessions. Choosing by
/// the name itself does the same without having to remember anything.
///
/// Public because a person's colour is theirs everywhere and not only in their
/// tracked changes: the stretches of a restricted document they are allowed to
/// edit are drawn in it as well, and two places choosing a colour for the same
/// name by two routes would sooner or later choose two colours.
#[must_use]
pub fn author_color(author: &str) -> Color {
    const COLOURS: [Color; 8] = [
        Color::rgb(0xC0, 0x25, 0x4B),
        Color::rgb(0x1F, 0x6F, 0xB2),
        Color::rgb(0x1E, 0x8A, 0x3C),
        Color::rgb(0x9B, 0x35, 0xA8),
        Color::rgb(0xB5, 0x62, 0x00),
        Color::rgb(0x00, 0x77, 0x8A),
        Color::rgb(0x7A, 0x4A, 0x1E),
        Color::rgb(0x5A, 0x3E, 0xC8),
    ];

    // A small deterministic mix of the bytes: the same name always lands on the
    // same colour, which is the whole point.
    let mut hash = 0u32;
    for byte in author.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u32::from(byte));
    }
    COLOURS[hash as usize % COLOURS.len()]
}

impl LayoutEngine<'_> {
    /// What number a note mark shows.
    fn note_number(&self, id: i32, endnote: bool) -> usize {
        self.note_numbers.get(&(endnote, id)).copied().unwrap_or(1)
    }

    /// Works out the numbers before anything is laid out.
    fn number_notes(&mut self, document: &Document) {
        self.note_numbers.clear();
        for kind in [wp_docx::notes::Kind::Footnote, wp_docx::notes::Kind::Endnote] {
            for note in document.notes(kind) {
                self.note_numbers.insert((kind.is_endnote(), note.id), note.number);
            }
        }
    }
}

impl LayoutEngine<'_> {
    /// How far down a page the text may go, once its footnotes are allowed for.
    ///
    /// Only the document's own body has footnotes under it. The room is kept
    /// from one layout to the next, so a header laid out on a page of its
    /// own must not read it as its own.
    fn limit(&self, page: usize, area: Placement) -> f32 {
        if !self.keeping {
            return area.bottom_limit;
        }
        area.bottom_limit - self.reserved.get(page).copied().unwrap_or(0.0)
    }

    /// Which notes belong to which page, and how much room they need.
    fn measure_footnotes(
        &mut self,
        pages: &[Page],
        notes: &[wp_docx::notes::Note],
        document: &Document,
        metrics: PageMetrics,
    ) -> Vec<f32> {
        let mut reserved = vec![0.0f32; pages.len()];
        let scale = self.pixels_per_point();

        let lines = Lines::of(pages);
        for note in notes {
            let Some(mark) = note.mark else { continue };
            let Some(page) = lines.page_of(mark) else { continue };
            // On the paper of the page the mark is on — and in its box, for
            // a section written down the page, where the notes run down the
            // page too and stand at its left.
            let paper = self.paper_of_page(page, document, metrics);
            let was = core::mem::replace(&mut self.vertical, paper.direction.is_vertical_writing());
            reserved[page] += self.note_height(note, document, paper.boxed());
            self.vertical = was;
        }

        // Room for the rule above the notes, on every page that has any.
        for room in &mut reserved {
            if *room > 0.0 {
                *room += SEPARATOR_SPACE * scale;
            }
        }
        reserved
    }

    /// How tall one note is once it is laid out.
    fn note_height(
        &mut self,
        note: &wp_docx::notes::Note,
        document: &Document,
        metrics: PageMetrics,
    ) -> f32 {
        let Some(body) = document.note_body(note.kind, note.id) else { return 0.0 };
        let mut scratch = vec![Page::default()];
        self.lay_out_note(&body, document, self.note_area(metrics), &mut scratch)
    }

    /// The area one note is laid out in: the width of the text, no bottom.
    fn note_area(&self, metrics: PageMetrics) -> Placement {
        let scale = self.pixels_per_point();
        Placement {
            left: metrics.margin_left * scale,
            top: 0.0,
            bottom_limit: f32::INFINITY,
            text_width: metrics.text_width() * scale,
            page_width: metrics.width * scale,
            page_height: metrics.height * scale,
            columns: 1,
            column_gap: 0.0,
            keeping: false,
        }
    }

    /// Lays a note out onto a scratch page and says how tall it came out.
    ///
    /// The list counters and the page reserve are put back afterwards: a note
    /// must not advance a numbered list in the body, and it must not be laid
    /// out into the room reserved for itself.
    fn lay_out_note(
        &mut self,
        body: &wp_docx::model::Body,
        document: &Document,
        area: Placement,
        pages: &mut Vec<Page>,
    ) -> f32 {
        let mut y = 0.0f32;
        let mut column = 0usize;
        let mut counted = 0usize;

        let counters = core::mem::replace(&mut self.counters, ListCounters::new());
        let reserved = core::mem::take(&mut self.reserved);
        self.place_blocks(
            &body.blocks,
            &mut counted,
            document,
            pages,
            &mut y,
            &mut column,
            area,
            None,
        );
        self.counters = counters;
        self.reserved = reserved;
        y
    }

    /// Ends the one sheet of a page on the web: the footnotes and then the
    /// endnotes after the text, under a rule, and the sheet as long as all of
    /// it and its bottom margin.
    ///
    /// An outline is ended on the same sheet and shows no notes at all: Word's
    /// outline is the headings and the text under them, and the web page it
    /// shares this sheet with is the one that carries the notes after the
    /// text. See [`Self::set_outline`].
    fn finish_web_page(&mut self, pages: &mut [Page], document: &Document, metrics: PageMetrics) {
        let Some(last) = pages.len().checked_sub(1) else { return };
        let scale = self.pixels_per_point();
        let left = metrics.margin_left * scale;
        let width = metrics.text_width() * scale;
        let mut y = pages.iter().map(content_bottom).fold(metrics.margin_top * scale, f32::max);

        let mut notes = Vec::new();
        if self.outline.is_none() {
            notes = document.notes(wp_docx::notes::Kind::Footnote);
            notes.extend(document.notes(wp_docx::notes::Kind::Endnote));
        }
        if !notes.is_empty() {
            y += SEPARATOR_SPACE * scale;
            pages[last].decorations.push(Decoration {
                x: left,
                y: y - SEPARATOR_SPACE * scale * 0.5,
                width: width / 3.0,
                height: 1.0,
                color: self.automatic_line,
            });
        }
        for note in notes {
            let Some(body) = document.note_body(note.kind, note.id) else { continue };
            let area = Placement {
                left,
                text_width: width,
                page_width: pages[last].width,
                page_height: pages[last].height,
                ..self.note_area(metrics)
            };
            let mut scratch = vec![Page {
                width: pages[last].width,
                height: pages[last].height,
                ..Page::default()
            }];
            let height = self.lay_out_note(&body, document, area, &mut scratch);
            merge_page(&mut pages[last], scratch.remove(0), y);
            y += height;
        }
        pages[last].height = y + metrics.margin_bottom * scale;
    }

    /// Draws the footnotes of each page at its foot, under a short rule.
    fn place_footnotes(
        &mut self,
        pages: &mut [Page],
        notes: &[wp_docx::notes::Note],
        document: &Document,
        metrics: PageMetrics,
    ) {
        let scale = self.pixels_per_point();

        // Grouped by page first, so the notes on one page stack in order.
        let mut by_page: HashMap<usize, Vec<&wp_docx::notes::Note>> = HashMap::new();
        let lines = Lines::of(pages);
        for note in notes {
            let Some(mark) = note.mark else { continue };
            let Some(page) = lines.page_of(mark) else { continue };
            by_page.entry(page).or_default().push(note);
        }

        let mut order: Vec<usize> = by_page.keys().copied().collect();
        order.sort_unstable();

        for index in order {
            let Some(mut on_page) = by_page.remove(&index) else { continue };
            on_page.sort_by_key(|note| note.number);
            let Some(page) = pages.get(index) else { continue };
            // The paper of this page, as its box: the notes go on before the
            // page is turned, so that they are turned with it.
            let paper = self.paper_of_page(index, document, metrics);
            let metrics = paper.boxed();
            let left = metrics.margin_left * scale;
            let width = metrics.text_width() * scale;
            let was = core::mem::replace(&mut self.vertical, paper.direction.is_vertical_writing());

            let bottom = page.height - metrics.margin_bottom * scale;
            let reserved = self.reserved.get(index).copied().unwrap_or(0.0);
            let mut y = bottom - reserved + SEPARATOR_SPACE * scale;

            // The rule Word draws above the notes, a third of the way across.
            let rule = y - SEPARATOR_SPACE * scale * 0.5;
            let colour = self.automatic_line;
            if let Some(page) = pages.get_mut(index) {
                page.decorations.push(Decoration {
                    x: left,
                    y: rule,
                    width: width / 3.0,
                    height: 1.0,
                    color: colour,
                });
            }

            for note in on_page {
                let Some(body) = document.note_body(note.kind, note.id) else { continue };
                let area = Placement {
                    left,
                    text_width: width,
                    page_width: pages[index].width,
                    page_height: pages[index].height,
                    ..self.note_area(metrics)
                };
                let mut scratch = vec![Page {
                    width: pages[index].width,
                    height: pages[index].height,
                    ..Page::default()
                }];
                let height = self.lay_out_note(&body, document, area, &mut scratch);

                // The lines are dropped: a note is drawn on the page but is not
                // text the caret can reach, because it lives in another part.
                merge_page(&mut pages[index], scratch.remove(0), y);
                y += height;
            }
            self.vertical = was;
        }
    }
}

/// The gap kept between the text and the rule above the notes, in points.
const SEPARATOR_SPACE: f32 = 12.0;

/// Which page a place in the text landed on.
/// Where every line of every page is, by the paragraph it shows.
///
/// A bookmark or a note mark is on the page whose line covers it, and a
/// document has hundreds of the one and thousands of the other: asked one at
/// a time of every line on every page, that was most of what a keystroke
/// cost on a long document with a table of contents.
struct Lines {
    by_paragraph: HashMap<usize, Vec<(usize, usize, usize)>>,
}

impl Lines {
    fn of(pages: &[Page]) -> Self {
        let mut by_paragraph: HashMap<usize, Vec<(usize, usize, usize)>> = HashMap::new();
        for (page, laid) in pages.iter().enumerate() {
            for line in &laid.lines {
                by_paragraph.entry(line.paragraph).or_default().push((
                    page,
                    line.start_offset,
                    line.end_offset,
                ));
            }
        }
        Self { by_paragraph }
    }

    /// The first page with a line covering the position.
    fn page_of(&self, at: wp_docx::TextPosition) -> Option<usize> {
        self.by_paragraph
            .get(&at.paragraph)?
            .iter()
            .find(|(_, start, end)| at.offset >= *start && at.offset <= *end)
            .map(|(page, _, _)| *page)
    }
}

impl LayoutEngine<'_> {
    /// Works out what every `SEQ` field in the document shows.
    ///
    /// A sequence is counted in reading order, so this is a walk over the whole
    /// body before anything is placed. Keyed by the paragraph and by which
    /// field of that paragraph it is, so laying the same paragraph out twice
    /// gives the same answer both times.
    fn number_sequences(&mut self, body: &Body) {
        self.sequence_numbers.clear();
        let mut counts: HashMap<String, usize> = HashMap::new();

        let mut paragraph_index = 0usize;
        count_sequences(
            &body.blocks,
            &mut paragraph_index,
            &mut counts,
            &mut self.sequence_numbers,
        );
    }

    /// Which page each bookmark begins on.
    fn locate_bookmarks(pages: &[Page], document: &Document) -> HashMap<String, usize> {
        let marks = document.bookmarks();
        let mut found = HashMap::new();
        if marks.is_empty() {
            return found;
        }
        let lines = Lines::of(pages);
        for mark in marks {
            if let Some(page) = lines.page_of(mark.range.0) {
                found.insert(mark.name, page + 1);
            }
        }
        found
    }

    /// What a sequence or a reference field shows.
    fn document_field_value(
        &self,
        instruction: &str,
        document: &Document,
        paragraph: usize,
        nth: usize,
    ) -> Option<String> {
        // A merge field shows the recipient being previewed, when there is one.
        if let Some(column) = wp_docx::merge::merge_column(instruction) {
            return self
                .merge_record
                .iter()
                .find(|(header, _)| header.eq_ignore_ascii_case(&column))
                .map(|(_, value)| value.clone());
        }

        // A formula reads the cells round it and does arithmetic on them. Worked
        // out here and never read back from the file, so a figure changed above
        // it changes the total under it. See [`wp_docx::formula`].
        if wp_docx::formula::is_formula(instruction) {
            return Some(document.formula_answer(instruction, paragraph));
        }

        // The fields that ask the document about itself. Worked out every time
        // rather than read from the file, so changing the title changes every
        // place that names it.
        let name = instruction.split_whitespace().next().unwrap_or_default().to_ascii_uppercase();
        let properties = || document.properties();
        match name.as_str() {
            "AUTHOR" => return Some(properties().author),
            "TITLE" => return Some(properties().title),
            "SUBJECT" => return Some(properties().subject),
            "KEYWORDS" => return Some(properties().keywords),
            "COMPANY" => return Some(properties().company),
            "CREATEDATE" => return Some(date_part(&properties().created)),
            "SAVEDATE" => return Some(date_part(&properties().modified)),
            _ => {}
        }

        if wp_docx::captions::sequence_name(instruction).is_some() {
            return Some(
                self.sequence_numbers.get(&(paragraph, nth)).copied().unwrap_or(1).to_string(),
            );
        }

        let (name, kind) = wp_docx::captions::reference_target(instruction)?;
        match kind {
            wp_docx::captions::Reference::Text => document.bookmark_text(name),
            // A page nobody has laid out yet is not known, and saying so is
            // better than saying one.
            wp_docx::captions::Reference::Page => {
                self.bookmark_pages.get(name).map(ToString::to_string)
            }
        }
    }
}

/// Counts the `SEQ` fields of a run of blocks, in reading order.
fn count_sequences(
    blocks: &[Block],
    paragraph: &mut usize,
    counts: &mut HashMap<String, usize>,
    out: &mut HashMap<(usize, usize), usize>,
) {
    for block in blocks {
        match block {
            Block::Paragraph(text) => {
                let mut nth = 0usize;
                for run in &text.runs {
                    let Some(instruction) = run.field.as_deref() else { continue };
                    if let Some(name) = wp_docx::captions::sequence_name(instruction) {
                        let count = counts.entry(name.to_owned()).or_insert(0);
                        *count += 1;
                        out.insert((*paragraph, nth), *count);
                    }
                    nth += 1;
                }
                *paragraph += 1;
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        count_sequences(&cell.blocks, paragraph, counts, out);
                    }
                }
            }
        }
    }
}

/// Word's automatic hyphenation, as the document has it set.
///
/// Word's Hyphenation Options, all four: whether words are broken at all
/// where nobody put a mark; whether a word in capitals may be; how close to
/// the margin a line has to come before a word is broken rather than carried
/// whole; and how many lines in a row may end with a hyphen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hyphenation {
    pub automatic: bool,
    pub capitals: bool,
    /// The zone, in pixels: a line whose last word would leave less room
    /// than this at the margin is left unbroken, and the word goes over
    /// whole. Word's default is a quarter of an inch.
    pub zone: f32,
    /// How many lines in a row may end with a hyphen; nought is no limit,
    /// which is what Word writes for it.
    pub limit: u32,
}

impl Default for Hyphenation {
    fn default() -> Self {
        Self { automatic: false, capitals: true, zone: 0.0, limit: 0 }
    }
}

impl Hyphenation {
    /// What the document says, with Word's own numbers where it says nothing.
    fn of(document: &Document, scale: f32) -> Self {
        let twips = document.hyphenation_zone().unwrap_or(360).max(0) as f32;
        Self {
            automatic: document.automatic_hyphenation(),
            capitals: document.hyphenate_capitals(),
            zone: twips / 20.0 * scale,
            limit: document.consecutive_hyphen_limit().unwrap_or(0).max(0) as u32,
        }
    }
}

/// What a line may do about a word that does not fit: break it where its
/// language allows, or carry it whole.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Breaking {
    /// Whether a word may be broken on this line at all — no, once as many
    /// lines in a row have ended with a hyphen as the document allows.
    allowed: bool,
    /// The hyphenation zone, in pixels. See [`Hyphenation::zone`].
    zone: f32,
}

/// Where a chunk of text may be broken, as byte offsets into it, by the
/// patterns of its language.
///
/// The word is the letters of the chunk with whatever is not a letter
/// stripped from either end: `(word,` is asked about as `word`. A word with
/// anything else inside it — a digit, an apostrophe — is not one the
/// patterns can speak for, and is not broken. Nor is a word in capitals,
/// when the document says to leave those alone.
fn word_cuts(chunk: &str, patterns: &wp_dict::hyphenation::Patterns, capitals: bool) -> Vec<usize> {
    let trimmed = chunk.trim_matches(|character: char| !character.is_alphabetic());
    if trimmed.is_empty() {
        return Vec::new();
    }
    let head = chunk.find(trimmed).unwrap_or(0);
    if !capitals && trimmed.chars().all(|character| !character.is_lowercase()) {
        return Vec::new();
    }
    patterns.breaks(trimmed).into_iter().map(|at| head + at).collect()
}

/// Cuts a chunk's glyphs into pieces at byte offsets of the chunk, keeping
/// each glyph with the characters it stands for. Each piece is its glyphs
/// and the offsets it begins and ends at. A cut that falls inside a glyph —
/// a ligature across the place — is not made: the pieces either side of it
/// stay one.
fn cut_glyphs(
    glyphs: Vec<ShapedGlyph>,
    start: usize,
    cuts: &[usize],
) -> Vec<(Vec<ShapedGlyph>, usize, usize)> {
    if cuts.is_empty() {
        let end = glyphs.last().map_or(start, |glyph| glyph.offset + glyph.length.max(1));
        return vec![(glyphs, start, end)];
    }
    let mut pieces: Vec<(Vec<ShapedGlyph>, usize, usize)> = Vec::new();
    let mut piece: Vec<ShapedGlyph> = Vec::new();
    let mut piece_start = start;
    let mut next_cut = 0usize;
    for glyph in glyphs {
        // A cut at the very cluster this glyph begins ends the piece before
        // it; a cut inside a glyph is passed over.
        while next_cut < cuts.len() && start + cuts[next_cut] <= glyph.offset {
            if start + cuts[next_cut] == glyph.offset && !piece.is_empty() {
                pieces.push((std::mem::take(&mut piece), piece_start, glyph.offset));
                piece_start = glyph.offset;
            }
            next_cut += 1;
        }
        piece.push(glyph);
    }
    let end = piece.last().map_or(piece_start, |glyph| glyph.offset + glyph.length.max(1));
    pieces.push((piece, piece_start, end));
    pieces
}

/// A percentage in thousandths of a per cent, as a fraction of one.
///
/// The unit the format counts shares in: 50000 is half. Kept to something
/// sensible, because a document may say anything.
fn share(thousandths: i32) -> f32 {
    (thousandths as f32 / 100_000.0).clamp(0.0, 10.0)
}

/// The narrowest stretch of room beside a drawing that counts as room at all,
/// in pixels.
///
/// A tenth of an inch at the usual ninety-six to the inch: about one letter of
/// ordinary text. A gap narrower than that would take a letter per line and
/// read as a column of nonsense, so it is left empty — which is what Word does
/// with the sliver between a picture and the margin.
const ROOM_FOR_TEXT: f32 = 9.6;

/// Breaks the next line out of a paragraph's items.
///
/// Greedy, as Word is: words are added until one does not fit and the line ends
/// before it. A single item wider than the line still goes on it, because there
/// is nowhere else for it to go and a line that fits nothing would never end.
fn break_next_line(items: &[Item], start: usize, available: f32, rules: Breaking) -> Line {
    break_next_line_fitting(items, start, available, false, rules)
}

/// The same, able to say that nothing fits.
///
/// `must_fit` is for a line laid out in pieces: a word too wide for the piece
/// beside a picture belongs in the piece on the other side of it, not cut in
/// half. The last piece has nowhere to pass a word on to, so it takes one
/// whatever its width — which is the rule this had always followed.
fn break_next_line_fitting(
    items: &[Item],
    start: usize,
    available: f32,
    must_fit: bool,
    rules: Breaking,
) -> Line {
    let mut index = start;
    let mut used = 0.0f32;
    // How much of the line was used at the last place it could have ended
    // before a word: what the line would be left with if the word went over
    // whole, which is what decides whether the word is broken instead.
    let mut used_at_opportunity = 0.0f32;

    // A space at the start of a line is dropped rather than indenting it.
    while index < items.len() && items[index].is_space && used == 0.0 {
        index += 1;
    }
    let first = index;
    let mut last_visible = first;
    // The last place the line could have ended, for when the item that does not
    // fit is one no line may begin with.
    let mut opportunity: Option<usize> = None;

    while index < items.len() {
        let item = &items[index];

        if item.hard_break.is_some() {
            return Line { items: first..index + 1, last_visible };
        }

        let would_be = used + item.width;
        // What the line costs if it ends here: an item that ends with an
        // optional hyphen has a hyphen drawn after it, and a line measured
        // without it is a line with a hyphen hanging past the margin.
        let closed = would_be + item.hyphen;
        // Nothing on the line yet and this will not fit: for a piece that has
        // somewhere to pass the word on to, the piece stays empty.
        if must_fit && closed > available && used == 0.0 && !item.is_space {
            return Line { items: first..first, last_visible: first };
        }
        if closed > available && used > 0.0 && !item.is_space {
            if item.breaks_before {
                return Line { items: first..index, last_visible };
            }
            // The rest of a word after a place its language allows a break:
            // the line may end here, with a hyphen after the piece before,
            // when the document lets it — and only when carrying the word
            // over whole would leave more room at the margin than the
            // hyphenation zone allows. Word's rule, and what the zone means.
            if item.auto_hyphen
                && rules.allowed
                && index > first
                && available - used_at_opportunity > rules.zone
            {
                return Line { items: first..index, last_visible };
            }
            // The item is glued to what comes before it — a closing bracket, a
            // full stop — so the line ends further back and the pair goes over
            // together.
            if let Some(end) = opportunity {
                return Line { items: first..end, last_visible: visible_end(items, first, end) };
            }
        }

        if index > first && item.breaks_before && !item.is_space {
            opportunity = Some(index);
            used_at_opportunity = used;
        }
        used = would_be;
        if !item.is_space {
            last_visible = index + 1;
        }
        index += 1;
    }

    Line { items: first..items.len(), last_visible }
}

/// Breaks the next line, cutting a word that is wider than the room it has.
///
/// # Why a word is ever cut
///
/// Because a cell can be narrower than a word, and so can a column. The rule
/// that a word too wide for the line goes on it anyway is right when the line
/// is the width of the page — there is nowhere else for it to go — and wrong
/// inside a table, where what it does is draw the word across the border of its
/// cell and over whatever is in the next one. It is what a bulleted list in a
/// narrow cell looked like, because the list's indent leaves a cell narrower
/// still.
///
/// Word cuts such a word between letters and carries the rest to the next line.
/// So the line is broken, and while what came out is one word that still does
/// not fit, that word is cut and the line broken again.
fn break_line(
    items: &mut Vec<Item>,
    levels: &mut Vec<u8>,
    cursor: usize,
    available: f32,
    rules: Breaking,
) -> Line {
    let mut line = break_next_line(items, cursor, available, rules);
    while line.items.end == line.items.start + 1
        && items
            .get(line.items.start)
            .is_some_and(|item| item.width > available && item.hard_break.is_none())
        && split_item(items, levels, line.items.start, available)
    {
        line = break_next_line(items, cursor, available, rules);
    }
    line
}

/// Cuts an item so that what is left of it fits a width.
///
/// The rest becomes an item of its own, right behind it, which the next line
/// picks up. Returns whether anything was cut: a single glyph wider than the
/// room has nowhere to be cut, and is drawn as it is.
///
/// The cut goes between clusters and never inside one, so a letter and the
/// accent shaped onto it stay together and an offset in the document still
/// falls between two glyphs rather than inside one.
fn split_item(items: &mut Vec<Item>, levels: &mut Vec<u8>, index: usize, available: f32) -> bool {
    let item = &items[index];
    // Only text is cut. A picture, an equation or a chart is one thing, and a
    // tab is measured when the line is placed rather than now.
    if item.is_tab || item.picture.is_some() || item.math.is_some() {
        return false;
    }
    if item.chart.is_some() || item.shape.is_some() || item.group.is_some() {
        return false;
    }
    if item.ink.is_some() || item.ruby.is_some() {
        return false;
    }

    // How many glyphs fit, counted to cluster boundaries: a glyph whose
    // character is the same as the one before it belongs with it.
    let mut used = 0.0f32;
    let mut fits = 0usize;
    let mut at_cluster = 0usize;
    for (position, glyph) in item.glyphs.iter().enumerate() {
        let starts_cluster = position == 0 || glyph.offset != item.glyphs[position - 1].offset;
        if starts_cluster {
            at_cluster = position;
        }
        if used + glyph.advance > available && at_cluster > 0 {
            fits = at_cluster;
            break;
        }
        used += glyph.advance;
        fits = position + 1;
    }

    // Nothing to gain: the whole item fits after all, or its first cluster does
    // not and cutting before it would leave an empty line.
    if fits == 0 || fits >= item.glyphs.len() {
        return false;
    }

    let mut rest = item.clone();
    rest.glyphs = rest.glyphs.split_off(fits);
    let mut head = items[index].clone();
    head.glyphs.truncate(fits);

    let boundary = rest.glyphs.first().map_or(head.end_offset, |glyph| glyph.offset);
    head.width = head.glyphs.iter().map(|glyph| glyph.advance).sum();
    head.end_offset = boundary;
    rest.width = rest.glyphs.iter().map(|glyph| glyph.advance).sum();
    rest.start_offset = boundary;
    // The line ends before what is left, which is the whole point of cutting.
    rest.breaks_before = true;
    // A hard break belongs to the end of the item, which is now the second one.
    head.hard_break = None;

    items[index] = head;
    items.insert(index + 1, rest);
    let level = levels.get(index).copied().unwrap_or(0);
    if index < levels.len() {
        levels.insert(index + 1, level);
    }
    true
}

/// Cuts the lines of a cell that the room it has leaves no place for.
///
/// A row of an exact height is that tall whatever is in it, and Word cuts the
/// text off where the row ends. This is the same cut made a line at a time: a
/// line that does not fit whole is not drawn at all, which keeps the text
/// inside the cell it belongs to. Word draws the top half of the line that
/// straddles the boundary, and doing that needs the glyphs clipped as they are
/// rasterized — named in the roadmap rather than pretended at here.
///
/// `already` is what the page held before the cell was placed: only what the
/// cell added is ever cut.
fn trim_cell(page: &mut Page, already: (usize, usize, usize), limit: f32) {
    let (glyphs_before, lines_before, decorations_before) = already;

    let mut cut = None;
    for index in lines_before..page.lines.len() {
        let line = &page.lines[index];
        let (_, top, _, height) =
            line.frame.rect(line.left, line.top(), 0.0, line.ascent + line.descent);
        if top + height > limit {
            cut = Some(index);
            break;
        }
    }
    let Some(cut) = cut else { return };

    let first_glyph = page.lines[cut].glyphs.start.max(glyphs_before);
    page.lines.truncate(cut);
    page.glyphs.truncate(first_glyph);
    // An underline under a line that is no longer drawn is not drawn either.
    let mut index = decorations_before;
    while index < page.decorations.len() {
        if page.decorations[index].y > limit {
            page.decorations.remove(index);
        } else {
            index += 1;
        }
    }
}

/// Where the drawn part of a line ends, once the spaces at its end are left
/// out.
fn visible_end(items: &[Item], first: usize, end: usize) -> usize {
    (first..end).rev().find(|index| !items[*index].is_space).map_or(first, |index| index + 1)
}

/// How tall a line is, once the paragraph's line spacing has had its say.
fn line_height(
    natural: f32,
    resolved: &wp_docx::model::ResolvedParagraphProperties,
    scale: f32,
) -> f32 {
    match resolved.line_spacing {
        Some(spacing) => match spacing.rule {
            // A multiple of single spacing, in 240ths.
            LineRule::Auto => natural * (spacing.value as f32 / 240.0),
            LineRule::Exact => spacing.value as f32 / TWIPS_PER_POINT * scale,
            LineRule::AtLeast => natural.max(spacing.value as f32 / TWIPS_PER_POINT * scale),
        },
        None => natural,
    }
}

/// A drawing floating on a page, and what the text is to do about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Float {
    pub page: usize,
    /// The box the text must keep out of, room round it included.
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub wrap: wp_docx::anchor::Wrap,
    /// Which side of it the text may run down. See
    /// [`wp_docx::anchor::WrapSide`].
    pub side: wp_docx::anchor::WrapSide,
    /// The shape and where it sits, for wrapping that follows its outline. A
    /// picture has none: a picture is the box it fills.
    pub outline: Option<Outline>,
}

/// The shape a float's text runs round, for tight and through wrapping.
///
/// The preset alone does not say what the shape is: a rounded rectangle with
/// its corner dragged square is a rectangle, and the text beside it should run
/// where the corner now is. So the handles travel with it. See
/// [`crate::geometry::Adjusts`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Outline {
    pub preset: crate::geometry::Preset,
    pub adjusts: crate::geometry::Adjusts,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The day out of a timestamp, which is what a date field shows.
///
/// A document's dates are written as `2026-09-08T11:07:00Z`; the time of day is
/// not what anybody means by the date a document was made.
fn date_part(stamp: &str) -> String {
    stamp.split('T').next().unwrap_or_default().to_owned()
}

/// Puts the play sign over every picture that stands for a video.
///
/// A pass of its own, after everything is placed, because a video may be in
/// the line or floating or inside a group, and what has to be true of all
/// three is the same: the sign goes over the middle of the frame, and it goes
/// over it rather than under it, which is what drawing it after the picture
/// and into the paths does.
///
/// Word draws the same sign, and it is the only thing that tells a video from
/// a picture of one.
fn mark_videos(pages: &mut [Page]) {
    use wp_raster::{Path, Point};

    for page in pages.iter_mut() {
        let mut badges = Vec::new();
        for image in page.images.iter().filter(|image| image.video) {
            let middle_x = image.x + image.width / 2.0;
            let middle_y = image.y + image.height / 2.0;
            // A fifth of the shorter side, which is what Word's own sign comes
            // to on a frame of any size.
            let radius = image.width.min(image.height) * 0.2;
            if radius < 2.0 {
                continue;
            }

            let mut circle = Path::new();
            crate::geometry::ellipse(&mut circle, middle_x, middle_y, radius, radius);
            // Dark and see-through, so a bright frame and a dark one both show
            // the sign and both still show what is under it.
            badges.push(PlacedPath { path: circle, color: Color::rgba(0, 0, 0, 170) });

            // The triangle, pointing the way a video plays. Set a little right
            // of the middle, because a triangle centred on its own box looks
            // left of centre inside a circle.
            let reach = radius * 0.45;
            let mut triangle = Path::new();
            triangle.move_to(Point::new(middle_x - reach * 0.7 + reach * 0.25, middle_y - reach));
            triangle.line_to(Point::new(middle_x + reach + reach * 0.25, middle_y));
            triangle.line_to(Point::new(middle_x - reach * 0.7 + reach * 0.25, middle_y + reach));
            triangle.close();
            badges.push(PlacedPath { path: triangle, color: Color::WHITE });
        }
        page.paths.extend(badges);
    }
}

/// How tall an item is, for the things that hang from the baseline.
fn item_height(item: &Item) -> f32 {
    let chart = item.chart.as_ref().map_or(0.0, |(_, height)| *height);
    let ink = item.ink.as_ref().map_or(0.0, |(_, height)| *height);
    chart.max(ink)
}

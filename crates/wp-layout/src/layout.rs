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
    VerticalAlignment,
};
use wp_docx::sections::{NumberFormat, Start};
use wp_docx::{Document, ListCounters, TextPosition};
use wp_font::{Font, GlyphId};
use wp_image::Image;
use wp_raster::Color;

use crate::library::FontLibrary;

/// Twentieths of a point, the unit the format measures almost everything in.
const TWIPS_PER_POINT: f32 = 20.0;
/// Points per inch, which is what makes a point a point.
const POINTS_PER_INCH: f32 = 72.0;

/// How far one level of an outline is indented past the one above it, in
/// points. A quarter of an inch, which is what Word steps by.
const OUTLINE_STEP: f32 = 18.0;

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

/// Space kept clear at the sides of a table cell when the table says nothing.
/// Word's own default, in twentieths of a point.
const DEFAULT_CELL_MARGIN_TWIPS: i32 = 108;
/// Space above and below the content of a cell, in pixels.
const CELL_PADDING_TOP: f32 = 2.0;
const CELL_PADDING_BOTTOM: f32 = 2.0;

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
}

impl PageLine {
    /// The vertical band the line occupies.
    #[must_use]
    pub fn top(&self) -> f32 {
        self.baseline - self.ascent
    }

    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.baseline + self.descent
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
    pub fill: Option<Color>,
    pub outline: Option<Color>,
    /// How thick the line round it is, in pixels.
    pub outline_weight: f32,
    /// The shadow under it, from the document theme, and how far it falls in
    /// pixels. See [`wp_docx::theme::Effect`].
    pub shadow: Option<(Color, f32)>,
    /// The glyphs of the text inside, already placed relative to the page.
    pub text: Vec<PositionedGlyph>,
    /// What the drawing is called, which is what a list of them shows.
    pub name: String,
    /// Where in the document the drawing is, so a press on it can put the
    /// caret beside it and the commands that act on it can find it.
    pub at: Option<TextPosition>,
    /// The shape this was placed from, kept until its text has been laid out.
    ///
    /// The text inside a shape is a document of its own, and laying one out
    /// while the outer one is being laid out would be the engine calling
    /// itself. So it is done in a pass of its own afterwards, and this is what
    /// that pass works from.
    pub(crate) source: Option<Box<wp_docx::shapes::Shape>>,
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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    /// Page size in pixels.
    pub width: f32,
    pub height: f32,
    pub glyphs: Vec<PositionedGlyph>,
    pub images: Vec<PlacedImage>,
    pub shapes: Vec<PlacedShape>,
    pub decorations: Vec<Decoration>,
    /// Shapes that are not rectangles: the slices of a pie, the line of a line
    /// chart. Already in page coordinates, and drawn over the decorations.
    pub paths: Vec<PlacedPath>,
    pub lines: Vec<PageLine>,
}

impl Page {
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

        // The line the point is on, or the nearest one above or below it.
        let line =
            self.lines.iter().find(|line| y >= line.top() && y <= line.bottom()).or_else(|| {
                self.lines.iter().min_by(|first, second| {
                    let distance = |line: &PageLine| {
                        if y < line.top() {
                            line.top() - y
                        } else {
                            y - line.bottom()
                        }
                    };
                    distance(first).total_cmp(&distance(second))
                })
            })?;

        // Before the first glyph or after the last, the answer is one end.
        if x <= line.left {
            return Some(TextPosition::new(line.paragraph, line.start_offset));
        }
        if x >= line.right {
            return Some(TextPosition::new(line.paragraph, line.end_offset));
        }

        for index in line.glyphs.clone() {
            let glyph = &self.glyphs[index];
            if x < glyph.x + glyph.advance {
                // Past the middle of a letter means the caret goes after it,
                // which is what makes clicking feel like it lands where aimed.
                let offset = if x > glyph.x + glyph.advance / 2.0 {
                    glyph.source.offset + glyph.source_length
                } else {
                    glyph.source.offset
                };
                return Some(TextPosition::new(glyph.source.paragraph, offset));
            }
        }

        Some(TextPosition::new(line.paragraph, line.end_offset))
    }

    /// Where a caret at a position should be drawn: its left edge, top, and
    /// height.
    #[must_use]
    pub fn caret_at(&self, position: TextPosition) -> Option<(f32, f32, f32)> {
        let line = self.line_of(position)?;

        let mut x = line.left;
        for index in line.glyphs.clone() {
            let glyph = &self.glyphs[index];
            if glyph.source.offset >= position.offset {
                x = glyph.x;
                break;
            }
            x = glyph.x + glyph.advance;
        }
        if position.offset >= line.end_offset {
            x = line.right;
        }

        Some((x, line.top(), line.ascent + line.descent))
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
            rects.push((left, line.top(), right - left, line.ascent + line.descent));
        }

        rects
    }

    /// Where an offset sits horizontally on one line.
    fn offset_x(&self, line: &PageLine, offset: usize) -> f32 {
        if offset >= line.end_offset {
            return line.right;
        }
        let mut x = line.left;
        for index in line.glyphs.clone() {
            let glyph = &self.glyphs[index];
            if glyph.source.offset >= offset {
                return glyph.x;
            }
            x = glyph.x + glyph.advance;
        }
        x
    }
}

/// How a run should look, reduced to what drawing needs.
#[derive(Clone, Debug)]
struct RunStyle {
    face: usize,
    size: f32,
    color: Color,
    /// The colour drawn behind the text, when the run asks for one.
    highlight: Option<Color>,
    underline: bool,
    strike: bool,
    right_to_left: bool,
    /// The shadow, outline, glow or reflection the run asks for.
    effect: Option<GlyphEffect>,
    /// How far off the line the run rides, positive upwards. Zero for text on
    /// the line, which is nearly all of it.
    raise: f32,
    ascent: f32,
    descent: f32,
    line_height: f32,
}

/// A glyph that has been chosen but not yet placed.
#[derive(Clone, Copy, Debug)]
struct ShapedGlyph {
    face: usize,
    glyph: GlyphId,
    advance: f32,
    /// Byte offset of the character it draws, within the paragraph text.
    offset: usize,
    /// Byte length of that character.
    length: usize,
}

/// The smallest thing a line can be broken between.
#[derive(Clone, Debug)]
struct Item {
    glyphs: Vec<ShapedGlyph>,
    width: f32,
    /// Whitespace collapses at the end of a line rather than being drawn.
    is_space: bool,
    /// Whether a line may end just before this item. Almost everything may
    /// begin one; a full stop that a change of formatting left in a run of its
    /// own may not.
    breaks_before: bool,
    /// A tab, whose width is not known until the line is being placed: it
    /// reaches to the next stop, which depends on where the line has got to.
    is_tab: bool,
    /// A picture drawn in the line, with the height it takes up.
    picture: Option<(Rc<Image>, f32)>,
    /// An equation drawn in the line, with the height it takes up.
    math: Option<(Box<crate::math::MathBox>, f32)>,
    /// A chart drawn in the line, with the height it takes up.
    chart: Option<(Box<crate::charting::ChartDrawing>, f32)>,
    /// A shape drawn in the line, with the height it takes up.
    shape: Option<(Box<wp_docx::shapes::Shape>, f32)>,
    /// Forces the rest of the paragraph onto a new line, or a new page.
    hard_break: Option<BreakKind>,
    style: usize,
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
    /// Whether tracked changes are shown as changes rather than as the text
    /// they would leave behind.
    show_markup: bool,
    /// How much of each page is kept for the footnotes printed at its foot.
    reserved: Vec<f32>,
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
    /// The shadow the document theme puts under a shape, if it puts one.
    ///
    /// Read once when the document starts being laid out, because a theme is a
    /// property of the document and asking the package for it per shape would
    /// re-parse the theme part for every drawing on the page.
    theme_effect: Option<wp_docx::theme::Effect>,
    /// How far apart the stops a tab falls back to are, in twentieths of a
    /// point.
    ///
    /// Read from the document when it is laid out, because every document
    /// says in its settings and half of them say something other than the
    /// half inch the format assumes.
    default_tab: i32,
    /// Which section each page of the document belongs to.
    ///
    /// Kept because a page cannot be asked: it is a picture of paper, and the
    /// header printed on it is the one belonging to the section whose text it
    /// holds. Filled in as the sections are laid out, and only for the document
    /// itself — a header or a footnote is laid out through the same code and
    /// has no sections of its own.
    page_sections: Vec<usize>,
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
            show_markup: true,
            note_numbers: HashMap::new(),
            sequence_numbers: HashMap::new(),
            bookmark_pages: HashMap::new(),
            reserved: Vec::new(),
            floats: Vec::new(),
            merge_record: Vec::new(),
            outline: None,
            outline_heading: 0,
            theme_effect: None,
            default_tab: DEFAULT_TAB_TWIPS,
            page_sections: Vec::new(),
        }
    }

    /// Sets whether tracked changes are drawn as changes.
    ///
    /// With this off the document is laid out as it would read once every
    /// change had been accepted, which is Word's "No Markup" view.
    #[must_use]
    pub fn with_markup(mut self, shown: bool) -> Self {
        self.show_markup = shown;
        self
    }

    /// Sets what colour text is drawn in when the document names none.
    ///
    /// Not a preference but a fact about the paper: on dark paper, automatic
    /// text is light. A colour the document does name is left alone.
    #[must_use]
    pub fn with_automatic_colors(mut self, text: Color, line: Color) -> Self {
        self.automatic_color = text;
        self.automatic_line = line;
        self
    }

    /// Shows the document as an outline: every paragraph indented to the depth
    /// of its heading, and anything deeper than `depth` left out.
    ///
    /// `Some(10)` is every level, headings and body text alike; `Some(1)` is
    /// the top headings alone. `None` shows the document as it is.
    #[must_use]
    pub fn with_outline(mut self, depth: Option<u8>) -> Self {
        self.outline = depth;
        self
    }
    /// Sets the resolution. Larger values render the same page bigger.
    #[must_use]
    pub fn with_dpi(mut self, dpi: f32) -> Self {
        self.dpi = dpi.clamp(24.0, 1200.0);
        self
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
            face,
            size,
            color,
            highlight: None,
            effect: None,
            underline: wanted.underline,
            strike: wanted.strike,
            right_to_left: false,
            raise: 0.0,
            ascent: size,
            descent: size * 0.25,
            line_height: size * 1.25,
        };

        // A line of interface text is one direction throughout — a button's
        // name, a font's name, a measurement — so its direction is taken from
        // the text itself rather than through the whole algorithm, which needs
        // a paragraph to work on.
        let mut glyphs = self.shape(text, &style, 0);
        if wp_bidi::Direction::from_text(text).is_right_to_left() {
            glyphs.reverse();
        }

        let mut pen = x;
        for glyph in glyphs {
            page.glyphs.push(PositionedGlyph {
                face: glyph.face,
                glyph: glyph.glyph,
                x: pen,
                baseline,
                advance: glyph.advance,
                size,
                color,
                effect: style.effect,
                source: TextPosition::default(),
                source_length: glyph.length,
                invisible: false,
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

    /// Lays a whole document out into pages.
    /// Shows one recipient's values in place of the merge fields' names.
    ///
    /// What previewing a mail merge is: the letter is not changed, only what
    /// its fields are worked out to.
    #[must_use]
    pub fn with_merge_record(mut self, record: Vec<(String, String)>) -> Self {
        self.merge_record = record;
        self
    }

    pub fn layout_document(&mut self, document: &Document) -> Vec<Page> {
        self.layout_document_with(document, PageMetrics::from_document(document))
    }

    /// The same, on paper of a size the caller chooses.
    ///
    /// What a view mode is: draft and web layout are the same document on
    /// different paper, not a different way of laying one out.
    pub fn layout_document_with(&mut self, document: &Document, metrics: PageMetrics) -> Vec<Page> {
        self.theme_effect = Some(document.theme().effect);
        // Where the tabs fall back to when a paragraph names no stops of its
        // own, which every document says for itself.
        self.default_tab = document.default_tab_width();
        self.number_notes(document);
        self.number_sequences(document);

        // Footnotes take room away from the text on the page they belong to, and
        // which page a mark lands on depends on how much room the text has — so
        // the two are worked out together. Two passes settle it for any
        // ordinary document; Word does the same and also stops.
        let footnotes = document.notes(wp_docx::notes::Kind::Footnote);
        let mut pages = self.layout_body(&document.body(), document, metrics);
        if !footnotes.is_empty() {
            for _ in 0..2 {
                let reserved = self.measure_footnotes(&pages, &footnotes, document, metrics);
                if reserved == self.reserved {
                    break;
                }
                self.reserved = reserved;
                pages = self.layout_body(&document.body(), document, metrics);
            }
            self.place_footnotes(&mut pages, &footnotes, document, metrics);
            self.reserved = Vec::new();
        }

        // A `PAGEREF` can only be answered once there are pages to count, so the
        // document is laid out again with the answers in. One extra pass: a
        // page number that moves the text that moves the page number is a
        // document nobody can typeset, and Word gives up at the same point.
        let had_references = !self.bookmark_pages.is_empty();
        self.locate_bookmarks(&pages, document);
        if !self.bookmark_pages.is_empty() || had_references {
            pages = self.layout_body(&document.body(), document, metrics);
            if !footnotes.is_empty() {
                self.reserved = self.measure_footnotes(&pages, &footnotes, document, metrics);
                pages = self.layout_body(&document.body(), document, metrics);
                self.place_footnotes(&mut pages, &footnotes, document, metrics);
                self.reserved = Vec::new();
            }
        }

        // The numbers down the margin, where a section asks for them.
        self.number_lines(&mut pages, document);

        // The header and the footer go on afterwards, once there are pages to
        // put them on and a total for a page number to count towards.
        self.place_all_furniture(&mut pages, document, metrics);

        // And the text inside every shape, which is a document of its own and
        // so is laid out once the outer one has stopped moving.
        self.fill_shapes(&mut pages, document);
        pages
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

        for Stretch { blocks: range, metrics, start, section } in stretches {
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

            // A section on paper of its own has to begin a page of its own,
            // because a page is one size all the way down.
            let first = pages.is_empty();
            if first || start.on_a_new_page() {
                pages.push(Page { width: page_width, height: page_height, ..Page::default() });
                // One that has to begin on an even or an odd page takes a blank
                // page in front of it when the count comes out wrong, which is
                // how a chapter always opens on the same side of the paper.
                let wrong = match start {
                    Start::EvenPage => pages.len() % 2 == 1,
                    Start::OddPage => pages.len() % 2 == 0,
                    _ => false,
                };
                if wrong && !first {
                    pages.push(Page { width: page_width, height: page_height, ..Page::default() });
                }
                y = top;
                column = 0;
            }

            let Some(blocks) = body.blocks.get(range) else { continue };
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
            );
            // Everything pushed while this section was placed is that section's.
            belongs.resize(pages.len(), section);
        }

        if of_the_document {
            self.page_sections = belongs;
        }

        if pages.is_empty() {
            let scale = self.pixels_per_point();
            pages.push(Page {
                width: metrics.width * scale,
                height: metrics.height * scale,
                ..Page::default()
            });
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
        if !is_the_document {
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
            let before = Mark::here(pages, *y, *column, 0, 0);
            let counted = *index;

            match &blocks[position] {
                Block::Paragraph(paragraph) => {
                    let resolved = document.resolve_paragraph(paragraph);
                    self.place_paragraph(*index, paragraph, document, pages, y, column, area);
                    *index += 1;

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
                    self.place_table(table, index, document, pages, y, column, area);
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

        // Shown as an outline, a paragraph sits at the depth of its heading and
        // one deeper than the level being shown is not drawn at all. Body text
        // counts as one step past the heading above it, which is where a reader
        // expects to find it.
        let mut outline_indent = 0.0;
        if let Some(depth) = self.outline {
            let heading = resolved.outline_level.map(|level| level + 1);
            if let Some(level) = heading {
                self.outline_heading = level;
            }
            // A heading is shown down to its own level; body text only when
            // every level is being shown, which is what Word's list calls All
            // Levels and what nine headings plus one comes to.
            if heading.unwrap_or(OUTLINE_ALL) > depth {
                return;
            }
            // It is still indented as though it were a level below the heading
            // above it, which is where a reader looks for it.
            let level = heading.unwrap_or_else(|| self.outline_heading.saturating_add(1)).max(1);
            outline_indent = f32::from(level - 1) * OUTLINE_STEP * scale;
        }

        // The mark a list paragraph carries, and the indents its level asks
        // for. Counting has to happen here even when the mark is not drawn, so
        // that a list continues across a page.
        let (mark, level_indent_start, level_indent_hanging) = self.list_mark(&resolved, document);

        // Spacing is in twentieths of a point, like almost everything else.
        let space_before = resolved.space_before as f32 / TWIPS_PER_POINT * scale;
        let space_after = resolved.space_after as f32 / TWIPS_PER_POINT * scale;
        // A list level's indents apply only where nothing else set one: a
        // paragraph that says where it sits has already been believed.
        let authored_indent = resolved.indent_start != 0 || resolved.indent_first_line != 0;
        let (indent_twips, first_twips) = if authored_indent {
            (resolved.indent_start, resolved.indent_first_line)
        } else {
            (level_indent_start, -level_indent_hanging)
        };
        let indent_start = indent_twips as f32 / TWIPS_PER_POINT * scale + outline_indent;
        let indent_end = resolved.indent_end as f32 / TWIPS_PER_POINT * scale;
        let indent_first = first_twips as f32 / TWIPS_PER_POINT * scale;

        if resolved.page_break_before && !pages.last().is_some_and(|page| page.glyphs.is_empty()) {
            self.start_page(pages, y, column, area);
        }

        *y += space_before;

        let mut styles = Vec::new();
        let mut items = self.build_items(paragraph, document, &mut styles, index);

        // Which direction each piece of the paragraph is drawn in.
        //
        // Not a property of the run it came from: a Hebrew phrase inside an
        // English sentence, or a price inside a Hebrew one, is a run of its own
        // whatever the document says about the paragraph, and finding those runs
        // needs the whole paragraph at once. See [`wp_bidi`].
        let text = document.paragraph_text(index).unwrap_or_default();
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
                // whatever its characters are, which is what `w:rtl` means.
                if styles.get(item.style).is_some_and(|style| style.right_to_left) {
                    level | 1
                } else {
                    level
                }
            })
            .collect();
        for (item, level) in items.iter_mut().zip(&item_levels) {
            // A piece that reads right to left is drawn from its right-hand
            // end, so its glyphs go down in the other order.
            if level % 2 == 1 {
                item.glyphs.reverse();
            }
        }
        let items = items;

        if items.is_empty() {
            // An empty paragraph still takes up a line's worth of height, and
            // still needs a line recorded: a caret has to be able to sit in it.
            let height = self.empty_line_height(paragraph, document);
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
        while cursor < items.len() || number == 0 {
            let extra_first = if number == 0 { indent_first.max(0.0) } else { 0.0 };
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
            let mut line = break_next_line(&items, cursor, line_width);
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
                line = break_next_line(&items, cursor, line_width);
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
                    && is_one_line_left(&items, cursor, line_width)
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
                line = break_next_line(&items, cursor, line_width);
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

            let baseline = *y + ascent;
            let next = line.items.end;
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
                        line_left,
                        indent_first,
                        baseline,
                    );
                }
            }

            self.place_line(
                &line,
                &items,
                &styles,
                pages.last_mut().expect("there is always a page"),
                LinePlacement {
                    left: line_left,
                    origin: here.left,
                    width: line_width,
                    baseline,
                    ascent,
                    descent,
                    alignment: resolved.alignment,
                    paragraph_rtl: resolved.right_to_left,
                    is_last_line: is_last,
                    paragraph: index,
                    page: page_index,
                    area,
                },
                Composition { stops: &resolved.tab_stops, levels: &item_levels },
            );

            *y += height;
            // A drawing wrapped above and below pushes the text past its foot,
            // which is the whole of what that wrapping means.
            *y = self.past_top_and_bottom_floats(page_index, *y);

            number += 1;
            if next <= cursor && number > 0 && cursor < items.len() {
                // Nothing was consumed, which would loop forever. One item goes
                // on the line whatever its width, which is what the breaker
                // does for an item wider than the page.
                cursor += 1;
            } else {
                cursor = next;
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

        let edges = [
            (&resolved.borders.top, left, top, width, 0.0),
            (&resolved.borders.bottom, left, bottom, width, 0.0),
            (&resolved.borders.start, left, top, 0.0, height),
            (&resolved.borders.end, right, top, 0.0, height),
        ];
        for (border, x, y, run_width, run_height) in edges {
            let Some(border) = border else { continue };
            if !border.is_visible() {
                continue;
            }
            let thickness = (border.width_points() * scale).max(1.0);
            let colour = border.color.as_deref().and_then(Color::from_hex).unwrap_or(automatic);
            page.decorations.push(Decoration {
                // A vertical edge is as thick as the line and as tall as the
                // band; a horizontal one the other way round.
                x: if run_width > 0.0 { x } else { x - thickness / 2.0 },
                y: if run_height > 0.0 { y } else { y - thickness / 2.0 },
                width: if run_width > 0.0 { run_width } else { thickness },
                height: if run_height > 0.0 { run_height } else { thickness },
                color: colour,
            });
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
        let columns = column_widths(table, area.text_width, scale);
        if columns.is_empty() {
            return;
        }

        let borders = document
            .styles()
            .resolve_table_borders(table.style.as_deref())
            .overlaid_with(&table.borders);

        let margin_start = table.cell_margin_start.unwrap_or(DEFAULT_CELL_MARGIN_TWIPS) as f32
            / TWIPS_PER_POINT
            * scale;
        let margin_end = table.cell_margin_end.unwrap_or(DEFAULT_CELL_MARGIN_TWIPS) as f32
            / TWIPS_PER_POINT
            * scale;
        // A table sits in whichever column the flow has reached.
        let table_left =
            area.in_column(*column).left + table.indent as f32 / TWIPS_PER_POINT * scale;

        for (row_number, row) in table.rows.iter().enumerate() {
            let spans = cell_spans(row, &columns, table_left);

            // Measured first, on a page of its own that is then thrown away.
            let saved_counters = self.counters.clone();
            let mut scratch =
                vec![Page { width: area.page_width, height: area.page_height, ..Page::default() }];
            let mut scratch_index = *index;
            let mut height = 0.0f32;
            for (cell, (left, width)) in row.cells.iter().zip(&spans) {
                let mut cell_y = 0.0f32;
                self.place_cell(
                    cell,
                    &mut scratch_index,
                    document,
                    &mut scratch,
                    &mut cell_y,
                    area,
                    *left + margin_start,
                    (*width - margin_start - margin_end).max(1.0),
                );
                height = height.max(cell_y);
            }
            self.counters = saved_counters;

            if let Some(wanted) = row.height {
                height = height.max(wanted as f32 / TWIPS_PER_POINT * scale);
            }
            height += CELL_PADDING_TOP + CELL_PADDING_BOTTOM;

            // A row that will not fit starts a page, unless it would not fit on
            // an empty one either — in which case it has to overflow somewhere.
            if *y + height > self.limit(pages.len().saturating_sub(1), area)
                && !pages.last().is_some_and(|p| p.glyphs.is_empty())
            {
                self.start_page(pages, y, column, area);
            }

            let row_top = *y;
            for (cell, (left, width)) in row.cells.iter().zip(&spans) {
                let mut cell_y = row_top + CELL_PADDING_TOP;
                self.place_cell(
                    cell,
                    index,
                    document,
                    pages,
                    &mut cell_y,
                    area,
                    *left + margin_start,
                    (*width - margin_start - margin_end).max(1.0),
                );
            }

            let row_bottom = row_top + height;
            if let Some(page) = pages.last_mut() {
                draw_row_borders(
                    page,
                    &borders,
                    row,
                    &spans,
                    row_top,
                    row_bottom,
                    row_number,
                    table.rows.len(),
                    scale,
                    self.automatic_line,
                );
            }
            *y = row_bottom;
        }
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
        self.place_blocks(&cell.blocks, index, document, pages, y, &mut cell_column, cell_area);
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

        let decoded = document
            .embedded_part(&picture.relationship)
            .and_then(|bytes| wp_image::decode(bytes).ok())
            .filter(|image| !image.is_empty())
            .map(Rc::new);

        self.pictures.insert(picture.relationship.clone(), decoded.clone());
        decoded
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
                color: style.color,
                effect: style.effect,
                // The mark is not part of the document's text, so it points at
                // nothing a caret could reach.
                source: TextPosition::default(),
                source_length: 0,
                invisible: false,
            });
            pen += glyph.advance;
        }
    }

    /// The height an empty paragraph occupies.
    fn empty_line_height(&mut self, paragraph: &Paragraph, document: &Document) -> f32 {
        let properties = document.styles().resolve_run(paragraph.style(), &Default::default());
        match self.style_for(&properties) {
            Some(style) => style.line_height,
            None => 0.0,
        }
    }

    /// Turns a paragraph into measured, breakable items.
    fn build_items(
        &mut self,
        paragraph: &Paragraph,
        document: &Document,
        styles: &mut Vec<RunStyle>,
        paragraph_index: usize,
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

            let resolved = document.resolve_run(paragraph, run);
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
        paragraph_index: usize,
        field_number: usize,
    ) {
        for content in &run.content {
            match content {
                RunContent::Text(text) => {
                    // A run inside a field shows what the field works out, not
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
                    let chunks = segment(&shown);
                    // However long the shown text is, the chunks between them
                    // take up exactly the bytes the document holds, so the
                    // caret still lands where the text really is.
                    let mut left = text.len();
                    for (number, chunk) in chunks.iter().enumerate() {
                        let start = *offset;
                        let glyphs = self.shape(&chunk.text, style, start);
                        let width = glyphs.iter().map(|glyph| glyph.advance).sum();
                        let taken = if number + 1 == chunks.len() {
                            left
                        } else {
                            chunk.text.len().min(left)
                        };
                        left -= taken;
                        *offset += taken;
                        items.push(Item {
                            glyphs,
                            width,
                            is_space: chunk.is_space,
                            breaks_before: true,
                            is_tab: false,
                            picture: None,
                            shape: None,
                            math: None,
                            chart: None,
                            hard_break: None,
                            style: style_index,
                            start_offset: start,
                            end_offset: *offset,
                        });
                    }
                }
                RunContent::Tab => {
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
                        picture: None,
                        math: None,
                        chart: None,
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
                        picture: None,
                        math: None,
                        chart: None,
                        shape: None,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Chart(reference) => {
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
                        picture: None,
                        math: None,
                        chart: drawn.map(|drawing| (Box::new(drawing), height)),
                        shape: None,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Math(math) => {
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
                        picture: None,
                        shape: None,
                        math: Some((Box::new(laid), height)),
                        chart: None,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Shape(shape) => {
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
                        picture: None,
                        math: None,
                        chart: None,
                        shape: Some((Box::new(shape.clone()), height)),
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Picture(picture) => {
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
                        picture: decoded.map(|image| (image, height)),
                        shape: None,
                        math: None,
                        chart: None,
                        hard_break: None,
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
                RunContent::Break(kind) => {
                    let start = *offset;
                    *offset += 1;
                    items.push(Item {
                        glyphs: Vec::new(),
                        width: 0.0,
                        is_space: false,
                        breaks_before: true,
                        is_tab: false,
                        picture: None,
                        math: None,
                        chart: None,
                        shape: None,
                        hard_break: Some(*kind),
                        style: style_index,
                        start_offset: start,
                        end_offset: *offset,
                    });
                }
            }
        }
    }

    /// How much room a line has, once the drawings floating beside it are out
    /// of the way.
    ///
    /// Given where the line would start and how wide it would be, this returns
    /// where it actually starts and how wide it actually is. Where a drawing
    /// splits the room in two — text could run down either side of it — the
    /// wider side is taken, which is what Word does and what keeps a column of
    /// four words wide from appearing beside a picture.
    fn usable_span(&self, page: usize, left: f32, width: f32, top: f32, bottom: f32) -> (f32, f32) {
        if self.floats.is_empty() {
            return (left, width);
        }

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
                (
                    wp_docx::anchor::Wrap::Tight | wp_docx::anchor::Wrap::Through,
                    Some((preset, x, y, width, height)),
                ) => {
                    match crate::geometry::span_between(preset, x, y, width, height, top, bottom) {
                        // The room asked for round the drawing is kept either side
                        // of the outline, the same as it is either side of the box.
                        Some((reaches_left, reaches_right)) => (
                            reaches_left - (x - float.left),
                            reaches_right + (float.right - (x + width)),
                        ),
                        // The band is beside the box but not beside the shape: a
                        // line there is not blocked at all.
                        None => continue,
                    }
                }
                _ => (float.left, float.right),
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

        // The widest stretch left. A line has to be somewhere, so a drawing
        // that covers everything leaves the line where it was rather than
        // leaving it nowhere.
        let widest = free.into_iter().filter(|(start, end)| end > start).max_by(|first, second| {
            (first.1 - first.0)
                .partial_cmp(&(second.1 - second.0))
                .unwrap_or(core::cmp::Ordering::Equal)
        });
        match widest {
            Some((start, end)) => (start, (end - start).max(1.0)),
            None => (left, width),
        }
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
        use wp_docx::anchor::{Placement as Where, Relative};

        let scale = self.pixels_per_point();
        let emu = |value: i64| value as f32 / wp_docx::shapes::EMU_PER_POINT as f32 * scale;

        // Across the page. The text area is what "margin" and "column" both
        // mean here: a document of one column has them in the same place, and
        // one of several has the column as the nearer answer.
        let (band_left, band_width) = match anchor.horizontal_from {
            Relative::Page => (0.0, area.page_width),
            _ => (area.left, area.text_width),
        };
        let x = match &anchor.horizontal {
            Where::Aligned(edge) => match edge.as_str() {
                "center" => band_left + (band_width - width) / 2.0,
                "right" | "outside" => band_left + band_width - width,
                _ => band_left,
            },
            Where::Offset(distance) => band_left + emu(*distance),
        };

        // Down the page.
        let y = match anchor.vertical_from {
            Relative::Page => match &anchor.vertical {
                Where::Aligned(edge) => match edge.as_str() {
                    "center" => (area.page_height - height) / 2.0,
                    "bottom" => area.page_height - height,
                    _ => 0.0,
                },
                Where::Offset(distance) => emu(*distance),
            },
            Relative::Margin => match &anchor.vertical {
                Where::Aligned(edge) => match edge.as_str() {
                    "center" => area.top + (area.bottom_limit - area.top - height) / 2.0,
                    "bottom" => area.bottom_limit - height,
                    _ => area.top,
                },
                Where::Offset(distance) => area.top + emu(*distance),
            },
            // Paragraph and line both mean "from where the text is now", which
            // is what anchors a drawing to the words it belongs with.
            _ => match &anchor.vertical {
                Where::Offset(distance) => line_top + emu(*distance),
                Where::Aligned(_) => line_top,
            },
        };

        let (left_room, right_room, top_room, bottom_room) = anchor.distance;
        self.floats.push(Float {
            page: page_index,
            left: x - emu(left_room),
            top: y - emu(top_room),
            right: x + width + emu(right_room),
            bottom: y + height + emu(bottom_room),
            wrap: anchor.wrap,
            // The shape itself, so that tight wrapping can follow its outline
            // rather than the box round it.
            outline: Some((crate::geometry::Preset::from_word(&shape.preset), x, y, width, height)),
        });

        page.shapes.push(PlacedShape {
            x,
            y,
            width,
            height,
            preset: crate::geometry::Preset::from_word(&shape.preset),
            fill: shape.fill.as_deref().and_then(Color::from_hex),
            outline: shape.outline.as_deref().and_then(Color::from_hex),
            outline_weight: (shape.outline_points() * scale).max(1.0),
            shadow: self.shape_shadow(scale),
            text: Vec::new(),
            name: shape.name.clone(),
            at,
            source: Some(Box::new(shape.clone())),
        });
    }

    /// The shadow the theme puts under a shape, in pixels.
    ///
    /// Word's Design ▸ Effects, which is a property of the document rather than
    /// of any one shape — so it is asked for here and applied to every shape
    /// alike. See [`wp_docx::theme::Effect`].
    fn shape_shadow(&self, scale: f32) -> Option<(Color, f32)> {
        let (distance, alpha) = self.theme_effect?.shadow()?;
        // The theme writes it in English metric units; a point is 12,700 of
        // them, and the second effect style is the one a plain shape uses.
        let points = distance as f32 / 12_700.0 * 2.0;
        let share = (alpha as f32 / 100_000.0 * 255.0).clamp(0.0, 255.0) as u8;
        Some((Color::rgba(0, 0, 0, share), (points * scale).max(1.0)))
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
                // Tall enough that the text never runs off the end of it: what
                // does not fit in a shape is hidden by the shape's edge, not
                // carried onto a second one.
                let metrics = PageMetrics {
                    width,
                    height: f32::MAX / 4.0,
                    margin_top: 0.0,
                    margin_right: 0.0,
                    margin_bottom: 0.0,
                    margin_left: 0.0,
                    columns: 1,
                    column_gap: 0.0,
                };

                let inner = self.layout_body(&source.body(), document, metrics);
                let Some(first) = inner.first() else { continue };
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
    /// Chooses a face and works out the metrics for one set of run properties.
    fn style_for(&mut self, properties: &ResolvedRunProperties) -> Option<RunStyle> {
        let face =
            self.library.select(properties.font.as_deref(), properties.bold, properties.italic)?;

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

        Some(RunStyle {
            face,
            size,
            color,
            effect,
            highlight: properties.highlight.as_deref().and_then(highlight_color),
            underline: properties.underline.is_visible(),
            strike: properties.strike,
            right_to_left: properties.right_to_left,
            raise: full_size * raise_fraction,
            // The line keeps the height of full-sized text, so a superscript
            // does not make its line shorter than the ones around it.
            ascent: ascent.max(full_size * 0.8),
            descent,
            line_height: line_height.max(full_size * 1.2),
        })
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
        let style = RunStyle {
            face: self.face,
            size,
            color,
            effect: None,
            highlight: None,
            underline: false,
            strike: false,
            right_to_left: false,
            raise: 0.0,
            ascent: size * 0.8,
            descent: size * 0.2,
            line_height: size * 1.2,
        };

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
                color,
                effect: None,
                // A chart is one character of the document, so nothing inside
                // it points at a place of its own.
                source: wp_docx::TextPosition::new(0, 0),
                source_length: 0,
                invisible: false,
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
        let style = RunStyle {
            face: self.face,
            size,
            color,
            effect: None,
            highlight: None,
            underline: false,
            strike: false,
            right_to_left: false,
            raise: 0.0,
            ascent: size * 0.8,
            descent: size * 0.2,
            line_height: size * 1.2,
        };

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
                color,
                effect: None,
                // An equation is one character of the document however many
                // letters are drawn for it, so no glyph inside it points at a
                // place of its own.
                source: wp_docx::TextPosition::new(0, 0),
                source_length: 0,
                invisible: false,
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

    /// Turns text into glyphs, falling back to another font per character when
    /// the chosen one has no glyph for it.
    fn shape(&mut self, text: &str, style: &RunStyle, base_offset: usize) -> Vec<ShapedGlyph> {
        // Text in a script that is written joined has to be shaped as a whole:
        // which glyph a letter takes depends on its neighbours, so it cannot be
        // decided a character at a time.
        if text.chars().any(wp_shape::is_joining_script) {
            if let Some(glyphs) = self.shape_joined(text, style, base_offset) {
                return glyphs;
            }
        }

        let mut glyphs = Vec::with_capacity(text.len());
        let mut previous: Option<GlyphId> = None;

        for (local, character) in text.char_indices() {
            let mut chosen = None;

            if let Some(font) = self.font(style.face) {
                if let Some(glyph) = font.glyph_for(character) {
                    let units = f32::from(font.units_per_em());
                    let mut advance = f32::from(font.advance(glyph)) * style.size / units;
                    if let Some(previous) = previous {
                        advance += f32::from(font.kerning(previous, glyph)) * style.size / units;
                    }
                    chosen = Some((style.face, glyph, advance));
                }
            }

            if chosen.is_none() {
                // The face cannot draw this character. Another one may be able
                // to, and an empty box helps nobody.
                if let Some((face, glyph)) = self.library.fallback_for(character, false, false) {
                    if let Some(font) = self.font(face) {
                        let units = f32::from(font.units_per_em());
                        let advance = f32::from(font.advance(glyph)) * style.size / units;
                        chosen = Some((face, glyph, advance));
                    }
                }
            }

            match chosen {
                Some((face, glyph, advance)) => {
                    glyphs.push(ShapedGlyph {
                        face,
                        glyph,
                        advance,
                        offset: base_offset + local,
                        length: character.len_utf8(),
                    });
                    previous = Some(glyph);
                }
                None => previous = None,
            }
        }

        // Which way round the glyphs go is not decided here: it belongs to the
        // paragraph, not the run, and the bidirectional algorithm settles it
        // once the whole paragraph is known. See [`wp_bidi`].
        glyphs
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
    ) -> Option<Vec<ShapedGlyph>> {
        // The run's own face first: a document that asks for a font gets it.
        // Failing that, whichever face can draw the first letter, because a
        // face without the letters cannot have the rules for joining them.
        let face = self.face_for_joining(text, style)?;
        let font = self.font(face)?;
        let units = f32::from(font.units_per_em());

        let shaped = wp_shape::shape(font, text);
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
            let advance = f32::from(font.advance(entry.glyph)) * style.size / units;
            // A glyph reaches to wherever the next one starts, so a ligature
            // covers every character that went into it.
            let length = shaped
                .get(index + 1)
                .map_or(text.len(), |next| next.cluster)
                .saturating_sub(entry.cluster);

            glyphs.push(ShapedGlyph {
                face,
                glyph: entry.glyph,
                advance,
                offset: base_offset + entry.cluster,
                length: length.max(1),
            });
        }

        // In the order they are stored, not the order they are drawn: which way
        // round a piece of text goes is settled once for the whole paragraph,
        // by the bidirectional algorithm. See [`wp_bidi`].
        Some(glyphs)
    }

    /// A face that can both draw a joined script and shape it.
    fn face_for_joining(&mut self, text: &str, style: &RunStyle) -> Option<usize> {
        let first = text.chars().find(|character| wp_shape::is_joining_script(*character))?;

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
fn is_one_line_left(items: &[Item], cursor: usize, width: f32) -> bool {
    if cursor >= items.len() {
        return false;
    }
    let line = break_next_line(items, cursor, width);
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
                color: style.color,
                effect: style.effect,
                // The dots are not text: they all point at the tab that made
                // them, so a click among them lands on the tab.
                source,
                source_length: 0,
                invisible: false,
            });
            x += one.advance;
        }
    }
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
                    color: style.color,
                    effect: style.effect,
                    source: TextPosition::new(placement.paragraph, item.start_offset),
                    source_length: item.end_offset - item.start_offset,
                    invisible: true,
                });
                x += advance;
                continue;
            }

            let start_x = x;
            // A superscript or subscript sits off the line rather than on it.
            let glyph_baseline = baseline - style.raise;
            for glyph in &item.glyphs {
                page.glyphs.push(PositionedGlyph {
                    face: glyph.face,
                    glyph: glyph.glyph,
                    x,
                    baseline: glyph_baseline,
                    advance: glyph.advance,
                    size: style.size,
                    color: style.color,
                    effect: style.effect,
                    source: TextPosition::new(placement.paragraph, glyph.offset),
                    source_length: glyph.length,
                    invisible: false,
                });
                x += glyph.advance;
            }
            if let Some((shape, height)) = &item.shape {
                // A drawing that floats is not on the line at all: it is put
                // where its anchor says and the text keeps out of its way.
                if let Some(anchor) = shape.anchor.clone() {
                    let line_top = baseline - placement.ascent;
                    let shape = shape.as_ref().clone();
                    self.place_float(
                        &anchor,
                        page,
                        placement.page,
                        &placement.area,
                        line_top,
                        item.width.max(shape.width_points() as f32 * self.pixels_per_point()),
                        *height,
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
                    fill: shape.fill.as_deref().and_then(Color::from_hex),
                    outline: shape.outline.as_deref().and_then(Color::from_hex),
                    outline_weight: (shape.outline_points() * self.pixels_per_point()).max(1.0),
                    shadow: self.shape_shadow(self.pixels_per_point()),
                    text: Vec::new(),
                    name: shape.name.clone(),
                    at: Some(TextPosition::new(placement.paragraph, item.start_offset)),
                    source: Some(shape.clone()),
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
            } else if let Some((laid, _)) = &item.math {
                // An equation is laid out already, against a baseline of its
                // own; putting it on the line is moving it to this one.
                let (glyphs, rules) = laid.placed(x, baseline);
                page.glyphs.extend(glyphs);
                page.decorations.extend(rules);
                x += item.width;
            } else if let Some((image, height)) = &item.picture {
                // A picture sits on the baseline, like a very tall letter.
                page.images.push(PlacedImage {
                    x,
                    y: baseline - height,
                    width: item.width,
                    height: *height,
                    image: Rc::clone(image),
                });
                x += item.width;
            } else if item.is_tab {
                // A tab is a distance to a place, not a distance to travel.
                let following = Following { from: index + 1, end: line_end, items, styles };
                let (target, leader) = self.tab_reach(x, following, &placement, stops);
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
                    color: style.color,
                    effect: style.effect,
                    source: TextPosition::new(placement.paragraph, item.start_offset),
                    source_length: item.end_offset - item.start_offset,
                    invisible: true,
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
            if style.underline && drawn_width > 0.0 {
                page.decorations.push(Decoration {
                    x: start_x,
                    // Just below the baseline, scaled so it stays proportional.
                    y: baseline + placement.descent * 0.25,
                    width: drawn_width,
                    height: (style.size * 0.05).max(1.0),
                    color: style.color,
                });
            }
            if style.strike && drawn_width > 0.0 {
                page.decorations.push(Decoration {
                    x: start_x,
                    y: baseline - style.ascent * 0.3,
                    width: drawn_width,
                    height: (style.size * 0.05).max(1.0),
                    color: style.color,
                });
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
struct Stretch {
    /// The blocks it covers.
    blocks: core::ops::Range<usize>,
    /// The paper they are printed on.
    metrics: PageMetrics,
    /// How the stretch begins.
    start: Start,
    /// Which section of the document it is, for the header printed on it.
    section: usize,
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
fn column_widths(table: &Table, available: f32, scale: f32) -> Vec<f32> {
    let mut widths: Vec<f32> =
        table.grid.iter().map(|twips| (*twips).max(0) as f32 / TWIPS_PER_POINT * scale).collect();

    if widths.is_empty() {
        // Without a grid, take the widths the cells state, and failing that
        // divide the space evenly.
        let columns = table.rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
        if columns == 0 {
            return Vec::new();
        }
        let stated: Option<Vec<f32>> = table.rows.first().map(|row| {
            row.cells
                .iter()
                .map(|cell| cell.width.unwrap_or(0).max(0) as f32 / TWIPS_PER_POINT * scale)
                .collect()
        });
        widths = match stated {
            Some(stated) if stated.iter().all(|width| *width > 0.0) => stated,
            _ => vec![available / columns as f32; columns],
        };
    }

    let total: f32 = widths.iter().sum();
    if total <= 0.0 {
        let columns = widths.len().max(1);
        return vec![available / columns as f32; columns];
    }
    // A table wider than the page is squeezed to fit rather than run off it.
    if total > available {
        let factor = available / total;
        for width in &mut widths {
            *width *= factor;
        }
    }
    widths
}

/// Where each cell of a row sits, as a left edge and a width.
///
/// A cell covering several columns takes their widths together, which is what
/// makes a merged heading line up with the columns under it.
fn cell_spans(row: &TableRow, columns: &[f32], left: f32) -> Vec<(f32, f32)> {
    let mut spans = Vec::with_capacity(row.cells.len());
    let mut x = left;
    let mut column = 0usize;

    for cell in &row.cells {
        let span = cell.span.max(1) as usize;
        let width: f32 = columns.iter().skip(column).take(span).sum();
        // A row with more cells than the grid has columns still has to put them
        // somewhere, so anything past the end gets the last column's width.
        let width = if width > 0.0 { width } else { columns.last().copied().unwrap_or(0.0) };
        spans.push((x, width));
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
) {
    let line = |border: &Option<Border>| -> Option<(f32, Color)> {
        let border = border.as_ref()?;
        if !border.is_visible() {
            return None;
        }
        let color = border.color.as_deref().and_then(Color::from_hex).unwrap_or(automatic);
        Some(((border.width_points() * scale).max(1.0), color))
    };

    let first_row = row_number == 0;
    let last_row = row_number + 1 == row_count;
    let horizontal = if first_row {
        line(&borders.top)
    } else {
        line(&borders.inside_horizontal).or_else(|| line(&borders.top))
    };
    let below = if last_row {
        line(&borders.bottom)
    } else {
        line(&borders.inside_horizontal).or_else(|| line(&borders.bottom))
    };

    let Some((left_edge, _)) = spans.first() else { return };
    let right_edge = spans.last().map_or(*left_edge, |(x, width)| x + width);

    for (index, (x, width)) in spans.iter().enumerate() {
        let cell = row.cells.get(index);
        // A cell of its own says what it wants; otherwise the table decides.
        let cell_borders = cell.map(|cell| &cell.borders);
        let cell_line = |pick: fn(&TableBorders) -> &Option<Border>| {
            cell_borders.and_then(|own| line(pick(own)))
        };

        // A cell continuing the one above has no line between the two: that is
        // what makes them look like one cell.
        let continues = cell.is_some_and(|cell| cell.merged_upwards);
        if let Some((thickness, color)) =
            cell_line(|borders| &borders.top).or(if continues { None } else { horizontal })
        {
            page.decorations.push(Decoration {
                x: *x,
                y: top,
                width: *width,
                height: thickness,
                color,
            });
        }
        if let Some((thickness, color)) = cell_line(|borders| &borders.bottom).or(below) {
            page.decorations.push(Decoration {
                x: *x,
                y: bottom - thickness,
                width: *width,
                height: thickness,
                color,
            });
        }

        let vertical = if index == 0 {
            line(&borders.start)
        } else {
            line(&borders.inside_vertical).or_else(|| line(&borders.start))
        };
        if let Some((thickness, color)) = cell_line(|borders| &borders.start).or(vertical) {
            page.decorations.push(Decoration {
                x: *x,
                y: top,
                width: thickness,
                height: bottom - top,
                color,
            });
        }
    }

    if let Some((thickness, color)) = line(&borders.end) {
        page.decorations.push(Decoration {
            x: right_edge - thickness,
            y: top,
            width: thickness,
            height: bottom - top,
            color,
        });
    }
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
        // A picture stands on the baseline, so the line has to be tall enough
        // to hold it or it would be drawn over the paragraph above.
        // An equation stands on the baseline too, and reaches further above it
        // than any letter does.
        // A chart stands on the baseline too.
        if let Some((_, chart_height)) = &items[index].chart {
            ascent = ascent.max(*chart_height);
            height = height.max(*chart_height + descent);
        }
        if let Some((laid, _)) = &items[index].math {
            ascent = ascent.max(laid.ascent);
            descent = descent.max(laid.descent);
            height = height.max(laid.ascent + laid.descent);
        }
        if let Some((_, picture_height)) = &items[index].picture {
            ascent = ascent.max(*picture_height);
            height = height.max(*picture_height + descent);
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
    character.is_whitespace() && wp_break::class_of(character) != wp_break::Class::Glue
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
                    color: Color::BLACK,
                    effect: None,
                    source: TextPosition::new(*paragraph, offset),
                    source_length: character.len_utf8(),
                    invisible: false,
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

        for (index, page) in pages.iter_mut().enumerate() {
            let section = self.page_sections.get(index).copied().unwrap_or(0);
            let Some(rules) = document.line_numbers_of(section) else {
                last_section = Some(section);
                continue;
            };

            // Where the numbers hang: to the left of the text, by the distance
            // the section asks for, or a quarter of an inch when it says
            // nothing — which is what Word calls automatic.
            let metrics = PageMetrics::from_setup(&document.sections()[section].setup);
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

        // Each header is read out of the package and parsed, so each one is
        // read once and kept: a hundred-page document would otherwise parse the
        // same header a hundred times over.
        let mut kept: HashMap<(usize, bool, Which), Option<Body>> = HashMap::new();

        for page in 0..total {
            let section = belongs[page];
            let first_of_section = page == 0 || belongs[page - 1] != section;
            let which = document.which_for_page(section, first_of_section, page + 1);

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

            let mut scratch =
                vec![Page { width: page.width, height: page.height, ..Page::default() }];
            let mut y = 0.0f32;
            let mut column = 0usize;
            let mut counted = 0usize;
            // Counting starts afresh so a numbered list in the body is not
            // advanced by anything in the furniture.
            let saved = core::mem::replace(&mut self.counters, ListCounters::new());
            self.place_blocks(
                &body.blocks,
                &mut counted,
                document,
                &mut scratch,
                &mut y,
                &mut column,
                area,
            );
            self.counters = saved;

            let offset = if footer {
                // The footer's bottom edge sits the footer distance up from the
                // bottom of the page, which is what `w:footer` measures.
                page.height - footer_distance as f32 / TWIPS_PER_POINT * scale - y
            } else {
                header_distance as f32 / TWIPS_PER_POINT * scale
            };

            merge_page(page, scratch.remove(0), offset);
        }

        self.field_page = None;
    }
}

/// Copies everything drawable from one page onto another, moved down by an
/// offset.
///
/// The lines are deliberately not copied: a header is drawn on the page but is
/// not part of the text, so a click in it must not put the caret there.
fn merge_page(page: &mut Page, from: Page, offset: f32) {
    for mut glyph in from.glyphs {
        glyph.baseline += offset;
        page.glyphs.push(glyph);
    }
    for mut decoration in from.decorations {
        decoration.y += offset;
        page.decorations.push(decoration);
    }
    for mut image in from.images {
        image.y += offset;
        page.images.push(image);
    }
}

/// The colour one author's changes are drawn in.
///
/// Word gives each reviewer a colour so two people's edits can be told apart at
/// a glance, and keeps it the same for that name across sessions. Choosing by
/// the name itself does the same without having to remember anything.
#[must_use]
fn author_color(author: &str) -> Color {
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
    fn limit(&self, page: usize, area: Placement) -> f32 {
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

        for note in notes {
            let Some(mark) = note.mark else { continue };
            let Some(page) = page_of(pages, mark) else { continue };
            reserved[page] += self.note_height(note, document, metrics);
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
        self.place_blocks(&body.blocks, &mut counted, document, pages, &mut y, &mut column, area);
        self.counters = counters;
        self.reserved = reserved;
        y
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
        let left = metrics.margin_left * scale;
        let width = metrics.text_width() * scale;

        // Grouped by page first, so the notes on one page stack in order.
        let mut by_page: HashMap<usize, Vec<&wp_docx::notes::Note>> = HashMap::new();
        for note in notes {
            let Some(mark) = note.mark else { continue };
            let Some(page) = page_of(pages, mark) else { continue };
            by_page.entry(page).or_default().push(note);
        }

        let mut order: Vec<usize> = by_page.keys().copied().collect();
        order.sort_unstable();

        for index in order {
            let Some(mut on_page) = by_page.remove(&index) else { continue };
            on_page.sort_by_key(|note| note.number);
            let Some(page) = pages.get(index) else { continue };

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
        }
    }
}

/// The gap kept between the text and the rule above the notes, in points.
const SEPARATOR_SPACE: f32 = 12.0;

/// Which page a place in the text landed on.
fn page_of(pages: &[Page], at: wp_docx::TextPosition) -> Option<usize> {
    pages.iter().position(|page| {
        page.lines.iter().any(|line| {
            line.paragraph == at.paragraph
                && at.offset >= line.start_offset
                && at.offset <= line.end_offset
        })
    })
}

impl LayoutEngine<'_> {
    /// Works out what every `SEQ` field in the document shows.
    ///
    /// A sequence is counted in reading order, so this is a walk over the whole
    /// body before anything is placed. Keyed by the paragraph and by which
    /// field of that paragraph it is, so laying the same paragraph out twice
    /// gives the same answer both times.
    fn number_sequences(&mut self, document: &Document) {
        self.sequence_numbers.clear();
        let mut counts: HashMap<String, usize> = HashMap::new();

        let body = document.body();
        let mut paragraph_index = 0usize;
        count_sequences(
            &body.blocks,
            &mut paragraph_index,
            &mut counts,
            &mut self.sequence_numbers,
        );
    }

    /// Which page each bookmark begins on.
    fn locate_bookmarks(&mut self, pages: &[Page], document: &Document) {
        self.bookmark_pages.clear();
        for mark in document.bookmarks() {
            let at = mark.range.0;
            let found = pages.iter().position(|page| {
                page.lines.iter().any(|line| {
                    line.paragraph == at.paragraph
                        && at.offset >= line.start_offset
                        && at.offset <= line.end_offset
                })
            });
            if let Some(page) = found {
                self.bookmark_pages.insert(mark.name, page + 1);
            }
        }
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

/// Breaks the next line out of a paragraph's items.
///
/// Greedy, as Word is: words are added until one does not fit and the line ends
/// before it. A single item wider than the line still goes on it, because there
/// is nowhere else for it to go and a line that fits nothing would never end.
fn break_next_line(items: &[Item], start: usize, available: f32) -> Line {
    let mut index = start;
    let mut used = 0.0f32;

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
        if would_be > available && used > 0.0 && !item.is_space {
            if item.breaks_before {
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
        }
        used = would_be;
        if !item.is_space {
            last_visible = index + 1;
        }
        index += 1;
    }

    Line { items: first..items.len(), last_visible }
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
    /// The shape and where it sits, for wrapping that follows its outline. A
    /// picture has none: a picture is the box it fills.
    pub outline: Option<(crate::geometry::Preset, f32, f32, f32, f32)>,
}

/// The day out of a timestamp, which is what a date field shows.
///
/// A document's dates are written as `2026-09-08T11:07:00Z`; the time of day is
/// not what anybody means by the date a document was made.
fn date_part(stamp: &str) -> String {
    stamp.split('T').next().unwrap_or_default().to_owned()
}

/// How tall an item is, for the things that hang from the baseline.
fn item_height(item: &Item) -> f32 {
    item.chart.as_ref().map_or(0.0, |(_, height)| *height)
}

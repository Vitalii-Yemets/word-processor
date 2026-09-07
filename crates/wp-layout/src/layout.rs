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
//! * **No complex shaping.** Each character becomes one glyph. That is correct
//!   for Latin, Cyrillic, Greek and CJK, and wrong for Arabic and the Indic
//!   scripts, where letters change form depending on their neighbours. Those
//!   need the shaping engine, which is a later stage.
//! * **No bidirectional algorithm.** A run marked right-to-left has its glyphs
//!   reversed, which puts the text in the right direction but does not implement
//!   the real rules for mixed-direction lines.
//! * **Kerning only from the old `kern` table.** Modern fonts put it in `GPOS`,
//!   which arrives with shaping.
//! * **Tables are laid out as their paragraphs**, without cells or borders.

use std::collections::HashMap;

use wp_docx::model::{
    Alignment, Block, Body, BreakKind, LineRule, Paragraph, ResolvedRunProperties, Run, RunContent,
};
use wp_docx::{Document, TextPosition};
use wp_font::{Font, GlyphId};
use wp_raster::Color;

use crate::library::FontLibrary;

/// Twentieths of a point, the unit the format measures almost everything in.
const TWIPS_PER_POINT: f32 = 20.0;
/// Points per inch, which is what makes a point a point.
const POINTS_PER_INCH: f32 = 72.0;

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
        }
    }
}

impl PageMetrics {
    /// The width text may occupy.
    #[must_use]
    pub fn text_width(&self) -> f32 {
        (self.width - self.margin_left - self.margin_right).max(1.0)
    }

    /// The height text may occupy.
    #[must_use]
    pub fn text_height(&self) -> f32 {
        (self.height - self.margin_top - self.margin_bottom).max(1.0)
    }

    /// Reads `w:sectPr` out of a document, falling back to A4.
    #[must_use]
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
    pub decorations: Vec<Decoration>,
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
        let line = self
            .lines
            .iter()
            .find(|line| y >= line.top() && y <= line.bottom())
            .or_else(|| {
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
            .or_else(|| {
                self.lines.iter().find(|line| line.paragraph == position.paragraph)
            })
    }

    /// Whether any of this page's text comes from a given paragraph.
    #[must_use]
    pub fn holds_paragraph(&self, paragraph: usize) -> bool {
        self.lines.iter().any(|line| line.paragraph == paragraph)
    }
}

/// How a run should look, reduced to what drawing needs.
#[derive(Clone, Debug)]
struct RunStyle {
    face: usize,
    size: f32,
    color: Color,
    underline: bool,
    strike: bool,
    right_to_left: bool,
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
}

impl<'a> LayoutEngine<'a> {
    #[must_use]
    pub fn new(library: &'a FontLibrary) -> Self {
        Self { library, dpi: 96.0, fonts: HashMap::new() }
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
        let mut page = Page::default();
        let size = size_points * self.pixels_per_point();

        let Some(face) = self.library.default_face(false, false) else {
            return page;
        };
        let style = RunStyle {
            face,
            size,
            color,
            underline: false,
            strike: false,
            right_to_left: false,
            ascent: size,
            descent: size * 0.25,
            line_height: size * 1.25,
        };

        let mut pen = x;
        for glyph in self.shape(text, &style, 0) {
            page.glyphs.push(PositionedGlyph {
                face: glyph.face,
                glyph: glyph.glyph,
                x: pen,
                baseline,
                advance: glyph.advance,
                size,
                color,
                source: TextPosition::default(),
                source_length: glyph.length,
            });
            pen += glyph.advance;
        }

        page.width = pen;
        page.height = baseline + style.descent;
        page
    }

    /// Lays a whole document out into pages.
    pub fn layout_document(&mut self, document: &Document) -> Vec<Page> {
        let metrics = PageMetrics::from_document(document);
        self.layout_body(&document.body(), document, metrics)
    }

    /// Lays a body out into pages.
    pub fn layout_body(
        &mut self,
        body: &Body,
        document: &Document,
        metrics: PageMetrics,
    ) -> Vec<Page> {
        let scale = self.pixels_per_point();
        let page_width = metrics.width * scale;
        let page_height = metrics.height * scale;
        let left = metrics.margin_left * scale;
        let top = metrics.margin_top * scale;
        let bottom_limit = page_height - metrics.margin_bottom * scale;
        let text_width = metrics.text_width() * scale;

        let mut pages = vec![Page { width: page_width, height: page_height, ..Page::default() }];
        let mut y = top;

        for (index, paragraph) in collect_paragraphs(&body.blocks).into_iter().enumerate() {
            self.place_paragraph(
                index,
                paragraph,
                document,
                &mut pages,
                &mut y,
                Placement { left, top, bottom_limit, text_width, page_width, page_height },
            );
        }

        pages
    }

    /// Where a paragraph may be drawn, in pixels.
    ///
    /// The index is the paragraph's place in reading order, which is what a
    /// caret and a click are expressed in.
    fn place_paragraph(
        &mut self,
        index: usize,
        paragraph: &Paragraph,
        document: &Document,
        pages: &mut Vec<Page>,
        y: &mut f32,
        area: Placement,
    ) {
        let resolved = document.resolve_paragraph(paragraph);
        let scale = self.pixels_per_point();

        // Spacing is in twentieths of a point, like almost everything else.
        let space_before = resolved.space_before as f32 / TWIPS_PER_POINT * scale;
        let space_after = resolved.space_after as f32 / TWIPS_PER_POINT * scale;
        let indent_start = resolved.indent_start as f32 / TWIPS_PER_POINT * scale;
        let indent_end = resolved.indent_end as f32 / TWIPS_PER_POINT * scale;
        let indent_first = resolved.indent_first_line as f32 / TWIPS_PER_POINT * scale;

        if resolved.page_break_before && !pages.last().is_some_and(|page| page.glyphs.is_empty()) {
            self.start_page(pages, y, area);
        }

        *y += space_before;

        let mut styles = Vec::new();
        let items = self.build_items(paragraph, document, &mut styles);
        if items.is_empty() {
            // An empty paragraph still takes up a line's worth of height, and
            // still needs a line recorded: a caret has to be able to sit in it.
            let height = self.empty_line_height(paragraph, document);
            if *y + height > area.bottom_limit
                && !pages.last().is_some_and(|page| page.glyphs.is_empty())
            {
                self.start_page(pages, y, area);
            }
            let ascent = height * 0.8;
            if let Some(page) = pages.last_mut() {
                page.lines.push(PageLine {
                    baseline: *y + ascent,
                    ascent,
                    descent: height - ascent,
                    left: area.left + indent_start,
                    right: area.left + indent_start,
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

        let available = (area.text_width - indent_start - indent_end).max(1.0);
        let first_line_width = (available - indent_first.max(0.0)).max(1.0);
        let lines = break_into_lines(&items, available, first_line_width);

        for (number, line) in lines.iter().enumerate() {
            let (ascent, descent, natural_height) = line_metrics(line, &items, &styles);
            let height = match resolved.line_spacing {
                Some(spacing) => match spacing.rule {
                    // A multiple of single spacing, in 240ths.
                    LineRule::Auto => natural_height * (spacing.value as f32 / 240.0),
                    LineRule::Exact => spacing.value as f32 / TWIPS_PER_POINT * scale,
                    LineRule::AtLeast => {
                        natural_height.max(spacing.value as f32 / TWIPS_PER_POINT * scale)
                    }
                },
                None => natural_height,
            };

            if *y + height > area.bottom_limit && !pages.last().is_some_and(|p| p.glyphs.is_empty())
            {
                self.start_page(pages, y, area);
            }

            let baseline = *y + ascent;
            let extra_first = if number == 0 { indent_first.max(0.0) } else { 0.0 };
            let line_left = area.left + indent_start + extra_first;
            let line_width = if number == 0 { first_line_width } else { available };

            self.place_line(
                line,
                &items,
                &styles,
                pages.last_mut().expect("there is always a page"),
                LinePlacement {
                    left: line_left,
                    width: line_width,
                    baseline,
                    ascent,
                    descent,
                    alignment: resolved.alignment,
                    paragraph_rtl: resolved.right_to_left,
                    is_last_line: number + 1 == lines.len(),
                    paragraph: index,
                },
            );

            *y += height;
        }

        *y += space_after;
    }

    fn start_page(&self, pages: &mut Vec<Page>, y: &mut f32, area: Placement) {
        pages.push(Page {
            width: area.page_width,
            height: area.page_height,
            ..Page::default()
        });
        *y = area.top;
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
    ) -> Vec<Item> {
        let mut items = Vec::new();
        // Byte offset within the paragraph's text, counted the same way the
        // editing layer counts it, so a glyph and a caret mean the same thing.
        let mut offset = 0usize;

        for run in &paragraph.runs {
            let resolved = document.resolve_run(paragraph, run);
            let Some(style) = self.style_for(&resolved) else {
                continue;
            };
            let style_index = styles.len();
            styles.push(style.clone());

            self.build_run_items(run, &style, style_index, &mut items, &mut offset);
        }

        items
    }

    fn build_run_items(
        &mut self,
        run: &Run,
        style: &RunStyle,
        style_index: usize,
        items: &mut Vec<Item>,
        offset: &mut usize,
    ) {
        for content in &run.content {
            match content {
                RunContent::Text(text) => {
                    for chunk in segment(text) {
                        let start = *offset;
                        let glyphs = self.shape(&chunk.text, style, start);
                        let width = glyphs.iter().map(|glyph| glyph.advance).sum();
                        *offset += chunk.text.len();
                        items.push(Item {
                            glyphs,
                            width,
                            is_space: chunk.is_space,
                            hard_break: None,
                            style: style_index,
                            start_offset: start,
                            end_offset: *offset,
                        });
                    }
                }
                RunContent::Tab => {
                    // A real tab stop table is a later stage; until then a tab
                    // advances by a fixed amount rather than being ignored.
                    // It contributes no bytes, because the text a caret moves
                    // through does not contain it either.
                    let width = style.size * 2.0;
                    items.push(Item {
                        glyphs: Vec::new(),
                        width,
                        is_space: false,
                        hard_break: None,
                        style: style_index,
                        start_offset: *offset,
                        end_offset: *offset,
                    });
                }
                RunContent::Break(kind) => {
                    items.push(Item {
                        glyphs: Vec::new(),
                        width: 0.0,
                        is_space: false,
                        hard_break: Some(*kind),
                        style: style_index,
                        start_offset: *offset,
                        end_offset: *offset,
                    });
                }
            }
        }
    }

    /// Chooses a face and works out the metrics for one set of run properties.
    fn style_for(&mut self, properties: &ResolvedRunProperties) -> Option<RunStyle> {
        let face = self.library.select(
            properties.font.as_deref(),
            properties.bold,
            properties.italic,
        )?;

        let size = properties.size_points() as f32 * self.pixels_per_point();
        let font = self.font(face)?;
        let units = f32::from(font.units_per_em());
        let metrics = font.vertical_metrics();

        let ascent = f32::from(metrics.ascender) * size / units;
        let descent = -f32::from(metrics.descender) * size / units;
        let line_height = metrics.line_height() as f32 * size / units;

        Some(RunStyle {
            face,
            size,
            color: properties
                .color
                .as_deref()
                .and_then(Color::from_hex)
                .unwrap_or(Color::BLACK),
            underline: properties.underline.is_visible(),
            strike: properties.strike,
            right_to_left: properties.right_to_left,
            ascent,
            descent,
            line_height,
        })
    }

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
        let mut glyphs = Vec::with_capacity(text.len());
        let mut previous: Option<GlyphId> = None;

        for (local, character) in text.char_indices() {
            let mut chosen = None;

            if let Some(font) = self.font(style.face) {
                if let Some(glyph) = font.glyph_for(character) {
                    let units = f32::from(font.units_per_em());
                    let mut advance = f32::from(font.advance(glyph)) * style.size / units;
                    if let Some(previous) = previous {
                        advance +=
                            f32::from(font.kerning(previous, glyph)) * style.size / units;
                    }
                    chosen = Some((style.face, glyph, advance));
                }
            }

            if chosen.is_none() {
                // The face cannot draw this character. Another one may be able
                // to, and an empty box helps nobody.
                if let Some((face, glyph)) =
                    self.library.fallback_for(character, false, false)
                {
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

        // A right-to-left run reads the other way. This is not the full
        // bidirectional algorithm, but it puts the text in the right direction
        // instead of backwards.
        if style.right_to_left {
            glyphs.reverse();
        }

        glyphs
    }

    /// Places one line's glyphs, applying alignment, and records the line.
    fn place_line(
        &self,
        line: &Line,
        items: &[Item],
        styles: &[RunStyle],
        page: &mut Page,
        placement: LinePlacement,
    ) {
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

        for index in line.items.clone() {
            let item = &items[index];
            let style = &styles[item.style];

            if item.is_space && index >= line.last_visible {
                continue;
            }

            let start_x = x;
            for glyph in &item.glyphs {
                page.glyphs.push(PositionedGlyph {
                    face: glyph.face,
                    glyph: glyph.glyph,
                    x,
                    baseline,
                    advance: glyph.advance,
                    size: style.size,
                    color: style.color,
                    source: TextPosition::new(placement.paragraph, glyph.offset),
                    source_length: glyph.length,
                });
                x += glyph.advance;
            }
            if item.glyphs.is_empty() {
                x += item.width;
            }
            if item.is_space {
                x += justify_extra;
            }

            let drawn_width = x - start_x;
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
        let start_offset = line
            .items
            .clone()
            .map(|index| items[index].start_offset)
            .min()
            .unwrap_or(0);
        let end_offset = line
            .items
            .clone()
            .map(|index| items[index].end_offset)
            .max()
            .unwrap_or(start_offset);

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
    width: f32,
    baseline: f32,
    ascent: f32,
    descent: f32,
    alignment: Alignment,
    paragraph_rtl: bool,
    is_last_line: bool,
    /// Which paragraph the line belongs to, in reading order.
    paragraph: usize,
}

/// The rectangle text is placed within, in pixels.
#[derive(Clone, Copy, Debug)]
struct Placement {
    left: f32,
    top: f32,
    bottom_limit: f32,
    text_width: f32,
    page_width: f32,
    page_height: f32,
}

/// One line: a span of items, and where the drawable part of it ends.
#[derive(Clone, Debug)]
struct Line {
    items: std::ops::Range<usize>,
    /// Index after the last item that is actually drawn, so trailing spaces can
    /// be skipped without losing them from the range.
    last_visible: usize,
}

/// Breaks measured items into lines.
fn break_into_lines(items: &[Item], width: f32, first_width: f32) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut used = 0.0f32;
    let mut last_visible = 0usize;
    let mut available = first_width;

    let mut index = 0usize;
    while index < items.len() {
        let item = &items[index];

        if item.hard_break.is_some() {
            lines.push(Line { items: start..index + 1, last_visible });
            start = index + 1;
            used = 0.0;
            last_visible = start;
            available = width;
            index += 1;
            continue;
        }

        // A space at the start of a line is dropped rather than indenting it.
        if item.is_space && used == 0.0 && index == start {
            start = index + 1;
            last_visible = start;
            index += 1;
            continue;
        }

        let would_be = used + item.width;
        // A single item wider than the line still has to go somewhere.
        if would_be > available && used > 0.0 && !item.is_space {
            lines.push(Line { items: start..index, last_visible });
            start = index;
            used = 0.0;
            last_visible = index;
            available = width;
            continue;
        }

        used = would_be;
        if !item.is_space {
            last_visible = index + 1;
        }
        index += 1;
    }

    if start < items.len() {
        lines.push(Line { items: start..items.len(), last_visible });
    } else if lines.is_empty() {
        lines.push(Line { items: 0..0, last_visible: 0 });
    }

    lines
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

/// Splits text into words, spaces, and characters that break on their own.
///
/// Chinese, Japanese and Korean are written without spaces, so a line may be
/// broken between almost any two characters. Treating those as words would put
/// a whole paragraph on one line.
fn segment(text: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_is_space = false;

    let flush = |chunks: &mut Vec<Chunk>, current: &mut String, is_space: bool| {
        if !current.is_empty() {
            chunks.push(Chunk { text: std::mem::take(current), is_space });
        }
    };

    for character in text.chars() {
        if breaks_on_its_own(character) {
            flush(&mut chunks, &mut current, current_is_space);
            chunks.push(Chunk { text: character.to_string(), is_space: false });
            continue;
        }

        let is_space = character.is_whitespace();
        if is_space != current_is_space {
            flush(&mut chunks, &mut current, current_is_space);
            current_is_space = is_space;
        }
        current.push(character);
    }
    flush(&mut chunks, &mut current, current_is_space);

    chunks
}

/// Whether a line may be broken after this character regardless of spaces.
fn breaks_on_its_own(character: char) -> bool {
    matches!(character as u32,
        // CJK ideographs, and the Japanese and Korean syllabaries.
        0x1100..=0x11FF
        | 0x2E80..=0x9FFF
        | 0xA960..=0xA97F
        | 0xAC00..=0xD7FF
        | 0xF900..=0xFAFF
        | 0xFF00..=0xFF60
        | 0x20000..=0x3FFFF
    )
}

/// Every paragraph, including the ones inside tables.
fn collect_paragraphs(blocks: &[Block]) -> Vec<&Paragraph> {
    let mut out = Vec::new();
    fn walk<'a>(blocks: &'a [Block], out: &mut Vec<&'a Paragraph>) {
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => out.push(paragraph),
                Block::Table(table) => {
                    for row in &table.rows {
                        for cell in &row.cells {
                            walk(&cell.blocks, out);
                        }
                    }
                }
            }
        }
    }
    walk(blocks, &mut out);
    out
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
}

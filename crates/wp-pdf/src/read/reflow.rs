//! From glyphs on pages to paragraphs in a document.
//!
//! A PDF says where every glyph goes and nothing about why. Reflowing it
//! is working the why back out: glyphs on one baseline are a line; a gap
//! wider than a letter is a space; lines that follow one another at the
//! usual pitch are a paragraph, and a wider gap, an indent, a bullet or a
//! change of size starts another; lines that share a column of the page
//! are read before the column beside them; rules that cross are a table's
//! borders and the lines between them are its cells; a bigger, bolder
//! paragraph is a heading. None of it is certain, all of it is what Word
//! does when it opens a PDF, and the result is a document that can be
//! edited, which is the point.

use std::collections::HashMap;

use wp_docx::model::{
    Alignment, Block, Body, NumberingReference, Paragraph, ParagraphProperties, Run, RunContent,
    RunProperties, Table, TableCell, TableRow, Underline, VerticalAlignment,
};

use super::content::{Drawn, PlacedPicture, Rectangle};

/// A character the document holds where a picture will go.
pub const PICTURE_MARK: char = '\u{FFFC}';

/// One page as drawn, with its size and the links laid over it.
pub struct PageDrawn {
    pub drawn: Drawn,
    pub width: f64,
    pub height: f64,
    /// Left, bottom, right, top; and the address.
    pub links: Vec<([f64; 4], String)>,
}

/// What reflowing gives: the body, and what is put into it after.
pub struct Reading {
    pub body: Body,
    pub pictures: Vec<PictureFound>,
    pub links: Vec<LinkFound>,
    /// Width, height, and the margins top, right, bottom, left, in twips.
    pub page: Option<(i32, i32, [i32; 4])>,
}

#[derive(Debug)]
pub struct PictureFound {
    pub paragraph: usize,
    pub offset: usize,
    pub bytes: Vec<u8>,
    pub extension: &'static str,
    pub width_emu: i64,
    pub height_emu: i64,
}

#[derive(Debug)]
pub struct LinkFound {
    pub paragraph: usize,
    pub start: usize,
    pub end: usize,
    pub address: String,
}

/// What the reflow finds along the way to put into the document after it
/// is built: the links, the pictures, and the addresses the links go to.
#[derive(Default)]
struct Found {
    addresses: Vec<String>,
    links: Vec<LinkFound>,
    pictures: Vec<PictureFound>,
    /// The pictures standing in lines of text, by the index their pieces
    /// hold, taken as they are placed.
    inline: Vec<Option<PlacedPicture>>,
}

/// How a glyph is formatted, as the document will hold it.
#[derive(Clone, Debug, PartialEq)]
struct Format {
    family: String,
    bold: bool,
    italic: bool,
    half_points: u32,
    colour: [u8; 3],
    underline: bool,
    strike: bool,
    vertical: VerticalAlignment,
}

/// One glyph, decorated, or a space put between two.
#[derive(Clone, Debug)]
struct Piece {
    x0: f64,
    x1: f64,
    y: f64,
    size: f64,
    text: String,
    format: Format,
    link: Option<usize>,
    /// The inline picture this piece stands for, by its index among them.
    picture: Option<usize>,
}

impl Piece {
    fn is_space(&self) -> bool {
        self.text.chars().all(char::is_whitespace)
    }
}

/// Glyphs on one baseline, in order, with spaces between words.
#[derive(Clone, Debug)]
struct Line {
    y: f64,
    x0: f64,
    x1: f64,
    /// The size most of the line is in.
    size: f64,
    pieces: Vec<Piece>,
    /// The width of the gaps taken as spaces, for telling justified text.
    space_widths: Vec<f64>,
}

impl Line {
    fn top(&self) -> f64 {
        self.y + self.size * 0.8
    }

    fn text(&self) -> String {
        self.pieces.iter().map(|piece| piece.text.as_str()).collect()
    }

    fn is_bold(&self) -> bool {
        let letters: Vec<&Piece> = self.pieces.iter().filter(|p| !p.is_space()).collect();
        !letters.is_empty()
            && letters.iter().filter(|p| p.format.bold).count() * 3 >= letters.len() * 2
    }
}

/// A table found from its borders.
struct Grid {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    /// Column edges, left to right.
    xs: Vec<f64>,
    /// Row edges, top to bottom.
    ys: Vec<f64>,
    horizontals: Vec<Rectangle>,
    verticals: Vec<Rectangle>,
    /// Row-major cells: the lines in each.
    cells: Vec<Vec<Line>>,
}

impl Grid {
    fn rows(&self) -> usize {
        self.ys.len().saturating_sub(1)
    }

    fn columns(&self) -> usize {
        self.xs.len().saturating_sub(1)
    }

    /// Whether a vertical border stands at column edge `edge` across row
    /// `row`.
    fn has_vertical(&self, edge: usize, row: usize) -> bool {
        let x = self.xs[edge];
        let mid = (self.ys[row] + self.ys[row + 1]) / 2.0;
        self.verticals.iter().any(|v| {
            ((v.x0 + v.x1) / 2.0 - x).abs() <= 2.5 && v.y0 <= mid + 1.0 && v.y1 >= mid - 1.0
        })
    }

    /// Whether a horizontal border lies at row edge `edge` across the
    /// columns from `from` to `to`.
    fn has_horizontal(&self, edge: usize, from: usize, to: usize) -> bool {
        let y = self.ys[edge];
        let mid = (self.xs[from] + self.xs[to]) / 2.0;
        self.horizontals.iter().any(|h| {
            ((h.y0 + h.y1) / 2.0 - y).abs() <= 2.5 && h.x0 <= mid + 1.0 && h.x1 >= mid - 1.0
        })
    }
}

/// Left, bottom, right, top.
type Bounds = (f64, f64, f64, f64);

/// Something on the page with a place: a block of lines, a table, or a
/// picture.
enum Item {
    Block(Vec<Line>),
    Table(Grid),
    Picture(PlacedPicture),
}

impl Item {
    fn bounds(&self) -> Bounds {
        match self {
            Self::Block(lines) => (
                lines.iter().map(|l| l.x0).fold(f64::INFINITY, f64::min),
                lines.iter().map(|l| l.y - l.size * 0.25).fold(f64::INFINITY, f64::min),
                lines.iter().map(|l| l.x1).fold(f64::NEG_INFINITY, f64::max),
                lines.iter().map(Line::top).fold(f64::NEG_INFINITY, f64::max),
            ),
            Self::Table(grid) => (grid.x0, grid.y0, grid.x1, grid.y1),
            Self::Picture(picture) => (picture.x0, picture.y0, picture.x1, picture.y1),
        }
    }
}

/// A paragraph as it is being built: its lines and what was worked out
/// about them.
struct Built {
    lines: Vec<Line>,
    properties: ParagraphProperties,
    /// The marker's left edge, when the paragraph is a list item.
    marker_x: Option<f64>,
    /// The size most of the paragraph is in, for telling headings.
    size: f64,
    bold: bool,
    /// The last line reaches the right edge of its column.
    ends_full: bool,
}

/// Turns pages into a document.
#[must_use]
pub fn reflow(pages: Vec<PageDrawn>) -> Reading {
    let mut found = Found::default();
    let body_size = body_size_of(&pages);

    // Every page's items, in reading order, with paragraphs built.
    let mut sequence: Vec<Sequenced> = Vec::new();
    let mut extent = Extent::default();
    for (page_index, page) in pages.into_iter().enumerate() {
        if page_index == 0 {
            extent.width = page.width;
            extent.height = page.height;
        }
        let link_base = found.addresses.len();
        for (_, address) in &page.links {
            found.addresses.push(address.clone());
        }
        let pieces = decorate(&page, link_base);
        let mut page_pictures: Vec<Option<PlacedPicture>> =
            page.drawn.pictures.into_iter().map(Some).collect();
        let mut lines = lines_of(pieces, &mut page_pictures, &mut found.inline);
        for line in &lines {
            extent.note(line.x0, line.y - line.size * 0.25, line.x1, line.top());
        }
        let text_left = lines.iter().map(|l| l.x0).fold(f64::INFINITY, f64::min);
        let text_right = lines.iter().map(|l| l.x1).fold(f64::NEG_INFINITY, f64::max);
        let grids = grids_of(&page.drawn.rectangles, &mut lines);
        let blocks = blocks_of(lines);
        let mut items: Vec<Item> = blocks.into_iter().map(Item::Block).collect();
        items.extend(grids.into_iter().map(Item::Table));
        items.extend(
            page_pictures
                .into_iter()
                .flatten()
                .filter(|p| p.x1 - p.x0 >= 4.0 && p.y1 - p.y0 >= 4.0)
                .map(Item::Picture),
        );
        let text_width = if text_right > text_left { text_right - text_left } else { page.width };
        let ordered = order(items, text_left, text_left + text_width);
        for (item, column) in ordered {
            match item {
                Item::Block(lines) => {
                    for built in
                        paragraphs_of(lines, column.0, column.1, body_size, page.width / 2.0)
                    {
                        sequence.push(Sequenced::Paragraph { built: Box::new(built) });
                    }
                }
                Item::Table(grid) => sequence.push(Sequenced::Table { grid: Box::new(grid) }),
                Item::Picture(picture) => {
                    let centred = ((picture.x0 - text_left) - (text_right - picture.x1)).abs()
                        < (text_right - text_left) * 0.1
                        && picture.x0 - text_left > 4.0;
                    sequence.push(Sequenced::Picture { picture, centred });
                }
            }
        }
        sequence.push(Sequenced::PageEnd);
    }
    join_across_pages(&mut sequence);
    let heading_sizes = heading_sizes(&sequence, body_size);
    let marker_levels = marker_levels(&sequence);

    let mut body = Body::default();
    let mut paragraphs_done = 0usize;
    for item in sequence {
        match item {
            Sequenced::Paragraph { built } => {
                let paragraph = paragraph_of(
                    *built,
                    body_size,
                    &heading_sizes,
                    &marker_levels,
                    &mut found,
                    paragraphs_done,
                );
                body.blocks.push(Block::Paragraph(paragraph));
                paragraphs_done += 1;
            }
            Sequenced::Table { grid } => {
                let table = table_of(*grid, body_size, &mut found, &mut paragraphs_done);
                body.blocks.push(Block::Table(Box::new(table)));
            }
            Sequenced::Picture { picture, centred } => {
                let mut paragraph = Paragraph::text(&PICTURE_MARK.to_string());
                if centred {
                    paragraph.properties.alignment = Some(Alignment::Center);
                }
                body.blocks.push(Block::Paragraph(paragraph));
                found.pictures.push(PictureFound {
                    paragraph: paragraphs_done,
                    offset: 0,
                    bytes: picture.bytes,
                    extension: picture.extension,
                    width_emu: ((picture.x1 - picture.x0) * 12700.0).round() as i64,
                    height_emu: ((picture.y1 - picture.y0) * 12700.0).round() as i64,
                });
                paragraphs_done += 1;
            }
            Sequenced::PageEnd => {}
        }
    }
    if body.blocks.is_empty() {
        body.blocks.push(Block::Paragraph(Paragraph::default()));
    }
    Reading { body, pictures: found.pictures, links: found.links, page: extent.page() }
}

/// An item of the document in reading order, before it is a block.
enum Sequenced {
    Paragraph { built: Box<Built> },
    Table { grid: Box<Grid> },
    Picture { picture: PlacedPicture, centred: bool },
    PageEnd,
}

/// Where text lies on the pages, for the margins.
#[derive(Default)]
struct Extent {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
    width: f64,
    height: f64,
    any: bool,
}

impl Extent {
    fn note(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        if !self.any {
            self.left = x0;
            self.bottom = y0;
            self.right = x1;
            self.top = y1;
            self.any = true;
        } else {
            self.left = self.left.min(x0);
            self.bottom = self.bottom.min(y0);
            self.right = self.right.max(x1);
            self.top = self.top.max(y1);
        }
    }

    fn page(&self) -> Option<(i32, i32, [i32; 4])> {
        if self.width <= 0.0 || self.height <= 0.0 {
            return None;
        }
        let twips = |points: f64| (points * 20.0).round() as i32;
        let margin = |points: f64| {
            // To the nearest twentieth of an inch, and never nothing.
            let twips = ((points * 20.0) / 72.0).round() as i32 * 72;
            twips.max(72)
        };
        if !self.any {
            return Some((twips(self.width), twips(self.height), [1440, 1440, 1440, 1440]));
        }
        Some((
            twips(self.width),
            twips(self.height),
            [
                margin(self.height - self.top),
                margin(self.width - self.right),
                margin(self.bottom),
                margin(self.left),
            ],
        ))
    }
}

/// The size most of the document's text is in.
fn body_size_of(pages: &[PageDrawn]) -> f64 {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for page in pages {
        for glyph in &page.drawn.glyphs {
            if !glyph.text.chars().all(char::is_whitespace) {
                *counts.entry((glyph.size * 2.0).round() as u32).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(size, count)| (*count, *size))
        .map_or(11.0, |(size, _)| f64::from(size) / 2.0)
}

/// Glyphs with their lines under and through them, and their links.
fn decorate(page: &PageDrawn, link_base: usize) -> Vec<Piece> {
    let thin: Vec<&Rectangle> = page
        .drawn
        .rectangles
        .iter()
        .filter(|r| r.filled && r.height() <= 2.5 && r.width() >= 2.0)
        .collect();
    let stroked: Vec<&Rectangle> = page
        .drawn
        .rectangles
        .iter()
        .filter(|r| !r.filled && r.height() <= 2.5 && r.width() >= 2.0)
        .collect();
    page.drawn
        .glyphs
        .iter()
        .map(|glyph| {
            let size = glyph.size.max(0.5);
            let (x0, x1) = (glyph.x.min(glyph.end_x), glyph.x.max(glyph.end_x));
            let centre_x = (x0 + x1) / 2.0;
            let under = |r: &Rectangle| {
                let y = (r.y0 + r.y1) / 2.0;
                r.x0 <= centre_x + 0.5
                    && r.x1 >= centre_x - 0.5
                    && y < glyph.y - 0.02 * size
                    && y > glyph.y - 0.35 * size
            };
            let through = |r: &Rectangle| {
                let y = (r.y0 + r.y1) / 2.0;
                r.x0 <= centre_x + 0.5
                    && r.x1 >= centre_x - 0.5
                    && y > glyph.y + 0.15 * size
                    && y < glyph.y + 0.55 * size
            };
            let underline = thin.iter().chain(&stroked).any(|r| under(r));
            let strike = thin.iter().chain(&stroked).any(|r| through(r));
            let vertical = if glyph.rise > 0.15 * size {
                VerticalAlignment::Superscript
            } else if glyph.rise < -0.15 * size {
                VerticalAlignment::Subscript
            } else {
                VerticalAlignment::Baseline
            };
            let link = page.links.iter().position(|(rect, _)| {
                let y = glyph.y + 0.4 * size;
                centre_x >= rect[0] - 1.0
                    && centre_x <= rect[2] + 1.0
                    && y >= rect[1] - 1.0
                    && y <= rect[3] + 1.0
            });
            let colour = [
                (glyph.colour[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                (glyph.colour[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                (glyph.colour[2].clamp(0.0, 1.0) * 255.0).round() as u8,
            ];
            Piece {
                x0,
                x1,
                y: glyph.y + glyph.rise,
                size,
                text: glyph.text.clone(),
                format: Format {
                    family: glyph.font.style.family.clone(),
                    bold: glyph.font.style.bold,
                    italic: glyph.font.style.italic || glyph.slanted,
                    half_points: ((size * 2.0).round() as u32).max(2),
                    colour,
                    underline,
                    strike,
                    vertical,
                },
                link: link.map(|index| link_base + index),
                picture: None,
            }
        })
        .collect()
}

/// Glyphs into lines: one baseline each, in x order, with spaces where
/// the gaps are, and cut where a gap is too wide to be in one column.
fn lines_of(
    mut pieces: Vec<Piece>,
    page_pictures: &mut [Option<PlacedPicture>],
    inline: &mut Vec<Option<PlacedPicture>>,
) -> Vec<Line> {
    // Superscripts and subscripts sit off the baseline: put them back on
    // the line they belong to by their own size.
    pieces.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
    let mut rows: Vec<Vec<Piece>> = Vec::new();
    for piece in pieces {
        let joined = rows.last_mut().is_some_and(|row| {
            let (y, size) = row_baseline(row);
            let tolerance = 0.45 * size.max(piece.size);
            if (piece.y - y).abs() <= tolerance {
                row.push(piece.clone());
                true
            } else {
                false
            }
        });
        if !joined {
            rows.push(vec![piece]);
        }
    }
    // A picture standing in a line of text, with text right beside it,
    // is a character of that line.
    for slot in page_pictures.iter_mut() {
        let Some(picture) = slot.as_ref() else { continue };
        let beside = |row: &Vec<Piece>| {
            let (y, size) = row_baseline(row);
            let reach = (2.5 * size).max(14.0);
            y >= picture.y0 - 0.5 * size
                && y <= picture.y1
                && row.iter().any(|p| {
                    (p.x1 <= picture.x0 + 1.0 && picture.x0 - p.x1 < reach)
                        || (p.x0 >= picture.x1 - 1.0 && p.x0 - picture.x1 < reach)
                })
        };
        let Some(index) = rows.iter().position(beside) else { continue };
        let (y, size) = row_baseline(&rows[index]);
        let piece = Piece {
            x0: picture.x0,
            x1: picture.x1,
            y,
            size,
            text: PICTURE_MARK.to_string(),
            format: Format {
                family: String::new(),
                bold: false,
                italic: false,
                half_points: ((size * 2.0).round() as u32).max(2),
                colour: [0, 0, 0],
                underline: false,
                strike: false,
                vertical: VerticalAlignment::Baseline,
            },
            link: None,
            picture: Some(inline.len()),
        };
        inline.push(slot.take());
        rows[index].push(piece);
    }
    let mut lines = Vec::new();
    for mut row in rows {
        row.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
        let (y, size) = row_baseline(&row);
        // Cut into segments at gaps wider than a few letters.
        let mut segments: Vec<Vec<Piece>> = Vec::new();
        for piece in row {
            let split = segments.last().is_some_and(|segment| {
                let last = segment.last().expect("a segment holds a piece");
                piece.x0 - last.x1 > (2.5 * size).max(14.0)
            });
            if split || segments.is_empty() {
                segments.push(vec![piece]);
            } else if let Some(segment) = segments.last_mut() {
                segment.push(piece);
            }
        }
        for segment in segments {
            if let Some(line) = line_of(segment, y, size) {
                lines.push(line);
            }
        }
    }
    lines
}

/// The baseline and size a row of pieces has most of.
fn row_baseline(row: &[Piece]) -> (f64, f64) {
    let mut counts: HashMap<u32, (usize, f64, f64)> = HashMap::new();
    for piece in row {
        if piece.is_space() {
            continue;
        }
        let entry = counts.entry((piece.size * 2.0).round() as u32).or_insert((0, 0.0, 0.0));
        entry.0 += 1;
        entry.1 += piece.y;
        entry.2 = piece.size;
    }
    counts
        .values()
        .max_by(|a, b| {
            a.0.cmp(&b.0).then(a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
        })
        .map_or_else(
            || (row.first().map_or(0.0, |p| p.y), row.first().map_or(10.0, |p| p.size)),
            |(count, sum, size)| (sum / *count as f64, *size),
        )
}

/// A segment as a line, with spaces put in and doubled ones taken out.
fn line_of(segment: Vec<Piece>, y: f64, size: f64) -> Option<Line> {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut space_widths = Vec::new();
    // The right edge of the last glyph that was not a space, for the width
    // of the space after it — whether the file wrote a space or left a gap.
    let mut last_visible: Option<f64> = None;
    for piece in segment {
        if let Some(last) = pieces.last() {
            let gap = piece.x0 - last.x1;
            let threshold = 0.14 * piece.size.min(last.size).max(1.0);
            if gap > threshold && !last.is_space() && !piece.is_space() {
                pieces.push(Piece {
                    x0: last.x1,
                    x1: piece.x0,
                    y,
                    size: last.size,
                    text: " ".to_owned(),
                    format: last.format.clone(),
                    link: if last.link == piece.link { last.link } else { None },
                    picture: None,
                });
            } else if piece.is_space() && last.is_space() {
                continue;
            }
        } else if piece.is_space() {
            continue;
        }
        if !piece.is_space() {
            if let (Some(visible), true) =
                (last_visible, pieces.last().is_some_and(Piece::is_space))
            {
                space_widths.push(piece.x0 - visible);
            }
            last_visible = Some(piece.x1);
        }
        pieces.push(piece);
    }
    while pieces.last().is_some_and(Piece::is_space) {
        pieces.pop();
    }
    if pieces.is_empty() || pieces.iter().all(Piece::is_space) {
        return None;
    }
    // A smaller glyph sitting above the baseline is a superscript, and one
    // hanging below it a subscript, whether or not the file said so.
    for piece in &mut pieces {
        if piece.size < 0.85 * size && piece.format.vertical == VerticalAlignment::Baseline {
            if piece.y - y > 0.15 * size {
                piece.format.vertical = VerticalAlignment::Superscript;
            } else if y - piece.y > 0.1 * size {
                piece.format.vertical = VerticalAlignment::Subscript;
            }
        }
    }
    let x0 = pieces.iter().map(|p| p.x0).fold(f64::INFINITY, f64::min);
    let x1 = pieces.iter().map(|p| p.x1).fold(f64::NEG_INFINITY, f64::max);
    Some(Line { y, x0, x1, size, pieces, space_widths })
}

/// Tables from the rules on the page: rules that cross make a grid, and
/// the lines inside it are taken out of the flow and into its cells.
fn grids_of(rectangles: &[Rectangle], lines: &mut Vec<Line>) -> Vec<Grid> {
    let horizontals: Vec<Rectangle> =
        rectangles.iter().filter(|r| r.height() <= 2.5 && r.width() >= 6.0).copied().collect();
    let verticals: Vec<Rectangle> =
        rectangles.iter().filter(|r| r.width() <= 2.5 && r.height() >= 6.0).copied().collect();
    if horizontals.len() < 2 || verticals.len() < 2 {
        return Vec::new();
    }
    // Connected groups, by crossings.
    let total = horizontals.len() + verticals.len();
    let mut parent: Vec<usize> = (0..total).collect();
    fn find(parent: &mut [usize], i: usize) -> usize {
        let mut root = i;
        while parent[root] != root {
            root = parent[root];
        }
        let mut at = i;
        while parent[at] != root {
            let next = parent[at];
            parent[at] = root;
            at = next;
        }
        root
    }
    for (hi, h) in horizontals.iter().enumerate() {
        let y = (h.y0 + h.y1) / 2.0;
        for (vi, v) in verticals.iter().enumerate() {
            let x = (v.x0 + v.x1) / 2.0;
            if x >= h.x0 - 2.0 && x <= h.x1 + 2.0 && y >= v.y0 - 2.0 && y <= v.y1 + 2.0 {
                let (a, b) = (find(&mut parent, hi), find(&mut parent, horizontals.len() + vi));
                parent[a] = b;
            }
        }
    }
    let mut groups: HashMap<usize, (Vec<Rectangle>, Vec<Rectangle>)> = HashMap::new();
    for (hi, h) in horizontals.iter().enumerate() {
        groups.entry(find(&mut parent, hi)).or_default().0.push(*h);
    }
    for (vi, v) in verticals.iter().enumerate() {
        groups.entry(find(&mut parent, horizontals.len() + vi)).or_default().1.push(*v);
    }
    let mut grids = Vec::new();
    for (group_h, group_v) in groups.into_values() {
        if group_h.len() < 2 || group_v.len() < 2 {
            continue;
        }
        let mut xs = cluster(group_v.iter().map(|v| (v.x0 + v.x1) / 2.0).collect(), 2.5);
        let mut ys = cluster(group_h.iter().map(|h| (h.y0 + h.y1) / 2.0).collect(), 2.5);
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        ys.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        if xs.len() < 2 || ys.len() < 2 {
            continue;
        }
        let rows = ys.len() - 1;
        let columns = xs.len() - 1;
        if rows * columns > 10_000 {
            continue;
        }
        let grid = Grid {
            x0: xs[0],
            y0: *ys.last().expect("two edges"),
            x1: *xs.last().expect("two edges"),
            y1: ys[0],
            xs,
            ys,
            horizontals: group_h,
            verticals: group_v,
            cells: vec![Vec::new(); rows * columns],
        };
        grids.push(grid);
    }
    // Lines inside a grid go to its cells — cut first where they cross a
    // column's edge, since the cells of one row share a baseline.
    let mut kept = Vec::new();
    for line in lines.drain(..) {
        let y = line.y + line.size * 0.3;
        let x = (line.x0 + line.x1) / 2.0;
        let Some(grid) =
            grids.iter_mut().find(|g| x >= g.x0 && x <= g.x1 && y >= g.y0 && y <= g.y1)
        else {
            kept.push(line);
            continue;
        };
        let row = grid.ys.windows(2).position(|w| y <= w[0] && y >= w[1]).unwrap_or(0);
        let columns = grid.columns();
        for part in cut_at(line, &grid.xs[1..grid.xs.len() - 1]) {
            let x = (part.x0 + part.x1) / 2.0;
            let column = grid.xs.windows(2).position(|w| x >= w[0] && x <= w[1]).unwrap_or(0);
            grid.cells[row * columns + column].push(part);
        }
    }
    *lines = kept;
    grids.retain(|grid| grid.cells.iter().any(|cell| !cell.is_empty()));
    grids
}

/// A line cut at the edges that fall inside it, each part trimmed of the
/// spaces at its ends.
fn cut_at(line: Line, edges: &[f64]) -> Vec<Line> {
    let inside: Vec<f64> =
        edges.iter().copied().filter(|edge| *edge > line.x0 && *edge < line.x1).collect();
    if inside.is_empty() {
        return vec![line];
    }
    let Line { y, size, pieces, .. } = line;
    let mut parts: Vec<Vec<Piece>> = vec![Vec::new(); inside.len() + 1];
    for piece in pieces {
        let centre = (piece.x0 + piece.x1) / 2.0;
        let index = inside.partition_point(|edge| *edge < centre);
        parts[index].push(piece);
    }
    parts
        .into_iter()
        .filter_map(|mut pieces| {
            while pieces.first().is_some_and(Piece::is_space) {
                pieces.remove(0);
            }
            while pieces.last().is_some_and(Piece::is_space) {
                pieces.pop();
            }
            if pieces.is_empty() {
                return None;
            }
            let x0 = pieces.iter().map(|p| p.x0).fold(f64::INFINITY, f64::min);
            let x1 = pieces.iter().map(|p| p.x1).fold(f64::NEG_INFINITY, f64::max);
            Some(Line { y, x0, x1, size, pieces, space_widths: Vec::new() })
        })
        .collect()
}

/// Values within a tolerance of one another, each cluster its mean.
fn cluster(mut values: Vec<f64>, tolerance: f64) -> Vec<f64> {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<(f64, usize)> = Vec::new();
    for value in values {
        match out.last_mut() {
            Some((mean, count)) if (value - *mean).abs() <= tolerance => {
                *mean = (*mean * *count as f64 + value) / (*count as f64 + 1.0);
                *count += 1;
            }
            _ => out.push((value, 1)),
        }
    }
    out.into_iter().map(|(mean, _)| mean).collect()
}

/// Lines into blocks: a line joins the block above it when they overlap
/// across and sit close enough to be in the same column of text.
fn blocks_of(mut lines: Vec<Line>) -> Vec<Vec<Line>> {
    lines.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
    let mut blocks: Vec<Vec<Line>> = Vec::new();
    for line in lines {
        let mut best: Option<usize> = None;
        for (index, block) in blocks.iter().enumerate() {
            let last = block.last().expect("a block holds a line");
            let overlap = line.x1.min(last.x1) - line.x0.max(last.x0);
            let narrower = (line.x1 - line.x0).min(last.x1 - last.x0).max(1.0);
            let gap = last.y - line.y;
            if overlap > narrower * 0.3 && gap >= -0.5 && gap < 2.2 * last.size.max(line.size) {
                best = Some(index);
                break;
            }
        }
        match best {
            Some(index) => blocks[index].push(line),
            None => blocks.push(vec![line]),
        }
    }
    blocks
}

/// Items into reading order, each with the left and right edges of the
/// column it sits in: a wide item reads before everything under it; narrow
/// ones between two wide ones read column by column.
fn order(items: Vec<Item>, text_left: f64, text_right: f64) -> Vec<(Item, (f64, f64))> {
    let text_width = text_right - text_left;
    let whole = (text_left, text_right);
    let mut items: Vec<(Item, Bounds)> = items
        .into_iter()
        .map(|item| {
            let bounds = item.bounds();
            (item, bounds)
        })
        .collect();
    items.sort_by(|a, b| b.1 .3.partial_cmp(&a.1 .3).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::new();
    let mut pending: Vec<(Item, Bounds)> = Vec::new();
    for (item, bounds) in items {
        let wide = bounds.2 - bounds.0 > 0.65 * text_width;
        if wide {
            flush(&mut pending, &mut out, whole);
            out.push((item, whole));
        } else {
            pending.push((item, bounds));
        }
    }
    flush(&mut pending, &mut out, whole);
    out
}

/// Pending narrow items, column by column, top to bottom in each. One
/// column is the whole text's width; two or more are each their own.
fn flush(pending: &mut Vec<(Item, Bounds)>, out: &mut Vec<(Item, (f64, f64))>, whole: (f64, f64)) {
    if pending.is_empty() {
        return;
    }
    let mut taken: Vec<(Item, Bounds)> = std::mem::take(pending);
    taken.sort_by(|a, b| a.1 .0.partial_cmp(&b.1 .0).unwrap_or(std::cmp::Ordering::Equal));
    // Columns: an item starts a new one when it begins past the right edge
    // of the one before.
    let mut columns: Vec<(f64, Vec<usize>)> = Vec::new();
    for (index, (_, bounds)) in taken.iter().enumerate() {
        match columns.last_mut() {
            Some((right, members)) if bounds.0 < *right - 2.0 => {
                members.push(index);
                *right = right.max(bounds.2);
            }
            _ => columns.push((bounds.2, vec![index])),
        }
    }
    // Columns are only columns when they stand side by side: two groups
    // of items one above the other, however they are placed across, are
    // read top to bottom.
    let mut index = 0;
    while index + 1 < columns.len() {
        let extent = |members: &[usize]| {
            let top = members.iter().map(|&i| taken[i].1 .3).fold(f64::NEG_INFINITY, f64::max);
            let bottom = members.iter().map(|&i| taken[i].1 .1).fold(f64::INFINITY, f64::min);
            (bottom, top)
        };
        let (bottom_a, top_a) = extent(&columns[index].1);
        let (bottom_b, top_b) = extent(&columns[index + 1].1);
        let overlap = top_a.min(top_b) - bottom_a.max(bottom_b);
        let shorter = (top_a - bottom_a).min(top_b - bottom_b);
        if overlap < shorter * 0.3 {
            let (right, members) = columns.remove(index + 1);
            columns[index].0 = columns[index].0.max(right);
            columns[index].1.extend(members);
            if index > 0 {
                index -= 1;
            }
        } else {
            index += 1;
        }
    }
    let edges: Vec<(f64, f64)> = columns
        .iter()
        .map(|(_, members)| {
            if columns.len() == 1 {
                return whole;
            }
            (
                members.iter().map(|&i| taken[i].1 .0).fold(f64::INFINITY, f64::min),
                members.iter().map(|&i| taken[i].1 .2).fold(f64::NEG_INFINITY, f64::max),
            )
        })
        .collect();
    let mut placed: Vec<(usize, usize, f64)> = Vec::new();
    for (column, (_, members)) in columns.iter().enumerate() {
        for &index in members {
            placed.push((column, index, taken[index].1 .3));
        }
    }
    placed.sort_by(|a, b| {
        a.0.cmp(&b.0).then(b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut items: Vec<Option<Item>> = taken.into_iter().map(|(item, _)| Some(item)).collect();
    for (column, index, _) in placed {
        if let Some(item) = items[index].take() {
            out.push((item, edges[column]));
        }
    }
}

/// Where a line's text starts after any list marker, and what the marker
/// is: a bullet, or a number.
fn marker_of(line: &Line) -> Option<(Marker, usize)> {
    let text = line.text();
    let mut chars = text.char_indices();
    let (_, first) = chars.next()?;
    const BULLETS: &[char] = &[
        '\u{2022}', '\u{25E6}', '\u{25AA}', '\u{25CF}', '\u{25A0}', '\u{25A1}', '\u{25CB}',
        '\u{2013}', '-', '*', '\u{27A2}', '\u{2713}', '\u{2714}', '\u{2756}', '\u{25C6}',
        '\u{25C7}', '\u{2023}', '\u{204C}', '\u{204D}', '\u{2043}', '\u{25AB}', '\u{FFED}',
        '\u{2219}', '\u{00B7}', '\u{2B24}', '\u{2751}', '\u{274F}', '\u{2605}', '\u{2606}',
        '\u{2192}', '\u{21D2}', '\u{2794}', '\u{27A4}', '\u{25BA}', '\u{25B6}', '\u{2751}',
    ];
    if BULLETS.contains(&first) {
        let rest = &text[first.len_utf8()..];
        if rest.starts_with(' ') || rest.is_empty() {
            let skip = first.len_utf8() + rest.len() - rest.trim_start().len();
            return Some((Marker::Bullet, skip));
        }
        return None;
    }
    // A number: `1.`, `1)`, `(1)`, `a.`, `iv.`, `2.3.1`.
    let mut end = 0;
    let bytes = text.as_bytes();
    let mut at = 0;
    if bytes.first() == Some(&b'(') {
        at = 1;
    }
    let label_start = at;
    while at < bytes.len()
        && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'.')
        && at - label_start < 8
    {
        at += 1;
    }
    if at == label_start {
        return None;
    }
    let label = &text[label_start..at];
    let label_body = label.trim_end_matches('.');
    if label_body.is_empty() {
        return None;
    }
    let numeric = label_body
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    let lettered = label_body.len() == 1 && label_body.bytes().all(|b| b.is_ascii_alphabetic());
    let roman = label_body.len() <= 5
        && label_body.bytes().all(|b| b"ivxlcIVXLC".contains(&b))
        && label_body.len() > 1
        || matches!(label_body, "i" | "v" | "x" | "I" | "V" | "X");
    if !(numeric || lettered || roman) {
        return None;
    }
    let closes = if bytes.first() == Some(&b'(') {
        if bytes.get(at) == Some(&b')') {
            at += 1;
            true
        } else {
            false
        }
    } else if label.ends_with('.') && (numeric || lettered || roman) {
        true
    } else if matches!(bytes.get(at), Some(b')' | b'.')) {
        at += 1;
        true
    } else {
        false
    };
    if !closes {
        return None;
    }
    // A word like `a.` alone is not a list; the marker must be followed
    // by a space and then something.
    let rest = &text[at..];
    if !rest.starts_with(' ') || rest.trim_start().is_empty() {
        return None;
    }
    end += at + rest.len() - rest.trim_start().len();
    let level = if numeric { label_body.matches('.').count().min(2) } else { 0 };
    Some((Marker::Number(level), end))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Marker {
    Bullet,
    Number(usize),
}

/// A block's lines into paragraphs.
fn paragraphs_of(
    lines: Vec<Line>,
    column_left: f64,
    column_right: f64,
    body_size: f64,
    page_centre: f64,
) -> Vec<Built> {
    let gaps: Vec<f64> = lines.windows(2).map(|w| w[0].y - w[1].y).filter(|g| *g > 0.5).collect();
    let mut sorted = gaps.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pitch = sorted
        .first()
        .copied()
        .unwrap_or_else(|| lines.first().map_or(body_size, |l| l.size) * 1.2);
    let pitch = pitch.max(lines.first().map_or(body_size, |l| l.size) * 0.9);

    let mut paragraphs: Vec<Vec<Line>> = Vec::new();
    let mut markers: Vec<Option<(Marker, usize)>> = Vec::new();
    for line in lines {
        let marker = marker_of(&line);
        let breaks = match paragraphs.last() {
            None => true,
            Some(current) => {
                let previous = current.last().expect("a paragraph holds a line");
                let gap = previous.y - line.y;
                let previous_marker = markers.last().copied().flatten();
                // A line well short of the edge ends a paragraph — unless the
                // next line starts in lower case, which is a word carried on.
                let ends_short = previous.x1 < column_right - 4.0 * previous.size
                    && !line.text().starts_with(|c: char| c.is_lowercase());
                let starts_at_left = line.x0 <= previous.x0 + 1.0
                    || current.len() >= 2 && line.x0 <= current[1].x0 + 1.0;
                let indented = line.x0 > previous.x0 + 3.0
                    && previous_marker.is_none()
                    && (current.len() != 1 || line.x0 > current[0].x0 + 3.0);
                let dedented = line.x0 < previous.x0 - 3.0
                    && (current.len() >= 2 || previous_marker.is_some());
                let size_change =
                    (line.size - previous.size).abs() > 0.15 * previous.size.max(line.size);
                gap > 1.4 * pitch
                    || marker.is_some()
                    || (ends_short && starts_at_left)
                    || (ends_short && line.x0 < previous.x0 - 3.0)
                    || indented
                    || dedented
                    || size_change
                    || (previous.is_bold() != line.is_bold() && ends_short)
            }
        };
        if breaks {
            paragraphs.push(vec![line]);
            markers.push(marker);
        } else if let Some(current) = paragraphs.last_mut() {
            current.push(line);
        }
    }

    let mut built = Vec::new();
    let count = paragraphs.len();
    let mut next_first_y: Vec<Option<f64>> =
        paragraphs.iter().skip(1).map(|p| p.first().map(|l| l.y)).collect();
    next_first_y.push(None);
    for (index, (mut lines, marker)) in paragraphs.into_iter().zip(markers).enumerate() {
        let mut properties = ParagraphProperties::default();
        let size = dominant_size(&lines);
        let bold = lines.iter().all(Line::is_bold);
        let width = (column_right - column_left).max(1.0);
        let first_x0 = lines[0].x0;
        let ends_full = lines.last().is_some_and(|l| l.x1 >= column_right - 1.5 * l.size);
        // Alignment, from the margins the lines leave.
        let lefts: Vec<f64> = lines.iter().map(|l| l.x0 - column_left).collect();
        let rights: Vec<f64> = lines.iter().map(|l| column_right - l.x1).collect();
        // Centred in the column, or — since the column's right edge is only
        // where the longest line reached — on the page's own centre line.
        let centred = lines.iter().zip(&lefts).zip(&rights).all(|((line, left), right)| {
            *left > 4.0
                && ((left - right).abs() < 6.0 + width * 0.03
                    || ((line.x0 + line.x1) / 2.0 - page_centre).abs() < 4.0 + width * 0.02)
        });
        // Right-aligned: every line ends at the right edge and starts well
        // in, and the lines start in different places (or there is one).
        let right_aligned = !centred
            && rights.iter().all(|r| *r < 2.0)
            && lefts.iter().all(|l| *l > 8.0)
            && (lefts.len() == 1 || lefts.windows(2).any(|w| (w[0] - w[1]).abs() > 2.0));
        // Justified: every line but the last reaches both edges (the first
        // may be indented), and the spaces are stretched by different
        // amounts on different lines.
        // The paragraph's own edges, since it may be indented on either side.
        let own_left = lines[1..].iter().map(|l| l.x0).fold(f64::INFINITY, f64::min);
        let own_right =
            lines[..lines.len() - 1].iter().map(|l| l.x1).fold(f64::NEG_INFINITY, f64::max);
        let full_but_last = lines.len() >= 3
            && lines[..lines.len() - 1].iter().all(|l| own_right - l.x1 < 2.0)
            && lines[1..lines.len() - 1].iter().all(|l| l.x0 - own_left < 2.0);
        // The last line is set with the font's own spaces; the full lines
        // of a justified paragraph have theirs stretched, by amounts that
        // differ from line to line.
        let justified = full_but_last && {
            let average = |l: &Line| {
                (!l.space_widths.is_empty())
                    .then(|| l.space_widths.iter().sum::<f64>() / l.space_widths.len() as f64)
            };
            let full: Vec<f64> = lines[..lines.len() - 1].iter().filter_map(average).collect();
            let natural = lines.last().and_then(average);
            let (min, max) =
                full.iter().fold((f64::INFINITY, 0.0f64), |(lo, hi), &a| (lo.min(a), hi.max(a)));
            !full.is_empty()
                && (natural.is_some_and(|natural| min > natural * 1.08)
                    || (full.len() >= 2 && max > min * 1.25))
        };
        let mut marker_x = None;
        if let Some((kind, skip)) = marker {
            let mut skip = skip;
            let mut kept = Vec::new();
            for piece in lines[0].pieces.drain(..) {
                if skip >= piece.text.len() && skip > 0 {
                    skip -= piece.text.len();
                    continue;
                }
                kept.push(piece);
            }
            marker_x = Some(first_x0);
            lines[0].pieces = kept;
            if let Some(first_kept) = lines[0].pieces.first() {
                lines[0].x0 = first_kept.x0;
            }
            properties.numbering = Some(NumberingReference {
                id: match kind {
                    Marker::Bullet => wp_docx::BULLET_LIST,
                    Marker::Number(_) => wp_docx::NUMBERED_LIST,
                },
                level: match kind {
                    Marker::Number(level) => level as u8,
                    Marker::Bullet => 0,
                },
            });
        } else if centred {
            properties.alignment = Some(Alignment::Center);
        } else if right_aligned {
            properties.alignment = Some(Alignment::End);
        } else {
            if justified {
                properties.alignment = Some(Alignment::Both);
            }
            let body_left = if lines.len() >= 2 {
                lines[1..].iter().map(|l| l.x0).fold(f64::INFINITY, f64::min)
            } else {
                lines[0].x0
            };
            let indent = body_left - column_left;
            if indent > 2.0 {
                properties.indent_start = Some((indent * 20.0).round() as i32);
            }
            let first_indent = lines[0].x0 - body_left;
            if first_indent.abs() > 2.0 && lines.len() >= 2 {
                properties.indent_first_line = Some((first_indent * 20.0).round() as i32);
            }
        }
        // The space after, from the gap to the next paragraph.
        if index + 1 < count {
            if let (Some(last), Some(next_y)) = (lines.last(), next_first_y[index]) {
                let extra = (last.y - next_y) - pitch;
                if extra > 1.5 {
                    properties.space_after = Some(((extra.min(3.0 * size)) * 20.0).round() as i32);
                }
            }
        }
        built.push(Built { lines, properties, marker_x, size, bold, ends_full });
    }
    built
}

fn dominant_size(lines: &[Line]) -> f64 {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for line in lines {
        for piece in &line.pieces {
            if !piece.is_space() {
                *counts.entry((piece.size * 2.0).round() as u32).or_default() +=
                    piece.text.chars().count();
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(size, count)| (*count, *size))
        .map_or(11.0, |(size, _)| f64::from(size) / 2.0)
}

/// A paragraph cut by a page end continues on the next page when its last
/// line was full and the next page starts with a plain line of the same
/// size.
fn join_across_pages(sequence: &mut Vec<Sequenced>) {
    let mut index = 0;
    while index + 2 < sequence.len() {
        if matches!(sequence[index + 1], Sequenced::PageEnd) {
            let joins = match (&sequence[index], &sequence[index + 2]) {
                (Sequenced::Paragraph { built: before }, Sequenced::Paragraph { built: after }) => {
                    before.ends_full
                        && after.marker_x.is_none()
                        && before.marker_x.is_none()
                        && after.properties.indent_first_line.is_none_or(|i| i <= 0)
                        && (after.size - before.size).abs() < 0.5
                        && after.properties.alignment == before.properties.alignment
                        && !after.lines.first().is_some_and(|l| {
                            l.text().starts_with(|c: char| c.is_uppercase())
                                && before
                                    .lines
                                    .last()
                                    .is_some_and(|l| l.text().ends_with(['.', '!', '?', ':']))
                        })
                }
                _ => false,
            };
            if joins {
                let Sequenced::Paragraph { built: after } = sequence.remove(index + 2) else {
                    unreachable!()
                };
                if let Sequenced::Paragraph { built: before } = &mut sequence[index] {
                    before.lines.extend(after.lines);
                    before.ends_full = after.ends_full;
                    if after.properties.space_after.is_some() {
                        before.properties.space_after = after.properties.space_after;
                    }
                }
                continue;
            }
        }
        index += 1;
    }
}

/// The sizes headings come in, largest first: a paragraph bigger than the
/// body and bold, or much bigger, of a few lines.
fn heading_sizes(sequence: &[Sequenced], body_size: f64) -> Vec<f64> {
    let mut sizes: Vec<f64> = Vec::new();
    for item in sequence {
        if let Sequenced::Paragraph { built } = item {
            if is_heading(built, body_size) && !sizes.iter().any(|s| (s - built.size).abs() < 0.5) {
                sizes.push(built.size);
            }
        }
    }
    sizes.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    sizes.truncate(6);
    sizes
}

fn is_heading(built: &Built, body_size: f64) -> bool {
    built.marker_x.is_none()
        && built.lines.len() <= 3
        && built.size >= body_size * 1.15
        && (built.bold || built.size >= body_size * 1.35)
        && built.lines.iter().map(|l| l.text().chars().count()).sum::<usize>() < 200
}

/// The levels list markers stand at, by how far in they are: the first
/// column of markers is level nought, the next level one, and so on.
fn marker_levels(sequence: &[Sequenced]) -> Vec<f64> {
    let xs: Vec<f64> = sequence
        .iter()
        .filter_map(|item| match item {
            Sequenced::Paragraph { built } => built.marker_x,
            _ => None,
        })
        .collect();
    let mut levels = cluster(xs, 6.0);
    levels.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    levels
}

/// The finished paragraph: runs from the lines, the style from the size.
fn paragraph_of(
    built: Built,
    body_size: f64,
    heading_sizes: &[f64],
    marker_levels: &[f64],
    found: &mut Found,
    paragraph_index: usize,
) -> Paragraph {
    let heading = heading_sizes.iter().position(|s| (s - built.size).abs() < 0.5);
    let heading = if is_heading(&built, body_size) { heading } else { None };
    let Built { lines, mut properties, marker_x, .. } = built;
    if let (Some(marker_x), Some(numbering)) = (marker_x, properties.numbering.as_mut()) {
        if numbering.level == 0 {
            let level = marker_levels.iter().position(|x| (x - marker_x).abs() <= 6.0).unwrap_or(0);
            numbering.level = level.min(wp_docx::LIST_LEVELS as usize - 1) as u8;
        }
    }
    if let Some(level) = heading {
        properties.style = Some(format!("Heading{}", level + 1));
    }
    let runs = runs_of(&lines, found, paragraph_index);
    Paragraph { properties, runs }
}

/// Lines into runs: joined with a space, or without the hyphen that broke
/// a word; pieces alike in format into one run.
fn runs_of(lines: &[Line], found: &mut Found, paragraph_index: usize) -> Vec<Run> {
    let mut pieces: Vec<Piece> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            let tail: String = pieces
                .iter()
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|p| p.text.as_str())
                .collect();
            let hyphenated = tail.ends_with(['-', '\u{AD}'])
                && tail.chars().rev().nth(1).is_some_and(char::is_alphabetic)
                && line
                    .pieces
                    .first()
                    .is_some_and(|first| first.text.starts_with(|c: char| c.is_lowercase()));
            if hyphenated {
                if let Some(last) = pieces.last_mut() {
                    last.text.pop();
                    if last.text.is_empty() {
                        pieces.pop();
                    }
                }
            } else if let Some(last) = pieces.last() {
                let mut space = last.clone();
                space.text = " ".to_owned();
                space.link = if line.pieces.first().and_then(|p| p.link) == last.link {
                    last.link
                } else {
                    None
                };
                pieces.push(space);
            }
        }
        pieces.extend(line.pieces.iter().cloned());
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut offset = 0usize;
    let mut open_link: Option<(usize, usize)> = None;
    for piece in pieces {
        match (open_link, piece.link) {
            (Some((index, _)), Some(link)) if link == index => {}
            (Some((index, start)), _) => {
                found.links.push(LinkFound {
                    paragraph: paragraph_index,
                    start,
                    end: offset,
                    address: found.addresses.get(index).cloned().unwrap_or_default(),
                });
                open_link = piece.link.map(|link| (link, offset));
            }
            (None, Some(link)) => open_link = Some((link, offset)),
            (None, None) => {}
        }
        if let Some(picture) = piece.picture.and_then(|index| found.inline.get_mut(index)?.take()) {
            found.pictures.push(PictureFound {
                paragraph: paragraph_index,
                offset,
                bytes: picture.bytes,
                extension: picture.extension,
                width_emu: ((picture.x1 - picture.x0) * 12700.0).round() as i64,
                height_emu: ((picture.y1 - picture.y0) * 12700.0).round() as i64,
            });
        }
        let properties = properties_of(&piece.format);
        match runs.last_mut() {
            Some(last) if last.properties == properties => {
                if let Some(RunContent::Text(text)) = last.content.last_mut() {
                    text.push_str(&piece.text);
                }
            }
            _ => runs.push(Run {
                properties,
                content: vec![RunContent::Text(piece.text.clone())],
                field: None,
                revision: None,
                format_change: None,
            }),
        }
        offset += piece.text.len();
    }
    if let Some((index, start)) = open_link {
        found.links.push(LinkFound {
            paragraph: paragraph_index,
            start,
            end: offset,
            address: found.addresses.get(index).cloned().unwrap_or_default(),
        });
    }
    runs
}

fn properties_of(format: &Format) -> RunProperties {
    RunProperties {
        bold: format.bold.then_some(true),
        italic: format.italic.then_some(true),
        underline: format.underline.then_some(Underline::Single),
        strike: format.strike.then_some(true),
        size_half_points: Some(format.half_points),
        color: (format.colour != [0, 0, 0]).then(|| {
            format!("{:02X}{:02X}{:02X}", format.colour[0], format.colour[1], format.colour[2])
        }),
        font: (!format.family.is_empty()).then(|| format.family.clone()),
        vertical_align: (format.vertical != VerticalAlignment::Baseline).then_some(format.vertical),
        ..RunProperties::default()
    }
}

/// A grid into a table: its cells' lines into paragraphs, spans where a
/// border is missing.
fn table_of(grid: Grid, body_size: f64, found: &mut Found, paragraphs_done: &mut usize) -> Table {
    let rows = grid.rows();
    let columns = grid.columns();
    // The borders were what made it a table, so it keeps them.
    let mut table = Table {
        grid: grid.xs.windows(2).map(|w| ((w[1] - w[0]) * 20.0).round() as i32).collect(),
        borders: wp_docx::model::TableBorders::grid(),
        ..Table::default()
    };
    let mut cells: Vec<Vec<Line>> = grid.cells.clone();
    for row in 0..rows {
        let mut table_row = TableRow::default();
        let mut column = 0;
        while column < columns {
            let mut span = 1;
            while column + span < columns && !grid.has_vertical(column + span, row) {
                span += 1;
            }
            let merged_upwards = row > 0 && !grid.has_horizontal(row, column, column + span);
            let mut lines = Vec::new();
            for c in column..column + span {
                lines.append(&mut cells[row * columns + c]);
            }
            let mut blocks = Vec::new();
            let left = grid.xs[column];
            let right = grid.xs[column + span];
            if !lines.is_empty() {
                lines.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
                for built in
                    paragraphs_of(lines, left + 2.0, right - 2.0, body_size, (left + right) / 2.0)
                {
                    let mut built = built;
                    built.properties.space_after = None;
                    let paragraph =
                        paragraph_of(built, body_size, &[], &[], found, *paragraphs_done);
                    blocks.push(Block::Paragraph(paragraph));
                    *paragraphs_done += 1;
                }
            }
            if blocks.is_empty() {
                blocks.push(Block::Paragraph(Paragraph::default()));
                *paragraphs_done += 1;
            }
            table_row.cells.push(TableCell {
                blocks,
                width: Some(((right - left) * 20.0).round() as i32),
                span: span as u32,
                merged_upwards,
                ..TableCell::default()
            });
            column += span;
        }
        table.rows.push(table_row);
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x0: f64, y: f64, size: f64) -> Line {
        let mut pieces = Vec::new();
        let mut x = x0;
        for c in text.chars() {
            let width = size * 0.5;
            pieces.push(Piece {
                x0: x,
                x1: x + width,
                y,
                size,
                text: c.to_string(),
                format: Format {
                    family: "Arial".into(),
                    bold: false,
                    italic: false,
                    half_points: (size * 2.0) as u32,
                    colour: [0, 0, 0],
                    underline: false,
                    strike: false,
                    vertical: VerticalAlignment::Baseline,
                },
                link: None,
                picture: None,
            });
            x += width;
        }
        Line { y, x0, x1: x, size, pieces, space_widths: Vec::new() }
    }

    #[test]
    fn markers_are_known() {
        assert_eq!(
            marker_of(&line("\u{2022} Milk", 0.0, 0.0, 10.0)).map(|m| m.0),
            Some(Marker::Bullet)
        );
        assert_eq!(
            marker_of(&line("1. First", 0.0, 0.0, 10.0)).map(|m| m.0),
            Some(Marker::Number(0))
        );
        assert_eq!(
            marker_of(&line("(2) Second", 0.0, 0.0, 10.0)).map(|m| m.0),
            Some(Marker::Number(0))
        );
        assert_eq!(
            marker_of(&line("2.1. Deeper", 0.0, 0.0, 10.0)).map(|m| m.0),
            Some(Marker::Number(1))
        );
        assert_eq!(
            marker_of(&line("iv) Fourth", 0.0, 0.0, 10.0)).map(|m| m.0),
            Some(Marker::Number(0))
        );
        assert!(marker_of(&line("a.m. is early", 0.0, 0.0, 10.0)).is_none());
        assert!(marker_of(&line("Plain text.", 0.0, 0.0, 10.0)).is_none());
        assert!(marker_of(&line("-5 degrees", 0.0, 0.0, 10.0)).is_none());
    }

    #[test]
    fn a_wider_gap_starts_a_paragraph_and_lines_join_with_a_space() {
        let lines = vec![
            line("The first line of one", 72.0, 700.0, 10.0),
            line("paragraph goes on here", 72.0, 688.0, 10.0),
            line("Another paragraph", 72.0, 668.0, 10.0),
        ];
        let built = paragraphs_of(lines, 72.0, 300.0, 10.0, 186.0);
        assert_eq!(built.len(), 2);
        let mut found = Found::default();
        let runs = runs_of(&built[0].lines, &mut found, 0);
        let text: String = runs
            .iter()
            .map(|r| {
                r.content
                    .iter()
                    .map(|c| match c {
                        RunContent::Text(t) => t.as_str(),
                        _ => "",
                    })
                    .collect::<String>()
            })
            .collect();
        assert_eq!(text, "The first line of one paragraph goes on here");
        assert!(built[0].properties.space_after.is_some());
    }

    #[test]
    fn a_hyphen_at_the_line_end_is_taken_out() {
        let lines = vec![line("docu-", 72.0, 700.0, 10.0), line("ment", 72.0, 688.0, 10.0)];
        let mut found = Found::default();
        let runs = runs_of(&lines, &mut found, 0);
        let RunContent::Text(text) = &runs[0].content[0] else { panic!() };
        assert_eq!(text, "document");
    }

    #[test]
    fn a_bullet_line_is_a_list_item_without_its_bullet() {
        let lines = vec![
            line("\u{2022} Milk", 72.0, 700.0, 10.0),
            line("\u{2022} Bread", 72.0, 688.0, 10.0),
        ];
        let built = paragraphs_of(lines, 72.0, 300.0, 10.0, 186.0);
        assert_eq!(built.len(), 2);
        assert_eq!(built[0].properties.numbering.map(|n| n.id), Some(wp_docx::BULLET_LIST));
        assert_eq!(built[0].lines[0].text(), "Milk");
    }

    #[test]
    fn a_centred_line_is_centred() {
        let built =
            paragraphs_of(vec![line("Title", 160.0, 700.0, 20.0)], 72.0, 298.0, 10.0, 185.0);
        assert_eq!(built[0].properties.alignment, Some(Alignment::Center));
    }

    #[test]
    fn crossing_rules_make_a_grid() {
        let mut rectangles = Vec::new();
        for y in [700.0, 680.0, 660.0] {
            rectangles.push(Rectangle {
                x0: 100.0,
                y0: y - 0.5,
                x1: 300.0,
                y1: y + 0.5,
                filled: true,
            });
        }
        for x in [100.0, 200.0, 300.0] {
            rectangles.push(Rectangle {
                x0: x - 0.5,
                y0: 660.0,
                x1: x + 0.5,
                y1: 700.0,
                filled: true,
            });
        }
        let mut lines = vec![
            line("A", 105.0, 686.0, 10.0),
            line("B", 205.0, 686.0, 10.0),
            line("Out", 105.0, 600.0, 10.0),
        ];
        let grids = grids_of(&rectangles, &mut lines);
        assert_eq!(grids.len(), 1);
        assert_eq!((grids[0].rows(), grids[0].columns()), (2, 2));
        assert_eq!(lines.len(), 1, "the line outside stays out");
        assert_eq!(grids[0].cells[0][0].text(), "A");
        assert_eq!(grids[0].cells[1][0].text(), "B");
    }
}

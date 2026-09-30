//! The document as Word's outline view shows it: every paragraph with a mark
//! beside it, indented to its level, folded under its heading or not, and
//! moved about and promoted from the Outlining tab or by its mark.
//!
//! # Why this needs no map between the outline and the document
//!
//! Word's outline view is the document, laid out differently: each paragraph
//! indented to the depth of its heading, and anything below the level being
//! shown left out. Nothing is renumbered and nothing is copied, so a caret in
//! the outline is a caret in the document and typing is typing — which is why
//! there is no map here to go wrong.
//!
//! The one thing that has to be looked after is a caret in a paragraph the
//! level has just hidden. It is moved to the nearest paragraph still on show,
//! because a caret nobody can see is a caret typing into the dark.
//!
//! # What is the view's and what is the document's
//!
//! A heading's level is the document's: promoting one gives it the style of
//! the level above, and that is saved, and taken back by one undo. Which
//! headings are folded, how deep the outline is shown, whether it is shown in
//! its formatting and a line at a time, are the view's: they are kept here, in
//! memory, for as long as the window looks at the document, and nothing of
//! them goes into the file.
//!
//! # The marks, as Word draws them
//!
//! Word's outline puts a mark at the left of every paragraph. The review's
//! picture of Word's own window (`word-outline-dark.png`) shows two of them:
//! a heading with something under it has a filled circle with a plus cut out
//! of it, and body text has a small filled dot. A heading with nothing under
//! it has the same circle with a minus, which is what Word 365 draws and what
//! the guides to its outline view describe beside the other two. A table is
//! body text to the outline, and each of its rows has a dot.

use std::collections::HashSet;
use std::ops::Range;

use wp_docx::outlining::{moved_to, Standing};
use wp_docx::TextPosition;
use wp_raster::{Color, Path, Point};
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};
use crate::messages::{t, with};

use super::views::View;
use super::Editor;

/// Every level a heading can be, and then everything.
///
/// Ten is what the engine reads as "all levels": nine headings and the body
/// text under them.
pub(super) const ALL_LEVELS: u8 = 10;

/// How far the middle of a mark stands to the left of its paragraph, in
/// points: three quarters of a level's step, which is where Word's stand.
const MARK_GAP: f32 = wp_layout::OUTLINE_STEP * 0.75;

/// How far round a heading's mark reaches, and a dot's, in points.
const HEADING_MARK: f32 = 4.5;
const BODY_MARK: f32 = 2.4;

/// How far the pointer has to move with a mark held before it is a drag
/// rather than a click that was not quite still.
const DRAG_SLACK: i32 = 4;

/// What the outline view keeps about itself.
///
/// The view's, never the document's: see the module's note.
#[derive(Clone, Debug)]
pub(super) struct Outlining {
    /// The headings folded, each by its paragraph and the words it had when
    /// it was last looked at — see [`Editor::keep_folds`].
    folded: Vec<Fold>,
    /// Word's Show First Line Only.
    pub(super) first_line_only: bool,
    /// Word's Show Text Formatting, which is on until it is taken off.
    pub(super) show_formatting: bool,
    /// Where every paragraph stood when the document was last laid out as an
    /// outline, and how many paragraphs there were.
    standings: Vec<Standing>,
    /// A mark held down, while it is.
    drag: Option<MarkDrag>,
}

impl Default for Outlining {
    fn default() -> Self {
        Self {
            folded: Vec::new(),
            first_line_only: false,
            show_formatting: true,
            standings: Vec::new(),
            drag: None,
        }
    }
}

/// A heading folded, and what it said.
///
/// The words are kept because a paragraph's number is not the paragraph: a
/// paragraph put in above it moves it down one, and the fold has to go with
/// it rather than stay where it was and fold whatever came to be there.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fold {
    paragraph: usize,
    text: String,
}

/// A mark held down: whose, where the press was, and whether it has moved
/// far enough to be a drag.
#[derive(Clone, Copy, Debug)]
struct MarkDrag {
    paragraph: usize,
    x: i32,
    y: i32,
    moved: bool,
}

/// What a mark says about its paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MarkKind {
    /// A heading with something under it: a circle with a plus in it.
    Parent,
    /// A heading with nothing under it: a circle with a minus in it.
    Childless,
    /// Body text, or a row of a table: a small dot.
    Body,
}

/// One mark, where it is drawn in the window.
#[derive(Clone, Copy, Debug)]
pub(super) struct Mark {
    pub(super) paragraph: usize,
    pub(super) kind: MarkKind,
    /// Its middle, and how far round it reaches.
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) radius: f32,
    /// Whether it can be taken hold of. A table's rows are marked and are
    /// moved with the table, not by their own marks.
    pub(super) movable: bool,
}

/// One line of the outline where it is drawn in the window.
#[derive(Clone, Copy, Debug)]
struct Drawn {
    paragraph: usize,
    /// The left edge of the sheet the line is on.
    sheet: f32,
    top: f32,
    bottom: f32,
    left: f32,
    right: f32,
    baseline: f32,
    ascent: f32,
}

/// A paragraph's heading level, counted from nought, if it is a heading.
fn level(standings: &[Standing], paragraph: usize) -> Option<u8> {
    standings.get(paragraph).and_then(|standing| standing.level())
}

/// Where what a heading holds ends: at the next heading as high as it or
/// higher, or at the end of the document. For anything that is not a
/// heading, just past itself.
fn section_end(standings: &[Standing], heading: usize) -> usize {
    let Some(own) = level(standings, heading) else {
        return (heading + 1).min(standings.len());
    };
    (heading + 1..standings.len())
        .find(|at| level(standings, *at).is_some_and(|found| found <= own))
        .unwrap_or(standings.len())
}

/// Whether a heading has anything under it: body text, a table, or a lower
/// heading.
fn has_content(standings: &[Standing], heading: usize) -> bool {
    level(standings, heading).is_some() && section_end(standings, heading) > heading + 1
}

/// The heading a paragraph is under — or is.
fn heading_of(standings: &[Standing], paragraph: usize) -> Option<usize> {
    (0..=paragraph.min(standings.len().saturating_sub(1)))
        .rev()
        .find(|at| level(standings, *at).is_some())
}

/// How many steps in each paragraph is, as the engine puts it: a heading one
/// for each level above its own, body text one past the heading above it or
/// one when there is none. See `LayoutEngine::outline_step`.
fn steps(standings: &[Standing]) -> Vec<u8> {
    let mut above = 0u8;
    standings
        .iter()
        .map(|standing| match standing {
            Standing::Heading(level) => {
                above = level + 1;
                *level
            }
            Standing::Body | Standing::InTable => above.max(1),
        })
        .collect()
}

/// The pieces a paragraph is moved among, in order, and which of them is its
/// own.
///
/// A heading moves with everything under it among the headings of its own
/// level under the same heading. Body text moves a paragraph at a time among
/// the body text between the same two headings, where a table is one piece.
fn units(standings: &[Standing], paragraph: usize) -> Option<(Vec<Range<usize>>, usize)> {
    let count = standings.len();
    let units: Vec<Range<usize>> = match *standings.get(paragraph)? {
        Standing::Heading(own) => {
            let higher = |at: &usize| level(standings, *at).is_some_and(|found| found < own);
            let start = (0..paragraph).rev().find(higher).map_or(0, |at| at + 1);
            let end = (paragraph + 1..count).find(higher).unwrap_or(count);
            (start..end)
                .filter(|at| level(standings, *at) == Some(own))
                .map(|at| at..section_end(standings, at))
                .collect()
        }
        Standing::Body | Standing::InTable => {
            let heading = |at: &usize| level(standings, *at).is_some();
            let start = (0..paragraph).rev().find(heading).map_or(0, |at| at + 1);
            let end = (paragraph + 1..count).find(heading).unwrap_or(count);
            let mut units = Vec::new();
            let mut at = start;
            while at < end {
                let mut next = at + 1;
                if standings[at] == Standing::InTable {
                    while next < end && standings[next] == Standing::InTable {
                        next += 1;
                    }
                }
                units.push(at..next);
                at = next;
            }
            units
        }
    };
    let own = units.iter().position(|unit| unit.contains(&paragraph))?;
    Some((units, own))
}

/// The places a piece can be moved to among the others: before each of them
/// and after the last, by the paragraph it would come before.
fn boundaries(units: &[Range<usize>]) -> Vec<usize> {
    let mut out: Vec<usize> = units.iter().map(|unit| unit.start).collect();
    if let Some(last) = units.last() {
        out.push(last.end);
    }
    out
}

/// A disc as a path: four quarter circles, which is what a rasterizer that
/// fills paths needs to draw a round thing with a soft edge.
fn disc(x: f32, y: f32, radius: f32) -> Path {
    // How far along its tangent a quarter circle's control point stands.
    const KAPPA: f32 = 0.552_284_8;
    let reach = radius * KAPPA;
    let mut path = Path::new();
    path.move_to(Point::new(x + radius, y));
    path.cubic_to(
        Point::new(x + radius, y + reach),
        Point::new(x + reach, y + radius),
        Point::new(x, y + radius),
    );
    path.cubic_to(
        Point::new(x - reach, y + radius),
        Point::new(x - radius, y + reach),
        Point::new(x - radius, y),
    );
    path.cubic_to(
        Point::new(x - radius, y - reach),
        Point::new(x - reach, y - radius),
        Point::new(x, y - radius),
    );
    path.cubic_to(
        Point::new(x + reach, y - radius),
        Point::new(x + radius, y - reach),
        Point::new(x + radius, y),
    );
    path.close();
    path
}

/// One colour most of the way to another.
fn mixed(from: Color, to: Color, share: f32) -> Color {
    let blend = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * share).round() as u8;
    Color::rgb(blend(from.red, to.red), blend(from.green, to.green), blend(from.blue, to.blue))
}

impl Editor {
    // --- Showing it -------------------------------------------------------

    /// How deep the outline goes, for the engine to be told.
    ///
    /// Nothing at all unless the outline is what is being looked at.
    #[must_use]
    pub(super) fn outline_for_layout(&self) -> Option<u8> {
        (self.view == View::Outline).then_some(self.outline_depth.clamp(1, ALL_LEVELS))
    }

    /// Tells the engine what the outline view shows, before the document is
    /// laid out: where each paragraph stands, what is folded away, and the
    /// two ticks. Outside the outline, nothing.
    pub(super) fn prepare_outline(&mut self) {
        if self.view != View::Outline {
            self.outlining.standings.clear();
            self.outlining.drag = None;
            self.engine.set_outline_folded(Vec::new());
            self.engine.set_outline_first_line(false);
            self.engine.set_outline_plain(false);
            return;
        }
        let before = self.outlining.standings.len();
        self.outlining.standings = self.document.outline();
        self.keep_folds(before == self.outlining.standings.len());
        let folded = self.folded_ranges();
        self.engine.set_outline_folded(folded);
        self.engine.set_outline_first_line(self.outlining.first_line_only);
        self.engine.set_outline_plain(!self.outlining.show_formatting);
    }

    /// Follows each folded heading through whatever was done to the
    /// document since the outline was last laid out.
    ///
    /// Still there with the same words, it stays. Moved — a paragraph put in
    /// or taken out above it — it is found again by its words, the nearest
    /// heading that has them. Its own words changed with nothing added or
    /// taken away, which is somebody typing in it, it stays and takes the
    /// new words. Anything else, and the fold goes: a heading that is no
    /// longer there has nothing to fold, and nor has one with nothing under
    /// it.
    fn keep_folds(&mut self, same_count: bool) {
        if self.outlining.folded.is_empty() {
            return;
        }
        let standings = self.outlining.standings.clone();
        let text = |at: usize| self.document.paragraph_text(at).unwrap_or_default();
        let mut kept: Vec<Fold> = Vec::new();
        for fold in &self.outlining.folded {
            let is_heading = level(&standings, fold.paragraph).is_some();
            let found = if is_heading && text(fold.paragraph) == fold.text {
                Some(fold.paragraph)
            } else {
                let mut headings: Vec<usize> =
                    (0..standings.len()).filter(|at| level(&standings, *at).is_some()).collect();
                headings.sort_by_key(|at| at.abs_diff(fold.paragraph));
                headings
                    .into_iter()
                    .find(|at| text(*at) == fold.text)
                    .or_else(|| (same_count && is_heading).then_some(fold.paragraph))
            };
            if let Some(paragraph) = found {
                if has_content(&standings, paragraph)
                    && !kept.iter().any(|other| other.paragraph == paragraph)
                {
                    kept.push(Fold { paragraph, text: text(paragraph) });
                }
            }
        }
        self.outlining.folded = kept;
    }

    /// The paragraphs folded away, as runs of their numbers.
    fn folded_ranges(&self) -> Vec<Range<usize>> {
        let standings = &self.outlining.standings;
        self.outlining
            .folded
            .iter()
            .filter(|fold| has_content(standings, fold.paragraph))
            .map(|fold| fold.paragraph + 1..section_end(standings, fold.paragraph))
            .collect()
    }

    /// Whether a heading is folded.
    #[must_use]
    fn is_folded(&self, heading: usize) -> bool {
        self.outlining.folded.iter().any(|fold| fold.paragraph == heading)
    }

    /// Folds a heading, or opens it out.
    fn set_folded(&mut self, heading: usize, folded: bool) {
        self.outlining.folded.retain(|fold| fold.paragraph != heading);
        if folded {
            let text = self.document.paragraph_text(heading).unwrap_or_default();
            self.outlining.folded.push(Fold { paragraph: heading, text });
        }
    }

    /// Every line of the outline on screen, first to last.
    fn drawn_lines(&self) -> Vec<Drawn> {
        let mut out = Vec::new();
        for (index, page) in self.pages.iter().enumerate() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            for line in &page.lines {
                out.push(Drawn {
                    paragraph: line.paragraph,
                    sheet: origin_x,
                    top: top + line.top(),
                    bottom: top + line.bottom(),
                    left: origin_x + line.left,
                    right: origin_x + line.right,
                    baseline: top + line.baseline,
                    ascent: line.ascent,
                });
            }
        }
        out
    }

    /// Where each paragraph's mark is, and each table row's.
    ///
    /// Worked out in one place for the drawing and the pointer both, so that
    /// a mark drawn is a mark that can be taken hold of where it is drawn.
    #[must_use]
    pub(super) fn outline_marks(&self) -> Vec<Mark> {
        if self.view != View::Outline {
            return Vec::new();
        }
        let standings = &self.outlining.standings;
        let steps = steps(standings);
        let scale = self.pixels_per_inch() / super::POINTS_PER_INCH;
        let margin = self.view_metrics().margin_left * scale;
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for line in self.drawn_lines() {
            let Some(standing) = standings.get(line.paragraph) else { continue };
            if *standing == Standing::InTable || !seen.insert(line.paragraph) {
                continue;
            }
            let kind = match standing {
                Standing::Heading(_) if has_content(standings, line.paragraph) => MarkKind::Parent,
                Standing::Heading(_) => MarkKind::Childless,
                _ => MarkKind::Body,
            };
            // Beside where the paragraph's level begins, which is where the
            // engine put it: the sheet's margin and a step for each level.
            let step = steps.get(line.paragraph).copied().unwrap_or(0);
            let left = line.sheet + margin + f32::from(step) * wp_layout::OUTLINE_STEP * scale;
            let radius = if kind == MarkKind::Body { BODY_MARK } else { HEADING_MARK } * scale;
            out.push(Mark {
                paragraph: line.paragraph,
                kind,
                x: left - MARK_GAP * scale,
                y: line.baseline - line.ascent * 0.3,
                radius,
                movable: true,
            });
        }
        out.extend(self.table_row_marks(scale));
        out
    }

    /// A dot beside each row of each table of the outline, which is how Word
    /// shows a table among the body text: row by row.
    fn table_row_marks(&self, scale: f32) -> Vec<Mark> {
        let mut out = Vec::new();
        for (index, page) in self.pages.iter().enumerate() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            // A row is the cells that share a top edge; its dot goes beside
            // the leftmost of them, level with the first line in it.
            let mut rows: Vec<(f32, f32, f32)> = Vec::new();
            for cell in &page.cells {
                match rows.iter_mut().find(|(y, ..)| (*y - cell.y).abs() < 0.5) {
                    Some(row) => row.1 = row.1.min(cell.x),
                    None => rows.push((cell.y, cell.x, cell.height)),
                }
            }
            for (row_top, row_left, row_height) in rows {
                let first = page
                    .lines
                    .iter()
                    .filter(|line| line.top() >= row_top - 0.5)
                    .filter(|line| line.bottom() <= row_top + row_height + 0.5)
                    .min_by(|one, other| one.baseline.total_cmp(&other.baseline));
                let Some(line) = first else { continue };
                out.push(Mark {
                    paragraph: line.paragraph,
                    kind: MarkKind::Body,
                    x: origin_x + row_left - MARK_GAP * scale,
                    y: top + line.baseline - line.ascent * 0.3,
                    radius: BODY_MARK * scale,
                    movable: false,
                });
            }
        }
        out
    }

    /// Draws the marks beside the paragraphs, the grey line under a heading
    /// whose text is folded away, and where a mark being dragged would land.
    pub(super) fn draw_outline(&mut self) {
        if self.view != View::Outline {
            return;
        }
        let ink = mixed(self.theme.page_text, self.theme.page, 0.25);
        let paper = self.theme.page;
        for mark in self.outline_marks() {
            self.canvas.fill_path(&disc(mark.x, mark.y, mark.radius), ink);
            if mark.kind == MarkKind::Body {
                continue;
            }
            // The plus or the minus is cut out of the disc in the paper's
            // colour, as Word's is.
            let arm = (mark.radius * 0.55).round().max(2.0) as i32;
            let thickness = (mark.radius / 3.0).round().max(1.0) as i32;
            let (x, y) = (mark.x.round() as i32, mark.y.round() as i32);
            let half = thickness / 2;
            self.canvas.fill_rect(x - arm, y - half, arm * 2 + 1 - thickness % 2, thickness, paper);
            if mark.kind == MarkKind::Parent {
                self.canvas.fill_rect(
                    x - half,
                    y - arm,
                    thickness,
                    arm * 2 + 1 - thickness % 2,
                    paper,
                );
            }
        }
        self.draw_folded_lines();
        self.draw_landing();
    }

    /// The grey line Word draws under a heading whose text is not showing,
    /// which is what says there is more under it than the outline shows.
    fn draw_folded_lines(&mut self) {
        let standings = &self.outlining.standings;
        let lines = self.drawn_lines();
        let shown: HashSet<usize> = lines.iter().map(|line| line.paragraph).collect();
        let grey = mixed(self.theme.page_text, self.theme.page, 0.5);
        let scale = self.pixels_per_inch() / super::POINTS_PER_INCH;
        let thickness = scale.round().max(1.0) as i32;
        let under: Vec<Drawn> = lines
            .into_iter()
            .filter(|line| {
                has_content(standings, line.paragraph) && !shown.contains(&(line.paragraph + 1))
            })
            .collect();
        for line in under {
            self.canvas.fill_rect(
                line.left as i32,
                (line.baseline + 2.0 * scale) as i32,
                (line.right - line.left).ceil() as i32,
                thickness,
                grey,
            );
        }
    }

    /// Where a mark being dragged would put its paragraph: a line across the
    /// outline at the place, in the accent colour.
    fn draw_landing(&mut self) {
        let Some(drag) = self.outlining.drag.filter(|drag| drag.moved) else { return };
        let Some((_, y, left)) = self.landing(drag.paragraph, self.pointer_y) else { return };
        let right = self.drawn_lines().iter().map(|line| line.right).fold(left + 1.0, f32::max);
        let accent = self.theme.accent;
        self.canvas.fill_rect(left as i32, y as i32 - 1, (right - left) as i32, 2, accent);
    }

    // --- Taking hold of it ------------------------------------------------

    /// The mark at a point, if there is one that can be taken hold of.
    fn mark_at(&self, x: i32, y: i32) -> Option<Mark> {
        let (x, y) = (x as f32, y as f32);
        self.outline_marks().into_iter().filter(|mark| mark.movable).find(|mark| {
            let reach = mark.radius.max(4.0) + 3.0;
            (mark.x - x).abs() <= reach && (mark.y - y).abs() <= reach
        })
    }

    /// A press on a mark: what it stands for is selected — a heading and
    /// everything under it, a paragraph of body text on its own — and the
    /// mark is held, to be dragged or let go.
    pub(super) fn press_outline_mark(&mut self, x: i32, y: i32) -> Option<Response> {
        let mark = self.mark_at(x, y)?;
        let standings = self.document.outline();
        let (units, own) = units(&standings, mark.paragraph)?;
        self.select_paragraphs(units[own].clone());
        self.outlining.drag = Some(MarkDrag { paragraph: mark.paragraph, x, y, moved: false });
        self.needs_redraw = true;
        Some(Response::Redraw)
    }

    /// Selects whole paragraphs, from the start of the first to the end of
    /// the last.
    fn select_paragraphs(&mut self, paragraphs: Range<usize>) {
        let last = paragraphs.end.saturating_sub(1).max(paragraphs.start);
        let end = self.document.paragraph_text(last).unwrap_or_default().len();
        self.document.set_caret(TextPosition::new(paragraphs.start, 0));
        self.document.extend_selection_to(TextPosition::new(last, end));
    }

    /// The pointer moving while a mark is held. Answers nothing when no mark
    /// is, so the move means whatever else it would.
    pub(super) fn drag_outline_mark(&mut self, x: i32, y: i32, held: bool) -> Option<Response> {
        let mut drag = self.outlining.drag?;
        if !held {
            // Let go somewhere the window did not hear about: over another
            // window, say. The drag ends where the pointer is now.
            return self.release_outline_mark(x, y);
        }
        if (x - drag.x).abs() > DRAG_SLACK || (y - drag.y).abs() > DRAG_SLACK {
            drag.moved = true;
        }
        self.outlining.drag = Some(drag);
        self.needs_redraw = true;
        Some(Response::Redraw)
    }

    /// A held mark let go: a click leaves what it selected, and a drag moves
    /// the heading with everything under it to where it was let go.
    pub(super) fn release_outline_mark(&mut self, _x: i32, y: i32) -> Option<Response> {
        let drag = self.outlining.drag.take()?;
        self.needs_redraw = true;
        if !drag.moved {
            return Some(Response::Redraw);
        }
        let Some((before, ..)) = self.landing(drag.paragraph, y as f32) else {
            return Some(Response::Redraw);
        };
        Some(self.move_outline_piece(drag.paragraph, before, true))
    }

    /// Where a piece dragged by its mark would land if let go at a height:
    /// the paragraph it would come before, and the line across the outline
    /// that shows it — its height and where it begins.
    fn landing(&self, paragraph: usize, y: f32) -> Option<(usize, f32, f32)> {
        let standings = &self.outlining.standings;
        let (units, own) = units(standings, paragraph)?;
        let lines = self.drawn_lines();
        let first_line = |at: usize| lines.iter().find(|line| line.paragraph == at);
        let mut best: Option<(usize, f32, f32)> = None;
        for (number, before) in boundaries(&units).into_iter().enumerate() {
            // Where it is already is nowhere to go.
            if number == own || number == own + 1 {
                continue;
            }
            let height = match units.get(number) {
                Some(unit) => first_line(unit.start).map(|line| line.top),
                None => lines
                    .iter()
                    .filter(|line| units.last().is_some_and(|last| last.contains(&line.paragraph)))
                    .map(|line| line.bottom)
                    .reduce(f32::max),
            };
            let Some(height) = height else { continue };
            let left = first_line(units[own].start).map_or(0.0, |line| line.left);
            if best.is_none_or(|(_, found, _)| (found - y).abs() > (height - y).abs()) {
                best = Some((before, height, left));
            }
        }
        best
    }

    /// Two clicks on a heading's mark fold what is under it away, or bring
    /// it back.
    pub(super) fn double_click_outline_mark(&mut self, x: i32, y: i32) -> Option<Response> {
        let mark = self.mark_at(x, y)?;
        self.outlining.drag = None;
        if mark.kind != MarkKind::Parent {
            return Some(Response::Redraw);
        }
        let folded = !self.is_folded(mark.paragraph);
        Some(self.fold_heading(mark.paragraph, folded))
    }

    // --- The Outlining tab ------------------------------------------------

    /// Folds a heading, or opens it out, and shows the outline again.
    fn fold_heading(&mut self, heading: usize, folded: bool) -> Response {
        self.set_folded(heading, folded);
        self.relayout();
        self.snap_caret_into_view();
        self.needs_redraw = true;
        self.report(if folded { "Collapsed" } else { "Expanded" })
    }

    /// Word's Expand and Collapse: what is under the heading the caret is in
    /// shown, or folded away.
    pub(super) fn expand_outline(&mut self, expand: bool) -> Response {
        let standings = self.document.outline();
        let caret = self.document.caret().paragraph;
        let Some(heading) = heading_of(&standings, caret) else {
            return self.report("There is no heading here");
        };
        if !has_content(&standings, heading) {
            return self.report("Nothing is under this heading");
        }
        if self.is_folded(heading) != expand {
            return Response::Ignored;
        }
        // The caret goes to the heading before what it was in is folded
        // away under it.
        if !expand && caret != heading {
            self.document.set_caret(TextPosition::new(heading, 0));
        }
        self.fold_heading(heading, !expand)
    }

    /// The paragraphs the level commands act on: every one the selection
    /// touches, or the one the caret is in.
    fn outline_targets(&self) -> Range<usize> {
        match self.document.selection() {
            Some((start, end)) => {
                // A selection that ends at the very start of a paragraph
                // does not take that paragraph with it.
                let last = if end.offset == 0 && end.paragraph > start.paragraph {
                    end.paragraph - 1
                } else {
                    end.paragraph
                };
                start.paragraph..last + 1
            }
            None => {
                let caret = self.document.caret().paragraph;
                caret..caret + 1
            }
        }
    }

    /// Word's Promote, Demote, Promote to Heading 1 and Demote to Body Text,
    /// and its box of levels: the level each paragraph is to be, by what it
    /// is now. `None` is body text.
    ///
    /// A heading folded goes with the headings folded under it, as Word's
    /// does: what cannot be seen is not left behind at its old level.
    pub(super) fn change_outline_level(&mut self, how: LevelChange) -> Response {
        let standings = self.document.outline();
        let mut changes: Vec<(usize, Option<u8>)> = Vec::new();
        for paragraph in self.outline_targets() {
            let Some(standing) = standings.get(paragraph).copied() else { continue };
            if standing == Standing::InTable {
                continue;
            }
            let now = standing.level();
            let above = heading_of(&standings, paragraph.saturating_sub(1))
                .filter(|_| paragraph > 0)
                .and_then(|at| level(&standings, at));
            let wanted = match how {
                LevelChange::Promote => match now {
                    Some(0) => continue,
                    Some(own) => Some(own - 1),
                    // Body text promoted is a heading at the level of the
                    // heading above it.
                    None => Some(above.unwrap_or(0)),
                },
                LevelChange::Demote => match now {
                    Some(own) if own < 8 => Some(own + 1),
                    _ => continue,
                },
                LevelChange::ToTop => Some(0),
                LevelChange::ToBody => None,
                LevelChange::To(level) => level,
            };
            if wanted == now {
                continue;
            }
            changes.push((paragraph, wanted));
            if let (Some(own), Some(new), true) = (now, wanted, self.is_folded(paragraph)) {
                if matches!(how, LevelChange::Promote | LevelChange::Demote) {
                    for under in paragraph + 1..section_end(&standings, paragraph) {
                        if let Some(theirs) = level(&standings, under) {
                            let moved = (i16::from(theirs) + i16::from(new) - i16::from(own))
                                .clamp(0, 8) as u8;
                            changes.push((under, Some(moved)));
                        }
                    }
                }
            }
        }
        if changes.is_empty() {
            return self.report("Nothing to change the level of");
        }

        // One step to take back, however many paragraphs it changed.
        self.document.begin_gesture();
        let mut changed = false;
        for (paragraph, wanted) in changes {
            let style = wanted.map(|level| {
                self.ensure_heading_style(level);
                format!("Heading{}", level + 1)
            });
            changed |= self.document.set_paragraph_style(paragraph, style.as_deref());
        }
        self.document.end_gesture();
        let note = match how {
            LevelChange::Promote => "Promoted",
            LevelChange::Demote => "Demoted",
            LevelChange::ToTop => "Promoted to Heading 1",
            LevelChange::ToBody => "Demoted to body text",
            LevelChange::To(_) => "Outline level changed",
        };
        self.edited(changed, note)
    }

    /// Puts a heading style into the document where it has none by that
    /// name: Word's built-in heading, based on the body style, at its level.
    fn ensure_heading_style(&mut self, level: u8) {
        let id = format!("Heading{}", level + 1);
        if self.document.styles().get(&id).is_some() {
            return;
        }
        let style = wp_docx::StyleDefinition {
            id,
            name: format!("heading {}", level + 1),
            based_on: Some("Normal".to_owned()),
            next: Some("Normal".to_owned()),
            paragraph: wp_docx::model::ParagraphProperties {
                keep_next: Some(true),
                keep_lines: Some(true),
                outline_level: Some(level),
                ..wp_docx::model::ParagraphProperties::default()
            },
            run: wp_docx::model::RunProperties {
                bold: Some(true),
                ..wp_docx::model::RunProperties::default()
            },
        };
        self.document.set_style(&style);
    }

    /// Word's Move Up and Move Down: the paragraph at the caret past the one
    /// before it or after it — a heading with everything under it, past the
    /// heading of its own level beside it.
    pub(super) fn move_outline(&mut self, up: bool) -> Response {
        let standings = self.document.outline();
        let caret = self.document.caret().paragraph;
        let Some((units, own)) = units(&standings, caret) else {
            return self.report("There is nothing here to move");
        };
        let places = boundaries(&units);
        let place = if up { own.checked_sub(1) } else { Some(own + 2) };
        let Some(before) = place.and_then(|at| places.get(at).copied()) else {
            return self.report(if up { "Nothing to move above" } else { "Nothing to move below" });
        };
        let unit = units[own].start;
        self.move_outline_piece(unit, before, false)
    }

    /// Moves a piece of the outline — a heading and everything under it, or
    /// a paragraph of body text — to come before a paragraph, as one step to
    /// take back; what is folded goes with it, and so does the selection,
    /// where there was one.
    fn move_outline_piece(&mut self, paragraph: usize, before: usize, reselect: bool) -> Response {
        let standings = self.document.outline();
        let Some((units, own)) = units(&standings, paragraph) else { return Response::Ignored };
        let unit = units[own].clone();
        if !self.document.move_paragraphs(unit.start, unit.end, before) {
            return self.report("It is already there");
        }
        for fold in &mut self.outlining.folded {
            fold.paragraph = moved_to(fold.paragraph, unit.start, unit.end, before);
        }
        if reselect {
            let start = moved_to(unit.start, unit.start, unit.end, before);
            self.select_paragraphs(start..start + unit.len());
        }
        self.edited(true, "Moved")
    }

    /// Drops open the levels the outline can be shown down to: Word's Show
    /// Level.
    pub(super) fn open_outline(&mut self) -> Response {
        if self.close_popup_if(Choice::OutlineLevel) {
            return Response::Redraw;
        }
        let Some((left, top, width)) = self.ribbon.command_rect(Command::OutlineShowLevel) else {
            return Response::Ignored;
        };
        let mut items: Vec<String> =
            (1..=9).map(|level| with("Level {0}", &[&level.to_string()])).collect();
        items.push(t("All Levels").to_owned());
        let current = usize::from(self.outline_depth.clamp(1, ALL_LEVELS)) - 1;
        self.popup = Some(Popup::new(
            Choice::OutlineLevel,
            items,
            Some(current),
            left,
            top,
            width.max(120.0),
        ));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Shows the outline down to the level that was chosen.
    ///
    /// Word's Show Level is the whole outline's, and whatever had been
    /// folded or opened by hand is shown to the level like everything else.
    pub(super) fn choose_outline_level(&mut self, index: usize) -> Response {
        self.popup = None;
        self.outline_depth = (index.min(usize::from(ALL_LEVELS) - 1) + 1) as u8;
        self.outlining.folded.clear();
        if self.view == View::Outline {
            self.relayout();
            self.snap_caret_into_view();
            self.needs_redraw = true;
        } else {
            self.set_view(View::Outline);
            self.snap_caret_into_view();
        }
        let named = self.show_level_name();
        self.report(&format!("{}: {named}", t("Show Level")))
    }

    /// What the Show Level box says: a level, or all of them.
    #[must_use]
    pub(super) fn show_level_name(&self) -> String {
        if self.outline_depth >= ALL_LEVELS {
            t("All Levels").to_owned()
        } else {
            with("Level {0}", &[&self.outline_depth.to_string()])
        }
    }

    /// What the level box says of the paragraph at the caret.
    #[must_use]
    pub(super) fn paragraph_level_name(&self) -> String {
        let caret = self.document.caret().paragraph;
        match level(&self.outlining.standings, caret) {
            Some(level) => with("Level {0}", &[&(level + 1).to_string()]),
            None => t("Body Text").to_owned(),
        }
    }

    /// Drops open the levels a paragraph can be: the nine headings, and body
    /// text.
    pub(super) fn open_paragraph_level(&mut self) -> Response {
        if self.close_popup_if(Choice::ParagraphLevel) {
            return Response::Redraw;
        }
        let Some((left, top, width)) = self.ribbon.command_rect(Command::OutlineLevelBox) else {
            return Response::Ignored;
        };
        let mut items: Vec<String> =
            (1..=9).map(|level| with("Level {0}", &[&level.to_string()])).collect();
        items.push(t("Body Text").to_owned());
        let caret = self.document.caret().paragraph;
        let current = self.document.outline().get(caret).and_then(|standing| standing.level());
        let current = current.map_or(9, usize::from);
        self.popup =
            Some(Popup::new(Choice::ParagraphLevel, items, Some(current), left, top, width));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Makes the paragraphs at the caret the level chosen from the box.
    pub(super) fn choose_paragraph_level(&mut self, index: usize) -> Response {
        self.popup = None;
        let wanted = (index < 9).then_some(index as u8);
        self.change_outline_level(LevelChange::To(wanted))
    }

    /// Word's Show Text Formatting and Show First Line Only.
    pub(super) fn toggle_outline_look(&mut self, first_line: bool) -> Response {
        let (on, name) = if first_line {
            self.outlining.first_line_only = !self.outlining.first_line_only;
            (self.outlining.first_line_only, "Show First Line Only")
        } else {
            self.outlining.show_formatting = !self.outlining.show_formatting;
            (self.outlining.show_formatting, "Show Text Formatting")
        };
        self.relayout();
        self.snap_caret_into_view();
        self.needs_redraw = true;
        self.report(&format!("{}: {}", t(name), if on { "on" } else { "off" }))
    }

    /// Folds the first heading that has anything under it: what the picture
    /// of a folded outline is taken of.
    pub(super) fn fold_first_heading(&mut self) {
        let standings = self.document.outline();
        if let Some(heading) = (0..standings.len()).find(|at| has_content(&standings, *at)) {
            self.set_folded(heading, true);
            self.relayout();
            self.snap_caret_into_view();
        }
    }

    /// Moves the caret to the nearest paragraph still on show.
    ///
    /// Only ever does anything in the outline, where a level can hide the
    /// paragraph the caret was in.
    pub(super) fn snap_caret_into_view(&mut self) {
        if self.caret_is_visible() {
            return;
        }
        let caret = self.document.caret();
        let mut best: Option<wp_docx::TextPosition> = None;
        for page in &self.pages {
            for line in &page.lines {
                let position = wp_docx::TextPosition::new(line.paragraph, line.start_offset);
                // The last paragraph at or before the caret, or failing that
                // the first one there is.
                if line.paragraph <= caret.paragraph || best.is_none() {
                    best = Some(position);
                }
            }
        }
        if let Some(position) = best {
            self.document.set_caret(position);
            self.reveal_caret();
        }
    }

    /// Whether the caret is in a paragraph that is being drawn.
    #[must_use]
    fn caret_is_visible(&self) -> bool {
        let caret = self.document.caret();
        self.pages
            .iter()
            .any(|page| page.lines.iter().any(|line| line.paragraph == caret.paragraph))
    }
}

/// How the level of a paragraph is to change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LevelChange {
    /// A level higher: Word's Promote.
    Promote,
    /// A level lower: Word's Demote.
    Demote,
    /// A first-level heading: Word's Promote to Heading 1.
    ToTop,
    /// Body text: Word's Demote to Body Text.
    ToBody,
    /// The level chosen from the box, or body text.
    To(Option<u8>),
}

#[cfg(test)]
mod tests {
    use wp_docx::model::{Block, Body, Paragraph, Table, TableCell, TableRow};
    use wp_docx::notes::Kind;
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_raster::Color;
    use wp_shell::accessibility::Role;
    use wp_shell::{App, Event, Key, Modifiers};

    use super::super::views::View;
    use super::super::Editor;
    use super::{mixed, MarkKind};
    use crate::chrome::ribbon::Tab;
    use crate::chrome::Command;

    const WIDTH: usize = 1400;
    const HEIGHT: usize = 900;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn heading(level: u8, text: &str) -> Block {
        Block::Paragraph(Paragraph::text(text).with_style(&format!("Heading{level}")))
    }

    fn body(text: &str) -> Block {
        Block::Paragraph(Paragraph::text(text))
    }

    /// A title, chapters with body text and a section under the first, a
    /// chapter with nothing under it, and a last chapter:
    ///
    /// 0 title · 1 Chapter 0 · 2 body · 3 Section 0 · 4 body · 5 Chapter 1 ·
    /// 6 body · 7 Empty chapter · 8 Chapter 2 · 9 body
    fn chapters() -> Vec<Block> {
        vec![
            Block::Paragraph(Paragraph::text("The title").with_style("Title")),
            heading(1, "Chapter 0"),
            body("Body under chapter 0"),
            heading(2, "Section 0"),
            body("Body under section 0"),
            heading(1, "Chapter 1"),
            body("Body under chapter 1"),
            heading(1, "Empty chapter"),
            heading(1, "Chapter 2"),
            body("Body under chapter 2"),
        ]
    }

    fn editor_of(blocks: Vec<Block>) -> Editor {
        let bytes = Document::create(&Body { blocks }).expect("a document").save().expect("saved");
        let document = Document::open(&bytes).expect("reopened");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: WIDTH as u32, height: HEIGHT as u32 });
        editor.draw(WIDTH, HEIGHT);
        editor
    }

    fn click(editor: &mut Editor, x: f32, y: f32) {
        let (x, y) = (x as i32, y as i32);
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseUp { x, y });
        editor.draw(WIDTH, HEIGHT);
    }

    fn key(editor: &mut Editor, key: Key) {
        editor.handle(Event::KeyDown { key, modifiers: Modifiers::default() });
        editor.draw(WIDTH, HEIGHT);
    }

    fn undo(editor: &mut Editor) {
        let control = Modifiers { control: true, ..Modifiers::default() };
        editor.handle(Event::KeyDown { key: Key::Letter('z'), modifiers: control });
        editor.draw(WIDTH, HEIGHT);
    }

    /// Opens a tab by pressing it on the strip.
    fn open_tab(editor: &mut Editor, tab: Tab) {
        let places = editor.ribbon_for_test().tab_places();
        let (_, left, width) =
            places.into_iter().find(|(found, ..)| *found == tab).expect("the tab is showing");
        let top = editor.ribbon_for_test().strip_top();
        click(editor, left + width / 2.0, top + 10.0);
    }

    /// Presses a button on a tab, as a person would.
    fn press(editor: &mut Editor, tab: Tab, command: Command) {
        open_tab(editor, tab);
        let places = editor.ribbon_for_test().command_places();
        let (_, left, top, width, height) = places
            .into_iter()
            .find(|(found, ..)| *found == command)
            .unwrap_or_else(|| panic!("{command:?} is not on {tab:?}"));
        click(editor, left + width / 2.0, top + height / 2.0);
    }

    /// Goes to the outline from the View tab's Outline button.
    fn outline(blocks: Vec<Block>) -> Editor {
        let mut editor = editor_of(blocks);
        press(&mut editor, Tab::View, Command::OutlineView);
        assert_eq!(editor.view, View::Outline);
        editor
    }

    /// The text of every paragraph with a line on the pages, in order.
    fn shown(editor: &Editor) -> Vec<String> {
        let mut out = Vec::new();
        for line in editor.pages.iter().flat_map(|page| &page.lines) {
            let text = editor.document.paragraph_text(line.paragraph).unwrap_or_default();
            if out.last() != Some(&text) {
                out.push(text);
            }
        }
        out
    }

    fn lines_of(editor: &Editor, paragraph: usize) -> usize {
        editor
            .pages
            .iter()
            .flat_map(|page| &page.lines)
            .filter(|l| l.paragraph == paragraph)
            .count()
    }

    fn texts(editor: &Editor) -> Vec<String> {
        (0..editor.document.paragraph_count())
            .map(|at| editor.document.paragraph_text(at).unwrap_or_default())
            .collect()
    }

    fn mark_of(editor: &Editor, paragraph: usize) -> super::Mark {
        editor
            .outline_marks()
            .into_iter()
            .find(|mark| mark.paragraph == paragraph && mark.movable)
            .unwrap_or_else(|| panic!("paragraph {paragraph} has no mark"))
    }

    fn rgb(colour: Color) -> (u8, u8, u8) {
        (colour.red, colour.green, colour.blue)
    }

    fn pixel(editor: &Editor, x: f32, y: f32) -> (u8, u8, u8) {
        rgb(editor.canvas().pixel(x.round() as usize, y.round() as usize))
    }

    #[test]
    fn the_outline_shows_no_notes_where_the_web_page_shows_them() {
        let mut editor = editor_of(chapters());
        editor.document.set_caret(TextPosition::new(2, 4));
        editor.document.add_note(Kind::Footnote, "A footnote.").expect("a note");
        editor.relayout();
        let below_the_text = |editor: &Editor| {
            let page = &editor.pages[0];
            let last = page.lines.iter().map(|line| line.baseline).fold(0.0f32, f32::max);
            page.glyphs.iter().any(|glyph| glyph.baseline > last + 0.5)
        };

        press(&mut editor, Tab::View, Command::WebLayout);
        assert_eq!(editor.view, View::Web);
        assert!(below_the_text(&editor), "the web page lost its note");

        press(&mut editor, Tab::View, Command::OutlineView);
        assert_eq!(editor.view, View::Outline);
        assert!(!below_the_text(&editor), "the outline shows the note after its text");
    }

    #[test]
    fn neither_the_outline_nor_the_web_page_scrolls_sideways() {
        let mut editor = editor_of(chapters());
        for (tab, command, view) in [
            (Tab::View, Command::WebLayout, View::Web),
            (Tab::View, Command::OutlineView, View::Outline),
        ] {
            press(&mut editor, tab, command);
            assert_eq!(editor.view, view);
            let page = &editor.pages[0];
            assert!(
                (page.width - editor.viewport_width()).abs() < 1.0,
                "{}: a sheet {} wide in a view {} wide",
                view.label(),
                page.width,
                editor.viewport_width()
            );
            assert_eq!(editor.page_origin(0).0, editor.content_left(), "{}", view.label());
            assert_eq!(editor.across_limit(), 0.0, "{} scrolls sideways", view.label());
            assert!(editor.across_bar.is_none(), "{} has a bar across the foot", view.label());
        }
    }

    #[test]
    fn the_rulers_go_with_the_outline_and_come_back_as_they_were() {
        let mut editor = editor_of(chapters());
        assert!(editor.show_rulers);
        let with_rulers = editor.content_left();

        press(&mut editor, Tab::View, Command::OutlineView);
        assert!(!editor.show_rulers, "the outline draws rulers");
        assert!(editor.content_left() < with_rulers, "the side ruler still takes room");
        let state = editor.toolbar_state();
        assert!(!crate::chrome::is_enabled(Command::ToggleRulers, &state), "Ruler is not grey");

        press(&mut editor, Tab::Outlining, Command::CloseOutlineView);
        assert_eq!(editor.view, View::Print);
        assert!(editor.show_rulers, "the rulers did not come back");
        assert_eq!(editor.content_left(), with_rulers);

        // Turned off, they stay off after the outline too.
        press(&mut editor, Tab::View, Command::ToggleRulers);
        assert!(!editor.show_rulers);
        press(&mut editor, Tab::View, Command::OutlineView);
        press(&mut editor, Tab::Outlining, Command::CloseOutlineView);
        assert!(!editor.show_rulers, "the rulers came back on their own");
    }

    #[test]
    fn the_outlining_tab_comes_with_the_outline_and_goes_with_it() {
        let mut editor = editor_of(chapters());
        let tabs = |editor: &Editor| -> Vec<Tab> {
            editor.ribbon_for_test().tab_places().into_iter().map(|(tab, ..)| tab).collect()
        };
        assert!(!tabs(&editor).contains(&Tab::Outlining), "the tab is there before the outline");

        press(&mut editor, Tab::View, Command::OutlineView);
        assert_eq!(editor.ribbon.tab, Tab::Outlining, "the outline did not open its tab");
        assert_eq!(&tabs(&editor)[..3], [Tab::File, Tab::Outlining, Tab::Home], "not after File");
        // Word's groups, and the way out.
        let commands: Vec<Command> = editor
            .ribbon_for_test()
            .command_places()
            .into_iter()
            .map(|(command, ..)| command)
            .collect();
        for wanted in [
            Command::OutlinePromoteToTop,
            Command::OutlinePromote,
            Command::OutlineLevelBox,
            Command::OutlineDemote,
            Command::OutlineDemoteToBody,
            Command::OutlineMoveUp,
            Command::OutlineMoveDown,
            Command::OutlineExpand,
            Command::OutlineCollapse,
            Command::OutlineShowLevel,
            Command::OutlineShowFormatting,
            Command::OutlineFirstLineOnly,
            Command::CloseOutlineView,
        ] {
            assert!(commands.contains(&wanted), "{wanted:?} is not on the tab");
        }

        press(&mut editor, Tab::Outlining, Command::CloseOutlineView);
        assert!(!tabs(&editor).contains(&Tab::Outlining), "the tab stayed after the outline");
        assert_eq!(editor.ribbon.tab, Tab::Home);
    }

    #[test]
    fn each_paragraph_has_the_mark_of_what_it_is() {
        let mut blocks = chapters();
        let cell = |text: &str| TableCell {
            blocks: vec![Block::Paragraph(Paragraph::text(text))],
            ..TableCell::default()
        };
        let row = |one: &str, two: &str| TableRow {
            cells: vec![cell(one), cell(two)],
            ..Default::default()
        };
        blocks.push(Block::Table(Box::new(Table {
            rows: vec![row("a", "b"), row("c", "d")],
            grid: vec![2000, 2000],
            ..Default::default()
        })));
        let editor = outline(blocks);
        let marks = editor.outline_marks();
        let kind = |paragraph: usize| mark_of(&editor, paragraph).kind;
        assert_eq!(kind(0), MarkKind::Body, "the title");
        assert_eq!(kind(1), MarkKind::Parent, "a chapter with text under it");
        assert_eq!(kind(2), MarkKind::Body);
        assert_eq!(kind(3), MarkKind::Parent, "a section with text under it");
        assert_eq!(kind(7), MarkKind::Childless, "a chapter with nothing under it");
        // A dot beside each row of the table, which is not taken hold of.
        let rows = marks.iter().filter(|mark| mark.paragraph >= 10).collect::<Vec<_>>();
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows.iter().all(|mark| mark.kind == MarkKind::Body && !mark.movable));

        // As drawn: a disc in the ink, with a plus or a minus in the paper's
        // colour cut out of a heading's, and a solid dot for body text.
        let ink = rgb(mixed(editor.theme.page_text, editor.theme.page, 0.25));
        let paper = rgb(editor.theme.page);
        let centre = |paragraph: usize| {
            let mark = mark_of(&editor, paragraph);
            (mark.x.round(), mark.y.round(), mark.radius)
        };
        let (x, y, _) = centre(2);
        assert_eq!(pixel(&editor, x, y), ink, "the dot of body text");
        let (x, y, radius) = centre(1);
        assert_eq!(pixel(&editor, x, y), paper, "the middle of the plus");
        assert_eq!(pixel(&editor, x, y - 2.0), paper, "the upright of the plus");
        assert!(radius > 5.0, "a heading's mark is too small to be a disc: {radius}");
        assert_eq!(pixel(&editor, x + 2.0, y + 2.0), ink, "the disc");
        let (x, y, _) = centre(7);
        assert_eq!(pixel(&editor, x, y), paper, "the minus");
        assert_eq!(pixel(&editor, x, y - 2.0), ink, "a minus has no upright");
        // And the headings sit to the left of their text, a level a step in.
        assert!(mark_of(&editor, 3).x > mark_of(&editor, 1).x + 10.0, "a section is not a step in");
        assert!(mark_of(&editor, 2).x > mark_of(&editor, 1).x + 10.0, "body text is not a step in");
    }

    #[test]
    fn two_clicks_on_a_mark_fold_the_heading_and_two_more_open_it() {
        let mut editor = outline(chapters());
        let double_click = |editor: &mut Editor| {
            let mark = mark_of(editor, 1);
            let (x, y) = (mark.x as i32, mark.y as i32);
            editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
            editor.handle(Event::MouseUp { x, y });
            editor.handle(Event::DoubleClick { x, y });
            editor.handle(Event::MouseUp { x, y });
            editor.draw(WIDTH, HEIGHT);
        };
        double_click(&mut editor);
        for hidden in 2..5 {
            assert_eq!(lines_of(&editor, hidden), 0, "paragraph {hidden} is still drawn");
        }
        assert!(lines_of(&editor, 5) > 0, "the next chapter went with it");
        assert_eq!(mark_of(&editor, 1).kind, MarkKind::Parent, "a folded heading still has a plus");
        // The grey line under a heading with its text folded away.
        let line =
            editor.drawn_lines().into_iter().find(|line| line.paragraph == 1).expect("a line");
        let scale = editor.pixels_per_inch() / super::super::POINTS_PER_INCH;
        let grey = rgb(mixed(editor.theme.page_text, editor.theme.page, 0.5));
        // Under the last figure, clear of the caret at the heading's start
        // and of any letter that reaches below the line.
        let (x, under) = (line.right - 3.0, (line.baseline + 2.0 * scale).floor());
        assert_eq!(pixel(&editor, x, under), grey, "no grey line under it");

        double_click(&mut editor);
        assert!(lines_of(&editor, 2) > 0 && lines_of(&editor, 4) > 0, "it did not open again");
        assert_ne!(pixel(&editor, x, under), grey, "the grey line stayed");
    }

    #[test]
    fn collapse_and_expand_fold_the_heading_the_caret_is_under() {
        let mut editor = outline(chapters());
        editor.document.set_caret(TextPosition::new(6, 3));
        press(&mut editor, Tab::Outlining, Command::OutlineCollapse);
        assert_eq!(lines_of(&editor, 6), 0, "the text under the chapter is still drawn");
        assert!(lines_of(&editor, 2) > 0, "another chapter was folded");
        assert_eq!(editor.document.caret().paragraph, 5, "the caret was left in hidden text");

        press(&mut editor, Tab::Outlining, Command::OutlineExpand);
        assert!(lines_of(&editor, 6) > 0, "it did not open again");
        // Nothing of the fold is written into the file.
        assert!(!editor.document.is_modified(), "folding changed the document");
    }

    #[test]
    fn a_fold_stays_with_its_heading_when_a_paragraph_goes_in_above_it() {
        let mut editor = outline(chapters());
        editor.document.set_caret(TextPosition::new(5, 0));
        press(&mut editor, Tab::Outlining, Command::OutlineCollapse);
        assert_eq!(lines_of(&editor, 6), 0);
        // Enter at the end of the title puts a paragraph in above it.
        let end = editor.document.paragraph_text(0).unwrap_or_default().len();
        editor.document.set_caret(TextPosition::new(0, end));
        key(&mut editor, Key::Enter);
        assert_eq!(editor.document.paragraph_text(6).as_deref(), Some("Chapter 1"));
        assert_eq!(lines_of(&editor, 7), 0, "the fold did not move with its heading");
        assert!(lines_of(&editor, 6) > 0 && lines_of(&editor, 5) > 0);
    }

    #[test]
    fn show_level_shows_the_headings_down_to_it_and_no_body_text() {
        let mut editor = outline(chapters());
        press(&mut editor, Tab::Outlining, Command::OutlineShowLevel);
        assert!(editor.popup.is_some(), "Show Level dropped nothing");
        // The list opens on what is in force, All Levels, the last: one down
        // from it goes round to Level 1.
        key(&mut editor, Key::Down);
        key(&mut editor, Key::Enter);
        assert_eq!(shown(&editor), ["Chapter 0", "Chapter 1", "Empty chapter", "Chapter 2"]);
        assert_eq!(editor.show_level_name(), "Level 1");
        // A heading with its text not showing has the grey line: marks are
        // still pluses where there is something under them.
        assert_eq!(mark_of(&editor, 1).kind, MarkKind::Parent);

        // Level 2 takes the section in, and still no body text.
        press(&mut editor, Tab::Outlining, Command::OutlineShowLevel);
        key(&mut editor, Key::Down);
        key(&mut editor, Key::Enter);
        assert_eq!(editor.show_level_name(), "Level 2");
        assert!(shown(&editor).contains(&"Section 0".to_owned()));
        assert!(
            !shown(&editor).iter().any(|text| text.starts_with("Body")),
            "{:?}",
            shown(&editor)
        );

        // All levels, two up and round from Level 2: everything.
        press(&mut editor, Tab::Outlining, Command::OutlineShowLevel);
        key(&mut editor, Key::Up);
        key(&mut editor, Key::Up);
        key(&mut editor, Key::Enter);
        assert_eq!(shown(&editor), texts(&editor));
        assert_eq!(editor.show_level_name(), "All Levels");
    }

    #[test]
    fn first_line_only_shows_a_line_of_each_paragraph_of_body_text() {
        let long = "Body text long enough to run on over several lines of the outline, \
                    so that showing only its first line leaves something out. "
            .repeat(6);
        let mut editor = outline(vec![heading(1, "Heading"), body(&long)]);
        assert!(lines_of(&editor, 1) > 2, "the paragraph is not long enough to prove anything");
        press(&mut editor, Tab::Outlining, Command::OutlineFirstLineOnly);
        assert_eq!(lines_of(&editor, 1), 1, "more than the first line is drawn");
        assert!(editor.toolbar_state().outline_first_line, "the tick is not on");
        press(&mut editor, Tab::Outlining, Command::OutlineFirstLineOnly);
        assert!(lines_of(&editor, 1) > 2, "the rest did not come back");
    }

    #[test]
    fn without_its_formatting_the_outline_is_one_font_at_one_size() {
        let mut editor = outline(chapters());
        let size_of = |editor: &Editor, paragraph: usize| {
            let page = &editor.pages[0];
            let line = page.lines.iter().find(|line| line.paragraph == paragraph).expect("line");
            page.glyphs[line.glyphs.start].size
        };
        assert!(size_of(&editor, 1) > size_of(&editor, 2), "a heading is set as body text");
        press(&mut editor, Tab::Outlining, Command::OutlineShowFormatting);
        assert!(!editor.toolbar_state().outline_formatting);
        assert_eq!(size_of(&editor, 1), size_of(&editor, 2), "the heading kept its size");
        assert_eq!(size_of(&editor, 0), size_of(&editor, 2), "the title kept its size");
    }

    #[test]
    fn promote_and_demote_change_a_level_and_each_is_one_step_back() {
        let mut editor = outline(chapters());
        let style = |editor: &Editor| editor.document.style_of(3);
        editor.document.set_caret(TextPosition::new(3, 0));
        let steps = editor.document.undo_depth();

        press(&mut editor, Tab::Outlining, Command::OutlinePromote);
        assert_eq!(style(&editor).as_deref(), Some("Heading1"));
        assert_eq!(editor.document.undo_depth(), steps + 1);
        undo(&mut editor);
        assert_eq!(style(&editor).as_deref(), Some("Heading2"));

        press(&mut editor, Tab::Outlining, Command::OutlineDemote);
        assert_eq!(style(&editor).as_deref(), Some("Heading3"));
        press(&mut editor, Tab::Outlining, Command::OutlineDemoteToBody);
        assert_eq!(style(&editor), None, "Demote to Body Text left a heading");
        assert_eq!(mark_of(&editor, 3).kind, MarkKind::Body);
        press(&mut editor, Tab::Outlining, Command::OutlinePromote);
        assert_eq!(
            style(&editor).as_deref(),
            Some("Heading1"),
            "body text promoted is not at the level of the heading above it"
        );
        press(&mut editor, Tab::Outlining, Command::OutlineDemote);
        press(&mut editor, Tab::Outlining, Command::OutlinePromoteToTop);
        assert_eq!(style(&editor).as_deref(), Some("Heading1"));

        // The box of levels, which opens on Level 1: three down is Level 4.
        press(&mut editor, Tab::Outlining, Command::OutlineLevelBox);
        for _ in 0..3 {
            key(&mut editor, Key::Down);
        }
        key(&mut editor, Key::Enter);
        assert_eq!(style(&editor).as_deref(), Some("Heading4"));
        assert_eq!(editor.toolbar_state().outline_level, "Level 4");

        // Every one of those was a step of its own.
        for _ in 0..6 {
            undo(&mut editor);
        }
        assert_eq!(style(&editor).as_deref(), Some("Heading2"), "not a step each");
    }

    #[test]
    fn move_up_and_down_take_a_heading_with_everything_under_it() {
        let mut editor = outline(chapters());
        let before = texts(&editor);
        editor.document.set_caret(TextPosition::new(1, 2));

        press(&mut editor, Tab::Outlining, Command::OutlineMoveDown);
        let texts_now = texts(&editor);
        assert_eq!(&texts_now[1..3], ["Chapter 1", "Body under chapter 1"]);
        assert_eq!(&texts_now[3..7], &before[1..5], "the chapter did not take its section along");
        assert_eq!(editor.document.caret().paragraph, 3, "the caret did not go with it");

        press(&mut editor, Tab::Outlining, Command::OutlineMoveUp);
        assert_eq!(texts(&editor), before, "moving it up again did not put it back");

        press(&mut editor, Tab::Outlining, Command::OutlineMoveDown);
        undo(&mut editor);
        assert_eq!(texts(&editor), before, "the move was not one step back");

        // The first of its level has nothing above it to move past.
        press(&mut editor, Tab::Outlining, Command::OutlineMoveUp);
        assert_eq!(texts(&editor), before);
    }

    #[test]
    fn a_click_on_a_mark_selects_the_heading_and_all_it_holds() {
        let mut editor = outline(chapters());
        let mark = mark_of(&editor, 1);
        click(&mut editor, mark.x, mark.y);
        let (start, end) = editor.document.selection().expect("a selection");
        assert_eq!(start, TextPosition::new(1, 0));
        assert_eq!(end.paragraph, 4, "the section under the chapter is not taken");
        assert_eq!(end.offset, "Body under section 0".len());
    }

    #[test]
    fn a_mark_dragged_moves_its_heading_among_the_headings_of_its_level() {
        let mut editor = outline(chapters());
        let before = texts(&editor);
        let mark = mark_of(&editor, 1);
        let last = editor.drawn_lines().iter().map(|line| line.bottom).fold(0.0f32, f32::max);
        let (x, y) = (mark.x as i32, mark.y as i32);
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseMove {
            x,
            y: y + 30,
            held: true,
            modifiers: Modifiers::default(),
        });
        let to = (last + 10.0) as i32;
        editor.handle(Event::MouseMove { x, y: to, held: true, modifiers: Modifiers::default() });
        editor.draw(WIDTH, HEIGHT);
        editor.handle(Event::MouseUp { x, y: to });
        editor.draw(WIDTH, HEIGHT);

        let after = texts(&editor);
        assert_eq!(&after[..1], &before[..1], "the title moved");
        assert_eq!(&after[1..6], &before[5..10], "the other chapters did not move up");
        assert_eq!(&after[6..], &before[1..5], "the chapter did not go last with its section");

        undo(&mut editor);
        assert_eq!(texts(&editor), before, "the drag was not one step back");
    }

    #[test]
    fn the_outlining_tab_and_all_on_it_are_named_to_a_screen_reader() {
        let editor = outline(chapters());
        let elements = editor.accessible_elements();
        let tab = elements
            .iter()
            .find(|element| element.role == Role::TabItem && element.name == "Outlining")
            .expect("the tab is not told of");
        assert!(tab.selected);
        assert_eq!(tab.access_key, "U");
        for (command, ..) in editor.ribbon_for_test().command_places() {
            let label = crate::chrome::tip::label_of(command).expect("a name");
            assert!(
                elements.iter().any(|element| element.name == label),
                "{command:?} is not told of as {label}"
            );
        }
        let ticked = elements.iter().find(|element| element.name == "Show Text Formatting");
        assert!(ticked.is_some_and(|element| element.role == Role::Toggle && element.selected));
    }
}

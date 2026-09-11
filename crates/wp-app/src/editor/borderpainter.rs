//! Word's Border Styles gallery and its Border Painter.
//!
//! # One job, two halves
//!
//! The gallery picks a line — a style, a width and a colour — and the painter
//! is the pen that puts that line on whichever edge it is dragged along. Either
//! without the other is useless: a gallery that only fills in a dialog is a
//! worse way of doing what the dialog already does, and a pen with no line to
//! carry has nothing to put down.
//!
//! # Why the pen is a mode and not a command
//!
//! Because it is used on several edges in a row. Word arms it with one press
//! and leaves it armed until it is pressed again or Escape is pressed, exactly
//! as the format painter behaves, and for the same reason: a person ruling the
//! lines of a table draws six of them and would otherwise press the button six
//! times.
//!
//! # Which edge the pen is on
//!
//! The nearest edge of the cell the pointer is over, and only when it is within
//! a few pixels of one. That needs the cells as rectangles, which the layout
//! now records — see [`wp_layout::PlacedCell`]. Nothing about the text says
//! where a cell's edges are.

use wp_docx::cells::CellEdge;
use wp_docx::model::Border;
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// How near the pointer has to be to an edge to be on it, in pixels.
///
/// Word's is about the same: near enough that aiming is not a game, far enough
/// that the two edges either side of a cell boundary are still told apart.
const REACH: f32 = 5.0;

/// The lines Word's Border Styles gallery offers.
///
/// Word's gallery is a grid of pictures under two headings — the theme's own
/// borders and the plain ones — and every one of them is a style, a width and a
/// colour. These are the plain ones, in Word's order: three weights of each of
/// the styles that read clearly at a table's scale.
const GALLERY: &[(&str, &str, u32)] = &[
    ("½ pt solid", "single", 4),
    ("¾ pt solid", "single", 6),
    ("1½ pt solid", "single", 12),
    ("2¼ pt solid", "single", 18),
    ("3 pt solid", "single", 24),
    ("½ pt dotted", "dotted", 4),
    ("1½ pt dotted", "dotted", 12),
    ("½ pt dashed", "dashed", 4),
    ("1½ pt dashed", "dashed", 12),
    ("½ pt dash dot", "dotDash", 4),
    ("1½ pt dash dot", "dotDash", 12),
    ("¾ pt double", "double", 6),
    ("1½ pt double", "double", 12),
    ("2¼ pt double", "double", 18),
    ("1½ pt triple", "triple", 12),
    ("1½ pt wave", "wave", 12),
];

/// The colours the gallery's lines come in, which are the ones the borders
/// dialog offers.
const INK: &[(&str, Option<&str>)] = &[
    ("Automatic", None),
    ("Black", Some("000000")),
    ("Grey", Some("808080")),
    ("Blue", Some("2B579A")),
    ("Red", Some("C00000")),
];

impl Editor {
    /// Drops open the gallery of lines.
    pub(super) fn open_border_styles(&mut self) -> Response {
        if self.close_popup_if(Choice::BorderStyle) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::BorderStyles) else {
            return Response::Ignored;
        };

        // Every line in every colour, which is how Word's gallery is arranged:
        // the same shapes over again in each of the theme's colours.
        let items: Vec<String> = INK
            .iter()
            .flat_map(|(ink, _)| GALLERY.iter().map(move |(label, _, _)| format!("{label}, {ink}")))
            .collect();
        let current = self.border_pen.as_ref().and_then(pen_row);
        self.popup = Some(Popup::new(Choice::BorderStyle, items, current, left, top, 260.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Takes a line off the gallery and puts the pen in hand.
    ///
    /// Picking a line arms the painter, as it does in Word: a person who chose
    /// a line chose it in order to draw with it.
    pub(super) fn choose_border_style(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(pen) = pen_at(index) else { return Response::Ignored };
        let name = pen_name(index).unwrap_or_default();
        self.border_pen = Some(pen);
        self.needs_redraw = true;
        self.report(&format!("{name} — now drag along the edges to rule them"))
    }

    /// Picks the pen up, or puts it down.
    pub(super) fn toggle_border_painter(&mut self) -> Response {
        if self.border_pen.take().is_some() {
            self.needs_redraw = true;
            return self.report("Border painter put down");
        }
        // Word's pen starts with whatever the gallery last showed; with nothing
        // chosen yet it is the line the gallery opens on.
        self.border_pen = pen_at(0);
        self.needs_redraw = true;
        self.report("Border painter picked up — now drag along the edges to rule them")
    }

    /// Whether the pen is in hand, which is what lights the button.
    #[must_use]
    pub(super) fn painting_borders(&self) -> bool {
        self.border_pen.is_some()
    }

    /// Rules the edge under the pointer, if the pen is in hand and there is
    /// one.
    ///
    /// Returns whether the press was the pen's. A press that lands nowhere near
    /// an edge is not: it puts the caret where it landed, the way a press
    /// always does, so that the pen does not swallow every click in the
    /// document while it is out.
    pub(super) fn paint_border_at(&mut self, x: i32, y: i32) -> bool {
        let Some(pen) = self.border_pen.clone() else { return false };
        let Some((at, edge)) = self.cell_edge_at(x, y) else { return false };

        let changed = self.document.set_cell_edge(at, edge, Some(&pen));
        if !changed {
            return false;
        }
        self.relayout();
        self.needs_redraw = true;
        self.edited(true, "Border painted");
        true
    }

    /// A point on one cell's top edge, by the cell's place on the page.
    ///
    /// Only for `--picture`, which has no pointer to press with and has to say
    /// where a press would have landed.
    pub(super) fn top_edge_of_cell(&self, cell: usize) -> Option<(i32, i32)> {
        let page = self.pages.first()?;
        let placed = page.cells.get(cell)?;
        let (origin_x, origin_y) = self.page_origin(0);
        let top = self.content_top() + origin_y - self.scroll_down();
        Some(((origin_x + placed.x + placed.width / 2.0) as i32, (top + placed.y) as i32))
    }

    /// The middle of one cell, and a point on its left edge, by the cell's
    /// place on the page.
    ///
    /// Only for `--picture`, as [`Self::top_edge_of_cell`] is.
    pub(super) fn middle_of_cell(&self, cell: usize) -> Option<(i32, i32)> {
        let (x, y, placed) = self.cell_corner(cell)?;
        Some(((x + placed.width / 2.0) as i32, (y + placed.height / 2.0) as i32))
    }

    pub(super) fn left_edge_of_cell(&self, cell: usize) -> Option<(i32, i32)> {
        let (x, y, placed) = self.cell_corner(cell)?;
        Some((x as i32, (y + placed.height / 2.0) as i32))
    }

    /// Where one cell's top left corner is on the screen.
    fn cell_corner(&self, cell: usize) -> Option<(f32, f32, wp_layout::PlacedCell)> {
        let placed = *self.pages.first()?.cells.get(cell)?;
        let (origin_x, origin_y) = self.page_origin(0);
        let top = self.content_top() + origin_y - self.scroll_down();
        Some((origin_x + placed.x, top + placed.y, placed))
    }

    /// Which edge of which cell the pointer is on.
    fn cell_edge_at(&self, x: i32, y: i32) -> Option<(wp_docx::TextPosition, CellEdge)> {
        let (x, y) = (x as f32, y as f32);
        let mut best: Option<(f32, wp_docx::TextPosition, CellEdge)> = None;

        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let page_x = x - origin_x;
            let page_y = y - self.content_top() - origin_y + self.scroll_down();

            for cell in &self.pages[index].cells {
                let Some((edge, away)) = cell.edge_near(page_x, page_y, REACH) else { continue };
                if best.as_ref().is_some_and(|(nearest, ..)| *nearest <= away) {
                    continue;
                }
                best = Some((away, cell.at, edge));
            }
        }
        best.map(|(_, at, edge)| (at, edge))
    }
}

// --- Draw Table and the Eraser ----------------------------------------------

/// Which of the three table pens is in hand.
///
/// All three are modes for the same reason and are told apart here rather than
/// by three flags, because exactly one of them can be in hand at a time: a pen
/// that both drew lines and rubbed them out would be a pen nobody could aim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TablePen {
    /// Word's Draw Table: a line drawn through a cell makes two cells of it.
    Draw,
    /// Word's Eraser: a line rubbed out makes one cell of two.
    Erase,
}

/// How long a drag has to be before it is a line rather than a click.
const STROKE: i32 = 8;

impl Editor {
    /// Picks up one of the two table pens, or puts it down.
    pub(super) fn toggle_table_pen(&mut self, pen: TablePen) -> Response {
        if self.table_pen == Some(pen) {
            self.table_pen = None;
            self.needs_redraw = true;
            return self.report(match pen {
                TablePen::Draw => "Table pen put down",
                TablePen::Erase => "Eraser put down",
            });
        }
        self.table_pen = Some(pen);
        self.needs_redraw = true;
        self.report(match pen {
            TablePen::Draw => "Table pen picked up — draw a line through a cell to split it",
            TablePen::Erase => "Eraser picked up — rub out a line to join the cells either side",
        })
    }

    /// Whether one of them is in hand.
    #[must_use]
    pub(super) fn holding_table_pen(&self, pen: TablePen) -> bool {
        self.table_pen == Some(pen)
    }

    /// Takes a press while a table pen is in hand.
    ///
    /// The eraser acts at once, because rubbing out a line needs no drag: the
    /// line is where the pointer is. The table pen waits for the drag to end,
    /// because which way the line was drawn is what decides whether the cell is
    /// split across or down.
    pub(super) fn table_pen_press(&mut self, x: i32, y: i32) -> bool {
        match self.table_pen {
            Some(TablePen::Erase) => self.erase_edge_at(x, y),
            Some(TablePen::Draw) => {
                // Only inside a cell: a line has to be drawn through something.
                if self.cell_at(x, y).is_none() {
                    return false;
                }
                self.drawing_from = Some((x, y));
                true
            }
            None => false,
        }
    }

    /// And the release that ends the line.
    pub(super) fn table_pen_release(&mut self, x: i32, y: i32) -> bool {
        let Some((from_x, from_y)) = self.drawing_from.take() else { return false };
        let (across, down) = ((x - from_x).abs(), (y - from_y).abs());
        if across < STROKE && down < STROKE {
            // A tap rather than a line. Word draws nothing for one either.
            return false;
        }
        let Some(at) = self.cell_at(from_x, from_y) else { return false };

        // A line drawn down the cell splits it across; one drawn across it
        // splits it down. The longer of the two directions is the one meant.
        let split = if down >= across {
            self.document.split_cell_across(at)
        } else {
            self.document.split_cell_down(at)
        };
        if !split {
            return false;
        }
        self.relayout();
        self.edited(true, if down >= across { "Cell split" } else { "Row split" });
        true
    }

    /// Rubs out the line under the pointer, joining the cells either side.
    fn erase_edge_at(&mut self, x: i32, y: i32) -> bool {
        let Some((at, edge)) = self.cell_edge_at(x, y) else { return false };
        // The cell on the other side of that edge, found by looking just past
        // it: the two cells either side of a line are what the line is between.
        let (beyond_x, beyond_y) = match edge {
            CellEdge::Top => (x, y - REACH as i32 * 2),
            CellEdge::Bottom => (x, y + REACH as i32 * 2),
            CellEdge::Start => (x - REACH as i32 * 2, y),
            CellEdge::End => (x + REACH as i32 * 2, y),
        };
        let Some(other) = self.cell_at(beyond_x, beyond_y) else {
            return self.report_bool("That line is the edge of the table");
        };
        if !self.document.erase_between(at, other) {
            return false;
        }
        self.relayout();
        self.edited(true, "Line rubbed out");
        true
    }

    /// A place in the document inside whichever cell a point is over.
    fn cell_at(&self, x: i32, y: i32) -> Option<wp_docx::TextPosition> {
        let (x, y) = (x as f32, y as f32);
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let page_x = x - origin_x;
            let page_y = y - self.content_top() - origin_y + self.scroll_down();

            for cell in &self.pages[index].cells {
                if page_x >= cell.x
                    && page_x < cell.x + cell.width
                    && page_y >= cell.y
                    && page_y < cell.y + cell.height
                {
                    return Some(cell.at);
                }
            }
        }
        None
    }

    /// Says something along the bottom and answers that the press was taken.
    fn report_bool(&mut self, message: &str) -> bool {
        self.report(message);
        true
    }
}

/// The line one row of the gallery stands for.
fn pen_at(index: usize) -> Option<Border> {
    let (colour, line) = (index / GALLERY.len(), index % GALLERY.len());
    let (_, style, size) = GALLERY.get(line)?;
    let (_, ink) = INK.get(colour)?;
    Some(Border::line(style, *size, *ink))
}

/// And what that row is called.
fn pen_name(index: usize) -> Option<String> {
    let (colour, line) = (index / GALLERY.len(), index % GALLERY.len());
    let (label, _, _) = GALLERY.get(line)?;
    let (ink, _) = INK.get(colour)?;
    Some(format!("{label}, {ink}"))
}

/// Which row of the gallery a line is, so the list opens on it.
fn pen_row(pen: &Border) -> Option<usize> {
    let colour = INK.iter().position(|(_, ink)| ink.map(str::to_owned) == pen.color)?;
    let line =
        GALLERY.iter().position(|(_, style, size)| *style == pen.style && *size == pen.size)?;
    Some(colour * GALLERY.len() + line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor showing a document with one table in it.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.insert_table(3, 3);
        editor.relayout();
        editor
    }

    /// A point on the screen a little way along one cell's top edge.
    fn on_top_edge(editor: &Editor, cell: usize) -> (i32, i32) {
        let page = editor.pages.first().expect("a page");
        let placed = page.cells.get(cell).copied().expect("a cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        ((origin_x + placed.x + placed.width / 2.0) as i32, (top + placed.y) as i32)
    }

    #[test]
    fn the_page_knows_where_the_cells_are() {
        let editor = editor();
        let page = editor.pages.first().expect("a page");
        assert_eq!(page.cells.len(), 9, "a three by three table is nine cells");
    }

    #[test]
    fn a_point_on_an_edge_finds_that_edge() {
        let editor = editor();
        let (x, y) = on_top_edge(&editor, 0);
        let (_, edge) = editor.cell_edge_at(x, y).expect("an edge");
        assert_eq!(edge, CellEdge::Top);
    }

    #[test]
    fn a_point_in_the_middle_of_a_cell_finds_no_edge() {
        // Or the pen would swallow every click in the table.
        let editor = editor();
        let page = editor.pages.first().expect("a page");
        let placed = page.cells[0];
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + placed.x + placed.width / 2.0) as i32;
        let y = (top + placed.y + placed.height / 2.0) as i32;
        assert!(editor.cell_edge_at(x, y).is_none());
    }

    #[test]
    fn the_pen_is_picked_up_and_put_down_by_the_same_button() {
        let mut editor = editor();
        assert!(!editor.painting_borders());
        editor.toggle_border_painter();
        assert!(editor.painting_borders());
        editor.toggle_border_painter();
        assert!(!editor.painting_borders());
    }

    #[test]
    fn choosing_a_line_puts_the_pen_in_hand() {
        let mut editor = editor();
        editor.choose_border_style(0);
        assert!(editor.painting_borders(), "picking a line did not arm the pen");
    }

    #[test]
    fn the_pen_rules_the_edge_it_is_pressed_on() {
        let mut editor = editor();
        editor.choose_border_style(pen_row(&Border::line("double", 12, None)).expect("a row"));
        let (x, y) = on_top_edge(&editor, 0);
        assert!(editor.paint_border_at(x, y), "the pen did not take");

        // The cell's own top border is what changed, and nothing else.
        let cell = first_cell(&editor.document);
        assert_eq!(cell.borders.top.as_ref().map(|line| line.style.as_str()), Some("double"));
        assert!(cell.borders.bottom.is_none(), "the pen ruled an edge nobody asked for");
    }

    #[test]
    fn a_press_away_from_any_edge_is_not_the_pens() {
        let mut editor = editor();
        editor.toggle_border_painter();
        let page = editor.pages.first().expect("a page");
        let placed = page.cells[0];
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + placed.x + placed.width / 2.0) as i32;
        let y = (top + placed.y + placed.height / 2.0) as i32;
        assert!(!editor.paint_border_at(x, y));
    }

    #[test]
    fn what_the_pen_ruled_survives_the_file() {
        let mut editor = editor();
        editor.choose_border_style(pen_row(&Border::line("dotted", 12, None)).expect("a row"));
        let (x, y) = on_top_edge(&editor, 0);
        editor.paint_border_at(x, y);

        let saved = editor.document.save().expect("saving");
        let reopened = Document::open(&saved).expect("reopening");
        let cell = first_cell(&reopened);
        assert_eq!(cell.borders.top.as_ref().map(|line| line.style.as_str()), Some("dotted"));
    }

    #[test]
    fn one_undo_takes_back_one_stroke() {
        let mut editor = editor();
        editor.choose_border_style(0);
        let (x, y) = on_top_edge(&editor, 0);
        editor.paint_border_at(x, y);
        assert!(first_cell(&editor.document).borders.top.is_some());

        editor.document.undo();
        assert!(first_cell(&editor.document).borders.top.is_none());
    }

    #[test]
    fn escape_puts_the_pen_down() {
        let mut editor = editor();
        editor.toggle_border_painter();
        editor.handle(Event::KeyDown {
            key: wp_shell::Key::Escape,
            modifiers: wp_shell::Modifiers::default(),
        });
        assert!(!editor.painting_borders(), "Escape left the pen in hand");
    }

    #[test]
    fn a_press_on_an_edge_while_the_pen_is_down_is_an_ordinary_press() {
        // The pen is a mode, and outside it a press is a press.
        let mut editor = editor();
        let (x, y) = on_top_edge(&editor, 0);
        assert!(!editor.paint_border_at(x, y));
        assert!(first_cell(&editor.document).borders.top.is_none());
    }

    /// How many cells the first row of the document's first table holds, and
    /// how many rows the table has.
    fn shape(document: &Document) -> (usize, usize) {
        for block in &document.body().blocks {
            if let Block::Table(table) = block {
                return (table.rows.len(), table.rows[0].cells.len());
            }
        }
        panic!("no table")
    }

    /// The middle of one cell, on the screen.
    fn middle_of(editor: &Editor, cell: usize) -> (i32, i32) {
        let page = editor.pages.first().expect("a page");
        let placed = page.cells.get(cell).copied().expect("a cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        (
            (origin_x + placed.x + placed.width / 2.0) as i32,
            (top + placed.y + placed.height / 2.0) as i32,
        )
    }

    #[test]
    fn a_line_drawn_down_a_cell_makes_two_cells_of_it() {
        let mut editor = editor();
        assert_eq!(shape(&editor.document), (3, 3));

        editor.toggle_table_pen(TablePen::Draw);
        let (x, y) = middle_of(&editor, 0);
        assert!(editor.table_pen_press(x, y));
        assert!(editor.table_pen_release(x, y + 40));

        let (rows, columns) = shape(&editor.document);
        assert_eq!(rows, 3, "a line down a cell made a row");
        assert_eq!(columns, 4, "the cell did not become two");
    }

    #[test]
    fn a_line_drawn_across_a_cell_makes_two_rows_of_it() {
        let mut editor = editor();
        editor.toggle_table_pen(TablePen::Draw);
        let (x, y) = middle_of(&editor, 0);
        assert!(editor.table_pen_press(x, y));
        assert!(editor.table_pen_release(x + 60, y));

        let (rows, columns) = shape(&editor.document);
        assert_eq!(rows, 4, "the row did not become two");
        assert_eq!(columns, 3, "a line across a cell made a column");
    }

    #[test]
    fn a_tap_with_the_pen_draws_nothing() {
        // A line has to be a line. Word draws nothing for a tap either.
        let mut editor = editor();
        editor.toggle_table_pen(TablePen::Draw);
        let (x, y) = middle_of(&editor, 0);
        editor.table_pen_press(x, y);
        assert!(!editor.table_pen_release(x + 2, y + 2));
        assert_eq!(shape(&editor.document), (3, 3));
    }

    #[test]
    fn the_eraser_joins_the_cells_either_side_of_a_line() {
        let mut editor = editor();
        editor.toggle_table_pen(TablePen::Erase);
        // The line between the first two cells of the first row.
        let page = editor.pages.first().expect("a page");
        let placed = page.cells[1];
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + placed.x) as i32;
        let y = (top + placed.y + placed.height / 2.0) as i32;

        assert!(editor.table_pen_press(x, y));
        let (rows, columns) = shape(&editor.document);
        assert_eq!(rows, 3);
        assert_eq!(columns, 2, "the two cells were not joined");
    }

    #[test]
    fn the_eraser_leaves_the_edge_of_the_table_alone() {
        // There is nothing on the other side of it to join to.
        let mut editor = editor();
        editor.toggle_table_pen(TablePen::Erase);
        let page = editor.pages.first().expect("a page");
        let placed = page.cells[0];
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + placed.x) as i32;
        let y = (top + placed.y + placed.height / 2.0) as i32;

        editor.table_pen_press(x, y);
        assert_eq!(shape(&editor.document), (3, 3));
    }

    #[test]
    fn one_pen_at_a_time() {
        let mut editor = editor();
        editor.toggle_table_pen(TablePen::Draw);
        assert!(editor.holding_table_pen(TablePen::Draw));
        editor.toggle_table_pen(TablePen::Erase);
        assert!(editor.holding_table_pen(TablePen::Erase));
        assert!(!editor.holding_table_pen(TablePen::Draw), "both pens are in hand");
    }

    #[test]
    fn escape_puts_a_table_pen_down_too() {
        let mut editor = editor();
        editor.toggle_table_pen(TablePen::Draw);
        editor.handle(Event::KeyDown {
            key: wp_shell::Key::Escape,
            modifiers: wp_shell::Modifiers::default(),
        });
        assert!(!editor.holding_table_pen(TablePen::Draw));
    }

    #[test]
    fn what_the_pen_drew_survives_the_file() {
        let mut editor = editor();
        editor.toggle_table_pen(TablePen::Draw);
        let (x, y) = middle_of(&editor, 0);
        editor.table_pen_press(x, y);
        editor.table_pen_release(x, y + 40);

        let saved = editor.document.save().expect("saving");
        let reopened = Document::open(&saved).expect("reopening");
        assert_eq!(shape(&reopened), (3, 4));
    }

    #[test]
    fn every_line_on_the_gallery_can_be_found_again() {
        // The list opens on the line the pen is holding, and a line that could
        // not be found would open it on the first row instead.
        for index in 0..GALLERY.len() * INK.len() {
            let pen = pen_at(index).expect("a line");
            assert_eq!(pen_row(&pen), Some(index));
        }
    }

    /// The first cell of the document's first table.
    fn first_cell(document: &Document) -> wp_docx::model::TableCell {
        for block in &document.body().blocks {
            if let Block::Table(table) = block {
                return table.rows[0].cells[0].clone();
            }
        }
        panic!("no table")
    }
}

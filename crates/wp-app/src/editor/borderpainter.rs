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

//! Dragging the lines of a table: column widths and row heights.
//!
//! # Why the page is asked and not the file
//!
//! Because what a person drags is the line they can see, and where that line is
//! drawn is not what the file says. A table fitted to its contents states no
//! widths at all; one at the window's width states the widths it had when it
//! was written. The laid-out cells are the only record of where the lines
//! actually are, so the drag starts from them — and ends by writing what it
//! worked out back into the file, which is what makes it stay.

use wp_docx::TextPosition;
use wp_shell::Response;

/// Twentieths of a point in a point, which is what the file measures in.
const TWIPS_PER_POINT: f32 = 20.0;

/// How near a line the pointer counts as being on it, in pixels.
const REACH: f32 = 4.0;

/// The narrowest a column may be dragged, and the shortest a row: a tenth of an
/// inch, which is about what Word allows and enough to still be pressable.
const LEAST: i32 = 144;

/// A line of a table being dragged.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct EdgeDrag {
    /// Which column's right-hand line, or which row's bottom line.
    pub at: usize,
    /// Whether it is a column's line — dragged across — or a row's.
    pub across: bool,
    /// Somewhere in the table, so the change can be aimed at it.
    pub inside: TextPosition,
    /// Where the pointer went down.
    pub from: i32,
    /// What the columns measured when it went down, in twentieths of a point.
    /// For a row's line, the one number is the row's height.
    pub were: Vec<i32>,
}

impl super::Editor {
    /// The line of a table the pointer is on, if it is on one.
    ///
    /// Answered for the pointer resting as well as for the pointer pressed,
    /// because the two must agree: a line that shows the resize pointer and
    /// then does not resize is worse than one that never offered.
    pub(super) fn table_edge_at(&self, x: i32, y: i32) -> Option<EdgeDrag> {
        let (page, px, py) = self.page_point(x, y)?;
        let scale = self.pixels_per_inch() / 72.0;
        if scale <= 0.0 {
            return None;
        }

        // The rows of cells on the page, gathered by the height they sit at.
        //
        // Gathered, and not filtered to the ones near the pointer: a pointer
        // four pixels above the line between two rows is near the cells of both
        // of them, and a row made of six cells out of two rows would be
        // measured as a table of six columns.
        let mut rows: Vec<(f32, f32, Vec<&wp_layout::PlacedCell>)> = Vec::new();
        for cell in &self.pages[page].cells {
            match rows.iter_mut().find(|(y, _, _)| (*y - cell.y).abs() < 0.5) {
                Some((_, height, cells)) => {
                    *height = height.max(cell.height);
                    cells.push(cell);
                }
                None => rows.push((cell.y, cell.height, vec![cell])),
            }
        }

        // The row the pointer is in, or the one it is nearest to — which at the
        // line between two rows is the one above, whose line it is.
        let away = |(y, height, _): &(f32, f32, Vec<&wp_layout::PlacedCell>)| {
            if py < *y {
                *y - py
            } else if py > *y + *height {
                py - *y - *height
            } else {
                0.0
            }
        };
        let (_, _, mut row) =
            rows.into_iter().min_by(|one, other| away(one).total_cmp(&away(other)))?;
        if row.is_empty() {
            return None;
        }
        row.sort_by(|one, other| one.x.total_cmp(&other.x));

        // A column's line: the right-hand side of one of the cells of the row.
        // The row has to hold the pointer between its top and bottom for that,
        // or a press just above the table would drag the line of the row below.
        if let Some((index, cell)) = row
            .iter()
            .enumerate()
            .find(|(_, cell)| (px - (cell.x + cell.width)).abs() <= REACH && py >= cell.y)
        {
            if py > cell.y + cell.height {
                return None;
            }
            // A row with cells merged across it says nothing about where the
            // columns are: its cells and the table's grid do not answer to one
            // another, and writing one as the other would be a table of the
            // wrong shape. Such a row is not a row to drag from.
            if self.document.table_grid_at(cell.at.paragraph).len() != row.len() {
                return None;
            }
            let widths = row
                .iter()
                .map(|cell| ((cell.width / scale) * TWIPS_PER_POINT).round() as i32)
                .collect();
            return Some(EdgeDrag {
                at: index,
                across: true,
                inside: cell.at,
                from: x,
                were: widths,
            });
        }

        // A row's line: the bottom of the cells beside the pointer. The cell
        // has to hold it across, so that the desk beside the table is not a
        // line to drag.
        let cell = row.iter().find(|cell| {
            px >= cell.x
                && px <= cell.x + cell.width
                && (py - (cell.y + cell.height)).abs() <= REACH
        })?;
        let height = ((cell.height / scale) * TWIPS_PER_POINT).round() as i32;
        Some(EdgeDrag { at: 0, across: false, inside: cell.at, from: y, were: vec![height] })
    }

    /// Takes a press on the line of a table as the start of a drag.
    ///
    /// Returns whether it did. The caret is left where it was: dragging a line
    /// changes the shape of the table and not where the next thing typed goes,
    /// which is how Word behaves and what lets a column be widened without
    /// losing one's place.
    pub(super) fn press_table_edge(&mut self, x: i32, y: i32) -> bool {
        let Some(drag) = self.table_edge_at(x, y) else { return false };
        // One drag is one change to take back, however many times the pointer
        // moves while it runs.
        self.document.begin_gesture();
        self.edge_drag = Some(drag);
        self.dragging = true;
        true
    }

    /// Carries the drag on to where the pointer is now.
    pub(super) fn drag_table_edge(&mut self, x: i32, y: i32) -> Response {
        let Some(drag) = self.edge_drag.clone() else { return Response::Ignored };
        let scale = self.pixels_per_inch() / 72.0;
        if scale <= 0.0 {
            return Response::Ignored;
        }
        let moved = if drag.across { x - drag.from } else { y - drag.from };
        let moved = ((moved as f32 / scale) * TWIPS_PER_POINT).round() as i32;

        let changed = if drag.across {
            self.widen_column(&drag, moved)
        } else {
            self.heighten_row(&drag, moved)
        };
        if !changed {
            return Response::Ignored;
        }

        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Ends the drag, if one is running.
    pub(super) fn release_table_edge(&mut self) -> bool {
        let Some(drag) = self.edge_drag.take() else { return false };
        self.document.end_gesture();
        self.status =
            if drag.across { "Column width changed" } else { "Row height changed" }.to_owned();
        true
    }

    /// Moves one line between two columns, or the last line of the table.
    ///
    /// Word's rule: an inner line takes the room it gains from the column on
    /// the other side of it, so the table stays the width it was. The line at
    /// the end has no column beyond it, so dragging that one makes the table
    /// wider or narrower.
    fn widen_column(&mut self, drag: &EdgeDrag, moved: i32) -> bool {
        let mut widths = drag.were.clone();
        let Some(width) = widths.get(drag.at).copied() else { return false };

        match widths.get(drag.at + 1).copied() {
            Some(next) => {
                // Neither of the two may be squeezed away, so the movement is
                // cut back to what both can give.
                let moved = moved.clamp(LEAST - width, next - LEAST);
                widths[drag.at] = width + moved;
                widths[drag.at + 1] = next - moved;
            }
            None => widths[drag.at] = (width + moved).max(LEAST),
        }

        self.change_table_at(drag.inside, |document| {
            // A dragged column is a column somebody has settled, so the table
            // stops working its widths out for itself — which is what Word does
            // the moment a line is dragged, and without which the drag would be
            // undone by the next layout.
            document.set_table_fit(wp_docx::model::TableFit::Fixed);
            document.set_table_grid(&widths)
        })
    }

    /// Makes the row the line belongs to taller or shorter.
    fn heighten_row(&mut self, drag: &EdgeDrag, moved: i32) -> bool {
        let Some(height) = drag.were.first().copied() else { return false };
        let wanted = (height + moved).max(LEAST);
        // At least, and not exactly: a row dragged taller in Word still grows
        // when more is typed into it than fits, and "exactly" is what the row
        // properties are for.
        self.change_table_at(drag.inside, |document| {
            document.set_table_row_height(Some(wanted), false)
        })
    }

    /// Runs a change on the table a place in the document is inside, and puts
    /// the caret back.
    ///
    /// Everything the document can be told about a table it is told about the
    /// table at the caret, and the caret is not in the table being dragged —
    /// it is wherever the person left it. So it goes there and comes back,
    /// which changes nothing else: widths are not paragraphs, and no index
    /// moves.
    fn change_table_at(
        &mut self,
        inside: TextPosition,
        change: impl FnOnce(&mut wp_docx::Document) -> bool,
    ) -> bool {
        let caret = self.document.caret();
        let selected = self.document.selections();
        self.document.set_caret(inside);
        let changed = change(&mut self.document);
        self.document.set_caret(caret);
        if !selected.is_empty() {
            self.document.set_selections(&selected);
        }
        changed
    }

    /// Which page a point in the window is on, and where on it.
    ///
    /// The nearest page, so that a drag that has run off the top or the bottom
    /// of one still means something.
    pub(super) fn page_point(&self, x: i32, y: i32) -> Option<(usize, f32, f32)> {
        let mut best: Option<(f32, usize, f32, f32)> = None;
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let page_x = x as f32 - origin_x;
            let page_y = y as f32 - self.content_top() - origin_y + self.scroll_down();
            let height = self.pages[index].height;

            let away = if page_y < 0.0 {
                -page_y
            } else if page_y > height {
                page_y - height
            } else {
                0.0
            };
            if best.is_some_and(|(nearest, ..)| nearest <= away) {
                continue;
            }
            best = Some((away, index, page_x, page_y));
        }
        best.map(|(_, index, page_x, page_y)| (index, page_x, page_y))
    }
}

#[cfg(test)]
mod tests {
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Cursor, Event, Modifiers};

    use super::super::Editor;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor with a three by three table, the caret in its first cell.
    fn with_table() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.set_caret(TextPosition::new(0, 6));
        assert!(editor.document.insert_table(3, 3), "the table went nowhere");
        editor.relayout();
        let (first, _) = editor.document.cell_paragraphs(0, 0).expect("a cell");
        editor.document.set_caret(TextPosition::new(first, 0));
        editor
    }

    /// The widths of the columns of the table, as they are drawn.
    fn widths(editor: &Editor) -> Vec<f32> {
        let mut row: Vec<&wp_layout::PlacedCell> = editor.pages[0]
            .cells
            .iter()
            .filter(|cell| (cell.y - editor.pages[0].cells[0].y).abs() < 0.5)
            .collect();
        row.sort_by(|one, other| one.x.total_cmp(&other.x));
        row.iter().map(|cell| cell.width).collect()
    }

    /// A point on the line between the first and the second column.
    fn first_line(editor: &Editor) -> (i32, i32) {
        let (_, cell) = editor.placed_cell(0, 0).expect("a cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        ((origin_x + cell.x + cell.width) as i32, (top + cell.y + cell.height / 2.0) as i32)
    }

    #[test]
    fn the_pointer_over_a_column_line_says_it_can_be_dragged() {
        let mut editor = with_table();
        let (x, y) = first_line(&editor);
        assert_eq!(editor.cursor(x, y), Cursor::ResizeHorizontal, "the pointer says nothing");
        assert_eq!(editor.cursor(x - 20, y), Cursor::Text, "away from the line it is text");
    }

    #[test]
    fn dragging_a_column_line_widens_one_column_and_narrows_the_next() {
        let mut editor = with_table();
        let before = widths(&editor);
        let (x, y) = first_line(&editor);

        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseMove {
            x: x + 40,
            y,
            held: true,
            modifiers: Modifiers::default(),
        });
        editor.handle(Event::MouseUp { x: x + 40, y });

        let after = widths(&editor);
        assert!(
            after[0] > before[0] + 20.0,
            "the first column did not widen: {before:?} {after:?}"
        );
        assert!(after[1] < before[1] - 20.0, "the second column did not narrow");
        let width = |row: &[f32]| row.iter().sum::<f32>();
        assert!(
            (width(&after) - width(&before)).abs() < 2.0,
            "the table changed width: {before:?} {after:?}"
        );
    }

    #[test]
    fn dragging_a_column_line_does_not_move_the_caret() {
        let mut editor = with_table();
        let caret = editor.document.caret();
        let (x, y) = first_line(&editor);

        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseMove {
            x: x + 30,
            y,
            held: true,
            modifiers: Modifiers::default(),
        });
        editor.handle(Event::MouseUp { x: x + 30, y });

        assert_eq!(editor.document.caret(), caret, "the drag moved the caret");
    }

    #[test]
    fn a_column_cannot_be_dragged_away_altogether() {
        let mut editor = with_table();
        let (x, y) = first_line(&editor);

        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseMove {
            x: x - 4000,
            y,
            held: true,
            modifiers: Modifiers::default(),
        });
        editor.handle(Event::MouseUp { x: x - 4000, y });

        let after = widths(&editor);
        assert!(after[0] > 1.0, "the first column was squeezed to nothing: {after:?}");
        assert!(after[1] > 1.0, "the second column was squeezed to nothing: {after:?}");
    }

    #[test]
    fn the_last_line_of_the_table_makes_it_wider() {
        let mut editor = with_table();
        let before: f32 = widths(&editor).iter().sum();
        let (_, cell) = editor.placed_cell(0, 2).expect("a cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + cell.x + cell.width) as i32;
        let y = (top + cell.y + cell.height / 2.0) as i32;

        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseMove {
            x: x - 60,
            y,
            held: true,
            modifiers: Modifiers::default(),
        });
        editor.handle(Event::MouseUp { x: x - 60, y });

        let after: f32 = widths(&editor).iter().sum();
        assert!(after < before - 30.0, "the table did not narrow: {before} to {after}");
    }

    #[test]
    fn dragging_the_bottom_of_a_row_makes_it_taller() {
        let mut editor = with_table();
        let (_, cell) = editor.placed_cell(0, 1).expect("a cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + cell.x + cell.width / 2.0) as i32;
        let y = (top + cell.y + cell.height) as i32;
        let before = cell.height;

        assert_eq!(editor.cursor(x, y), Cursor::ResizeVertical, "the pointer says nothing");
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseMove {
            x,
            y: y + 30,
            held: true,
            modifiers: Modifiers::default(),
        });
        editor.handle(Event::MouseUp { x, y: y + 30 });

        let (_, after) = editor.placed_cell(0, 1).expect("a cell");
        assert!(after.height > before + 15.0, "the row did not grow: {before} to {}", after.height);
    }

    #[test]
    fn a_row_whose_cells_are_merged_is_not_a_row_to_drag_from() {
        // Its cells and the table's grid no longer answer to one another, so
        // what the cells measure is not what the grid should say. The rows that
        // were not merged still drag.
        let mut editor = with_table();
        let (first, _) = editor.document.cell_paragraphs(0, 0).expect("a cell");
        let (second, _) = editor.document.cell_paragraphs(0, 1).expect("a cell");
        editor.document.set_caret(TextPosition::new(first, 0));
        editor.document.extend_selection_to(TextPosition::new(second, 0));
        assert!(editor.document.merge_cells(), "nothing was merged");
        editor.document.set_caret(TextPosition::new(first, 0));
        editor.relayout();

        let (_, merged) = editor.placed_cell(0, 0).expect("the merged cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + merged.x + merged.width) as i32;
        let y = (top + merged.y + merged.height / 2.0) as i32;
        assert_eq!(editor.cursor(x, y), Cursor::Text, "the merged row offered a drag");

        let (_, below) = editor.placed_cell(1, 0).expect("a cell of the second row");
        let x = (origin_x + below.x + below.width) as i32;
        let y = (top + below.y + below.height / 2.0) as i32;
        assert_eq!(
            editor.cursor(x, y),
            Cursor::ResizeHorizontal,
            "the row below should still drag"
        );
    }

    #[test]
    fn one_drag_is_one_thing_to_take_back() {
        let mut editor = with_table();
        let before = widths(&editor);
        let (x, y) = first_line(&editor);

        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        for step in 1..=4 {
            editor.handle(Event::MouseMove {
                x: x + step * 10,
                y,
                held: true,
                modifiers: Modifiers::default(),
            });
        }
        editor.handle(Event::MouseUp { x: x + 40, y });
        assert!(editor.document.undo(), "there was nothing to undo");
        editor.relayout();

        let after = widths(&editor);
        assert!(
            (after[0] - before[0]).abs() < 2.0,
            "one undo did not put the columns back: {before:?} {after:?}"
        );
    }
}

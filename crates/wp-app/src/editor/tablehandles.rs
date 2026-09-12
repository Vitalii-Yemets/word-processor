//! The two handles a table carries: the one that moves it and the one that
//! resizes it.
//!
//! # What they are
//!
//! Word draws a small square just outside the top-left corner of a table
//! whenever the pointer is over it. Dragging that square moves the whole table
//! to another place in the document; pressing it selects the whole table. At
//! the bottom-right corner it draws another, and dragging that one makes the
//! whole table wider or narrower, every column keeping its share.
//!
//! They are the only way Word offers to move a table, which is why a table
//! that cannot be dragged is a table that cannot be put where it belongs.
//!
//! # Why the move is a move and not a cut and a paste
//!
//! Because a table carries more than a program knows. Cutting it out and
//! writing it back would rebuild it from what was understood, and what was not
//! understood — a colleague's tracked change, a content control, a property
//! from a later version of the format — would be left behind. So the element
//! itself is taken out of one place in the tree and put into another, exactly
//! as [`crate::editor::grouping`] moves a drawing between groups.

use wp_shell::Response;

use super::Editor;

/// How wide the handles are, in pixels.
pub(super) const HANDLE: f32 = 11.0;

/// How far outside the corner of the table they sit.
const CLEAR: f32 = 5.0;

/// Which handle, and what it is about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum TableHandle {
    /// The square at the top left: drag to move the table, press to take it.
    Move,
    /// The square at the bottom right: drag to make the table wider.
    Resize,
}

/// A table handle being dragged.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct HandleDrag {
    pub handle: TableHandle,
    /// Somewhere inside the table, so a command can be aimed at it.
    pub inside: wp_docx::TextPosition,
    /// Where the pointer went down.
    pub from: (i32, i32),
    /// Whether the pointer has moved since: a press that does not move is a
    /// press, and takes the table rather than moving it.
    pub moved: bool,
    /// What the columns were when it began, in twentieths of a point.
    pub widths: Vec<i32>,
    /// How wide the table was drawn then, in pixels.
    pub drawn: f32,
}

impl Editor {
    /// The table the handles belong to: the one under the pointer, or the one
    /// the caret is in.
    ///
    /// Word shows them for the table the pointer is over. The caret's table
    /// counts too, because a table being typed in is a table being worked on
    /// and its handles are then what the next gesture is likely to want.
    fn handled_table(&self) -> Option<(usize, f32, f32, f32, f32, wp_docx::TextPosition)> {
        let pointer = self.page_point(self.pointer_x as i32, self.pointer_y as i32);
        let under = pointer.and_then(|(page, px, py)| {
            self.pages[page]
                .cells
                .iter()
                .find(|cell| {
                    px >= cell.x - HANDLE
                        && px <= cell.x + cell.width + HANDLE
                        && py >= cell.y - HANDLE
                        && py <= cell.y + cell.height + HANDLE
                })
                .map(|cell| (page, cell.at))
        });

        // Which table that cell is in, as the first and last paragraph of it,
        // so that every cell of the same table can be gathered up.
        let (page, inside) = match under {
            Some(found) => found,
            None => {
                let place = self.document.table_here()?;
                let _ = place;
                let (page, _) = self.placed_cell(0, 0)?;
                (page, self.document.caret())
            }
        };
        let (first, last) = self.document.table_paragraphs_at(inside.paragraph)?;

        let cells: Vec<&wp_layout::PlacedCell> = self.pages[page]
            .cells
            .iter()
            .filter(|cell| (first..=last).contains(&cell.at.paragraph))
            .collect();
        let left = cells.iter().map(|cell| cell.x).fold(f32::MAX, f32::min);
        let top = cells.iter().map(|cell| cell.y).fold(f32::MAX, f32::min);
        let right = cells.iter().map(|cell| cell.x + cell.width).fold(f32::MIN, f32::max);
        let bottom = cells.iter().map(|cell| cell.y + cell.height).fold(f32::MIN, f32::max);
        if right <= left || bottom <= top {
            return None;
        }
        Some((page, left, top, right, bottom, inside))
    }

    /// Where the two handles are drawn, in window coordinates.
    ///
    /// Empty when there is no table to show them for, which is most of the
    /// time: a handle drawn where there is no table is a handle that lies.
    pub(super) fn table_handles(&self) -> Vec<(TableHandle, f32, f32)> {
        let Some((page, left, top, right, bottom, _)) = self.handled_table() else {
            return Vec::new();
        };
        let (origin_x, origin_y) = self.page_origin(page);
        let up = self.content_top() + origin_y - self.scroll_down();
        vec![
            (TableHandle::Move, origin_x + left - CLEAR - HANDLE, up + top - CLEAR - HANDLE),
            (TableHandle::Resize, origin_x + right + CLEAR, up + bottom + CLEAR),
        ]
    }

    /// Which handle a point is on, if it is on one.
    pub(super) fn table_handle_at(&self, x: i32, y: i32) -> Option<TableHandle> {
        self.table_handles().into_iter().find_map(|(handle, hx, hy)| {
            let inside = x as f32 >= hx
                && x as f32 <= hx + HANDLE
                && y as f32 >= hy
                && y as f32 <= hy + HANDLE;
            inside.then_some(handle)
        })
    }

    /// Takes a press on one of them.
    pub(super) fn press_table_handle(&mut self, x: i32, y: i32) -> bool {
        let Some(handle) = self.table_handle_at(x, y) else { return false };
        let Some((page, left, _, right, _, inside)) = self.handled_table() else { return false };
        let _ = page;

        let caret = self.document.caret();
        self.document.set_caret(inside);
        let widths = self.document.table_grid_at(inside.paragraph);
        self.document.set_caret(caret);

        self.document.begin_gesture();
        self.handle_drag = Some(HandleDrag {
            handle,
            inside,
            from: (x, y),
            moved: false,
            widths,
            drawn: right - left,
        });
        self.dragging = true;
        true
    }

    /// Carries the drag on.
    pub(super) fn drag_table_handle(&mut self, x: i32, y: i32) -> Response {
        let Some(mut drag) = self.handle_drag.clone() else { return Response::Ignored };
        if (x - drag.from.0).abs() > 2 || (y - drag.from.1).abs() > 2 {
            drag.moved = true;
            self.handle_drag = Some(drag.clone());
        }
        if !drag.moved {
            return Response::Ignored;
        }

        match drag.handle {
            // Moving shows where the table would land rather than moving it on
            // every pointer movement: a table that jumped about under the hand
            // would take the text it lands in with it, over and over.
            TableHandle::Move => {
                self.needs_redraw = true;
                Response::Redraw
            }
            TableHandle::Resize => self.resize_table(&drag, x),
        }
    }

    /// Ends it: moves the table, or takes it, or leaves it resized.
    pub(super) fn release_table_handle(&mut self, x: i32, y: i32) -> bool {
        let Some(drag) = self.handle_drag.take() else { return false };
        self.document.end_gesture();

        match (drag.handle, drag.moved) {
            // A press that did not move takes the whole table, which is what
            // Word's move handle does when it is clicked rather than dragged.
            (TableHandle::Move, false) | (TableHandle::Resize, false) => {
                self.document.set_caret(drag.inside);
                let Some(place) = self.document.table_here() else { return true };
                self.select_cells(
                    (0, place.rows.saturating_sub(1)),
                    (0, place.columns.saturating_sub(1)),
                );
                self.status = "Table selected".to_owned();
                self.needs_redraw = true;
            }
            (TableHandle::Move, true) => {
                let landed = self.drop_table(&drag, x, y);
                self.status = if landed {
                    "Table moved".to_owned()
                } else {
                    "A table cannot be moved into itself".to_owned()
                };
                self.relayout();
                self.needs_redraw = true;
            }
            (TableHandle::Resize, true) => {
                self.status = "Table resized".to_owned();
                self.needs_redraw = true;
            }
        }
        true
    }

    /// Puts the table where the pointer let go of it.
    fn drop_table(&mut self, drag: &HandleDrag, x: i32, y: i32) -> bool {
        let Some(at) = self.position_at(x, y) else { return false };
        self.document.set_caret(drag.inside);
        let moved = self.document.move_table_before(at.paragraph);
        if moved {
            // The caret follows the table, as it follows text that is moved.
            if let Some((first, _)) = self.document.table_paragraphs() {
                self.document.set_caret(wp_docx::TextPosition::new(first, 0));
            }
        }
        moved
    }

    /// Makes the whole table wider or narrower, every column keeping its share.
    fn resize_table(&mut self, drag: &HandleDrag, x: i32) -> Response {
        if drag.widths.is_empty() || drag.drawn <= 1.0 {
            return Response::Ignored;
        }
        let wanted = (x - drag.from.0) as f32 + drag.drawn;
        let ratio = (wanted / drag.drawn).clamp(0.1, 10.0);

        let widths: Vec<i32> = drag
            .widths
            .iter()
            .map(|width| ((*width as f32 * ratio).round() as i32).max(144))
            .collect();

        let caret = self.document.caret();
        let selected = self.document.selections();
        self.document.set_caret(drag.inside);
        // A table somebody has sized is a table whose columns are settled, so
        // it stops working its own width out — the same reason dragging a
        // column's line fixes them. See [`super::tableedges`].
        self.document.set_table_fit(wp_docx::model::TableFit::Fixed);
        let changed = self.document.set_table_grid(&widths);
        self.document.set_caret(caret);
        if !selected.is_empty() {
            self.document.set_selections(&selected);
        }
        if !changed {
            return Response::Ignored;
        }

        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }
}

#[cfg(test)]
mod tests {
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Modifiers};

    use super::{Editor, TableHandle};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor whose document is three paragraphs with a table after the
    /// first, and the caret in the table's first cell.
    fn editor() -> Editor {
        let mut body = Body::default();
        for text in ["First", "Second", "Third"] {
            body.blocks.push(Block::Paragraph(Paragraph::text(text)));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.set_caret(TextPosition::new(0, 5));
        assert!(editor.document.insert_table(2, 2), "the table went nowhere");
        editor.relayout();
        let (first, _) = editor.document.cell_paragraphs(0, 0).expect("a cell");
        editor.document.set_caret(TextPosition::new(first, 0));
        editor
    }

    /// Where a handle is, in window coordinates.
    fn handle_at(editor: &Editor, which: TableHandle) -> (i32, i32) {
        let (_, x, y) = editor
            .table_handles()
            .into_iter()
            .find(|(handle, _, _)| *handle == which)
            .expect("the handle is on the page");
        ((x + super::HANDLE / 2.0) as i32, (y + super::HANDLE / 2.0) as i32)
    }

    /// The words of the document, paragraph by paragraph.
    fn paragraphs(editor: &Editor) -> Vec<String> {
        (0..editor.document.paragraph_count())
            .map(|index| editor.document.paragraph_text(index).unwrap_or_default())
            .collect()
    }

    #[test]
    fn a_table_has_a_handle_at_each_of_two_corners() {
        let editor = editor();
        let handles = editor.table_handles();
        assert_eq!(handles.len(), 2, "a table should carry two handles");
        let move_handle = handles[0];
        let resize = handles[1];
        assert!(move_handle.1 < resize.1, "the move handle should be to the left");
        assert!(move_handle.2 < resize.2, "and above");
    }

    #[test]
    fn a_document_with_no_table_has_no_handles() {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Only text")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        assert!(editor.table_handles().is_empty(), "handles were drawn for no table");
    }

    #[test]
    fn pressing_the_move_handle_takes_the_whole_table() {
        let mut editor = editor();
        let (x, y) = handle_at(&editor, TableHandle::Move);
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseUp { x, y });

        let taken = editor.document.selected_cells().expect("a block of cells");
        assert_eq!(taken.rows, (0, 1), "the rows were not all taken");
        assert_eq!(taken.columns, (0, 1), "the columns were not all taken");
    }

    #[test]
    fn dragging_the_move_handle_moves_the_table_down_the_document() {
        let mut editor = editor();
        let before = paragraphs(&editor);
        let at = |text: &str| {
            paragraphs(&editor).iter().position(|found| found == text).expect("the paragraph")
        };
        assert!(at("First") < at("Second"), "the document is not in the order it was written");
        assert!(
            editor.document.table_at(at("First") + 1).is_some(),
            "the table should sit right after the first paragraph: {before:?}"
        );

        // On to the last paragraph, which is where the table should land.
        let last = at("Third");
        let (x, y) = handle_at(&editor, TableHandle::Move);
        let (target_x, target_y) = paragraph_middle(&editor, last);
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseMove {
            x: target_x,
            y: target_y,
            held: true,
            modifiers: Modifiers::default(),
        });
        editor.handle(Event::MouseUp { x: target_x, y: target_y });
        editor.relayout();

        let after = paragraphs(&editor);
        assert_eq!(after.len(), before.len(), "a paragraph was lost or gained");
        let now = |text: &str| after.iter().position(|found| found == text).expect("the paragraph");
        assert!(
            now("Second") < now("Third"),
            "the paragraphs came out in the wrong order: {after:?}"
        );
        // The table is no longer between the first two paragraphs, and is
        // above the one it was dropped on.
        assert!(
            editor.document.table_at(now("First") + 1).is_none(),
            "the table did not leave its old place: {after:?}"
        );
        assert!(
            editor.document.table_at(now("Third").saturating_sub(1)).is_some(),
            "the table is not where it was dropped: {after:?}"
        );
    }

    /// The middle of a paragraph on the page, in window coordinates.
    fn paragraph_middle(editor: &Editor, paragraph: usize) -> (i32, i32) {
        let line = editor.pages[0]
            .lines
            .iter()
            .find(|line| line.paragraph == paragraph)
            .expect("the paragraph is on the page");
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        ((origin_x + line.left + 2.0) as i32, (top + line.baseline) as i32)
    }

    #[test]
    fn a_table_cannot_be_dropped_inside_itself() {
        let mut editor = editor();
        let before = paragraphs(&editor);
        let (x, y) = handle_at(&editor, TableHandle::Move);
        let (inside, _) = editor.document.cell_paragraphs(1, 1).expect("a cell");
        let (target_x, target_y) = paragraph_middle(&editor, inside);

        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseMove {
            x: target_x,
            y: target_y,
            held: true,
            modifiers: Modifiers::default(),
        });
        editor.handle(Event::MouseUp { x: target_x, y: target_y });
        editor.relayout();

        assert_eq!(paragraphs(&editor), before, "the document changed");
    }

    #[test]
    fn dragging_the_resize_handle_makes_the_whole_table_narrower() {
        let mut editor = editor();
        let width = |editor: &Editor| {
            let cells = &editor.pages[0].cells;
            let left = cells.iter().map(|cell| cell.x).fold(f32::MAX, f32::min);
            let right = cells.iter().map(|cell| cell.x + cell.width).fold(f32::MIN, f32::max);
            right - left
        };
        let before = width(&editor);

        let (x, y) = handle_at(&editor, TableHandle::Resize);
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseMove {
            x: x - 200,
            y,
            held: true,
            modifiers: Modifiers::default(),
        });
        editor.handle(Event::MouseUp { x: x - 200, y });
        editor.relayout();

        let after = width(&editor);
        assert!(after < before - 150.0, "the table did not narrow: {before} then {after}");

        // Every column kept its share of it, which is what resizing a table
        // means: the columns are not evened out by being scaled.
        let mut row: Vec<&wp_layout::PlacedCell> = editor.pages[0].cells.iter().collect();
        row.sort_by(|one, other| one.x.total_cmp(&other.x));
        let first = row[0].width;
        assert!(first > 10.0, "a column was squeezed away: {first}");
    }

    #[test]
    fn one_resize_is_one_thing_to_take_back() {
        let mut editor = editor();
        let (x, y) = handle_at(&editor, TableHandle::Resize);
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        for step in 1..=4 {
            editor.handle(Event::MouseMove {
                x: x - step * 40,
                y,
                held: true,
                modifiers: Modifiers::default(),
            });
        }
        editor.handle(Event::MouseUp { x: x - 160, y });
        editor.relayout();
        let narrowed: f32 = editor.pages[0].cells.iter().map(|cell| cell.width).fold(0.0, f32::max);

        assert!(editor.document.undo(), "there was nothing to undo");
        editor.relayout();
        let back: f32 = editor.pages[0].cells.iter().map(|cell| cell.width).fold(0.0, f32::max);
        assert!(back > narrowed + 20.0, "one undo did not put the table back");
    }
}

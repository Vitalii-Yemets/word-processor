//! Working in a table: the keys that move the caret about one.
//!
//! # Why this is here rather than in the document
//!
//! Because a table is the one place in a word processor where the model and
//! what a person does with it are furthest apart. The model is rows of cells of
//! paragraphs; a person sees a grid, presses Tab, and expects the caret in the
//! next cell — and at the last one, expects a new row. None of that is in the
//! model, and all of it is what using a table *is*.
//!
//! So this is that layer, and it is tested by doing rather than by reading the
//! file afterwards: a table that saves correctly and cannot be typed in is a
//! table nobody can use.

use wp_shell::Response;

use super::Editor;

impl Editor {
    /// Moves the caret a cell on, or with Shift a cell back.
    ///
    /// Word's Tab. It moves by cell and not by character, it takes what is in
    /// the cell it lands on, and at the last cell of the last row it adds a row
    /// rather than doing nothing — which is how a table is filled in without
    /// ever reaching for the ribbon.
    pub(super) fn step_cell(&mut self, forwards: bool) -> Response {
        let Some(place) = self.document.table_here() else { return Response::Ignored };

        match self.document.cell_beside(forwards) {
            Some((row, column)) => {
                if !self.take_cell(row, column) {
                    return Response::Ignored;
                }
                self.moved()
            }
            // Before the first cell there is nowhere to go back to.
            None if !forwards => Response::Ignored,
            // Past the last one there is: Word adds a row and carries on into
            // it, which is how a table grows while it is being filled in.
            None => {
                if !self.document.insert_table_row(true) {
                    return Response::Ignored;
                }
                self.take_cell(place.row + 1, 0);
                self.edited(true, "Row added")
            }
        }
    }

    /// The middle of a cell in window coordinates, for the scenes that prove
    /// what a press or a drag on a table does.
    pub(super) fn cell_middle_for_scene(&self, row: usize, column: usize) -> (i32, i32) {
        let Some((page, cell)) = self.placed_cell(row, column) else { return (0, 0) };
        let (origin_x, origin_y) = self.page_origin(page);
        let top = self.content_top() + origin_y - self.scroll_down();
        ((origin_x + cell.x + cell.width / 2.0) as i32, (top + cell.y + cell.height / 2.0) as i32)
    }

    /// The cells of the selection, when what is selected is a block of them.
    ///
    /// Empty when the selection is a stretch of text — inside one cell or
    /// anywhere outside a table — because then it is the text that is shown as
    /// selected and not the cells.
    pub(super) fn selected_cell_rects(&self) -> Vec<(usize, wp_layout::PlacedCell)> {
        let Some(range) = self.document.selected_cells() else { return Vec::new() };
        if range.rows.0 == range.rows.1 && range.columns.0 == range.columns.1 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for row in range.rows.0..=range.rows.1 {
            for column in range.columns.0..=range.columns.1 {
                if let Some(found) = self.placed_cell(row, column) {
                    out.push(found);
                }
            }
        }
        out
    }

    /// Selects a rectangle of the cells of the table at the caret.
    ///
    /// Every cell of it whole, which is what a cell selection is: one stretch
    /// of text per cell rather than one stretch running from the first to the
    /// last, because the cells between two cells in reading order are not the
    /// cells between them on the page. The rectangle itself is recorded as well
    /// — an empty cell holds no text to select, and a new table is all empty
    /// cells.
    pub(super) fn select_cells(&mut self, rows: (usize, usize), columns: (usize, usize)) -> bool {
        let Some(place) = self.document.table_here() else { return false };

        let mut stretches = Vec::new();
        for row in rows.0..=rows.1 {
            for column in columns.0..=columns.1 {
                if let Some(stretch) = self.document.cell_text_range(row, column) {
                    stretches.push(stretch);
                }
            }
        }
        if stretches.is_empty() {
            return false;
        }

        self.document.set_selections(&stretches);
        self.document.select_cell_block(wp_docx::cells::CellRange {
            table: place.table,
            rows,
            columns,
        });
        self.needs_redraw = true;
        true
    }

    /// Selects one whole row of the table at the caret, as Word's selection bar
    /// does when the pointer is beside a table.
    pub(super) fn select_table_row(&mut self, row: usize) -> bool {
        let Some(place) = self.document.table_here() else { return false };
        if row >= place.rows {
            return false;
        }
        let columns = self.document.cells_in_row(row);
        columns > 0 && self.select_cells((row, row), (0, columns - 1))
    }

    /// And one whole column, which Word takes when the pointer is just above
    /// the table and pressed.
    pub(super) fn select_table_column(&mut self, column: usize) -> bool {
        let Some(place) = self.document.table_here() else { return false };
        if column >= place.columns {
            return false;
        }
        self.select_cells((0, place.rows - 1), (column, column))
    }

    /// The column the pointer is just above, if it is above one.
    ///
    /// Word's column selector: a narrow band along the top edge of a table,
    /// where a press takes the whole column under it. Above the table only —
    /// the same band inside it is the line between two rows, which is dragged
    /// rather than pressed.
    ///
    /// Answers the column and a place inside its top cell, because everything
    /// the document can be told about a table it is told about the table at the
    /// caret, and the caret may be nowhere near this one.
    pub(super) fn column_bar_at(&self, x: i32, y: i32) -> Option<(usize, wp_docx::TextPosition)> {
        /// How far above the table the band reaches, in pixels.
        const BAND: f32 = 6.0;

        let (page, px, py) = self.page_point(x, y)?;
        let cell = self.pages[page].cells.iter().find(|cell| {
            px >= cell.x && px <= cell.x + cell.width && py < cell.y && py >= cell.y - BAND
        })?;
        let place = self.document.table_at(cell.at.paragraph)?;
        (place.row == 0).then_some((place.column, cell.at))
    }

    /// Takes a press in that band as taking the column.
    pub(super) fn press_column_bar(&mut self, x: i32, y: i32) -> bool {
        let Some((column, inside)) = self.column_bar_at(x, y) else { return false };
        self.document.set_caret(inside);
        if !self.select_table_column(column) {
            return false;
        }
        self.cell_anchor = Some((0, column));
        self.dragging = true;
        true
    }

    /// Carries a drag on across the cells of a table.
    ///
    /// Returns whether it took the drag, which it does only once the pointer
    /// has left the cell the drag began in. Inside one cell a drag selects
    /// letters like any other, which is what Word does and what makes a table
    /// of text editable at all.
    ///
    /// # Why cells and not a stretch of text
    ///
    /// Because the cells of a table are not written in the order they are read.
    /// A stretch from the second cell of the first row to the second cell of
    /// the third row runs through the third cell of the first row, all of the
    /// second, and the first of the third — six cells to take two columns of
    /// three. Word takes the rectangle, and so does this: every cell of it
    /// whole, and nothing outside it.
    pub(super) fn extend_cell_drag(&mut self, x: i32, y: i32) -> bool {
        let Some(anchor) = self.cell_anchor else { return false };
        let Some(at) = self.position_at(x, y) else { return false };
        let Some(place) = self.document.table_at(at.paragraph) else { return false };

        // The same cell as the one the drag began in: an ordinary drag through
        // the text of that cell.
        if (place.row, place.column) == anchor {
            return false;
        }

        let rows = (place.row.min(anchor.0), place.row.max(anchor.0));
        let columns = (place.column.min(anchor.1), place.column.max(anchor.1));
        self.select_cells(rows, columns)
    }

    /// The up and down arrows in a table, which move by row.
    ///
    /// Returns whether it took the move: it does not when the cell has another
    /// line that way, because a cell of several lines is moved through line by
    /// line like any other text.
    ///
    /// # Why the ordinary move cannot do this
    ///
    /// Because it moves to the next line of the document, and the next line of
    /// the document after the first cell of a row is the second cell of the
    /// same row — which is beside the caret, not below it. The arrow then
    /// appeared to do nothing at all, three times, before finally reaching the
    /// row below. Word moves down a row, into the cell under the one the caret
    /// is in, and that is what this does.
    pub(super) fn step_table_row(&mut self, downwards: bool, extend: bool) -> bool {
        let Some(place) = self.document.table_here() else { return false };
        let Some((first, last)) = self.document.cell_paragraphs(place.row, place.column) else {
            return false;
        };

        // Another line of the same cell that way is an ordinary move.
        let lines = self.lines();
        let Some(current) = self.caret_line() else { return false };
        let Some(index) = lines.iter().position(|entry| *entry == current) else { return false };
        let beside = if downwards { index + 1 } else { index.wrapping_sub(1) };
        if let Some(&(page, line)) = lines.get(beside) {
            let paragraph = self.pages[page].lines[line].paragraph;
            if (first..=last).contains(&paragraph) {
                return false;
            }
        }

        let wanted_x = self.caret_across();

        // Off the end of the table, the caret leaves it — for the paragraph
        // after the table going down, and the one before it going up, which is
        // where Word puts it.
        let row = if downwards { place.row + 1 } else { place.row.wrapping_sub(1) };
        if row >= place.rows {
            let Some((above, below)) = self.document.table_paragraphs() else { return false };
            let outside = if downwards { below + 1 } else { above.wrapping_sub(1) };
            if outside >= self.document.paragraph_count() {
                return true;
            }
            return self.caret_onto_paragraph(outside, !downwards, wanted_x, extend);
        }

        // Into the cell below or above. The wanted place across the page is
        // held inside that cell, so that a row whose columns do not line up
        // with this one — a merge, most often — still takes the caret rather
        // than handing it to the cell next door.
        let Some((_, cell)) = self.placed_cell(row, place.column) else { return false };
        let across = wanted_x.clamp(cell.x + 1.0, cell.x + cell.width - 1.0);
        let Some((below, above)) = self.document.cell_paragraphs(row, place.column) else {
            return false;
        };
        let paragraph = if downwards { below } else { above };
        self.caret_onto_paragraph(paragraph, !downwards, across, extend)
    }

    /// Where the caret is across the page, which a vertical move keeps.
    fn caret_across(&self) -> f32 {
        self.caret_line()
            .and_then(|(page, _)| self.pages.get(page))
            .and_then(|page| page.caret_at(self.document.caret(), 1.0))
            .map_or(0.0, |(x, _, _, _)| x)
    }

    /// Puts the caret on the first or the last line of a paragraph, as near
    /// across the page as it is now.
    fn caret_onto_paragraph(
        &mut self,
        paragraph: usize,
        last_line: bool,
        across: f32,
        extend: bool,
    ) -> bool {
        let mut found = None;
        for (page_index, line_index) in self.lines() {
            if self.pages[page_index].lines[line_index].paragraph != paragraph {
                continue;
            }
            found = Some((page_index, line_index));
            if !last_line {
                break;
            }
        }

        let Some((page_index, line_index)) = found else { return false };
        let page = &self.pages[page_index];
        let baseline = page.lines[line_index].baseline;
        let Some(position) = page.position_at(across, baseline) else { return false };
        self.document.move_caret(position, extend);
        true
    }

    /// One cell of the table at the caret, as the page has it.
    ///
    /// The page keeps the cell rectangles without saying which row and column
    /// each one is, so the cell is found by what is inside it: the paragraphs
    /// of that row and column.
    pub(super) fn placed_cell(
        &self,
        row: usize,
        column: usize,
    ) -> Option<(usize, wp_layout::PlacedCell)> {
        let (first, last) = self.document.cell_paragraphs(row, column)?;
        self.pages.iter().enumerate().find_map(|(index, page)| {
            page.cells
                .iter()
                .find(|cell| (first..=last).contains(&cell.at.paragraph))
                .map(|cell| (index, *cell))
        })
    }

    /// Puts the caret in a cell of the table it is already in, over whatever
    /// that cell holds.
    ///
    /// Word takes the cell's text rather than putting the caret at the front of
    /// it, so that filling a table in is a matter of typing and pressing Tab:
    /// what is typed next replaces what was there. An empty cell has nothing to
    /// take, and gets the caret.
    fn take_cell(&mut self, row: usize, column: usize) -> bool {
        let Some((start, end)) = self.document.cell_text_range(row, column) else { return false };
        if start == end {
            self.document.set_caret(start);
        } else {
            self.document.set_selections(&[(start, end)]);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    use super::Editor;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor holding a document of one paragraph, ready for a table.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// An editor with a three by three table in it, the caret in its first
    /// cell.
    fn with_table() -> Editor {
        let mut editor = editor();
        editor.document.set_caret(TextPosition::new(0, 6));
        assert!(editor.document.insert_table(3, 3), "the table went nowhere");
        editor.relayout();
        let (first, _) = editor.document.cell_paragraphs(0, 0).expect("a first cell");
        editor.document.set_caret(TextPosition::new(first, 0));
        editor
    }

    fn typed(editor: &mut Editor, text: &str) {
        for character in text.chars() {
            editor.handle(Event::Char(character));
        }
    }

    fn key(editor: &mut Editor, key: Key, shift: bool) {
        editor
            .handle(Event::KeyDown { key, modifiers: Modifiers { shift, ..Modifiers::default() } });
    }

    /// What every cell of the table at the caret holds, row by row.
    fn grid(editor: &Editor) -> Vec<Vec<String>> {
        editor.document.table_rows_text()
    }

    #[test]
    fn a_table_can_be_typed_into() {
        let mut editor = with_table();
        typed(&mut editor, "One");
        assert_eq!(grid(&editor)[0][0], "One", "what was typed is not in the first cell");
    }

    #[test]
    fn tab_moves_to_the_next_cell_and_typing_fills_it() {
        let mut editor = with_table();
        typed(&mut editor, "One");
        key(&mut editor, Key::Tab, false);
        typed(&mut editor, "Two");
        key(&mut editor, Key::Tab, false);
        typed(&mut editor, "Three");

        let grid = grid(&editor);
        assert_eq!(grid[0][0], "One");
        assert_eq!(grid[0][1], "Two", "Tab did not reach the second cell");
        assert_eq!(grid[0][2], "Three", "nor the third");
    }

    #[test]
    fn tab_types_nothing() {
        // It is a move and not a character: a tab in the text of a cell would
        // be a table nobody could keep tidy.
        let mut editor = with_table();
        key(&mut editor, Key::Tab, false);
        assert!(!grid(&editor).iter().flatten().any(|cell| cell.contains('\t')), "Tab typed a tab");
    }

    #[test]
    fn tab_at_the_end_of_a_row_goes_on_to_the_next() {
        let mut editor = with_table();
        for _ in 0..3 {
            key(&mut editor, Key::Tab, false);
        }
        typed(&mut editor, "Second row");
        assert_eq!(grid(&editor)[1][0], "Second row", "Tab did not wrap to the next row");
    }

    #[test]
    fn tab_at_the_last_cell_adds_a_row() {
        let mut editor = with_table();
        for _ in 0..8 {
            key(&mut editor, Key::Tab, false);
        }
        assert_eq!(grid(&editor).len(), 3, "there should still be three rows");
        key(&mut editor, Key::Tab, false);
        assert_eq!(grid(&editor).len(), 4, "Tab at the last cell should add a row");
        typed(&mut editor, "New");
        assert_eq!(grid(&editor)[3][0], "New", "and the caret should be in it");
    }

    #[test]
    fn shift_and_tab_goes_back_a_cell() {
        let mut editor = with_table();
        key(&mut editor, Key::Tab, false);
        key(&mut editor, Key::Tab, false);
        key(&mut editor, Key::Tab, true);
        typed(&mut editor, "Back");
        assert_eq!(grid(&editor)[0][1], "Back", "Shift and Tab did not go back");
    }

    #[test]
    fn shift_and_tab_at_the_start_of_a_row_goes_to_the_end_of_the_one_above() {
        let mut editor = with_table();
        for _ in 0..3 {
            key(&mut editor, Key::Tab, false);
        }
        key(&mut editor, Key::Tab, true);
        typed(&mut editor, "Last of the first");
        assert_eq!(grid(&editor)[0][2], "Last of the first", "it did not go up a row");
    }

    #[test]
    fn typing_over_a_cell_tab_landed_on_replaces_what_was_there() {
        let mut editor = with_table();
        typed(&mut editor, "One");
        key(&mut editor, Key::Tab, false);
        typed(&mut editor, "Two");
        key(&mut editor, Key::Tab, true);
        typed(&mut editor, "Changed");
        assert_eq!(grid(&editor)[0][0], "Changed", "it should have replaced what was there");
    }

    #[test]
    fn control_and_tab_types_a_tab_in_the_cell() {
        // Tab moves, so Word asks for a real tab with Control held.
        let mut editor = with_table();
        editor.handle(Event::KeyDown {
            key: Key::Tab,
            modifiers: Modifiers { control: true, ..Modifiers::default() },
        });
        let (first, _) = editor.document.cell_paragraphs(0, 0).expect("a cell");
        assert_eq!(
            editor.document.paragraph_text(first).as_deref(),
            Some("\t"),
            "Ctrl+Tab did not type a tab"
        );
    }

    #[test]
    fn enter_in_a_cell_adds_a_line_to_that_cell() {
        let mut editor = with_table();
        typed(&mut editor, "One");
        key(&mut editor, Key::Enter, false);
        typed(&mut editor, "Two");

        assert_eq!(grid(&editor).len(), 3, "Enter should not add a row to the table");
        let (first, last) = editor.document.cell_paragraphs(0, 0).expect("a cell");
        assert_eq!(last, first + 1, "the cell should hold two paragraphs now");
        assert_eq!(editor.document.paragraph_text(first).as_deref(), Some("One"));
        assert_eq!(editor.document.paragraph_text(last).as_deref(), Some("Two"));
    }

    #[test]
    fn backspace_at_the_start_of_a_cell_leaves_the_table_alone() {
        // In Word it does nothing: a cell is not joined onto the one before it
        // the way a paragraph is.
        let mut editor = with_table();
        key(&mut editor, Key::Tab, false);
        typed(&mut editor, "Two");
        let start = editor.document.cell_paragraphs(0, 1).expect("a cell").0;
        editor.document.set_caret(TextPosition::new(start, 0));

        let before = grid(&editor);
        key(&mut editor, Key::Backspace, false);
        assert_eq!(grid(&editor), before, "Backspace changed the table");
        assert!(editor.document.table_here().is_some(), "the table is gone");
    }

    #[test]
    fn a_row_can_be_added_and_the_table_keeps_what_was_in_it() {
        let mut editor = with_table();
        typed(&mut editor, "One");
        assert!(editor.document.insert_table_row(true), "no row was added");
        editor.relayout();

        let grid = grid(&editor);
        assert_eq!(grid.len(), 4, "there should be four rows");
        assert_eq!(grid[0][0], "One", "and the first cell should still say what it said");
        assert_eq!(grid[1].len(), 3, "and the new row should have three cells");
        assert!(grid[1].iter().all(String::is_empty), "the new row should be empty");
    }

    #[test]
    fn a_column_can_be_added_to_every_row_at_once() {
        let mut editor = with_table();
        assert!(editor.document.insert_table_column(true), "no column was added");
        editor.relayout();
        for (index, row) in grid(&editor).iter().enumerate() {
            assert_eq!(row.len(), 4, "row {index} did not gain a cell");
        }
    }

    #[test]
    fn a_row_can_be_deleted() {
        let mut editor = with_table();
        typed(&mut editor, "One");
        for _ in 0..3 {
            key(&mut editor, Key::Tab, false);
        }
        typed(&mut editor, "Two");

        // The caret is in the second row, which is the one that goes.
        assert!(editor.document.delete_table_row(), "no row was deleted");
        editor.relayout();
        let grid = grid(&editor);
        assert_eq!(grid.len(), 2, "there should be two rows left");
        assert_eq!(grid[0][0], "One", "and the first should be untouched");
    }

    #[test]
    fn deleting_every_row_deletes_the_table() {
        let mut editor = with_table();
        for _ in 0..3 {
            editor.document.delete_table_row();
            editor.relayout();
        }
        assert!(editor.document.table_here().is_none(), "the table should be gone");
    }

    #[test]
    fn two_cells_can_be_merged() {
        let mut editor = with_table();
        let (first, _) = editor.document.cell_paragraphs(0, 0).expect("a cell");
        let (second, _) = editor.document.cell_paragraphs(0, 1).expect("a cell");
        editor.document.set_caret(TextPosition::new(first, 0));
        editor.document.extend_selection_to(TextPosition::new(second, 0));

        assert!(editor.document.merge_cells(), "nothing was merged");
        editor.relayout();
        assert_eq!(grid(&editor)[0].len(), 2, "the first row should have one cell fewer");
    }

    #[test]
    fn a_press_puts_the_caret_in_the_cell_it_landed_in() {
        // The one that made tables unusable: every cell of a row is at the same
        // height, so a press has to be answered by the cell it is in and not by
        // the first line that happens to be level with it.
        let mut editor = with_table();
        for row in 0..3 {
            for column in 0..3 {
                let (x, y) = cell_middle(&editor, row, column);
                editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
                editor.handle(Event::MouseUp { x, y });
                let place = editor.document.table_here().expect("in the table");
                assert_eq!(
                    (place.row, place.column),
                    (row, column),
                    "a press in the cell at {row},{column} landed elsewhere"
                );
            }
        }
    }

    #[test]
    fn a_press_in_a_cell_that_has_text_lands_in_that_cell() {
        // Text in the cells is what the old answer went by, so the press is
        // tried again with every cell filled in.
        let mut editor = with_table();
        for _ in 0..9 {
            typed(&mut editor, "Text");
            key(&mut editor, Key::Tab, false);
        }
        editor.relayout();

        let (x, y) = cell_middle(&editor, 2, 1);
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseUp { x, y });
        let place = editor.document.table_here().expect("in the table");
        assert_eq!((place.row, place.column), (2, 1), "the press landed in the wrong cell");
    }

    /// The middle of one cell of the table on the page, in window coordinates.
    fn cell_middle(editor: &Editor, row: usize, column: usize) -> (i32, i32) {
        let (_, cell) = editor.placed_cell(row, column).expect("the cell is on the page");
        let (origin_x, origin_y) = editor.page_origin(0);
        let x = origin_x + cell.x + cell.width / 2.0;
        let y = editor.content_top() + origin_y - editor.scroll_down() + cell.y + cell.height / 2.0;
        (x as i32, y as i32)
    }

    /// Which cell of the table the caret is in.
    fn cell_of_caret(editor: &Editor) -> Option<(usize, usize)> {
        let place = editor.document.table_here()?;
        Some((place.row, place.column))
    }

    #[test]
    fn the_down_arrow_goes_to_the_cell_below() {
        let mut editor = with_table();
        key(&mut editor, Key::Down, false);
        assert_eq!(cell_of_caret(&editor), Some((1, 0)), "down did not reach the second row");
        key(&mut editor, Key::Down, false);
        assert_eq!(cell_of_caret(&editor), Some((2, 0)), "nor the third");
    }

    #[test]
    fn the_down_arrow_stays_in_its_column() {
        let mut editor = with_table();
        key(&mut editor, Key::Tab, false);
        key(&mut editor, Key::Down, false);
        assert_eq!(cell_of_caret(&editor), Some((1, 1)), "down changed column");
    }

    #[test]
    fn the_up_arrow_goes_to_the_cell_above() {
        let mut editor = with_table();
        for _ in 0..4 {
            key(&mut editor, Key::Tab, false);
        }
        assert_eq!(cell_of_caret(&editor), Some((1, 1)), "Tab did not get to the middle");
        key(&mut editor, Key::Up, false);
        assert_eq!(cell_of_caret(&editor), Some((0, 1)), "up did not go up a row");
    }

    #[test]
    fn the_down_arrow_leaves_the_table_at_the_last_row() {
        let mut editor = editor();
        editor.document.set_caret(TextPosition::new(0, 6));
        assert!(editor.document.insert_table(3, 3), "no table");
        editor.relayout();
        let (first, _) = editor.document.cell_paragraphs(2, 0).expect("a cell");
        editor.document.set_caret(TextPosition::new(first, 0));

        key(&mut editor, Key::Down, false);
        assert!(editor.document.table_here().is_none(), "the caret is still in the table");
    }

    #[test]
    fn the_up_arrow_leaves_the_table_at_the_first_row() {
        let mut editor = with_table();
        key(&mut editor, Key::Up, false);
        assert!(editor.document.table_here().is_none(), "the caret is still in the table");
        assert_eq!(editor.document.caret().paragraph, 0, "it should be in the paragraph above");
    }

    #[test]
    fn the_arrows_move_within_a_cell_that_has_two_lines_of_its_own() {
        let mut editor = with_table();
        typed(&mut editor, "One");
        key(&mut editor, Key::Enter, false);
        typed(&mut editor, "Two");
        let (first, last) = editor.document.cell_paragraphs(0, 0).expect("a cell");

        key(&mut editor, Key::Up, false);
        assert_eq!(editor.document.caret().paragraph, first, "up left the cell");
        key(&mut editor, Key::Down, false);
        assert_eq!(editor.document.caret().paragraph, last, "down left the cell");
    }

    #[test]
    fn the_right_arrow_at_the_end_of_a_cell_goes_into_the_next_one() {
        let mut editor = with_table();
        typed(&mut editor, "One");
        key(&mut editor, Key::Right, false);
        assert_eq!(cell_of_caret(&editor), Some((0, 1)), "right did not reach the next cell");
    }

    /// Fills every cell of the table in with what its place is.
    fn fill(editor: &mut Editor) {
        for row in 0..3 {
            for column in 0..3 {
                let (first, _) = editor.document.cell_paragraphs(row, column).expect("a cell");
                editor.document.set_caret(TextPosition::new(first, 0));
                typed(editor, &format!("{row}{column}"));
            }
        }
        editor.relayout();
    }

    /// Whether a cell has any of the selection in it.
    fn is_selected(editor: &Editor, row: usize, column: usize) -> bool {
        let Some((first, last)) = editor.document.cell_paragraphs(row, column) else {
            return false;
        };
        editor
            .document
            .selections()
            .iter()
            .any(|(start, end)| start.paragraph <= last && end.paragraph >= first)
    }

    /// Drags from the middle of one cell to the middle of another.
    fn drag(editor: &mut Editor, from: (usize, usize), to: (usize, usize)) {
        let (x, y) = cell_middle(editor, from.0, from.1);
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        let (x, y) = cell_middle(editor, to.0, to.1);
        editor.handle(Event::MouseMove { x, y, held: true, modifiers: Modifiers::default() });
        editor.handle(Event::MouseUp { x, y });
    }

    #[test]
    fn a_drag_across_cells_takes_the_rectangle_between_them() {
        // Word's rule, and the reason it cannot be a stretch of text: the cells
        // between two cells in reading order are not the cells between them on
        // the page.
        let mut editor = with_table();
        fill(&mut editor);
        drag(&mut editor, (0, 0), (1, 1));

        for (row, column) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            assert!(is_selected(&editor, row, column), "the cell at {row},{column} is not taken");
        }
        for (row, column) in [(0, 2), (1, 2), (2, 0), (2, 1), (2, 2)] {
            assert!(!is_selected(&editor, row, column), "the cell at {row},{column} is taken");
        }
    }

    #[test]
    fn a_drag_inside_one_cell_takes_letters_and_not_cells() {
        let mut editor = with_table();
        fill(&mut editor);
        let (_, cell) = editor.placed_cell(1, 1).expect("a cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let y = (top + cell.y + cell.height / 2.0) as i32;

        editor.handle(Event::MouseDown {
            x: (origin_x + cell.x + 12.0) as i32,
            y,
            modifiers: Modifiers::default(),
        });
        editor.handle(Event::MouseMove {
            x: (origin_x + cell.x + cell.width - 12.0) as i32,
            y,
            held: true,
            modifiers: Modifiers::default(),
        });

        assert!(!is_selected(&editor, 1, 0), "the cell beside it was taken");
        let selected = editor.document.selections();
        assert_eq!(selected.len(), 1, "one stretch, inside the one cell");
    }

    #[test]
    fn deleting_a_rectangle_of_cells_empties_them_and_keeps_the_table() {
        // Word's Delete on a block of cells clears what is in them. The cells
        // themselves stay: taking them out is what Delete Rows is for.
        let mut editor = with_table();
        fill(&mut editor);
        drag(&mut editor, (0, 0), (1, 1));
        key(&mut editor, Key::Delete, false);
        editor.relayout();

        let grid = grid(&editor);
        assert_eq!(grid.len(), 3, "the table lost a row");
        assert_eq!(grid[0].len(), 3, "the table lost a column");
        assert_eq!(grid[0][0], "", "the first cell was not emptied");
        assert_eq!(grid[0][1], "", "the second cell was not emptied");
        assert_eq!(grid[1][0], "", "the cell below was not emptied");
        assert_eq!(grid[1][1], "", "nor the one beside it");
        assert_eq!(grid[0][2], "02", "a cell outside the rectangle was emptied");
        assert_eq!(grid[2][2], "22", "and so was the last one");
    }

    #[test]
    fn typing_over_a_rectangle_of_cells_empties_them_and_types_in_the_first() {
        let mut editor = with_table();
        fill(&mut editor);
        drag(&mut editor, (0, 0), (1, 1));
        typed(&mut editor, "New");
        editor.relayout();

        let grid = grid(&editor);
        assert_eq!(grid[0][0], "New", "the typing did not go into the first cell");
        assert_eq!(grid[0][1], "", "the second cell was not emptied");
        assert_eq!(grid[1][0], "", "nor the one below");
        assert_eq!(grid[1][1], "", "nor the fourth");
        assert_eq!(grid[2][2], "22", "a cell outside the rectangle was changed");
    }

    #[test]
    fn shift_and_a_press_in_another_cell_takes_the_cells_between() {
        let mut editor = with_table();
        fill(&mut editor);
        let (first, _) = editor.document.cell_paragraphs(0, 0).expect("a cell");
        editor.document.set_caret(TextPosition::new(first, 0));

        let (x, y) = cell_middle(&editor, 2, 1);
        editor.handle(Event::MouseDown {
            x,
            y,
            modifiers: Modifiers { shift: true, ..Modifiers::default() },
        });
        editor.handle(Event::MouseUp { x, y });

        for row in 0..3 {
            for column in 0..2 {
                assert!(is_selected(&editor, row, column), "the cell at {row},{column} is not in");
            }
            assert!(!is_selected(&editor, row, 2), "the last column should be out of it");
        }
    }

    #[test]
    fn a_press_in_the_bar_beside_a_row_takes_the_whole_row() {
        // The selection bar down the left of the page takes a line of text;
        // beside a table it takes the row, which is what Word takes. A line of
        // it would be one cell's worth of one row, and nobody means that.
        let mut editor = with_table();
        fill(&mut editor);
        let (_, cell) = editor.placed_cell(1, 0).expect("a cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + cell.x / 2.0) as i32;
        let y = (top + cell.y + cell.height / 2.0) as i32;

        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseUp { x, y });

        let taken = editor.document.selected_cells().expect("a block of cells");
        assert_eq!(taken.rows, (1, 1), "another row was taken");
        assert_eq!(taken.columns, (0, 2), "the row was not taken whole");
    }

    #[test]
    fn a_press_above_a_column_takes_the_whole_column() {
        let mut editor = with_table();
        fill(&mut editor);
        let (_, cell) = editor.placed_cell(0, 1).expect("a cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + cell.x + cell.width / 2.0) as i32;
        let y = (top + cell.y - 3.0) as i32;

        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseUp { x, y });

        let taken = editor.document.selected_cells().expect("a block of cells");
        assert_eq!(taken.columns, (1, 1), "another column was taken");
        assert_eq!(taken.rows, (0, 2), "the column was not taken whole");
    }

    #[test]
    fn a_block_of_empty_cells_is_a_block_all_the_same() {
        // Nothing to select in them, and Merge Cells has to work on them: a
        // table somebody has only just inserted is all empty cells.
        let mut editor = with_table();
        drag(&mut editor, (0, 0), (0, 1));

        let taken = editor.document.selected_cells().expect("a block of cells");
        assert_eq!(taken.rows, (0, 0));
        assert_eq!(taken.columns, (0, 1));
        assert!(editor.document.merge_cells(), "the empty cells would not merge");
        editor.relayout();
        assert_eq!(grid(&editor)[0].len(), 2, "the row should have one cell fewer");
    }

    #[test]
    fn moving_the_caret_gives_up_a_block_of_cells() {
        let mut editor = with_table();
        drag(&mut editor, (0, 0), (1, 1));
        assert!(editor.document.selected_cells().is_some(), "nothing was taken");

        key(&mut editor, Key::Right, false);
        let here = editor.document.selected_cells().expect("the caret's own cell");
        assert_eq!(here.rows, (here.rows.1, here.rows.1), "the block outlived the selection");
        assert_eq!(here.columns.0, here.columns.1, "the block outlived the selection");
    }

    #[test]
    fn a_table_survives_being_saved_and_opened_again() {
        let mut editor = with_table();
        typed(&mut editor, "One");
        key(&mut editor, Key::Tab, false);
        typed(&mut editor, "Two");

        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        let rows = reopened.table_rows_text_at(1);
        assert_eq!(rows.len(), 3, "the table did not survive");
        assert_eq!(rows[0][0], "One");
        assert_eq!(rows[0][1], "Two");
    }
}

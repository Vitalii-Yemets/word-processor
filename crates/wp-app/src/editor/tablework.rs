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

/// What is drawn as the selection. See [`Editor::selection_as_drawn`].
pub(super) struct DrawnSelection {
    /// The cells shown selected, each with the page it is on.
    pub(super) cells: Vec<(usize, wp_layout::PlacedCell)>,
    /// And the stretches of text shown selected.
    pub(super) text: Vec<(wp_docx::TextPosition, wp_docx::TextPosition)>,
}

impl Editor {
    /// Moves the caret a cell on, or with Shift a cell back.
    ///
    /// Word's Tab. It moves by cell and not by character, it takes what is in
    /// the cell it lands on, and at the last cell of the last row it adds a row
    /// rather than doing nothing — which is how a table is filled in without
    /// ever reaching for the ribbon.
    ///
    /// Only through the cells that are drawn: the continuations a merge down
    /// leaves in the rows below its first are passed over. See
    /// [`wp_docx::Document::cell_beside`].
    pub(super) fn step_cell(&mut self, forwards: bool) -> Response {
        if self.document.table_here().is_none() {
            return Response::Ignored;
        }

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
            // it, which is how a table grows while it is being filled in. The
            // row goes under the last row of the table, which is not always
            // the caret's: a table whose last cells are merged down from above
            // ends below the row the last cell that is drawn starts in.
            None => {
                if !self.document.append_table_row() {
                    return Response::Ignored;
                }
                let Some(last) =
                    self.document.table_here().and_then(|place| place.rows.checked_sub(1))
                else {
                    return Response::Ignored;
                };
                self.take_cell(last, 0);
                self.edited(true, "Row added")
            }
        }
    }

    /// Carries the caret on past a cell that a merge down hid, after the left
    /// or right arrow put it in one.
    ///
    /// Those arrows move through the text in the order it is written, and a
    /// continuation of a merge is written in each row the merged cell covers:
    /// one arrow too many at the end of a row put the caret in an empty
    /// paragraph nobody can see, where whatever was typed next went unseen
    /// too — Word draws nothing of a continuation either. So the caret goes
    /// on, the way it was going, to the next place that is drawn.
    pub(super) fn step_over_hidden_cells(&mut self, forwards: bool, extend: bool) {
        // One cell at a time, and never more times than there are paragraphs:
        // a table of nothing but continuations cannot keep the arrow going.
        for _ in 0..self.document.paragraph_count() {
            let here = self.document.caret().paragraph;
            let Some((first, last)) = self.document.hidden_cell_at(here) else { return };
            let to = if forwards {
                if last + 1 >= self.document.paragraph_count() {
                    return;
                }
                wp_docx::TextPosition::new(last + 1, 0)
            } else {
                let Some(before) = first.checked_sub(1) else { return };
                let end = self.document.paragraph_text(before).map_or(0, |text| text.len());
                wp_docx::TextPosition::new(before, end)
            };
            self.document.move_caret(to, extend);
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
    /// selected and not the cells. The rows a selection takes whole by
    /// running out of a table are cells as well, as Word draws them: the text
    /// on either side of them is drawn as text. See
    /// [`Self::selection_as_drawn`], which is what drawing asks; this is the
    /// half of it the tests look at.
    #[cfg(test)]
    pub(super) fn selected_cell_rects(&self) -> Vec<(usize, wp_layout::PlacedCell)> {
        self.selection_as_drawn().cells
    }

    /// What is drawn as the selection: the cells shown selected, and the
    /// stretches of text — none when the selection is a block of cells, and
    /// otherwise every stretch but for the rows of a table it takes whole,
    /// which are among the cells.
    pub(super) fn selection_as_drawn(&self) -> DrawnSelection {
        if let Some(block) = self.block_cell_rects() {
            return DrawnSelection { cells: block, text: Vec::new() };
        }
        let rows = self.document.whole_rows_selected();
        let mut cells = Vec::new();
        if !rows.is_empty() {
            for (index, page) in self.pages.iter().enumerate() {
                for cell in &page.cells {
                    let at = cell.at.paragraph;
                    if rows.iter().any(|(first, last)| (*first..=*last).contains(&at)) {
                        cells.push((index, *cell));
                    }
                }
            }
        }

        let mut text = Vec::new();
        for (start, end) in self.document.selections() {
            let mut from = start;
            for &(first, last) in &rows {
                if last < from.paragraph || first > end.paragraph {
                    continue;
                }
                // Up to the end of the paragraph before the row — an offset
                // past the end of a line is the end of it.
                if let Some(before) = first.checked_sub(1).filter(|_| from.paragraph < first) {
                    text.push((from, wp_docx::TextPosition::new(before, usize::MAX)));
                }
                from = wp_docx::TextPosition::new(last + 1, 0);
            }
            if from < end {
                text.push((from, end));
            }
        }
        DrawnSelection { cells, text }
    }

    /// The cells of a block of more than one, when that is what is selected.
    fn block_cell_rects(&self) -> Option<Vec<(usize, wp_layout::PlacedCell)>> {
        let range = self.document.selected_cells()?;
        if range.rows.0 == range.rows.1 && range.columns.0 == range.columns.1 {
            return None;
        }
        let mut out = Vec::new();
        for row in range.rows.0..=range.rows.1 {
            for column in range.columns.0..=range.columns.1 {
                if let Some(found) = self.placed_cell(row, column) {
                    out.push(found);
                }
            }
        }
        Some(out)
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
        // Another line of the same cell that way is an ordinary move.
        if self.document.table_here().is_none() || self.cell_goes_on(downwards) != Some(false) {
            return false;
        }

        let wanted_x = self.caret_across();

        // Off the end of the table, the caret leaves it — for the paragraph
        // after the table going down, and the one before it going up, which is
        // where Word puts it. The row next door is the next one that is drawn
        // there: down from a cell merged over several rows is past the last
        // of them, and up into one is into the cell itself rather than into a
        // continuation of it. See [`wp_docx::Document::cell_over_or_under`].
        let Some((row, column)) = self.document.cell_over_or_under(downwards) else {
            let Some((above, below)) = self.document.table_paragraphs() else { return false };
            let outside = if downwards { below + 1 } else { above.wrapping_sub(1) };
            if outside >= self.document.paragraph_count() {
                return true;
            }
            return self.caret_onto_paragraph(outside, !downwards, wanted_x, extend);
        };

        // Into the cell below or above. The wanted place across the page is
        // held inside that cell, so that a row whose columns do not line up
        // with this one — a merge, most often — still takes the caret rather
        // than handing it to the cell next door.
        let Some((_, cell)) = self.placed_cell(row, column) else { return false };
        let across = wanted_x.clamp(cell.x + 1.0, cell.x + cell.width - 1.0);
        let Some((below, above)) = self.document.cell_paragraphs(row, column) else {
            return false;
        };
        let paragraph = if downwards { below } else { above };
        self.caret_onto_paragraph(paragraph, !downwards, across, extend)
    }

    /// Whether the caret's cell has another line of its own above the caret,
    /// or below it: `None` when the caret is in no cell, or on no line.
    fn cell_goes_on(&self, downwards: bool) -> Option<bool> {
        let place = self.document.table_here()?;
        let (first, last) = self.document.cell_paragraphs(place.row, place.column)?;
        let lines = self.lines();
        let current = self.caret_line()?;
        let index = lines.iter().position(|entry| *entry == current)?;
        let beside = if downwards { index + 1 } else { index.wrapping_sub(1) };
        Some(lines.get(beside).is_some_and(|&(page, line)| {
            (first..=last).contains(&self.pages[page].lines[line].paragraph)
        }))
    }

    /// Shift and an arrow in a table, which select cells rather than text.
    ///
    /// Returns whether it took the key. Word's rules: inside the text of a
    /// cell, Shift+Left and Shift+Right select letters as anywhere else, and
    /// the step past the cell's edge takes the cell and the one beside it,
    /// whole; Shift+Up and Shift+Down take the cell and the one above or
    /// below — unless the cell has another line that way, which is text too.
    /// Once the selection is cells, each arrow moves the far corner of the
    /// block a cell, and out of the top or the bottom of the table it takes
    /// the rows from the one it began in, and the text beyond them.
    ///
    /// # Why not the ordinary move with Shift
    ///
    /// Because that moved the caret into the next cell as text, and the
    /// selection became a stretch from one cell to another running through
    /// every cell between them in the order the file is written: drawn as a
    /// block of two cells, and acted on as the text of five. Delete then took
    /// the only paragraph of cells nobody saw selected, which is a file Word
    /// calls damaged. This goes through [`Self::select_cells`], as a drag and
    /// Shift and a press do.
    pub(super) fn shift_arrow_in_table(&mut self, key: wp_shell::Key) -> bool {
        use wp_shell::Key;

        let Some(place) = self.document.table_here() else { return false };
        let here = (place.row, place.column);

        // A block already: its far corner moves a cell.
        if let Some(block) = self.document.selected_block() {
            if block.table != place.table {
                return false;
            }
            let anchor = self.block_anchor(&block);
            // The corner of the block across from where it is anchored.
            let corner = (
                if anchor.0 == block.rows.0 { block.rows.1 } else { block.rows.0 },
                if anchor.1 == block.columns.0 { block.columns.1 } else { block.columns.0 },
            );
            let corner = match key {
                Key::Left => (corner.0, corner.1.saturating_sub(1)),
                Key::Right => (corner.0, corner.1 + 1),
                Key::Up if corner.0 == 0 => {
                    return self.take_rows_out_of_the_table(anchor.0, false)
                }
                Key::Up => (corner.0 - 1, corner.1),
                Key::Down if corner.0 + 1 >= place.rows => {
                    return self.take_rows_out_of_the_table(anchor.0, true);
                }
                Key::Down => (corner.0 + 1, corner.1),
                _ => return false,
            };
            return self.select_block_from(anchor, corner);
        }

        // Text inside the one cell — an anchor anywhere else is a selection
        // from outside the table, and moves as text does.
        let anchor = self.document.selection_anchor().unwrap_or(self.document.caret());
        let anchored = self.document.table_at(anchor.paragraph);
        if anchored.is_none_or(|at| at.table != place.table || (at.row, at.column) != here) {
            return false;
        }
        let Some((first, last)) = self.document.cell_paragraphs(here.0, here.1) else {
            return false;
        };
        let caret = self.document.caret();
        let end = self.document.paragraph_text(last).map_or(0, |text| text.len());
        let corner = match key {
            Key::Left if caret == wp_docx::TextPosition::new(first, 0) => {
                (here.0, here.1.saturating_sub(1))
            }
            Key::Right if caret == wp_docx::TextPosition::new(last, end) => (here.0, here.1 + 1),
            Key::Up | Key::Down => {
                let downwards = key == Key::Down;
                if self.cell_goes_on(downwards) != Some(false) {
                    return false;
                }
                match self.document.cell_over_or_under(downwards) {
                    Some(cell) => cell,
                    None => return self.take_rows_out_of_the_table(here.0, downwards),
                }
            }
            _ => return false,
        };
        self.cell_anchor = Some(here);
        self.select_block_from(here, corner)
    }

    /// Which corner a block is anchored at: the cell a drag or the keyboard
    /// began it in, when that is a corner of it, and its first cell when the
    /// block was made some other way — by the Select menu, say.
    fn block_anchor(&self, block: &wp_docx::cells::CellRange) -> (usize, usize) {
        self.cell_anchor
            .filter(|&(row, column)| {
                (row == block.rows.0 || row == block.rows.1)
                    && (column == block.columns.0 || column == block.columns.1)
            })
            .unwrap_or((block.rows.0, block.columns.0))
    }

    /// Selects the block from one cell to another, the second held inside the
    /// row it is in — a row ends where its cells do, and a merged one has
    /// fewer.
    fn select_block_from(&mut self, anchor: (usize, usize), corner: (usize, usize)) -> bool {
        let cells = self.document.cells_in_row(corner.0);
        let corner = (corner.0, corner.1.min(cells.saturating_sub(1)));
        let rows = (anchor.0.min(corner.0), anchor.0.max(corner.0));
        let columns = (anchor.1.min(corner.1), anchor.1.max(corner.1));
        self.select_cells(rows, columns)
    }

    /// Takes the rows of the table from one to its end, or to its start, and
    /// the caret on out of it: Word's Shift+Down out of the last row, or
    /// Shift+Up out of the first.
    ///
    /// The selection is anchored at the edge of the row it began in and ends
    /// where the caret lands past the table; the document takes every row
    /// between whole. See [`wp_docx::Document::selections`].
    fn take_rows_out_of_the_table(&mut self, row: usize, downwards: bool) -> bool {
        let Some((above, below)) = self.document.table_paragraphs() else { return false };
        let outside = if downwards { below + 1 } else { above.wrapping_sub(1) };
        // Nothing past the table that way: there is nowhere for the
        // selection to go, and the key has done all it can.
        if outside >= self.document.paragraph_count() {
            return true;
        }
        let edge = if downwards {
            self.document
                .cell_paragraphs(row, 0)
                .map(|(first, _)| wp_docx::TextPosition::new(first, 0))
        } else {
            let last_cell = self.document.cells_in_row(row).saturating_sub(1);
            self.document.cell_text_range(row, last_cell).map(|(_, end)| end)
        };
        let Some(edge) = edge else { return false };
        let wanted_x = self.caret_across();
        self.document.set_caret(edge);
        self.caret_onto_paragraph(outside, !downwards, wanted_x, true)
    }

    /// A drag that began in a cell and has gone out of its table: from the
    /// start of the cell it began in to where the pointer is, which the
    /// document takes as whole rows from that cell's row, and the text past
    /// the table as far as the pointer — Word's. A block of cells the drag
    /// made on its way out is given up for them.
    ///
    /// Returns whether it took the drag.
    pub(super) fn drag_out_of_the_table(&mut self, at: wp_docx::TextPosition) -> bool {
        let Some((row, column)) = self.cell_anchor else { return false };
        let inside = self.document.selection_anchor().unwrap_or(self.document.caret());
        let Some(began) = self.document.table_at(inside.paragraph) else { return false };
        if self.document.table_at(at.paragraph).is_some_and(|there| there.table == began.table) {
            return false;
        }
        let Some((first, _)) = self.document.cell_paragraphs_at(inside.paragraph, row, column)
        else {
            return false;
        };
        self.document.set_caret(wp_docx::TextPosition::new(first, 0));
        self.document.move_caret(at, true);
        self.needs_redraw = true;
        true
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
    pub(super) fn take_cell(&mut self, row: usize, column: usize) -> bool {
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

    /// How far past the right-hand edge of a cell anything in it is drawn.
    fn overflow(editor: &Editor, row: usize, column: usize) -> f32 {
        let (page, cell) = editor.placed_cell(row, column).expect("the cell is on the page");
        let (first, last) = editor.document.cell_paragraphs(row, column).expect("a cell");
        editor.pages[page]
            .glyphs
            .iter()
            .filter(|glyph| (first..=last).contains(&glyph.source.paragraph))
            .map(|glyph| glyph.x + glyph.advance - (cell.x + cell.width))
            .fold(0.0, f32::max)
    }

    #[test]
    fn a_word_too_wide_for_its_cell_is_broken_rather_than_drawn_over_the_border() {
        // Word breaks a word that will not fit its cell at all. Ours drew it
        // across the border and over whatever was in the next cell, which is
        // what a bulleted list in a narrow cell looked like: the list's indent
        // leaves the cell narrower still.
        let mut editor = with_table();
        editor.run(crate::chrome::Command::Bullets);
        typed(&mut editor, "укецукецукецукецукецуке");
        editor.relayout();

        let past = overflow(&editor, 0, 0);
        assert!(past <= 1.0, "the word is drawn {past} pixels past the edge of its cell");
        assert!(
            editor.pages[0].lines.iter().filter(|line| line.paragraph == 1).count() >= 2,
            "the word was not broken onto a second line"
        );
    }

    #[test]
    fn a_long_word_in_a_plain_cell_stays_inside_it() {
        let mut editor = with_table();
        typed(&mut editor, "Averylongwordwithnospacesinitatall");
        editor.relayout();
        let past = overflow(&editor, 0, 0);
        assert!(past <= 1.0, "the word is drawn {past} pixels past the edge of its cell");
    }

    /// Where the text of a cell is, as a rectangle on the page.
    fn text_box(editor: &Editor, row: usize, column: usize) -> (f32, f32, f32, f32) {
        let (page, _) = editor.placed_cell(row, column).expect("the cell is on the page");
        let (first, last) = editor.document.cell_paragraphs(row, column).expect("a cell");
        let mut left = f32::MAX;
        let mut right = f32::MIN;
        let mut top = f32::MAX;
        let mut bottom = f32::MIN;
        for line in
            editor.pages[page].lines.iter().filter(|line| (first..=last).contains(&line.paragraph))
        {
            left = left.min(line.left);
            right = right.max(line.right);
            top = top.min(line.top());
            bottom = bottom.max(line.bottom());
        }
        assert!(right > f32::MIN, "the cell has no lines on the page");
        (left, top, right, bottom)
    }

    #[test]
    fn text_in_a_cell_can_be_centred_across_it() {
        let mut editor = with_table();
        typed(&mut editor, "Mid");
        editor.run(crate::chrome::Command::AlignCell(4));
        editor.relayout();

        let (_, cell) = editor.placed_cell(0, 0).expect("a cell");
        let (left, _, right, _) = text_box(&editor, 0, 0);
        let text_middle = (left + right) / 2.0;
        let cell_middle = cell.x + cell.width / 2.0;
        assert!(
            (text_middle - cell_middle).abs() < 2.0,
            "the text sits at {text_middle} and the middle of the cell is {cell_middle}"
        );
    }

    #[test]
    fn text_in_a_tall_cell_can_be_centred_down_it() {
        let mut editor = with_table();
        typed(&mut editor, "Mid");
        // Word centres the paragraph and the room it asks for round itself, so
        // the space after the paragraph is part of what is centred. It is taken
        // off here, which makes the two measurements the same thing and the
        // test about the centring rather than about the spacing.
        editor.document.set_paragraph_format(&wp_docx::model::ParagraphProperties {
            space_before: Some(0),
            space_after: Some(0),
            ..wp_docx::model::ParagraphProperties::default()
        });
        // A row taller than its text is the only case where down-the-cell
        // alignment shows at all.
        assert!(editor.document.set_table_row_height(Some(1440), false), "no height was set");
        editor.run(crate::chrome::Command::AlignCell(4));
        editor.relayout();

        let (_, cell) = editor.placed_cell(0, 0).expect("a cell");
        let (_, top, _, bottom) = text_box(&editor, 0, 0);
        let text_middle = (top + bottom) / 2.0;
        let cell_middle = cell.y + cell.height / 2.0;
        assert!(
            (text_middle - cell_middle).abs() < 4.0,
            "the text sits at {text_middle} and the middle of the cell is {cell_middle}"
        );
    }

    #[test]
    fn a_row_given_a_height_takes_it_and_gives_it_back() {
        let mut editor = with_table();
        typed(&mut editor, "One");
        editor.relayout();
        let (_, before) = editor.placed_cell(0, 0).expect("a cell");

        assert!(editor.document.set_table_row_height(Some(1440), false), "no height was set");
        editor.relayout();
        let (_, taller) = editor.placed_cell(0, 0).expect("a cell");
        assert!(taller.height > before.height * 1.5, "the row did not grow: {}", taller.height);

        // Nothing asked for is Word's automatic: the row goes back to the
        // height of what is in it.
        assert!(editor.document.set_table_row_height(None, false), "the height would not clear");
        editor.relayout();
        let (_, back) = editor.placed_cell(0, 0).expect("a cell");
        assert!(
            (back.height - before.height).abs() < 1.0,
            "the row stayed {} when it was {} to begin with",
            back.height,
            before.height
        );
    }

    #[test]
    fn a_row_asked_for_an_exact_height_is_that_tall_and_no_taller() {
        // Word's Exactly: the row is the height it is told, and text that does
        // not fit is cut off rather than making the row grow.
        let mut editor = with_table();
        typed(&mut editor, "One");
        key(&mut editor, Key::Enter, false);
        typed(&mut editor, "Two");
        key(&mut editor, Key::Enter, false);
        typed(&mut editor, "Three");
        editor.relayout();
        let (_, grown) = editor.placed_cell(0, 0).expect("a cell");

        assert!(editor.document.set_table_row_height(Some(720), true), "no height was set");
        editor.relayout();
        let (_, exact) = editor.placed_cell(0, 0).expect("a cell");
        assert!(
            exact.height < grown.height,
            "three lines in a half-inch row came out {} tall",
            exact.height
        );
    }

    #[test]
    fn dragging_a_row_shorter_brings_it_back_to_its_text() {
        let mut editor = with_table();
        typed(&mut editor, "One");
        editor.relayout();
        let (_, hugging) = editor.placed_cell(0, 0).expect("a cell");

        assert!(editor.document.set_table_row_height(Some(1440), false), "no height was set");
        editor.relayout();
        let (_, cell) = editor.placed_cell(0, 0).expect("a cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + cell.x + cell.width / 2.0) as i32;
        let y = (top + cell.y + cell.height) as i32;

        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseMove {
            x,
            y: y - 400,
            held: true,
            modifiers: Modifiers::default(),
        });
        editor.handle(Event::MouseUp { x, y: y - 400 });

        let (_, after) = editor.placed_cell(0, 0).expect("a cell");
        assert!(
            after.height < hugging.height + 2.0,
            "dragging up left the row {} tall when its text needs {}",
            after.height,
            hugging.height
        );
    }

    #[test]
    fn every_row_of_a_new_table_is_the_same_height() {
        let mut editor = with_table();
        editor.relayout();
        let heights: Vec<f32> =
            (0..3).map(|row| editor.placed_cell(row, 0).expect("a cell").1.height).collect();
        assert!(
            heights.windows(2).all(|pair| (pair[0] - pair[1]).abs() < 0.5),
            "the rows came out {heights:?}"
        );
    }

    #[test]
    fn a_row_with_text_in_it_is_no_taller_than_an_empty_one() {
        // One line of text and one empty paragraph are the same height, so the
        // rows are too — a table whose rows jump about as it is filled in looks
        // broken whatever the file says.
        let mut editor = with_table();
        typed(&mut editor, "One");
        editor.relayout();
        let filled = editor.placed_cell(0, 0).expect("a cell").1.height;
        let empty = editor.placed_cell(1, 0).expect("a cell").1.height;
        assert!((filled - empty).abs() < 0.5, "filled {filled} against empty {empty}");
    }

    /// Merges a block of cells as a person does: dragged across, and Merge
    /// Cells pressed.
    fn merge(editor: &mut Editor, from: (usize, usize), to: (usize, usize)) {
        drag(editor, from, to);
        editor.run(crate::chrome::Command::MergeCells);
        editor.relayout();
    }

    /// Puts the caret at the start of one cell, as a press in it would.
    fn caret_into(editor: &mut Editor, row: usize, column: usize) {
        // Cells are found through the table the caret is in, and the table
        // begins at the paragraph after the one it was put in after.
        if editor.document.table_here().is_none() {
            editor.document.set_caret(TextPosition::new(1, 0));
        }
        let (first, _) = editor.document.cell_paragraphs(row, column).expect("a cell");
        editor.document.set_caret(TextPosition::new(first, 0));
    }

    #[test]
    fn merging_leaves_the_caret_at_the_start_of_the_merged_cell() {
        // It was left on the paragraph number it had been on, which after the
        // merge belonged to the next cell along.
        let mut editor = with_table();
        fill(&mut editor);
        merge(&mut editor, (0, 0), (0, 1));

        assert_eq!(cell_of_caret(&editor), Some((0, 0)), "the caret is not in the merged cell");
        let (first, last) = editor.document.cell_paragraphs(0, 0).expect("a cell");
        assert_eq!(editor.document.caret(), TextPosition::new(first, 0), "nor at its start");
        let said: Vec<String> = (first..=last)
            .map(|paragraph| editor.document.paragraph_text(paragraph).unwrap_or_default())
            .collect();
        assert_eq!(said, ["00", "01"], "the merged cell lost what the second cell held");
    }

    #[test]
    fn merging_the_whole_table_keeps_the_caret_in_it() {
        // Every cell merged left the caret in the paragraph after the table,
        // and the table's two tabs went with it.
        let mut editor = with_table();
        fill(&mut editor);
        merge(&mut editor, (0, 0), (2, 2));

        assert_eq!(cell_of_caret(&editor), Some((0, 0)), "the caret was thrown out of the table");
        let state = editor.toolbar_state();
        assert!(
            crate::chrome::ribbon::Tab::TableLayout.applies(&state),
            "the table tabs went away"
        );
    }

    #[test]
    fn tab_in_a_table_merged_into_one_cell_adds_a_row() {
        // Tab went into the second row's continuation, which nothing draws.
        let mut editor = with_table();
        fill(&mut editor);
        merge(&mut editor, (0, 0), (2, 2));
        // Wherever the merge left it: this is about Tab.
        editor.document.set_caret(TextPosition::new(1, 0));

        key(&mut editor, Key::Tab, false);
        assert_eq!(grid(&editor).len(), 4, "Tab should have added a row");
        assert_eq!(cell_of_caret(&editor), Some((3, 0)), "and put the caret in it");
        typed(&mut editor, "New");
        assert_eq!(grid(&editor)[3][0], "New");
    }

    #[test]
    fn tab_and_the_arrows_walk_a_table_with_a_merge_down() {
        // The first column merged down the whole table: Tab and Shift+Tab go
        // through the cells that are drawn, the arrows step over the ones that
        // are not, and a block dragged over it is still the cells it covers.
        let mut editor = with_table();
        fill(&mut editor);
        merge(&mut editor, (0, 0), (2, 0));
        caret_into(&mut editor, 0, 0);

        let mut visited = vec![cell_of_caret(&editor)];
        for _ in 0..6 {
            key(&mut editor, Key::Tab, false);
            visited.push(cell_of_caret(&editor));
        }
        let drawn = [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 1), (2, 2)];
        assert_eq!(visited, drawn.map(Some), "Tab went into a cell that is not drawn");

        let mut back = vec![cell_of_caret(&editor)];
        for _ in 0..6 {
            key(&mut editor, Key::Tab, true);
            back.push(cell_of_caret(&editor));
        }
        let mut reversed = drawn;
        reversed.reverse();
        assert_eq!(back, reversed.map(Some), "Shift+Tab went into a cell that is not drawn");

        // Down out of the merged cell — from its last line, the ones above it
        // being lines of the same cell — is past the rows it covers, which
        // here is past the table.
        let (_, last) = editor.document.cell_paragraphs(0, 0).expect("the merged cell");
        editor.document.set_caret(TextPosition::new(last, 0));
        key(&mut editor, Key::Down, false);
        assert!(editor.document.table_here().is_none(), "down went into a continuation");

        caret_into(&mut editor, 0, 1);
        key(&mut editor, Key::Down, false);
        assert_eq!(cell_of_caret(&editor), Some((1, 1)), "down left its column");
        caret_into(&mut editor, 2, 1);
        key(&mut editor, Key::Up, false);
        assert_eq!(cell_of_caret(&editor), Some((1, 1)), "up left its column");

        let (last, _) = editor.document.cell_paragraphs(0, 2).expect("a cell");
        let end = editor.document.paragraph_text(last).unwrap_or_default().len();
        editor.document.set_caret(TextPosition::new(last, end));
        key(&mut editor, Key::Right, false);
        assert_eq!(cell_of_caret(&editor), Some((1, 1)), "right went into a continuation");
        caret_into(&mut editor, 2, 1);
        key(&mut editor, Key::Left, false);
        assert_eq!(cell_of_caret(&editor), Some((1, 2)), "left went into a continuation");

        drag(&mut editor, (0, 1), (2, 2));
        let taken = editor.document.selected_cells().expect("a block of cells");
        assert_eq!(
            (taken.rows, taken.columns),
            ((0, 2), (1, 2)),
            "the block is not the one dragged"
        );
        assert_eq!(editor.selected_cell_rects().len(), 6, "the block is not drawn as six cells");

        // And the last Tab of all adds a row.
        caret_into(&mut editor, 2, 2);
        key(&mut editor, Key::Tab, false);
        assert_eq!(grid(&editor).len(), 4, "Tab after the last cell did not add a row");
        assert_eq!(cell_of_caret(&editor), Some((3, 0)));
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

    /// A key pressed with Control held, and Shift as well if asked.
    fn control(editor: &mut Editor, key: Key, shift: bool) {
        editor.handle(Event::KeyDown {
            key,
            modifiers: Modifiers { control: true, shift, ..Modifiers::default() },
        });
    }

    /// What each cell of the table holds, paragraph by paragraph: what a
    /// review writes as `(P[00])(P[01])…`. A cell with no paragraph at all is
    /// an empty list.
    fn shapes(editor: &Editor) -> Vec<Vec<Vec<String>>> {
        let Some(table) = editor.document.body().blocks.into_iter().find_map(|block| match block {
            Block::Table(table) => Some(table),
            Block::Paragraph(_) => None,
        }) else {
            return Vec::new();
        };
        let words = |blocks: &[Block]| -> Vec<String> {
            blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Paragraph(paragraph) => Some(paragraph.plain_text()),
                    Block::Table(_) => None,
                })
                .collect()
        };
        table
            .rows
            .iter()
            .map(|row| row.cells.iter().map(|cell| words(&cell.blocks)).collect())
            .collect()
    }

    /// The same, written out by hand for a test to expect.
    fn cells(rows: &[&[&[&str]]]) -> Vec<Vec<Vec<String>>> {
        rows.iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.iter().map(|text| (*text).to_owned()).collect())
                    .collect()
            })
            .collect()
    }

    /// Whether every cell of every table has a paragraph and ends with one —
    /// a cell that does not is a file Word calls damaged.
    fn cells_are_whole(blocks: &[Block]) -> bool {
        blocks.iter().all(|block| match block {
            Block::Paragraph(_) => true,
            Block::Table(table) => table.rows.iter().flat_map(|row| &row.cells).all(|cell| {
                matches!(cell.blocks.last(), Some(Block::Paragraph(_)))
                    && cells_are_whole(&cell.blocks)
            }),
        })
    }

    /// Holds the document to that, as it is and as it opens again once saved.
    fn assert_whole(editor: &Editor) {
        assert!(
            cells_are_whole(&editor.document.body().blocks),
            "a cell has no paragraph: {:?}",
            shapes(editor)
        );
        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("the file does not open again");
        assert!(cells_are_whole(&reopened.body().blocks), "reopened, a cell has no paragraph");
    }

    /// The table filled in, with the caret at the start of one cell.
    fn filled_at(row: usize, column: usize) -> Editor {
        let mut editor = with_table();
        fill(&mut editor);
        caret_into(&mut editor, row, column);
        editor
    }

    #[test]
    fn shift_and_down_takes_the_cell_and_the_one_below_it() {
        // Word's Shift+Down in a table: the caret's cell and the one below,
        // and a row more at each press. It was a stretch of text from one
        // cell to the next row, through every cell between in the order the
        // file is written — drawn as two cells and acted on as five.
        let mut editor = filled_at(0, 0);
        key(&mut editor, Key::Down, true);
        let taken = editor.document.selected_cells().expect("a block of cells");
        assert_eq!((taken.rows, taken.columns), ((0, 1), (0, 0)));
        for row in 0..3 {
            for column in 0..3 {
                let wanted = column == 0 && row < 2;
                assert_eq!(
                    is_selected(&editor, row, column),
                    wanted,
                    "the cell at {row},{column} is {} the selection",
                    if wanted { "not in" } else { "in" }
                );
            }
        }

        key(&mut editor, Key::Down, true);
        let taken = editor.document.selected_cells().expect("a block of cells");
        assert_eq!((taken.rows, taken.columns), ((0, 2), (0, 0)), "it did not grow by a row");
        assert_eq!(editor.selected_cell_rects().len(), 3, "the column is not drawn");

        key(&mut editor, Key::Up, true);
        let taken = editor.document.selected_cells().expect("a block of cells");
        assert_eq!((taken.rows, taken.columns), ((0, 1), (0, 0)), "Shift+Up did not give it back");
    }

    #[test]
    fn delete_after_shift_and_down_empties_the_two_cells_and_nothing_else() {
        // It emptied the whole first row and took the only paragraph of the
        // second and third cells — a file Word calls damaged.
        let mut editor = filled_at(0, 0);
        key(&mut editor, Key::Down, true);
        key(&mut editor, Key::Delete, false);
        assert_eq!(
            shapes(&editor),
            cells(&[
                &[&[""], &["01"], &["02"]],
                &[&[""], &["11"], &["12"]],
                &[&["20"], &["21"], &["22"]],
            ])
        );
        assert_whole(&editor);
    }

    #[test]
    fn typing_after_shift_and_down_empties_the_cells_and_types_in_the_first() {
        let mut editor = filled_at(0, 0);
        key(&mut editor, Key::Down, true);
        typed(&mut editor, "X");
        assert_eq!(
            shapes(&editor),
            cells(&[
                &[&["X"], &["01"], &["02"]],
                &[&[""], &["11"], &["12"]],
                &[&["20"], &["21"], &["22"]],
            ])
        );
        assert_whole(&editor);
    }

    #[test]
    fn enter_after_shift_and_down_empties_the_cells_and_breaks_the_first() {
        let mut editor = filled_at(0, 0);
        key(&mut editor, Key::Down, true);
        key(&mut editor, Key::Enter, false);
        assert_eq!(
            shapes(&editor),
            cells(&[
                &[&["", ""], &["01"], &["02"]],
                &[&[""], &["11"], &["12"]],
                &[&["20"], &["21"], &["22"]],
            ])
        );
        assert_whole(&editor);
    }

    #[test]
    fn pasting_after_shift_and_down_empties_the_cells_and_pastes_into_the_first() {
        use crate::editor::paste::PasteAs;
        for how in [PasteAs::TextOnly, PasteAs::KeepSource] {
            let mut editor = filled_at(0, 0);
            key(&mut editor, Key::Down, true);
            let blocks = [Block::Paragraph(Paragraph::text("Z"))];
            editor.put_down("Z", &blocks, how);
            assert_eq!(
                shapes(&editor),
                cells(&[
                    &[&["Z"], &["01"], &["02"]],
                    &[&[""], &["11"], &["12"]],
                    &[&["20"], &["21"], &["22"]],
                ]),
                "pasted {how:?}"
            );
            assert_whole(&editor);
        }
    }

    #[test]
    fn bold_after_shift_and_down_twice_bolds_the_column_it_shows() {
        // It bolded everything from the second cell of the first row to the
        // first of the third: seven cells, while the drawing showed three.
        let mut editor = filled_at(0, 1);
        key(&mut editor, Key::Down, true);
        key(&mut editor, Key::Down, true);
        control(&mut editor, Key::Letter('b'), false);

        let bold: Vec<Vec<bool>> = (0..3)
            .map(|row| {
                (0..3)
                    .map(|column| {
                        let (first, _) =
                            editor.document.cell_paragraphs(row, column).expect("a cell");
                        editor.document.formatting_at(TextPosition::new(first, 1)).0.bold
                    })
                    .collect()
            })
            .collect();
        let column = vec![false, true, false];
        assert_eq!(bold, vec![column.clone(), column.clone(), column]);
    }

    #[test]
    fn shift_and_right_goes_through_the_text_and_then_takes_the_cell_beside() {
        // Inside the text of a cell, Shift+Right selects letters; the step
        // past the cell's end takes the cell and the next one, whole. It ran
        // into the next cell as text, and Delete then did nothing to it.
        let mut editor = filled_at(0, 0);
        key(&mut editor, Key::Right, true);
        key(&mut editor, Key::Right, true);
        assert!(editor.selected_cell_rects().is_empty(), "letters were taken as cells");
        assert_eq!(editor.document.selected_text(), "00");

        key(&mut editor, Key::Right, true);
        let taken = editor.document.selected_cells().expect("a block of cells");
        assert_eq!((taken.rows, taken.columns), ((0, 0), (0, 1)));
        assert_eq!(editor.selected_cell_rects().len(), 2, "the two cells are not drawn");
        key(&mut editor, Key::Delete, false);
        assert_eq!(
            shapes(&editor)[0],
            cells(&[&[&[""], &[""], &["02"]]])[0],
            "Delete did not empty the two cells"
        );
        assert_whole(&editor);

        // And the other way, from the start of a cell.
        let mut editor = filled_at(1, 2);
        key(&mut editor, Key::Left, true);
        let taken = editor.document.selected_cells().expect("a block of cells");
        assert_eq!((taken.rows, taken.columns), ((1, 1), (1, 2)), "Shift+Left did not take cells");
    }

    #[test]
    fn a_plain_arrow_or_escape_gives_up_the_block() {
        let mut editor = filled_at(0, 0);
        key(&mut editor, Key::Down, true);
        assert_eq!(editor.selected_cell_rects().len(), 2);
        key(&mut editor, Key::Escape, false);
        assert!(editor.selected_cell_rects().is_empty(), "Escape kept the block");

        key(&mut editor, Key::Down, true);
        key(&mut editor, Key::Left, false);
        assert!(editor.selected_cell_rects().is_empty(), "the arrow kept the block");
        assert!(editor.document.selection().is_none());
    }

    #[test]
    fn select_all_in_a_table_and_delete_takes_the_table_with_everything_else() {
        // Word's Ctrl+A selects the whole document from inside a table too,
        // and Delete leaves one empty paragraph. The table stayed, with not a
        // paragraph in any of its cells.
        let mut editor = filled_at(1, 1);
        control(&mut editor, Key::Letter('a'), false);
        key(&mut editor, Key::Delete, false);
        assert!(shapes(&editor).is_empty(), "the table stayed: {:?}", shapes(&editor));
        assert_eq!(editor.document.plain_text().trim(), "");
        assert_whole(&editor);
    }

    #[test]
    fn shift_ctrl_end_and_home_from_a_cell_take_whole_rows() {
        // Out of a table the selection is rows: from the caret's row to the
        // end of the table and on, or to its start and back.
        let mut editor = filled_at(1, 1);
        control(&mut editor, Key::End, true);
        key(&mut editor, Key::Delete, false);
        assert_eq!(shapes(&editor), cells(&[&[&["00"], &["01"], &["02"]]]));
        assert_whole(&editor);

        let mut editor = filled_at(1, 1);
        control(&mut editor, Key::Home, true);
        key(&mut editor, Key::Delete, false);
        assert_eq!(shapes(&editor), cells(&[&[&["20"], &["21"], &["22"]]]));
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some(""));
        assert_whole(&editor);
    }

    /// The paragraph after the table, with some words typed into it, and a
    /// point on its line.
    fn words_after_the_table(editor: &mut Editor) -> (usize, (i32, i32)) {
        let (_, last) = editor.document.table_paragraphs().expect("the table");
        let after = last + 1;
        editor.document.set_caret(TextPosition::new(after, 0));
        typed(editor, "After the table");
        editor.relayout();
        let line = editor.pages[0]
            .lines
            .iter()
            .find(|line| line.paragraph == after)
            .expect("a line after the table")
            .clone();
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + line.left + 30.0) as i32;
        let y = (top + (line.top() + line.bottom()) / 2.0) as i32;
        (after, (x, y))
    }

    #[test]
    fn a_drag_from_a_cell_out_past_the_table_takes_the_rows_and_the_paragraph() {
        // Word takes the rows from the one the drag began in to the end of
        // the table, and the words after it as far as the pointer. By way of
        // another cell first, too: the block it made there is given up for
        // the rows.
        for by_way_of_another_cell in [false, true] {
            let mut editor = filled_at(1, 0);
            let row_start = editor.document.caret();
            let (after, (to_x, to_y)) = words_after_the_table(&mut editor);
            // Cells are found through the table the caret is in.
            caret_into(&mut editor, 1, 1);

            let (x, y) = cell_middle(&editor, 1, 1);
            editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
            if by_way_of_another_cell {
                let (x, y) = cell_middle(&editor, 2, 2);
                editor.handle(Event::MouseMove {
                    x,
                    y,
                    held: true,
                    modifiers: Modifiers::default(),
                });
            }
            editor.handle(Event::MouseMove {
                x: to_x,
                y: to_y,
                held: true,
                modifiers: Modifiers::default(),
            });
            editor.handle(Event::MouseUp { x: to_x, y: to_y });

            let stretches = editor.document.selections();
            assert_eq!(stretches.len(), 1, "the rows and the words are one stretch");
            assert_eq!(stretches[0].0, row_start, "the row is not taken whole");
            assert_eq!(stretches[0].1.paragraph, after, "the words after it are not taken");
            assert_eq!(
                editor.selected_cell_rects().len(),
                6,
                "the two rows are not drawn as cells"
            );

            key(&mut editor, Key::Delete, false);
            assert_eq!(shapes(&editor), cells(&[&[&["00"], &["01"], &["02"]]]));
            assert_whole(&editor);
        }
    }

    #[test]
    fn shift_and_down_out_of_the_last_row_takes_the_rows_and_the_paragraph_after() {
        // From the middle row: the first press takes its cell and the one
        // below, the second leaves the table with both rows.
        let mut editor = filled_at(1, 0);
        let row_start = editor.document.caret();
        caret_into(&mut editor, 1, 1);
        key(&mut editor, Key::Down, true);
        key(&mut editor, Key::Down, true);
        assert!(editor.document.table_here().is_none(), "the caret did not leave the table");
        let (start, _) = editor.document.selection().expect("a selection");
        assert_eq!(start, row_start, "the rows are not taken from the first");
        assert_eq!(editor.selected_cell_rects().len(), 6, "the two rows are not drawn as cells");

        key(&mut editor, Key::Delete, false);
        assert_eq!(shapes(&editor), cells(&[&[&["00"], &["01"], &["02"]]]));
        assert_whole(&editor);

        // And from the text of a cell in the last row, in one press.
        let mut editor = filled_at(2, 0);
        let row_start = editor.document.caret();
        caret_into(&mut editor, 2, 1);
        key(&mut editor, Key::Down, true);
        let (start, _) = editor.document.selection().expect("a selection");
        assert_eq!(start, row_start, "the row is not taken from its first cell");
        assert_eq!(editor.selected_cell_rects().len(), 3, "the row is not drawn as cells");
        key(&mut editor, Key::Delete, false);
        assert_eq!(shapes(&editor).len(), 2, "the last row did not go");
        assert_whole(&editor);
    }
}

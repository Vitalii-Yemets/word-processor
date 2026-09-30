//! Joining and splitting the cells of a table.
//!
//! # Two different mechanisms, not one
//!
//! Joining cells across a row and joining them down a column are not the same
//! thing in the format, and a rectangle needs both:
//!
//! * Across, it is `w:gridSpan`: one cell says it covers several columns of the
//!   grid, and the cells it swallowed are removed from the row.
//! * Down, it is `w:vMerge`: every row keeps its cell, but all but the first say
//!   they continue the one above and hold nothing of their own.
//!
//! Getting these the wrong way round produces a table that looks right in this
//! program and wrong in Word, which is the worst kind of wrong.
//!
//! # Nothing is lost either way
//!
//! A merge keeps every paragraph of every cell it takes in, and a split shares
//! them out again. Both work on the grid rather than on the cells as they are
//! numbered along each row, because the grid is what lines the rows up: the
//! third cell of a row with a merged pair in it is under the fourth column, and
//! only the grid knows that. See [`Shape`].

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::{edit, position, read, Document, TextPosition};

/// The most columns a split may leave a table with, which is the most a table
/// may have in Word.
const MOST_COLUMNS: usize = 63;

/// The most rows one split may make of a cell, the most Insert Table makes.
const MOST_ROWS: usize = 100;

/// What Word's Split Cells dialog starts out saying about the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitDefaults {
    /// How many columns the cell is to become.
    pub columns: usize,
    /// And how many rows.
    pub rows: usize,
    /// Whether more than one cell is selected, which is when Word offers to
    /// merge them before splitting.
    pub several: bool,
}

/// One edge of one cell.
///
/// Named by where it is rather than by the element that holds it, because the
/// format's own names are inconsistent: a cell's left edge is `w:left` and a
/// paragraph's is `w:start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellEdge {
    Top,
    Bottom,
    Start,
    End,
}

impl CellEdge {
    /// What the element inside `w:tcBorders` is called.
    #[must_use]
    pub fn element(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Start => "left",
            Self::End => "right",
        }
    }
}

/// The rectangle of cells a selection covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellRange {
    pub table: Vec<usize>,
    /// First and last row, both included.
    pub rows: (usize, usize),
    /// First and last column, both included.
    pub columns: (usize, usize),
}

impl Document {
    /// Which cells the selection covers, when it is inside one table.
    ///
    /// Every stretch of it, and not only the one the caret is in: a block of
    /// cells taken by dragging across them is one stretch per cell, and the
    /// rectangle they make is what a command about it is about. Reaching for
    /// [`Document::selection`] here made Merge Cells merge the one cell the
    /// caret happened to be in.
    #[must_use]
    pub fn selected_cells(&self) -> Option<CellRange> {
        let here = self.table_here()?;

        // A block taken cell by cell says so itself, which is the only way an
        // empty one can be known. See [`Document::select_cell_block`].
        if let Some(range) = self.cell_block_now() {
            if range.table == here.table {
                return Some(range);
            }
        }

        let stretches = self.stretches_as_made();
        let (Some(&(start, _)), Some(&(_, end))) = (stretches.first(), stretches.last()) else {
            return Some(CellRange {
                table: here.table,
                rows: (here.row, here.row),
                columns: (here.column, here.column),
            });
        };

        // Both ends have to be in the same table, or there is no rectangle.
        let at_start = self.table_at(start.paragraph)?;
        let at_end = self.table_at(end.paragraph)?;
        if at_start.table != here.table || at_end.table != here.table {
            return None;
        }

        Some(CellRange {
            table: here.table,
            rows: (at_start.row.min(at_end.row), at_start.row.max(at_end.row)),
            columns: (at_start.column.min(at_end.column), at_start.column.max(at_end.column)),
        })
    }

    /// Says that a block of cells is what is selected.
    ///
    /// Called with the stretches of text in those cells already selected: this
    /// is the part of a cell selection that the stretches cannot express, which
    /// is which cells were taken when the cells are empty. It is believed only
    /// while the selection is still the one it was given with, so there is
    /// nothing to undo it with. See the `cell_block` field.
    pub fn select_cell_block(&mut self, range: CellRange) {
        self.cell_block = Some((range, self.anchor, self.caret));
    }

    /// The block of cells taken, if one was and the selection is still it.
    #[must_use]
    fn cell_block_now(&self) -> Option<CellRange> {
        let (range, anchor, caret) = self.cell_block.as_ref()?;
        (*anchor == self.anchor && *caret == self.caret).then(|| range.clone())
    }

    /// The block of cells the selection is, when it is more than one cell.
    ///
    /// A block taken cell by cell says so itself (see
    /// [`Document::select_cell_block`]); a stretch of text from one cell into
    /// another is the block its two ends make, as Word's selection is. Either
    /// way what is done to it is done to the cells: Delete empties them, and
    /// what is typed over them goes into the first.
    #[must_use]
    pub fn selected_block(&self) -> Option<CellRange> {
        if let Some(range) = self.cell_block_now() {
            if range.rows.0 != range.rows.1 || range.columns.0 != range.columns.1 {
                return Some(range);
            }
        }
        let anchor = self.anchor.filter(|anchor| *anchor != self.caret)?;
        let (start, end) =
            if anchor <= self.caret { (anchor, self.caret) } else { (self.caret, anchor) };
        let (from, to) = self.paths_of(start, end)?;
        self.cells_between(&from, &to)
    }

    /// Where the first cell of a block starts: where what is typed over the
    /// block goes, and where Word leaves the caret once it is emptied.
    #[must_use]
    pub(crate) fn block_start(&self, range: &CellRange) -> Option<TextPosition> {
        self.block_stretches(range).first().map(|(start, _)| *start)
    }

    /// What a stretch of text selects.
    ///
    /// # Not always the text between its ends
    ///
    /// Because the cells of a table are not written in the order they are
    /// read. A stretch from one cell into another runs, in the order of the
    /// file, through every cell between them along the rows — which are not
    /// the cells between them on the page. Word takes the rectangle its two
    /// ends make, and so does this: every cell of it whole, one stretch per
    /// cell, and nothing outside it. Taking the text between instead is what
    /// emptied cells the selection never showed of their only paragraph.
    ///
    /// A stretch that runs out of a table — from a cell on past the table's
    /// end, or into a cell from before its start — takes whole rows, as Word's
    /// does: its end in the table goes to the edge of its row. Whole rows are
    /// what can go from a table without leaving a cell with nothing in it.
    ///
    /// Every cell of a block is given, the empty ones as empty stretches: the
    /// first is where the block starts whatever it holds.
    pub(crate) fn as_selected(
        &self,
        start: TextPosition,
        end: TextPosition,
    ) -> Vec<(TextPosition, TextPosition)> {
        if start.paragraph == end.paragraph {
            return vec![(start, end)];
        }
        let Some((from, to)) = self.paths_of(start, end) else { return vec![(start, end)] };
        if let Some(range) = self.cells_between(&from, &to) {
            let cells = self.block_stretches(&range);
            if !cells.is_empty() {
                return cells;
            }
        }
        vec![self.rows_taken_whole(start, end, &from, &to)]
    }

    /// The paths to the paragraphs two places are in.
    fn paths_of(&self, start: TextPosition, end: TextPosition) -> Option<(Vec<usize>, Vec<usize>)> {
        let (from, to) =
            position::paragraph_paths(&self.tree().root, start.paragraph, end.paragraph);
        Some((from?, to?))
    }

    /// The rectangle of cells between two paragraphs, when they are in
    /// different cells of one table.
    ///
    /// Told by where their paths part: at the rows of a table, or at the cells
    /// of one row. Parting anywhere else — inside one cell, or outside every
    /// table — they are not in two cells of the same table, and there is no
    /// rectangle. A paragraph in a table inside a cell counts as being in
    /// that cell, which is what the table it is in is to the outer one.
    fn cells_between(&self, from: &[usize], to: &[usize]) -> Option<CellRange> {
        let root = &self.tree().root;
        let common = from.iter().zip(to).take_while(|(one, other)| one == other).count();
        let parting = edit::element_at_path(root, from.get(..common)?)?;
        let depth = if parting.is(Some(read::W), "tbl") {
            common
        } else if parting.is(Some(read::W), "tr") {
            common.checked_sub(1)?
        } else {
            return None;
        };
        let table = edit::element_at_path(root, &from[..depth])?;
        if !table.is(Some(read::W), "tbl") {
            return None;
        }
        // The row and the cell along it a path goes through.
        let place = |path: &[usize]| -> Option<(usize, usize)> {
            let (row_at, cell_at) = (*path.get(depth)?, *path.get(depth + 1)?);
            let row = table.children.get(row_at)?.as_element()?;
            let cell = row.children.get(cell_at)?.as_element()?;
            if !row.is(Some(read::W), "tr") || !cell.is(Some(read::W), "tc") {
                return None;
            }
            let rank = |element: &Element, local: &str, at: usize| {
                positions_of(element, local).iter().position(|position| *position == at)
            };
            Some((rank(table, "tr", row_at)?, rank(row, "tc", cell_at)?))
        };
        let (one, other) = (place(from)?, place(to)?);
        Some(CellRange {
            table: from[..depth].to_vec(),
            rows: (one.0.min(other.0), one.0.max(other.0)),
            columns: (one.1.min(other.1), one.1.max(other.1)),
        })
    }

    /// Every cell of a block, each as a stretch from the start of its first
    /// paragraph to the end of its last, in reading order.
    ///
    /// The block is grown until no merged cell is cut by its edge, as Word's
    /// selection is (see [`Shape`]), and a cell of it that holds nothing is an
    /// empty stretch at its one paragraph.
    fn block_stretches(&self, range: &CellRange) -> Vec<(TextPosition, TextPosition)> {
        let root = &self.tree().root;
        let Some(table) = edit::element_at_path(root, &range.table) else { return Vec::new() };
        let shape = Shape::of(table);
        let Some(block) = shape.block_of(range) else { return Vec::new() };
        let Some((first, _)) = crate::tables::paragraphs_under(root, &range.table) else {
            return Vec::new();
        };
        let spans = cell_spans(table, first);

        let mut out = Vec::new();
        for row in block.rows.0..=block.rows.1 {
            for column in 0..shape.rows[row].len() {
                if !shape.overlaps(row, column, &block) {
                    continue;
                }
                if let Some(Some((first, last, length))) =
                    spans.get(row).and_then(|row| row.get(column))
                {
                    out.push((TextPosition::new(*first, 0), TextPosition::new(*last, *length)));
                }
            }
        }
        out
    }

    /// A stretch that runs out of a table, grown to the edges of the rows it
    /// takes there.
    ///
    /// Its start goes back to the start of its row in the outermost table it
    /// is in and its end is not, and its end on to the end of its row in the
    /// outermost table it is in and its start is not. A stretch that leaves
    /// no table is what it was.
    fn rows_taken_whole(
        &self,
        start: TextPosition,
        end: TextPosition,
        from: &[usize],
        to: &[usize],
    ) -> (TextPosition, TextPosition) {
        let root = &self.tree().root;
        // The depth of the outermost table a path is in and another is not.
        let left = |path: &[usize], other: &[usize]| {
            (0..path.len()).find(|&depth| {
                !other.starts_with(&path[..depth])
                    && edit::element_at_path(root, &path[..depth])
                        .is_some_and(|element| element.is(Some(read::W), "tbl"))
            })
        };

        let mut start = start;
        if let Some(depth) = left(from, to) {
            if let Some((first, _)) = crate::tables::paragraphs_under(root, &from[..=depth]) {
                start = TextPosition::new(first, 0);
            }
        }
        let mut end = end;
        if let Some(depth) = left(to, from) {
            if let Some((_, last)) = crate::tables::paragraphs_under(root, &to[..=depth]) {
                end =
                    TextPosition::new(last, self.paragraph_text(last).map_or(0, |text| text.len()));
            }
        }
        (start, end)
    }

    /// The rows of tables the selection takes whole by running out of them,
    /// each as its first and last paragraph.
    ///
    /// What is drawn as cells, the way Word draws them, while the rest of such
    /// a selection is drawn as text. A row of the table both ends of a stretch
    /// are in is not one: that is a block of cells, or text inside one.
    #[must_use]
    pub fn whole_rows_selected(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (start, end) in self.selections() {
            if start.paragraph == end.paragraph {
                continue;
            }
            let length = self.paragraph_text(end.paragraph).map_or(0, |text| text.len());
            let stretch = Stretch { start, end, end_whole: end.offset >= length };
            gather_rows(&self.tree().root, &mut 0, &stretch, true, &mut out);
        }
        out
    }

    /// Puts a line on one edge of the cell a place in the document is inside,
    /// or takes it off.
    ///
    /// What Word's Border Painter does. The pen is dragged along an edge and
    /// the line it carries lands on that edge of that cell and nowhere else —
    /// which is why this is one edge of one cell rather than the whole table's
    /// borders, the only thing that could be said before.
    pub fn set_cell_edge(
        &mut self,
        at: TextPosition,
        edge: CellEdge,
        border: Option<&crate::model::Border>,
    ) -> bool {
        let Some(place) = self.table_at(at.paragraph) else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &place.table)
        else {
            return false;
        };
        let Some(row_position) = positions_of(table, "tr").get(place.row).copied() else {
            return false;
        };
        let Some(row) = table.children.get_mut(row_position).and_then(Node::as_element_mut) else {
            return false;
        };
        let Some(cell_position) = positions_of(row, "tc").get(place.column).copied() else {
            return false;
        };
        let Some(cell) = row.children.get_mut(cell_position).and_then(Node::as_element_mut) else {
            return false;
        };

        let properties = properties_of(cell, prefix.as_deref());
        // The set of lines is one element holding one child per edge, so the
        // one being changed is taken out and put back rather than the whole
        // set being rewritten: the other three edges are nobody's business
        // here.
        if properties.child(Some(read::W), "tcBorders").is_none() {
            edit::insert_ordered(
                properties,
                Element::new(&edit::name_with(prefix.as_deref(), "tcBorders"), Some(read::W)),
                CELL_PROPERTY_ORDER,
            );
        }
        let Some(borders) = properties.child_mut(Some(read::W), "tcBorders") else { return false };
        borders.remove_children_named(Some(read::W), edge.element());
        if let Some(border) = border {
            borders.push_element(edit::border_element(edge.element(), border, prefix.as_deref()));
        }

        self.note_change();
        true
    }

    /// Joins the selected cells into one.
    ///
    /// # What happens to what was in them
    ///
    /// Word keeps it. The paragraphs of every cell go into the merged one in
    /// the order a person reads the cells — along the first row, then along
    /// the next — and a cell with nothing in it adds nothing. Across a row the
    /// cells to the right come out of it, and they used to take their text
    /// with them; down a column the cells below stay, as continuations, and
    /// what a continuation holds is drawn by nobody — Word included. So both
    /// give theirs up, and each continuation is left with the one empty
    /// paragraph a cell must have, which is how Word writes one.
    ///
    /// The selection is taken whole: a merged cell it cuts through is part of
    /// it, as Word's selection is.
    ///
    /// # Where the caret goes
    ///
    /// To the start of the merged cell, as in Word. Left where it was, it
    /// stood on a paragraph number that now belonged to the next cell along —
    /// or, with the whole table merged, to the paragraph after the table,
    /// which took the table's two tabs away with it.
    pub fn merge_cells(&mut self) -> bool {
        let Some(range) = self.selected_cells() else { return false };
        let Some(table) = edit::element_at_path(&self.tree().root, &range.table) else {
            return false;
        };
        let shape = Shape::of(table);
        let Some(block) = shape.block_of(&range) else { return false };
        if shape.heads_in(&block).len() < 2 || !shape.tiles(&block) {
            return false;
        }
        let Some(head) = shape.column_starting(block.rows.0, block.columns.0) else {
            return false;
        };

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let Some(table) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &range.table)
        else {
            return false;
        };
        merge_block(table, &shape, &block, prefix.as_deref());

        self.caret_into_cell(&range.table, block.rows.0, head);
        self.note_change();
        true
    }

    /// Draws a line down through one cell, making two cells of it.
    ///
    /// What Word's Draw Table does when a line is drawn from the top of a cell
    /// to the bottom of it. The table gains a column of the grid, the cell
    /// becomes two, and every other row's cell that crossed the new line covers
    /// one column more than it did — so nothing but this cell looks any
    /// different.
    pub fn split_cell_across(&mut self, at: TextPosition) -> bool {
        let Some(place) = self.table_at(at.paragraph) else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &place.table)
        else {
            return false;
        };

        for (number, row_position) in positions_of(table, "tr").into_iter().enumerate() {
            let Some(row) = table.children.get_mut(row_position).and_then(Node::as_element_mut)
            else {
                continue;
            };
            let Some(cell_position) = positions_of(row, "tc").get(place.column).copied() else {
                continue;
            };

            if number == place.row {
                // The cell the line was drawn through becomes two.
                let Some(model) =
                    row.children.get(cell_position).and_then(Node::as_element).cloned()
                else {
                    continue;
                };
                let fresh = empty_cell(&model, prefix.as_deref());
                row.insert_element(cell_position + 1, fresh);
                continue;
            }

            // Every other row's cell simply covers one column more.
            let Some(cell) = row.children.get_mut(cell_position).and_then(Node::as_element_mut)
            else {
                continue;
            };
            let span = span_of(cell) + 1;
            let properties = properties_of(cell, prefix.as_deref());
            properties.remove_children_named(Some(read::W), "gridSpan");
            properties_insert(properties, prefix.as_deref(), "gridSpan", Some(&span.to_string()));
        }

        crate::tables::widen_grid(table, prefix.as_deref(), place.column);
        self.note_change();
        true
    }

    /// Draws a line across through one cell, making two rows of it.
    ///
    /// The other half of Word's Draw Table. A table has no way to say that one
    /// cell is two rows tall while its neighbours are one, so the table gains a
    /// whole row and every other column is merged down across the two — which
    /// is how Word writes it, and why a table drawn this way is full of
    /// `w:vMerge`.
    pub fn split_cell_down(&mut self, at: TextPosition) -> bool {
        let Some(place) = self.table_at(at.paragraph) else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &place.table)
        else {
            return false;
        };
        let Some(row_position) = positions_of(table, "tr").get(place.row).copied() else {
            return false;
        };
        let Some(row) = table.children.get_mut(row_position).and_then(Node::as_element_mut) else {
            return false;
        };

        // The row below: the split cell's other half, and a continuation of
        // every cell beside it.
        let mut below = Element::new(&edit::name_with(prefix.as_deref(), "tr"), Some(read::W));
        let cells = positions_of(row, "tc");
        for (number, position) in cells.iter().copied().enumerate() {
            let Some(model) = row.children.get(position).and_then(Node::as_element).cloned() else {
                continue;
            };
            let mut fresh = empty_cell(&model, prefix.as_deref());
            if number != place.column {
                let properties = properties_of(&mut fresh, prefix.as_deref());
                properties.remove_children_named(Some(read::W), "vMerge");
                properties_insert(properties, prefix.as_deref(), "vMerge", None);
            }
            below.push_element(fresh);
        }

        // And the row above: every cell but the split one now starts a merge.
        for (number, position) in cells.into_iter().enumerate() {
            if number == place.column {
                continue;
            }
            let Some(cell) = row.children.get_mut(position).and_then(Node::as_element_mut) else {
                continue;
            };
            let properties = properties_of(cell, prefix.as_deref());
            properties.remove_children_named(Some(read::W), "vMerge");
            properties_insert(properties, prefix.as_deref(), "vMerge", Some("restart"));
        }

        table.insert_element(row_position + 1, below);
        self.note_change();
        true
    }

    /// Rubs out the line between two cells, making one cell of them.
    ///
    /// Word's Eraser. The two cells are named by a place inside each, which is
    /// what the pointer can say: it is over an edge, and there is a cell either
    /// side of it.
    pub fn erase_between(&mut self, one: TextPosition, other: TextPosition) -> bool {
        let Some(here) = self.table_at(one.paragraph) else { return false };
        let Some(there) = self.table_at(other.paragraph) else { return false };
        if here.table != there.table {
            return false;
        }

        // Merging works on what is selected, so the two cells are selected and
        // then merged: one mechanism rather than two that have to agree.
        let kept = (self.caret(), self.selections());
        self.set_caret(one);
        self.extend_selection_to(other);
        let merged = self.merge_cells();
        if !merged {
            self.set_caret(kept.0);
        }
        merged
    }

    /// What Word's Split Cells dialog starts out saying about the selection.
    ///
    /// For one cell, as many columns and rows as it covers, so that OK on its
    /// own takes a merge apart again — and two columns and one row for a cell
    /// that covers one of each, since one by one is no split at all, and two
    /// by one is what Word offers there. For several cells, the rectangle they
    /// make, and the offer to merge them first.
    #[must_use]
    pub fn split_defaults(&self) -> Option<SplitDefaults> {
        let range = self.selected_cells()?;
        let table = edit::element_at_path(&self.tree().root, &range.table)?;
        let shape = Shape::of(table);
        let block = shape.block_of(&range)?;
        let heads = shape.heads_in(&block);
        let &(row, column) = heads.first()?;
        if heads.len() > 1 {
            return Some(SplitDefaults {
                columns: block.columns.1 - block.columns.0,
                rows: block.rows.1 - block.rows.0 + 1,
                several: true,
            });
        }
        let span = shape.slot(row, column)?.span;
        let tall = shape.height_of(row, column);
        let columns = if span == 1 && tall == 1 { 2 } else { span };
        Some(SplitDefaults { columns, rows: tall, several: false })
    }

    /// Splits the selected cells into columns and rows: Word's Split Cells.
    ///
    /// Any cell, and not only one that was merged. Split across, a cell's
    /// width is shared out evenly between the new cells — or, where it covers
    /// as many columns of the grid as it is split into, it is given those
    /// columns back — and the grid gains whatever new columns that takes; a
    /// cell above or below that crossed one of them covers both halves, so
    /// nothing but the split cell looks any different. Split down, a cell of
    /// one row gets rows added under it, and every other cell of the row is
    /// merged down across them; a cell already merged down over several rows
    /// is shared out over those rows instead, which is why the rows it is
    /// split into have to divide them — Word's rule too.
    ///
    /// Its paragraphs are shared out over the new cells in reading order, one
    /// each where there are fewer than cells and the first cells taking one
    /// more where they do not go evenly. That is what makes a split undo a
    /// merge: three cells merged are one cell of three paragraphs, and split
    /// into three again each gets its own back.
    ///
    /// With `merge_first` and several cells selected they are merged first,
    /// which is Word's "Merge cells before split"; without it, each selected
    /// cell is split on its own. Either way it is one step to undo.
    pub fn split_cells(&mut self, columns: usize, rows: usize, merge_first: bool) -> bool {
        if !(1..=MOST_COLUMNS).contains(&columns)
            || !(1..=MOST_ROWS).contains(&rows)
            || (columns == 1 && rows == 1)
        {
            return false;
        }
        let Some(range) = self.selected_cells() else { return false };
        let Some(table) = edit::element_at_path(&self.tree().root, &range.table) else {
            return false;
        };
        let shape = Shape::of(table);
        let Some(block) = shape.block_of(&range) else { return false };
        let heads = shape.heads_in(&block);
        let Some(&(first_row, first_column)) = heads.first() else { return false };

        self.begin_gesture();
        let done = if merge_first && heads.len() > 1 {
            // Asked before anything is merged, so that a count the merged
            // cell could not be split into leaves the table as it was.
            fits(block.rows.1 - block.rows.0 + 1, rows)
                && self.merge_cells()
                && self.table_here().is_some_and(|merged| {
                    self.split_one(&range.table, merged.row, merged.column, columns, rows)
                })
        } else {
            // From the last cell back to the first: splitting one adds rows
            // below it and cells to its right, and neither moves a cell that
            // comes before it in the table.
            let mut done = false;
            for &(row, column) in heads.iter().rev() {
                done |= self.split_one(&range.table, row, column, columns, rows);
            }
            if done {
                self.caret_into_cell(&range.table, first_row, first_column);
            }
            done
        };
        self.end_gesture();
        done
    }

    /// Splits one cell of a table into columns and rows. See
    /// [`Document::split_cells`].
    fn split_one(
        &mut self,
        table_path: &[usize],
        row: usize,
        column: usize,
        columns: usize,
        rows: usize,
    ) -> bool {
        let Some(table) = edit::element_at_path(&self.tree().root, table_path) else {
            return false;
        };
        let shape = Shape::of(table);
        let (row, column) = shape.head_of(row, column);
        let Some(slot) = shape.slot(row, column) else { return false };
        let tall = shape.height_of(row, column);
        if !fits(tall, rows) {
            return false;
        }
        let Some(grid) = Grid::split(&shape, slot, columns) else { return false };

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let Some(table) = edit::element_at_path_mut(&mut self.tree_to_edit().root, table_path)
        else {
            return false;
        };
        split_in_place(table, &shape, &grid, (row, column), tall, rows, prefix.as_deref());

        // The first of the new cells keeps its place in its row, so it is
        // named the way the cell it came from was.
        self.caret_into_cell(table_path, row, column);
        self.note_change();
        true
    }

    /// Gives the selected columns the same width as one another.
    ///
    /// Word's Distribute Columns. With nothing selected, or with the whole
    /// table selected, every column of it; with a block selected, the columns
    /// that block covers — and the room they shared between them stays theirs,
    /// so the rest of the table does not move.
    ///
    /// # Why the cells are written and not only the grid
    ///
    /// Because a column's width is written twice over: once in the table's grid
    /// and once in every cell of that column. Writing the grid alone left the
    /// cells still asking for what they asked for before, and a cell's own
    /// width is what the layout believes — so the button did nothing at all to
    /// look at. See [`Document::set_table_grid`], which is what writes both.
    pub fn distribute_columns(&mut self) -> bool {
        let Some(place) = self.table_here() else { return false };
        let mut widths = self.table_grid_at(self.caret().paragraph);
        if widths.is_empty() {
            // A table that states no grid: the columns are even shares of the
            // text width, which is what evening them out comes to anyway.
            widths = vec![9360 / place.columns.max(1) as i32; place.columns.max(1)];
        }

        let (first, last) = self
            .selected_cells()
            .filter(|range| range.columns.1 > range.columns.0)
            .map_or((0, widths.len().saturating_sub(1)), |range| range.columns);
        let last = last.min(widths.len().saturating_sub(1));
        if last <= first {
            return false;
        }

        let total: i32 = widths[first..=last].iter().sum();
        let count = (last - first + 1) as i32;
        let each = (total / count).max(1);
        for width in &mut widths[first..=last] {
            *width = each;
        }
        // The remainder goes on the first of them, so the table keeps the
        // width it had to the twentieth of a point.
        widths[first] += total - each * count;

        self.set_table_grid(&widths)
    }
}

/// The paragraphs of every cell of a table, row by row: the first and the
/// last, and how long the last is — or nothing, for a cell with no paragraph.
///
/// One walk of the table, counting on from the number of its first paragraph,
/// rather than one walk of the document per cell: a block of cells is asked
/// for every time the selection is drawn.
fn cell_spans(table: &Element, first: usize) -> Vec<Vec<Option<(usize, usize, usize)>>> {
    let mut counter = first;
    let mut rows = Vec::new();
    for child in table.child_elements() {
        if !child.is(Some(read::W), "tr") {
            counter += position::paragraphs_in(child);
            continue;
        }
        let mut cells = Vec::new();
        for inside in child.child_elements() {
            if !inside.is(Some(read::W), "tc") {
                counter += position::paragraphs_in(inside);
                continue;
            }
            let paragraphs = position::paragraphs(inside);
            cells.push(paragraphs.last().map(|last| {
                let length = position::paragraph_text(last).len();
                (counter, counter + paragraphs.len() - 1, length)
            }));
            counter += paragraphs.len();
        }
        rows.push(cells);
    }
    rows
}

/// A stretch of the selection, and whether its end takes the whole of the
/// paragraph it is in.
struct Stretch {
    start: TextPosition,
    end: TextPosition,
    end_whole: bool,
}

impl Stretch {
    /// Whether every paragraph from one to another is inside it, whole.
    fn covers(&self, first: usize, last: usize) -> bool {
        (first > self.start.paragraph || (first == self.start.paragraph && self.start.offset == 0))
            && (last < self.end.paragraph || (last == self.end.paragraph && self.end_whole))
    }
}

/// Finds the rows a stretch takes whole under an element, counting paragraphs
/// as it goes. See [`Document::whole_rows_selected`].
///
/// `holds_both` says whether the table the element is in holds both ends of
/// the stretch, in which case its rows are not taken whole by it.
fn gather_rows(
    element: &Element,
    counter: &mut usize,
    stretch: &Stretch,
    holds_both: bool,
    out: &mut Vec<(usize, usize)>,
) {
    for child in element.child_elements() {
        let count = position::paragraphs_in(child);
        if count == 0 {
            continue;
        }
        let (first, last) = (*counter, *counter + count - 1);
        // Everything from here on is past the stretch.
        if first > stretch.end.paragraph {
            return;
        }
        if child.is(Some(read::W), "p") || last < stretch.start.paragraph {
            *counter += count;
            continue;
        }
        if child.is(Some(read::W), "tr") && !holds_both && stretch.covers(first, last) {
            out.push((first, last));
            *counter += count;
            continue;
        }
        let holds = if child.is(Some(read::W), "tbl") {
            first <= stretch.start.paragraph && stretch.end.paragraph <= last
        } else {
            holds_both
        };
        gather_rows(child, counter, stretch, holds, out);
    }
}

/// Mends the merges down a table after rows were taken out of it.
///
/// A cell that continued a merge whose first row went is where the merge
/// starts now — or a cell of its own, when nothing below it continues it.
/// Left as it was, it would go on saying it continues a cell that is not
/// there.
pub(crate) fn mend_merges(table: &mut Element, prefix: Option<&str>) {
    let shape = Shape::of(table);
    // Whether a cell of a row continues one of the same columns above it.
    let continues = |row: usize, slot: Slot, above: bool| {
        let other = if above { row.checked_sub(1) } else { Some(row + 1) };
        other
            .and_then(|other| Some((other, shape.covering(other, slot.start)?)))
            .and_then(|(other, column)| shape.slot(other, column))
            .is_some_and(|found| {
                found.start == slot.start
                    && found.span == slot.span
                    && if above { found.down != Down::Alone } else { found.down == Down::Continues }
            })
    };
    for (number, row_at) in positions_of(table, "tr").into_iter().enumerate() {
        let Some(row) = table.children.get_mut(row_at).and_then(Node::as_element_mut) else {
            continue;
        };
        for (column, cell_at) in positions_of(row, "tc").into_iter().enumerate() {
            let Some(slot) = shape.slot(number, column) else { continue };
            if slot.down != Down::Continues || continues(number, slot, true) {
                continue;
            }
            let down = if continues(number, slot, false) { Down::Starts } else { Down::Alone };
            if let Some(cell) = row.children.get_mut(cell_at).and_then(Node::as_element_mut) {
                set_down(properties_of(cell, prefix), down, prefix);
            }
        }
    }
}

/// The order the schema requires for the children of `w:tcPr`.
const CELL_PROPERTY_ORDER: &[&str] = &[
    "cnfStyle",
    "tcW",
    "gridSpan",
    "hMerge",
    "vMerge",
    "tcBorders",
    "shd",
    "noWrap",
    "tcMar",
    "textDirection",
    "tcFitText",
    "vAlign",
    "hideMark",
];

/// The `w:tcPr` of a cell, made if it is not there.
fn properties_of<'a>(cell: &'a mut Element, prefix: Option<&str>) -> &'a mut Element {
    if cell.child(Some(read::W), "tcPr").is_none() {
        // It is the first child of a cell, which the schema requires.
        cell.insert_element(0, Element::new(&edit::name_with(prefix, "tcPr"), Some(read::W)));
    }
    cell.child_mut(Some(read::W), "tcPr").expect("just inserted, or already there")
}

/// Puts a property into a `w:tcPr` where the schema says it goes.
fn properties_insert(
    properties: &mut Element,
    prefix: Option<&str>,
    local: &str,
    value: Option<&str>,
) {
    let mut element = Element::new(&edit::name_with(prefix, local), Some(read::W));
    if let Some(value) = value {
        element.set_namespaced_attribute(&edit::name_with(prefix, "val"), read::W, value);
    }
    edit::insert_ordered(properties, element, CELL_PROPERTY_ORDER);
}

/// How many columns of the grid a cell covers.
fn span_of(cell: &Element) -> u32 {
    cell.child(Some(read::W), "tcPr")
        .and_then(|properties| properties.child(Some(read::W), "gridSpan"))
        .and_then(|span| span.attribute(Some(read::W), "val"))
        .and_then(|text| text.parse().ok())
        .unwrap_or(1)
        .max(1)
}

/// A copy of a cell holding one empty paragraph and no merge of its own.
fn empty_cell(model: &Element, prefix: Option<&str>) -> Element {
    let mut cell = Element::new(&edit::name_with(prefix, "tc"), Some(read::W));
    cell.attributes = model.attributes.clone();

    if let Some(properties) = model.child(Some(read::W), "tcPr") {
        let mut copied = properties.clone();
        copied.remove_children_named(Some(read::W), "gridSpan");
        copied.remove_children_named(Some(read::W), "vMerge");
        cell.push_element(copied);
    }
    cell.push_element(edit::paragraph_element(&crate::model::Paragraph::default(), prefix));
    cell
}

/// Where each named child of an element sits among all its children.
fn positions_of(element: &Element, local: &str) -> Vec<usize> {
    element
        .children
        .iter()
        .enumerate()
        .filter(|(_, node)| node.as_element().is_some_and(|child| child.is(Some(read::W), local)))
        .map(|(index, _)| index)
        .collect()
}

/// Where the nth named child sits.
fn position_of(element: &Element, local: &str, nth: usize) -> Option<usize> {
    positions_of(element, local).get(nth).copied()
}

/// Whether a cell merged down over `tall` rows can be split into `rows`.
///
/// A cell of one row can be split into any number, which adds rows to the
/// table. A cell merged over several is shared out over the rows it already
/// covers, so the rows asked for have to divide them evenly — which is the
/// rule Word keeps as well.
fn fits(tall: usize, rows: usize) -> bool {
    tall == 1 || (rows <= tall && tall % rows == 0)
}

/// How a cell takes part in a merge down its column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Down {
    /// A cell of its own.
    Alone,
    /// The first of a merge down, which is the cell that is drawn.
    Starts,
    /// One of the rest: in the file, and not on the page.
    Continues,
}

/// One cell of a row, placed on the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slot {
    /// The first column of the grid it covers.
    start: usize,
    /// How many it covers.
    span: usize,
    down: Down,
}

impl Slot {
    /// The column of the grid just past it.
    fn end(self) -> usize {
        self.start + self.span
    }
}

/// A rectangle of the grid: rows with both ends included, and columns of the
/// grid from the first up to but not including the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Block {
    rows: (usize, usize),
    columns: (usize, usize),
}

/// The cells of a table as they sit on its grid.
///
/// A cell is named by its row and by where it comes in that row, which is how
/// the rest of the program names it — and which says nothing about where it
/// is across the table once a row has a merged cell in it. This says, and it
/// is what everything that has to line rows up asks: a merge, a split, and
/// Tab and the arrows, which must pass over what a merge hid.
pub(crate) struct Shape {
    rows: Vec<Vec<Slot>>,
    /// How wide each column of the grid is, in twentieths of a point — as
    /// many as the widest row needs, whatever the grid says.
    widths: Vec<i32>,
    /// Whether the table states a grid of its own. One that does not is left
    /// without one, and its cells' widths are left as they are.
    stated: bool,
}

impl Shape {
    /// Reads a `w:tbl`.
    pub(crate) fn of(table: &Element) -> Self {
        let rows: Vec<Vec<Slot>> = table
            .children_named(Some(read::W), "tr")
            .map(|row| {
                // A row may leave columns empty before its first cell.
                let mut at = row_count(row, "gridBefore");
                row.children_named(Some(read::W), "tc")
                    .map(|cell| {
                        let slot =
                            Slot { start: at, span: span_of(cell) as usize, down: down_of(cell) };
                        at += slot.span;
                        slot
                    })
                    .collect()
            })
            .collect();
        let needed = rows.iter().filter_map(|row| row.last()).map(|slot| slot.end()).max();

        let stated: Vec<Option<i32>> = table
            .child(Some(read::W), "tblGrid")
            .map(|grid| {
                grid.children_named(Some(read::W), "gridCol")
                    .map(|column| {
                        column
                            .attribute(Some(read::W), "w")
                            .and_then(|text| text.parse().ok())
                            .filter(|width: &i32| *width > 0)
                    })
                    .collect()
            })
            .unwrap_or_default();
        // A column the grid gives no width is given the average of the ones
        // it does, and a row running past the grid gets more of the last:
        // the widths are only ever used to find where edges fall, and an edge
        // has to fall somewhere.
        let known: Vec<i32> = stated.iter().flatten().copied().collect();
        let usual = if known.is_empty() {
            1440
        } else {
            (known.iter().sum::<i32>() / known.len() as i32).max(1)
        };
        let mut widths: Vec<i32> = stated.iter().map(|width| width.unwrap_or(usual)).collect();
        let last = widths.last().copied().unwrap_or(usual);
        widths.resize(widths.len().max(needed.unwrap_or(0)), last);

        Self { rows, widths, stated: !stated.is_empty() }
    }

    /// One cell.
    fn slot(&self, row: usize, column: usize) -> Option<Slot> {
        self.rows.get(row)?.get(column).copied()
    }

    /// The first column of the grid a cell covers.
    pub(crate) fn start_of(&self, row: usize, column: usize) -> Option<usize> {
        Some(self.slot(row, column)?.start)
    }

    /// Whether a cell continues a merge down from the row above, and so is
    /// in the file and not on the page.
    pub(crate) fn hidden(&self, row: usize, column: usize) -> bool {
        row > 0 && self.slot(row, column).is_some_and(|slot| slot.down == Down::Continues)
    }

    /// The cell of a row that covers a column of the grid.
    fn covering(&self, row: usize, grid_column: usize) -> Option<usize> {
        self.rows
            .get(row)?
            .iter()
            .position(|slot| slot.start <= grid_column && grid_column < slot.end())
    }

    /// The same, or the cell nearest it where the row stops short of it.
    pub(crate) fn nearest(&self, row: usize, grid_column: usize) -> Option<usize> {
        let cells = self.rows.get(row)?;
        self.covering(row, grid_column)
            .or_else(|| cells.iter().rposition(|slot| slot.start <= grid_column))
            .or_else(|| (!cells.is_empty()).then_some(0))
    }

    /// The cell that starts at a column of the grid in a row.
    fn column_starting(&self, row: usize, grid_column: usize) -> Option<usize> {
        self.rows.get(row)?.iter().position(|slot| slot.start == grid_column)
    }

    /// The cell that is drawn for a place in the table: the place itself, or
    /// the first cell of the merge down it continues.
    pub(crate) fn head_of(&self, row: usize, column: usize) -> (usize, usize) {
        let (mut row, mut column) = (row, column);
        while self.hidden(row, column) {
            let Some(start) = self.start_of(row, column) else { break };
            let Some(above) = self.covering(row - 1, start) else { break };
            row -= 1;
            column = above;
        }
        (row, column)
    }

    /// How many rows a cell covers: one, or as many as its merge down runs
    /// over.
    pub(crate) fn height_of(&self, row: usize, column: usize) -> usize {
        let Some(start) = self.start_of(row, column) else { return 1 };
        let mut height = 1;
        while let Some(below) = self.covering(row + height, start) {
            if self.start_of(row + height, below) != Some(start)
                || !self.hidden(row + height, below)
            {
                break;
            }
            height += 1;
        }
        height
    }

    /// Every cell that is drawn, in the order Tab visits them: along each
    /// row, one row after another.
    pub(crate) fn drawn(&self) -> Vec<(usize, usize)> {
        (0..self.rows.len())
            .flat_map(|row| (0..self.rows[row].len()).map(move |column| (row, column)))
            .filter(|&(row, column)| !self.hidden(row, column))
            .collect()
    }

    /// Whether a cell lies inside a rectangle of the grid, even in part.
    fn overlaps(&self, row: usize, column: usize, block: &Block) -> bool {
        (block.rows.0..=block.rows.1).contains(&row)
            && self
                .slot(row, column)
                .is_some_and(|slot| slot.start < block.columns.1 && slot.end() > block.columns.0)
    }

    /// The rectangle of the grid a selection covers.
    ///
    /// The selection names cells by row and by place in the row, and the
    /// rectangle is every column of the grid those cells cover. It is grown
    /// until no cell is cut by its edge — a cell covering two columns is
    /// taken whole, and so is a cell merged down, from its first row to its
    /// last — which is what Word's own selection does.
    fn block_of(&self, range: &CellRange) -> Option<Block> {
        let last_row = self.rows.len().checked_sub(1)?;
        let rows = (range.rows.0.min(last_row), range.rows.1.min(last_row));
        let mut columns = (usize::MAX, 0usize);
        for row in rows.0..=rows.1 {
            for column in range.columns.0..=range.columns.1 {
                if let Some(slot) = self.slot(row, column) {
                    columns = (columns.0.min(slot.start), columns.1.max(slot.end()));
                }
            }
        }
        if columns.0 >= columns.1 {
            return None;
        }

        let mut block = Block { rows, columns };
        loop {
            let before = block;
            for row in block.rows.0..=block.rows.1 {
                for column in 0..self.rows[row].len() {
                    if !self.overlaps(row, column, &block) {
                        continue;
                    }
                    let Some(slot) = self.slot(row, column) else { continue };
                    block.columns =
                        (block.columns.0.min(slot.start), block.columns.1.max(slot.end()));
                    let (head_row, head_column) = self.head_of(row, column);
                    let bottom = head_row + self.height_of(head_row, head_column) - 1;
                    block.rows = (block.rows.0.min(head_row), block.rows.1.max(bottom));
                }
            }
            if block == before {
                return Some(block);
            }
        }
    }

    /// The cells of a rectangle that are drawn, in reading order.
    fn heads_in(&self, block: &Block) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for row in block.rows.0..=block.rows.1 {
            for column in 0..self.rows[row].len() {
                if self.overlaps(row, column, block) && !self.hidden(row, column) {
                    out.push((row, column));
                }
            }
        }
        out
    }

    /// Whether every row of a rectangle has cells from its left edge to its
    /// right, so that one cell can take their place in each.
    ///
    /// A row that leaves columns empty at its start or its end can have a
    /// hole where the rectangle is, and a merged cell cannot be made over a
    /// hole.
    fn tiles(&self, block: &Block) -> bool {
        (block.rows.0..=block.rows.1).all(|row| {
            let inside: Vec<Slot> = (0..self.rows[row].len())
                .filter(|&column| self.overlaps(row, column, block))
                .filter_map(|column| self.slot(row, column))
                .collect();
            inside.first().is_some_and(|first| first.start == block.columns.0)
                && inside.last().is_some_and(|last| last.end() == block.columns.1)
        })
    }
}

/// The grid after a cell is split across.
///
/// Kept as the edges of the columns rather than as their widths: splitting
/// adds edges, and every other cell of the table keeps the edges it had —
/// which is how the cells above and below a split one come to cover both of
/// its halves without a width anywhere being worked out again.
struct Grid {
    /// Where each column of the grid as it was begins, and where the last
    /// ends, in twentieths of a point from the table's left edge.
    old: Vec<i32>,
    /// And the same for the grid as it is to be.
    new: Vec<i32>,
    /// Where each new cell begins and ends, from left to right.
    pieces: Vec<(i32, i32)>,
}

impl Grid {
    /// The grid for one cell split into so many columns.
    fn split(shape: &Shape, slot: Slot, columns: usize) -> Option<Self> {
        let mut old = vec![0i32];
        for width in &shape.widths {
            old.push(old.last().copied().unwrap_or(0) + width);
        }
        let (left, right) = (*old.get(slot.start)?, *old.get(slot.end())?);

        let cuts: Vec<i32> = if columns == slot.span {
            // A cell as many columns wide as it is split into gets those
            // columns back, which is what makes a split undo a merge.
            old[slot.start + 1..slot.end()].to_vec()
        } else {
            // Otherwise the cell is shared out evenly, as Word shares it.
            let room = i64::from(right - left);
            (1..columns as i64).map(|at| left + (room * at / columns as i64) as i32).collect()
        };
        let mut edges = vec![left];
        edges.extend(&cuts);
        edges.push(right);
        // A cell too narrow to share out has nothing to give the new ones.
        if edges.windows(2).any(|pair| pair[1] <= pair[0]) {
            return None;
        }

        let mut new = old.clone();
        new.extend(&cuts);
        new.sort_unstable();
        new.dedup();
        if new.len() - 1 > MOST_COLUMNS {
            return None;
        }
        let pieces = edges.windows(2).map(|pair| (pair[0], pair[1])).collect();
        Some(Self { old, new, pieces })
    }

    /// Which column of the new grid an edge is the left edge of.
    fn at(&self, edge: i32) -> usize {
        self.new.binary_search(&edge).unwrap_or_else(|at| at)
    }

    /// How many columns of the new grid a stretch of the old one covers.
    fn moved(&self, start: usize, end: usize) -> usize {
        let edge = |column: usize| self.old.get(column).or(self.old.last()).copied().unwrap_or(0);
        self.at(edge(end)).saturating_sub(self.at(edge(start)))
    }

    /// The widths of the new grid's columns.
    fn widths(&self) -> Vec<i32> {
        self.new.windows(2).map(|pair| pair[1] - pair[0]).collect()
    }
}

/// Merges a rectangle of cells into the first of them, in place.
fn merge_block(table: &mut Element, shape: &Shape, block: &Block, prefix: Option<&str>) {
    let (first, last) = block.columns;
    let width = shape.stated.then(|| shape.widths[first..last].iter().sum::<i32>());
    let rows = positions_of(table, "tr");

    // Everything the cells held, in the order a person reads them.
    let mut gathered = Vec::new();
    for number in block.rows.0..=block.rows.1 {
        let Some(row) = rows.get(number).and_then(|at| table.children.get(*at)) else { continue };
        let Some(row) = row.as_element() else { continue };
        for (column, cell) in row.children_named(Some(read::W), "tc").enumerate() {
            if !shape.overlaps(number, column, block) {
                continue;
            }
            let content = content_of(cell);
            if !holds_nothing(&content) {
                gathered.extend(content);
            }
        }
    }

    for number in block.rows.0..=block.rows.1 {
        let Some(&row_at) = rows.get(number) else { continue };
        let Some(row) = table.children.get_mut(row_at).and_then(Node::as_element_mut) else {
            continue;
        };
        let inside: Vec<usize> = positions_of(row, "tc")
            .into_iter()
            .enumerate()
            .filter(|(column, _)| shape.overlaps(number, *column, block))
            .map(|(_, at)| at)
            .collect();
        let Some(&at) = inside.first() else { continue };
        let Some(model) = row.children.get(at).and_then(Node::as_element).cloned() else {
            continue;
        };
        for position in inside.iter().rev() {
            row.children.remove(*position);
        }

        // The first row's cell is the one that is drawn, and it takes what
        // they all held; below it, each row keeps a continuation of it. The
        // first cell's own properties are the merged cell's, as in Word.
        let (content, down) = if number == block.rows.0 {
            let content = if gathered.is_empty() {
                content_of(&model)
            } else {
                std::mem::take(&mut gathered)
            };
            (content, if block.rows.1 > block.rows.0 { Down::Starts } else { Down::Alone })
        } else {
            (Vec::new(), Down::Continues)
        };
        row.insert_element(at, rebuilt(&model, content, last - first, width, down, prefix));
    }
}

/// Splits one cell of a table in place, and moves every other cell onto the
/// grid the split makes.
///
/// `head` is the cell as it is drawn — the first row of it, where it is
/// merged down — and `tall` how many rows it covers.
fn split_in_place(
    table: &mut Element,
    shape: &Shape,
    grid: &Grid,
    head: (usize, usize),
    tall: usize,
    rows: usize,
    prefix: Option<&str>,
) {
    let (head_row, head_column) = head;
    let Some(start) = shape.start_of(head_row, head_column) else { return };
    let row_positions = positions_of(table, "tr");
    let columns = grid.pieces.len();

    // What the cell held — and anything a continuation of it held, which
    // nobody drew — cut into blocks and shared out over the cells it becomes.
    let mut blocks = Vec::new();
    for number in head_row..head_row + tall {
        let Some(column) = shape.covering(number, start) else { continue };
        let Some(row) = row_positions.get(number).and_then(|at| table.children.get(*at)) else {
            continue;
        };
        let Some(cell) =
            row.as_element().and_then(|row| row.children_named(Some(read::W), "tc").nth(column))
        else {
            continue;
        };
        let content = content_of(cell);
        if !holds_nothing(&content) {
            blocks.extend(blocks_of(content));
        }
    }
    let mut shares = share_out(blocks, columns * rows).into_iter();
    // How many of the rows it covers each new cell covers: one, unless a cell
    // merged over several is split into fewer.
    let group = if tall == 1 { 1 } else { tall / rows };

    for (number, &row_at) in row_positions.iter().enumerate() {
        let Some(row) = table.children.get_mut(row_at).and_then(Node::as_element_mut) else {
            continue;
        };
        move_row_onto(row, &shape.rows[number], grid, prefix);
        if !(head_row..head_row + tall).contains(&number) {
            continue;
        }
        let Some(column) = shape.covering(number, start) else { continue };
        let Some(at) = position_of(row, "tc", column) else { continue };
        let Some(model) = row.children.get(at).and_then(Node::as_element).cloned() else {
            continue;
        };
        let offset = number - head_row;
        let down = if group == 1 {
            Down::Alone
        } else if offset % group == 0 {
            Down::Starts
        } else {
            Down::Continues
        };
        row.children.remove(at);
        for (index, &(left, right)) in grid.pieces.iter().enumerate() {
            let content = if down == Down::Continues {
                Vec::new()
            } else {
                shares.next().unwrap_or_default()
            };
            let span = grid.at(right) - grid.at(left);
            let width = shape.stated.then_some(right - left);
            row.insert_element(at + index, rebuilt(&model, content, span, width, down, prefix));
        }

        // A row split into several: every other cell of it covers them all,
        // so each is merged down over the rows about to be added.
        if tall == 1 && rows > 1 {
            for (rank, position) in positions_of(row, "tc").into_iter().enumerate() {
                if (column..column + columns).contains(&rank) {
                    continue;
                }
                let Some(cell) = row.children.get_mut(position).and_then(Node::as_element_mut)
                else {
                    continue;
                };
                if down_of(cell) == Down::Alone {
                    set_down(properties_of(cell, prefix), Down::Starts, prefix);
                }
            }
        }
    }

    if tall == 1 && rows > 1 {
        let Some(&head_at) = row_positions.get(head_row) else { return };
        let Some(model) = table.children.get(head_at).and_then(Node::as_element).cloned() else {
            return;
        };
        for added in 1..rows {
            let fresh = added_row(&model, head_column..head_column + columns, &mut shares, prefix);
            table.insert_element(head_at + added, fresh);
        }
    }

    if shape.stated {
        write_grid(table, &grid.widths(), prefix);
    }
}

/// A row added under one being split down: the new cells where the split
/// cell's pieces are, and a continuation of every other cell of the row.
fn added_row(
    model: &Element,
    pieces: std::ops::Range<usize>,
    shares: &mut impl Iterator<Item = Vec<Node>>,
    prefix: Option<&str>,
) -> Element {
    let mut row = model.clone();
    // The marks Word gives a row to tell it from every other are the old
    // row's, and two rows with one mark are two rows Word cannot tell apart.
    row.attributes.retain(|attribute| {
        let local = attribute.name.rsplit(':').next().unwrap_or(&attribute.name);
        local != "paraId" && local != "textId"
    });
    let mut rank = 0usize;
    for node in &mut row.children {
        let Some(cell) = node.as_element_mut() else { continue };
        if !cell.is(Some(read::W), "tc") {
            continue;
        }
        let (content, down) = if pieces.contains(&rank) {
            (shares.next().unwrap_or_default(), Down::Alone)
        } else {
            (Vec::new(), Down::Continues)
        };
        *cell = rebuilt(cell, content, span_of(cell) as usize, None, down, prefix);
        rank += 1;
    }
    row
}

/// Moves every cell of a row onto the grid a split makes: the same edges,
/// counted in the new grid's columns.
fn move_row_onto(row: &mut Element, slots: &[Slot], grid: &Grid, prefix: Option<&str>) {
    for (slot, at) in slots.iter().zip(positions_of(row, "tc")) {
        let span = grid.moved(slot.start, slot.end());
        if span == slot.span {
            continue;
        }
        let Some(cell) = row.children.get_mut(at).and_then(Node::as_element_mut) else { continue };
        set_span(properties_of(cell, prefix), span, prefix);
    }

    // The columns a row leaves empty before its first cell and after its last
    // are counted in columns of the grid as well.
    let before = row_count(row, "gridBefore");
    if before > 0 {
        set_row_count(row, "gridBefore", grid.moved(0, before));
    }
    let after = row_count(row, "gridAfter");
    if after > 0 {
        let end = slots.last().map_or(before, |slot| slot.end());
        set_row_count(row, "gridAfter", grid.moved(end, end + after));
    }
}

/// A cell made from another: the same properties, its own span, width and
/// part in a merge down, and what it is given to hold.
fn rebuilt(
    model: &Element,
    content: Vec<Node>,
    span: usize,
    width: Option<i32>,
    down: Down,
    prefix: Option<&str>,
) -> Element {
    let mut cell = model.clone();
    cell.children
        .retain(|node| node.as_element().is_some_and(|child| child.is(Some(read::W), "tcPr")));
    let properties = properties_of(&mut cell, prefix);
    set_span(properties, span, prefix);
    set_down(properties, down, prefix);
    if let Some(width) = width {
        restate_width(properties, width, prefix);
    }
    cell.children.extend(content);
    // A cell must end with a paragraph — Word calls a file whose cell does
    // not damaged — and one given nothing is one empty paragraph.
    let ends_well = cell
        .children
        .last()
        .and_then(Node::as_element)
        .is_some_and(|last| last.is(Some(read::W), "p"));
    if !ends_well {
        cell.push_element(edit::paragraph_element(&crate::model::Paragraph::default(), prefix));
    }
    cell
}

/// Everything a cell holds: every child but its properties.
fn content_of(cell: &Element) -> Vec<Node> {
    cell.children
        .iter()
        .filter(|node| node.as_element().is_some_and(|child| !child.is(Some(read::W), "tcPr")))
        .cloned()
        .collect()
}

/// Whether what a cell holds is nothing: the one empty paragraph a cell has
/// to have. Word merges such a cell in without adding anything for it.
fn holds_nothing(content: &[Node]) -> bool {
    match content {
        [] => true,
        [only] => only.as_element().is_some_and(|paragraph| {
            paragraph.is(Some(read::W), "p")
                && paragraph.child_elements().all(|child| child.is(Some(read::W), "pPr"))
        }),
        _ => false,
    }
}

/// What a cell holds, cut into the pieces a split shares out: one per
/// paragraph or table, each with whatever sits between it and the next — a
/// bookmark's end, say — kept with it rather than left in a cell of its own.
fn blocks_of(content: Vec<Node>) -> Vec<Vec<Node>> {
    const BLOCKS: &[&str] = &["p", "tbl", "sdt", "customXml"];
    let mut out: Vec<Vec<Node>> = Vec::new();
    let mut waiting = Vec::new();
    for node in content {
        let block = node
            .as_element()
            .is_some_and(|element| BLOCKS.iter().any(|local| element.is(Some(read::W), local)));
        if block {
            let mut piece = std::mem::take(&mut waiting);
            piece.push(node);
            out.push(piece);
        } else if let Some(last) = out.last_mut() {
            last.push(node);
        } else {
            waiting.push(node);
        }
    }
    if !waiting.is_empty() {
        out.push(waiting);
    }
    out
}

/// Shares pieces out over so many cells, in order: one each while there are
/// fewer pieces than cells, and otherwise as evenly as they go, the first
/// cells taking one more.
fn share_out(pieces: Vec<Vec<Node>>, cells: usize) -> Vec<Vec<Node>> {
    let cells = cells.max(1);
    let each = pieces.len() / cells;
    let extra = pieces.len() % cells;
    let mut pieces = pieces.into_iter();
    (0..cells)
        .map(|index| {
            let count = each + usize::from(index < extra);
            pieces.by_ref().take(count).flatten().collect()
        })
        .collect()
}

/// How a cell takes part in a merge down, read from its properties.
fn down_of(cell: &Element) -> Down {
    let merge = cell
        .child(Some(read::W), "tcPr")
        .and_then(|properties| properties.child(Some(read::W), "vMerge"));
    match merge {
        None => Down::Alone,
        // A `w:vMerge` that says nothing continues the one above, as the
        // format has it; only "restart" starts one.
        Some(merge) if merge.attribute(Some(read::W), "val") == Some("restart") => Down::Starts,
        Some(_) => Down::Continues,
    }
}

/// Says how many columns of the grid a cell covers.
fn set_span(properties: &mut Element, span: usize, prefix: Option<&str>) {
    properties.remove_children_named(Some(read::W), "gridSpan");
    if span > 1 {
        properties_insert(properties, prefix, "gridSpan", Some(&span.to_string()));
    }
}

/// Says what part a cell takes in a merge down.
fn set_down(properties: &mut Element, down: Down, prefix: Option<&str>) {
    properties.remove_children_named(Some(read::W), "vMerge");
    match down {
        Down::Alone => {}
        Down::Starts => properties_insert(properties, prefix, "vMerge", Some("restart")),
        // Written with no value, as Word writes a continuation.
        Down::Continues => properties_insert(properties, prefix, "vMerge", None),
    }
}

/// Gives a cell's stated width a new number, where it states one in
/// twentieths of a point. A width stated as a share of the table, or left to
/// the layout, is not this program's to turn into a number.
fn restate_width(properties: &mut Element, width: i32, prefix: Option<&str>) {
    let Some(stated) = properties.child_mut(Some(read::W), "tcW") else { return };
    if !matches!(stated.attribute(Some(read::W), "type"), None | Some("dxa")) {
        return;
    }
    stated.set_namespaced_attribute(&edit::name_with(prefix, "w"), read::W, &width.to_string());
}

/// A count a row states in its properties: `w:gridBefore` or `w:gridAfter`.
fn row_count(row: &Element, local: &str) -> usize {
    row.child(Some(read::W), "trPr")
        .and_then(|properties| properties.child(Some(read::W), local))
        .and_then(|count| count.attribute(Some(read::W), "val"))
        .and_then(|text| text.parse().ok())
        .unwrap_or(0)
}

/// And gives one a new value, where the row states it.
fn set_row_count(row: &mut Element, local: &str, value: usize) {
    let Some(count) = row
        .child_mut(Some(read::W), "trPr")
        .and_then(|properties| properties.child_mut(Some(read::W), local))
    else {
        return;
    };
    let name = count.prefix().map_or_else(|| "val".to_owned(), |prefix| format!("{prefix}:val"));
    count.set_namespaced_attribute(&name, read::W, &value.to_string());
}

/// Writes the columns of a table's grid, keeping whatever else the grid
/// holds.
fn write_grid(table: &mut Element, widths: &[i32], prefix: Option<&str>) {
    let Some(grid) = table.child_mut(Some(read::W), "tblGrid") else { return };
    grid.remove_children_named(Some(read::W), "gridCol");
    // The columns come first in it, before any record of a tracked change.
    for (index, width) in widths.iter().enumerate() {
        let mut column = Element::new(&edit::name_with(prefix, "gridCol"), Some(read::W));
        column.set_namespaced_attribute(&edit::name_with(prefix, "w"), read::W, &width.to_string());
        grid.insert_element(index, column);
    }
}

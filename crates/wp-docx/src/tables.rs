//! Editing a table that is already in the document.
//!
//! # Working on the tree, not on the model
//!
//! Everything here changes `w:tbl` elements in place rather than rebuilding a
//! table from the read model and writing it back. That is the whole point: a
//! real table carries cell shading, vertical merges, conditional formatting and
//! properties this program has never heard of, and rebuilding it would throw
//! all of that away the first time somebody added a row.
//!
//! # What a caret in a table knows
//!
//! A caret is a paragraph index and an offset — it says nothing about tables.
//! [`Document::table_here`] works out the rest by walking up from the paragraph
//! to the row and the cell that contain it, which is also how it answers "am I
//! in a table at all".

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::model::TableBorders;
use crate::{edit, position, read, Document};

/// Where the caret is inside a table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TablePosition {
    /// Path from the document root to the `w:tbl`.
    pub table: Vec<usize>,
    /// Which row the caret is in, counted from zero.
    pub row: usize,
    /// Which cell of that row.
    pub column: usize,
    /// How many rows and columns the table has.
    pub rows: usize,
    pub columns: usize,
}

impl Document {
    /// Whether a paragraph sits inside a table.
    ///
    /// Asked by the line numbering, which counts the lines of the text and not
    /// the lines inside a table — that is Word's rule, and it is why a numbered
    /// contract does not number its own schedule of figures.
    #[must_use]
    pub fn paragraph_in_table(&self, paragraph: usize) -> bool {
        let Some(path) = position::paragraph_path(&self.tree().root, paragraph) else {
            return false;
        };
        (0..path.len()).any(|depth| {
            element_at(&self.tree().root, &path[..depth])
                .is_some_and(|element| element.is(Some(read::W), "tbl"))
        })
    }

    /// Where the caret is in a table, if it is in one at all.
    #[must_use]
    pub fn table_here(&self) -> Option<TablePosition> {
        self.table_at(self.caret().paragraph)
    }

    /// And where any paragraph is, which is what something reading the document
    /// rather than following the caret asks — a formula being worked out, for
    /// one. See [`crate::formula`].
    #[must_use]
    pub fn table_at(&self, paragraph: usize) -> Option<TablePosition> {
        let path = position::paragraph_path(&self.tree().root, paragraph)?;

        // Walk back up the path looking for the row and the table. A table
        // inside a table means the innermost one wins, which is what the caret
        // is actually in.
        let mut cell_index = None;
        let mut row_index = None;
        let mut table_path = None;

        for depth in (0..path.len()).rev() {
            let element = element_at(&self.tree().root, &path[..depth])?;
            let child_position = path[depth];
            if element.is(Some(read::W), "tr") && cell_index.is_none() {
                cell_index = Some(element_rank(element, "tc", child_position));
            } else if element.is(Some(read::W), "tbl") && row_index.is_none() {
                row_index = Some(element_rank(element, "tr", child_position));
                table_path = Some(path[..depth].to_vec());
                break;
            }
        }

        let table_path = table_path?;
        let table = element_at(&self.tree().root, &table_path)?;
        let rows = count_children(table, "tr");
        let row = row_index?;
        let columns = table
            .children_named(Some(read::W), "tr")
            .nth(row)
            .map_or(0, |row| count_children(row, "tc"));

        Some(TablePosition {
            table: table_path,
            row,
            column: cell_index.unwrap_or(0),
            rows,
            columns,
        })
    }

    /// The paragraphs one cell of the table at the caret covers.
    ///
    /// As a first and a last index into the document's paragraphs, because
    /// that is what a selection is made of: a cell holds at least one paragraph
    /// and may hold several, so a cell is a range and not a number.
    #[must_use]
    pub fn cell_paragraphs(&self, row: usize, column: usize) -> Option<(usize, usize)> {
        self.cell_paragraphs_at(self.caret().paragraph, row, column)
    }

    /// The same for the table round any paragraph: what a drag that began in
    /// a cell asks once the pointer, and the caret with it, has left the
    /// table.
    #[must_use]
    pub fn cell_paragraphs_at(
        &self,
        paragraph: usize,
        row: usize,
        column: usize,
    ) -> Option<(usize, usize)> {
        let place = self.table_at(paragraph)?;
        let table = element_at(&self.tree().root, &place.table)?;

        // The path to the cell: the table, then the row's place among its
        // children, then the cell's among that row's.
        let row_at = child_index_of(table, "tr", row)?;
        let row_element = table.children_named(Some(read::W), "tr").nth(row)?;
        let cell_at = child_index_of(row_element, "tc", column)?;

        let mut path = place.table.clone();
        path.push(row_at);
        path.push(cell_at);
        paragraphs_under(&self.tree().root, &path)
    }

    /// And the whole table at the caret.
    #[must_use]
    pub fn table_paragraphs(&self) -> Option<(usize, usize)> {
        let place = self.table_here()?;
        paragraphs_under(&self.tree().root, &place.table)
    }

    /// The same for the table round any paragraph, which is what a pointer over
    /// a table asks: the caret may be nowhere near it.
    #[must_use]
    pub fn table_paragraphs_at(&self, paragraph: usize) -> Option<(usize, usize)> {
        let place = self.table_at(paragraph)?;
        paragraphs_under(&self.tree().root, &place.table)
    }

    /// Moves the table at the caret so that it comes before a paragraph.
    ///
    /// Word's move handle: the square outside the top-left corner of a table,
    /// dragged to put the table somewhere else. The element itself is taken out
    /// of the tree and put back in — not rebuilt — so everything the table
    /// carries that this program has never heard of goes with it.
    ///
    /// Answers false rather than doing something surprising when the table
    /// cannot go where it was asked: into itself, or out of the cell or the
    /// body it lives in. Word will move a table into a cell of another table;
    /// this will not, and says so.
    pub fn move_table_before(&mut self, paragraph: usize) -> bool {
        let Some(place) = self.table_here() else { return false };
        let Some((first, last)) = paragraphs_under(&self.tree().root, &place.table) else {
            return false;
        };
        // Into itself is nowhere.
        if (first..=last).contains(&paragraph) {
            return false;
        }
        let Some(target) = position::paragraph_path(&self.tree().root, paragraph) else {
            return false;
        };
        let Some((table_at, table_parent)) = place.table.split_last() else { return false };
        let Some((target_at, target_parent)) = target.split_last() else { return false };
        if table_parent != target_parent {
            return false;
        }
        let (table_at, target_at) = (*table_at, *target_at);
        if table_at == target_at {
            return false;
        }

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let parent_path = table_parent.to_vec();
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &parent_path)
        else {
            return false;
        };

        let element = parent.children.remove(table_at);
        // Taking it out moved everything after it up one, so a place past the
        // table is one less than it was.
        let at = if target_at > table_at { target_at - 1 } else { target_at };
        parent.children.insert(at, element);

        self.clamp_caret_after_edit();
        self.note_change();
        true
    }

    /// How many cells a row of the table at the caret has.
    ///
    /// Row by row rather than once for the table, because rows do not have to
    /// agree: two cells merged across leave that row one cell shorter, and a
    /// table read from Word may have been built that way in the first place.
    #[must_use]
    pub fn cells_in_row(&self, row: usize) -> usize {
        let Some(place) = self.table_here() else { return 0 };
        let Some(table) = element_at(&self.tree().root, &place.table) else { return 0 };
        table
            .children_named(Some(read::W), "tr")
            .nth(row)
            .map_or(0, |row| count_children(row, "tc"))
    }

    /// The cell Tab moves the caret on to, or Shift+Tab back to.
    ///
    /// Word's Tab in a table moves by cell rather than by character — it is how
    /// a table is filled in without ever reaching for the mouse. At the end of
    /// a row it goes on to the first cell of the next one, and at the very last
    /// cell it answers `None`: that is where Word adds a row instead, which is
    /// a decision for the editor and not for the document.
    ///
    /// Only through cells that are drawn. A cell merged down is written once
    /// per row it covers, and every row but its first holds a continuation
    /// that nothing draws; counting those, Tab went into them — and in a
    /// table merged into one cell went from the one cell there is into the
    /// second row, where there is nothing to see and nowhere to type.
    #[must_use]
    pub fn cell_beside(&self, forwards: bool) -> Option<(usize, usize)> {
        let place = self.table_here()?;
        let table = element_at(&self.tree().root, &place.table)?;
        let drawn = crate::cells::Shape::of(table).drawn();
        // A row and a place along it, compared as a pair, is reading order —
        // which also answers for a caret that is in a continuation, where a
        // press can put it: on from there, not from the cell it continues.
        let here = (place.row, place.column);
        if forwards {
            drawn.into_iter().find(|cell| *cell > here)
        } else {
            drawn.into_iter().rev().find(|cell| *cell < here)
        }
    }

    /// The cell the down arrow takes the caret into, or the up arrow — or
    /// `None` where the table ends that way.
    ///
    /// By what is drawn, as Tab goes: down out of a cell merged over three
    /// rows is into the row after the third, and up into a merged cell is into
    /// the cell itself, never into one of the continuations that stand for it
    /// in the rows below its first. The column is the one the caret's cell
    /// starts at, counted on the grid, so that a row whose cells do not line
    /// up with this one still gives the cell under it rather than the one that
    /// happens to be as far along the row.
    #[must_use]
    pub fn cell_over_or_under(&self, downwards: bool) -> Option<(usize, usize)> {
        let place = self.table_here()?;
        let table = element_at(&self.tree().root, &place.table)?;
        let shape = crate::cells::Shape::of(table);
        let (row, column) = shape.head_of(place.row, place.column);
        let start = shape.start_of(row, column)?;
        if downwards {
            let mut next = row + shape.height_of(row, column);
            loop {
                let found = shape.nearest(next, start)?;
                if !shape.hidden(next, found) {
                    return Some((next, found));
                }
                next += 1;
            }
        } else {
            let above = row.checked_sub(1)?;
            let found = shape.nearest(above, start)?;
            Some(shape.head_of(above, found))
        }
    }

    /// The paragraphs of the cell a paragraph is in, when that cell is a
    /// continuation of a merge down — in the file and not on the page.
    ///
    /// Asked by the arrows, which step over such a cell rather than put the
    /// caret where nobody can see it.
    #[must_use]
    pub fn hidden_cell_at(&self, paragraph: usize) -> Option<(usize, usize)> {
        let place = self.table_at(paragraph)?;
        let table = element_at(&self.tree().root, &place.table)?;
        if !crate::cells::Shape::of(table).hidden(place.row, place.column) {
            return None;
        }
        let row_at = child_index_of(table, "tr", place.row)?;
        let row = table.children_named(Some(read::W), "tr").nth(place.row)?;
        let cell_at = child_index_of(row, "tc", place.column)?;
        let mut path = place.table.clone();
        path.push(row_at);
        path.push(cell_at);
        paragraphs_under(&self.tree().root, &path)
    }

    /// Everything in one cell of the table at the caret, as a stretch.
    ///
    /// What Tab selects when it lands on a cell, and what selecting a cell with
    /// the mouse comes to. From the start of the cell's first paragraph to the
    /// end of its last: a cell holds paragraphs, so a cell is a range.
    #[must_use]
    pub fn cell_text_range(
        &self,
        row: usize,
        column: usize,
    ) -> Option<(crate::TextPosition, crate::TextPosition)> {
        let (first, last) = self.cell_paragraphs(row, column)?;
        let end = self.paragraph_text(last).unwrap_or_default().len();
        Some((crate::TextPosition::new(first, 0), crate::TextPosition::new(last, end)))
    }

    /// Turns the table at the caret back into ordinary paragraphs.
    ///
    /// Word's Convert to Text. Its default separator is a tab, so a row becomes
    /// one paragraph with its cells tabbed apart — which is what makes the
    /// result convertible back into a table, and what keeps a table of figures
    /// readable after it stops being a table.
    pub fn convert_table_to_text(&mut self) -> bool {
        let Some(place) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = element_at(&self.tree().root, &place.table) else { return false };
        let rows: Vec<Element> = table
            .children_named(Some(read::W), "tr")
            .map(|row| row_as_paragraph(row, prefix.as_deref()))
            .collect();
        if rows.is_empty() {
            return false;
        }

        // The table's place among its parent's children, which is where the
        // paragraphs go.
        let Some((parent_path, at)) = place.table.split_last().map(|(at, rest)| (rest, *at)) else {
            return false;
        };
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_to_edit().root, parent_path)
        else {
            return false;
        };
        if at >= parent.children.len() {
            return false;
        }
        parent.children.remove(at);
        for (offset, paragraph) in rows.into_iter().enumerate() {
            parent.children.insert(at + offset, wp_xml::tree::Node::Element(paragraph));
        }

        self.clamp_caret();
        self.note_change();
        true
    }
    /// Adds a row above or below the one the caret is in.
    pub fn insert_table_row(&mut self, below: bool) -> bool {
        let Some(place) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &place.table)
        else {
            return false;
        };
        let Some(position) = child_position(table, "tr", place.row) else { return false };

        // The new row is a copy of the one it sits beside, emptied. Copying is
        // what carries the cell widths, the borders and everything else the row
        // was set up with; anything else would produce a row that looked wrong.
        let Some(model) = table.children.get(position).and_then(Node::as_element).cloned() else {
            return false;
        };
        let fresh = empty_row(&model, prefix.as_deref());
        table.insert_element(if below { position + 1 } else { position }, fresh);

        self.note_change();
        true
    }

    /// Adds a row after the last row of the table at the caret.
    ///
    /// What Tab does after the last cell that is drawn. After the last row
    /// rather than after the caret's: where the last cells of a table are
    /// merged down from above, the caret's row is not the last, and a row put
    /// under it would cut the merge in two. The row is a copy of the last one,
    /// emptied, with any merge down taken off it — a merge ends at the last
    /// row it was made over, and a row added under the table is cells of its
    /// own, as Word adds it.
    pub fn append_table_row(&mut self) -> bool {
        let Some(place) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &place.table)
        else {
            return false;
        };
        let Some(position) =
            place.rows.checked_sub(1).and_then(|last| child_position(table, "tr", last))
        else {
            return false;
        };
        let Some(model) = table.children.get(position).and_then(Node::as_element).cloned() else {
            return false;
        };
        let mut fresh = empty_row(&model, prefix.as_deref());
        for cell in fresh.child_elements_mut().filter(|child| child.is(Some(read::W), "tc")) {
            if let Some(properties) = cell.child_mut(Some(read::W), "tcPr") {
                properties.remove_children_named(Some(read::W), "vMerge");
            }
        }
        table.insert_element(position + 1, fresh);

        self.note_change();
        true
    }

    /// Takes every selected row out of the table, or the caret's row when
    /// nothing is selected.
    ///
    /// Word's Delete Rows: three rows selected are three rows deleted. Taking
    /// the caret's row alone meant selecting a block and pressing it once per
    /// row, which is not what the button says.
    ///
    /// Removing the last row removes the table, because a table with no rows is
    /// not a table — Word will not open one.
    pub fn delete_table_row(&mut self) -> bool {
        let Some(place) = self.table_here() else { return false };
        let (first, last) =
            self.selected_cells().map_or((place.row, place.row), |range| range.rows);
        let last = last.min(place.rows.saturating_sub(1));
        if last + 1 - first >= place.rows {
            return self.delete_table();
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let Some(table) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &place.table)
        else {
            return false;
        };
        // From the last back to the first: taking one out moves every row after
        // it up, and the ones still to go would then be the wrong rows.
        for row in (first..=last).rev() {
            if let Some(position) = child_position(table, "tr", row) {
                table.children.remove(position);
            }
        }

        // The caret goes into the row that took their place, which is where
        // Word leaves it — deleting a row while typing does not throw the caret
        // out of the table.
        let table_path = place.table.clone();
        if !self.caret_into_cell(&table_path, first, place.column) {
            self.clamp_caret_after_edit();
        }
        self.note_change();
        true
    }

    /// Adds a column to the left or the right of the one the caret is in.
    pub fn insert_table_column(&mut self, after: bool) -> bool {
        let Some(place) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &place.table)
        else {
            return false;
        };

        // Every row gets a cell, copied from the one beside it in that row so
        // the new column inherits whatever the old one was set up with.
        let row_positions: Vec<usize> = table
            .children
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node.as_element().is_some_and(|child| child.is(Some(read::W), "tr"))
            })
            .map(|(index, _)| index)
            .collect();

        for row_position in row_positions {
            let Some(row) = table.children.get_mut(row_position).and_then(Node::as_element_mut)
            else {
                continue;
            };
            let Some(cell_position) = child_position(row, "tc", place.column.min(place.columns))
            else {
                continue;
            };
            let Some(model) = row.children.get(cell_position).and_then(Node::as_element).cloned()
            else {
                continue;
            };
            let fresh = empty_cell(&model, prefix.as_deref());
            row.insert_element(if after { cell_position + 1 } else { cell_position }, fresh);
        }

        // The grid gains a column too, or Word draws the table at the wrong
        // width.
        add_grid_column(table, prefix.as_deref(), place.column, after);

        self.note_change();
        true
    }

    /// Takes every selected column out of the table, or the caret's column when
    /// nothing is selected. See [`Document::delete_table_row`].
    pub fn delete_table_column(&mut self) -> bool {
        let Some(place) = self.table_here() else { return false };
        let (first, last) =
            self.selected_cells().map_or((place.column, place.column), |range| range.columns);
        let last = last.min(place.columns.saturating_sub(1));
        if last + 1 - first >= place.columns {
            return self.delete_table();
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let Some(table) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &place.table)
        else {
            return false;
        };

        let row_positions: Vec<usize> = table
            .children
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node.as_element().is_some_and(|child| child.is(Some(read::W), "tr"))
            })
            .map(|(index, _)| index)
            .collect();

        for row_position in row_positions {
            let Some(row) = table.children.get_mut(row_position).and_then(Node::as_element_mut)
            else {
                continue;
            };
            // Backwards, for the reason the rows go backwards.
            for column in (first..=last).rev() {
                if let Some(cell_position) = child_position(row, "tc", column) {
                    row.children.remove(cell_position);
                }
            }
        }
        for column in (first..=last).rev() {
            remove_grid_column(table, column);
        }

        let table_path = place.table.clone();
        if !self.caret_into_cell(&table_path, place.row, first) {
            self.clamp_caret_after_edit();
        }
        self.note_change();
        true
    }

    /// Removes the whole table.
    pub fn delete_table(&mut self) -> bool {
        let Some(place) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let Some((position, parent_path)) = place.table.split_last() else { return false };
        let position = *position;
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_to_edit().root, parent_path)
        else {
            return false;
        };
        parent.children.remove(position);

        self.clamp_caret_after_edit();
        self.note_change();
        true
    }

    /// Sets the lines a table draws.
    pub fn set_table_borders(&mut self, borders: &TableBorders) -> bool {
        let Some(place) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &place.table)
        else {
            return false;
        };
        let properties = table_properties_of(table, prefix.as_deref());
        properties.remove_children_named(Some(read::W), "tblBorders");
        // The borders go first among the properties the schema allows here,
        // after the style and the width — `insert_ordered` knows where.
        edit::insert_ordered(
            properties,
            edit::table_borders_element(borders, prefix.as_deref()),
            edit::TABLE_PROPERTY_ORDER,
        );

        self.note_change();
        true
    }

    /// Puts the caret in one cell of a table named by its path.
    ///
    /// By the path rather than by [`Document::cell_paragraphs`], because what
    /// asks is a row or a column being deleted, or cells merged or split: the
    /// caret is about to be nowhere, and asking where the caret is would be
    /// asking the question backwards.
    pub(crate) fn caret_into_cell(
        &mut self,
        table_path: &[usize],
        row: usize,
        column: usize,
    ) -> bool {
        let Some(table) = element_at(&self.tree().root, table_path) else { return false };
        let rows = count_children(table, "tr");
        if rows == 0 {
            return false;
        }
        let row = row.min(rows - 1);
        let Some(row_at) = child_index_of(table, "tr", row) else { return false };
        let Some(row_element) = table.children_named(Some(read::W), "tr").nth(row) else {
            return false;
        };
        let columns = count_children(row_element, "tc");
        if columns == 0 {
            return false;
        }
        let Some(cell_at) = child_index_of(row_element, "tc", column.min(columns - 1)) else {
            return false;
        };

        let mut path = table_path.to_vec();
        path.push(row_at);
        path.push(cell_at);
        let Some((first, _)) = paragraphs_under(&self.tree().root, &path) else { return false };
        self.set_caret(crate::TextPosition::new(first, 0));
        true
    }

    /// Puts the caret somewhere that still exists after a structural change.
    fn clamp_caret_after_edit(&mut self) {
        let count = self.paragraph_count();
        let caret = self.caret();
        if caret.paragraph >= count {
            let last = count.saturating_sub(1);
            let offset = self.paragraph_text(last).map_or(0, |text| text.len());
            self.set_caret(crate::TextPosition::new(last, offset));
        } else {
            // The offset may now be past the end of a shorter paragraph.
            let length = self.paragraph_text(caret.paragraph).map_or(0, |text| text.len());
            if caret.offset > length {
                self.set_caret(crate::TextPosition::new(caret.paragraph, length));
            }
        }
    }
}

/// The `w:tblPr` of a table, made if it is not there.
fn table_properties_of<'a>(table: &'a mut Element, prefix: Option<&str>) -> &'a mut Element {
    if table.child(Some(read::W), "tblPr").is_none() {
        // It is the first child of a table, which the schema requires.
        table.insert_element(0, Element::new(&edit::name_with(prefix, "tblPr"), Some(read::W)));
    }
    table.child_mut(Some(read::W), "tblPr").expect("just inserted, or already there")
}

/// The element at a path from the root.
fn element_at<'a>(root: &'a Element, path: &[usize]) -> Option<&'a Element> {
    let mut current = root;
    for &index in path {
        current = current.children.get(index)?.as_element()?;
    }
    Some(current)
}

/// How many children of a given name an element has.
fn count_children(element: &Element, local: &str) -> usize {
    element.children_named(Some(read::W), local).count()
}

/// Where the nth child of a given name sits among all the children.
fn child_position(element: &Element, local: &str, nth: usize) -> Option<usize> {
    element
        .children
        .iter()
        .enumerate()
        .filter(|(_, node)| node.as_element().is_some_and(|child| child.is(Some(read::W), local)))
        .map(|(index, _)| index)
        .nth(nth)
}

/// Which of the named children a child position corresponds to.
fn element_rank(element: &Element, local: &str, child_position: usize) -> usize {
    element
        .children
        .iter()
        .take(child_position)
        .filter(|node| node.as_element().is_some_and(|child| child.is(Some(read::W), local)))
        .count()
}

/// A copy of a row with every cell emptied.
fn empty_row(model: &Element, prefix: Option<&str>) -> Element {
    let mut row = Element::new(&edit::name_with(prefix, "tr"), Some(read::W));
    row.attributes = model.attributes.clone();

    for child in &model.children {
        let Node::Element(child) = child else { continue };
        if child.is(Some(read::W), "tc") {
            row.push_element(empty_cell(child, prefix));
        } else {
            // The row's own properties — its height, its header flag — are kept.
            row.push_element(child.clone());
        }
    }
    row
}

/// A copy of a cell with its content replaced by one empty paragraph.
fn empty_cell(model: &Element, prefix: Option<&str>) -> Element {
    let mut cell = Element::new(&edit::name_with(prefix, "tc"), Some(read::W));
    cell.attributes = model.attributes.clone();

    if let Some(properties) = model.child(Some(read::W), "tcPr") {
        cell.push_element(properties.clone());
    }
    // A cell must end with a paragraph, and an empty cell is one empty
    // paragraph — a cell with no paragraph at all is a file Word refuses.
    cell.push_element(edit::paragraph_element(&crate::model::Paragraph::default(), prefix));
    cell
}

/// Widens the grid by one column, copying the width beside it.
pub(crate) fn widen_grid(table: &mut Element, prefix: Option<&str>, column: usize) {
    add_grid_column(table, prefix, column, true);
}

/// The same, saying which side the new column goes.
fn add_grid_column(table: &mut Element, prefix: Option<&str>, column: usize, after: bool) {
    let Some(grid) = table.child_mut(Some(read::W), "tblGrid") else { return };
    let Some(position) = child_position(grid, "gridCol", column) else { return };
    let Some(model) = grid.children.get(position).and_then(Node::as_element).cloned() else {
        return;
    };

    // The new column takes half the width of the one it was split from, so the
    // table stays the width it was.
    let width: i32 =
        model.attribute(Some(read::W), "w").and_then(|text| text.parse().ok()).unwrap_or(1440);
    let half = (width / 2).max(1);

    let mut fresh = Element::new(&edit::name_with(prefix, "gridCol"), Some(read::W));
    fresh.set_namespaced_attribute(&edit::name_with(prefix, "w"), read::W, &half.to_string());
    grid.insert_element(if after { position + 1 } else { position }, fresh);

    if let Some(existing) = grid.children.get_mut(if after { position } else { position + 1 }) {
        if let Some(existing) = existing.as_element_mut() {
            existing.set_namespaced_attribute(
                &edit::name_with(prefix, "w"),
                read::W,
                &(width - half).to_string(),
            );
        }
    }
}

/// Narrows the grid by one column.
fn remove_grid_column(table: &mut Element, column: usize) {
    let Some(grid) = table.child_mut(Some(read::W), "tblGrid") else { return };
    if let Some(position) = child_position(grid, "gridCol", column) {
        grid.children.remove(position);
    }
}

/// Which child of its parent the nth element of a kind is.
///
/// The rows of a table are its `w:tr` children, but they are not necessarily
/// its only children — a `w:tblPr` and a `w:tblGrid` come first — so the third
/// row is not the third child.
fn child_index_of(parent: &Element, local: &str, wanted: usize) -> Option<usize> {
    let mut seen = 0usize;
    for (index, node) in parent.children.iter().enumerate() {
        let Some(child) = node.as_element() else { continue };
        if !child.is(Some(read::W), local) {
            continue;
        }
        if seen == wanted {
            return Some(index);
        }
        seen += 1;
    }
    None
}

/// The first and last paragraph, in document order, under a path.
///
/// Counted the same way every paragraph index in this program is counted: in
/// reading order from the start of the part, tables included.
pub(crate) fn paragraphs_under(root: &Element, prefix: &[usize]) -> Option<(usize, usize)> {
    let mut counter = 0usize;
    let mut path = Vec::new();
    let mut found: Option<(usize, usize)> = None;
    walk_counting(root, &mut path, &mut counter, prefix, &mut found);
    found
}

fn walk_counting(
    element: &Element,
    path: &mut Vec<usize>,
    counter: &mut usize,
    prefix: &[usize],
    found: &mut Option<(usize, usize)>,
) {
    for (index, node) in element.children.iter().enumerate() {
        let Some(child) = node.as_element() else { continue };
        if child.namespace.as_deref() != Some(read::W) {
            continue;
        }

        path.push(index);
        if child.is(Some(read::W), "p") {
            if path.len() >= prefix.len() && path[..prefix.len()] == *prefix {
                *found = Some(match *found {
                    Some((first, _)) => (first, *counter),
                    None => (*counter, *counter),
                });
            }
            *counter += 1;
        } else {
            walk_counting(child, path, counter, prefix, found);
        }
        path.pop();
    }
}

/// One row of a table as a single paragraph, its cells tabbed apart.
fn row_as_paragraph(row: &Element, prefix: Option<&str>) -> Element {
    let mut paragraph = Element::new(&edit::name_with(prefix, "p"), Some(read::W));

    for (number, cell) in row.children_named(Some(read::W), "tc").enumerate() {
        if number > 0 {
            // A tab between one cell's words and the next, which is Word's own
            // separator and what makes the result convertible back.
            let mut run = Element::new(&edit::name_with(prefix, "r"), Some(read::W));
            run.push_element(Element::new(&edit::name_with(prefix, "tab"), Some(read::W)));
            paragraph.push_element(run);
        }

        // Every run of every paragraph of the cell, in order, so the words keep
        // the formatting they were written with.
        for inner in cell.children_named(Some(read::W), "p") {
            for run in inner.children_named(Some(read::W), "r") {
                paragraph.push_element(run.clone());
            }
        }
    }
    paragraph
}

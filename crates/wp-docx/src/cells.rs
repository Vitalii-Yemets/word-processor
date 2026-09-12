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

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::{edit, read, Document, TextPosition};

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

        let stretches = self.selections();
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

        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &place.table) else {
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

        self.mark_modified();
        true
    }

    /// Joins the selected cells into one.
    pub fn merge_cells(&mut self) -> bool {
        let Some(range) = self.selected_cells() else { return false };
        let (first_row, last_row) = range.rows;
        let (first_column, last_column) = range.columns;
        if first_row == last_row && first_column == last_column {
            return false;
        }

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &range.table) else {
            return false;
        };

        for (number, row_position) in positions_of(table, "tr").into_iter().enumerate() {
            if number < first_row || number > last_row {
                continue;
            }
            let Some(row) = table.children.get_mut(row_position).and_then(Node::as_element_mut)
            else {
                continue;
            };

            // The cells to the right of the first go, and how many columns they
            // covered is added to it: the table has to stay the width it was.
            let cells = positions_of(row, "tc");
            let mut covered = 0u32;
            for column in (first_column..=last_column).rev() {
                let Some(position) = cells.get(column).copied() else { continue };
                let Some(cell) = row.children.get(position).and_then(Node::as_element) else {
                    continue;
                };
                covered += span_of(cell);
                if column > first_column {
                    row.children.remove(position);
                }
            }

            let Some(position) = cells.get(first_column).copied() else { continue };
            let Some(cell) = row.children.get_mut(position).and_then(Node::as_element_mut) else {
                continue;
            };
            let properties = properties_of(cell, prefix.as_deref());

            properties.remove_children_named(Some(read::W), "gridSpan");
            if covered > 1 {
                properties_insert(
                    properties,
                    prefix.as_deref(),
                    "gridSpan",
                    Some(&covered.to_string()),
                );
            }

            // Down the rows: the first starts the merge and the rest continue it.
            if last_row > first_row {
                properties.remove_children_named(Some(read::W), "vMerge");
                let value = if number == first_row { Some("restart") } else { None };
                properties_insert(properties, prefix.as_deref(), "vMerge", value);
            }
        }

        self.mark_modified();
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

        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &place.table) else {
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
        self.mark_modified();
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

        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &place.table) else {
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
        self.mark_modified();
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

    /// Splits the cell the caret is in back into separate cells.
    pub fn split_cell(&mut self) -> bool {
        let Some(place) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &place.table) else {
            return false;
        };
        let Some(row_position) = position_of(table, "tr", place.row) else { return false };
        let Some(row) = table.children.get_mut(row_position).and_then(Node::as_element_mut) else {
            return false;
        };
        let Some(cell_position) = position_of(row, "tc", place.column) else { return false };
        let Some(model) = row.children.get(cell_position).and_then(Node::as_element).cloned()
        else {
            return false;
        };

        let span = span_of(&model);
        let merged = model
            .child(Some(read::W), "tcPr")
            .is_some_and(|properties| properties.child(Some(read::W), "vMerge").is_some());
        if span <= 1 && !merged {
            return false;
        }

        if let Some(cell) = row.children.get_mut(cell_position).and_then(Node::as_element_mut) {
            if let Some(properties) = cell.child_mut(Some(read::W), "tcPr") {
                properties.remove_children_named(Some(read::W), "vMerge");
                properties.remove_children_named(Some(read::W), "gridSpan");
            }
        }

        // A cell that covered several columns becomes that many cells again.
        for offset in 1..span as usize {
            let fresh = empty_cell(&model, prefix.as_deref());
            row.insert_element(cell_position + offset, fresh);
        }

        self.mark_modified();
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

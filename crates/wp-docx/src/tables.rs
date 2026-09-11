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
        let path = position::paragraph_path(&self.tree().root, self.caret().paragraph)?;

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
        let place = self.table_here()?;
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
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, parent_path) else {
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
        self.mark_modified();
        true
    }
    /// Adds a row above or below the one the caret is in.
    pub fn insert_table_row(&mut self, below: bool) -> bool {
        let Some(place) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &place.table) else {
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

        self.mark_modified();
        true
    }

    /// Takes the row the caret is in out of the table.
    ///
    /// Removing the last row removes the table, because a table with no rows is
    /// not a table — Word will not open one.
    pub fn delete_table_row(&mut self) -> bool {
        let Some(place) = self.table_here() else { return false };
        if place.rows <= 1 {
            return self.delete_table();
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &place.table) else {
            return false;
        };
        let Some(position) = child_position(table, "tr", place.row) else { return false };
        table.children.remove(position);

        self.clamp_caret_after_edit();
        self.mark_modified();
        true
    }

    /// Adds a column to the left or the right of the one the caret is in.
    pub fn insert_table_column(&mut self, after: bool) -> bool {
        let Some(place) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &place.table) else {
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

        self.mark_modified();
        true
    }

    /// Takes the column the caret is in out of the table.
    pub fn delete_table_column(&mut self) -> bool {
        let Some(place) = self.table_here() else { return false };
        if place.columns <= 1 {
            return self.delete_table();
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &place.table) else {
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
            if let Some(cell_position) = child_position(row, "tc", place.column) {
                row.children.remove(cell_position);
            }
        }
        remove_grid_column(table, place.column);

        self.clamp_caret_after_edit();
        self.mark_modified();
        true
    }

    /// Removes the whole table.
    pub fn delete_table(&mut self) -> bool {
        let Some(place) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let Some((position, parent_path)) = place.table.split_last() else { return false };
        let position = *position;
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, parent_path) else {
            return false;
        };
        parent.children.remove(position);

        self.clamp_caret_after_edit();
        self.mark_modified();
        true
    }

    /// Sets the lines a table draws.
    pub fn set_table_borders(&mut self, borders: &TableBorders) -> bool {
        let Some(place) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &place.table) else {
            return false;
        };
        let properties = table_properties_of(table, prefix.as_deref());
        properties.remove_children_named(Some(read::W), "tblBorders");
        // The borders go first among the properties the schema allows here,
        // after the style and the width — `insert_ordered` knows where.
        edit::insert_ordered(
            properties,
            edit::table_borders_element(borders, prefix.as_deref()),
            TABLE_PROPERTY_ORDER,
        );

        self.mark_modified();
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

/// The order the schema requires for the children of `w:tblPr`.
const TABLE_PROPERTY_ORDER: &[&str] = &[
    "tblStyle",
    "tblpPr",
    "tblOverlap",
    "bidiVisual",
    "tblStyleRowBandSize",
    "tblStyleColBandSize",
    "tblW",
    "jc",
    "tblCellSpacing",
    "tblInd",
    "tblBorders",
    "shd",
    "tblLayout",
    "tblCellMar",
    "tblLook",
];

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
fn paragraphs_under(root: &Element, prefix: &[usize]) -> Option<(usize, usize)> {
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

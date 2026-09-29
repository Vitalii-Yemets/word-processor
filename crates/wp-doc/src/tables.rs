//! Tables: rows described by the sprms of the mark that ends them, and
//! tables inside tables.
//!
//! A table is paragraphs marked as in one. A cell ends with its own mark, and
//! a row with a mark of its own whose sprms describe the row: where each
//! cell's edges are, and for each cell how it is merged, its four lines, its
//! colour and where its text sits up and down it; the table's own lines; the
//! row's height, and whether it is a header. A table inside a cell is the
//! same again one level deeper, with a number on each paragraph saying how
//! deep, and its cells and rows ended by paragraph marks that say so. So each
//! level is built apart, and a table deeper than the paragraph now being read
//! is finished and put in the cell it is in.

use wp_docx::model::{Block, Table, TableBorders, TableCell, TableRow, TextDirection};
use wp_docx::table_properties::CellAlignment;

use crate::format::{border, border_80, shading, shading_80};
use crate::sprm::{self, Sprm};

/// A cell's width when nothing says one.
const USUAL_CELL: i32 = 1440;

/// How a cell is merged with its neighbour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Merge {
    #[default]
    None,
    /// The first of several merged cells.
    First,
    /// One merged into the cell before it, across or above.
    Continue,
}

#[derive(Clone, Debug, Default)]
struct CellDefinition {
    right: i32,
    horizontal: Merge,
    vertical: Merge,
    borders: TableBorders,
    shading: Option<String>,
    alignment: CellAlignment,
    direction: TextDirection,
}

/// A row as the mark that ends it describes it.
#[derive(Clone, Debug, Default)]
pub(crate) struct RowDefinition {
    left: i32,
    cells: Vec<CellDefinition>,
    borders: TableBorders,
    header: bool,
    height: Option<i32>,
    exact: bool,
}

impl RowDefinition {
    /// The row the sprms describe, each laid over what the ones before it
    /// said.
    pub fn from_sprms(sprms: &[Sprm<'_>]) -> Self {
        let mut row = Self::default();
        for sprm in sprms {
            match sprm.code {
                sprm::T_DEFINITION => row.define(sprm.operand),
                sprm::T_BORDERS_80 => {
                    let side = |index: usize| sprm.operand.get(index * 4..).and_then(border_80);
                    row.borders = TableBorders {
                        top: side(0),
                        start: side(1),
                        bottom: side(2),
                        end: side(3),
                        inside_horizontal: side(4),
                        inside_vertical: side(5),
                    };
                }
                sprm::T_BORDERS => {
                    let side = |index: usize| sprm.operand.get(index * 8..).and_then(border);
                    row.borders = TableBorders {
                        top: side(0),
                        start: side(1),
                        bottom: side(2),
                        end: side(3),
                        inside_horizontal: side(4),
                        inside_vertical: side(5),
                    };
                }
                sprm::T_SET_BORDER_80 | sprm::T_SET_BORDER => {
                    let (Some(&first), Some(&last), Some(&sides)) =
                        (sprm.operand.first(), sprm.operand.get(1), sprm.operand.get(2))
                    else {
                        continue;
                    };
                    let line = if sprm.code == sprm::T_SET_BORDER {
                        sprm.operand.get(3..).and_then(border)
                    } else {
                        sprm.operand.get(3..).and_then(border_80)
                    };
                    for cell in row.cells_in(first, last) {
                        if sides & 0x01 != 0 {
                            cell.borders.top = line.clone();
                        }
                        if sides & 0x02 != 0 {
                            cell.borders.start = line.clone();
                        }
                        if sides & 0x04 != 0 {
                            cell.borders.bottom = line.clone();
                        }
                        if sides & 0x08 != 0 {
                            cell.borders.end = line.clone();
                        }
                    }
                }
                sprm::T_SHADING_80 => {
                    for (cell, two) in row.cells.iter_mut().zip(sprm.operand.chunks_exact(2)) {
                        cell.shading = shading_80(u16::from_le_bytes([two[0], two[1]]));
                    }
                }
                sprm::T_SHADING | sprm::T_SHADING_2ND | sprm::T_SHADING_3RD => {
                    let from = match sprm.code {
                        sprm::T_SHADING => 0,
                        sprm::T_SHADING_2ND => 22,
                        _ => 44,
                    };
                    for (cell, ten) in
                        row.cells.iter_mut().skip(from).zip(sprm.operand.chunks_exact(10))
                    {
                        cell.shading = shading(ten);
                    }
                }
                sprm::T_SET_SHADING_80 => {
                    let (Some(&first), Some(&last), Some(two)) =
                        (sprm.operand.first(), sprm.operand.get(1), sprm.operand.get(2..4))
                    else {
                        continue;
                    };
                    let colour = shading_80(u16::from_le_bytes([two[0], two[1]]));
                    for cell in row.cells_in(first, last) {
                        cell.shading = colour.clone();
                    }
                }
                sprm::T_VERTICAL_MERGE => {
                    let (Some(&index), Some(&merge)) = (sprm.operand.first(), sprm.operand.get(1))
                    else {
                        continue;
                    };
                    if let Some(cell) = row.cells.get_mut(usize::from(index)) {
                        cell.vertical = vertical_merge(merge);
                    }
                }
                sprm::T_VERTICAL_ALIGN => {
                    let (Some(&first), Some(&last), Some(&alignment)) =
                        (sprm.operand.first(), sprm.operand.get(1), sprm.operand.get(2))
                    else {
                        continue;
                    };
                    for cell in row.cells_in(first, last) {
                        cell.alignment = cell_alignment(alignment);
                    }
                }
                sprm::T_MERGE | sprm::T_SPLIT => {
                    let (Some(&first), Some(&last)) = (sprm.operand.first(), sprm.operand.get(1))
                    else {
                        continue;
                    };
                    let merging = sprm.code == sprm::T_MERGE;
                    for (index, cell) in row.cells_in(first, last.saturating_add(1)).enumerate() {
                        cell.horizontal = match (merging, index) {
                            (false, _) => Merge::None,
                            (true, 0) => Merge::First,
                            (true, _) => Merge::Continue,
                        };
                    }
                }
                sprm::T_ROW_HEIGHT => {
                    let height = i32::from(sprm.i16());
                    row.height = (height != 0).then_some(height.abs());
                    row.exact = height < 0;
                }
                sprm::T_HEADER => row.header = sprm.on(),
                _ => {}
            }
        }
        row
    }

    /// The row's definition proper: how many cells, the edges between them,
    /// and a description of each — which may stop short, the rest being
    /// plain cells.
    fn define(&mut self, operand: &[u8]) {
        let Some(&count) = operand.first() else { return };
        let count = usize::from(count);
        let edge = |index: usize| {
            operand
                .get(1 + index * 2..3 + index * 2)
                .map(|two| i32::from(i16::from_le_bytes([two[0], two[1]])))
        };
        let Some(left) = edge(0) else { return };
        self.left = left;
        let described = 1 + (count + 1) * 2;
        self.cells = (0..count)
            .map(|index| {
                let right = edge(index + 1).unwrap_or(left + USUAL_CELL * (index as i32 + 1));
                let mut cell = CellDefinition { right, ..CellDefinition::default() };
                if let Some(tc) = operand.get(described + index * 20..described + index * 20 + 20) {
                    let flags = u16::from_le_bytes([tc[0], tc[1]]);
                    cell.horizontal = match flags & 0x0003 {
                        1 => Merge::First,
                        2 | 3 => Merge::Continue,
                        _ => Merge::None,
                    };
                    cell.direction = match (flags >> 2) & 0x07 {
                        1 | 5 => TextDirection::Down,
                        3 => TextDirection::Up,
                        _ => TextDirection::Horizontal,
                    };
                    cell.vertical = vertical_merge(((flags >> 5) & 0x03) as u8);
                    cell.alignment = cell_alignment(((flags >> 7) & 0x03) as u8);
                    cell.borders = TableBorders {
                        top: border_80(&tc[4..8]),
                        start: border_80(&tc[8..12]),
                        bottom: border_80(&tc[12..16]),
                        end: border_80(&tc[16..20]),
                        ..TableBorders::default()
                    };
                }
                cell
            })
            .collect();
    }

    /// The cells from one number up to, not including, another.
    fn cells_in(&mut self, first: u8, last: u8) -> impl Iterator<Item = &mut CellDefinition> {
        let (first, last) = (usize::from(first), usize::from(last));
        self.cells
            .iter_mut()
            .enumerate()
            .filter(move |(index, _)| *index >= first && *index < last)
            .map(|(_, cell)| cell)
    }
}

fn vertical_merge(value: u8) -> Merge {
    match value {
        1 => Merge::Continue,
        3 => Merge::First,
        _ => Merge::None,
    }
}

fn cell_alignment(value: u8) -> CellAlignment {
    match value {
        1 => CellAlignment::Middle,
        2 => CellAlignment::Bottom,
        _ => CellAlignment::Top,
    }
}

/// One table being built.
#[derive(Debug, Default)]
pub(crate) struct Level {
    table: Table,
    /// Each finished row's cells' left and right edges, for the columns.
    edges: Vec<Vec<(i32, i32)>>,
    row: Vec<TableCell>,
    pub cell_blocks: Vec<Block>,
}

impl Level {
    /// A cell's paragraphs are all in: it is a cell.
    pub fn end_cell(&mut self) {
        let blocks = core::mem::take(&mut self.cell_blocks);
        self.row.push(TableCell { blocks, ..TableCell::default() });
    }

    /// The row ends: its cells take what the description says about them.
    pub fn finish_row(&mut self, definition: &RowDefinition) {
        if !self.cell_blocks.is_empty() {
            self.end_cell();
        }
        let cells = core::mem::take(&mut self.row);
        if cells.is_empty() {
            return;
        }
        let mut kept: Vec<TableCell> = Vec::new();
        let mut edges: Vec<(i32, i32)> = Vec::new();
        let mut left = definition.left;
        for (index, mut cell) in cells.into_iter().enumerate() {
            let described = definition.cells.get(index);
            let right = described
                .map(|cell| cell.right)
                .filter(|right| *right > left)
                .unwrap_or(left + USUAL_CELL);
            if let Some(described) = described {
                // A cell merged into the one before it widens that one.
                if described.horizontal == Merge::Continue && !kept.is_empty() {
                    if let (Some(edge), Some(previous)) = (edges.last_mut(), kept.last_mut()) {
                        edge.1 = right;
                        previous.width = Some(right - edge.0);
                        if !cell.blocks.iter().all(|block| block.plain_text().is_empty()) {
                            previous.blocks.extend(cell.blocks);
                        }
                    }
                    left = right;
                    continue;
                }
                cell.borders = described.borders.clone();
                cell.shading = described.shading.clone();
                cell.merged_upwards = described.vertical == Merge::Continue;
                cell.vertical = described.alignment;
                cell.direction = described.direction;
            }
            cell.width = Some(right - left);
            edges.push((left, right));
            kept.push(cell);
            left = right;
        }
        if self.table.rows.is_empty() {
            self.table.borders = definition.borders.clone();
        }
        self.table.rows.push(TableRow {
            cells: kept,
            height: definition.height,
            height_exact: definition.exact,
            is_header: definition.header,
        });
        self.edges.push(edges);
    }

    /// The table is finished: its columns are every edge any row has.
    pub fn into_table(mut self) -> Option<Table> {
        if !self.cell_blocks.is_empty() || !self.row.is_empty() {
            self.finish_row(&RowDefinition::default());
        }
        if self.table.rows.is_empty() {
            return None;
        }
        let mut all: Vec<i32> = self.edges.iter().flatten().flat_map(|&(l, r)| [l, r]).collect();
        all.sort_unstable();
        all.dedup();
        self.table.grid = all.windows(2).map(|pair| pair[1] - pair[0]).collect();
        for (row, edges) in self.table.rows.iter_mut().zip(&self.edges) {
            for (cell, &(left, right)) in row.cells.iter_mut().zip(edges) {
                let from = all.iter().position(|edge| *edge == left).unwrap_or(0);
                let to = all.iter().position(|edge| *edge == right).unwrap_or(from + 1);
                cell.span = u32::try_from(to.saturating_sub(from)).unwrap_or(1).max(1);
            }
        }
        Some(self.table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_its_edges_its_cells_and_their_merges() {
        // Three cells: the first merged down into from above, the second
        // with a double red line along its top, the third over two columns
        // of the grid another row has.
        let mut operand = vec![3];
        for edge in [-108i16, 1332, 2772, 5652] {
            operand.extend_from_slice(&edge.to_le_bytes());
        }
        let mut first = [0u8; 20];
        first[0] = 1 << 5;
        let mut second = [0u8; 20];
        second[4..8].copy_from_slice(&[12, 3, 6, 0]);
        operand.extend_from_slice(&first);
        operand.extend_from_slice(&second);
        let mut group = sprm::T_DEFINITION.to_le_bytes().to_vec();
        group.extend_from_slice(&(operand.len() as u16 + 1).to_le_bytes());
        group.extend_from_slice(&operand);
        let definition = RowDefinition::from_sprms(&sprm::parse(&group));
        assert_eq!(definition.cells.len(), 3);
        assert_eq!(definition.cells[0].vertical, Merge::Continue);
        let top = definition.cells[1].borders.top.clone().expect("a line");
        assert_eq!((top.style.as_str(), top.color.as_deref()), ("double", Some("FF0000")));
        assert_eq!(definition.cells[2].right, 5652);

        let mut level = Level::default();
        for _ in 0..3 {
            level.cell_blocks.push(Block::Paragraph(wp_docx::model::Paragraph::text("x")));
            level.end_cell();
        }
        level.finish_row(&definition);
        let table = level.into_table().expect("a table");
        assert!(table.rows[0].cells[0].merged_upwards);
        assert_eq!(table.grid, vec![1440, 1440, 2880]);
    }
}

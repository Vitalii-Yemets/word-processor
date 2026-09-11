//! How wide a table's columns come out: Word's AutoFit.
//!
//! # The three, and where they come from
//!
//! A table says two things about its width — `w:tblW`, the width it would like
//! to be, and `w:tblLayout`, whether its columns may be worked out at all — and
//! Word turns them into one question with three answers. See
//! [`wp_docx::model::TableFit`].
//!
//! * **Fixed column width.** The grid is the geometry and nothing in the cells
//!   changes it. This is what every table here did before, and it is still what
//!   a table says `w:tblLayout w:type="fixed"` to get.
//! * **Fit to window.** The table is a stated part of the text area — all of it,
//!   usually — and the columns share that width out in the proportions they
//!   would have had.
//! * **Fit to contents.** The columns are as wide as what is in them, so the
//!   table grows and shrinks as it is typed in.
//!
//! # Measuring a cell with nothing to break it against
//!
//! Every other measuring pass in this program is given a width first: a line is
//! broken where it reaches the edge, so the edge has to be known. Fitting to
//! contents is the other way round — the width is the answer, not the question
//! — so a cell is measured by building its items and never breaking them.
//!
//! That gives two numbers per cell, and they are the two any table algorithm
//! needs:
//!
//! * how wide it *must* be, which is its widest single item — the longest word,
//!   the widest picture. Narrower than that and something spills out of its
//!   column.
//! * how wide it *would like* to be, which is all its items in one line. Wider
//!   than that is room it has no use for.
//!
//! A column takes the largest of each over its cells. If every column can have
//! what it would like, it does, and the table is as wide as its contents need —
//! which is what fitting to contents means. If they cannot, every column gets
//! what it must have and what is left over is shared out in proportion to what
//! each still wanted, which is what every table layout worth the name does with
//! a table too wide for the paper.
//!
//! # A cell that states a width still gets it
//!
//! `w:tcW` is a *preferred* width, not a measurement, and Word writes one on
//! every cell of every table it makes. So a stated width is a floor: the column
//! is at least that wide, and the content is what can make it wider. That is
//! why a table from Word keeps the shape it had there, and why **AutoFit
//! Contents** is the command that clears those preferences — with nothing
//! preferred, the text alone decides, and the table hugs it.
//!
//! # What this costs
//!
//! A pass over the text of every cell of every fitted table, every time the
//! document is laid out again. The measuring is items only — no line breaking,
//! no placing, no glyph positions kept — but it is not free, and on a document
//! of many large tables it will be felt. Nothing is cached yet; see **B6**.

use wp_docx::model::{Block, Table, TableFit, TableRow};
use wp_docx::Document;

use crate::layout::{LayoutEngine, TWIPS_PER_POINT};

/// What one column has to be, and what it would like to be, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Need {
    /// Narrower than this and something spills out.
    pub least: f32,
    /// Wider than this is room with nothing in it.
    pub wanted: f32,
}

impl Need {
    /// The larger of two needs, side by side in the same column.
    fn beside(self, other: Self) -> Self {
        Self { least: self.least.max(other.least), wanted: self.wanted.max(other.wanted) }
    }

    /// One need after another, down the same line.
    fn after(self, other: Self) -> Self {
        Self { least: self.least.max(other.least), wanted: self.wanted + other.wanted }
    }
}

impl LayoutEngine<'_> {
    /// How wide each column of a table is drawn, in pixels.
    pub(crate) fn column_widths(
        &mut self,
        table: &Table,
        document: &Document,
        available: f32,
        scale: f32,
    ) -> Vec<f32> {
        let columns = count_columns(table);
        if columns == 0 {
            return Vec::new();
        }

        match table.fit {
            TableFit::Fixed => fixed_widths(table, columns, available, scale),
            TableFit::Window(percent) => {
                // The table is that much of the text area, and the columns take
                // it in the shares they would otherwise have had.
                let target = (available * percent.clamp(1, 100) as f32 / 100.0).max(1.0);
                let shares = self.wanted_widths(table, document, columns, target, scale);
                spread(&shares, target)
            }
            TableFit::Contents => {
                let needs = self.needs_of(table, document, columns, available, scale);
                fit_to_contents(&needs, available)
            }
        }
    }

    /// What each column would like to be, for sharing a fixed width out.
    ///
    /// The grid is believed where there is one, because a table being fitted to
    /// the window is being asked about its total width and not about its
    /// proportions — Word keeps those. Where there is no grid the content
    /// decides them.
    fn wanted_widths(
        &mut self,
        table: &Table,
        document: &Document,
        columns: usize,
        available: f32,
        scale: f32,
    ) -> Vec<f32> {
        let grid = grid_widths(table, columns, scale);
        if grid.iter().any(|width| *width > 0.0) {
            return grid;
        }
        self.needs_of(table, document, columns, available, scale)
            .iter()
            .map(|need| need.wanted)
            .collect()
    }

    /// What every column of the table needs, measured from what is in it.
    fn needs_of(
        &mut self,
        table: &Table,
        document: &Document,
        columns: usize,
        available: f32,
        scale: f32,
    ) -> Vec<Need> {
        let mut needs = vec![Need::default(); columns];
        let (margin_start, margin_end) = crate::layout::cell_margins(table, scale);
        let margins = margin_start + margin_end;

        for row in &table.rows {
            let mut at = 0usize;
            for cell in &row.cells {
                let span = (cell.span.max(1) as usize).min(columns.saturating_sub(at)).max(1);
                if at >= columns {
                    break;
                }

                // A cell that continues the one above holds nothing of its own,
                // so it asks for nothing.
                let mut need = if cell.merged_upwards {
                    Need::default()
                } else {
                    let mut need = self.blocks_need(&cell.blocks, document, available, scale);
                    // A cell whose text is turned asks the other way round: what
                    // the column has to hold is how deep its lines stack, not
                    // how long the text is. Asking for the length would make the
                    // column as wide as the heading is long, which is the whole
                    // thing a turned heading is there to avoid.
                    if cell.direction.is_turned() {
                        let depth = self.blocks_depth(&cell.blocks, document);
                        need = Need { least: depth, wanted: depth };
                    }
                    need.least += margins;
                    need.wanted += margins;
                    need
                };

                // A width the cell states is a floor rather than an answer: the
                // column is at least that wide, and the content can want more.
                if let Some(stated) = stated_width(cell.width, available, scale) {
                    need.least = need.least.max(stated);
                    need.wanted = need.wanted.max(stated);
                }

                // A cell covering several columns is asking of all of them
                // together, so it asks a share of each. Anything already needed
                // by one of them stands.
                let share =
                    Need { least: need.least / span as f32, wanted: need.wanted / span as f32 };
                for column in &mut needs[at..at + span] {
                    *column = column.beside(share);
                }
                at += span;
            }
        }
        needs
    }

    /// What a run of blocks needs: the widest of them, since they sit one under
    /// another.
    fn blocks_need(
        &mut self,
        blocks: &[Block],
        document: &Document,
        available: f32,
        scale: f32,
    ) -> Need {
        let mut need = Need::default();
        for block in blocks {
            let of_block = match block {
                Block::Paragraph(paragraph) => {
                    let indents = ((paragraph.properties.indent_start.unwrap_or(0)
                        + paragraph.properties.indent_end.unwrap_or(0))
                        as f32
                        / TWIPS_PER_POINT
                        * scale)
                        .max(0.0);
                    let mut of_paragraph = self.paragraph_need(paragraph, document);
                    of_paragraph.least += indents;
                    of_paragraph.wanted += indents;
                    of_paragraph
                }
                // A table inside a cell needs whatever its own columns need.
                Block::Table(inner) => {
                    let columns = count_columns(inner);
                    if columns == 0 {
                        Need::default()
                    } else {
                        self.needs_of(inner, document, columns, available, scale)
                            .into_iter()
                            .fold(Need::default(), |total, column| total.after(column))
                    }
                }
            };
            need = need.beside(of_block);
        }
        need
    }

    /// How deep a run of blocks stacks: the sum of their line heights.
    ///
    /// What a cell whose text is turned needs across its column. One line of it
    /// where there is one paragraph, which is what a turned heading is.
    fn blocks_depth(&mut self, blocks: &[Block], document: &Document) -> f32 {
        let mut depth = 0.0f32;
        for block in blocks {
            depth += match block {
                Block::Paragraph(paragraph) => self.paragraph_depth(paragraph, document),
                // A table inside a turned cell is a corner too far: it asks for
                // nothing rather than for something wrong.
                Block::Table(_) => 0.0,
            };
        }
        depth.max(1.0)
    }

    /// How tall one line of a paragraph is.
    fn paragraph_depth(
        &mut self,
        paragraph: &wp_docx::model::Paragraph,
        document: &Document,
    ) -> f32 {
        let counters = self.saved_counters();
        let mut styles = Vec::new();
        let mut text = String::new();
        let items = self.build_items(paragraph, document, &mut styles, 0, &mut text);
        self.restore_counters(counters);

        items
            .iter()
            .filter_map(|item| styles.get(item.style))
            .map(|style| style.line_height)
            .fold(0.0f32, f32::max)
            // A paragraph with nothing in it still takes a line.
            .max(self.empty_line_height(paragraph, document))
    }

    /// And what one paragraph needs, from its items alone.
    fn paragraph_need(
        &mut self,
        paragraph: &wp_docx::model::Paragraph,
        document: &Document,
    ) -> Need {
        // The counting state is put back afterwards: measuring a numbered
        // paragraph must not use up the number it would be given when it is
        // really laid out. `place_table` guards its own measuring pass the same
        // way.
        let counters = self.saved_counters();
        let mut styles = Vec::new();
        let mut text = String::new();
        let items = self.build_items(paragraph, document, &mut styles, 0, &mut text);
        self.restore_counters(counters);

        let mut need = Need::default();
        for item in &items {
            need.wanted += item.width;
            // A space is not what makes a column too narrow: a line may be
            // broken at one, so it is never part of what must fit.
            if !item.is_space {
                need.least = need.least.max(item.width);
            }
        }
        need
    }
}

/// How many columns the table has: what its grid says, or the widest row.
pub(crate) fn count_columns(table: &Table) -> usize {
    if !table.grid.is_empty() {
        return table.grid.len();
    }
    table.rows.iter().map(columns_of_row).max().unwrap_or(0)
}

/// How many columns of the grid one row covers.
fn columns_of_row(row: &TableRow) -> usize {
    row.cells.iter().map(|cell| cell.span.max(1) as usize).sum()
}

/// The grid, in pixels, padded or trimmed to the number of columns.
fn grid_widths(table: &Table, columns: usize, scale: f32) -> Vec<f32> {
    let mut widths: Vec<f32> =
        table.grid.iter().map(|twips| (*twips).max(0) as f32 / TWIPS_PER_POINT * scale).collect();
    widths.resize(columns, 0.0);
    widths
}

/// A width the cell states for itself, in pixels.
///
/// `w:tcW` counts in twips when it says `dxa` and in fiftieths of a per cent
/// when it says `pct`; the model keeps the number and this reads it as twips,
/// which is what every table this program has met writes.
fn stated_width(width: Option<i32>, available: f32, scale: f32) -> Option<f32> {
    let twips = width?;
    if twips <= 0 {
        return None;
    }
    let points = twips as f32 / TWIPS_PER_POINT * scale;
    // Nothing states a width wider than the text area; one that does is asking
    // for a table that runs off the paper.
    Some(points.min(available))
}

/// The grid as it stands, squeezed if the table is wider than the paper.
fn fixed_widths(table: &Table, columns: usize, available: f32, scale: f32) -> Vec<f32> {
    let mut widths = grid_widths(table, columns, scale);
    if widths.iter().all(|width| *width <= 0.0) {
        // Without a grid there is nothing fixed about it; an even share is the
        // only answer that does not put every column in the same place.
        return vec![available / columns as f32; columns];
    }
    // A column the grid says nothing about takes an even share of what is left.
    let stated: f32 = widths.iter().sum();
    let missing = widths.iter().filter(|width| **width <= 0.0).count();
    if missing > 0 {
        let each = ((available - stated) / missing as f32).max(1.0);
        for width in widths.iter_mut().filter(|width| **width <= 0.0) {
            *width = each;
        }
    }

    let total: f32 = widths.iter().sum();
    if total > available && total > 0.0 {
        let factor = available / total;
        for width in &mut widths {
            *width *= factor;
        }
    }
    widths
}

/// Gives every column what it would like, where there is room for that.
///
/// Where there is not, every column gets what it must have and the rest is
/// shared out in proportion to what each still wanted. A table whose columns
/// cannot all have even that is squeezed, because a column off the edge of the
/// paper is worse than a narrow one.
fn fit_to_contents(needs: &[Need], available: f32) -> Vec<f32> {
    let wanted: f32 = needs.iter().map(|need| need.wanted).sum();
    if wanted <= available {
        // The whole point of fitting to contents: the table is as wide as what
        // is in it and no wider.
        return needs.iter().map(|need| need.wanted.max(1.0)).collect();
    }

    let least: f32 = needs.iter().map(|need| need.least).sum();
    if least >= available {
        return spread(
            &needs.iter().map(|need| need.least.max(1.0)).collect::<Vec<_>>(),
            available,
        );
    }

    let spare = available - least;
    let asking: f32 = needs.iter().map(|need| (need.wanted - need.least).max(0.0)).sum();
    needs
        .iter()
        .map(|need| {
            let share =
                if asking > 0.0 { (need.wanted - need.least).max(0.0) / asking } else { 0.0 };
            (need.least + spare * share).max(1.0)
        })
        .collect()
}

/// The same widths, scaled to add up to exactly what is given.
fn spread(widths: &[f32], target: f32) -> Vec<f32> {
    let total: f32 = widths.iter().sum();
    if total <= 0.0 {
        let columns = widths.len().max(1);
        return vec![target / columns as f32; widths.len()];
    }
    let factor = target / total;
    widths.iter().map(|width| (width * factor).max(1.0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needs(pairs: &[(f32, f32)]) -> Vec<Need> {
        pairs.iter().map(|(least, wanted)| Need { least: *least, wanted: *wanted }).collect()
    }

    #[test]
    fn a_table_that_fits_is_as_wide_as_its_contents() {
        let widths = fit_to_contents(&needs(&[(20.0, 60.0), (20.0, 40.0)]), 500.0);
        assert_eq!(widths, vec![60.0, 40.0]);
    }

    #[test]
    fn one_that_does_not_fit_keeps_what_each_column_must_have() {
        let widths = fit_to_contents(&needs(&[(50.0, 400.0), (50.0, 100.0)]), 200.0);
        assert!(
            widths.iter().all(|width| *width >= 50.0),
            "a column went below its least: {widths:?}"
        );
        assert!((widths.iter().sum::<f32>() - 200.0).abs() < 0.01, "{widths:?}");
        // What is left over goes where it was most wanted.
        assert!(widths[0] > widths[1]);
    }

    #[test]
    fn a_table_whose_words_will_not_fit_at_all_is_squeezed() {
        let widths = fit_to_contents(&needs(&[(300.0, 400.0), (300.0, 400.0)]), 200.0);
        assert!((widths.iter().sum::<f32>() - 200.0).abs() < 0.01, "{widths:?}");
    }

    #[test]
    fn fitting_to_the_window_keeps_the_proportions() {
        let widths = spread(&[100.0, 200.0, 100.0], 800.0);
        assert_eq!(widths, vec![200.0, 400.0, 200.0]);
    }

    #[test]
    fn a_grid_of_nothing_shares_the_room_out_evenly() {
        assert_eq!(spread(&[0.0, 0.0], 100.0), vec![50.0, 50.0]);
    }

    #[test]
    fn two_needs_side_by_side_take_the_larger() {
        let one = Need { least: 10.0, wanted: 50.0 };
        let other = Need { least: 30.0, wanted: 40.0 };
        assert_eq!(one.beside(other), Need { least: 30.0, wanted: 50.0 });
    }

    #[test]
    fn two_needs_in_a_line_add_up_what_they_want() {
        let one = Need { least: 10.0, wanted: 50.0 };
        let other = Need { least: 30.0, wanted: 40.0 };
        assert_eq!(one.after(other), Need { least: 30.0, wanted: 90.0 });
    }
}

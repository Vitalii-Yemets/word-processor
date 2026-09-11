//! The room inside a cell and the room between cells: Word's Table Options.

use wp_docx::model::{Block, Body, CellMargins, Paragraph, Table, TableCell, TableFit, TableRow};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A two-column table with the margins and spacing asked for.
///
/// The columns are fixed, so that a change to the room inside a cell shows as a
/// change to the room and not as a change to the width of the table.
fn document(margins: CellMargins, spacing: Option<i32>, cell: CellMargins) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Before")));

    let mut table = Table::from_rows(vec![TableRow::from_cells(vec![
        TableCell { margins: cell, ..TableCell::text("First") },
        TableCell::text("Second"),
    ])])
    .with_grid(vec![4000, 4000]);
    table.cell_margins = margins;
    table.cell_spacing = spacing;
    table.fit = TableFit::Fixed;
    body.blocks.push(Block::Table(Box::new(table)));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn pages(margins: CellMargins, spacing: Option<i32>, cell: CellMargins) -> Vec<Page> {
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(&document(margins, spacing, cell), PageMetrics::default())
}

/// Where the first letter of a cell's text sits.
fn first_letter(page: &Page, word: &str) -> (f32, f32) {
    let glyph = page
        .lines
        .iter()
        .find(|line| line.end_offset == word.len())
        .map(|line| &page.glyphs[line.glyphs.start])
        .expect("a line of that length");
    (glyph.x, glyph.baseline)
}

#[test]
fn the_room_at_the_sides_is_the_documents_and_not_the_layouts() {
    let narrow = pages(CellMargins::default(), None, CellMargins::default());
    let wide = pages(
        CellMargins { start: Some(720), ..CellMargins::default() },
        None,
        CellMargins::default(),
    );

    let (narrow_x, _) = first_letter(&narrow[0], "First");
    let (wide_x, _) = first_letter(&wide[0], "First");
    assert!(
        wide_x > narrow_x + 30.0,
        "half an inch of margin moved the text {}",
        wide_x - narrow_x
    );
}

#[test]
fn the_room_above_the_text_is_too() {
    let tight = pages(CellMargins::default(), None, CellMargins::default());
    let roomy = pages(
        CellMargins { top: Some(720), ..CellMargins::default() },
        None,
        CellMargins::default(),
    );

    let (_, tight_y) = first_letter(&tight[0], "First");
    let (_, roomy_y) = first_letter(&roomy[0], "First");
    assert!(roomy_y > tight_y + 30.0, "half an inch above the text moved it {}", roomy_y - tight_y);

    // And the row grew by it, rather than the text being pushed out of the cell.
    let height = |pages: &[Page]| pages[0].cells.iter().map(|cell| cell.height).fold(0.0, f32::max);
    assert!(height(&roomy) > height(&tight) + 30.0, "the row did not grow with the margin");
}

#[test]
fn a_cell_that_states_its_own_room_gets_it() {
    let pages = pages(
        CellMargins::default(),
        None,
        CellMargins { start: Some(720), ..CellMargins::default() },
    );
    let page = &pages[0];
    let (first, _) = first_letter(page, "First");
    let (second, _) = first_letter(page, "Second");

    let cells = &page.cells;
    let first_cell = cells[0];
    let second_cell = cells[1];
    assert!(
        first - first_cell.x > second - second_cell.x + 30.0,
        "the cell's own margin was not used"
    );
}

#[test]
fn room_between_the_cells_holds_them_apart() {
    let together = pages(CellMargins::default(), None, CellMargins::default());
    let apart = pages(CellMargins::default(), Some(240), CellMargins::default());

    let touching = &together[0].cells;
    assert!(
        (touching[0].x + touching[0].width - touching[1].x).abs() < 0.51,
        "cells with no spacing do not touch"
    );

    let spaced = &apart[0].cells;
    let gap = spaced[1].x - (spaced[0].x + spaced[0].width);
    assert!(gap > 10.0, "the cells are still touching: a gap of {gap}");
}

#[test]
fn the_room_between_them_comes_out_of_the_table_and_not_out_of_the_page() {
    // The grid is still the table's geometry: the spacing is taken from inside
    // each column rather than added to the table's width.
    let together = pages(CellMargins::default(), None, CellMargins::default());
    let apart = pages(CellMargins::default(), Some(240), CellMargins::default());

    let span = |pages: &[Page]| {
        let cells = &pages[0].cells;
        let left = cells.iter().map(|cell| cell.x).fold(f32::MAX, f32::min);
        let right = cells.iter().map(|cell| cell.x + cell.width).fold(f32::MIN, f32::max);
        right - left
    };
    assert!(span(&apart) < span(&together), "the table grew instead of the cells shrinking");
}

#[test]
fn a_spaced_row_is_taller_by_the_spacing() {
    let together = pages(CellMargins::default(), None, CellMargins::default());
    let apart = pages(CellMargins::default(), Some(240), CellMargins::default());

    let row = |pages: &[Page]| pages[0].cells.iter().map(|cell| cell.height).fold(0.0, f32::max);
    // The cells themselves are no taller; the row around them is.
    assert!((row(&apart) - row(&together)).abs() < 1.0, "the cells changed height");
}

#[test]
fn the_room_survives_being_saved_and_opened_again() {
    let margins =
        CellMargins { top: Some(100), start: Some(200), bottom: Some(300), end: Some(400) };
    let document = document(margins, Some(240), CellMargins { top: Some(50), ..margins });

    let bytes = document.save().expect("saving");
    let reopened = Document::open(&bytes).expect("reopening");
    let table = reopened
        .body()
        .blocks
        .into_iter()
        .find_map(|block| match block {
            Block::Table(table) => Some(*table),
            Block::Paragraph(_) => None,
        })
        .expect("a table");

    assert_eq!(table.cell_margins, margins);
    assert_eq!(table.cell_spacing, Some(240));
    assert_eq!(table.rows[0].cells[0].margins.top, Some(50));
    assert_eq!(table.rows[0].cells[1].margins, CellMargins::default());
}

#[test]
fn a_side_nobody_stated_is_words_own_default() {
    let margins = CellMargins { start: Some(200), ..CellMargins::default() };
    let (top, start, bottom, end) = margins.or_usual();
    assert_eq!(start, 200);
    assert_eq!(end, CellMargins::USUAL_SIDE);
    assert_eq!(top, CellMargins::USUAL_UP_AND_DOWN);
    assert_eq!(bottom, CellMargins::USUAL_UP_AND_DOWN);
}

#[test]
fn a_cells_own_room_covers_only_the_sides_it_states() {
    let table = CellMargins { top: Some(10), start: Some(20), bottom: None, end: Some(40) };
    let cell = CellMargins { top: Some(99), ..CellMargins::default() };
    let together = cell.over(table);

    assert_eq!(together.top, Some(99), "the cell's own side did not win");
    assert_eq!(together.start, Some(20), "a side the cell left alone did not come from the table");
    assert_eq!(together.bottom, None);
    assert_eq!(together.end, Some(40));
}

//! Joining and splitting table cells.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};

/// A document with one paragraph and then a table of the given size.
fn document(rows: usize, columns: usize) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("before")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document.set_caret(TextPosition::new(0, 6));
    assert!(document.insert_table(rows, columns));
    document
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn table_of(document: &Document) -> wp_docx::model::Table {
    document
        .body()
        .blocks
        .into_iter()
        .find_map(|block| match block {
            Block::Table(table) => Some(*table),
            Block::Paragraph(_) => None,
        })
        .expect("a table")
}

/// The first paragraph index inside the table, which is its first cell.
fn first_cell(document: &Document) -> usize {
    // The paragraph before the table is index 0; the table's cells follow.
    let _ = document;
    1
}

/// Selects from one cell to another by their paragraph indices.
fn select_cells(document: &mut Document, from: usize, to: usize) {
    document.move_caret(TextPosition::new(from, 0), false);
    document.move_caret(TextPosition::new(to, 0), true);
}

#[test]
fn merging_across_a_row_leaves_one_cell_covering_the_others() {
    let mut document = document(2, 3);
    let first = first_cell(&document);
    select_cells(&mut document, first, first + 2);
    assert!(document.merge_cells());

    let table = table_of(&round_trip(&document));
    assert_eq!(table.rows[0].cells.len(), 1, "the row should hold one cell now");
    assert_eq!(table.rows[0].cells[0].span, 3, "and it should cover all three columns");
    assert_eq!(table.rows[1].cells.len(), 3, "the row below is untouched");
}

#[test]
fn merging_down_a_column_leaves_the_lower_cells_continuing_the_first() {
    let mut document = document(3, 2);
    let first = first_cell(&document);
    // Down the first column: cells are numbered along each row, so the cell
    // below the first is two paragraphs on.
    select_cells(&mut document, first, first + 4);
    assert!(document.merge_cells());

    let table = table_of(&round_trip(&document));
    assert_eq!(table.rows.len(), 3);
    assert!(!table.rows[0].cells[0].merged_upwards, "the first starts the merge");
    assert!(table.rows[1].cells[0].merged_upwards, "the second continues it");
    assert!(table.rows[2].cells[0].merged_upwards, "and so does the third");
}

#[test]
fn merging_one_cell_with_itself_does_nothing() {
    let mut document = document(2, 2);
    assert!(!document.merge_cells(), "there is nothing to merge");
}

#[test]
fn a_merged_cell_can_be_split_again() {
    let mut document = document(2, 3);
    let first = first_cell(&document);
    select_cells(&mut document, first, first + 2);
    document.merge_cells();

    document.set_caret(TextPosition::new(first, 0));
    assert!(document.split_cells(3, 1, false));

    let table = table_of(&round_trip(&document));
    assert_eq!(table.rows[0].cells.len(), 3);
    assert!(table.rows[0].cells.iter().all(|cell| cell.span == 1));
}

#[test]
fn splitting_into_one_column_and_one_row_does_nothing() {
    let mut document = document(2, 2);
    assert!(!document.split_cells(1, 1, false), "one by one is no split at all");
}

#[test]
fn merging_keeps_the_table_the_width_it_was() {
    let mut document = document(2, 3);
    let before: i32 = table_of(&document).grid.iter().sum();
    let first = first_cell(&document);
    select_cells(&mut document, first, first + 2);
    document.merge_cells();

    let after: i32 = table_of(&round_trip(&document)).grid.iter().sum();
    assert_eq!(before, after, "the grid should not have changed at all");
}

#[test]
fn merging_can_be_undone() {
    let mut document = document(2, 3);
    let first = first_cell(&document);
    select_cells(&mut document, first, first + 2);
    document.merge_cells();
    assert!(document.undo());
    assert_eq!(table_of(&document).rows[0].cells.len(), 3);
}

#[test]
fn tab_passes_over_the_cells_a_merge_down_hid() {
    // Word's Tab goes to the next cell that is drawn. The continuations of a
    // merge down are in the file and not on the page.
    let mut document = filled(3, 3);
    select_block(&mut document, (0, 2), (0, 0));
    assert!(document.merge_cells());

    document.set_caret(TextPosition::new(document.cell_paragraphs(0, 2).expect("a cell").0, 0));
    assert_eq!(document.cell_beside(true), Some((1, 1)), "Tab went into a continuation");
    document.set_caret(TextPosition::new(document.cell_paragraphs(1, 1).expect("a cell").0, 0));
    assert_eq!(document.cell_beside(false), Some((0, 2)), "Shift+Tab went into one");
}

#[test]
fn a_table_merged_into_one_cell_has_nowhere_for_tab_to_go() {
    // Which is where the editor adds a row, as it does after the last cell of
    // any table.
    let mut document = filled(3, 3);
    select_block(&mut document, (0, 2), (0, 2));
    assert!(document.merge_cells());
    document.set_caret(TextPosition::new(first_cell(&document), 0));
    assert_eq!(document.cell_beside(true), None, "Tab found a cell nobody can see");
    assert_eq!(document.cell_beside(false), None);
}

#[test]
fn columns_can_be_given_equal_widths() {
    let mut document = document(2, 3);
    // Adding a column halves one of them, so they start out uneven.
    document.set_caret(TextPosition::new(first_cell(&document), 0));
    document.insert_table_column(true);

    let uneven = table_of(&document).grid;
    assert!(uneven.iter().any(|width| *width != uneven[0]), "they should start uneven");

    assert!(document.distribute_columns());
    let even = table_of(&round_trip(&document)).grid;
    assert!(even.iter().all(|width| (*width - even[0]).abs() <= 1), "got {even:?}");
}

#[test]
fn nothing_happens_when_the_caret_is_not_in_a_table() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("plain")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");

    assert!(!document.merge_cells());
    assert!(!document.split_cells(2, 1, false));
    assert!(document.split_defaults().is_none());
    assert!(!document.distribute_columns());
    assert!(document.selected_cells().is_none());
}

/// A document whose table has every cell filled in with where it is: "00" in
/// the first, "01" beside it, and so on.
fn filled(rows: usize, columns: usize) -> Document {
    let mut document = document(rows, columns);
    for row in 0..rows {
        for column in 0..columns {
            // A new table holds one paragraph per cell, one after another.
            let paragraph = first_cell(&document) + row * columns + column;
            document.set_caret(TextPosition::new(paragraph, 0));
            assert!(document.type_text(&format!("{row}{column}")), "the cell would not take text");
        }
    }
    document.set_caret(TextPosition::new(first_cell(&document), 0));
    document
}

/// Takes a block of cells whole, as a drag across them or the Select menu
/// does: one stretch per cell, and the block said in so many words.
fn select_block(document: &mut Document, rows: (usize, usize), columns: (usize, usize)) {
    if document.table_here().is_none() {
        document.set_caret(TextPosition::new(first_cell(document), 0));
    }
    let mut stretches = Vec::new();
    for row in rows.0..=rows.1 {
        for column in columns.0..=columns.1 {
            if let Some(stretch) = document.cell_text_range(row, column) {
                stretches.push(stretch);
            }
        }
    }
    document.set_selections(&stretches);
    let table = document.table_here().expect("the selection is in the table").table;
    document.select_cell_block(wp_docx::cells::CellRange { table, rows, columns });
}

/// What each paragraph of one cell says.
fn texts(table: &wp_docx::model::Table, row: usize, column: usize) -> Vec<String> {
    table.rows[row].cells[column]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph.plain_text()),
            Block::Table(_) => None,
        })
        .collect()
}

/// Every word of every row, one row after another.
fn every_text(table: &wp_docx::model::Table) -> Vec<Vec<Vec<String>>> {
    (0..table.rows.len())
        .map(|row| {
            (0..table.rows[row].cells.len()).map(|column| texts(table, row, column)).collect()
        })
        .collect()
}

#[test]
fn merging_across_keeps_the_paragraphs_of_every_cell_in_order() {
    // Word keeps what each merged cell held as paragraphs of the one they
    // became. Merging sideways took the cells to the right out of the row
    // with everything in them.
    let mut document = filled(2, 3);
    select_block(&mut document, (0, 0), (0, 2));
    assert!(document.merge_cells());

    let table = table_of(&round_trip(&document));
    assert_eq!(texts(&table, 0, 0), ["00", "01", "02"], "the merged cell lost what it was given");
    assert_eq!(texts(&table, 1, 2), ["12"], "a cell outside the merge changed");
}

#[test]
fn merging_down_keeps_the_paragraphs_and_leaves_the_continuations_empty() {
    // Down a column the lower cells stay in their rows as continuations, and
    // nothing draws what is in one — so what they held has to move up into
    // the cell that is drawn, and they are left with the one empty paragraph
    // Word writes in them.
    let mut document = filled(3, 2);
    select_block(&mut document, (0, 2), (0, 0));
    assert!(document.merge_cells());

    let table = table_of(&round_trip(&document));
    assert_eq!(texts(&table, 0, 0), ["00", "10", "20"], "the merged cell lost what it was given");
    for row in 1..3 {
        assert!(table.rows[row].cells[0].merged_upwards, "row {row} does not continue the merge");
        assert_eq!(texts(&table, row, 0), [""], "row {row}'s continuation still holds text");
    }
}

#[test]
fn merging_a_block_takes_the_paragraphs_across_and_then_down() {
    let mut document = filled(3, 3);
    select_block(&mut document, (0, 1), (0, 1));
    assert!(document.merge_cells());

    let table = table_of(&round_trip(&document));
    assert_eq!(texts(&table, 0, 0), ["00", "01", "10", "11"], "Word's reading order");
    assert_eq!(table.rows[0].cells[0].span, 2);
    assert!(table.rows[1].cells[0].merged_upwards);
    assert_eq!(table.rows[1].cells[0].span, 2, "the continuation is as wide as the cell");
    assert_eq!(texts(&table, 1, 1), ["12"], "a cell outside the block moved");
}

#[test]
fn an_empty_cell_adds_no_empty_paragraph_to_a_merge() {
    let mut document = document(1, 3);
    for (column, text) in [(0, "A"), (2, "C")] {
        document.set_caret(TextPosition::new(first_cell(&document) + column, 0));
        document.type_text(text);
    }
    select_block(&mut document, (0, 0), (0, 2));
    assert!(document.merge_cells());
    assert_eq!(texts(&table_of(&round_trip(&document)), 0, 0), ["A", "C"]);
}

#[test]
fn merging_puts_the_caret_at_the_start_of_the_merged_cell() {
    // It was left on the paragraph it had been on, which after the merge was
    // the first paragraph of the next cell along.
    let mut document = filled(2, 3);
    select_block(&mut document, (0, 0), (0, 1));
    assert!(document.merge_cells());

    let place = document.table_here().expect("the caret left the table");
    assert_eq!((place.row, place.column), (0, 0), "the caret is not in the merged cell");
    let (first, _) = document.cell_paragraphs(0, 0).expect("a merged cell");
    assert_eq!(document.caret(), TextPosition::new(first, 0), "and not at its start");
}

#[test]
fn merging_the_whole_table_leaves_the_caret_in_it() {
    // The worst case: every cell merged, and the caret on a paragraph number
    // that had come to mean the paragraph after the table.
    let mut document = filled(3, 3);
    select_block(&mut document, (0, 2), (0, 2));
    assert!(document.merge_cells());

    let place = document.table_here().expect("the caret was thrown out of the table");
    assert_eq!((place.row, place.column), (0, 0));
    let table = table_of(&round_trip(&document));
    assert_eq!(
        texts(&table, 0, 0),
        ["00", "01", "02", "10", "11", "12", "20", "21", "22"],
        "the merged cell lost what it was given"
    );
}

#[test]
fn undoing_a_merge_brings_back_every_cell_and_its_text() {
    let mut document = filled(3, 3);
    let before = every_text(&table_of(&document));
    select_block(&mut document, (0, 2), (0, 1));
    assert!(document.merge_cells());
    assert!(document.undo(), "there was nothing to undo");
    assert_eq!(every_text(&table_of(&document)), before, "one undo did not bring it all back");
}

#[test]
fn a_plain_cell_can_be_split_into_columns_and_rows() {
    // Word splits any cell. Splitting it across gives the grid a column and
    // the cells above and below it a span over both halves; splitting it
    // down gives the table a row and every other cell of the row a merge
    // down across both.
    let mut document = filled(2, 2);
    let width: i32 = table_of(&document).grid.iter().sum();
    document.set_caret(TextPosition::new(first_cell(&document), 0));
    assert!(document.split_cells(2, 2, false), "the cell would not split");

    let saved = round_trip(&document);
    let table = table_of(&saved);
    assert_eq!(table.grid.len(), 3, "the grid did not gain a column");
    assert_eq!(table.grid.iter().sum::<i32>(), width, "the table changed width");
    assert_eq!(table.rows.len(), 3, "the table did not gain a row");
    assert_eq!(table.rows[0].cells.iter().map(|cell| cell.span).collect::<Vec<_>>(), [1, 1, 1]);
    assert_eq!(table.rows[1].cells.iter().map(|cell| cell.span).collect::<Vec<_>>(), [1, 1, 1]);
    assert!(!table.rows[0].cells[2].merged_upwards, "the cell beside starts the merge down");
    assert!(table.rows[1].cells[2].merged_upwards, "and the row added continues it");
    assert!(!table.rows[1].cells[0].merged_upwards, "the halves are cells of their own");
    assert_eq!(table.rows[2].cells[0].span, 2, "the cell below covers both new columns");
    assert_eq!(texts(&table, 0, 0), ["00"], "the text stays in the first of them");
    assert_eq!(texts(&table, 0, 2), ["01"], "the cell beside keeps its text");
    let xml = saved.package().xml_part("word/document.xml").expect("there").expect("text");
    assert!(xml.contains(r#"<w:vMerge w:val="restart"/>"#), "{xml}");
}

#[test]
fn splitting_a_whole_table_merged_into_one_cell_leaves_no_merge_behind() {
    // Splitting it gave three cells in the first row and left the lower rows
    // continuing a cell that was no longer there: a ragged table.
    let mut document = filled(3, 3);
    let before = every_text(&table_of(&document));
    select_block(&mut document, (0, 2), (0, 2));
    assert!(document.merge_cells());
    document.set_caret(TextPosition::new(first_cell(&document), 0));
    assert!(document.split_cells(3, 3, false), "the cell would not split");

    let saved = round_trip(&document);
    let table = table_of(&saved);
    assert_eq!(table.rows.len(), 3);
    for (number, row) in table.rows.iter().enumerate() {
        assert_eq!(row.cells.len(), 3, "row {number} is ragged");
        assert!(
            row.cells.iter().all(|cell| cell.span == 1 && !cell.merged_upwards),
            "row {number}"
        );
    }
    assert_eq!(every_text(&table), before, "every cell should have its own paragraph back");
    let xml = saved.package().xml_part("word/document.xml").expect("there").expect("text");
    assert!(!xml.contains("vMerge"), "a merge was left behind: {xml}");
}

#[test]
fn the_split_dialog_starts_from_what_the_selection_covers() {
    use wp_docx::cells::SplitDefaults;
    let mut document = filled(3, 3);
    assert_eq!(
        document.split_defaults(),
        Some(SplitDefaults { columns: 2, rows: 1, several: false }),
        "one plain cell: Word's two columns and one row"
    );

    select_block(&mut document, (1, 2), (0, 1));
    assert_eq!(
        document.split_defaults(),
        Some(SplitDefaults { columns: 2, rows: 2, several: true }),
        "several cells: the rectangle, and the offer to merge them"
    );

    assert!(document.merge_cells());
    assert_eq!(
        document.split_defaults(),
        Some(SplitDefaults { columns: 2, rows: 2, several: false }),
        "a merged cell: what it covers, so that OK takes the merge apart"
    );
}

#[test]
fn a_cell_merged_down_is_split_only_into_rows_that_divide_it() {
    let mut document = filled(3, 2);
    select_block(&mut document, (0, 2), (0, 0));
    assert!(document.merge_cells());
    assert!(!document.split_cells(1, 2, false), "three rows cannot be shared into two");

    // One row: the cell stays three rows tall and becomes two side by side.
    assert!(document.split_cells(2, 1, false));
    let table = table_of(&round_trip(&document));
    assert_eq!(table.rows.len(), 3, "the table gained or lost a row");
    for row in 0..3 {
        assert_eq!(table.rows[row].cells.len(), 3, "row {row} is ragged");
        let continues = row > 0;
        for column in 0..2 {
            assert_eq!(table.rows[row].cells[column].merged_upwards, continues, "{row},{column}");
        }
    }
    assert_eq!(texts(&table, 0, 0), ["00", "10"], "the first new cell takes the first share");
    assert_eq!(texts(&table, 0, 1), ["20"]);
}

#[test]
fn several_cells_are_each_split_when_they_are_not_merged_first() {
    let mut document = filled(2, 3);
    select_block(&mut document, (0, 0), (0, 1));
    assert!(document.split_cells(2, 1, false));

    let table = table_of(&round_trip(&document));
    assert_eq!(table.grid.len(), 5, "each of the two cells gave the grid a column");
    assert_eq!(table.rows[0].cells.len(), 5);
    assert_eq!(table.rows[1].cells.iter().map(|cell| cell.span).collect::<Vec<_>>(), [2, 2, 1]);
    assert_eq!(texts(&table, 0, 0), ["00"]);
    assert_eq!(texts(&table, 0, 2), ["01"], "the second cell's text moved");
    assert_eq!(texts(&table, 0, 4), ["02"]);
    let place = document.table_here().expect("the caret left the table");
    assert_eq!((place.row, place.column), (0, 0), "the caret is not in the first new cell");
}

#[test]
fn merging_before_a_split_shares_the_paragraphs_out_and_is_one_undo() {
    let mut document = filled(2, 3);
    let before = every_text(&table_of(&document));
    select_block(&mut document, (0, 0), (0, 2));
    assert!(document.split_cells(2, 1, true));

    let table = table_of(&round_trip(&document));
    assert_eq!(table.rows[0].cells.len(), 2, "the row was not merged and split in two");
    assert_eq!(texts(&table, 0, 0), ["00", "01"], "the first takes the share left over");
    assert_eq!(texts(&table, 0, 1), ["02"]);
    let width: i32 = table.grid.iter().sum();
    assert_eq!(width, table_of(&document).grid.iter().sum::<i32>());

    assert!(document.undo(), "there was nothing to undo");
    assert_eq!(every_text(&table_of(&document)), before, "one undo did not take it all back");
}

#[test]
fn merged_ruled_and_shaded_cells_are_written_from_the_model() {
    use wp_docx::model::{Border, Table, TableBorders, TableCell, TableRow};
    let top = || Some(Border::line("double", 6, Some("FF0000")));
    let table = Table {
        rows: vec![
            TableRow {
                cells: vec![
                    TableCell::text("Tall"),
                    TableCell {
                        shading: Some("D9E2F3".to_owned()),
                        borders: TableBorders { top: top(), ..TableBorders::default() },
                        ..TableCell::text("Shaded")
                    },
                ],
                ..TableRow::default()
            },
            TableRow {
                cells: vec![
                    TableCell { merged_upwards: true, ..TableCell::default() },
                    TableCell::text("Plain"),
                ],
                ..TableRow::default()
            },
        ],
        grid: vec![2000, 2000],
        ..Table::default()
    };
    let mut body = Body::default();
    body.blocks.push(Block::Table(Box::new(table)));
    body.blocks.push(Block::Paragraph(Paragraph::text("after")));
    let document = round_trip(&Document::create(&body).expect("a document"));
    let table = table_of(&document);
    assert!(!table.rows[0].cells[0].merged_upwards);
    assert!(table.rows[1].cells[0].merged_upwards, "the merge was not written");
    let shaded = &table.rows[0].cells[1];
    assert_eq!(shaded.shading.as_deref(), Some("D9E2F3"));
    assert_eq!(shaded.borders.top, top());
    let xml = document.package().xml_part("word/document.xml").expect("there").expect("text");
    assert!(xml.contains(r#"<w:vMerge w:val="restart"/>"#), "{xml}");
}

//! Every command a table has, tried the way a person presses it.
//!
//! # Why a file of nothing but tests
//!
//! Because a table has more commands than anything else in the program — two
//! whole ribbon tabs of them — and each one was written and proved on its own,
//! at a different time. What none of them proved is that the *set* of them
//! still works: that pressing Merge Cells after selecting a row does what the
//! row selection said it would, that the Height box changes the rows that are
//! selected rather than the one the caret is in, that a command about a cell
//! means every selected cell.
//!
//! So this goes down the two tabs, button by button, and presses each one
//! through [`Editor::run`] — the same road a press on the ribbon takes — and
//! then looks at what it did to the document or to the page. What is not here
//! is what a table cannot yet do, and that belongs in the roadmap rather than
//! in a test that passes.

#![cfg(test)]

use wp_docx::model::{Block, Body, Paragraph, TableFit, TextDirection};
use wp_docx::{Document, TextPosition};
use wp_layout::FontLibrary;
use wp_shell::{App, Event, Modifiers};

use super::Editor;
use crate::chrome::dialog::Answer;
use crate::chrome::palette::Kind as PaletteKind;
use crate::chrome::{Command, TableBorderChoice};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// An editor with a three by three table, the caret in its first cell.
fn editor() -> Editor {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");
    let mut editor = Editor::new(library(), document, None);
    // These tests are written in inches, so they say so: what a box
    // shows otherwise depends on the machine the test runs on.
    editor.unit = crate::measure::Unit::Inches;
    editor.handle(Event::Resized { width: 1400, height: 900 });
    editor.document.set_caret(TextPosition::new(0, 6));
    assert!(editor.document.insert_table(3, 3), "the table went nowhere");
    editor.relayout();
    caret_into(&mut editor, 0, 0);
    editor
}

/// Puts the caret in one cell, as a press in it would.
fn caret_into(editor: &mut Editor, row: usize, column: usize) {
    let (first, _) = editor.document.cell_paragraphs(row, column).expect("a cell");
    editor.document.set_caret(TextPosition::new(first, 0));
}

/// Types into the cell the caret is in.
fn write(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        editor.handle(Event::Char(character));
    }
}

/// What every cell of the table holds.
fn grid(editor: &Editor) -> Vec<Vec<String>> {
    editor.document.table_rows_text()
}

/// The widths of the columns as they are drawn, in the first row.
fn widths(editor: &Editor) -> Vec<f32> {
    let top = editor.pages[0].cells.first().map_or(0.0, |cell| cell.y);
    let mut row: Vec<&wp_layout::PlacedCell> =
        editor.pages[0].cells.iter().filter(|cell| (cell.y - top).abs() < 0.5).collect();
    row.sort_by(|one, other| one.x.total_cmp(&other.x));
    row.iter().map(|cell| cell.width).collect()
}

/// Turns off the gridlines drawn on screen for a table with no borders.
///
/// They are decorations like the borders themselves, so a count of what is
/// drawn cannot tell a table with borders from one with gridlines instead
/// while they are on — and they are on by default, as they are in Word.
fn hide_gridlines(editor: &mut Editor) {
    if editor.show_table_gridlines {
        editor.run(Command::ViewGridlines);
    }
}

// --- The Table Design tab ---------------------------------------------------

#[test]
fn each_of_the_six_style_options_turns_over() {
    let mut editor = editor();
    for command in [
        Command::TableHeaderRow,
        Command::TableTotalRow,
        Command::TableFirstColumn,
        Command::TableLastColumn,
        Command::TableBandedRows,
        Command::TableBandedColumns,
    ] {
        let before = editor.document.table_look().expect("a table");
        editor.run(command);
        let after = editor.document.table_look().expect("a table");
        assert_ne!(after, before, "{command:?} changed nothing");
        editor.run(command);
        assert_eq!(
            editor.document.table_look().expect("a table"),
            before,
            "{command:?} would not turn back"
        );
    }
}

#[test]
fn a_table_style_can_be_put_on_and_taken_off() {
    let mut editor = editor();
    editor.run(Command::TableStyles);
    editor.choose_table_style(1);
    assert!(editor.document.table_style().is_some(), "no style was named");

    editor.choose_table_style(0);
    assert!(editor.document.table_style().is_none(), "the style would not come off");
}

#[test]
fn the_three_border_buttons_put_lines_on_and_take_them_off() {
    let mut editor = editor();
    hide_gridlines(&mut editor);
    editor.run(Command::TableBorders(TableBorderChoice::None));
    editor.relayout();
    let bare = editor.pages[0].decorations.len();

    editor.run(Command::TableBorders(TableBorderChoice::All));
    editor.relayout();
    let all = editor.pages[0].decorations.len();
    assert!(all > bare, "All drew no more lines than None: {bare} then {all}");

    editor.run(Command::TableBorders(TableBorderChoice::Outside));
    editor.relayout();
    let outside = editor.pages[0].decorations.len();
    assert!(outside < all, "Outside drew as many lines as All: {all} then {outside}");
    assert!(outside > bare, "Outside drew no lines at all");
}

#[test]
fn shading_colours_the_cell_and_not_the_paragraph() {
    let mut editor = editor();
    editor.apply_color(PaletteKind::Shading, Some("FFFF00"), "Yellow");
    assert_eq!(editor.document.cell_shading().as_deref(), Some("FFFF00"), "the cell has no colour");

    editor.apply_color(PaletteKind::Shading, None, "No Colour");
    assert!(editor.document.cell_shading().is_none(), "the colour would not come off");
}

#[test]
fn shading_colours_every_selected_cell() {
    // Word colours what is selected. Colouring only the caret's cell meant
    // selecting a row, pressing a colour, and watching one cell change.
    let mut editor = editor();
    assert!(editor.select_table_row(1), "the row would not select");
    editor.apply_color(PaletteKind::Shading, Some("FF0000"), "Red");

    for column in 0..3 {
        caret_into(&mut editor, 1, column);
        assert_eq!(
            editor.document.cell_shading().as_deref(),
            Some("FF0000"),
            "the cell at 1,{column} was not coloured"
        );
    }
    caret_into(&mut editor, 0, 0);
    assert!(editor.document.cell_shading().is_none(), "a cell outside the row was coloured");
}

// --- The Table Layout tab ---------------------------------------------------

#[test]
fn the_select_menu_offers_the_cell_the_row_the_column_and_the_table() {
    let mut editor = editor();
    caret_into(&mut editor, 1, 1);

    let expected = [((1, 1), (1, 1)), ((0, 2), (1, 1)), ((1, 1), (0, 2)), ((0, 2), (0, 2))];
    for (index, (rows, columns)) in expected.into_iter().enumerate() {
        caret_into(&mut editor, 1, 1);
        editor.choose_table_part(index);
        let taken = editor.document.selected_cells().expect("a block of cells");
        assert_eq!((taken.rows, taken.columns), (rows, columns), "row {index} of the menu");
    }
}

#[test]
fn view_gridlines_turns_over_and_changes_what_is_drawn() {
    let mut editor = editor();
    editor.run(Command::TableBorders(TableBorderChoice::None));
    editor.relayout();

    let was = editor.show_table_gridlines;
    editor.run(Command::ViewGridlines);
    editor.relayout();
    assert_ne!(editor.show_table_gridlines, was, "the switch did not turn");

    let with = editor.pages[0].decorations.len();
    editor.run(Command::ViewGridlines);
    editor.relayout();
    let without = editor.pages[0].decorations.len();
    assert_ne!(with, without, "the gridlines are drawn either way");
}

#[test]
fn the_four_insert_buttons_each_add_one() {
    for (command, rows, columns) in [
        (Command::InsertRowAbove, 4, 3),
        (Command::InsertRowBelow, 4, 3),
        (Command::InsertColumnLeft, 3, 4),
        (Command::InsertColumnRight, 3, 4),
    ] {
        let mut editor = editor();
        caret_into(&mut editor, 1, 1);
        editor.run(command);
        editor.relayout();
        let grid = grid(&editor);
        assert_eq!(grid.len(), rows, "{command:?} left {} rows", grid.len());
        assert_eq!(grid[0].len(), columns, "{command:?} left {} columns", grid[0].len());
    }
}

#[test]
fn insert_row_puts_the_row_on_the_side_it_says() {
    let mut editor = editor();
    caret_into(&mut editor, 1, 0);
    write(&mut editor, "Middle");
    editor.relayout();

    caret_into(&mut editor, 1, 0);
    editor.run(Command::InsertRowAbove);
    editor.relayout();
    assert_eq!(grid(&editor)[2][0], "Middle", "the row went in below instead of above");

    caret_into(&mut editor, 2, 0);
    editor.run(Command::InsertRowBelow);
    editor.relayout();
    assert_eq!(grid(&editor)[2][0], "Middle", "the row went in above instead of below");
    assert!(grid(&editor)[3].iter().all(String::is_empty), "the new row is not empty");
}

#[test]
fn insert_column_puts_the_column_on_the_side_it_says() {
    let mut editor = editor();
    caret_into(&mut editor, 0, 1);
    write(&mut editor, "Middle");
    editor.relayout();

    caret_into(&mut editor, 0, 1);
    editor.run(Command::InsertColumnLeft);
    editor.relayout();
    assert_eq!(grid(&editor)[0][2], "Middle", "the column went in on the wrong side");

    caret_into(&mut editor, 0, 2);
    editor.run(Command::InsertColumnRight);
    editor.relayout();
    assert_eq!(grid(&editor)[0][2], "Middle", "the column went in on the wrong side");
}

#[test]
fn the_three_delete_buttons_take_what_they_say() {
    let mut editor = editor();
    caret_into(&mut editor, 1, 1);
    editor.run(Command::DeleteRow);
    editor.relayout();
    assert_eq!(grid(&editor).len(), 2, "the row did not go");

    caret_into(&mut editor, 0, 1);
    editor.run(Command::DeleteColumn);
    editor.relayout();
    assert_eq!(grid(&editor)[0].len(), 2, "the column did not go");

    editor.run(Command::DeleteTable);
    editor.relayout();
    assert!(editor.document.table_here().is_none(), "the table did not go");
}

#[test]
fn delete_row_takes_every_selected_row() {
    // Word's Delete Rows with three rows selected deletes three rows.
    let mut editor = editor();
    caret_into(&mut editor, 0, 0);
    write(&mut editor, "Keep");
    editor.relayout();

    assert!(editor.select_cells((1, 2), (0, 2)), "the rows would not select");
    editor.run(Command::DeleteRow);
    editor.relayout();

    let grid = grid(&editor);
    assert_eq!(grid.len(), 1, "only one row should be left, not {}", grid.len());
    assert_eq!(grid[0][0], "Keep", "the wrong row was kept");
}

#[test]
fn delete_column_takes_every_selected_column() {
    let mut editor = editor();
    caret_into(&mut editor, 0, 2);
    write(&mut editor, "Keep");
    editor.relayout();

    assert!(editor.select_cells((0, 2), (0, 1)), "the columns would not select");
    editor.run(Command::DeleteColumn);
    editor.relayout();

    let grid = grid(&editor);
    assert_eq!(grid[0].len(), 1, "only one column should be left, not {}", grid[0].len());
    assert_eq!(grid[0][0], "Keep", "the wrong column was kept");
}

#[test]
fn merge_and_split_go_both_ways() {
    let mut editor = editor();
    assert!(editor.select_cells((0, 0), (0, 1)), "the cells would not select");
    editor.run(Command::MergeCells);
    editor.relayout();
    assert_eq!(grid(&editor)[0].len(), 2, "the cells did not merge");

    caret_into(&mut editor, 0, 0);
    editor.run(Command::SplitCells);
    editor.relayout();
    assert_eq!(grid(&editor)[0].len(), 3, "the cell did not split again");
}

#[test]
fn the_autofit_menu_has_its_three_answers() {
    let mut editor = editor();
    // The menu hangs under its button, so the tab that holds it has to be the
    // one showing and the ribbon has to have been drawn — which is how it is
    // whenever a person can press it.
    editor.ribbon.tab = crate::chrome::ribbon::Tab::TableLayout;
    editor.draw(1400, 900);
    editor.run(Command::AutoFit);
    assert!(editor.popup.is_some(), "the menu did not drop open");

    for (row, wanted) in [(0, TableFit::Contents), (2, TableFit::Fixed)] {
        editor.choose_autofit(row);
        editor.relayout();
        assert_eq!(editor.document.table_fit(), wanted, "row {row} of the menu");
    }
    editor.choose_autofit(1);
    editor.relayout();
    assert!(matches!(editor.document.table_fit(), TableFit::Window(_)), "row 1 of the menu");
}

#[test]
fn the_height_box_sets_the_height_of_every_selected_row() {
    let mut editor = editor();
    let before = editor.placed_cell(0, 0).expect("a cell").1.height;

    assert!(editor.select_cells((0, 1), (0, 2)), "the rows would not select");
    editor.type_in_box(Command::RowHeightBox);
    write(&mut editor, "1");
    editor.finish_box();
    editor.relayout();

    for row in 0..2 {
        let height = editor.placed_cell(row, 0).expect("a cell").1.height;
        assert!(height > before * 1.5, "row {row} came out {height} tall");
    }
    let last = editor.placed_cell(2, 0).expect("a cell").1.height;
    assert!((last - before).abs() < 1.0, "a row outside the selection changed");
}

#[test]
fn the_width_box_sets_the_width_of_the_column() {
    let mut editor = editor();
    let before = widths(&editor);

    caret_into(&mut editor, 0, 0);
    assert!(editor.select_table_column(0), "the column would not select");
    editor.type_in_box(Command::ColumnWidthBox);
    write(&mut editor, "1");
    editor.finish_box();
    editor.relayout();

    let after = widths(&editor);
    assert!(after[0] < before[0] * 0.8, "the column did not narrow: {before:?} then {after:?}");
}

#[test]
fn distribute_makes_the_columns_even() {
    let mut editor = editor();
    // A table with one column dragged wide, which is what Distribute is for.
    assert!(editor.document.set_table_grid(&[5000, 2000, 2360]), "the grid would not be set");
    editor.relayout();
    let uneven = widths(&editor);
    assert!(uneven[0] > uneven[1] * 1.5, "the columns were not uneven to begin with");

    editor.run(Command::DistributeColumns);
    editor.relayout();
    let even = widths(&editor);
    assert!(
        even.windows(2).all(|pair| (pair[0] - pair[1]).abs() < 2.0),
        "the columns came out {even:?}"
    );
}

#[test]
fn text_direction_turns_the_text_and_comes_back_round() {
    let mut editor = editor();
    write(&mut editor, "Turned");
    editor.relayout();

    editor.run(Command::TextDirection);
    editor.relayout();
    assert!(editor.document.cell_direction().expect("a cell").is_turned(), "it did not turn");
    assert!(
        editor.pages[0].lines.iter().any(|line| line.frame.is_turned()),
        "the text is turned in the file and not on the page"
    );

    editor.run(Command::TextDirection);
    editor.relayout();
    assert!(editor.document.cell_direction().expect("a cell").is_turned(), "the second way round");

    editor.run(Command::TextDirection);
    editor.relayout();
    assert_eq!(
        editor.document.cell_direction().expect("a cell"),
        TextDirection::Horizontal,
        "three presses should come back round"
    );
    assert!(
        !editor.pages[0].lines.iter().any(|line| line.frame.is_turned()),
        "it is still drawn turned"
    );
}

#[test]
fn repeat_header_rows_turns_over() {
    let mut editor = editor();
    assert_eq!(editor.document.table_header_row(), Some(false));
    editor.run(Command::RepeatHeaderRow);
    assert_eq!(editor.document.table_header_row(), Some(true), "it did not take");
    editor.run(Command::RepeatHeaderRow);
    assert_eq!(editor.document.table_header_row(), Some(false), "it would not turn back");
}

#[test]
fn sort_puts_the_rows_in_order() {
    let mut editor = editor();
    for (row, word) in [(0, "Charlie"), (1, "Alpha"), (2, "Bravo")] {
        caret_into(&mut editor, row, 0);
        write(&mut editor, word);
    }
    editor.relayout();

    caret_into(&mut editor, 0, 0);
    assert!(editor.document.sort_table_rows(
        &[wp_docx::sorting::SortKey::new(0, wp_docx::sorting::SortKind::Text, false)],
        false
    ));
    editor.relayout();

    let first_column: Vec<String> = grid(&editor).iter().map(|row| row[0].clone()).collect();
    assert_eq!(first_column, vec!["Alpha", "Bravo", "Charlie"], "the rows are out of order");
}

#[test]
fn convert_to_text_leaves_the_words_and_takes_the_table() {
    let mut editor = editor();
    write(&mut editor, "Kept");
    editor.run(Command::ConvertToText);
    editor.relayout();

    assert!(editor.document.table_here().is_none(), "the table is still there");
    assert!(editor.document.plain_text().contains("Kept"), "the words went with it");
}

#[test]
fn a_formula_adds_up_the_column_above_it() {
    let mut editor = editor();
    for (row, number) in [(0, "2"), (1, "3")] {
        caret_into(&mut editor, row, 0);
        write(&mut editor, number);
    }
    editor.relayout();

    caret_into(&mut editor, 2, 0);
    editor.run(Command::Formula);
    assert!(editor.dialog.is_some(), "the Formula dialog did not open");
    editor.finish_dialog(Answer::Accept);
    editor.relayout();

    assert!(
        grid(&editor)[2][0].contains('5'),
        "the sum did not come out: {:?}",
        grid(&editor)[2][0]
    );
}

#[test]
fn the_properties_dialog_opens_on_the_table() {
    let mut editor = editor();
    editor.run(Command::TableProperties);
    assert!(editor.dialog.is_some(), "the dialog did not open");
    editor.finish_dialog(Answer::Cancel);
    assert!(editor.dialog.is_none(), "the dialog would not close");
}

#[test]
fn the_pen_divides_a_cell_and_the_eraser_joins_two_back_together() {
    // Word's Draw Table pen draws a line through a cell, which divides it; its
    // Eraser rubs out the line between two cells, which joins them. A tap with
    // the pen draws nothing, in Word and here.
    let mut editor = editor();
    editor.draw(1400, 900);
    let (_, cell) = editor.placed_cell(0, 0).expect("a cell");
    let (origin_x, origin_y) = editor.page_origin(0);
    let top = editor.content_top() + origin_y - editor.scroll_down();
    let middle = (top + cell.y + cell.height / 2.0) as i32;
    let left = (origin_x + cell.x + 4.0) as i32;
    let right = (origin_x + cell.x + cell.width - 4.0) as i32;

    editor.run(Command::DrawTable);
    editor.handle(Event::MouseDown { x: left, y: middle, modifiers: Modifiers::default() });
    editor.handle(Event::MouseUp { x: left, y: middle });
    editor.relayout();
    assert_eq!(grid(&editor).len(), 3, "a tap with the pen changed the table");

    // A line drawn across the cell divides it downwards: the row becomes two,
    // and every cell beside the divided one spans both of them.
    editor.handle(Event::MouseDown { x: left, y: middle, modifiers: Modifiers::default() });
    editor.handle(Event::MouseUp { x: right, y: middle });
    editor.relayout();
    assert_eq!(grid(&editor).len(), 4, "the pen did not divide the cell");
    assert_eq!(editor.document.cells_in_row(0), 3, "the row lost or gained a cell");

    // And the eraser puts the two halves back together. The row keeps its cell
    // — a cell joined to the one above it is written as a continuation of it,
    // not taken out of the row — so what says it worked is the line between
    // them: it is gone from the page.
    let halves = editor.placed_cell(0, 0).expect("a cell").1;
    let lines_before = editor.pages[0].decorations.len();
    editor.run(Command::Eraser);
    let x = (origin_x + halves.x + halves.width / 2.0) as i32;
    let y = (top + halves.y + halves.height) as i32;
    assert!(editor.table_pen_press(x, y), "the eraser found no line to rub out");
    editor.relayout();

    let boundary = halves.y + halves.height;
    let along = |editor: &Editor| {
        editor.pages[0]
            .decorations
            .iter()
            .filter(|line| (line.y - boundary).abs() < 2.0 && line.width > line.height)
            .count()
    };
    assert_eq!(along(&editor), 0, "the line between the joined halves is still drawn");
    assert!(
        editor.pages[0].decorations.len() < lines_before,
        "nothing came off the page when the line was rubbed out"
    );
}

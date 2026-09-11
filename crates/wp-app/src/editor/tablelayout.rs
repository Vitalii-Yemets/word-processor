//! The Table Layout tab: what can be done to a table once it is there.
//!
//! # Nine alignments and not three
//!
//! Word's Alignment group is a three-by-three grid, because a cell has two
//! independent questions to answer: where the text sits across the cell and
//! where it sits up and down it. Three buttons could only answer the first, and
//! a person looking for "align middle centre" found a tab that did not have it.
//! One press here answers both, which is what each of Word's nine does.
//!
//! # Selecting part of a table
//!
//! Word's Select menu takes a cell, a column, a row or the whole table. What it
//! selects is text — a table is paragraphs like everything else — so selecting
//! a row means selecting from the start of its first paragraph to the end of
//! its last. That is why these live here and not in the table model: the model
//! knows where the cells are, and only the editor knows what a selection is.
//!
//! # Why fixing the columns is done here
//!
//! Word's Fixed Column Width keeps the widths that are in front of you, and the
//! widths in front of you are the ones the layout worked out — not the ones the
//! file was last written with, which a table fitted to its contents has long
//! since stopped obeying. So the editor reads the columns off the page it has
//! drawn and writes those into the document before fixing them. The model could
//! not do it: it has never seen a page.

use wp_docx::model::{Alignment, TableFit};
use wp_docx::table_properties::CellAlignment;
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// Twentieths of a point, which is the unit a column's width is written in.
const TWIPS_PER_POINT: f32 = 20.0;

/// Word's AutoFit menu, in Word's order.
pub(super) const AUTOFITS: &[(&str, TableFit)] = &[
    ("AutoFit Contents", TableFit::Contents),
    ("AutoFit Window", TableFit::Window(100)),
    ("Fixed Column Width", TableFit::Fixed),
];

/// Whether two fits are the same row of that menu.
///
/// A table that is eighty per cent of the text area is fitted to the window,
/// even though the row offers a hundred: the row is what it *does*, and what it
/// does is fit the table to a part of the window.
pub(super) fn same_fit(row: TableFit, here: TableFit) -> bool {
    matches!(
        (row, here),
        (TableFit::Contents, TableFit::Contents)
            | (TableFit::Fixed, TableFit::Fixed)
            | (TableFit::Window(_), TableFit::Window(_))
    )
}

/// Word's nine, in Word's order: across the top row first.
pub(super) const ALIGNMENTS: &[(CellAlignment, Alignment, &str)] = &[
    (CellAlignment::Top, Alignment::Start, "Align Top Left"),
    (CellAlignment::Top, Alignment::Center, "Align Top Center"),
    (CellAlignment::Top, Alignment::End, "Align Top Right"),
    (CellAlignment::Middle, Alignment::Start, "Align Center Left"),
    (CellAlignment::Middle, Alignment::Center, "Align Center"),
    (CellAlignment::Middle, Alignment::End, "Align Center Right"),
    (CellAlignment::Bottom, Alignment::Start, "Align Bottom Left"),
    (CellAlignment::Bottom, Alignment::Center, "Align Bottom Center"),
    (CellAlignment::Bottom, Alignment::End, "Align Bottom Right"),
];

/// What Word's Select menu offers.
const SELECTING: &[&str] = &["Select Cell", "Select Column", "Select Row", "Select Table"];

impl Editor {
    /// One of the nine: across and down in one press.
    pub(super) fn align_cell(&mut self, which: usize) -> Response {
        let Some((down, across, name)) = ALIGNMENTS.get(which).copied() else {
            return Response::Ignored;
        };
        if self.document.table_here().is_none() {
            return self.report("Put the caret in a table first");
        }

        // Both, in one gesture, so one undo takes the whole answer back.
        self.document.begin_gesture();
        let mut changed = self.document.set_cell_alignment(down);
        changed |= self.document.set_alignment_here(across);
        self.document.end_gesture();

        self.relayout();
        self.edited(changed, name)
    }

    /// Which of the nine the cell at the caret is in, so its button can show as
    /// pressed.
    #[must_use]
    pub(super) fn cell_alignment_here(&self) -> Option<usize> {
        let down = self.document.cell_alignment()?;
        let across = self.document.alignment_here();
        ALIGNMENTS
            .iter()
            .position(|(vertical, horizontal, _)| *vertical == down && *horizontal == across)
    }

    /// Repeats the first row at the top of every page the table runs onto.
    pub(super) fn toggle_repeat_header(&mut self) -> Response {
        let Some(header) = self.document.table_header_row() else {
            return self.report("Put the caret in a table first");
        };
        let changed = self.document.set_table_header_row(!header);
        self.relayout();
        self.edited(
            changed,
            if header { "Header row not repeated" } else { "Header row repeated on each page" },
        )
    }

    /// Turns the text in the cell at the caret a right angle.
    ///
    /// A button that cycles rather than a menu, which is what Word's is: across,
    /// then down, then up, then across again. Three presses and it is back where
    /// it started, so nothing is stranded.
    pub(super) fn turn_cell_text(&mut self) -> Response {
        let Some(direction) = self.document.cell_direction() else {
            return self.report("Put the caret in a table first");
        };
        let wanted = direction.next();
        let changed = self.document.set_cell_direction(wanted);
        self.relayout();
        self.edited(changed, wanted.label())
    }

    /// Sets the table to whichever of the three was picked.
    pub(super) fn choose_autofit(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((label, fit)) = AUTOFITS.get(index).copied() else { return Response::Ignored };
        if self.document.table_here().is_none() {
            return self.report("Put the caret in a table first");
        }

        // Fixing the columns keeps the ones in front of you, so they are
        // written down before the table is told to stop working them out. Both
        // changes are one thing to undo, because they are one command.
        let widths = if fit == TableFit::Fixed { self.drawn_column_widths() } else { Vec::new() };
        self.document.begin_gesture();
        if !widths.is_empty() {
            self.document.set_table_grid(&widths);
        }
        let changed = self.document.set_table_fit(fit);
        self.document.end_gesture();

        self.relayout();
        self.edited(changed, label)
    }

    /// How wide the columns of the table at the caret came out, in twentieths
    /// of a point.
    ///
    /// Read off the page rather than out of the file: what a table fitted to
    /// its contents is drawn at is not what the file says, and freezing the
    /// file's widths would move the table the moment it was frozen.
    ///
    /// The row with the most cells in it is the one asked, because a row whose
    /// cells are merged together says nothing about the columns underneath
    /// them. Where even that row does not answer for every column — every row
    /// has a merge in it — nothing is written and the file's own grid stands.
    fn drawn_column_widths(&self) -> Vec<i32> {
        let Some((first, last)) = self.document.table_paragraphs() else { return Vec::new() };
        let Some(place) = self.document.table_here() else { return Vec::new() };

        // The cells of this table, gathered into rows by where they sit.
        let mut rows: Vec<(f32, Vec<&wp_layout::PlacedCell>)> = Vec::new();
        for cell in self.pages.iter().flat_map(|page| &page.cells) {
            if cell.at.paragraph < first || cell.at.paragraph > last {
                continue;
            }
            match rows.iter_mut().find(|(y, _)| (*y - cell.y).abs() < 0.5) {
                Some((_, row)) => row.push(cell),
                None => rows.push((cell.y, vec![cell])),
            }
        }

        let Some((_, mut widest)) = rows.into_iter().max_by_key(|(_, row)| row.len()) else {
            return Vec::new();
        };
        if widest.len() != place.columns {
            return Vec::new();
        }
        widest.sort_by(|one, other| one.x.total_cmp(&other.x));

        let scale = self.pixels_per_inch() / 72.0;
        if scale <= 0.0 {
            return Vec::new();
        }
        widest.iter().map(|cell| ((cell.width / scale) * TWIPS_PER_POINT).round() as i32).collect()
    }

    /// Drops open Word's Select menu.
    pub(super) fn open_table_select(&mut self) -> Response {
        if self.close_popup_if(Choice::TablePart) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::SelectTablePart) else {
            return Response::Ignored;
        };
        let items = SELECTING.iter().map(|name| (*name).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::TablePart, items, None, left, top, 190.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Selects whichever part of the table was picked.
    pub(super) fn choose_table_part(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(position) = self.document.table_here() else {
            return self.report("Put the caret in a table first");
        };
        let caret = self.caret();

        // Which paragraphs the wanted part covers. A cell holds at least one
        // paragraph and may hold several, so a part is a range of paragraphs
        // rather than a count of cells.
        let range = match index {
            // The cell the caret is in: the paragraphs of that cell alone.
            0 => self.document.cell_paragraphs(position.row, position.column),
            1 => {
                let mut first = usize::MAX;
                let mut last = 0usize;
                for row in 0..position.rows {
                    let Some((start, end)) = self.document.cell_paragraphs(row, position.column)
                    else {
                        continue;
                    };
                    first = first.min(start);
                    last = last.max(end);
                }
                (first != usize::MAX).then_some((first, last))
            }
            2 => {
                let mut first = usize::MAX;
                let mut last = 0usize;
                for column in 0..position.columns {
                    let Some((start, end)) = self.document.cell_paragraphs(position.row, column)
                    else {
                        continue;
                    };
                    first = first.min(start);
                    last = last.max(end);
                }
                (first != usize::MAX).then_some((first, last))
            }
            3 => self.document.table_paragraphs(),
            _ => None,
        };

        let Some((first, last)) = range else { return Response::Ignored };
        let end = self.document.paragraph_text(last).map_or(0, |text| text.len());
        self.document.set_caret(TextPosition::new(first, 0));
        self.document.extend_selection_to(TextPosition::new(last, end));

        let _ = caret;
        self.needs_redraw = true;
        self.report(SELECTING.get(index).copied().unwrap_or_default())
    }

    /// Word's Convert to Text: the table's cells as paragraphs.
    pub(super) fn convert_table_to_text(&mut self) -> Response {
        if self.document.table_here().is_none() {
            return self.report("Put the caret in a table first");
        }
        let changed = self.document.convert_table_to_text();
        self.relayout();
        self.edited(changed, "Table converted to text")
    }

    /// Shows the boundaries of a table that draws no lines of its own.
    ///
    /// Word's View Gridlines. A table with no borders is invisible, and a
    /// person editing one has to be able to see where the cells are — but the
    /// lines are drawn on the screen only and never printed, which is what
    /// makes them gridlines rather than borders.
    pub(super) fn toggle_table_gridlines(&mut self) -> Response {
        self.show_table_gridlines = !self.show_table_gridlines;
        self.relayout();
        self.report(if self.show_table_gridlines {
            "Table gridlines shown"
        } else {
            "Table gridlines hidden"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph, TextDirection};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.insert_table(3, 3);
        editor.relayout();
        editor
    }

    #[test]
    fn word_has_nine_of_them_and_so_does_this() {
        assert_eq!(ALIGNMENTS.len(), 9);
    }

    #[test]
    fn one_press_answers_both_questions() {
        let mut editor = editor();
        // "Align Bottom Right", which is the last of the nine.
        editor.align_cell(8);

        assert_eq!(editor.document.cell_alignment(), Some(CellAlignment::Bottom));
        assert_eq!(editor.document.alignment_here(), Alignment::End);
        assert_eq!(editor.cell_alignment_here(), Some(8));
    }

    #[test]
    fn both_halves_come_back_with_one_undo() {
        let mut editor = editor();
        editor.align_cell(4);
        assert_eq!(editor.cell_alignment_here(), Some(4));

        editor.document.undo();
        assert_ne!(editor.cell_alignment_here(), Some(4), "one undo left half of it behind");
    }

    #[test]
    fn selecting_a_row_selects_every_cell_of_it() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(2, 0));
        editor.choose_table_part(2);

        let (start, end) = editor.document.selection().expect("a selection");
        assert!(end.paragraph > start.paragraph, "the selection covers one paragraph");
    }

    #[test]
    fn selecting_the_table_selects_all_of_it() {
        let mut editor = editor();
        editor.choose_table_part(3);

        let (start, end) = editor.document.selection().expect("a selection");
        // Nine cells of one paragraph each.
        assert_eq!(end.paragraph - start.paragraph, 8);
    }

    #[test]
    fn repeating_the_header_row_is_a_switch_that_turns_over() {
        let mut editor = editor();
        assert_eq!(editor.document.table_header_row(), Some(false));

        editor.toggle_repeat_header();
        assert_eq!(editor.document.table_header_row(), Some(true));
        editor.toggle_repeat_header();
        assert_eq!(editor.document.table_header_row(), Some(false));
    }

    #[test]
    fn converting_a_table_to_text_leaves_the_words_and_takes_the_table() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(1, 0));
        editor.document.type_text("Kept");
        editor.convert_table_to_text();

        assert!(editor.document.table_here().is_none(), "the table is still there");
        assert!(editor.document.plain_text().contains("Kept"), "the words went with it");
    }

    /// How wide the table on the page is: from the left of its first cell to
    /// the right of its last.
    fn drawn_width(editor: &Editor) -> f32 {
        let cells: Vec<&wp_layout::PlacedCell> =
            editor.pages.iter().flat_map(|page| &page.cells).collect();
        let left = cells.iter().map(|cell| cell.x).fold(f32::MAX, f32::min);
        let right = cells.iter().map(|cell| cell.x + cell.width).fold(f32::MIN, f32::max);
        assert!(right > left, "no table was drawn");
        right - left
    }

    /// Which row of the AutoFit menu a fit is.
    fn autofit_row(fit: TableFit) -> usize {
        AUTOFITS.iter().position(|(_, found)| same_fit(*found, fit)).expect("a row")
    }

    #[test]
    fn a_new_table_is_as_wide_as_the_text() {
        // Word's Insert Table gives a table that fills the text area, and every
        // cell states the width of its column to say so.
        let editor = editor();
        let width = drawn_width(&editor);
        let page = editor.pages.first().expect("a page");
        assert!(width > page.width * 0.6, "a new table came out {width} wide");
    }

    #[test]
    fn fitting_to_contents_makes_a_table_hug_its_text() {
        let mut editor = editor();
        let before = drawn_width(&editor);

        editor.choose_autofit(autofit_row(TableFit::Contents));
        let after = drawn_width(&editor);
        assert!(after < before / 2.0, "it did not shrink: {before} then {after}");
        assert_eq!(editor.document.table_fit(), TableFit::Contents);
    }

    #[test]
    fn a_table_fitted_to_its_contents_grows_as_it_is_typed_in() {
        // The whole point of the thing: the width is the answer to what is in
        // the cells, so it changes when they do.
        let mut editor = editor();
        editor.choose_autofit(autofit_row(TableFit::Contents));
        let empty = drawn_width(&editor);

        editor.document.set_caret(wp_docx::TextPosition::new(1, 0));
        editor.document.type_text("Something rather long to put in a cell");
        editor.relayout();
        let filled = drawn_width(&editor);
        assert!(filled > empty, "it did not grow: {empty} then {filled}");

        // And back again when the words go.
        for _ in 0.."Something rather long to put in a cell".len() {
            editor.document.backspace();
        }
        editor.relayout();
        assert!(drawn_width(&editor) < filled, "it did not shrink again");
    }

    #[test]
    fn fitting_to_the_window_fills_the_text_area() {
        let mut editor = editor();
        editor.choose_autofit(autofit_row(TableFit::Contents));
        let hugged = drawn_width(&editor);

        editor.choose_autofit(autofit_row(TableFit::Window(100)));
        let filled = drawn_width(&editor);
        assert!(filled > hugged * 2.0, "it did not fill the page: {hugged} then {filled}");
        assert!(matches!(editor.document.table_fit(), TableFit::Window(_)));
    }

    #[test]
    fn fixing_the_columns_keeps_the_width_in_front_of_you() {
        let mut editor = editor();
        editor.choose_autofit(autofit_row(TableFit::Contents));
        let hugged = drawn_width(&editor);

        editor.choose_autofit(autofit_row(TableFit::Fixed));
        let fixed = drawn_width(&editor);
        assert!((fixed - hugged).abs() < 2.0, "the table jumped: {hugged} then {fixed}");
        assert_eq!(editor.document.table_fit(), TableFit::Fixed);

        // And now typing does not move it, which is what fixed means.
        editor.document.set_caret(wp_docx::TextPosition::new(1, 0));
        editor.document.type_text("Something rather long to put in a cell");
        editor.relayout();
        assert!((drawn_width(&editor) - fixed).abs() < 2.0, "a fixed column moved");
    }

    #[test]
    fn one_undo_takes_back_the_whole_command() {
        let mut editor = editor();
        let before = drawn_width(&editor);
        editor.choose_autofit(autofit_row(TableFit::Fixed));

        editor.document.undo();
        editor.relayout();
        assert!((drawn_width(&editor) - before).abs() < 2.0, "undo left the table changed");
    }

    #[test]
    fn the_menu_says_which_of_the_three_is_in_force() {
        let mut editor = editor();
        for fit in [TableFit::Contents, TableFit::Window(100), TableFit::Fixed] {
            editor.choose_autofit(autofit_row(fit));
            assert_eq!(
                autofit_row(editor.document.table_fit()),
                autofit_row(fit),
                "the menu would show the wrong row for {fit:?}"
            );
        }
    }

    #[test]
    fn a_menu_row_is_what_it_does_rather_than_what_it_says() {
        assert!(same_fit(TableFit::Window(100), TableFit::Window(80)));
        assert!(!same_fit(TableFit::Window(100), TableFit::Fixed));
        assert!(!same_fit(TableFit::Contents, TableFit::Fixed));
    }

    #[test]
    fn the_text_direction_button_goes_round_the_three() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(1, 0));
        assert_eq!(editor.document.cell_direction(), Some(TextDirection::Horizontal));

        editor.turn_cell_text();
        assert_eq!(editor.document.cell_direction(), Some(TextDirection::Down));
        editor.turn_cell_text();
        assert_eq!(editor.document.cell_direction(), Some(TextDirection::Up));
        editor.turn_cell_text();
        assert_eq!(editor.document.cell_direction(), Some(TextDirection::Horizontal));
    }

    #[test]
    fn turning_one_cell_leaves_the_others_alone() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(1, 0));
        editor.turn_cell_text();

        editor.document.set_caret(wp_docx::TextPosition::new(2, 0));
        assert_eq!(editor.document.cell_direction(), Some(TextDirection::Horizontal));
    }

    #[test]
    fn turning_the_text_makes_the_row_taller() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(1, 0));
        editor.document.type_text("A heading long enough to see");
        editor.relayout();
        let before = editor
            .pages
            .iter()
            .flat_map(|page| &page.cells)
            .map(|cell| cell.height)
            .fold(0.0f32, f32::max);

        editor.turn_cell_text();
        let after = editor
            .pages
            .iter()
            .flat_map(|page| &page.cells)
            .map(|cell| cell.height)
            .fold(0.0f32, f32::max);
        assert!(after > before * 2.0, "the row did not grow: {before} then {after}");
    }

    #[test]
    fn the_gridlines_switch_turns_over() {
        let mut editor = editor();
        let before = editor.show_table_gridlines;
        editor.toggle_table_gridlines();
        assert_ne!(editor.show_table_gridlines, before);
    }
}

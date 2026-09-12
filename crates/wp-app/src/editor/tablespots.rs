//! The two things Word offers the pointer at the edges of a table: the line
//! double-clicked to fit a column to what is in it, and the buttons pressed to
//! put a row or a column in.
//!
//! # Why they are here and not on the ribbon
//!
//! Because they are how a table is actually shaped. The Insert group exists,
//! and nobody reaches for it: Word shows a small circled plus beside the line
//! between two rows the moment the pointer goes near it, and pressing that is
//! one gesture instead of three. The same for columns along the top. A table
//! whose only way to gain a row is a ribbon button is a table that is tiring to
//! build.
//!
//! Both work from the cells on the page rather than from the file, for the
//! reason [`super::tableedges`] gives: what a person aims at is the line they
//! can see, and where that line is drawn is not what the file says.

use wp_shell::Response;

use super::Editor;

/// How wide the round buttons are, in pixels.
pub(super) const SPOT: f32 = 13.0;

/// How far outside the table they sit, and how near the pointer has to be to a
/// line for its button to appear.
const CLEAR: f32 = 4.0;
const NEAR: f32 = 26.0;

/// The narrowest a column is fitted to, in twentieths of a point: a quarter of
/// an inch, which is about what Word will shrink one to.
const LEAST: i32 = 360;

/// Twentieths of a point in a point.
const TWIPS_PER_POINT: f32 = 20.0;

/// A place a row or a column can be put in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Spot {
    /// Whether it puts in a row or a column.
    pub row: bool,
    /// Which row or column it goes in front of. Past the last one means the
    /// end of the table, which is how a row is added at the bottom.
    pub at: usize,
    /// Where the button is drawn, in window coordinates.
    pub x: f32,
    pub y: f32,
    /// Somewhere inside the table, so the change can be aimed at it.
    pub inside: wp_docx::TextPosition,
}

impl Editor {
    /// The button the pointer is near, if it is near one.
    ///
    /// One at a time, as Word shows one at a time: the nearest line to the
    /// pointer is the one being aimed at, and a row of buttons down the side of
    /// every table would be a row of buttons nobody asked for.
    pub(super) fn insert_spot(&self) -> Option<Spot> {
        let (x, y) = (self.pointer_x, self.pointer_y);
        let (page, px, py) = self.page_point(x as i32, y as i32)?;
        let (origin_x, origin_y) = self.page_origin(page);
        let up = self.content_top() + origin_y - self.scroll_down();

        // The table the pointer is beside, and the lines of it.
        let (first, last) = self.table_beside(page, px, py)?;
        let cells: Vec<&wp_layout::PlacedCell> = self.pages[page]
            .cells
            .iter()
            .filter(|cell| (first..=last).contains(&cell.at.paragraph))
            .collect();
        let inside = cells.first()?.at;

        let left = cells.iter().map(|cell| cell.x).fold(f32::MAX, f32::min);
        let top = cells.iter().map(|cell| cell.y).fold(f32::MAX, f32::min);
        let right = cells.iter().map(|cell| cell.x + cell.width).fold(f32::MIN, f32::max);
        let bottom = cells.iter().map(|cell| cell.y + cell.height).fold(f32::MIN, f32::max);

        // Down the left-hand side, a button for every line between two rows and
        // one at each end. The pointer has to be beside the table rather than
        // in it: inside, the lines are dragged rather than pressed.
        if px < left && px >= left - NEAR {
            let mut heights: Vec<f32> = cells.iter().map(|cell| cell.y).collect();
            heights.push(bottom);
            heights.sort_by(f32::total_cmp);
            heights.dedup_by(|one, other| (*one - *other).abs() < 0.5);

            let (index, at_y) = heights
                .iter()
                .enumerate()
                .min_by(|(_, one), (_, other)| (py - **one).abs().total_cmp(&(py - **other).abs()))
                .map(|(index, at)| (index, *at))?;
            if (py - at_y).abs() > NEAR {
                return None;
            }
            return Some(Spot {
                row: true,
                at: index,
                x: origin_x + left - CLEAR - SPOT,
                y: up + at_y - SPOT / 2.0,
                inside,
            });
        }

        // And along the top, one for every line between two columns.
        if py < top && py >= top - NEAR {
            let mut edges: Vec<f32> =
                cells.iter().filter(|cell| (cell.y - top).abs() < 0.5).map(|cell| cell.x).collect();
            edges.push(right);
            edges.sort_by(f32::total_cmp);
            edges.dedup_by(|one, other| (*one - *other).abs() < 0.5);

            let (index, at_x) = edges
                .iter()
                .enumerate()
                .min_by(|(_, one), (_, other)| (px - **one).abs().total_cmp(&(px - **other).abs()))
                .map(|(index, at)| (index, *at))?;
            if (px - at_x).abs() > NEAR {
                return None;
            }
            return Some(Spot {
                row: false,
                at: index,
                x: origin_x + at_x - SPOT / 2.0,
                y: up + top - CLEAR - SPOT,
                inside,
            });
        }

        None
    }

    /// Which table the pointer is beside, as its first and last paragraph.
    ///
    /// Beside rather than inside: the buttons are outside the table, so the
    /// cells cannot be asked which one the point is in. The nearest cell within
    /// reach answers for its table.
    fn table_beside(&self, page: usize, px: f32, py: f32) -> Option<(usize, usize)> {
        let away = |cell: &wp_layout::PlacedCell| {
            let across = if px < cell.x {
                cell.x - px
            } else if px > cell.x + cell.width {
                px - cell.x - cell.width
            } else {
                0.0
            };
            let down = if py < cell.y {
                cell.y - py
            } else if py > cell.y + cell.height {
                py - cell.y - cell.height
            } else {
                0.0
            };
            across.max(down)
        };

        let nearest = self.pages[page]
            .cells
            .iter()
            .min_by(|one, other| away(one).total_cmp(&away(other)))
            .filter(|cell| away(cell) <= NEAR)?;
        self.document.table_paragraphs_at(nearest.at.paragraph)
    }

    /// Whether a point is on that button.
    pub(super) fn spot_at(&self, x: i32, y: i32) -> Option<Spot> {
        let spot = self.insert_spot()?;
        let inside = x as f32 >= spot.x
            && x as f32 <= spot.x + SPOT
            && y as f32 >= spot.y
            && y as f32 <= spot.y + SPOT;
        inside.then_some(spot)
    }

    /// Puts the row or the column in.
    pub(super) fn press_insert_spot(&mut self, x: i32, y: i32) -> bool {
        let Some(spot) = self.spot_at(x, y) else { return false };
        self.document.set_caret(spot.inside);
        let Some(place) = self.document.table_here() else { return false };

        // The caret decides which row or column the new one goes beside, so it
        // is put in the one the line belongs to: in front of it, or — for the
        // line at the very end — behind the last.
        let changed = if spot.row {
            let last = spot.at >= place.rows;
            let row = if last { place.rows.saturating_sub(1) } else { spot.at };
            if !self.take_cell(row, place.column.min(1)) {
                return false;
            }
            self.document.insert_table_row(last)
        } else {
            let last = spot.at >= place.columns;
            let column = if last { place.columns.saturating_sub(1) } else { spot.at };
            if !self.take_cell(place.row, column) {
                return false;
            }
            self.document.insert_table_column(last)
        };
        if !changed {
            return false;
        }

        self.relayout();
        self.status = if spot.row { "Row added" } else { "Column added" }.to_owned();
        self.needs_redraw = true;
        true
    }

    /// Fits one column to what is in it: Word's double click on a column's
    /// line.
    ///
    /// The same question **C15**'s AutoFit Contents asks of the whole table,
    /// asked of one column — and answered the same way, from what the text
    /// measured on the page rather than from what the file says.
    pub(super) fn fit_column_at(&mut self, x: i32, y: i32) -> Response {
        let Some(edge) = self.table_edge_at(x, y) else { return Response::Ignored };
        if !edge.across {
            return Response::Ignored;
        }
        let Some((page, _, _)) = self.page_point(x, y) else { return Response::Ignored };

        let mut widths = edge.were.clone();
        let Some(width) = widths.get_mut(edge.at) else { return Response::Ignored };
        let Some(wanted) = self.widest_in_column(page, edge.inside, edge.at) else {
            return Response::Ignored;
        };
        if (*width - wanted).abs() < 2 {
            return Response::Ignored;
        }
        *width = wanted;

        let caret = self.document.caret();
        self.document.set_caret(edge.inside);
        self.document.begin_gesture();
        self.document.set_table_fit(wp_docx::model::TableFit::Fixed);
        let changed = self.document.set_table_grid(&widths);
        self.document.end_gesture();
        self.document.set_caret(caret);

        self.relayout();
        self.edited(changed, "Column fitted to its contents")
    }

    /// How wide one column's widest text is, in twentieths of a point, with the
    /// room a cell keeps clear inside itself added on.
    fn widest_in_column(
        &self,
        page: usize,
        inside: wp_docx::TextPosition,
        column: usize,
    ) -> Option<i32> {
        let (first, last) = self.document.table_paragraphs_at(inside.paragraph)?;
        let cells: Vec<&wp_layout::PlacedCell> = self.pages[page]
            .cells
            .iter()
            .filter(|cell| (first..=last).contains(&cell.at.paragraph))
            .collect();

        // The cells of that column: the ones whose left edge is the column's.
        let mut edges: Vec<f32> = cells.iter().map(|cell| cell.x).collect();
        edges.sort_by(f32::total_cmp);
        edges.dedup_by(|one, other| (*one - *other).abs() < 0.5);
        let at_x = edges.get(column).copied()?;

        let mut widest = 0.0f32;
        let mut inset = f32::MAX;
        for cell in cells.iter().filter(|cell| (cell.x - at_x).abs() < 0.5) {
            for line in self.pages[page].lines.iter().filter(|line| {
                let (_, y, _, height) =
                    line.frame.rect(line.left, line.top(), 0.0, line.ascent + line.descent);
                let middle = y + height / 2.0;
                middle >= cell.y && middle <= cell.y + cell.height && line.left >= cell.x - 1.0
            }) {
                widest = widest.max(line.right - line.left);
                inset = inset.min(line.left - cell.x);
            }
        }
        if widest <= 0.0 {
            return None;
        }
        // The text plus the room either side of it, which is what makes the
        // column fit what is in it rather than pressing against it.
        let margin = if inset == f32::MAX { 0.0 } else { inset };
        let scale = self.pixels_per_inch() / 72.0;
        if scale <= 0.0 {
            return None;
        }
        let points = (widest + margin * 2.0) / scale;
        Some(((points * TWIPS_PER_POINT).round() as i32).max(LEAST))
    }
}

#[cfg(test)]
mod tests {
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Modifiers};

    use super::Editor;

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
        editor.document.set_caret(TextPosition::new(0, 6));
        assert!(editor.document.insert_table(3, 3), "the table went nowhere");
        editor.relayout();
        let (first, _) = editor.document.cell_paragraphs(0, 0).expect("a cell");
        editor.document.set_caret(TextPosition::new(first, 0));
        editor
    }

    fn grid(editor: &Editor) -> Vec<Vec<String>> {
        editor.document.table_rows_text()
    }

    fn write(editor: &mut Editor, row: usize, column: usize, text: &str) {
        let (first, _) = editor.document.cell_paragraphs(row, column).expect("a cell");
        editor.document.set_caret(TextPosition::new(first, 0));
        for character in text.chars() {
            editor.handle(Event::Char(character));
        }
    }

    /// The widths of the columns as they are drawn.
    fn widths(editor: &Editor) -> Vec<f32> {
        let top = editor.pages[0].cells.first().map_or(0.0, |cell| cell.y);
        let mut row: Vec<&wp_layout::PlacedCell> =
            editor.pages[0].cells.iter().filter(|cell| (cell.y - top).abs() < 0.5).collect();
        row.sort_by(|one, other| one.x.total_cmp(&other.x));
        row.iter().map(|cell| cell.width).collect()
    }

    /// Moves the pointer, which is what makes a button appear.
    fn point_at(editor: &mut Editor, x: i32, y: i32) {
        editor.handle(Event::MouseMove { x, y, held: false, modifiers: Modifiers::default() });
    }

    /// Where the table is on the screen: left, top, right, bottom.
    fn table_box(editor: &Editor) -> (f32, f32, f32, f32) {
        let cells = &editor.pages[0].cells;
        let (origin_x, origin_y) = editor.page_origin(0);
        let up = editor.content_top() + origin_y - editor.scroll_down();
        (
            origin_x + cells.iter().map(|cell| cell.x).fold(f32::MAX, f32::min),
            up + cells.iter().map(|cell| cell.y).fold(f32::MAX, f32::min),
            origin_x + cells.iter().map(|cell| cell.x + cell.width).fold(f32::MIN, f32::max),
            up + cells.iter().map(|cell| cell.y + cell.height).fold(f32::MIN, f32::max),
        )
    }

    #[test]
    fn a_button_appears_beside_the_line_between_two_rows() {
        let mut editor = editor();
        let (left, _, _, _) = table_box(&editor);
        let (_, cell) = editor.placed_cell(1, 0).expect("a cell");
        let (_, origin_y) = editor.page_origin(0);
        let up = editor.content_top() + origin_y - editor.scroll_down();

        point_at(&mut editor, (left - 12.0) as i32, (up + cell.y) as i32);
        let spot = editor.insert_spot().expect("no button beside the line");
        assert!(spot.row, "the button is about a column and not a row");
        assert_eq!(spot.at, 1, "the button is beside the wrong line");
    }

    #[test]
    fn no_button_appears_in_the_middle_of_the_page() {
        let mut editor = editor();
        point_at(&mut editor, 700, 600);
        assert!(editor.insert_spot().is_none(), "a button appeared away from every table");
    }

    #[test]
    fn pressing_the_button_beside_a_line_puts_a_row_in_there() {
        let mut editor = editor();
        write(&mut editor, 0, 0, "Top");
        write(&mut editor, 1, 0, "Bottom");
        editor.relayout();

        let (left, _, _, _) = table_box(&editor);
        let (_, cell) = editor.placed_cell(1, 0).expect("a cell");
        let (_, origin_y) = editor.page_origin(0);
        let up = editor.content_top() + origin_y - editor.scroll_down();
        let (x, y) = ((left - 12.0) as i32, (up + cell.y) as i32);

        point_at(&mut editor, x, y);
        let spot = editor.spot_at(x, y).expect("the pointer is not on the button");
        let _ = spot;
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseUp { x, y });
        editor.relayout();

        let grid = grid(&editor);
        assert_eq!(grid.len(), 4, "no row was added");
        assert_eq!(grid[0][0], "Top", "the row went in above the wrong line");
        assert!(grid[1].iter().all(String::is_empty), "the new row is not the empty one");
        assert_eq!(grid[2][0], "Bottom", "the row below moved somewhere else");
    }

    #[test]
    fn the_button_at_the_foot_of_the_table_adds_a_row_at_the_end() {
        let mut editor = editor();
        write(&mut editor, 2, 0, "Last");
        editor.relayout();

        let (left, _, _, bottom) = table_box(&editor);
        let (x, y) = ((left - 12.0) as i32, bottom as i32);
        point_at(&mut editor, x, y);
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseUp { x, y });
        editor.relayout();

        let grid = grid(&editor);
        assert_eq!(grid.len(), 4, "no row was added");
        assert_eq!(grid[2][0], "Last", "the row went in above the last one");
        assert!(grid[3].iter().all(String::is_empty), "the new row is not at the end");
    }

    #[test]
    fn the_button_above_a_line_puts_a_column_in() {
        let mut editor = editor();
        write(&mut editor, 0, 0, "One");
        write(&mut editor, 0, 1, "Two");
        editor.relayout();

        let (_, top, _, _) = table_box(&editor);
        let (_, cell) = editor.placed_cell(0, 1).expect("a cell");
        let (origin_x, _) = editor.page_origin(0);
        let (x, y) = ((origin_x + cell.x) as i32, (top - 10.0) as i32);

        point_at(&mut editor, x, y);
        let spot = editor.insert_spot().expect("no button above the line");
        assert!(!spot.row, "the button is about a row and not a column");
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        editor.handle(Event::MouseUp { x, y });
        editor.relayout();

        let grid = grid(&editor);
        assert_eq!(grid[0].len(), 4, "no column was added");
        assert_eq!(grid[0][0], "One", "the column went in on the wrong side");
        assert_eq!(grid[0][1], "", "the new column is not the empty one");
        assert_eq!(grid[0][2], "Two", "the column beside it moved somewhere else");
    }

    #[test]
    fn double_clicking_a_column_line_fits_that_column_to_its_text() {
        let mut editor = editor();
        write(&mut editor, 0, 0, "Short");
        write(&mut editor, 1, 0, "A good deal longer than that one");
        editor.relayout();
        let before = widths(&editor);

        let (_, cell) = editor.placed_cell(0, 0).expect("a cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let up = editor.content_top() + origin_y - editor.scroll_down();
        let x = (origin_x + cell.x + cell.width) as i32;
        let y = (up + cell.y + cell.height / 2.0) as i32;

        editor.handle(Event::DoubleClick { x, y });
        editor.relayout();

        let after = widths(&editor);
        assert!(
            (after[0] - before[0]).abs() > 2.0,
            "the column did not change at all: {before:?} then {after:?}"
        );
        // Wide enough for the longest line in it, and not much wider.
        let longest =
            editor.pages[0].lines.iter().map(|line| line.right - line.left).fold(0.0, f32::max);
        assert!(after[0] >= longest, "the text no longer fits: {after:?} against {longest}");
        assert!(after[0] < longest * 1.4, "the column is far wider than its text: {after:?}");
    }

    #[test]
    fn double_clicking_in_the_middle_of_a_cell_still_takes_the_word() {
        // The fit is a double click on the *line*. In the cell it has to go on
        // meaning what a double click has always meant.
        let mut editor = editor();
        write(&mut editor, 0, 0, "Chosen word");
        editor.relayout();
        let (_, cell) = editor.placed_cell(0, 0).expect("a cell");
        let (origin_x, origin_y) = editor.page_origin(0);
        let up = editor.content_top() + origin_y - editor.scroll_down();

        editor.handle(Event::DoubleClick {
            x: (origin_x + cell.x + 20.0) as i32,
            y: (up + cell.y + cell.height / 2.0) as i32,
        });
        assert_eq!(editor.document.selected_text(), "Chosen", "the word was not taken");
    }
}

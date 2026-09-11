//! Text in a cell turned a right angle: Word's Text Direction.

use wp_docx::model::{Block, Body, Paragraph, Table, TableCell, TableRow, TextDirection};
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics, Renderer, Turn};
use wp_raster::{Canvas, Color};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A cell holding one line of text, turned whichever way is asked.
fn cell(text: &str, direction: TextDirection) -> TableCell {
    TableCell { direction, ..TableCell::text(text) }
}

/// A document with one two-column table: the first cell as asked, the second
/// the ordinary way up.
fn document(direction: TextDirection) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
    body.blocks.push(Block::Table(Box::new(
        Table::from_rows(vec![TableRow::from_cells(vec![
            cell("Turned heading", direction),
            cell("Across", TextDirection::Horizontal),
        ])])
        .with_grid(vec![2000, 6000]),
    )));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// The pages of that document.
fn pages(direction: TextDirection) -> Vec<Page> {
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(&document(direction), PageMetrics::default())
}

/// Every line of a page that is turned.
fn turned_lines(page: &Page) -> Vec<&wp_layout::PageLine> {
    page.lines.iter().filter(|line| line.frame.is_turned()).collect()
}

#[test]
fn a_turned_cell_makes_the_row_as_tall_as_its_text_is_long() {
    let straight = pages(TextDirection::Horizontal);
    let turned = pages(TextDirection::Down);

    let row_height =
        |pages: &[Page]| pages[0].cells.iter().map(|cell| cell.height).fold(0.0f32, f32::max);
    let (before, after) = (row_height(&straight), row_height(&turned));
    assert!(after > before * 2.0, "the row did not grow: {before} then {after}");
}

#[test]
fn the_turned_text_stays_inside_its_cell() {
    for direction in [TextDirection::Down, TextDirection::Up] {
        let pages = pages(direction);
        let page = &pages[0];
        let cell = page.cells.first().copied().expect("a cell");

        let turned: Vec<_> = turned_lines(page);
        assert!(!turned.is_empty(), "{direction:?} put no turned line on the page");

        for line in &turned {
            for index in line.glyphs.clone() {
                let glyph = &page.glyphs[index];
                assert!(
                    glyph.x >= cell.x - 1.0 && glyph.x <= cell.x + cell.width + 1.0,
                    "{direction:?} put a letter at {} outside {}..{}",
                    glyph.x,
                    cell.x,
                    cell.x + cell.width
                );
                assert!(
                    glyph.baseline >= cell.y - 1.0 && glyph.baseline <= cell.y + cell.height + 1.0,
                    "{direction:?} put a letter at {} outside {}..{}",
                    glyph.baseline,
                    cell.y,
                    cell.y + cell.height
                );
            }
        }
    }
}

#[test]
fn text_that_reads_downwards_runs_down_the_page() {
    let pages = pages(TextDirection::Down);
    let page = &pages[0];
    let line = turned_lines(page).first().copied().expect("a turned line").clone();

    let first = &page.glyphs[line.glyphs.start];
    let last = &page.glyphs[line.glyphs.end - 1];
    assert!(last.baseline > first.baseline, "the letters do not go downwards");
    assert!((last.x - first.x).abs() < 1.0, "the letters do not stay in one column");
}

#[test]
fn text_that_reads_upwards_runs_up_the_page() {
    let pages = pages(TextDirection::Up);
    let page = &pages[0];
    let line = turned_lines(page).first().copied().expect("a turned line").clone();

    let first = &page.glyphs[line.glyphs.start];
    let last = &page.glyphs[line.glyphs.end - 1];
    assert!(last.baseline < first.baseline, "the letters do not go upwards");
}

#[test]
fn the_two_directions_are_each_other_the_other_way_round() {
    let down = pages(TextDirection::Down);
    let up = pages(TextDirection::Up);
    let line_of = |pages: &[Page]| {
        let page = &pages[0];
        let line = turned_lines(page).first().copied().expect("a turned line").clone();
        let first = &page.glyphs[line.glyphs.start];
        (first.x, first.baseline)
    };
    let (_, down_y) = line_of(&down);
    let (_, up_y) = line_of(&up);

    // One starts at the top of the cell and the other at the bottom, which is
    // what reading downwards and reading upwards mean.
    assert!(down_y < up_y, "both directions started at the same end");
}

#[test]
fn a_click_in_a_turned_cell_lands_in_its_text() {
    let pages = pages(TextDirection::Down);
    let page = &pages[0];
    let line = turned_lines(page).first().copied().expect("a turned line").clone();
    let glyph = &page.glyphs[line.glyphs.start + 3];

    // A point on the fourth letter, which is in the turned cell and nowhere
    // near the ordinary line beside it.
    let found = page.position_at(glyph.x, glyph.baseline).expect("a position");
    assert_eq!(found.paragraph, glyph.source.paragraph, "the click left the cell");
    assert!(
        found.offset == glyph.source.offset || found.offset == glyph.source.offset + 1,
        "the click landed at {} rather than near {}",
        found.offset,
        glyph.source.offset
    );
}

#[test]
fn the_caret_in_a_turned_cell_lies_the_other_way() {
    let pages = pages(TextDirection::Down);
    let page = &pages[0];
    let line = turned_lines(page).first().copied().expect("a turned line").clone();

    let at = TextPosition::new(line.paragraph, 1);
    let (_, _, width, height) = page.caret_at(at, 2.0).expect("a caret");
    assert!(width > height, "the caret is standing up in a cell whose text lies down");

    // And an ordinary line's caret stands up.
    let straight = page.lines.iter().find(|line| !line.frame.is_turned()).expect("a line");
    let at = TextPosition::new(straight.paragraph, 1);
    let (_, _, width, height) = page.caret_at(at, 2.0).expect("a caret");
    assert!(height > width, "the caret is lying down in ordinary text");
}

#[test]
fn a_selection_band_in_a_turned_cell_lies_the_other_way() {
    let pages = pages(TextDirection::Down);
    let page = &pages[0];
    let line = turned_lines(page).first().copied().expect("a turned line").clone();

    let rects = page.selection_rects(
        TextPosition::new(line.paragraph, 0),
        TextPosition::new(line.paragraph, 6),
    );
    let band = rects.first().copied().expect("a band");
    assert!(band.3 > band.2, "the band across a turned line is wider than it is tall");
}

#[test]
fn the_letters_are_drawn_turned_and_not_merely_moved() {
    // The proof that the outlines themselves are turned: a page of turned text
    // makes ink in a tall narrow band, where the same text drawn straight makes
    // it in a wide flat one.
    let extent = |direction: TextDirection| {
        let pages = pages(direction);
        let page = &pages[0];
        let mut renderer = Renderer::new(library());
        let canvas: Canvas = renderer.render(page, Color::WHITE);

        let line = turned_lines(page).first().copied().or_else(|| page.lines.first());
        let _ = line;
        // The ink in the first cell's part of the page.
        let cell = page.cells.first().copied().expect("a cell");
        let (mut left, mut top, mut right, mut bottom) = (usize::MAX, usize::MAX, 0usize, 0usize);
        for y in (cell.y.max(0.0) as usize)..((cell.y + cell.height) as usize).min(canvas.height())
        {
            for x in
                (cell.x.max(0.0) as usize)..((cell.x + cell.width) as usize).min(canvas.width())
            {
                if canvas.pixel(x, y) != Color::WHITE {
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x);
                    bottom = bottom.max(y);
                }
            }
        }
        assert!(right > left && bottom > top, "nothing was drawn in the cell");
        (right - left, bottom - top)
    };

    let (straight_across, straight_down) = extent(TextDirection::Horizontal);
    let (turned_across, turned_down) = extent(TextDirection::Down);
    assert!(straight_across > straight_down, "straight text is not wider than it is tall");
    assert!(turned_down > turned_across, "turned text is not taller than it is wide");
}

#[test]
fn which_way_up_a_cell_is_survives_being_saved_and_opened_again() {
    for direction in [TextDirection::Down, TextDirection::Up, TextDirection::Horizontal] {
        let document = document(direction);
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
        assert_eq!(table.rows[0].cells[0].direction, direction);
        assert_eq!(table.rows[0].cells[1].direction, TextDirection::Horizontal);
    }
}

#[test]
fn a_page_of_ordinary_text_says_none_of_it_is_turned() {
    let pages = pages(TextDirection::Horizontal);
    let page = &pages[0];
    assert!(page.turned.is_empty(), "something was marked as turned");
    assert_eq!(page.turn_of(0), Turn::None);
}

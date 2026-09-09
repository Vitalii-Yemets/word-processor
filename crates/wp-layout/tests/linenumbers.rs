//! The numbers down the margin, where a section asks for them.

use wp_docx::appearance::{LineNumbers, Restart};
use wp_docx::model::{Block, Body, Paragraph, Table, TableCell, TableRow};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A document of a few short paragraphs.
fn document(paragraphs: usize) -> Document {
    let mut body = Body::default();
    for index in 0..paragraphs {
        body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn pages(document: &Document) -> Vec<Page> {
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(document, PageMetrics::default())
}

/// The glyphs drawn to the left of every line of text, which is where the
/// numbers go and where nothing else is drawn.
fn in_the_margin(page: &Page) -> usize {
    let text_left = page.lines.iter().map(|line| line.left).fold(f32::MAX, f32::min);
    page.glyphs.iter().filter(|glyph| glyph.x < text_left - 1.0 && !glyph.invisible).count()
}

#[test]
fn nothing_is_numbered_until_a_section_asks() {
    let laid = pages(&document(5));
    assert_eq!(in_the_margin(&laid[0]), 0);
}

#[test]
fn every_line_is_numbered_when_the_section_asks() {
    let mut document = document(5);
    document.set_line_numbers(Some(LineNumbers::default()));
    let laid = pages(&document);
    // Five lines, five numbers, each one figure.
    assert_eq!(in_the_margin(&laid[0]), 5);
}

#[test]
fn counting_by_five_prints_every_fifth() {
    let mut document = document(12);
    document.set_line_numbers(Some(LineNumbers { count_by: 5, ..LineNumbers::default() }));
    let laid = pages(&document);
    // Twelve lines: 5 and 10 are printed, and each is one or two figures.
    assert_eq!(in_the_margin(&laid[0]), 3, "5 and 10 make three figures between them");
}

#[test]
fn the_numbering_can_start_at_a_number_of_its_own() {
    let mut document = document(3);
    document.set_line_numbers(Some(LineNumbers { start: 10, ..LineNumbers::default() }));
    let laid = pages(&document);
    // 10, 11, 12: two figures each.
    assert_eq!(in_the_margin(&laid[0]), 6);
}

#[test]
fn the_lines_inside_a_table_are_not_counted() {
    // Word counts the lines of the text and not the rows of a table, and a
    // program that counted them would number a contract's schedule.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
    body.blocks.push(Block::Table(Box::new(Table {
        rows: vec![TableRow {
            cells: vec![TableCell {
                blocks: vec![Block::Paragraph(Paragraph::text("In a cell"))],
                ..TableCell::default()
            }],
            ..TableRow::default()
        }],
        ..Table::default()
    })));
    body.blocks.push(Block::Paragraph(Paragraph::text("After")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document.set_line_numbers(Some(LineNumbers::default()));

    let laid = pages(&document);
    // Two numbered lines, not three: the cell is not one of them.
    assert_eq!(in_the_margin(&laid[0]), 2);
}

#[test]
fn the_numbering_can_start_again_on_every_page() {
    let mut document = document(90);
    document.set_line_numbers(Some(LineNumbers {
        restart: Restart::NewPage,
        ..LineNumbers::default()
    }));
    let laid = pages(&document);
    assert!(laid.len() > 1, "the document did not run over a page");

    // The first line of the second page is numbered 1 again, so the second page
    // draws no more figures than the first for the same number of lines.
    let first = in_the_margin(&laid[0]);
    let second = in_the_margin(&laid[1]);
    assert!(second <= first, "the numbers went on rising: {first} then {second}");
}

#[test]
fn the_numbering_runs_on_when_nothing_says_otherwise() {
    let mut document = document(90);
    document.set_line_numbers(Some(LineNumbers::default()));
    let laid = pages(&document);
    assert!(laid.len() > 1);

    // Running on means the second page's numbers have more figures than the
    // first page's, because they are larger.
    let first = in_the_margin(&laid[0]);
    let second = in_the_margin(&laid[1]);
    assert!(second > 0 && first > 0);
    let lines_first = laid[0].lines.len();
    let lines_second = laid[1].lines.len();
    assert!(
        second as f32 / lines_second as f32 > first as f32 / lines_first as f32,
        "the numbers did not grow: {first} for {lines_first}, {second} for {lines_second}"
    );
}

//! Breaks that end a page or a column, and a document laid out as a browser
//! would show it.

use wp_docx::furniture::{Furniture, Which};
use wp_docx::model::{Block, Body, BreakKind, Paragraph, Run, RunContent};
use wp_docx::notes::Kind;
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A paragraph with a break of some kind in the middle of it.
fn broken(kind: BreakKind) -> Paragraph {
    Paragraph::from_runs(vec![
        Run::text("Before the break."),
        Run { content: vec![RunContent::Break(kind)], ..Run::text("") },
        Run::text("After the break."),
    ])
}

fn document_of(blocks: Vec<Block>) -> Document {
    let bytes = Document::create(&Body { blocks }).expect("a document").save().expect("saved");
    Document::open(&bytes).expect("reopened")
}

/// Which page the line holding some paragraph's text at an offset landed on.
fn page_of(pages: &[Page], paragraph: usize, offset: usize) -> Option<usize> {
    pages.iter().position(|page| {
        page.lines.iter().any(|line| {
            line.paragraph == paragraph && line.start_offset <= offset && offset < line.end_offset
        })
    })
}

#[test]
fn a_page_break_in_a_paragraph_begins_a_new_page() {
    if library().is_empty() {
        return;
    }
    let document = document_of(vec![Block::Paragraph(broken(BreakKind::Page))]);
    let pages = LayoutEngine::new(library()).layout_document(&document);
    assert_eq!(pages.len(), 2, "the break did not end the page");
    let after = "Before the break.".len() + 1;
    assert_eq!(page_of(&pages, 0, 0), Some(0));
    assert_eq!(
        page_of(&pages, 0, after),
        Some(1),
        "what follows the break is not on the next page"
    );
}

#[test]
fn a_column_break_moves_to_the_next_column_and_a_page_break_past_them_all() {
    if library().is_empty() {
        return;
    }
    let mut document = document_of(vec![
        Block::Paragraph(broken(BreakKind::Column)),
        Block::Paragraph(broken(BreakKind::Page)),
    ]);
    assert!(document.set_columns(2, 720));
    let pages = LayoutEngine::new(library()).layout_document(&document);
    let after = "Before the break.".len() + 1;
    // The column break: the same page, further across.
    assert_eq!(page_of(&pages, 0, after), Some(0));
    let left_of = |paragraph: usize, offset: usize| {
        pages[0]
            .lines
            .iter()
            .find(|line| {
                line.paragraph == paragraph
                    && line.start_offset <= offset
                    && offset < line.end_offset
            })
            .map(|line| line.left)
    };
    assert!(
        left_of(0, after) > left_of(0, 0),
        "the text after the column break did not move across"
    );
    // The page break, from the second column: a new page, not a third column.
    assert_eq!(page_of(&pages, 1, after), Some(1));
}

#[test]
fn a_break_inside_a_table_cell_breaks_no_page() {
    if library().is_empty() {
        return;
    }
    let cell = wp_docx::model::TableCell {
        blocks: vec![Block::Paragraph(broken(BreakKind::Page))],
        ..wp_docx::model::TableCell::default()
    };
    let table = wp_docx::model::Table {
        rows: vec![wp_docx::model::TableRow { cells: vec![cell], ..Default::default() }],
        grid: vec![4000],
        ..Default::default()
    };
    let document = document_of(vec![
        Block::Table(Box::new(table)),
        Block::Paragraph(Paragraph::text("After the table.")),
    ]);
    assert_eq!(LayoutEngine::new(library()).layout_document(&document).len(), 1);
}

/// A document with everything a web page has no room for: a header and a
/// footer, a page break, a second section in two columns, and a footnote.
fn document_for_the_web() -> Document {
    let mut blocks = vec![Block::Paragraph(Paragraph::text("A first paragraph with a note."))];
    blocks.push(Block::Paragraph(broken(BreakKind::Page)));
    blocks.push(Block::Paragraph(Paragraph::text("The end of the first section.")));
    for index in 0..40 {
        blocks.push(Block::Paragraph(Paragraph::text(&format!(
            "Words in the second section {index}."
        ))));
    }
    let mut document = document_of(blocks);
    document.set_caret(TextPosition::new(0, "A first paragraph with a note".len()));
    document.add_note(Kind::Footnote, "The note.").expect("a note");
    let head = Body { blocks: vec![Block::Paragraph(Paragraph::text("Running head"))] };
    document.set_caret(TextPosition::new(0, 0));
    document.set_furniture_body(Furniture::Header, Which::Default, &head).expect("a header");
    assert!(document.end_section_at(2, wp_docx::sections::Start::NextPage));
    document.set_caret(TextPosition::new(3, 0));
    assert!(document.set_columns(2, 720));
    document
}

#[test]
fn a_page_on_the_web_is_one_sheet_in_one_column_with_the_notes_at_the_end() {
    if library().is_empty() {
        return;
    }
    let document = document_for_the_web();
    let printed = LayoutEngine::new(library()).layout_document(&document);
    assert!(printed.len() >= 3, "on paper it is three pages at least: {}", printed.len());

    let metrics = PageMetrics {
        width: 700.0,
        height: f32::MAX / 4.0,
        margin_top: 36.0,
        margin_right: 36.0,
        margin_bottom: 36.0,
        margin_left: 36.0,
        ..PageMetrics::from_document(&document)
    };
    let mut engine = LayoutEngine::new(library());
    engine.set_web(true);
    let pages = engine.layout_document_with(&document, metrics);
    assert_eq!(pages.len(), 1, "a page on the web has no page breaks");
    let page = &pages[0];
    assert!(
        page.height < 20_000.0,
        "the sheet is as long as its text, not endless: {}",
        page.height
    );

    // One column: every line of the second section starts at the margin.
    let lefts: Vec<f32> =
        page.lines.iter().filter(|line| line.paragraph >= 3).map(|line| line.left).collect();
    assert!(!lefts.is_empty());
    assert!(lefts.iter().all(|left| (*left - lefts[0]).abs() < 1.0), "{lefts:?}");

    // No header: the first thing on the sheet is the first paragraph.
    let first = page.lines.iter().map(|line| line.baseline).fold(f32::MAX, f32::min);
    let body_first = page.lines.iter().find(|line| line.paragraph == 0).map(|line| line.baseline);
    assert_eq!(Some(first), body_first, "something was drawn above the text");

    // The note is under the last line of the text, not at the foot of a page.
    let last_line = page.lines.iter().map(|line| line.baseline).fold(0.0f32, f32::max);
    let lowest_glyph = page.glyphs.iter().map(|glyph| glyph.baseline).fold(0.0f32, f32::max);
    assert!(lowest_glyph > last_line, "the note is not after the text");
    assert!(lowest_glyph < page.height, "the note is off the sheet");

    // And the same document, shown as itself again, is on paper once more.
    engine.set_web(false);
    assert_eq!(engine.layout_document(&document).len(), printed.len());
}

/// The family of the face the first glyph of a document was drawn with.
fn family_drawn(document: &Document) -> String {
    let pages = LayoutEngine::new(library()).layout_document(document);
    let glyph = pages[0].glyphs.first().expect("a glyph");
    library().face(glyph.face).expect("the face").family.clone()
}

#[test]
fn a_missing_font_is_stood_in_for_by_what_the_font_table_says() {
    use wp_docx::fonts::{FontClass, FontEntry};
    if !library().has_family("DejaVu Serif") || !library().has_family("DejaVu Sans Mono") {
        return;
    }
    let paragraph = |font: &str| {
        let mut run = Run::text("Words");
        run.properties.font = Some(font.to_owned());
        Block::Paragraph(Paragraph::from_runs(vec![run]))
    };
    // Nothing known about it: the ordinary stand-in, without serifs.
    let unknown = document_of(vec![paragraph("A Serif Nobody Has")]);
    let plain = family_drawn(&unknown);
    assert!(!plain.to_lowercase().contains("serif") || plain.to_lowercase().contains("sans"));

    // Said to have serifs: one with serifs.
    let mut said = document_of(vec![paragraph("A Serif Nobody Has")]);
    said.set_font_table(&[FontEntry {
        name: "A Serif Nobody Has".to_owned(),
        class: FontClass::Roman,
        ..FontEntry::default()
    }])
    .expect("the table");
    let family = family_drawn(&said);
    assert!(family.contains("Serif") && !family.contains("Sans"), "{family}");

    // Said to be a typewriter's: every letter the same width.
    let mut fixed = document_of(vec![paragraph("A Typewriter Nobody Has")]);
    fixed
        .set_font_table(&[FontEntry {
            name: "A Typewriter Nobody Has".to_owned(),
            fixed_pitch: Some(true),
            ..FontEntry::default()
        }])
        .expect("the table");
    assert!(family_drawn(&fixed).contains("Mono"), "{}", family_drawn(&fixed));

    // And another name it goes by, which the machine has, before any of that.
    let mut other = document_of(vec![paragraph("Calibri Nobody Has")]);
    other
        .set_font_table(&[FontEntry {
            name: "Calibri Nobody Has".to_owned(),
            alt_name: Some("DejaVu Sans Mono".to_owned()),
            class: FontClass::Roman,
            ..FontEntry::default()
        }])
        .expect("the table");
    assert_eq!(family_drawn(&other), "DejaVu Sans Mono");
}

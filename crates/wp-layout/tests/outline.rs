//! A document laid out as Word's outline view shows it: its headings and
//! the text under them, on the web's one sheet, with no notes, folded where
//! the view folds it, a line at a time where it asks, and plain where it
//! asks.

use wp_docx::model::{Block, Body, Paragraph, Run, Table, TableCell, TableRow};
use wp_docx::notes::Kind;
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

fn document_of(blocks: Vec<Block>) -> Document {
    let bytes = Document::create(&Body { blocks }).expect("a document").save().expect("saved");
    Document::open(&bytes).expect("reopened")
}

fn heading(level: u8, text: &str) -> Block {
    Block::Paragraph(Paragraph::text(text).with_style(&format!("Heading{level}")))
}

fn body(text: &str) -> Block {
    Block::Paragraph(Paragraph::text(text))
}

/// A table of one row and two cells.
fn table() -> Block {
    let cell = |text: &str| TableCell {
        blocks: vec![Block::Paragraph(Paragraph::text(text))],
        ..TableCell::default()
    };
    Block::Table(Box::new(Table {
        rows: vec![TableRow { cells: vec![cell("Left"), cell("Right")], ..Default::default() }],
        grid: vec![2000, 2000],
        ..Default::default()
    }))
}

/// The web's sheet, which the outline is laid out on too.
fn sheet(document: &Document) -> PageMetrics {
    PageMetrics {
        width: 700.0,
        height: f32::MAX / 4.0,
        margin_top: 36.0,
        margin_right: 36.0,
        margin_bottom: 36.0,
        margin_left: 36.0,
        ..PageMetrics::from_document(document)
    }
}

/// An engine set up the way the window sets one up for its outline view.
fn outline_engine(depth: u8) -> LayoutEngine<'static> {
    let mut engine = LayoutEngine::new(library());
    engine.set_web(true);
    engine.set_outline(Some(depth));
    engine
}

/// The paragraphs folded away under a heading: from one number up to
/// another.
fn folded(from: usize, to: usize) -> Vec<core::ops::Range<usize>> {
    std::iter::once(from..to).collect()
}

/// How many lines a paragraph was given.
fn lines_of(pages: &[Page], paragraph: usize) -> usize {
    pages.iter().flat_map(|page| &page.lines).filter(|line| line.paragraph == paragraph).count()
}

/// Where a paragraph's first line begins.
fn left_of(pages: &[Page], paragraph: usize) -> f32 {
    pages
        .iter()
        .flat_map(|page| &page.lines)
        .find(|line| line.paragraph == paragraph)
        .map(|line| line.left)
        .unwrap_or_else(|| panic!("paragraph {paragraph} has no line"))
}

#[test]
fn an_outline_shows_no_notes_where_the_web_shows_them_after_the_text() {
    if library().is_empty() {
        return;
    }
    let mut document = document_of(vec![
        heading(1, "A heading"),
        body("A paragraph with a note."),
        body("And one after it."),
    ]);
    document.set_caret(TextPosition::new(1, "A paragraph with a note".len()));
    document.add_note(Kind::Footnote, "The footnote.").expect("a footnote");
    document.set_caret(TextPosition::new(2, 3));
    document.add_note(Kind::Endnote, "The endnote.").expect("an endnote");
    let metrics = sheet(&document);

    // On the web, the notes come after the text, below its last line.
    let mut web = LayoutEngine::new(library());
    web.set_web(true);
    let pages = web.layout_document_with(&document, metrics);
    let last_line = pages[0].lines.iter().map(|line| line.baseline).fold(0.0f32, f32::max);
    let lowest = pages[0].glyphs.iter().map(|glyph| glyph.baseline).fold(0.0f32, f32::max);
    assert!(lowest > last_line, "the web page lost its notes");

    // In the outline, nothing is drawn below the last line of the text.
    let pages = outline_engine(10).layout_document_with(&document, metrics);
    let last_line = pages[0].lines.iter().map(|line| line.baseline).fold(0.0f32, f32::max);
    let lowest = pages[0].glyphs.iter().map(|glyph| glyph.baseline).fold(0.0f32, f32::max);
    assert!(lowest <= last_line + 0.5, "the outline drew the notes: {lowest} past {last_line}");
    assert!(pages[0].decorations.is_empty(), "the rule over the notes is drawn");
}

#[test]
fn what_a_heading_is_folded_over_is_left_out_and_the_rest_is_not() {
    if library().is_empty() {
        return;
    }
    let document = document_of(vec![
        heading(1, "First"),
        body("Under the first."),
        heading(1, "Second"),
        body("Under the second."),
    ]);
    let mut engine = outline_engine(10);
    engine.set_outline_folded(folded(1, 2));
    let pages = engine.layout_document_with(&document, sheet(&document));
    assert_eq!(lines_of(&pages, 1), 0, "the text folded under the first heading is drawn");
    for shown in [0, 2, 3] {
        assert!(lines_of(&pages, shown) > 0, "paragraph {shown} is missing");
    }

    // And unfolded, it is back.
    engine.set_outline_folded(Vec::new());
    let pages = engine.layout_document_with(&document, sheet(&document));
    assert!(lines_of(&pages, 1) > 0);
}

#[test]
fn a_table_is_body_text_to_an_outline() {
    if library().is_empty() {
        return;
    }
    let document = document_of(vec![heading(1, "Heading"), table(), heading(1, "After")]);
    let metrics = sheet(&document);

    // All levels: the table is drawn, a step in from its heading.
    let pages = outline_engine(10).layout_document_with(&document, metrics);
    assert!(!pages[0].cells.is_empty(), "the table is not drawn");
    let heading_left = left_of(&pages, 0);
    let table_left = pages[0].cells.iter().map(|cell| cell.x).fold(f32::MAX, f32::min);
    assert!(table_left > heading_left + 10.0, "the table is not a step in: {table_left}");

    // Headings only: no body text, and so no table — not an empty grid.
    let pages = outline_engine(1).layout_document_with(&document, metrics);
    assert!(pages[0].cells.is_empty(), "a table was drawn with the body text hidden");
    assert!(lines_of(&pages, 3) > 0, "the heading after it went with it");

    // Folded away under its heading, the same.
    let mut engine = outline_engine(10);
    engine.set_outline_folded(folded(1, 3));
    let pages = engine.layout_document_with(&document, metrics);
    assert!(pages[0].cells.is_empty(), "a folded table was drawn");
    assert_eq!(lines_of(&pages, 1) + lines_of(&pages, 2), 0);
}

#[test]
fn first_line_only_leaves_body_text_a_line_and_an_ellipsis() {
    if library().is_empty() {
        return;
    }
    let long = "A paragraph of body text long enough to run over several lines of the \
                sheet it is laid out on, so that there is something after its first line \
                for the outline to leave out and say it has left out. "
        .repeat(3);
    let document = document_of(vec![heading(1, "Heading"), body(&long), body("Short.")]);
    let metrics = sheet(&document);

    let pages = outline_engine(10).layout_document_with(&document, metrics);
    assert!(lines_of(&pages, 1) > 2, "the paragraph is not long enough to prove anything");

    let mut engine = outline_engine(10);
    engine.set_outline_first_line(true);
    let pages = engine.layout_document_with(&document, metrics);
    assert_eq!(lines_of(&pages, 1), 1, "more than the first line is shown");
    assert_eq!(lines_of(&pages, 2), 1, "a paragraph of one line lost it");
    assert_eq!(lines_of(&pages, 0), 1, "the heading lost its line");
    // The ellipsis: glyphs of no line, at the end of the first line.
    let page = &pages[0];
    let line = page.lines.iter().find(|line| line.paragraph == 1).expect("the line");
    let outside: Vec<_> = page
        .glyphs
        .iter()
        .enumerate()
        .filter(|(at, _)| !page.lines.iter().any(|line| line.glyphs.contains(at)))
        .map(|(_, glyph)| glyph)
        .collect();
    assert!(
        outside.iter().any(
            |glyph| glyph.x >= line.right - 0.5 && (glyph.baseline - line.baseline).abs() < 0.5
        ),
        "no ellipsis after the first line"
    );
}

#[test]
fn an_outline_without_its_formatting_draws_every_run_in_one_font_and_size() {
    if library().is_empty() {
        return;
    }
    let mut bold = Run::text("Large and bold");
    bold.properties.bold = Some(true);
    bold.properties.size_half_points = Some(48);
    let document = document_of(vec![
        heading(1, "Heading"),
        Block::Paragraph(Paragraph::from_runs(vec![bold])),
        body("Plain body text"),
    ]);
    let metrics = sheet(&document);
    let size_of = |pages: &[Page], paragraph: usize| {
        let line = pages[0].lines.iter().find(|line| line.paragraph == paragraph).expect("a line");
        let glyph = &pages[0].glyphs[line.glyphs.start];
        (glyph.size, glyph.face)
    };

    let pages = outline_engine(10).layout_document_with(&document, metrics);
    assert_ne!(size_of(&pages, 0), size_of(&pages, 2), "the heading is drawn as body text");

    let mut engine = outline_engine(10);
    engine.set_outline_plain(true);
    let pages = engine.layout_document_with(&document, metrics);
    assert_eq!(size_of(&pages, 0), size_of(&pages, 2), "the heading kept its formatting");
    assert_eq!(size_of(&pages, 1), size_of(&pages, 2), "the run kept its formatting");
}

#[test]
fn body_text_is_a_step_in_from_its_heading_and_a_title_where_body_text_is() {
    if library().is_empty() {
        return;
    }
    let document = document_of(vec![
        Block::Paragraph(
            Paragraph::text("A title, centred")
                .with_style("Title")
                .with_alignment(wp_docx::model::Alignment::Center),
        ),
        heading(1, "Heading"),
        body("Under it."),
        heading(2, "Deeper"),
        body("Under that."),
    ]);
    let pages = outline_engine(10).layout_document_with(&document, sheet(&document));
    let (title, first, under, deeper, under_deeper) = (
        left_of(&pages, 0),
        left_of(&pages, 1),
        left_of(&pages, 2),
        left_of(&pages, 3),
        left_of(&pages, 4),
    );
    assert!(under > first + 10.0, "body text is not a step in from its heading");
    assert!((title - under).abs() < 0.5, "a title is not where body text is: {title} {under}");
    assert!((deeper - under).abs() < 0.5, "a second level is not a step in from the first");
    assert!(under_deeper > deeper + 10.0);
}

#[test]
fn a_heading_kept_with_what_is_left_out_stays_where_it_is() {
    if library().is_empty() {
        return;
    }
    // A heading asks to stay with the paragraph after it. Left out of the
    // outline, that paragraph put nothing on the page, and was taken for one
    // that began a new page: the heading went with it, to a page a quarter
    // of the largest float down the outline's one sheet.
    let document = document_of(vec![
        heading(1, "First"),
        body("Under the first."),
        heading(1, "Second"),
        body("Under the second."),
    ]);
    let metrics = sheet(&document);
    let lowest = |pages: &[Page]| {
        pages.iter().flat_map(|page| &page.lines).map(|line| line.baseline).fold(0.0f32, f32::max)
    };

    let pages = outline_engine(1).layout_document_with(&document, metrics);
    assert_eq!(pages.len(), 1);
    assert!(lowest(&pages) < 1_000.0, "a heading went down the sheet: {}", lowest(&pages));

    let mut engine = outline_engine(10);
    engine.set_outline_folded(folded(1, 2));
    let pages = engine.layout_document_with(&document, metrics);
    assert!(lowest(&pages) < 1_000.0, "a heading went down the sheet: {}", lowest(&pages));
}

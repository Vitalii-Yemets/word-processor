//! The rules about where a paragraph may be broken across a page.
//!
//! Word never leaves the first line of a paragraph alone at the foot of a page,
//! nor the last line alone at the top of the next, and it never splits a
//! paragraph that has been told to stay whole. A program that broke pages
//! anywhere would put its page breaks in different places from Word's for
//! almost every document, which is the most visible way of being wrong.

use wp_docx::model::{Block, Body, Paragraph, ParagraphProperties};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// Words enough to fill a given number of lines of an ordinary page.
fn lines_of_text(lines: usize) -> String {
    let mut text = String::new();
    for index in 0..lines {
        // Roughly a line each at the default size on A4 with inch margins.
        text.push_str(&format!(
            "Line {index} of this paragraph, written long enough that it fills the width. "
        ));
    }
    text
}

/// A document of filler paragraphs, then one built by the caller.
fn document(filler: usize, last: Paragraph) -> Document {
    let mut body = Body::default();
    for index in 0..filler {
        body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
    }
    body.blocks.push(Block::Paragraph(last));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn pages(document: &Document) -> Vec<Page> {
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(document, PageMetrics::default())
}

/// How many lines of one paragraph landed on each page.
fn lines_per_page(pages: &[Page], paragraph: usize) -> Vec<usize> {
    pages
        .iter()
        .map(|page| page.lines.iter().filter(|line| line.paragraph == paragraph).count())
        .collect()
}

/// The number of filler paragraphs that just fills a page, leaving room for one
/// or two more lines.
fn filler_to_fill_a_page() -> usize {
    let mut count = 1;
    loop {
        let laid = pages(&document(count, Paragraph::text("x")));
        if laid.len() > 1 {
            return count - 2;
        }
        count += 1;
        assert!(count < 200, "the page never filled");
    }
}

#[test]
fn a_paragraph_may_be_broken_across_a_page_when_nothing_says_otherwise() {
    let filler = filler_to_fill_a_page();
    // Both rules off: the break may fall anywhere.
    let properties =
        ParagraphProperties { widow_control: Some(false), ..ParagraphProperties::default() };
    let paragraph = Paragraph { properties, runs: Paragraph::text(&lines_of_text(6)).runs };

    let laid = pages(&document(filler, paragraph));
    assert!(laid.len() > 1, "the document did not run over a page");
    let spread = lines_per_page(&laid, filler);
    assert!(spread[0] > 0 && spread[1] > 0, "the paragraph did not cross the break: {spread:?}");
}

#[test]
fn a_single_first_line_is_not_left_behind() {
    // With one line's room left on the page, an ordinary paragraph moves whole
    // to the next page rather than leaving its first line alone.
    let filler = filler_to_fill_a_page() + 1;
    let paragraph = Paragraph::text(&lines_of_text(6));

    let laid = pages(&document(filler, paragraph));
    let spread = lines_per_page(&laid, filler);
    assert_ne!(spread[0], 1, "one line was left behind: {spread:?}");
}

#[test]
fn a_single_last_line_is_not_left_alone_at_the_top() {
    // Whatever the paragraph's length, the page it carries over to never gets
    // exactly one of its lines while the page before it keeps the rest.
    for length in 2..9 {
        for extra in 0..3 {
            let filler = filler_to_fill_a_page() + extra;
            let paragraph = Paragraph::text(&lines_of_text(length));
            let laid = pages(&document(filler, paragraph));
            let spread = lines_per_page(&laid, filler);
            let crossed = spread.iter().filter(|count| **count > 0).count() > 1;
            if !crossed {
                continue;
            }
            let last = spread.iter().rposition(|count| *count > 0).expect("a page");
            assert_ne!(
                spread[last], 1,
                "a line was left alone at the top: {length} lines, {extra} extra, {spread:?}"
            );
        }
    }
}

#[test]
fn a_paragraph_told_to_stay_whole_moves_whole() {
    let filler = filler_to_fill_a_page() + 1;
    let properties =
        ParagraphProperties { keep_lines: Some(true), ..ParagraphProperties::default() };
    let paragraph = Paragraph { properties, runs: Paragraph::text(&lines_of_text(5)).runs };

    let laid = pages(&document(filler, paragraph));
    let spread = lines_per_page(&laid, filler);
    let touched = spread.iter().filter(|count| **count > 0).count();
    assert_eq!(touched, 1, "the paragraph was split: {spread:?}");
}

#[test]
fn a_paragraph_taller_than_a_page_is_still_broken() {
    // The rules cannot be obeyed, and a paragraph that cannot be laid out at
    // all would be worse than one broken in a place nobody asked for.
    let properties =
        ParagraphProperties { keep_lines: Some(true), ..ParagraphProperties::default() };
    let paragraph = Paragraph { properties, runs: Paragraph::text(&lines_of_text(90)).runs };

    let laid = pages(&document(0, paragraph));
    assert!(laid.len() > 1, "a paragraph longer than a page stayed on one");
    let spread = lines_per_page(&laid, 0);
    assert!(spread[0] > 0 && spread[1] > 0, "{spread:?}");
}

#[test]
fn a_heading_is_not_left_alone_at_the_foot_of_a_page() {
    // Every one of Word's heading styles asks to stay with what follows, which
    // is why a heading never ends a page there. Here the paragraph asks
    // directly, so the test does not depend on the styles.
    let filler = filler_to_fill_a_page() + 1;
    let heading = Paragraph {
        properties: ParagraphProperties { keep_next: Some(true), ..ParagraphProperties::default() },
        runs: Paragraph::text("A heading").runs,
    };

    let mut body = Body::default();
    for index in 0..filler {
        body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
    }
    body.blocks.push(Block::Paragraph(heading));
    body.blocks.push(Block::Paragraph(Paragraph::text(&lines_of_text(6))));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");
    let laid = pages(&document);

    let heading_page = laid
        .iter()
        .position(|page| page.lines.iter().any(|line| line.paragraph == filler))
        .expect("the heading was laid out");
    let text_page = laid
        .iter()
        .position(|page| page.lines.iter().any(|line| line.paragraph == filler + 1))
        .expect("the paragraph was laid out");
    assert_eq!(heading_page, text_page, "the heading was left on the page before its text");
}

#[test]
fn a_run_of_headings_moves_together() {
    let filler = filler_to_fill_a_page() + 2;
    let heading = || Paragraph {
        properties: ParagraphProperties { keep_next: Some(true), ..ParagraphProperties::default() },
        runs: Paragraph::text("A heading").runs,
    };

    let mut body = Body::default();
    for index in 0..filler {
        body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
    }
    body.blocks.push(Block::Paragraph(heading()));
    body.blocks.push(Block::Paragraph(heading()));
    body.blocks.push(Block::Paragraph(Paragraph::text(&lines_of_text(6))));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");
    let laid = pages(&document);

    let page_of = |paragraph: usize| {
        laid.iter()
            .position(|page| page.lines.iter().any(|line| line.paragraph == paragraph))
            .expect("laid out")
    };
    assert_eq!(page_of(filler), page_of(filler + 2), "the first heading was left behind");
    assert_eq!(page_of(filler + 1), page_of(filler + 2), "the second heading was left behind");
}

#[test]
fn a_page_break_the_person_asked_for_is_not_undone() {
    // A heading followed by a paragraph that begins a page stays where it is:
    // the break was asked for, and keeping them together would take it back.
    let heading = Paragraph {
        properties: ParagraphProperties { keep_next: Some(true), ..ParagraphProperties::default() },
        runs: Paragraph::text("A heading").runs,
    };
    let after = Paragraph {
        properties: ParagraphProperties {
            page_break_before: Some(true),
            ..ParagraphProperties::default()
        },
        runs: Paragraph::text("On a page of its own").runs,
    };

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(heading));
    body.blocks.push(Block::Paragraph(after));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");
    let laid = pages(&document);

    assert_eq!(laid.len(), 2, "the break went missing");
    assert!(laid[0].lines.iter().any(|line| line.paragraph == 0), "the heading moved");
}

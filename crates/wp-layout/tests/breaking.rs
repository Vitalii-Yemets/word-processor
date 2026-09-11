//! Where a line ends.
//!
//! The rules are the ones a reader notices only when they are broken: a line
//! that begins with a full stop, or a hyphenated word split in front of its
//! hyphen instead of after it.

use wp_docx::model::{Block, Body, Paragraph, Run, RunProperties};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, PageMetrics};

/// The fonts, read once per test.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// The lines a paragraph comes out as, as the text of each.
fn lines_of(runs: Vec<Run>) -> Vec<String> {
    let text: String = runs
        .iter()
        .flat_map(|run| run.content.iter())
        .filter_map(|content| match content {
            wp_docx::model::RunContent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(runs)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");

    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document_with(&document, PageMetrics::default());
    pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .map(|line| text[line.start_offset..line.end_offset].to_owned())
        .collect()
}

/// The same, for a paragraph of one run.
fn lines(text: &str) -> Vec<String> {
    lines_of(vec![Run::text(text)])
}

/// Enough words to reach past the end of a line, ending just before the piece
/// of text a test is about.
fn filler(width: usize) -> String {
    "filler ".repeat(width)
}

#[test]
fn a_paragraph_wider_than_the_page_is_broken_into_lines() {
    let lines = lines(&filler(40));
    assert!(lines.len() > 1, "nothing was broken: {lines:?}");
    assert!(lines.iter().all(|line| !line.is_empty()));
}

#[test]
fn a_line_never_begins_with_a_full_stop() {
    // Long enough that some line ends near the sentence's end whatever the
    // fonts measure, and repeated so it happens many times over.
    let text = "A sentence of a certain length that ends in a full stop. ".repeat(20);
    let lines = lines(&text);
    assert!(lines.len() > 1, "the text never wrapped, so the rule was not tried");
    for line in &lines {
        let first = line.trim_start().chars().next();
        assert!(
            !matches!(first, Some('.') | Some(',')),
            "a line began with punctuation: {line:?} of {lines:?}"
        );
    }
}

#[test]
fn a_hyphenated_word_breaks_after_its_hyphen_and_not_before_it() {
    // A hyphenated word too long for any line: it has to be broken somewhere,
    // and the hyphen is the only place allowed.
    let left = "a".repeat(100);
    let right = "b".repeat(100);
    let lines = lines(&format!("{left}-{right}"));
    assert!(lines.len() > 1, "the text never wrapped, so the rule was not tried");
    assert!(
        lines.iter().any(|line| line.trim_end().ends_with('-')),
        "the word was not broken at its hyphen: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.trim_start().starts_with('-')),
        "a line was left beginning with the hyphen: {lines:?}"
    );
}

#[test]
fn a_non_breaking_space_holds_two_words_together() {
    // Two words joined by a non-breaking space, and together too long for any
    // line: they still may not be parted, so the line runs on instead.
    let text = format!("{}\u{00A0}{}", "a".repeat(80), "b".repeat(80));
    let lines = lines(&text);
    assert!(
        !lines.iter().any(|line| line.trim_start().starts_with('b')),
        "the pair was broken at the non-breaking space: {lines:?}"
    );
}

#[test]
fn a_full_stop_in_a_run_of_its_own_stays_with_its_sentence() {
    // A change of formatting leaves the full stop in a run of its own. It is
    // still a full stop, and no line may begin with it — here the word before
    // it fills a line on its own, so the stop has nowhere to go but with it.
    let bold = RunProperties { bold: Some(true), ..RunProperties::default() };
    let runs = vec![
        Run::text(&format!("{}{}", filler(5), "a".repeat(150))),
        Run {
            properties: bold,
            content: Run::text(".").content,
            field: None,
            revision: None,
            format_change: None,
        },
        Run::text(" and more text after it"),
    ];

    let lines = lines_of(runs);
    assert!(lines.len() > 1, "the text never wrapped, so the rule was not tried");
    assert!(
        !lines.iter().any(|line| line.trim_start().starts_with('.')),
        "the full stop was left to begin a line: {lines:?}"
    );
}

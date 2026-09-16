//! Everything two documents can differ by besides their words: formatting,
//! moves, and what a comparison is told to take notice of.

use wp_docx::compare::Options;
use wp_docx::model::{Alignment, Block, Body, Paragraph};
use wp_docx::revisions::ChangeKind;
use wp_docx::revisions::Decision;
use wp_docx::Document;

/// A document of the paragraphs given.
fn document(paragraphs: &[&str]) -> Document {
    let mut body = Body::default();
    for text in paragraphs {
        body.blocks.push(Block::Paragraph(Paragraph::text(text)));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// The XML of a document, for asking what was actually written.
fn xml(document: &Document) -> String {
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    package.xml_part("word/document.xml").expect("the document").expect("readable")
}

/// Compares and says how many changes were marked.
fn compare(original: &mut Document, revised: &Document, options: Options) -> usize {
    original.compare_with_options(revised, "Compare", options)
}

#[test]
fn a_paragraph_nobody_retyped_and_somebody_restyled_is_a_change() {
    let mut original = document(&["The same words"]);
    let mut revised = document(&["The same words"]);
    revised.set_paragraph_style(0, Some("Heading1"));

    let marked = compare(&mut original, &revised, Options::default());
    assert!(marked > 0, "a re-styled paragraph was not seen to have changed");

    let original = round_trip(&original);
    assert!(xml(&original).contains("w:pPrChange"), "{}", xml(&original));
    assert_eq!(original.style_here().as_deref(), Some("Heading1"), "the style did not arrive");
    let changes = original.changes();
    assert!(changes.iter().any(|change| change.kind == ChangeKind::Formatting), "{changes:?}");
}

#[test]
fn formatting_is_left_alone_when_it_is_not_being_compared() {
    let mut original = document(&["The same words"]);
    let mut revised = document(&["The same words"]);
    revised.set_paragraph_style(0, Some("Heading1"));

    let marked =
        compare(&mut original, &revised, Options { formatting: false, ..Options::default() });
    assert_eq!(marked, 0, "formatting was compared when it was not asked for");
    assert!(!xml(&original).contains("pPrChange"));
}

#[test]
fn rejecting_a_formatting_change_puts_the_old_formatting_back() {
    let mut original = document(&["The same words"]);
    let mut revised = document(&["The same words"]);
    revised.set_paragraph_style(0, Some("Heading1"));
    compare(&mut original, &revised, Options::default());

    let mut original = round_trip(&original);
    original.resolve_all_revisions(Decision::Reject);
    let original = round_trip(&original);
    assert_eq!(original.style_here(), None, "the style stayed after the change was rejected");
    assert!(!xml(&original).contains("pPrChange"), "the record stayed behind");
}

#[test]
fn accepting_one_leaves_the_formatting_and_takes_the_record_away() {
    let mut original = document(&["The same words"]);
    let mut revised = document(&["The same words"]);
    revised.set_paragraph_style(0, Some("Heading1"));
    compare(&mut original, &revised, Options::default());

    let mut original = round_trip(&original);
    original.resolve_all_revisions(Decision::Accept);
    let original = round_trip(&original);
    assert_eq!(original.style_here().as_deref(), Some("Heading1"));
    assert!(!xml(&original).contains("pPrChange"));
}

#[test]
fn a_paragraph_carried_somewhere_else_is_one_move_and_not_two_changes() {
    let mut original = document(&["First", "The travelling paragraph", "Second", "Third"]);
    let revised = document(&["First", "Second", "Third", "The travelling paragraph"]);

    compare(&mut original, &revised, Options::default());
    let written = xml(&original);
    assert!(written.contains("w:moveFrom"), "{written}");
    assert!(written.contains("w:moveTo"), "{written}");
    assert!(written.contains("w:moveFromRangeStart"), "the marks naming the move are missing");
    assert!(written.contains("w:moveToRangeStart"));

    let original = round_trip(&original);
    let changes = original.changes();
    assert!(changes.iter().any(|change| change.kind == ChangeKind::MovedFrom), "{changes:?}");
    assert!(changes.iter().any(|change| change.kind == ChangeKind::MovedTo), "{changes:?}");
}

#[test]
fn a_move_not_asked_for_is_a_deletion_and_an_insertion() {
    let mut original = document(&["First", "The travelling paragraph", "Second"]);
    let revised = document(&["First", "Second", "The travelling paragraph"]);

    compare(&mut original, &revised, Options { moves: false, ..Options::default() });
    let written = xml(&original);
    assert!(!written.contains("moveFrom"), "a move was marked when moves were not asked for");
    assert!(written.contains("w:del"), "{written}");
    assert!(written.contains("w:ins"));
}

#[test]
fn accepting_a_move_keeps_where_it_went_and_drops_where_it_was() {
    let mut original = document(&["First", "The travelling paragraph", "Second"]);
    let revised = document(&["First", "Second", "The travelling paragraph"]);
    compare(&mut original, &revised, Options::default());

    let mut original = round_trip(&original);
    original.resolve_all_revisions(Decision::Accept);
    let original = round_trip(&original);

    let text = original.plain_text();
    assert_eq!(text.matches("The travelling paragraph").count(), 1, "{text}");
    let last = original.paragraph_count() - 1;
    assert_eq!(
        original.paragraph_text(last).unwrap_or_default().trim(),
        "The travelling paragraph",
        "it did not end up where it went"
    );
    assert!(!xml(&original).contains("moveFrom"), "the marks stayed behind");
    assert!(!xml(&original).contains("moveToRangeStart"));
}

#[test]
fn rejecting_a_move_puts_it_back_where_it_was() {
    let mut original = document(&["First", "The travelling paragraph", "Second"]);
    let revised = document(&["First", "Second", "The travelling paragraph"]);
    compare(&mut original, &revised, Options::default());

    let mut original = round_trip(&original);
    original.resolve_all_revisions(Decision::Reject);
    let original = round_trip(&original);

    let text = original.plain_text();
    assert_eq!(text.matches("The travelling paragraph").count(), 1, "{text}");
    assert_eq!(
        original.paragraph_text(1).unwrap_or_default().trim(),
        "The travelling paragraph",
        "it did not go back where it came from"
    );
}

#[test]
fn a_change_of_case_is_a_change_or_is_not_as_asked() {
    let mut told = document(&["The Cat Sat"]);
    let revised = document(&["the cat sat"]);
    assert!(compare(&mut told, &revised, Options::default()) > 0, "a case change was missed");

    let mut ignored = document(&["The Cat Sat"]);
    assert_eq!(
        compare(&mut ignored, &revised, Options { case: false, ..Options::default() }),
        0,
        "a case change was marked when case was not being compared"
    );
    assert_eq!(ignored.plain_text(), "The Cat Sat", "the words changed anyway");
}

#[test]
fn a_change_of_spacing_is_a_change_or_is_not_as_asked() {
    let mut told = document(&["The cat  sat"]);
    let revised = document(&["The cat sat"]);
    assert!(compare(&mut told, &revised, Options::default()) > 0, "a spacing change was missed");

    let mut ignored = document(&["The cat  sat"]);
    assert_eq!(
        compare(&mut ignored, &revised, Options { white_space: false, ..Options::default() }),
        0,
        "spacing was compared when it was not asked for"
    );
}

#[test]
fn the_words_still_win_when_everything_is_being_compared() {
    // The options add to what is noticed; they must not take the words away.
    let mut original = document(&["The cat sat"]);
    let revised = document(&["The cat sat down"]);
    assert!(compare(&mut original, &revised, Options::default()) > 0);
    let original = round_trip(&original);
    assert!(original.plain_text().contains("down"), "{}", original.plain_text());
    assert!(original.changes().iter().any(|change| change.kind == ChangeKind::Insertion));
}

#[test]
fn the_key_a_comparison_uses_says_what_is_being_ignored() {
    let everything = Options::default();
    assert_eq!(everything.key("The  Cat "), "The  Cat ");

    let loose = Options { case: false, white_space: false, ..Options::default() };
    assert_eq!(loose.key("The  Cat "), "the cat");
    assert_eq!(loose.key("the cat"), "the cat", "two that differ only so are one");
}

#[test]
fn a_moved_paragraph_that_was_also_edited_is_not_called_a_move() {
    // A move is text that arrived somewhere else unchanged. Anything else is
    // an edit, and calling it a move would be saying nothing about the edit.
    let mut original = document(&["First", "The travelling paragraph", "Second"]);
    let revised = document(&["First", "Second", "The travelling paragraph, rewritten"]);

    compare(&mut original, &revised, Options::default());
    assert!(!xml(&original).contains("moveFrom"), "an edited paragraph was called a move");
}

#[test]
fn an_empty_paragraph_moving_is_not_a_move() {
    let mut original = document(&["First", "", "Second"]);
    let revised = document(&["First", "Second", ""]);
    compare(&mut original, &revised, Options::default());
    assert!(!xml(&original).contains("moveFrom"), "a blank line was called a move");
}

#[test]
fn the_paragraph_properties_that_changed_are_the_ones_that_arrive() {
    // Not only the style: everything the paragraph itself says.
    let mut original = document(&["Words"]);
    let mut revised = document(&["Words"]);
    revised.set_paragraph_alignment(0, Alignment::Center);

    assert!(compare(&mut original, &revised, Options::default()) > 0);
    let original = round_trip(&original);
    assert_eq!(original.alignment_here(), Alignment::Center);
    assert!(xml(&original).contains("w:pPrChange"));
}

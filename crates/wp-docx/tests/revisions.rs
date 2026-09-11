//! Tracked changes: recording, accepting and rejecting them.

use wp_docx::model::{Block, Body, Paragraph, RevisionKind};
use wp_docx::revisions::{Decision, Reviser};
use wp_docx::{Document, TextPosition};

fn document(text: &str) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text(text)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document.set_reviser(Reviser {
        author: "Ada Lovelace".to_owned(),
        date: "2026-09-07T23:00:00Z".to_owned(),
    });
    document
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Every run of the first paragraph, with the change it belongs to.
fn runs(document: &Document) -> Vec<(String, Option<RevisionKind>)> {
    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!("a paragraph") };
    paragraph
        .runs
        .iter()
        .map(|run| {
            let text: String = run
                .content
                .iter()
                .filter_map(|piece| match piece {
                    wp_docx::model::RunContent::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            (text, run.revision.as_ref().map(|change| change.kind))
        })
        .collect()
}

#[test]
fn a_new_document_is_not_tracking_changes() {
    assert!(!document("text").tracking_changes());
    assert_eq!(document("text").revision_count(), 0);
}

#[test]
fn tracking_can_be_switched_on_and_survives_a_save() {
    let mut document = document("text");
    assert!(document.set_tracking_changes(true));
    assert!(document.tracking_changes());
    assert!(round_trip(&document).tracking_changes());
}

#[test]
fn switching_it_on_twice_changes_nothing_the_second_time() {
    let mut document = document("text");
    assert!(document.set_tracking_changes(true));
    assert!(!document.set_tracking_changes(true));
}

#[test]
fn typing_with_tracking_on_records_an_insertion() {
    let mut document = document("one three");
    document.set_tracking_changes(true);
    document.set_caret(TextPosition::new(0, 4));
    assert!(document.type_text("two "));

    let reopened = round_trip(&document);
    assert_eq!(reopened.revision_count(), 1);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("one two three"));

    let inserted: Vec<String> = runs(&reopened)
        .into_iter()
        .filter(|(_, kind)| *kind == Some(RevisionKind::Inserted))
        .map(|(text, _)| text)
        .collect();
    assert_eq!(inserted, ["two "]);
}

#[test]
fn an_insertion_carries_who_made_it_and_when() {
    let mut document = document("ab");
    document.set_tracking_changes(true);
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("X");

    let Block::Paragraph(paragraph) = &round_trip(&document).body().blocks[0] else {
        panic!("a paragraph")
    };
    let change =
        paragraph.runs.iter().find_map(|run| run.revision.clone()).expect("a recorded change");
    assert_eq!(change.author, "Ada Lovelace");
    assert_eq!(change.date, "2026-09-07T23:00:00Z");
}

#[test]
fn deleting_with_tracking_on_keeps_the_words_and_marks_them() {
    let mut document = document("one two three");
    document.set_tracking_changes(true);
    document.move_caret(TextPosition::new(0, 4), false);
    document.move_caret(TextPosition::new(0, 8), true);
    assert!(document.delete_selection());

    let reopened = round_trip(&document);
    // The document now reads without the words...
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("one three"));
    assert_eq!(reopened.plain_text(), "one three");
    // ...but they are still in the file, marked as deleted.
    assert_eq!(reopened.revision_count(), 1);
    let deleted: Vec<String> = runs(&reopened)
        .into_iter()
        .filter(|(_, kind)| *kind == Some(RevisionKind::Deleted))
        .map(|(text, _)| text)
        .collect();
    assert_eq!(deleted, ["two "]);
}

#[test]
fn accepting_an_insertion_keeps_the_text_and_drops_the_mark() {
    let mut document = document("one three");
    document.set_tracking_changes(true);
    document.set_caret(TextPosition::new(0, 4));
    document.type_text("two ");

    assert_eq!(document.resolve_all_revisions(Decision::Accept), 1);
    let reopened = round_trip(&document);
    assert_eq!(reopened.revision_count(), 0);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("one two three"));
}

#[test]
fn rejecting_an_insertion_takes_the_text_away() {
    let mut document = document("one three");
    document.set_tracking_changes(true);
    document.set_caret(TextPosition::new(0, 4));
    document.type_text("two ");

    assert_eq!(document.resolve_all_revisions(Decision::Reject), 1);
    let reopened = round_trip(&document);
    assert_eq!(reopened.revision_count(), 0);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("one three"));
}

#[test]
fn accepting_a_deletion_takes_the_text_away_for_good() {
    let mut document = document("one two three");
    document.set_tracking_changes(true);
    document.move_caret(TextPosition::new(0, 4), false);
    document.move_caret(TextPosition::new(0, 8), true);
    document.delete_selection();

    assert_eq!(document.resolve_all_revisions(Decision::Accept), 1);
    let reopened = round_trip(&document);
    assert_eq!(reopened.revision_count(), 0);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("one three"));
}

#[test]
fn rejecting_a_deletion_brings_the_text_back() {
    let mut document = document("one two three");
    document.set_tracking_changes(true);
    document.move_caret(TextPosition::new(0, 4), false);
    document.move_caret(TextPosition::new(0, 8), true);
    document.delete_selection();

    assert_eq!(document.resolve_all_revisions(Decision::Reject), 1);
    let reopened = round_trip(&document);
    assert_eq!(reopened.revision_count(), 0);
    // The words are back, and back as ordinary text rather than as delText.
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("one two three"));
    assert!(runs(&reopened).iter().all(|(_, kind)| kind.is_none()));
}

#[test]
fn several_changes_are_each_given_their_own_number() {
    let mut document = document("aaa bbb");
    document.set_tracking_changes(true);
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("X");
    document.set_caret(TextPosition::new(0, 8));
    document.type_text("Y");

    assert_eq!(round_trip(&document).revision_count(), 2);
}

#[test]
fn typing_with_tracking_off_records_nothing() {
    let mut document = document("one three");
    document.set_caret(TextPosition::new(0, 4));
    document.type_text("two ");
    assert_eq!(round_trip(&document).revision_count(), 0);
    assert_eq!(round_trip(&document).paragraph_text(0).as_deref(), Some("one two three"));
}

#[test]
fn a_recorded_change_can_be_undone_like_any_other_edit() {
    let mut document = document("one three");
    document.set_tracking_changes(true);
    document.set_caret(TextPosition::new(0, 4));
    document.type_text("two ");
    assert!(document.undo());
    assert_eq!(document.revision_count(), 0);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("one three"));
}

#[test]
fn deciding_on_nothing_at_all_reports_nothing() {
    let mut document = document("plain");
    assert_eq!(document.resolve_all_revisions(Decision::Accept), 0);
}

#[test]
fn the_caret_still_points_at_something_after_a_rejection() {
    let mut document = document("one three");
    document.set_tracking_changes(true);
    document.set_caret(TextPosition::new(0, 4));
    document.type_text("two ");
    document.resolve_all_revisions(Decision::Reject);

    let caret = document.caret();
    let length = document.paragraph_text(caret.paragraph).map_or(0, |text| text.len());
    assert!(caret.offset <= length, "the caret is past the end of its paragraph");
}

// --- Formatting changes -----------------------------------------------------

/// The formatting changes recorded on the first paragraph's runs, as the author
/// of each.
fn format_changes(document: &Document) -> Vec<String> {
    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!("a paragraph") };
    paragraph
        .runs
        .iter()
        .filter_map(|run| run.format_change.as_ref())
        .map(|change| change.author.clone())
        .collect()
}

/// Makes the first word bold with changes being recorded.
fn embolden(document: &mut Document) {
    document.set_caret(TextPosition::new(0, 0));
    document.move_caret(TextPosition::new(0, 3), true);
    assert!(document.set_format(wp_docx::CharacterFormat::Bold, true));
}

#[test]
fn formatting_while_changes_are_tracked_is_recorded_as_one() {
    let mut document = document("one two");
    document.set_tracking_changes(true);
    embolden(&mut document);

    assert_eq!(format_changes(&document), vec!["Ada Lovelace".to_owned()]);
    // And it survives the file, which is the whole point of writing it down.
    assert_eq!(format_changes(&round_trip(&document)), vec!["Ada Lovelace".to_owned()]);
}

#[test]
fn formatting_with_changes_untracked_is_recorded_as_nothing() {
    let mut document = document("one two");
    embolden(&mut document);
    assert!(format_changes(&document).is_empty());
}

#[test]
fn accepting_a_formatting_change_keeps_the_formatting() {
    let mut document = document("one two");
    document.set_tracking_changes(true);
    embolden(&mut document);

    assert_eq!(document.resolve_all_revisions(Decision::Accept), 1);
    assert!(format_changes(&document).is_empty(), "the record is still there");

    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!("a paragraph") };
    assert_eq!(paragraph.runs[0].properties.bold, Some(true), "the bold went with it");
}

#[test]
fn rejecting_one_puts_the_formatting_back_as_it_was() {
    let mut document = document("one two");
    document.set_tracking_changes(true);
    embolden(&mut document);

    assert_eq!(document.resolve_all_revisions(Decision::Reject), 1);
    assert!(format_changes(&document).is_empty());

    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!("a paragraph") };
    assert_ne!(paragraph.runs[0].properties.bold, Some(true), "the bold stayed");
}

#[test]
fn formatting_the_same_run_twice_keeps_what_it_first_said() {
    // The record is of what nobody has touched, not of the last state before
    // the last keystroke: two presses of Bold and Italic are one change as far
    // as anybody reviewing the document is concerned.
    let mut document = document("one two");
    document.set_tracking_changes(true);
    embolden(&mut document);
    document.set_caret(TextPosition::new(0, 0));
    document.move_caret(TextPosition::new(0, 3), true);
    document.set_format(wp_docx::CharacterFormat::Italic, true);

    assert_eq!(format_changes(&document).len(), 1);
    document.resolve_all_revisions(Decision::Reject);
    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!("a paragraph") };
    assert_ne!(paragraph.runs[0].properties.bold, Some(true));
    assert_ne!(paragraph.runs[0].properties.italic, Some(true));
}

#[test]
fn a_formatting_change_counts_as_a_change() {
    let mut document = document("one two");
    document.set_tracking_changes(true);
    embolden(&mut document);
    assert_eq!(document.revision_count(), 1);
}

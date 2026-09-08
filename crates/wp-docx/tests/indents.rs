//! Setting all three indents at once, as dragging a ruler marker does.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};

fn document(lines: &[&str]) -> Document {
    let mut body = Body::default();
    for line in lines {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_paragraph_starts_with_no_indents() {
    assert_eq!(document(&["plain"]).indents_here(), (0, 0, 0));
}

#[test]
fn all_three_survive_being_written_and_read_back() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.set_indents_here(720, 360, 1440));
    assert_eq!(round_trip(&document).indents_here(), (720, 360, 1440));
}

#[test]
fn a_hanging_first_line_is_a_negative_offset() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    document.set_indents_here(720, -360, 0);
    assert_eq!(round_trip(&document).indents_here(), (720, -360, 0));
}

#[test]
fn a_hanging_indent_is_written_as_word_writes_one() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    document.set_indents_here(720, -360, 0);

    let reopened = round_trip(&document);
    let part =
        reopened.package().xml_part("word/document.xml").expect("the document").expect("readable");
    assert!(part.contains(r#"w:hanging="360""#), "got {part}");
    assert!(!part.contains("firstLine"), "a hanging indent is not a first-line one");
}

#[test]
fn nothing_is_indented_off_the_left_of_the_paper() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    document.set_indents_here(-500, 0, -500);
    assert_eq!(round_trip(&document).indents_here(), (0, 0, 0));
}

#[test]
fn the_first_line_may_hang_no_further_out_than_the_paper_edge() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    document.set_indents_here(360, -720, 0);
    // Hanging further than the indent would put the first line off the page.
    assert_eq!(round_trip(&document).indents_here(), (360, -360, 0));
}

#[test]
fn setting_the_same_indents_twice_changes_nothing() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.set_indents_here(720, 0, 0));
    assert!(!document.set_indents_here(720, 0, 0));
}

#[test]
fn every_paragraph_the_selection_touches_is_indented() {
    let mut document = document(&["one", "two", "three"]);
    document.move_caret(TextPosition::new(0, 0), false);
    document.move_caret(TextPosition::new(1, 3), true);
    document.set_indents_here(720, 0, 0);

    let reopened = round_trip(&document);
    for paragraph in 0..2 {
        let mut probe = round_trip(&reopened);
        probe.set_caret(TextPosition::new(paragraph, 0));
        assert_eq!(probe.indents_here().0, 720, "paragraph {paragraph}");
    }
    let mut probe = round_trip(&reopened);
    probe.set_caret(TextPosition::new(2, 0));
    assert_eq!(probe.indents_here().0, 0, "the third was not selected");
}

#[test]
fn indenting_can_be_undone() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    document.set_indents_here(720, 360, 0);
    assert!(document.undo());
    assert_eq!(document.indents_here(), (0, 0, 0));
}

#[test]
fn indenting_changes_not_one_character_of_the_text() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    document.set_indents_here(720, 360, 720);
    assert_eq!(round_trip(&document).plain_text(), "plain");
}

// --- Gestures -----------------------------------------------------------------

#[test]
fn a_whole_gesture_comes_back_in_one_undo() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));

    // What a drag across an inch of ruler does: many small changes in a row.
    document.begin_gesture();
    for step in 1..=8 {
        document.set_indents_here(step * 180, 0, 0);
    }
    document.end_gesture();
    assert_eq!(document.indents_here().0, 1440);

    assert!(document.undo());
    assert_eq!(document.indents_here(), (0, 0, 0), "one undo should undo the whole drag");
}

#[test]
fn changes_after_a_gesture_are_their_own_steps_again() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));

    document.begin_gesture();
    document.set_indents_here(360, 0, 0);
    document.set_indents_here(720, 0, 0);
    document.end_gesture();
    document.set_indents_here(1440, 0, 0);

    assert!(document.undo());
    assert_eq!(document.indents_here().0, 720, "the step after the drag is its own");
    assert!(document.undo());
    assert_eq!(document.indents_here().0, 0);
}

#[test]
fn a_gesture_that_changes_nothing_leaves_the_history_alone() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    let before = document.undo_depth();

    document.begin_gesture();
    document.set_indents_here(0, 0, 0);
    document.end_gesture();
    assert_eq!(document.undo_depth(), before);
}

//! Moving and deleting by the word.

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

fn at(document: &Document) -> (usize, usize) {
    let caret = document.caret();
    (caret.paragraph, caret.offset)
}

// --- Moving -------------------------------------------------------------------

#[test]
fn right_steps_from_word_to_word() {
    let mut document = document(&["one two three"]);
    document.set_caret(TextPosition::new(0, 0));

    document.word_right(false);
    assert_eq!(at(&document), (0, 4));
    document.word_right(false);
    assert_eq!(at(&document), (0, 8));
}

#[test]
fn left_steps_back_from_word_to_word() {
    let mut document = document(&["one two three"]);
    document.set_caret(TextPosition::new(0, 13));

    document.word_left(false);
    assert_eq!(at(&document), (0, 8));
    document.word_left(false);
    assert_eq!(at(&document), (0, 4));
    document.word_left(false);
    assert_eq!(at(&document), (0, 0));
}

#[test]
fn right_at_the_end_of_a_paragraph_goes_to_the_next_one() {
    let mut document = document(&["one", "two"]);
    document.set_caret(TextPosition::new(0, 3));
    document.word_right(false);
    assert_eq!(at(&document), (1, 0));
}

#[test]
fn left_at_the_start_of_a_paragraph_goes_to_the_end_of_the_one_before() {
    let mut document = document(&["one", "two"]);
    document.set_caret(TextPosition::new(1, 0));
    document.word_left(false);
    assert_eq!(at(&document), (0, 3));
}

#[test]
fn neither_runs_off_the_ends_of_the_document() {
    let mut document = document(&["one"]);
    document.set_caret(TextPosition::new(0, 0));
    document.word_left(false);
    assert_eq!(at(&document), (0, 0));

    document.set_caret(TextPosition::new(0, 3));
    document.word_right(false);
    assert_eq!(at(&document), (0, 3));
}

#[test]
fn holding_shift_selects_a_word_at_a_time() {
    let mut document = document(&["one two three"]);
    document.set_caret(TextPosition::new(0, 0));
    document.word_right(true);
    assert_eq!(document.selected_text(), "one ");
    document.word_right(true);
    assert_eq!(document.selected_text(), "one two ");
}

#[test]
fn moving_without_shift_drops_the_selection() {
    let mut document = document(&["one two three"]);
    document.set_caret(TextPosition::new(0, 0));
    document.word_right(true);
    document.word_right(false);
    assert!(document.selection().is_none());
}

#[test]
fn it_works_in_a_document_written_in_russian() {
    let mut document = document(&["одно два три"]);
    document.set_caret(TextPosition::new(0, 0));
    document.word_right(false);
    // "одно" is eight bytes and the space is one.
    assert_eq!(at(&document), (0, 9));
    document.word_left(false);
    assert_eq!(at(&document), (0, 0));
}

// --- Moving by paragraph ------------------------------------------------------

#[test]
fn up_goes_to_the_start_of_the_paragraph_then_to_the_one_above() {
    let mut document = document(&["first line", "second line"]);
    document.set_caret(TextPosition::new(1, 6));

    document.paragraph_up(false);
    assert_eq!(at(&document), (1, 0));
    document.paragraph_up(false);
    assert_eq!(at(&document), (0, 0));
    document.paragraph_up(false);
    assert_eq!(at(&document), (0, 0), "it stops at the top");
}

#[test]
fn down_goes_to_the_start_of_the_next_paragraph() {
    let mut document = document(&["first line", "second line"]);
    document.set_caret(TextPosition::new(0, 3));
    document.paragraph_down(false);
    assert_eq!(at(&document), (1, 0));
}

#[test]
fn down_in_the_last_paragraph_goes_to_the_end_of_it() {
    let mut document = document(&["only line"]);
    document.set_caret(TextPosition::new(0, 3));
    document.paragraph_down(false);
    assert_eq!(at(&document), (0, 9));
    document.paragraph_down(false);
    assert_eq!(at(&document), (0, 9), "it stops at the bottom");
}

// --- Deleting -----------------------------------------------------------------

#[test]
fn deleting_back_takes_the_word_before_the_caret() {
    let mut document = document(&["one two three"]);
    document.set_caret(TextPosition::new(0, 13));
    assert!(document.delete_word_back());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("one two "));
    assert_eq!(at(&document), (0, 8));
}

#[test]
fn deleting_back_from_the_middle_of_a_word_takes_only_its_beginning() {
    let mut document = document(&["one two"]);
    document.set_caret(TextPosition::new(0, 6));
    document.delete_word_back();
    assert_eq!(document.paragraph_text(0).as_deref(), Some("one o"));
}

#[test]
fn deleting_back_at_the_start_of_a_paragraph_joins_it_to_the_one_above() {
    let mut document = document(&["one", "two"]);
    document.set_caret(TextPosition::new(1, 0));
    assert!(document.delete_word_back());
    assert_eq!(document.paragraph_count(), 1);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("onetwo"));
}

#[test]
fn deleting_back_at_the_very_start_of_the_document_does_nothing() {
    let mut document = document(&["one"]);
    document.set_caret(TextPosition::new(0, 0));
    assert!(!document.delete_word_back());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("one"));
}

#[test]
fn deleting_forward_takes_the_word_after_the_caret() {
    let mut document = document(&["one two three"]);
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.delete_word_forward());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("two three"));
}

#[test]
fn deleting_forward_at_the_end_of_a_paragraph_pulls_up_the_next() {
    let mut document = document(&["one", "two"]);
    document.set_caret(TextPosition::new(0, 3));
    assert!(document.delete_word_forward());
    assert_eq!(document.paragraph_count(), 1);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("onetwo"));
}

#[test]
fn deleting_a_word_takes_the_selection_when_there_is_one() {
    let mut document = document(&["one two three"]);
    document.move_caret(TextPosition::new(0, 0), false);
    document.move_caret(TextPosition::new(0, 7), true);
    assert!(document.delete_word_back());
    assert_eq!(document.paragraph_text(0).as_deref(), Some(" three"));
}

#[test]
fn each_word_deleted_is_its_own_undo_step() {
    let mut document = document(&["one two three"]);
    document.set_caret(TextPosition::new(0, 13));
    document.delete_word_back();
    document.delete_word_back();
    assert_eq!(document.paragraph_text(0).as_deref(), Some("one "));

    assert!(document.undo());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("one two "), "one undo, one word");
    assert!(document.undo());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("one two three"));
}

#[test]
fn deleting_a_word_can_be_redone() {
    let mut document = document(&["one two"]);
    document.set_caret(TextPosition::new(0, 7));
    document.delete_word_back();
    document.undo();
    assert!(document.redo());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("one "));
}

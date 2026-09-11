//! A selection made of more than one stretch.
//!
//! Word lets a person hold Ctrl and drag out another stretch without losing the
//! ones already chosen. The point of it is that every command that works on a
//! selection then works on all of them at once — which is what these check.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{CharacterFormat, Document, TextPosition};

fn document(lines: &[&str]) -> Document {
    let mut body = Body::default();
    for line in lines {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Selects a stretch of one paragraph, keeping whatever was selected before.
fn also_select(document: &mut Document, paragraph: usize, from: usize, to: usize) {
    document.add_selection(TextPosition::new(paragraph, from), TextPosition::new(paragraph, to));
}

/// Selects the first stretch, dropping anything selected before it.
fn select(document: &mut Document, paragraph: usize, from: usize, to: usize) {
    document.set_caret(TextPosition::new(paragraph, from));
    document.extend_selection_to(TextPosition::new(paragraph, to));
}

/// Whether every run covering a range is bold.
fn is_bold(document: &Document, paragraph: usize, from: usize, to: usize) -> bool {
    let mut probe = document.clone();
    select(&mut probe, paragraph, from, to);
    probe.format_is_on(CharacterFormat::Bold)
}

#[test]
fn one_stretch_is_what_it_has_always_been() {
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 4, 7);
    assert_eq!(document.selections().len(), 1);
    assert_eq!(document.selected_text(), "two");
}

#[test]
fn several_stretches_are_all_of_them_in_order() {
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 0, 3);
    also_select(&mut document, 0, 8, 13);

    let stretches = document.selections();
    assert_eq!(stretches.len(), 2);
    assert_eq!(stretches[0].0.offset, 0, "they came back out of order");
    assert_eq!(document.selected_text(), "one\nthree");
}

#[test]
fn two_stretches_that_overlap_are_one() {
    // A person who dragged over the same words twice selected them once, and a
    // command that ran over them twice would do everything twice.
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 4, 13);
    also_select(&mut document, 0, 0, 7);

    assert_eq!(document.selections(), vec![(TextPosition::new(0, 0), TextPosition::new(0, 13))]);
}

#[test]
fn an_empty_stretch_is_not_a_stretch() {
    let mut document = document(&["one two three"]);
    also_select(&mut document, 0, 4, 4);
    assert!(document.selections().is_empty());
    assert!(document.selection().is_none());
}

#[test]
fn formatting_reaches_every_stretch() {
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 0, 3);
    also_select(&mut document, 0, 8, 13);

    assert!(document.set_format(CharacterFormat::Bold, true));
    assert!(is_bold(&document, 0, 0, 3), "the first stretch was left alone");
    assert!(is_bold(&document, 0, 8, 13), "the second stretch was left alone");
    assert!(!is_bold(&document, 0, 4, 7), "the gap between them was bolded too");
}

#[test]
fn one_undo_takes_back_what_one_press_put_on() {
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 0, 3);
    also_select(&mut document, 0, 8, 13);
    document.set_format(CharacterFormat::Bold, true);

    assert!(document.undo());
    assert!(!is_bold(&document, 0, 0, 3));
    assert!(!is_bold(&document, 0, 8, 13));
}

#[test]
fn a_format_already_on_throughout_reads_as_on() {
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 0, 3);
    also_select(&mut document, 0, 8, 13);
    document.set_format(CharacterFormat::Bold, true);

    // Both stretches are bold, so the button is lit; the gap is not part of
    // the selection and must not be consulted.
    select(&mut document, 0, 0, 3);
    also_select(&mut document, 0, 8, 13);
    assert!(document.format_is_on(CharacterFormat::Bold));
}

#[test]
fn one_stretch_of_several_not_formatted_reads_as_off() {
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 0, 3);
    document.set_format(CharacterFormat::Bold, true);

    select(&mut document, 0, 8, 13);
    also_select(&mut document, 0, 0, 3);
    assert!(!document.format_is_on(CharacterFormat::Bold), "half bold read as bold");
}

#[test]
fn deleting_takes_every_stretch_and_leaves_the_gaps() {
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 0, 3);
    also_select(&mut document, 0, 8, 13);

    assert!(document.delete_selection());
    assert_eq!(document.paragraph_text(0).as_deref(), Some(" two "));
    assert!(document.selections().is_empty(), "something is still selected");
}

#[test]
fn deleting_across_paragraphs_takes_the_later_stretch_first() {
    // Taking the first one out would move everything after it, and the second
    // would be taken from the wrong place.
    let mut document = document(&["first line", "second line"]);
    select(&mut document, 0, 0, 5);
    also_select(&mut document, 1, 0, 6);

    assert!(document.delete_selection());
    assert_eq!(document.paragraph_text(0).as_deref(), Some(" line"));
    assert_eq!(document.paragraph_text(1).as_deref(), Some(" line"));
}

#[test]
fn copying_gives_every_stretch() {
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 0, 3);
    also_select(&mut document, 0, 8, 13);

    let copied = document.copy_selection();
    assert_eq!(copied.len(), 2);
    assert_eq!(document.selected_text(), "one\nthree");
}

#[test]
fn a_paragraph_touched_by_two_stretches_is_changed_once() {
    // Indenting it twice would indent it twice as far.
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 0, 3);
    also_select(&mut document, 0, 8, 13);

    document.adjust_indent_here(720);
    assert_eq!(document.indents_here().0, 720, "the paragraph was indented once for each stretch");
}

#[test]
fn moving_the_caret_drops_every_stretch() {
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 0, 3);
    also_select(&mut document, 0, 8, 13);

    document.set_caret(TextPosition::new(0, 5));
    assert!(document.selections().is_empty());
}

// --- Select All Text With Similar Formatting --------------------------------

#[test]
fn every_stretch_set_the_same_way_is_found() {
    let mut document = document(&["one two three", "two again"]);
    // Bold the two "two"s and nothing else.
    select(&mut document, 0, 4, 7);
    also_select(&mut document, 1, 0, 3);
    document.set_format(CharacterFormat::Bold, true);

    // With the caret in one of them, the other is found too.
    select(&mut document, 0, 4, 7);
    assert_eq!(document.select_similar(), 2);
    assert_eq!(document.selected_text(), "two\ntwo");
}

#[test]
fn what_is_set_another_way_is_left_out() {
    let mut document = document(&["plain bold plain"]);
    select(&mut document, 0, 6, 10);
    document.set_format(CharacterFormat::Bold, true);

    select(&mut document, 0, 6, 10);
    assert_eq!(document.select_similar(), 1);
    assert_eq!(document.selected_text(), "bold");
}

#[test]
fn two_runs_next_to_each_other_that_look_alike_are_one_stretch() {
    // A document's run boundaries are not something anybody put there on
    // purpose, and a selection that stopped at one would look arbitrary.
    let mut document = document(&["one two three"]);
    // Bolding two touching ranges leaves two runs that look the same.
    select(&mut document, 0, 0, 4);
    document.set_format(CharacterFormat::Bold, true);
    select(&mut document, 0, 4, 7);
    document.set_format(CharacterFormat::Bold, true);

    select(&mut document, 0, 0, 3);
    assert_eq!(document.select_similar(), 1);
    assert_eq!(document.selected_text(), "one two");
}

#[test]
fn what_it_finds_can_then_be_formatted_all_at_once() {
    // The whole reason the command exists.
    let mut document = document(&["heading", "body", "heading"]);
    select(&mut document, 0, 0, 7);
    also_select(&mut document, 2, 0, 7);
    document.set_format(CharacterFormat::Italic, true);

    select(&mut document, 0, 0, 7);
    assert_eq!(document.select_similar(), 2);
    document.set_format(CharacterFormat::Bold, true);

    assert!(is_bold(&document, 0, 0, 7));
    assert!(is_bold(&document, 2, 0, 7));
    assert!(!is_bold(&document, 1, 0, 4), "the body was bolded too");
}

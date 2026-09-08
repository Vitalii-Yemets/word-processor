//! Character formatting that is written onto a run rather than toggled.
//!
//! Bold and italic have been covered since the beginning. These four were not,
//! and were silently doing nothing: the writer had no branch for them, so the
//! command reported success and the document was unchanged.

use wp_docx::model::{Block, Body, Paragraph, VerticalAlignment};
use wp_docx::{CharacterFormat, Document, TextPosition};

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

fn select(document: &mut Document, paragraph: usize, from: usize, to: usize) {
    document.move_caret(TextPosition::new(paragraph, from), false);
    document.move_caret(TextPosition::new(paragraph, to), true);
}

/// The properties of the run holding a piece of text, after saving.
fn run_properties(document: &Document, text: &str) -> wp_docx::model::RunProperties {
    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!("a paragraph") };
    paragraph
        .runs
        .iter()
        .find(|run| run.plain_text() == text)
        .map(|run| run.properties.clone())
        .unwrap_or_else(|| panic!("no run says {text:?}"))
}

// --- Highlight ----------------------------------------------------------------

#[test]
fn a_highlight_is_written_and_reads_back() {
    let mut document = document(&["one two"]);
    select(&mut document, 0, 0, 3);
    assert!(document.set_highlight(Some("yellow")));

    let reopened = round_trip(&document);
    assert_eq!(run_properties(&reopened, "one").highlight.as_deref(), Some("yellow"));
}

#[test]
fn only_the_selection_is_highlighted() {
    let mut document = document(&["one two"]);
    select(&mut document, 0, 0, 3);
    document.set_highlight(Some("cyan"));

    let reopened = round_trip(&document);
    assert_eq!(run_properties(&reopened, " two").highlight, None);
}

#[test]
fn a_highlight_can_be_taken_off_again() {
    let mut document = document(&["one two"]);
    select(&mut document, 0, 0, 3);
    document.set_highlight(Some("yellow"));
    select(&mut document, 0, 0, 3);
    assert!(document.set_highlight(None));

    let reopened = round_trip(&document);
    assert_eq!(run_properties(&reopened, "one").highlight, None);
    let part =
        reopened.package().xml_part("word/document.xml").expect("the document").expect("readable");
    assert!(!part.contains("highlight"), "taking it off should leave nothing: {part}");
}

#[test]
fn highlighting_what_is_already_highlighted_changes_nothing() {
    let mut document = document(&["one two"]);
    select(&mut document, 0, 0, 3);
    assert!(document.set_highlight(Some("yellow")));
    select(&mut document, 0, 0, 3);
    assert!(!document.set_highlight(Some("yellow")));
}

// --- Superscript and subscript ------------------------------------------------

#[test]
fn a_superscript_is_written_and_reads_back() {
    let mut document = document(&["x2"]);
    select(&mut document, 0, 1, 2);
    assert!(document.set_format(CharacterFormat::Superscript, true));

    let reopened = round_trip(&document);
    assert_eq!(run_properties(&reopened, "2").vertical_align, Some(VerticalAlignment::Superscript));
}

#[test]
fn a_subscript_is_written_and_reads_back() {
    let mut document = document(&["H2O"]);
    select(&mut document, 0, 1, 2);
    document.set_format(CharacterFormat::Subscript, true);

    let reopened = round_trip(&document);
    assert_eq!(run_properties(&reopened, "2").vertical_align, Some(VerticalAlignment::Subscript));
}

#[test]
fn going_back_to_the_baseline_takes_the_property_away() {
    let mut document = document(&["x2"]);
    select(&mut document, 0, 1, 2);
    document.set_format(CharacterFormat::Superscript, true);
    select(&mut document, 0, 1, 2);
    assert!(document.set_format(CharacterFormat::Superscript, false));

    let reopened = round_trip(&document);
    let part =
        reopened.package().xml_part("word/document.xml").expect("the document").expect("readable");
    assert!(!part.contains("vertAlign"), "the baseline is written by writing nothing: {part}");
}

#[test]
fn raising_text_changes_not_one_character_of_it() {
    let mut document = document(&["x2"]);
    select(&mut document, 0, 1, 2);
    document.set_format(CharacterFormat::Superscript, true);
    assert_eq!(round_trip(&document).plain_text(), "x2");
}

#[test]
fn raising_text_can_be_undone() {
    let mut document = document(&["x2"]);
    select(&mut document, 0, 1, 2);
    document.set_format(CharacterFormat::Superscript, true);
    assert!(document.undo());
    assert_eq!(run_properties(&document, "x2").vertical_align, None);
}

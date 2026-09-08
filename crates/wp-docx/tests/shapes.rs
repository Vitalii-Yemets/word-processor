//! Shapes and text boxes in a document.

use wp_docx::model::{Block, Body, Paragraph, RunContent};
use wp_docx::shapes::Shape;
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
fn a_document_starts_with_no_shapes() {
    assert!(document(&["plain"]).shapes().is_empty());
}

#[test]
fn a_shape_can_be_put_in_and_reads_back_after_saving() {
    let mut document = document(&["one two"]);
    document.set_caret(TextPosition::new(0, 3));
    assert!(document.insert_shape(&Shape::preset("ellipse", 120.0, 60.0)));

    let shapes = round_trip(&document).shapes();
    assert_eq!(shapes.len(), 1);
    assert_eq!(shapes[0].preset, "ellipse");
    assert!((shapes[0].width_points() - 120.0).abs() < 0.1);
    assert!((shapes[0].height_points() - 60.0).abs() < 0.1);
}

#[test]
fn a_shape_takes_one_character_of_the_text() {
    let mut document = document(&["one two"]);
    document.set_caret(TextPosition::new(0, 3));
    document.insert_shape(&Shape::preset("rect", 100.0, 50.0));

    let reopened = round_trip(&document);
    // The shape reads as nothing, the way a picture does — but the caret can
    // stand either side of it, which is what the extra character is for.
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("one\u{1} two"));
}

#[test]
fn the_caret_ends_up_after_the_shape() {
    let mut document = document(&["one two"]);
    document.set_caret(TextPosition::new(0, 3));
    document.insert_shape(&Shape::preset("rect", 100.0, 50.0));
    assert_eq!(document.caret().offset, 4);
}

#[test]
fn a_text_box_keeps_what_is_written_in_it() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_shape(&Shape::text_box(200.0, 80.0, "A note\nin two lines"));

    let shapes = round_trip(&document).shapes();
    assert_eq!(shapes.len(), 1);
    assert!(shapes[0].has_text());
    assert_eq!(shapes[0].body().plain_text(), "A note\nin two lines");
}

#[test]
fn the_words_in_a_text_box_are_not_the_words_of_the_document() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_shape(&Shape::text_box(200.0, 80.0, "hidden words"));

    let reopened = round_trip(&document);
    assert!(
        !reopened.plain_text().contains("hidden words"),
        "a text box is beside the text, not in it: {:?}",
        reopened.plain_text()
    );
}

#[test]
fn a_shape_is_written_as_a_drawing_word_will_recognise() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_shape(&Shape::preset("star5", 100.0, 100.0));

    let reopened = round_trip(&document);
    let part =
        reopened.package().xml_part("word/document.xml").expect("the document").expect("readable");
    assert!(part.contains("wps:wsp"), "no word-processing shape: {part}");
    assert!(part.contains(r#"prst="star5""#), "no geometry: {part}");
}

#[test]
fn several_shapes_are_all_found() {
    let mut document = document(&["one", "two"]);
    document.set_caret(TextPosition::new(0, 3));
    document.insert_shape(&Shape::preset("rect", 50.0, 50.0));
    document.set_caret(TextPosition::new(1, 3));
    document.insert_shape(&Shape::preset("ellipse", 50.0, 50.0));

    assert_eq!(round_trip(&document).shapes().len(), 2);
}

#[test]
fn a_shape_can_be_taken_back_out_with_backspace() {
    let mut document = document(&["one"]);
    document.set_caret(TextPosition::new(0, 3));
    document.insert_shape(&Shape::preset("rect", 50.0, 50.0));
    assert!(document.backspace());

    let reopened = round_trip(&document);
    assert!(reopened.shapes().is_empty());
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("one"));
}

#[test]
fn putting_a_shape_in_can_be_undone() {
    let mut document = document(&["one"]);
    document.set_caret(TextPosition::new(0, 3));
    document.insert_shape(&Shape::preset("rect", 50.0, 50.0));
    assert!(document.undo());
    assert!(document.shapes().is_empty());
}

#[test]
fn a_shape_read_from_a_document_is_a_shape_and_not_a_picture() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_shape(&Shape::preset("rect", 50.0, 50.0));

    let reopened = round_trip(&document);
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else { panic!("a paragraph") };
    let kinds: Vec<&RunContent> = paragraph.runs.iter().flat_map(|run| &run.content).collect();
    assert!(
        kinds.iter().any(|piece| matches!(piece, RunContent::Shape(_))),
        "the drawing was read as something else"
    );
    assert!(!kinds.iter().any(|piece| matches!(piece, RunContent::Picture(_))));
}

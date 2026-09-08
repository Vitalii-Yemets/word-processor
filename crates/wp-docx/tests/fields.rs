//! Fields dropped into the text, which say what the document says about itself.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::properties::Properties;
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
fn a_field_is_written_as_a_field_and_not_as_words() {
    let mut document = document(&["Written by "]);
    document.set_caret(TextPosition::new(0, 11));
    assert!(document.insert_field("AUTHOR", "Ann Roe"));

    let reopened = round_trip(&document);
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else { panic!("a paragraph") };
    let instruction = paragraph.runs.iter().find_map(|run| run.field.as_deref()).expect("a field");
    assert_eq!(instruction, "AUTHOR");
}

#[test]
fn what_the_field_says_now_is_in_the_text() {
    let mut document = document(&["Written by "]);
    document.set_caret(TextPosition::new(0, 11));
    document.insert_field("AUTHOR", "Ann Roe");

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("Written by Ann Roe"));
}

#[test]
fn the_caret_ends_up_after_what_the_field_says() {
    let mut document = document(&["Written by "]);
    document.set_caret(TextPosition::new(0, 11));
    document.insert_field("AUTHOR", "Ann Roe");
    assert_eq!(document.caret().offset, 18);
}

#[test]
fn a_field_can_be_put_in_the_middle_of_a_word() {
    let mut document = document(&["abcdef"]);
    document.set_caret(TextPosition::new(0, 3));
    document.insert_field("TITLE", "X");
    assert_eq!(round_trip(&document).paragraph_text(0).as_deref(), Some("abcXdef"));
}

#[test]
fn putting_a_field_in_can_be_undone() {
    let mut document = document(&["Written by "]);
    document.set_caret(TextPosition::new(0, 11));
    document.insert_field("AUTHOR", "Ann Roe");
    assert!(document.undo());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Written by "));
}

#[test]
fn several_fields_are_each_their_own() {
    let mut document = document(&["", ""]);
    document.set_caret(TextPosition::new(0, 0));
    document.insert_field("TITLE", "A Report");
    document.set_caret(TextPosition::new(1, 0));
    document.insert_field("AUTHOR", "Ann Roe");

    let reopened = round_trip(&document);
    let instructions: Vec<String> = reopened
        .body()
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => paragraph.runs.iter().find_map(|run| run.field.clone()),
            Block::Table(_) => None,
        })
        .collect();
    assert_eq!(instructions, vec!["TITLE".to_owned(), "AUTHOR".to_owned()]);
}

#[test]
fn a_field_and_the_properties_it_names_are_kept_apart() {
    let mut document = document(&[""]);
    document
        .set_properties(&Properties { author: "Ann Roe".to_owned(), ..Properties::default() })
        .expect("writing");
    document.set_caret(TextPosition::new(0, 0));
    document.insert_field("AUTHOR", "Ann Roe");

    // Changing the property does not rewrite the text: the field still says
    // what it last worked out, and something has to work it out again.
    document
        .set_properties(&Properties { author: "John Doe".to_owned(), ..Properties::default() })
        .expect("writing");

    let reopened = round_trip(&document);
    assert_eq!(reopened.properties().author, "John Doe");
    assert_eq!(
        reopened.paragraph_text(0).as_deref(),
        Some("Ann Roe"),
        "the cached answer stays until the field is worked out again"
    );
}

#[test]
fn text_typed_after_a_field_lands_after_it_and_not_inside_it() {
    let mut document = document(&["Page "]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_field("PAGE", "1");
    document.type_text(" of many");

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("Page 1 of many"));

    // The words are beside the field, not in it — so working the field out
    // again replaces the number and leaves the words alone.
    let part =
        reopened.package().xml_part("word/document.xml").expect("the document").expect("readable");
    let field_at = part.find("fldSimple").expect("a field");
    let field_end = part[field_at..].find("</w:fldSimple>").expect("the field ends") + field_at;
    assert!(
        !part[field_at..field_end].contains("of many"),
        "the typed words went inside the field: {part}"
    );
}

#[test]
fn text_typed_before_a_field_lands_before_it() {
    let mut document = document(&["x"]);
    document.set_caret(TextPosition::new(0, 1));
    document.insert_field("PAGE", "1");
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("| ");

    assert_eq!(round_trip(&document).paragraph_text(0).as_deref(), Some("x| 1"));
}

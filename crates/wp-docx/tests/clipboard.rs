//! Copying and pasting inside a document, with the formatting kept.

use wp_docx::model::{Block, Body, Paragraph, Run, RunContent, RunProperties};
use wp_docx::{Document, TextPosition};

/// Two paragraphs, the first of them "plain bold after" with "bold" in bold.
fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        properties: wp_docx::model::ParagraphProperties::default(),
        runs: vec![
            Run::text("plain "),
            Run {
                properties: RunProperties { bold: Some(true), ..RunProperties::default() },
                content: vec![RunContent::Text("bold".to_owned())],
                field: None,
                revision: None,
            },
            Run::text(" after"),
        ],
    }));
    body.blocks.push(Block::Paragraph(Paragraph::text("second")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Selects a stretch of the first paragraph.
fn select(document: &mut Document, from: usize, to: usize) {
    document.set_caret(TextPosition::new(0, from));
    document.extend_selection_to(TextPosition::new(0, to));
}

#[test]
fn nothing_selected_copies_nothing() {
    assert!(document().copy_selection().is_empty());
}

#[test]
fn the_words_of_the_selection_are_what_is_copied() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();
    assert_eq!(copied.len(), 1);
    assert_eq!(copied[0].plain_text(), "plain");
}

#[test]
fn a_selection_over_several_paragraphs_copies_each_of_them() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 6));
    document.extend_selection_to(TextPosition::new(1, 6));
    let copied = document.copy_selection();
    assert_eq!(copied.len(), 2, "{copied:?}");
    assert_eq!(copied[0].plain_text(), "bold after");
    assert_eq!(copied[1].plain_text(), "second");
}

#[test]
fn pasting_puts_the_words_where_the_caret_is() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    assert!(document.paste_blocks(&copied));
    assert_eq!(round_trip(&document).plain_text(), "plain bold after\nsecondplain");
}

#[test]
fn what_was_bold_is_bold_where_it_lands() {
    let mut document = document();
    // Just the bold word.
    select(&mut document, 6, 10);
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    let mut reopened = round_trip(&document);
    // Inside what was pasted, which now ends the second paragraph.
    reopened.set_caret(TextPosition::new(1, 8));
    assert!(
        reopened.format_is_on(wp_docx::CharacterFormat::Bold),
        "the formatting was lost: {:?}",
        reopened.body().blocks[1]
    );
}

#[test]
fn plain_text_stays_plain_where_it_lands() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(1, 8));
    assert!(!reopened.format_is_on(wp_docx::CharacterFormat::Bold));
}

#[test]
fn pasting_several_paragraphs_makes_several_paragraphs() {
    let mut document = document();
    document.select_all();
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_count(), 3, "{}", reopened.plain_text());
}

#[test]
fn pasting_over_a_selection_replaces_it() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();

    // Over the whole of the second paragraph.
    document.set_caret(TextPosition::new(1, 0));
    document.extend_selection_to(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    assert_eq!(round_trip(&document).plain_text(), "plain bold after\nplain");
}

#[test]
fn pasting_nothing_changes_nothing() {
    let mut document = document();
    assert!(!document.paste_blocks(&[]));
}

#[test]
fn a_paste_is_one_thing_to_undo() {
    let mut document = document();
    document.select_all();
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);
    assert!(document.undo());
    assert_eq!(document.plain_text(), "plain bold after\nsecond");
}

#[test]
fn the_style_of_a_paragraph_comes_across_with_it() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        properties: wp_docx::model::ParagraphProperties {
            style: Some("Heading1".to_owned()),
            ..wp_docx::model::ParagraphProperties::default()
        },
        runs: vec![Run::text("A heading")],
    }));
    body.blocks.push(Block::Paragraph(Paragraph::text("body")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    select(&mut document, 0, 9);
    let copied = document.copy_selection();

    let Block::Paragraph(first) = &copied[0] else { panic!("a paragraph") };
    assert_eq!(first.properties.style.as_deref(), Some("Heading1"));
}

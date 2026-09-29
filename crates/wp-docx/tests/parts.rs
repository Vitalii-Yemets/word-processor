//! Editing a part of the package other than the document: a header or a footer.

use wp_docx::furniture::Furniture;
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::notes::Kind;
use wp_docx::{Document, TextPosition};

/// A document with a header on it.
fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("The body of the document")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document
        .set_furniture(
            Furniture::Header,
            wp_docx::furniture::Preset::Text,
            wp_docx::model::Alignment::Start,
            "Title",
        )
        .expect("a header");

    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_document_is_editing_itself_to_begin_with() {
    assert_eq!(document().part_being_edited(), None);
}

#[test]
fn a_header_can_be_entered_and_says_so() {
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    assert!(document.enter_part(&part));
    assert_eq!(document.part_being_edited(), Some(part.as_str()));
}

#[test]
fn inside_a_header_the_text_is_the_headers() {
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    let body = document.plain_text();
    document.enter_part(&part);

    assert_ne!(document.plain_text(), body, "the body is still what is being edited");
    assert!(document.plain_text().contains("Title"), "{}", document.plain_text());
}

#[test]
fn typing_inside_a_header_changes_the_header_and_not_the_body() {
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("New ");
    document.leave_part();

    let reopened = round_trip(&document);
    assert_eq!(reopened.plain_text(), "The body of the document", "the body was changed");
    let header = reopened.furniture(Furniture::Header).expect("a header");
    let text: String = header.blocks.iter().map(Block::plain_text).collect::<Vec<_>>().join("\n");
    assert!(text.contains("New "), "the header was not changed: {text:?}");
}

#[test]
fn leaving_puts_the_document_back() {
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    assert!(document.leave_part());
    assert_eq!(document.part_being_edited(), None);
    assert_eq!(document.plain_text(), "The body of the document");
}

#[test]
fn leaving_when_nothing_was_entered_does_nothing() {
    assert!(!document().leave_part());
}

#[test]
fn a_part_that_is_not_there_is_not_entered() {
    assert!(!document().enter_part("word/nothing.xml"));
}

#[test]
fn undo_comes_back_to_the_part_the_change_was_made_in() {
    // The behaviour that matters: edit the header, come out, press undo, and
    // the header edit is taken back — which means going back into the header.
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("New ");
    document.leave_part();

    assert!(document.undo());
    assert_eq!(document.part_being_edited(), Some(part.as_str()), "undo did not go back");
    assert!(!document.plain_text().contains("New "), "the change was not taken back");
}

#[test]
fn a_change_in_the_body_undoes_in_the_body() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("A ");
    assert!(document.undo());
    assert_eq!(document.part_being_edited(), None);
    assert_eq!(document.plain_text(), "The body of the document");
}

#[test]
fn what_was_typed_in_a_header_survives_being_saved() {
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("Kept ");
    document.leave_part();

    // Saved and reopened twice, because the part has to reach the package on
    // the way out and stay there.
    let once = round_trip(&document);
    let twice = round_trip(&once);
    let header = twice.furniture(Furniture::Header).expect("a header");
    let text: String = header.blocks.iter().map(Block::plain_text).collect::<Vec<_>>().join("\n");
    assert!(text.contains("Kept "), "{text:?}");
}

// --- Undo across parts --------------------------------------------------------

/// A body of two paragraphs, "A" and "B", under a header that says "H".
fn two_paragraphs_under_a_header() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("A")));
    body.blocks.push(Block::Paragraph(Paragraph::text("B")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document
        .set_furniture(
            Furniture::Header,
            wp_docx::furniture::Preset::Text,
            wp_docx::model::Alignment::Start,
            "H",
        )
        .expect("a header");
    round_trip(&document)
}

/// The header's words, as the file holds them.
fn header_text(document: &Document) -> String {
    let header = document.furniture(Furniture::Header).expect("a header");
    header.blocks.iter().map(Block::plain_text).collect::<Vec<_>>().join("\n")
}

#[test]
fn undo_after_leaving_a_header_takes_the_change_back_in_the_header() {
    let mut document = two_paragraphs_under_a_header();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("X");
    document.leave_part();

    assert!(document.undo());
    assert_eq!(document.part_being_edited(), Some(part.as_str()), "undo did not go back in");
    assert_eq!(document.tree().root.local_name(), "hdr", "the tree is not the header's");
    assert_eq!(document.plain_text(), "H", "the header was not put back");
    assert_eq!(document.caret(), TextPosition::new(0, 0), "the caret is not where X went in");

    // The body was not touched, and a save writes each part under its own name.
    let reopened = round_trip(&document);
    assert_eq!(reopened.plain_text(), "A\nB", "the header's paragraph landed in the body");
    assert_eq!(header_text(&reopened), "H");
}

#[test]
fn redo_after_undo_in_a_header_puts_the_change_back_in_the_header() {
    let mut document = two_paragraphs_under_a_header();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("X");
    document.leave_part();
    assert!(document.undo());

    assert!(document.redo());
    assert_eq!(document.part_being_edited(), Some(part.as_str()));
    assert_eq!(document.plain_text(), "XH", "redo put back something else");
    assert_eq!(document.caret(), TextPosition::new(0, 1), "the caret is not after the X");

    let reopened = round_trip(&document);
    assert_eq!(reopened.plain_text(), "A\nB");
    assert_eq!(header_text(&reopened), "XH");
}

#[test]
fn redo_from_the_body_goes_back_into_the_header() {
    // Undo went into the header, the person came back out, and redo has to go
    // in again: the step belongs to the header wherever the caret is now.
    let mut document = two_paragraphs_under_a_header();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("X");
    document.leave_part();
    assert!(document.undo());
    document.leave_part();

    assert!(document.redo());
    assert_eq!(document.part_being_edited(), Some(part.as_str()), "redo did not go back in");
    assert_eq!(document.plain_text(), "XH");
    document.leave_part();
    assert_eq!(document.plain_text(), "A\nB", "redo changed the body");
}

#[test]
fn a_new_paragraph_in_a_header_undoes_and_redoes_in_the_header() {
    // Enter changes the shape of the part, so the step keeps the whole tree
    // rather than one paragraph; that tree is the header's, and so is the one
    // kept for redo.
    let mut document = two_paragraphs_under_a_header();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    document.set_caret(TextPosition::new(0, 1));
    document.press_enter();
    assert_eq!(document.paragraph_count(), 2);
    document.leave_part();

    assert!(document.undo());
    assert_eq!(document.part_being_edited(), Some(part.as_str()));
    assert_eq!(document.paragraph_count(), 1);
    document.leave_part();
    assert_eq!(document.plain_text(), "A\nB");

    assert!(document.redo());
    assert_eq!(document.part_being_edited(), Some(part.as_str()), "redo stayed in the body");
    assert_eq!(document.paragraph_count(), 2, "redo did not put the header's paragraph back");
    let reopened = round_trip(&document);
    assert_eq!(reopened.plain_text(), "A\nB");
}

#[test]
fn every_part_that_can_be_entered_undoes_in_itself() {
    // Headers, footers, the notes and the comments are all entered the same
    // way, so a step made in any of them has to go back into that one.
    let mut document = two_paragraphs_under_a_header();
    document
        .set_furniture(
            Furniture::Footer,
            wp_docx::furniture::Preset::Text,
            wp_docx::model::Alignment::Start,
            "F",
        )
        .expect("a footer");
    document.set_caret(TextPosition::new(0, 1));
    document.add_note(Kind::Footnote, "Foot").expect("a footnote");
    document.add_note(Kind::Endnote, "End").expect("an endnote");
    document.set_caret(TextPosition::new(1, 0));
    document.add_comment("Said", "Someone", "2026-09-29T00:00:00Z").expect("a comment");
    let document = round_trip(&document);
    let body = document.plain_text();

    let parts = [
        document.furniture_part(Furniture::Header).expect("a header part"),
        document.furniture_part(Furniture::Footer).expect("a footer part"),
        document.notes_part(Kind::Footnote).expect("a footnotes part"),
        document.notes_part(Kind::Endnote).expect("an endnotes part"),
        document.comments_part().expect("a comments part"),
    ];
    for part in parts {
        let mut document = document.clone();
        assert!(document.enter_part(&part), "{part} could not be entered");
        let before = document.plain_text();
        let last = document.paragraph_count() - 1;
        document.set_caret(TextPosition::new(last, 0));
        document.type_text("X");
        let after = document.plain_text();
        document.leave_part();

        assert!(document.undo(), "{part}");
        assert_eq!(document.part_being_edited(), Some(part.as_str()), "{part}");
        assert_eq!(document.plain_text(), before, "{part} was not put back");
        document.leave_part();
        assert_eq!(document.plain_text(), body, "undo in {part} changed the body");

        assert!(document.redo(), "{part}");
        assert_eq!(document.part_being_edited(), Some(part.as_str()), "{part}");
        assert_eq!(document.plain_text(), after, "{part} was not redone");
        document.leave_part();
        assert_eq!(document.plain_text(), body, "redo in {part} changed the body");
    }
}

#[test]
fn entering_a_part_drops_what_was_selected_in_the_one_left() {
    // Stretches picked out with Ctrl held are places in the body; in a header
    // they would be places in text that is not there.
    let mut document = two_paragraphs_under_a_header();
    document.add_selection(TextPosition::new(0, 0), TextPosition::new(0, 1));
    document.add_selection(TextPosition::new(1, 0), TextPosition::new(1, 1));
    assert_eq!(document.selections().len(), 2);

    let part = document.furniture_part(Furniture::Header).expect("a header part");
    assert!(document.enter_part(&part));
    assert!(document.selections().is_empty(), "{:?}", document.selections());
}

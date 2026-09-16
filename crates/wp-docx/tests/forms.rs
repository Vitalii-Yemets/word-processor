//! Putting a form field into a document and filling it in.

use wp_docx::forms::{FormKind, TICKED, UNTICKED};
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Name: ")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// A document with one field of the given kind at the end of its paragraph.
fn with_field(kind: FormKind, items: &[&str]) -> Document {
    let mut document = document();
    let end = document.paragraph_text(0).unwrap_or_default().len();
    document.set_caret(TextPosition::new(0, end));
    let items: Vec<String> = items.iter().map(|item| (*item).to_owned()).collect();
    assert!(document.insert_form_field(kind, "Field", &items));
    round_trip(&document)
}

#[test]
fn a_text_field_goes_in_and_reads_back_as_one() {
    let document = with_field(FormKind::Text, &[]);
    let fields = document.form_fields();
    assert_eq!(fields.len(), 1, "{fields:?}");
    assert_eq!(fields[0].kind, FormKind::Text);
    assert_eq!(fields[0].name, "Field");
    assert!(fields[0].enabled);
    // Its answer is empty and sits at the end of the words before it.
    assert_eq!(fields[0].start, TextPosition::new(0, 6));
    assert_eq!(fields[0].end, TextPosition::new(0, 6));
}

#[test]
fn a_tick_box_starts_unticked_and_ticks() {
    let mut document = with_field(FormKind::CheckBox, &[]);
    assert!(!document.form_fields()[0].checked);
    assert!(document.plain_text().contains(UNTICKED), "{}", document.plain_text());

    let at = document.form_fields()[0].start;
    assert!(document.set_check_box(at, true));
    let reopened = round_trip(&document);
    assert!(reopened.form_fields()[0].checked, "the document does not say it is ticked");
    assert!(reopened.plain_text().contains(TICKED), "{}", reopened.plain_text());

    // And back again.
    let mut reopened = reopened;
    let at = reopened.form_fields()[0].start;
    assert!(reopened.set_check_box(at, false));
    let twice = round_trip(&reopened);
    assert!(!twice.form_fields()[0].checked);
    assert!(twice.plain_text().contains(UNTICKED));
}

#[test]
fn a_drop_down_offers_what_it_was_given_and_the_first_is_chosen() {
    let document = with_field(FormKind::DropDown, &["Mr", "Ms", "Dr"]);
    let field = &document.form_fields()[0];
    assert_eq!(field.kind, FormKind::DropDown);
    assert_eq!(field.items, vec!["Mr", "Ms", "Dr"]);
    assert_eq!(field.chosen, 0);
    assert!(document.plain_text().contains("Mr"), "{}", document.plain_text());
}

#[test]
fn choosing_one_of_them_puts_it_in_the_text_and_remembers_which() {
    let mut document = with_field(FormKind::DropDown, &["Mr", "Ms", "Dr"]);
    let at = document.form_fields()[0].start;
    assert!(document.choose_form_item(at, 2));

    let reopened = round_trip(&document);
    let field = &reopened.form_fields()[0];
    assert_eq!(field.chosen, 2, "the document does not say which was chosen");
    assert!(reopened.plain_text().contains("Dr"), "{}", reopened.plain_text());
    assert!(!reopened.plain_text().contains("Mr"), "the old answer is still there");
}

#[test]
fn a_kind_that_is_not_the_one_asked_for_is_refused() {
    let mut document = with_field(FormKind::Text, &[]);
    let at = document.form_fields()[0].start;
    assert!(!document.set_check_box(at, true), "a text field was ticked");
    assert!(!document.choose_form_item(at, 0), "a text field was picked from");
}

#[test]
fn a_position_with_no_field_at_it_changes_nothing() {
    let mut document = with_field(FormKind::CheckBox, &[]);
    assert!(!document.set_check_box(TextPosition::new(0, 0), true));
    assert!(!document.form_fields()[0].checked);
}

#[test]
fn two_fields_in_one_paragraph_are_told_apart() {
    let mut document = document();
    let end = document.paragraph_text(0).unwrap_or_default().len();
    document.set_caret(TextPosition::new(0, end));
    document.insert_form_field(FormKind::CheckBox, "First", &[]);
    let end = document.paragraph_text(0).unwrap_or_default().len();
    document.set_caret(TextPosition::new(0, end));
    document.insert_form_field(FormKind::CheckBox, "Second", &[]);

    let mut document = round_trip(&document);
    let fields = document.form_fields();
    assert_eq!(fields.len(), 2, "{fields:?}");
    assert_eq!(fields[0].name, "First");
    assert_eq!(fields[1].name, "Second");

    // Ticking the second leaves the first alone.
    let at = fields[1].start;
    assert!(document.set_check_box(at, true));
    let fields = round_trip(&document).form_fields();
    assert!(!fields[0].checked, "the wrong box was ticked");
    assert!(fields[1].checked, "the right box was not ticked");
}

#[test]
fn the_field_is_written_the_way_word_writes_one() {
    let document = with_field(FormKind::DropDown, &["Mr", "Ms"]);
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let xml = package.xml_part("word/document.xml").expect("the document").expect("readable");

    for wanted in [
        "w:ffData",
        "w:ddList",
        "w:listEntry",
        "FORMDROPDOWN",
        "w:fldCharType=\"begin\"",
        "w:fldCharType=\"separate\"",
        "w:fldCharType=\"end\"",
    ] {
        assert!(xml.contains(wanted), "no {wanted} in the document");
    }
}

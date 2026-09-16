//! Content controls put into a document and filled in.

use wp_docx::controls::{ControlKind, TICKED, UNTICKED};
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

fn with_control(kind: ControlKind, alias: &str, items: &[&str]) -> Document {
    let mut document = document();
    let end = document.paragraph_text(0).unwrap_or_default().len();
    document.set_caret(TextPosition::new(0, end));
    let items: Vec<String> = items.iter().map(|item| (*item).to_owned()).collect();
    assert!(document.insert_control(kind, alias, &items));
    round_trip(&document)
}

#[test]
fn a_document_has_no_controls_to_begin_with() {
    assert!(document().controls().is_empty());
}

#[test]
fn every_kind_goes_in_and_reads_back_as_the_kind_it_was() {
    for kind in ControlKind::ALL {
        let document = with_control(*kind, "Surname", &["one", "two"]);
        let controls = document.controls();
        assert_eq!(controls.len(), 1, "{}: {controls:?}", kind.label());
        assert_eq!(controls[0].kind, *kind, "{}", kind.label());
        assert_eq!(controls[0].alias, "Surname", "{}", kind.label());
        assert_eq!(controls[0].tag, "surname", "{}", kind.label());
    }
}

#[test]
fn a_control_shows_something_before_anybody_writes_in_it() {
    let document = with_control(ControlKind::PlainText, "Surname", &[]);
    assert!(
        document.plain_text().contains("Enter surname"),
        "an empty control shows nothing at all: {}",
        document.plain_text()
    );
}

#[test]
fn a_tick_box_ticks_and_unticks() {
    let mut document = with_control(ControlKind::CheckBox, "Agreed", &[]);
    assert!(!document.controls()[0].checked);
    assert!(document.plain_text().contains(UNTICKED));

    let at = document.controls()[0].start;
    assert!(document.set_control_checked(at, true));
    let reopened = round_trip(&document);
    assert!(reopened.controls()[0].checked, "the document does not say it is ticked");
    assert!(reopened.plain_text().contains(TICKED));

    let mut reopened = reopened;
    let at = reopened.controls()[0].start;
    assert!(reopened.set_control_checked(at, false));
    assert!(!round_trip(&reopened).controls()[0].checked);
}

#[test]
fn a_list_offers_what_it_was_given_and_one_can_be_chosen() {
    let mut document = with_control(ControlKind::DropDown, "Title", &["Mr", "Ms", "Dr"]);
    let control = &document.controls()[0];
    assert_eq!(
        control.items,
        vec![
            ("Mr".to_owned(), "Mr".to_owned()),
            ("Ms".to_owned(), "Ms".to_owned()),
            ("Dr".to_owned(), "Dr".to_owned())
        ]
    );

    let at = control.start;
    assert!(document.choose_control_item(at, 2));
    let reopened = round_trip(&document);
    assert!(reopened.plain_text().contains("Dr"), "{}", reopened.plain_text());
    assert!(!reopened.plain_text().contains("Mr"), "the old answer is still there");
}

#[test]
fn words_can_be_written_into_a_text_control() {
    let mut document = with_control(ControlKind::PlainText, "Surname", &[]);
    let at = document.controls()[0].start;
    assert!(document.set_control_text(at, "Habgood"));

    let reopened = round_trip(&document);
    assert_eq!(reopened.plain_text(), "Name: Habgood");
    assert_eq!(reopened.controls().len(), 1, "the control itself was written over");
}

#[test]
fn a_kind_that_is_not_the_one_asked_for_is_refused() {
    let mut document = with_control(ControlKind::PlainText, "Surname", &[]);
    let at = document.controls()[0].start;
    assert!(!document.set_control_checked(at, true), "a text control was ticked");
    assert!(!document.choose_control_item(at, 0), "a text control was picked from");

    let mut ticked = with_control(ControlKind::CheckBox, "Agreed", &[]);
    let at = ticked.controls()[0].start;
    assert!(!ticked.set_control_text(at, "words"), "words were written into a tick box");
}

#[test]
fn two_controls_side_by_side_are_told_apart() {
    let mut document = document();
    let end = document.paragraph_text(0).unwrap_or_default().len();
    document.set_caret(TextPosition::new(0, end));
    document.insert_control(ControlKind::CheckBox, "First", &[]);
    let end = document.paragraph_text(0).unwrap_or_default().len();
    document.set_caret(TextPosition::new(0, end));
    document.insert_control(ControlKind::CheckBox, "Second", &[]);

    let mut document = round_trip(&document);
    let controls = document.controls();
    assert_eq!(controls.len(), 2, "{controls:?}");
    assert_eq!(controls[0].alias, "First");
    assert_eq!(controls[1].alias, "Second");

    let at = controls[1].start;
    assert!(document.set_control_checked(at, true));
    let controls = round_trip(&document).controls();
    assert!(!controls[0].checked, "the wrong box was ticked");
    assert!(controls[1].checked, "the right box was not ticked");
}

#[test]
fn the_control_is_written_the_way_word_writes_one() {
    let document = with_control(ControlKind::DropDown, "Title", &["Mr", "Ms"]);
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let xml = package.xml_part("word/document.xml").expect("the document").expect("readable");

    for wanted in ["w:sdt", "w:sdtPr", "w:sdtContent", "w:dropDownList", "w:listItem", "w:alias"] {
        assert!(xml.contains(wanted), "no {wanted} in the document");
    }
}

#[test]
fn a_tick_box_names_the_namespace_word_put_it_in() {
    let document = with_control(ControlKind::CheckBox, "Agreed", &[]);
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let xml = package.xml_part("word/document.xml").expect("the document").expect("readable");

    assert!(xml.contains("w14:checkbox"), "{xml}");
    assert!(
        xml.contains("http://schemas.microsoft.com/office/word/2010/wordml"),
        "the namespace is used and never declared"
    );
}

#[test]
fn the_text_inside_a_control_is_part_of_the_document_like_any_other() {
    // A reader that did not know what a content control was would still see
    // the words, which is what wrapping rather than replacing is for.
    let document = with_control(ControlKind::PlainText, "Surname", &[]);
    assert!(document.plain_text().starts_with("Name: "));
    assert_eq!(document.paragraph_count(), 1);
}

//! Text effects: what is written, what comes back, and what Word will accept.

use wp_docx::effects::{Effect, MC, W14};
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};

/// A document of one sentence, with the whole of it selected.
fn selected() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Glowing words")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document.select_all();
    document
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    let mut reopened = Document::open(&bytes).expect("reopening");
    reopened.set_caret(TextPosition::new(0, 3));
    reopened
}

fn document_xml(document: &Document) -> String {
    document.package().xml_part("word/document.xml").expect("the document").expect("readable")
}

#[test]
fn text_has_no_effect_to_begin_with() {
    assert_eq!(selected().text_effect_here(), Effect::None);
}

#[test]
fn every_effect_can_be_applied_and_reads_back_after_saving() {
    for effect in Effect::DRAWN {
        let mut document = selected();
        assert!(document.set_text_effect(*effect), "{}", effect.label());

        let reopened = round_trip(&document);
        assert_eq!(reopened.text_effect_here(), *effect, "{}", effect.label());
    }
}

#[test]
fn an_effect_is_written_in_the_extension_namespace() {
    let mut document = selected();
    document.set_text_effect(Effect::Glow);

    let part = document_xml(&round_trip(&document));
    assert!(part.contains(&format!("xmlns:w14=\"{W14}\"")), "the namespace is missing: {part}");
    assert!(part.contains("<w14:glow"), "the effect is missing: {part}");
}

#[test]
fn the_extension_namespace_is_marked_ignorable() {
    // Without this a reader that has never heard of w14 stops at the element
    // instead of skipping it, and the document does not open at all.
    let mut document = selected();
    document.set_text_effect(Effect::Shadow);

    let part = document_xml(&round_trip(&document));
    assert!(
        part.contains(&format!("xmlns:mc=\"{MC}\"")),
        "markup compatibility is missing: {part}"
    );
    assert!(part.contains("mc:Ignorable="), "nothing is marked ignorable: {part}");
    let ignorable = part
        .split("mc:Ignorable=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the list of ignorable prefixes");
    assert!(ignorable.split_whitespace().any(|prefix| prefix == "w14"), "got {ignorable:?}");
}

#[test]
fn an_effect_can_be_taken_off_again() {
    let mut document = selected();
    document.set_text_effect(Effect::Outline);

    let mut reopened = round_trip(&document);
    reopened.select_all();
    assert!(reopened.set_text_effect(Effect::None));

    let plain = round_trip(&reopened);
    assert_eq!(plain.text_effect_here(), Effect::None);
    assert!(!document_xml(&plain).contains("w14:textOutline"), "the outline is still there");
}

#[test]
fn taking_an_effect_off_text_that_never_had_one_changes_nothing() {
    let mut document = selected();
    assert!(!document.set_text_effect(Effect::None));
}

#[test]
fn a_document_with_no_effects_does_not_declare_the_namespace() {
    // Declaring it would be harmless, but a file that says nothing about
    // effects should read like one.
    let part = document_xml(&round_trip(&selected()));
    assert!(!part.contains(W14), "an unused namespace was declared: {part}");
}

#[test]
fn an_effect_does_not_touch_a_character_of_the_text() {
    let mut document = selected();
    document.set_text_effect(Effect::Reflection);
    assert_eq!(round_trip(&document).plain_text(), "Glowing words");
}

#[test]
fn one_effect_replaces_another_rather_than_joining_it() {
    let mut document = selected();
    document.set_text_effect(Effect::Glow);

    let mut reopened = round_trip(&document);
    reopened.select_all();
    reopened.set_text_effect(Effect::Shadow);

    let part = document_xml(&round_trip(&reopened));
    assert!(part.contains("<w14:shadow"), "the new effect is missing: {part}");
    assert!(!part.contains("<w14:glow"), "the old effect is still there: {part}");
}

#[test]
fn applying_an_effect_can_be_undone() {
    let mut document = selected();
    document.set_text_effect(Effect::Glow);
    assert!(document.undo());
    assert_eq!(document.text_effect_here(), Effect::None);
}

#[test]
fn an_effect_on_part_of_a_sentence_stays_on_that_part() {
    let mut document = selected();
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 7));
    document.set_text_effect(Effect::Shadow);

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(0, 3));
    assert_eq!(reopened.text_effect_here(), Effect::Shadow);
    // "words", past the shadowed part.
    reopened.set_caret(TextPosition::new(0, 10));
    assert_eq!(reopened.text_effect_here(), Effect::None);
}

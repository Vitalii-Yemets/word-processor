//! Merge rules in a document: written, saved, reopened and worked out.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::rules::Rule;
use wp_docx::{Document, TextPosition};

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Dear reader, ")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    // At the end of the sentence.
    document.set_caret(TextPosition::new(0, 13));
    document
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn document_xml(document: &Document) -> String {
    document.package().xml_part("word/document.xml").expect("the document").expect("readable")
}

fn leeds() -> Vec<(String, String)> {
    vec![("Name".to_owned(), "Ann".to_owned()), ("City".to_owned(), "Leeds".to_owned())]
}

fn york() -> Vec<(String, String)> {
    vec![("Name".to_owned(), "Ben".to_owned()), ("City".to_owned(), "York".to_owned())]
}

/// A document carrying one rule.
fn with_rule(rule: Rule, typed: &str) -> Document {
    let mut document = document();
    let instruction = rule.instruction(typed).expect("an instruction");
    assert!(document.insert_rule(&instruction), "{instruction}");
    round_trip(&document)
}

#[test]
fn a_rule_is_written_as_a_field_the_long_way_round() {
    let document = with_rule(Rule::SkipIf, "City; Leeds");
    let part = document_xml(&document);
    assert!(part.contains("fldChar"), "not written the long way: {part}");
    assert!(part.contains("instrText"), "no instruction: {part}");
    assert!(part.contains("SKIPIF"), "no rule: {part}");
    // The condition is a field of its own inside it.
    assert!(part.contains("MERGEFIELD City"), "no nested field: {part}");
}

#[test]
fn an_empty_rule_is_not_put_in() {
    let mut document = document();
    assert!(!document.insert_rule("   "));
}

#[test]
fn a_rule_says_nothing_in_the_text_of_the_document() {
    // The instruction is not what the document says: it is how the document
    // works something out.
    let document = with_rule(Rule::SkipIf, "City; Leeds");
    assert_eq!(document.plain_text(), "Dear reader, ");
}

#[test]
fn a_skip_rule_leaves_out_the_recipients_it_names() {
    let document = with_rule(Rule::SkipIf, "City; Leeds");
    assert!(document.record_is_skipped(&leeds()), "the recipient was not skipped");
    assert!(!document.record_is_skipped(&york()), "the wrong recipient was skipped");
}

#[test]
fn a_document_with_no_rules_skips_nobody() {
    assert!(!document().record_is_skipped(&leeds()));
}

#[test]
fn a_choice_says_one_thing_or_the_other() {
    let mut document = with_rule(Rule::If, "City; Leeds; near you; far away");
    assert_eq!(document.apply_merge_rules(&leeds(), 1), 1);
    assert!(document.plain_text().contains("near you"), "{}", document.plain_text());

    let mut other = with_rule(Rule::If, "City; Leeds; near you; far away");
    other.apply_merge_rules(&york(), 2);
    assert!(other.plain_text().contains("far away"), "{}", other.plain_text());
}

#[test]
fn the_record_number_is_the_number_of_the_recipient() {
    let mut document = with_rule(Rule::RecordNumber, "");
    document.apply_merge_rules(&leeds(), 7);
    assert!(document.plain_text().ends_with('7'), "{}", document.plain_text());
}

#[test]
fn a_rule_that_has_been_worked_out_is_no_longer_a_field() {
    let mut document = with_rule(Rule::RecordNumber, "");
    document.apply_merge_rules(&leeds(), 1);
    let part = document_xml(&round_trip(&document));
    assert!(!part.contains("MERGEREC"), "the rule is still there: {part}");
}

#[test]
fn a_field_that_is_not_a_rule_is_left_alone() {
    // A page number is a field the long way round too, and a merge must not
    // touch it.
    let mut document = document();
    document.insert_field("PAGE", "1");

    let mut reopened = round_trip(&document);
    reopened.apply_merge_rules(&leeds(), 1);
    assert!(document_xml(&round_trip(&reopened)).contains("PAGE"), "the page number was eaten");
}

#[test]
fn working_the_rules_out_can_be_undone() {
    let mut document = with_rule(Rule::If, "City; Leeds; near you; far away");
    document.apply_merge_rules(&leeds(), 1);
    assert!(document.undo());
    assert!(!document.plain_text().contains("near you"));
}

#[test]
fn every_rule_can_be_put_in_and_survives_being_saved() {
    for (rule, typed) in [
        (Rule::SkipIf, "City; Leeds"),
        (Rule::If, "City; Leeds; yes; no"),
        (Rule::RecordNumber, ""),
        (Rule::SequenceNumber, ""),
    ] {
        let document = with_rule(rule, typed);
        let part = document_xml(&document);
        assert!(part.contains("fldChar"), "{}: {part}", rule.label());
    }
}

//! Limiting formatting to a selection of styles.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::protection::{EditMode, Protection};
use wp_docx::styles::StyleDefinition;
use wp_docx::Document;

/// A document with three styles of its own, which is what a restriction picks
/// from.
fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("One")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    for (id, name) in [("Heading1", "heading 1"), ("Quote", "Quote"), ("Caption", "caption")] {
        let mut wanted = StyleDefinition {
            id: id.to_owned(),
            name: name.to_owned(),
            based_on: None,
            next: None,
            paragraph: Default::default(),
            run: Default::default(),
        };
        wanted.run.bold = Some(true);
        assert!(document.set_style(&wanted), "{id} was not written");
    }
    document
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn limited(document: &mut Document, allowed: &[&str]) {
    let allowed: Vec<String> = allowed.iter().map(|id| (*id).to_owned()).collect();
    assert!(document.allow_only_styles(&allowed), "nothing was locked");
    assert!(document.set_protection(Some(&Protection::on_formatting())), "nothing was restricted");
}

#[test]
fn a_document_locks_nothing_to_begin_with() {
    let document = document();
    assert!(!document.formatting_is_limited());
    assert!(!document.style_is_locked("Quote"));
    assert_eq!(document.allowed_styles().len(), document.styles().all().len());
}

#[test]
fn the_styles_that_were_not_chosen_are_locked_and_the_chosen_ones_are_not() {
    let mut document = document();
    limited(&mut document, &["Heading1"]);

    let document = round_trip(&document);
    assert!(document.formatting_is_limited(), "the restriction did not survive");
    assert!(!document.style_is_locked("Heading1"), "an allowed style was locked");
    assert!(document.style_is_locked("Quote"), "a style nobody allowed is free");
    assert!(document.style_is_locked("Caption"));
    assert!(document.allowed_styles().iter().any(|id| id == "Heading1"));
}

#[test]
fn a_locked_style_cannot_be_applied_while_the_restriction_stands() {
    let mut document = document();
    limited(&mut document, &["Heading1"]);

    assert!(!document.set_paragraph_style(0, Some("Quote")), "a locked style was applied");
    assert_eq!(document.style_here(), None);

    // And the allowed one still can.
    assert!(document.set_paragraph_style(0, Some("Heading1")));
    assert_eq!(document.style_here().as_deref(), Some("Heading1"));
}

#[test]
fn clearing_the_style_is_always_allowed() {
    // Otherwise a paragraph that was given a style before the restriction
    // went on could never be taken out of it, which would be a trap rather
    // than a restriction.
    let mut document = document();
    assert!(document.set_paragraph_style(0, Some("Quote")));
    limited(&mut document, &["Heading1"]);

    assert!(document.set_paragraph_style(0, None), "a paragraph could not be put back to Normal");
    assert_eq!(document.style_here(), None);
}

#[test]
fn a_lock_means_nothing_until_something_is_being_enforced() {
    let mut document = document();
    let allowed = vec!["Heading1".to_owned()];
    assert!(document.allow_only_styles(&allowed));

    assert!(document.style_is_locked("Quote"), "the mark was not written");
    assert!(document.style_is_available("Quote"), "the mark stood on its own");
    assert!(document.set_paragraph_style(0, Some("Quote")), "an unenforced lock refused a style");
}

#[test]
fn lifting_the_restriction_leaves_the_marks_where_they_were() {
    // Word's behaviour, and what makes putting the same restriction back a
    // matter of one tick rather than thirty.
    let mut document = document();
    limited(&mut document, &["Heading1"]);
    assert!(document.set_protection(None));

    let document = round_trip(&document);
    assert!(!document.formatting_is_limited());
    assert!(document.style_is_locked("Quote"), "the ticks were thrown away");
    assert!(document.style_is_available("Quote"), "and it is not being enforced");
}

#[test]
fn allowing_everything_takes_every_mark_off() {
    let mut document = document();
    limited(&mut document, &["Heading1"]);
    assert!(document.allow_every_style());

    let document = round_trip(&document);
    assert!(!document.style_is_locked("Quote"));
    assert!(!document.style_is_locked("Caption"));
}

#[test]
fn one_style_can_be_locked_and_unlocked_by_itself() {
    let mut document = document();
    assert!(document.set_style_locked("Quote", true));
    assert!(!document.set_style_locked("Quote", true), "saying it twice changed something");
    assert!(round_trip(&document).style_is_locked("Quote"));

    assert!(document.set_style_locked("Quote", false));
    assert!(!round_trip(&document).style_is_locked("Quote"));

    assert!(
        !document.set_style_locked("NoSuchStyle", true),
        "a style that is not there was locked"
    );
}

#[test]
fn the_formatting_restriction_stands_without_any_restriction_on_the_editing() {
    // Word's two halves are independent, and this is the half that is easy to
    // get wrong: a document with `w:formatting` and no `w:edit` is a document
    // anybody may type in and nobody may format.
    let mut document = document();
    limited(&mut document, &["Heading1"]);

    let document = round_trip(&document);
    assert_eq!(document.protection(), None, "the words were restricted too");
    assert!(document.formatting_is_limited());
    let rules = document.protection_rules().expect("a restriction");
    assert_eq!(rules.mode, None);
    assert!(rules.formatting);
}

#[test]
fn both_halves_stand_together_and_behind_one_password() {
    let mut document = document();
    let wanted = Protection::new(EditMode::ReadOnly)
        .limiting_formatting(true)
        .behind("shibboleth", b"0123456789abcdef");
    assert!(document.set_protection(Some(&wanted)));

    let document = round_trip(&document);
    let rules = document.protection_rules().expect("a restriction");
    assert_eq!(rules.mode, Some(EditMode::ReadOnly));
    assert!(rules.formatting);
    assert!(rules.theme_locked);
    assert!(document.theme_is_locked());
    assert!(rules.opens_with("shibboleth"));
    assert!(!rules.opens_with("open sesame"));
}

#[test]
fn a_restriction_that_restricts_nothing_is_not_one() {
    let mut document = document();
    let nothing = Protection { mode: None, ..Protection::default() };
    assert!(!nothing.restricts_anything());
    document.set_protection(Some(&nothing));
    assert!(round_trip(&document).protection_rules().is_none());
}

#[test]
fn the_restriction_is_written_the_way_word_writes_it() {
    let mut document = document();
    limited(&mut document, &["Heading1"]);
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");

    let settings = package.xml_part("word/settings.xml").expect("settings").expect("readable");
    assert!(settings.contains("w:formatting=\"1\""), "{settings}");
    assert!(settings.contains("w:enforcement=\"1\""));
    assert!(!settings.contains("w:edit="), "an editing restriction nobody asked for");

    let styles = package.xml_part("word/styles.xml").expect("styles").expect("readable");
    assert!(styles.contains("w:locked"), "no style was marked");
    // And the hundreds a styles part mentions without defining.
    assert!(styles.contains("defLockedState"), "the latent styles were left free");
}

#[test]
fn the_mark_goes_where_the_schema_says_it_goes() {
    // `w:locked` belongs before the formatting, and a document with it after
    // is one Word refuses to open.
    let mut document = document();
    assert!(document.set_style_locked("Heading1", true));
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let styles = package.xml_part("word/styles.xml").expect("styles").expect("readable");

    let start = styles.find("Heading1").expect("the style");
    let rest = &styles[start..];
    let locked = rest.find("<w:locked").expect("the mark");
    let run = rest.find("<w:rPr").expect("the formatting");
    assert!(locked < run, "the mark was written after the formatting");
}

#[test]
fn a_password_taken_off_leaves_no_hash_behind() {
    // The restriction's element is edited rather than rewritten, so this is
    // worth asking plainly.
    let mut document = document();
    let behind = Protection::new(EditMode::ReadOnly).behind("shibboleth", b"0123456789abcdef");
    assert!(document.set_protection(Some(&behind)));
    assert!(document.set_protection(Some(&Protection::new(EditMode::ReadOnly))));

    let document = round_trip(&document);
    let rules = document.protection_rules().expect("still restricted");
    assert!(rules.password.is_none(), "the old hash is still in the file");
    assert!(rules.opens_with(""), "and it is still being asked for");
}

#[test]
fn what_the_program_does_not_model_survives_a_restriction_being_written_again() {
    let mut document = document();
    assert!(document.set_protection(Some(&Protection::new(EditMode::ReadOnly))));

    // Word's "Block Quick Style Set switching", which this program does not
    // model because it has no Quick Style Sets to block. A document that
    // carries one must not lose it by being restricted again here.
    let bytes = document.save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let settings = package.xml_part("word/settings.xml").expect("settings").expect("readable");
    let settings =
        settings.replace("<w:documentProtection ", "<w:documentProtection w:styleLockQFSet=\"1\" ");
    package.add_part(
        "word/settings.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
        settings.into_bytes(),
    );
    let mut document = Document::open(&package.save().expect("saving")).expect("reopening");

    assert!(document.set_protection(Some(&Protection::new(EditMode::Comments))));
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let settings = package.xml_part("word/settings.xml").expect("settings").expect("readable");
    assert!(
        settings.contains("styleLockQFSet"),
        "an attribute this program does not model was lost"
    );
    assert!(settings.contains("w:edit=\"comments\""));
}

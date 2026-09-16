//! The document that asks to be opened read-only.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::protection::{EditMode, Protection};
use wp_docx::readonly::WriteProtection;
use wp_docx::Document;

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("One")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_document_asks_nothing_to_begin_with() {
    assert_eq!(document().write_protection(), None);
}

#[test]
fn a_recommendation_survives_being_written_and_read_back() {
    let mut document = document();
    assert!(document.set_write_protection(Some(&WriteProtection::recommended())));

    let asked = round_trip(&document).write_protection().expect("the recommendation was lost");
    assert!(asked.recommended);
    assert!(asked.password.is_none());
    // A recommendation is a request, and anybody may decline it.
    assert!(asked.opens_with(""), "a recommendation with no password refused somebody");
}

#[test]
fn a_password_to_modify_takes_the_right_word_and_no_other() {
    let mut document = document();
    let wanted = WriteProtection::behind("Fenchurch", b"0123456789abcdef");
    assert!(document.set_write_protection(Some(&wanted)));

    let asked = round_trip(&document).write_protection().expect("the password was lost");
    assert!(asked.opens_with("Fenchurch"));
    assert!(!asked.opens_with("fenchurch"), "the case did not matter");
    assert!(!asked.opens_with(""), "an empty answer opened it for writing");
}

#[test]
fn both_halves_stand_together() {
    let mut document = document();
    let wanted = WriteProtection {
        recommended: true,
        ..WriteProtection::behind("word", b"saltsaltsaltsalt")
    };
    assert!(document.set_write_protection(Some(&wanted)));

    let asked = round_trip(&document).write_protection().expect("lost");
    assert!(asked.recommended);
    assert!(asked.opens_with("word"));
}

#[test]
fn taking_it_off_leaves_nothing_behind() {
    let mut document = document();
    document.set_write_protection(Some(&WriteProtection::behind("word", b"0123456789abcdef")));
    assert!(document.set_write_protection(None));

    let document = round_trip(&document);
    assert_eq!(document.write_protection(), None);
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let settings = package.xml_part("word/settings.xml").expect("settings").expect("readable");
    assert!(!settings.contains("writeProtection"), "{settings}");
}

#[test]
fn a_password_taken_off_a_recommendation_leaves_the_recommendation() {
    let mut document = document();
    document.set_write_protection(Some(&WriteProtection {
        recommended: true,
        ..WriteProtection::behind("word", b"0123456789abcdef")
    }));
    assert!(document.set_write_protection(Some(&WriteProtection::recommended())));

    let asked = round_trip(&document).write_protection().expect("the recommendation went too");
    assert!(asked.recommended);
    assert!(asked.password.is_none(), "the hash is still in the file");
    assert!(asked.opens_with(""), "and it is still being asked for");
}

#[test]
fn it_is_written_the_way_word_writes_it_and_where_the_schema_says() {
    let mut document = document();
    document.set_write_protection(Some(&WriteProtection {
        recommended: true,
        ..WriteProtection::behind("word", b"0123456789abcdef")
    }));
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let settings = package.xml_part("word/settings.xml").expect("settings").expect("readable");

    assert!(settings.contains("w:writeProtection"), "{settings}");
    assert!(settings.contains("w:recommended=\"1\""));
    assert!(settings.contains("w:algorithmName=\"SHA-512\""));
    assert!(settings.contains("w:hashValue="));
    assert!(settings.contains("w:saltValue="));
    assert!(settings.contains("w:spinCount="));

    // The schema puts it first, and a settings part in the wrong order is one
    // Word refuses to open.
    let body = settings.split("<w:settings").nth(1).expect("the settings element");
    let first = body.find("<w:").expect("some setting");
    assert!(
        body[first..].starts_with("<w:writeProtection"),
        "it was not written first: {}",
        &body[first..first + 40.min(body.len() - first)]
    );
}

#[test]
fn the_older_way_of_writing_the_password_is_read_too() {
    // A document out of Word 2007, which named its hash by a number.
    let mut document = document();
    document.set_write_protection(Some(&WriteProtection::recommended()));
    let bytes = document.save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let settings = package.xml_part("word/settings.xml").expect("settings").expect("readable");
    let settings = settings.replace(
        "<w:writeProtection ",
        "<w:writeProtection w:cryptAlgorithmSid=\"4\" w:cryptSpinCount=\"50000\" \
         w:hash=\"abcd\" w:salt=\"efgh\" ",
    );
    package.add_part(
        "word/settings.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
        settings.into_bytes(),
    );

    let document = Document::open(&package.save().expect("saving")).expect("reopening");
    let asked = document.write_protection().expect("the older attributes were not read");
    let password = asked.password.expect("no password was found");
    assert!(password.understood(), "SHA-1 is one this program has");
    assert_eq!(password.algorithm_name(), "SHA-1");
}

#[test]
fn it_is_a_different_question_from_the_restriction() {
    // The two live side by side in the settings and mean different things: one
    // says whether the file opens for writing, the other what may be done to
    // the text once it is open.
    let mut document = document();
    document.set_write_protection(Some(&WriteProtection::recommended()));
    document.set_protection(Some(&Protection::new(EditMode::Comments)));

    let document = round_trip(&document);
    assert!(document.write_protection().is_some_and(|asked| asked.recommended));
    assert_eq!(document.protection(), Some(EditMode::Comments));
}

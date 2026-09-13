//! Change Case, in the language the text is written in.

use wp_docx::model::{Block, Body, Paragraph, Run};
use wp_docx::page::CaseChange;
use wp_docx::{Document, TextPosition};

/// One paragraph of text tagged with a language.
fn document(text: &str, language: &str) -> Document {
    let run = Run::text(text).in_language(language);
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![run])));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// The whole paragraph, put through Word's `Aa` button.
fn changed(text: &str, language: &str, wanted: CaseChange) -> String {
    let mut document = document(text, language);
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, text.len()));
    assert!(document.change_case(wanted), "nothing changed");
    document.paragraph_text(0).unwrap_or_default()
}

#[test]
fn a_turkish_i_keeps_its_dot_in_capitals() {
    assert_eq!(changed("istanbul", "tr-TR", CaseChange::Upper), "\u{0130}STANBUL");
}

#[test]
fn an_english_i_does_not() {
    // The same text, the same button, a different answer — because the
    // document says a different language.
    assert_eq!(changed("istanbul", "en-GB", CaseChange::Upper), "ISTANBUL");
}

#[test]
fn a_turkish_capital_i_loses_its_dot_going_the_other_way() {
    assert_eq!(changed("ISTANBUL", "tr-TR", CaseChange::Lower), "\u{0131}stanbul");
}

#[test]
fn capitalising_a_word_follows_the_language_too() {
    // Not only the all-capitals button: every one of the five has to ask.
    assert_eq!(changed("istanbul", "tr-TR", CaseChange::Capitalize), "\u{0130}stanbul");
    assert_eq!(changed("istanbul", "en-GB", CaseChange::Capitalize), "Istanbul");
}

#[test]
fn greek_drops_its_accents_in_capitals() {
    let word = "\u{03AC}\u{03BD}\u{03B8}\u{03C1}\u{03C9}\u{03C0}\u{03BF}\u{03C2}";
    assert_eq!(
        changed(word, "el-GR", CaseChange::Upper),
        "\u{0391}\u{039D}\u{0398}\u{03A1}\u{03A9}\u{03A0}\u{039F}\u{03A3}"
    );
}

#[test]
fn the_sharp_s_becomes_two_letters_in_any_language() {
    assert_eq!(changed("stra\u{00DF}e", "de-DE", CaseChange::Upper), "STRASSE");
}

#[test]
fn a_sigma_at_the_end_of_a_word_is_written_as_one() {
    assert_eq!(
        changed("\u{039F}\u{0394}\u{039F}\u{03A3}", "el-GR", CaseChange::Lower),
        "\u{03BF}\u{03B4}\u{03BF}\u{03C2}"
    );
}

#[test]
fn a_document_that_says_nothing_about_its_language_gets_the_standards_answer() {
    let mut document = {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("istanbul")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    };
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 8));
    assert!(document.change_case(CaseChange::Upper));
    assert_eq!(document.paragraph_text(0).unwrap_or_default(), "ISTANBUL");
}

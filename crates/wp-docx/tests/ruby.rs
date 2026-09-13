//! A word with its reading over it, in a document.

use wp_docx::model::{Block, Body, Paragraph, Run, RunContent};
use wp_docx::ruby::{Align, Ruby};
use wp_docx::{Document, TextPosition};

const WORD: &str = "\u{6F22}\u{5B57}";
const READING: &str = "\u{304B}\u{3093}\u{3058}";

/// A document whose one paragraph holds a ruby, written the way Word writes
/// one: the reading first, the word after it.
fn with_ruby() -> Document {
    let plain = {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run::text("placeholder")])));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    };

    let bytes = plain.save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("the package");
    let main = package.main_document_part().expect("the main part");
    let xml = package.xml_part(&main).expect("the part").expect("text");
    let ruby = format!(
        "</w:t></w:r><w:r><w:ruby><w:rubyPr><w:rubyAlign w:val=\"distributeSpace\"/>\
         <w:hps w:val=\"10\"/><w:hpsRaise w:val=\"22\"/><w:hpsBaseText w:val=\"21\"/>\
         <w:lid w:val=\"ja-JP\"/></w:rubyPr><w:rt><w:r><w:t>{READING}</w:t></w:r></w:rt>\
         <w:rubyBase><w:r><w:t>{WORD}</w:t></w:r></w:rubyBase></w:ruby></w:r><w:r><w:t>"
    );
    let replaced = xml.replace("placeholder", &format!("before{ruby}after"));
    package.set_part(&main, replaced.into_bytes());

    let bytes = package.save().expect("saving the package");
    Document::open(&bytes).expect("reopening")
}

/// The ruby in the first paragraph, if there is one.
fn ruby_in(document: &Document) -> Option<Ruby> {
    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { return None };
    paragraph.runs.iter().find_map(|run| {
        run.content.iter().find_map(|piece| match piece {
            RunContent::Ruby(ruby) => Some((**ruby).clone()),
            _ => None,
        })
    })
}

#[test]
fn a_document_with_a_reading_in_it_keeps_it() {
    let ruby = ruby_in(&with_ruby()).expect("a ruby");
    assert_eq!(ruby.plain_text(), WORD);
    assert_eq!(ruby.reading(), READING);
    assert_eq!(ruby.properties.align, Align::DistributeSpace);
}

#[test]
fn the_word_is_the_text_and_the_reading_is_not() {
    // What a search finds, what a word count counts, and what the caret walks
    // through: the word. The reading is an annotation about it.
    let document = with_ruby();
    assert_eq!(document.paragraph_text(0).unwrap_or_default(), format!("before{WORD}after"));
}

#[test]
fn typing_beside_it_leaves_it_alone() {
    // The paragraph is rewritten from the model when it is edited, so this is
    // where a ruby would be lost if the writer could not write one.
    let mut document = with_ruby();
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.type_text("new "));

    let bytes = document.save().expect("saving");
    let read = Document::open(&bytes).expect("reopening");
    let ruby = ruby_in(&read).expect("the ruby survived the edit");
    assert_eq!(ruby.plain_text(), WORD);
    assert_eq!(ruby.reading(), READING);
    assert_eq!(read.paragraph_text(0).unwrap_or_default(), format!("new before{WORD}after"));
}

#[test]
fn the_caret_walks_the_word_and_not_the_reading() {
    // Two characters under the reading, however many the reading has: a caret
    // that counted the reading would stop in places the document has not got.
    let document = with_ruby();
    let text = document.paragraph_text(0).unwrap_or_default();
    assert_eq!(text.chars().count(), "before".len() + 2 + "after".len());
}

#[test]
fn a_ruby_made_here_is_written_the_way_word_writes_one() {
    let mut body = Body::default();
    let mut run = Run::default();
    run.content.push(RunContent::Ruby(Box::new(Ruby::over(WORD, READING, 21))));
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![run])));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");

    let ruby = ruby_in(&document).expect("a ruby");
    assert_eq!(ruby.plain_text(), WORD);
    assert_eq!(ruby.reading(), READING);
    assert_eq!(ruby.properties.base_size_half_points, Some(21));
}

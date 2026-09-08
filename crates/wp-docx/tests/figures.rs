//! The table of figures and the index.

use wp_docx::captions::Label;
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};

fn document(lines: &[&str]) -> Document {
    let mut body = Body::default();
    for line in lines {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// A document with two figure captions and one table caption.
fn captioned() -> Document {
    let mut document = document(&["", "A drawing.", "Another drawing.", "A table."]);
    document.set_caret(TextPosition::new(1, 0));
    document.add_caption(Label::Figure, "the map");
    document.set_caret(TextPosition::new(3, 0));
    document.add_caption(Label::Figure, "the chart");
    document.set_caret(TextPosition::new(5, 0));
    document.add_caption(Label::Table, "the figures");
    document
}

// --- Table of figures ---------------------------------------------------------

#[test]
fn a_document_has_no_table_of_figures_until_one_is_put_in() {
    assert!(!captioned().has_figures(Label::Figure));
}

#[test]
fn a_table_of_figures_gathers_the_captions_of_its_label() {
    let mut document = captioned();
    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.insert_figures(Label::Figure, &[]), 2);

    let text = round_trip(&document).plain_text();
    assert!(text.contains("Table of Figures"), "the heading is missing");
    assert!(text.contains("the map"));
    assert!(text.contains("the chart"));
}

#[test]
fn it_leaves_the_captions_of_other_labels_out() {
    let mut document = captioned();
    document.set_caret(TextPosition::new(0, 0));
    document.insert_figures(Label::Figure, &[]);

    let reopened = round_trip(&document);
    // "the figures" is a table caption, not a figure one; it should not be
    // gathered into the table of figures.
    let heading_at = reopened.plain_text().find("Table of Figures").expect("a heading");
    let gathered = &reopened.plain_text()[heading_at..];
    let before_body = gathered.find("A drawing").unwrap_or(gathered.len());
    assert!(!gathered[..before_body].contains("the figures"), "a table caption was gathered");
}

#[test]
fn updating_replaces_the_table_rather_than_adding_a_second() {
    let mut document = captioned();
    document.set_caret(TextPosition::new(0, 0));
    document.insert_figures(Label::Figure, &[]);
    let after_first = round_trip(&document).paragraph_count();

    document.set_caret(TextPosition::new(0, 0));
    document.insert_figures(Label::Figure, &[]);
    assert_eq!(round_trip(&document).paragraph_count(), after_first);
    assert_eq!(round_trip(&document).plain_text().matches("Table of Figures").count(), 1);
}

#[test]
fn a_table_of_figures_and_a_table_of_contents_do_not_replace_each_other() {
    let mut document = captioned();
    document.set_caret(TextPosition::new(0, 0));
    document.insert_figures(Label::Figure, &[]);
    document.set_caret(TextPosition::new(0, 0));
    document.insert_contents(3, &[]);

    let text = round_trip(&document).plain_text();
    assert!(text.contains("Table of Figures"), "the table of figures was replaced");
    assert!(text.contains("Contents"), "the table of contents is missing");
}

#[test]
fn page_numbers_are_used_when_they_are_known() {
    let mut document = captioned();
    document.set_caret(TextPosition::new(0, 0));
    // Caption paragraphs are 2 and 4 after the captions were added.
    let mut pages = vec![0usize; document.paragraph_count()];
    pages[2] = 3;
    pages[4] = 7;
    document.insert_figures(Label::Figure, &pages);

    let text = round_trip(&document).plain_text();
    assert!(text.contains("\t3"), "got {text:?}");
    assert!(text.contains("\t7"));
}

// --- Index --------------------------------------------------------------------

#[test]
fn a_document_starts_with_nothing_marked() {
    assert!(document(&["plain"]).index_entries(&[]).is_empty());
}

#[test]
fn a_word_can_be_marked_for_the_index() {
    let mut document = document(&["apples and pears"]);
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.mark_index_entry("apples"));

    let entries = round_trip(&document).index_entries(&[]);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text, "apples");
}

#[test]
fn a_mark_changes_not_one_character_of_the_text() {
    let mut document = document(&["apples and pears"]);
    document.set_caret(TextPosition::new(0, 7));
    document.mark_index_entry("apples");

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("apples and pears"));
    assert_eq!(reopened.plain_text(), "apples and pears");
}

#[test]
fn an_index_gathers_what_was_marked_under_letter_headings() {
    let mut document = document(&["", "apples and pears", "cider too"]);
    document.set_caret(TextPosition::new(1, 0));
    document.mark_index_entry("apples");
    document.set_caret(TextPosition::new(2, 0));
    document.mark_index_entry("cider");

    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.insert_index(&[]), 2);

    let text = round_trip(&document).plain_text();
    assert!(text.contains("Index"), "the heading is missing");
    assert!(text.contains("apples"));
    assert!(text.contains("cider"));
    assert!(text.contains("A"), "no letter heading");
}

#[test]
fn the_same_words_marked_twice_make_one_line_with_both_pages() {
    let mut document = document(&["", "apples here", "apples again"]);
    document.set_caret(TextPosition::new(1, 0));
    document.mark_index_entry("apples");
    document.set_caret(TextPosition::new(2, 0));
    document.mark_index_entry("apples");

    document.set_caret(TextPosition::new(0, 0));
    // Paragraph 1 is on page 2, paragraph 2 on page 5.
    assert_eq!(document.insert_index(&[0, 2, 5]), 1, "one line, not two");

    let text = round_trip(&document).plain_text();
    assert!(text.contains("2, 5"), "both pages should be listed: got {text:?}");
}

#[test]
fn entries_are_listed_in_alphabetical_order() {
    let mut document = document(&["", "one", "two"]);
    document.set_caret(TextPosition::new(2, 0));
    document.mark_index_entry("zebra");
    document.set_caret(TextPosition::new(1, 0));
    document.mark_index_entry("apple");

    document.set_caret(TextPosition::new(0, 0));
    document.insert_index(&[]);

    let text = round_trip(&document).plain_text();
    let apple = text.find("apple").expect("apple");
    let zebra = text.find("zebra").expect("zebra");
    assert!(apple < zebra, "the index is not in order");
}

#[test]
fn updating_the_index_replaces_it() {
    let mut document = document(&["", "apples"]);
    document.set_caret(TextPosition::new(1, 0));
    document.mark_index_entry("apples");
    document.set_caret(TextPosition::new(0, 0));
    document.insert_index(&[]);
    let after_first = round_trip(&document).paragraph_count();

    document.set_caret(TextPosition::new(0, 0));
    document.insert_index(&[]);
    assert_eq!(round_trip(&document).paragraph_count(), after_first);
}

#[test]
fn an_index_with_nothing_marked_says_so() {
    let mut document = document(&["", "plain"]);
    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.insert_index(&[]), 0);
    assert!(round_trip(&document).plain_text().contains("Nothing has been marked"));
}

#[test]
fn marking_can_be_undone() {
    let mut document = document(&["apples"]);
    document.set_caret(TextPosition::new(0, 0));
    document.mark_index_entry("apples");
    assert!(document.undo());
    assert!(document.index_entries(&[]).is_empty());
}

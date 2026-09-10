//! Copying and pasting inside a document, with the formatting kept.

use wp_docx::clipboard::Formatting;
use wp_docx::model::{Block, Body, Paragraph, Run, RunContent, RunProperties};
use wp_docx::{Document, TextPosition};

/// Two paragraphs, the first of them "plain bold after" with "bold" in bold.
fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        properties: wp_docx::model::ParagraphProperties::default(),
        runs: vec![
            Run::text("plain "),
            Run {
                properties: RunProperties { bold: Some(true), ..RunProperties::default() },
                content: vec![RunContent::Text("bold".to_owned())],
                field: None,
                revision: None,
            },
            Run::text(" after"),
        ],
    }));
    body.blocks.push(Block::Paragraph(Paragraph::text("second")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Selects a stretch of the first paragraph.
fn select(document: &mut Document, from: usize, to: usize) {
    document.set_caret(TextPosition::new(0, from));
    document.extend_selection_to(TextPosition::new(0, to));
}

#[test]
fn nothing_selected_copies_nothing() {
    assert!(document().copy_selection().is_empty());
}

#[test]
fn the_words_of_the_selection_are_what_is_copied() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();
    assert_eq!(copied.len(), 1);
    assert_eq!(copied[0].plain_text(), "plain");
}

#[test]
fn a_selection_over_several_paragraphs_copies_each_of_them() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 6));
    document.extend_selection_to(TextPosition::new(1, 6));
    let copied = document.copy_selection();
    assert_eq!(copied.len(), 2, "{copied:?}");
    assert_eq!(copied[0].plain_text(), "bold after");
    assert_eq!(copied[1].plain_text(), "second");
}

#[test]
fn pasting_puts_the_words_where_the_caret_is() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    assert!(document.paste_blocks(&copied));
    assert_eq!(round_trip(&document).plain_text(), "plain bold after\nsecondplain");
}

#[test]
fn what_was_bold_is_bold_where_it_lands() {
    let mut document = document();
    // Just the bold word.
    select(&mut document, 6, 10);
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    let mut reopened = round_trip(&document);
    // Inside what was pasted, which now ends the second paragraph.
    reopened.set_caret(TextPosition::new(1, 8));
    assert!(
        reopened.format_is_on(wp_docx::CharacterFormat::Bold),
        "the formatting was lost: {:?}",
        reopened.body().blocks[1]
    );
}

#[test]
fn plain_text_stays_plain_where_it_lands() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(1, 8));
    assert!(!reopened.format_is_on(wp_docx::CharacterFormat::Bold));
}

#[test]
fn pasting_several_paragraphs_makes_several_paragraphs() {
    let mut document = document();
    document.select_all();
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_count(), 3, "{}", reopened.plain_text());
}

#[test]
fn pasting_over_a_selection_replaces_it() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();

    // Over the whole of the second paragraph.
    document.set_caret(TextPosition::new(1, 0));
    document.extend_selection_to(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    assert_eq!(round_trip(&document).plain_text(), "plain bold after\nplain");
}

#[test]
fn pasting_nothing_changes_nothing() {
    let mut document = document();
    assert!(!document.paste_blocks(&[]));
}

#[test]
fn a_paste_is_one_thing_to_undo() {
    let mut document = document();
    document.select_all();
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);
    assert!(document.undo());
    assert_eq!(document.plain_text(), "plain bold after\nsecond");
}

#[test]
fn the_style_of_a_paragraph_comes_across_with_it() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        properties: wp_docx::model::ParagraphProperties {
            style: Some("Heading1".to_owned()),
            ..wp_docx::model::ParagraphProperties::default()
        },
        runs: vec![Run::text("A heading")],
    }));
    body.blocks.push(Block::Paragraph(Paragraph::text("body")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    select(&mut document, 0, 9);
    let copied = document.copy_selection();

    let Block::Paragraph(first) = &copied[0] else { panic!("a paragraph") };
    assert_eq!(first.properties.style.as_deref(), Some("Heading1"));
}

// --- The paste options ------------------------------------------------------

/// A document whose first paragraph is a centred heading in twenty-eight point
/// red Georgia with a bold word in it, and whose second is ordinary text.
fn heading_and_body() -> Document {
    let large = |text: &str, bold: bool| Run {
        properties: RunProperties {
            bold: bold.then_some(true),
            font: Some("Georgia".to_owned()),
            size_half_points: Some(56),
            color: Some("FF0000".to_owned()),
            ..RunProperties::default()
        },
        content: vec![RunContent::Text(text.to_owned())],
        field: None,
        revision: None,
    };

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        properties: wp_docx::model::ParagraphProperties {
            style: Some("Heading1".to_owned()),
            alignment: Some(wp_docx::model::Alignment::Center),
            ..wp_docx::model::ParagraphProperties::default()
        },
        runs: vec![large("Large ", false), large("bold", true)],
    }));
    body.blocks.push(Block::Paragraph(Paragraph::text("ordinary")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Copies the whole of the heading.
fn copy_the_heading(document: &mut Document) -> Vec<Block> {
    select(document, 0, "Large bold".len());
    document.copy_selection()
}

/// One paragraph of a document, read back from what was written.
fn paragraph_of(document: &Document, index: usize) -> Paragraph {
    let body = document.body();
    let Some(Block::Paragraph(paragraph)) = body.blocks.get(index) else {
        panic!("no paragraph {index}");
    };
    paragraph.clone()
}

/// The run of a paragraph that holds a word.
fn run_holding(paragraph: &Paragraph, word: &str) -> Run {
    paragraph
        .runs
        .iter()
        .find(|run| run.plain_text().contains(word))
        .unwrap_or_else(|| panic!("no run holding {word:?} in {:?}", paragraph.plain_text()))
        .clone()
}

#[test]
fn keeping_the_source_formatting_keeps_the_size_and_the_colour() {
    let mut document = heading_and_body();
    let copied = copy_the_heading(&mut document);

    // Into the end of the ordinary paragraph.
    document.set_caret(TextPosition::new(1, "ordinary".len()));
    assert!(document.paste_blocks_as(&copied, Formatting::Source));

    let document = round_trip(&document);
    let paragraph = paragraph_of(&document, 1);
    assert_eq!(paragraph.plain_text(), "ordinaryLarge bold");

    let large = run_holding(&paragraph, "Large");
    assert_eq!(large.properties.size_half_points, Some(56), "the size did not come across");
    assert_eq!(large.properties.color.as_deref(), Some("FF0000"));
    assert_eq!(large.properties.font.as_deref(), Some("Georgia"));
}

#[test]
fn merging_the_formatting_keeps_the_emphasis_and_nothing_else() {
    let mut document = heading_and_body();
    let copied = copy_the_heading(&mut document);

    document.set_caret(TextPosition::new(1, "ordinary".len()));
    assert!(document.paste_blocks_as(&copied, Formatting::Merged));

    let document = round_trip(&document);
    let paragraph = paragraph_of(&document, 1);

    // The size, the colour and the font are the destination's now.
    let large = run_holding(&paragraph, "Large");
    assert_eq!(large.properties.size_half_points, None, "the size came with it");
    assert_eq!(large.properties.color, None, "the colour came with it");
    assert_eq!(large.properties.font, None, "the font came with it");

    // But a bold word is still bold: that is the whole point of merging rather
    // than pasting the words alone.
    let bold = run_holding(&paragraph, "bold");
    assert_eq!(bold.properties.bold, Some(true));
    assert_eq!(bold.properties.size_half_points, None);
}

#[test]
fn an_empty_paragraph_takes_the_shape_of_what_is_pasted_into_it() {
    // Word's rule: there is nothing there to disagree with the pasted
    // paragraph, so the pasted paragraph's own shape wins.
    let mut document = heading_and_body();
    let copied = copy_the_heading(&mut document);

    document.set_caret(TextPosition::new(1, 0));
    document.extend_selection_to(TextPosition::new(1, "ordinary".len()));
    document.delete_selection();
    assert!(document.paste_blocks_as(&copied, Formatting::Source));

    let document = round_trip(&document);
    let paragraph = paragraph_of(&document, 1);
    assert_eq!(paragraph.properties.style.as_deref(), Some("Heading1"));
    assert_eq!(paragraph.properties.alignment, Some(wp_docx::model::Alignment::Center));
}

#[test]
fn a_paragraph_with_words_in_it_keeps_its_own_shape() {
    let mut document = heading_and_body();
    let copied = copy_the_heading(&mut document);

    document.set_caret(TextPosition::new(1, "ordinary".len()));
    assert!(document.paste_blocks_as(&copied, Formatting::Source));

    let document = round_trip(&document);
    let paragraph = paragraph_of(&document, 1);
    assert_eq!(paragraph.properties.style, None, "the paragraph became a heading");
    assert_eq!(paragraph.properties.alignment, None, "the paragraph was centred");
}

#[test]
fn merging_never_takes_the_shape_of_what_is_pasted() {
    let mut document = heading_and_body();
    let copied = copy_the_heading(&mut document);

    // Even into an empty paragraph, which is where Keep Source would.
    document.set_caret(TextPosition::new(1, 0));
    document.extend_selection_to(TextPosition::new(1, "ordinary".len()));
    document.delete_selection();
    assert!(document.paste_blocks_as(&copied, Formatting::Merged));

    let document = round_trip(&document);
    assert_eq!(paragraph_of(&document, 1).properties.style, None, "merging made it a heading");
}

#[test]
fn a_paragraph_made_by_the_paste_carries_the_shape_it_was_copied_with() {
    // Two paragraphs copied and pasted at the end of the document: the second
    // of them was made by this paste, so it is the source's shape throughout.
    let mut document = heading_and_body();
    document.select_all();
    let copied = document.copy_selection();
    assert_eq!(copied.len(), 2);

    document.set_caret(TextPosition::new(1, "ordinary".len()));
    assert!(document.paste_blocks_as(&copied, Formatting::Source));

    let document = round_trip(&document);
    assert_eq!(paragraph_of(&document, 1).plain_text(), "ordinaryLarge bold");
    // The paragraph the paste made carries the second copied paragraph's
    // shape, which is no style at all.
    assert_eq!(paragraph_of(&document, 2).plain_text(), "ordinary");
    assert_eq!(paragraph_of(&document, 2).properties.style, None);
}

#[test]
fn either_way_of_pasting_is_one_thing_to_undo() {
    for formatting in [Formatting::Source, Formatting::Merged] {
        let mut document = heading_and_body();
        let before = document.plain_text();
        let copied = copy_the_heading(&mut document);

        document.set_caret(TextPosition::new(1, "ordinary".len()));
        document.paste_blocks_as(&copied, formatting);
        assert!(document.undo(), "{formatting:?} could not be taken back");
        assert_eq!(document.plain_text(), before, "{formatting:?} left something behind");
    }
}

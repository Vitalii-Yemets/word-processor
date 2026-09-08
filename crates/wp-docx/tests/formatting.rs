//! Tests for formatting applied from the keyboard.
//!
//! Two questions run through all of them. Did the right characters change — and
//! did everything else survive? Making half a word bold means cutting a run in
//! two, and a run cut in two must come out carrying everything it carried
//! before: its colour, its font, its language, its whole identity.

use wp_docx::model::{Alignment, Block, Body, BreakKind, Paragraph, Run, RunContent, Underline};
use wp_docx::{CharacterFormat, Document, TextPosition};

fn open(body: &Body) -> Document {
    let bytes = Document::create(body).unwrap().save().unwrap();
    Document::open(&bytes).unwrap()
}

fn document_with(lines: &[&str]) -> Document {
    let mut body = Body::default();
    for line in lines {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }
    open(&body)
}

/// The runs of one paragraph as text and resolved formatting.
fn runs_of(document: &Document, index: usize) -> Vec<(String, bool, bool, bool)> {
    let body = document.body();
    let Block::Paragraph(paragraph) = &body.blocks[index] else {
        panic!("block {index} is not a paragraph")
    };
    paragraph
        .runs
        .iter()
        .map(|run| {
            let resolved = document.resolve_run(paragraph, run);
            (run.plain_text(), resolved.bold, resolved.italic, resolved.underline.is_visible())
        })
        .collect()
}

/// Just the text and boldness, which is what most of these tests ask about.
fn bold_map(document: &Document, index: usize) -> Vec<(String, bool)> {
    runs_of(document, index).into_iter().map(|(text, bold, _, _)| (text, bold)).collect()
}

fn select(document: &mut Document, from: (usize, usize), to: (usize, usize)) {
    document.set_caret(TextPosition::new(from.0, from.1));
    document.extend_selection_to(TextPosition::new(to.0, to.1));
}

// --- Applying a format ------------------------------------------------------

#[test]
fn bolding_part_of_a_run_splits_it_in_three() {
    let mut document = document_with(&["one two three"]);
    select(&mut document, (0, 4), (0, 7));

    assert!(document.toggle_format(CharacterFormat::Bold));
    assert_eq!(
        bold_map(&document, 0),
        vec![("one ".to_owned(), false), ("two".to_owned(), true), (" three".to_owned(), false),]
    );
}

#[test]
fn formatting_never_changes_the_text() {
    let mut document = document_with(&["the quick brown fox"]);
    let before = document.plain_text();
    select(&mut document, (0, 4), (0, 9));
    document.toggle_format(CharacterFormat::Bold);

    assert_eq!(document.plain_text(), before);
}

#[test]
fn bolding_from_the_start_of_a_paragraph() {
    let mut document = document_with(&["start here"]);
    select(&mut document, (0, 0), (0, 5));
    document.toggle_format(CharacterFormat::Bold);

    assert_eq!(
        bold_map(&document, 0),
        vec![("start".to_owned(), true), (" here".to_owned(), false)]
    );
}

#[test]
fn bolding_to_the_end_of_a_paragraph() {
    let mut document = document_with(&["start here"]);
    select(&mut document, (0, 6), (0, 10));
    document.toggle_format(CharacterFormat::Bold);

    assert_eq!(
        bold_map(&document, 0),
        vec![("start ".to_owned(), false), ("here".to_owned(), true)]
    );
}

#[test]
fn a_whole_paragraph_can_be_formatted_without_splitting_anything() {
    let mut document = document_with(&["all of it"]);
    document.select_all();
    document.toggle_format(CharacterFormat::Italic);

    let runs = runs_of(&document, 0);
    assert_eq!(runs.len(), 1, "there was nothing to split");
    assert!(runs[0].2, "and it should be italic");
}

#[test]
fn formatting_reaches_across_paragraphs() {
    let mut document = document_with(&["first line", "second line", "third line"]);
    select(&mut document, (0, 6), (2, 5));
    document.toggle_format(CharacterFormat::Bold);

    assert_eq!(
        bold_map(&document, 0),
        vec![("first ".to_owned(), false), ("line".to_owned(), true)]
    );
    assert_eq!(bold_map(&document, 1), vec![("second line".to_owned(), true)]);
    assert_eq!(
        bold_map(&document, 2),
        vec![("third".to_owned(), true), (" line".to_owned(), false)]
    );
}

#[test]
fn each_format_is_independent_of_the_others() {
    let mut document = document_with(&["word"]);
    document.select_all();
    document.toggle_format(CharacterFormat::Bold);
    document.select_all();
    document.toggle_format(CharacterFormat::Italic);
    document.select_all();
    document.toggle_format(CharacterFormat::Underline);

    let runs = runs_of(&document, 0);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0], ("word".to_owned(), true, true, true));
}

#[test]
fn a_split_run_keeps_everything_it_was() {
    // The whole risk of splitting: a run cut in two must not lose its identity.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run::text("coloured text")
        .colored("FF0000")
        .sized(18.0)
        .in_language("fr-FR")])));
    let mut document = open(&body);

    select(&mut document, (0, 0), (0, 8));
    document.toggle_format(CharacterFormat::Bold);

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let body = reopened.body();
    let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!() };

    assert_eq!(paragraph.runs.len(), 2, "should be exactly two halves");
    for run in &paragraph.runs {
        assert_eq!(run.properties.color.as_deref(), Some("FF0000"));
        assert_eq!(run.properties.size_half_points, Some(36));
        assert_eq!(run.properties.language.as_deref(), Some("fr-FR"));
    }
    assert_eq!(paragraph.runs[0].properties.bold, Some(true));
    assert_ne!(paragraph.runs[1].properties.bold, Some(true));
}

#[test]
fn a_line_break_in_a_split_run_is_neither_lost_nor_doubled() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run {
        content: vec![
            RunContent::Text("before".to_owned()),
            RunContent::Break(BreakKind::Line),
            RunContent::Text("after".to_owned()),
        ],
        ..Run::default()
    }])));
    let mut document = open(&body);

    // A split at the break, which is where duplication would show.
    select(&mut document, (0, 0), (0, 6));
    document.toggle_format(CharacterFormat::Bold);

    let body = document.body();
    let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!() };
    let breaks = paragraph
        .runs
        .iter()
        .flat_map(|run| run.content.iter())
        .filter(|piece| matches!(piece, RunContent::Break(_)))
        .count();

    assert_eq!(breaks, 1, "the break should still be there exactly once");
    // Reading text out renders the break as a line ending; the caret model does
    // not count it, which is why the split above used offset 6 and not 7.
    assert_eq!(paragraph.plain_text(), "before\nafter");
}

#[test]
fn formatting_the_same_range_twice_does_not_keep_splitting_runs() {
    let mut document = document_with(&["one two three"]);
    select(&mut document, (0, 4), (0, 7));
    document.toggle_format(CharacterFormat::Bold);
    let after_first = runs_of(&document, 0).len();

    select(&mut document, (0, 4), (0, 7));
    document.toggle_format(CharacterFormat::Italic);

    assert_eq!(runs_of(&document, 0).len(), after_first, "no new runs were needed");
}

#[test]
fn formatting_a_multibyte_word_cuts_on_character_boundaries() {
    let mut document = document_with(&["Привет мир"]);
    // "Привет " is thirteen bytes: six two-byte letters and a space.
    select(&mut document, (0, 13), (0, 19));
    document.toggle_format(CharacterFormat::Bold);

    assert_eq!(
        bold_map(&document, 0),
        vec![("Привет ".to_owned(), false), ("мир".to_owned(), true)]
    );
}

// --- Reading the current state ----------------------------------------------

#[test]
fn a_plain_selection_is_not_bold() {
    let mut document = document_with(&["plain text"]);
    document.select_all();
    assert!(!document.format_is_on(CharacterFormat::Bold));
}

#[test]
fn a_partly_bold_selection_reads_as_not_bold() {
    // So that pressing Ctrl+B over it makes all of it bold, rather than
    // swapping the two halves over.
    let mut document = document_with(&["one two"]);
    select(&mut document, (0, 0), (0, 3));
    document.toggle_format(CharacterFormat::Bold);

    document.select_all();
    assert!(!document.format_is_on(CharacterFormat::Bold));

    document.toggle_format(CharacterFormat::Bold);
    assert!(bold_map(&document, 0).iter().all(|(_, bold)| *bold), "all of it now");
}

#[test]
fn a_fully_bold_selection_reads_as_bold() {
    let mut document = document_with(&["every word"]);
    document.select_all();
    document.toggle_format(CharacterFormat::Bold);

    document.select_all();
    assert!(document.format_is_on(CharacterFormat::Bold));
}

#[test]
fn bold_inherited_from_a_style_is_still_bold() {
    // The run says nothing about weight; the paragraph style says everything.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("A heading").with_style("Heading1")));
    let mut document = open(&body);

    document.select_all();
    assert!(document.format_is_on(CharacterFormat::Bold), "Heading 1 is bold");
}

#[test]
fn turning_off_bold_that_came_from_a_style_overrides_it() {
    // Saying nothing about bold would inherit the heading's bold straight back,
    // so switching it off has to be written down.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("A heading").with_style("Heading1")));
    let mut document = open(&body);

    document.select_all();
    document.toggle_format(CharacterFormat::Bold);

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let body = reopened.body();
    let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!() };

    assert_eq!(paragraph.runs[0].properties.bold, Some(false), "written off, not left out");
    assert!(!reopened.resolve_run(paragraph, &paragraph.runs[0]).bold);
}

#[test]
fn underline_is_read_as_on_only_when_something_is_drawn() {
    let mut document = document_with(&["text"]);
    document.select_all();
    assert!(!document.format_is_on(CharacterFormat::Underline));

    document.toggle_format(CharacterFormat::Underline);
    document.select_all();
    assert!(document.format_is_on(CharacterFormat::Underline));

    document.toggle_format(CharacterFormat::Underline);
    let body = document.body();
    let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!() };
    assert_eq!(paragraph.runs[0].properties.underline, Some(Underline::None));
}

// --- Formatting before typing -----------------------------------------------

#[test]
fn bold_chosen_before_typing_applies_to_what_is_typed() {
    let mut document = document_with(&["plain "]);
    document.set_caret(TextPosition::new(0, 6));

    assert!(document.set_format(CharacterFormat::Bold, true));
    for character in "bold".chars() {
        document.type_text(&character.to_string());
    }

    assert_eq!(
        bold_map(&document, 0),
        vec![("plain ".to_owned(), false), ("bold".to_owned(), true)]
    );
}

#[test]
fn what_is_chosen_before_typing_is_reported_as_on() {
    let mut document = document_with(&["text"]);
    document.set_caret(TextPosition::new(0, 4));
    document.set_format(CharacterFormat::Italic, true);

    assert!(document.format_is_on(CharacterFormat::Italic), "the next word will be italic");
}

#[test]
fn moving_the_caret_forgets_formatting_chosen_for_a_place_now_left() {
    let mut document = document_with(&["one two"]);
    document.set_caret(TextPosition::new(0, 3));
    document.set_format(CharacterFormat::Bold, true);

    document.set_caret(TextPosition::new(0, 7));
    assert!(!document.format_is_on(CharacterFormat::Bold));

    document.type_text("!");
    assert!(bold_map(&document, 0).iter().all(|(_, bold)| !*bold));
}

#[test]
fn typing_at_the_end_of_a_bold_word_stays_bold() {
    let mut document = document_with(&["word"]);
    document.select_all();
    document.toggle_format(CharacterFormat::Bold);

    document.set_caret(TextPosition::new(0, 4));
    assert!(document.format_is_on(CharacterFormat::Bold), "the caret is in bold text");

    document.type_text("s");
    assert_eq!(bold_map(&document, 0), vec![("words".to_owned(), true)]);
}

#[test]
fn choosing_a_format_and_changing_your_mind_leaves_no_trace() {
    // Ctrl+B and then Ctrl+B again is back to where it started, so the text
    // typed afterwards should say nothing about weight at all.
    let mut document = document_with(&["plain "]);
    document.set_caret(TextPosition::new(0, 6));
    document.toggle_format(CharacterFormat::Bold);
    document.toggle_format(CharacterFormat::Bold);
    assert!(!document.format_is_on(CharacterFormat::Bold));

    document.type_text("more");

    let body = document.body();
    let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!() };
    assert_eq!(paragraph.runs.len(), 1, "no run was split off");
    assert_eq!(paragraph.runs[0].properties.bold, None, "nothing was written down");
    assert_eq!(paragraph.plain_text(), "plain more");
}

#[test]
fn choosing_bold_where_the_text_is_already_bold_writes_nothing() {
    let mut document = document_with(&["word"]);
    document.select_all();
    document.toggle_format(CharacterFormat::Bold);

    document.set_caret(TextPosition::new(0, 4));
    document.set_format(CharacterFormat::Bold, true);
    document.type_text("s");

    let runs = runs_of(&document, 0);
    assert_eq!(runs.len(), 1, "asking for what is already there splits nothing");
    assert_eq!(runs[0].0, "words");
}

#[test]
fn an_empty_heading_reports_the_formatting_its_style_gives_it() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("").with_style("Heading1")));
    let document = open(&body);

    assert!(document.format_is_on(CharacterFormat::Bold), "before a letter is typed");
}

// --- Paragraph formatting ---------------------------------------------------

#[test]
fn a_style_can_be_applied_to_the_paragraph_at_the_caret() {
    let mut document = document_with(&["not a heading yet"]);
    document.set_caret(TextPosition::new(0, 4));

    assert!(document.set_paragraph_style_here(Some("Heading1")));
    assert_eq!(document.style_here().as_deref(), Some("Heading1"));
}

#[test]
fn a_style_reaches_every_paragraph_the_selection_touches() {
    let mut document = document_with(&["one", "two", "three"]);
    select(&mut document, (0, 1), (2, 1));
    document.set_paragraph_style_here(Some("Heading2"));

    let body = document.body();
    for block in &body.blocks {
        let Block::Paragraph(paragraph) = block else { panic!() };
        assert_eq!(paragraph.style(), Some("Heading2"));
    }
}

#[test]
fn a_style_can_be_taken_off_again() {
    let mut document = document_with(&["text"]);
    document.set_paragraph_style_here(Some("Heading1"));
    document.set_paragraph_style_here(None);

    assert_eq!(document.style_here(), None);
}

#[test]
fn alignment_can_be_set_and_read_back() {
    let mut document = document_with(&["centre me"]);
    assert_eq!(document.alignment_here(), Alignment::Start);

    assert!(document.set_alignment_here(Alignment::Center));
    assert_eq!(document.alignment_here(), Alignment::Center);

    document.set_alignment_here(Alignment::Both);
    assert_eq!(document.alignment_here(), Alignment::Both);
}

#[test]
fn setting_a_style_does_not_disturb_the_alignment_beside_it() {
    // Both live in w:pPr, and the schema demands an order there.
    let mut document = document_with(&["text"]);
    document.set_alignment_here(Alignment::Center);
    document.set_paragraph_style_here(Some("Heading1"));

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    assert_eq!(reopened.style_here().as_deref(), Some("Heading1"));
    assert_eq!(reopened.alignment_here(), Alignment::Center);
}

// --- Undo -------------------------------------------------------------------

#[test]
fn formatting_is_one_undo_step() {
    let mut document = document_with(&["one two three"]);
    select(&mut document, (0, 4), (0, 7));
    document.toggle_format(CharacterFormat::Bold);

    assert!(document.undo());
    assert!(bold_map(&document, 0).iter().all(|(_, bold)| !*bold));
    assert_eq!(runs_of(&document, 0).len(), 1, "the split should be taken back too");
}

#[test]
fn undoing_formatting_returns_the_original_bytes() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("first")));
    body.blocks.push(Block::Paragraph(Paragraph::text("second")));
    let original = Document::create(&body).unwrap().save().unwrap();

    let mut document = Document::open(&original).unwrap();
    select(&mut document, (0, 1), (1, 3));
    document.toggle_format(CharacterFormat::Bold);
    document.set_alignment_here(Alignment::Center);
    document.set_paragraph_style_here(Some("Heading1"));

    while document.undo() {}

    assert!(!document.is_modified());
    assert_eq!(document.save().unwrap(), original);
}

#[test]
fn formatting_empty_paragraphs_leaves_nothing_to_undo() {
    // There are no runs to change, so there must be no step to take back.
    let mut document = document_with(&["", ""]);
    select(&mut document, (0, 0), (1, 0));

    assert!(!document.set_format(CharacterFormat::Bold, true));
    assert!(!document.can_undo());
    assert!(!document.is_modified());
}

#[test]
fn bolding_text_that_is_already_bold_leaves_nothing_to_undo() {
    let mut document = document_with(&["word"]);
    document.select_all();
    document.set_format(CharacterFormat::Bold, true);
    let depth = document.undo_depth();

    document.select_all();
    assert!(!document.set_format(CharacterFormat::Bold, true), "nothing changed");
    assert_eq!(document.undo_depth(), depth, "so nothing was recorded");
}

// --- Round trip -------------------------------------------------------------

#[test]
fn formatting_survives_being_saved_and_reopened() {
    let mut document = document_with(&["the quick brown fox"]);
    select(&mut document, (0, 4), (0, 9));
    document.toggle_format(CharacterFormat::Bold);
    select(&mut document, (0, 10), (0, 15));
    document.toggle_format(CharacterFormat::Italic);

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();

    assert_eq!(reopened.plain_text(), "the quick brown fox");
    let runs = runs_of(&reopened, 0);
    let bold: Vec<&String> = runs.iter().filter(|(_, b, _, _)| *b).map(|(t, ..)| t).collect();
    let italic: Vec<&String> = runs.iter().filter(|(_, _, i, _)| *i).map(|(t, ..)| t).collect();
    assert_eq!(bold, vec!["quick"]);
    assert_eq!(italic, vec!["brown"]);
}

#[test]
fn an_edit_after_formatting_still_writes_a_readable_document() {
    let mut document = document_with(&["one two three"]);
    select(&mut document, (0, 4), (0, 7));
    document.toggle_format(CharacterFormat::Bold);

    // Typing at the end of the bold run stays inside it, so the "!" is bold.
    document.set_caret(TextPosition::new(0, 7));
    document.type_text("!");
    document.press_enter();
    document.type_text("more");

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    assert_eq!(reopened.plain_text(), "one two!\nmore three");
    assert_eq!(bold_map(&reopened, 0), vec![("one ".to_owned(), false), ("two!".to_owned(), true)]);
}

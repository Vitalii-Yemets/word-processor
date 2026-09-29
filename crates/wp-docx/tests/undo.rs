//! Tests for the caret, the selection, and undo.
//!
//! Undo is the feature a person has to be able to trust without thinking. Every
//! test here asks the same thing: after taking a change back, is the document
//! exactly what it was — the text, the formatting, and the caret alike?

use wp_docx::model::{Block, Body, Paragraph, Run};
use wp_docx::{Document, TextPosition};

fn document_with(lines: &[&str]) -> Document {
    let mut body = Body::default();
    for line in lines {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }
    let bytes = Document::create(&body).unwrap().save().unwrap();
    Document::open(&bytes).unwrap()
}

/// Types a string one character at a time, as a person does.
fn type_out(document: &mut Document, text: &str) {
    for character in text.chars() {
        document.type_text(&character.to_string());
    }
}

#[test]
fn a_new_document_has_nothing_to_undo() {
    let document = document_with(&["text"]);
    assert!(!document.can_undo());
    assert!(!document.can_redo());
}

#[test]
fn undo_takes_back_a_change_exactly() {
    let mut document = document_with(&["original"]);
    let before = document.plain_text();

    document.set_caret(TextPosition::new(0, 8));
    document.type_text(" more");
    assert_eq!(document.plain_text(), "original more");

    assert!(document.undo());
    assert_eq!(document.plain_text(), before);
    assert!(!document.is_modified(), "undoing back to the start is not a change");
}

#[test]
fn undo_puts_the_caret_back_too() {
    // Restoring the text but not the caret leaves it somewhere the user did not
    // leave it, which is worse than not undoing at all.
    let mut document = document_with(&["one", "two"]);
    document.set_caret(TextPosition::new(1, 3));
    document.type_text("!");
    assert_eq!(document.caret(), TextPosition::new(1, 4));

    document.undo();
    assert_eq!(document.caret(), TextPosition::new(1, 3));
}

#[test]
fn redo_puts_a_change_back() {
    let mut document = document_with(&["start"]);
    document.set_caret(TextPosition::new(0, 5));
    document.type_text("ed");

    document.undo();
    assert_eq!(document.plain_text(), "start");
    assert!(document.can_redo());

    assert!(document.redo());
    assert_eq!(document.plain_text(), "started");
    assert!(!document.can_redo());
}

#[test]
fn a_new_change_discards_what_could_be_redone() {
    // The redo described a future that no longer follows from the present.
    let mut document = document_with(&["base"]);
    document.set_caret(TextPosition::new(0, 4));
    document.type_text("X");
    document.undo();
    assert!(document.can_redo());

    document.type_text("Y");
    assert!(!document.can_redo());
    assert_eq!(document.plain_text(), "baseY");
}

#[test]
fn typing_a_word_is_one_undo_not_one_per_letter() {
    // One undo per keystroke is a typing replay, not undo.
    let mut document = document_with(&[""]);
    type_out(&mut document, "hello");

    assert_eq!(document.plain_text(), "hello");
    assert_eq!(document.undo_depth(), 1, "the word should be a single step");

    document.undo();
    assert_eq!(document.plain_text(), "");
}

#[test]
fn a_space_ends_the_undo_step() {
    // So undo takes back a word at a time, which is what a person means by it.
    let mut document = document_with(&[""]);
    type_out(&mut document, "one two");

    document.undo();
    assert_eq!(document.plain_text(), "one ", "the last word should go first");

    // The space closed the step it ended, so it goes with the word before it.
    document.undo();
    assert_eq!(document.plain_text(), "");
}

#[test]
fn moving_the_caret_ends_the_undo_step() {
    // Typing after moving somewhere else is a new thought, not a continuation.
    let mut document = document_with(&["ab"]);
    document.set_caret(TextPosition::new(0, 2));
    type_out(&mut document, "XY");
    document.set_caret(TextPosition::new(0, 0));
    type_out(&mut document, "Z");

    document.undo();
    assert_eq!(document.plain_text(), "abXY");
    document.undo();
    assert_eq!(document.plain_text(), "ab");
}

#[test]
fn repeated_backspaces_are_one_undo() {
    let mut document = document_with(&["abcdef"]);
    document.set_caret(TextPosition::new(0, 6));
    for _ in 0..3 {
        document.backspace();
    }
    assert_eq!(document.plain_text(), "abc");

    document.undo();
    assert_eq!(document.plain_text(), "abcdef");
}

#[test]
fn undo_restores_formatting_not_just_text() {
    // The tree comes back as it was, so a run that was cut in half by an edit is
    // whole again, with its formatting.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run::text("bold").bold()])));
    let bytes = Document::create(&body).unwrap().save().unwrap();
    let mut document = Document::open(&bytes).unwrap();

    document.set_caret(TextPosition::new(0, 2));
    document.press_enter();
    assert_eq!(document.paragraph_count(), 2);

    document.undo();
    assert_eq!(document.paragraph_count(), 1);

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else { panic!() };
    assert_eq!(paragraph.plain_text(), "bold");
    assert_eq!(paragraph.runs[0].properties.bold, Some(true));
}

#[test]
fn undoing_everything_returns_the_original_bytes() {
    // The strongest form of the promise: after taking back every change, saving
    // produces the file that was opened.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("first")));
    body.blocks.push(Block::Paragraph(Paragraph::text("second").with_style("Heading1")));
    let original = Document::create(&body).unwrap().save().unwrap();

    let mut document = Document::open(&original).unwrap();
    document.set_caret(TextPosition::new(0, 5));
    type_out(&mut document, " more text");
    document.press_enter();
    type_out(&mut document, "a new paragraph");
    document.backspace();
    document.set_caret(TextPosition::new(1, 0));
    document.delete_forward();

    while document.undo() {}

    assert_eq!(document.plain_text(), "first\nsecond");
    assert!(!document.is_modified());
    assert_eq!(document.save().unwrap(), original, "the file did not come back");
}

// --- Undo and saving ----------------------------------------------------------

/// The text of a file, as it would be read back from disk.
fn text_of(bytes: &[u8]) -> String {
    Document::open(bytes).unwrap().plain_text()
}

#[test]
fn undo_after_a_save_is_a_change_and_the_next_save_writes_it() {
    // The file on disk says "AB" and undo leaves "A" on the screen. That is a
    // document that differs from its file: it has to say so, and saving it has
    // to write "A" rather than hand back the package that still holds "AB".
    let mut document = document_with(&["A"]);
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("B");
    let written = document.save().unwrap();
    document.mark_saved().unwrap();
    assert_eq!(text_of(&written), "AB");
    assert!(!document.is_modified());

    assert!(document.undo());
    assert_eq!(document.plain_text(), "A");
    assert_eq!(text_of(&document.save().unwrap()), "A", "the save wrote the file as it was");
    assert!(document.is_modified(), "the file says AB and the screen says A");
}

#[test]
fn undo_past_the_save_and_redo_back_to_it_is_no_change() {
    // Whether the document has changed is where the history stands against
    // the save, in both directions: going past it is a change, and coming back
    // to it is none.
    let mut document = document_with(&["A"]);
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("B");
    document.save().unwrap();
    document.mark_saved().unwrap();

    assert!(document.undo());
    assert!(document.is_modified(), "undo went past what is on disk");
    assert!(document.redo());
    assert_eq!(document.plain_text(), "AB");
    assert!(!document.is_modified(), "redo came back to what is on disk");
    assert_eq!(text_of(&document.save().unwrap()), "AB");
}

#[test]
fn an_edit_after_a_save_taken_back_again_is_no_change() {
    // Saving ends the step being typed, so what was typed after it comes off
    // on its own and leaves exactly the saved document.
    let mut document = document_with(&["A"]);
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("B");
    document.save().unwrap();
    document.mark_saved().unwrap();

    document.type_text("C");
    assert!(document.is_modified());
    assert!(document.undo());
    assert_eq!(document.plain_text(), "AB");
    assert!(!document.is_modified(), "the document is the saved one again");
}

#[test]
fn a_change_said_to_be_there_stays_until_it_is_saved() {
    // A document recovered after a crash is not on disk, whatever its history
    // says; taking back what was typed into it does not put it there.
    let mut document = document_with(&["recovered"]);
    document.mark_modified();
    document.set_caret(TextPosition::new(0, 9));
    document.type_text("!");
    assert!(document.undo());
    assert!(document.is_modified(), "undo made a recovered document count as saved");

    document.save().unwrap();
    document.mark_saved().unwrap();
    assert!(!document.is_modified());
}

#[test]
fn a_change_said_to_be_there_is_not_taken_back_by_undo() {
    // Said after a step rather than before one, it still holds: undo takes
    // back what the history recorded, and this it did not.
    let mut document = document_with(&["saved"]);
    document.set_caret(TextPosition::new(0, 5));
    document.type_text("!");
    document.mark_modified();
    assert!(document.undo());
    assert!(document.is_modified(), "undo took back something it never had");
}

// --- Changes the history does not record ---------------------------------------

#[test]
fn an_edit_off_the_record_before_a_step_is_still_a_change_after_undo() {
    let mut document = document_with(&["A", "B"]);
    document.tree_mut().root.set_attribute("data-off-the-record", "yes");
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("X");
    assert!(document.undo());
    assert!(document.is_modified(), "the edit before the step is still there");
}

#[test]
fn an_edit_off_the_record_after_a_step_is_still_a_change_after_undo() {
    // The step keeps one paragraph, so undoing it leaves the edit made after
    // it where it is — and the document is not the one on disk.
    let mut document = document_with(&["A", "B"]);
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("X");
    document.tree_mut().root.set_attribute("data-off-the-record", "yes");
    assert!(document.undo());
    assert_eq!(document.plain_text(), "A\nB");
    assert!(document.is_modified(), "undo landed on unchanged with the edit still in the tree");
}

#[test]
fn a_part_written_off_the_record_is_still_a_change_after_undo() {
    // The zoom a document is kept at lives in its settings, and nothing takes
    // it back: after undo the settings are not the ones on disk.
    let mut document = document_with(&["A"]);
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("X");
    assert!(document.set_zoom_percent(150));
    assert!(document.undo());
    assert!(document.is_modified(), "undo landed on unchanged with the settings changed");
    let reopened = Document::open(&document.save().unwrap()).unwrap();
    assert_eq!(reopened.zoom_percent(), Some(150));
}

#[test]
fn replacing_text_throughout_is_one_step() {
    let mut document = document_with(&["one fish", "two fish"]);
    assert_eq!(document.replace_text("fish", "cats"), 2);
    assert_eq!(document.plain_text(), "one cats\ntwo cats");
    assert!(document.undo(), "there was nothing to undo");
    assert_eq!(document.plain_text(), "one fish\ntwo fish");
    assert!(!document.is_modified());
}

#[test]
fn a_paragraph_added_at_the_end_can_be_undone() {
    let mut document = document_with(&["first"]);
    assert!(document.append_paragraph(&Paragraph::text("added")));
    assert!(document.undo(), "there was nothing to undo");
    assert_eq!(document.plain_text(), "first");
    assert!(!document.is_modified());
}

#[test]
fn a_paragraph_styled_or_aligned_by_its_number_can_be_undone() {
    let mut document = document_with(&["first", "second"]);
    assert!(document.set_paragraph_style(1, Some("Heading1")));
    assert!(document.set_paragraph_alignment(0, wp_docx::model::Alignment::Center));
    assert!(document.undo(), "the alignment was not a step");
    assert!(document.undo(), "the style was not a step");
    assert_eq!(document.style_of(1), None);
    assert!(!document.is_modified());
}

#[test]
fn a_style_written_can_be_undone() {
    let mut document = document_with(&["text"]);
    let wanted = wp_docx::StyleDefinition {
        id: "Pullquote".to_owned(),
        name: "Pullquote".to_owned(),
        based_on: None,
        next: None,
        paragraph: Default::default(),
        run: Default::default(),
    };
    assert!(document.set_style(&wanted));
    assert!(document.styles().get("Pullquote").is_some());
    assert!(document.undo(), "there was nothing to undo");
    assert!(document.styles().get("Pullquote").is_none(), "the style is still there");
    assert!(!document.is_modified());
}

#[test]
fn the_default_font_set_can_be_undone() {
    let mut document = document_with(&["text"]);
    let bold = wp_docx::model::RunProperties { bold: Some(true), ..Default::default() };
    assert!(document.set_default_character_format(&bold));
    document.set_caret(TextPosition::new(0, 1));
    assert!(document.character_format_here().bold);
    assert!(document.undo(), "there was nothing to undo");
    assert!(!document.character_format_here().bold, "the default is still bold");
    assert!(!document.is_modified());
}

#[test]
fn hyphenation_switched_on_can_be_undone() {
    let mut document = document_with(&["text"]);
    assert!(document.set_automatic_hyphenation(true));
    assert!(document.undo(), "there was nothing to undo");
    assert!(!document.automatic_hyphenation());
    assert!(!document.is_modified());
}

#[test]
fn a_theme_chosen_can_be_undone() {
    use wp_docx::gallery::COLOR_SCHEMES;
    let mut document = document_with(&["text"]);
    document.set_theme(&COLOR_SCHEMES[1].theme()).unwrap();
    let mut document = Document::open(&document.save().unwrap()).unwrap();
    let before = document.theme();

    assert!(document.set_theme(&COLOR_SCHEMES[2].theme()).unwrap());
    assert!(document.undo(), "there was nothing to undo");
    assert_eq!(document.theme(), before);
    assert!(!document.is_modified());
}

// --- Selection --------------------------------------------------------------

#[test]
fn there_is_no_selection_until_one_is_made() {
    let mut document = document_with(&["text"]);
    assert_eq!(document.selection(), None);

    document.set_caret(TextPosition::new(0, 2));
    assert_eq!(document.selection(), None, "a caret is not a selection");
}

#[test]
fn extending_selects_between_the_two_ends() {
    let mut document = document_with(&["hello world"]);
    document.set_caret(TextPosition::new(0, 6));
    document.extend_selection_to(TextPosition::new(0, 11));

    assert_eq!(document.selection(), Some((TextPosition::new(0, 6), TextPosition::new(0, 11))));
    assert_eq!(document.selected_text(), "world");
}

#[test]
fn a_selection_made_backwards_reads_the_same() {
    let mut document = document_with(&["hello world"]);
    document.set_caret(TextPosition::new(0, 11));
    document.extend_selection_to(TextPosition::new(0, 6));

    assert_eq!(document.selected_text(), "world");
}

#[test]
fn a_selection_across_paragraphs_reads_with_line_breaks() {
    let mut document = document_with(&["first", "middle", "last"]);
    document.set_caret(TextPosition::new(0, 3));
    document.extend_selection_to(TextPosition::new(2, 2));

    assert_eq!(document.selected_text(), "st\nmiddle\nla");
}

#[test]
fn select_all_covers_the_document() {
    let mut document = document_with(&["one", "two", "three"]);
    document.select_all();

    assert_eq!(document.selected_text(), "one\ntwo\nthree");
}

#[test]
fn deleting_a_selection_inside_one_paragraph() {
    let mut document = document_with(&["abcdefgh"]);
    document.set_caret(TextPosition::new(0, 2));
    document.extend_selection_to(TextPosition::new(0, 6));

    assert!(document.delete_selection());
    assert_eq!(document.plain_text(), "abgh");
    assert_eq!(document.caret(), TextPosition::new(0, 2));
    assert_eq!(document.selection(), None);
}

#[test]
fn deleting_a_selection_across_paragraphs_joins_what_is_left() {
    let mut document = document_with(&["first", "middle", "last"]);
    document.set_caret(TextPosition::new(0, 3));
    document.extend_selection_to(TextPosition::new(2, 2));

    assert!(document.delete_selection());
    assert_eq!(document.plain_text(), "first");
    assert_eq!(document.paragraph_count(), 1, "the paragraphs between should be gone");
    assert_eq!(document.caret(), TextPosition::new(0, 3));
}

#[test]
fn typing_replaces_the_selection() {
    let mut document = document_with(&["hello world"]);
    document.set_caret(TextPosition::new(0, 6));
    document.extend_selection_to(TextPosition::new(0, 11));

    document.type_text("everyone");
    assert_eq!(document.plain_text(), "hello everyone");
    assert_eq!(document.selection(), None);
}

#[test]
fn backspace_removes_the_selection_rather_than_one_character() {
    let mut document = document_with(&["abcdef"]);
    document.set_caret(TextPosition::new(0, 1));
    document.extend_selection_to(TextPosition::new(0, 4));

    document.backspace();
    assert_eq!(document.plain_text(), "aef");
}

#[test]
fn replacing_a_selection_can_be_undone_in_one_step() {
    let mut document = document_with(&["hello world"]);
    document.set_caret(TextPosition::new(0, 6));
    document.extend_selection_to(TextPosition::new(0, 11));
    document.type_text("everyone");
    assert_eq!(document.plain_text(), "hello everyone");

    // Replacing is one change, so one undo brings the selected text back.
    assert!(document.undo());
    assert_eq!(document.plain_text(), "hello world");
}

#[test]
fn selecting_in_a_multibyte_language_never_splits_a_character() {
    let mut document = document_with(&["Привет мир"]);
    document.select_all();

    assert_eq!(document.selected_text(), "Привет мир");
    document.type_text("Здравствуйте");
    assert_eq!(document.plain_text(), "Здравствуйте");

    assert!(document.undo());
    assert_eq!(document.plain_text(), "Привет мир");
}

#[test]
fn the_caret_walks_character_by_character_across_paragraphs() {
    let mut document = document_with(&["Привет", "мир"]);
    document.set_caret(TextPosition::new(0, 0));

    // Six two-byte letters, then across the paragraph break.
    for _ in 0..6 {
        document.caret_right(false);
    }
    assert_eq!(document.caret(), TextPosition::new(0, 12));

    document.caret_right(false);
    assert_eq!(document.caret(), TextPosition::new(1, 0), "should cross into the next");

    document.caret_left(false);
    assert_eq!(document.caret(), TextPosition::new(0, 12), "and back again");
}

#[test]
fn the_caret_stops_at_the_ends_of_the_document() {
    let mut document = document_with(&["only"]);
    document.set_caret(TextPosition::new(0, 0));
    document.caret_left(false);
    assert_eq!(document.caret(), TextPosition::new(0, 0));

    document.set_caret(TextPosition::new(0, 4));
    document.caret_right(false);
    assert_eq!(document.caret(), TextPosition::new(0, 4));
}

#[test]
fn moving_with_shift_extends_the_selection() {
    let mut document = document_with(&["abcdef"]);
    document.set_caret(TextPosition::new(0, 1));
    for _ in 0..3 {
        document.caret_right(true);
    }

    assert_eq!(document.selected_text(), "bcd");
}

#[test]
fn a_selection_that_is_typed_over_still_saves_a_readable_document() {
    let mut document = document_with(&["one", "two", "three"]);
    document.set_caret(TextPosition::new(0, 1));
    document.extend_selection_to(TextPosition::new(2, 2));
    document.type_text("X");

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    assert_eq!(reopened.plain_text(), "oXree");
}

// --- Pasting ----------------------------------------------------------------

#[test]
fn pasting_puts_text_in_at_the_caret() {
    let mut document = document_with(&["hello world"]);
    document.set_caret(TextPosition::new(0, 5));

    assert!(document.paste(","));
    assert_eq!(document.plain_text(), "hello, world");
    assert_eq!(document.caret(), TextPosition::new(0, 6));
}

#[test]
fn pasting_line_breaks_makes_paragraphs() {
    let mut document = document_with(&["start end"]);
    document.set_caret(TextPosition::new(0, 6));

    document.paste("one\ntwo\n");
    assert_eq!(document.plain_text(), "start one\ntwo\nend");
    assert_eq!(document.paragraph_count(), 3);
}

#[test]
fn pasting_accepts_either_line_ending() {
    // Text copied from another program can carry carriage returns.
    let mut document = document_with(&[""]);
    document.paste("first\r\nsecond\rthird");

    assert_eq!(document.plain_text(), "first\nsecond\nthird");
}

#[test]
fn a_whole_paste_is_one_undo_however_many_paragraphs_it_makes() {
    let mut document = document_with(&["keep"]);
    document.set_caret(TextPosition::new(0, 4));
    document.paste("\na\nb\nc");
    assert_eq!(document.paragraph_count(), 4);

    assert!(document.undo());
    assert_eq!(document.plain_text(), "keep");
    assert_eq!(document.paragraph_count(), 1);
}

#[test]
fn pasting_replaces_the_selection() {
    let mut document = document_with(&["one", "two", "three"]);
    document.set_caret(TextPosition::new(0, 1));
    document.extend_selection_to(TextPosition::new(2, 2));

    document.paste("X");
    assert_eq!(document.plain_text(), "oXree");

    document.undo();
    assert_eq!(document.plain_text(), "one\ntwo\nthree");
}

#[test]
fn what_is_copied_is_what_comes_back_when_pasted() {
    // The round trip a person actually performs: select, copy, paste elsewhere.
    let mut document = document_with(&["alpha beta", "gamma"]);
    document.set_caret(TextPosition::new(0, 6));
    document.extend_selection_to(TextPosition::new(1, 5));
    let copied = document.selected_text();
    assert_eq!(copied, "beta\ngamma");

    document.set_caret(TextPosition::new(0, 0));
    document.paste(&copied);
    assert_eq!(document.plain_text(), "beta\ngammaalpha beta\ngamma");
}

#[test]
fn undo_does_not_grow_without_limit() {
    // A long session must not accumulate history for ever.
    let mut document = document_with(&[""]);
    for index in 0..400 {
        document.type_text(&format!("{index} "));
    }
    assert!(document.undo_depth() <= 200, "history should be bounded");

    // And what is still there works.
    assert!(document.undo());
}

// --- Tabs -------------------------------------------------------------------

#[test]
fn a_tab_is_one_character_the_caret_can_stand_either_side_of() {
    let mut document = document_with(&["ab"]);
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("\t");

    assert_eq!(document.plain_text(), "a\tb");
    assert_eq!(document.caret(), TextPosition::new(0, 2), "the caret is past the tab");

    document.caret_left(false);
    assert_eq!(document.caret(), TextPosition::new(0, 1), "and can step back before it");
}

#[test]
fn a_tab_is_stored_as_an_element_rather_than_a_character() {
    // Word stores a tab as w:tab. A tab character inside w:t is not the same
    // thing and does not lay out as a tab.
    let mut document = document_with(&["ab"]);
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("\t");

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else { panic!() };

    let tabs = paragraph
        .runs
        .iter()
        .flat_map(|run| run.content.iter())
        .filter(|piece| matches!(piece, wp_docx::model::RunContent::Tab))
        .count();
    assert_eq!(tabs, 1);
    assert_eq!(reopened.plain_text(), "a\tb");
}

#[test]
fn backspace_deletes_a_whole_tab() {
    let mut document = document_with(&["ab"]);
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("\t");

    document.backspace();
    assert_eq!(document.plain_text(), "ab");
    assert_eq!(document.caret(), TextPosition::new(0, 1));
}

#[test]
fn delete_forward_removes_a_tab_in_front_of_the_caret() {
    let mut document = document_with(&["ab"]);
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("\t");
    document.set_caret(TextPosition::new(0, 1));

    document.delete_forward();
    assert_eq!(document.plain_text(), "ab");
}

#[test]
fn a_tab_at_the_start_of_a_paragraph_stays_before_the_text() {
    let mut document = document_with(&["indented"]);
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("\t");

    assert_eq!(document.plain_text(), "\tindented");
}

#[test]
fn a_tab_at_the_end_of_a_paragraph_stays_after_the_text() {
    let mut document = document_with(&["trailing"]);
    document.set_caret(TextPosition::new(0, 8));
    document.type_text("\t");

    assert_eq!(document.plain_text(), "trailing\t");
}

#[test]
fn several_tabs_in_a_row_all_arrive() {
    let mut document = document_with(&[""]);
    for _ in 0..3 {
        document.type_text("\t");
    }
    document.type_text("after");

    assert_eq!(document.plain_text(), "\t\t\tafter");
    assert_eq!(document.caret(), TextPosition::new(0, 8));
}

#[test]
fn typing_beside_a_tab_lands_on_the_right_side_of_it() {
    let mut document = document_with(&[""]);
    document.type_text("\t");
    document.type_text("after");
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("before");

    assert_eq!(document.plain_text(), "before\tafter");
}

#[test]
fn a_selection_across_a_tab_reads_and_deletes_it() {
    let mut document = document_with(&["one"]);
    document.set_caret(TextPosition::new(0, 1));
    document.type_text("\t");
    assert_eq!(document.plain_text(), "o\tne");

    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 3));
    assert_eq!(document.selected_text(), "o\tn");

    document.delete_selection();
    assert_eq!(document.plain_text(), "e");
}

#[test]
fn pasting_text_with_tabs_makes_real_tabs() {
    let mut document = document_with(&[""]);
    document.paste("name\tvalue\nsecond\trow");

    assert_eq!(document.plain_text(), "name\tvalue\nsecond\trow");
    assert_eq!(document.paragraph_count(), 2);

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let tabs = reopened
        .body()
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            Block::Table(_) => None,
        })
        .flat_map(|paragraph| paragraph.runs.iter())
        .flat_map(|run| run.content.iter())
        .filter(|piece| matches!(piece, wp_docx::model::RunContent::Tab))
        .count();
    assert_eq!(tabs, 2);
}

#[test]
fn a_tab_can_be_undone() {
    let mut document = document_with(&["text"]);
    document.set_caret(TextPosition::new(0, 2));
    document.type_text("\t");
    assert_eq!(document.plain_text(), "te\txt");

    assert!(document.undo());
    assert_eq!(document.plain_text(), "text");
}

#[test]
fn a_tab_inside_a_formatted_run_keeps_that_run_together() {
    // The tab goes inside the run, so it takes the run's formatting rather than
    // cutting the word into unrelated pieces.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run::text("bolded").bold()])));
    let bytes = Document::create(&body).unwrap().save().unwrap();
    let mut document = Document::open(&bytes).unwrap();

    document.set_caret(TextPosition::new(0, 3));
    document.type_text("\t");

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else { panic!() };

    assert_eq!(paragraph.plain_text(), "bol\tded");
    assert_eq!(paragraph.runs.len(), 1, "one run, with the tab inside it");
    assert_eq!(paragraph.runs[0].properties.bold, Some(true));
}

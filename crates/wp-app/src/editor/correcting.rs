//! Where AutoCorrect meets the document.
//!
//! [`crate::autocorrect`] decides what should be corrected; this puts the
//! correction in. The two are apart because the deciding is a question about
//! text and nothing else — which is what makes it testable a word at a time —
//! and the putting in is a question about a document with a caret in it.
//!
//! # When a correction happens
//!
//! A character that ends a word — a space, a full stop, a comma, Enter — is the
//! moment Word looks at the word just finished, and so is this. A quote is
//! different: which way it curls is known the instant it is typed, so it is
//! changed before it goes in rather than afterwards.
//!
//! # Why it is its own step to undo
//!
//! Because a correction is the program's doing and not the person's, and the
//! first thing anybody does when it is wrong is press Ctrl+Z. One press has to
//! take back the correction and leave what was actually typed; a second takes
//! back the typing. That is what Word does, and it is why the correction is
//! wrapped in a gesture of its own rather than folded into the keystroke.

use wp_docx::TextPosition;

use super::Editor;

impl Editor {
    /// What a typed character should be instead, if it should be something
    /// else.
    pub(super) fn correct_character(&mut self, typed: char) -> char {
        if self.document.selection().is_some() {
            // Typing over a selection replaces it; what was in front of the
            // selection is not what the quote would curl against.
            return typed;
        }
        let before = self.text_before_caret();
        self.autocorrect.on_character(&before, typed).unwrap_or(typed)
    }

    /// Corrects the word just finished, if the character that finished it ends
    /// a word at all.
    pub(super) fn correct_word(&mut self, typed: char) {
        if !ends_a_word(typed) {
            return;
        }

        // The text before the caret, less the character that has just gone in:
        // the word is what comes before the boundary, not including it.
        let before = self.text_before_caret();
        let Some(word_end) = before.len().checked_sub(typed.len_utf8()) else { return };
        let text = &before[..word_end];

        // A hyphen between two words becomes a dash, which is a correction to
        // the character before the word rather than to the word.
        if typed == ' ' {
            if self.begin_automatic_list(text) {
                return;
            }
            if let Some(dash) = self.autocorrect.dash_before(text) {
                self.put_correction(word_end, &dash);
                return;
            }
        }

        let Some(correction) = self.autocorrect.on_word(text) else { return };
        self.put_correction(word_end, &correction);
    }

    /// Turns a paragraph that begins with a list marker into a list.
    ///
    /// `- ` and `* ` make a bulleted one, `1. ` and `1) ` a numbered one. The
    /// marker itself goes away, because it is not text any more: the list draws
    /// its own. Returns whether it did anything.
    ///
    /// Only at the very start of a paragraph that is not already a list, and
    /// only for the number one: a list that began at 1 is the only one this can
    /// make, and starting a "7. " list at one would be worse than leaving the
    /// text alone.
    fn begin_automatic_list(&mut self, text: &str) -> bool {
        if !self.autocorrect.automatic_lists || self.document.list_here().is_some() {
            return false;
        }
        let id = match text {
            "-" | "*" | "\u{2022}" => wp_docx::BULLET_LIST,
            "1." | "1)" => wp_docx::NUMBERED_LIST,
            _ => return false,
        };
        // The marker has to be the whole of the paragraph so far. Anything
        // before it means the hyphen is in the middle of a sentence.
        let caret = self.document.caret();
        if caret.offset != text.len() + 1 {
            return false;
        }

        // One gesture: one undo takes the list back and leaves what was typed,
        // which is how a person says "no, I meant a hyphen".
        self.document.begin_gesture();
        self.document.set_caret(TextPosition::new(caret.paragraph, 0));
        self.document.extend_selection_to(TextPosition::new(caret.paragraph, caret.offset));
        self.document.delete_selection();
        self.document.set_list_here(Some(wp_docx::model::NumberingReference { id, level: 0 }));
        self.document.end_gesture();

        self.relayout();
        true
    }

    /// Replaces the characters before a point with what should be there.
    ///
    /// The caret is put back where it was afterwards — after the boundary
    /// character — because the person is still typing and a caret that jumped
    /// backwards would swallow the next keystroke into the middle of the word.
    fn put_correction(&mut self, ends_at: usize, correction: &crate::autocorrect::Correction) {
        let caret = self.document.caret();
        let Some(text) = self.document.paragraph_text(caret.paragraph) else { return };

        // How many bytes the characters being taken away occupy.
        let start = text[..ends_at]
            .char_indices()
            .rev()
            .nth(correction.taking - 1)
            .map(|(at, _)| at)
            .unwrap_or(0);
        if start >= ends_at {
            return;
        }

        // One gesture, so one undo takes the correction back and leaves what
        // was typed.
        self.document.begin_gesture();
        self.document.set_caret(TextPosition::new(caret.paragraph, start));
        self.document.extend_selection_to(TextPosition::new(caret.paragraph, ends_at));
        self.document.delete_selection();
        self.document.type_text(&correction.putting);

        // Where the caret was, moved by however much the correction changed the
        // length of the word.
        let taken = ends_at - start;
        let put = correction.putting.len();
        let moved = (caret.offset + put).saturating_sub(taken);
        self.document.set_caret(TextPosition::new(caret.paragraph, moved));
        self.document.end_gesture();

        self.relayout();
    }

    /// The text of the paragraph up to the caret.
    fn text_before_caret(&self) -> String {
        let caret = self.document.caret();
        let Some(text) = self.document.paragraph_text(caret.paragraph) else {
            return String::new();
        };
        let at = caret.offset.min(text.len());
        // A caret between the bytes of one character cannot happen, but a
        // document from elsewhere could put one there, and slicing on a
        // boundary that is not one would panic.
        if !text.is_char_boundary(at) {
            return String::new();
        }
        text[..at].to_owned()
    }
}

/// Whether a character ends a word.
///
/// Word's list: a space, and the punctuation that closes a sentence or a
/// clause. A hyphen does not, because a hyphenated word is one word.
fn ends_a_word(character: char) -> bool {
    character.is_whitespace()
        || matches!(character, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\'')
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::default()));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// Types a string the way a person does: one character at a time, through
    /// everything a keystroke goes through.
    fn type_out(editor: &mut Editor, text: &str) {
        for character in text.chars() {
            editor.handle(Event::Char(character));
        }
    }

    fn text(editor: &Editor) -> String {
        editor.document.plain_text()
    }

    #[test]
    fn a_misspelling_is_corrected_when_the_word_is_finished() {
        let mut editor = editor();
        type_out(&mut editor, "teh");
        // Not yet: the word is not finished.
        assert_eq!(text(&editor), "teh");

        type_out(&mut editor, " ");
        assert_eq!(text(&editor), "the ");
    }

    #[test]
    fn the_caret_stays_where_the_typing_left_it() {
        let mut editor = editor();
        type_out(&mut editor, "teh cat");
        assert_eq!(text(&editor), "the cat", "the caret was put back in the wrong place");
    }

    #[test]
    fn one_undo_takes_the_correction_back_and_leaves_the_typing() {
        // The first thing anybody does when a correction is wrong.
        let mut editor = editor();
        type_out(&mut editor, "teh ");
        assert_eq!(text(&editor), "the ");

        editor.document.undo();
        assert!(text(&editor).starts_with("teh"), "got {:?}", text(&editor));
    }

    #[test]
    fn a_quote_curls_as_it_is_typed() {
        let mut editor = editor();
        type_out(&mut editor, "\"yes\"");
        assert_eq!(text(&editor), "\u{201C}yes\u{201D}");
    }

    #[test]
    fn a_hyphen_between_words_becomes_a_dash() {
        let mut editor = editor();
        type_out(&mut editor, "one - two");
        assert!(text(&editor).contains('\u{2013}'), "got {:?}", text(&editor));
    }

    #[test]
    fn a_hyphenated_word_keeps_its_hyphen() {
        let mut editor = editor();
        type_out(&mut editor, "well-known ");
        assert!(text(&editor).contains('-'), "got {:?}", text(&editor));
        assert!(!text(&editor).contains('\u{2013}'));
    }

    #[test]
    fn a_sentence_starts_with_a_capital() {
        let mut editor = editor();
        type_out(&mut editor, "hello there. how are you");
        assert!(text(&editor).starts_with("Hello"), "got {:?}", text(&editor));
        assert!(text(&editor).contains("How are"), "got {:?}", text(&editor));
    }

    #[test]
    fn nothing_is_corrected_while_it_is_switched_off() {
        let mut editor = editor();
        editor.autocorrect = crate::autocorrect::AutoCorrect {
            replace_text: false,
            sentence_case: false,
            curly_quotes: false,
            dashes: false,
            ..crate::autocorrect::AutoCorrect::default()
        };
        type_out(&mut editor, "teh \"one - two");
        assert_eq!(text(&editor), "teh \"one - two");
    }

    #[test]
    fn a_hyphen_at_the_start_of_a_line_makes_a_bulleted_list() {
        let mut editor = editor();
        type_out(&mut editor, "- milk");
        assert_eq!(text(&editor), "milk", "the marker is still text");
        let list = editor.document.list_here().expect("a list");
        assert_eq!(list.id, wp_docx::BULLET_LIST);
    }

    #[test]
    fn one_and_a_full_stop_makes_a_numbered_list() {
        let mut editor = editor();
        type_out(&mut editor, "1. first");
        assert_eq!(text(&editor), "first");
        let list = editor.document.list_here().expect("a list");
        assert_eq!(list.id, wp_docx::NUMBERED_LIST);
    }

    #[test]
    fn a_hyphen_in_the_middle_of_a_line_is_left_alone() {
        let mut editor = editor();
        type_out(&mut editor, "buy - milk");
        assert!(editor.document.list_here().is_none(), "a list was made out of a dash");
    }

    #[test]
    fn a_list_is_not_made_while_it_is_switched_off() {
        let mut editor = editor();
        editor.autocorrect.automatic_lists = false;
        type_out(&mut editor, "- milk");
        assert!(editor.document.list_here().is_none());
        assert_eq!(text(&editor), "- milk");
    }

    #[test]
    fn one_undo_takes_the_list_back_and_leaves_the_typing() {
        let mut editor = editor();
        type_out(&mut editor, "- ");
        assert!(editor.document.list_here().is_some());

        editor.document.undo();
        assert!(editor.document.list_here().is_none(), "the list stayed");
        assert!(text(&editor).starts_with('-'), "got {:?}", text(&editor));
    }

    #[test]
    fn a_correction_does_not_run_away_with_the_rest_of_the_line() {
        // The replacement is longer than what it replaces, and the caret has
        // to end up after the space rather than inside the word.
        let mut editor = editor();
        type_out(&mut editor, "alot of");
        assert_eq!(text(&editor), "a lot of");
    }
}

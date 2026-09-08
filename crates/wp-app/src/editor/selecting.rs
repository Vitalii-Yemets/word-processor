//! Choosing text with the mouse, the way Word does it.
//!
//! # The five ways a click selects
//!
//! One click puts the caret where it landed. Two select the word. Three select
//! the paragraph. Ctrl and a click select the sentence. And a click in the
//! margin down the left of the page — the selection bar, which every word
//! processor has had for forty years and which nothing on screen marks —
//! selects the whole line, two select the paragraph, three the document.
//!
//! # Why a drag remembers which of them started it
//!
//! Because a drag that began on a double click goes on selecting whole words,
//! and one that began on a triple click goes on selecting whole paragraphs.
//! Dragging after a double click and getting half a word is the thing that
//! makes a text editor feel wrong, and it is invisible until you try it.
//!
//! # Why the third click is counted here
//!
//! The system reports two clicks as a double click and says nothing about a
//! third: there is no triple-click message. What arrives is an ordinary press,
//! and the only thing that makes it a triple click is that it came soon after
//! a double click and landed in the same place. Soon is the system's own
//! double-click time, because that is the speed the person set.

use std::time::Instant;

use wp_docx::TextPosition;
use wp_shell::Response;

/// How far a third click may land from the second and still be a third click.
const NEAR: i32 = 4;

use super::Editor;

/// How much of the text a drag takes at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Granularity {
    /// Character by character, which is what a plain drag does.
    #[default]
    Character,
    /// Whole words, after a double click.
    Word,
    /// Whole paragraphs, after a triple click.
    Paragraph,
    /// Whole lines, after a click in the selection bar.
    Line,
}

impl Editor {
    /// Whether a press is the third click of a triple click.
    #[must_use]
    pub(super) fn is_third_click(&self, x: i32, y: i32) -> bool {
        let Some((when, at_x, at_y)) = self.last_double_click else { return false };
        if (x - at_x).abs() > NEAR || (y - at_y).abs() > NEAR {
            return false;
        }
        when.elapsed().as_millis() <= u128::from(wp_shell::double_click_millis())
    }

    /// Remembers a double click, so the next press can tell it is a third.
    pub(super) fn note_double_click(&mut self, x: i32, y: i32) {
        self.last_double_click = Some((Instant::now(), x, y));
    }

    /// Selects the whole paragraph a point is in.
    pub(super) fn select_paragraph_at(&mut self, x: i32, y: i32) -> Response {
        let Some(at) = self.position_at(x, y) else { return Response::Ignored };
        self.select_paragraph(at.paragraph);
        self.drag_by = Granularity::Paragraph;
        self.dragging = true;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Selects one paragraph, whole.
    pub(super) fn select_paragraph(&mut self, index: usize) {
        let length = self.document.paragraph_text(index).map_or(0, |text| text.len());
        self.document.set_caret(TextPosition::new(index, 0));
        self.document.extend_selection_to(TextPosition::new(index, length));
    }

    /// Selects the sentence a point is in, which is what Ctrl and a click do.
    pub(super) fn select_sentence_at(&mut self, x: i32, y: i32) -> Response {
        let Some(at) = self.position_at(x, y) else { return Response::Ignored };
        let Some(text) = self.document.paragraph_text(at.paragraph) else {
            return Response::Ignored;
        };
        let (start, end) = sentence_around(&text, at.offset);

        self.document.set_caret(TextPosition::new(at.paragraph, start));
        self.document.extend_selection_to(TextPosition::new(at.paragraph, end));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Whether a point is in the margin down the left of the page.
    ///
    /// That strip is the selection bar: a click in it takes a whole line.
    #[must_use]
    pub(super) fn in_selection_bar(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        let (px, py) = (x as f32, y as f32);
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            let page = &self.pages[index];
            if py < top || py > top + page.height {
                continue;
            }
            // From the left edge of the paper to where the text begins.
            let text_left = page.lines.iter().map(|line| line.left).fold(f32::MAX, f32::min);
            if text_left == f32::MAX || px < origin_x || px >= origin_x + (text_left - origin_x) {
                continue;
            }
            // The line the pointer is level with.
            let found = page
                .lines
                .iter()
                .position(|line| py >= top + line.top() && py < top + line.bottom());
            return found.map(|line| (index, line));
        }
        None
    }

    /// Selects the line a click in the selection bar is level with.
    pub(super) fn select_line(&mut self, page: usize, line: usize) -> Response {
        let Some(found) = self.pages.get(page).and_then(|page| page.lines.get(line)) else {
            return Response::Ignored;
        };
        let (paragraph, start, end) = (found.paragraph, found.start_offset, found.end_offset);

        self.document.set_caret(TextPosition::new(paragraph, start));
        self.document.extend_selection_to(TextPosition::new(paragraph, end));
        self.drag_by = Granularity::Line;
        self.dragging = true;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Grows the selection to whole words or paragraphs while a drag runs.
    ///
    /// A plain drag needs none of this: the caret goes where the pointer is.
    pub(super) fn extend_drag(&mut self, x: i32, y: i32) -> Response {
        let Some(at) = self.position_at(x, y) else { return Response::Ignored };

        match self.drag_by {
            Granularity::Character => {
                self.document.move_caret(at, true);
            }
            Granularity::Word => {
                let Some(text) = self.document.paragraph_text(at.paragraph) else {
                    return Response::Ignored;
                };
                let (start, end) = word_around(&text, at.offset);
                // Whichever end of the word is further from where the drag
                // began is the one to reach to.
                let anchor = self.document.selection().map_or(at, |(from, _)| from);
                let wanted =
                    if TextPosition::new(at.paragraph, end) > anchor { end } else { start };
                self.document.move_caret(TextPosition::new(at.paragraph, wanted), true);
            }
            Granularity::Paragraph | Granularity::Line => {
                let length =
                    self.document.paragraph_text(at.paragraph).map_or(0, |text| text.len());
                self.document.move_caret(TextPosition::new(at.paragraph, length), true);
            }
        }

        self.needs_redraw = true;
        Response::Redraw
    }
}

/// Where the word round an offset starts and ends.
#[must_use]
fn word_around(text: &str, offset: usize) -> (usize, usize) {
    let letters = |character: char| character.is_alphanumeric() || character == '\'';
    let offset = offset.min(text.len());

    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, character)| letters(*character))
        .map(|(at, _)| at)
        .last()
        .unwrap_or(offset);
    let end = text[offset..]
        .char_indices()
        .take_while(|(_, character)| letters(*character))
        .map(|(at, character)| offset + at + character.len_utf8())
        .last()
        .unwrap_or(offset);
    (start, end)
}

/// Where the sentence round an offset starts and ends.
///
/// A sentence ends at a full stop, a question mark or an exclamation mark, and
/// the space after it belongs to the sentence — which is what Word selects and
/// what makes a sentence deleted this way leave no double space behind.
#[must_use]
fn sentence_around(text: &str, offset: usize) -> (usize, usize) {
    let ends = |character: char| matches!(character, '.' | '?' | '!');
    let offset = offset.min(text.len());

    let mut start = 0usize;
    for (at, character) in text[..offset].char_indices() {
        if ends(character) {
            start = at + character.len_utf8();
        }
    }
    // Past the spaces after the last full stop, which belong to the sentence
    // before it.
    while start < text.len() && text[start..].starts_with(' ') {
        start += 1;
    }

    let mut end = text.len();
    for (at, character) in text[offset..].char_indices() {
        if ends(character) {
            end = offset + at + character.len_utf8();
            break;
        }
    }
    while end < text.len() && text[end..].starts_with(' ') {
        end += 1;
    }
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_found_round_a_point_inside_it() {
        assert_eq!(word_around("one two three", 5), (4, 7));
    }
    #[test]
    fn a_point_at_the_end_of_a_word_takes_that_word() {
        // Offset three is the end of "one" and the start of the space; a
        // person clicking there clicked on "one".
        assert_eq!(word_around("one two", 3), (0, 3));
    }

    #[test]
    fn a_point_in_the_space_between_words_takes_neither() {
        assert_eq!(word_around("one  two", 4), (4, 4));
    }

    #[test]
    fn a_word_at_the_start_and_at_the_end_are_both_found() {
        assert_eq!(word_around("one two", 1), (0, 3));
        assert_eq!(word_around("one two", 6), (4, 7));
    }

    #[test]
    fn an_apostrophe_is_part_of_the_word_round_it() {
        assert_eq!(word_around("it doesn't matter", 6), (3, 10));
    }

    #[test]
    fn a_sentence_runs_from_one_full_stop_to_the_next() {
        let text = "First one. Second one. Third one.";
        assert_eq!(sentence_around(text, 12), (11, 23));
    }

    #[test]
    fn the_first_sentence_starts_at_the_start() {
        assert_eq!(sentence_around("First one. Second.", 2), (0, 11));
    }

    #[test]
    fn the_last_sentence_ends_at_the_end() {
        let text = "First one. Last one";
        assert_eq!(sentence_around(text, 14), (11, text.len()));
    }

    #[test]
    fn a_question_and_a_shout_end_a_sentence_too() {
        assert_eq!(sentence_around("Really? Yes!", 2), (0, 8));
        assert_eq!(sentence_around("Really? Yes!", 9), (8, 12));
    }

    #[test]
    fn the_space_after_a_sentence_belongs_to_it() {
        // "First one. " is eleven characters including the space.
        let (_, end) = sentence_around("First one. Second.", 2);
        assert_eq!(end, 11);
    }

    #[test]
    fn a_paragraph_with_no_full_stop_is_one_sentence() {
        let text = "no punctuation here";
        assert_eq!(sentence_around(text, 5), (0, text.len()));
    }
}

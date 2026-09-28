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
//! changed before it goes in rather than afterwards. Enter is different again:
//! it looks at the whole paragraph being left, which is where three hyphens on
//! a line of their own become a line.
//!
//! # Why it is its own step to undo
//!
//! Because a correction is the program's doing and not the person's, and the
//! first thing anybody does when it is wrong is press Ctrl+Z. One press has to
//! take back the correction and leave what was actually typed; a second takes
//! back the typing. That is what Word does, and it is why the correction is
//! wrapped in a gesture of its own rather than folded into the keystroke.
//!
//! # The little box
//!
//! Word's AutoCorrect Options button: rest the pointer on a word that was just
//! corrected and a thin blue bar appears under its first letter; move onto
//! the bar and it becomes a small box with a lightning bolt, offering the
//! word back, offering to stop making that correction, and offering the
//! dialog. It is the answer to the correction that was noticed three words
//! later, when Ctrl+Z would take back the three words first. The bar is there
//! so that a pointer crossing a corrected word on its way somewhere else does
//! not drop a button on the text; the box comes only to a pointer that went
//! looking for it. The last correction is what it remembers, and it forgets
//! when that correction is undone, or edited, or another is made.

use wp_docx::model::{Border, NumberingReference, RunProperties};
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::autocorrect::{ends_a_word, Correction, Emphasis, Kind};
use crate::chrome::icons::Icon;
use crate::chrome::pastebadge::{self, PasteBadge};
use crate::chrome::popup::Row;
use crate::chrome::{Choice, Popup};

use super::Editor;

/// A correction just made: what the little box under it is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Made {
    /// The paragraph it is in.
    pub paragraph: usize,
    /// Where in the paragraph what was put in begins and ends.
    pub start: usize,
    pub end: usize,
    /// What was typed, before it was corrected.
    pub original: String,
    /// What is there now, so that an edit to it can be noticed.
    pub putting: String,
    /// What came before the word, which is what the exceptions learn from.
    pub ahead: String,
    /// Which rule made it, which decides what the box says.
    pub kind: Kind,
    /// How deep the undo history was once it was made, which is how the box
    /// knows whether it is still the last thing done.
    pub depth: usize,
    /// Whether the box is showing: it appears when the pointer reaches the
    /// bar, and stays while the pointer is on it or on the word, or its list
    /// is open.
    pub shown: bool,
    /// Whether the pointer is on the box.
    pub hot: bool,
    /// Whether the thin blue bar is showing, which it does while the pointer
    /// rests on the word and the box is not yet out.
    pub bar: bool,
}

/// The thin blue bar: how wide and how tall it is, in pixels, and how much
/// room round it still counts as being on it — it is small, and a pointer
/// that has to land on three pixels exactly is a pointer that misses.
const BAR_WIDTH: f32 = 10.0;
const BAR_HEIGHT: f32 = 3.0;
const BAR_SLACK: f32 = 3.0;

/// The three lines of the box, top to bottom.
const TAKE_BACK: usize = 0;
const STOP: usize = 1;
const OPTIONS: usize = 2;

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

        // `\alpha` finished becomes α, where the Math AutoCorrect tab says to
        // do that outside an equation too.
        if let Some(greek) = self.autocorrect.math_word(text, false) {
            self.put_correction(word_end, &greek);
            return;
        }

        // `*bold*` closed: the marks go and the formatting comes.
        if let Some(emphasis) = self.autocorrect.emphasis(text) {
            self.put_emphasis(word_end, &emphasis);
            return;
        }

        // An address becomes a link, which is a correction to what the word is
        // rather than to how it is spelled.
        let word = text.rsplit(char::is_whitespace).next().unwrap_or_default();
        if self.autocorrect.is_address(word) {
            self.put_hyperlink(word_end, word);
            return;
        }

        let Some(correction) = self.autocorrect.on_word(text) else { return };
        self.put_correction(word_end, &correction);
    }

    /// What Enter does to the paragraph it is ending, before it ends it: a
    /// line, a table or a heading, where the paragraph is one of those typed
    /// out. Returns whether Enter's work is done.
    pub(super) fn correct_paragraph_end(&mut self) -> bool {
        self.make_border_line() || self.make_table() || self.make_heading()
    }

    /// Three hyphens on a line of their own are not a paragraph: they are a
    /// line under the paragraph above, which is what Word makes of them. The
    /// hyphens go, the paragraph above gains the line, and the caret is left
    /// on the empty paragraph under it — which is where Enter would have put
    /// it, so Enter has done its work.
    fn make_border_line(&mut self) -> bool {
        let caret = self.document.caret();
        let Some(text) = self.document.paragraph_text(caret.paragraph) else { return false };
        let Some((style, size)) = self.autocorrect.border_line(&text) else { return false };
        if self.document.list_here().is_some() {
            return false;
        }

        // One gesture: the hyphens go and the line comes, and one undo takes
        // both back and leaves the hyphens as typed.
        self.document.begin_gesture();
        self.document.set_caret(TextPosition::new(caret.paragraph, 0));
        self.document.extend_selection_to(TextPosition::new(caret.paragraph, text.len()));
        self.document.delete_selection();

        // Under the paragraph above — or, at the top of the document where
        // there is none, under the paragraph itself, with a new one made below
        // so the caret still ends up under the line.
        let lined = caret.paragraph.saturating_sub(1);
        self.document.set_caret(TextPosition::new(lined, 0));
        let mut borders = self.document.borders_here();
        borders.bottom = Some(Border::line(style, size, Some("auto")));
        self.document.set_borders_here(&borders);
        if caret.paragraph == 0 {
            self.document.press_enter();
            // Enter carries the paragraph's formatting down with it; the line
            // is the one thing that must not come.
            let mut below = self.document.borders_here();
            below.bottom = None;
            self.document.set_borders_here(&below);
        } else {
            self.document.set_caret(TextPosition::new(caret.paragraph, 0));
        }
        self.document.end_gesture();

        self.made(Made {
            paragraph: lined,
            start: 0,
            end: 0,
            original: text,
            putting: String::new(),
            ahead: String::new(),
            kind: Kind::BorderLine,
            depth: self.document.undo_depth(),
            shown: false,
            hot: false,
            bar: false,
        });
        self.relayout();
        true
    }

    /// `+---+------+` and Enter: the line becomes a table of one row, a column
    /// for each run of hyphens, as wide as the run is on the page — so the
    /// table's lines fall where the plus signs were. The caret goes into the
    /// first cell.
    fn make_table(&mut self) -> bool {
        let caret = self.document.caret();
        let Some(text) = self.document.paragraph_text(caret.paragraph) else { return false };
        let Some(edges) = self.autocorrect.table_columns(&text) else { return false };
        if self.document.list_here().is_some() || self.document.table_here().is_some() {
            return false;
        }
        let widths = self.column_widths_of(caret.paragraph, &edges);

        self.document.begin_gesture();
        let changed = self.document.replace_paragraph_with_table(&widths);
        self.document.end_gesture();
        if !changed {
            return false;
        }
        self.made(Made {
            paragraph: caret.paragraph,
            start: 0,
            end: 0,
            original: text,
            putting: String::new(),
            ahead: String::new(),
            kind: Kind::Table,
            depth: self.document.undo_depth(),
            shown: false,
            hot: false,
            bar: false,
        });
        self.relayout();
        true
    }

    /// How wide each column of a table typed as plus signs is, in twentieths
    /// of a point: the distance between two plus signs on the page.
    ///
    /// Where the line is not laid out as one line — too long for the page, or
    /// not laid out at all — the widths are shared out by how many hyphens
    /// each column has, across the width a new table is given.
    fn column_widths_of(&self, paragraph: usize, edges: &[usize]) -> Vec<i32> {
        let twips_per_pixel = 1440.0 / self.pixels_per_inch();
        let places: Option<Vec<(f32, f32)>> = edges
            .iter()
            .map(|at| {
                self.caret_rect_at(TextPosition::new(paragraph, *at)).map(|(x, y, _, _)| (x, y))
            })
            .collect();
        if let Some(places) = places {
            let one_line = places.iter().all(|(_, y)| (*y - places[0].1).abs() < 0.5);
            let widths: Vec<i32> = places
                .windows(2)
                .map(|pair| ((pair[1].0 - pair[0].0).abs() * twips_per_pixel).round() as i32)
                .collect();
            if one_line && widths.iter().all(|width| *width > 0) {
                return widths;
            }
        }
        let runs: Vec<usize> = edges.windows(2).map(|pair| pair[1] - pair[0]).collect();
        let total: usize = runs.iter().sum::<usize>().max(1);
        runs.iter().map(|run| (9360 * *run / total) as i32).collect()
    }

    /// A line entered twice becomes a heading, where that is switched on.
    ///
    /// Enter pressed in an empty paragraph under a line that is one line long,
    /// begins with a capital and does not end with punctuation: the line takes
    /// Heading 1, or Heading 2 for a line begun with one tab and so on down,
    /// the tabs go, and the caret stays where it is, on the empty paragraph
    /// under the heading — the second Enter has done its work.
    fn make_heading(&mut self) -> bool {
        let caret = self.document.caret();
        if caret.paragraph == 0 || !self.autocorrect.headings {
            return false;
        }
        if self.document.paragraph_text(caret.paragraph).is_none_or(|here| !here.is_empty()) {
            return false;
        }
        let above = caret.paragraph - 1;
        let Some(text) = self.document.paragraph_text(above) else { return false };
        let Some((level, tabs)) = self.autocorrect.heading_level(&text) else { return false };
        let style = format!("Heading{level}");

        // Only a plain paragraph of the text: not one already given a style,
        // nor one in a list or a table, where a short line is something else.
        let plain = self.document.style_of(above).is_none_or(|id| id == "Normal");
        if !plain
            || self.document.paragraph_in_table(above)
            || self.document.paragraph_in_table(caret.paragraph)
            || !self.document.style_is_available(&style)
        {
            return false;
        }
        // One line long: where it begins and where it ends on the same line.
        let (Some(first), Some(last)) = (
            self.caret_rect_at(TextPosition::new(above, 0)),
            self.caret_rect_at(TextPosition::new(above, text.len())),
        ) else {
            return false;
        };
        if (first.1 - last.1).abs() > 0.5 {
            return false;
        }
        self.document.set_caret(TextPosition::new(above, 0));
        let listed = self.document.list_here().is_some();
        if listed {
            self.document.set_caret(caret);
            return false;
        }

        self.document.begin_gesture();
        if tabs > 0 {
            self.document.extend_selection_to(TextPosition::new(above, tabs));
            self.document.delete_selection();
        }
        self.document.set_paragraph_style_here(Some(&style));
        self.document.set_caret(caret);
        self.document.end_gesture();

        self.made(Made {
            paragraph: above,
            start: 0,
            end: text.len() - tabs,
            original: text.clone(),
            putting: text[tabs..].to_owned(),
            ahead: String::new(),
            kind: Kind::Heading,
            depth: self.document.undo_depth(),
            shown: false,
            hot: false,
            bar: false,
        });
        self.relayout();
        true
    }

    /// Turns a paragraph that begins with a list marker into a list.
    ///
    /// `- ` and `* ` make a bulleted one, `1. ` and `1) ` a numbered one — and
    /// `7. ` a numbered one that begins at seven, which is what a person who
    /// typed seven meant. The marker itself goes away, because it is not text
    /// any more: the list draws its own. Returns whether it did anything.
    ///
    /// Only at the very start of a paragraph that is not already a list.
    fn begin_automatic_list(&mut self, text: &str) -> bool {
        if !self.autocorrect.automatic_lists || self.document.list_here().is_some() {
            return false;
        }
        // The marker has to be the whole of the paragraph so far. Anything
        // before it means the hyphen is in the middle of a sentence.
        let caret = self.document.caret();
        if caret.offset != text.len() + 1 {
            return false;
        }

        let id = match text {
            "-" | "*" | "\u{2022}" => wp_docx::BULLET_LIST,
            _ => match self.autocorrect.list_start(text) {
                Some(1) => wp_docx::NUMBERED_LIST,
                Some(start) => match self.document.numbered_list_starting_at(start) {
                    Some(id) => id,
                    None => return false,
                },
                None => return false,
            },
        };

        // One gesture: one undo takes the list back and leaves what was typed,
        // which is how a person says "no, I meant a hyphen".
        self.document.begin_gesture();
        self.document.set_caret(TextPosition::new(caret.paragraph, 0));
        self.document.extend_selection_to(TextPosition::new(caret.paragraph, caret.offset));
        self.document.delete_selection();
        self.document.set_list_here(Some(NumberingReference { id, level: 0 }));
        self.document.end_gesture();

        self.made(Made {
            paragraph: caret.paragraph,
            start: 0,
            end: 0,
            original: format!("{text} "),
            putting: String::new(),
            ahead: String::new(),
            kind: Kind::List,
            depth: self.document.undo_depth(),
            shown: false,
            hot: false,
            bar: false,
        });
        self.relayout();
        true
    }

    /// Replaces the characters before a point with what should be there.
    ///
    /// The caret is put back where it was afterwards — after the boundary
    /// character — because the person is still typing and a caret that jumped
    /// backwards would swallow the next keystroke into the middle of the word.
    fn put_correction(&mut self, ends_at: usize, correction: &Correction) {
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
        let original = text[start..ends_at].to_owned();
        let ahead = text[..start].to_owned();

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

        self.made(Made {
            paragraph: caret.paragraph,
            start,
            end: start + put,
            original,
            putting: correction.putting.clone(),
            ahead,
            kind: correction.kind,
            depth: self.document.undo_depth(),
            shown: false,
            hot: false,
            bar: false,
        });
        self.relayout();
    }

    /// Gives `*bold*` its formatting and takes the marks away.
    fn put_emphasis(&mut self, ends_at: usize, emphasis: &Emphasis) {
        // A document that limits formatting to a selection of styles is one
        // where this correction is formatting by hand under another name.
        // Word's Restrict Editing has a box that lets it through; without it
        // the marks stay as marks. See [`super::protection`].
        if !self.autoformat_may_override() {
            return;
        }
        let caret = self.document.caret();
        let Some(text) = self.document.paragraph_text(caret.paragraph) else { return };
        if emphasis.close >= ends_at || emphasis.open >= emphasis.close {
            return;
        }
        let mark = text[emphasis.close..].chars().next().map_or(1, char::len_utf8);
        let original = text[emphasis.open..emphasis.close + mark].to_owned();
        let inner = text[emphasis.open + mark..emphasis.close].to_owned();

        self.document.begin_gesture();
        // The closing mark first, so the opening one's offset still holds.
        self.document.set_caret(TextPosition::new(caret.paragraph, emphasis.close));
        self.document
            .extend_selection_to(TextPosition::new(caret.paragraph, emphasis.close + mark));
        self.document.delete_selection();
        self.document.set_caret(TextPosition::new(caret.paragraph, emphasis.open));
        self.document.extend_selection_to(TextPosition::new(caret.paragraph, emphasis.open + mark));
        self.document.delete_selection();
        // Then the inside, which is now two marks shorter, gets its formatting.
        let inner_start = emphasis.open;
        let inner_end = emphasis.close - mark;
        self.document.set_caret(TextPosition::new(caret.paragraph, inner_start));
        self.document.extend_selection_to(TextPosition::new(caret.paragraph, inner_end));
        let change = if emphasis.bold {
            RunProperties { bold: Some(true), ..Default::default() }
        } else {
            RunProperties { italic: Some(true), ..Default::default() }
        };
        self.document.apply_run_formatting(&change);
        // And the caret back where the typing left it, two marks earlier —
        // with the formatting turned off again there, or everything typed
        // after the word would be bold too.
        self.document
            .set_caret(TextPosition::new(caret.paragraph, caret.offset.saturating_sub(2 * mark)));
        self.document.end_gesture();

        self.made(Made {
            paragraph: caret.paragraph,
            start: inner_start,
            end: inner_end,
            original,
            putting: inner,
            ahead: String::new(),
            kind: Kind::Emphasis,
            depth: self.document.undo_depth(),
            shown: false,
            hot: false,
            bar: false,
        });
        self.relayout();
    }

    /// Makes an address just typed into a link to itself.
    fn put_hyperlink(&mut self, ends_at: usize, word: &str) {
        let caret = self.document.caret();
        let start = ends_at.saturating_sub(word.len());

        self.document.begin_gesture();
        self.document.set_caret(TextPosition::new(caret.paragraph, start));
        self.document.extend_selection_to(TextPosition::new(caret.paragraph, ends_at));
        let changed = self.document.add_hyperlink(word, word);
        self.document.set_caret(caret);
        self.document.end_gesture();
        if !changed {
            return;
        }

        self.made(Made {
            paragraph: caret.paragraph,
            start,
            end: ends_at,
            original: word.to_owned(),
            putting: word.to_owned(),
            ahead: String::new(),
            kind: Kind::Hyperlink,
            depth: self.document.undo_depth(),
            shown: false,
            hot: false,
            bar: false,
        });
        self.relayout();
    }

    /// Remembers what was just corrected, which is what the little box under
    /// it is about.
    fn made(&mut self, made: Made) {
        self.corrected = Some(made);
    }

    // --- The little box ------------------------------------------------------

    /// The correction the box is about, if it is still there to be about.
    ///
    /// An edit to the corrected word — or to the paragraph it was in — leaves
    /// the box pointing at something else, so the word is checked against
    /// what was put in before the box is believed.
    fn correction_still_there(&self) -> bool {
        let Some(made) = &self.corrected else { return false };
        let Some(text) = self.document.paragraph_text(made.paragraph) else { return false };
        let word = made.end <= text.len()
            && text.is_char_boundary(made.start)
            && text.is_char_boundary(made.end)
            && text[made.start..made.end] == made.putting;
        // A table is still the one AutoFormat made while it begins where the
        // line was and nothing has been typed into it: taking back a table
        // with somebody's words in it would take the words too.
        if made.kind == Kind::Table {
            return word
                && self.document.table_paragraphs_at(made.paragraph).is_some_and(
                    |(first, last)| {
                        first == made.paragraph
                            && (first..=last).all(|at| {
                                self.document.paragraph_text(at).is_some_and(|text| text.is_empty())
                            })
                    },
                );
        }
        word
    }

    /// Where the box is, if there is one to draw.
    ///
    /// Worked out afresh rather than remembered, for the same reason the
    /// paste button's is: the page it points at moves.
    pub(super) fn correction_badge(&self) -> Option<PasteBadge> {
        let made = self.corrected.as_ref()?;
        if !made.shown || !self.correction_still_there() {
            return None;
        }
        let (x, y, _, height) =
            self.caret_rect_at(TextPosition::new(made.paragraph, made.start))?;
        let mut badge = PasteBadge::for_correction(x, y + height + pastebadge::DROP);
        badge.hot = made.hot;
        if badge.top < self.content_top() || badge.top + pastebadge::HEIGHT > self.window_bottom() {
            return None;
        }
        Some(badge)
    }

    /// Draws it, when there is one — or the thin bar that comes before it.
    pub(super) fn draw_correction_badge(&mut self) {
        let theme = self.theme;
        if let Some(badge) = self.correction_badge() {
            badge.draw(&mut self.canvas, &mut self.chrome_engine, &mut self.renderer, &theme);
            return;
        }
        let Some((left, top)) = self.correction_bar() else { return };
        self.canvas.fill_rect(
            left.round() as i32,
            top.round() as i32,
            BAR_WIDTH as i32,
            BAR_HEIGHT as i32,
            theme.accent,
        );
    }

    /// Where the thin blue bar is, if it is showing: under the first letter
    /// of the word, touching the bottom of its line.
    pub(super) fn correction_bar(&self) -> Option<(f32, f32)> {
        let made = self.corrected.as_ref()?;
        if !made.bar || made.shown || !self.correction_still_there() {
            return None;
        }
        let (x, y, _, height) =
            self.caret_rect_at(TextPosition::new(made.paragraph, made.start))?;
        let top = y + height;
        if top < self.content_top() || top + BAR_HEIGHT > self.window_bottom() {
            return None;
        }
        Some((x, top))
    }

    /// Whether a point is on the bar, or near enough to it to mean it.
    fn over_correction_bar(&self, x: i32, y: i32) -> bool {
        let Some(made) = &self.corrected else { return false };
        if !made.bar || !self.correction_still_there() {
            return false;
        }
        let Some((left, line_top, _, height)) =
            self.caret_rect_at(TextPosition::new(made.paragraph, made.start))
        else {
            return false;
        };
        let x = crate::chrome::mirror::flip(x) as f32;
        let (y, top) = (y as f32, line_top + height);
        x >= left - BAR_SLACK
            && x < left + BAR_WIDTH + BAR_SLACK
            && y >= top
            && y < top + BAR_HEIGHT + BAR_SLACK
    }

    /// Whether a point is on the box.
    pub(super) fn over_correction_badge(&self, x: i32, y: i32) -> bool {
        self.correction_badge().is_some_and(|badge| badge.covers(x, y))
    }

    /// Whether a point is on the word that was corrected: on its line, and
    /// between its ends.
    fn over_corrected_word(&self, x: i32, y: i32) -> bool {
        let Some(made) = &self.corrected else { return false };
        if !self.correction_still_there() {
            return false;
        }
        let Some(at) = self.position_at(x, y) else { return false };
        if at.paragraph != made.paragraph || at.offset < made.start || at.offset > made.end {
            return false;
        }
        let Some((_, top, _, height)) = self.caret_rect_at(at) else { return false };
        (y as f32) >= top && (y as f32) < top + height
    }

    /// Follows the pointer: the bar while it rests on the word, the box once
    /// it reaches the bar, the box lit while it is on it, and nothing once it
    /// has left the word and the box both. True when anything changed.
    pub(super) fn follow_correction_badge(&mut self, x: i32, y: i32) -> bool {
        let Some(before) = self.corrected.as_ref().map(|made| (made.shown, made.hot, made.bar))
        else {
            return false;
        };
        let over_box = self.over_correction_badge(x, y);
        let open =
            self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::AutoCorrectOption);
        let over_word = self.over_corrected_word(x, y);
        let over_bar = self.over_correction_bar(x, y);

        let shown = if before.0 { over_box || open || over_word } else { over_bar };
        let bar = !shown && (over_word || (before.2 && over_bar));
        let hot = shown && over_box;
        let Some(made) = &mut self.corrected else { return false };
        if (made.shown, made.hot, made.bar) == (shown, hot, bar) {
            return false;
        }
        made.shown = shown;
        made.hot = hot;
        made.bar = bar;
        true
    }

    /// Puts the box away and forgets the correction it was about.
    pub(super) fn forget_correction(&mut self) {
        if self.corrected.take().is_some() {
            self.needs_redraw = true;
        }
    }

    /// Whether the box is showing, or the bar before it, which is what Escape
    /// has to know.
    pub(super) fn offering_correction_options(&self) -> bool {
        self.corrected.as_ref().is_some_and(|made| made.shown || made.bar)
    }

    /// Opens the box's list: the word back, the rule stopped, the dialog.
    pub(super) fn open_correction_options(&mut self) -> Response {
        let Some(made) = self.corrected.clone() else { return Response::Ignored };
        let Some(badge) = self.correction_badge() else { return Response::Ignored };

        // Word names the word for a replacement, because "Undo Automatic
        // Corrections" would not say which; for the rest it names the rule.
        let take_back = match made.kind {
            Kind::Replacement => format!("Change back to \u{201C}{}\u{201D}", made.original),
            other => other.undo_label().to_owned(),
        };
        let items = vec![
            take_back,
            made.kind.stop_label(made.original.trim_end()),
            "Control AutoCorrect Options\u{2026}".to_owned(),
        ];
        let rows = vec![
            Row::new(crate::chrome::popup::Kind::Choice, Icon::Undo),
            Row::new(crate::chrome::popup::Kind::Choice, Icon::None),
            Row::new(crate::chrome::popup::Kind::Choice, Icon::Settings),
        ];
        self.popup = Some(
            Popup::new(
                Choice::AutoCorrectOption,
                items,
                None,
                badge.left,
                badge.top + pastebadge::HEIGHT,
                320.0,
            )
            .with_rows(rows),
        );
        self.needs_redraw = true;
        Response::Redraw
    }

    /// One of the box's lines was picked.
    pub(super) fn choose_correction_option(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(made) = self.corrected.clone() else { return Response::Ignored };
        match index {
            TAKE_BACK => self.take_correction_back(&made),
            STOP => {
                // Stopping the rule takes back what it did as well: a person
                // who says "stop doing that" means this time too.
                let response = self.take_correction_back(&made);
                self.autocorrect.stop(made.kind, made.original.trim_end());
                self.settings.autocorrect = Some(self.autocorrect.clone());
                self.settings.save();
                response
            }
            OPTIONS => {
                self.forget_correction();
                self.open_autocorrect()
            }
            _ => Response::Ignored,
        }
    }

    /// Takes a correction back.
    ///
    /// While it is still the last thing done that is an undo, which is what
    /// Word's line in the box does. After more typing it cannot be — an undo
    /// would take the typing first — so the correction is reversed by hand:
    /// what was typed goes back in place of what was put.
    fn take_correction_back(&mut self, made: &Made) -> Response {
        if self.document.undo_depth() == made.depth {
            let changed = self.document.undo();
            if changed {
                self.correction_undone();
            }
            self.needs_redraw = true;
            return self.edited(changed, "");
        }
        if !self.correction_still_there() {
            self.forget_correction();
            return Response::Ignored;
        }

        let caret = self.document.caret();
        let at = TextPosition::new(made.paragraph, made.start);
        self.document.begin_gesture();
        let changed = match made.kind {
            Kind::List => {
                self.document.set_caret(at);
                self.document.set_list_here(None);
                self.document.type_text(&made.original)
            }
            Kind::BorderLine => {
                self.document.set_caret(at);
                let mut borders = self.document.borders_here();
                borders.bottom = None;
                self.document.set_borders_here(&borders)
            }
            Kind::Hyperlink => {
                self.document.set_caret(at);
                self.document.remove_hyperlink()
            }
            Kind::Table => {
                // The table goes, and the line of plus signs goes back into
                // the paragraph that was made under it.
                self.document.set_caret(at);
                let removed = self.document.delete_table();
                self.document.set_caret(at);
                removed && self.document.type_text(&made.original)
            }
            Kind::Heading => {
                // Back to the plain paragraph it was, tabs and all.
                self.document.set_caret(at);
                self.document.set_paragraph_style_here(None);
                self.document.extend_selection_to(TextPosition::new(made.paragraph, made.end));
                self.document.delete_selection();
                self.document.type_text(&made.original)
            }
            Kind::Emphasis => {
                // The formatting comes off, and the marks go back on.
                let end = TextPosition::new(made.paragraph, made.end);
                self.document.set_caret(at);
                self.document.extend_selection_to(end);
                let plain =
                    RunProperties { bold: Some(false), italic: Some(false), ..Default::default() };
                self.document.apply_run_formatting(&plain);
                self.document.set_caret(at);
                self.document.extend_selection_to(end);
                self.document.delete_selection();
                self.document.type_text(&made.original)
            }
            _ => {
                self.document.set_caret(at);
                self.document.extend_selection_to(TextPosition::new(made.paragraph, made.end));
                self.document.delete_selection();
                self.document.type_text(&made.original)
            }
        };
        // The caret where it was, moved by however much the paragraph grew,
        // if it was after the word.
        let grown = made.original.len() as isize - made.putting.len() as isize;
        let back = if caret.paragraph == made.paragraph && caret.offset >= made.end {
            TextPosition::new(caret.paragraph, (caret.offset as isize + grown).max(0) as usize)
        } else {
            caret
        };
        self.document.set_caret(back);
        self.document.end_gesture();

        self.correction_undone();
        self.edited(changed, "")
    }

    /// What follows a correction being taken back, by the box or by Ctrl+Z:
    /// the box goes, and the exceptions learn from it where they are set to.
    pub(super) fn correction_undone(&mut self) {
        let Some(made) = self.corrected.take() else { return };
        self.needs_redraw = true;
        if self.autocorrect.learn_from_undo(made.kind, made.original.trim_end(), &made.ahead) {
            self.settings.autocorrect = Some(self.autocorrect.clone());
            self.settings.save();
        }
    }

    /// Whether Ctrl+Z now would take back the last correction — which is
    /// what makes it teach the exceptions.
    pub(super) fn undo_takes_back_correction(&self) -> bool {
        self.corrected.as_ref().is_some_and(|made| made.depth == self.document.undo_depth())
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

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

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

    fn press(editor: &mut Editor, key: Key) {
        editor.handle(Event::KeyDown { key, modifiers: Modifiers::default() });
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
    fn seven_and_a_full_stop_makes_a_list_that_begins_at_seven() {
        let mut editor = editor();
        type_out(&mut editor, "7. seventh");
        assert_eq!(text(&editor), "seventh");
        let list = editor.document.list_here().expect("a list");
        assert_ne!(list.id, wp_docx::NUMBERED_LIST, "the list began at one");
        let level = editor.document.numbering().level(list.id, 0).expect("a level");
        assert_eq!(level.start, 7);
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

    #[test]
    fn stars_round_a_word_make_it_bold_and_take_the_stars_away() {
        let mut editor = editor();
        type_out(&mut editor, "A *bold* word");
        assert_eq!(text(&editor), "A bold word");
        editor.document.set_caret(TextPosition::new(0, 3));
        assert!(editor.document.character_format_here().bold, "the word is not bold");
        editor.document.set_caret(TextPosition::new(0, 8));
        assert!(!editor.document.character_format_here().bold, "the bold ran on");
    }

    #[test]
    fn underscores_round_words_make_them_italic() {
        let mut editor = editor();
        type_out(&mut editor, "_two words_ here");
        assert_eq!(text(&editor), "two words here");
        editor.document.set_caret(TextPosition::new(0, 5));
        assert!(editor.document.character_format_here().italic);
    }

    #[test]
    fn an_address_becomes_a_link_to_itself() {
        let mut editor = editor();
        type_out(&mut editor, "See www.example.com now");
        assert_eq!(text(&editor), "See www.example.com now");
        editor.document.set_caret(TextPosition::new(0, 8));
        let link = editor.document.hyperlink_here().expect("a link");
        assert_eq!(link.text, "www.example.com");
        assert_eq!(link.range, (4, 19));
        editor.document.set_caret(TextPosition::new(0, 21));
        assert!(editor.document.hyperlink_here().is_none(), "the link ran on");
    }

    #[test]
    fn three_hyphens_and_enter_make_a_line_under_the_paragraph_above() {
        let mut editor = editor();
        type_out(&mut editor, "Heading");
        press(&mut editor, Key::Enter);
        type_out(&mut editor, "---");
        press(&mut editor, Key::Enter);

        assert_eq!(editor.document.paragraph_count(), 2, "{:?}", text(&editor));
        assert_eq!(editor.document.paragraph_text(1).as_deref(), Some(""));
        editor.document.set_caret(TextPosition::new(0, 0));
        let borders = editor.document.borders_here();
        let bottom = borders.bottom.expect("a line under the heading");
        assert_eq!(bottom.style, "single");
        editor.document.set_caret(TextPosition::new(1, 0));
        assert!(editor.document.borders_here().bottom.is_none(), "the line came down too");
    }

    #[test]
    fn a_line_at_the_top_of_the_document_goes_under_the_first_paragraph() {
        let mut editor = editor();
        type_out(&mut editor, "===");
        press(&mut editor, Key::Enter);
        assert_eq!(editor.document.paragraph_count(), 2);
        assert_eq!(editor.document.caret(), TextPosition::new(1, 0), "the caret is above the line");
        editor.document.set_caret(TextPosition::new(0, 0));
        let bottom = editor.document.borders_here().bottom.expect("a line");
        assert_eq!(bottom.style, "double");
    }

    #[test]
    fn a_line_is_not_made_while_it_is_switched_off() {
        let mut editor = editor();
        editor.autocorrect.border_lines = false;
        type_out(&mut editor, "---");
        press(&mut editor, Key::Enter);
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("---"));
    }

    // --- The little box ------------------------------------------------------

    /// Rests the pointer on a position, which is what makes the box appear.
    fn point_at(editor: &mut Editor, at: TextPosition) -> (i32, i32) {
        let (x, y, _, height) = editor.caret_rect_at(at).expect("on the page");
        let (x, y) = ((x + 2.0) as i32, (y + height / 2.0) as i32);
        editor.handle(Event::MouseMove { x, y, held: false, modifiers: Modifiers::default() });
        (x, y)
    }

    /// Points at the word, and then at the bar that appears under it, which
    /// is what brings the box out.
    fn reach_box(editor: &mut Editor, at: TextPosition) {
        point_at(editor, at);
        let (x, y) = editor.correction_bar().expect("the bar under the word");
        editor.handle(Event::MouseMove {
            x: (x + 2.0) as i32,
            y: (y + 1.0) as i32,
            held: false,
            modifiers: Modifiers::default(),
        });
    }

    #[test]
    fn a_bar_comes_first_and_the_box_when_the_pointer_reaches_it() {
        let mut editor = editor();
        type_out(&mut editor, "teh cat");
        assert!(editor.correction_bar().is_none(), "it appeared before the pointer came");
        assert!(editor.correction_badge().is_none());

        // Resting on the word: the thin bar under its first letter, and no box
        // yet — a pointer crossing the word on its way elsewhere drops nothing
        // on the text.
        point_at(&mut editor, TextPosition::new(0, 1));
        let (bar_x, bar_y) = editor.correction_bar().expect("the bar");
        let (word_x, word_y, _, height) = editor.caret_rect_at(TextPosition::new(0, 0)).unwrap();
        assert!((bar_x - word_x).abs() < 0.5 && (bar_y - (word_y + height)).abs() < 0.5);
        assert!(editor.correction_badge().is_none());

        // Onto the bar: the box, with the lightning bolt.
        reach_box(&mut editor, TextPosition::new(0, 1));
        let badge = editor.correction_badge().expect("the box");
        assert_eq!(badge.icon, Icon::Lightning);
        assert!(editor.correction_bar().is_none(), "the bar became the box");

        // Back on the word, the box stays; off the word and the box, it goes.
        point_at(&mut editor, TextPosition::new(0, 1));
        assert!(editor.correction_badge().is_some());
        point_at(&mut editor, TextPosition::new(0, 6));
        assert!(editor.correction_badge().is_none());
        assert!(editor.correction_bar().is_none());
    }

    #[test]
    fn the_box_offers_the_word_back_and_gives_it() {
        let mut editor = editor();
        type_out(&mut editor, "teh cat");
        reach_box(&mut editor, TextPosition::new(0, 1));
        editor.open_correction_options();
        let popup = editor.popup.as_ref().expect("the list");
        assert_eq!(popup.choice, Choice::AutoCorrectOption);
        assert_eq!(popup.item(TAKE_BACK), Some("Change back to \u{201C}teh\u{201D}"));
        assert_eq!(popup.item(STOP), Some("Stop Automatically Correcting \u{201C}teh\u{201D}"));

        // Three words later, when an undo would take the words first.
        editor.choose_correction_option(TAKE_BACK);
        assert_eq!(text(&editor), "teh cat");
        assert!(editor.corrected.is_none(), "the box is still about a correction that is gone");
        assert!(editor.autocorrect.on_word("a teh").is_some(), "the rule was stopped as well");
    }

    #[test]
    fn stopping_a_correction_takes_it_back_and_stops_it_for_good() {
        let mut editor = editor();
        type_out(&mut editor, "teh ");
        reach_box(&mut editor, TextPosition::new(0, 1));
        editor.open_correction_options();
        editor.choose_correction_option(STOP);
        assert_eq!(text(&editor), "teh ");
        assert_eq!(editor.autocorrect.on_word("a teh"), None);
        assert!(
            editor
                .settings
                .autocorrect
                .as_ref()
                .is_some_and(|rules| !rules.replacements.contains_key("teh")),
            "the settings still have the pair"
        );

        type_out(&mut editor, "teh ");
        assert_eq!(text(&editor), "teh teh ");
    }

    #[test]
    fn the_box_takes_back_a_capital_and_names_what_it_undoes() {
        let mut editor = editor();
        type_out(&mut editor, "hello ");
        reach_box(&mut editor, TextPosition::new(0, 1));
        editor.open_correction_options();
        assert_eq!(
            editor.popup.as_ref().unwrap().item(TAKE_BACK),
            Some("Undo Automatic Capitalization")
        );
        editor.choose_correction_option(TAKE_BACK);
        assert_eq!(text(&editor), "hello ");
    }

    #[test]
    fn the_box_takes_a_list_back_and_puts_the_marker_back() {
        let mut editor = editor();
        type_out(&mut editor, "- milk");
        reach_box(&mut editor, TextPosition::new(0, 0));
        editor.open_correction_options();
        assert_eq!(
            editor.popup.as_ref().unwrap().item(TAKE_BACK),
            Some("Undo Automatic Numbering")
        );
        editor.choose_correction_option(TAKE_BACK);
        assert!(editor.document.list_here().is_none());
        assert_eq!(text(&editor), "- milk");
    }

    #[test]
    fn the_box_takes_a_line_back() {
        let mut editor = editor();
        type_out(&mut editor, "Heading");
        press(&mut editor, Key::Enter);
        type_out(&mut editor, "---");
        press(&mut editor, Key::Enter);
        reach_box(&mut editor, TextPosition::new(0, 0));
        editor.open_correction_options();
        editor.choose_correction_option(TAKE_BACK);
        editor.document.set_caret(TextPosition::new(0, 0));
        assert!(editor.document.borders_here().bottom.is_none(), "the line stayed");
    }

    #[test]
    fn the_box_goes_when_the_word_is_edited() {
        let mut editor = editor();
        type_out(&mut editor, "teh ");
        editor.document.set_caret(TextPosition::new(0, 3));
        press(&mut editor, Key::Backspace);
        point_at(&mut editor, TextPosition::new(0, 1));
        assert!(editor.correction_badge().is_none(), "the box points at a word that is gone");
    }

    #[test]
    fn a_capital_undone_after_an_abbreviation_teaches_the_abbreviation() {
        let mut editor = editor();
        editor.autocorrect.add_first_letter_exceptions = true;
        type_out(&mut editor, "Sent Wed. hello ");
        assert!(text(&editor).contains("Hello"), "got {:?}", text(&editor));

        // Ctrl+Z straight away: the word back, and the lesson learnt.
        editor.handle(Event::KeyDown {
            key: Key::Letter('z'),
            modifiers: Modifiers { control: true, ..Modifiers::default() },
        });
        assert!(text(&editor).contains("hello"), "got {:?}", text(&editor));
        assert!(editor.autocorrect.first_letter.contains("wed."), "nothing was learnt");

        type_out(&mut editor, "Sent Wed. hello ");
        assert!(
            !text(&editor).contains("Hello"),
            "the lesson was not applied: {:?}",
            text(&editor)
        );
    }

    #[test]
    fn nothing_is_learnt_when_the_list_does_not_add_to_itself() {
        let mut editor = editor();
        editor.autocorrect.add_first_letter_exceptions = false;
        type_out(&mut editor, "Sent Wed. hello ");
        editor.handle(Event::KeyDown {
            key: Key::Letter('z'),
            modifiers: Modifiers { control: true, ..Modifiers::default() },
        });
        assert!(!editor.autocorrect.first_letter.contains("wed."));
    }

    #[test]
    fn escape_puts_the_box_away() {
        let mut editor = editor();
        type_out(&mut editor, "teh ");
        reach_box(&mut editor, TextPosition::new(0, 1));
        assert!(editor.correction_badge().is_some());
        press(&mut editor, Key::Escape);
        assert!(editor.corrected.is_none());

        // And the bar, before it is a box.
        type_out(&mut editor, "teh ");
        point_at(&mut editor, TextPosition::new(0, 5));
        assert!(editor.correction_bar().is_some());
        press(&mut editor, Key::Escape);
        assert!(editor.corrected.is_none());
    }

    #[test]
    fn a_math_name_in_the_text_is_left_alone_unless_the_tab_says_otherwise() {
        let mut editor = editor();
        type_out(&mut editor, "\\alpha is a command");
        assert_eq!(text(&editor), "\\alpha is a command", "Word ships it off");

        let mut editor = super::tests::editor();
        editor.autocorrect.math_outside = true;
        type_out(&mut editor, "An angle \\alpha of ");
        assert_eq!(text(&editor), "An angle \u{3B1} of ");
        // And the box offers it back, naming it.
        reach_box(&mut editor, TextPosition::new(0, 9));
        editor.open_correction_options();
        assert_eq!(
            editor.popup.as_ref().unwrap().item(STOP),
            Some("Stop Automatically Correcting \u{201C}\\alpha\u{201D}")
        );
        editor.choose_correction_option(TAKE_BACK);
        assert_eq!(text(&editor), "An angle \\alpha of ");
    }

    // --- Tables and headings as they are typed ---------------------------------

    #[test]
    fn plus_signs_and_hyphens_and_enter_make_a_table_where_the_line_was() {
        let mut editor = editor();
        type_out(&mut editor, "Before");
        press(&mut editor, Key::Enter);
        type_out(&mut editor, "+----+--------+");
        press(&mut editor, Key::Enter);

        // The line is gone and a table of one row and two columns stands in
        // its place, with the caret in its first cell and a paragraph after.
        let place = editor.document.table_here().expect("a table at the caret");
        assert_eq!((place.rows, place.columns, place.row, place.column), (1, 2, 0, 0));
        assert_eq!(editor.document.caret(), TextPosition::new(1, 0));
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("Before"));
        assert!(!editor.document.paragraph_in_table(0));
        assert!(!text(&editor).contains('+'), "{:?}", text(&editor));
        assert_eq!(editor.document.paragraph_count(), 4, "before, two cells, and one after");

        // Each column as wide as its hyphens were on the page: the second
        // twice the first, near enough — the plus signs take room of their
        // own, the same on each side.
        let grid = editor.document.table_grid_at(1);
        assert_eq!(grid.len(), 2);
        let ratio = grid[1] as f32 / grid[0] as f32;
        assert!((1.6..2.1).contains(&ratio), "{grid:?}");
        // Measured in the line's own font, not guessed: fifteen characters of
        // an eleven-point font are an inch or so, nothing like the page.
        assert!(grid.iter().sum::<i32>() < 3000, "{grid:?}");

        // One undo takes it back and leaves what was typed.
        editor.handle(Event::KeyDown {
            key: Key::Letter('z'),
            modifiers: Modifiers { control: true, ..Modifiers::default() },
        });
        assert!(editor.document.table_here().is_none());
        assert_eq!(editor.document.paragraph_text(1).as_deref(), Some("+----+--------+"));
    }

    #[test]
    fn a_line_of_one_column_is_a_table_of_one_column() {
        let mut editor = editor();
        type_out(&mut editor, "+------+");
        press(&mut editor, Key::Enter);
        let place = editor.document.table_here().expect("a table");
        assert_eq!((place.rows, place.columns), (1, 1));
    }

    #[test]
    fn a_table_is_not_made_while_it_is_switched_off() {
        let mut editor = editor();
        editor.autocorrect.tables = false;
        type_out(&mut editor, "+---+---+");
        press(&mut editor, Key::Enter);
        assert!(editor.document.table_at(0).is_none());
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("+---+---+"));
    }

    #[test]
    fn the_box_takes_a_table_back_and_puts_the_line_back() {
        let mut editor = editor();
        type_out(&mut editor, "+---+---+");
        press(&mut editor, Key::Enter);
        // Typing below the table, so an undo would take that first.
        editor.document.set_caret(TextPosition::new(2, 0));
        type_out(&mut editor, "after");
        reach_box(&mut editor, TextPosition::new(0, 0));
        editor.open_correction_options();
        assert_eq!(editor.popup.as_ref().unwrap().item(TAKE_BACK), Some("Undo Automatic Table"));
        editor.choose_correction_option(TAKE_BACK);
        assert!(editor.document.table_at(0).is_none());
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("+---+---+after"));
    }

    #[test]
    fn a_table_with_words_in_it_is_not_taken_back() {
        let mut editor = editor();
        type_out(&mut editor, "+---+---+");
        press(&mut editor, Key::Enter);
        type_out(&mut editor, "Name");
        point_at(&mut editor, TextPosition::new(0, 0));
        assert!(editor.correction_bar().is_none() && editor.correction_badge().is_none());
    }

    #[test]
    fn a_line_entered_twice_becomes_a_heading_where_that_is_switched_on() {
        let mut editor = editor();
        editor.autocorrect.headings = true;
        type_out(&mut editor, "Introduction");
        press(&mut editor, Key::Enter);
        press(&mut editor, Key::Enter);

        assert_eq!(editor.document.style_of(0).as_deref(), Some("Heading1"));
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("Introduction"));
        assert_eq!(editor.document.paragraph_count(), 2, "the second Enter made nothing");
        assert_eq!(editor.document.caret(), TextPosition::new(1, 0));
        assert_ne!(editor.document.style_of(1).as_deref(), Some("Heading1"));

        // One undo: the plain line again, and the empty paragraph under it.
        editor.handle(Event::KeyDown {
            key: Key::Letter('z'),
            modifiers: Modifiers { control: true, ..Modifiers::default() },
        });
        assert_ne!(editor.document.style_of(0).as_deref(), Some("Heading1"));
        assert_eq!(editor.document.paragraph_count(), 2);
    }

    #[test]
    fn a_tab_in_front_of_the_line_is_a_level_down() {
        let mut editor = editor();
        editor.autocorrect.headings = true;
        editor.document.type_text("\tWhat came before");
        press(&mut editor, Key::Enter);
        press(&mut editor, Key::Enter);
        assert_eq!(editor.document.style_of(0).as_deref(), Some("Heading2"));
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("What came before"));
    }

    #[test]
    fn a_heading_is_not_made_of_a_sentence_or_while_switched_off() {
        // Word ships it off: two Enters are two paragraphs.
        let mut editor = editor();
        type_out(&mut editor, "Introduction");
        press(&mut editor, Key::Enter);
        press(&mut editor, Key::Enter);
        assert_eq!(editor.document.paragraph_count(), 3);
        assert_ne!(editor.document.style_of(0).as_deref(), Some("Heading1"));

        // And switched on, a line that ends a sentence is not one.
        let mut editor = super::tests::editor();
        editor.autocorrect.headings = true;
        type_out(&mut editor, "It was late.");
        press(&mut editor, Key::Enter);
        press(&mut editor, Key::Enter);
        assert_eq!(editor.document.paragraph_count(), 3);
        assert_ne!(editor.document.style_of(0).as_deref(), Some("Heading1"));
    }

    #[test]
    fn a_line_longer_than_a_line_is_not_a_heading() {
        let mut editor = editor();
        editor.autocorrect.headings = true;
        let long = "A title that goes on ".repeat(12);
        editor.document.type_text(long.trim_end());
        editor.relayout();
        press(&mut editor, Key::Enter);
        press(&mut editor, Key::Enter);
        assert_ne!(editor.document.style_of(0).as_deref(), Some("Heading1"));
    }

    #[test]
    fn the_box_takes_a_heading_back() {
        let mut editor = editor();
        editor.autocorrect.headings = true;
        editor.document.type_text("\tMethods");
        press(&mut editor, Key::Enter);
        press(&mut editor, Key::Enter);
        type_out(&mut editor, "Body text");
        reach_box(&mut editor, TextPosition::new(0, 1));
        editor.open_correction_options();
        assert_eq!(
            editor.popup.as_ref().unwrap().item(TAKE_BACK),
            Some("Undo Automatic Heading Style")
        );
        editor.choose_correction_option(TAKE_BACK);
        assert_ne!(editor.document.style_of(0).as_deref(), Some("Heading2"));
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("\tMethods"));
        assert_eq!(editor.document.paragraph_text(1).as_deref(), Some("Body text"));
    }
}

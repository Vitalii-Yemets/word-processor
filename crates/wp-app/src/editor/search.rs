//! Finding and replacing, and the strip that drives it.

use wp_docx::search::Matching;
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Focus, Hit, Purpose};

use super::Editor;

impl Editor {
    /// Opens the find strip, or the find-and-replace one.
    pub(super) fn open_find(&mut self, replacing: bool) -> Response {
        match &mut self.find_bar {
            // Already open: pressing the command again grows it into the
            // replace form, or moves on to the next match.
            Some(bar) if replacing && !bar.replacing => {
                bar.replacing = true;
                bar.focus = Focus::Replace;
            }
            Some(_) => return self.find_step(true),
            None => {
                let mut bar = FindBar::new(replacing);
                // Whatever is selected is what a person means to look for,
                // which saves typing it out again.
                let selected = self.document.selected_text();
                if !selected.is_empty() && !selected.contains('\n') {
                    bar.needle = selected;
                }
                self.find_bar = Some(bar);
            }
        }
        self.count_matches();
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Closes the strip.
    pub(super) fn close_find(&mut self) -> Response {
        if self.find_bar.take().is_none() {
            return Response::Ignored;
        }
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Whether the strip has the keyboard.
    #[must_use]
    pub(super) fn find_has_keyboard(&self) -> bool {
        self.find_bar.is_some()
    }

    /// Types into whichever of the strip's fields has the keyboard.
    pub(super) fn type_into_find(&mut self, character: char) -> Response {
        let Some(bar) = &mut self.find_bar else { return Response::Ignored };

        match character {
            // Enter finds the next one, which is what it does in every search
            // box ever made — or finishes the comment, when that is what the
            // strip was opened for.
            '\r' | '\n' => match bar.purpose {
                Purpose::Comment => return self.finish_comment(),
                Purpose::Note => return self.finish_note(),
                Purpose::Caption => return self.finish_caption(),
                Purpose::Bookmark => return self.finish_bookmark(),
                Purpose::IndexEntry => return self.finish_index_entry(),
                Purpose::Source => return self.finish_source(),
                Purpose::Link => return self.finish_link(),
                Purpose::Watermark => return self.finish_watermark(),
                Purpose::Property => return self.finish_property(),
                Purpose::Authority => return self.finish_authority(),
                Purpose::TextBox => return self.finish_text_box(),
                Purpose::WordArt => return self.finish_word_art(),
                Purpose::RowHeight => return self.finish_row_height(),
                Purpose::Chart => return self.finish_chart(),
                Purpose::Rule => return self.finish_rule(),
                Purpose::Equation => return self.finish_equation(),
                Purpose::Video => return self.finish_video(),
                Purpose::Macro => return self.finish_macro(),
                Purpose::Diagram => return self.finish_diagram(),
                Purpose::Signature => return self.finish_signature_line(),
                Purpose::Envelope => return self.finish_envelope(),
                Purpose::Label => return self.finish_labels(),
                Purpose::Find | Purpose::Replace => return self.find_step(true),
            },
            '\u{1b}' => return self.close_find(),
            // Tab moves between the two fields.
            '\t' => {
                if bar.replacing {
                    bar.focus = if bar.focus == Focus::Find { Focus::Replace } else { Focus::Find };
                }
            }
            character => {
                if bar.type_character(character) {
                    self.count_matches();
                    // Searching as you type: the first match is shown before
                    // anybody presses anything.
                    self.jump_to_match(true, false);
                }
            }
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Reacts to a press on the strip.
    pub(super) fn pressed_in_find(&mut self, x: i32, y: i32) -> Response {
        let Some(bar) = &mut self.find_bar else { return Response::Ignored };
        let Some(hit) = bar.hit(x, y) else { return Response::Ignored };

        match hit {
            Hit::FindField => bar.focus = Focus::Find,
            Hit::ReplaceField => bar.focus = Focus::Replace,
            Hit::FindNext => return self.find_step(true),
            Hit::FindPrevious => return self.find_step(false),
            Hit::Replace => return self.replace_one(),
            Hit::ReplaceAll => return self.replace_all(),
            Hit::MatchCase => {
                bar.match_case = !bar.match_case;
                self.count_matches();
            }
            Hit::WholeWord => {
                bar.whole_word = !bar.whole_word;
                self.count_matches();
            }
            Hit::Close => return self.close_find(),
            Hit::Add => {
                return match bar.purpose {
                    Purpose::Note => self.finish_note(),
                    Purpose::Caption => self.finish_caption(),
                    Purpose::Bookmark => self.finish_bookmark(),
                    Purpose::IndexEntry => self.finish_index_entry(),
                    Purpose::Source => self.finish_source(),
                    Purpose::Link => self.finish_link(),
                    Purpose::Watermark => self.finish_watermark(),
                    Purpose::Property => self.finish_property(),
                    Purpose::Authority => self.finish_authority(),
                    Purpose::TextBox => self.finish_text_box(),
                    Purpose::WordArt => self.finish_word_art(),
                    Purpose::RowHeight => self.finish_row_height(),
                    Purpose::Chart => self.finish_chart(),
                    Purpose::Rule => self.finish_rule(),
                    Purpose::Equation => self.finish_equation(),
                    Purpose::Video => self.finish_video(),
                    Purpose::Macro => self.finish_macro(),
                    Purpose::Diagram => self.finish_diagram(),
                    Purpose::Signature => self.finish_signature_line(),
                    Purpose::Envelope => self.finish_envelope(),
                    Purpose::Label => self.finish_labels(),
                    _ => self.finish_comment(),
                };
            }
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Moves to the next match, or the previous one.
    fn find_step(&mut self, forwards: bool) -> Response {
        if self.find_bar.as_ref().is_some_and(|bar| bar.needle.is_empty()) {
            return Response::Ignored;
        }
        if self.jump_to_match(forwards, true) {
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let needle = self.find_bar.as_ref().map(|bar| bar.needle.clone()).unwrap_or_default();
        self.report(&format!("\"{needle}\" is not in the document"))
    }

    /// Puts the caret on a match and selects it.
    ///
    /// `advance` is whether to start looking past the current match rather than
    /// at it, which is the difference between "find the next one" and "show me
    /// where the first one is as I type".
    fn jump_to_match(&mut self, forwards: bool, advance: bool) -> bool {
        let Some(bar) = &self.find_bar else { return false };
        if bar.needle.is_empty() {
            return false;
        }

        let matches = self.all_matches();
        if matches.is_empty() {
            return false;
        }

        let caret = self.caret();
        let place = |position: &TextPosition| (position.paragraph, position.offset);
        let here = place(&caret);
        let wanted = if forwards {
            // Round the end and back to the beginning, which is what a search
            // that stopped at the last page would fail to do.
            matches
                .iter()
                .find(|(at, _)| if advance { place(at) > here } else { place(at) >= here })
                .or_else(|| matches.first())
        } else {
            matches.iter().rev().find(|(at, _)| place(at) < here).or_else(|| matches.last())
        };

        let Some((found, end)) = wanted.copied() else { return false };
        self.document.set_caret(found);
        self.document.extend_selection_to(TextPosition::new(found.paragraph, end));
        self.reveal_caret();
        true
    }

    /// Every place the needle appears, in reading order, with where each ends.
    ///
    /// A match is not always as long as what was typed into the box: a search
    /// that does not mind capitals matches a Turkish `İ` with an `i`, and one
    /// that does not mind how an accent was written matches two characters
    /// with one. So the end is carried rather than worked out.
    fn all_matches(&self) -> Vec<(TextPosition, usize)> {
        let Some(bar) = &self.find_bar else { return Vec::new() };
        let mut found = self.document.find_all(&bar.needle, self.matching());
        found.truncate(5000);
        found
    }

    /// What the strip's toggles say a search should mind about.
    fn matching(&self) -> Matching {
        let Some(bar) = &self.find_bar else { return Matching::default() };
        Matching { match_case: bar.match_case, whole_word: bar.whole_word }
    }

    /// Counts what the current needle finds, for the strip to show.
    fn count_matches(&mut self) {
        let found = self.all_matches().len();
        if let Some(bar) = &mut self.find_bar {
            bar.found = found;
        }
    }

    /// Replaces the match that is selected, then moves to the next.
    fn replace_one(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let (needle, replacement) = (bar.needle.clone(), bar.replacement.clone());
        if needle.is_empty() {
            return Response::Ignored;
        }

        // Only replace when the selection really is a match; otherwise this is
        // a "find the first one" press. The selection is asked the same way the
        // search asks the document, so what was highlighted is what counts as a
        // match — capitals, accents and whole words included.
        let selected = self.document.selected_text();
        let hit = wp_docx::search::matches(&selected, &needle, self.matching())
            .first()
            .is_some_and(|found| found.start == 0 && found.end == selected.len());
        if !hit {
            return self.find_step(true);
        }

        let changed = self.document.paste(&replacement);
        if changed {
            self.relayout();
        }
        self.count_matches();
        self.jump_to_match(true, false);
        self.needs_redraw = true;
        self.status = format!("Replaced one of \"{needle}\"");
        Response::Redraw
    }

    /// Replaces every match at once.
    fn replace_all(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let (needle, replacement) = (bar.needle.clone(), bar.replacement.clone());
        if needle.is_empty() {
            return Response::Ignored;
        }

        // What is replaced is what the search finds, and nothing else: the
        // count the strip has been showing is the number that will change.
        let count = self.document.replace_matching(&needle, &replacement, self.matching());
        if count > 0 {
            self.relayout();
        }
        self.count_matches();
        self.needs_redraw = true;
        self.status = match count {
            0 => format!("\"{needle}\" is not in the document"),
            1 => "Replaced one".to_owned(),
            many => format!("Replaced {many}"),
        };
        Response::Redraw
    }
}

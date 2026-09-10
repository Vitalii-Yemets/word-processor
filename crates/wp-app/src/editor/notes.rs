//! Footnotes and endnotes, from the References tab.

use wp_docx::notes::Kind;
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::findbar::FindBar;

use super::Editor;

impl Editor {
    /// Opens the strip that takes the text of a new note.
    pub(super) fn start_note(&mut self, kind: Kind) -> Response {
        self.note_kind = kind;
        self.find_bar = Some(FindBar::for_note());
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report(match kind {
            Kind::Footnote => "Type the footnote, then press Enter",
            Kind::Endnote => "Type the endnote, then press Enter",
        })
    }

    /// Puts whatever was typed into that strip at the caret.
    pub(super) fn finish_note(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let text = bar.needle.trim().to_owned();
        if text.is_empty() {
            return self.close_find();
        }

        let kind = self.note_kind;
        match self.document.add_note(kind, &text) {
            Ok(_) => {
                self.find_bar = None;
                self.relayout();
                self.reveal_caret();
                self.report(match kind {
                    Kind::Footnote => "Footnote added",
                    Kind::Endnote => "Endnote added",
                })
            }
            Err(error) => {
                wp_shell::dialog::show_error(&format!("Cannot add the note: {error}"));
                Response::Ignored
            }
        }
    }

    /// Moves the caret to the next note mark, of either kind.
    ///
    /// What the button's face does. Its arrow asks which kind and which way,
    /// and goes to [`Editor::step_note_of`].
    pub(super) fn step_note(&mut self) -> Response {
        let mut marks: Vec<(TextPosition, String)> = Vec::new();
        for kind in [Kind::Footnote, Kind::Endnote] {
            for note in self.document.notes(kind) {
                if let Some(mark) = note.mark {
                    marks.push((mark, note.text));
                }
            }
        }
        self.go_to_note(marks, true, "This document has no notes")
    }

    /// The same for one kind of note, in either direction.
    pub(super) fn step_note_of(&mut self, kind: Kind, forwards: bool) -> Response {
        let marks: Vec<(TextPosition, String)> = self
            .document
            .notes(kind)
            .into_iter()
            .filter_map(|note| note.mark.map(|mark| (mark, note.text)))
            .collect();
        let nothing = match kind {
            Kind::Footnote => "This document has no footnotes",
            Kind::Endnote => "This document has no endnotes",
        };
        self.go_to_note(marks, forwards, nothing)
    }

    /// Goes to the nearest of a set of marks, wrapping round the document.
    ///
    /// Wrapping is what Word does: the notes are a ring, and stepping past the
    /// last one comes back to the first rather than stopping dead.
    fn go_to_note(
        &mut self,
        mut marks: Vec<(TextPosition, String)>,
        forwards: bool,
        nothing: &str,
    ) -> Response {
        if marks.is_empty() {
            return self.report(nothing);
        }
        marks.sort_by_key(|(mark, _)| (mark.paragraph, mark.offset));

        let caret = self.caret();
        let after =
            |mark: &TextPosition| (mark.paragraph, mark.offset) > (caret.paragraph, caret.offset);
        let before =
            |mark: &TextPosition| (mark.paragraph, mark.offset) < (caret.paragraph, caret.offset);

        let found = if forwards {
            marks.iter().find(|(mark, _)| after(mark)).or_else(|| marks.first())
        } else {
            marks.iter().rev().find(|(mark, _)| before(mark)).or_else(|| marks.last())
        };
        let (mark, text) = found.cloned().expect("the list is not empty");

        self.document.set_caret(mark);
        self.reveal_caret();
        self.needs_redraw = true;
        self.report(&text)
    }

    /// Removes the note whose mark is nearest the caret.
    pub(super) fn delete_note_here(&mut self) -> Response {
        let caret = self.caret();
        let mut nearest: Option<(usize, Kind, i32)> = None;

        for kind in [Kind::Footnote, Kind::Endnote] {
            for note in self.document.notes(kind) {
                let Some(mark) = note.mark else { continue };
                if mark.paragraph != caret.paragraph {
                    continue;
                }
                let distance = mark.offset.abs_diff(caret.offset);
                if nearest.is_none_or(|(best, ..)| distance < best) {
                    nearest = Some((distance, kind, note.id));
                }
            }
        }

        let Some((_, kind, id)) = nearest else {
            return self.report("There is no note in this paragraph");
        };
        let changed = self.document.delete_note(kind, id);
        self.edited(changed, "Note deleted")
    }
}

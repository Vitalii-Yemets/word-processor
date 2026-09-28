//! Word's additional actions on the right-click menu: what the text under the
//! pointer was recognised as, and the offers made for it. The recognising is
//! [`crate::actions`]; this is where an offer taken goes into the document.

use wp_docx::TextPosition;
use wp_shell::Response;

use crate::actions::Offer;
use crate::locale;

use super::Editor;

/// What one of the menu's additional actions puts in, and in place of what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pending {
    pub paragraph: usize,
    pub start: usize,
    pub end: usize,
    pub putting: String,
}

impl Editor {
    /// The offers for whatever is recognised at a place, each with where it
    /// goes; empty where nothing is, or the Actions tab has them off.
    pub(super) fn actions_at(&self, at: TextPosition) -> Vec<(Offer, Pending)> {
        let Some(text) = self.document.paragraph_text(at.paragraph) else { return Vec::new() };
        let machine = locale::current();
        let day_first = machine.date_order != wp_shell::locale::DateOrder::MonthDayYear;
        let found = self.autocorrect.actions.at(&text, at.offset, &machine.months, day_first);
        let mut out = Vec::new();
        for thing in found {
            let written = &text[thing.start..thing.end];
            for offer in crate::actions::offers(&thing, written, machine.decimal == ',') {
                let pending = Pending {
                    paragraph: at.paragraph,
                    start: thing.start,
                    end: thing.end,
                    putting: offer.putting.clone(),
                };
                out.push((offer, pending));
            }
        }
        out
    }

    /// One of them taken: what was recognised goes, and what was offered
    /// comes, as one thing to undo.
    pub(super) fn take_action(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(pending) = self.pending_actions.get(index).cloned() else {
            return Response::Ignored;
        };
        self.pending_actions.clear();
        let at = |offset| TextPosition::new(pending.paragraph, offset);
        self.document.begin_gesture();
        self.document.move_caret(at(pending.start), false);
        self.document.move_caret(at(pending.end), true);
        let changed = self.document.paste(&pending.putting);
        self.document.end_gesture();
        self.relayout();
        self.reveal_caret();
        self.edited(
            changed,
            &crate::messages::with("Changed to \u{201C}{0}\u{201D}", &[&pending.putting]),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::chrome::popup::Kind;
    use crate::chrome::Choice;
    use crate::editor::Editor;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn editor(text: &str) -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text(text)));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
        let mut editor = Editor::new(library, document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// Right-clicks the text at an offset of the first paragraph.
    fn right_click(editor: &mut Editor, offset: usize) {
        let (x, y, _, height) =
            editor.caret_rect_at(TextPosition::new(0, offset)).expect("on the page");
        editor.open_context_menu((x + 1.0) as i32, (y + height / 2.0) as i32);
    }

    fn menu(editor: &Editor) -> Vec<(String, Kind)> {
        let popup = editor.popup.as_ref().expect("the menu");
        assert_eq!(popup.choice, Choice::Context);
        (0..)
            .map_while(|index| {
                popup.item(index).map(|label| (label.to_owned(), popup.row(index).kind))
            })
            .collect()
    }

    #[test]
    fn a_measurement_is_offered_in_the_other_system_and_converted() {
        let mut editor = editor("A pipe 5 inches long.");
        editor.autocorrect.actions.enabled = true;
        right_click(&mut editor, 9);
        let lines = menu(&editor);
        let heading = lines.iter().position(|(label, _)| label == "Additional Actions");
        let heading = heading.expect("no Additional Actions on the menu");
        assert_eq!(lines[heading].1, Kind::Disabled, "the heading is a heading");
        let convert = lines
            .iter()
            .position(|(label, _)| label.trim() == "Convert to 12.7 cm")
            .expect("no conversion offered");

        editor.choose_context_entry(convert);
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("A pipe 12.7 cm long."));
        // One undo takes it back.
        editor.document.undo();
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("A pipe 5 inches long."));
    }

    #[test]
    fn a_date_is_offered_written_another_way() {
        let mut editor = editor("Due on 28 September 2026 at noon.");
        editor.autocorrect.actions.enabled = true;
        right_click(&mut editor, 12);
        let lines = menu(&editor);
        let iso = lines
            .iter()
            .position(|(label, _)| label.trim() == "Change to 2026-09-28")
            .expect("no other way of writing it");
        editor.choose_context_entry(iso);
        assert_eq!(
            editor.document.paragraph_text(0).as_deref(),
            Some("Due on 2026-09-28 at noon.")
        );
    }

    #[test]
    fn nothing_is_offered_while_the_tab_has_it_off() {
        let mut editor = editor("A pipe 5 inches long.");
        right_click(&mut editor, 9);
        assert!(!menu(&editor).iter().any(|(label, _)| label == "Additional Actions"));

        // Nor away from anything recognised.
        let mut editor = super::tests::editor("A pipe 5 inches long.");
        editor.autocorrect.actions.enabled = true;
        right_click(&mut editor, 2);
        assert!(!menu(&editor).iter().any(|(label, _)| label == "Additional Actions"));
    }
}

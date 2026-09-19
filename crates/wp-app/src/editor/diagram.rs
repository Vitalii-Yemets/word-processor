//! Diagrams, from the Insert tab's SmartArt button.
//!
//! Two steps, because a diagram is two questions: which arrangement, and what
//! goes in it. The arrangement is a list; the words are typed into the strip,
//! one box per semicolon, the same way a source and an address are typed.

use wp_docx::diagram::Arrangement;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{Choice, Command, Popup};
use crate::messages::{t, with};

use super::Editor;

impl Editor {
    /// Drops open the arrangements a diagram can take.
    pub(super) fn open_diagram(&mut self) -> Response {
        if self.close_popup_if(Choice::Diagram) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::SmartArt) else {
            return Response::Ignored;
        };

        let items =
            Arrangement::ALL.iter().map(|entry| t(entry.label()).to_owned()).collect::<Vec<_>>();
        self.popup = Some(Popup::new(Choice::Diagram, items, None, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Remembers the arrangement and asks what goes in the boxes.
    pub(super) fn choose_diagram(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(arrangement) = Arrangement::ALL.get(index).copied() else {
            return Response::Ignored;
        };
        self.diagram_arrangement = arrangement;

        self.find_bar = Some(FindBar::for_purpose(Purpose::Diagram));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report(t("Type what the boxes say, with semicolons between them"))
    }

    /// Draws the diagram.
    pub(super) fn finish_diagram(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.clone();
        self.find_bar = None;
        self.needs_redraw = true;

        let items: Vec<String> = typed
            .split(';')
            .map(|part| part.trim().to_owned())
            .filter(|part| !part.is_empty())
            .collect();
        if items.is_empty() {
            return self.report(t("Nothing was typed, so no diagram was drawn"));
        }

        let room = self.text_width_emu();
        match self.document.insert_diagram(self.diagram_arrangement, &items, room) {
            Ok(true) => {
                self.relayout();
                self.reveal_caret();
                // The diagram comes up chosen, with its tab and its Text Pane,
                // which is how Word hands a new one over.
                self.choose_drawing_here();
                self.note_diagram_chosen();
                self.open_text_pane();
                let named = t(self.diagram_arrangement.label());
                self.edited(true, &with("{0}, {1} boxes", &[named, &items.len().to_string()]))
            }
            Ok(false) => self.report(t("The diagram could not be drawn")),
            Err(error) => {
                self.report(&with("The diagram could not be drawn: {0}", &[&error.to_string()]))
            }
        }
    }
}

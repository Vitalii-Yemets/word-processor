//! Envelopes and labels, from the Mailings tab.
//!
//! Each makes a document of its own rather than changing the one open, because
//! a letter and its envelope are different sizes of paper and this program has
//! one page size per document. So the open document is put away first — and
//! only after it has been asked about, if there is anything unsaved in it.

use wp_docx::stationery::{ENVELOPES, LABEL_SHEETS};
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

impl Editor {
    /// Drops open the sizes of envelope.
    pub(super) fn open_envelopes(&mut self) -> Response {
        if self.close_popup_if(Choice::Envelope) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Envelopes) else {
            return Response::Ignored;
        };
        let items = ENVELOPES.iter().map(|entry| entry.name.to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Envelope, items, None, left, top, 280.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Remembers the size and asks for the address.
    pub(super) fn choose_envelope(&mut self, index: usize) -> Response {
        self.popup = None;
        if ENVELOPES.get(index).is_none() {
            return Response::Ignored;
        }
        self.stationery_choice = index;

        self.find_bar = Some(FindBar::for_purpose(Purpose::Envelope));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the address, using semicolons between the lines")
    }

    /// Makes the envelope.
    pub(super) fn finish_envelope(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.trim().to_owned();
        self.find_bar = None;
        self.needs_redraw = true;
        if typed.is_empty() {
            return self.report("Nothing was typed, so no envelope was made");
        }
        let Some(envelope) = ENVELOPES.get(self.stationery_choice) else {
            return Response::Ignored;
        };

        // The open document is not thrown away without asking.
        if !self.may_discard() {
            return Response::Ignored;
        }

        // Semicolons rather than line breaks, because the strip takes one line.
        let delivery = typed.replace(';', "\n");
        let sender = self.document.properties().company;
        match wp_docx::Document::create_envelope(envelope, &delivery, &sender) {
            Ok(document) => {
                self.set_document(document, None);
                self.report(&format!("Envelope: {}", envelope.name))
            }
            Err(error) => self.report(&format!("The envelope could not be made: {error}")),
        }
    }

    /// Drops open the sheets of labels.
    pub(super) fn open_labels(&mut self) -> Response {
        if self.close_popup_if(Choice::Label) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Labels) else {
            return Response::Ignored;
        };
        let items = LABEL_SHEETS.iter().map(|entry| entry.name.to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Label, items, None, left, top, 320.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Remembers the sheet and asks what the labels should say.
    pub(super) fn choose_label_sheet(&mut self, index: usize) -> Response {
        self.popup = None;
        if LABEL_SHEETS.get(index).is_none() {
            return Response::Ignored;
        }
        self.stationery_choice = index;

        self.find_bar = Some(FindBar::for_purpose(Purpose::Label));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type what the labels should say, using semicolons between the lines")
    }

    /// Makes the sheet of labels.
    pub(super) fn finish_labels(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.trim().to_owned();
        self.find_bar = None;
        self.needs_redraw = true;
        if typed.is_empty() {
            return self.report("Nothing was typed, so no labels were made");
        }
        let Some(sheet) = LABEL_SHEETS.get(self.stationery_choice) else {
            return Response::Ignored;
        };
        if !self.may_discard() {
            return Response::Ignored;
        }

        let text = typed.replace(';', "\n");
        match wp_docx::Document::create_labels(sheet, &text) {
            Ok(document) => {
                self.set_document(document, None);
                self.report(&format!("{} labels: {}", sheet.rows * sheet.columns, sheet.name))
            }
            Err(error) => self.report(&format!("The labels could not be made: {error}")),
        }
    }
}

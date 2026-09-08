//! Hyperlinks, from the Insert tab.

use wp_docx::links::Destination;
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};

use super::Editor;

impl Editor {
    /// Opens the strip that takes the address of a link.
    pub(super) fn start_link(&mut self) -> Response {
        let mut bar = FindBar::for_purpose(Purpose::Link);
        // A link that is already there is opened for editing rather than laid
        // over, which is what pressing the command inside one means.
        if let Some(link) = self.document.hyperlink_here() {
            bar.needle = match &link.destination {
                Destination::Address(address) => address.clone(),
                Destination::Place(place) => place.clone(),
            };
        }
        self.find_bar = Some(bar);
        self.clamp_scroll();
        self.needs_redraw = true;

        if self.document.selection().is_some() {
            self.report("Type the address, then press Enter")
        } else {
            self.report("Type the address — it will be typed in as the words too")
        }
    }

    /// Makes the link that was typed.
    pub(super) fn finish_link(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.trim().to_owned();
        if typed.is_empty() {
            return self.close_find();
        }
        self.find_bar = None;

        // Replacing a link means taking the old one off first, or the new one
        // would be wrapped round the old.
        if self.document.hyperlink_here().is_some() {
            self.document.remove_hyperlink();
        }

        let changed = self.document.add_hyperlink(&typed, "");
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("Link to {typed}"))
    }

    /// Takes the link off whatever the caret is in.
    pub(super) fn drop_link(&mut self) -> Response {
        if self.document.hyperlink_here().is_none() {
            return self.report("The caret is not in a link");
        }
        let changed = self.document.remove_hyperlink();
        self.relayout();
        self.edited(changed, "Link removed")
    }

    /// Goes where the link under the caret goes.
    ///
    /// A place in the document is gone to; an address is handed to the desktop,
    /// which knows what the person opens pages with.
    pub(super) fn follow_link(&mut self) -> Response {
        let Some(link) = self.document.hyperlink_here() else { return Response::Ignored };
        match link.destination {
            Destination::Place(place) => {
                let Some(mark) = self.document.bookmark(&place) else {
                    return self.report(&format!("There is no bookmark called {place}"));
                };
                self.document.set_caret(TextPosition::new(mark.range.0.paragraph, 0));
                self.reveal_caret();
                self.needs_redraw = true;
                self.report(&format!("Went to {place}"))
            }
            Destination::Address(address) => {
                if wp_shell::desktop::open(&address) {
                    return self.report(&format!("Opened {address}"));
                }
                // Refused rather than failed: the address is shown so it can be
                // read and copied, which is the whole of what is left to do.
                self.report(&format!("This link was not opened: {address}"))
            }
        }
    }

    /// What the status bar says about the link under the caret, if any.
    #[must_use]
    pub(super) fn link_note(&self) -> Option<String> {
        let link = self.document.hyperlink_here()?;
        Some(match link.destination {
            Destination::Address(address) => format!("{address} — Ctrl+click to follow"),
            Destination::Place(place) => format!("Goes to {place} — Ctrl+click to follow"),
        })
    }
}

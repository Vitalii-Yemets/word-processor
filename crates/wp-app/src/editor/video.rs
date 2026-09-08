//! A video from the web, put into the document.
//!
//! # Why this is a link and not a player
//!
//! Word embeds an online video as a picture of the video with a player behind
//! it, and playing it opens a browser engine inside the document window. This
//! program has no browser engine and does not talk to the network, so it has
//! neither the picture — which would have to be fetched — nor anywhere to play
//! the video.
//!
//! What it can do honestly is put the video in the document as what it is: a
//! title and the address it lives at, as a link that opens in whatever the
//! person watches videos with. Printed, that is also more use than a still
//! frame of a video nobody can play off paper.

use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};

use super::Editor;

impl Editor {
    /// Asks for the address of the video.
    pub(super) fn start_video(&mut self) -> Response {
        self.find_bar = Some(FindBar::for_purpose(Purpose::Video));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the address of the video, then a semicolon and its title")
    }

    /// Puts the link in.
    pub(super) fn finish_video(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.clone();
        self.find_bar = None;
        self.needs_redraw = true;

        let mut parts = typed.split(';');
        let address = parts.next().unwrap_or_default().trim().to_owned();
        let title = parts.next().unwrap_or_default().trim().to_owned();
        if address.is_empty() {
            return self.report("No address was typed, so no video was added");
        }
        if !wp_shell::desktop::is_safe_to_open(&address) {
            return self.report(&format!("{address} is not an address that can be opened"));
        }

        // The play sign in front of the title, which is how a video is marked
        // everywhere and reads as one even on paper.
        let shown = if title.is_empty() { address.clone() } else { format!("▶ {title}") };

        // One gesture: the words and the link round them are one thing to undo.
        self.document.begin_gesture();
        self.document.type_text(&shown);
        // Back over what was just typed, so the link covers it.
        let end = self.document.caret();
        let start =
            wp_docx::TextPosition::new(end.paragraph, end.offset.saturating_sub(shown.len()));
        self.document.set_caret(start);
        self.document.extend_selection_to(end);
        let linked = self.document.add_hyperlink(&address, "");
        self.document.end_gesture();

        self.relayout();
        self.reveal_caret();
        self.edited(linked, &format!("Video: {address}"))
    }
}

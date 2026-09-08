//! Signature lines, and text brought in from another document.
//!
//! The two sit together because they are the two halves of Word's Text group
//! that were left: one puts a place to sign at the caret, the other puts
//! somebody else's document there.

use wp_docx::signature::Signer;
use wp_docx::Document;
use wp_shell::dialog::FileFilter;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};

use super::Editor;

impl Editor {
    /// Asks who is to sign.
    pub(super) fn start_signature_line(&mut self) -> Response {
        self.find_bar = Some(FindBar::for_purpose(Purpose::Signature));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the signer's name, then a semicolon and their title")
    }

    /// Puts the line in.
    pub(super) fn finish_signature_line(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let signer = Signer::parse(&bar.needle);
        self.find_bar = None;
        self.needs_redraw = true;

        if signer.is_empty() {
            return self.report("Nobody was named, so no line was put in");
        }
        let named = signer.name.clone();
        let changed = self.document.insert_signature_line(&signer);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("Signature line for {named}"))
    }

    /// Brings another document's text in at the caret.
    pub(super) fn insert_text_from_file(&mut self) -> Response {
        let filters = [
            FileFilter { label: "Word documents", pattern: "*.docx" },
            FileFilter { label: "All files", pattern: "*.*" },
        ];
        let Some(path) = wp_shell::dialog::open_file("Text from File", &filters) else {
            return Response::Ignored;
        };

        let opened = std::fs::read(&path)
            .map_err(|error| format!("Cannot read {}: {error}", path.display()))
            .and_then(|bytes| {
                Document::open(&bytes)
                    .map_err(|error| format!("Cannot open {}: {error}", path.display()))
            });

        let other = match opened {
            Ok(document) => document,
            Err(message) => return self.report(&message),
        };

        let brought = self.document.insert_text_from(&other);
        if brought == 0 {
            return self.report("That document has nothing in it");
        }
        self.relayout();
        self.reveal_caret();
        self.edited(true, &format!("{brought} paragraphs from {}", file_name(&path)))
    }
}

/// The last part of a path, for saying where something came from.
fn file_name(path: &std::path::Path) -> String {
    path.file_name().and_then(|name| name.to_str()).unwrap_or("the file").to_owned()
}

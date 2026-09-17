//! The Document Recovery pane, and what its rows do.
//!
//! The pane itself is furniture and lives in [`crate::chrome::recoverypane`];
//! this is the half that opens a recovered document, throws one away, and
//! decides when the pane has done its work.

use std::path::PathBuf;

use wp_docx::Document;
use wp_shell::Response;

use super::autorecover::Recovered;
use super::Editor;
use crate::chrome::recoverypane::{Hit, RecoveryPane};

impl Editor {
    /// Shows the pane, if a run that did not end left anything behind.
    ///
    /// Called once, as the window comes up. Word does the same and at the
    /// same moment: before the person has touched anything, so that the
    /// first thing they see is that their work is not gone.
    pub fn show_recovered(&mut self, found: Vec<Recovered>) {
        if found.is_empty() {
            return;
        }
        self.status = match found.len() {
            1 => "1 document was recovered".to_owned(),
            many => format!("{many} documents were recovered"),
        };
        self.recovery = Some(RecoveryPane::new(found));
        self.needs_redraw = true;
    }

    /// Whether the pane is showing, which is what takes the left of the
    /// window from the navigation pane while it is.
    #[must_use]
    pub(super) fn recovering(&self) -> bool {
        self.recovery.is_some()
    }

    /// A press in the pane.
    pub(super) fn pressed_in_recovery(&mut self, x: i32, y: i32) -> Response {
        let Some(pane) = &self.recovery else { return Response::Ignored };
        match pane.hit(x, y) {
            Some(Hit::Open(index)) => self.open_recovered(index),
            Some(Hit::Delete(index)) => self.discard_recovered(index),
            Some(Hit::Close) => self.close_recovery(),
            None => Response::Ignored,
        }
    }

    /// Opens one of the recovered copies, as the document being edited.
    ///
    /// It comes up under the name it had, with its changes in it and not
    /// saved: the copy is not the document, and where it goes is the
    /// person's to say. Which is why the file it points at is the original
    /// and not the copy — Save puts it back where it came from.
    pub(super) fn open_recovered(&mut self, index: usize) -> Response {
        let Some(pane) = &self.recovery else { return Response::Ignored };
        let Some(entry) = pane.entries.get(index).cloned() else { return Response::Ignored };
        if !self.may_discard() {
            return Response::Ignored;
        }
        let Ok(bytes) = std::fs::read(&entry.copy) else {
            self.status = crate::messages::with("{0} could not be read", &[&entry.name]);
            return Response::Redraw;
        };
        match Document::open(&bytes) {
            Ok(document) => {
                let file: Option<PathBuf> = entry.original.clone();
                self.set_document(document, file);
                // Recovered work is work that is not on disk. Saying so is
                // what makes Save ask, and the title show that there is
                // something to lose - and the bar across the top says it for
                // as long as the document is open, which is how long it is
                // true. See [`crate::chrome::infobar`].
                self.document.mark_modified();
                self.info_bar = Some(crate::chrome::infobar::InfoBar::new(
                    crate::chrome::infobar::Because::Recovered,
                ));
                self.status = crate::messages::with("{0} recovered", &[&entry.name]);
                if let Some(pane) = &mut self.recovery {
                    pane.opened = Some(index);
                }
                self.update_title();
                self.needs_redraw = true;
                Response::Redraw
            }
            Err(error) => {
                self.status = crate::messages::with(
                    "{0} could not be opened: {1}",
                    &[&entry.name, &error.to_string()],
                );
                Response::Redraw
            }
        }
    }

    /// Throws one copy away, the person having said they do not want it.
    fn discard_recovered(&mut self, index: usize) -> Response {
        let Some(pane) = &mut self.recovery else { return Response::Ignored };
        let Some(entry) = pane.entries.get(index).cloned() else { return Response::Ignored };
        entry.remove();
        pane.remove(index);
        self.status = crate::messages::with("{0} was not kept", &[&entry.name]);
        if self.recovery.as_ref().is_some_and(|pane| pane.entries.is_empty()) {
            self.recovery = None;
        }
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Shuts the pane. Anything still in it is gone for good, so it is asked
    /// about first — which is Word's question, in Word's words.
    pub(super) fn close_recovery(&mut self) -> Response {
        let Some(pane) = &self.recovery else { return Response::Ignored };
        let left: Vec<Recovered> = pane.entries.clone();
        if !left.is_empty() {
            let question = if left.len() == 1 {
                format!(
                    "You have a recovered file that you have not saved.\n\n\
                     {} will be removed if you close this pane. Close it?",
                    left[0].name
                )
            } else {
                format!(
                    "You have {} recovered files that you have not saved.\n\n\
                     They will be removed if you close this pane. Close it?",
                    left.len()
                )
            };
            if !wp_shell::dialog::ask_yes_no(&question) {
                return Response::Ignored;
            }
        }
        for entry in &left {
            entry.remove();
        }
        self.recovery = None;
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::recoverypane;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("wp-recover-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let _ = std::fs::create_dir_all(&folder);
        folder
    }

    /// A copy on disk, as a run that did not end would have left it.
    fn copy_of(folder: &std::path::Path, name: &str, text: &str) -> Recovered {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text(text)));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let copy = folder.join(format!("{name}.docx"));
        std::fs::write(&copy, bytes).expect("writing the copy");
        Recovered {
            name: format!("{name}.docx"),
            original: Some(folder.join(format!("{name}-original.docx"))),
            saved: "2026-09-16T11:22:33Z".to_owned(),
            copy,
        }
    }

    fn editor() -> Editor {
        let document = Document::create(&Body::default()).expect("a document");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    #[test]
    fn a_recovered_document_opens_under_its_own_name_with_its_changes_unsaved() {
        let folder = folder("open");
        let entry = copy_of(&folder, "letter", "What was typed before the crash");
        let mut editor = editor();
        editor.show_recovered(vec![entry.clone()]);
        assert!(editor.recovering(), "the pane is showing");

        editor.open_recovered(0);
        assert!(
            editor.document.paragraph_text(0).is_some_and(|text| text.contains("before the crash")),
            "the copy's text is what came up"
        );
        assert_eq!(editor.file, entry.original, "and Save would put it back where it came from");
        assert!(editor.document.is_modified(), "recovered work is work that is not on disk");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_copy_thrown_away_is_gone_from_the_disk_and_from_the_pane() {
        let folder = folder("discard");
        let entry = copy_of(&folder, "notes", "Something");
        let mut editor = editor();
        editor.show_recovered(vec![entry.clone()]);

        editor.discard_recovered(0);
        assert!(!entry.copy.exists(), "the copy is off the disk");
        assert!(!editor.recovering(), "and the pane, having nothing left, is gone");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_pane_takes_the_left_of_the_window_while_it_is_up() {
        let folder = folder("width");
        let entry = copy_of(&folder, "wide", "Something");
        let mut editor = editor();
        let before = editor.pane_width();
        editor.show_recovered(vec![entry]);
        assert_eq!(editor.pane_width(), recoverypane::WIDTH);
        assert_ne!(editor.pane_width(), before);
        let _ = std::fs::remove_dir_all(&folder);
    }
}

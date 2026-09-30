//! The Document Recovery pane, and what its rows do.
//!
//! The pane itself is furniture and lives in [`crate::chrome::recoverypane`];
//! this is the half that opens a recovered document, throws one away, and
//! decides when the pane has done its work.

use std::path::PathBuf;

use wp_docx::Document;
use wp_shell::Response;

use super::autorecover::Recovered;
use super::dialogs::Asking;
use super::Editor;
use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::chrome::recoverypane::{Hit, RecoveryPane};
use crate::chrome::{Choice, Popup};
use crate::messages::t;

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

    /// Word's Recover Unsaved Documents: the copies of work that was not
    /// saved, in the same pane a start after a crash shows them in — at any
    /// time, and not only then.
    pub(super) fn recover_unsaved(&mut self) -> Response {
        let copies = super::autorecover::unsaved_copies(&self.recovery_name);
        if copies.is_empty() {
            return self.report("There are no unsaved documents to recover");
        }
        let count = copies.len();
        self.recovery = Some(RecoveryPane::new(copies));
        self.status = match count {
            1 => "1 unsaved document can be recovered".to_owned(),
            many => format!("{many} unsaved documents can be recovered"),
        };
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
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
            Some(Hit::Menu(index)) => self.open_recovered_menu(index),
            Some(Hit::Close) => self.close_recovery(),
            None => Response::Ignored,
        }
    }

    /// Drops a row's menu: Word's four things to do with a recovered copy.
    fn open_recovered_menu(&mut self, index: usize) -> Response {
        if self.close_popup_if(Choice::Recovered) {
            return Response::Redraw;
        }
        let Some(pane) = &self.recovery else { return Response::Ignored };
        let Some((left, top)) = pane.menu_place(index) else { return Response::Ignored };
        let items =
            [t("Open"), t("Save As…"), t("Delete"), t("Show Repairs")].map(str::to_owned).to_vec();
        self.recovery_menu = Some(index);
        self.popup = Some(Popup::new(Choice::Recovered, items, None, left, top, 160.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Does what was chosen from a row's menu.
    pub(super) fn choose_recovered(&mut self, choice: usize) -> Response {
        self.popup = None;
        let Some(index) = self.recovery_menu.take() else { return Response::Ignored };
        match choice {
            0 => self.open_recovered(index),
            1 => self.save_recovered_as(index),
            2 => self.discard_recovered(index),
            3 => self.show_repairs(index),
            _ => Response::Ignored,
        }
    }

    /// Opens a copy and asks where to keep it: Word's Save As on the pane.
    /// Once it is kept, the copy has done its work and goes, as a copy does
    /// whenever the work reaches the disk.
    fn save_recovered_as(&mut self, index: usize) -> Response {
        self.after_asking_to_save(move |editor| editor.save_recovered_as_now(index))
    }

    fn save_recovered_as_now(&mut self, index: usize) -> Response {
        let Some(entry) = self.recovery.as_ref().and_then(|pane| pane.entries.get(index).cloned())
        else {
            return Response::Ignored;
        };
        self.open_recovered_now(index);
        let opened = self.recovery.as_ref().is_some_and(|pane| pane.opened == Some(index));
        if !opened {
            return Response::Redraw;
        }
        if self.save_as_now() {
            entry.remove();
            if let Some(pane) = &mut self.recovery {
                pane.remove(index);
            }
            if self.recovery.as_ref().is_some_and(|pane| pane.entries.is_empty()) {
                self.recovery = None;
            }
            // Saved, and so not recovered work any more.
            self.info_bar = None;
            self.relayout();
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Says what had to be repaired in a copy for it to open: Word's Show
    /// Repairs. A copy is written whole by this program and read back by
    /// it, so what there is to say is whether it opens as it was written,
    /// or why it does not — a copy cut short by the program stopping in the
    /// middle of writing it is one that does not, and nothing in it can be
    /// put back.
    fn show_repairs(&mut self, index: usize) -> Response {
        let Some(entry) = self.recovery.as_ref().and_then(|pane| pane.entries.get(index).cloned())
        else {
            return Response::Ignored;
        };
        let said = match std::fs::read(&entry.copy) {
            Err(error) => crate::messages::with(
                "The copy of {0} could not be read: {1}. There is nothing in it to repair.",
                &[&entry.name, &error.to_string()],
            ),
            Ok(bytes) => match Document::open(&bytes) {
                Ok(_) => crate::messages::with(
                    "The copy of {0} opens as it was written. Nothing had to be repaired.",
                    &[&entry.name],
                ),
                Err(error) => crate::messages::with(
                    "The copy of {0} is damaged and cannot be repaired: {1}.",
                    &[&entry.name, &error.to_string()],
                ),
            },
        };
        let dialog = Dialog::message(t("Show Repairs"), vec![Field::note(&said)]);
        self.ask(Asking::Repairs, dialog)
    }

    /// Opens one of the recovered copies, as the document being edited.
    ///
    /// It comes up under the name it had, with its changes in it and not
    /// saved: the copy is not the document, and where it goes is the
    /// person's to say. Which is why the file it points at is the original
    /// and not the copy — Save puts it back where it came from.
    ///
    /// The document open now is asked about first, and the copy waits on the
    /// answer.
    pub(super) fn open_recovered(&mut self, index: usize) -> Response {
        self.after_asking_to_save(move |editor| editor.open_recovered_now(index))
    }

    fn open_recovered_now(&mut self, index: usize) -> Response {
        let Some(pane) = &self.recovery else { return Response::Ignored };
        let Some(entry) = pane.entries.get(index).cloned() else { return Response::Ignored };
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
    /// about first — which is Word's question, in Word's words, in a dialog
    /// of this program's own, and the pane stays until it is answered.
    pub(super) fn close_recovery(&mut self) -> Response {
        let Some(pane) = &self.recovery else { return Response::Ignored };
        let left = pane.entries.len();
        if left == 0 {
            return self.close_recovery_now();
        }
        let question = if left == 1 {
            crate::messages::with(
                "You have a recovered file that you have not saved.\n\n{0} will be removed if you close this pane. Close it?",
                &[&pane.entries[0].name],
            )
        } else {
            crate::messages::with(
                "You have {0} recovered files that you have not saved.\n\nThey will be removed if you close this pane. Close it?",
                &[&left.to_string()],
            )
        };
        let dialog = Dialog::with_buttons(
            "Word Processor",
            Field::notes(&question),
            vec![
                Button { label: "Yes".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "No".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(560.0);
        self.ask(Asking::CloseRecovery, dialog)
    }

    /// Yes: the copies go, and the pane with them.
    pub(super) fn close_recovery_now(&mut self) -> Response {
        let left: Vec<Recovered> =
            self.recovery.as_ref().map(|pane| pane.entries.clone()).unwrap_or_default();
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

    /// Each row drops Word's menu, and each of its four does what it says:
    /// Show Repairs tells whether the copy opens as it was written or is
    /// past repair, and Delete throws it away.
    #[test]
    fn a_row_s_menu_offers_what_word_s_does_and_each_does_it() {
        let folder = folder("menu");
        let good = copy_of(&folder, "sound", "Whole");
        let bad = copy_of(&folder, "broken", "Cut short");
        std::fs::write(&bad.copy, b"PK\x03\x04 and then nothing").expect("damaging it");
        let mut editor = editor();
        editor.show_recovered(vec![good.clone(), bad.clone()]);
        editor.draw(1400, 900);

        editor.open_recovered_menu(0);
        if let Ok(directory) = std::env::var("WP_PROOFS") {
            let picture = wp_raster::encode_png(editor.draw(1400, 900));
            let _ = std::fs::create_dir_all(&directory);
            let _ =
                std::fs::write(std::path::Path::new(&directory).join("recovery-menu.png"), picture);
        }
        let popup = editor.popup.as_ref().expect("the menu");
        assert_eq!(popup.choice, Choice::Recovered);
        let items: Vec<&str> = (0..4).filter_map(|at| popup.item(at)).collect();
        assert_eq!(items, ["Open", "Save As…", "Delete", "Show Repairs"]);

        let said = |editor: &Editor| match editor.dialog.as_ref().map(|d| &d.fields[0]) {
            Some(Field::Said { value, .. }) => value.clone(),
            other => panic!("no repairs said: {other:?}"),
        };
        editor.choose_recovered(3);
        assert!(said(&editor).contains("opens as it was written"), "{}", said(&editor));
        editor.dialog = None;

        editor.open_recovered_menu(1);
        editor.choose_recovered(3);
        assert!(said(&editor).contains("damaged and cannot be repaired"), "{}", said(&editor));
        editor.dialog = None;

        editor.open_recovered_menu(1);
        editor.choose_recovered(2);
        assert!(!bad.copy.exists(), "Delete takes the copy off the disk");
        assert_eq!(editor.recovery.as_ref().map(|pane| pane.entries.len()), Some(1));

        // Save As opens it and asks where; asked nowhere — there is no
        // dialog here to answer — it is not saved, and the copy stays.
        editor.draw(1400, 900);
        editor.open_recovered_menu(0);
        editor.choose_recovered(1);
        assert!(editor.document.paragraph_text(0).is_some_and(|text| text.contains("Whole")));
        assert!(good.copy.exists(), "not saved, so still the only copy of the work");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Shutting the pane with copies still in it asks first, in a dialog of
    /// this program's own rather than the system's box, and the pane waits
    /// for the answer: No keeps everything, Yes takes the copies with it.
    #[test]
    fn closing_the_pane_with_copies_in_it_asks_and_waits_for_the_answer() {
        let folder = folder("close");
        let entry = copy_of(&folder, "kept", "Something");
        let mut editor = editor();
        editor.show_recovered(vec![entry.clone()]);
        let key = |editor: &mut Editor, key: wp_shell::Key| {
            editor.handle(Event::KeyDown { key, modifiers: wp_shell::Modifiers::default() })
        };

        editor.close_recovery();
        assert_eq!(editor.asking, Some(Asking::CloseRecovery), "nothing was asked");
        assert!(editor.recovering(), "the pane went before the answer");
        let dialog = editor.dialog.as_ref().expect("a question");
        let said: Vec<String> = dialog
            .fields
            .iter()
            .filter_map(|field| match field {
                Field::Said { value, .. } => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            said,
            [
                "You have a recovered file that you have not saved.",
                "kept.docx will be removed if you close this pane. Close it?",
            ]
        );
        let buttons: Vec<&str> =
            dialog.buttons.iter().map(|button| button.label.as_str()).collect();
        assert_eq!(buttons, ["Yes", "No"]);

        key(&mut editor, wp_shell::Key::Escape);
        assert!(editor.recovering(), "No shut the pane");
        assert!(entry.copy.exists(), "No threw the copy away");

        editor.close_recovery();
        key(&mut editor, wp_shell::Key::Enter);
        assert!(!editor.recovering(), "Yes left the pane up");
        assert!(!entry.copy.exists(), "and the copy on the disk");
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

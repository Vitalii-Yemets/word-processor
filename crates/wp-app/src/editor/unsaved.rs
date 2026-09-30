//! The question about unsaved changes, and what waits on its answer.
//!
//! # Why it waits
//!
//! Word asks "Want to save your changes to "Document1"?" in a dialog of its
//! own, with Save, Don't Save and Cancel. This program asked through the
//! system's message box, called from inside whatever wanted the document
//! gone, which stopped there until the box was answered: a light box in a
//! dark window, with the system's icon and the system's buttons in the
//! system's language — "Да / Нет / Отмена" under an English interface —
//! and a loop of its own running inside the application's handling of the
//! press, while the window could draw nothing.
//!
//! Now the question is one of this program's dialogs, and what was going to
//! happen once the document was safe — the window closing, another document
//! opening, a new one being made — is handed to it and waits for the
//! answer. A request to close the window puts the question up and is
//! refused; the answer is what closes it.
//!
//! # And the questions a save asks of its own
//!
//! Saving in a kind that cannot hold macros asks first, in Word's words, and
//! saving as text asks how. Save answered to the question above goes through
//! the same save, so what waits on it waits on those too: see
//! [`Editor::after_a_question_of_the_save`].

use std::path::Path;

use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};

use super::dialogs::Asking;
use super::Editor;

/// What waits on a question: the rest of whatever asked it.
pub(super) type Then = Box<dyn FnOnce(&mut Editor) -> Response>;

/// Word's middle button.
pub(super) const DONT_SAVE: &str = "Don't Save";

/// What every question of this program's own carries across its top: its
/// own name, where Word's carry Word's.
const CAPTION: &str = "Word Processor";

/// Word's own words, for a macro-enabled document saved in a kind that
/// cannot carry what makes it one.
const MACRO_FREE: &str = "The following features cannot be saved in macro-free documents:\n\n    \u{2022} VBA project\n\nTo save a file with these features, choose No, and then choose a macro-enabled file type in the file type list.\n\nTo continue saving as a macro-free document, choose Yes.\n\nSave {0} as a macro-free document?";

impl Editor {
    /// Asks about unsaved changes before they are thrown away, and then does
    /// what was waiting on the answer — straight away, where there is
    /// nothing to lose.
    pub(super) fn after_asking_to_save(
        &mut self,
        then: impl FnOnce(&mut Editor) -> Response + 'static,
    ) -> Response {
        // The document is being closed, which it has macros for; they run
        // before the question about saving, as Word runs them, so that what
        // they change is part of what is asked about. Once for one closing,
        // however many times the closing asks.
        if !self.closing_raised {
            self.closing_raised = true;
            self.raise(super::autoevents::Moment::Closing);
        }
        if !self.document.is_modified() {
            // A question still up about changes that are no longer there
            // has nothing left to ask.
            if self.asking_to_save() {
                self.dialog = None;
                self.asking = None;
                self.waiting_on_save = None;
            }
            return then(self);
        }
        self.waiting_on_save = Some(Box::new(then));
        let question = crate::messages::with(
            "Want to save your changes to \"{0}\"?",
            &[&self.document_name()],
        );
        // As wide as the question needs, the name in it being whatever the
        // file is called.
        let wide = (question.chars().count() as f32 * 7.0 + 64.0).clamp(420.0, 900.0);
        let dialog = Dialog::with_buttons(
            CAPTION,
            vec![Field::note(&question)],
            vec![
                Button { label: "Save".to_owned(), answer: Answer::Accept, default: true },
                Button {
                    label: DONT_SAVE.to_owned(),
                    answer: Answer::Named(DONT_SAVE),
                    default: false,
                },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(wide);
        self.ask(Asking::SaveChanges, dialog)
    }

    /// Whether the question about unsaved changes is up.
    #[must_use]
    pub(super) fn asking_to_save(&self) -> bool {
        self.asking == Some(Asking::SaveChanges)
    }

    /// Save, or Don't Save.
    pub(super) fn answer_save_changes(&mut self, answer: Answer) -> Response {
        let Some(then) = self.waiting_on_save.take() else { return Response::Redraw };
        if answer == Answer::Named(DONT_SAVE) {
            // Word's setting: work thrown away on purpose can still be got
            // back, because "don't save" is answered by people in a hurry.
            // The copy is left where the next start will find it.
            if self.keep_autosaved {
                self.keep_unsaved_copy();
            } else {
                self.drop_recovery_copy();
            }
            return then(self);
        }
        // Save: where the document came from, or wherever Save As is told.
        self.after_saving = Some(then);
        self.needs_redraw = true;
        if self.save_now() {
            return self.carry_on_after_saving();
        }
        // The save has a question of its own up — the macros the kind
        // cannot hold, how to write a text file — and the rest waits on
        // that one.
        if self.in_dialog() {
            return Response::Redraw;
        }
        // Not saved: Save As was cancelled, or the disk said no. Word keeps
        // the document open, and so does this; the next attempt is a new
        // one.
        self.after_saving = None;
        self.closing_raised = false;
        Response::Redraw
    }

    /// Cancel, the cross or Escape: nothing is done, and the next attempt to
    /// close is a new one.
    pub(super) fn cancel_save_changes(&mut self) {
        self.waiting_on_save = None;
        self.closing_raised = false;
    }

    /// After a dialog the save put up has gone: what was waiting on the save,
    /// if the document reached the disk, and nothing at all if it did not.
    pub(super) fn after_a_question_of_the_save(&mut self, response: Response) -> Response {
        if self.after_saving.is_none() || self.in_dialog() {
            return response;
        }
        if self.document.is_modified() {
            self.after_saving = None;
            self.closing_raised = false;
            return response;
        }
        self.carry_on_after_saving()
    }

    fn carry_on_after_saving(&mut self) -> Response {
        match self.after_saving.take() {
            Some(then) => then(self),
            None => Response::Redraw,
        }
    }

    /// The window going, once there is nothing left in it to lose.
    ///
    /// A run that ends properly takes its copy with it, which is how the
    /// next start knows that a copy still lying there means a run that did
    /// not.
    pub(super) fn close_now(&mut self) -> Response {
        self.finish_autorecover();
        Response::Close
    }

    /// Whether the macros may go, for a save in a kind that cannot hold
    /// them: yes when Yes has just been said to this save, and otherwise
    /// the question, in Word's words, with the save waiting on the answer.
    pub(super) fn macros_may_go(&mut self, path: &Path, filtered: bool) -> bool {
        if core::mem::take(&mut self.macros_agreed) {
            return true;
        }
        self.saving_without_macros = Some((path.to_path_buf(), filtered));
        let name =
            path.file_name().and_then(|name| name.to_str()).unwrap_or(super::files::UNTITLED);
        let question = crate::messages::with(MACRO_FREE, &[name]);
        let dialog = Dialog::with_buttons(
            CAPTION,
            Field::notes(&question),
            vec![
                Button { label: "Yes".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "No".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(640.0);
        self.ask(Asking::MacroFree, dialog);
        false
    }

    /// Yes: the save goes on, without the macros.
    pub(super) fn save_without_macros(&mut self) -> Response {
        if let Some((path, filtered)) = self.saving_without_macros.take() {
            self.macros_agreed = true;
            self.write_document_as(&path, filtered);
            self.macros_agreed = false;
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// No: nothing is written, and Save As can be asked again for a kind
    /// that keeps them.
    pub(super) fn keep_the_macros(&mut self) {
        self.saving_without_macros = None;
        self.status = String::from("Not saved");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::dialog::Field;
    use std::path::PathBuf;
    use wp_docx::kinds::Kind;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("A letter")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        // Nothing of the person's own: a copy made on Don't Save would be
        // left where the next start of the real program would find it.
        editor.keep_autosaved = false;
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// A folder of this test's own, because the tests run side by side.
    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("wp-unsaved-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&folder);
        folder
    }

    fn key(editor: &mut Editor, key: Key) -> Response {
        editor.handle(Event::KeyDown { key, modifiers: Modifiers::default() })
    }

    /// A change, typed as a person types one.
    fn change(editor: &mut Editor) {
        editor.handle(Event::Char('x'));
        assert!(editor.document.is_modified(), "typing did not change the document");
    }

    /// The words on the question and its buttons, as they are drawn.
    fn what_it_says(editor: &Editor) -> (String, Vec<String>) {
        let dialog = editor.dialog.as_ref().expect("a question");
        let said = dialog
            .fields
            .iter()
            .map(|field| match field {
                Field::Said { value, .. } => value.clone(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        (said, dialog.buttons.iter().map(|button| button.label.clone()).collect())
    }

    #[test]
    fn closing_with_changes_asks_in_word_s_words_and_closes_nothing() {
        let mut editor = editor();
        change(&mut editor);
        assert_eq!(editor.handle(Event::Closing), Response::Refuse, "the window went unasked");
        assert!(editor.asking_to_save(), "nothing was asked");
        let (said, buttons) = what_it_says(&editor);
        assert_eq!(said, "Want to save your changes to \"Document\"?");
        assert_eq!(buttons, vec!["Save", "Don't Save", "Cancel"]);
        assert!(editor.document.is_modified());

        // Asked to close again while the question is up: the same question,
        // still up, and still nothing closed.
        assert_eq!(editor.handle(Event::Closing), Response::Refuse);
        assert!(editor.asking_to_save());
    }

    #[test]
    fn closing_with_nothing_to_lose_asks_nothing() {
        let mut editor = editor();
        assert_eq!(editor.handle(Event::Closing), Response::Ignored);
        assert!(!editor.in_dialog());
    }

    #[test]
    fn cancel_leaves_the_window_open_and_the_changes_in_it() {
        let mut editor = editor();
        change(&mut editor);
        editor.handle(Event::Closing);
        assert_eq!(key(&mut editor, Key::Escape), Response::Redraw);
        assert!(!editor.in_dialog());
        assert!(editor.document.is_modified(), "the changes went");
        assert!(editor.document.plain_text().contains('x'));

        // And the next attempt is a new one, which asks again.
        assert_eq!(editor.handle(Event::Closing), Response::Refuse);
        assert!(editor.asking_to_save());
    }

    #[test]
    fn don_t_save_closes_the_window() {
        let mut editor = editor();
        change(&mut editor);
        editor.handle(Event::Closing);
        // The keyboard starts on Save; Tab goes to Don't Save.
        key(&mut editor, Key::Tab);
        assert_eq!(key(&mut editor, Key::Enter), Response::Close);
    }

    #[test]
    fn save_on_a_document_with_a_file_saves_it_and_then_closes() {
        let folder = folder("with-a-file");
        let path = folder.join("Letter.docx");
        let mut editor = editor();
        assert!(editor.write_document(&path));
        change(&mut editor);

        editor.handle(Event::Closing);
        assert_eq!(key(&mut editor, Key::Enter), Response::Close, "saved, and not closed");
        assert!(!editor.document.is_modified());
        assert!(editor.asked_where.is_empty(), "Save As was asked of a document with a file");
        let saved = Document::open(&std::fs::read(&path).expect("the file")).expect("a document");
        assert!(saved.plain_text().contains('x'), "the change did not reach the disk");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn save_on_a_new_document_asks_where_first_and_closes_once_it_is_there() {
        let folder = folder("new");
        let path = folder.join("Letter.docx");
        let mut editor = editor();
        change(&mut editor);
        editor.handle(Event::Closing);

        editor.answer_where = Some(path.clone());
        assert_eq!(key(&mut editor, Key::Enter), Response::Close);
        assert_eq!(editor.asked_where, vec![PathBuf::from("Document.docx")]);
        assert!(path.is_file(), "nothing was written where Save As was told");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn save_as_cancelled_keeps_the_window_open() {
        let mut editor = editor();
        change(&mut editor);
        editor.handle(Event::Closing);
        // Nobody answers Save As.
        assert_eq!(key(&mut editor, Key::Enter), Response::Redraw);
        assert_eq!(editor.asked_where.len(), 1, "Save As was not asked");
        assert!(!editor.in_dialog());
        assert!(editor.document.is_modified());
    }

    #[test]
    fn new_and_open_ask_the_same_question_and_wait_for_it() {
        let mut editor = editor();
        change(&mut editor);
        editor.run(super::super::Command::New);
        assert!(editor.asking_to_save(), "New threw the changes away unasked");
        assert!(editor.document.plain_text().contains('x'));

        // Don't Save: the new document comes, and nothing is closed.
        key(&mut editor, Key::Tab);
        assert_ne!(key(&mut editor, Key::Enter), Response::Close);
        assert!(!editor.document.plain_text().contains('x'), "no new document");
        assert!(!editor.in_dialog(), "it asked twice");
    }

    /// A document carrying a Visual Basic project, made the way a test can.
    fn with_macros() -> Document {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("one")));
        let mut document = Document::create(&body).expect("a document");
        document.set_kind(Kind::MacroEnabledDocument);
        let bytes = document.save().expect("saving");
        let mut package = wp_opc::Package::open(&bytes).expect("a package");
        package.add_part(
            "word/vbaProject.bin",
            "application/vnd.ms-office.vbaProject",
            vec![0xD0, 0xCF, 0x11, 0xE0, 1, 2, 3, 4],
        );
        let mut relationships = package.relationships("word/document.xml").expect("relationships");
        relationships.add(
            "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
            "vbaProject.bin",
            wp_opc::TargetMode::Internal,
        );
        package.set_relationships(&relationships).expect("writing them");
        Document::open(&package.save().expect("saving the package")).expect("reopening")
    }

    #[test]
    fn saving_macros_away_asks_in_a_dialog_of_its_own_and_yes_saves() {
        let folder = folder("macro-free");
        let path = folder.join("Letter.docx");
        let mut editor = editor();
        editor.set_document(with_macros(), None);

        assert!(!editor.write_document(&path), "saved without asking");
        assert_eq!(editor.asking, Some(Asking::MacroFree));
        assert!(!path.exists(), "written before the answer");
        let (said, buttons) = what_it_says(&editor);
        assert!(said.contains("Save Letter.docx as a macro-free document?"), "{said}");
        assert!(said.contains("VBA project"), "{said}");
        assert_eq!(buttons, vec!["Yes", "No"]);

        assert_eq!(key(&mut editor, Key::Enter), Response::Redraw);
        assert!(path.is_file(), "Yes did not save");
        assert!(!editor.in_dialog());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn no_keeps_the_macros_and_writes_nothing() {
        let folder = folder("macro-kept");
        let path = folder.join("Letter.doc");
        let mut editor = editor();
        editor.set_document(with_macros(), None);
        editor.write_document(&path);
        assert_eq!(editor.asking, Some(Asking::MacroFree));
        key(&mut editor, Key::Escape);
        assert!(!path.exists(), "No wrote the file");
        assert_eq!(editor.status, "Not saved");
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Save answered to the question about closing, on a document whose
    /// kind asks a question of its own: the window waits for that one too,
    /// and closes once the file is written.
    #[test]
    fn a_close_waits_on_the_question_the_save_asks() {
        let folder = folder("macro-close");
        let path = folder.join("Letter.docx");
        let mut editor = editor();
        editor.set_document(with_macros(), Some(path.clone()));
        change(&mut editor);

        editor.handle(Event::Closing);
        assert_eq!(key(&mut editor, Key::Enter), Response::Redraw, "closed before the macros");
        assert_eq!(editor.asking, Some(Asking::MacroFree));
        assert_eq!(key(&mut editor, Key::Enter), Response::Close);
        assert!(path.is_file());
        let _ = std::fs::remove_dir_all(folder);
    }
}

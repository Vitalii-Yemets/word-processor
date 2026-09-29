//! Plain text files: opening one, and saving as one.
//!
//! # What Word does
//!
//! A `.txt` opened is decoded and shown as paragraphs in the Normal style;
//! where Word cannot tell which encoding the bytes are in, its File
//! Conversion dialog asks, with a preview of the text under each choice, so
//! the person can see which makes it readable. Saving as text shows the same
//! dialog the other way round: which encoding to write, whether to end lines
//! where the page wrapped them and with which characters, whether to stand
//! in plain characters for the ones the encoding has not got — and a preview
//! with what cannot be written marked. Saving a text file again asks again,
//! every time, because every save of a text file throws the formatting away
//! and Word does not do that silently.
//!
//! # What this does
//!
//! The same, with the two dialogs built here from [`wp_text`], which knows
//! the encodings and how sure a guess is. "Windows (Default)" and "MS-DOS"
//! are the machine's own two code pages, asked of the system.

use std::path::{Path, PathBuf};

use wp_docx::Document;
use wp_shell::Response;
use wp_text::{Encoding, LineEnding};

use crate::chrome::dialog::{Answer, Button, Dialog, Field};

use super::dialogs::Asking;
use super::Editor;

/// What is known about the text file being opened or saved while its dialog
/// is up.
#[derive(Clone, Debug)]
pub(super) struct TextFile {
    pub path: PathBuf,
    /// The bytes read, when opening; nothing when saving.
    pub bytes: Vec<u8>,
    /// The encodings the dialog's list offers, in its order.
    pub choices: Vec<Encoding>,
}

/// The rows of the File Conversion dialog when opening.
const OPEN_NOTE: usize = 0;
const OPEN_ENCODING: usize = 1;
const OPEN_PREVIEW: usize = 2;

/// And when saving.
const SAVE_WARNING: usize = 0;
const SAVE_ENCODING: usize = 1;
const SAVE_OPTIONS: usize = 2;
const SAVE_LINE_BREAKS: usize = 3;
const SAVE_ENDINGS: usize = 4;
const SAVE_SUBSTITUTE: usize = 5;
const SAVE_PREVIEW: usize = 6;
const SAVE_LOST: usize = 7;

/// How many lines the preview shows: what the dialog has room for.
const PREVIEW_LINES: usize = crate::chrome::dialog::LINES_SHOWN;

/// Whether a path names a text file.
#[must_use]
pub fn is_text_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"))
}

/// The machine's two code pages, as encodings this program knows — or the
/// Western European ones where the machine's are not known here.
pub(super) fn system_encodings() -> (Encoding, Encoding) {
    let (windows, dos) = wp_shell::system_code_pages();
    (
        Encoding::code_page(windows).unwrap_or(Encoding::CodePage(1252)),
        Encoding::code_page(dos).unwrap_or(Encoding::CodePage(437)),
    )
}

/// The list the dialogs offer: the machine's two first, named as Word names
/// them, then every encoding there is.
fn choices() -> (Vec<Encoding>, Vec<String>) {
    let (windows, dos) = system_encodings();
    let mut encodings = vec![windows, dos];
    let mut names = vec![
        format!("Windows (Default) \u{2014} {}", windows.name()),
        format!("MS-DOS \u{2014} {}", dos.name()),
    ];
    for encoding in Encoding::all() {
        encodings.push(encoding);
        names.push(encoding.name().to_owned());
    }
    (encodings, names)
}

impl Editor {
    /// Opens a text file: at once when the bytes say what they are, and
    /// through the File Conversion dialog when they do not.
    ///
    /// Whoever calls this has already asked about unsaved changes.
    pub(super) fn open_text_path(&mut self, path: &Path, bytes: Vec<u8>) -> Response {
        let (windows, _) = system_encodings();
        let detected = wp_text::detect(&bytes, windows);
        if detected.sure {
            return self.set_text_document(path, detected.encoding, &bytes);
        }

        let (encodings, _) = choices();
        let current = encodings.iter().position(|held| *held == detected.encoding).unwrap_or(0);
        self.text_file = Some(TextFile { path: path.to_path_buf(), bytes, choices: encodings });
        let dialog = self.text_open_dialog(current);
        self.ask(Asking::TextOpen, dialog)
    }

    /// Word's File Conversion, opening: which encoding makes the text
    /// readable, with the text under the chosen one to look at.
    fn text_open_dialog(&self, current: usize) -> Dialog {
        let (encodings, names) = choices();
        let preview = self
            .text_file
            .as_ref()
            .map(|file| {
                let encoding = encodings.get(current).copied().unwrap_or(Encoding::Utf8);
                preview_lines(&encoding.decode(&file.bytes))
            })
            .unwrap_or_default();
        let name = self
            .text_file
            .as_ref()
            .and_then(|file| file.path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("text")
            .to_owned();

        let fields = vec![
            Field::Said {
                label: String::new(),
                value: "Select the encoding that makes your document readable.".to_owned(),
            },
            Field::Choice { label: "Text encoding".to_owned(), items: names, current },
            Field::Lines {
                label: "Preview".to_owned(),
                lines: preview,
                scroll: 0,
                marks: Vec::new(),
            },
        ];
        crate::chrome::dialog::check_rows(
            "File Conversion",
            &fields,
            &[(OPEN_NOTE, "a line"), (OPEN_ENCODING, "a list"), (OPEN_PREVIEW, "some lines")],
        );
        Dialog::with_buttons(
            &format!("File Conversion \u{2014} {name}"),
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(560.0)
    }

    /// The encoding changed: the preview shows the text under the new one.
    pub(super) fn text_open_dialog_changed(&mut self) {
        let Some(dialog) = self.dialog.take() else { return };
        self.dialog = Some(self.text_open_dialog(dialog.chose(OPEN_ENCODING)));
    }

    /// What OK does: the file is read under the encoding chosen.
    pub(super) fn apply_text_open(&mut self, dialog: &Dialog) -> Response {
        let Some(file) = self.text_file.take() else { return Response::Redraw };
        let encoding =
            file.choices.get(dialog.chose(OPEN_ENCODING)).copied().unwrap_or(Encoding::Utf8);
        self.set_text_document(&file.path, encoding, &file.bytes)
    }

    /// Cancel on the dialog: nothing is opened.
    pub(super) fn cancel_text_open(&mut self) {
        self.text_file = None;
        self.status = String::from("Not opened");
    }

    /// The document the bytes hold under an encoding, opened.
    fn set_text_document(&mut self, path: &Path, encoding: Encoding, bytes: &[u8]) -> Response {
        let lines = wp_text::lines(&encoding.decode(bytes));
        match Document::from_text(&lines) {
            Ok(document) => {
                self.set_document(document, Some(path.to_path_buf()));
                self.text_encoding = Some(encoding);
                self.status = crate::messages::with(
                    "Opened {0} as {1}",
                    &[&path.display().to_string(), encoding.name()],
                );
                self.remember_recent(path);
                Response::Redraw
            }
            Err(error) => {
                let message = crate::messages::with(
                    "Cannot open {0}: {1}",
                    &[&path.display().to_string(), &error.to_string()],
                );
                wp_shell::dialog::show_error(&message);
                self.status = message;
                self.needs_redraw = true;
                Response::Redraw
            }
        }
    }

    /// Saving as text: the File Conversion dialog the other way round. The
    /// writing happens when it is answered, so this only opens it.
    pub(super) fn begin_text_save(&mut self, path: &Path) -> Response {
        let (encodings, _) = choices();
        let wanted = self.text_encoding.unwrap_or_else(|| system_encodings().0);
        let current = encodings.iter().position(|held| *held == wanted).unwrap_or(0);
        self.text_file =
            Some(TextFile { path: path.to_path_buf(), bytes: Vec::new(), choices: encodings });
        let dialog = self.text_save_dialog(current, false, 0, false);
        self.ask(Asking::TextSave, dialog)
    }

    /// Word's File Conversion, saving: the warning, the encoding, the
    /// options, and the preview with what cannot be written counted.
    fn text_save_dialog(
        &self,
        current: usize,
        line_breaks: bool,
        ending: usize,
        substitute: bool,
    ) -> Dialog {
        let (encodings, names) = choices();
        let encoding = encodings.get(current).copied().unwrap_or(Encoding::Utf8);
        let ending_kind = LineEnding::ALL.get(ending).copied().unwrap_or(LineEnding::CrLf);
        let text = self.text_to_save(line_breaks, ending_kind);
        let (_, lost) = encoding.encode(&text, substitute);
        // The text as it is, what the encoding cannot write in red, as
        // Word's preview has it.
        let preview = preview_lines(&text);
        let marks: Vec<Vec<(usize, usize)>> = preview
            .iter()
            .map(|line| {
                encoding
                    .unwritable(line, substitute)
                    .into_iter()
                    .map(|at| (at, at + line[at..].chars().next().map_or(1, char::len_utf8)))
                    .collect()
            })
            .collect();
        let name = self
            .text_file
            .as_ref()
            .and_then(|file| file.path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("text")
            .to_owned();

        let fields = vec![
            Field::Said {
                label: String::new(),
                value: "Warning: Saving as a text file will cause all formatting, pictures, and \
                        objects in your file to be lost."
                    .to_owned(),
            },
            Field::Choice { label: "Text encoding".to_owned(), items: names, current },
            Field::Group("Options".to_owned()),
            Field::Check { label: "Insert line breaks".to_owned(), on: line_breaks },
            Field::Choice {
                label: "End lines with".to_owned(),
                items: LineEnding::ALL.iter().map(|kind| kind.label().to_owned()).collect(),
                current: ending,
            },
            Field::Check { label: "Allow character substitution".to_owned(), on: substitute },
            Field::Lines { label: "Preview".to_owned(), lines: preview, scroll: 0, marks },
            Field::Said {
                label: String::new(),
                value: match lost {
                    0 => String::new(),
                    1 => "1 character cannot be saved in this encoding and will be a ?".to_owned(),
                    many => {
                        format!("{many} characters cannot be saved in this encoding and will be ?")
                    }
                },
            },
        ];
        crate::chrome::dialog::check_rows(
            "File Conversion",
            &fields,
            &[
                (SAVE_WARNING, "a line"),
                (SAVE_ENCODING, "a list"),
                (SAVE_OPTIONS, "a group"),
                (SAVE_LINE_BREAKS, "a tick box"),
                (SAVE_ENDINGS, "a list"),
                (SAVE_SUBSTITUTE, "a tick box"),
                (SAVE_PREVIEW, "some lines"),
                (SAVE_LOST, "a line"),
            ],
        );
        Dialog::with_buttons(
            &format!("File Conversion \u{2014} {name}"),
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(560.0)
    }

    /// A field changed: the preview shows what would be written now.
    pub(super) fn text_save_dialog_changed(&mut self) {
        let Some(dialog) = self.dialog.take() else { return };
        self.dialog = Some(self.text_save_dialog(
            dialog.chose(SAVE_ENCODING),
            dialog.ticked(SAVE_LINE_BREAKS),
            dialog.chose(SAVE_ENDINGS),
            dialog.ticked(SAVE_SUBSTITUTE),
        ));
    }

    /// What OK does: the text is written the way the dialog says.
    pub(super) fn apply_text_save(&mut self, dialog: &Dialog) -> Response {
        let Some(file) = self.text_file.take() else { return Response::Redraw };
        let encoding =
            file.choices.get(dialog.chose(SAVE_ENCODING)).copied().unwrap_or(Encoding::Utf8);
        let ending =
            LineEnding::ALL.get(dialog.chose(SAVE_ENDINGS)).copied().unwrap_or(LineEnding::CrLf);
        let text = self.text_to_save(dialog.ticked(SAVE_LINE_BREAKS), ending);
        let (bytes, lost) = encoding.encode(&text, dialog.ticked(SAVE_SUBSTITUTE));

        if let Err(error) = std::fs::write(&file.path, &bytes) {
            let message = format!("Cannot write {}: {error}", file.path.display());
            wp_shell::dialog::show_error(&message);
            self.status = message;
            return self.after_file_command();
        }
        let _ = self.document.mark_saved();
        self.file = Some(file.path.clone());
        self.text_encoding = Some(encoding);
        self.status = if lost == 0 {
            format!("Saved {} as {}", file.path.display(), encoding.name())
        } else {
            format!(
                "Saved {} as {}; {lost} character{} could not be written",
                file.path.display(),
                encoding.name(),
                if lost == 1 { "" } else { "s" }
            )
        };
        self.remember_recent(&file.path);
        self.after_file_command()
    }

    /// Cancel on the dialog: nothing is written.
    pub(super) fn cancel_text_save(&mut self) {
        self.text_file = None;
        self.status = String::from("Not saved");
    }

    /// The document as the lines of a text file.
    ///
    /// One line per paragraph, or — with line breaks put in — one per line
    /// the page wrapped it into, which is Word's "Insert line breaks": the
    /// text keeps the shape it had on the page.
    fn text_to_save(&self, line_breaks: bool, ending: LineEnding) -> String {
        let mut lines: Vec<String> = Vec::new();
        for (index, paragraph) in
            wp_text::lines(&self.document.plain_text()).into_iter().enumerate()
        {
            if !line_breaks {
                lines.push(paragraph);
                continue;
            }
            let mut starts: Vec<usize> = self
                .pages
                .iter()
                .flat_map(|page| page.lines.iter())
                .filter(|line| line.paragraph == index && line.start_offset > 0)
                .map(|line| line.start_offset)
                .filter(|start| *start < paragraph.len() && paragraph.is_char_boundary(*start))
                .collect();
            starts.sort_unstable();
            starts.dedup();
            let mut from = 0;
            for start in starts {
                lines.push(paragraph[from..start].trim_end().to_owned());
                from = start;
            }
            lines.push(paragraph[from..].to_owned());
        }
        wp_text::join(&lines, ending)
    }
}

/// The first few lines of a text, for the preview.
fn preview_lines(text: &str) -> Vec<String> {
    wp_text::lines(text).into_iter().take(PREVIEW_LINES).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor(text: &str) -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text(text)));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("wp-text-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&folder);
        folder
    }

    #[test]
    fn a_file_that_says_what_it_is_opens_at_once() {
        let folder = folder("sure");
        let path = folder.join("notes.txt");
        std::fs::write(&path, "\u{FEFF}Line one\r\nLine two\r\n").unwrap();
        let mut editor = editor("");
        editor.open_path(&path);
        assert!(editor.dialog.is_none(), "it asked when the mark had answered");
        assert_eq!(editor.document.paragraph_count(), 2);
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("Line one"));
        assert_eq!(editor.text_encoding, Some(Encoding::Utf8));
        assert_eq!(editor.document_name(), "notes.txt");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_file_that_does_not_say_is_asked_about_with_a_preview() {
        let folder = folder("unsure");
        let path = folder.join("russian.txt");
        let (bytes, _) = Encoding::CodePage(1251).encode("Привет\r\nмир", false);
        std::fs::write(&path, &bytes).unwrap();
        let mut editor = editor("");
        editor.open_path(&path);
        assert_eq!(editor.asking, Some(Asking::TextOpen), "it did not ask");

        // Choosing Cyrillic (Windows) in the list changes the preview to
        // readable text, and OK opens it that way.
        let (encodings, _) = choices();
        let cyrillic = encodings.iter().position(|held| *held == Encoding::CodePage(1251)).unwrap();
        editor.dialog = Some(editor.text_open_dialog(cyrillic));
        let dialog = editor.dialog.clone().unwrap();
        match &dialog.fields[OPEN_PREVIEW] {
            Field::Lines { lines, .. } => assert_eq!(lines[0], "Привет"),
            other => panic!("{other:?}"),
        }
        editor.asking = None;
        editor.dialog = None;
        editor.apply_text_open(&dialog);
        assert_eq!(editor.document.paragraph_text(1).as_deref(), Some("мир"));
        assert_eq!(editor.text_encoding, Some(Encoding::CodePage(1251)));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn saving_as_text_asks_and_writes_what_was_chosen() {
        let folder = folder("save");
        let path = folder.join("out.txt");
        let mut editor = editor("Привет — “мир”");
        editor.begin_text_save(&path);
        assert_eq!(editor.asking, Some(Asking::TextSave));

        let (encodings, _) = choices();
        let cyrillic = encodings.iter().position(|held| *held == Encoding::CodePage(1251)).unwrap();
        let dialog = editor.text_save_dialog(cyrillic, false, 2, true);
        editor.asking = None;
        editor.dialog = None;
        editor.apply_text_save(&dialog);

        let written = std::fs::read(&path).unwrap();
        assert_eq!(Encoding::CodePage(1251).decode(&written), "Привет — “мир”\n");
        assert_eq!(editor.file.as_deref(), Some(path.as_path()));
        assert!(!editor.document.is_modified());
        assert_eq!(editor.text_encoding, Some(Encoding::CodePage(1251)));

        // ASCII cannot hold it, and the dialog says how much it would lose.
        let ascii = encodings.iter().position(|held| *held == Encoding::Ascii).unwrap();
        editor.text_file =
            Some(TextFile { path: path.clone(), bytes: Vec::new(), choices: encodings });
        let dialog = editor.text_save_dialog(ascii, false, 0, false);
        match &dialog.fields[SAVE_LOST] {
            Field::Said { value, .. } => assert!(value.contains("cannot be saved"), "{value}"),
            other => panic!("{other:?}"),
        }
        // The preview shows the text as it is, what ASCII cannot write in
        // red: every letter of it and the dash and the quotes.
        match &dialog.fields[SAVE_PREVIEW] {
            Field::Lines { lines, marks, .. } => {
                assert_eq!(lines[0], "Привет — “мир”");
                assert_eq!(marks[0].len(), 12);
                assert_eq!(marks[0][0], (0, "П".len()));
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn line_breaks_follow_the_page() {
        let mut editor = editor(&"word ".repeat(60));
        let plain = editor.text_to_save(false, LineEnding::Lf);
        assert_eq!(plain.matches('\n').count(), 1, "one paragraph is one line");
        let broken = editor.text_to_save(true, LineEnding::Lf);
        assert!(broken.matches('\n').count() > 2, "the paragraph wrapped on the page: {broken:?}");
        assert_eq!(broken.replace('\n', " ").trim_end(), plain.trim_end());
        editor.text_file = None;
    }
}

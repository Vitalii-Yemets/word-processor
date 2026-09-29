//! Opening a file as a kind the person says.
//!
//! # What Word does
//!
//! Word tells what a file is by itself and converts it without asking —
//! unless "Confirm file format conversion on open" is ticked in its
//! Options, and then every file that is not a Word document is put to the
//! Convert File dialog first, the kind Word took it for chosen in a list of
//! every kind it reads, even when the kind is plain. One of those kinds is
//! not a kind at all: "Recover Text from Any File" reads the text out of a
//! file of any sort, which is what is left to do with one that nothing
//! else will open.
//!
//! # What this does
//!
//! The same: the kinds this program reads, what a file is taken for — by
//! its extension, or by its first bytes where the extension says nothing —
//! the dialog, and the recovery, which takes the file as text where it is
//! text and otherwise the runs of readable characters in it, single bytes
//! or UTF-16, in the order they stand.

use std::path::{Path, PathBuf};

use wp_shell::Response;
use wp_text::Encoding;

use crate::chrome::dialog::{Answer, Button, Dialog, Field, TreeRow};

use super::dialogs::Asking;
use super::Editor;

/// A kind of file this program reads, or the recovery of any file's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Word,
    Word97,
    Rtf,
    WebPage,
    SingleFileWebPage,
    Text,
    OpenDocument,
    Pdf,
    Recover,
}

impl Kind {
    /// Every kind, in the order Word's Convert File list has them.
    pub(crate) const ALL: [Self; 9] = [
        Self::Word,
        Self::Word97,
        Self::OpenDocument,
        Self::Pdf,
        Self::Text,
        Self::Recover,
        Self::Rtf,
        Self::SingleFileWebPage,
        Self::WebPage,
    ];

    /// What Word's list calls it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Word => "Word Document",
            Self::Word97 => "Word 97-2003 Document",
            Self::Rtf => "Rich Text Format",
            Self::WebPage => "Web Pages",
            Self::SingleFileWebPage => "Single File Web Page",
            Self::Text => "Plain Text",
            Self::OpenDocument => "OpenDocument Text",
            Self::Pdf => "PDF Files",
            Self::Recover => "Recover Text from Any File",
        }
    }

    /// What a file is taken for: by its extension, or where that says
    /// nothing, by its first bytes.
    pub(crate) fn of(path: &Path, bytes: &[u8]) -> Self {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        match extension.as_str() {
            "docx" | "docm" | "dotx" | "dotm" => return Self::Word,
            "doc" | "dot" => return Self::Word97,
            "rtf" => return Self::Rtf,
            "htm" | "html" => return Self::WebPage,
            "mht" | "mhtml" => return Self::SingleFileWebPage,
            "txt" => return Self::Text,
            "odt" => return Self::OpenDocument,
            "pdf" => return Self::Pdf,
            _ => {}
        }
        let start = &bytes[..bytes.len().min(1024)];
        let lower = String::from_utf8_lossy(start).to_ascii_lowercase();
        if bytes.starts_with(b"PK\x03\x04") {
            if lower.contains("mimetypeapplication/vnd.oasis.opendocument") {
                Self::OpenDocument
            } else {
                Self::Word
            }
        } else if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
            Self::Word97
        } else if bytes.starts_with(b"{\\rtf") {
            Self::Rtf
        } else if wp_pdf::looks_like_pdf(bytes) {
            Self::Pdf
        } else if lower.contains("mime-version:") {
            Self::SingleFileWebPage
        } else if lower.contains("<html") || lower.contains("<!doctype html") {
            Self::WebPage
        } else {
            Self::Text
        }
    }
}

/// A file read and waiting for the Convert File dialog's answer.
#[derive(Clone, Debug)]
pub(super) struct Converting {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

/// The one row of the dialog: the list of kinds.
const KINDS: usize = 0;

impl Editor {
    /// Word's Convert File: which kind to read the file as, the kind it was
    /// taken for chosen.
    pub(super) fn ask_conversion(&mut self, path: &Path, bytes: Vec<u8>, taken: Kind) -> Response {
        self.converting = Some(Converting { path: path.to_path_buf(), bytes });
        // A list, as Word's is, rather than a drop-down: every kind in sight.
        let fields = vec![Field::Tree {
            label: "Convert file from".to_owned(),
            rows: Kind::ALL.iter().map(|kind| TreeRow::plain(kind.label())).collect(),
            current: Kind::ALL.iter().position(|kind| *kind == taken).unwrap_or(0),
            scroll: 0,
        }];
        crate::chrome::dialog::check_rows("Convert File", &fields, &[(KINDS, "a list of rows")]);
        let dialog = Dialog::with_buttons(
            "Convert File",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        );
        self.ask(Asking::Convert, dialog)
    }

    /// OK: the file is read as the kind chosen.
    pub(super) fn apply_conversion(&mut self, dialog: &Dialog) -> Response {
        let Some(waiting) = self.converting.take() else { return Response::Redraw };
        let kind = Kind::ALL.get(dialog.chose_row(KINDS)).copied().unwrap_or(Kind::Word);
        self.open_read(&waiting.path, waiting.bytes, kind)
    }

    /// Cancel: nothing is opened.
    pub(super) fn cancel_conversion(&mut self) {
        self.converting = None;
        self.status = String::from("Not opened");
    }
}

/// The text of a file of any kind, as paragraphs: the file as text where it
/// is text, and where it is not, the runs of readable characters in it —
/// single bytes in the machine's code page, or UTF-16 — each a paragraph, in
/// the order they stand.
pub(crate) fn recovered_text(bytes: &[u8], default: Encoding) -> Vec<String> {
    let detected = wp_text::detect(bytes, default);
    let controls =
        bytes.iter().filter(|&&b| b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0C)).count();
    let utf16 = matches!(detected.encoding, Encoding::Utf16Le | Encoding::Utf16Be);
    if utf16 || controls * 100 <= bytes.len() {
        return wp_text::lines(&detected.encoding.decode(bytes));
    }
    let readable = |c: char| !c.is_control() && c != '\u{FFFD}';
    let mut runs: Vec<(usize, String)> = Vec::new();
    // Single bytes.
    let mut current = String::new();
    let mut start = 0;
    for (at, &byte) in bytes.iter().enumerate() {
        let character = if byte < 0x80 {
            char::from(byte)
        } else {
            default.decode(&[byte]).chars().next().unwrap_or('\u{FFFD}')
        };
        if readable(character) {
            if current.is_empty() {
                start = at;
            }
            current.push(character);
        } else {
            push_run(&mut runs, start, &mut current);
        }
    }
    push_run(&mut runs, start, &mut current);
    // And UTF-16, least significant byte first, on either alignment.
    for offset in 0..2 {
        let mut current = String::new();
        let mut start = 0;
        let mut at = offset;
        while at + 1 < bytes.len() {
            let (low, high) = (bytes[at], bytes[at + 1]);
            let unit = u16::from_le_bytes([low, high]);
            // Any two bytes are some character of UTF-16, so only the
            // alphabets whose text it is plain in are taken: Latin, with a
            // nought for its second byte, and Greek and Cyrillic by theirs.
            let character = char::from_u32(u32::from(unit)).filter(|&c| {
                readable(c)
                    && match high {
                        0 => (0x20..0x7F).contains(&low) || low >= 0xA0,
                        0x03 | 0x04 => c.is_alphabetic(),
                        _ => false,
                    }
            });
            match character {
                Some(character) => {
                    if current.is_empty() {
                        start = at;
                    }
                    current.push(character);
                }
                None => push_run(&mut runs, start, &mut current),
            }
            at += 2;
        }
        push_run(&mut runs, start, &mut current);
    }
    runs.sort_by_key(|(at, _)| *at);
    runs.into_iter().map(|(_, text)| text).collect()
}

/// A run of readable characters, kept if it is text rather than bytes that
/// happen to be letters: four letters or figures at least, and mostly
/// letters, figures and spaces — begun at a letter or figure, and not at an
/// accented one glued to a plain word, which is a stray byte before it.
fn push_run(runs: &mut Vec<(usize, String)>, start: usize, current: &mut String) {
    let mut text = current.trim();
    loop {
        let mut characters = text.chars();
        match (characters.next(), characters.next()) {
            (Some(first), _) if !first.is_alphanumeric() => text = &text[first.len_utf8()..],
            (Some(first), Some(second)) if !first.is_ascii() && second.is_ascii_alphanumeric() => {
                text = &text[first.len_utf8()..];
            }
            _ => break,
        }
    }
    let characters = text.chars().count();
    let wordy = text.chars().filter(|c| c.is_alphanumeric() || *c == ' ').count();
    let letters = text.chars().filter(|c| c.is_alphanumeric()).count();
    if letters >= 4 && wordy * 10 >= characters * 7 {
        runs.push((start, text.to_owned()));
    }
    current.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn editor() -> Editor {
        let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("")));
        let document = Document::create(&body).expect("a document");
        let mut editor = Editor::new(library, document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    fn file(name: &str, bytes: &[u8]) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("wp-convert-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&folder);
        let path = folder.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn with_conversion_confirmed_a_file_is_asked_about_and_read_as_the_kind_chosen() {
        let path = file("letter.rtf", b"{\\rtf1\\ansi Dear reader\\par}");
        let mut editor = editor();
        editor.confirm_conversion = true;
        editor.open_path(&path);
        assert_eq!(editor.asking, Some(Asking::Convert), "it did not ask");
        let dialog = editor.dialog.clone().expect("the dialog");
        assert_eq!(
            dialog.chose_row(KINDS),
            Kind::ALL.iter().position(|k| *k == Kind::Rtf).unwrap()
        );

        // Cancel opens nothing.
        editor.asking = None;
        editor.dialog = None;
        editor.cancel_conversion();
        assert_eq!(editor.document.plain_text().trim(), "");

        // Plain Text, chosen, reads the file's own characters.
        editor.open_path(&path);
        let mut dialog = editor.dialog.clone().expect("the dialog");
        if let Field::Tree { current, .. } = &mut dialog.fields[KINDS] {
            *current = Kind::ALL.iter().position(|k| *k == Kind::Text).unwrap();
        }
        editor.asking = None;
        editor.dialog = None;
        editor.apply_conversion(&dialog);
        assert_eq!(editor.document.plain_text().trim_end(), "{\\rtf1\\ansi Dear reader\\par}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn without_it_a_file_opens_as_what_it_is() {
        let path = file("plain.rtf", b"{\\rtf1\\ansi Dear reader\\par}");
        let mut editor = editor();
        editor.open_path(&path);
        assert!(editor.dialog.is_none());
        assert_eq!(editor.document.plain_text().trim_end(), "Dear reader");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_files_text_is_recovered_whatever_it_is() {
        let mut bytes = vec![0x7Fu8, 0, 1, 2, 0xFE];
        bytes.extend_from_slice(b"Words worth keeping");
        bytes.extend_from_slice(&[0, 3, 0x14, 0x88, 0]);
        let path = file("broken.bin", &bytes);
        let mut editor = editor();
        editor.open_path_as(&path, Some(Kind::Recover));
        assert_eq!(editor.document.plain_text().trim_end(), "Words worth keeping");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_is_taken_for_its_kind_by_its_extension_or_its_bytes() {
        assert_eq!(Kind::of(Path::new("a.DOCX"), b""), Kind::Word);
        assert_eq!(Kind::of(Path::new("a.pdf"), b""), Kind::Pdf);
        assert_eq!(Kind::of(Path::new("a.bin"), b"{\\rtf1 hello}"), Kind::Rtf);
        assert_eq!(Kind::of(Path::new("a"), b"%PDF-1.4"), Kind::Pdf);
        assert_eq!(Kind::of(Path::new("a"), &[0xD0, 0xCF, 0x11, 0xE0, 0xA1]), Kind::Word97);
        assert_eq!(Kind::of(Path::new("a.dat"), b"<!DOCTYPE html><html>"), Kind::WebPage);
        assert_eq!(Kind::of(Path::new("a.log"), b"just words"), Kind::Text);
    }

    #[test]
    fn the_text_of_any_file_is_recovered() {
        // Text is text.
        assert_eq!(
            recovered_text(b"one\r\ntwo\r\n", Encoding::CodePage(1252)),
            vec!["one".to_owned(), "two".to_owned()]
        );
        // A binary file: the readable runs in it, single bytes and UTF-16,
        // in the order they stand; the noise between them gone.
        let mut bytes = vec![0u8, 1, 2, 3, 0xFF, 0x10];
        bytes.extend_from_slice(b"First words here");
        bytes.extend_from_slice(&[0, 0, 7, 3, 1]);
        for unit in "Second, wide".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0, 2, 9, 0x11, 0x12, b'a', b'b', 0x01]);
        for unit in "Привет мир".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0, 5]);
        let recovered = recovered_text(&bytes, Encoding::CodePage(1252));
        assert_eq!(
            recovered,
            vec!["First words here".to_owned(), "Second, wide".to_owned(), "Привет мир".to_owned()]
        );
    }
}

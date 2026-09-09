//! Opening, saving and printing — everything that puts the document on disk or
//! on paper.

use std::path::{Path, PathBuf};

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::Document;
use wp_shell::Response;

use super::Editor;

/// What a document with no file of its own is called.
pub const UNTITLED: &str = "Document";

/// The file types the open and save dialogs offer.
pub const DOCUMENT_FILTERS: &[wp_shell::dialog::FileFilter] = &[
    wp_shell::dialog::FileFilter { label: "Word documents (*.docx)", pattern: "*.docx" },
    wp_shell::dialog::FileFilter { label: "All files (*.*)", pattern: "*.*" },
];

impl Editor {
    /// What the document is called, for the caption and for asking about it.
    pub(super) fn document_name(&self) -> String {
        match &self.file {
            Some(path) => {
                path.file_name().and_then(|name| name.to_str()).unwrap_or(UNTITLED).to_owned()
            }
            None => UNTITLED.to_owned(),
        }
    }

    /// Puts the document's name in the caption bar, with a mark when it has
    /// unsaved changes.
    ///
    /// Only when it has actually changed: the caption is set through the
    /// window, and doing that on every keystroke would be work for nothing.
    pub(super) fn update_title(&mut self) {
        let wanted = format!(
            "{}{} — Word Processor",
            if self.document.is_modified() { "*" } else { "" },
            self.document_name()
        );
        if wanted != self.title {
            wp_shell::set_title(&wanted);
            self.title = wanted;
        }
    }

    /// Writes the document to a path. Returns whether it got there.
    fn write_document(&mut self, path: &Path) -> bool {
        let bytes = match self.document.save() {
            Ok(bytes) => bytes,
            Err(error) => {
                let message = format!("Cannot save: {error}");
                wp_shell::dialog::show_error(&message);
                self.status = message;
                return false;
            }
        };

        if let Err(error) = std::fs::write(path, &bytes) {
            let message = format!("Cannot write {}: {error}", path.display());
            wp_shell::dialog::show_error(&message);
            self.status = message;
            return false;
        }

        // Only once the bytes are really on disk does the document count as
        // saved.
        let _ = self.document.mark_saved();
        self.file = Some(path.to_path_buf());
        self.status = format!("Saved {}", path.display());
        self.update_title();
        true
    }

    /// Saves where the document came from, asking where when it came from
    /// nowhere.
    pub(super) fn save_now(&mut self) -> bool {
        match self.file.clone() {
            Some(path) => self.write_document(&path),
            None => self.save_as_now(),
        }
    }

    pub(super) fn save_as_now(&mut self) -> bool {
        let suggested =
            self.file.clone().unwrap_or_else(|| PathBuf::from(format!("{UNTITLED}.docx")));
        match wp_shell::dialog::save_file("Save as", DOCUMENT_FILTERS, Some(&suggested)) {
            Some(path) => self.write_document(&path),
            None => {
                self.status = String::from("Not saved");
                false
            }
        }
    }

    pub(super) fn after_file_command(&mut self) -> Response {
        self.update_title();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Asks about unsaved changes before they are thrown away.
    ///
    /// Returns whether it is all right to go ahead. A document with no changes
    /// asks nothing, because there is nothing to lose.
    pub(super) fn may_discard(&mut self) -> bool {
        if !self.document.is_modified() {
            return true;
        }
        match wp_shell::dialog::ask_to_save(&self.document_name()) {
            wp_shell::dialog::Answer::Yes => self.save_now(),
            wp_shell::dialog::Answer::No => true,
            wp_shell::dialog::Answer::Cancel => false,
        }
    }

    /// Replaces what is being edited, and starts afresh around it.
    pub(super) fn set_document(&mut self, document: Document, file: Option<PathBuf>) {
        self.document = document;
        self.file = file;
        self.scroll = 0.0;
        self.dragging = false;
        self.relayout();
        self.update_title();
    }

    pub(super) fn new_document(&mut self) -> Response {
        if !self.may_discard() {
            return Response::Ignored;
        }
        // One empty paragraph, so there is somewhere for the caret to be.
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::default()));

        match Document::create(&body) {
            Ok(mut document) => {
                // A new document is made with whatever theme was last set as
                // the default, which is what the Design tab's button is for.
                let _ = document.set_theme(&self.default_theme());
                self.set_document(document, None);
                self.status = String::from("New document");
                Response::Redraw
            }
            Err(error) => {
                wp_shell::dialog::show_error(&format!("Cannot make a document: {error}"));
                Response::Ignored
            }
        }
    }

    pub(super) fn open_document(&mut self) -> Response {
        if !self.may_discard() {
            return Response::Ignored;
        }
        let Some(path) = wp_shell::dialog::open_file("Open", DOCUMENT_FILTERS) else {
            return Response::Ignored;
        };

        let opened = std::fs::read(&path)
            .map_err(|error| format!("Cannot read {}: {error}", path.display()))
            .and_then(|bytes| {
                Document::open(&bytes)
                    .map_err(|error| format!("Cannot open {}: {error}", path.display()))
            });

        match opened {
            Ok(document) => {
                self.set_document(document, Some(path.clone()));
                self.status = format!("Opened {}", path.display());
                Response::Redraw
            }
            Err(message) => {
                wp_shell::dialog::show_error(&message);
                self.status = message;
                self.needs_redraw = true;
                Response::Redraw
            }
        }
    }
}

/// Today's date, written the way a document wants it.
///
/// Worked out from the system clock rather than asked of the operating system,
/// because there is no portable call for a formatted date and the arithmetic is
/// the same everywhere. Proleptic Gregorian, which is what every calendar in
/// use agrees on for any date this program will see.
#[must_use]
pub(crate) fn today() -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];

    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let mut days = (seconds / 86_400) as i64;

    // Counting forward from 1970 a year at a time: the range is small and the
    // arithmetic is obvious, which matters more here than being clever.
    let mut year = 1970i64;
    loop {
        let length = if is_leap(year) { 366 } else { 365 };
        if days < length {
            break;
        }
        days -= length;
        year += 1;
    }

    let lengths = [31, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0usize;
    while month < 12 && days >= lengths[month] {
        days -= lengths[month];
        month += 1;
    }

    format!("{} {} {year}", days + 1, MONTHS[month.min(11)])
}

/// Whether a year has a twenty-ninth of February.
fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// The name to record as the author of a comment or a change.
///
/// The machine's user name, which is the only name the program has to go on
/// without asking for one. Word does the same when nothing has told it better.
#[must_use]
pub(crate) fn user_name() -> String {
    for variable in ["USERNAME", "USER", "LOGNAME"] {
        if let Ok(name) = std::env::var(variable) {
            let name = name.trim();
            if !name.is_empty() {
                return name.to_owned();
            }
        }
    }
    "Author".to_owned()
}

/// The moment now, in the form the format records dates in.
///
/// ISO 8601 in UTC, which is what `w:date` holds. Worked out from the system
/// clock by the same arithmetic [`today`] uses, because there is no portable
/// call that formats one.
#[must_use]
pub(crate) fn timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());

    let (mut days, rest) = ((seconds / 86_400) as i64, seconds % 86_400);
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);

    let mut year = 1970i64;
    loop {
        let length = if is_leap(year) { 366 } else { 365 };
        if days < length {
            break;
        }
        days -= length;
        year += 1;
    }

    let lengths = [31, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0usize;
    while month < 12 && days >= lengths[month] {
        days -= lengths[month];
        month += 1;
    }

    format!("{year:04}-{:02}-{:02}T{hour:02}:{minute:02}:{second:02}Z", month + 1, days + 1)
}

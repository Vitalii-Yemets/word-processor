//! Opening, saving and printing — everything that puts the document on disk or
//! on paper.

use std::path::{Path, PathBuf};

use wp_docx::kinds::Kind;
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::Document;
use wp_shell::Response;

use super::Editor;

/// What a document with no file of its own is called.
pub const UNTITLED: &str = "Document";

/// The file types the Open dialog offers: Word's four, together and apart.
pub const DOCUMENT_FILTERS: &[wp_shell::dialog::FileFilter] = &[
    wp_shell::dialog::FileFilter {
        label: "Word documents (*.docx;*.docm;*.dotx;*.dotm)",
        pattern: "*.docx;*.docm;*.dotx;*.dotm",
    },
    wp_shell::dialog::FileFilter { label: "Word Document (*.docx)", pattern: "*.docx" },
    wp_shell::dialog::FileFilter {
        label: "Word Macro-Enabled Document (*.docm)",
        pattern: "*.docm",
    },
    wp_shell::dialog::FileFilter { label: "Word Template (*.dotx)", pattern: "*.dotx" },
    wp_shell::dialog::FileFilter {
        label: "Word Macro-Enabled Template (*.dotm)",
        pattern: "*.dotm",
    },
    wp_shell::dialog::FileFilter { label: "Word 97-2003 Documents (*.doc)", pattern: "*.doc" },
    wp_shell::dialog::FileFilter { label: "Rich Text Format (*.rtf)", pattern: "*.rtf" },
    wp_shell::dialog::FileFilter {
        label: "Web Pages (*.htm;*.html;*.mht;*.mhtml)",
        pattern: "*.htm;*.html;*.mht;*.mhtml",
    },
    wp_shell::dialog::FileFilter { label: "Text Files (*.txt)", pattern: "*.txt" },
    wp_shell::dialog::FileFilter { label: "OpenDocument Text (*.odt)", pattern: "*.odt" },
    wp_shell::dialog::FileFilter { label: "PDF Files (*.pdf)", pattern: "*.pdf" },
    wp_shell::dialog::FileFilter { label: "All files (*.*)", pattern: "*.*" },
];

/// And the ones Save As offers, which are the kinds a document can be made
/// into, in the order Word lists them. The extension follows the kind chosen:
/// that is what "Save as type" means.
pub const SAVE_FILTERS: &[wp_shell::dialog::FileFilter] = &[
    wp_shell::dialog::FileFilter { label: "Word Document (*.docx)", pattern: "*.docx" },
    wp_shell::dialog::FileFilter {
        label: "Word Macro-Enabled Document (*.docm)",
        pattern: "*.docm",
    },
    wp_shell::dialog::FileFilter { label: "Word Template (*.dotx)", pattern: "*.dotx" },
    wp_shell::dialog::FileFilter {
        label: "Word Macro-Enabled Template (*.dotm)",
        pattern: "*.dotm",
    },
    wp_shell::dialog::FileFilter { label: "Rich Text Format (*.rtf)", pattern: "*.rtf" },
    wp_shell::dialog::FileFilter { label: "Plain Text (*.txt)", pattern: "*.txt" },
    wp_shell::dialog::FileFilter { label: "Web Page (*.htm;*.html)", pattern: "*.htm;*.html" },
    wp_shell::dialog::FileFilter {
        label: "Single File Web Page (*.mht;*.mhtml)",
        pattern: "*.mht;*.mhtml",
    },
    wp_shell::dialog::FileFilter { label: "OpenDocument Text (*.odt)", pattern: "*.odt" },
];

/// The kind a path's extension asks for, if it asks for one of the four.
#[must_use]
pub fn kind_of_path(path: &Path) -> Option<Kind> {
    path.extension().and_then(|extension| extension.to_str()).and_then(Kind::of_extension)
}

/// Whether a path names a Rich Text file, which is read and written as one
/// rather than as a package.
#[must_use]
pub fn is_rtf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rtf"))
}

/// Whether a path names a Word 97-2003 document, which is read as one and
/// opens in Compatibility Mode.
#[must_use]
pub fn is_doc_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("doc"))
}

/// Whether a path names an OpenDocument Text package, which is read and
/// written as one — a package too, but not a Word one.
#[must_use]
pub fn is_odt_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("odt"))
}

/// Whether a path names a PDF, which is converted into a document when it
/// is opened and never written back.
#[must_use]
pub fn is_pdf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

/// What Word says before it converts a PDF, and asks whether to go on.
pub const PDF_CONVERSION_NOTICE: &str = "Word will now convert your PDF to an editable Word document. This may take a while. The resulting Word document will be optimized to allow you to edit the text, so it might not look exactly like the original PDF, especially if the original file contained lots of graphics.";

/// Whether a path names a web page, a single-file one or not.
#[must_use]
pub fn is_web_path(path: &Path) -> bool {
    web_kind(path).is_some()
}

/// Which kind of web page a path names: a page beside its pictures, or one
/// file holding both.
fn web_kind(path: &Path) -> Option<WebKind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "htm" | "html" => Some(WebKind::Page),
        "mht" | "mhtml" => Some(WebKind::SingleFile),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WebKind {
    Page,
    SingleFile,
}

/// Whether a path names a template, which is opened by making a document
/// from it rather than by opening it.
#[must_use]
pub fn is_template_path(path: &Path) -> bool {
    kind_of_path(path).is_some_and(Kind::is_template)
}

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
        // A Rich Text file is in Compatibility Mode, and Word's caption says
        // so: what it holds is what the older format can hold.
        let compatibility =
            self.file.as_deref().is_some_and(|path| is_rtf_path(path) || is_doc_path(path));
        let wanted = format!(
            "{}{}{} — Word Processor",
            if self.document.is_modified() { "*" } else { "" },
            self.document_name(),
            if compatibility { " [Compatibility Mode]" } else { "" }
        );
        if wanted != self.title {
            wp_shell::set_title(&wanted);
            self.title = wanted;
        }
    }

    /// Writes the document to a path. Returns whether it got there.
    ///
    /// The path's extension says which of the four kinds the file is to be,
    /// and the document is made that kind before it is written: a document
    /// saved as `.dotx` is a template from then on. A kind that cannot hold
    /// macros is asked about first, in Word's words, when there are macros
    /// to lose.
    fn write_document(&mut self, path: &Path) -> bool {
        // A text file is written through its own dialog, which asks how, and
        // is not written until that is answered: see [`super::textfiles`].
        if super::textfiles::is_text_path(path) {
            self.begin_text_save(path);
            return false;
        }
        // A web page is written as one — the pictures in a folder beside it
        // named after it, which is Word's Web Page — or as one file holding
        // both, which is Word's Single File Web Page.
        if let Some(kind) = web_kind(path) {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("page.htm");
            let title = self.document.properties().title;
            let title = (!title.is_empty()).then_some(title.as_str());
            let written = match kind {
                WebKind::SingleFile => {
                    std::fs::write(path, wp_html::write_mht(&self.document, name, title))
                }
                WebKind::Page => {
                    let page = wp_html::write(&self.document, name, title);
                    let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
                    let mut written = std::fs::write(path, page.html.as_bytes());
                    for picture in &page.pictures {
                        if written.is_err() {
                            break;
                        }
                        let target = folder.join(&picture.name);
                        if let Some(parent) = target.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        written = std::fs::write(target, &picture.bytes);
                    }
                    written
                }
            };
            if let Err(error) = written {
                let message = format!("Cannot write {}: {error}", path.display());
                wp_shell::dialog::show_error(&message);
                self.status = message;
                return false;
            }
            let _ = self.document.mark_saved();
            self.file = Some(path.to_path_buf());
            self.status = format!("Saved {} as a web page", path.display());
            self.update_title();
            self.remember_recent(path);
            return true;
        }
        // A Rich Text file is written as one: what the model holds, as RTF,
        // and nothing of the package.
        if is_rtf_path(path) {
            let bytes = wp_rtf::write(&self.document);
            if let Err(error) = std::fs::write(path, &bytes) {
                let message = format!("Cannot write {}: {error}", path.display());
                wp_shell::dialog::show_error(&message);
                self.status = message;
                return false;
            }
            let _ = self.document.mark_saved();
            self.file = Some(path.to_path_buf());
            self.status = format!("Saved {} as Rich Text Format", path.display());
            self.update_title();
            self.remember_recent(path);
            return true;
        }
        // An OpenDocument package holds what the model holds, in its own
        // words; the Word package is not kept.
        if is_odt_path(path) {
            let bytes = match wp_odt::save(&self.document) {
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
            let _ = self.document.mark_saved();
            self.file = Some(path.to_path_buf());
            self.status = format!("Saved {} as OpenDocument Text", path.display());
            self.update_title();
            self.remember_recent(path);
            return true;
        }
        if let Some(kind) = kind_of_path(path) {
            if !kind.allows_macros() && self.document.has_macros() {
                let question = format!(
                    "The following features cannot be saved in macro-free documents:\n\n    \u{2022} VBA project\n\nTo save a file with these features, choose No, and then choose a macro-enabled file type in the file type list.\n\nTo continue saving as a macro-free document, choose Yes.\n\nSave {} as a macro-free document?",
                    path.file_name().and_then(|name| name.to_str()).unwrap_or(UNTITLED)
                );
                if !wp_shell::dialog::ask_yes_no(&question) {
                    self.status = String::from("Not saved");
                    return false;
                }
            }
            self.document.set_kind(kind);
        }

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
        self.remember_recent(path);
        true
    }

    /// Puts a document at the top of the list the Open page shows.
    ///
    /// Word remembers a document both when it is opened and when it is saved,
    /// which is why saving one under a new name puts the new name on the list
    /// and not the old one.
    pub(super) fn remember_recent(&mut self, path: &Path) {
        if self.settings.remember(path) {
            self.settings.save();
        }
    }

    /// Saves where the document came from, asking where when it came from
    /// nowhere.
    pub(super) fn save_now(&mut self) -> bool {
        match self.file.clone() {
            // A PDF is not written back: the document made from it is saved
            // as a Word document, and Save asks where.
            Some(path) if is_pdf_path(&path) => self.save_as_now(),
            Some(path) => self.write_document(&path),
            None => self.save_as_now(),
        }
    }

    pub(super) fn save_as_now(&mut self) -> bool {
        let suggested = match self.file.clone() {
            Some(path) if is_pdf_path(&path) => path.with_extension("docx"),
            Some(path) => path,
            None => PathBuf::from(self.untitled_name()),
        };
        self.save_into(&suggested)
    }

    /// What a document with no file of its own would be saved as: the kind it
    /// is, which is what the Save As list opens on.
    pub(super) fn untitled_name(&self) -> String {
        format!("{UNTITLED}.{}", self.document.kind().extension())
    }

    /// Asks where the document goes, starting at a path already worked out.
    ///
    /// The Save As page's recent folders come here: the folder is known, the
    /// name is not, and the dialog opens where the folder is rather than
    /// wherever it happened to be last.
    pub(super) fn save_into(&mut self, suggested: &Path) -> bool {
        match wp_shell::dialog::save_file("Save as", SAVE_FILTERS, Some(suggested)) {
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
        // Whatever was opened is not the text file the last one was.
        self.text_encoding = None;
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
        self.open_path(&path)
    }

    /// Makes a new document from a template, which is what opening a template
    /// from the shell does: Word's verb on a `.dotx` is New, not Open, and the
    /// template stays as it was. The document is untitled, remembers the
    /// template it came from, and is saved as a document.
    ///
    /// Whoever calls this has already asked about unsaved changes.
    pub(super) fn new_from_template(&mut self, path: &Path) -> Response {
        let made = std::fs::read(path)
            .map_err(|error| format!("Cannot read {}: {error}", path.display()))
            .and_then(|bytes| {
                Document::from_template(&bytes, path.to_str())
                    .map_err(|error| format!("Cannot open {}: {error}", path.display()))
            });
        match made {
            Ok(document) => {
                self.set_document(document, None);
                self.status = format!("New document from {}", path.display());
                self.remember_recent(path);
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

    /// Opens a document whose path is already known.
    ///
    /// The Open page's list of documents opened lately goes straight here: it
    /// knows the path, so asking for it again through a file dialog would be
    /// asking a question that has already been answered.
    ///
    /// A template is opened as itself here, for editing it: this is File ▸
    /// Open, and the template is what was asked for.
    ///
    /// Whoever calls this has already asked about unsaved changes.
    pub(crate) fn open_path(&mut self, path: &Path) -> Response {
        let path = path.to_path_buf();
        // A text file is not a package: it is read as text, through the File
        // Conversion dialog where its bytes do not say what they are.
        if super::textfiles::is_text_path(&path) {
            return match std::fs::read(&path) {
                Ok(bytes) => self.open_text_path(&path, bytes),
                Err(error) => {
                    let message = format!("Cannot read {}: {error}", path.display());
                    wp_shell::dialog::show_error(&message);
                    self.status = message;
                    self.needs_redraw = true;
                    Response::Redraw
                }
            };
        }
        if is_pdf_path(&path) && !wp_shell::dialog::ask_ok_cancel(PDF_CONVERSION_NOTICE) {
            self.status = String::from("Not opened");
            return Response::Redraw;
        }
        let rich = is_rtf_path(&path);
        let web = web_kind(&path);
        let opened = std::fs::read(&path)
            .map_err(|error| format!("Cannot read {}: {error}", path.display()))
            .and_then(|bytes| {
                if is_doc_path(&path) {
                    return wp_doc::open(&bytes)
                        .map_err(|error| format!("Cannot open {}: {error}", path.display()));
                }
                if is_odt_path(&path) {
                    return wp_odt::open(&bytes)
                        .map_err(|error| format!("Cannot open {}: {error}", path.display()));
                }
                if is_pdf_path(&path) {
                    return wp_pdf::open(&bytes)
                        .map_err(|error| format!("Cannot open {}: {error}", path.display()));
                }
                match (rich, web) {
                    (true, _) => wp_rtf::open(&bytes),
                    (_, Some(WebKind::Page)) => wp_html::open_html(&bytes, Some(&path)),
                    (_, Some(WebKind::SingleFile)) => wp_html::open_mht(&bytes),
                    _ => Document::open(&bytes),
                }
                .map_err(|error| format!("Cannot open {}: {error}", path.display()))
            });

        match opened {
            Ok(document) => {
                self.set_document(document, Some(path.clone()));
                self.status = format!("Opened {}", path.display());
                self.remember_recent(&path);
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

#[cfg(test)]
mod tests {
    use super::*;
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

    /// A folder of this test's own, because the tests run side by side.
    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("wp-kinds-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&folder);
        folder
    }

    #[test]
    fn the_extension_says_which_kind_the_file_becomes() {
        let folder = folder("extension");
        let mut editor = editor("A letter");
        assert_eq!(editor.untitled_name(), "Document.docx");

        let template = folder.join("Letter.dotx");
        assert!(editor.write_document(&template));
        assert_eq!(editor.document.kind(), Kind::Template);
        assert_eq!(editor.untitled_name(), "Document.dotx");
        let reopened = Document::open(&std::fs::read(&template).unwrap()).unwrap();
        assert_eq!(reopened.kind(), Kind::Template, "the file is not a template");

        let macro_document = folder.join("Letter.docm");
        assert!(editor.write_document(&macro_document));
        assert_eq!(editor.document.kind(), Kind::MacroEnabledDocument);
        assert_eq!(editor.document_name(), "Letter.docm");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_template_opened_makes_a_document_and_stays_as_it_was() {
        let folder = folder("template");
        let template = folder.join("Letter.dotm");
        let mut editor = editor("Dear");
        assert!(editor.write_document(&template));
        let before = std::fs::read(&template).unwrap();

        editor.new_from_template(&template);
        assert!(editor.file.is_none(), "the template itself is what is open");
        assert_eq!(editor.document.kind(), Kind::Document);
        assert_eq!(editor.document_name(), UNTITLED);
        assert!(!editor.document.is_modified());
        assert_eq!(editor.document.plain_text().trim_end(), "Dear");
        assert_eq!(
            editor.document.attached_template().as_deref(),
            template.to_str(),
            "the document does not know where it came from"
        );
        assert_eq!(std::fs::read(&template).unwrap(), before, "the template was changed");

        // File ▸ Open on the same template opens the template itself.
        editor.open_path(&template);
        assert_eq!(editor.document.kind(), Kind::MacroEnabledTemplate);
        assert_eq!(editor.document_name(), "Letter.dotm");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_rich_text_file_is_saved_and_opened_as_one() {
        let folder = folder("rtf");
        let path = folder.join("letter.rtf");
        let mut editor = editor("Dear reader");
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 4));
        editor.document.apply_run_formatting(&wp_docx::model::RunProperties {
            bold: Some(true),
            ..Default::default()
        });
        assert!(editor.write_document(&path));
        let written = std::fs::read(&path).unwrap();
        assert!(written.starts_with(br"{\rtf1"), "not an RTF file");
        assert!(editor.title.contains("[Compatibility Mode]"), "{}", editor.title);

        let mut editor = self::editor("");
        editor.open_path(&path);
        assert_eq!(editor.document.plain_text().trim_end(), "Dear reader");
        assert_eq!(editor.document_name(), "letter.rtf");
        editor.document.set_caret(wp_docx::TextPosition::new(0, 1));
        assert!(editor.document.character_format_here().bold, "the bold was lost");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn an_opendocument_package_is_saved_and_opened_as_one() {
        let folder = folder("odt");
        let path = folder.join("letter.odt");
        let mut editor = editor("Dear reader");
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 4));
        editor.document.apply_run_formatting(&wp_docx::model::RunProperties {
            bold: Some(true),
            ..Default::default()
        });
        assert!(editor.write_document(&path));
        let written = std::fs::read(&path).unwrap();
        assert!(written.starts_with(b"PK"), "not a package");
        assert!(
            written[30..].starts_with(b"mimetypeapplication/vnd.oasis.opendocument.text"),
            "the mimetype is not first and stored"
        );
        assert!(!editor.title.contains("[Compatibility Mode]"), "{}", editor.title);

        let mut editor = self::editor("");
        editor.open_path(&path);
        assert_eq!(editor.document.plain_text().trim_end(), "Dear reader");
        assert_eq!(editor.document_name(), "letter.odt");
        editor.document.set_caret(wp_docx::TextPosition::new(0, 1));
        assert!(editor.document.character_format_here().bold, "the bold was lost");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_pdf_is_converted_into_a_document_and_saved_as_a_word_document() {
        let folder = folder("pdf");
        let path = folder.join("letter.pdf");
        let mut editor = editor("Dear reader");
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 4));
        editor.document.apply_run_formatting(&wp_docx::model::RunProperties {
            bold: Some(true),
            ..Default::default()
        });
        let pages = editor.layout_for_print(wp_layout::Device::paper());
        std::fs::write(&path, wp_pdf::write(&pages, editor.library, "letter")).unwrap();

        let mut editor = self::editor("");
        editor.open_path(&path);
        assert_eq!(editor.document.plain_text().trim_end(), "Dear reader");
        assert_eq!(editor.document_name(), "letter.pdf");
        editor.document.set_caret(wp_docx::TextPosition::new(0, 1));
        assert!(editor.document.character_format_here().bold, "the bold was lost");
        // Saving goes to Save As, as a Word document beside the PDF.
        assert!(editor.file.as_deref().is_some_and(is_pdf_path));
        let suggested = editor.file.clone().map(|path| path.with_extension("docx")).unwrap();
        assert!(editor.write_document(&suggested));
        assert_eq!(editor.document_name(), "letter.docx");
        assert!(Document::open(&std::fs::read(&suggested).unwrap()).is_ok());
        assert!(wp_pdf::looks_like_pdf(&std::fs::read(&path).unwrap()), "the PDF was written over");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_web_page_is_saved_with_its_pictures_beside_it_and_opened_again() {
        let folder = folder("web");
        let path = folder.join("letter.htm");
        let mut editor = editor("Dear reader");
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n\x2d\xb4\0\0\0\0IEND\xaeB`\x82";
        editor.document.set_caret(wp_docx::TextPosition::new(0, 4));
        let _ = editor.document.insert_picture(png, "png", 96 * 9525, 96 * 9525);
        assert!(editor.write_document(&path));
        assert!(
            folder.join("letter_files").join("image001.png").exists(),
            "the picture was not put beside the page"
        );
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("letter_files/image001.png"), "{written}");

        let has_picture = |editor: &Editor| {
            editor.document.body().paragraphs().iter().any(|paragraph| {
                paragraph.runs.iter().any(|run| {
                    run.content.iter().any(|c| matches!(c, wp_docx::model::RunContent::Picture(_)))
                })
            })
        };
        let mut editor = self::editor("");
        editor.open_path(&path);
        assert_eq!(editor.document.plain_text().trim_end(), "Dear reader");
        assert!(has_picture(&editor), "the picture did not come back from beside the page");
        assert_eq!(editor.document_name(), "letter.htm");

        let single = folder.join("letter.mht");
        assert!(editor.write_document(&single));
        let mut editor = self::editor("");
        editor.open_path(&single);
        assert_eq!(editor.document.plain_text().trim_end(), "Dear reader");
        assert!(has_picture(&editor), "the picture did not come back from inside the file");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_path_names_its_kind() {
        assert_eq!(kind_of_path(Path::new("C:/a/b.DOTX")), Some(Kind::Template));
        assert_eq!(kind_of_path(Path::new("b.docx")), Some(Kind::Document));
        assert_eq!(kind_of_path(Path::new("b.txt")), None);
        assert!(is_template_path(Path::new("b.dotm")));
        assert!(!is_template_path(Path::new("b.docm")));
    }
}

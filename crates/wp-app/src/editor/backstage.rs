//! The File tab: what each of its places says, and what pressing a line does.
//!
//! The drawing is [`crate::chrome::backstage`]; this is everything that needs
//! to know what a document is. Word's backstage answers four questions —
//! *what is this?*, *what else have I got?*, *where does this one go?*, and
//! *how do I get a copy out?* — and each place here answers one of them.
//!
//! # Why Print and Options are not pages here
//!
//! Because they exist already. Print is a whole page of its own, built when
//! printing was, and Options is a dialog, built when the settings were. Drawing
//! a second Print page inside the backstage would be two pages to keep in step,
//! and the one that fell behind would be the one somebody was looking at.

use std::path::{Path, PathBuf};

use wp_shell::Response;

use crate::chrome::backstage::{Backstage, Contents, Hit, Place, Row};

use super::files::UNTITLED;
use super::properties::Field;
use super::Editor;

impl Editor {
    /// Whether the backstage is what the window is showing.
    pub(super) fn in_backstage(&self) -> bool {
        self.backstage.is_some()
    }

    /// Opens the backstage at a place, or shuts it if that place is showing.
    ///
    /// Pressing File twice in Word puts it away again, and so does pressing the
    /// same place twice.
    pub(super) fn open_backstage(&mut self, place: Place) -> Response {
        match &mut self.backstage {
            Some(open) if open.place == place => return self.close_backstage(),
            Some(open) => open.show(place),
            None => {
                let mut opened = Backstage::new();
                opened.show(place);
                self.backstage = Some(opened);
            }
        }
        // Nothing behind it is being pointed at any more.
        self.hide_mini_bar();
        self.hide_key_tips();
        self.popup = None;
        self.tip = None;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Goes back to the document.
    pub(super) fn close_backstage(&mut self) -> Response {
        if self.backstage.take().is_none() {
            return Response::Ignored;
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// What the place that is showing has to say.
    pub(super) fn backstage_contents(&self) -> Contents {
        match self.backstage.as_ref().map(|open| open.place).unwrap_or(Place::Info) {
            Place::Info => self.info_page(),
            Place::New => self.new_page(),
            Place::Open => self.open_page(),
            Place::SaveAs => self.save_as_page(),
            Place::Export => self.export_page(),
            // The rest do their work and leave, so they are never showing.
            other => Contents { heading: other.label().to_owned(), ..Contents::default() },
        }
    }

    /// Word's Info: what this document is and what it says about itself.
    fn info_page(&self) -> Contents {
        let name = self.document_name();
        let properties = self.document.properties();

        let where_it_is = match &self.file {
            Some(path) => path
                .parent()
                .map(|folder| folder.display().to_string())
                .unwrap_or_else(|| path.display().to_string()),
            None => String::from("Not saved yet"),
        };
        let size = self
            .file
            .as_ref()
            .and_then(|path| std::fs::metadata(path).ok())
            .map(|found| size_in_words(found.len()))
            .unwrap_or_default();

        let text = self.counted_text(true);
        let facts = vec![
            (String::from("Where"), where_it_is),
            (String::from("Size"), size),
            (String::from("Pages"), self.pages.len().max(1).to_string()),
            (String::from("Words"), super::draw::count_words(&text).to_string()),
            (String::from("Characters"), text.chars().count().to_string()),
            (String::from("Paragraphs"), self.document.paragraph_count().to_string()),
            (String::from("Created"), on_the_day(&properties.created)),
            (String::from("Last saved"), on_the_day(&properties.modified)),
            (String::from("Last saved by"), properties.last_modified_by.clone()),
        ];

        // The properties are lines to press, because in Word's Info they are
        // boxes to type in: the panel down the right of that page is the one
        // place a title or an author is set.
        let rows = Field::ALL
            .iter()
            .map(|field| {
                let value = field.read(&properties);
                let note = if value.trim().is_empty() {
                    format!("Add {}", field.label().to_lowercase())
                } else {
                    value
                };
                Row::new(field.label(), note)
            })
            .collect();

        Contents {
            heading: name,
            facts,
            rows_heading: String::from("Properties"),
            rows,
            ..Contents::default()
        }
    }

    /// Word's New. There is one thing to make, and it says so.
    fn new_page(&self) -> Contents {
        Contents {
            heading: String::from("New"),
            rows: vec![Row::new(
                "Blank document",
                "An empty document in the theme new documents are made with",
            )],
            ..Contents::default()
        }
    }

    /// Word's Open: browse for one, or take one that was open lately.
    fn open_page(&self) -> Contents {
        let mut rows = vec![Row::new("Browse", "Look for a document on this computer")];
        rows.extend(self.settings.recent.iter().map(|path| {
            let path = Path::new(path);
            let name =
                path.file_name().and_then(|name| name.to_str()).unwrap_or(UNTITLED).to_owned();
            let folder =
                path.parent().map(|folder| folder.display().to_string()).unwrap_or_default();
            Row::new(name, folder)
        }));

        Contents {
            heading: String::from("Open"),
            rows_heading: String::from("Recent"),
            // Browse is above the heading, because it is not a recent document.
            rows_heading_at: 1,
            nothing: String::from("Nothing has been opened yet"),
            rows,
            ..Contents::default()
        }
    }

    /// Word's Save As: the folders it has seen lately, and a way to any other.
    fn save_as_page(&self) -> Contents {
        let mut rows = vec![Row::new("Browse", "Choose where this document goes")];
        rows.extend(
            self.recent_folders()
                .into_iter()
                .map(|folder| Row::new(folder_name(&folder), folder.display().to_string())),
        );

        Contents {
            heading: String::from("Save As"),
            facts: vec![(
                String::from("This document"),
                match &self.file {
                    Some(path) => path.display().to_string(),
                    None => String::from("Not saved yet"),
                },
            )],
            rows_heading: String::from("Recent folders"),
            rows_heading_at: 1,
            rows,
            nothing: String::from("Nowhere has been saved to yet"),
        }
    }

    /// Word's Export. What this program can write that is not a `.docx` is a
    /// PDF, and it says that rather than offering a list of one.
    fn export_page(&self) -> Contents {
        Contents {
            heading: String::from("Export"),
            rows: vec![Row::new(
                "Create a PDF",
                "The document exactly as it prints, in a file anybody can open",
            )],
            ..Contents::default()
        }
    }

    /// The folders the documents opened lately are in, each named once.
    ///
    /// Word's Save As lists folders rather than files, and a person who has
    /// eight documents in one folder wants that folder once.
    fn recent_folders(&self) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = Vec::new();
        for path in &self.settings.recent {
            let Some(folder) = Path::new(path).parent() else { continue };
            if folder.as_os_str().is_empty() || folders.iter().any(|found| found == folder) {
                continue;
            }
            folders.push(folder.to_path_buf());
        }
        folders
    }

    /// Goes to a tab on the strip.
    ///
    /// File is not a tab in the sense the others are: it has no ribbon page,
    /// and choosing it opens the backstage over the whole window. Every way of
    /// choosing a tab comes through here so that is true of all of them —
    /// pressing it, and Alt then F.
    pub(super) fn choose_tab(&mut self, tab: crate::chrome::ribbon::Tab) -> Response {
        if tab == crate::chrome::ribbon::Tab::File {
            return self.open_backstage(Place::Info);
        }
        self.ribbon.tab = tab;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Steps up or down the rail with the arrow keys.
    ///
    /// Only over the places that have a page. An arrow key that saved the
    /// document or shut it because it stepped onto Save or Close would be a
    /// keyboard that does something nobody asked for.
    pub(super) fn walk_the_rail(&mut self, downwards: bool) -> Response {
        let Some(open) = &self.backstage else { return Response::Ignored };
        let pages: Vec<Place> =
            Place::ALL.iter().copied().filter(|place| place.has_a_page()).collect();
        let Some(at) = pages.iter().position(|place| *place == open.place) else {
            return Response::Ignored;
        };
        let wanted = if downwards { at + 1 } else { at.wrapping_sub(1) };
        let Some(place) = pages.get(wanted).copied() else { return Response::Ignored };
        self.go_to_place(place)
    }

    /// Draws it, filling the window under the caption bar.
    pub(super) fn draw_backstage(&mut self) {
        // Worked out before the pane is taken out of the editor, because what
        // it has to say depends on which place is showing.
        let contents = self.backstage_contents();
        let Some(mut open) = self.backstage.take() else { return };
        let theme = self.theme;
        open.draw(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            &contents,
            crate::chrome::TITLE_HEIGHT,
            &theme,
        );
        self.backstage = Some(open);
    }

    /// Follows the pointer over the backstage.
    pub(super) fn backstage_hover(&mut self, x: i32, y: i32) -> bool {
        self.backstage.as_mut().is_some_and(|open| open.hover(x, y))
    }

    /// Winds the page of the backstage.
    pub(super) fn backstage_scroll(&mut self, notches: f32) -> bool {
        self.backstage.as_mut().is_some_and(|open| open.scroll_by(notches))
    }

    /// A press on the backstage.
    pub(super) fn backstage_press(&mut self, x: i32, y: i32) -> Response {
        let Some(open) = &self.backstage else { return Response::Ignored };
        let Some(hit) = open.at(x, y) else { return Response::Ignored };
        match hit {
            Hit::Back => self.close_backstage(),
            Hit::Place(place) => self.go_to_place(place),
            Hit::Row(index) => self.press_backstage_row(index),
        }
    }

    /// Goes to one of the places on the rail.
    fn go_to_place(&mut self, place: Place) -> Response {
        // The ones with a page stay here and show it.
        if place.has_a_page() {
            if let Some(open) = &mut self.backstage {
                if open.place == place {
                    return Response::Ignored;
                }
                open.show(place);
                self.needs_redraw = true;
                return Response::Redraw;
            }
            return Response::Ignored;
        }

        // And the ones without do their work and leave. Save is the exception
        // to leaving: Word stays in the backstage when a save it could do
        // silently is done, and goes back to the document only when it had to
        // ask where.
        match place {
            Place::Save => {
                let asked = self.file.is_none();
                self.save_now();
                self.update_title();
                if asked {
                    return self.close_backstage();
                }
                self.needs_redraw = true;
                Response::Redraw
            }
            Place::Print => {
                self.close_backstage();
                self.open_print()
            }
            Place::Close => {
                if self.may_discard() {
                    self.close_backstage();
                    self.new_document()
                } else {
                    Response::Ignored
                }
            }
            Place::Options => {
                self.close_backstage();
                self.open_options()
            }
            // Every other place has a page and was dealt with above.
            _ => Response::Ignored,
        }
    }

    /// A press on one of the lines of the page that is showing.
    fn press_backstage_row(&mut self, index: usize) -> Response {
        let Some(open) = &self.backstage else { return Response::Ignored };
        match open.place {
            // The properties: the same strip that takes any other single line
            // of typing, which is where a title or an author is set.
            Place::Info => {
                self.close_backstage();
                self.choose_property(index)
            }
            Place::New => {
                self.close_backstage();
                self.new_document()
            }
            Place::Open => self.open_from_page(index),
            Place::SaveAs => self.save_from_page(index),
            Place::Export => {
                let pages: Vec<usize> = (1..=self.pages.len()).collect();
                if self.write_pdf(&pages) {
                    self.close_backstage()
                } else {
                    self.needs_redraw = true;
                    Response::Redraw
                }
            }
            _ => Response::Ignored,
        }
    }

    /// The first line of the Open page browses; the rest are the documents
    /// opened lately.
    fn open_from_page(&mut self, index: usize) -> Response {
        if index == 0 {
            self.close_backstage();
            return self.open_document();
        }
        let Some(path) = self.settings.recent.get(index - 1).cloned() else {
            return Response::Ignored;
        };
        if !self.may_discard() {
            return Response::Ignored;
        }
        self.close_backstage();
        self.open_path(Path::new(&path))
    }

    /// The first line of the Save As page browses; the rest are the folders the
    /// recent documents came from.
    fn save_from_page(&mut self, index: usize) -> Response {
        if index == 0 {
            self.save_as_now();
            return self.close_backstage();
        }
        let Some(folder) = self.recent_folders().into_iter().nth(index - 1) else {
            return Response::Ignored;
        };
        // The dialog is still shown, because a folder is where the document
        // goes and not what it is called. It simply opens there, which is what
        // Word's recent folders do.
        let name = match &self.file {
            Some(path) => path.file_name().map(std::ffi::OsStr::to_os_string),
            None => None,
        };
        let suggested = folder.join(name.unwrap_or_else(|| format!("{UNTITLED}.docx").into()));
        self.save_into(&suggested);
        self.close_backstage()
    }
}

/// A file's size the way a person says it.
///
/// Word's Info says "24.5KB"; this says the same thing in the same units, which
/// are powers of a thousand and not of 1024 — the units the operating system
/// uses when it says how big a file is.
fn size_in_words(bytes: u64) -> String {
    const STEP: f64 = 1000.0;
    let bytes = bytes as f64;
    if bytes < STEP {
        return format!("{bytes:.0} bytes");
    }
    for (limit, mark) in [(STEP * STEP, "KB"), (STEP * STEP * STEP, "MB")] {
        if bytes < limit {
            return format!("{:.1}{mark}", bytes / (limit / STEP));
        }
    }
    format!("{:.1}GB", bytes / (STEP * STEP * STEP))
}

/// The day out of a date stamp from the document's properties.
///
/// They are written the way the format demands — `2026-09-10T11:04:00Z` — and
/// the time of day is not what anybody is asking when they look at Info.
fn on_the_day(stamp: &str) -> String {
    match stamp.split_once('T') {
        Some((day, _)) => day.to_owned(),
        None => stamp.to_owned(),
    }
}

/// What a folder is called, for a line that shows the whole path underneath.
fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| folder.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("A sentence of five words")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    #[test]
    fn the_file_tab_opens_the_backstage_rather_than_a_ribbon_page() {
        let mut editor = editor();
        assert!(!editor.in_backstage());
        editor.open_backstage(Place::Info);
        assert!(editor.in_backstage());
        // And the tab strip is not left showing a File tab that has no page.
        assert_ne!(editor.ribbon.tab, crate::chrome::ribbon::Tab::File);
    }

    #[test]
    fn pressing_the_same_place_twice_puts_it_away() {
        let mut editor = editor();
        editor.open_backstage(Place::Info);
        editor.open_backstage(Place::Info);
        assert!(!editor.in_backstage());
    }

    #[test]
    fn escape_goes_back_to_the_document() {
        let mut editor = editor();
        editor.open_backstage(Place::Info);
        editor.handle(Event::KeyDown { key: Key::Escape, modifiers: Modifiers::default() });
        assert!(!editor.in_backstage());
    }

    #[test]
    fn info_says_what_the_document_is() {
        let editor = editor();
        let contents = editor.info_page();
        let facts: Vec<&str> = contents.facts.iter().map(|(label, _)| label.as_str()).collect();
        assert!(facts.contains(&"Words"), "got {facts:?}");
        let words = contents
            .facts
            .iter()
            .find(|(label, _)| label == "Words")
            .map(|(_, value)| value.clone())
            .expect("a count of the words");
        assert_eq!(words, "5", "the paragraph has five words in it");
    }

    #[test]
    fn info_offers_every_property_word_offers() {
        let editor = editor();
        let contents = editor.info_page();
        assert_eq!(contents.rows.len(), Field::ALL.len());
        assert_eq!(contents.rows[0].title, "Title");
    }

    #[test]
    fn the_open_page_lists_what_was_opened_lately_under_a_way_to_browse() {
        let mut editor = editor();
        editor.settings.recent =
            vec![String::from("/somewhere/Report.docx"), String::from("/elsewhere/Notes.docx")];

        let contents = editor.open_page();
        assert_eq!(contents.rows[0].title, "Browse");
        assert_eq!(contents.rows[1].title, "Report.docx");
        assert_eq!(contents.rows[1].note, "/somewhere");
        assert_eq!(contents.rows[2].title, "Notes.docx");
    }

    #[test]
    fn save_as_names_each_folder_once() {
        // Eight documents in one folder is one folder, not eight lines saying
        // the same thing.
        let mut editor = editor();
        editor.settings.recent = vec![
            String::from("/somewhere/One.docx"),
            String::from("/somewhere/Two.docx"),
            String::from("/elsewhere/Three.docx"),
        ];
        let folders = editor.recent_folders();
        assert_eq!(folders.len(), 2, "got {folders:?}");
    }

    #[test]
    fn a_place_that_simply_does_something_leaves_the_backstage() {
        let mut editor = editor();
        editor.open_backstage(Place::Info);
        editor.go_to_place(Place::Print);
        assert!(!editor.in_backstage(), "the backstage stayed open over the Print page");
        assert!(editor.printing());
    }

    #[test]
    fn a_size_is_said_the_way_a_person_says_it() {
        assert_eq!(size_in_words(0), "0 bytes");
        assert_eq!(size_in_words(999), "999 bytes");
        assert_eq!(size_in_words(1_500), "1.5KB");
        assert_eq!(size_in_words(2_500_000), "2.5MB");
        assert_eq!(size_in_words(3_000_000_000), "3.0GB");
    }

    #[test]
    fn a_date_stamp_is_shown_as_a_day() {
        assert_eq!(on_the_day("2026-09-10T11:04:00Z"), "2026-09-10");
        // And one written some other way is shown as it was written rather
        // than thrown away.
        assert_eq!(on_the_day("who knows"), "who knows");
    }
}

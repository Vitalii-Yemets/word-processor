//! Comparing two documents, combining three, and the view that shows them
//! side by side.
//!
//! # The two commands under one button
//!
//! Word's Compare button drops open onto two things. **Compare** takes this
//! document and another and marks how the second differs from the first, as
//! tracked changes nobody wrote — they were worked out. **Combine** takes two
//! copies of this document that two people edited without seeing each other's
//! work, and puts both sets of changes in, each marked with its author's
//! name. See [`wp_docx::compare`] and [`wp_docx::combine`].
//!
//! # Why the view matters
//!
//! Because a document full of tracked changes is hard to read, and the
//! question a person actually has is "what did it say before, and what does
//! it say now?" Word answers it by showing all three at once: the result in
//! the window, and the two it came from in a column beside it. This is that
//! column. It is not a second window and not a second editor — the two
//! documents beside the result are laid out once and drawn, and nothing can
//! be typed into them.

use wp_docx::compare::Options;
use wp_docx::Document;

use crate::chrome::dialog::{Dialog, Field};

use super::dialogs::Asking;
use wp_layout::{LayoutEngine, Page};
use wp_shell::Response;

use super::Editor;

/// A rectangle drawn round something, which every pane in this program
/// draws round its boxes.
fn outline(
    canvas: &mut wp_raster::Canvas,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    colour: wp_raster::Color,
) {
    let (x, y, width, height) = (x as i32, y as i32, width as i32, height as i32);
    canvas.fill_rect(x, y, width, 1, colour);
    canvas.fill_rect(x, y + height - 1, width, 1, colour);
    canvas.fill_rect(x, y, 1, height, colour);
    canvas.fill_rect(x + width - 1, y, 1, height, colour);
}

/// The three things the Compare button drops open onto, in Word's order.
///
/// Named here rather than written where the list is built, so that what the
/// button offers and what pressing a line does are read off one list.
pub(super) fn choices() -> Vec<String> {
    use crate::messages::t;
    vec![
        t("Compare Two Documents").to_owned(),
        t("Combine Revisions from Two Authors").to_owned(),
        t("Close the Comparison").to_owned(),
    ]
}

/// How wide the column beside the document is.
///
/// Wide enough that a page in it is readable as a shape — where the
/// paragraphs are, which of them is longer — rather than as a grey smudge.
pub(super) const WIDTH: f32 = 260.0;

/// The room round a page inside it.
const PADDING: f32 = 12.0;

/// How tall the caption over each half is.
const CAPTION: f32 = 22.0;

/// The two documents a comparison came from, laid out to be looked at.
#[derive(Debug)]
pub(super) struct Comparing {
    /// What each is called, which is the name of the file it came from.
    pub original: String,
    pub revised: String,
    pub original_pages: Vec<Page>,
    pub revised_pages: Vec<Page>,
}

/// Where each answer sits in the Compare dialog.
///
/// Named rather than counted twice, for the reason every other dialog in this
/// program names its rows.
pub(super) const MOVES: usize = 3;
pub(super) const FORMATTING: usize = 4;
pub(super) const CASE: usize = 5;
pub(super) const WHITE_SPACE: usize = 6;
pub(super) const INTO: usize = 8;

/// What the dialog said to take notice of.
#[must_use]
pub(super) fn options_from(dialog: &crate::chrome::dialog::Dialog) -> Options {
    Options {
        moves: dialog.ticked(MOVES),
        formatting: dialog.ticked(FORMATTING),
        case: dialog.ticked(CASE),
        white_space: dialog.ticked(WHITE_SPACE),
    }
}

impl Editor {
    /// Word's Compare dialog: what the two documents are, what counts as a
    /// difference, and where the answer goes.
    pub(super) fn open_compare_dialog(&mut self) -> Response {
        let (original, revised) = match &self.to_compare {
            Some((path, _)) => (self.document_name(), name_of(path)),
            None => return Response::Ignored,
        };
        let defaults = Options::default();

        let dialog = Dialog::new(
            "Compare Documents",
            vec![
                Field::Said { label: "Original".to_owned(), value: original },
                Field::Said { label: "Revised".to_owned(), value: revised },
                Field::Heading("Show changes".to_owned()),
                Field::Check { label: "Moves".to_owned(), on: defaults.moves },
                Field::Check { label: "Formatting".to_owned(), on: defaults.formatting },
                Field::Check { label: "Case changes".to_owned(), on: defaults.case },
                Field::Check { label: "White space".to_owned(), on: defaults.white_space },
                Field::Heading("Show changes in".to_owned()),
                Field::Choice {
                    label: "Where the comparison goes".to_owned(),
                    items: vec![
                        crate::messages::t("A new document").to_owned(),
                        crate::messages::t("This document").to_owned(),
                    ],
                    current: 0,
                },
            ],
        )
        .wide(460.0);
        self.ask(Asking::Compare, dialog)
    }

    /// Gives up on a comparison that was asked about and not answered.
    pub(super) fn cancel_comparison(&mut self) {
        self.to_compare = None;
    }
}

impl Editor {
    /// How much room the comparison column takes, which is none when it is
    /// shut.
    pub(super) fn compare_pane_width(&self) -> f32 {
        if self.comparing.is_some() {
            WIDTH
        } else {
            0.0
        }
    }

    /// Does whichever was chosen.
    pub(super) fn choose_comparing(&mut self, index: usize) -> Response {
        self.popup = None;
        match index {
            0 => self.compare_documents(),
            1 => self.combine_documents(),
            2 => self.close_comparison(),
            _ => Response::Ignored,
        }
    }

    /// Shuts the column.
    pub(super) fn close_comparison(&mut self) -> Response {
        if self.comparing.take().is_none() {
            return self.report("There is nothing to close");
        }
        self.clamp_scroll();
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Word's Combine: two people's copies of this document, put together.
    pub(super) fn combine_documents(&mut self) -> Response {
        let Some((first_path, first)) = self.ask_for_a_document("The first author's copy") else {
            return Response::Ignored;
        };
        let Some((second_path, second)) = self.ask_for_a_document("The second author's copy")
        else {
            return Response::Ignored;
        };

        // Whoever each document says wrote it last, which is what a document
        // knows about its own author. The file's name where it says nothing,
        // because a change marked "Unknown" twice is two changes nobody can
        // tell apart.
        let authors = (author_of(&first, &first_path), author_of(&second, &second_path));
        let before = self.document.clone();
        let combined = self.document.combine(&first, &second, (&authors.0, &authors.1));

        self.show_comparison(&before, &first, &second_path, Some(&second));
        // The column shows the two copies rather than this document before
        // and one of them, because those are what a combining came from.
        if let Some(comparing) = self.comparing.as_mut() {
            comparing.original = name_of(&first_path);
            comparing.revised = name_of(&second_path);
        }
        self.relayout();
        self.reveal_caret();

        let note = match (combined.changes, combined.conflicts) {
            (0, _) => "The two copies say what this document says".to_owned(),
            (changes, 0) => format!("{changes} changes from {} and {}", authors.0, authors.1),
            (changes, conflicts) => format!(
                "{changes} changes, and {conflicts} places where {} and {} disagree",
                authors.0, authors.1
            ),
        };
        self.edited(combined.changes > 0, &note)
    }

    /// Asks for a document and opens it.
    fn ask_for_a_document(
        &mut self,
        title: &'static str,
    ) -> Option<(std::path::PathBuf, Document)> {
        let filters = [wp_shell::dialog::FileFilter { label: "Word documents", pattern: "*.docx" }];
        let path = wp_shell::dialog::open_file(crate::messages::t(title), &filters)?;
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.report(&format!("It could not be read: {error}"));
                return None;
            }
        };
        match Document::open(&bytes) {
            Ok(document) => Some((path, document)),
            Err(error) => {
                self.report(&format!("It could not be opened: {error}"));
                None
            }
        }
    }

    /// Opens the column, with the two documents the result came from laid out
    /// in it.
    pub(super) fn show_comparison(
        &mut self,
        original: &Document,
        revised: &Document,
        revised_path: &std::path::Path,
        _second: Option<&Document>,
    ) {
        let original_pages = self.lay_out(original);
        let revised_pages = self.lay_out(revised);
        self.comparing = Some(Comparing {
            original: crate::messages::t("This document, before").to_owned(),
            revised: name_of(revised_path),
            original_pages,
            revised_pages,
        });
        self.clamp_scroll();
        self.needs_redraw = true;
    }

    /// Lays a document out to be looked at rather than edited.
    fn lay_out(&self, document: &Document) -> Vec<Page> {
        let mut engine = LayoutEngine::new(self.library);
        engine.layout_document(document)
    }

    /// Draws the column: the two documents, one above the other.
    pub(super) fn draw_compare_pane(&mut self) {
        if self.comparing.is_none() {
            return;
        }
        let theme = self.theme;
        let left = self.view_width as f32 - WIDTH - crate::chrome::SCROLLBAR_THICKNESS;
        let (top, bottom) = (self.whole_content_top(), self.window_bottom());
        let height = (bottom - top).max(1.0);
        self.canvas.fill_rect(left as i32, top as i32, WIDTH as i32, height as i32, theme.pane);
        self.canvas.fill_rect(left as i32, top as i32, 1, height as i32, theme.field_edge);

        let half = height / 2.0;
        let showing = self.caret_page().saturating_sub(1);
        let (original, revised) = match &self.comparing {
            Some(comparing) => (comparing.original.clone(), comparing.revised.clone()),
            None => return,
        };
        self.draw_comparison_half(left, top, half, &original, showing, true);
        self.draw_comparison_half(left, top + half, half, &revised, showing, false);
    }

    /// One half of it: a caption, and the page under it.
    fn draw_comparison_half(
        &mut self,
        left: f32,
        top: f32,
        height: f32,
        caption: &str,
        showing: usize,
        first: bool,
    ) {
        let theme = self.theme;
        let line = self.chrome_engine.simple_line(
            &crate::messages::translated(caption),
            left + PADDING,
            top + CAPTION - 6.0,
            9.0,
            theme.text,
        );
        self.renderer.draw_onto(&mut self.canvas, &line, 0.0, 0.0);

        let room_top = top + CAPTION;
        let room_height = (height - CAPTION - PADDING).max(1.0);
        let room_width = WIDTH - PADDING * 2.0;

        let Some(comparing) = &self.comparing else { return };
        let pages = if first { &comparing.original_pages } else { &comparing.revised_pages };
        let Some(page) = pages.get(showing).or_else(|| pages.first()) else { return };

        let scale = (room_width / page.width).min(room_height / page.height);
        let width = page.width * scale;
        let page_height = page.height * scale;
        let page_left = left + (WIDTH - width) / 2.0;
        let page_top = room_top;

        self.canvas.fill_rect(
            page_left as i32,
            page_top as i32,
            width as i32,
            page_height as i32,
            theme.page,
        );
        outline(&mut self.canvas, page_left, page_top, width, page_height, theme.page_edge);

        let transform = wp_raster::Transform::scale(scale, scale)
            .then(&wp_raster::Transform::translate(page_left, page_top));
        let page = page.clone();
        self.renderer.draw_transformed(&mut self.canvas, &page, &transform);
    }
}

/// Whoever a document says wrote it last, or the file's own name.
fn author_of(document: &Document, path: &std::path::Path) -> String {
    let said = document.properties().last_modified_by;
    if said.trim().is_empty() {
        name_of(path)
    } else {
        said
    }
}

/// A file's name without the folders in front of it.
fn name_of(path: &std::path::Path) -> String {
    path.file_stem().and_then(|name| name.to_str()).unwrap_or("Document").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::dialog::Answer;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        for line in ["one", "two", "three"] {
            body.blocks.push(Block::Paragraph(Paragraph::text(line)));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// The same three paragraphs with the case of one of them changed, which
    /// is a difference only if case is being compared.
    fn cased() -> Vec<u8> {
        let mut body = Body::default();
        for line in ["one", "TWO", "three"] {
            body.blocks.push(Block::Paragraph(Paragraph::text(line)));
        }
        Document::create(&body).expect("a document").save().expect("saving")
    }

    fn revised() -> Document {
        let mut body = Body::default();
        for line in ["one", "TWO", "three"] {
            body.blocks.push(Block::Paragraph(Paragraph::text(line)));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    }

    #[test]
    fn the_column_takes_its_width_out_of_the_page_area() {
        let mut editor = editor();
        let before = editor.viewport_width();
        assert_eq!(editor.compare_pane_width(), 0.0);

        let original = editor.document.clone();
        let revised = revised();
        editor.show_comparison(&original, &revised, std::path::Path::new("Revised.docx"), None);

        assert_eq!(editor.compare_pane_width(), WIDTH);
        assert!(
            (before - editor.viewport_width() - WIDTH).abs() < 0.5,
            "the page area did not make room: {before} then {}",
            editor.viewport_width()
        );
    }

    #[test]
    fn closing_it_gives_the_room_back() {
        let mut editor = editor();
        let before = editor.viewport_width();
        let original = editor.document.clone();
        editor.show_comparison(&original, &revised(), std::path::Path::new("Revised.docx"), None);

        editor.close_comparison();
        assert_eq!(editor.compare_pane_width(), 0.0);
        assert!((before - editor.viewport_width()).abs() < 0.5);
        // And closing what is already closed says so rather than doing
        // nothing silently.
        editor.close_comparison();
        assert!(editor.status.contains("nothing to close"), "{}", editor.status);
    }

    #[test]
    fn both_documents_are_laid_out_to_be_looked_at() {
        let mut editor = editor();
        let original = editor.document.clone();
        editor.show_comparison(&original, &revised(), std::path::Path::new("Revised.docx"), None);

        let comparing = editor.comparing.as_ref().expect("the column");
        assert!(!comparing.original_pages.is_empty());
        assert!(!comparing.revised_pages.is_empty());
        assert_eq!(comparing.revised, "Revised", "the file's own name is what it is called");
    }

    #[test]
    fn the_button_offers_the_two_things_it_does_and_a_way_out() {
        let items = choices();
        assert_eq!(items.len(), 3);
        assert!(items[0].contains("Compare"), "{items:?}");
        assert!(items[1].contains("Combine"), "{items:?}");
        assert!(items[2].contains("Close"), "{items:?}");
    }

    #[test]
    fn the_last_of_them_closes_the_column() {
        let mut editor = editor();
        let original = editor.document.clone();
        editor.show_comparison(&original, &revised(), std::path::Path::new("Revised.docx"), None);
        assert!(editor.comparing.is_some());

        editor.choose_comparing(choices().len() - 1);
        assert!(editor.comparing.is_none(), "the last line did not close it");
    }
    /// Puts a document in the way a file dialog would have, and opens the
    /// question Word asks before it compares.
    fn about_to_compare(editor: &mut Editor, revised: Document) {
        editor.to_compare = Some((std::path::PathBuf::from("Revised.docx"), Box::new(revised)));
        editor.open_compare_dialog();
    }

    #[test]
    fn the_dialog_names_both_documents_and_offers_what_word_offers() {
        let mut editor = editor();
        about_to_compare(&mut editor, revised());
        let dialog = editor.dialog.as_ref().expect("the dialog");

        assert_eq!(dialog.title, "Compare Documents");
        // A row the dialog says rather than asks, so it is read as one.
        let Some(Field::Said { value, .. }) = dialog.fields.get(1) else {
            panic!("the revised document is not named: {:?}", dialog.fields.get(1))
        };
        assert!(value.contains("Revised"), "{value}");
        for row in [MOVES, FORMATTING, CASE, WHITE_SPACE] {
            assert!(dialog.ticked(row), "row {row} is not ticked to begin with");
        }
        assert_eq!(dialog.chose(INTO), 0, "the answer does not go into a new document");
    }

    #[test]
    fn what_the_ticks_say_is_what_is_compared() {
        // The same two documents, compared twice: once taking notice of the
        // case and once not.
        let mut told = editor();
        told.document.set_caret(wp_docx::TextPosition::new(0, 0));
        about_to_compare(&mut told, Document::open(&cased()).expect("a document"));
        told.finish_dialog(Answer::Accept);
        assert!(
            told.document.changes().iter().any(|change| {
                change.kind == wp_docx::revisions::ChangeKind::Insertion
                    || change.kind == wp_docx::revisions::ChangeKind::Deletion
            }),
            "a change of case was not marked"
        );

        let mut ignored = editor();
        about_to_compare(&mut ignored, Document::open(&cased()).expect("a document"));
        if let Some(Field::Check { on, .. }) =
            ignored.dialog.as_mut().expect("the dialog").fields.get_mut(CASE)
        {
            *on = false;
        }
        ignored.finish_dialog(Answer::Accept);
        assert!(
            ignored.document.changes().is_empty(),
            "a change of case was marked when case was not being compared: {:?}",
            ignored.document.changes()
        );
    }

    #[test]
    fn a_comparison_into_a_new_document_leaves_the_file_it_came_from_alone() {
        let mut fresh = editor();
        fresh.file = Some(std::path::PathBuf::from("Original.docx"));
        about_to_compare(&mut fresh, revised());
        fresh.finish_dialog(Answer::Accept);
        assert!(fresh.file.is_none(), "the result kept the name of the document it came from");

        // And into this document, where it keeps it.
        let mut kept = editor();
        kept.file = Some(std::path::PathBuf::from("Original.docx"));
        about_to_compare(&mut kept, revised());
        if let Some(Field::Choice { current, .. }) =
            kept.dialog.as_mut().expect("the dialog").fields.get_mut(INTO)
        {
            *current = 1;
        }
        kept.finish_dialog(Answer::Accept);
        assert_eq!(kept.file.as_deref(), Some(std::path::Path::new("Original.docx")));
    }

    #[test]
    fn saying_no_to_the_dialog_compares_nothing() {
        let mut editor = editor();
        about_to_compare(&mut editor, revised());
        editor.finish_dialog(Answer::Cancel);
        assert!(editor.to_compare.is_none(), "the document it was going to compare is still there");
        assert!(editor.document.changes().is_empty());
    }
}

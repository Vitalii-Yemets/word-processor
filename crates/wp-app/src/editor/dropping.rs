//! What comes in from the desktop by drag, and what goes out that way.
//!
//! Three things can be dropped on the window: files, which open — or, if
//! they are pictures, go into the page where they land; text another
//! program is dragging, in whatever formats it carries, which is pasted
//! where it lands; and text this program itself is dragging, when it comes
//! back in from a trip outside the window. And selected text dragged out
//! of the window goes to whatever program takes it, as a copy or as a
//! move — the same formats a copy puts on the clipboard, since a drag is a
//! copy that has not let go yet. See [`super::dragtext`] for a drag that
//! stays inside the window, and [`super::clipboardformats`] for the
//! formats.

use std::path::{Path, PathBuf};

use wp_docx::TextPosition;
use wp_shell::clipboard::Contents;
use wp_shell::{DragEffect, Response};

use super::files::{is_doc_path, is_odt_path, is_pdf_path, is_rtf_path, is_web_path};
use super::paste::PasteAs;
use super::Editor;

/// The picture files a drop puts on the page rather than opening.
const PICTURE_EXTENSIONS: &[&str] =
    &["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "emf", "wmf"];

impl Editor {
    /// Something from outside is being dragged over the page: the mark
    /// that shows where it would land follows the pointer.
    pub(super) fn foreign_drag_over(&mut self, x: i32, y: i32) -> Response {
        let onto = self.position_at(x, y);
        if self.foreign_drop == onto {
            return Response::Ignored;
        }
        self.foreign_drop = onto;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Whatever was being dragged over the page has gone away again.
    pub(super) fn foreign_drag_left(&mut self) -> Response {
        if self.foreign_drop.take().is_none() {
            return Response::Ignored;
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Files dropped on the window: a picture goes into the page where it
    /// landed; a document opens.
    pub(super) fn drop_files(&mut self, paths: Vec<PathBuf>, x: i32, y: i32) -> Response {
        self.foreign_drop = None;
        let mut response = Response::Redraw;
        for path in &paths {
            if is_picture_path(path) {
                if self.is_locked() {
                    return self.refuse_locked();
                }
                if let Some(at) = self.position_at(x, y) {
                    self.document.move_caret(at, false);
                }
                response = self.insert_picture_file(path);
            } else if is_document_path(path) {
                // One document opens; the others would each need a window
                // of their own, and the first is what was meant.
                let path = path.clone();
                return self.after_asking_to_save(move |editor| editor.open_path(&path));
            }
        }
        self.needs_redraw = true;
        response
    }

    /// Text another program dragged in, let go on the page: pasted where it
    /// landed, in the richest format it came in.
    pub(super) fn drop_data(&mut self, contents: Contents, x: i32, y: i32) -> Response {
        self.foreign_drop = None;
        if self.is_locked() {
            return self.refuse_locked();
        }
        if let Some(at) = self.position_at(x, y) {
            self.document.move_caret(at, false);
        }
        let (text, blocks) = self.take_contents(contents);
        if text.is_empty() && blocks.is_empty() {
            self.needs_redraw = true;
            return Response::Redraw;
        }
        self.put_down(&text, &blocks, PasteAs::KeepSource)
    }

    /// The text being dragged has left the window: it goes to the desktop
    /// as a drag, and what the other program did with it decides what
    /// happens to the original.
    pub(super) fn drag_text_out(&mut self) -> Response {
        let Some(drag) = self.text_drag.clone() else { return Response::Ignored };
        let text = self.document.selected_text();
        let contents = self.clipboard_contents_of_selection(&text, drag.blocks());
        match wp_shell::start_drag(&contents) {
            // Back in this window after all: the drag finishes the way a
            // drag inside the window does.
            DragEffect::DroppedOnSelf { x, y, copying } => self.drop_text(x, y, copying),
            DragEffect::Move => {
                self.text_drag = None;
                let (start, end) = drag.from();
                self.document.set_caret(start);
                self.document.extend_selection_to(end);
                let changed = self.document.delete_selection();
                let words = text.split_whitespace().count();
                self.edited(changed, &format!("Moved {words} words"))
            }
            DragEffect::Copy | DragEffect::None => {
                self.text_drag = None;
                self.needs_redraw = true;
                Response::Redraw
            }
        }
    }

    /// Whether a point is outside the drawing area, which is where a drag
    /// inside the window becomes a drag to the desktop.
    #[must_use]
    pub(super) fn is_outside_window(&self, x: i32, y: i32) -> bool {
        x < 0 || y < 0 || x >= self.view_width as i32 || y >= self.view_height as i32
    }

    /// Where whatever is being dragged would land, from inside the window
    /// or from outside it, for the mark that shows it.
    #[must_use]
    pub(super) fn drop_target(&self) -> Option<TextPosition> {
        self.text_drop_target().or(self.foreign_drop)
    }
}

fn is_picture_path(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| {
        PICTURE_EXTENSIONS.iter().any(|known| extension.eq_ignore_ascii_case(known))
    })
}

fn is_document_path(path: &Path) -> bool {
    super::files::kind_of_path(path).is_some()
        || is_doc_path(path)
        || is_rtf_path(path)
        || is_odt_path(path)
        || is_pdf_path(path)
        || is_web_path(path)
        || super::textfiles::is_text_path(path)
}

#[cfg(test)]
mod tests {
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use super::*;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor(text: &str) -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::default()));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.type_text(text);
        editor.relayout();
        editor
    }

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("wp-drop-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&folder);
        folder
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n\x2d\xb4\0\0\0\0IEND\xaeB`\x82";

    #[test]
    fn text_dragged_in_from_another_program_is_pasted_where_it_lands() {
        let mut editor = editor("One two");
        let (x, y) =
            editor.caret_rect().map(|(x, y, _, h)| (x as i32, y as i32 + h as i32 / 2)).unwrap();
        editor.handle(Event::DataDragOver { x, y });
        assert!(editor.drop_target().is_some(), "no mark where it would land");
        editor.handle(Event::DataDragLeft);
        assert!(editor.drop_target().is_none());
        let contents = Contents { text: Some(" three".to_owned()), ..Contents::default() };
        editor.handle(Event::DataDropped { contents, x, y, copying: false });
        assert_eq!(editor.document.plain_text().trim_end_matches('\n'), "One two three");
        assert!(editor.drop_target().is_none());
    }

    #[test]
    fn a_picture_file_dropped_on_the_page_goes_into_it() {
        let folder = folder("picture");
        let path = folder.join("dot.png");
        std::fs::write(&path, PNG).unwrap();
        let mut editor = editor("Here: ");
        let (x, y) =
            editor.caret_rect().map(|(x, y, _, h)| (x as i32, y as i32 + h as i32 / 2)).unwrap();
        editor.handle(Event::FilesDropped { paths: vec![path], x, y });
        let has_picture = editor.document.body().paragraphs().iter().any(|paragraph| {
            paragraph.runs.iter().any(|run| {
                run.content.iter().any(|c| matches!(c, wp_docx::model::RunContent::Picture(_)))
            })
        });
        assert!(has_picture, "the picture did not go in");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_document_dropped_on_the_window_opens() {
        let folder = folder("document");
        let path = folder.join("other.docx");
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("The other document")));
        std::fs::write(&path, Document::create(&body).unwrap().save().unwrap()).unwrap();
        let mut editor = editor("");
        editor.handle(Event::FilesDropped { paths: vec![path], x: 700, y: 400 });
        assert_eq!(editor.document.plain_text().trim_end_matches('\n'), "The other document");
        assert_eq!(editor.document_name(), "other.docx");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_point_past_the_edge_is_outside() {
        let editor = editor("");
        assert!(editor.is_outside_window(-1, 10));
        assert!(editor.is_outside_window(10, 900));
        assert!(!editor.is_outside_window(10, 10));
    }
}

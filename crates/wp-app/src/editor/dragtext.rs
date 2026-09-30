//! Taking hold of selected text and putting it somewhere else.
//!
//! # What Word does, and why the press cannot decide on its own
//!
//! Pressing inside a selection means one of two things, and which one is not
//! known until the pointer either moves or does not. Moving means the text is
//! being carried somewhere; not moving means the selection is being given up
//! and the caret put where the click was. So the press decides nothing: it
//! remembers where it landed, and the first movement past a few pixels is what
//! turns it into a drag.
//!
//! Holding Ctrl leaves the text where it was and puts a copy down instead,
//! which is what Ctrl does to every drag on every desktop.
//!
//! # Why a bookmark holds the place
//!
//! Moving text is a deletion and an insertion, and the first of them moves
//! everything after it — including where the second was supposed to go. Rather
//! than work out how far the target shifted, a bookmark is put at it: a
//! bookmark is a marker in the document, so deleting text elsewhere leaves it
//! exactly where it was. The text is deleted, the bookmark is asked where it
//! ended up, and the text goes in there. The bookmark is taken out again
//! afterwards, and the whole thing is one gesture, so one undo takes it back.

use wp_docx::model::Block;
use wp_docx::TextPosition;
use wp_shell::Response;

use super::Editor;

/// How far the pointer must move before a press becomes a drag.
const THRESHOLD: i32 = 4;
/// The name of the marker that holds the place while the text is moved.
///
/// Underscored and unlikely: a document of somebody's own is allowed to have a
/// bookmark called anything, and this one is gone before they can see it.
const PLACE: &str = "_wp_dragging";

/// The text being carried, and where it came from.
#[derive(Clone, Debug)]
pub(super) struct TextDrag {
    /// The stretch it was taken from.
    from: (TextPosition, TextPosition),
    /// What it holds, with its formatting.
    blocks: Vec<Block>,
    /// Where it would land if it were let go now.
    pub(super) onto: Option<TextPosition>,
}

impl TextDrag {
    /// The stretch the text was taken from.
    #[must_use]
    pub(super) fn from(&self) -> (TextPosition, TextPosition) {
        self.from
    }

    /// What is being carried.
    #[must_use]
    pub(super) fn blocks(&self) -> &[Block] {
        &self.blocks
    }
}

impl Editor {
    /// Whether a press should wait to see if it becomes a drag.
    ///
    /// Only a plain press inside the selection: with Shift it is a reach, and
    /// with Ctrl on a link it is a jump.
    #[must_use]
    pub(super) fn press_may_drag_text(&self, x: i32, y: i32, shift: bool, control: bool) -> bool {
        if shift || control {
            return false;
        }
        let Some((start, end)) = self.document.selection() else { return false };
        let Some(at) = self.position_at(x, y) else { return false };
        at >= start && at <= end
    }

    /// Remembers where a press that might become a drag landed.
    pub(super) fn wait_for_text_drag(&mut self, x: i32, y: i32) {
        self.pending_text_drag = Some((x, y));
    }

    /// Whether text is being carried.
    #[must_use]
    pub(super) fn dragging_text(&self) -> bool {
        self.text_drag.is_some()
    }

    /// Follows the pointer, starting the drag once it has moved far enough.
    pub(super) fn drag_text(&mut self, x: i32, y: i32) -> Response {
        if let Some((from_x, from_y)) = self.pending_text_drag {
            if (x - from_x).abs() < THRESHOLD && (y - from_y).abs() < THRESHOLD {
                return Response::Ignored;
            }
            let Some(from) = self.document.selection() else {
                self.pending_text_drag = None;
                return Response::Ignored;
            };
            self.pending_text_drag = None;
            self.text_drag =
                Some(TextDrag { from, blocks: self.document.copy_selection(), onto: None });
        }

        let onto = self.position_at(x, y);
        let Some(drag) = &mut self.text_drag else { return Response::Ignored };
        if drag.onto == onto {
            return Response::Ignored;
        }
        drag.onto = onto;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts the text down where the pointer let go.
    pub(super) fn drop_text(&mut self, x: i32, y: i32, copying: bool) -> Response {
        let Some(drag) = self.text_drag.take() else {
            // The press never became a drag, so it was a click after all: the
            // selection is given up and the caret goes where it landed.
            let Some(waited) = self.pending_text_drag.take() else { return Response::Ignored };
            let _ = waited;
            if let Some(at) = self.position_at(x, y) {
                self.document.move_caret(at, false);
            }
            self.needs_redraw = true;
            return Response::Redraw;
        };

        let Some(onto) = self.position_at(x, y) else {
            self.needs_redraw = true;
            return Response::Redraw;
        };
        // Let go inside the text it was taken from: nothing was asked for.
        let (start, end) = drag.from;
        if onto >= start && onto <= end {
            self.needs_redraw = true;
            return Response::Redraw;
        }
        if drag.blocks.is_empty() {
            return Response::Ignored;
        }

        self.put_dragged(&drag, onto, copying);

        self.relayout();
        self.reveal_caret();
        let words: usize =
            drag.blocks.iter().map(|block| block.plain_text().split_whitespace().count()).sum();
        let what = if copying { "Copied" } else { "Moved" };
        self.edited(true, &format!("{what} {words} words"))
    }

    /// Puts what is being carried down: a copy of it with Ctrl held, and
    /// otherwise the text itself, taken from where it was. One gesture, so
    /// one undo takes the whole move back.
    fn put_dragged(&mut self, drag: &TextDrag, onto: TextPosition, copying: bool) {
        self.document.begin_gesture();
        if copying {
            self.document.set_caret(onto);
            self.document.paste_blocks(&drag.blocks);
        } else {
            self.move_text(drag, onto);
        }
        self.document.end_gesture();
    }

    /// Takes the text out and puts it in where the bookmark ended up.
    fn move_text(&mut self, drag: &TextDrag, onto: TextPosition) {
        // The marker goes in first, while everything is still where it was.
        self.document.set_caret(onto);
        self.document.add_bookmark(PLACE);

        let (start, end) = drag.from;
        self.document.set_caret(start);
        self.document.extend_selection_to(end);
        self.document.delete_selection();

        // Wherever the marker has ended up is where the text goes.
        let landed = self
            .document
            .bookmark(PLACE)
            .map_or_else(|| self.document.caret(), |mark| mark.range.0);
        self.document.remove_bookmark(PLACE);
        self.document.set_caret(landed);
        self.document.paste_blocks(&drag.blocks);
    }

    /// Where the text would land, for the mark that shows it.
    #[must_use]
    pub(super) fn text_drop_target(&self) -> Option<TextPosition> {
        self.text_drag.as_ref().and_then(|drag| drag.onto)
    }

    /// Gives up a drag without moving anything.
    pub(super) fn cancel_text_drag(&mut self) {
        self.pending_text_drag = None;
        self.text_drag = None;
    }
}

#[cfg(test)]
mod tests {
    use wp_docx::model::{Block, Body, Paragraph, RunContent};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n\x2d\xb4\0\0\0\0IEND\xaeB`\x82";

    /// "Before after " with a picture between the two words, and the picture
    /// selected, as it is when somebody presses on it to drag it.
    fn editor() -> Editor {
        let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before after ")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library, document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.set_caret(TextPosition::new(0, 7));
        assert!(editor.document.insert_picture(PNG, "png", 914_400, 914_400).expect("inserted"));
        editor.document.set_caret(TextPosition::new(0, 7));
        editor.document.extend_selection_to(TextPosition::new(0, 8));
        editor
    }

    /// What a drag takes hold of once the pointer has moved far enough.
    fn taken(editor: &Editor) -> TextDrag {
        let from = editor.document.selection().expect("a selection");
        TextDrag { from, blocks: editor.document.copy_selection(), onto: None }
    }

    /// The document saved and opened again, and how many pictures in it
    /// reach their parts.
    fn saved(editor: &Editor) -> (Document, usize) {
        let document = Document::open(&editor.document.save().expect("saving")).expect("reopening");
        let body = document.body();
        let pictures = body
            .paragraphs()
            .iter()
            .flat_map(|paragraph| &paragraph.runs)
            .flat_map(|run| &run.content)
            .filter(|piece| match piece {
                RunContent::Picture(picture) => {
                    document.embedded_part(&picture.relationship).is_some()
                }
                _ => false,
            })
            .count();
        (document, pictures)
    }

    #[test]
    fn a_picture_dragged_goes_with_its_part() {
        let mut editor = editor();
        let drag = taken(&editor);
        // Let go at the end of the paragraph.
        editor.put_dragged(&drag, TextPosition::new(0, "Before \u{1}after ".len()), false);

        let (document, pictures) = saved(&editor);
        assert_eq!(document.paragraph_text(0).as_deref(), Some("Before after \u{1}"));
        assert_eq!(pictures, 1, "the dragged picture was lost");
    }

    #[test]
    fn a_picture_dragged_with_ctrl_held_is_there_twice() {
        let mut editor = editor();
        let drag = taken(&editor);
        editor.put_dragged(&drag, TextPosition::new(0, "Before \u{1}after ".len()), true);

        let (document, pictures) = saved(&editor);
        assert_eq!(document.paragraph_text(0).as_deref(), Some("Before \u{1}after \u{1}"));
        assert_eq!(pictures, 2);
    }

    #[test]
    fn a_drag_is_one_thing_to_take_back() {
        let mut editor = editor();
        let drag = taken(&editor);
        editor.put_dragged(&drag, TextPosition::new(0, "Before \u{1}after ".len()), false);
        assert!(editor.document.undo());
        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("Before \u{1}after "));
    }
}

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

        // One gesture, so one undo takes the whole move back.
        self.document.begin_gesture();
        if copying {
            self.document.set_caret(onto);
            self.document.paste_blocks(&drag.blocks);
        } else {
            self.move_text(&drag, onto);
        }
        self.document.end_gesture();

        self.relayout();
        self.reveal_caret();
        let words: usize =
            drag.blocks.iter().map(|block| block.plain_text().split_whitespace().count()).sum();
        let what = if copying { "Copied" } else { "Moved" };
        self.edited(true, &format!("{what} {words} words"))
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

//! Shapes and text boxes, from the Insert tab.
//!
//! # Where a shape goes
//!
//! In the line of text, where the caret is — the same place a picture goes.
//! Word's shapes float above the page by default and the text runs round them;
//! that needs an anchor and a wrapping rule, and neither is here yet. A shape
//! in the line is a shape Word reads and shows correctly, and it is the half of
//! the job that can be done without the other half.

use wp_docx::shapes::Shape;
use wp_layout::geometry::Preset;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// How big a new shape is, in points.
///
/// Word makes one about an inch and a half across when you click rather than
/// drag, and this is the same.
const SHAPE_WIDTH: f64 = 108.0;
const SHAPE_HEIGHT: f64 = 72.0;

/// And a new text box, which is wider than it is tall because text is.
const BOX_WIDTH: f64 = 216.0;
const BOX_HEIGHT: f64 = 72.0;

impl Editor {
    /// Drops open the shapes that can be drawn.
    pub(super) fn open_shapes(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Shape) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::InsertShape) else {
            return Response::Ignored;
        };

        let items = Preset::ALL.iter().map(|preset| preset.label().to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Shape, items, None, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts the shape that was chosen at the caret.
    pub(super) fn choose_shape(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(preset) = Preset::ALL.get(index).copied() else { return Response::Ignored };

        let mut shape = Shape::preset(preset.word(), SHAPE_WIDTH, SHAPE_HEIGHT);
        shape.name = preset.label().to_owned();
        // A line has no inside, so it has no fill and is drawn thicker — a
        // hairline is a line nobody can see.
        if !preset.is_closed() {
            shape.fill = None;
            shape.outline = Some("2F528F".to_owned());
            shape.outline_emu = wp_docx::shapes::EMU_PER_POINT * 2;
        }

        let changed = self.document.insert_shape(&shape);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("{} added", preset.label()))
    }

    /// Opens the strip that takes the words for a text box.
    pub(super) fn start_text_box(&mut self) -> Response {
        let mut bar = FindBar::for_purpose(Purpose::TextBox);
        let selected = self.document.selected_text();
        if !selected.is_empty() && !selected.contains('\n') {
            bar.needle = selected;
        }
        self.find_bar = Some(bar);
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type what the box should say, then press Enter")
    }

    /// Puts the text box in.
    pub(super) fn finish_text_box(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let text = bar.needle.trim().to_owned();
        self.find_bar = None;
        self.needs_redraw = true;

        let shape = Shape::text_box(BOX_WIDTH, BOX_HEIGHT, &text);
        let changed = self.document.insert_shape(&shape);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, "Text box added")
    }
}

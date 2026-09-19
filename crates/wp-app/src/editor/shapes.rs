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

        let items = Preset::all().iter().map(|preset| preset.label().to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Shape, items, None, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts the shape that was chosen at the caret.
    pub(super) fn choose_shape(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(preset) = Preset::all().get(index).copied() else { return Response::Ignored };

        let mut shape = Shape::preset(preset.word(), SHAPE_WIDTH, SHAPE_HEIGHT);
        shape.name = preset.label().to_owned();
        // A line has no inside, so it has no fill, and Word draws one in the
        // first accent at the theme's first line style — half a point.
        if !preset.is_closed() {
            use wp_docx::colour::Colour;
            use wp_docx::theme::Slot;
            shape.fill = wp_docx::fills::Fill::None;
            shape.outline = Some(Colour::scheme(Slot::Accent1));
            shape.outline_emu = 0;
            shape.line_style = 1;
            shape.ink = Some(Colour::scheme(Slot::Dark1));
        }

        let changed = self.document.insert_shape(&shape);
        self.choose_drawing_here();
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
        self.choose_drawing_here();
        self.relayout();
        self.reveal_caret();
        self.edited(changed, "Text box added")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::colour::{Base, Colour};
    use wp_docx::fills::Fill;
    use wp_docx::gallery::COLOR_SCHEMES;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::theme::Slot;
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Some words")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// The colour the first shape on the first page is filled with.
    fn painted(editor: &Editor) -> wp_raster::Color {
        editor.pages[0].shapes[0].fill.colour().expect("a colour")
    }

    #[test]
    fn a_shape_from_the_gallery_names_the_theme_and_changes_with_it() {
        let mut editor = editor();
        let rectangle = Preset::all().iter().position(|preset| preset.word() == "rect").unwrap();
        editor.choose_shape(rectangle);

        let shape = editor.document.shapes().into_iter().next().expect("the shape");
        assert_eq!(shape.fill, Fill::Styled { index: 1, colour: Colour::scheme(Slot::Accent1) });
        assert_eq!(shape.outline.map(|colour| colour.base), Some(Base::Scheme(Slot::Accent1)));
        assert_eq!(shape.ink, Some(Colour::scheme(Slot::Light1)));
        assert_eq!(painted(&editor), wp_raster::Color::rgb(0x44, 0x72, 0xC4), "the Office blue");

        // Design ▸ Colors ▸ Red: the shape is red now, and the line round it
        // is a darker red, because neither of them was ever a colour.
        let red = COLOR_SCHEMES.iter().position(|scheme| scheme.name == "Red").unwrap();
        editor.choose_theme_colors(red);
        assert_eq!(painted(&editor), wp_raster::Color::rgb(0xC0, 0, 0));
        assert_eq!(editor.pages[0].shapes[0].outline, Some(wp_raster::Color::rgb(0x60, 0, 0)));

        // And the file says what the shape said: the style, not a colour.
        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        let shape = reopened.shapes().into_iter().next().expect("the shape");
        assert_eq!(shape.fill, Fill::Styled { index: 1, colour: Colour::scheme(Slot::Accent1) });
        assert_eq!(shape.line_style, 2);
    }

    #[test]
    fn a_line_from_the_gallery_is_the_theme_s_first_line_style_in_the_first_accent() {
        let mut editor = editor();
        let line = Preset::all().iter().position(|preset| preset.word() == "line").unwrap();
        editor.choose_shape(line);
        let shape = editor.document.shapes().into_iter().next().expect("the shape");
        assert_eq!(shape.fill, Fill::None);
        assert_eq!(shape.outline, Some(Colour::scheme(Slot::Accent1)));
        assert_eq!(shape.line_style, 1);
        assert_eq!(
            editor.pages[0].shapes[0].outline,
            Some(wp_raster::Color::rgb(0x44, 0x72, 0xC4))
        );
    }
}

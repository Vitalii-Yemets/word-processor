//! Where a drawing sits and what the text does about it, from the Layout tab.
//!
//! # What these commands act on
//!
//! The drawing nearest the caret. Word acts on the one that is selected, and
//! selecting a drawing means clicking it — which needs handles, a selection
//! that is not a stretch of text, and a way to drag. None of that is here yet,
//! so the caret stands in for it: put the caret beside a drawing and these
//! commands are about that drawing.

use wp_docx::anchor::{Anchor, Placement, Wrap};
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// Where a drawing can be put across the page.
const POSITIONS: &[(&str, &str)] = &[("Left", "left"), ("Centre", "center"), ("Right", "right")];

impl Editor {
    /// The anchor a drawing has, or the one it gets when it begins to float.
    ///
    /// A drawing that starts floating goes on top of the ones already there,
    /// which is what Word does and what anybody who has just made a drawing
    /// float expects: they want to see it.
    fn anchor_of(&self) -> Anchor {
        match self.document.anchor_here() {
            Some(anchor) => anchor,
            None => Anchor { depth: self.document.next_drawing_depth(), ..Anchor::default() },
        }
    }

    /// Drops open what the text can do about a drawing.
    pub(super) fn open_wrapping(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Wrap) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        if !self.document.drawing_here() {
            return self.report("Put the caret beside a shape or a picture first");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::WrapText) else {
            return Response::Ignored;
        };

        let here = self.document.anchor_here();
        let mut items = vec!["In Line with Text".to_owned()];
        items.extend(Wrap::ALL.iter().map(|wrap| wrap.label().to_owned()));

        let current = match here {
            None => Some(0),
            Some(anchor) => Wrap::ALL.iter().position(|wrap| *wrap == anchor.wrap).map(|at| at + 1),
        };
        self.popup = Some(Popup::new(Choice::Wrap, items, current, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Sets what the text does about the drawing at the caret.
    pub(super) fn choose_wrapping(&mut self, index: usize) -> Response {
        self.popup = None;

        // The first line puts the drawing back in the line of text; the rest
        // are the ways of floating.
        let (anchor, note) = match index.checked_sub(1) {
            None => (None, "In line with text".to_owned()),
            Some(at) => {
                let Some(wrap) = Wrap::ALL.get(at).copied() else { return Response::Ignored };
                let anchor = Anchor {
                    wrap,
                    // Behind and in front are the same wrapping — none — and
                    // differ only in which is drawn over which.
                    behind_text: wrap == Wrap::None,
                    ..self.anchor_of()
                };
                (Some(anchor), format!("Wrap: {}", wrap.label()))
            }
        };

        let changed = self.document.set_anchor_here(anchor.as_ref());
        self.relayout();
        self.edited(changed, &note)
    }

    /// Drops open where a drawing can sit across the page.
    pub(super) fn open_position(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Position) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        if !self.document.drawing_here() {
            return self.report("Put the caret beside a shape or a picture first");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Position) else {
            return Response::Ignored;
        };

        let items = POSITIONS.iter().map(|(label, _)| (*label).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Position, items, None, left, top, 200.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Moves the drawing at the caret across the page.
    pub(super) fn choose_position(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((label, edge)) = POSITIONS.get(index).copied() else { return Response::Ignored };

        // A drawing has to float before it can be put anywhere, so one that was
        // in the line starts floating with the wrapping Word gives it.
        let mut anchor = self.anchor_of();
        anchor.horizontal = Placement::Aligned(edge.to_owned());

        let changed = self.document.set_anchor_here(Some(&anchor));
        self.relayout();
        self.edited(changed, &format!("Position: {label}"))
    }

    /// Puts the drawing at the caret in front of the text, or behind it.
    ///
    /// Word's last entry on each of the two Arrange menus, and the one that
    /// takes a drawing out of the pile altogether: the text no longer keeps out
    /// of its way, and it is drawn over the words or under them.
    pub(super) fn set_shape_depth(&mut self, behind: bool) -> Response {
        if !self.document.drawing_here() {
            return self.report("Put the caret beside a shape or a picture first");
        }
        let mut anchor = self.anchor_of();
        anchor.wrap = Wrap::None;
        anchor.behind_text = behind;

        let changed = self.document.set_anchor_here(Some(&anchor));
        self.relayout();
        self.edited(changed, if behind { "Behind text" } else { "In front of text" })
    }

    /// Drops open one of Word's two Arrange menus.
    pub(super) fn open_arrange(&mut self, forwards: bool) -> Response {
        let (choice, command) = if forwards {
            (Choice::Forward, Command::BringForward)
        } else {
            (Choice::Backward, Command::SendBackward)
        };
        if self.close_popup_if(choice) {
            return Response::Redraw;
        }
        if !self.document.drawing_here() {
            return self.report("Put the caret beside a shape or a picture first");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(command) else {
            return Response::Ignored;
        };

        let items: Vec<String> =
            arrange_entries(forwards).iter().map(|(label, _)| (*label).to_owned()).collect();
        self.popup = Some(Popup::new(choice, items, None, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Runs whichever of a menu's three was chosen.
    pub(super) fn choose_arrange(&mut self, forwards: bool, index: usize) -> Response {
        self.popup = None;
        let Some((_, command)) = arrange_entries(forwards).get(index).copied() else {
            return Response::Ignored;
        };
        self.run(command)
    }

    /// Moves the drawing at the caret through the pile of drawings.
    ///
    /// The pile is every floating drawing in the document, ordered by the
    /// number its anchor carries. One step puts this drawing just past its
    /// neighbour; `all_the_way` puts it past every one of them.
    ///
    /// Nothing at all if it is already at that end, and nothing if it does not
    /// float: a drawing in the line of text is part of the text, and there is
    /// nothing for it to be in front of.
    pub(super) fn move_shape_depth(&mut self, forwards: bool, all_the_way: bool) -> Response {
        if !self.document.drawing_here() {
            return self.report("Put the caret beside a shape or a picture first");
        }
        let Some(mut anchor) = self.document.anchor_here() else {
            return self.report("A drawing in the line of text is not in front of anything");
        };

        // Every floating drawing's depth, pictures and shapes alike. The one at
        // the caret is among them, and comparing against itself is harmless:
        // nothing is strictly above or below its own number.
        let others = self.document.drawing_depths();

        let wanted = if forwards {
            let above = others.iter().copied().filter(|depth| *depth > anchor.depth);
            if all_the_way { above.max() } else { above.min() }.map(|depth| depth.saturating_add(1))
        } else {
            let below = others.iter().copied().filter(|depth| *depth < anchor.depth);
            if all_the_way { below.min() } else { below.max() }.map(|depth| depth.saturating_sub(1))
        };
        let Some(wanted) = wanted else {
            return self.report(if forwards {
                "This drawing is already in front of the others"
            } else {
                "This drawing is already behind the others"
            });
        };

        anchor.depth = wanted;
        let changed = self.document.set_anchor_here(Some(&anchor));
        self.relayout();
        self.edited(
            changed,
            match (forwards, all_the_way) {
                (true, false) => "Bring Forward",
                (true, true) => "Bring to Front",
                (false, false) => "Send Backward",
                (false, true) => "Send to Back",
            },
        )
    }
}

/// What one of the two Arrange menus offers, in Word's order.
#[must_use]
fn arrange_entries(forwards: bool) -> &'static [(&'static str, Command)] {
    if forwards {
        &[
            ("Bring Forward", Command::BringForward),
            ("Bring to Front", Command::BringToFront),
            ("Bring in Front of Text", Command::BringInFrontOfText),
        ]
    } else {
        &[
            ("Send Backward", Command::SendBackward),
            ("Send to Back", Command::SendToBack),
            ("Send Behind Text", Command::SendBehindText),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::anchor::USUAL_DEPTH;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// A document holding two floating shapes, one after the other.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Text")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        for name in ["First", "Second"] {
            let shape = wp_docx::shapes::Shape {
                name: name.to_owned(),
                width_emu: 914_400,
                height_emu: 914_400,
                fill: Some("4472C4".to_owned()),
                anchor: Some(wp_docx::anchor::Anchor::default()),
                ..wp_docx::shapes::Shape::default()
            };
            assert!(editor.document.insert_shape(&shape), "the shape went nowhere");
        }
        editor.relayout();
        editor
    }

    /// The depths of every floating shape, in the order the document holds
    /// them.
    fn depths(editor: &Editor) -> Vec<u32> {
        editor
            .document
            .shapes()
            .iter()
            .filter_map(|shape| shape.anchor.as_ref())
            .map(|anchor| anchor.depth)
            .collect()
    }

    /// Puts the caret beside the first of the two shapes.
    fn on_first(editor: &mut Editor) {
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        assert_eq!(
            editor.document.shape_here().map(|shape| shape.name),
            Some("First".to_owned()),
            "the caret is not beside the first shape"
        );
    }

    #[test]
    fn each_new_drawing_goes_on_top_of_the_ones_already_there() {
        // Word counts up from its own number, and so does this. Two drawings
        // at the same depth would be two nothing could tell apart.
        let editor = editor();
        assert_eq!(depths(&editor), vec![USUAL_DEPTH, USUAL_DEPTH + 1]);
    }

    #[test]
    fn bringing_one_forward_puts_it_past_the_other() {
        let mut editor = editor();
        on_first(&mut editor);
        editor.move_shape_depth(true, false);

        let depths = depths(&editor);
        assert!(depths[0] > depths[1], "got {depths:?}");
    }

    #[test]
    fn sending_one_back_puts_it_behind_the_other() {
        let mut editor = editor();
        on_first(&mut editor);
        editor.move_shape_depth(true, false);
        editor.move_shape_depth(false, false);

        let depths = depths(&editor);
        assert!(depths[0] < depths[1], "got {depths:?}");
    }

    #[test]
    fn one_already_at_the_back_does_not_move() {
        let mut editor = editor();
        on_first(&mut editor);
        editor.move_shape_depth(false, false);
        let before = depths(&editor);

        // Everything is at the same depth, so there is nothing below it.
        editor.move_shape_depth(false, false);
        assert_eq!(depths(&editor), before);
    }

    #[test]
    fn the_order_survives_being_saved_and_opened_again() {
        let mut editor = editor();
        on_first(&mut editor);
        editor.move_shape_depth(true, true);
        let wanted = depths(&editor);

        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        let after: Vec<u32> = reopened
            .shapes()
            .iter()
            .filter_map(|shape| shape.anchor.as_ref())
            .map(|anchor| anchor.depth)
            .collect();
        assert_eq!(after, wanted);
    }

    #[test]
    fn the_page_draws_them_in_that_order() {
        let mut editor = editor();
        on_first(&mut editor);
        editor.move_shape_depth(true, true);
        editor.relayout();

        let page = editor.pages.first().expect("a page");
        let order: Vec<u32> = page
            .drawings_over()
            .iter()
            .filter_map(|drawing| match drawing {
                wp_layout::Drawing::Shape(shape) => Some(shape.depth),
                wp_layout::Drawing::Picture(_) => None,
            })
            .collect();
        assert!(order.windows(2).all(|pair| pair[0] <= pair[1]), "got {order:?}");
        assert_eq!(order.len(), 2, "both shapes should be in front of the text");
    }

    #[test]
    fn a_drawing_in_the_line_of_text_is_not_in_front_of_anything() {
        let mut editor = editor();
        on_first(&mut editor);
        // Put it back in the line, and the pile no longer has anything to say
        // about it.
        let mut shape = editor.document.shape_here().expect("a shape");
        shape.anchor = None;
        editor.document.replace_shape_here(&shape);

        editor.move_shape_depth(true, false);
        assert!(editor.document.shape_here().expect("a shape").anchor.is_none());
    }

    #[test]
    fn a_picture_answers_the_arrange_commands_too() {
        // The whole of C26: Wrap Text, Position and the pile are about a
        // drawing, and a picture is a drawing.
        let mut editor = editor();
        let canvas = wp_raster::Canvas::filled(20, 20, wp_raster::Color::BLACK);
        let bytes = wp_raster::encode_png(&canvas);
        // At the end of the paragraph, past the two shapes: the caret stands in
        // for choosing a drawing, so a picture put between them would be a
        // picture the caret cannot be beside without also being beside a shape.
        let end = editor.document.paragraph_text(0).map_or(0, |text| text.len());
        editor.document.set_caret(wp_docx::TextPosition::new(0, end));
        editor.document.insert_picture(&bytes, "png", 914_400, 914_400).expect("a picture");
        editor.document.set_caret(wp_docx::TextPosition::new(0, end + 1));
        assert!(editor.document.drawing_here(), "the caret is not beside the picture");
        assert!(editor.document.shape_here().is_none(), "the caret is beside a shape as well");

        // Wrapped square, which puts it off the line.
        editor.choose_wrapping(1);
        let anchor = editor.document.anchor_here().expect("an anchor");
        assert_eq!(anchor.wrap, wp_docx::anchor::Wrap::Square);

        // And in front of the two shapes, which are in the same pile.
        editor.move_shape_depth(true, true);
        let depths = depths(&editor);
        let picture = editor.document.anchor_here().expect("an anchor").depth;
        assert!(depths.iter().all(|shape| *shape < picture), "got {depths:?} against {picture}");

        // Back in the line, which is where it started.
        editor.choose_wrapping(0);
        assert!(editor.document.anchor_here().is_none());
    }

    #[test]
    fn each_menu_offers_words_three() {
        assert_eq!(arrange_entries(true).len(), 3);
        assert_eq!(arrange_entries(false).len(), 3);
    }
}

//! Choosing a drawing, moving it, and resizing it.
//!
//! # The second kind of selection
//!
//! A selection of text is a stretch of the document, or several of them since
//! **C22**. A drawing is not a stretch of anything: it is one thing, chosen or
//! not. So it is kept beside the other: [`Editor::chosen_drawing`] is the place
//! one drawing is at, and every command that acts on a drawing asks it.
//!
//! The two cannot both be what a command is about — Bold with a picture chosen
//! would have nothing to embolden — so choosing a drawing puts the caret beside
//! it and a press in the text gives it up. That is Word's arrangement as well.
//!
//! # Why a place rather than "the drawing at the caret"
//!
//! Because a caret between two drawings is beside both, and something has to
//! settle which is meant. Before this the caret was the whole answer and a
//! shape next to a picture always won; now the drawing chosen is named by the
//! place it is at, which is one drawing however many are around it. See
//! [`wp_docx::Document::drawing_place_here`].
//!
//! # Why a drawing starts floating when it is dragged
//!
//! Because a drawing in the line of text has no position of its own to change.
//! It sits where the words put it. Dragging one is asking for it to be
//! somewhere in particular, which is what floating means — so the first drag
//! anchors it, exactly as Word's does. Resizing one does not: a picture in the
//! line stays in the line however big it is made.

use wp_docx::anchor::{Anchor, Placement, Wrap};
use wp_docx::shapes::EMU_PER_POINT;
use wp_docx::TextPosition;
use wp_layout::Drawing;
use wp_shell::{Cursor, Response};

use super::Editor;

/// How big the square handles are, in pixels.
pub(super) const HANDLE: f32 = 7.0;

/// The least a drawing can be dragged down to, in points.
///
/// A drawing pulled down to nothing would be a drawing nobody could take hold
/// of again.
const LEAST: f32 = 8.0;

/// Which part of a selected drawing was taken hold of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Grip {
    /// The middle: the drawing moves.
    Body,
    /// A corner or an edge: the drawing changes size.
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl Grip {
    /// The eight that change the size, in the order they are drawn.
    const EDGES: &'static [Self] = &[
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Right,
        Self::BottomRight,
        Self::Bottom,
        Self::BottomLeft,
        Self::Left,
    ];

    /// Where on the drawing's box the handle sits, as a fraction of it.
    fn at(self) -> (f32, f32) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::Top => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::Right => (1.0, 0.5),
            Self::BottomRight => (1.0, 1.0),
            Self::Bottom => (0.5, 1.0),
            Self::BottomLeft => (0.0, 1.0),
            Self::Left => (0.0, 0.5),
            Self::Body => (0.5, 0.5),
        }
    }

    /// How far dragging it moves each edge: left, top, right, bottom.
    fn moves(self) -> (f32, f32, f32, f32) {
        match self {
            Self::TopLeft => (1.0, 1.0, 0.0, 0.0),
            Self::Top => (0.0, 1.0, 0.0, 0.0),
            Self::TopRight => (0.0, 1.0, 1.0, 0.0),
            Self::Right => (0.0, 0.0, 1.0, 0.0),
            Self::BottomRight => (0.0, 0.0, 1.0, 1.0),
            Self::Bottom => (0.0, 0.0, 0.0, 1.0),
            Self::BottomLeft => (1.0, 0.0, 0.0, 1.0),
            Self::Left => (1.0, 0.0, 0.0, 0.0),
            Self::Body => (1.0, 1.0, 1.0, 1.0),
        }
    }

    /// Which pointer belongs over it.
    fn cursor(self) -> Cursor {
        match self {
            Self::Top | Self::Bottom => Cursor::ResizeVertical,
            Self::Left | Self::Right => Cursor::ResizeHorizontal,
            // The corners want a diagonal pointer, which the shell does not
            // offer yet; the horizontal one at least says the edge can be
            // dragged.
            _ => Cursor::ResizeHorizontal,
        }
    }
}

/// A drag of a drawing that is under way.
#[derive(Clone, Debug)]
pub(super) struct ShapeDrag {
    pub grip: Grip,
    /// Where the pointer was when it began.
    pub from_x: f32,
    pub from_y: f32,
    /// Which drawing is being dragged.
    pub at: TextPosition,
    /// What it was when the drag began, so every move is measured from the
    /// start rather than from the last move — which would drift.
    pub width_emu: i64,
    pub height_emu: i64,
    pub anchor: Option<Anchor>,
    /// Where in the pile it goes if this drag is what makes it float.
    pub depth: u32,
}

/// A drawing on a page: where it was drawn, and where it is in the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct OnPage {
    pub at: TextPosition,
    /// On the screen, not on the page: the scroll and the page's corner are
    /// already taken into account.
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl Editor {
    /// Chooses one, and puts the caret beside it.
    ///
    /// The caret follows so that everything which asks the caret — the ribbon
    /// showing what is in force, Backspace, the scroll that keeps the caret in
    /// view — still gets a sensible answer. Which drawing the Arrange commands
    /// act on is the selection's doing, not the caret's.
    pub(super) fn choose_drawing_at(&mut self, at: TextPosition) {
        self.chosen_drawing = Some(at);
        self.document.set_caret(TextPosition::new(at.paragraph, at.offset + 1));
        self.needs_redraw = true;
    }

    /// Chooses the drawing the caret is beside, which is the one just put in.
    ///
    /// A drawing inserted comes up chosen, with its handles round it, because
    /// that is what Word does and because the thing anybody does next with a
    /// shape they have just made is move it.
    pub(super) fn choose_drawing_here(&mut self) {
        if let Some(at) = self.document.drawing_place_here() {
            self.chosen_drawing = Some(at);
            self.needs_redraw = true;
        }
    }

    /// Gives up the drawing that was chosen, if one was.
    ///
    /// Returns whether anything was given up, so a press that let go of a
    /// drawing can be told from one that did nothing.
    pub(super) fn drop_chosen_drawing(&mut self) -> bool {
        if self.chosen_drawing.take().is_none() {
            return false;
        }
        self.needs_redraw = true;
        true
    }

    /// Which drawing the commands are about: the one chosen, else the one the
    /// caret is beside.
    ///
    /// The caret is still an answer because the keyboard has to be able to
    /// reach a drawing: a picture typed in and then wrapped never went near the
    /// mouse.
    #[must_use]
    pub(super) fn drawing_in_hand(&self) -> Option<TextPosition> {
        self.chosen_drawing.or_else(|| self.document.drawing_place_here())
    }

    /// Every drawing on the pages, the one nearest the reader first.
    ///
    /// That is the order a press asks them in: a drawing laid over another is
    /// the one that was pressed.
    #[must_use]
    pub(super) fn drawings_facing(&self) -> Vec<OnPage> {
        let mut out = Vec::new();
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            let page = &self.pages[index];

            // In front of the text first, then behind it; within each, the one
            // drawn last is the one on top.
            let facing = page.drawings_over().into_iter().rev();
            for drawing in facing.chain(page.drawings_under().into_iter().rev()) {
                let (at, x, y, width, height) = match drawing {
                    Drawing::Shape(shape) => {
                        (shape.at, shape.x, shape.y, shape.width, shape.height)
                    }
                    Drawing::Picture(picture) => {
                        (picture.at, picture.x, picture.y, picture.width, picture.height)
                    }
                };
                let Some(at) = at else { continue };
                out.push(OnPage { at, left: origin_x + x, top: top + y, width, height });
            }
        }
        out
    }

    /// Where on the screen the chosen drawing is, if it is on a page.
    pub(super) fn chosen_drawing_box(&self) -> Option<OnPage> {
        let at = self.chosen_drawing?;
        self.drawings_facing().into_iter().find(|drawing| drawing.at == at)
    }

    /// The drawing at a point on the screen, if there is one.
    #[must_use]
    pub(super) fn drawing_under(&self, x: i32, y: i32) -> Option<TextPosition> {
        let (px, py) = (x as f32, y as f32);
        self.drawings_facing()
            .into_iter()
            .find(|drawing| {
                px >= drawing.left
                    && px < drawing.left + drawing.width
                    && py >= drawing.top
                    && py < drawing.top + drawing.height
            })
            .map(|drawing| drawing.at)
    }

    /// Which handle of the chosen drawing a point is on, if any.
    pub(super) fn grip_at(&self, x: i32, y: i32) -> Option<Grip> {
        let drawing = self.chosen_drawing_box()?;
        let (px, py) = (x as f32, y as f32);

        for grip in Grip::EDGES {
            let (fx, fy) = grip.at();
            let cx = drawing.left + drawing.width * fx;
            let cy = drawing.top + drawing.height * fy;
            if (px - cx).abs() <= HANDLE && (py - cy).abs() <= HANDLE {
                return Some(*grip);
            }
        }
        if px >= drawing.left
            && px < drawing.left + drawing.width
            && py >= drawing.top
            && py < drawing.top + drawing.height
        {
            return Some(Grip::Body);
        }
        None
    }

    /// Which pointer belongs over a point, when a drawing is chosen.
    pub(super) fn shape_cursor(&self, x: i32, y: i32) -> Option<Cursor> {
        match self.grip_at(x, y)? {
            Grip::Body => Some(Cursor::Arrow),
            grip => Some(grip.cursor()),
        }
    }

    /// Takes hold of a drawing, or of one of its handles.
    ///
    /// Returns whether it took hold of anything. A press that lands on no
    /// drawing gives up the one that was chosen and answers no, so that it goes
    /// on to mean whatever it would have meant in the text.
    pub(super) fn press_on_shape(&mut self, x: i32, y: i32) -> bool {
        // A handle of the drawing already chosen comes first: it lies on the
        // drawing's edge, and outside it at the corners.
        if let Some(grip) = self.grip_at(x, y) {
            if let Some(at) = self.chosen_drawing {
                return self.take_hold_of(at, grip, x, y);
            }
        }

        let Some(at) = self.drawing_under(x, y) else {
            // Word gives the drawing up when the next press lands somewhere
            // else. While Select Objects is in hand the press stops there: it
            // is about drawings and nothing else, so one that finds none of
            // them puts the caret nowhere.
            self.drop_chosen_drawing();
            return self.choosing_drawings;
        };

        self.choose_drawing_at(at);
        self.take_hold_of(at, Grip::Body, x, y)
    }

    /// Remembers what the drawing was, so the drag can be measured from it.
    fn take_hold_of(&mut self, at: TextPosition, grip: Grip, x: i32, y: i32) -> bool {
        let Some((width_emu, height_emu)) = self.document.drawing_size_at(at) else {
            return false;
        };
        // One drag is one thing to undo, however many moves it is made of.
        self.document.begin_gesture();
        self.shape_drag = Some(ShapeDrag {
            grip,
            from_x: x as f32,
            from_y: y as f32,
            at,
            width_emu,
            height_emu,
            anchor: self.document.anchor_at(at),
            depth: self.document.next_drawing_depth(),
        });
        self.needs_redraw = true;
        true
    }

    /// Carries on a drag of a drawing.
    pub(super) fn drag_shape(&mut self, x: i32, y: i32) -> Response {
        let Some(drag) = self.shape_drag.clone() else { return Response::Ignored };
        let scale = self.pixels_per_inch() / 72.0;
        if scale <= 0.0 {
            return Response::Ignored;
        }

        // How far the pointer has come, in points rather than pixels: the
        // document is measured in points and the zoom must not change how far
        // a drag moves a drawing.
        let dx = (x as f32 - drag.from_x) / scale;
        let dy = (y as f32 - drag.from_y) / scale;
        let (left, top, right, bottom) = drag.grip.moves();

        let mut changed = false;
        if drag.grip != Grip::Body {
            let width = emu_to_points(drag.width_emu) + (right - left) * dx;
            let height = emu_to_points(drag.height_emu) + (bottom - top) * dy;
            changed |= self.document.set_drawing_size_at(
                drag.at,
                points_to_emu(width.max(LEAST)),
                points_to_emu(height.max(LEAST)),
            );
        }

        // The body moves the drawing. So does a left or top handle, because the
        // opposite edge is the one staying put — but only for a drawing that
        // already floats: one in the line has nowhere to be moved to, and
        // making it float because it was made wider would be a surprise.
        let floats = drag.anchor.is_some();
        let (across, down) = match drag.grip {
            Grip::Body => (dx, dy),
            _ if floats => (dx * left, dy * top),
            _ => (0.0, 0.0),
        };
        if drag.grip == Grip::Body || across != 0.0 || down != 0.0 {
            let anchor = moved(drag.anchor.clone(), across, down, drag.depth);
            changed |= self.document.set_anchor_at(drag.at, Some(&anchor));
        }

        if !changed {
            return Response::Ignored;
        }
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Lets go at the end of a drag.
    pub(super) fn release_shape(&mut self) {
        if self.shape_drag.take().is_some() {
            self.document.end_gesture();
            self.update_title();
        }
    }

    /// Whether a drawing is being dragged.
    #[must_use]
    pub(super) fn dragging_shape(&self) -> bool {
        self.shape_drag.is_some()
    }

    /// Turns Word's Select Objects on, or off again.
    ///
    /// A mode rather than a command, the way the format painter is: while it is
    /// on, a press chooses a drawing and never puts the caret in the text.
    /// Escape puts it down, as Escape puts down every other mode here.
    pub(super) fn toggle_choosing_drawings(&mut self) -> Response {
        self.choosing_drawings = !self.choosing_drawings;
        if !self.choosing_drawings {
            self.drop_chosen_drawing();
        }
        self.needs_redraw = true;
        let note = if self.choosing_drawings {
            "Select Objects: click a shape or a picture. Escape to stop"
        } else {
            "Select Objects off"
        };
        self.report(note)
    }

    /// Whether presses are choosing drawings rather than text.
    #[must_use]
    pub(super) fn choosing_drawings(&self) -> bool {
        self.choosing_drawings
    }
}

/// The same anchor, moved by a distance in points.
///
/// A drawing that was in the line of text starts floating, because a drawing in
/// the line has no position of its own to move. It goes on top of the pile
/// while it is about it: that is what Word does, and what anybody who has just
/// dragged a drawing out of the text expects to see.
fn moved(anchor: Option<Anchor>, dx: f32, dy: f32, depth: u32) -> Anchor {
    let mut anchor = anchor.unwrap_or(Anchor { wrap: Wrap::Square, depth, ..Anchor::default() });
    let across = match anchor.horizontal {
        Placement::Offset(distance) => distance,
        // A drawing lined up with an edge and then dragged is no longer lined
        // up with it, so the alignment gives way to a distance.
        Placement::Aligned(_) => 0,
    };
    let down = match anchor.vertical {
        Placement::Offset(distance) => distance,
        Placement::Aligned(_) => 0,
    };
    anchor.horizontal = Placement::Offset(across + points_to_emu(dx));
    anchor.vertical = Placement::Offset(down + points_to_emu(dy));
    anchor
}

/// A measurement in points, as the format writes it.
fn points_to_emu(points: f32) -> i64 {
    (f64::from(points) * EMU_PER_POINT as f64) as i64
}

/// And back again.
fn emu_to_points(emu: i64) -> f32 {
    (emu as f64 / EMU_PER_POINT as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor showing a document with one floating shape in it.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Some words to flow round it")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        let shape = wp_docx::shapes::Shape {
            name: "Box".to_owned(),
            width_emu: 914_400,
            height_emu: 914_400,
            fill: Some("4472C4".to_owned()),
            anchor: Some(Anchor { wrap: Wrap::Square, ..Anchor::default() }),
            ..wp_docx::shapes::Shape::default()
        };
        editor.document.set_caret(TextPosition::new(0, 0));
        assert!(editor.document.insert_shape(&shape), "the shape went nowhere");
        editor.relayout();
        editor
    }

    /// The middle of the one drawing, on the screen.
    fn middle(editor: &Editor) -> (i32, i32) {
        let drawing = editor.drawings_facing().first().copied().expect("a drawing");
        ((drawing.left + drawing.width / 2.0) as i32, (drawing.top + drawing.height / 2.0) as i32)
    }

    /// One of its handles, on the screen.
    fn handle(editor: &Editor, grip: Grip) -> (i32, i32) {
        let drawing = editor.chosen_drawing_box().expect("a chosen drawing");
        let (fx, fy) = grip.at();
        ((drawing.left + drawing.width * fx) as i32, (drawing.top + drawing.height * fy) as i32)
    }

    #[test]
    fn a_press_inside_a_drawing_chooses_it() {
        let mut editor = editor();
        assert!(editor.chosen_drawing.is_none());
        let (x, y) = middle(&editor);
        assert!(editor.press_on_shape(x, y), "the press found no drawing");
        assert!(editor.chosen_drawing.is_some());
        editor.release_shape();
    }

    #[test]
    fn a_press_away_from_every_drawing_gives_it_up() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y);
        editor.release_shape();

        assert!(!editor.press_on_shape(x + 500, y + 250), "the press took hold of something");
        assert!(editor.chosen_drawing.is_none());
    }

    #[test]
    fn a_drag_moves_the_drawing() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y);
        editor.drag_shape(x + 60, y + 40);
        editor.release_shape();

        let at = editor.chosen_drawing.expect("a chosen drawing");
        let anchor = editor.document.anchor_at(at).expect("an anchor");
        let Placement::Offset(across) = anchor.horizontal else { panic!("not an offset") };
        let Placement::Offset(down) = anchor.vertical else { panic!("not an offset") };
        assert!(across > 0, "it did not move across");
        assert!(down > 0, "it did not move down");
    }

    #[test]
    fn a_drag_on_a_handle_resizes_it() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y);
        editor.release_shape();
        let at = editor.chosen_drawing.expect("a chosen drawing");
        let (before, _) = editor.document.drawing_size_at(at).expect("a size");

        let (hx, hy) = handle(&editor, Grip::Right);
        assert!(editor.press_on_shape(hx, hy), "the handle was not found");
        editor.drag_shape(hx + 40, hy);
        editor.release_shape();

        let (after, _) = editor.document.drawing_size_at(at).expect("a size");
        assert!(after > before, "it did not grow: {before} then {after}");
    }

    #[test]
    fn a_drawing_cannot_be_dragged_away_to_nothing() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y);
        editor.release_shape();
        let at = editor.chosen_drawing.expect("a chosen drawing");

        let (hx, hy) = handle(&editor, Grip::Right);
        editor.press_on_shape(hx, hy);
        editor.drag_shape(hx - 5000, hy);
        editor.release_shape();

        let (width, _) = editor.document.drawing_size_at(at).expect("a size");
        assert!(width >= points_to_emu(LEAST), "it was dragged out of existence");
    }

    #[test]
    fn one_undo_takes_back_one_drag() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y);
        for step in 1..=4 {
            editor.drag_shape(x + step * 15, y);
        }
        editor.release_shape();

        let at = editor.chosen_drawing.expect("a chosen drawing");
        editor.document.undo();
        assert_eq!(
            editor.document.anchor_at(at).expect("an anchor").horizontal,
            Placement::Offset(0)
        );
    }

    #[test]
    fn a_picture_is_chosen_and_dragged_like_a_shape() {
        // The whole point of naming a drawing by its place: a picture answers
        // the same presses a shape does.
        let mut editor = editor();
        let canvas = wp_raster::Canvas::filled(40, 40, wp_raster::Color::BLACK);
        let bytes = wp_raster::encode_png(&canvas);
        let end = editor.document.paragraph_text(0).map_or(0, |text| text.len());
        editor.document.set_caret(TextPosition::new(0, end));
        editor.document.insert_picture(&bytes, "png", 914_400, 914_400).expect("a picture");
        editor.relayout();

        let at = TextPosition::new(0, end);
        let drawing = editor
            .drawings_facing()
            .into_iter()
            .find(|drawing| drawing.at == at)
            .expect("the picture is not on the page");
        let (x, y) = (
            (drawing.left + drawing.width / 2.0) as i32,
            (drawing.top + drawing.height / 2.0) as i32,
        );

        assert!(editor.press_on_shape(x, y), "the picture was not pressed");
        assert_eq!(editor.chosen_drawing, Some(at));
        editor.drag_shape(x + 40, y + 30);
        editor.release_shape();

        // It was in the line, so the drag made it float.
        let anchor = editor.document.anchor_at(at).expect("the picture did not start floating");
        assert_eq!(anchor.wrap, Wrap::Square);
    }

    #[test]
    fn the_drawing_chosen_is_the_one_the_commands_act_on() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y);
        editor.release_shape();
        // The caret sits after the drawing, which in a paragraph of text is
        // somewhere the caret alone would find no drawing at all.
        editor.document.set_caret(TextPosition::new(0, 6));
        assert_eq!(editor.drawing_in_hand(), editor.chosen_drawing);
    }

    #[test]
    fn every_handle_sits_somewhere_on_the_edge() {
        for grip in Grip::EDGES {
            let (x, y) = grip.at();
            assert!(x == 0.0 || x == 1.0 || y == 0.0 || y == 1.0, "{grip:?} is not on an edge");
        }
    }

    #[test]
    fn the_eight_handles_are_all_different_places() {
        for (at, grip) in Grip::EDGES.iter().enumerate() {
            assert!(
                !Grip::EDGES[..at].iter().any(|earlier| earlier.at() == grip.at()),
                "{grip:?} is where another handle already is"
            );
        }
    }

    #[test]
    fn dragging_the_right_edge_widens_without_moving_the_left() {
        let (left, _, right, _) = Grip::Right.moves();
        assert_eq!(left, 0.0, "the left edge should stay where it is");
        assert_eq!(right, 1.0);
    }

    #[test]
    fn dragging_the_left_edge_moves_it_and_narrows_the_shape() {
        let (left, _, right, _) = Grip::Left.moves();
        assert_eq!(left, 1.0);
        assert_eq!(right, 0.0);
    }

    #[test]
    fn dragging_the_middle_moves_every_edge_together() {
        assert_eq!(Grip::Body.moves(), (1.0, 1.0, 1.0, 1.0));
    }

    #[test]
    fn a_drawing_in_the_line_starts_floating_when_it_is_moved() {
        let anchor = moved(None, 10.0, 20.0, 5);
        assert_eq!(anchor.wrap, Wrap::Square);
        assert_eq!(anchor.depth, 5, "it did not go on top of the pile");
        assert_eq!(anchor.horizontal, Placement::Offset(points_to_emu(10.0)));
        assert_eq!(anchor.vertical, Placement::Offset(points_to_emu(20.0)));
    }

    #[test]
    fn moving_a_drawing_that_was_lined_up_with_an_edge_gives_up_the_alignment() {
        let before =
            Anchor { horizontal: Placement::Aligned("right".to_owned()), ..Anchor::default() };
        let after = moved(Some(before), 5.0, 0.0, 0);
        assert_eq!(after.horizontal, Placement::Offset(points_to_emu(5.0)));
    }

    #[test]
    fn moving_a_drawing_twice_adds_the_distances_up() {
        let once = moved(None, 10.0, 0.0, 0);
        let twice = moved(Some(once), 10.0, 0.0, 0);
        assert_eq!(twice.horizontal, Placement::Offset(points_to_emu(20.0)));
    }
}

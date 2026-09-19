//! Choosing a drawing, moving it, and resizing it.
//!
//! # The second kind of selection
//!
//! A selection of text is a stretch of the document, or several of them since
//! **C22**. A drawing is not a stretch of anything: it is one thing, chosen or
//! not. So it is kept beside the other: [`Editor::chosen_drawings`] is the list
//! of places those drawings are at, and every command that acts on a drawing
//! asks it.
//!
//! A list rather than one place, because Align lines drawings up with each other
//! and one drawing has nothing to line up with. Shift and a click adds one to
//! the list or takes it out again; a band swept round a handful takes all of
//! them; and a drag on any of them moves every one.
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

use wp_docx::anchor::{Anchor, Placement, Relative, Wrap};
use wp_docx::floating::Turned;
use wp_docx::shapes::EMU_PER_POINT;
use wp_docx::TextPosition;
use wp_layout::Drawing;
use wp_shell::{Cursor, Response};

use super::Editor;

/// How big the square handles are, in pixels.
pub(super) const HANDLE: f32 = 7.0;

/// How far above the top edge the round handle that turns a drawing sits, in
/// pixels.
///
/// Far enough that it is not mistaken for the handle that changes the height,
/// which is the one directly under it.
pub(super) const TURN_REACH: f32 = 20.0;

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
    /// The round one above the top edge: the drawing turns.
    Turn,
    /// One of the shape's own yellow handles: the shape changes, and neither
    /// its size nor its place does. Which one, counted the way
    /// [`wp_layout::handles::handles_in`] lists them.
    Adjust(usize),
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
            // Above the box rather than on it; how far above is
            // [`TURN_REACH`], which a fraction of the box cannot say.
            Self::Turn => (0.5, 0.0),
            // Wherever the shape says, which is not a fraction of the box
            // either: see [`Editor::shape_handles`].
            Self::Adjust(_) => (0.5, 0.5),
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
            // These two change the drawing without moving an edge of it: one
            // turns it and one changes the shape itself.
            Self::Turn | Self::Adjust(_) => (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Which pointer belongs over it.
    fn cursor(self) -> Cursor {
        match self {
            Self::Top | Self::Bottom => Cursor::ResizeVertical,
            Self::Left | Self::Right => Cursor::ResizeHorizontal,
            // The circling arrow Word shows is not one the shell offers; the
            // hand at least says this is something to take hold of, and the
            // same goes for a yellow handle.
            Self::Turn | Self::Adjust(_) => Cursor::Hand,
            // The corners want a diagonal pointer, which the shell does not
            // offer yet; the horizontal one at least says the edge can be
            // dragged.
            _ => Cursor::ResizeHorizontal,
        }
    }
}

/// One drawing being dragged, as it was when the drag began.
///
/// Kept so that every move is measured from the start rather than from the last
/// move, which would drift.
#[derive(Clone, Debug)]
pub(super) struct Held {
    pub at: TextPosition,
    pub width_emu: i64,
    pub height_emu: i64,
    pub anchor: Option<Anchor>,
    /// How far round it was already turned, which a turn by the handle adds to.
    pub turned: Turned,
}

/// A drag of one drawing or of several that is under way.
#[derive(Clone, Debug)]
pub(super) struct ShapeDrag {
    pub grip: Grip,
    /// Where the pointer was when it began.
    pub from_x: f32,
    pub from_y: f32,
    /// Which drawings are being dragged: all of the chosen ones when the body
    /// was taken hold of, and only the one whose handle it is otherwise.
    pub held: Vec<Held>,
    /// Where in the pile a drawing goes if this drag is what makes it float.
    pub depth: u32,
    /// The middle of the drawing whose handle is held, on the screen. A turn is
    /// measured about it; nothing else uses it.
    pub middle: (f32, f32),
}

/// Where the handle that turns a drawing sits, on the screen.
///
/// Above the middle of the top edge, clear of the handle that changes the
/// height. Word puts it there, and puts it there whichever way round the
/// drawing already is: a handle that moved with the drawing would be a handle
/// nobody could find twice.
pub(super) fn turn_handle(drawing: &OnPage) -> (f32, f32) {
    (drawing.left + drawing.width / 2.0, drawing.top - TURN_REACH)
}

/// A drawing on a page: where it was drawn, and where it is in the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct OnPage {
    pub at: TextPosition,
    /// Which page it is on, which is what lining it up with the paper asks.
    pub page: usize,
    /// On the screen, not on the page: the scroll and the page's corner are
    /// already taken into account.
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// What the Align menu lines drawings up against.
///
/// Word's own three, at the foot of the same menu as the alignments themselves:
/// they are not more commands but what the commands are measured from, which is
/// why they are a mode and not eight more rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AlignTo {
    /// The drawings chosen, lined up with each other. Word's own default, and
    /// the only one of the three that needs more than one drawing.
    #[default]
    EachOther,
    /// The edges of the paper.
    Page,
    /// The edges of the text.
    Margin,
}

impl Editor {
    /// Chooses one drawing, giving up whatever was chosen before, and puts the
    /// caret beside it.
    ///
    /// The caret follows so that everything which asks the caret — the ribbon
    /// showing what is in force, Backspace, the scroll that keeps the caret in
    /// view — still gets a sensible answer. Which drawings the Arrange commands
    /// act on is the selection's doing, not the caret's.
    pub(super) fn choose_drawing_at(&mut self, at: TextPosition) {
        self.chosen_drawings = vec![at];
        self.document.set_caret(TextPosition::new(at.paragraph, at.offset + 1));
        self.note_diagram_chosen();
        self.needs_redraw = true;
    }

    /// Adds one to the drawings already chosen, or takes it out again.
    ///
    /// Shift and a click, which is how a second drawing joins the first
    /// everywhere. Clicking a chosen drawing again with Shift down lets go of
    /// that one and keeps the rest, because a selection you cannot subtract
    /// from is one you have to start again.
    pub(super) fn also_choose_drawing_at(&mut self, at: TextPosition) {
        match self.chosen_drawings.iter().position(|held| *held == at) {
            Some(index) => {
                self.chosen_drawings.remove(index);
            }
            None => self.chosen_drawings.push(at),
        }
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
            self.chosen_drawings = vec![at];
            self.needs_redraw = true;
        }
    }

    /// Gives up every drawing that was chosen, if any was.
    ///
    /// Returns whether anything was given up, so a press that let go of a
    /// drawing can be told from one that did nothing.
    pub(super) fn drop_chosen_drawing(&mut self) -> bool {
        if self.chosen_drawings.is_empty() {
            return false;
        }
        self.chosen_drawings.clear();
        self.needs_redraw = true;
        true
    }

    /// Which drawing the commands that act on one are about: the first chosen,
    /// else the one the caret is beside.
    ///
    /// The caret is still an answer because the keyboard has to be able to
    /// reach a drawing: a picture typed in and then wrapped never went near the
    /// mouse.
    #[must_use]
    pub(super) fn drawing_in_hand(&self) -> Option<TextPosition> {
        self.chosen_drawings.first().copied().or_else(|| self.document.drawing_place_here())
    }

    /// And which drawings the commands that act on several are about.
    ///
    /// Every one chosen, in the order they were chosen; failing that the one
    /// the caret is beside, so that a command given from the keyboard still has
    /// something to work on.
    #[must_use]
    pub(super) fn drawings_in_hand(&self) -> Vec<TextPosition> {
        if !self.chosen_drawings.is_empty() {
            return self.chosen_drawings.clone();
        }
        self.document.drawing_place_here().into_iter().collect()
    }

    /// Every drawing on the pages, the one nearest the reader first, each as
    /// one drawing however many pieces it is drawn in.
    ///
    /// That is the order a press asks them in: a drawing laid over another is
    /// the one that was pressed.
    ///
    /// A group is drawn as the several drawings inside it, all answering to the
    /// one place in the text — so the several are folded back into the one
    /// rectangle that holds them all. Everything above this asks "where is that
    /// drawing", and for a group the answer is where the whole of it is: that is
    /// what the handles go round, what Align lines up, and what Group measures.
    #[must_use]
    pub(super) fn drawings_facing(&self) -> Vec<OnPage> {
        let mut out: Vec<OnPage> = Vec::new();
        for drawing in self.placed_drawings() {
            let Some(already) = out.iter_mut().find(|held| held.at == drawing.at) else {
                out.push(drawing);
                continue;
            };
            let left = already.left.min(drawing.left);
            let top = already.top.min(drawing.top);
            let right = (already.left + already.width).max(drawing.left + drawing.width);
            let bottom = (already.top + already.height).max(drawing.top + drawing.height);
            already.left = left;
            already.top = top;
            already.width = right - left;
            already.height = bottom - top;
        }
        out
    }

    /// Every piece of every drawing, as the page holds them.
    ///
    /// The same list before a group's members are folded together, which is
    /// what a press is asked against: clicking the paper inside a group and
    /// beside everything in it is clicking the paper.
    #[must_use]
    fn placed_drawings(&self) -> Vec<OnPage> {
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
                    Drawing::Ink(ink) => (ink.at, ink.x, ink.y, ink.width, ink.height),
                };
                let Some(at) = at else { continue };
                out.push(OnPage {
                    at,
                    page: index,
                    left: origin_x + x,
                    top: top + y,
                    width,
                    height,
                });
            }
        }
        out
    }

    /// Where on the screen each chosen drawing is, of those on a page.
    pub(super) fn chosen_drawing_boxes(&self) -> Vec<OnPage> {
        if self.chosen_drawings.is_empty() {
            return Vec::new();
        }
        let facing = self.drawings_facing();
        self.chosen_drawings
            .iter()
            .filter_map(|at| facing.iter().find(|drawing| drawing.at == *at).copied())
            .collect()
    }

    /// The drawing at a point on the screen, if there is one.
    #[must_use]
    pub(super) fn drawing_under(&self, x: i32, y: i32) -> Option<TextPosition> {
        let (px, py) = (x as f32, y as f32);
        self.placed_drawings()
            .into_iter()
            .find(|drawing| {
                px >= drawing.left
                    && px < drawing.left + drawing.width
                    && py >= drawing.top
                    && py < drawing.top + drawing.height
            })
            .map(|drawing| drawing.at)
    }

    /// Which handle of which chosen drawing a point is on, if any.
    ///
    /// Every chosen drawing carries its own handles, so a point is asked of
    /// each in turn: dragging one drawing's corner resizes that drawing, and
    /// dragging any of their bodies moves all of them together.
    pub(super) fn grip_at(&self, x: i32, y: i32) -> Option<(TextPosition, Grip)> {
        let (px, py) = (x as f32, y as f32);
        for drawing in self.chosen_drawing_boxes() {
            // The one that turns the drawing comes first: it stands clear of
            // the box, so nothing else can be where it is.
            let (turn_x, turn_y) = turn_handle(&drawing);
            if (px - turn_x).abs() <= HANDLE && (py - turn_y).abs() <= HANDLE {
                return Some((drawing.at, Grip::Turn));
            }
            for grip in Grip::EDGES {
                let (fx, fy) = grip.at();
                let cx = drawing.left + drawing.width * fx;
                let cy = drawing.top + drawing.height * fy;
                if (px - cx).abs() <= HANDLE && (py - cy).abs() <= HANDLE {
                    return Some((drawing.at, *grip));
                }
            }
            // The shape's own handles come before its body: they sit inside the
            // box, and a press on one that fell through to the body would move
            // the drawing instead of changing it.
            for (index, handle) in self.shape_handles(&drawing).into_iter().enumerate() {
                let at = handle.at();
                if (px - at.x).abs() <= HANDLE && (py - at.y).abs() <= HANDLE {
                    return Some((drawing.at, Grip::Adjust(index)));
                }
            }
            if px >= drawing.left
                && px < drawing.left + drawing.width
                && py >= drawing.top
                && py < drawing.top + drawing.height
            {
                return Some((drawing.at, Grip::Body));
            }
        }
        None
    }

    /// The yellow handles of a chosen drawing, on the screen.
    ///
    /// Where each one sits is the shape's own business — a corner handle slides
    /// along the top edge and a star's slides out from the middle — so the
    /// shape is asked. See [`wp_layout::handles::handles_in`].
    pub(super) fn shape_handles(&self, drawing: &OnPage) -> Vec<wp_layout::handles::Handle> {
        let Some(shape) = self.document.shape_at(drawing.at) else { return Vec::new() };
        let preset = wp_layout::geometry::Preset::from_word(&shape.preset);
        let adjusts = wp_layout::geometry::Adjusts::from_pairs(&shape.adjusts);
        wp_layout::handles::handles_in(
            preset,
            &adjusts,
            drawing.left,
            drawing.top,
            drawing.width,
            drawing.height,
        )
    }

    /// Which pointer belongs over a point, when a drawing is chosen.
    pub(super) fn shape_cursor(&self, x: i32, y: i32) -> Option<Cursor> {
        match self.grip_at(x, y)?.1 {
            Grip::Body => Some(Cursor::Arrow),
            grip => Some(grip.cursor()),
        }
    }

    /// Takes hold of a drawing, or of one of its handles.
    ///
    /// Returns whether it took hold of anything. A press that lands on no
    /// drawing gives up the one that was chosen and answers no, so that it goes
    /// on to mean whatever it would have meant in the text.
    pub(super) fn press_on_shape(&mut self, x: i32, y: i32, adding: bool) -> bool {
        // A handle of a drawing already chosen comes first: it lies on the
        // drawing's edge, and outside it at the corners.
        if !adding {
            if let Some((at, grip)) = self.grip_at(x, y) {
                return self.take_hold_of(at, grip, x, y);
            }
        }

        let Some(at) = self.drawing_under(x, y) else {
            // Word gives the drawings up when the next press lands somewhere
            // else. While Select Objects is in hand the press stops there: it
            // is about drawings and nothing else, so one that finds none of
            // them puts the caret nowhere — and a drag from there is the band
            // that gathers up whatever it is drawn round.
            self.drop_chosen_drawing();
            if self.choosing_drawings {
                self.choosing_band = Some((x, y, x, y));
                return true;
            }
            return false;
        };

        if adding {
            self.also_choose_drawing_at(at);
            // Shift and a click is about choosing rather than about moving, so
            // nothing is taken hold of: a drag from here would be a surprise.
            return true;
        }

        // A drawing already among the chosen keeps the rest of them, so that a
        // handful can be dragged by any one of it.
        if !self.chosen_drawings.contains(&at) {
            self.choose_drawing_at(at);
        }
        self.take_hold_of(at, Grip::Body, x, y)
    }

    /// Carries on the band being dragged round a handful of drawings.
    pub(super) fn drag_band(&mut self, x: i32, y: i32) -> Response {
        let Some((from_x, from_y, _, _)) = self.choosing_band else { return Response::Ignored };
        self.choosing_band = Some((from_x, from_y, x, y));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Lets the band go, choosing every drawing it was drawn round.
    ///
    /// Round rather than over: Word takes a drawing the band touches at all,
    /// which is what anybody sweeping a rectangle over a group of them means.
    pub(super) fn release_band(&mut self) -> bool {
        let Some((from_x, from_y, to_x, to_y)) = self.choosing_band.take() else { return false };
        let (left, right) = (from_x.min(to_x) as f32, from_x.max(to_x) as f32);
        let (top, bottom) = (from_y.min(to_y) as f32, from_y.max(to_y) as f32);

        self.chosen_drawings = self
            .drawings_facing()
            .into_iter()
            .filter(|drawing| {
                drawing.left < right
                    && drawing.left + drawing.width > left
                    && drawing.top < bottom
                    && drawing.top + drawing.height > top
            })
            .map(|drawing| drawing.at)
            .collect();
        // The last one chosen is the one the caret goes beside, so that
        // everything asking the caret has an answer inside the selection.
        if let Some(at) = self.chosen_drawings.first().copied() {
            self.document.set_caret(TextPosition::new(at.paragraph, at.offset + 1));
        }
        self.needs_redraw = true;
        true
    }

    /// Whether a band is being dragged.
    #[must_use]
    pub(super) fn dragging_band(&self) -> bool {
        self.choosing_band.is_some()
    }

    /// Where the band is on the screen, for drawing it.
    #[must_use]
    pub(super) fn band_rect(&self) -> Option<(i32, i32, i32, i32)> {
        let (from_x, from_y, to_x, to_y) = self.choosing_band?;
        Some((from_x.min(to_x), from_y.min(to_y), (from_x - to_x).abs(), (from_y - to_y).abs()))
    }

    /// Remembers what the drawings were, so the drag can be measured from them.
    ///
    /// A handle belongs to one drawing and moves only that one; the body moves
    /// everything chosen, which is what makes a handful of drawings something
    /// that can be arranged rather than something that has to be moved one at a
    /// time.
    fn take_hold_of(&mut self, at: TextPosition, grip: Grip, x: i32, y: i32) -> bool {
        let dragging: Vec<TextPosition> = if grip == Grip::Body {
            let mut all = self.drawings_in_hand();
            if !all.contains(&at) {
                all.push(at);
            }
            all
        } else {
            vec![at]
        };

        let held: Vec<Held> = dragging
            .into_iter()
            .filter_map(|at| {
                let (width_emu, height_emu) = self.document.drawing_size_at(at)?;
                Some(Held {
                    at,
                    width_emu,
                    height_emu,
                    anchor: self.document.anchor_at(at),
                    turned: self.document.drawing_turn_at(at),
                })
            })
            .collect();
        if held.is_empty() {
            return false;
        }

        // Where the drawing whose handle this is has its middle, which is what
        // a turn is measured about.
        let middle = self
            .chosen_drawing_boxes()
            .into_iter()
            .find(|drawing| drawing.at == at)
            .map_or((x as f32, y as f32), |drawing| {
                (drawing.left + drawing.width / 2.0, drawing.top + drawing.height / 2.0)
            });

        // One drag is one thing to undo, however many moves and however many
        // drawings it is made of.
        self.document.begin_gesture();
        self.shape_drag = Some(ShapeDrag {
            grip,
            from_x: x as f32,
            from_y: y as f32,
            held,
            depth: self.document.next_drawing_depth(),
            middle,
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

        // The handle above the drawing turns it and does nothing else: how far
        // round is the angle the pointer has swept about the drawing's middle,
        // added to the turn it already had.
        if drag.grip == Grip::Turn {
            return self.turn_by_handle(&drag, x, y);
        }
        // And a yellow one changes the shape and neither moves nor resizes it.
        if let Grip::Adjust(index) = drag.grip {
            return self.adjust_by_handle(&drag, index, x, y);
        }

        // How far the pointer has come, in points rather than pixels: the
        // document is measured in points and the zoom must not change how far
        // a drag moves a drawing.
        let dx = (x as f32 - drag.from_x) / scale;
        let dy = (y as f32 - drag.from_y) / scale;
        let (left, top, right, bottom) = drag.grip.moves();

        let mut changed = false;
        for held in &drag.held {
            if drag.grip != Grip::Body {
                let width = emu_to_points(held.width_emu) + (right - left) * dx;
                let height = emu_to_points(held.height_emu) + (bottom - top) * dy;
                changed |= self.document.set_drawing_size_at(
                    held.at,
                    points_to_emu(width.max(LEAST)),
                    points_to_emu(height.max(LEAST)),
                );
            }

            // The body moves the drawing. So does a left or top handle, because
            // the opposite edge is the one staying put — but only for a drawing
            // that already floats: one in the line has nowhere to be moved to,
            // and making it float because it was made wider would be a
            // surprise.
            let floats = held.anchor.is_some();
            let (across, down) = match drag.grip {
                Grip::Body => (dx, dy),
                _ if floats => (dx * left, dy * top),
                _ => (0.0, 0.0),
            };
            if drag.grip == Grip::Body || across != 0.0 || down != 0.0 {
                let anchor = moved(held.anchor.clone(), across, down, drag.depth);
                changed |= self.document.set_anchor_at(held.at, Some(&anchor));
            }
        }

        if !changed {
            return Response::Ignored;
        }
        // A drawing that has moved takes the connectors fastened to it with
        // it. The screen would follow the join anyway, because the layout works
        // one out afresh every time; this is so that the file says the same
        // thing, and a document saved here opens in Word with its connectors
        // where they are on the screen.
        self.document.rejoin_connectors();
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Moves the yellow handle a drag has hold of.
    ///
    /// The handle itself says what a place is worth, so a drag anywhere is a
    /// value: it slides along its own line, and a pointer that has wandered off
    /// that line counts for how far along it has come and no more. See
    /// [`wp_layout::handles::Handle::value_at`].
    fn adjust_by_handle(&mut self, drag: &ShapeDrag, index: usize, x: i32, y: i32) -> Response {
        let Some(held) = drag.held.first() else { return Response::Ignored };
        let Some(drawing) = self.chosen_drawing_boxes().into_iter().find(|box_| box_.at == held.at)
        else {
            return Response::Ignored;
        };
        let Some(handle) = self.shape_handles(&drawing).into_iter().nth(index) else {
            return Response::Ignored;
        };
        let value = handle.value_at(wp_raster::Point::new(x as f32, y as f32));
        // Held to what a shape can be: the format states these over a hundred
        // thousandth, and a shape whose handle is dragged past what it means is
        // a shape drawn inside out.
        let value = value.clamp(0, 100_000);
        if !self.document.set_adjust_at(held.at, handle.name, value) {
            return Response::Ignored;
        }
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Turns the drawing a drag of its round handle is turning.
    ///
    /// Measured as a sweep from where the pointer started rather than from
    /// straight up, so that taking hold of the handle does not itself move the
    /// drawing: a turn is a difference, the way every other drag here is.
    fn turn_by_handle(&mut self, drag: &ShapeDrag, x: i32, y: i32) -> Response {
        let (middle_x, middle_y) = drag.middle;
        let was = (drag.from_y - middle_y).atan2(drag.from_x - middle_x);
        let now = (y as f32 - middle_y).atan2(x as f32 - middle_x);
        // The format counts clockwise, and so does a screen whose y counts
        // down the page, so the two agree without a change of sign.
        let swept = (now - was) / core::f32::consts::TAU * Turned::WHOLE as f32;
        let swept = swept.round() as i32;
        if swept == 0 {
            return Response::Ignored;
        }

        let mut changed = false;
        for held in &drag.held {
            changed |= self.document.set_drawing_turn_at(held.at, held.turned.turned_by(swept));
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
        if let Some(drag) = self.shape_drag.take() {
            // A drawing dropped somewhere else hangs from the paragraph it was
            // dropped nearest, which is Word's rule and what keeps it where it
            // was put. See [`Editor::reanchor`].
            if drag.grip == Grip::Body {
                self.reanchor(&drag);
            }
            self.document.end_gesture();
            self.update_title();
        }
    }

    /// Re-hangs the dragged drawings on the paragraphs they were dropped
    /// nearest.
    ///
    /// # Why a drawing changes paragraph at all
    ///
    /// Because a floating drawing hangs from one, and that is what it moves
    /// with. A picture dragged three pages down and still tied to where it came
    /// from follows that paragraph about: a line added above it moves the
    /// picture, and it comes back to where it started on the next edit. Word
    /// re-hangs it, and only Lock anchor stops that — which is what the lock is
    /// for.
    ///
    /// The drawing does not move on the page: the distance it hangs at is
    /// worked out afresh from where it is drawn now, so the paragraph changes
    /// underneath it and nothing else does.
    fn reanchor(&mut self, drag: &ShapeDrag) {
        // Where each of them is drawn now, and what it hangs from.
        let mut held: Vec<(TextPosition, Anchor)> = Vec::new();
        for one in &drag.held {
            let Some(anchor) = self.document.anchor_at(one.at) else { continue };
            // A locked anchor is one the person has said to leave alone.
            if anchor.locked {
                continue;
            }
            held.push((one.at, anchor));
        }

        for (at, anchor) in held {
            let Some(drawn) = self.drawings_facing().into_iter().find(|found| found.at == at)
            else {
                continue;
            };
            let Some((paragraph, line_top)) = self.paragraph_under(drawn.page, drawn.top) else {
                continue;
            };
            if paragraph == at.paragraph {
                continue;
            }

            // How far below that paragraph's first line the drawing is drawn.
            // Only the frames that mean "from where the text is" are measured
            // that way; from the page or the margin, the distance says the same
            // thing whichever paragraph the drawing hangs from.
            let anchor = match anchor.vertical_from {
                Relative::Paragraph | Relative::Line => {
                    let scale = self.pixels_per_inch() / 72.0;
                    if scale <= 0.0 {
                        continue;
                    }
                    let down = (drawn.top - line_top) / scale;
                    Anchor { vertical: Placement::Offset(points_to_emu(down)), ..anchor }
                }
                _ => anchor,
            };

            let Some(moved_to) = self.document.move_drawing_to(at, paragraph) else { continue };
            self.document.set_anchor_at(moved_to, Some(&anchor));
        }
        self.relayout();
    }

    /// Which paragraph a point down a page belongs to, and where that
    /// paragraph's first line sits on the screen.
    ///
    /// The paragraph whose own band the point is in, or the nearest one above
    /// it: a drawing dropped in the white space under the text hangs from the
    /// last paragraph, which is what Word does with it.
    pub(super) fn paragraph_under(&self, page: usize, top: f32) -> Option<(usize, f32)> {
        let (_, origin_y) = self.page_origin(page);
        let up = self.content_top() + origin_y - self.scroll_down();
        let lines = &self.pages.get(page)?.lines;

        let mut best: Option<(usize, f32)> = None;
        for line in lines {
            let at = up + line.top();
            let nearer = best.is_none_or(|(_, found)| (at - top).abs() < (found - top).abs());
            if nearer {
                best = Some((line.paragraph, at));
            }
        }
        // The first line of whichever paragraph that was: a drawing hangs from
        // the paragraph, and the paragraph begins where its first line does.
        let (paragraph, _) = best?;
        let first =
            lines.iter().find(|line| line.paragraph == paragraph).map(|line| up + line.top())?;
        Some((paragraph, first))
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
        // A share of a frame gives way too: what a drag or an Align hands
        // over is a distance, and the drawing is no longer a share of
        // anything.
        Placement::Aligned(_) | Placement::Percent(_) => 0,
    };
    let down = match anchor.vertical {
        Placement::Offset(distance) => distance,
        // A share of a frame gives way too: what a drag or an Align hands
        // over is a distance, and the drawing is no longer a share of
        // anything.
        Placement::Aligned(_) | Placement::Percent(_) => 0,
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
            fill: wp_docx::fills::Fill::solid("4472C4"),
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
        let drawing = editor.chosen_drawing_boxes().into_iter().next().expect("a chosen drawing");
        let (fx, fy) = grip.at();
        ((drawing.left + drawing.width * fx) as i32, (drawing.top + drawing.height * fy) as i32)
    }

    /// An editor whose document is several paragraphs with the drawing hanging
    /// from the first, so that a drag has somewhere to take it.
    fn editor_with_paragraphs() -> Editor {
        let mut body = Body::default();
        for text in ["First paragraph", "Second paragraph", "Third paragraph"] {
            body.blocks.push(Block::Paragraph(Paragraph::text(text)));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        let shape = wp_docx::shapes::Shape {
            name: "Box".to_owned(),
            width_emu: 457_200,
            height_emu: 228_600,
            fill: wp_docx::fills::Fill::solid("4472C4"),
            anchor: Some(Anchor { wrap: Wrap::Square, ..Anchor::default() }),
            ..wp_docx::shapes::Shape::default()
        };
        editor.document.set_caret(TextPosition::new(0, 0));
        assert!(editor.document.insert_shape(&shape), "the shape went nowhere");
        editor.relayout();
        editor
    }

    #[test]
    fn a_drawing_dragged_down_the_page_hangs_from_the_paragraph_it_landed_by() {
        // Word's rule, and what keeps a picture where it was put: one still
        // tied to the paragraph it came from follows that paragraph about.
        let mut editor = editor_with_paragraphs();
        let before = editor.drawings_facing().first().copied().expect("a drawing");
        assert_eq!(before.at.paragraph, 0, "it should start on the first paragraph");

        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y, false);
        // Down to the third paragraph, whose first line the layout knows.
        let third = editor.pages[0]
            .lines
            .iter()
            .find(|line| line.paragraph == 2)
            .expect("the third paragraph is on the page")
            .clone();
        let (_, origin_y) = editor.page_origin(0);
        let up = editor.content_top() + origin_y - editor.scroll_down();
        let wanted = (up + third.baseline) as i32;
        editor.drag_shape(x, wanted);
        editor.release_shape();
        editor.relayout();

        let after = editor.drawings_facing().first().copied().expect("a drawing");
        assert_ne!(
            after.at.paragraph, 0,
            "the drawing is still hanging from the paragraph it came from"
        );
        // And it has not moved on the page: the paragraph changed underneath it
        // and nothing else did.
        assert!(
            (after.top - (up + third.top())).abs() < 12.0,
            "it jumped: {} against {}",
            after.top,
            up + third.top()
        );
    }

    #[test]
    fn a_locked_anchor_keeps_the_paragraph_it_hangs_from() {
        let mut editor = editor_with_paragraphs();
        let at = editor.drawings_facing().first().copied().expect("a drawing").at;
        let anchor = Anchor { locked: true, ..editor.document.anchor_at(at).expect("an anchor") };
        assert!(editor.document.set_anchor_at(at, Some(&anchor)));
        editor.relayout();

        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y, false);
        editor.drag_shape(x, y + 120);
        editor.release_shape();
        editor.relayout();

        let after = editor.drawings_facing().first().copied().expect("a drawing");
        assert_eq!(after.at.paragraph, 0, "a locked anchor was moved anyway");
    }

    #[test]
    fn a_drawing_dragged_a_little_stays_where_it_hangs() {
        // The paragraph it was dropped nearest is the one it came from, so
        // nothing about the document's shape changes.
        let mut editor = editor_with_paragraphs();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y, false);
        editor.drag_shape(x + 20, y + 4);
        editor.release_shape();

        let after = editor.drawings_facing().first().copied().expect("a drawing");
        assert_eq!(after.at.paragraph, 0, "a small drag moved the anchor");
    }

    #[test]
    fn a_press_inside_a_drawing_chooses_it() {
        let mut editor = editor();
        assert!(editor.chosen_drawings.is_empty());
        let (x, y) = middle(&editor);
        assert!(editor.press_on_shape(x, y, false), "the press found no drawing");
        assert!(!editor.chosen_drawings.is_empty());
        editor.release_shape();
    }

    #[test]
    fn a_press_away_from_every_drawing_gives_it_up() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y, false);
        editor.release_shape();

        assert!(
            !editor.press_on_shape(x + 500, y + 250, false),
            "the press took hold of something"
        );
        assert!(editor.chosen_drawings.is_empty());
    }

    #[test]
    fn a_drag_moves_the_drawing() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y, false);
        editor.drag_shape(x + 60, y + 40);
        editor.release_shape();

        let at = editor.chosen_drawings.first().copied().expect("a chosen drawing");
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
        editor.press_on_shape(x, y, false);
        editor.release_shape();
        let at = editor.chosen_drawings.first().copied().expect("a chosen drawing");
        let (before, _) = editor.document.drawing_size_at(at).expect("a size");

        let (hx, hy) = handle(&editor, Grip::Right);
        assert!(editor.press_on_shape(hx, hy, false), "the handle was not found");
        editor.drag_shape(hx + 40, hy);
        editor.release_shape();

        let (after, _) = editor.document.drawing_size_at(at).expect("a size");
        assert!(after > before, "it did not grow: {before} then {after}");
    }

    #[test]
    fn a_drawing_cannot_be_dragged_away_to_nothing() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y, false);
        editor.release_shape();
        let at = editor.chosen_drawings.first().copied().expect("a chosen drawing");

        let (hx, hy) = handle(&editor, Grip::Right);
        editor.press_on_shape(hx, hy, false);
        editor.drag_shape(hx - 5000, hy);
        editor.release_shape();

        let (width, _) = editor.document.drawing_size_at(at).expect("a size");
        assert!(width >= points_to_emu(LEAST), "it was dragged out of existence");
    }

    #[test]
    fn one_undo_takes_back_one_drag() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y, false);
        for step in 1..=4 {
            editor.drag_shape(x + step * 15, y);
        }
        editor.release_shape();

        let at = editor.chosen_drawings.first().copied().expect("a chosen drawing");
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

        assert!(editor.press_on_shape(x, y, false), "the picture was not pressed");
        assert_eq!(editor.chosen_drawings, vec![at]);
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
        editor.press_on_shape(x, y, false);
        editor.release_shape();
        // The caret sits after the drawing, which in a paragraph of text is
        // somewhere the caret alone would find no drawing at all.
        editor.document.set_caret(TextPosition::new(0, 6));
        assert_eq!(editor.drawing_in_hand(), editor.chosen_drawings.first().copied());
    }

    #[test]
    fn the_handle_that_turns_a_drawing_stands_clear_above_it() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y, false);
        editor.release_shape();

        let drawing = editor.chosen_drawing_boxes().into_iter().next().expect("a chosen drawing");
        let (turn_x, turn_y) = turn_handle(&drawing);
        assert!(turn_y < drawing.top, "it is not above the drawing");
        assert_eq!(
            editor.grip_at(turn_x as i32, turn_y as i32),
            Some((drawing.at, Grip::Turn)),
            "the handle above the drawing is not the one that turns it"
        );
        // And the one directly under it is still the one that changes the
        // height, which is the reason it stands as far off as it does.
        let (top_x, top_y) = handle(&editor, Grip::Top);
        assert_eq!(editor.grip_at(top_x, top_y), Some((drawing.at, Grip::Top)));
    }

    #[test]
    fn that_handle_is_drawn_where_it_is_pressed() {
        // A handle that can be pressed and cannot be seen is a handle nobody
        // will ever press.
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y, false);
        editor.release_shape();

        let drawing = editor.chosen_drawing_boxes().into_iter().next().expect("a chosen drawing");
        let (turn_x, turn_y) = turn_handle(&drawing);
        let accent = editor.theme.accent;
        let canvas = editor.draw(1400, 900);

        let reach = HANDLE as i32;
        let mut found = false;
        for down in -reach..=reach {
            for across in -reach..=reach {
                let (px, py) = (turn_x as i32 + across, turn_y as i32 + down);
                if px < 0 || py < 0 {
                    continue;
                }
                found |= canvas.pixel(px as usize, py as usize) == accent;
            }
        }
        assert!(found, "nothing is drawn where the handle that turns the drawing is pressed");
    }

    #[test]
    fn a_drag_of_that_handle_turns_the_drawing() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y, false);
        editor.release_shape();
        let at = editor.chosen_drawings.first().copied().expect("a chosen drawing");
        assert_eq!(editor.document.drawing_turn_at(at).rotation, 0);

        let drawing = editor.chosen_drawing_boxes().into_iter().next().expect("a chosen drawing");
        let (turn_x, turn_y) = turn_handle(&drawing);
        let (middle_x, middle_y) =
            (drawing.left + drawing.width / 2.0, drawing.top + drawing.height / 2.0);
        assert!(editor.press_on_shape(turn_x as i32, turn_y as i32, false));
        // A quarter of the way round: from above the middle to the right of it.
        let reach = middle_y - turn_y;
        editor.drag_shape((middle_x + reach) as i32, middle_y as i32);
        editor.release_shape();

        let turned = editor.document.drawing_turn_at(at);
        let quarter = Turned::WHOLE / 4;
        assert!(
            (turned.rotation - quarter).abs() < quarter / 20,
            "a quarter sweep turned it by {}",
            turned.rotation
        );
    }

    #[test]
    fn one_undo_takes_back_a_whole_turn_of_the_handle() {
        let mut editor = editor();
        let (x, y) = middle(&editor);
        editor.press_on_shape(x, y, false);
        editor.release_shape();
        let at = editor.chosen_drawings.first().copied().expect("a chosen drawing");

        let drawing = editor.chosen_drawing_boxes().into_iter().next().expect("a chosen drawing");
        let (turn_x, turn_y) = turn_handle(&drawing);
        let (middle_x, middle_y) =
            (drawing.left + drawing.width / 2.0, drawing.top + drawing.height / 2.0);
        editor.press_on_shape(turn_x as i32, turn_y as i32, false);
        // Several moves, as a real drag is made of.
        for step in 1..=4 {
            let reach = (middle_y - turn_y) * step as f32 / 4.0;
            editor.drag_shape((middle_x + reach) as i32, (middle_y - reach) as i32);
        }
        editor.release_shape();
        assert_ne!(editor.document.drawing_turn_at(at).rotation, 0, "it did not turn at all");

        editor.document.undo();
        assert_eq!(editor.document.drawing_turn_at(at).rotation, 0, "one undo was not enough");
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

    /// An editor showing one chosen rounded rectangle, which is a shape with a
    /// handle of its own.
    fn editor_with_a_handle() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Some words to flow round it")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        let shape = wp_docx::shapes::Shape {
            name: "Box".to_owned(),
            preset: "roundRect".to_owned(),
            width_emu: 1_828_800,
            height_emu: 914_400,
            fill: wp_docx::fills::Fill::solid("4472C4"),
            anchor: Some(Anchor { wrap: Wrap::Square, ..Anchor::default() }),
            ..wp_docx::shapes::Shape::default()
        };
        editor.document.set_caret(TextPosition::new(0, 0));
        assert!(editor.document.insert_shape(&shape), "the shape went nowhere");
        editor.relayout();
        editor.choose_drawing_here();
        editor
    }

    /// What the one shape's handle is worth in the document now.
    fn corner_of(editor: &Editor) -> Option<i32> {
        let at = editor.drawings_facing().first().copied().expect("a drawing").at;
        editor.document.shape_at(at).and_then(|shape| shape.adjust("adj"))
    }

    #[test]
    fn a_shape_that_can_be_changed_shows_a_handle_to_change_it_by() {
        let editor = editor_with_a_handle();
        let drawing = editor.chosen_drawing_boxes().into_iter().next().expect("a chosen drawing");
        let handles = editor.shape_handles(&drawing);
        assert_eq!(handles.len(), 1, "a rounded rectangle has one handle");

        // On the top edge, a sixth of the shorter side in from the corner.
        let at = handles[0].at();
        assert!((at.y - drawing.top).abs() < 1.0, "it should sit on the top edge");
        let along = (at.x - drawing.left) / drawing.height;
        assert!((along - 1.0 / 6.0).abs() < 0.01, "it sits {along} of the way along");
    }

    #[test]
    fn a_press_on_the_yellow_handle_takes_hold_of_it() {
        // And not of the body underneath it, which would move the drawing
        // instead of changing it.
        let editor = editor_with_a_handle();
        let drawing = editor.chosen_drawing_boxes().into_iter().next().expect("a chosen drawing");
        let at = editor.shape_handles(&drawing)[0].at();
        let grip = editor.grip_at(at.x as i32, at.y as i32);
        assert_eq!(grip.map(|(_, grip)| grip), Some(Grip::Adjust(0)), "it took hold of {grip:?}");
    }

    #[test]
    fn dragging_the_yellow_handle_changes_the_shape() {
        let mut editor = editor_with_a_handle();
        assert_eq!(corner_of(&editor), None, "a shape nobody has dragged says nothing");

        let drawing = editor.chosen_drawing_boxes().into_iter().next().expect("a chosen drawing");
        let at = editor.shape_handles(&drawing)[0].at();
        assert!(
            editor.press_on_shape(at.x as i32, at.y as i32, false),
            "nothing was taken hold of"
        );
        // Further along the top edge: a rounder corner.
        editor.drag_shape((drawing.left + drawing.height / 2.0) as i32, drawing.top as i32);
        editor.release_shape();

        let corner = corner_of(&editor).expect("the handle was written");
        assert!(corner > 40_000, "the corner is {corner} and was dragged to half");
    }

    #[test]
    fn one_drag_of_a_yellow_handle_is_one_thing_to_undo() {
        let mut editor = editor_with_a_handle();
        let drawing = editor.chosen_drawing_boxes().into_iter().next().expect("a chosen drawing");
        let at = editor.shape_handles(&drawing)[0].at();

        assert!(editor.press_on_shape(at.x as i32, at.y as i32, false));
        // Several moves, as a real drag is.
        for step in 1..=4 {
            let along = drawing.left + drawing.height * (0.1 * step as f32 + 0.1);
            editor.drag_shape(along as i32, drawing.top as i32);
        }
        editor.release_shape();
        assert!(corner_of(&editor).is_some(), "the drag changed nothing");

        editor.document.undo();
        assert_eq!(corner_of(&editor), None, "one drag should be one thing to undo");
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

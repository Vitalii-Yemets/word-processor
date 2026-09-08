//! Taking hold of a drawing: selecting it, moving it, and resizing it.
//!
//! # What being selected means here
//!
//! That the caret is beside the drawing. There is no second kind of selection
//! and no second kind of caret: pressing a drawing puts the caret next to it,
//! and every command that acts on a drawing acts on the one the caret is
//! beside. One idea rather than two, and it means the keyboard can reach a
//! drawing as well as the mouse — Backspace deletes it because it is a
//! character, and always was.
//!
//! # Why a drawing starts floating when it is dragged
//!
//! Because a drawing in the line of text has no position of its own to change.
//! It sits where the words put it. Dragging one is asking for it to be
//! somewhere in particular, which is what floating means — so the first drag
//! anchors it, exactly as Word's does.

use wp_docx::anchor::{Anchor, Placement, Wrap};
use wp_docx::shapes::{Shape, EMU_PER_POINT};
use wp_docx::TextPosition;
use wp_shell::{Cursor, Response};

use super::Editor;

/// How big the square handles are, in pixels.
pub(super) const HANDLE: f32 = 7.0;

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
    /// What the drawing was then, so every move is measured from the start
    /// rather than from the last move — which would drift.
    pub original: Shape,
}

impl Editor {
    /// Where on the screen the drawing beside the caret is, if it is shown.
    pub(super) fn selected_shape_box(&self) -> Option<(f32, f32, f32, f32)> {
        let caret = self.document.caret();
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let y = self.content_top() + origin_y - self.scroll_down();
            for shape in &self.pages[index].shapes {
                let Some(at) = shape.at else { continue };
                if at.paragraph != caret.paragraph {
                    continue;
                }
                if caret.offset != at.offset && caret.offset != at.offset + 1 {
                    continue;
                }
                return Some((origin_x + shape.x, y + shape.y, shape.width, shape.height));
            }
        }
        None
    }

    /// The drawing at a point on the screen, if there is one.
    pub(super) fn shape_at(&self, x: i32, y: i32) -> Option<TextPosition> {
        let (px, py) = (x as f32, y as f32);
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            // The last drawing on the page is the one drawn on top, so it is
            // the one a press lands on.
            for shape in self.pages[index].shapes.iter().rev() {
                let left = origin_x + shape.x;
                let shape_top = top + shape.y;
                if px >= left
                    && px < left + shape.width
                    && py >= shape_top
                    && py < shape_top + shape.height
                {
                    return shape.at;
                }
            }
        }
        None
    }

    /// Which handle of the selected drawing a point is on, if any.
    pub(super) fn grip_at(&self, x: i32, y: i32) -> Option<Grip> {
        let (left, top, width, height) = self.selected_shape_box()?;
        let (px, py) = (x as f32, y as f32);

        for grip in Grip::EDGES {
            let (fx, fy) = grip.at();
            let cx = left + width * fx;
            let cy = top + height * fy;
            if (px - cx).abs() <= HANDLE && (py - cy).abs() <= HANDLE {
                return Some(*grip);
            }
        }
        if px >= left && px < left + width && py >= top && py < top + height {
            return Some(Grip::Body);
        }
        None
    }

    /// Which pointer belongs over a point, when a drawing is selected.
    pub(super) fn shape_cursor(&self, x: i32, y: i32) -> Option<Cursor> {
        match self.grip_at(x, y)? {
            Grip::Body => Some(Cursor::Arrow),
            grip => Some(grip.cursor()),
        }
    }

    /// Takes hold of a drawing, or of one of its handles.
    ///
    /// Returns whether it took hold of anything.
    pub(super) fn press_on_shape(&mut self, x: i32, y: i32) -> bool {
        // A handle of the drawing already selected comes first: it lies on the
        // drawing's edge, and outside it at the corners.
        if let Some(grip) = self.grip_at(x, y) {
            if let Some(shape) = self.document.shape_here() {
                self.document.begin_gesture();
                self.shape_drag =
                    Some(ShapeDrag { grip, from_x: x as f32, from_y: y as f32, original: shape });
                self.needs_redraw = true;
                return true;
            }
        }

        let Some(at) = self.shape_at(x, y) else { return false };
        // The caret goes after the drawing, which is where it would be if the
        // drawing had just been typed.
        self.document.set_caret(TextPosition::new(at.paragraph, at.offset + 1));
        let Some(shape) = self.document.shape_here() else { return false };

        self.document.begin_gesture();
        self.shape_drag = Some(ShapeDrag {
            grip: Grip::Body,
            from_x: x as f32,
            from_y: y as f32,
            original: shape,
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

        let mut shape = drag.original.clone();
        if drag.grip == Grip::Body {
            shape.anchor = Some(moved(shape.anchor.clone(), dx, dy));
        } else {
            // An edge changes the size; a left or top edge changes the position
            // as well, because the opposite edge is the one staying put.
            let width = drag.original.width_points() as f32 + (right - left) * dx;
            let height = drag.original.height_points() as f32 + (bottom - top) * dy;
            shape.width_emu = points_to_emu(width.max(8.0));
            shape.height_emu = points_to_emu(height.max(8.0));
            if left > 0.0 || top > 0.0 {
                shape.anchor = Some(moved(shape.anchor.clone(), dx * left, dy * top));
            }
        }

        if !self.document.replace_shape_here(&shape) {
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
}

/// The same anchor, moved by a distance in points.
///
/// A drawing that was in the line of text starts floating, because a drawing in
/// the line has no position of its own to move.
fn moved(anchor: Option<Anchor>, dx: f32, dy: f32) -> Anchor {
    let mut anchor = anchor.unwrap_or(Anchor { wrap: Wrap::Square, ..Anchor::default() });
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let anchor = moved(None, 10.0, 20.0);
        assert_eq!(anchor.wrap, Wrap::Square);
        assert_eq!(anchor.horizontal, Placement::Offset(points_to_emu(10.0)));
        assert_eq!(anchor.vertical, Placement::Offset(points_to_emu(20.0)));
    }

    #[test]
    fn moving_a_drawing_that_was_lined_up_with_an_edge_gives_up_the_alignment() {
        let before =
            Anchor { horizontal: Placement::Aligned("right".to_owned()), ..Anchor::default() };
        let after = moved(Some(before), 5.0, 0.0);
        assert_eq!(after.horizontal, Placement::Offset(points_to_emu(5.0)));
    }

    #[test]
    fn moving_a_drawing_twice_adds_the_distances_up() {
        let once = moved(None, 10.0, 0.0);
        let twice = moved(Some(once), 10.0, 0.0);
        assert_eq!(twice.horizontal, Placement::Offset(points_to_emu(20.0)));
    }
}

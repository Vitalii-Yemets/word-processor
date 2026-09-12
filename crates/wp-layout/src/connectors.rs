//! Word's lines and connectors, and what is drawn at the ends of one.
//!
//! # What a connector is drawn from
//!
//! Its own box, and nothing else. A connector runs from one corner of the box
//! to the opposite one, and which corner is the start is said by the flips on
//! the drawing: a connector drawn up and to the left is the same shape as one
//! drawn down and to the right, mirrored. That is why these need no more than
//! the box to draw, and it is why a connector *joined* to two shapes is a
//! further thing — the join moves the box, and the box is what is drawn. See
//! the roadmap's D22.
//!
//! # The bends
//!
//! An elbow connector goes across, then down, then across again, and the
//! handles say where it turns. The straight one has no bend, and the curved
//! ones are the same turns taken as curves rather than as corners.

use wp_raster::{Path, Point};

use crate::geometry::{Adjusts, Preset};

/// The outline of a line or a connector, or `None` if the preset is not one.
///
/// These are open shapes: what comes back is the line itself and not an area,
/// and it is drawn by laying a band along it. See
/// [`crate::geometry::Preset::is_closed`].
pub(crate) fn path_in(
    preset: Preset,
    adjusts: &Adjusts,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> Option<Path> {
    let mut path = Path::new();
    let point = Point::new;
    let (width, height) = (right - left, bottom - top);

    match preset {
        Preset::Line | Preset::StraightConnector => {
            path.move_to(point(left, top));
            path.line_to(point(right, bottom));
        }
        Preset::BentConnector2 => {
            // One bend: along and then down, which is the corner the two ends
            // make between them.
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(right, bottom));
        }
        Preset::BentConnector3 => {
            // Two bends: out to where the handle says, across, and in again.
            let turn = left + width * adjusts.share(1, 0.5);
            path.move_to(point(left, top));
            path.line_to(point(turn, top));
            path.line_to(point(turn, bottom));
            path.line_to(point(right, bottom));
        }
        Preset::BentConnector4 => {
            let across = left + width * adjusts.share(1, 0.5);
            let down = top + height * adjusts.share(2, 0.5);
            path.move_to(point(left, top));
            path.line_to(point(across, top));
            path.line_to(point(across, down));
            path.line_to(point(right, down));
            path.line_to(point(right, bottom));
        }
        Preset::BentConnector5 => {
            let first = left + width * adjusts.share(1, 0.25);
            let down = top + height * adjusts.share(2, 0.5);
            let second = left + width * adjusts.share(3, 0.75);
            path.move_to(point(left, top));
            path.line_to(point(first, top));
            path.line_to(point(first, down));
            path.line_to(point(second, down));
            path.line_to(point(second, bottom));
            path.line_to(point(right, bottom));
        }
        Preset::CurvedConnector2 => {
            // The same corner as the bent one, turned instead of cornered: the
            // control points sit where the corner was.
            path.move_to(point(left, top));
            path.cubic_to(point(right, top), point(right, top), point(right, bottom));
        }
        Preset::CurvedConnector3 => {
            let turn = left + width * adjusts.share(1, 0.5);
            path.move_to(point(left, top));
            path.cubic_to(point(turn, top), point(turn, bottom), point(right, bottom));
        }
        Preset::CurvedConnector4 => {
            let across = left + width * adjusts.share(1, 0.5);
            let down = top + height * adjusts.share(2, 0.5);
            path.move_to(point(left, top));
            path.cubic_to(point(across, top), point(across, top), point(across, down));
            path.cubic_to(point(across, down), point(right, down), point(right, bottom));
        }
        Preset::CurvedConnector5 => {
            let first = left + width * adjusts.share(1, 0.25);
            let down = top + height * adjusts.share(2, 0.5);
            let second = left + width * adjusts.share(3, 0.75);
            path.move_to(point(left, top));
            path.cubic_to(point(first, top), point(first, top), point(first, down));
            path.cubic_to(point(first, down), point(second, down), point(second, down));
            path.cubic_to(point(second, down), point(second, bottom), point(right, bottom));
        }
        _ => return None,
    }
    Some(path)
}

/// Which end of a line something is drawn at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineTip {
    /// Where the line starts, which is where the format's `headEnd` goes.
    Head,
    /// And where it ends: `tailEnd`.
    Tail,
}

/// What is drawn at one end of a line: the arrowhead, or nothing.
///
/// Which way it points comes from the line itself — the first two places its
/// path names for the head, the last two for the tail — so an arrow on a bent
/// connector points the way that leg of it goes.
#[must_use]
pub fn arrowhead(path: &Path, tip: LineTip, end: wp_docx::lines::LineEnd, weight: f32) -> Path {
    use wp_docx::lines::EndKind;

    let mut head = Path::new();
    if end.is_nothing() {
        return head;
    }
    let places: Vec<Point> = path.points().collect();
    let (at, from) = match tip {
        LineTip::Head => (places.first(), places.get(1)),
        LineTip::Tail => {
            (places.last(), places.len().checked_sub(2).and_then(|back| places.get(back)))
        }
    };
    let (Some(at), Some(from)) = (at, from) else {
        return head;
    };
    // Along the line, towards the end, and across it.
    let (dx, dy) = (at.x - from.x, at.y - from.y);
    let length = dx.hypot(dy);
    if length <= 0.0 {
        return head;
    }
    let (ux, uy) = (dx / length, dy / length);
    let (nx, ny) = (-uy, ux);
    let along = weight * end.length.times();
    let across = weight * end.width.times() / 2.0;
    let back = |distance: f32, side: f32| {
        Point::new(at.x - ux * distance + nx * side, at.y - uy * distance + ny * side)
    };

    match end.kind {
        EndKind::None => {}
        EndKind::Triangle => {
            head.move_to(*at);
            head.line_to(back(along, across));
            head.line_to(back(along, -across));
            head.close();
        }
        EndKind::Stealth => {
            // A triangle with the back of it cut in, which is what makes it a
            // dart rather than a head.
            head.move_to(*at);
            head.line_to(back(along, across));
            head.line_to(back(along * 0.6, 0.0));
            head.line_to(back(along, -across));
            head.close();
        }
        EndKind::Diamond => {
            head.move_to(*at);
            head.line_to(back(along / 2.0, across));
            head.line_to(back(along, 0.0));
            head.line_to(back(along / 2.0, -across));
            head.close();
        }
        EndKind::Oval => {
            // A round end, centred on the point rather than behind it: an oval
            // is a bead on the end of the line and not a head pointing on.
            let (cx, cy) = (at.x, at.y);
            let (rx, ry) = (across, across);
            crate::geometry::ellipse(&mut head, cx, cy, rx, ry);
        }
        EndKind::Arrow => {
            // Two strokes and no inside: the open V. Each is laid down as a
            // band of the line's own width, the way the line itself is.
            for side in [across, -across] {
                let corner = back(along, side);
                let (bx, by) = (corner.x - at.x, corner.y - at.y);
                let span = bx.hypot(by).max(0.001);
                let (px, py) = (-by / span * weight / 2.0, bx / span * weight / 2.0);
                head.move_to(Point::new(at.x + px, at.y + py));
                head.line_to(Point::new(corner.x + px, corner.y + py));
                head.line_to(Point::new(corner.x - px, corner.y - py));
                head.line_to(Point::new(at.x - px, at.y - py));
                head.close();
            }
        }
    }
    head
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{self, Adjusts};
    use wp_docx::lines::{EndKind, EndSize, LineEnd};

    /// What is drawn at one end of a shape, at the size the format falls back
    /// on and on a line two thick.
    fn end_of(preset: Preset, tip: LineTip, kind: EndKind) -> Path {
        let path = geometry::path_in(preset, &Adjusts::NONE, 0.0, 0.0, 100.0, 60.0);
        arrowhead(&path, tip, LineEnd { kind, ..LineEnd::default() }, 2.0)
    }

    #[test]
    fn a_line_with_nothing_at_its_end_draws_nothing_there() {
        assert!(end_of(Preset::StraightConnector, LineTip::Tail, EndKind::None).is_empty());
    }

    #[test]
    fn an_arrowhead_stands_at_the_end_of_the_line_it_belongs_to() {
        // The tail of a straight connector is the far corner of its box and
        // the head is the near one, because that is the way it is drawn.
        let tail = end_of(Preset::StraightConnector, LineTip::Tail, EndKind::Triangle);
        let at = tail.points().next().expect("the point of it");
        assert!((at.x - 100.0).abs() < 0.01 && (at.y - 60.0).abs() < 0.01, "the tail is at {at:?}");

        let head = end_of(Preset::StraightConnector, LineTip::Head, EndKind::Triangle);
        let at = head.points().next().expect("the point of it");
        assert!(at.x.abs() < 0.01 && at.y.abs() < 0.01, "the head is at {at:?}");
    }

    #[test]
    fn an_arrowhead_points_the_way_its_own_leg_goes() {
        // The last leg of this connector goes down, so the head on it points
        // down: everything behind the point is above it. An arrowhead worked
        // out from the box rather than from the line would point down and to
        // the right, which is where no part of this connector goes.
        let head = end_of(Preset::BentConnector2, LineTip::Tail, EndKind::Triangle);
        let places: Vec<Point> = head.points().collect();
        let tip = places[0];
        assert!(
            places[1..].iter().all(|at| at.y < tip.y - 0.5),
            "the head does not point down: {places:?}"
        );
    }

    #[test]
    fn every_kind_of_end_draws_something() {
        for kind in
            [EndKind::Triangle, EndKind::Stealth, EndKind::Diamond, EndKind::Oval, EndKind::Arrow]
        {
            assert!(
                !end_of(Preset::StraightConnector, LineTip::Tail, kind).is_empty(),
                "{kind:?} draws nothing"
            );
        }
    }

    #[test]
    fn a_head_asked_to_be_bigger_is_bigger() {
        // Both of the sizes are reckoned from the width of the line, so a
        // thick line gets a big arrowhead without anybody asking for one.
        let path =
            geometry::path_in(Preset::StraightConnector, &Adjusts::NONE, 0.0, 0.0, 100.0, 60.0);
        let reach = |width: EndSize, length: EndSize| {
            let end = LineEnd { kind: EndKind::Triangle, width, length };
            let head = arrowhead(&path, LineTip::Tail, end, 2.0);
            let far = head
                .points()
                .fold((f32::MAX, f32::MIN), |(low, high), at| (low.min(at.x), high.max(at.x)));
            far.1 - far.0
        };
        assert!(
            reach(EndSize::Small, EndSize::Small) < reach(EndSize::Large, EndSize::Large),
            "the sizes make no difference"
        );
    }
}

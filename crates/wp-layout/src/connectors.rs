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

/// Where a shape offers to be joined, counted the way the format counts them.
///
/// # Which point is which
///
/// The first four of nearly every preset are the top, the left, the bottom and
/// the right, in that order, and that is what this answers for every shape. A
/// few presets offer more than four — a triangle offers its corners as well —
/// and an index this program does not know is taken to the middle of the shape:
/// a connector joined to the middle of the thing it names is still joined to
/// it, which is nearer the truth than a connector left where it was drawn.
#[must_use]
pub fn connection_site(index: u32, x: f32, y: f32, width: f32, height: f32) -> Point {
    let (mx, my) = (x + width / 2.0, y + height / 2.0);
    match index {
        0 => Point::new(mx, y),
        1 => Point::new(x, my),
        2 => Point::new(mx, y + height),
        3 => Point::new(x + width, my),
        _ => Point::new(mx, my),
    }
}

/// Which way a connector leaves a shape at one of its connection points.
///
/// Outwards, always: a connector fastened to the right of a shape leaves by the
/// right, whatever is on the right. That is the whole of what keeps a route
/// from setting off through the shape it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    Up,
    Left,
    Down,
    Right,
}

impl Facing {
    /// Which way the connection point of that index faces, in the order the
    /// format counts them. See [`connection_site`].
    #[must_use]
    pub fn of_site(index: u32) -> Self {
        match index {
            0 => Self::Up,
            1 => Self::Left,
            2 => Self::Down,
            _ => Self::Right,
        }
    }

    /// The same facing with the two measures swapped, for working a route out
    /// on its side.
    fn turned(self) -> Self {
        match self {
            Self::Up => Self::Left,
            Self::Left => Self::Up,
            Self::Down => Self::Right,
            Self::Right => Self::Down,
        }
    }

    fn is_upright(self) -> bool {
        matches!(self, Self::Up | Self::Down)
    }
}

/// One end of a route: where it is fastened, which way it leaves, and the box
/// it has to keep out of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub at: Point,
    pub faces: Facing,
    /// The shape's box, as left, top, right and bottom.
    pub shape: (f32, f32, f32, f32),
}

impl Place {
    fn turned(self) -> Self {
        let (left, top, right, bottom) = self.shape;
        Self {
            at: Point::new(self.at.y, self.at.x),
            faces: self.faces.turned(),
            shape: (top, left, bottom, right),
        }
    }
}

/// How a connector gets from one shape to the other.
///
/// # What it is for
///
/// A connector fastened to the right of one shape and the left of another
/// standing to its left has nowhere sensible to go in a straight elbow: the
/// line would leave the first shape and turn straight back through it. This
/// lays the legs out so that each end leaves by the side it is fastened to and
/// neither shape is crossed.
///
/// # How the cases are halved
///
/// A route that leaves the start upwards is the same route with the two
/// measures swapped, so it is worked out on its side and turned back at the
/// end. What is left is a start that leaves sideways, which is three cases:
/// the two ends facing each other with room between them, an end that is
/// entered from above or below, and everything else — which goes round by a
/// lane clear of both shapes.
#[must_use]
pub fn route(from: Place, to: Place, stand_off: f32) -> Vec<Point> {
    if from.faces.is_upright() {
        let turned = route(from.turned(), to.turned(), stand_off);
        return turned.into_iter().map(|at| Point::new(at.y, at.x)).collect();
    }

    let out = if from.faces == Facing::Right { stand_off } else { -stand_off };
    let first = Point::new(from.at.x + out, from.at.y);
    let last = match to.faces {
        Facing::Right => Point::new(to.at.x + stand_off, to.at.y),
        Facing::Left => Point::new(to.at.x - stand_off, to.at.y),
        Facing::Down => Point::new(to.at.x, to.at.y + stand_off),
        Facing::Up => Point::new(to.at.x, to.at.y - stand_off),
    };
    let boxes = [from.shape, to.shape];

    // The two facing each other with room between them: out, across, in.
    if !to.faces.is_upright() {
        let facing_each_other = (out > 0.0 && to.faces == Facing::Left && first.x <= last.x)
            || (out < 0.0 && to.faces == Facing::Right && first.x >= last.x);
        if facing_each_other {
            let middle = (first.x + last.x) / 2.0;
            let places =
                vec![from.at, Point::new(middle, from.at.y), Point::new(middle, to.at.y), to.at];
            if clear_of(&places, &boxes) {
                return tidied(places);
            }
        }
    } else {
        // An end entered from above or below: out, along, and down into it.
        let places = vec![from.at, first, Point::new(last.x, first.y), last, to.at];
        let far_enough = (out > 0.0 && last.x >= first.x) || (out < 0.0 && last.x <= first.x);
        if far_enough && clear_of(&places, &boxes) {
            return tidied(places);
        }
    }

    // And the way round: out of both shapes, along a lane that is clear of
    // them, and back in. Above them or below them, whichever is nearer to the
    // two ends.
    let (above, below) =
        (from.shape.1.min(to.shape.1) - stand_off, from.shape.3.max(to.shape.3) + stand_off);
    let middle = (from.at.y + to.at.y) / 2.0;
    let lane = if (middle - above).abs() <= (middle - below).abs() { above } else { below };
    let lane =
        if to.faces.is_upright() && (to.at.y - lane).abs() < stand_off { middle } else { lane };

    tidied(vec![from.at, first, Point::new(first.x, lane), Point::new(last.x, lane), last, to.at])
}

/// Whether a route keeps out of the boxes at either end of it.
///
/// The ends themselves sit on the edge of their own shapes, so the first and
/// last steps of any route touch one: what is asked is whether a leg runs
/// *through* a shape, which is what a route round them must not do.
fn clear_of(places: &[Point], boxes: &[(f32, f32, f32, f32); 2]) -> bool {
    const CLOSE: f32 = 0.5;
    for pair in places.windows(2) {
        let (one, two) = (pair[0], pair[1]);
        for &(left, top, right, bottom) in boxes {
            let (low_x, high_x) = (one.x.min(two.x), one.x.max(two.x));
            let (low_y, high_y) = (one.y.min(two.y), one.y.max(two.y));
            let across = low_x < right - CLOSE && high_x > left + CLOSE;
            let down = low_y < bottom - CLOSE && high_y > top + CLOSE;
            if across && down {
                return false;
            }
        }
    }
    true
}

/// A route with the steps that say nothing taken out: a place the same as the
/// one before it, and a place in the middle of two others in a straight line
/// with them.
fn tidied(places: Vec<Point>) -> Vec<Point> {
    const CLOSE: f32 = 0.01;
    let mut out: Vec<Point> = Vec::with_capacity(places.len());
    for at in places {
        if out.last().is_some_and(|last: &Point| {
            (last.x - at.x).abs() < CLOSE && (last.y - at.y).abs() < CLOSE
        }) {
            continue;
        }
        if out.len() >= 2 {
            let (one, two) = (out[out.len() - 2], out[out.len() - 1]);
            let straight = ((one.x - two.x).abs() < CLOSE && (two.x - at.x).abs() < CLOSE)
                || ((one.y - two.y).abs() < CLOSE && (two.y - at.y).abs() < CLOSE);
            if straight {
                out.pop();
            }
        }
        out.push(at);
    }
    out
}

/// The path a route is drawn as.
///
/// A curved connector takes the same route with its corners turned rather than
/// cornered, which is the whole difference between the two families.
#[must_use]
pub fn route_path(places: &[Point], curved: bool) -> Path {
    let mut path = Path::new();
    let Some(first) = places.first() else {
        return path;
    };
    path.move_to(*first);
    if !curved || places.len() < 3 {
        for at in places.iter().skip(1) {
            path.line_to(*at);
        }
        return path;
    }

    // How far back from a corner the turn starts: a quarter of the shorter of
    // the two legs that meet there, so a short leg is not turned away
    // altogether.
    for index in 1..places.len() - 1 {
        let (before, corner, after) = (places[index - 1], places[index], places[index + 1]);
        let back = (before.x - corner.x).hypot(before.y - corner.y);
        let on = (after.x - corner.x).hypot(after.y - corner.y);
        let reach = (back.min(on) / 4.0).max(0.01);
        let towards = |other: Point, length: f32| {
            let (dx, dy) = (other.x - corner.x, other.y - corner.y);
            let span = dx.hypot(dy).max(0.001);
            Point::new(corner.x + dx / span * length, corner.y + dy / span * length)
        };
        path.line_to(towards(before, reach));
        path.quad_to(corner, towards(after, reach));
    }
    path.line_to(*places.last().expect("a route with a last place"));
    path
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
    #[test]
    fn the_first_four_connection_points_are_the_four_sides() {
        // The order the format counts them in: the top, the left, the bottom,
        // the right. A connector fastened to the second point of a box and
        // drawn to the fourth would cross the box itself if the order here
        // were wrong.
        let at = |index| connection_site(index, 10.0, 20.0, 100.0, 60.0);
        assert_eq!(at(0), Point::new(60.0, 20.0), "the top");
        assert_eq!(at(1), Point::new(10.0, 50.0), "the left");
        assert_eq!(at(2), Point::new(60.0, 80.0), "the bottom");
        assert_eq!(at(3), Point::new(110.0, 50.0), "the right");
    }

    #[test]
    fn a_connection_point_nobody_knows_is_the_middle_of_the_shape() {
        // Which keeps the connector joined to the thing it names, and is nearer
        // the truth than leaving it where it was drawn.
        assert_eq!(connection_site(9, 10.0, 20.0, 100.0, 60.0), Point::new(60.0, 50.0));
    }
    /// A box, as the router is given one.
    const fn shape(left: f32, top: f32, right: f32, bottom: f32) -> (f32, f32, f32, f32) {
        (left, top, right, bottom)
    }

    /// Whether any leg of a route runs through a box rather than beside it.
    fn runs_through(places: &[Point], (left, top, right, bottom): (f32, f32, f32, f32)) -> bool {
        places.windows(2).any(|pair| {
            let (one, two) = (pair[0], pair[1]);
            let across = one.x.min(two.x) < right - 0.5 && one.x.max(two.x) > left + 0.5;
            let down = one.y.min(two.y) < bottom - 0.5 && one.y.max(two.y) > top + 0.5;
            across && down
        })
    }

    #[test]
    fn two_shapes_facing_each_other_are_joined_straight_across() {
        // Nothing is in the way, so nothing is gone round: the shortest route
        // is the right one.
        let first = shape(0.0, 20.0, 100.0, 80.0);
        let second = shape(200.0, 20.0, 300.0, 80.0);
        let places = route(
            Place { at: Point::new(100.0, 50.0), faces: Facing::Right, shape: first },
            Place { at: Point::new(200.0, 50.0), faces: Facing::Left, shape: second },
            10.0,
        );
        assert_eq!(places, vec![Point::new(100.0, 50.0), Point::new(200.0, 50.0)]);
    }

    #[test]
    fn a_shape_standing_to_the_left_is_gone_round_rather_than_through() {
        // The case the routing is for: fastened to the right of one shape and
        // the left of another that stands to its left. A connector drawn from
        // one corner of the box between them to the other would set off through
        // the shape it came out of.
        let first = shape(200.0, 20.0, 300.0, 80.0);
        let second = shape(0.0, 120.0, 100.0, 180.0);
        let places = route(
            Place { at: Point::new(300.0, 50.0), faces: Facing::Right, shape: first },
            Place { at: Point::new(0.0, 150.0), faces: Facing::Left, shape: second },
            10.0,
        );

        assert_eq!(
            places.first(),
            Some(&Point::new(300.0, 50.0)),
            "it starts where it is fastened"
        );
        assert_eq!(places.last(), Some(&Point::new(0.0, 150.0)), "and ends where it is fastened");
        assert!(places[1].x > 300.0, "it should leave by the right: {places:?}");
        assert!(!runs_through(&places, first), "it goes through the shape it came out of");
        assert!(!runs_through(&places, second), "it goes through the shape it goes to");
    }

    #[test]
    fn a_connector_fastened_underneath_leaves_downwards() {
        // The flowchart case: the bottom of one box to the top of the next.
        let first = shape(0.0, 0.0, 100.0, 60.0);
        let second = shape(200.0, 200.0, 300.0, 260.0);
        let places = route(
            Place { at: Point::new(50.0, 60.0), faces: Facing::Down, shape: first },
            Place { at: Point::new(250.0, 200.0), faces: Facing::Up, shape: second },
            10.0,
        );

        assert!(places[1].y > 60.0, "it should leave downwards: {places:?}");
        assert_eq!(places.last(), Some(&Point::new(250.0, 200.0)));
        assert!(!runs_through(&places, first) && !runs_through(&places, second));
    }

    #[test]
    fn a_route_is_drawn_with_corners_or_with_curves() {
        use wp_raster::Command;

        let places = vec![Point::new(0.0, 0.0), Point::new(50.0, 0.0), Point::new(50.0, 50.0)];
        let cornered = route_path(&places, false);
        let curved = route_path(&places, true);

        assert!(
            cornered.commands.iter().all(|step| !matches!(step, Command::QuadTo(..))),
            "an elbow is drawn with corners"
        );
        assert!(
            curved.commands.iter().any(|step| matches!(step, Command::QuadTo(..))),
            "and a curved connector with curves"
        );
    }
}

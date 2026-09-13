//! The block arrows of Word's gallery.
//!
//! # Why they are here and not with the other shapes
//!
//! Because there are twenty-eight of them and they are nearly all one shape.
//! An arrow is a shaft with a head on it; a double arrow is a shaft with a head
//! at each end; the quad arrow has four. Written out one by one they would be
//! twenty-eight lists of points with nothing to say for themselves, and the
//! thing that actually differs between them — which sides the heads are on —
//! would be buried. So the heads are the argument, and the outline is worked
//! out once.
//!
//! The bent ones and the curved ones are their own shapes, and are here for
//! company: an elbow arrow and a bent arrow are the same idea with a different
//! path, and looking for them anywhere else would be looking in the wrong file.
//!
//! # What decides the proportions
//!
//! Word's own adjustments, as its defaults leave them. The shaft is half the
//! shorter side of the box, and a head is half the shorter side long — so a
//! long arrow has a head the size of a short one, which is what makes a row of
//! arrows of different lengths look like a row of arrows. Word states these as
//! `a:gd` formulas over `ss`, the shorter side; the numbers below are those
//! formulas at their default adjustments rather than the formulas themselves.

use wp_raster::{Path, Point};

use crate::geometry::{arc_into, fallback, share_of, Adjusts, Preset};

/// An angle in degrees, as [`arc_into`] wants it.
///
/// The shapes below are described in degrees because that is how anybody thinks
/// about a quarter turn; the arcs are drawn in radians because that is what the
/// arithmetic wants.
fn turn(degrees: f32) -> f32 {
    degrees.to_radians()
}

/// Which way an arrow's head points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Right,
    Left,
    Up,
    Down,
}

/// The outline of one of the block arrows, if the preset is one.
///
/// `None` for anything else, which is how the caller knows to look elsewhere.
pub(crate) fn path_in(
    preset: Preset,
    adjusts: &Adjusts,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> Option<Path> {
    let mut path = Path::new();
    let box_ = Box_ { left, top, right, bottom };

    match preset {
        Preset::Arrow => straight(&mut path, box_, adjusts, &[Side::Right]),
        Preset::LeftArrow => straight(&mut path, box_, adjusts, &[Side::Left]),
        Preset::UpArrow => straight(&mut path, box_, adjusts, &[Side::Up]),
        Preset::DownArrow => straight(&mut path, box_, adjusts, &[Side::Down]),
        Preset::LeftRightArrow => straight(&mut path, box_, adjusts, &[Side::Left, Side::Right]),
        Preset::UpDownArrow => straight(&mut path, box_, adjusts, &[Side::Up, Side::Down]),
        Preset::QuadArrow => {
            straight(&mut path, box_, adjusts, &[Side::Left, Side::Right, Side::Up, Side::Down]);
        }
        Preset::LeftRightUpArrow => {
            straight(&mut path, box_, adjusts, &[Side::Left, Side::Right, Side::Up]);
        }
        Preset::NotchedArrow => notched(&mut path, box_),
        Preset::StripedArrow => striped(&mut path, box_),
        Preset::HomePlate => home_plate(&mut path, box_),
        Preset::Chevron => chevron(&mut path, box_),
        Preset::BentArrow => elbow(&mut path, box_, true, false),
        Preset::BentUpArrow => elbow(&mut path, box_, false, false),
        Preset::LeftUpArrow => elbow(&mut path, box_, false, true),
        Preset::UturnArrow => uturn(&mut path, box_),
        Preset::CurvedRightArrow => curved(&mut path, box_, Side::Right),
        Preset::CurvedLeftArrow => curved(&mut path, box_, Side::Left),
        Preset::CurvedUpArrow => curved(&mut path, box_, Side::Up),
        Preset::CurvedDownArrow => curved(&mut path, box_, Side::Down),
        Preset::RightArrowCallout => callout(&mut path, box_, &[Side::Right]),
        Preset::LeftArrowCallout => callout(&mut path, box_, &[Side::Left]),
        Preset::UpArrowCallout => callout(&mut path, box_, &[Side::Up]),
        Preset::DownArrowCallout => callout(&mut path, box_, &[Side::Down]),
        Preset::LeftRightArrowCallout => callout(&mut path, box_, &[Side::Left, Side::Right]),
        Preset::QuadArrowCallout => {
            callout(&mut path, box_, &[Side::Left, Side::Right, Side::Up, Side::Down]);
        }
        Preset::CircularArrow => circular(&mut path, box_),
        Preset::SwooshArrow => swoosh(&mut path, box_),
        _ => return None,
    }
    Some(path)
}

/// The box an arrow is drawn in.
#[derive(Clone, Copy, Debug)]
struct Box_ {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Box_ {
    fn width(self) -> f32 {
        self.right - self.left
    }

    fn height(self) -> f32 {
        self.bottom - self.top
    }

    /// The shorter side, which every proportion here is a share of. Word calls
    /// it `ss` and states its own arrows in terms of it.
    fn shorter(self) -> f32 {
        self.width().min(self.height())
    }

    fn middle_x(self) -> f32 {
        (self.left + self.right) / 2.0
    }

    fn middle_y(self) -> f32 {
        (self.top + self.bottom) / 2.0
    }
}

/// A straight arrow with heads on the sides given.
///
/// # Why there are two shapes here and not one
///
/// An arrow along one axis is a bar with a head on one or both ends. An arrow
/// along both is a cross with a head on each side that asked for one. They
/// share nothing but the proportions: writing the cross and leaving out the
/// arms would give an up arrow a bar across its middle, which is what it had
/// until this was split in two.
fn straight(path: &mut Path, box_: Box_, adjusts: &Adjusts, heads: &[Side]) {
    let across = heads.iter().any(|side| matches!(side, Side::Left | Side::Right));
    let down = heads.iter().any(|side| matches!(side, Side::Up | Side::Down));
    match (across, down) {
        (true, true) => cross(path, box_, adjusts, heads),
        (false, true) => along(path, box_, adjusts, heads, false),
        // No heads at all is a bar across, which is what a shaft on its own
        // looks like; nothing asks for one, and it is better than nothing.
        _ => along(path, box_, adjusts, heads, true),
    }
}

/// An arrow along one axis: a bar with a head on one end or on both.
fn along(path: &mut Path, box_: Box_, adjusts: &Adjusts, heads: &[Side], horizontal: bool) {
    let point = Point::new;
    let has = |side: Side| heads.contains(&side);
    // Word states these over `ss`, the shorter side of the box, and not over
    // the side the arrow lies across: an arrow along the long side of a wide
    // box has the same shaft and the same head as one in a square box, which
    // is what stops a wide double arrow coming out as a diamond. The head's
    // base is the other thing, and that does span the box.
    let (length, reach) =
        if horizontal { (box_.width(), box_.height()) } else { (box_.height(), box_.width()) };
    let shorter = box_.shorter();
    let heads_on = usize::from(has(Side::Left) || has(Side::Up))
        + usize::from(has(Side::Right) || has(Side::Down));
    // The head as long as the second handle says and the shaft as thick as the
    // first, both over `ss`; half the shorter side each when nothing says
    // otherwise, which is what the format falls back on. Two heads that will
    // not both fit are cut to what there is room for, and that is what makes a
    // two-headed arrow in a square box come out as a diamond.
    let head =
        (shorter * adjusts.share(2, share_of(fallback::HALF))).min(length / heads_on.max(1) as f32);
    let half = shorter * adjusts.share(1, share_of(fallback::HALF)) / 2.0;

    // Along the arrow: where the shaft starts and ends. Across it: the edges of
    // the shaft, and the edges of the box, which a head's base stands on.
    let (from, to) = (0.0, length);
    let start = if has(Side::Left) || has(Side::Up) { head } else { from };
    let finish = if has(Side::Right) || has(Side::Down) { to - head } else { to };
    let middle = reach / 2.0;

    // One walk in the arrow's own frame, laid down whichever way round it goes:
    // along the arrow and across it, rather than in x and y.
    let mut put = |along: f32, across: f32, first: bool| {
        let at = if horizontal {
            point(box_.left + along, box_.top + across)
        } else {
            point(box_.left + across, box_.top + along)
        };
        if first {
            path.move_to(at);
        } else {
            path.line_to(at);
        }
    };

    put(start, middle - half, true);
    put(finish, middle - half, false);
    if has(Side::Right) || has(Side::Down) {
        put(finish, 0.0, false);
        put(to, middle, false);
        put(finish, reach, false);
    }
    put(finish, middle + half, false);
    put(start, middle + half, false);
    if has(Side::Left) || has(Side::Up) {
        put(start, reach, false);
        put(from, middle, false);
        put(start, 0.0, false);
    }
    path.close();
}

/// An arrow along both axes: a cross with a head on each side that asked for
/// one.
///
/// The walk goes clockwise from the shaft's top left corner, and at each side
/// either turns the corner or goes out to the point and back. A side with no
/// head contributes its corner and no more, which is how one walk serves the
/// quad arrow and the three-headed one alike.
fn cross(path: &mut Path, box_: Box_, adjusts: &Adjusts, heads: &[Side]) {
    let has = |side: Side| heads.contains(&side);
    let shorter = box_.shorter();

    // Thinner than a single-axis arrow: four heads on a half-thickness shaft
    // would leave nothing between them.
    let thick = shorter * adjusts.share(1, 0.23);
    let half = thick / 2.0;
    let reach = shorter * adjusts.share(3, 0.25);
    let across_reach = reach.min(box_.width() / 2.0);
    let down_reach = reach.min(box_.height() / 2.0);
    let base = shorter * 0.25;

    let shaft_left = if has(Side::Left) { box_.left + across_reach } else { box_.left };
    let shaft_right = if has(Side::Right) { box_.right - across_reach } else { box_.right };
    let shaft_top = if has(Side::Up) { box_.top + down_reach } else { box_.top };
    let shaft_bottom = if has(Side::Down) { box_.bottom - down_reach } else { box_.bottom };

    let (mx, my) = (box_.middle_x(), box_.middle_y());
    let point = Point::new;

    path.move_to(point(shaft_left, my - half));
    if has(Side::Up) {
        path.line_to(point(mx - half, my - half));
        path.line_to(point(mx - base, shaft_top));
        path.line_to(point(mx, box_.top));
        path.line_to(point(mx + base, shaft_top));
        path.line_to(point(mx + half, my - half));
    }
    path.line_to(point(shaft_right, my - half));
    if has(Side::Right) {
        path.line_to(point(shaft_right, my - base));
        path.line_to(point(box_.right, my));
        path.line_to(point(shaft_right, my + base));
    }
    path.line_to(point(shaft_right, my + half));
    if has(Side::Down) {
        path.line_to(point(mx + half, my + half));
        path.line_to(point(mx + base, shaft_bottom));
        path.line_to(point(mx, box_.bottom));
        path.line_to(point(mx - base, shaft_bottom));
        path.line_to(point(mx - half, my + half));
    }
    path.line_to(point(shaft_left, my + half));
    if has(Side::Left) {
        path.line_to(point(shaft_left, my + base));
        path.line_to(point(box_.left, my));
        path.line_to(point(shaft_left, my - base));
    }
    path.close();
}

/// A right arrow with a notch cut into its tail.
fn notched(path: &mut Path, box_: Box_) {
    let shorter = box_.shorter();
    let head = (shorter * 0.5).min(box_.width());
    let neck = box_.right - head;
    let notch = (shorter * 0.5).min(box_.width() - head);
    let (top, bottom) = (box_.middle_y() - shorter * 0.25, box_.middle_y() + shorter * 0.25);
    let point = Point::new;

    path.move_to(point(box_.left, top));
    path.line_to(point(neck, top));
    path.line_to(point(neck, box_.top));
    path.line_to(point(box_.right, box_.middle_y()));
    path.line_to(point(neck, box_.bottom));
    path.line_to(point(neck, bottom));
    path.line_to(point(box_.left, bottom));
    // The notch, which is what makes it notched: the tail is cut in to the
    // same depth as the head is long.
    path.line_to(point(box_.left + notch, box_.middle_y()));
    path.close();
}

/// A right arrow whose tail is a row of bars rather than a shaft.
///
/// Three pieces rather than one, which is what the shape is: filled by the
/// nonzero rule they come out as three, and that is how Word draws it too.
fn striped(path: &mut Path, box_: Box_) {
    let shorter = box_.shorter();
    let head = (shorter * 0.5).min(box_.width());
    let neck = box_.right - head;
    let (top, bottom) = (box_.middle_y() - shorter * 0.25, box_.middle_y() + shorter * 0.25);
    let point = Point::new;

    // The stripes take the first fifth of the shaft: a narrow bar, a gap, a
    // wider one, a gap, and then the shaft proper.
    let room = (neck - box_.left).max(1.0);
    let bar = room * 0.08;
    let gap = room * 0.06;

    let mut x = box_.left;
    for width in [bar, bar * 2.0] {
        path.move_to(point(x, top));
        path.line_to(point(x + width, top));
        path.line_to(point(x + width, bottom));
        path.line_to(point(x, bottom));
        path.close();
        x += width + gap;
    }

    path.move_to(point(x, top));
    path.line_to(point(neck, top));
    path.line_to(point(neck, box_.top));
    path.line_to(point(box_.right, box_.middle_y()));
    path.line_to(point(neck, box_.bottom));
    path.line_to(point(neck, bottom));
    path.line_to(point(x, bottom));
    path.close();
}

/// The five-sided arrow Word calls a pentagon and everyone else calls a tag.
fn home_plate(path: &mut Path, box_: Box_) {
    let cut = (box_.height() / 2.0).min(box_.width());
    let point = Point::new;
    path.move_to(point(box_.left, box_.top));
    path.line_to(point(box_.right - cut, box_.top));
    path.line_to(point(box_.right, box_.middle_y()));
    path.line_to(point(box_.right - cut, box_.bottom));
    path.line_to(point(box_.left, box_.bottom));
    path.close();
}

/// The same with the back cut in as well, which is Word's chevron.
fn chevron(path: &mut Path, box_: Box_) {
    let cut = (box_.height() / 2.0).min(box_.width() / 2.0);
    let point = Point::new;
    path.move_to(point(box_.left, box_.top));
    path.line_to(point(box_.right - cut, box_.top));
    path.line_to(point(box_.right, box_.middle_y()));
    path.line_to(point(box_.right - cut, box_.bottom));
    path.line_to(point(box_.left, box_.bottom));
    path.line_to(point(box_.left + cut, box_.middle_y()));
    path.close();
}

/// An elbow: an arm along the bottom and an arm up the right-hand side.
///
/// Word has three of these and they differ in two answers: whether the corner
/// is turned square or round, and whether the far end of the bottom arm has a
/// head of its own. The bent arrow is round-cornered with one head; the bent-up
/// arrow square-cornered with one; the left-up arrow square-cornered with two.
fn elbow(path: &mut Path, box_: Box_, rounded: bool, head_left: bool) {
    let point = Point::new;
    let shorter = box_.shorter();
    let thick = shorter * 0.22;
    let head = shorter * 0.4;
    let base = thick * 2.0;

    // The vertical arm, set in far enough that its head's base fits the box.
    let stem = box_.right - base / 2.0;
    // The bottom arm, set up far enough that its own head's base fits too.
    let along = if head_left { box_.bottom - base / 2.0 } else { box_.bottom - thick / 2.0 };
    let (inner_x, outer_x) = (stem - thick / 2.0, stem + thick / 2.0);
    let (inner_y, outer_y) = (along - thick / 2.0, along + thick / 2.0);
    // How far the round corner reaches, which is nothing at all when the
    // corner is square.
    let turn_by = if rounded { (thick * 1.6).min((inner_x - box_.left).max(0.0)) } else { 0.0 };
    let tail = box_.left + if head_left { head } else { 0.0 };

    // Along the inside of the bottom arm, up the inside of the stem, across the
    // head at the top, and back down the outside.
    path.move_to(point(tail, inner_y));
    if rounded {
        path.line_to(point(inner_x - turn_by, inner_y));
        path.quad_to(point(inner_x, inner_y), point(inner_x, inner_y - turn_by));
    } else {
        path.line_to(point(inner_x, inner_y));
    }
    path.line_to(point(inner_x, box_.top + head));
    path.line_to(point(stem - base / 2.0, box_.top + head));
    path.line_to(point(stem, box_.top));
    path.line_to(point(stem + base / 2.0, box_.top + head));
    path.line_to(point(outer_x, box_.top + head));
    path.line_to(point(outer_x, outer_y));
    path.line_to(point(tail, outer_y));
    if head_left {
        // The second head, on the far end of the bottom arm.
        path.line_to(point(tail, box_.bottom));
        path.line_to(point(box_.left, along));
        path.line_to(point(tail, box_.bottom - base));
    }
    path.close();
}

/// An arrow that goes up, turns over and comes back down, with the head at the
/// bottom of the second leg.
fn uturn(path: &mut Path, box_: Box_) {
    let point = Point::new;
    let shorter = box_.shorter();
    let thick = shorter * 0.22;
    let head = shorter * 0.35;
    let base = thick * 2.0;

    // The two legs, and the bend that joins them over the top.
    let left_leg = box_.left + thick / 2.0;
    let right_leg = box_.right - base / 2.0;
    let bend = (left_leg + right_leg) / 2.0;
    let rx = (right_leg - left_leg) / 2.0;
    let ry = rx.min(box_.height() / 3.0);
    let top = box_.top + ry + thick / 2.0;

    path.move_to(point(left_leg - thick / 2.0, box_.bottom));
    path.line_to(point(left_leg - thick / 2.0, top));
    // Over the top: the outer edge of the bend, left to right.
    arc_into(path, bend, top, rx + thick / 2.0, ry + thick / 2.0, turn(180.0), turn(360.0));
    // Down the second leg to the head.
    path.line_to(point(right_leg + thick / 2.0, box_.bottom - head));
    path.line_to(point(right_leg + base / 2.0, box_.bottom - head));
    path.line_to(point(right_leg, box_.bottom));
    path.line_to(point(right_leg - base / 2.0, box_.bottom - head));
    path.line_to(point(right_leg - thick / 2.0, box_.bottom - head));
    path.line_to(point(right_leg - thick / 2.0, top));
    // And back along the inside of the bend, right to left.
    arc_into(
        path,
        bend,
        top,
        (rx - thick / 2.0).max(0.5),
        (ry - thick / 2.0).max(0.5),
        turn(360.0),
        turn(180.0),
    );
    path.line_to(point(left_leg + thick / 2.0, box_.bottom));
    path.close();
}

/// An arrow bent round a quarter of a circle.
///
/// # Which quarter, and which way round it
///
/// By where the band has to end: an arrow pointing right ends where the curve
/// is going right, and on an ellipse there is exactly one such point in each
/// quarter. So the quarter is chosen by its last tangent rather than by a
/// picture of the shape, and the head is built from that tangent, which is the
/// same arithmetic for all four and for the circular arrow below.
fn curved(path: &mut Path, box_: Box_, towards: Side) {
    let shorter = box_.shorter();
    let thick = shorter * 0.16;
    let head = shorter * 0.28;

    // The corner the quarter turns about, its two radii, and the angles it
    // sweeps between. Each is set so that the point of the head lands on the
    // edge the arrow aims at, and the far shoulder of the head on the edge
    // beside it: a band that reached the edge itself would leave the head
    // hanging outside the box.
    let (cx, cy, rx, ry, from, to) = match towards {
        Side::Right => (
            box_.right - head,
            box_.bottom,
            box_.width() - head,
            box_.height() - thick / 2.0,
            180.0,
            270.0,
        ),
        Side::Left => (
            box_.left + head,
            box_.top,
            box_.width() - head,
            box_.height() - thick / 2.0,
            0.0,
            90.0,
        ),
        Side::Up => (
            box_.right,
            box_.top + head,
            box_.width() - thick / 2.0,
            box_.height() - head,
            90.0,
            180.0,
        ),
        Side::Down => (
            box_.left,
            box_.bottom - head,
            box_.width() - thick / 2.0,
            box_.height() - head,
            270.0,
            360.0,
        ),
    };
    let (rx, ry) = (rx.max(1.0), ry.max(1.0));
    let thick = thick.min(rx.min(ry) / 2.0);

    arc_into(path, cx, cy, rx, ry, turn(from), turn(to));
    head_across(path, cx, cy, rx, ry, to, thick, head);
    arc_into(path, cx, cy, rx - thick, ry - thick, turn(to), turn(from));
    path.close();
}

/// The head at the end of a curved band, built from the tangent there.
///
/// The path is standing at the outer end of the band; this takes it out to the
/// point and back to the inner end, leaving a triangle standing across the band
/// and pointing on round the curve.
#[allow(clippy::too_many_arguments, reason = "an arc has a centre, two radii and an angle")]
fn head_across(
    path: &mut Path,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    at: f32,
    thick: f32,
    head: f32,
) {
    let point = Point::new;
    let angle = turn(at);
    let (cos, sin) = (angle.cos(), angle.sin());
    // The tangent of an ellipse, taken as a direction rather than as a speed.
    let (tx, ty) = (-rx * sin, ry * cos);
    let length = (tx * tx + ty * ty).sqrt().max(0.001);
    let (tx, ty) = (tx / length, ty / length);

    let outer = point(cx + rx * cos, cy + ry * sin);
    let inner = point(cx + (rx - thick) * cos, cy + (ry - thick) * sin);
    let (out_x, out_y) = (outer.x - inner.x, outer.y - inner.y);
    // A shoulder each side of the band, and the point ahead of both.
    path.line_to(point(outer.x + out_x * 0.5, outer.y + out_y * 0.5));
    path.line_to(point(
        (outer.x + inner.x) / 2.0 + tx * head,
        (outer.y + inner.y) / 2.0 + ty * head,
    ));
    path.line_to(point(inner.x - out_x * 0.5, inner.y - out_y * 0.5));
}

/// A box with an arrow standing out of one of its sides, or out of two, or out
/// of all four: Word's arrow callouts.
///
/// The box is where the words go and the arrow points at what they are about,
/// so the box keeps the larger part of the shape however many arrows come out
/// of it, and each arrow stands out of the middle of its own side.
fn callout(path: &mut Path, box_: Box_, sides: &[Side]) {
    let point = Point::new;
    let shorter = box_.shorter();
    let has = |side: Side| sides.contains(&side);
    let two_of = |first: Side, second: Side| {
        f32::from(u8::from(has(first)) + u8::from(has(second))).max(1.0)
    };

    // How far an arrow stands out of its side: a share of the shorter side of
    // the box, and never so much that the box it comes out of is left the
    // smaller part of the shape.
    let reach_x =
        (shorter * 0.28).min(box_.width() / (2.0 * two_of(Side::Left, Side::Right) + 1.0));
    let reach_y = (shorter * 0.28).min(box_.height() / (2.0 * two_of(Side::Up, Side::Down) + 1.0));
    // Across an arrow: half its shaft, and half the base of its head, which is
    // the wider of the two or it would not read as a head at all.
    let shaft = (shorter * 0.09).min(box_.width().min(box_.height()) / 8.0);
    let base = shaft * 2.0;
    // And along it: how much of the reach is shaft, the rest being head.
    let (stem_x, stem_y) = (reach_x * 0.35, reach_y * 0.35);

    // The box itself, with each side that has an arrow pushed in to leave room
    // for it.
    let left = box_.left + if has(Side::Left) { reach_x } else { 0.0 };
    let right = box_.right - if has(Side::Right) { reach_x } else { 0.0 };
    let top = box_.top + if has(Side::Up) { reach_y } else { 0.0 };
    let bottom = box_.bottom - if has(Side::Down) { reach_y } else { 0.0 };
    let (mx, my) = (box_.middle_x(), box_.middle_y());

    // Round the box from its top left corner, stepping out of each side that
    // has an arrow as the middle of that side is passed.
    path.move_to(point(left, top));
    if has(Side::Up) {
        path.line_to(point(mx - shaft, top));
        path.line_to(point(mx - shaft, top - stem_y));
        path.line_to(point(mx - base, top - stem_y));
        path.line_to(point(mx, box_.top));
        path.line_to(point(mx + base, top - stem_y));
        path.line_to(point(mx + shaft, top - stem_y));
        path.line_to(point(mx + shaft, top));
    }
    path.line_to(point(right, top));
    if has(Side::Right) {
        path.line_to(point(right, my - shaft));
        path.line_to(point(right + stem_x, my - shaft));
        path.line_to(point(right + stem_x, my - base));
        path.line_to(point(box_.right, my));
        path.line_to(point(right + stem_x, my + base));
        path.line_to(point(right + stem_x, my + shaft));
        path.line_to(point(right, my + shaft));
    }
    path.line_to(point(right, bottom));
    if has(Side::Down) {
        path.line_to(point(mx + shaft, bottom));
        path.line_to(point(mx + shaft, bottom + stem_y));
        path.line_to(point(mx + base, bottom + stem_y));
        path.line_to(point(mx, box_.bottom));
        path.line_to(point(mx - base, bottom + stem_y));
        path.line_to(point(mx - shaft, bottom + stem_y));
        path.line_to(point(mx - shaft, bottom));
    }
    path.line_to(point(left, bottom));
    if has(Side::Left) {
        path.line_to(point(left, my + shaft));
        path.line_to(point(left - stem_x, my + shaft));
        path.line_to(point(left - stem_x, my + base));
        path.line_to(point(box_.left, my));
        path.line_to(point(left - stem_x, my - base));
        path.line_to(point(left - stem_x, my - shaft));
        path.line_to(point(left, my - shaft));
    }
    path.close();
}

/// An arrow bent round most of a circle, with the head at the end of it.
fn circular(path: &mut Path, box_: Box_) {
    let (cx, cy) = (box_.middle_x(), box_.middle_y());
    let shorter = box_.shorter();
    let head = shorter * 0.22;
    let thick = shorter * 0.12;
    // The band is drawn inside the box with room for the head, which reaches
    // further out than the band does: a head hanging over the edge would be a
    // shape drawn outside the box it was given.
    let (rx, ry) =
        ((box_.width() / 2.0 - head / 2.0).max(1.0), (box_.height() / 2.0 - head / 2.0).max(1.0));
    let thick = thick.min(rx.min(ry) / 2.0);

    // Three quarters of the way round, leaving the gap a circular arrow has.
    let (from, to) = (300.0, 570.0);
    arc_into(path, cx, cy, rx, ry, turn(from), turn(to));

    // The head, standing across the end of the band and pointing on round it.
    head_across(path, cx, cy, rx, ry, to, thick, head);
    arc_into(path, cx, cy, rx - thick, ry - thick, turn(to), turn(from));
    path.close();
}

/// Word's swoosh: a stroke that starts at a point and thickens into a head.
fn swoosh(path: &mut Path, box_: Box_) {
    let point = Point::new;
    let shorter = box_.shorter();
    let head = shorter * 0.42;
    let thick = shorter * 0.14;

    // The head points up and to the right, so it stands across that diagonal.
    let slant = std::f32::consts::FRAC_1_SQRT_2;
    let middle = point(box_.right - head * slant, box_.top + head * slant);
    let across = slant * thick;

    // Two curves from one point: out along the bottom and up into the head,
    // then back underneath it to where it started. A tail that comes to a point
    // is what makes this a swoosh rather than a bent band.
    let tail = point(box_.left, box_.bottom);
    path.move_to(tail);
    path.quad_to(point(box_.right, box_.bottom), point(middle.x + across, middle.y + across));
    // The head stands out past both edges of the stroke, or it is a sharpened
    // end rather than an arrow.
    let barb = across * 1.8;
    path.line_to(point(middle.x + barb, middle.y + barb));
    path.line_to(point(box_.right, box_.top));
    path.line_to(point(middle.x - barb, middle.y - barb));
    path.line_to(point(middle.x - across, middle.y - across));
    path.quad_to(point(box_.left + box_.width() * 0.47, box_.bottom - box_.height() * 0.18), tail);
    path.close();
}

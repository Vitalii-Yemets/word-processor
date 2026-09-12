//! Word's callouts: the four bubbles and the twelve with a leader line.
//!
//! # What sets a callout apart from every other shape
//!
//! It is drawn partly *outside* its own box. The box is where the words go and
//! the tail points at what they are about, which is somewhere else — so the one
//! rule every other shape here keeps, that nothing is drawn outside the box it
//! was given, is the rule a callout is for breaking. Where the tail goes is the
//! shape's own business: it is written in the file, behind the handles.
//!
//! # The bubbles
//!
//! A body with a wedge cut out of one side of it and taken out to the point.
//! The wedge is part of the same outline and not a triangle laid on top: a
//! triangle laid on top would show a line across the bubble where its base sat,
//! and a bubble has no line across it.
//!
//! # The twelve with a leader
//!
//! These are a box of words with a line going out of it, and the four families
//! differ in nothing but what is drawn: the line alone, the line and a border
//! round the words, the line and a bar down the side of them, or all three.
//! That is why a callout with no border is drawn with no band round its edge at
//! all — see [`crate::geometry::Preset::edge_drawn`] — and why the leader and
//! the bar are drawn with the outline rather than being part of the area.

use wp_raster::{Path, Point};

use crate::geometry::{arc_into, rounded_rectangle, Adjusts, Preset};

use core::f32::consts::{FRAC_PI_2, PI, TAU};

/// The outline of a callout, or `None` if the preset is not one.
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
    let (mx, my) = ((left + right) / 2.0, (top + bottom) / 2.0);
    let shorter = width.min(height);

    match preset {
        Preset::Callout | Preset::RoundedCallout => {
            let tip = tip_of(adjusts, left, top, right, bottom);
            let radius = if preset == Preset::RoundedCallout {
                shorter * adjusts.share(3, 1.0 / 6.0)
            } else {
                0.0
            };
            let radius = radius.min(width / 2.0).min(height / 2.0);
            bubble(&mut path, left, top, right, bottom, radius, tip, shorter * 0.2);
        }
        Preset::OvalCallout => {
            let tip = tip_of(adjusts, left, top, right, bottom);
            // Where the tail leaves the oval: the direction of the point,
            // measured in the oval's own stretch so that the wedge sits square
            // on the curve rather than leaning along it.
            let along = ((tip.y - my) / (height / 2.0)).atan2((tip.x - mx) / (width / 2.0));
            let half = 0.22;
            // The long way round, in two halves: one run of an arc is sixteen
            // straight pieces, and sixteen is not enough for a whole oval.
            let sweep = TAU - half * 2.0;
            arc_into(
                &mut path,
                mx,
                my,
                width / 2.0,
                height / 2.0,
                along + half,
                along + half + sweep / 2.0,
            );
            arc_into(
                &mut path,
                mx,
                my,
                width / 2.0,
                height / 2.0,
                along + half + sweep / 2.0,
                along + half + sweep,
            );
            path.line_to(tip);
            path.close();
        }
        Preset::Cloud | Preset::CloudCallout => {
            cloud(&mut path, left, top, right, bottom);
            if preset == Preset::CloudCallout {
                // The three bubbles that lead to the point, each smaller than
                // the one before it. They are contours of their own, because
                // that is what they are: three bubbles, not a tail.
                let tip = tip_of(adjusts, left, top, right, bottom);
                // Out past the edge of the cloud rather than inside it: the
                // first bubble laps over it a little, the way Word draws it,
                // and the other two are clear of it.
                for (step, size) in [(0.82_f32, 0.07_f32), (0.92, 0.05), (1.0, 0.032)] {
                    let at = point(mx + (tip.x - mx) * step, my + (tip.y - my) * step);
                    let reach = shorter * size;
                    // Where the bubble starts, said out loud: an arc that
                    // followed on from the cloud would drag an edge across to
                    // it. Two halves, because one run of an arc is sixteen
                    // straight pieces and a circle wants more than that.
                    path.move_to(point(at.x + reach, at.y));
                    arc_into(&mut path, at.x, at.y, reach, reach, 0.0, PI);
                    arc_into(&mut path, at.x, at.y, reach, reach, PI, TAU);
                    path.close();
                }
            }
        }
        Preset::LineCallout1
        | Preset::LineCallout2
        | Preset::LineCallout3
        | Preset::AccentCallout1
        | Preset::AccentCallout2
        | Preset::AccentCallout3
        | Preset::BorderCallout1
        | Preset::BorderCallout2
        | Preset::BorderCallout3
        | Preset::AccentBorderCallout1
        | Preset::AccentBorderCallout2
        | Preset::AccentBorderCallout3 => {
            // The words sit in the box and nothing else here is an area: the
            // leader and the bar are lines, and lines are drawn with the
            // outline. See [`rules_into`].
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        _ => return None,
    }
    Some(path)
}

/// The lines a callout is drawn with beside the band round its words: the
/// leader out to what it points at, and the accent bar down the side of them.
#[allow(clippy::too_many_arguments, reason = "a shape, its handles, its box and a weight")]
pub(crate) fn rules_into(
    path: &mut Path,
    preset: Preset,
    adjusts: &Adjusts,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    weight: f32,
) {
    let (left, top, right, bottom) = (x, y, x + width, y + height);
    let Some(segments) = segments_of(preset) else {
        return;
    };

    // The leader: the points the handles give, joined up. The first is where it
    // leaves the words and the last is what it points at.
    let places = leader(adjusts, segments, left, top, right, bottom);
    for pair in places.windows(2) {
        rule(path, pair[0], pair[1], weight);
    }
    if accented(preset) {
        // The bar down the side the leader leaves by, which is what the accent
        // in the shape's name is. Down and not across: Word's accent is always
        // beside the words, so a leader that left by the top or the bottom
        // still has its bar on the side it is nearer.
        let leaves = places.first().map_or(left, |first| first.x);
        let side = if leaves > (left + right) / 2.0 { right } else { left };
        rule(path, Point::new(side, top), Point::new(side, bottom), weight);
    }
}

/// How many segments a line callout's leader has, or `None` for a shape that
/// has no leader.
fn segments_of(preset: Preset) -> Option<usize> {
    Some(match preset {
        Preset::LineCallout1
        | Preset::AccentCallout1
        | Preset::BorderCallout1
        | Preset::AccentBorderCallout1 => 1,
        Preset::LineCallout2
        | Preset::AccentCallout2
        | Preset::BorderCallout2
        | Preset::AccentBorderCallout2 => 2,
        Preset::LineCallout3
        | Preset::AccentCallout3
        | Preset::BorderCallout3
        | Preset::AccentBorderCallout3 => 3,
        _ => return None,
    })
}

/// Whether the shape has a bar down the side of its words.
fn accented(preset: Preset) -> bool {
    matches!(
        preset,
        Preset::AccentCallout1
            | Preset::AccentCallout2
            | Preset::AccentCallout3
            | Preset::AccentBorderCallout1
            | Preset::AccentBorderCallout2
            | Preset::AccentBorderCallout3
    )
}

/// Where a bubble points.
///
/// The two handles are the point itself, each a fraction of the box measured
/// from the middle of it. Word's own values put it below the bubble and to the
/// left, which is where a tail usually hangs.
fn tip_of(adjusts: &Adjusts, left: f32, top: f32, right: f32, bottom: f32) -> Point {
    let (width, height) = (right - left, bottom - top);
    let (mx, my) = ((left + right) / 2.0, (top + bottom) / 2.0);
    Point::new(mx + width * adjusts.share(1, -0.208_33), my + height * adjusts.share(2, 0.625))
}

/// Where a leader line goes: the place it leaves the words, the bends, and the
/// point.
///
/// The handles come in pairs — down first and then across, each a fraction of
/// the box from its top left corner, and a negative one is outside the box.
/// That is the order Word's own values are in, and it is why the default leader
/// runs out of the left-hand side: the across of every pair is negative.
fn leader(
    adjusts: &Adjusts,
    segments: usize,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> Vec<Point> {
    let (width, height) = (right - left, bottom - top);
    // Where each point goes when the document says nothing: out of the left at
    // a fifth down, and then away and down for the shapes that bend.
    const FALLBACK: [(f32, f32); 4] =
        [(0.187_5, -0.083_33), (0.187_5, -0.166_67), (0.45, -0.28), (0.75, -0.28)];

    let mut places: Vec<Point> = (0..=segments)
        .map(|index| {
            let (down, across) = FALLBACK[index.min(FALLBACK.len() - 1)];
            Point::new(
                left + width * adjusts.share(index * 2 + 2, across),
                top + height * adjusts.share(index * 2 + 1, down),
            )
        })
        .collect();
    // The first pair says where the leader leaves the words, and it is taken to
    // the edge of the box: a line that stopped short of the words, or started
    // inside them, would not join the words to what they are about.
    if let Some(first) = places.first_mut() {
        *first = onto_edge(*first, left, top, right, bottom);
    }
    places
}

/// The nearest point on the edge of the box.
///
/// A point outside it comes back on the side it is beyond; one inside goes out
/// to whichever side is nearest.
fn onto_edge(at: Point, left: f32, top: f32, right: f32, bottom: f32) -> Point {
    let (x, y) = (at.x.clamp(left, right), at.y.clamp(top, bottom));
    if x != at.x || y != at.y {
        return Point::new(x, y);
    }
    let sides = [
        (at.x - left, Point::new(left, at.y)),
        (right - at.x, Point::new(right, at.y)),
        (at.y - top, Point::new(at.x, top)),
        (bottom - at.y, Point::new(at.x, bottom)),
    ];
    sides.into_iter().min_by(|one, two| one.0.total_cmp(&two.0)).map_or(at, |(_, place)| place)
}

/// The body of a bubble: a rectangle, taken round at the corners if asked, with
/// the wedge of the tail cut into whichever side the point is beyond.
#[allow(clippy::too_many_arguments, reason = "a box, a corner, a point and a base")]
fn bubble(
    path: &mut Path,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius: f32,
    tip: Point,
    base: f32,
) {
    let point = Point::new;
    // A point inside the box has no side to leave by, and a bubble with no tail
    // is a box. The side is the one the point is furthest beyond.
    let out = [
        (left - tip.x, Side::Left),
        (tip.x - right, Side::Right),
        (top - tip.y, Side::Up),
        (tip.y - bottom, Side::Down),
    ];
    let (past, side) =
        out.into_iter().fold(
            (0.0, None),
            |(most, side), (by, which)| {
                if by > most {
                    (by, Some(which))
                } else {
                    (most, side)
                }
            },
        );
    if past <= 0.0 || side.is_none() {
        rounded_rectangle(path, left, top, right, bottom, radius);
        return;
    }
    let side = side.expect("a side the point is beyond");

    // Clockwise from the top left corner, one side at a time, stepping out to
    // the point on the way along the side it leaves by.
    path.move_to(point(left + radius, top));
    along(path, point(left + radius, top), point(right - radius, top), side == Side::Up, tip, base);
    path.quad_to(point(right, top), point(right, top + radius));
    along(
        path,
        point(right, top + radius),
        point(right, bottom - radius),
        side == Side::Right,
        tip,
        base,
    );
    path.quad_to(point(right, bottom), point(right - radius, bottom));
    along(
        path,
        point(right - radius, bottom),
        point(left + radius, bottom),
        side == Side::Down,
        tip,
        base,
    );
    path.quad_to(point(left, bottom), point(left, bottom - radius));
    along(
        path,
        point(left, bottom - radius),
        point(left, top + radius),
        side == Side::Left,
        tip,
        base,
    );
    path.quad_to(point(left, top), point(left + radius, top));
    path.close();
}

/// Which side of a bubble the tail leaves by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Up,
    Down,
}

/// One side of a bubble, with or without the wedge in it.
///
/// The wedge stands where the point falls on this side, and is kept inside the
/// ends of it: a tail whose base ran off the corner would leave the bubble open.
fn along(path: &mut Path, from: Point, to: Point, wedge: bool, tip: Point, base: f32) {
    if !wedge {
        path.line_to(to);
        return;
    }
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let span = dx.hypot(dy).max(0.001);
    let (ux, uy) = (dx / span, dy / span);
    let half = (base / 2.0).min(span / 2.0);
    let where_on = ((tip.x - from.x) * ux + (tip.y - from.y) * uy).clamp(half, span - half);
    let at = |distance: f32| Point::new(from.x + ux * distance, from.y + uy * distance);

    path.line_to(at(where_on - half));
    path.line_to(tip);
    path.line_to(at(where_on + half));
    path.line_to(to);
}

/// A cloud: a ring of round bumps, each drawn from where it crosses the bump
/// behind it to where it crosses the one ahead.
///
/// # Why the bumps are round and why they are counted
///
/// Round, so that where two of them cross can be worked out exactly: each
/// crossing is worked out once and used by both bumps, which makes the ring one
/// unbroken edge. Two arcs that only nearly met would leave a nick in the
/// outline and a chord drawn across the inside of the cloud.
///
/// And counted from how far it is round the oval they sit on, so that a cloud
/// stretched wide gets more bumps rather than gaps between the ones it had. A
/// bump has to lap over its neighbours or there is no crossing to draw to.
fn cloud(path: &mut Path, left: f32, top: f32, right: f32, bottom: f32) {
    let (width, height) = (right - left, bottom - top);
    let (mx, my) = ((left + right) / 2.0, (top + bottom) / 2.0);
    let middle = Point::new(mx, my);
    let bump = width.min(height) * 0.16;
    let (rx, ry) = ((width / 2.0 - bump).max(0.1), (height / 2.0 - bump).max(0.1));
    let round_about = PI * (rx + ry);
    let count = ((round_about / (bump * 1.4)).round() as usize).clamp(8, 24);

    let at = |index: usize| {
        let angle = -FRAC_PI_2 + TAU * index as f32 / count as f32;
        Point::new(mx + rx * angle.cos(), my + ry * angle.sin())
    };
    let crossings: Vec<Point> = (0..count)
        .map(|index| crossing(at(index), at((index + 1) % count), bump, middle))
        .collect();

    for index in 0..count {
        let centre = at(index);
        let behind = crossings[(index + count - 1) % count];
        let ahead = crossings[index];
        let from = (behind.y - centre.y).atan2(behind.x - centre.x);
        let mut to = (ahead.y - centre.y).atan2(ahead.x - centre.x);
        // The way round that goes outside: the one that passes the direction
        // the bump faces away from the middle of the cloud.
        let mut facing = (centre.y - my).atan2(centre.x - mx);
        while to < from {
            to += TAU;
        }
        while facing < from {
            facing += TAU;
        }
        if facing > to {
            to -= TAU;
        }
        arc_into(path, centre.x, centre.y, bump, bump, from, to);
    }
    path.close();
}

/// Where two bumps of the same size cross, on the outside of the cloud.
///
/// Two of them that do not reach each other have no crossing, and the point
/// between them is taken instead: that leaves the ring flat there rather than
/// leaving it open.
fn crossing(one: Point, two: Point, reach: f32, middle: Point) -> Point {
    let (dx, dy) = (two.x - one.x, two.y - one.y);
    let apart = dx.hypot(dy);
    let between = Point::new((one.x + two.x) / 2.0, (one.y + two.y) / 2.0);
    if apart <= 0.0 || apart >= reach * 2.0 {
        return between;
    }
    let out = (reach * reach - apart * apart / 4.0).max(0.0).sqrt();
    let (nx, ny) = (-dy / apart * out, dx / apart * out);
    let (near, far) =
        (Point::new(between.x + nx, between.y + ny), Point::new(between.x - nx, between.y - ny));
    // The one further from the middle of the cloud is the one on the outside.
    if (near.x - middle.x).hypot(near.y - middle.y) > (far.x - middle.x).hypot(far.y - middle.y) {
        near
    } else {
        far
    }
}

/// A line inside or beside a shape, given a width.
///
/// Wound the way the shapes are wound, so that it adds to what they fill rather
/// than cancelling it. The same rule the flowchart shapes and the scrolls are
/// drawn by.
fn rule(path: &mut Path, from: Point, to: Point, weight: f32) {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = dx.hypot(dy);
    if length <= 0.0 {
        return;
    }
    let (nx, ny) = (-dy / length * weight / 2.0, dx / length * weight / 2.0);
    path.move_to(Point::new(from.x - nx, from.y - ny));
    path.line_to(Point::new(to.x - nx, to.y - ny));
    path.line_to(Point::new(to.x + nx, to.y + ny));
    path.line_to(Point::new(from.x + nx, from.y + ny));
    path.close();
}

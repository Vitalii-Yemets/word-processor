//! Word's stars and banners: the explosions, the ten stars, the ribbons, the
//! scrolls and the waves.
//!
//! # Why the curves here are arcs
//!
//! Several of these have an edge that has to *reach* the side of the box and
//! come back — the apex of a wave, the bow of a curved ribbon. A quadratic
//! whose curve touches the edge has its control point beyond it, and a shape is
//! measured by the points its path names: a control point outside the box is a
//! shape that reads as reaching outside the box, and the test that says no
//! shape does would be right to fail. Every point of an arc is on the shape, so
//! the waves and the bows are arcs.
//!
//! # The pairs that differ in nothing but which way up
//!
//! The down ribbon is the up ribbon upside down, and the curved down ribbon the
//! curved up one. They are drawn once and turned over, because two copies of
//! the same arithmetic is two places for it to drift.

use wp_raster::{Path, Point, Transform};

use crate::geometry::{arc_into, Preset};

use core::f32::consts::{FRAC_PI_2, PI, TAU};

/// The outline of a star or a banner, or `None` if the preset is not one.
pub(crate) fn path_in(
    preset: Preset,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> Option<Path> {
    let mut path = Path::new();
    let point = Point::new;
    let (width, height) = (right - left, bottom - top);
    let (mx, my) = ((left + right) / 2.0, (top + bottom) / 2.0);

    match preset {
        Preset::Explosion1 => seal(&mut path, left, top, right, bottom, SEAL_ONE),
        Preset::Explosion2 => seal(&mut path, left, top, right, bottom, SEAL_TWO),
        // The ten stars. What tells them apart is how many points they have and
        // how far in the dips between them go, and the format has an answer for
        // each: see [`STARS`].
        Preset::Star4
        | Preset::Star
        | Preset::Star6
        | Preset::Star7
        | Preset::Star8
        | Preset::Star10
        | Preset::Star12
        | Preset::Star16
        | Preset::Star24
        | Preset::Star32 => {
            let (points, inner) = star_of(preset)?;
            star(&mut path, mx, my, width / 2.0, height / 2.0, points, inner);
        }
        Preset::UpRibbon => ribbon(&mut path, left, top, right, bottom),
        Preset::DownRibbon => {
            // The same banner the other way up.
            let mut up = Path::new();
            ribbon(&mut up, left, top, right, bottom);
            return Some(turned_over(&up, my));
        }
        Preset::CurvedUpRibbon => curved_ribbon(&mut path, left, top, right, bottom),
        Preset::CurvedDownRibbon => {
            let mut up = Path::new();
            curved_ribbon(&mut up, left, top, right, bottom);
            return Some(turned_over(&up, my));
        }
        Preset::VerticalScroll => {
            // The rolls run down the sides, the left one curled at the top and
            // the right one at the bottom: a sheet rolled from both ends and
            // laid flat. The sheet itself is inset by the curl, so that what
            // bulges out of it reaches the edge of the box and no further.
            let (_, curl) = roll_of(width, height);
            arc_into(&mut path, left + curl, top + curl, curl, curl, PI, TAU);
            path.line_to(point(right, top + curl));
            path.line_to(point(right, bottom - curl));
            arc_into(&mut path, right - curl, bottom - curl, curl, curl, 0.0, PI);
            path.line_to(point(left, bottom - curl));
            path.close();
        }
        Preset::HorizontalScroll => {
            // The same laid the other way: the rolls along the top and the
            // bottom, curled at opposite ends.
            let (_, curl) = roll_of(width, height);
            path.move_to(point(left + curl, top));
            path.line_to(point(right - curl, top));
            arc_into(&mut path, right - curl, top + curl, curl, curl, -FRAC_PI_2, FRAC_PI_2);
            path.line_to(point(right - curl, bottom));
            path.line_to(point(left + curl, bottom));
            arc_into(&mut path, left + curl, bottom - curl, curl, curl, FRAC_PI_2, PI * 1.5);
            path.close();
        }
        Preset::Wave | Preset::DoubleWave => {
            // A band whose top and bottom undulate together, so that it is the
            // same width all the way along. The double wave is the same with
            // twice as many humps in it.
            let humps = if preset == Preset::DoubleWave { 4 } else { 2 };
            let swing = height * 0.14;
            wave_edge(&mut path, left, right, top + swing, swing, humps, true);
            path.line_to(point(right, bottom - swing));
            wave_edge(&mut path, left, right, bottom - swing, swing, humps, false);
            path.close();
        }
        _ => return None,
    }
    Some(path)
}

/// The lines drawn inside a star or a banner, with the shape's own outline.
///
/// Only the scrolls have any: where the rolled paper meets the sheet, and the
/// far side of each curl. Without them a scroll is a rectangle with two corners
/// bulging, and nothing says it is rolled.
pub(crate) fn rules_into(
    path: &mut Path,
    preset: Preset,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    weight: f32,
) {
    let point = Point::new;
    let (left, top, right, bottom) = (x, y, x + width, y + height);
    let (roll, curl) = roll_of(width, height);

    match preset {
        Preset::VerticalScroll => {
            // Down each side: the edge of the rolled paper.
            rule(path, point(left + roll, top + curl), point(left + roll, bottom - curl), weight);
            rule(path, point(right - roll, top + curl), point(right - roll, bottom - curl), weight);
            // And the far side of each curl, which is the paper coming round.
            arc_rule(path, left + curl, top + curl, curl, curl, 0.0, PI, weight);
            arc_rule(path, right - curl, bottom - curl, curl, curl, PI, TAU, weight);
        }
        Preset::HorizontalScroll => {
            rule(path, point(left + curl, top + roll), point(right - curl, top + roll), weight);
            rule(
                path,
                point(left + curl, bottom - roll),
                point(right - curl, bottom - roll),
                weight,
            );
            arc_rule(path, right - curl, top + curl, curl, curl, FRAC_PI_2, PI * 1.5, weight);
            arc_rule(path, left + curl, bottom - curl, curl, curl, -FRAC_PI_2, FRAC_PI_2, weight);
        }
        _ => {}
    }
}

/// How wide a scroll's roll is, and the radius of the curl at the end of it.
///
/// A sixth of the shorter side, which is the roll the format draws when nothing
/// says otherwise.
fn roll_of(width: f32, height: f32) -> (f32, f32) {
    let roll = width.min(height) / 6.0;
    (roll, roll / 2.0)
}

/// How many points each of the ten stars has, and how far in the dips go.
///
/// The second number is the inner radius as a fraction of the outer one, and it
/// is the whole of the difference between a spiky star and a blunt one. These
/// are the format's own: a five-pointed star dips to 0.382, which is the
/// pentagram the golden ratio gives, and the more points a star has the less
/// far in it dips, because the points would otherwise be too thin to see.
const STARS: &[(Preset, usize, f32)] = &[
    (Preset::Star4, 4, 0.25),
    (Preset::Star, 5, 0.381_966),
    (Preset::Star6, 6, 0.577_36),
    (Preset::Star7, 7, 0.692_02),
    (Preset::Star8, 8, 0.75),
    (Preset::Star10, 10, 0.850_66),
    (Preset::Star12, 12, 0.75),
    (Preset::Star16, 16, 0.75),
    (Preset::Star24, 24, 0.75),
    (Preset::Star32, 32, 0.75),
];

fn star_of(preset: Preset) -> Option<(usize, f32)> {
    STARS.iter().find(|(it, ..)| *it == preset).map(|(_, points, inner)| (*points, *inner))
}

/// A star of so many points, filling the box.
///
/// The points sit on the box's own ellipse and the dips between them on an
/// ellipse of `inner` times its radii. The first point is at the top, which is
/// the way up every one of Word's stars is drawn.
fn star(path: &mut Path, cx: f32, cy: f32, rx: f32, ry: f32, points: usize, inner: f32) {
    let step = PI / points as f32;
    for index in 0..points * 2 {
        let angle = -FRAC_PI_2 + step * index as f32;
        let reach = if index % 2 == 0 { 1.0 } else { inner };
        let at = Point::new(cx + rx * reach * angle.cos(), cy + ry * reach * angle.sin());
        if index == 0 {
            path.move_to(at);
        } else {
            path.line_to(at);
        }
    }
    path.close();
}

/// The two explosions, as the reach of the shape at each step round it.
///
/// A list rather than a rule, because an explosion is irregular on purpose and
/// a rule that drew one would draw it regular. The numbers are fractions of the
/// way out to the edge, they alternate long and short, and they are the same
/// every time the shape is drawn: an explosion that came out differently each
/// time would be a shape that changed under the person who put it there.
const SEAL_ONE: &[f32] = &[
    1.00, 0.42, 0.86, 0.35, 0.97, 0.46, 0.78, 0.30, 0.92, 0.40, 1.00, 0.36, 0.84, 0.44, 0.95, 0.32,
    0.88, 0.41, 0.99, 0.38, 0.80, 0.45, 0.90, 0.33,
];

const SEAL_TWO: &[f32] = &[
    0.95, 0.38, 1.00, 0.44, 0.82, 0.34, 0.93, 0.40, 0.99, 0.36, 0.86, 0.46, 1.00, 0.32, 0.90, 0.42,
    0.96, 0.37, 0.83, 0.45, 0.98, 0.33, 0.88, 0.43, 1.00, 0.39, 0.85, 0.35, 0.94, 0.41, 0.91, 0.30,
];

/// An explosion, from the reaches given.
///
/// The points are worked out on a circle and then fitted to the box, so that a
/// shape whose longest spike happens not to point along an axis still fills the
/// box it was given and still stays inside it.
fn seal(path: &mut Path, left: f32, top: f32, right: f32, bottom: f32, reaches: &[f32]) {
    let step = TAU / reaches.len() as f32;
    let spikes: Vec<Point> = reaches
        .iter()
        .enumerate()
        .map(|(index, reach)| {
            let angle = -FRAC_PI_2 + step * index as f32;
            Point::new(reach * angle.cos(), reach * angle.sin())
        })
        .collect();

    let (mut low_x, mut low_y, mut high_x, mut high_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for spike in &spikes {
        low_x = low_x.min(spike.x);
        low_y = low_y.min(spike.y);
        high_x = high_x.max(spike.x);
        high_y = high_y.max(spike.y);
    }
    let across = (high_x - low_x).max(0.001);
    let down = (high_y - low_y).max(0.001);

    for (index, spike) in spikes.iter().enumerate() {
        let at = Point::new(
            left + (spike.x - low_x) / across * (right - left),
            top + (spike.y - low_y) / down * (bottom - top),
        );
        if index == 0 {
            path.move_to(at);
        } else {
            path.line_to(at);
        }
    }
    path.close();
}

/// A banner: a panel across the middle with a tail out of each end.
///
/// Drawn the way up the up ribbon goes — the panel raised and the tails hanging
/// lower. The down ribbon is this turned over.
fn ribbon(path: &mut Path, left: f32, top: f32, right: f32, bottom: f32) {
    let point = Point::new;
    let (width, height) = (right - left, bottom - top);
    // How far the tails hang below the panel, how long each tail is, and how
    // deep the V is that is cut into the end of it.
    let step = height / 4.0;
    let tail = width / 4.0;
    let notch = tail * 0.5;

    let (panel_left, panel_right) = (left + tail, right - tail);
    let (panel_top, panel_bottom) = (top, bottom - step);
    let (tail_top, tail_bottom) = (top + step, bottom);
    let tail_middle = (tail_top + tail_bottom) / 2.0;

    path.move_to(point(panel_left, panel_top));
    path.line_to(point(panel_right, panel_top));
    path.line_to(point(panel_right, tail_top));
    path.line_to(point(right, tail_top));
    path.line_to(point(right - notch, tail_middle));
    path.line_to(point(right, tail_bottom));
    path.line_to(point(panel_right, tail_bottom));
    path.line_to(point(panel_right, panel_bottom));
    path.line_to(point(panel_left, panel_bottom));
    path.line_to(point(panel_left, tail_bottom));
    path.line_to(point(left, tail_bottom));
    path.line_to(point(left + notch, tail_middle));
    path.line_to(point(left, tail_top));
    path.line_to(point(panel_left, tail_top));
    path.close();
}

/// The same banner bowed: the band follows an arc and the tails hang from its
/// ends.
///
/// Drawn curving up. The curved down ribbon is this turned over.
fn curved_ribbon(path: &mut Path, left: f32, top: f32, right: f32, bottom: f32) {
    let point = Point::new;
    let (width, height) = (right - left, bottom - top);
    let middle_x = (left + right) / 2.0;
    // How far the ends of the band sit below its middle, how deep the band is,
    // how long a tail is, and the V cut into the end of it.
    let bow = height * 0.22;
    let deep = height * 0.3;
    let tail = width * 0.14;
    let notch = height * 0.12;

    // Where along the bow a tail hangs from, as an angle: the band is an arc,
    // so a place along it is an angle and not a distance.
    let share = (1.0 - 2.0 * tail / width).clamp(-1.0, 1.0);
    let phi = share.acos();

    // Over the top of the bow, left to right.
    arc_into(path, middle_x, top + bow, width / 2.0, bow, PI, TAU);
    // Down the outside of the right-hand tail, the V cut into its end, and
    // back up its inside to the underside of the band.
    path.line_to(point(right, bottom));
    path.line_to(point(right - tail / 2.0, bottom - notch));
    path.line_to(point(right - tail, bottom));
    path.line_to(point(
        right - tail,
        top + bow + deep - bow * (1.0 - share * share).sqrt().max(0.0),
    ));
    // Back along the underside of the band, right to left.
    arc_into(path, middle_x, top + bow + deep, width / 2.0, bow, TAU - phi, PI + phi);
    // And the left-hand tail, the same the other way about.
    path.line_to(point(left + tail, bottom));
    path.line_to(point(left + tail / 2.0, bottom - notch));
    path.line_to(point(left, bottom));
    path.close();
}

/// A wave along one edge, as a run of arcs that rise and fall in turn.
///
/// `rightwards` says which way the edge is being drawn. The humps themselves
/// are the same either way round and only the order they are walked in
/// changes, or the band they make would not be the same width all along.
#[allow(clippy::too_many_arguments, reason = "a wave has ends, a line, a swing and a count")]
fn wave_edge(
    path: &mut Path,
    left: f32,
    right: f32,
    middle: f32,
    swing: f32,
    humps: usize,
    rightwards: bool,
) {
    let step = (right - left) / humps as f32;
    for index in 0..humps {
        let hump = if rightwards { index } else { humps - 1 - index };
        let centre = left + step * (hump as f32 + 0.5);
        // A negative radius walks the same arc the other way, which is what
        // draws the hump backwards without mirroring it.
        let half = if rightwards { step / 2.0 } else { -step / 2.0 };
        let (from, to) = if hump % 2 == 0 { (PI, TAU) } else { (PI, 0.0) };
        arc_into(path, centre, middle, half, swing, from, to);
    }
}

/// The same shape upside down, about the middle of its own box.
fn turned_over(path: &Path, middle_y: f32) -> Path {
    let flip = Transform::translate(0.0, -middle_y)
        .then(&Transform::scale(1.0, -1.0))
        .then(&Transform::translate(0.0, middle_y));
    path.transformed(&flip)
}

/// A line inside a shape, given a width and wound the way the shapes here are.
///
/// The same as the flowchart shapes use, and for the same reason: by the
/// nonzero rule a contour wound against its neighbour leaves a hole, so a rule
/// wound the other way would draw a gap across the outline where it met it.
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

/// The same along an arc: out one side of it and back along the other.
#[allow(clippy::too_many_arguments, reason = "an arc has a centre, two radii and two angles")]
fn arc_rule(path: &mut Path, cx: f32, cy: f32, rx: f32, ry: f32, from: f32, to: f32, weight: f32) {
    let half = weight / 2.0;
    path.move_to(Point::new(cx + (rx + half) * from.cos(), cy + (ry + half) * from.sin()));
    arc_into(path, cx, cy, rx + half, ry + half, from, to);
    arc_into(path, cx, cy, (rx - half).max(0.1), (ry - half).max(0.1), to, from);
    path.close();
}

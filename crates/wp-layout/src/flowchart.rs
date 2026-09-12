//! The twenty-eight shapes a flowchart is drawn with.
//!
//! # Why these are a file of their own
//!
//! For the same reason the block arrows are: twenty-eight shapes in the middle
//! of everything else buries the rest of the gallery, and these have an idea of
//! their own. Most of them are a rectangle with one edge changed — a corner cut
//! off, a side taken round, a bottom made into a wave — and reading them next
//! to each other is the only way to see that the card and the manual input
//! differ in nothing but which edge is cut.
//!
//! # The rules inside some of them
//!
//! Eight of these have a line drawn *inside* the shape: the predefined process
//! has one down each end, the "or" has a cross through it, the magnetic disk
//! has the near side of its lid. Those lines are not part of the shape's area.
//! The fill does not know about them, and neither does the text that wraps
//! round the shape — they are drawn with the shape's own outline, and they are
//! asked for alongside it. See [`rules_into`].
//!
//! Without them a predefined process is a process and a sort is a decision:
//! two shapes under one drawing, which is a drawing that lies about which of
//! them it is.

use wp_raster::{Path, Point};

use crate::geometry::{arc_into, ellipse, rounded_rectangle, Preset};

use core::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

/// The outline of a flowchart shape, or `None` if the preset is not one.
///
/// The box is given as its four edges, because that is how these are drawn:
/// nearly every one of them names a corner of it.
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
        Preset::FlowProcess | Preset::FlowPredefinedProcess | Preset::FlowInternalStorage => {
            // Three shapes that are all the same rectangle. What tells them
            // apart is drawn with the outline rather than the fill, because
            // that is what it is: a line on the box, not a change to its
            // shape. See [`rules_into`].
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::FlowAlternateProcess => {
            rounded_rectangle(&mut path, left, top, right, bottom, width.min(height) / 6.0);
        }
        Preset::FlowDecision | Preset::FlowSort => {
            // The sort is a decision with a line across it, and that line is
            // drawn with the outline too.
            path.move_to(point(mx, top));
            path.line_to(point(right, my));
            path.line_to(point(mx, bottom));
            path.line_to(point(left, my));
            path.close();
        }
        Preset::FlowData => {
            // A parallelogram leaning right: what goes in at one end of a
            // step and comes out of the other.
            let lean = width / 5.0;
            path.move_to(point(left + lean, top));
            path.line_to(point(right, top));
            path.line_to(point(right - lean, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::FlowDocument => {
            // A sheet of paper: square at the top and a wave along the bottom.
            let drop = height * 0.12;
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(right, bottom - drop));
            sheet_wave(&mut path, left, right, bottom - drop, drop);
            path.close();
        }
        Preset::FlowMultidocument => {
            // Three sheets, each up and to the right of the one in front of
            // it. This is the silhouette of the three; what shows the near
            // edges of the two behind is drawn with the outline, because a
            // sheet in front hides all of the one behind it but its edge.
            let (step_x, step_y) = (width * 0.07, height * 0.11);
            let drop = height * 0.1;
            path.move_to(point(left, top + step_y * 2.0));
            path.line_to(point(left + step_x, top + step_y * 2.0));
            path.line_to(point(left + step_x, top + step_y));
            path.line_to(point(left + step_x * 2.0, top + step_y));
            path.line_to(point(left + step_x * 2.0, top));
            path.line_to(point(right, top));
            // Down the back sheet and in a step at a time to the front one.
            // The sheets behind show a corner rather than a wave: the wave of
            // each is behind the sheet in front of it but for the very end.
            path.line_to(point(right, bottom - step_y * 2.0));
            path.line_to(point(right - step_x, bottom - step_y * 2.0));
            path.line_to(point(right - step_x, bottom - step_y));
            path.line_to(point(right - step_x * 2.0, bottom - step_y));
            path.line_to(point(right - step_x * 2.0, bottom - drop));
            sheet_wave(&mut path, left, right - step_x * 2.0, bottom - drop, drop);
            path.close();
        }
        Preset::FlowTerminator => {
            // Where a chart starts and stops: a box with both ends taken
            // fully round. The ends are a sixth of the width across, as the
            // format draws them, so a wide one keeps its straight sides.
            let reach = (width * 0.1609).min(width / 2.0);
            path.move_to(point(left + reach, top));
            path.line_to(point(right - reach, top));
            arc_into(&mut path, right - reach, my, reach, height / 2.0, -FRAC_PI_2, FRAC_PI_2);
            path.line_to(point(left + reach, bottom));
            arc_into(&mut path, left + reach, my, reach, height / 2.0, FRAC_PI_2, PI * 1.5);
            path.close();
        }
        Preset::FlowPreparation => {
            let reach = width / 5.0;
            path.move_to(point(left + reach, top));
            path.line_to(point(right - reach, top));
            path.line_to(point(right, my));
            path.line_to(point(right - reach, bottom));
            path.line_to(point(left + reach, bottom));
            path.line_to(point(left, my));
            path.close();
        }
        Preset::FlowManualInput => {
            // The top slopes up to the right: the sheet a person feeds in.
            path.move_to(point(left, top + height / 5.0));
            path.line_to(point(right, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::FlowManualOperation => {
            // A trapezoid narrowing downwards, which is the one thing that
            // tells it from the preparation above it in the gallery.
            let narrow = width / 5.0;
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(right - narrow, bottom));
            path.line_to(point(left + narrow, bottom));
            path.close();
        }
        Preset::FlowConnector | Preset::FlowSummingJunction | Preset::FlowOr => {
            // Three circles. The summing junction has a cross through it the
            // way a saltire lies and the "or" has one the way a plus does,
            // and both are drawn with the outline.
            ellipse(&mut path, mx, my, width / 2.0, height / 2.0);
        }
        Preset::FlowOffpageConnector => {
            // A box coming to a point at the bottom: what carries the chart
            // onto another page.
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(right, bottom - height / 5.0));
            path.line_to(point(mx, bottom));
            path.line_to(point(left, bottom - height / 5.0));
            path.close();
        }
        Preset::FlowCard => {
            // The punched card, with the corner cut off that says which way
            // round it goes in.
            path.move_to(point(left + width / 5.0, top));
            path.line_to(point(right, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.line_to(point(left, top + height / 5.0));
            path.close();
        }
        Preset::FlowPunchedTape => {
            // A strip of tape: the same wave along the top and the bottom, so
            // the strip is the same width all the way along. Mirroring the top
            // to get the bottom would give a shape that bulges in the middle,
            // and tape does not.
            let drop = height * 0.2;
            path.move_to(point(left, top + drop / 2.0));
            path.cubic_to(
                point(left + width * 0.25, top),
                point(left + width * 0.75, top + drop),
                point(right, top + drop / 2.0),
            );
            path.line_to(point(right, bottom - drop / 2.0));
            path.cubic_to(
                point(left + width * 0.75, bottom),
                point(left + width * 0.25, bottom - drop),
                point(left, bottom - drop / 2.0),
            );
            path.close();
        }
        Preset::FlowCollate => {
            // Two triangles meeting in the middle, base to base: the shape
            // that says a batch is being put in order.
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(mx, my));
            path.close();
            path.move_to(point(mx, my));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::FlowExtract => {
            path.move_to(point(mx, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::FlowMerge => {
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(mx, bottom));
            path.close();
        }
        Preset::FlowStoredData => {
            // Both ends curve the same way: the right one bulges out and the
            // left one bites in by as much, which is a slice of a cylinder
            // seen from the side.
            let reach = width / 6.0;
            path.move_to(point(left, top));
            path.line_to(point(right - reach, top));
            arc_into(&mut path, right - reach, my, reach, height / 2.0, -FRAC_PI_2, FRAC_PI_2);
            path.line_to(point(left, bottom));
            // And up the left side, bulging right by the same reach: the bite
            // that makes this a store rather than a box.
            arc_into(&mut path, left, my, reach, height / 2.0, FRAC_PI_2, -FRAC_PI_2);
            path.close();
        }
        Preset::FlowDelay => {
            // A box with its right half taken round: something waiting.
            path.move_to(point(left, top));
            path.line_to(point(mx, top));
            arc_into(&mut path, mx, my, width / 2.0, height / 2.0, -FRAC_PI_2, FRAC_PI_2);
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::FlowSequentialStorage => {
            // A reel of tape: the disc, and the tape leaving it along the
            // bottom towards the right.
            let tail = height * 0.12;
            let (rx, ry) = (width / 2.0, (height - tail) / 2.0);
            let (cx, cy) = (mx, top + ry);
            // Round the disc from its lowest point, the long way about, to
            // where the tape leaves it.
            path.move_to(point(cx, cy + ry));
            arc_into(&mut path, cx, cy, rx, ry, FRAC_PI_2, FRAC_PI_2 + TAU - FRAC_PI_4);
            path.line_to(point(right, cy + ry * FRAC_PI_4.sin()));
            path.line_to(point(right, bottom));
            path.line_to(point(cx, bottom));
            path.close();
        }
        Preset::FlowMagneticDisk => {
            // A cylinder standing up. The near side of its lid is inside the
            // shape and is drawn with the outline.
            let lid = height / 8.0;
            path.move_to(point(left, top + lid));
            arc_into(&mut path, mx, top + lid, width / 2.0, lid, PI, TAU);
            path.line_to(point(right, bottom - lid));
            arc_into(&mut path, mx, bottom - lid, width / 2.0, lid, 0.0, PI);
            path.close();
        }
        Preset::FlowDirectStorage => {
            // The same cylinder lying on its side, so the arc inside it is the
            // near end rather than the lid.
            let end = width / 8.0;
            path.move_to(point(left + end, top));
            path.line_to(point(right - end, top));
            arc_into(&mut path, right - end, my, end, height / 2.0, -FRAC_PI_2, FRAC_PI_2);
            path.line_to(point(left + end, bottom));
            arc_into(&mut path, left + end, my, end, height / 2.0, FRAC_PI_2, PI * 1.5);
            path.close();
        }
        Preset::FlowDisplay => {
            // A point at the left and a round end at the right: what is shown
            // to a person rather than stored.
            let reach = width / 6.0;
            path.move_to(point(left, my));
            path.line_to(point(left + reach, top));
            path.line_to(point(right - reach, top));
            arc_into(&mut path, right - reach, my, reach, height / 2.0, -FRAC_PI_2, FRAC_PI_2);
            path.line_to(point(left + reach, bottom));
            path.close();
        }
        _ => return None,
    }
    Some(path)
}

/// The lines drawn inside a flowchart shape, with the shape's own outline.
///
/// Nothing is added for a shape that has none, which is most of them. The
/// weight is the outline's, because these *are* the outline: a shape drawn
/// with no line has no rules inside it either, which is what Word does too.
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
    let (mx, my) = (left + width / 2.0, top + height / 2.0);

    match preset {
        Preset::FlowPredefinedProcess => {
            // A rule down each end, an eighth of the way in: the two sides of
            // a step someone has already written down elsewhere.
            for across in [left + width / 8.0, right - width / 8.0] {
                rule(path, point(across, top), point(across, bottom), weight);
            }
        }
        Preset::FlowInternalStorage => {
            // One rule down and one across, both an eighth in, which is the
            // corner this shape is known by.
            rule(path, point(left + width / 8.0, top), point(left + width / 8.0, bottom), weight);
            rule(path, point(left, top + height / 8.0), point(right, top + height / 8.0), weight);
        }
        Preset::FlowSummingJunction => {
            // A saltire with its ends on the circle rather than in the corners
            // of the box: the shape is the circle, and the cross belongs to it.
            let (rx, ry) = (width / 2.0 * FRAC_PI_4.cos(), height / 2.0 * FRAC_PI_4.sin());
            rule(path, point(mx - rx, my - ry), point(mx + rx, my + ry), weight);
            rule(path, point(mx - rx, my + ry), point(mx + rx, my - ry), weight);
        }
        Preset::FlowOr => {
            rule(path, point(left, my), point(right, my), weight);
            rule(path, point(mx, top), point(mx, bottom), weight);
        }
        Preset::FlowSort => {
            // Across the middle, from one point of the diamond to the other:
            // what tells a sort from a decision.
            rule(path, point(left, my), point(right, my), weight);
        }
        Preset::FlowMagneticDisk => {
            // The near side of the lid, which is the arc that makes the shape
            // a cylinder rather than a tube.
            let lid = height / 8.0;
            arc_rule(path, mx, top + lid, width / 2.0, lid, 0.0, PI, weight);
        }
        Preset::FlowDirectStorage => {
            // The same for the cylinder on its side: its near end.
            let end = width / 8.0;
            arc_rule(path, left + end, my, end, height / 2.0, -FRAC_PI_2, FRAC_PI_2, weight);
        }
        Preset::FlowMultidocument => {
            // The near edges of the two sheets behind the front one: the top
            // of each and its right-hand side, from where the silhouette
            // leaves that sheet to where it picks it up again. The rest of
            // every sheet but the front is hidden, so these four lines are the
            // whole of what says there are three.
            let (step_x, step_y) = (width * 0.07, height * 0.11);
            for sheet in [1.0_f32, 2.0] {
                let down = top + step_y * (3.0 - sheet);
                let across = right - step_x * (3.0 - sheet);
                rule(path, point(left + step_x * sheet, down), point(across, down), weight);
                rule(path, point(across, down), point(across, bottom - step_y * sheet), weight);
            }
        }
        _ => {}
    }
}

/// A line inside a shape, given a width.
///
/// Wound the way the shapes here are wound, so that it adds to what they fill
/// rather than cancelling it: by the nonzero rule a contour wound against its
/// neighbour leaves a hole, and a rule that punched a hole where it met the
/// outline would draw a gap across it instead of a line.
fn rule(path: &mut Path, from: Point, to: Point, weight: f32) {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = dx.hypot(dy);
    if length <= 0.0 {
        return;
    }
    // The line's own direction turned a quarter, scaled to half the weight.
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

/// The wave along the bottom of a sheet of paper, drawn right to left.
///
/// One S: below the line at the right-hand end of it and above the line at the
/// left. The path is standing at the right-hand end already, and `middle` is
/// the height both ends sit at.
fn sheet_wave(path: &mut Path, left: f32, right: f32, middle: f32, drop: f32) {
    let width = right - left;
    path.cubic_to(
        Point::new(right - width * 0.25, middle + drop),
        Point::new(left + width * 0.25, middle - drop),
        Point::new(left, middle),
    );
}

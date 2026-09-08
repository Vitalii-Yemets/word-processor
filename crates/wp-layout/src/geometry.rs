//! The outlines of the shapes a document can hold.
//!
//! # Why the shapes are named and not drawn
//!
//! A document does not carry the outline of a star. It carries the word
//! `star5` and a box to fit it in — a *preset geometry* — and every program
//! that opens the document is expected to know what that means. There are about
//! 180 of them in the format.
//!
//! So this is that knowledge: the ones a person actually reaches for, each
//! turned into a path inside whatever box it was given. A preset this does not
//! know is drawn as the rectangle it occupies, which is wrong but is visibly a
//! shape in the right place and the right size, rather than nothing at all.
//!
//! # How an outline is drawn without a stroker
//!
//! By filling a ring. The outer edge is the shape; the inner edge is the same
//! shape a little smaller, wound the other way round. Filled by the nonzero
//! rule, the two together leave a band — which is the outline. It costs nothing
//! and needs no stroking algorithm, and for the shapes here it is exact where
//! the edges are straight and close where they curve.

use wp_raster::{Command, Path, Point};

/// How closely a curve is followed. Four segments to the quarter is smooth at
/// any size a shape is drawn on a page.
const ARC_STEPS: usize = 8;

/// The shapes this program can draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preset {
    #[default]
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Triangle,
    RightTriangle,
    Diamond,
    Pentagon,
    Hexagon,
    Star,
    /// An arrow pointing right, which is the one an arrow usually is.
    Arrow,
    /// A straight line from one corner of the box to the other.
    Line,
    /// A speech bubble.
    Callout,
}

impl Preset {
    /// The name the format knows it by.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Rectangle => "rect",
            Self::RoundedRectangle => "roundRect",
            Self::Ellipse => "ellipse",
            Self::Triangle => "triangle",
            Self::RightTriangle => "rtTriangle",
            Self::Diamond => "diamond",
            Self::Pentagon => "pentagon",
            Self::Hexagon => "hexagon",
            Self::Star => "star5",
            Self::Arrow => "rightArrow",
            Self::Line => "line",
            Self::Callout => "wedgeRectCallout",
        }
    }

    /// Reads one back out of a document.
    ///
    /// A preset this does not know becomes a rectangle: the shape is then the
    /// wrong shape but the right size in the right place, which is a better
    /// showing of the document than leaving it out.
    #[must_use]
    pub fn from_word(name: &str) -> Self {
        Self::ALL.iter().copied().find(|preset| preset.word() == name).unwrap_or(Self::Rectangle)
    }

    /// What a person is shown when picking one.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::RoundedRectangle => "Rounded Rectangle",
            Self::Ellipse => "Oval",
            Self::Triangle => "Isosceles Triangle",
            Self::RightTriangle => "Right Triangle",
            Self::Diamond => "Diamond",
            Self::Pentagon => "Pentagon",
            Self::Hexagon => "Hexagon",
            Self::Star => "5-Point Star",
            Self::Arrow => "Right Arrow",
            Self::Line => "Line",
            Self::Callout => "Speech Bubble",
        }
    }

    /// Whether the shape encloses an area that text could sit in.
    ///
    /// A line does not, which is why a line never carries text and never has a
    /// fill.
    #[must_use]
    pub fn is_closed(self) -> bool {
        self != Self::Line
    }

    /// Every preset that can be picked.
    pub const ALL: &'static [Self] = &[
        Self::Rectangle,
        Self::RoundedRectangle,
        Self::Ellipse,
        Self::Triangle,
        Self::RightTriangle,
        Self::Diamond,
        Self::Pentagon,
        Self::Hexagon,
        Self::Star,
        Self::Arrow,
        Self::Line,
        Self::Callout,
    ];
}

/// The outline of a shape, inside the box given.
///
/// `x` and `y` are the top left corner and the box grows right and down, which
/// is how a canvas is measured.
#[must_use]
pub fn path_in(preset: Preset, x: f32, y: f32, width: f32, height: f32) -> Path {
    let mut path = Path::new();
    if width <= 0.0 || height <= 0.0 {
        return path;
    }
    let (left, top, right, bottom) = (x, y, x + width, y + height);
    let point = |px: f32, py: f32| Point::new(px, py);

    match preset {
        Preset::Rectangle | Preset::Callout => {
            path.move_to(point(left, top));
            path.line_to(point(right, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
            if preset == Preset::Callout {
                // The tail, hanging from the lower left of the bubble. Word
                // puts it wherever the shape's adjustment says; this puts it
                // where a bubble usually has one.
                let tip_x = left + width * 0.20;
                path.move_to(point(left + width * 0.18, bottom));
                path.line_to(point(left + width * 0.34, bottom));
                path.line_to(point(tip_x, bottom + height * 0.22));
                path.close();
            }
        }
        Preset::RoundedRectangle => {
            // A sixth of the shorter side, which is the corner Word rounds by
            // default.
            let radius = (width.min(height) / 6.0).min(width / 2.0).min(height / 2.0);
            rounded_rectangle(&mut path, left, top, right, bottom, radius);
        }
        Preset::Ellipse => {
            ellipse(
                &mut path,
                (left + right) / 2.0,
                (top + bottom) / 2.0,
                width / 2.0,
                height / 2.0,
            );
        }
        Preset::Triangle => {
            path.move_to(point((left + right) / 2.0, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::RightTriangle => {
            path.move_to(point(left, top));
            path.line_to(point(right, bottom));
            path.line_to(point(left, bottom));
            path.close();
        }
        Preset::Diamond => {
            path.move_to(point((left + right) / 2.0, top));
            path.line_to(point(right, (top + bottom) / 2.0));
            path.line_to(point((left + right) / 2.0, bottom));
            path.line_to(point(left, (top + bottom) / 2.0));
            path.close();
        }
        Preset::Pentagon => regular(&mut path, left, top, width, height, 5),
        Preset::Hexagon => regular(&mut path, left, top, width, height, 6),
        Preset::Star => star(&mut path, left, top, width, height),
        Preset::Arrow => {
            // The shaft is the middle half of the height and the head takes the
            // last third of the width, which is the arrow Word draws.
            let shaft_top = top + height * 0.25;
            let shaft_bottom = bottom - height * 0.25;
            let neck = left + width * 0.66;
            path.move_to(point(left, shaft_top));
            path.line_to(point(neck, shaft_top));
            path.line_to(point(neck, top));
            path.line_to(point(right, (top + bottom) / 2.0));
            path.line_to(point(neck, bottom));
            path.line_to(point(neck, shaft_bottom));
            path.line_to(point(left, shaft_bottom));
            path.close();
        }
        Preset::Line => {
            path.move_to(point(left, top));
            path.line_to(point(right, bottom));
        }
    }
    path
}

/// The band that draws a shape's outline.
///
/// The shape, and the shape a little smaller wound the other way — filled by
/// the nonzero rule the two leave a ring. A line has no inside, so its outline
/// is drawn as a long thin rectangle along it instead.
#[must_use]
pub fn outline_in(preset: Preset, x: f32, y: f32, width: f32, height: f32, weight: f32) -> Path {
    let weight = weight.max(0.5);
    if preset == Preset::Line {
        return thick_line(x, y, x + width, y + height, weight);
    }

    let mut path = path_in(preset, x, y, width, height);
    // The inside of the band: the same shape, inset by the weight on every
    // side, and reversed so that the nonzero rule leaves a hole rather than
    // filling it in twice.
    let inset_width = (width - weight * 2.0).max(0.0);
    let inset_height = (height - weight * 2.0).max(0.0);
    if inset_width <= 0.0 || inset_height <= 0.0 {
        return path;
    }
    let inner = path_in(preset, x + weight, y + weight, inset_width, inset_height);
    path.extend_reversed(&inner);
    path
}

/// A rectangle drawn along a line, which is how a line is given a width.
fn thick_line(x1: f32, y1: f32, x2: f32, y2: f32, weight: f32) -> Path {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let length = dx.hypot(dy);
    let mut path = Path::new();
    if length <= 0.0 {
        return path;
    }
    // The line's own direction turned a quarter, scaled to half the weight:
    // that is the offset from the middle of the line to either edge of it.
    let (nx, ny) = (-dy / length * weight / 2.0, dx / length * weight / 2.0);
    path.move_to(Point::new(x1 + nx, y1 + ny));
    path.line_to(Point::new(x2 + nx, y2 + ny));
    path.line_to(Point::new(x2 - nx, y2 - ny));
    path.line_to(Point::new(x1 - nx, y1 - ny));
    path.close();
    path
}

/// A rectangle with its corners taken off in quarter circles.
fn rounded_rectangle(path: &mut Path, left: f32, top: f32, right: f32, bottom: f32, radius: f32) {
    let point = Point::new;
    path.move_to(point(left + radius, top));
    path.line_to(point(right - radius, top));
    path.quad_to(point(right, top), point(right, top + radius));
    path.line_to(point(right, bottom - radius));
    path.quad_to(point(right, bottom), point(right - radius, bottom));
    path.line_to(point(left + radius, bottom));
    path.quad_to(point(left, bottom), point(left, bottom - radius));
    path.line_to(point(left, top + radius));
    path.quad_to(point(left, top), point(left + radius, top));
    path.close();
}

/// An ellipse about a centre, drawn as a run of quadratic curves.
fn ellipse(path: &mut Path, cx: f32, cy: f32, rx: f32, ry: f32) {
    let steps = ARC_STEPS * 4;
    let angle_of = |step: usize| step as f32 / steps as f32 * core::f32::consts::TAU;
    let at = |angle: f32| Point::new(cx + rx * angle.cos(), cy + ry * angle.sin());

    path.move_to(at(0.0));
    for step in 1..=steps {
        let previous = angle_of(step - 1);
        let angle = angle_of(step);
        // The control point of a quadratic through two points of a circle sits
        // where the two tangents meet, which is at the half-angle and further
        // out by the secant of it.
        let middle = (previous + angle) / 2.0;
        let reach = 1.0 / ((angle - previous) / 2.0).cos();
        let control = Point::new(cx + rx * reach * middle.cos(), cy + ry * reach * middle.sin());
        path.quad_to(control, at(angle));
    }
    path.close();
}

/// A regular polygon of `sides` sides, filling the box.
///
/// Point upwards, which is how a pentagon and a hexagon are both drawn.
fn regular(path: &mut Path, left: f32, top: f32, width: f32, height: f32, sides: usize) {
    let (cx, cy) = (left + width / 2.0, top + height / 2.0);
    let (rx, ry) = (width / 2.0, height / 2.0);
    let quarter = core::f32::consts::FRAC_PI_2;

    for step in 0..sides {
        let angle = -quarter + step as f32 / sides as f32 * core::f32::consts::TAU;
        let point = Point::new(cx + rx * angle.cos(), cy + ry * angle.sin());
        if step == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close();
}

/// A five-pointed star filling the box.
fn star(path: &mut Path, left: f32, top: f32, width: f32, height: f32) {
    let (cx, cy) = (left + width / 2.0, top + height / 2.0);
    let (rx, ry) = (width / 2.0, height / 2.0);
    // The inner radius of a five-pointed star, which is what makes it that
    // star and not a different one.
    let inner = 0.382;
    let quarter = core::f32::consts::FRAC_PI_2;

    for step in 0..10 {
        let angle = -quarter + step as f32 / 10.0 * core::f32::consts::TAU;
        let reach = if step % 2 == 0 { 1.0 } else { inner };
        let point = Point::new(cx + rx * reach * angle.cos(), cy + ry * reach * angle.sin());
        if step == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close();
}

/// How far across a shape reaches between two heights.
///
/// # Why the text needs this
///
/// Word's Tight and Through wrapping run the text up to the shape itself
/// rather than to the box round it, so a line beside the point of a triangle
/// gets nearly the whole width and one beside its base gets none. Taking the
/// box for both is what makes text wrapped round a circle look wrapped round a
/// square.
///
/// The answer is the leftmost and rightmost the outline reaches anywhere in the
/// band, which is what a line of text in that band has to keep out of. Nothing
/// comes back when the shape does not reach into the band at all.
#[must_use]
pub fn span_between(
    preset: Preset,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    top: f32,
    bottom: f32,
) -> Option<(f32, f32)> {
    if bottom <= top || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let path = path_in(preset, x, y, width, height);

    let mut left = f32::MAX;
    let mut right = f32::MIN;
    let mut note = |point: Point| {
        left = left.min(point.x);
        right = right.max(point.x);
    };

    // The outline is walked as straight steps: a curve is flattened finely
    // enough that the answer is right to within a fraction of a pixel, which is
    // finer than a line of text can be placed anyway.
    let mut start = Point::new(x, y);
    let mut here = start;
    for command in &path.commands {
        match *command {
            Command::MoveTo(point) => {
                start = point;
                here = point;
            }
            Command::LineTo(point) => {
                clip_segment(here, point, top, bottom, &mut note);
                here = point;
            }
            Command::QuadTo(control, point) => {
                let mut previous = here;
                for step in 1..=CURVE_STEPS {
                    let t = step as f32 / CURVE_STEPS as f32;
                    let next = quadratic(here, control, point, t);
                    clip_segment(previous, next, top, bottom, &mut note);
                    previous = next;
                }
                here = point;
            }
            Command::CubicTo(first, second, point) => {
                let mut previous = here;
                for step in 1..=CURVE_STEPS {
                    let t = step as f32 / CURVE_STEPS as f32;
                    let next = cubic(here, first, second, point, t);
                    clip_segment(previous, next, top, bottom, &mut note);
                    previous = next;
                }
                here = point;
            }
            Command::Close => {
                clip_segment(here, start, top, bottom, &mut note);
                here = start;
            }
        }
    }

    (right > left).then_some((left, right))
}

/// How many straight steps a curve is walked in.
const CURVE_STEPS: usize = 16;

/// Notes where a straight step lies inside a band of heights.
fn clip_segment(from: Point, to: Point, top: f32, bottom: f32, note: &mut impl FnMut(Point)) {
    // `high` is the end nearer the top of the page, which is the smaller y.
    let (high, low) = if from.y <= to.y { (from, to) } else { (to, from) };
    // Wholly above the band, or wholly below it: nothing of it is beside the
    // line.
    if low.y < top || high.y > bottom {
        return;
    }

    // Both ends inside the band count as they are; an end outside is replaced
    // by where the step crosses the edge.
    for point in [high, low] {
        if point.y >= top && point.y <= bottom {
            note(point);
        }
    }
    for edge in [top, bottom] {
        if (high.y..=low.y).contains(&edge) && (low.y - high.y).abs() > f32::EPSILON {
            let share = (edge - high.y) / (low.y - high.y);
            note(Point::new(high.x + (low.x - high.x) * share, edge));
        }
    }
}

/// A point along a quadratic curve.
fn quadratic(from: Point, control: Point, to: Point, t: f32) -> Point {
    let inverse = 1.0 - t;
    Point::new(
        inverse * inverse * from.x + 2.0 * inverse * t * control.x + t * t * to.x,
        inverse * inverse * from.y + 2.0 * inverse * t * control.y + t * t * to.y,
    )
}

/// And along a cubic one.
fn cubic(from: Point, first: Point, second: Point, to: Point, t: f32) -> Point {
    let inverse = 1.0 - t;
    let (a, b, c, d) = (
        inverse * inverse * inverse,
        3.0 * inverse * inverse * t,
        3.0 * inverse * t * t,
        t * t * t,
    );
    Point::new(
        a * from.x + b * first.x + c * second.x + d * to.x,
        a * from.y + b * first.y + c * second.y + d * to.y,
    )
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_reaches_right_across_at_every_height() {
        let span = span_between(Preset::Rectangle, 10.0, 20.0, 100.0, 50.0, 30.0, 40.0);
        let (left, right) = span.expect("a rectangle covers the band");
        assert!((left - 10.0).abs() < 0.5, "{left}");
        assert!((right - 110.0).abs() < 0.5, "{right}");
    }

    #[test]
    fn a_band_above_the_shape_is_not_beside_it() {
        assert_eq!(span_between(Preset::Rectangle, 10.0, 20.0, 100.0, 50.0, 0.0, 10.0), None);
    }

    #[test]
    fn a_band_below_the_shape_is_not_beside_it_either() {
        assert_eq!(span_between(Preset::Rectangle, 10.0, 20.0, 100.0, 50.0, 100.0, 120.0), None);
    }

    #[test]
    fn a_triangle_is_narrow_at_its_point_and_wide_at_its_base() {
        let point =
            span_between(Preset::Triangle, 0.0, 0.0, 100.0, 100.0, 0.0, 10.0).expect("the point");
        let base =
            span_between(Preset::Triangle, 0.0, 0.0, 100.0, 100.0, 90.0, 100.0).expect("the base");
        let width = |(left, right): (f32, f32)| right - left;
        assert!(width(point) < width(base) / 2.0, "{point:?} against {base:?}");
    }

    #[test]
    fn a_circle_is_widest_across_its_middle() {
        let middle =
            span_between(Preset::Ellipse, 0.0, 0.0, 100.0, 100.0, 45.0, 55.0).expect("the middle");
        let top =
            span_between(Preset::Ellipse, 0.0, 0.0, 100.0, 100.0, 0.0, 10.0).expect("the top");
        let width = |(left, right): (f32, f32)| right - left;
        assert!(width(middle) > width(top), "{middle:?} against {top:?}");
        // And as wide as the box it was given, near enough.
        assert!((width(middle) - 100.0).abs() < 2.0, "{middle:?}");
    }

    #[test]
    fn a_shape_never_reaches_outside_the_box_it_was_given() {
        for preset in Preset::ALL {
            if *preset == Preset::Callout {
                continue;
            }
            for band in 0..10 {
                let top = band as f32 * 10.0;
                let Some((left, right)) =
                    span_between(*preset, 0.0, 0.0, 100.0, 100.0, top, top + 10.0)
                else {
                    continue;
                };
                assert!(left >= -0.5, "{} reaches to {left}", preset.word());
                assert!(right <= 100.5, "{} reaches to {right}", preset.word());
            }
        }
    }

    #[test]
    fn a_band_of_no_height_is_no_band() {
        assert_eq!(span_between(Preset::Rectangle, 0.0, 0.0, 100.0, 100.0, 50.0, 50.0), None);
    }

    /// The box a path covers, as left, top, right, bottom.
    fn bounds(path: &Path) -> (f32, f32, f32, f32) {
        let mut bounds = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for point in path.points() {
            bounds.0 = bounds.0.min(point.x);
            bounds.1 = bounds.1.min(point.y);
            bounds.2 = bounds.2.max(point.x);
            bounds.3 = bounds.3.max(point.y);
        }
        bounds
    }

    #[test]
    fn every_preset_survives_being_written_and_read_back() {
        for preset in Preset::ALL {
            assert_eq!(Preset::from_word(preset.word()), *preset);
        }
    }

    #[test]
    fn a_preset_nobody_knows_becomes_a_rectangle() {
        assert_eq!(Preset::from_word("cloudCallout"), Preset::Rectangle);
    }

    #[test]
    fn every_shape_stays_inside_the_box_it_was_given() {
        for preset in Preset::ALL {
            // The bubble's tail hangs below the box on purpose, which is what a
            // tail is.
            if *preset == Preset::Callout {
                continue;
            }
            let path = path_in(*preset, 10.0, 20.0, 100.0, 60.0);
            let (left, top, right, bottom) = bounds(&path);
            assert!(left >= 9.9, "{} starts at {left}", preset.label());
            assert!(top >= 19.9, "{} starts at {top}", preset.label());
            assert!(right <= 110.1, "{} reaches {right}", preset.label());
            assert!(bottom <= 80.1, "{} reaches {bottom}", preset.label());
        }
    }

    #[test]
    fn every_shape_fills_the_box_it_was_given() {
        for preset in Preset::ALL {
            let path = path_in(*preset, 0.0, 0.0, 100.0, 60.0);
            let (left, top, right, bottom) = bounds(&path);
            assert!(right - left > 50.0, "{} is only {} wide", preset.label(), right - left);
            assert!(bottom - top > 30.0, "{} is only {} tall", preset.label(), bottom - top);
        }
    }

    #[test]
    fn a_box_of_no_size_draws_nothing() {
        for preset in Preset::ALL {
            assert!(path_in(*preset, 0.0, 0.0, 0.0, 50.0).points().next().is_none());
            assert!(path_in(*preset, 0.0, 0.0, 50.0, -1.0).points().next().is_none());
        }
    }

    #[test]
    fn an_outline_is_a_ring_and_so_has_more_in_it_than_the_shape() {
        for preset in Preset::ALL {
            if *preset == Preset::Line {
                continue;
            }
            let shape = path_in(*preset, 0.0, 0.0, 100.0, 60.0);
            let outline = outline_in(*preset, 0.0, 0.0, 100.0, 60.0, 2.0);
            assert!(
                outline.points().count() > shape.points().count(),
                "{} has no inner edge",
                preset.label()
            );
        }
    }

    #[test]
    fn an_outline_thicker_than_the_shape_is_solid_rather_than_empty() {
        let outline = outline_in(Preset::Rectangle, 0.0, 0.0, 4.0, 4.0, 10.0);
        assert!(outline.points().next().is_some(), "a very thick outline should still draw");
    }

    #[test]
    fn a_line_is_drawn_as_a_band_along_itself() {
        let outline = outline_in(Preset::Line, 0.0, 0.0, 100.0, 0.0, 4.0);
        let (_, top, _, bottom) = bounds(&outline);
        assert!((bottom - top - 4.0).abs() < 0.01, "the band is {} thick", bottom - top);
    }

    #[test]
    fn only_a_line_is_open() {
        for preset in Preset::ALL {
            assert_eq!(preset.is_closed(), *preset != Preset::Line, "{}", preset.label());
        }
    }

    #[test]
    fn an_outline_really_does_leave_a_hole_when_it_is_filled() {
        use wp_raster::{Canvas, Color};

        let mut canvas = Canvas::filled(60, 60, Color::WHITE);
        let outline = outline_in(Preset::Rectangle, 10.0, 10.0, 40.0, 40.0, 4.0);
        canvas.fill_path(&outline, Color::BLACK);

        // On the band, and inside it where the hole is.
        assert_eq!(canvas.pixel(11, 30), Color::BLACK, "the band was not drawn");
        assert_eq!(canvas.pixel(30, 30), Color::WHITE, "the ring was filled in solid");
    }

    #[test]
    fn an_ellipse_reaches_the_middle_of_every_side() {
        let path = path_in(Preset::Ellipse, 0.0, 0.0, 100.0, 60.0);
        let (left, top, right, bottom) = bounds(&path);
        assert!((left - 0.0).abs() < 0.5 && (right - 100.0).abs() < 0.5, "{left} to {right}");
        assert!((top - 0.0).abs() < 0.5 && (bottom - 60.0).abs() < 0.5, "{top} to {bottom}");
    }
}

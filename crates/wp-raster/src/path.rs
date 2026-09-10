//! Geometry: points, transforms and paths.
//!
//! Deliberately independent of where a path came from. A glyph outline, a table
//! border and a shape from a drawing all become the same thing before they are
//! filled, so the rasterizer needs to know about only one of them.

/// A point in device space, measured in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub fn midpoint(self, other: Self) -> Self {
        Self { x: (self.x + other.x) / 2.0, y: (self.y + other.y) / 2.0 }
    }
}

/// An affine transform, stored as the six values that matter.
///
/// Laid out so that `x' = a·x + c·y + e` and `y' = b·x + d·y + f`, which is the
/// order PostScript, PDF and every graphics API since have used.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Transform {
    /// The transform that changes nothing.
    pub const IDENTITY: Self = Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    #[must_use]
    pub const fn translate(x: f32, y: f32) -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: x, f: y }
    }

    #[must_use]
    pub const fn scale(x: f32, y: f32) -> Self {
        Self { a: x, b: 0.0, c: 0.0, d: y, e: 0.0, f: 0.0 }
    }

    /// A turn about the origin, anticlockwise on a y-up grid and clockwise on
    /// a canvas, which is the same turn seen from the other side.
    #[must_use]
    pub fn rotate(radians: f32) -> Self {
        let (sin, cos) = (radians.sin(), radians.cos());
        Self { a: cos, b: sin, c: -sin, d: cos, e: 0.0, f: 0.0 }
    }

    /// A turn about a point rather than about the origin.
    #[must_use]
    pub fn rotate_about(radians: f32, x: f32, y: f32) -> Self {
        Self::translate(-x, -y).then(&Self::rotate(radians)).then(&Self::translate(x, y))
    }

    /// A transform placing a glyph on the page.
    ///
    /// Font outlines are in a y-up design grid; a raster canvas is y-down. The
    /// negative vertical scale is what turns one into the other, and forgetting
    /// it draws every letter upside down.
    #[must_use]
    pub const fn glyph(scale: f32, origin_x: f32, baseline_y: f32) -> Self {
        Self::stretched_glyph(scale, 1.0, origin_x, baseline_y)
    }

    /// The same, drawn wider or narrower than it is tall.
    ///
    /// Word's Scale: a letter at 150 per cent is a wide letter, not a normal
    /// letter with a gap after it, so the outline is stretched rather than only
    /// the room it takes up.
    #[must_use]
    pub const fn stretched_glyph(scale: f32, stretch: f32, origin_x: f32, baseline_y: f32) -> Self {
        Self { a: scale * stretch, b: 0.0, c: 0.0, d: -scale, e: origin_x, f: baseline_y }
    }

    #[must_use]
    pub fn apply(&self, point: Point) -> Point {
        Point {
            x: self.a * point.x + self.c * point.y + self.e,
            y: self.b * point.x + self.d * point.y + self.f,
        }
    }

    /// This transform followed by `other`.
    #[must_use]
    pub fn then(&self, other: &Self) -> Self {
        Self {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// One step in describing a shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    MoveTo(Point),
    LineTo(Point),
    /// A quadratic curve, as TrueType outlines use.
    QuadTo(Point, Point),
    /// A cubic curve, as PostScript outlines and drawings use.
    CubicTo(Point, Point, Point),
    /// Closes the current contour back to where it began.
    Close,
}

/// A shape to fill.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub commands: Vec<Command>,
}

impl Path {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn move_to(&mut self, point: Point) -> &mut Self {
        self.commands.push(Command::MoveTo(point));
        self
    }

    pub fn line_to(&mut self, point: Point) -> &mut Self {
        self.commands.push(Command::LineTo(point));
        self
    }

    pub fn quad_to(&mut self, control: Point, point: Point) -> &mut Self {
        self.commands.push(Command::QuadTo(control, point));
        self
    }

    pub fn cubic_to(&mut self, first: Point, second: Point, point: Point) -> &mut Self {
        self.commands.push(Command::CubicTo(first, second, point));
        self
    }

    pub fn close(&mut self) -> &mut Self {
        self.commands.push(Command::Close);
        self
    }

    /// A rectangle, wound clockwise.
    #[must_use]
    pub fn rectangle(x: f32, y: f32, width: f32, height: f32) -> Self {
        let mut path = Self::new();
        path.move_to(Point::new(x, y))
            .line_to(Point::new(x + width, y))
            .line_to(Point::new(x + width, y + height))
            .line_to(Point::new(x, y + height))
            .close();
        path
    }

    /// Every point the path names, control points and all.
    ///
    /// Enough to bound a path: a curve never reaches outside the box holding
    /// its ends and its controls, so a box round these contains the shape.
    pub fn points(&self) -> impl Iterator<Item = Point> + '_ {
        self.commands.iter().flat_map(|command| match command {
            Command::MoveTo(point) | Command::LineTo(point) => vec![*point],
            Command::QuadTo(control, point) => vec![*control, *point],
            Command::CubicTo(first, second, point) => vec![*first, *second, *point],
            Command::Close => Vec::new(),
        })
    }

    /// Adds another path to this one, wound the other way round.
    ///
    /// What makes a hole. Filling uses the nonzero rule, under which a contour
    /// inside another one wound the same way is filled and one wound the other
    /// way is not — so this is how a ring, a letter O, or the outline of a
    /// shape is drawn without any stroking at all.
    pub fn extend_reversed(&mut self, other: &Self) -> &mut Self {
        // Walked backwards, each command's end point becomes the previous
        // command's start, so the points are collected first and the commands
        // rebuilt from them.
        let mut contours: Vec<Vec<Command>> = Vec::new();
        for command in &other.commands {
            if matches!(command, Command::MoveTo(_)) || contours.is_empty() {
                contours.push(Vec::new());
            }
            if let Some(last) = contours.last_mut() {
                last.push(*command);
            }
        }

        for contour in contours {
            let mut points: Vec<Point> = Vec::new();
            let mut controls: Vec<Vec<Point>> = Vec::new();
            for command in &contour {
                match command {
                    Command::MoveTo(point) | Command::LineTo(point) => {
                        points.push(*point);
                        controls.push(Vec::new());
                    }
                    Command::QuadTo(control, point) => {
                        points.push(*point);
                        controls.push(vec![*control]);
                    }
                    Command::CubicTo(first, second, point) => {
                        points.push(*point);
                        controls.push(vec![*second, *first]);
                    }
                    Command::Close => {}
                }
            }
            if points.is_empty() {
                continue;
            }

            self.move_to(points[points.len() - 1]);
            for at in (1..points.len()).rev() {
                match controls[at].as_slice() {
                    [control] => {
                        self.quad_to(*control, points[at - 1]);
                    }
                    [second, first] => {
                        self.cubic_to(*second, *first, points[at - 1]);
                    }
                    _ => {
                        self.line_to(points[at - 1]);
                    }
                }
            }
            self.close();
        }
        self
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The same path with a transform applied to every point.
    #[must_use]
    pub fn transformed(&self, transform: &Transform) -> Self {
        let commands = self
            .commands
            .iter()
            .map(|command| match command {
                Command::MoveTo(point) => Command::MoveTo(transform.apply(*point)),
                Command::LineTo(point) => Command::LineTo(transform.apply(*point)),
                Command::QuadTo(control, point) => {
                    Command::QuadTo(transform.apply(*control), transform.apply(*point))
                }
                Command::CubicTo(first, second, point) => Command::CubicTo(
                    transform.apply(*first),
                    transform.apply(*second),
                    transform.apply(*point),
                ),
                Command::Close => Command::Close,
            })
            .collect();
        Self { commands }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_changes_nothing() {
        let point = Point::new(3.0, 4.0);
        assert_eq!(Transform::IDENTITY.apply(point), point);
    }

    #[test]
    fn a_glyph_transform_flips_the_vertical_axis() {
        // Font outlines are y-up, the canvas is y-down. Getting this wrong draws
        // every letter upside down.
        let transform = Transform::glyph(2.0, 100.0, 50.0);
        let baseline = transform.apply(Point::new(0.0, 0.0));
        let above = transform.apply(Point::new(0.0, 10.0));

        assert_eq!(baseline, Point::new(100.0, 50.0));
        assert!(above.y < baseline.y, "a point above the baseline draws higher up");
    }

    #[test]
    fn transforms_compose_in_the_order_written() {
        let scale_then_move = Transform::scale(2.0, 2.0).then(&Transform::translate(10.0, 0.0));
        assert_eq!(scale_then_move.apply(Point::new(1.0, 0.0)), Point::new(12.0, 0.0));

        let move_then_scale = Transform::translate(10.0, 0.0).then(&Transform::scale(2.0, 2.0));
        assert_eq!(move_then_scale.apply(Point::new(1.0, 0.0)), Point::new(22.0, 0.0));
    }

    #[test]
    fn a_rectangle_is_a_closed_contour() {
        let path = Path::rectangle(1.0, 2.0, 10.0, 20.0);
        assert_eq!(path.commands.len(), 5);
        assert_eq!(path.commands[0], Command::MoveTo(Point::new(1.0, 2.0)));
        assert_eq!(path.commands[4], Command::Close);
    }
}

#[cfg(test)]
mod rotation_tests {
    use super::{Point, Transform};

    /// Close enough for a rotation, which cannot land exactly on a float.
    fn near(left: Point, right: Point) {
        assert!(
            (left.x - right.x).abs() < 0.001 && (left.y - right.y).abs() < 0.001,
            "{left:?} is not {right:?}"
        );
    }

    #[test]
    fn a_quarter_turn_takes_the_x_axis_to_the_y_axis() {
        let quarter = Transform::rotate(core::f32::consts::FRAC_PI_2);
        near(quarter.apply(Point::new(1.0, 0.0)), Point::new(0.0, 1.0));
        near(quarter.apply(Point::new(0.0, 1.0)), Point::new(-1.0, 0.0));
    }

    #[test]
    fn turning_about_a_point_leaves_that_point_where_it_was() {
        let turn = Transform::rotate_about(0.7, 40.0, 25.0);
        near(turn.apply(Point::new(40.0, 25.0)), Point::new(40.0, 25.0));
    }

    #[test]
    fn a_full_turn_changes_nothing() {
        let full = Transform::rotate(core::f32::consts::TAU);
        near(full.apply(Point::new(3.0, -7.0)), Point::new(3.0, -7.0));
    }
}

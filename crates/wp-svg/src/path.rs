//! The `d` attribute of an SVG path, turned into an outline.
//!
//! # What this has to get right
//!
//! The grammar is small but full of traps, and every one of them appears in
//! real files:
//!
//! * Numbers run together without separators — `M0 0L1-1` is three numbers,
//!   and `1.5.5` is two.
//! * A command letter is repeated implicitly. `L 1 1 2 2` is two line
//!   segments, and after a `moveto` the repeat is a `lineto`, not another
//!   `moveto`.
//! * The smooth curves `S` and `T` reflect the previous control point, and
//!   only when the previous command was of the matching kind.
//! * `Z` returns to the start of the *current subpath*, not to the origin, and
//!   a command after it starts again from that same point.
//!
//! Getting any of those wrong makes a shape that is almost right, which is
//! worse than one that is obviously wrong.

use wp_raster::{Path, Point};

/// Why a path could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A letter that is not an SVG path command.
    UnknownCommand(char),
    /// A command ran out of numbers before it had all of them.
    Truncated(char),
    /// Something that should have been a number was not.
    NotANumber(String),
    /// The data began with something other than a `moveto`.
    NoInitialMove,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownCommand(letter) => write!(f, "{letter:?} is not a path command"),
            Self::Truncated(letter) => write!(f, "command {letter:?} is missing numbers"),
            Self::NotANumber(text) => write!(f, "{text:?} is not a number"),
            Self::NoInitialMove => f.write_str("path data does not start with a move"),
        }
    }
}

impl std::error::Error for Error {}

/// Reads SVG path data into an outline.
///
/// The result is in the coordinates the data was written in; scaling it to
/// wherever it is going is the caller's business.
pub fn parse(data: &str) -> Result<Path, Error> {
    Parser::new(data).run()
}

/// How much a cubic is allowed to stray from the straight line before it is
/// split. The rasterizer flattens curves itself, so these are handed to it
/// whole — this is only used for the arc, which has no curve command of its
/// own and is approximated by cubics.
const ARC_SEGMENT_LIMIT: f32 = core::f32::consts::FRAC_PI_2;

struct Parser<'a> {
    rest: &'a str,
    path: Path,
    /// Where the pen is.
    current: Point,
    /// Where the current subpath began, which is where `Z` returns to.
    subpath_start: Point,
    /// The last curve's second control point, for the smooth commands.
    last_cubic_control: Option<Point>,
    last_quad_control: Option<Point>,
    started: bool,
}

impl<'a> Parser<'a> {
    fn new(data: &'a str) -> Self {
        Self {
            rest: data,
            path: Path::new(),
            current: Point::new(0.0, 0.0),
            subpath_start: Point::new(0.0, 0.0),
            last_cubic_control: None,
            last_quad_control: None,
            started: false,
        }
    }

    fn run(mut self) -> Result<Path, Error> {
        self.skip_separators();
        while let Some(letter) = self.take_command() {
            self.command(letter)?;
            self.skip_separators();
        }
        Ok(self.path)
    }

    /// One command letter and every set of numbers that follows it.
    fn command(&mut self, letter: char) -> Result<(), Error> {
        let relative = letter.is_ascii_lowercase();
        let kind = letter.to_ascii_uppercase();

        if kind != 'M' && !self.started {
            return Err(Error::NoInitialMove);
        }

        let mut first = true;
        loop {
            match kind {
                'M' => {
                    // Only the first pair moves; the rest are lines, which is
                    // the rule that catches everyone out.
                    let point = self.point(relative)?;
                    if first {
                        self.path.move_to(point);
                        self.subpath_start = point;
                        self.started = true;
                    } else {
                        self.path.line_to(point);
                    }
                    self.current = point;
                    self.forget_controls();
                }
                'L' => {
                    let point = self.point(relative)?;
                    self.path.line_to(point);
                    self.current = point;
                    self.forget_controls();
                }
                'H' => {
                    let x = self.number(kind)?;
                    let point =
                        Point::new(if relative { self.current.x + x } else { x }, self.current.y);
                    self.path.line_to(point);
                    self.current = point;
                    self.forget_controls();
                }
                'V' => {
                    let y = self.number(kind)?;
                    let point =
                        Point::new(self.current.x, if relative { self.current.y + y } else { y });
                    self.path.line_to(point);
                    self.current = point;
                    self.forget_controls();
                }
                'C' => {
                    let first_control = self.point(relative)?;
                    let second_control = self.point(relative)?;
                    let end = self.point(relative)?;
                    self.path.cubic_to(first_control, second_control, end);
                    self.current = end;
                    self.last_cubic_control = Some(second_control);
                    self.last_quad_control = None;
                }
                'S' => {
                    // The first control point is the reflection of the last
                    // one, or the current point when there was no curve before.
                    let first_control = self.reflected_cubic();
                    let second_control = self.point(relative)?;
                    let end = self.point(relative)?;
                    self.path.cubic_to(first_control, second_control, end);
                    self.current = end;
                    self.last_cubic_control = Some(second_control);
                    self.last_quad_control = None;
                }
                'Q' => {
                    let control = self.point(relative)?;
                    let end = self.point(relative)?;
                    self.path.quad_to(control, end);
                    self.current = end;
                    self.last_quad_control = Some(control);
                    self.last_cubic_control = None;
                }
                'T' => {
                    let control = self.reflected_quad();
                    let end = self.point(relative)?;
                    self.path.quad_to(control, end);
                    self.current = end;
                    self.last_quad_control = Some(control);
                    self.last_cubic_control = None;
                }
                'A' => {
                    let radii = self.point(false)?;
                    let rotation = self.number(kind)?;
                    let large = self.flag(kind)?;
                    let sweep = self.flag(kind)?;
                    let end = self.point(relative)?;
                    self.arc(radii, rotation, large, sweep, end);
                    self.current = end;
                    self.forget_controls();
                }
                'Z' => {
                    self.path.close();
                    self.current = self.subpath_start;
                    self.forget_controls();
                    return Ok(());
                }
                other => return Err(Error::UnknownCommand(other)),
            }

            first = false;
            let _ = first;
            self.skip_separators();
            // The command repeats for as long as numbers keep coming.
            if !self.starts_with_number() {
                return Ok(());
            }
        }
    }

    fn forget_controls(&mut self) {
        self.last_cubic_control = None;
        self.last_quad_control = None;
    }

    fn reflected_cubic(&self) -> Point {
        match self.last_cubic_control {
            Some(control) => {
                Point::new(self.current.x * 2.0 - control.x, self.current.y * 2.0 - control.y)
            }
            None => self.current,
        }
    }

    fn reflected_quad(&self) -> Point {
        match self.last_quad_control {
            Some(control) => {
                Point::new(self.current.x * 2.0 - control.x, self.current.y * 2.0 - control.y)
            }
            None => self.current,
        }
    }

    /// An elliptical arc, as the cubics that stand in for it.
    ///
    /// Follows the conversion the SVG specification sets out in its
    /// implementation notes: the endpoint form is turned into a centre, two
    /// angles and a sweep, and the sweep is cut into quarter-turns.
    fn arc(&mut self, radii: Point, rotation_degrees: f32, large: bool, sweep: bool, end: Point) {
        let start = self.current;
        let (mut rx, mut ry) = (radii.x.abs(), radii.y.abs());
        if rx < f32::EPSILON || ry < f32::EPSILON || (start.x == end.x && start.y == end.y) {
            // A degenerate arc is a straight line, which is what the
            // specification says to draw.
            self.path.line_to(end);
            return;
        }

        let angle = rotation_degrees.to_radians();
        let (sin, cos) = angle.sin_cos();

        // The start point in the ellipse's own frame, with the chord halved.
        let dx = (start.x - end.x) / 2.0;
        let dy = (start.y - end.y) / 2.0;
        let x1 = cos * dx + sin * dy;
        let y1 = -sin * dx + cos * dy;

        // Radii too small to reach are scaled up until they just do.
        let check = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
        if check > 1.0 {
            let growth = check.sqrt();
            rx *= growth;
            ry *= growth;
        }

        let numerator = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
        let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
        let mut factor = if denominator > 0.0 { (numerator / denominator).sqrt() } else { 0.0 };
        if large == sweep {
            factor = -factor;
        }

        let cx1 = factor * rx * y1 / ry;
        let cy1 = -factor * ry * x1 / rx;
        let cx = cos * cx1 - sin * cy1 + (start.x + end.x) / 2.0;
        let cy = sin * cx1 + cos * cy1 + (start.y + end.y) / 2.0;

        let start_angle = ((y1 - cy1) / ry).atan2((x1 - cx1) / rx);
        let end_angle = ((-y1 - cy1) / ry).atan2((-x1 - cx1) / rx);
        let mut delta = end_angle - start_angle;
        if !sweep && delta > 0.0 {
            delta -= core::f32::consts::TAU;
        } else if sweep && delta < 0.0 {
            delta += core::f32::consts::TAU;
        }

        let steps = (delta.abs() / ARC_SEGMENT_LIMIT).ceil().max(1.0) as usize;
        let step = delta / steps as f32;
        // The handle length that makes a cubic match a circular arc of this
        // angle to within a thousandth of a radius.
        let handle = 4.0 / 3.0 * (step / 4.0).tan();

        let mut theta = start_angle;
        for _ in 0..steps {
            let next = theta + step;
            let (sin_from, cos_from) = theta.sin_cos();
            let (sin_to, cos_to) = next.sin_cos();

            let on_ellipse = |c: f32, s: f32| {
                Point::new(cx + rx * c * cos - ry * s * sin, cy + rx * c * sin + ry * s * cos)
            };
            let tangent =
                |c: f32, s: f32| (-rx * s * cos - ry * c * sin, -rx * s * sin + ry * c * cos);

            let from = on_ellipse(cos_from, sin_from);
            let to = on_ellipse(cos_to, sin_to);
            let (from_dx, from_dy) = tangent(cos_from, sin_from);
            let (to_dx, to_dy) = tangent(cos_to, sin_to);

            self.path.cubic_to(
                Point::new(from.x + handle * from_dx, from.y + handle * from_dy),
                Point::new(to.x - handle * to_dx, to.y - handle * to_dy),
                to,
            );
            theta = next;
        }
    }

    // --- Reading the text -----------------------------------------------------

    fn skip_separators(&mut self) {
        let end = self
            .rest
            .find(|c: char| !c.is_ascii_whitespace() && c != ',')
            .unwrap_or(self.rest.len());
        self.rest = &self.rest[end..];
    }

    fn take_command(&mut self) -> Option<char> {
        let letter = self.rest.chars().next()?;
        if !letter.is_ascii_alphabetic() {
            return None;
        }
        self.rest = &self.rest[letter.len_utf8()..];
        Some(letter)
    }

    fn starts_with_number(&self) -> bool {
        matches!(self.rest.chars().next(), Some(c) if c.is_ascii_digit() || c == '+' || c == '-' || c == '.')
    }

    /// One number, which may run straight into the one after it.
    fn number(&mut self, command: char) -> Result<f32, Error> {
        self.skip_separators();
        let bytes = self.rest.as_bytes();
        let mut end = 0usize;

        if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
            end += 1;
        }
        let mut seen_dot = false;
        while end < bytes.len() {
            match bytes[end] {
                b'0'..=b'9' => end += 1,
                // Only one dot belongs to a number: "1.5.5" is two of them.
                b'.' if !seen_dot => {
                    seen_dot = true;
                    end += 1;
                }
                b'e' | b'E'
                    if end + 1 < bytes.len()
                        && (bytes[end + 1].is_ascii_digit()
                            || ((bytes[end + 1] == b'+' || bytes[end + 1] == b'-')
                                && end + 2 < bytes.len()
                                && bytes[end + 2].is_ascii_digit())) =>
                {
                    end += 2;
                    while end < bytes.len() && bytes[end].is_ascii_digit() {
                        end += 1;
                    }
                    break;
                }
                _ => break,
            }
        }

        if end == 0 {
            return Err(Error::Truncated(command));
        }
        let text = &self.rest[..end];
        let value: f32 = text.parse().map_err(|_| Error::NotANumber(text.to_owned()))?;
        self.rest = &self.rest[end..];
        Ok(value)
    }

    /// A `0` or `1`, which an arc's two flags are written as and which may be
    /// run together with what follows.
    fn flag(&mut self, command: char) -> Result<bool, Error> {
        self.skip_separators();
        match self.rest.as_bytes().first() {
            Some(b'0') => {
                self.rest = &self.rest[1..];
                Ok(false)
            }
            Some(b'1') => {
                self.rest = &self.rest[1..];
                Ok(true)
            }
            _ => Err(Error::Truncated(command)),
        }
    }

    fn point(&mut self, relative: bool) -> Result<Point, Error> {
        let x = self.number('?')?;
        let y = self.number('?')?;
        Ok(if relative {
            Point::new(self.current.x + x, self.current.y + y)
        } else {
            Point::new(x, y)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_raster::Command;

    fn commands(data: &str) -> Vec<Command> {
        parse(data).expect("valid path data").commands
    }

    #[test]
    fn a_move_and_a_line() {
        assert_eq!(
            commands("M1 2L3 4"),
            vec![Command::MoveTo(Point::new(1.0, 2.0)), Command::LineTo(Point::new(3.0, 4.0))]
        );
    }

    #[test]
    fn numbers_may_run_together_without_separators() {
        // "0-1" is two numbers, and so is "1.5.5".
        assert_eq!(
            commands("M0-1L1.5.5"),
            vec![Command::MoveTo(Point::new(0.0, -1.0)), Command::LineTo(Point::new(1.5, 0.5))]
        );
    }

    #[test]
    fn a_command_repeats_for_as_long_as_numbers_keep_coming() {
        assert_eq!(
            commands("M0 0L1 1 2 2 3 3"),
            vec![
                Command::MoveTo(Point::new(0.0, 0.0)),
                Command::LineTo(Point::new(1.0, 1.0)),
                Command::LineTo(Point::new(2.0, 2.0)),
                Command::LineTo(Point::new(3.0, 3.0)),
            ]
        );
    }

    #[test]
    fn a_repeated_move_is_a_line() {
        // The rule everyone gets wrong: only the first pair after M moves.
        assert_eq!(
            commands("M0 0 5 5"),
            vec![Command::MoveTo(Point::new(0.0, 0.0)), Command::LineTo(Point::new(5.0, 5.0))]
        );
    }

    #[test]
    fn relative_commands_are_measured_from_where_the_pen_is() {
        assert_eq!(
            commands("M1 1l2 2l3 3"),
            vec![
                Command::MoveTo(Point::new(1.0, 1.0)),
                Command::LineTo(Point::new(3.0, 3.0)),
                Command::LineTo(Point::new(6.0, 6.0)),
            ]
        );
    }

    #[test]
    fn horizontal_and_vertical_lines_keep_the_other_coordinate() {
        assert_eq!(
            commands("M1 2H5V9"),
            vec![
                Command::MoveTo(Point::new(1.0, 2.0)),
                Command::LineTo(Point::new(5.0, 2.0)),
                Command::LineTo(Point::new(5.0, 9.0)),
            ]
        );
    }

    #[test]
    fn closing_returns_to_the_start_of_the_subpath_not_the_origin() {
        let path = parse("M5 5L9 9ZL1 1").expect("valid");
        assert_eq!(path.commands[2], Command::Close);
        // The line after Z is drawn from (5,5), which is where Z left the pen.
        assert_eq!(path.commands[3], Command::LineTo(Point::new(1.0, 1.0)));

        // And a relative move after Z is measured from there too.
        let path = parse("M5 5L9 9Zl1 1").expect("valid");
        assert_eq!(path.commands[3], Command::LineTo(Point::new(6.0, 6.0)));
    }

    #[test]
    fn a_smooth_cubic_reflects_the_previous_control_point() {
        let path = parse("M0 0C1 1 2 2 3 3S5 5 6 6").expect("valid");
        let Command::CubicTo(first, ..) = path.commands[2] else { panic!("a cubic") };
        // The reflection of (2,2) through the current point (3,3).
        assert_eq!(first, Point::new(4.0, 4.0));
    }

    #[test]
    fn a_smooth_cubic_with_nothing_to_reflect_uses_the_current_point() {
        let path = parse("M2 2S5 5 6 6").expect("valid");
        let Command::CubicTo(first, ..) = path.commands[1] else { panic!("a cubic") };
        assert_eq!(first, Point::new(2.0, 2.0));
    }

    #[test]
    fn an_arc_becomes_curves_that_end_where_it_should() {
        let path = parse("M0 0A5 5 0 0 1 10 0").expect("valid");
        let Some(Command::CubicTo(_, _, end)) = path.commands.last() else { panic!("a cubic") };
        assert!((end.x - 10.0).abs() < 0.01 && end.y.abs() < 0.01, "ends at (10, 0), got {end:?}");
    }

    #[test]
    fn a_degenerate_arc_is_a_straight_line() {
        assert_eq!(
            commands("M0 0A0 0 0 0 1 4 4"),
            vec![Command::MoveTo(Point::new(0.0, 0.0)), Command::LineTo(Point::new(4.0, 4.0))]
        );
    }

    #[test]
    fn data_that_does_not_start_with_a_move_is_refused() {
        assert_eq!(parse("L1 1"), Err(Error::NoInitialMove));
    }

    #[test]
    fn a_command_missing_its_numbers_is_refused() {
        assert_eq!(parse("M0 0L1"), Err(Error::Truncated('?')));
    }

    #[test]
    fn empty_data_is_an_empty_path() {
        assert!(parse("").expect("valid").is_empty());
        assert!(parse("   ").expect("valid").is_empty());
    }
}

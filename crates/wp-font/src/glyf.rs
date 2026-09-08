//! Glyph outlines from the `glyf` table.
//!
//! A TrueType outline is a set of closed contours made of quadratic curves. The
//! points are stored in a compressed form — deltas, with flags that say how many
//! bytes each one takes and whether it repeats — and a point may sit *on* the
//! curve or be a control point off it.
//!
//! Two consecutive off-curve points imply an on-curve point exactly between
//! them. The format relies on this to save space, and a reader that ignores it
//! draws the wrong shape, so the implied points are reconstructed here.

use crate::read::Reader;
use crate::{Bounds, Error, Font, GlyphId};

/// A point in font design units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    #[must_use]
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn midpoint(self, other: Self) -> Self {
        Self { x: (self.x + other.x) / 2.0, y: (self.y + other.y) / 2.0 }
    }
}

/// One step in drawing a glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    /// A quadratic curve: a control point, then the point it ends at.
    QuadTo(Point, Point),
    /// Closes the current contour back to where it started.
    Close,
}

/// A glyph's shape.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outline {
    pub commands: Vec<PathCommand>,
    pub bounds: Bounds,
}

impl Outline {
    /// Whether there is anything to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// How deep composite glyphs may nest.
///
/// A composite refers to other glyphs, and a damaged font can make one refer to
/// itself. Following that would not terminate.
const MAX_COMPOSITE_DEPTH: usize = 8;

// Flags in a simple glyph's point data.
const ON_CURVE: u8 = 0x01;
const X_SHORT: u8 = 0x02;
const Y_SHORT: u8 = 0x04;
const REPEAT: u8 = 0x08;
const X_SAME_OR_POSITIVE: u8 = 0x10;
const Y_SAME_OR_POSITIVE: u8 = 0x20;

// Flags in a composite glyph's component records.
const ARGS_ARE_WORDS: u16 = 0x0001;
const ARGS_ARE_XY: u16 = 0x0002;
const HAS_SCALE: u16 = 0x0008;
const MORE_COMPONENTS: u16 = 0x0020;
const HAS_X_AND_Y_SCALE: u16 = 0x0040;
const HAS_TWO_BY_TWO: u16 = 0x0080;

/// Reads a glyph's outline.
pub(crate) fn outline(font: &Font<'_>, glyph: GlyphId) -> Result<Option<Outline>, Error> {
    outline_at_depth(font, glyph, 0)
}

fn outline_at_depth(
    font: &Font<'_>,
    glyph: GlyphId,
    depth: usize,
) -> Result<Option<Outline>, Error> {
    if depth > MAX_COMPOSITE_DEPTH {
        return Err(Error::RecursiveGlyph);
    }

    let Some((start, end)) = font.glyph_range(glyph)? else {
        // A glyph with no outline, such as a space.
        return Ok(None);
    };

    let data = font.data();
    let mut reader = Reader::at(data, start)?;
    let contour_count = reader.i16()?;
    let bounds = Bounds {
        min_x: reader.i16()?,
        min_y: reader.i16()?,
        max_x: reader.i16()?,
        max_y: reader.i16()?,
    };

    let mut result = if contour_count >= 0 {
        Outline { commands: read_simple(&mut reader, contour_count as usize, end)?, bounds }
    } else {
        Outline { commands: read_composite(font, &mut reader, depth)?, bounds }
    };

    if result.commands.is_empty() {
        return Ok(None);
    }
    result.bounds = bounds;
    Ok(Some(result))
}

/// A point as the file stores it, before curves are worked out.
#[derive(Clone, Copy)]
struct ContourPoint {
    position: Point,
    on_curve: bool,
}

fn read_simple(
    reader: &mut Reader<'_>,
    contour_count: usize,
    end_of_glyph: usize,
) -> Result<Vec<PathCommand>, Error> {
    if contour_count == 0 {
        return Ok(Vec::new());
    }

    let mut contour_ends = Vec::with_capacity(contour_count);
    for _ in 0..contour_count {
        contour_ends.push(usize::from(reader.u16()?));
    }
    // The ends must ascend, and the last one gives the point count.
    let point_count = contour_ends.last().map_or(0, |last| last + 1);
    if point_count == 0 || point_count > 10_000 {
        return Err(Error::MalformedTable("glyf"));
    }

    let instruction_length = usize::from(reader.u16()?);
    reader.skip(instruction_length)?;

    // Flags, which repeat to save space.
    let mut flags = Vec::with_capacity(point_count);
    while flags.len() < point_count {
        if reader.position() >= end_of_glyph {
            return Err(Error::MalformedTable("glyf"));
        }
        let flag = reader.u8()?;
        flags.push(flag);
        if flag & REPEAT != 0 {
            let extra = reader.u8()?;
            for _ in 0..extra {
                if flags.len() >= point_count {
                    break;
                }
                flags.push(flag);
            }
        }
    }

    // Coordinates are deltas from the previous point, in one of three forms
    // depending on the flags: one byte, two bytes, or "same as before".
    let mut x = 0i32;
    let mut xs = Vec::with_capacity(point_count);
    for &flag in &flags {
        if flag & X_SHORT != 0 {
            let delta = i32::from(reader.u8()?);
            x += if flag & X_SAME_OR_POSITIVE != 0 { delta } else { -delta };
        } else if flag & X_SAME_OR_POSITIVE == 0 {
            x += i32::from(reader.i16()?);
        }
        xs.push(x);
    }

    let mut y = 0i32;
    let mut ys = Vec::with_capacity(point_count);
    for &flag in &flags {
        if flag & Y_SHORT != 0 {
            let delta = i32::from(reader.u8()?);
            y += if flag & Y_SAME_OR_POSITIVE != 0 { delta } else { -delta };
        } else if flag & Y_SAME_OR_POSITIVE == 0 {
            y += i32::from(reader.i16()?);
        }
        ys.push(y);
    }

    let points: Vec<ContourPoint> = flags
        .iter()
        .zip(xs)
        .zip(ys)
        .map(|((flag, x), y)| ContourPoint {
            position: Point::new(x as f32, y as f32),
            on_curve: flag & ON_CURVE != 0,
        })
        .collect();

    let mut commands = Vec::new();
    let mut first = 0usize;
    for &last in &contour_ends {
        if last >= points.len() || last < first {
            return Err(Error::MalformedTable("glyf"));
        }
        emit_contour(&points[first..=last], &mut commands);
        first = last + 1;
    }

    Ok(commands)
}

/// Turns one contour's points into drawing commands.
fn emit_contour(points: &[ContourPoint], out: &mut Vec<PathCommand>) {
    let count = points.len();
    if count == 0 {
        return;
    }

    // Drawing has to begin at a point that is actually on the curve. If the
    // contour has none — which happens, and is legal — the midpoint between the
    // last and first control points is on the curve by construction.
    let (start, first_step) = match points.iter().position(|point| point.on_curve) {
        Some(index) => (points[index].position, index + 1),
        None => (points[count - 1].position.midpoint(points[0].position), 0),
    };

    out.push(PathCommand::MoveTo(start));

    let mut pending_control: Option<Point> = None;
    for step in 0..count {
        let point = points[(first_step + step) % count];

        if point.on_curve {
            match pending_control.take() {
                Some(control) => out.push(PathCommand::QuadTo(control, point.position)),
                None => out.push(PathCommand::LineTo(point.position)),
            }
        } else if let Some(control) = pending_control.replace(point.position) {
            // Two control points in a row: the on-curve point between them is
            // implied rather than stored.
            let implied = control.midpoint(point.position);
            out.push(PathCommand::QuadTo(control, implied));
        }
    }

    if let Some(control) = pending_control {
        out.push(PathCommand::QuadTo(control, start));
    }
    out.push(PathCommand::Close);
}

/// A composite glyph is built out of other glyphs, each placed by a transform.
/// Accented letters are made this way: one "e", one acute accent, moved.
fn read_composite(
    font: &Font<'_>,
    reader: &mut Reader<'_>,
    depth: usize,
) -> Result<Vec<PathCommand>, Error> {
    let mut commands = Vec::new();

    loop {
        let flags = reader.u16()?;
        let component = GlyphId(reader.u16()?);

        let (dx, dy) = if flags & ARGS_ARE_WORDS != 0 {
            (f32::from(reader.i16()?), f32::from(reader.i16()?))
        } else {
            (f32::from(reader.i8()?), f32::from(reader.i8()?))
        };
        // Arguments can also be point numbers to align on, which is rare and not
        // handled; treating them as an offset of zero is closer than misplacing
        // the component wildly.
        let (dx, dy) = if flags & ARGS_ARE_XY != 0 { (dx, dy) } else { (0.0, 0.0) };

        let (a, b, c, d) = if flags & HAS_SCALE != 0 {
            let scale = reader.f2dot14()?;
            (scale, 0.0, 0.0, scale)
        } else if flags & HAS_X_AND_Y_SCALE != 0 {
            (reader.f2dot14()?, 0.0, 0.0, reader.f2dot14()?)
        } else if flags & HAS_TWO_BY_TWO != 0 {
            (reader.f2dot14()?, reader.f2dot14()?, reader.f2dot14()?, reader.f2dot14()?)
        } else {
            (1.0, 0.0, 0.0, 1.0)
        };

        if let Some(child) = outline_at_depth(font, component, depth + 1)? {
            let transform = |point: Point| Point {
                x: a * point.x + c * point.y + dx,
                y: b * point.x + d * point.y + dy,
            };
            for command in child.commands {
                commands.push(match command {
                    PathCommand::MoveTo(point) => PathCommand::MoveTo(transform(point)),
                    PathCommand::LineTo(point) => PathCommand::LineTo(transform(point)),
                    PathCommand::QuadTo(control, point) => {
                        PathCommand::QuadTo(transform(control), transform(point))
                    }
                    PathCommand::Close => PathCommand::Close,
                });
            }
        }

        if flags & MORE_COMPONENTS == 0 {
            break;
        }
    }

    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(x: f32, y: f32) -> ContourPoint {
        ContourPoint { position: Point::new(x, y), on_curve: true }
    }

    fn off(x: f32, y: f32) -> ContourPoint {
        ContourPoint { position: Point::new(x, y), on_curve: false }
    }

    #[test]
    fn a_square_becomes_lines() {
        let mut commands = Vec::new();
        emit_contour(&[on(0.0, 0.0), on(10.0, 0.0), on(10.0, 10.0), on(0.0, 10.0)], &mut commands);

        assert_eq!(
            commands,
            vec![
                PathCommand::MoveTo(Point::new(0.0, 0.0)),
                PathCommand::LineTo(Point::new(10.0, 0.0)),
                PathCommand::LineTo(Point::new(10.0, 10.0)),
                PathCommand::LineTo(Point::new(0.0, 10.0)),
                PathCommand::LineTo(Point::new(0.0, 0.0)),
                PathCommand::Close,
            ]
        );
    }

    #[test]
    fn a_control_point_becomes_a_curve() {
        let mut commands = Vec::new();
        emit_contour(&[on(0.0, 0.0), off(5.0, 10.0), on(10.0, 0.0)], &mut commands);

        assert_eq!(commands[0], PathCommand::MoveTo(Point::new(0.0, 0.0)));
        assert_eq!(commands[1], PathCommand::QuadTo(Point::new(5.0, 10.0), Point::new(10.0, 0.0)));
    }

    #[test]
    fn two_control_points_in_a_row_imply_a_point_between_them() {
        // The format leaves the on-curve point out to save space. A reader that
        // ignores that draws a visibly wrong shape.
        let mut commands = Vec::new();
        emit_contour(&[on(0.0, 0.0), off(4.0, 8.0), off(8.0, 8.0), on(12.0, 0.0)], &mut commands);

        assert_eq!(
            commands[1],
            PathCommand::QuadTo(Point::new(4.0, 8.0), Point::new(6.0, 8.0)),
            "the implied midpoint should be at (6, 8)"
        );
        assert_eq!(commands[2], PathCommand::QuadTo(Point::new(8.0, 8.0), Point::new(12.0, 0.0)));
    }

    #[test]
    fn a_contour_with_no_on_curve_point_still_draws() {
        // Legal, and used for shapes made entirely of curves, such as an O.
        let mut commands = Vec::new();
        emit_contour(
            &[off(0.0, 10.0), off(10.0, 10.0), off(10.0, 0.0), off(0.0, 0.0)],
            &mut commands,
        );

        assert!(matches!(commands.first(), Some(PathCommand::MoveTo(_))));
        assert!(matches!(commands.last(), Some(PathCommand::Close)));
        // Starts halfway between the last and first control points.
        assert_eq!(commands[0], PathCommand::MoveTo(Point::new(0.0, 5.0)));
    }

    #[test]
    fn an_empty_contour_produces_nothing() {
        let mut commands = Vec::new();
        emit_contour(&[], &mut commands);
        assert!(commands.is_empty());
    }
}

//! Ink laid out: the strokes as bands along the line the pen took.
//!
//! # Why the strokes are fitted into the box rather than drawn where they say
//!
//! Because the two numbers mean different things. A stroke says where the pen
//! was on whatever the pen was drawn on; the run says how big the ink is in
//! the document. Word writes the second as the extent of the first, so fitting
//! one into the other changes nothing — and when the file disagrees with
//! itself, fitting is what keeps the whole of somebody's handwriting inside
//! the room the text made for it.
//!
//! The fit is the same both ways and the ink is centred in what is left over.
//! Handwriting stretched to fill a box is somebody else's handwriting.

use wp_docx::ink::Ink;
use wp_raster::{Color, Path, Point};

/// Ink laid out: paths ready to be put on a page, and the colour of each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InkDrawing {
    pub paths: Vec<(Path, Color)>,
}

impl InkDrawing {
    /// The same drawing, moved to where it goes on the page.
    #[must_use]
    pub fn translated(&self, x: f32, y: f32) -> Self {
        Self {
            paths: self.paths.iter().map(|(path, colour)| (moved(path, x, y), *colour)).collect(),
        }
    }

    /// The same drawing, put through a transform: turned with the page it
    /// is on, for a section written down the page.
    #[must_use]
    pub fn transformed(&self, transform: &wp_raster::Transform) -> Self {
        Self {
            paths: self
                .paths
                .iter()
                .map(|(path, colour)| (path.transformed(transform), *colour))
                .collect(),
        }
    }
}

/// How ink is fitted into the box it is drawn in: the same both ways, and
/// centred in what is left. The one place that says how a point of the ink
/// becomes a point of the box, so that what draws the ink and what asks
/// which stroke the pointer is on agree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// The corner of what the ink covers, in its own space.
    pub left: i64,
    pub top: i64,
    /// Pixels to one English metric unit.
    pub scale: f32,
    /// The room left over on each side once the ink is fitted, halved.
    pub spare_x: f32,
    pub spare_y: f32,
}

impl Fit {
    /// The fit of some ink into a box that many pixels across and down.
    #[must_use]
    pub fn of(ink: &Ink, width: f32, height: f32) -> Option<Self> {
        let (left, top, right, bottom) = ink.bounds()?;
        let across = (right - left).max(1) as f32;
        let down = (bottom - top).max(1) as f32;
        let scale = (width / across).min(height / down);
        Some(Self {
            left,
            top,
            scale,
            spare_x: (width - across * scale) / 2.0,
            spare_y: (height - down * scale) / 2.0,
        })
    }

    /// Where a point of the ink falls in the box.
    #[must_use]
    pub fn to_box(&self, x: i64, y: i64) -> Point {
        Point {
            x: self.spare_x + (x - self.left) as f32 * self.scale,
            y: self.spare_y + (y - self.top) as f32 * self.scale,
        }
    }

    /// And which point of the ink a point of the box is.
    #[must_use]
    pub fn from_box(&self, x: f32, y: f32) -> (i64, i64) {
        let scale = self.scale.max(f32::EPSILON);
        (
            self.left + ((x - self.spare_x) / scale) as i64,
            self.top + ((y - self.spare_y) / scale) as i64,
        )
    }
}

/// Lays ink out in a box that many pixels across and down.
#[must_use]
pub fn draw(ink: &Ink, width: f32, height: f32) -> InkDrawing {
    let mut out = InkDrawing::default();
    let Some(fit) = Fit::of(ink, width, height) else { return out };
    let scale = fit.scale;

    for stroke in &ink.strokes {
        if stroke.points.len() < 2 {
            continue;
        }
        let Some(colour) = Color::from_hex(&stroke.colour) else { continue };
        // Transparency counts from nothing at all to invisible, and an alpha
        // counts the other way: a highlighter is written as half transparent
        // and drawn as half opaque.
        let colour = Color { alpha: 255u8.saturating_sub(stroke.transparency), ..colour };

        let mut path = Path::new();
        for (index, (x, y)) in stroke.points.iter().enumerate() {
            let point = fit.to_box(*x, *y);
            if index == 0 {
                path.move_to(point);
            } else {
                path.line_to(point);
            }
        }

        // A pen thinner than a pixel is still a pen: what it draws has to be
        // visible, or the ink is there and nobody can see it.
        let weight = (stroke.width_emu as f32 * scale).max(1.0);
        let band = if stroke.pressure.len() == stroke.points.len() {
            // A pen that said how hard it was pressed draws a stroke that
            // swells and thins with it: half the pen's width at no pressure,
            // the width itself halfway, half as much again pressed hard. An
            // estimate of Word's curve, which is not published.
            let widths: Vec<f32> =
                stroke.pressure.iter().map(|pressed| (weight * (0.5 + pressed)).max(1.0)).collect();
            ribbon_along(&path, &widths)
        } else {
            crate::geometry::band_along(&path, weight)
        };
        out.paths.push((band, colour));
    }
    out
}

/// A band along a path of straight pieces whose width changes from point to
/// point: each piece is drawn as wide as its ends say, and each turn is
/// patched over as wide as the turn is. Laid down the way
/// [`crate::geometry::band_along`] lays a band, and for the same reason.
fn ribbon_along(path: &Path, widths: &[f32]) -> Path {
    use wp_raster::Command;

    let points: Vec<Point> = path
        .commands
        .iter()
        .filter_map(|command| match command {
            Command::MoveTo(point) | Command::LineTo(point) => Some(*point),
            _ => None,
        })
        .collect();
    let width_at = |index: usize| widths.get(index).copied().unwrap_or(1.0);

    let mut band = Path::new();
    for (index, pair) in points.windows(2).enumerate() {
        let (from, to) = (pair[0], pair[1]);
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let length = dx.hypot(dy);
        if length <= 0.0 {
            continue;
        }
        let (nx, ny) = (-dy / length, dx / length);
        let (start, end) = (width_at(index) / 2.0, width_at(index + 1) / 2.0);
        band.move_to(Point::new(from.x + nx * start, from.y + ny * start));
        band.line_to(Point::new(to.x + nx * end, to.y + ny * end));
        band.line_to(Point::new(to.x - nx * end, to.y - ny * end));
        band.line_to(Point::new(from.x - nx * start, from.y - ny * start));
        band.close();
    }
    for (index, turn) in points.iter().enumerate().take(points.len().saturating_sub(1)).skip(1) {
        // Wound the same way as the pieces, or by the nonzero rule the patch
        // would cancel the piece under it and the stroke come out dashed.
        let half = width_at(index) / 2.0;
        band.move_to(Point::new(turn.x - half, turn.y + half));
        band.line_to(Point::new(turn.x + half, turn.y + half));
        band.line_to(Point::new(turn.x + half, turn.y - half));
        band.line_to(Point::new(turn.x - half, turn.y - half));
        band.close();
    }
    band
}

/// A path with every point moved by the same amount.
fn moved(path: &Path, x: f32, y: f32) -> Path {
    use wp_raster::Command;

    let shift = |point: &Point| Point { x: point.x + x, y: point.y + y };
    let mut out = Path::new();
    out.commands = path
        .commands
        .iter()
        .map(|command| match command {
            Command::MoveTo(point) => Command::MoveTo(shift(point)),
            Command::LineTo(point) => Command::LineTo(shift(point)),
            Command::QuadTo(control, point) => Command::QuadTo(shift(control), shift(point)),
            Command::CubicTo(first, second, point) => {
                Command::CubicTo(shift(first), shift(second), shift(point))
            }
            Command::Close => Command::Close,
        })
        .collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::ink::Stroke;

    fn ink() -> Ink {
        Ink {
            strokes: vec![Stroke {
                colour: "FF0000".to_owned(),
                width_emu: 9_525,
                transparency: 0,
                flat: false,
                points: vec![(0, 0), (36_000, 0), (36_000, 36_000)],
                pressure: Vec::new(),
            }],
        }
    }

    /// The rectangle a drawing covers.
    fn bounds(drawing: &InkDrawing) -> (f32, f32, f32, f32) {
        use wp_raster::Command;

        let mut bounds = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (path, _) in &drawing.paths {
            for command in &path.commands {
                let points = match command {
                    Command::MoveTo(point) | Command::LineTo(point) => vec![*point],
                    Command::QuadTo(one, two) => vec![*one, *two],
                    Command::CubicTo(one, two, three) => vec![*one, *two, *three],
                    Command::Close => Vec::new(),
                };
                for point in points {
                    bounds.0 = bounds.0.min(point.x);
                    bounds.1 = bounds.1.min(point.y);
                    bounds.2 = bounds.2.max(point.x);
                    bounds.3 = bounds.3.max(point.y);
                }
            }
        }
        bounds
    }

    #[test]
    fn ink_is_drawn_inside_the_room_it_was_given() {
        let drawing = draw(&ink(), 100.0, 100.0);
        let (left, top, right, bottom) = bounds(&drawing);
        assert!(left >= -0.5 && top >= -0.5, "the ink starts outside the box at {left},{top}");
        assert!(right <= 100.5 && bottom <= 100.5, "the ink runs past the box at {right},{bottom}");
    }

    #[test]
    fn handwriting_is_not_stretched_to_fill_the_box() {
        // The same ink in a box twice as wide is the same shape, moved along:
        // a letter stretched sideways is somebody else's handwriting.
        let square = draw(&ink(), 100.0, 100.0);
        let wide = draw(&ink(), 200.0, 100.0);
        let (left, top, right, bottom) = bounds(&square);
        let (wide_left, wide_top, wide_right, wide_bottom) = bounds(&wide);
        assert!(((right - left) - (wide_right - wide_left)).abs() < 0.5);
        assert!(((bottom - top) - (wide_bottom - wide_top)).abs() < 0.5);
        assert!(wide_left > left, "the ink was not centred in the wider box");
    }

    #[test]
    fn a_stroke_is_drawn_in_its_own_colour() {
        let drawing = draw(&ink(), 100.0, 100.0);
        assert_eq!(drawing.paths.len(), 1);
        assert_eq!(drawing.paths[0].1, Color::rgb(255, 0, 0));
    }

    #[test]
    fn a_highlighter_is_drawn_so_the_words_show_through_it() {
        let mut ink = ink();
        ink.strokes[0].transparency = 128;
        let drawing = draw(&ink, 100.0, 100.0);
        assert_eq!(drawing.paths[0].1.alpha, 127);
    }

    #[test]
    fn ink_with_nothing_in_it_draws_nothing() {
        assert!(draw(&Ink::default(), 100.0, 100.0).paths.is_empty());
    }

    #[test]
    fn moving_a_drawing_moves_every_point_of_it() {
        let drawing = draw(&ink(), 100.0, 100.0);
        let (left, top, _, _) = bounds(&drawing);
        let moved = drawing.translated(10.0, 20.0);
        let (moved_left, moved_top, _, _) = bounds(&moved);
        assert!((moved_left - left - 10.0).abs() < 0.01);
        assert!((moved_top - top - 20.0).abs() < 0.01);
    }
}

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
#[derive(Clone, Debug, Default)]
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
}

/// Lays ink out in a box that many pixels across and down.
#[must_use]
pub fn draw(ink: &Ink, width: f32, height: f32) -> InkDrawing {
    let mut out = InkDrawing::default();
    let Some((left, top, right, bottom)) = ink.bounds() else { return out };

    let across = (right - left).max(1) as f32;
    let down = (bottom - top).max(1) as f32;
    let scale = (width / across).min(height / down);
    // What is left over after the fit, halved: the ink sits in the middle of
    // the room it was given.
    let spare_x = (width - across * scale) / 2.0;
    let spare_y = (height - down * scale) / 2.0;

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
            let point = Point {
                x: spare_x + (x - left) as f32 * scale,
                y: spare_y + (y - top) as f32 * scale,
            };
            if index == 0 {
                path.move_to(point);
            } else {
                path.line_to(point);
            }
        }

        // A pen thinner than a pixel is still a pen: what it draws has to be
        // visible, or the ink is there and nobody can see it.
        let weight = (stroke.width_emu as f32 * scale).max(1.0);
        out.paths.push((crate::geometry::band_along(&path, weight), colour));
    }
    out
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

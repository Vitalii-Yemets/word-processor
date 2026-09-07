//! Turning paths into anti-aliased coverage.
//!
//! # How it works
//!
//! The obvious way to draw a shape is to ask, for each pixel, whether it is
//! inside. That answers yes or no, which gives jagged edges, and doing it
//! properly means sampling each pixel many times.
//!
//! This uses a better method. Every line of the outline deposits a *signed area*
//! into the pixels it crosses: how much of each pixel it covers, positive going
//! down and negative going up. Running a total across each row afterwards then
//! gives the exact coverage of every pixel in one pass — a real fraction, not a
//! yes or no — and the sign cancellation is what makes the inside of an "o"
//! come out empty.
//!
//! Curves are flattened into lines first, finely enough that the difference is
//! below what a pixel can show.

use crate::path::{Command, Path, Point};

/// A grayscale coverage image: how much of each pixel a shape covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    width: usize,
    height: usize,
    /// One byte per pixel, 0 for uncovered and 255 for fully covered.
    coverage: Vec<u8>,
}

impl Mask {
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Coverage at a pixel, or zero outside the mask.
    #[must_use]
    pub fn at(&self, x: usize, y: usize) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.coverage[y * self.width + x]
    }

    #[must_use]
    pub fn coverage(&self) -> &[u8] {
        &self.coverage
    }

    /// Whether nothing at all was drawn.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.coverage.iter().all(|&value| value == 0)
    }
}

/// Accumulates paths, then produces the coverage they add up to.
#[derive(Clone, Debug)]
pub struct Rasterizer {
    width: usize,
    height: usize,
    /// One cell per pixel plus one spare column per row: a span ending at the
    /// right edge still has somewhere to deposit its remainder, which is then
    /// discarded.
    stride: usize,
    area: Vec<f32>,
}

/// The largest number of line segments one curve is broken into.
///
/// A bound is needed because the estimate is driven by coordinates that come
/// from a font file, and a malformed one can ask for a great many.
const MAX_CURVE_STEPS: usize = 128;

impl Rasterizer {
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        let stride = width + 1;
        Self { width, height, stride, area: vec![0.0; stride * height] }
    }

    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Forgets everything drawn so far, keeping the buffer.
    pub fn clear(&mut self) {
        self.area.fill(0.0);
    }

    /// Adds a path to be filled.
    ///
    /// Filling uses the nonzero winding rule, which is what font outlines
    /// assume: a counter-drawn inner contour cancels the outer one and leaves a
    /// hole.
    pub fn fill(&mut self, path: &Path) {
        let mut start = Point::new(0.0, 0.0);
        let mut current = start;

        for command in &path.commands {
            match *command {
                Command::MoveTo(point) => {
                    // An unclosed contour is closed implicitly; leaving it open
                    // would let coverage leak across the whole row.
                    if current != start {
                        self.add_line(current, start);
                    }
                    start = point;
                    current = point;
                }
                Command::LineTo(point) => {
                    self.add_line(current, point);
                    current = point;
                }
                Command::QuadTo(control, point) => {
                    self.add_quad(current, control, point);
                    current = point;
                }
                Command::CubicTo(first, second, point) => {
                    self.add_cubic(current, first, second, point);
                    current = point;
                }
                Command::Close => {
                    self.add_line(current, start);
                    current = start;
                }
            }
        }

        if current != start {
            self.add_line(current, start);
        }
    }

    /// Produces the coverage of everything added so far.
    #[must_use]
    pub fn finish(&self) -> Mask {
        let mut coverage = vec![0u8; self.width * self.height];

        for row in 0..self.height {
            // The running total across a row is what turns deposited areas into
            // actual coverage.
            let mut total = 0.0f32;
            let source = row * self.stride;
            let destination = row * self.width;
            for column in 0..self.width {
                total += self.area[source + column];
                let value = total.abs().min(1.0);
                coverage[destination + column] = (value * 255.0 + 0.5) as u8;
            }
        }

        Mask { width: self.width, height: self.height, coverage }
    }

    /// Adds a value to one cell, ignoring anything outside the buffer.
    fn deposit(&mut self, index: usize, value: f32) {
        if let Some(cell) = self.area.get_mut(index) {
            *cell += value;
        }
    }

    fn add_quad(&mut self, from: Point, control: Point, to: Point) {
        // How far the curve strays from the straight line between its ends
        // decides how finely it needs breaking up.
        let deviation_x = from.x - 2.0 * control.x + to.x;
        let deviation_y = from.y - 2.0 * control.y + to.y;
        let deviation = deviation_x * deviation_x + deviation_y * deviation_y;

        if deviation < 0.1 {
            self.add_line(from, to);
            return;
        }

        let steps = (1.0 + (3.0 * deviation).sqrt().sqrt()) as usize;
        let steps = steps.clamp(1, MAX_CURVE_STEPS);

        let mut previous = from;
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let inverse = 1.0 - t;
            let point = Point::new(
                inverse * inverse * from.x + 2.0 * inverse * t * control.x + t * t * to.x,
                inverse * inverse * from.y + 2.0 * inverse * t * control.y + t * t * to.y,
            );
            self.add_line(previous, point);
            previous = point;
        }
    }

    fn add_cubic(&mut self, from: Point, first: Point, second: Point, to: Point) {
        let deviation_x = (from.x - 3.0 * first.x + 3.0 * second.x - to.x).abs();
        let deviation_y = (from.y - 3.0 * first.y + 3.0 * second.y - to.y).abs();
        let deviation = deviation_x * deviation_x + deviation_y * deviation_y;

        if deviation < 0.1 {
            self.add_line(from, to);
            return;
        }

        let steps = (1.0 + (3.0 * deviation).sqrt().sqrt()) as usize;
        let steps = steps.clamp(1, MAX_CURVE_STEPS);

        let mut previous = from;
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let inverse = 1.0 - t;
            let point = Point::new(
                inverse * inverse * inverse * from.x
                    + 3.0 * inverse * inverse * t * first.x
                    + 3.0 * inverse * t * t * second.x
                    + t * t * t * to.x,
                inverse * inverse * inverse * from.y
                    + 3.0 * inverse * inverse * t * first.y
                    + 3.0 * inverse * t * t * second.y
                    + t * t * t * to.y,
            );
            self.add_line(previous, point);
            previous = point;
        }
    }

    /// Deposits the signed area of one line segment.
    fn add_line(&mut self, from: Point, to: Point) {
        // A horizontal line covers no area, and dividing by its height would not
        // end well.
        if (from.y - to.y).abs() < 1e-6 {
            return;
        }
        if !from.x.is_finite() || !from.y.is_finite() || !to.x.is_finite() || !to.y.is_finite() {
            return;
        }

        // Going up cancels going down, which is what makes the counter of an "o"
        // come out empty rather than filled twice.
        let (direction, top, bottom) =
            if from.y < to.y { (1.0f32, from, to) } else { (-1.0f32, to, from) };

        let slope = (bottom.x - top.x) / (bottom.y - top.y);
        let first_y = top.y.max(0.0);
        let last_y = bottom.y.min(self.height as f32);
        if last_y <= first_y {
            return;
        }

        let mut x = top.x + (first_y - top.y) * slope;
        let first_row = first_y as usize;
        let last_row = (last_y.ceil() as usize).min(self.height);

        for row in first_row..last_row {
            let row_top = (row as f32).max(first_y);
            let row_bottom = ((row + 1) as f32).min(last_y);
            let height = row_bottom - row_top;
            if height <= 0.0 {
                continue;
            }

            let next_x = x + slope * height;
            self.deposit_span(row, x, next_x, height * direction);
            x = next_x;
        }
    }

    /// Spreads one row's worth of a line across the columns it crosses.
    fn deposit_span(&mut self, row: usize, from_x: f32, to_x: f32, signed_height: f32) {
        let limit = self.width as f32;
        let (left, right) = if from_x < to_x { (from_x, to_x) } else { (to_x, from_x) };
        // Anything left of the canvas still counts as crossed, so it is clamped
        // into the first column rather than dropped.
        let left = left.clamp(0.0, limit);
        let right = right.clamp(0.0, limit);

        let left_floor = left.floor();
        let first_column = left_floor as usize;
        let right_ceil = right.ceil();
        let last_column = right_ceil as usize;
        let base = row * self.stride;

        if last_column <= first_column + 1 {
            // The whole span sits within one column.
            let centre = 0.5 * (left + right) - left_floor;
            self.deposit(base + first_column, signed_height * (1.0 - centre));
            self.deposit(base + first_column + 1, signed_height * centre);
            return;
        }

        let inverse_width = (right - left).recip();
        if !inverse_width.is_finite() {
            return;
        }

        // The two end columns are entered and left part-way, so they take a
        // triangular share; the columns between them are crossed completely.
        let first_fraction = 1.0 - (left - left_floor);
        let first_area = 0.5 * inverse_width * first_fraction * first_fraction;
        let last_fraction = right - (right_ceil - 1.0);
        let last_area = 0.5 * inverse_width * last_fraction * last_fraction;

        self.deposit(base + first_column, signed_height * first_area);

        if last_column == first_column + 2 {
            self.deposit(
                base + first_column + 1,
                signed_height * (1.0 - first_area - last_area),
            );
        } else {
            let second_area = inverse_width * (1.5 - (left - left_floor));
            self.deposit(base + first_column + 1, signed_height * (second_area - first_area));

            for column in (first_column + 2)..(last_column - 1) {
                self.deposit(base + column, signed_height * inverse_width);
            }

            let covered =
                second_area + (last_column - first_column - 3) as f32 * inverse_width;
            self.deposit(base + last_column - 1, signed_height * (1.0 - covered - last_area));
        }

        self.deposit(base + last_column, signed_height * last_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(width: usize, height: usize, path: &Path) -> Mask {
        let mut rasterizer = Rasterizer::new(width, height);
        rasterizer.fill(path);
        rasterizer.finish()
    }

    #[test]
    fn an_empty_path_draws_nothing() {
        let mask = fill(8, 8, &Path::new());
        assert!(mask.is_blank());
    }

    #[test]
    fn a_whole_pixel_rectangle_is_fully_covered() {
        let mask = fill(10, 10, &Path::rectangle(2.0, 2.0, 4.0, 4.0));

        assert_eq!(mask.at(3, 3), 255, "inside the rectangle");
        assert_eq!(mask.at(5, 5), 255, "still inside at the far corner");
        assert_eq!(mask.at(1, 3), 0, "outside to the left");
        assert_eq!(mask.at(6, 3), 0, "outside to the right");
        assert_eq!(mask.at(3, 1), 0, "above");
        assert_eq!(mask.at(3, 6), 0, "below");
    }

    #[test]
    fn a_half_pixel_edge_is_half_covered() {
        // This is the whole point of the method: an edge that falls between
        // pixels produces a fraction, not a jagged yes or no.
        let mask = fill(10, 10, &Path::rectangle(2.5, 2.0, 4.0, 4.0));

        let edge = mask.at(2, 3);
        assert!(
            (100..=155).contains(&edge),
            "the half-covered pixel should be around 128, got {edge}"
        );
        assert_eq!(mask.at(3, 3), 255, "the pixel beyond it is full");
    }

    #[test]
    fn winding_cancels_so_a_ring_has_a_hole() {
        // The inner contour is wound the other way. Without the sign, the middle
        // of every "o" would be filled in.
        let mut path = Path::rectangle(1.0, 1.0, 10.0, 10.0);
        path.move_to(Point::new(4.0, 4.0))
            .line_to(Point::new(4.0, 8.0))
            .line_to(Point::new(8.0, 8.0))
            .line_to(Point::new(8.0, 4.0))
            .close();

        let mask = fill(14, 14, &path);
        assert_eq!(mask.at(2, 6), 255, "the ring itself is filled");
        assert_eq!(mask.at(6, 6), 0, "the hole is empty");
    }

    #[test]
    fn an_unclosed_contour_is_closed_anyway() {
        // A path that forgets to close would otherwise leak coverage across the
        // rest of the row.
        let mut path = Path::new();
        path.move_to(Point::new(2.0, 2.0))
            .line_to(Point::new(6.0, 2.0))
            .line_to(Point::new(6.0, 6.0))
            .line_to(Point::new(2.0, 6.0));

        let mask = fill(10, 10, &path);
        assert_eq!(mask.at(3, 3), 255);
        assert_eq!(mask.at(8, 3), 0, "coverage should not run off to the right");
    }

    #[test]
    fn a_triangle_shades_from_edge_to_edge() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(20.0, 0.0))
            .line_to(Point::new(20.0, 20.0))
            .close();

        let mask = fill(20, 20, &path);
        assert_eq!(mask.at(19, 1), 255, "deep inside the triangle");
        assert_eq!(mask.at(1, 18), 0, "well outside it");
        // On the diagonal the coverage is partial.
        let diagonal = mask.at(10, 10);
        assert!(diagonal > 0 && diagonal < 255, "the diagonal edge should be soft");
    }

    #[test]
    fn a_curve_is_drawn_as_a_curve() {
        let mut path = Path::new();
        path.move_to(Point::new(2.0, 18.0))
            .quad_to(Point::new(10.0, -6.0), Point::new(18.0, 18.0))
            .close();

        let mask = fill(20, 20, &path);
        // The arch is high in the middle and low at the sides.
        assert!(mask.at(10, 6) > 0, "the middle of the arch should be filled");
        assert_eq!(mask.at(2, 4), 0, "the corner outside the arch should be empty");
    }

    #[test]
    fn shapes_outside_the_canvas_do_not_panic_or_leak() {
        let mut rasterizer = Rasterizer::new(8, 8);
        for path in [
            Path::rectangle(-100.0, -100.0, 50.0, 50.0),
            Path::rectangle(100.0, 100.0, 50.0, 50.0),
            Path::rectangle(-10.0, -10.0, 100.0, 100.0),
            Path::rectangle(f32::NAN, 0.0, 10.0, 10.0),
            Path::rectangle(0.0, 0.0, f32::INFINITY, 10.0),
        ] {
            rasterizer.fill(&path);
        }
        let _ = rasterizer.finish();
    }

    #[test]
    fn a_shape_larger_than_the_canvas_fills_it() {
        let mask = fill(8, 8, &Path::rectangle(-10.0, -10.0, 100.0, 100.0));
        assert_eq!(mask.at(0, 0), 255);
        assert_eq!(mask.at(7, 7), 255);
    }

    #[test]
    fn clearing_forgets_what_was_drawn() {
        let mut rasterizer = Rasterizer::new(8, 8);
        rasterizer.fill(&Path::rectangle(1.0, 1.0, 4.0, 4.0));
        rasterizer.clear();
        assert!(rasterizer.finish().is_blank());
    }
}

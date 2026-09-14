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

/// Which parts of a path count as inside it.
///
/// # Why there are two
///
/// Because two answers are wanted and neither is wrong. A font outline draws
/// a hole by winding the inner contour the other way round, and expects the two
/// to cancel — that is the nonzero rule. A drawing that stacks overlapping
/// shapes expects every second layer to be a hole whichever way it was wound —
/// that is the even-odd rule, and it is what the metafile formats ask for
/// unless they say otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rule {
    /// A contour wound the other way cancels the one round it.
    #[default]
    Nonzero,
    /// Every second crossing is a hole, however it was wound.
    EvenOdd,
}

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

    /// A mask of a given size with nothing in it.
    #[must_use]
    pub fn empty(width: usize, height: usize) -> Self {
        Self { width, height, coverage: vec![0; width * height] }
    }

    /// The same coverage blurred.
    ///
    /// # Why three passes of a box
    ///
    /// A box blur on its own looks like a box: a shadow blurred that way has
    /// square corners and a visible edge where the box stops. Three of them in
    /// a row is close enough to a Gaussian that the difference cannot be seen,
    /// and each pass costs one addition and one subtraction per pixel however
    /// wide the blur is — which is what makes a wide blur affordable at all.
    ///
    /// The mask is not made bigger: a blur spreads coverage outwards, so
    /// whatever is to be blurred should be rasterized with room round it
    /// already. A blur with no room to spread into is a blur cut off square,
    /// which is the thing this is for avoiding.
    #[must_use]
    pub fn blurred(&self, radius: f32) -> Self {
        // Three boxes whose widths add up to the radius asked for: the standard
        // way of choosing them, so that the three together stand for a Gaussian
        // of that radius.
        let reach = radius.max(0.0);
        if reach < 0.5 || self.width == 0 || self.height == 0 {
            return self.clone();
        }
        let box_of = (reach * 0.9).round().max(1.0) as usize;

        let mut coverage = self.coverage.clone();
        for _ in 0..3 {
            coverage = run_across(&coverage, self.width, self.height, box_of);
            coverage = run_down(&coverage, self.width, self.height, box_of);
        }
        Self { width: self.width, height: self.height, coverage }
    }

    /// The coverage turned inside out: what was covered is not, and what was
    /// not is.
    ///
    /// An inner shadow is a shadow of everything *outside* the shape, laid
    /// inside it, which is what this is for.
    #[must_use]
    pub fn inverted(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            coverage: self.coverage.iter().map(|value| 255 - value).collect(),
        }
    }

    /// The same coverage moved, in a mask of the same size.
    ///
    /// What falls off the edge is lost, which is what makes this the right
    /// thing for a shadow: the mask was made with room round the shape for
    /// exactly this, and a shadow that wanted more room than that was asked for
    /// is a shadow drawn wrong either way.
    #[must_use]
    pub fn shifted(&self, across: i32, down: i32) -> Self {
        let mut coverage = vec![0u8; self.coverage.len()];
        for y in 0..self.height {
            let from_y = y as i32 - down;
            if from_y < 0 || from_y >= self.height as i32 {
                continue;
            }
            for x in 0..self.width {
                let from_x = x as i32 - across;
                if from_x < 0 || from_x >= self.width as i32 {
                    continue;
                }
                coverage[y * self.width + x] =
                    self.coverage[from_y as usize * self.width + from_x as usize];
            }
        }
        Self { width: self.width, height: self.height, coverage }
    }

    /// Two coverages multiplied: what both of them cover.
    ///
    /// The other mask is taken from the same corner as this one, and anything
    /// outside it covers nothing.
    #[must_use]
    pub fn times(&self, other: &Self) -> Self {
        let coverage = (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .map(|(x, y)| {
                let one = u32::from(self.at(x, y));
                let two = u32::from(other.at(x, y));
                ((one * two + 127) / 255) as u8
            })
            .collect();
        Self { width: self.width, height: self.height, coverage }
    }

    /// The same coverage made stronger, to a limit of solid.
    ///
    /// A glow is a blur that does not fade away as fast as a blur does: Word's
    /// is solid against the shape and thins outwards, and a plain blur of the
    /// shape is faint everywhere.
    #[must_use]
    pub fn strengthened(&self, times: f32) -> Self {
        let times = times.max(0.0);
        Self {
            width: self.width,
            height: self.height,
            coverage: self
                .coverage
                .iter()
                .map(|value| (f32::from(*value) * times).min(255.0) as u8)
                .collect(),
        }
    }
}

/// One box-blur pass along the rows, by a running total.
fn run_across(coverage: &[u8], width: usize, height: usize, reach: usize) -> Vec<u8> {
    let mut out = vec![0u8; coverage.len()];
    let span = reach * 2 + 1;
    for y in 0..height {
        let row = y * width;
        // The total of the window, which starts hanging off the left-hand end:
        // the pixels outside the mask count as nothing, which is what lets a
        // shadow fade at the edge of what was rasterized.
        let mut total: u32 = 0;
        for x in 0..=reach.min(width.saturating_sub(1)) {
            total += u32::from(coverage[row + x]);
        }
        for x in 0..width {
            out[row + x] = (total / span as u32) as u8;
            let leaving = x.checked_sub(reach);
            if let Some(leaving) = leaving {
                total -= u32::from(coverage[row + leaving]);
            }
            let arriving = x + reach + 1;
            if arriving < width {
                total += u32::from(coverage[row + arriving]);
            }
        }
    }
    out
}

/// And one down the columns.
fn run_down(coverage: &[u8], width: usize, height: usize, reach: usize) -> Vec<u8> {
    let mut out = vec![0u8; coverage.len()];
    let span = reach * 2 + 1;
    for x in 0..width {
        let mut total: u32 = 0;
        for y in 0..=reach.min(height.saturating_sub(1)) {
            total += u32::from(coverage[y * width + x]);
        }
        for y in 0..height {
            out[y * width + x] = (total / span as u32) as u8;
            if let Some(leaving) = y.checked_sub(reach) {
                total -= u32::from(coverage[leaving * width + x]);
            }
            let arriving = y + reach + 1;
            if arriving < height {
                total += u32::from(coverage[arriving * width + x]);
            }
        }
    }
    out
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

/// How far a flattened curve may stray from the curve itself, in pixels. A
/// tenth is below what anybody can see and above what costs anything.
const FLATNESS: f32 = 0.1;

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

    /// Produces the coverage of everything added so far, by the nonzero rule.
    #[must_use]
    pub fn finish(&self) -> Mask {
        self.finish_by(Rule::Nonzero)
    }

    /// The same, by whichever rule is asked for.
    #[must_use]
    pub fn finish_by(&self, rule: Rule) -> Mask {
        let mut coverage = vec![0u8; self.width * self.height];

        for row in 0..self.height {
            // The running total across a row is what turns deposited areas into
            // actual coverage.
            let mut total = 0.0f32;
            let source = row * self.stride;
            let destination = row * self.width;
            for column in 0..self.width {
                total += self.area[source + column];
                let value = match rule {
                    Rule::Nonzero => total.abs().min(1.0),
                    // Every second turn of the winding is a hole: the total
                    // folded back on itself at every even number.
                    Rule::EvenOdd => {
                        let folded = total.rem_euclid(2.0);
                        if folded > 1.0 {
                            2.0 - folded
                        } else {
                            folded
                        }
                    }
                };
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
        // How far a cubic strays from the straight line between its ends is
        // bounded by three quarters of the larger of its two second
        // differences, and breaking it into n pieces divides that by n
        // squared. That is what decides how many pieces it takes.
        //
        // This matters more than it sounds. A quadratic curve covers a short
        // piece of a letter and there are many of them; one cubic covers a
        // quarter of an O. A count that is generous for the first is what
        // makes the second come out as a polygon.
        let first_x = from.x - 2.0 * first.x + second.x;
        let first_y = from.y - 2.0 * first.y + second.y;
        let second_x = first.x - 2.0 * second.x + to.x;
        let second_y = first.y - 2.0 * second.y + to.y;
        let deviation =
            (first_x * first_x + first_y * first_y).max(second_x * second_x + second_y * second_y);

        if deviation < 0.01 {
            self.add_line(from, to);
            return;
        }

        let steps = (0.75 * deviation.sqrt() / FLATNESS).sqrt().ceil() as usize;
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
            self.deposit(base + first_column + 1, signed_height * (1.0 - first_area - last_area));
        } else {
            let second_area = inverse_width * (1.5 - (left - left_floor));
            self.deposit(base + first_column + 1, signed_height * (second_area - first_area));

            for column in (first_column + 2)..(last_column - 1) {
                self.deposit(base + column, signed_height * inverse_width);
            }

            let covered = second_area + (last_column - first_column - 3) as f32 * inverse_width;
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

#[cfg(test)]
mod rule_tests {
    use super::*;
    use crate::path::Path;

    /// A square with a smaller square inside it, both wound the same way.
    fn ring() -> Path {
        let mut path = Path::rectangle(0.0, 0.0, 20.0, 20.0);
        let inner = Path::rectangle(5.0, 5.0, 10.0, 10.0);
        for command in &inner.commands {
            path.commands.push(*command);
        }
        path
    }

    fn covered(mask: &Mask, x: usize, y: usize) -> u8 {
        mask.coverage()[y * mask.width() + x]
    }

    #[test]
    fn two_contours_wound_alike_are_solid_by_the_nonzero_rule() {
        let mut rasterizer = Rasterizer::new(20, 20);
        rasterizer.fill(&ring());
        let mask = rasterizer.finish_by(Rule::Nonzero);
        assert_eq!(covered(&mask, 10, 10), 255, "the middle should be filled");
        assert_eq!(covered(&mask, 2, 2), 255, "and so should the rest");
    }

    #[test]
    fn the_same_two_leave_a_hole_by_the_even_odd_rule() {
        let mut rasterizer = Rasterizer::new(20, 20);
        rasterizer.fill(&ring());
        let mask = rasterizer.finish_by(Rule::EvenOdd);
        assert_eq!(covered(&mask, 10, 10), 0, "the middle should be a hole");
        assert_eq!(covered(&mask, 2, 2), 255, "and the rest still filled");
    }
}

#[cfg(test)]
mod blur_tests {
    use super::*;

    /// A mask with one solid square in the middle of it.
    fn square(size: usize, from: usize, to: usize) -> Mask {
        let mut coverage = vec![0u8; size * size];
        for y in from..to {
            for x in from..to {
                coverage[y * size + x] = 255;
            }
        }
        Mask { width: size, height: size, coverage }
    }

    #[test]
    fn a_blur_spreads_coverage_outside_what_was_covered() {
        let mask = square(40, 15, 25);
        let blurred = mask.blurred(4.0);
        assert_eq!(mask.at(12, 20), 0, "nothing was there to begin with");
        assert!(blurred.at(12, 20) > 0, "and the blur should have reached it");
    }

    #[test]
    fn a_blur_fades_outwards_rather_than_stopping() {
        // What tells a blur from a box: each step out is fainter than the last.
        let blurred = square(60, 20, 40).blurred(6.0);
        let near = blurred.at(18, 30);
        let far = blurred.at(14, 30);
        let further = blurred.at(10, 30);
        assert!(near > far && far > further, "{near} {far} {further}");
    }

    #[test]
    fn a_blur_keeps_the_middle_of_a_wide_shape_solid() {
        // A shadow with a hole in the middle would be a shadow of an outline.
        let blurred = square(80, 20, 60).blurred(5.0);
        assert_eq!(blurred.at(40, 40), 255);
    }

    #[test]
    fn a_blur_of_nothing_is_nothing() {
        assert!(Mask::empty(20, 20).blurred(5.0).is_blank());
    }

    #[test]
    fn a_blur_of_no_radius_changes_nothing() {
        let mask = square(20, 5, 15);
        assert_eq!(mask.blurred(0.0).coverage(), mask.coverage());
    }

    #[test]
    fn turning_a_mask_inside_out_covers_what_it_did_not() {
        let mask = square(10, 3, 7);
        let inside_out = mask.inverted();
        assert_eq!(inside_out.at(0, 0), 255);
        assert_eq!(inside_out.at(5, 5), 0);
    }

    #[test]
    fn two_masks_multiplied_cover_what_both_of_them_do() {
        let one = square(10, 0, 6);
        let two = square(10, 4, 10);
        let both = one.times(&two);
        assert_eq!(both.at(5, 5), 255, "where the two overlap");
        assert_eq!(both.at(1, 1), 0, "and where only one of them covers");
    }

    #[test]
    fn a_strengthened_mask_is_stronger_but_never_more_than_solid() {
        let mask = square(10, 3, 7).blurred(2.0);
        let stronger = mask.strengthened(3.0);
        assert!(stronger.at(2, 5) > mask.at(2, 5), "it should be stronger");
        // And held at solid rather than wrapping round to nothing, which would
        // leave a hole in the middle of a glow.
        assert_eq!(mask.strengthened(10.0).at(5, 5), 255);
    }
}

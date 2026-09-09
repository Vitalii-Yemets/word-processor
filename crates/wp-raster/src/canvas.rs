//! A pixel buffer to draw into.

use crate::path::{Command, Path, Point};
use crate::raster::{Mask, Rasterizer};

/// A colour with straight (not premultiplied) alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const TRANSPARENT: Self = Self { red: 0, green: 0, blue: 0, alpha: 0 };

    #[must_use]
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue, alpha: 255 }
    }

    #[must_use]
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self { red, green, blue, alpha }
    }

    /// Reads the six hex digits a document uses, as in `w:color w:val="C00000"`.
    ///
    /// The value "auto" means "let the program decide", which is not a colour;
    /// callers get `None` and pick their own default.
    #[must_use]
    pub fn from_hex(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('#');
        if text.eq_ignore_ascii_case("auto") || text.len() != 6 {
            return None;
        }
        let value = u32::from_str_radix(text, 16).ok()?;
        Some(Self::rgb(
            ((value >> 16) & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            (value & 0xFF) as u8,
        ))
    }
}

/// A band that drawing is confined to.
///
/// Half-open: the right and bottom edges are outside it, the way a range is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clip {
    pub left: usize,
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
}

/// A rectangular buffer of pixels, stored as red, green, blue, alpha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    /// Where drawing is allowed, if it has been narrowed. Nothing means the
    /// whole canvas.
    clip: Option<Clip>,
}

impl Canvas {
    /// A fully transparent canvas.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, pixels: vec![0; width * height * 4], clip: None }
    }

    /// A canvas filled with one colour, which is how a page starts.
    #[must_use]
    pub fn filled(width: usize, height: usize, color: Color) -> Self {
        let mut canvas = Self::new(width, height);
        canvas.clear(color);
        canvas
    }

    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// The raw pixels, four bytes each in red, green, blue, alpha order.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Confines drawing to a band, and gives back what it was confined to
    /// before so it can be put back.
    ///
    /// Nothing outside the band is touched afterwards: this is what lets two
    /// views of one document be drawn into the same window without either
    /// spilling into the other.
    pub fn set_clip(&mut self, x: i32, y: i32, width: i32, height: i32) -> Option<Clip> {
        let previous = self.clip;
        let wanted = Clip {
            left: x.max(0) as usize,
            top: y.max(0) as usize,
            right: (x + width).clamp(0, self.width as i32) as usize,
            bottom: (y + height).clamp(0, self.height as i32) as usize,
        };
        // A band inside a band is the overlap of the two, so nesting works.
        self.clip = Some(match previous {
            None => wanted,
            Some(outer) => Clip {
                left: wanted.left.max(outer.left),
                top: wanted.top.max(outer.top),
                right: wanted.right.min(outer.right),
                bottom: wanted.bottom.min(outer.bottom),
            },
        });
        previous
    }

    /// Puts back whatever [`Self::set_clip`] gave.
    pub fn restore_clip(&mut self, previous: Option<Clip>) {
        self.clip = previous;
    }

    /// Whether a pixel may be drawn.
    #[must_use]
    fn allowed(&self, x: usize, y: usize) -> bool {
        match self.clip {
            None => true,
            Some(clip) => x >= clip.left && x < clip.right && y >= clip.top && y < clip.bottom,
        }
    }
    /// Replaces every pixel with one colour.
    pub fn clear(&mut self, color: Color) {
        for pixel in self.pixels.chunks_exact_mut(4) {
            pixel[0] = color.red;
            pixel[1] = color.green;
            pixel[2] = color.blue;
            pixel[3] = color.alpha;
        }
    }

    /// The colour at a pixel, or transparent outside the canvas.
    #[must_use]
    pub fn pixel(&self, x: usize, y: usize) -> Color {
        if x >= self.width || y >= self.height {
            return Color::TRANSPARENT;
        }
        let at = (y * self.width + x) * 4;
        Color::rgba(self.pixels[at], self.pixels[at + 1], self.pixels[at + 2], self.pixels[at + 3])
    }

    /// Draws one pixel over what is already there.
    ///
    /// `coverage` scales the colour's own alpha, which is how anti-aliased edges
    /// blend rather than replace.
    pub fn blend(&mut self, x: usize, y: usize, color: Color, coverage: u8) {
        if x >= self.width || y >= self.height || coverage == 0 || color.alpha == 0 {
            return;
        }
        if !self.allowed(x, y) {
            return;
        }

        // Both factors are 0..255, so the product is scaled back down.
        let source_alpha = (u32::from(color.alpha) * u32::from(coverage) + 127) / 255;
        if source_alpha == 0 {
            return;
        }

        let at = (y * self.width + x) * 4;
        let inverse = 255 - source_alpha;

        let mix = |source: u8, destination: u8| -> u8 {
            let value = u32::from(source) * source_alpha + u32::from(destination) * inverse;
            ((value + 127) / 255) as u8
        };

        self.pixels[at] = mix(color.red, self.pixels[at]);
        self.pixels[at + 1] = mix(color.green, self.pixels[at + 1]);
        self.pixels[at + 2] = mix(color.blue, self.pixels[at + 2]);
        // The result is at least as opaque as either layer.
        let destination_alpha = u32::from(self.pixels[at + 3]);
        let combined = source_alpha + destination_alpha * inverse / 255;
        self.pixels[at + 3] = combined.min(255) as u8;
    }

    /// Fills an axis-aligned rectangle, clipped to the canvas.
    pub fn fill_rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: Color) {
        let left = x.max(0) as usize;
        let top = y.max(0) as usize;
        let right = (x + width).clamp(0, self.width as i32) as usize;
        let bottom = (y + height).clamp(0, self.height as i32) as usize;

        for row in top..bottom {
            for column in left..right {
                self.blend(column, row, color, 255);
            }
        }
    }

    /// Draws a coverage mask in one colour, with its top-left corner at
    /// `(x, y)`.
    pub fn draw_mask(&mut self, mask: &Mask, x: i32, y: i32, color: Color) {
        for row in 0..mask.height() {
            let target_y = y + row as i32;
            if target_y < 0 || target_y >= self.height as i32 {
                continue;
            }
            for column in 0..mask.width() {
                let target_x = x + column as i32;
                if target_x < 0 || target_x >= self.width as i32 {
                    continue;
                }
                self.blend(target_x as usize, target_y as usize, color, mask.at(column, row));
            }
        }
    }

    /// Fills a path in one colour.
    ///
    /// Only the area the path actually covers is rasterized, rather than the
    /// whole canvas. A page holds thousands of glyphs, each a few dozen pixels
    /// across; rasterizing the full page for every one of them would make
    /// drawing a page take seconds instead of milliseconds.
    pub fn fill_path(&mut self, path: &Path, color: Color) {
        let Some((min_x, min_y, max_x, max_y)) = bounds_of(path) else {
            return;
        };

        // Clip to the canvas before deciding how large a buffer to allocate: a
        // path with enormous coordinates must not ask for an enormous one.
        let left = min_x.floor().max(0.0) as i64;
        let top = min_y.floor().max(0.0) as i64;
        let right = (max_x.ceil() + 1.0).min(self.width as f32) as i64;
        let bottom = (max_y.ceil() + 1.0).min(self.height as f32) as i64;
        if right <= left || bottom <= top {
            return;
        }

        let width = (right - left) as usize;
        let height = (bottom - top) as usize;

        let mut rasterizer = Rasterizer::new(width, height);
        rasterizer.fill(
            &path.transformed(&crate::path::Transform::translate(-(left as f32), -(top as f32))),
        );

        self.draw_mask(&rasterizer.finish(), left as i32, top as i32, color);
    }

    /// The pixels of one rectangle, copied out.
    ///
    /// For putting something back the way it was: the caret blinks twice a
    /// second, and redrawing the whole window each time — every glyph on the
    /// page, rasterized again — is work nobody asked for. Keeping what was
    /// under it and putting it back is a few hundred bytes and no drawing at
    /// all.
    #[must_use]
    pub fn copy_rect(&self, x: i32, y: i32, width: i32, height: i32) -> Vec<u8> {
        let mut out = Vec::new();
        let (left, top, right, bottom) = self.clamped(x, y, width, height);
        for row in top..bottom {
            let start = (row * self.width + left) * 4;
            let end = (row * self.width + right) * 4;
            out.extend_from_slice(&self.pixels[start..end]);
        }
        out
    }

    /// Puts pixels copied out by [`Canvas::copy_rect`] back where they came
    /// from.
    ///
    /// The rectangle must be the one they were taken from; anything else is
    /// ignored rather than drawn askew.
    pub fn paste_rect(&mut self, x: i32, y: i32, width: i32, height: i32, pixels: &[u8]) {
        let (left, top, right, bottom) = self.clamped(x, y, width, height);
        let row_bytes = (right - left) * 4;
        if row_bytes == 0 || pixels.len() != row_bytes * (bottom - top) {
            return;
        }
        for (number, row) in (top..bottom).enumerate() {
            let start = (row * self.width + left) * 4;
            let from = number * row_bytes;
            self.pixels[start..start + row_bytes].copy_from_slice(&pixels[from..from + row_bytes]);
        }
    }

    /// A rectangle cut down to what is actually on the canvas, as left, top,
    /// right and bottom.
    fn clamped(&self, x: i32, y: i32, width: i32, height: i32) -> (usize, usize, usize, usize) {
        let left = (x.max(0) as usize).min(self.width);
        let top = (y.max(0) as usize).min(self.height);
        let right = ((x + width.max(0)).max(0) as usize).min(self.width);
        let bottom = ((y + height.max(0)).max(0) as usize).min(self.height);
        (left, top, right.max(left), bottom.max(top))
    }

    /// Draws another canvas on top of this one.
    pub fn draw_canvas(&mut self, other: &Canvas, x: i32, y: i32) {
        for row in 0..other.height {
            for column in 0..other.width {
                let source = other.pixel(column, row);
                if source.alpha == 0 {
                    continue;
                }
                let target_x = x + column as i32;
                let target_y = y + row as i32;
                if target_x < 0 || target_y < 0 {
                    continue;
                }
                self.blend(target_x as usize, target_y as usize, source, 255);
            }
        }
    }

    /// Draws a rectangle of RGBA pixels, scaled to a given size.
    ///
    /// # Why the sampling is what it is
    ///
    /// A picture in a document is nearly always drawn smaller than it was
    /// stored, and a photograph reduced by nearest-neighbour sampling comes out
    /// visibly ragged — every other row and column simply thrown away. So each
    /// destination pixel is the average of the source pixels it covers, which
    /// is what makes a shrunken photograph look like the photograph. Enlarging
    /// falls back to taking the nearest source pixel, because there is nothing
    /// to average.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_pixels(
        &mut self,
        pixels: &[u8],
        source_width: usize,
        source_height: usize,
        x: i32,
        y: i32,
        width: usize,
        height: usize,
    ) {
        if source_width == 0 || source_height == 0 || width == 0 || height == 0 {
            return;
        }

        for row in 0..height {
            let target_y = y + row as i32;
            if target_y < 0 || target_y as usize >= self.height {
                continue;
            }
            // The band of source rows this destination row stands for.
            let from_y = row * source_height / height;
            let to_y = (((row + 1) * source_height).div_ceil(height)).min(source_height);

            for column in 0..width {
                let target_x = x + column as i32;
                if target_x < 0 || target_x as usize >= self.width {
                    continue;
                }
                let from_x = column * source_width / width;
                let to_x = (((column + 1) * source_width).div_ceil(width)).min(source_width);

                let mut totals = [0u32; 4];
                let mut counted = 0u32;
                for sample_y in from_y..to_y.max(from_y + 1) {
                    for sample_x in from_x..to_x.max(from_x + 1) {
                        let at = (sample_y.min(source_height - 1) * source_width
                            + sample_x.min(source_width - 1))
                            * 4;
                        let Some(sample) = pixels.get(at..at + 4) else { continue };
                        for (total, value) in totals.iter_mut().zip(sample) {
                            *total += u32::from(*value);
                        }
                        counted += 1;
                    }
                }
                if counted == 0 {
                    continue;
                }

                let alpha = (totals[3] / counted) as u8;
                if alpha == 0 {
                    continue;
                }
                let color = Color {
                    red: (totals[0] / counted) as u8,
                    green: (totals[1] / counted) as u8,
                    blue: (totals[2] / counted) as u8,
                    alpha,
                };
                self.blend(target_x as usize, target_y as usize, color, alpha);
            }
        }
    }

    /// The pixels in blue, green, red, alpha order, which is what Windows wants
    /// for a device-independent bitmap.
    #[must_use]
    pub fn to_bgra(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len());
        for pixel in self.pixels.chunks_exact(4) {
            out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
        out
    }
}

/// The bounding box of a path, if it has any points.
fn bounds_of(path: &Path) -> Option<(f32, f32, f32, f32)> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut seen = false;

    let mut include = |point: Point| {
        if point.x.is_finite() && point.y.is_finite() {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
            seen = true;
        }
    };

    for command in &path.commands {
        match *command {
            Command::MoveTo(point) | Command::LineTo(point) => include(point),
            Command::QuadTo(control, point) => {
                include(control);
                include(point);
            }
            Command::CubicTo(first, second, point) => {
                include(first);
                include(second);
                include(point);
            }
            Command::Close => {}
        }
    }

    seen.then_some((min_x, min_y, max_x, max_y))
}

#[cfg(test)]
mod tests {

    #[test]
    fn drawing_outside_a_clip_leaves_the_canvas_alone() {
        let mut canvas = Canvas::filled(20, 20, Color::WHITE);
        canvas.set_clip(0, 0, 20, 10);
        canvas.fill_rect(0, 0, 20, 20, Color::BLACK);

        assert_eq!(canvas.pixel(5, 5), Color::BLACK, "inside the band");
        assert_eq!(canvas.pixel(5, 15), Color::WHITE, "outside it");
    }

    #[test]
    fn a_clip_can_be_put_back_the_way_it_was() {
        let mut canvas = Canvas::filled(20, 20, Color::WHITE);
        let previous = canvas.set_clip(0, 0, 20, 10);
        canvas.restore_clip(previous);
        canvas.fill_rect(0, 0, 20, 20, Color::BLACK);

        assert_eq!(canvas.pixel(5, 15), Color::BLACK, "the band was not lifted");
    }

    #[test]
    fn a_clip_inside_a_clip_is_the_overlap_of_the_two() {
        let mut canvas = Canvas::filled(20, 20, Color::WHITE);
        canvas.set_clip(0, 0, 20, 10);
        canvas.set_clip(0, 5, 20, 10);
        canvas.fill_rect(0, 0, 20, 20, Color::BLACK);

        assert_eq!(canvas.pixel(5, 2), Color::WHITE, "above the inner band");
        assert_eq!(canvas.pixel(5, 7), Color::BLACK, "in both bands");
        assert_eq!(canvas.pixel(5, 12), Color::WHITE, "below the outer band");
    }

    #[test]
    fn a_clip_outside_the_canvas_stops_everything() {
        let mut canvas = Canvas::filled(20, 20, Color::WHITE);
        canvas.set_clip(50, 50, 10, 10);
        canvas.fill_rect(0, 0, 20, 20, Color::BLACK);
        assert_eq!(canvas.pixel(5, 5), Color::WHITE);
    }
    use super::*;

    #[test]
    fn a_new_canvas_is_transparent() {
        let canvas = Canvas::new(4, 4);
        assert_eq!(canvas.pixel(0, 0), Color::TRANSPARENT);
    }

    #[test]
    fn filling_and_reading_back_agree() {
        let canvas = Canvas::filled(4, 4, Color::rgb(10, 20, 30));
        assert_eq!(canvas.pixel(2, 2), Color::rgb(10, 20, 30));
    }

    #[test]
    fn drawing_outside_the_canvas_is_ignored() {
        let mut canvas = Canvas::filled(4, 4, Color::WHITE);
        canvas.fill_rect(-100, -100, 50, 50, Color::BLACK);
        canvas.fill_rect(100, 100, 50, 50, Color::BLACK);
        assert_eq!(canvas.pixel(0, 0), Color::WHITE);
        assert_eq!(canvas.pixel(3, 3), Color::WHITE);
    }

    #[test]
    fn a_rectangle_lands_where_it_was_asked_to() {
        let mut canvas = Canvas::filled(10, 10, Color::WHITE);
        canvas.fill_rect(2, 3, 4, 2, Color::BLACK);

        assert_eq!(canvas.pixel(2, 3), Color::BLACK);
        assert_eq!(canvas.pixel(5, 4), Color::BLACK);
        assert_eq!(canvas.pixel(6, 4), Color::WHITE, "one past the right edge");
        assert_eq!(canvas.pixel(2, 5), Color::WHITE, "one past the bottom edge");
    }

    #[test]
    fn half_transparent_paint_lands_halfway() {
        let mut canvas = Canvas::filled(2, 2, Color::WHITE);
        canvas.blend(0, 0, Color::rgba(0, 0, 0, 128), 255);

        let result = canvas.pixel(0, 0);
        assert!((120..=136).contains(&result.red), "expected roughly half grey, got {result:?}");
    }

    #[test]
    fn filling_a_path_only_touches_where_it_is() {
        let mut canvas = Canvas::filled(20, 20, Color::WHITE);
        canvas.fill_path(&Path::rectangle(5.0, 5.0, 6.0, 6.0), Color::BLACK);

        assert_eq!(canvas.pixel(7, 7), Color::BLACK, "inside the shape");
        assert_eq!(canvas.pixel(2, 2), Color::WHITE, "away from it");
        assert_eq!(canvas.pixel(15, 15), Color::WHITE);
    }

    #[test]
    fn a_path_with_absurd_coordinates_does_not_allocate_absurdly() {
        // The bounds are clipped before a buffer is sized, so a shape claiming
        // to be a million pixels wide does not try to allocate one.
        let mut canvas = Canvas::filled(16, 16, Color::WHITE);
        canvas.fill_path(&Path::rectangle(-1e9, -1e9, 2e9, 2e9), Color::BLACK);
        assert_eq!(canvas.pixel(8, 8), Color::BLACK);
    }

    #[test]
    fn colours_are_read_from_the_form_documents_use() {
        assert_eq!(Color::from_hex("C00000"), Some(Color::rgb(0xC0, 0, 0)));
        assert_eq!(Color::from_hex("#ffffff"), Some(Color::WHITE));
        // "auto" means the program decides, which is not a colour.
        assert_eq!(Color::from_hex("auto"), None);
        assert_eq!(Color::from_hex("nonsense"), None);
        assert_eq!(Color::from_hex(""), None);
    }

    #[test]
    fn bgra_order_is_swapped_for_windows() {
        let canvas = Canvas::filled(1, 1, Color::rgb(1, 2, 3));
        assert_eq!(canvas.to_bgra(), vec![3, 2, 1, 255]);
    }
}

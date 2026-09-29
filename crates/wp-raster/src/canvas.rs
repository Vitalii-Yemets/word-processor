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

/// How a rectangle of pixels is turned as it is drawn.
///
/// Mirroring happens first and the turn second, which is the order the drawing
/// formats state it in and the order a person would do it with a sheet of
/// paper: turn the mirrored picture, not mirror the turned one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Turned {
    /// Clockwise on a canvas, because the canvas counts down the page.
    pub radians: f32,
    pub flipped_across: bool,
    pub flipped_down: bool,
}

impl Turned {
    /// Whether it is turned or mirrored at all.
    ///
    /// Nothing at all is the ordinary case and has a much cheaper path.
    #[must_use]
    pub fn is_turned(self) -> bool {
        self.radians != 0.0 || self.flipped_across || self.flipped_down
    }
}

/// A rectangular buffer of pixels, stored as red, green, blue, alpha.
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    /// Where drawing is allowed, if it has been narrowed. Nothing means the
    /// whole canvas.
    clip: Option<Clip>,
    /// How many of the canvas's pixels one of the caller's is. See
    /// [`Canvas::set_scale`].
    scale: f32,
    /// The width to turn drawing about, where the window reads right to
    /// left. See [`Canvas::set_mirror`].
    mirror: Option<f32>,
}

impl Canvas {
    /// A fully transparent canvas.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height * 4],
            clip: None,
            scale: 1.0,
            mirror: None,
        }
    }

    /// Draws everything from here on scaled up by a factor.
    ///
    /// # Why the canvas scales rather than the caller
    ///
    /// A screen at two hundred dots to the inch shows a window twice as many
    /// pixels across as the same window at a hundred, and everything drawn
    /// on it — the ribbon, the page, the text — has to be twice as big in
    /// pixels to be the same size to the eye. The program that draws the
    /// window works in the pixels of an ordinary screen, as every measurement
    /// in it does; the canvas turns those into the screen's own. Shapes and
    /// letters are paths, so they come out sharp at any factor; pictures are
    /// resampled; a pixel becomes a block. Coordinates given to the canvas
    /// are the caller's, and the ones it reports back are too.
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    }

    /// The factor drawing is scaled by. See [`Canvas::set_scale`].
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Draws everything from here on turned about, for a window read right
    /// to left.
    ///
    /// # Why the canvas turns it and not the caller
    ///
    /// Because every one of the hundreds of places that draw something
    /// would otherwise have to work out where it goes in a mirrored
    /// window, and the one that forgot would be the one nobody noticed.
    /// Here there is a single rule — what was at `x` is now at
    /// `width - x - its own width` — and it is applied to everything the
    /// canvas draws.
    ///
    /// # What is turned and what is not
    ///
    /// Where a thing goes is turned; what it is made of is not. A button
    /// that was on the left is on the right, and the picture on it is the
    /// same picture — turning that would be turning a photograph over.
    /// The exception is a shape given as a path: an arrow that points the
    /// way a person reads has to point the other way in a window read the
    /// other way, so a path is reflected rather than moved. Text is left
    /// to [`Canvas::suspend_mirror`], which is how a line of it is moved
    /// as one piece instead of letter by letter.
    ///
    /// `None` puts it back the way every other window is.
    pub fn set_mirror(&mut self, width: Option<f32>) {
        self.mirror = width.filter(|width| width.is_finite() && *width > 0.0);
    }

    /// The width drawing is turned about, if it is.
    #[must_use]
    pub fn mirror(&self) -> Option<f32> {
        self.mirror
    }

    /// Stops turning for as long as the answer is held, and gives back
    /// what to put back afterwards.
    ///
    /// For something that has to be drawn the way round it was made — a
    /// line of text, the contents of a page — at a place that has been
    /// worked out already.
    pub fn suspend_mirror(&mut self) -> Option<f32> {
        self.mirror.take()
    }

    /// Where a span of the caller's begins once the window is turned.
    fn across(&self, x: i32, width: i32) -> i32 {
        // Saturating, so that a span nobody brought within reach first (see
        // `within_reach`) is turned as far as an i32 goes and then clipped,
        // rather than overflowing on its way to the canvas.
        match self.mirror {
            Some(about) => (about.round() as i32).saturating_sub(x).saturating_sub(width),
            None => x,
        }
    }

    /// A coordinate of the caller's, in the canvas's own pixels.
    fn device(&self, value: i32) -> i32 {
        if self.scale == 1.0 {
            value
        } else {
            (value as f32 * self.scale).round() as i32
        }
    }

    /// A span of the caller's, in the canvas's own pixels: its start and its
    /// length, worked out from both edges so that neighbours still meet.
    fn device_span(&self, start: i32, length: i32) -> (i32, i32) {
        if self.scale == 1.0 {
            return (start, length);
        }
        let from = self.device(start);
        let to = self.device(start.saturating_add(length));
        (from, to.saturating_sub(from))
    }

    /// A canvas filled with one colour, which is how a page starts.
    #[must_use]
    pub fn filled(width: usize, height: usize, color: Color) -> Self {
        let mut canvas = Self::new(width, height);
        canvas.clear(color);
        canvas
    }

    /// How wide the canvas is in the caller's pixels: its own width over
    /// the scale. What everything drawn on it is laid out against.
    #[must_use]
    pub fn width(&self) -> usize {
        if self.scale == 1.0 {
            self.width
        } else {
            (self.width as f32 / self.scale).round() as usize
        }
    }

    /// How tall, in the caller's pixels.
    #[must_use]
    pub fn height(&self) -> usize {
        if self.scale == 1.0 {
            self.height
        } else {
            (self.height as f32 / self.scale).round() as usize
        }
    }

    /// How many pixels the canvas really has across: what [`Canvas::pixels`]
    /// is laid out as.
    #[must_use]
    pub fn pixel_width(&self) -> usize {
        self.width
    }

    /// How many pixels the canvas really has down.
    #[must_use]
    pub fn pixel_height(&self) -> usize {
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
        let (x, width) = self.device_span(self.across(x, width), width);
        let (y, height) = self.device_span(y, height);
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
        let x = self.across(x as i32, 1);
        let (x, y) = (self.device(x) as usize, self.device(y as i32) as usize);
        if x >= self.width || y >= self.height {
            return Color::TRANSPARENT;
        }
        let at = (y * self.width + x) * 4;
        Color::rgba(self.pixels[at], self.pixels[at + 1], self.pixels[at + 2], self.pixels[at + 3])
    }

    /// Draws one pixel over what is already there.
    ///
    /// `coverage` scales the colour's own alpha, which is how anti-aliased edges
    /// blend rather than replace. One of the caller's pixels, which on a
    /// scaled canvas is a block of its own.
    pub fn blend(&mut self, x: usize, y: usize, color: Color, coverage: u8) {
        if self.scale == 1.0 && self.mirror.is_none() {
            self.blend_device(x, y, color, coverage);
            return;
        }
        let (left, width) = self.device_span(self.across(x as i32, 1), 1);
        let (top, height) = self.device_span(y as i32, 1);
        for row in top..top + height.max(1) {
            for column in left..left + width.max(1) {
                if row >= 0 && column >= 0 {
                    self.blend_device(column as usize, row as usize, color, coverage);
                }
            }
        }
    }

    /// Draws one of the canvas's own pixels over what is already there.
    fn blend_device(&mut self, x: usize, y: usize, color: Color, coverage: u8) {
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
    ///
    /// A caller's rectangle may start on the canvas and run as far as an i32
    /// reaches — a float too large for one casts to `i32::MAX` — and its far
    /// edge is then past what an i32 holds: an overflow in a debug build and,
    /// wrapped round to a negative number, a rectangle silently not drawn in
    /// a release one. So its edges are first brought in to a distance from
    /// the canvas that nothing after can overflow, and the far ones added up
    /// wide as well; what shows on the canvas is the same either way.
    pub fn fill_rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: Color) {
        let (x, width) = within_reach(x, width);
        let (y, height) = within_reach(y, height);
        let (x, width) = self.device_span(self.across(x, width), width);
        let (y, height) = self.device_span(y, height);
        let left = x.max(0) as usize;
        let top = y.max(0) as usize;
        let right = (i64::from(x) + i64::from(width)).clamp(0, self.width as i64) as usize;
        let bottom = (i64::from(y) + i64::from(height)).clamp(0, self.height as i64) as usize;

        for row in top..bottom {
            for column in left..right {
                self.blend_device(column, row, color, 255);
            }
        }
    }

    /// Draws a coverage mask in one colour, with its top-left corner at
    /// `(x, y)`.
    pub fn draw_mask(&mut self, mask: &Mask, x: i32, y: i32, color: Color) {
        let x = self.across(x, mask.width() as i32);
        if self.scale == 1.0 {
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
                    self.blend_device(
                        target_x as usize,
                        target_y as usize,
                        color,
                        mask.at(column, row),
                    );
                }
            }
            return;
        }
        // Scaled: every pixel of the canvas inside the mask's place asks the
        // mask which of its own it stands for.
        let (left, width) = self.device_span(x, mask.width() as i32);
        let (top, height) = self.device_span(y, mask.height() as i32);
        for row in top.max(0)..(top + height).min(self.height as i32) {
            let source_row =
                (((row - top) as f32 / self.scale) as usize).min(mask.height().saturating_sub(1));
            for column in left.max(0)..(left + width).min(self.width as i32) {
                let source_column = (((column - left) as f32 / self.scale) as usize)
                    .min(mask.width().saturating_sub(1));
                self.blend_device(
                    column as usize,
                    row as usize,
                    color,
                    mask.at(source_column, source_row),
                );
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
        self.fill_path_by(path, color, crate::raster::Rule::Nonzero);
    }

    /// The same, by whichever rule is asked for. See [`crate::raster::Rule`].
    pub fn fill_path_by(&mut self, path: &Path, color: Color, rule: crate::raster::Rule) {
        self.fill_path_using(path, rule, |_, _| color);
    }

    /// Fills a path with a colour that is different at every pixel.
    ///
    /// # Why a path is filled by asking rather than by being told
    ///
    /// Because a shade is not a colour. A gradient, a hatching, a picture used
    /// as a fill: none of them can be handed in as one colour, and all of them
    /// are the same thing as far as filling goes — a rule that says what colour
    /// a place is. So the rule is handed in instead, and asked once for every
    /// pixel the shape covers. The coordinates it is asked about are the
    /// canvas's own, so a shade knows where on the page it is.
    pub fn fill_path_using(
        &mut self,
        path: &Path,
        rule: crate::raster::Rule,
        colour: impl Fn(usize, usize) -> Color,
    ) {
        let turned;
        let scaled;
        let path = match self.mirror {
            // Reflected about the window's middle, in the caller's own
            // coordinates, before anything else is done to it.
            Some(about) => {
                turned = path.transformed(&crate::path::Transform {
                    a: -1.0,
                    b: 0.0,
                    c: 0.0,
                    d: 1.0,
                    e: about,
                    f: 0.0,
                });
                &turned
            }
            None => path,
        };
        let path = if self.scale == 1.0 {
            path
        } else {
            scaled = path.transformed(&crate::path::Transform::scale(self.scale, self.scale));
            &scaled
        };
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

        let mask = rasterizer.finish_by(rule);
        for row in 0..mask.height() {
            let y = top + row as i64;
            if y < 0 || y >= self.height as i64 {
                continue;
            }
            for column in 0..mask.width() {
                let x = left + column as i64;
                if x < 0 || x >= self.width as i64 {
                    continue;
                }
                let (x, y) = (x as usize, y as usize);
                let shade = if self.scale == 1.0 {
                    colour(x, y)
                } else {
                    colour((x as f32 / self.scale) as usize, (y as f32 / self.scale) as usize)
                };
                self.blend_device(x, y, shade, mask.at(column, row));
            }
        }
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
        let (x, width) = self.device_span(self.across(x, width), width);
        let (y, height) = self.device_span(y, height);
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
        let (x, width) = self.device_span(self.across(x, width), width);
        let (y, height) = self.device_span(y, height);
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

    /// Draws another canvas on top of this one, pixel for pixel, at a place
    /// of the caller's.
    pub fn draw_canvas(&mut self, other: &Canvas, x: i32, y: i32) {
        let x = self.across(x, other.width as i32);
        let (x, y) = (self.device(x), self.device(y));
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
                self.blend_device(target_x as usize, target_y as usize, source, 255);
            }
        }
    }

    /// Draws pixels into a rectangle turned about its middle.
    ///
    /// # Why it works backwards
    ///
    /// Because a turn does not send whole pixels to whole pixels. Walking the
    /// source and putting each pixel where it lands would leave gaps between
    /// them wherever the turn spreads them apart. So the destination is walked
    /// instead: every pixel of the box the turned rectangle needs is asked
    /// which part of the picture it stands for, and a pixel whose answer is
    /// outside the picture is left alone.
    ///
    /// Each answer is the average of the same block of source pixels the
    /// straight version would have used, so a photograph turned through an
    /// angle is no coarser than the same photograph not turned.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    pub fn draw_pixels_turned(
        &mut self,
        pixels: &[u8],
        source_width: usize,
        source_height: usize,
        x: i32,
        y: i32,
        width: usize,
        height: usize,
        turned: Turned,
    ) {
        if !turned.is_turned() {
            self.draw_pixels(pixels, source_width, source_height, x, y, width, height);
            return;
        }
        let (x, width) = self.device_span(self.across(x, width as i32), width as i32);
        let (y, height) = self.device_span(y, height as i32);
        let (width, height) = (width.max(0) as usize, height.max(0) as usize);
        if source_width == 0 || source_height == 0 || width == 0 || height == 0 {
            return;
        }

        let (sin, cos) = turned.radians.sin_cos();
        let (middle_x, middle_y) = (x as f32 + width as f32 / 2.0, y as f32 + height as f32 / 2.0);
        // How far the turned rectangle reaches from its middle, which is the
        // box that has to be walked.
        let (half_width, half_height) = (width as f32 / 2.0, height as f32 / 2.0);
        let reach_x = half_width * cos.abs() + half_height * sin.abs();
        let reach_y = half_width * sin.abs() + half_height * cos.abs();

        let from_x = (middle_x - reach_x).floor().max(0.0) as usize;
        let to_x = (middle_x + reach_x).ceil().max(0.0) as usize;
        let from_y = (middle_y - reach_y).floor().max(0.0) as usize;
        let to_y = (middle_y + reach_y).ceil().max(0.0) as usize;

        // How much of the picture one drawn pixel stands for.
        let across = (source_width as f32 / width as f32).max(1.0) as usize;
        let down = (source_height as f32 / height as f32).max(1.0) as usize;

        for target_y in from_y..to_y.min(self.height) {
            for target_x in from_x..to_x.min(self.width) {
                // Where this pixel is before the turn, measured from the
                // middle: the turn undone.
                let (dx, dy) = (target_x as f32 + 0.5 - middle_x, target_y as f32 + 0.5 - middle_y);
                let mut straight_x = dx * cos + dy * sin + half_width;
                let mut straight_y = -dx * sin + dy * cos + half_height;
                // The mirroring is undone after the turn, because it was done
                // before it: a picture is mirrored where it stands and the
                // whole of it, mirror and all, is then turned.
                if turned.flipped_across {
                    straight_x = width as f32 - straight_x;
                }
                if turned.flipped_down {
                    straight_y = height as f32 - straight_y;
                }
                if straight_x < 0.0
                    || straight_y < 0.0
                    || straight_x >= width as f32
                    || straight_y >= height as f32
                {
                    continue;
                }

                let source_x = (straight_x * source_width as f32 / width as f32) as usize;
                let source_y = (straight_y * source_height as f32 / height as f32) as usize;
                let mut totals = [0u32; 4];
                let mut counted = 0u32;
                for sample_y in source_y..(source_y + down).min(source_height) {
                    for sample_x in source_x..(source_x + across).min(source_width) {
                        let at = (sample_y * source_width + sample_x) * 4;
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
                self.blend_device(target_x, target_y, color, alpha);
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
        let (x, width) = self.device_span(self.across(x, width as i32), width as i32);
        let (y, height) = self.device_span(y, height as i32);
        let (width, height) = (width.max(0) as usize, height.max(0) as usize);
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
                self.blend_device(target_x as usize, target_y as usize, color, alpha);
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

/// A span of a caller's, with both its edges brought within reach: no
/// further from nought than a canvas could ever be across, and near enough
/// that turning it and scaling it cannot overflow an i32.
///
/// Sixteen million pixels is a thousand times the widest screen there is, so
/// nothing that could be seen is cut off; and a span inside it can be turned
/// about the width of a window and scaled a hundredfold and still be an i32.
/// The edges are clamped rather than the length, so that a span reaching far
/// to one side ends where it did on the other.
fn within_reach(start: i32, length: i32) -> (i32, i32) {
    const REACH: i64 = 1 << 24;
    let from = i64::from(start).clamp(-REACH, REACH);
    let to = (i64::from(start) + i64::from(length)).clamp(-REACH, REACH);
    (from as i32, (to - from) as i32)
}

/// The bounding box of a path, if it has any points.
pub fn bounds_of(path: &Path) -> Option<(f32, f32, f32, f32)> {
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
    fn a_rectangle_whose_end_is_past_what_an_i32_holds_is_clipped() {
        // Starting on the canvas and running as far as an i32 reaches, which
        // is what a float too large for one comes to when it is cast. Its far
        // edge is past i32::MAX, and it has to be clipped rather than
        // overflow — or, wrapped round, not be drawn at all.
        let mut canvas = Canvas::filled(10, 10, Color::WHITE);
        canvas.fill_rect(2, 3, i32::MAX, i32::MAX, Color::BLACK);
        assert_eq!(canvas.pixel(2, 3), Color::BLACK, "where it starts");
        assert_eq!(canvas.pixel(9, 9), Color::BLACK, "all the way to the corner");
        assert_eq!(canvas.pixel(1, 3), Color::WHITE, "left of it");
        assert_eq!(canvas.pixel(2, 2), Color::WHITE, "above it");

        // And turned right to left and drawn at twice the size, which work
        // the span out by other roads. Clipping must not move it: one that
        // lies wholly left of the canvas before it is turned lies wholly
        // right of it after, and nothing of it may show.
        let mut canvas = Canvas::filled(20, 20, Color::WHITE);
        canvas.set_scale(2.0);
        canvas.set_mirror(Some(10.0));
        canvas.fill_rect(i32::MIN, 3, i32::MAX, i32::MAX, Color::BLACK);
        assert_eq!(canvas.pixel(0, 9), Color::WHITE, "one off the canvas was drawn on it");
        assert_eq!(canvas.pixel(9, 9), Color::WHITE, "one off the canvas was drawn on it");

        canvas.fill_rect(-5, 3, i32::MAX, i32::MAX, Color::BLACK);
        assert_eq!(canvas.pixel(0, 9), Color::BLACK, "where it starts, turned");
        assert_eq!(canvas.pixel(9, 9), Color::BLACK, "all the way across");
        assert_eq!(canvas.pixel(0, 2), Color::WHITE, "above it");
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

    /// A picture of four quarters, each a colour of its own, so that a turn
    /// can be seen rather than only measured.
    fn quarters() -> Vec<u8> {
        let mut pixels = Vec::new();
        for y in 0..8usize {
            for x in 0..8usize {
                let colour = match (x < 4, y < 4) {
                    (true, true) => [255, 0, 0, 255],
                    (false, true) => [0, 255, 0, 255],
                    (true, false) => [0, 0, 255, 255],
                    (false, false) => [255, 255, 0, 255],
                };
                pixels.extend_from_slice(&colour);
            }
        }
        pixels
    }

    /// Which quarter of the drawn rectangle a colour ended up in.
    fn corners(canvas: &Canvas) -> [Color; 4] {
        [canvas.pixel(4, 4), canvas.pixel(12, 4), canvas.pixel(4, 12), canvas.pixel(12, 12)]
    }

    #[test]
    fn a_straight_turn_draws_what_the_plain_one_draws() {
        let mut turned = Canvas::filled(16, 16, Color::WHITE);
        turned.draw_pixels_turned(&quarters(), 8, 8, 0, 0, 16, 16, Turned::default());
        let mut plain = Canvas::filled(16, 16, Color::WHITE);
        plain.draw_pixels(&quarters(), 8, 8, 0, 0, 16, 16);
        assert_eq!(turned, plain, "a turn of nothing is not the picture itself");
    }

    #[test]
    fn a_quarter_turn_carries_each_corner_round_to_the_next() {
        let straight = {
            let mut canvas = Canvas::filled(16, 16, Color::WHITE);
            canvas.draw_pixels(&quarters(), 8, 8, 0, 0, 16, 16);
            corners(&canvas)
        };
        let mut canvas = Canvas::filled(16, 16, Color::WHITE);
        let quarter = core::f32::consts::FRAC_PI_2;
        canvas.draw_pixels_turned(
            &quarters(),
            8,
            8,
            0,
            0,
            16,
            16,
            Turned { radians: quarter, ..Turned::default() },
        );
        let after = corners(&canvas);
        // Clockwise: top left goes to top right, top right to bottom right.
        assert_eq!(after[1], straight[0], "the top left did not come round to the top right");
        assert_eq!(after[3], straight[1]);
        assert_eq!(after[2], straight[3]);
        assert_eq!(after[0], straight[2]);
    }

    #[test]
    fn mirroring_across_swaps_left_and_right_and_leaves_top_and_bottom() {
        let straight = {
            let mut canvas = Canvas::filled(16, 16, Color::WHITE);
            canvas.draw_pixels(&quarters(), 8, 8, 0, 0, 16, 16);
            corners(&canvas)
        };
        let mut canvas = Canvas::filled(16, 16, Color::WHITE);
        canvas.draw_pixels_turned(
            &quarters(),
            8,
            8,
            0,
            0,
            16,
            16,
            Turned { flipped_across: true, ..Turned::default() },
        );
        let after = corners(&canvas);
        assert_eq!(after[0], straight[1], "the top right is not on the left");
        assert_eq!(after[1], straight[0]);
        assert_eq!(after[2], straight[3]);
        assert_eq!(after[3], straight[2]);
    }

    #[test]
    fn mirroring_down_swaps_top_and_bottom() {
        let straight = {
            let mut canvas = Canvas::filled(16, 16, Color::WHITE);
            canvas.draw_pixels(&quarters(), 8, 8, 0, 0, 16, 16);
            corners(&canvas)
        };
        let mut canvas = Canvas::filled(16, 16, Color::WHITE);
        canvas.draw_pixels_turned(
            &quarters(),
            8,
            8,
            0,
            0,
            16,
            16,
            Turned { flipped_down: true, ..Turned::default() },
        );
        let after = corners(&canvas);
        assert_eq!(after[0], straight[2]);
        assert_eq!(after[2], straight[0]);
    }

    #[test]
    fn a_turned_picture_reaches_outside_the_box_it_was_given() {
        // A square turned an eighth of a turn does not fit in its own square,
        // and the corners it grows are the whole point of walking the
        // destination rather than the source.
        let mut canvas = Canvas::filled(40, 40, Color::WHITE);
        canvas.draw_pixels_turned(
            &quarters(),
            8,
            8,
            12,
            12,
            16,
            16,
            Turned { radians: core::f32::consts::FRAC_PI_4, ..Turned::default() },
        );
        // Straight above the middle, which the unturned square does not reach.
        assert_ne!(canvas.pixel(20, 9), Color::WHITE, "the turned corner is missing");
    }

    #[test]
    fn nothing_at_all_is_the_cheap_case() {
        assert!(!Turned::default().is_turned());
        assert!(Turned { radians: 0.1, ..Turned::default() }.is_turned());
        assert!(Turned { flipped_down: true, ..Turned::default() }.is_turned());
    }

    #[test]
    fn bgra_order_is_swapped_for_windows() {
        let canvas = Canvas::filled(1, 1, Color::rgb(1, 2, 3));
        assert_eq!(canvas.to_bgra(), vec![3, 2, 1, 255]);
    }
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    fn raw(canvas: &Canvas, x: usize, y: usize) -> u8 {
        canvas.pixels[(y * canvas.width + x) * 4 + 3]
    }

    #[test]
    fn a_scaled_canvas_draws_the_callers_pixels_as_blocks() {
        let mut canvas = Canvas::new(8, 8);
        canvas.set_scale(2.0);
        canvas.fill_rect(1, 1, 2, 1, Color::BLACK);
        // The caller's rectangle from 1 to 3 across and 1 to 2 down is the
        // canvas's from 2 to 6 across and 2 to 4 down.
        assert_eq!(canvas.pixel(1, 1), Color::BLACK, "the caller's own pixel reads back");
        assert_eq!(raw(&canvas, 2, 2), 255);
        assert_eq!(raw(&canvas, 5, 3), 255);
        assert_eq!(raw(&canvas, 1, 2), 0);
        assert_eq!(raw(&canvas, 6, 2), 0);
        assert_eq!(raw(&canvas, 2, 4), 0);
        // A blended pixel is a block too.
        canvas.blend(0, 0, Color::BLACK, 255);
        assert_eq!(raw(&canvas, 0, 0), 255);
        assert_eq!(raw(&canvas, 1, 1), 255);
        assert_eq!(raw(&canvas, 2, 0), 0);
    }

    #[test]
    fn a_path_on_a_scaled_canvas_is_scaled_before_it_is_rasterized() {
        let mut canvas = Canvas::new(8, 8);
        canvas.set_scale(2.0);
        let mut path = Path::new();
        path.move_to(Point { x: 1.0, y: 1.0 });
        path.line_to(Point { x: 3.0, y: 1.0 });
        path.line_to(Point { x: 3.0, y: 3.0 });
        path.line_to(Point { x: 1.0, y: 3.0 });
        path.close();
        canvas.fill_path(&path, Color::BLACK);
        assert_eq!(raw(&canvas, 2, 2), 255);
        assert_eq!(raw(&canvas, 5, 5), 255);
        assert_eq!(raw(&canvas, 6, 6), 0);
        assert_eq!(raw(&canvas, 1, 1), 0);
    }

    #[test]
    fn what_is_copied_out_of_a_scaled_canvas_goes_back_where_it_was() {
        let mut canvas = Canvas::new(8, 8);
        canvas.set_scale(2.0);
        canvas.fill_rect(0, 0, 4, 4, Color::WHITE);
        let under = canvas.copy_rect(1, 1, 1, 1);
        assert_eq!(under.len(), 2 * 2 * 4);
        canvas.fill_rect(1, 1, 1, 1, Color::BLACK);
        assert_eq!(canvas.pixel(1, 1), Color::BLACK);
        canvas.paste_rect(1, 1, 1, 1, &under);
        assert_eq!(canvas.pixel(1, 1), Color::WHITE);
    }
}

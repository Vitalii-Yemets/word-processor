//! Drawing a laid-out page into pixels.
//!
//! Everything the layout stage decided is already in device coordinates, so this
//! only has to fetch each glyph's outline, scale it, and fill it.
//!
//! Outlines are cached as they are fetched. A page of text asks for the letter
//! "e" hundreds of times, and re-reading it out of the font file each time would
//! dominate the cost of drawing a page.

use std::collections::HashMap;

use wp_font::{GlyphId, PathCommand};
use wp_raster::{Canvas, Color, Path, Point, Transform};

use wp_docx::effects::Effect;

use crate::device::Device;
use crate::layout::{Drawing, GlyphEffect, Page, PositionedGlyph};
use crate::library::FontLibrary;

/// Draws pages, keeping the outlines it has already read.
#[derive(Debug)]
pub struct Renderer<'a> {
    library: &'a FontLibrary,
    /// Outlines in font units, keyed by face and glyph.
    outlines: HashMap<(usize, u16), Option<CachedOutline>>,
}

/// A glyph's outline together with the grid it was designed on.
#[derive(Clone, Debug)]
struct CachedOutline {
    path: Path,
    units_per_em: f32,
}

impl<'a> Renderer<'a> {
    #[must_use]
    pub fn new(library: &'a FontLibrary) -> Self {
        Self { library, outlines: HashMap::new() }
    }

    /// Draws a page onto a new canvas.
    #[must_use]
    pub fn render(&mut self, page: &Page, background: Color) -> Canvas {
        let width = page.width.round().max(1.0) as usize;
        let height = page.height.round().max(1.0) as usize;
        let mut canvas = Canvas::filled(width, height, background);
        self.draw_onto(&mut canvas, page, 0.0, 0.0);
        canvas
    }

    /// The image a device is given for one page.
    ///
    /// The whole sheet at the device's resolution, less the band the device
    /// cannot draw in. A printer's origin is the corner of what it can reach
    /// rather than the corner of the paper, so an image that included that band
    /// would come out shifted by it — every line a quarter of an inch too far
    /// down and to the right.
    #[must_use]
    pub fn page_for_device(&mut self, page: &Page, device: Device, background: Color) -> Canvas {
        let (width, height) = Self::printable_dots(page, device);
        self.band_for_device(page, device, 0, height, background, width)
    }

    /// One band of that image, so that a whole page need never be held.
    ///
    /// At six hundred dots to the inch a page of A4 is a hundred and forty
    /// megabytes of pixels; a printer is fed a few hundred rows at a time and
    /// this is what draws them.
    #[must_use]
    pub fn band_for_device(
        &mut self,
        page: &Page,
        device: Device,
        top: usize,
        rows: usize,
        background: Color,
        width: usize,
    ) -> Canvas {
        let left = device.dots(device.unprintable.left);
        let above = device.dots(device.unprintable.top);
        let mut canvas = Canvas::filled(width.max(1), rows.max(1), background);
        self.draw_onto(&mut canvas, page, -left, -(above + top as f32));
        canvas
    }

    /// How much of a page a device can actually draw, in its own dots.
    #[must_use]
    pub fn printable_dots(page: &Page, device: Device) -> (usize, usize) {
        let across = device.dots(device.unprintable.left + device.unprintable.right);
        let down = device.dots(device.unprintable.top + device.unprintable.bottom);
        let width = (page.width - across).round().max(1.0) as usize;
        let height = (page.height - down).round().max(1.0) as usize;
        (width, height)
    }

    /// Draws a page onto an existing canvas at an offset.
    ///
    /// The offset is what makes scrolling work: the same page is drawn at a
    /// different vertical position rather than laid out again.
    /// Draws only what falls inside a rectangle, and leaves out the rest.
    ///
    /// For text that has to stay inside a box it may be longer than: a font
    /// with a long name in a narrow list, a path in a field. A letter is drawn
    /// only if the whole of it fits, so the text stops rather than being cut
    /// down the middle — which is what every list in Word does.
    pub fn draw_within(
        &mut self,
        canvas: &mut Canvas,
        page: &Page,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let mut inside = Page { width: page.width, height: page.height, ..Page::default() };
        inside.glyphs = page
            .glyphs
            .iter()
            .filter(|glyph| {
                glyph.x >= x
                    && glyph.x + glyph.advance <= x + width
                    && glyph.baseline >= y
                    && glyph.baseline <= y + height + glyph.size
            })
            .copied()
            .collect();
        self.draw_onto(canvas, &inside, 0.0, 0.0);
    }

    pub fn draw_onto(&mut self, canvas: &mut Canvas, page: &Page, offset_x: f32, offset_y: f32) {
        // Decorations go first so that a glyph sitting on an underline is drawn
        // over it rather than under it.
        // The drawings that go under the text: pictures and shapes in one
        // sequence, ordered by what their anchors say rather than by which kind
        // they are. A caption drawn over a picture is a caption; a picture
        // drawn over a caption is a mistake.
        for drawing in page.drawings_under() {
            draw_drawing(canvas, drawing, offset_x, offset_y);
        }

        // Shapes that are not rectangles: the slices of a pie, the line of a
        // line chart. Over the decorations and under the text, the same as a
        // shape is.
        for placed in &page.paths {
            let moved = placed.path.transformed(&Transform::translate(offset_x, offset_y));
            canvas.fill_path(&moved, placed.color);
        }

        for decoration in &page.decorations {
            canvas.fill_rect(
                (decoration.x + offset_x).round() as i32,
                (decoration.y + offset_y).round() as i32,
                decoration.width.ceil().max(1.0) as i32,
                decoration.height.ceil().max(1.0) as i32,
                decoration.color,
            );
        }

        // The text of the document, and then the text inside the shapes under
        // it — which is placed in page coordinates already and so is drawn the
        // same way. A shape in front of the text has its own text drawn with
        // it, below, or the shape would be filled in over its own words.
        let inside: Vec<&PositionedGlyph> = page
            .shapes
            .iter()
            .filter(|shape| !shape.over_text)
            .flat_map(|shape| &shape.text)
            .collect();
        self.draw_glyphs(canvas, page.glyphs.iter().chain(inside), offset_x, offset_y);

        // And last, the drawings a person put in front of the text. Word's
        // "In Front of Text", which until now was a command that said it had
        // done something and had not.
        for drawing in page.drawings_over() {
            draw_drawing(canvas, drawing, offset_x, offset_y);
            if let Drawing::Shape(shape) = drawing {
                self.draw_glyphs(canvas, shape.text.iter(), offset_x, offset_y);
            }
        }
    }

    /// Draws letters onto the canvas, wherever they came from.
    fn draw_glyphs<'glyphs>(
        &mut self,
        canvas: &mut Canvas,
        glyphs: impl Iterator<Item = &'glyphs PositionedGlyph>,
        offset_x: f32,
        offset_y: f32,
    ) {
        for glyph in glyphs {
            // A tab or a break takes up room and carries a position, but there
            // is nothing to draw for it.
            if glyph.invisible {
                continue;
            }
            let baseline = glyph.baseline + offset_y;
            // Skip anything entirely off the canvas before doing any work for it.
            if baseline + glyph.size < 0.0 || baseline - glyph.size * 2.0 > canvas.height() as f32 {
                continue;
            }

            let Some(cached) = self.outline(glyph.face, glyph.glyph) else {
                continue;
            };
            // Font outlines are y-up on a design grid; the canvas is y-down in
            // pixels. This is the transform that reconciles the two.
            let scale = glyph.size / cached.units_per_em;
            let transform =
                Transform::stretched_glyph(scale, glyph.stretch, glyph.x + offset_x, baseline);
            let path = cached.path.transformed(&transform);

            // The effect goes under the letter: a shadow behind it, an outline
            // sticking out from under its edges, a glow further out still, a
            // reflection below it. Drawing it afterwards would put it on top.
            if let Some(effect) = glyph.effect {
                draw_effect(canvas, &path, &effect, glyph.size, baseline);
            }
            canvas.fill_path(&path, glyph.color);
        }
    }

    /// Draws a page's text through a transform.
    ///
    /// (See [`draw_drawing`] below for the one that draws a picture or a
    /// shape's body, which is not a method because it needs nothing the
    /// renderer holds.)
    ///
    /// Only the glyphs: the things this is for — a watermark turned corner to
    /// corner, and whatever else is drawn at an angle later — are made of
    /// letters and nothing else, and a rotated underline or a rotated picture
    /// would need thinking about separately.
    ///
    /// The transform is applied *after* the one that puts each glyph on the
    /// page, so it works in page coordinates: to turn a line about a point,
    /// pass [`Transform::rotate_about`] naming that point.
    pub fn draw_transformed(&mut self, canvas: &mut Canvas, page: &Page, transform: &Transform) {
        for glyph in &page.glyphs {
            if glyph.invisible {
                continue;
            }
            let Some(cached) = self.outline(glyph.face, glyph.glyph) else {
                continue;
            };
            let scale = glyph.size / cached.units_per_em;
            let placed = Transform::stretched_glyph(scale, glyph.stretch, glyph.x, glyph.baseline)
                .then(transform);
            canvas.fill_path(&cached.path.transformed(&placed), glyph.color);
        }
    }

    /// The outline of a glyph in font units, read once and kept.
    fn outline(&mut self, face: usize, glyph: GlyphId) -> Option<&CachedOutline> {
        let key = (face, glyph.0);
        if !self.outlines.contains_key(&key) {
            let cached = self.read_outline(face, glyph);
            self.outlines.insert(key, cached);
        }
        self.outlines.get(&key)?.as_ref()
    }

    fn read_outline(&self, face: usize, glyph: GlyphId) -> Option<CachedOutline> {
        let font = self.library.face(face)?.font()?;
        let outline = font.outline(glyph).ok()??;

        let mut path = Path::new();
        for command in &outline.commands {
            match *command {
                PathCommand::MoveTo(point) => {
                    path.move_to(Point::new(point.x, point.y));
                }
                PathCommand::LineTo(point) => {
                    path.line_to(Point::new(point.x, point.y));
                }
                PathCommand::QuadTo(control, point) => {
                    path.quad_to(Point::new(control.x, control.y), Point::new(point.x, point.y));
                }
                PathCommand::Close => {
                    path.close();
                }
            }
        }

        Some(CachedOutline { path, units_per_em: f32::from(font.units_per_em()) })
    }

    /// How many distinct glyphs have been read so far.
    #[must_use]
    pub fn cached_glyphs(&self) -> usize {
        self.outlines.len()
    }
}

/// The eight directions an outline or a glow is pushed out in.
///
/// A stroker would follow the letter's edge and give an even line. Copies of
/// the letter pushed out in a ring and drawn under it give the same look from
/// a distance, and cost one fill each instead of a curve offsetting algorithm.
const RING: &[(f32, f32)] = &[
    (-1.0, 0.0),
    (1.0, 0.0),
    (0.0, -1.0),
    (0.0, 1.0),
    (-0.7, -0.7),
    (0.7, -0.7),
    (-0.7, 0.7),
    (0.7, 0.7),
];

/// Draws the effect a glyph asks for, underneath the glyph itself.
///
/// Word blurs its shadow and its glow and fades its reflection away down the
/// page. Neither the blur nor the fade is done here: a shadow is a flat offset
/// copy, a glow is three rings each fainter than the last, and a reflection is
/// one faint mirrored copy. From a page's reading distance the difference is
/// small; up close it is visible, and worth saying so.
/// Draws one drawing's body: a picture's pixels, or a shape's shadow, fill and
/// outline.
///
/// Not the text inside a shape, which is letters and goes through the renderer.
fn draw_drawing(canvas: &mut Canvas, drawing: Drawing<'_>, offset_x: f32, offset_y: f32) {
    match drawing {
        Drawing::Picture(picture) => canvas.draw_pixels(
            &picture.image.pixels,
            picture.image.width,
            picture.image.height,
            (picture.x + offset_x).round() as i32,
            (picture.y + offset_y).round() as i32,
            picture.width.round().max(0.0) as usize,
            picture.height.round().max(0.0) as usize,
        ),
        Drawing::Shape(shape) => {
            let (x, y) = (shape.x + offset_x, shape.y + offset_y);
            // The shadow first, under everything: an offset copy of whatever
            // the shape's outline encloses.
            if let Some((colour, distance)) = shape.shadow {
                let path = crate::geometry::path_in(
                    shape.preset,
                    x + distance,
                    y + distance,
                    shape.width,
                    shape.height,
                );
                canvas.fill_path(&path, colour);
            }
            if let Some(fill) = shape.fill {
                let path = crate::geometry::path_in(shape.preset, x, y, shape.width, shape.height);
                canvas.fill_path(&path, fill);
            }
            if let Some(outline) = shape.outline {
                let path = crate::geometry::outline_in(
                    shape.preset,
                    x,
                    y,
                    shape.width,
                    shape.height,
                    shape.outline_weight,
                );
                canvas.fill_path(&path, outline);
            }
        }
    }
}

fn draw_effect(canvas: &mut Canvas, path: &Path, effect: &GlyphEffect, size: f32, baseline: f32) {
    match effect.kind {
        Effect::None => {}
        Effect::Shadow => {
            // Down and to the right, which is where the format says it falls.
            let offset = (size * 0.06).max(1.0);
            let shifted = path.transformed(&Transform::translate(offset, offset));
            canvas.fill_path(&shifted, faded(effect.color, 150));
        }
        Effect::Outline => {
            let spread = (size * 0.045).max(0.75);
            for (dx, dy) in RING {
                let shifted = path.transformed(&Transform::translate(dx * spread, dy * spread));
                canvas.fill_path(&shifted, effect.color);
            }
        }
        Effect::Glow => {
            // Three rings, each further out and fainter, which is as close to a
            // blur as this gets.
            for (step, alpha) in [(1.0_f32, 90_u8), (2.0, 55), (3.0, 28)] {
                let spread = (size * 0.05 * step).max(step);
                for (dx, dy) in RING {
                    let shifted = path.transformed(&Transform::translate(dx * spread, dy * spread));
                    canvas.fill_path(&shifted, faded(effect.color, alpha));
                }
            }
        }
        Effect::Reflection => {
            // Turned over about the baseline and dropped a little, so the
            // letter and its reflection do not touch.
            let gap = size * 0.1;
            let mirrored = Transform::translate(0.0, -baseline)
                .then(&Transform::scale(1.0, -1.0))
                .then(&Transform::translate(0.0, baseline + gap));
            canvas.fill_path(&path.transformed(&mirrored), faded(effect.color, 70));
        }
    }
}

/// A colour with less of it showing through.
fn faded(color: Color, alpha: u8) -> Color {
    Color { alpha: ((u16::from(color.alpha) * u16::from(alpha)) / 255) as u8, ..color }
}

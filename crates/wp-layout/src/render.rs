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
use crate::layout::{Drawing, GlyphEffect, Page, PositionedGlyph, Turn};
use crate::library::FontLibrary;

/// Draws pages, keeping the outlines it has already read.
#[derive(Debug)]
pub struct Renderer<'a> {
    library: &'a FontLibrary,
    /// Outlines in font units, keyed by face and glyph.
    outlines: HashMap<(usize, u16), Option<CachedOutline>>,
    /// Which glyphs a coloured glyph is drawn from, and in what.
    layers: HashMap<(usize, u16), Option<ColourLayers>>,
    /// The pictures of a font that keeps its glyphs as pictures, decoded.
    pictures: HashMap<(usize, u16), Option<CachedPicture>>,
}

/// Which glyphs a coloured glyph is drawn from, and in what: `None` where the
/// layer takes the colour of the text around it.
type ColourLayers = Vec<(GlyphId, Option<Color>)>;

/// A glyph's outline together with the grid it was designed on.
#[derive(Clone, Debug)]
struct CachedOutline {
    path: Path,
    units_per_em: f32,
}

/// A glyph a font keeps as a picture, decoded to pixels.
#[derive(Clone, Debug)]
struct CachedPicture {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
    /// The size the picture was drawn for, which is what says how far it has
    /// to be scaled for the size the text is.
    pixels_per_em: u16,
    bearing_x: i8,
    bearing_y: i8,
}

impl<'a> Renderer<'a> {
    #[must_use]
    pub fn new(library: &'a FontLibrary) -> Self {
        Self { library, outlines: HashMap::new(), layers: HashMap::new(), pictures: HashMap::new() }
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
        // A window read right to left moves what is drawn; it does not turn
        // it inside out. So a page — which here is as much a line of a
        // label as a page of a document — is moved across as one piece,
        // and everything in it is drawn the way round it was laid out.
        if let Some(about) = canvas.mirror() {
            let Some((from, to)) = page.horizontal_extent() else { return };
            let moved = about - (from + offset_x) - (to + offset_x);
            let held = canvas.suspend_mirror();
            self.draw_onto(canvas, page, offset_x + moved, offset_y);
            canvas.set_mirror(held);
            return;
        }
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

        // The text of the document. Which of its letters are turned is kept as
        // spans rather than on every letter, so it is looked up here; a letter
        // in a turned cell turns about its own origin, which is already where
        // it belongs on the page.
        let own = page.glyphs.iter().enumerate().map(|(at, glyph)| {
            let turn = Self::quarter_turn(page.turn_of(at)).map(|angle| {
                Transform::rotate_about(angle, glyph.x + offset_x, glyph.baseline + offset_y)
            });
            (glyph, turn)
        });
        self.draw_glyphs(canvas, own, offset_x, offset_y);

        // Then the text inside the shapes under it, which is placed in page
        // coordinates already and so is drawn the same way — turned with the
        // shape it is in, because the words on a turned sign are turned too. A
        // shape in front of the text has its own text drawn with it, below, or
        // the shape would be filled in over its own words.
        for shape in page.shapes.iter().filter(|shape| !shape.over_text) {
            let turn = shape_turn(shape, offset_x, offset_y);
            let text = shape.text.iter().map(|glyph| (glyph, turn));
            self.draw_glyphs(canvas, text, offset_x, offset_y);
        }

        // And last, the drawings a person put in front of the text. Word's
        // "In Front of Text", which until now was a command that said it had
        // done something and had not.
        for drawing in page.drawings_over() {
            draw_drawing(canvas, drawing, offset_x, offset_y);
            if let Drawing::Shape(shape) = drawing {
                let turn = shape_turn(shape, offset_x, offset_y);
                let text = shape.text.iter().map(|glyph| (glyph, turn));
                self.draw_glyphs(canvas, text, offset_x, offset_y);
            }
        }
    }

    /// Draws letters onto the canvas, wherever they came from.
    ///
    /// A letter may carry a transform in page coordinates, applied after the one
    /// that puts it where it belongs: the quarter turn of a cell that reads
    /// sideways, or the angle of the shape whose text it is.
    fn draw_glyphs<'glyphs>(
        &mut self,
        canvas: &mut Canvas,
        glyphs: impl Iterator<Item = (&'glyphs PositionedGlyph, Option<Transform>)>,
        offset_x: f32,
        offset_y: f32,
    ) {
        for (glyph, turn) in glyphs {
            // A tab or a break takes up room and carries a position, but there
            // is nothing to draw for it.
            if glyph.invisible {
                continue;
            }
            let x = glyph.x + offset_x;
            let baseline = glyph.baseline + offset_y;
            // Skip anything entirely off the canvas before doing any work for
            // it. A turned letter reaches as far along the page as it is tall,
            // so the band it could be in is the same one either way — but a
            // letter turned with a shape lands somewhere else altogether, so
            // what is tested is where it lands.
            let landed = turn.map_or(baseline, |turn| turn.apply(Point { x, y: baseline }).y);
            if landed + glyph.size < 0.0 || landed - glyph.size * 2.0 > canvas.height() as f32 {
                continue;
            }

            // A font may hold this glyph as a picture, or as a stack of other
            // glyphs each in its own colour. Either way it is not one shape in
            // the colour of the text.
            if self.draw_coloured(canvas, glyph, x, baseline, turn) {
                continue;
            }

            let Some(cached) = self.outline(glyph.face, glyph.glyph) else {
                continue;
            };
            // Font outlines are y-up on a design grid; the canvas is y-down in
            // pixels. This is the transform that reconciles the two.
            let scale = glyph.size / cached.units_per_em;
            let mut transform = Transform::stretched_glyph(scale, glyph.stretch, x, baseline);
            if let Some(turn) = turn {
                transform = transform.then(&turn);
            }
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

    /// How far round a turned letter goes, in radians.
    ///
    /// Clockwise on a canvas for text that reads downwards, the other way for
    /// text that reads upwards. Nothing at all for the ordinary way up, which
    /// is what almost every letter of almost every document is.
    fn quarter_turn(turn: Turn) -> Option<f32> {
        match turn {
            Turn::None => None,
            Turn::Down => Some(core::f32::consts::FRAC_PI_2),
            Turn::Up => Some(-core::f32::consts::FRAC_PI_2),
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

    /// Draws a glyph that a font holds in colours of its own.
    ///
    /// Returns whether it did: a letter in an ordinary font is not one of
    /// these, and is drawn the way every letter is.
    fn draw_coloured(
        &mut self,
        canvas: &mut Canvas,
        glyph: &PositionedGlyph,
        x: f32,
        baseline: f32,
        turn: Option<Transform>,
    ) -> bool {
        // A picture is drawn square on or not at all: a turned emoji would
        // need the pixels turned with it, and nothing yet asks for one.
        if turn.is_none() {
            if let Some(picture) = self.picture(glyph.face, glyph.glyph, glyph.size) {
                let scale = glyph.size / f32::from(picture.pixels_per_em.max(1));
                let width = (picture.width as f32 * scale).round().max(1.0) as usize;
                let height = (picture.height as f32 * scale).round().max(1.0) as usize;
                let left = x + f32::from(picture.bearing_x) * scale;
                let top = baseline - f32::from(picture.bearing_y) * scale;
                canvas.draw_pixels(
                    &picture.pixels,
                    picture.width,
                    picture.height,
                    left.round() as i32,
                    top.round() as i32,
                    width,
                    height,
                );
                return true;
            }
        }

        let Some(layers) = self.layers(glyph.face, glyph.glyph) else { return false };
        if layers.is_empty() {
            return false;
        }

        // Each layer is an ordinary glyph in a colour of its own, drawn in the
        // order the font gives: the first is at the back.
        for (id, colour) in layers {
            let Some(cached) = self.outline(glyph.face, id) else { continue };
            let scale = glyph.size / cached.units_per_em;
            let mut transform = Transform::stretched_glyph(scale, glyph.stretch, x, baseline);
            if let Some(turn) = turn {
                transform = transform.then(&turn);
            }
            canvas.fill_path(&cached.path.transformed(&transform), colour.unwrap_or(glyph.color));
        }
        true
    }

    /// The layers of a glyph, read once and kept.
    fn layers(&mut self, face: usize, glyph: GlyphId) -> Option<ColourLayers> {
        let key = (face, glyph.0);
        if !self.layers.contains_key(&key) {
            let read = self.read_layers(face, glyph);
            self.layers.insert(key, read);
        }
        self.layers.get(&key).cloned().flatten()
    }

    fn read_layers(&self, face: usize, glyph: GlyphId) -> Option<ColourLayers> {
        let font = self.library.face(face)?.font()?;
        Some(
            font.colour_layers(glyph)?
                .into_iter()
                .map(|layer| {
                    let colour = layer.colour.map(|colour| {
                        Color::rgba(colour.red, colour.green, colour.blue, colour.alpha)
                    });
                    (layer.glyph, colour)
                })
                .collect(),
        )
    }

    /// The picture of a glyph, decoded once and kept.
    ///
    /// A font of pictures holds several sizes; the one nearest what is being
    /// drawn is taken, decoded, and then scaled to whatever size is wanted, so
    /// the decoding is paid for once however many times the emoji appears.
    fn picture(&mut self, face: usize, glyph: GlyphId, size: f32) -> Option<&CachedPicture> {
        let key = (face, glyph.0);
        if !self.pictures.contains_key(&key) {
            let read = self.read_picture(face, glyph, size);
            self.pictures.insert(key, read);
        }
        self.pictures.get(&key)?.as_ref()
    }

    fn read_picture(&self, face: usize, glyph: GlyphId, size: f32) -> Option<CachedPicture> {
        let font = self.library.face(face)?.font()?;
        let wanted = size.round().clamp(1.0, f32::from(u16::MAX)) as u16;
        let bitmap = font.bitmap(glyph, wanted)?;
        let image = wp_image::png::decode(bitmap.png).ok()?;
        Some(CachedPicture {
            pixels: image.pixels,
            width: image.width,
            height: image.height,
            pixels_per_em: bitmap.pixels_per_em,
            bearing_x: bitmap.bearing_x,
            bearing_y: bitmap.bearing_y,
        })
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
                PathCommand::CubicTo(first, second, point) => {
                    path.cubic_to(
                        Point::new(first.x, first.y),
                        Point::new(second.x, second.y),
                        Point::new(point.x, point.y),
                    );
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
        Drawing::Picture(picture) => canvas.draw_pixels_turned(
            &picture.image.pixels,
            picture.image.width,
            picture.image.height,
            (picture.x + offset_x).round() as i32,
            (picture.y + offset_y).round() as i32,
            picture.width.round().max(0.0) as usize,
            picture.height.round().max(0.0) as usize,
            wp_raster::Turned {
                radians: picture.turn,
                flipped_across: picture.flipped_across,
                flipped_down: picture.flipped_down,
            },
        ),
        Drawing::Ink(ink) => {
            for (path, colour) in &ink.drawing.paths {
                let moved = path.transformed(&Transform::translate(offset_x, offset_y));
                canvas.fill_path(&moved, *colour);
            }
        }
        Drawing::Shape(shape) => {
            let (x, y) = (shape.x + offset_x, shape.y + offset_y);
            let turn = shape_turn(shape, offset_x, offset_y);
            let turned = |path: Path| match turn {
                Some(turn) => path.transformed(&turn),
                None => path,
            };
            // What the shape covers, which everything drawn round it is worked
            // out from. It is turned with the shape but what falls on it is
            // not: the light does not turn with a sign, so a sign on its side
            // still casts its shadow downwards.
            let area = turned(crate::geometry::path_in(
                shape.preset,
                &shape.adjusts,
                x,
                y,
                shape.width,
                shape.height,
            ));

            // The theme's own shadow, for a shape whose document says nothing
            // about its effects: an offset copy and no blur, which is what a
            // theme's effect style amounts to here.
            if shape.effects.is_nothing() {
                if let Some((colour, distance)) = shape.shadow {
                    let path = area.transformed(&Transform::translate(distance, distance));
                    canvas.fill_path(&path, colour);
                }
            } else {
                draw_under(canvas, shape, &area);
            }
            // A soft edge is the shape drawn through its own coverage, blurred:
            // solid in the middle and fading to nothing at the edge. It fades
            // outwards as well, but the fill is held to the shape anyway, so
            // what shows is the fade inwards — which is the soft edge.
            let softly = (shape.effects.soft_edge > 0.0)
                .then(|| coverage_of(&area, 1.0))
                .flatten()
                .map(|(mask, at_x, at_y)| (mask.blurred(shape.effects.soft_edge), at_x, at_y));
            let soften = |colour: Color, px: usize, py: usize| match &softly {
                Some((mask, at_x, at_y)) => {
                    let (column, row) = (px as i32 - at_x, py as i32 - at_y);
                    let coverage = if column < 0 || row < 0 {
                        0
                    } else {
                        mask.at(column as usize, row as usize)
                    };
                    let alpha = (u32::from(colour.alpha) * u32::from(coverage) + 127) / 255;
                    Color::rgba(colour.red, colour.green, colour.blue, alpha as u8)
                }
                None => colour,
            };

            // The depth behind the face, drawn before it and therefore under
            // it.
            if !shape.solid.is_flat() {
                draw_depth(canvas, shape, &area);
            }

            if !shape.fill.is_nothing() {
                let path = turned(crate::geometry::path_in(
                    shape.preset,
                    &shape.adjusts,
                    x,
                    y,
                    shape.width,
                    shape.height,
                ));
                // Where in the shape a pixel is, as fractions of its box: a
                // gradient needs to know, and the fill is asked pixel by pixel
                // so that it can be a gradient or a hatching and not only a
                // colour. See [`crate::paint::Paint`].
                let (width, height) = (shape.width.max(0.0001), shape.height.max(0.0001));
                let fill = &shape.fill;
                canvas.fill_path_using(&path, wp_raster::Rule::Nonzero, |px, py| {
                    let across = (px as f32 - x) / width;
                    let down = (py as f32 - y) / height;
                    let colour =
                        fill.at_pixel(px, py, across.clamp(0.0, 1.0), down.clamp(0.0, 1.0));
                    soften(colour, px, py)
                });
            }
            // The shadow inside it, over the fill and under the line: a shadow
            // that fell over the line would make the line look like a hole too.
            draw_inside(canvas, shape, &area);

            // And the bevel round the face, which is light on one side and
            // dark on the other.
            if shape.solid.bevel > 0.0 {
                draw_bevel(canvas, shape, x, y);
            }

            if let Some(outline) = shape.outline {
                // A connector that had to be routed round the shapes it joins
                // is drawn from the route rather than from its preset: what it
                // is fastened to settled where it goes, and the preset settles
                // only whether the corners are turned. See
                // [`crate::connectors::route`].
                let along = shape.route.clone().unwrap_or_else(|| {
                    crate::geometry::path_in(
                        shape.preset,
                        &shape.adjusts,
                        x,
                        y,
                        shape.width,
                        shape.height,
                    )
                });
                let band = if shape.route.is_some() {
                    crate::geometry::band_along(&along, shape.outline_weight)
                } else {
                    crate::geometry::outline_in(
                        shape.preset,
                        &shape.adjusts,
                        x,
                        y,
                        shape.width,
                        shape.height,
                        shape.outline_weight,
                    )
                };
                let band = turned(band);
                canvas.fill_path_using(&band, wp_raster::Rule::Nonzero, |px, py| {
                    soften(outline, px, py)
                });

                // And what is drawn at the ends of that line. They are worked
                // out from the shape and turned with it, so an arrow on a
                // drawing stood on its side still points along its own line.
                for (tip, end) in [
                    (crate::connectors::LineTip::Head, shape.head_end),
                    (crate::connectors::LineTip::Tail, shape.tail_end),
                ] {
                    let head = crate::connectors::arrowhead(&along, tip, end, shape.outline_weight);
                    if !head.is_empty() {
                        canvas.fill_path(&turned(head), outline);
                    }
                }
            }
        }
    }
}

/// The coverage of a path, and where the top left of it lands on the page.
///
/// Only as big as the path and the room asked for round it: a mask the size of
/// the page for every effect on every shape would be a page's worth of work per
/// shadow.
fn coverage_of(path: &Path, room: f32) -> Option<(wp_raster::Mask, i32, i32)> {
    let mut low = (f32::MAX, f32::MAX);
    let mut high = (f32::MIN, f32::MIN);
    for at in path.points() {
        low = (low.0.min(at.x), low.1.min(at.y));
        high = (high.0.max(at.x), high.1.max(at.y));
    }
    if low.0 > high.0 {
        return None;
    }
    let room = room.max(0.0).ceil();
    let left = (low.0 - room).floor();
    let top = (low.1 - room).floor();
    let width = (high.0 + room - left).ceil().max(1.0);
    let height = (high.1 + room - top).ceil().max(1.0);
    // A mask of a few million pixels is a shape nobody asked to be drawn that
    // big; the effects on it are left off rather than the program stopping.
    if width * height > 16_000_000.0 {
        return None;
    }

    let mut raster = wp_raster::Rasterizer::new(width as usize, height as usize);
    raster.fill(&path.transformed(&Transform::translate(-left, -top)));
    Some((raster.finish(), left as i32, top as i32))
}

/// The shadow, the glow and the reflection, which are drawn before the shape
/// itself and therefore under it.
fn draw_under(canvas: &mut Canvas, shape: &crate::PlacedShape, area: &Path) {
    let effects = &shape.effects;

    // The reflection first, because it is furthest from the shape: the shape
    // upside down about its own bottom edge, fading downwards.
    if let Some(reflection) = effects.reflection {
        // The bottom of what is drawn, and not the bottom of the shape's own
        // box: the path has the page's own corner and the scroll in it
        // already, and a reflection mirrored about a line somewhere else lands
        // somewhere else.
        let (top, bottom) = area
            .points()
            .fold((f32::MAX, f32::MIN), |(top, bottom), at| (top.min(at.y), bottom.max(at.y)));
        let tall = (bottom - top).max(1.0);
        let flip = Transform::translate(0.0, -bottom)
            .then(&Transform::scale(1.0, -1.0))
            .then(&Transform::translate(0.0, bottom + reflection.below));
        let mirrored = area.transformed(&flip);
        if let Some((mask, at_x, at_y)) = coverage_of(&mirrored, reflection.blur + 1.0) {
            let mask = mask.blurred(reflection.blur);
            // Which colour: the fill's own at the middle of the shape, because
            // a reflection is the shape again and not a shadow of it.
            let colour = shape.fill.at_pixel(0, 0, 0.5, 0.5);
            let fade = (tall * reflection.fades_by).max(1.0);
            for row in 0..mask.height() {
                let down = (at_y + row as i32) as f32 - (bottom + reflection.below);
                let share = 1.0 - (down / fade).clamp(0.0, 1.0);
                let alpha = (f32::from(reflection.start) * share) as u8;
                if alpha == 0 {
                    continue;
                }
                let colour = Color::rgba(colour.red, colour.green, colour.blue, alpha);
                let (at_x, at_y) = (at_x, at_y + row as i32);
                if at_y < 0 {
                    continue;
                }
                for column in 0..mask.width() {
                    let coverage = mask.at(column, row);
                    let across = at_x + column as i32;
                    if coverage > 0 && across >= 0 {
                        canvas.blend(across as usize, at_y as usize, colour, coverage);
                    }
                }
            }
        }
    }

    // Then the shadow it casts, which is the shape's own coverage moved and
    // blurred.
    if let Some(shadow) = effects.outer_shadow {
        let moved = area.transformed(&Transform::translate(shadow.across, shadow.down));
        if let Some((mask, at_x, at_y)) = coverage_of(&moved, shadow.blur + 1.0) {
            canvas.draw_mask(&mask.blurred(shadow.blur), at_x, at_y, shadow.colour);
        }
    }

    // And the glow round it, which is the same blurred and then made stronger:
    // a glow is solid against the shape and thins outwards, and a plain blur is
    // faint everywhere.
    if let Some(glow) = effects.glow {
        if let Some((mask, at_x, at_y)) = coverage_of(area, glow.reach + 1.0) {
            let spread = mask.blurred(glow.reach / 2.0).strengthened(2.5);
            canvas.draw_mask(&spread, at_x, at_y, glow.colour);
        }
    }
}

/// The shadow drawn inside the shape, which goes over its fill.
///
/// The shadow of everything *outside* the shape, laid inside it: the coverage
/// turned inside out, moved, blurred, and then held to the shape itself so that
/// none of it falls outside.
fn draw_inside(canvas: &mut Canvas, shape: &crate::PlacedShape, area: &Path) {
    let Some(shadow) = shape.effects.inner_shadow else {
        return;
    };
    let room = shadow.blur + shadow.across.abs() + shadow.down.abs() + 1.0;
    let Some((mask, at_x, at_y)) = coverage_of(area, room) else {
        return;
    };
    // The same coverage moved, turned inside out and blurred: what falls on the
    // inside of the shape when the light comes from the other side. Held to the
    // shape itself at the end, so that none of it falls outside.
    let moved = mask.shifted(shadow.across.round() as i32, shadow.down.round() as i32);
    let inside = moved.inverted().blurred(shadow.blur).times(&mask);
    canvas.draw_mask(&inside, at_x, at_y, shadow.colour);
}

/// The depth behind a shape: the face again and again, stepped back the way the
/// scene is turned.
///
/// # Why copies rather than sides
///
/// Because the sides of a solid seen flat on *are* the face swept along the
/// depth, and sweeping a shape of curves and corners into a band means working
/// out its silhouette from the direction it is swept in. Stepping the shape
/// back a pixel at a time fills the same area, and the step is a pixel because
/// anything coarser leaves the sides striped.
fn draw_depth(canvas: &mut Canvas, shape: &crate::PlacedShape, area: &Path) {
    let solid = shape.solid;
    let reach = solid.across.hypot(solid.down);
    if reach <= 0.0 {
        return;
    }
    // What the sides are: the colour the document gives, or the shape's own
    // fill taken darker, which is what a side away from the light looks like.
    let sides = solid.sides.unwrap_or_else(|| {
        let face = shape.fill.at_pixel(0, 0, 0.5, 0.5);
        Color::rgba(
            (u32::from(face.red) * 2 / 3) as u8,
            (u32::from(face.green) * 2 / 3) as u8,
            (u32::from(face.blue) * 2 / 3) as u8,
            face.alpha,
        )
    });

    let steps = reach.ceil().max(1.0) as usize;
    for step in (1..=steps).rev() {
        let along = step as f32 / steps as f32;
        let at = area.transformed(&Transform::translate(solid.across * along, solid.down * along));
        canvas.fill_path(&at, sides);
    }
}

/// The bevel round the face of a solid: the edge rolled over, lit from one
/// side.
///
/// The band is the shape's own outline at the width of the bevel. Which half of
/// that band catches the light is worked out by moving the shape: shift it away
/// from the light and the edge it leaves uncovered is the edge the light falls
/// on.
fn draw_bevel(canvas: &mut Canvas, shape: &crate::PlacedShape, x: f32, y: f32) {
    let solid = shape.solid;
    let band = crate::geometry::outline_in(
        shape.preset,
        &shape.adjusts,
        x,
        y,
        shape.width,
        shape.height,
        solid.bevel,
    );
    let Some((band, at_x, at_y)) = coverage_of(&band, 1.0) else {
        return;
    };
    let Some((face, face_x, face_y)) = coverage_of(
        &crate::geometry::path_in(shape.preset, &shape.adjusts, x, y, shape.width, shape.height),
        1.0,
    ) else {
        return;
    };
    // Both are wanted in the same frame.
    let face = face.shifted(face_x - at_x, face_y - at_y);
    let step = solid.bevel.ceil().max(1.0) as i32;

    // The light comes from above and to the left, which is where Word's own
    // lighting comes from unless a document says otherwise.
    let lit = band.times(&face.shifted(step, step).inverted());
    let shaded = band.times(&face.shifted(-step, -step).inverted());
    let white = Color::rgba(255, 255, 255, (200.0 * solid.shine) as u8);
    let black = Color::rgba(0, 0, 0, (150.0 * solid.shine) as u8);
    canvas.draw_mask(&lit, at_x, at_y, white);
    canvas.draw_mask(&shaded, at_x, at_y, black);
}

/// How a shape is turned where it stands, in page coordinates.
///
/// A drawing turns about its own middle and is mirrored about its own middle,
/// and the mirroring comes first: the format states it that way, and so would
/// anyone doing it with a sheet of paper — turn the mirrored shape, not mirror
/// the turned one. `None` for a shape that is neither, which is nearly all of
/// them and saves every path a pass through a transform.
fn shape_turn(
    shape: &crate::layout::PlacedShape,
    offset_x: f32,
    offset_y: f32,
) -> Option<Transform> {
    if shape.turn == 0.0 && !shape.flipped_across && !shape.flipped_down {
        return None;
    }
    let middle_x = shape.x + offset_x + shape.width / 2.0;
    let middle_y = shape.y + offset_y + shape.height / 2.0;
    let mirror = Transform::scale(
        if shape.flipped_across { -1.0 } else { 1.0 },
        if shape.flipped_down { -1.0 } else { 1.0 },
    );
    Some(
        Transform::translate(-middle_x, -middle_y)
            .then(&mirror)
            .then(&Transform::rotate(shape.turn))
            .then(&Transform::translate(middle_x, middle_y)),
    )
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

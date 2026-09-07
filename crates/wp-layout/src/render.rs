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

use crate::layout::Page;
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

    /// Draws a page onto an existing canvas at an offset.
    ///
    /// The offset is what makes scrolling work: the same page is drawn at a
    /// different vertical position rather than laid out again.
    pub fn draw_onto(&mut self, canvas: &mut Canvas, page: &Page, offset_x: f32, offset_y: f32) {
        // Decorations go first so that a glyph sitting on an underline is drawn
        // over it rather than under it.
        for decoration in &page.decorations {
            canvas.fill_rect(
                (decoration.x + offset_x).round() as i32,
                (decoration.y + offset_y).round() as i32,
                decoration.width.ceil().max(1.0) as i32,
                decoration.height.ceil().max(1.0) as i32,
                decoration.color,
            );
        }

        for glyph in &page.glyphs {
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
            let transform = Transform::glyph(scale, glyph.x + offset_x, baseline);
            canvas.fill_path(&cached.path.transformed(&transform), glyph.color);
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
        let font = self.library.face(face)?.font().ok()?;
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
                    path.quad_to(
                        Point::new(control.x, control.y),
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

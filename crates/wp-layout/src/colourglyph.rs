//! A colour glyph drawn as a picture, whatever kind of colour glyph it is.
//!
//! # Why one picture for every kind
//!
//! A font keeps an emoji in one of four ways — a list of layers, a tree of
//! paints, a picture, or an SVG drawing — and everything that draws one wants
//! the same thing back: pixels, and where they go against the pen and the
//! baseline. The screen draws them; a PDF carries them. So every kind but the
//! plain list of layers, which is drawn as the outlines it is, becomes the
//! same [`GlyphPicture`] here: a picture from a font that keeps pictures is
//! decoded, a tree of paints is painted, a drawing is drawn.
//!
//! # Painting a tree
//!
//! Each node of a `COLR` version 1 tree is a picture of its own the size of
//! the glyph's box: a gradient fills it, a glyph's outline keeps only what is
//! inside it, a transform moves the space the nodes under it are painted in,
//! a composite puts one node's picture onto another's the way its mode says.
//! The colours of a gradient are mixed as the format requires — in linear
//! light, premultiplied — and turned back into ordinary colours after. See
//! [`wp_raster::compose`].

use wp_font::paint::{ColourGlyph, ColourLine, CompositeMode, Extend, Paint, Swatch};
use wp_font::{Font, GlyphId, ImageFormat, Outline, PathCommand};
use wp_raster::compose::{self, Mode, Pixmap, Premultiplied, Spread};
use wp_raster::{Color, Path, Point, Rule, Transform};

/// A glyph as pixels, and where they go.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphPicture {
    /// Red, green, blue and opacity, not premultiplied, row after row.
    pub pixels: Vec<u8>,
    pub width: usize,
    pub height: usize,
    /// The size the picture is drawn for, in pixels to the em: a glyph drawn
    /// at another size is this picture scaled.
    pub pixels_per_em: f32,
    /// Where its left edge is, right of the pen, and its top edge, above the
    /// baseline, in the pixels of that size.
    pub left: f32,
    pub top: f32,
}

/// The largest side a painted picture may have, in pixels: enough for a glyph
/// printed at several centimetres, and a bound on what a font can make this do.
const LARGEST: f32 = 2048.0;

/// A glyph drawn as a picture, at a size in pixels to the em and against the
/// colour of the text around it — or `None` for a glyph that is not kept as
/// a picture, a tree or a drawing.
#[must_use]
pub fn picture_of(
    font: &Font<'_>,
    glyph: GlyphId,
    pixels_per_em: f32,
    text: Color,
) -> Option<GlyphPicture> {
    if let Some(tree) = font.colour_glyph(glyph) {
        return painted(font, &tree, pixels_per_em, text);
    }
    let wanted = pixels_per_em.round().clamp(1.0, f32::from(u16::MAX)) as u16;
    if let Some(bitmap) = font.bitmap(glyph, wanted) {
        let image = match bitmap.format {
            ImageFormat::Png => wp_image::png::decode(bitmap.data),
            ImageFormat::Jpeg => wp_image::jpeg::decode(bitmap.data),
            ImageFormat::Tiff => wp_image::tiff::decode(bitmap.data),
        }
        .ok()?;
        let top = if bitmap.from_bottom {
            f32::from(bitmap.bearing_y) + image.height as f32
        } else {
            f32::from(bitmap.bearing_y)
        };
        return Some(GlyphPicture {
            pixels: image.pixels,
            width: image.width,
            height: image.height,
            pixels_per_em: f32::from(bitmap.pixels_per_em.max(1)),
            left: f32::from(bitmap.bearing_x),
            top,
        });
    }
    if let Some(document) = font.svg_document(glyph) {
        return drawn(font, glyph, document.data, pixels_per_em, text);
    }
    None
}

/// The largest an SVG document may come to once it is decompressed: a
/// drawing of a glyph is kilobytes, and a document that says it is more is
/// one made to fill memory.
const LARGEST_DOCUMENT: usize = 16 << 20;

/// A glyph drawn from its SVG document.
///
/// The document's space is the glyph's: one unit to a font unit, the origin
/// at the pen, y running down — so a glyph above the baseline is drawn at
/// negative y. The element drawn is the one whose id is `glyph` and the
/// glyph's number; one document may hold many.
fn drawn(
    font: &Font<'_>,
    glyph: GlyphId,
    data: &[u8],
    pixels_per_em: f32,
    text: Color,
) -> Option<GlyphPicture> {
    let data = if data.starts_with(&[0x1F, 0x8B]) {
        std::borrow::Cow::Owned(wp_deflate::inflate_gzip(data, LARGEST_DOCUMENT).ok()?)
    } else {
        std::borrow::Cow::Borrowed(data)
    };
    let text_of = core::str::from_utf8(&data).ok()?;
    let mut document = wp_svg::Document::parse(text_of).ok()?;
    let units = f32::from(font.units_per_em().max(1));
    document.set_viewport(units);
    let id = format!("glyph{}", glyph.0);

    let scale = pixels_per_em / units;
    let (x0, y0, x1, y1) = document.reach(&id, &Transform::scale(1.0, 1.0))?;
    if !(x1 > x0 && y1 > y0) {
        return None;
    }
    let width = ((x1 - x0) * scale).ceil().min(LARGEST) as usize + 2;
    let height = ((y1 - y0) * scale).ceil().min(LARGEST) as usize + 2;
    let place =
        Transform { a: scale, b: 0.0, c: 0.0, d: scale, e: 1.0 - x0 * scale, f: 1.0 - y0 * scale };
    let mut picture = Pixmap::new(width, height);
    document.draw(&id, &mut picture, &place, text);
    Some(GlyphPicture {
        pixels: picture.to_rgba(),
        width,
        height,
        pixels_per_em,
        left: x0 * scale - 1.0,
        top: 1.0 - y0 * scale,
    })
}

/// A glyph's outline as a path, in font units.
pub(crate) fn path_of(outline: &Outline) -> Path {
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
    path
}

// ---------------------------------------------------------------------------
// Painting a tree
// ---------------------------------------------------------------------------

/// A tree of paints as a picture.
fn painted(
    font: &Font<'_>,
    tree: &ColourGlyph,
    pixels_per_em: f32,
    text: Color,
) -> Option<GlyphPicture> {
    let units = f32::from(font.units_per_em().max(1));
    let scale = pixels_per_em / units;
    // The box to paint: the glyph's own, where the font gives one, or else
    // the reach of every outline in the tree.
    let (x0, y0, x1, y1) =
        tree.clip.or_else(|| reach(font, &tree.paint, IDENTITY)).or_else(|| {
            let metrics = font.vertical_metrics();
            let advance = f32::from(font.advance(GlyphId(0)).max(1));
            Some((0.0, f32::from(metrics.descender), advance, f32::from(metrics.ascender)))
        })?;
    if !(x1 > x0 && y1 > y0) {
        return None;
    }
    let width = ((x1 - x0) * scale).ceil().min(LARGEST) as usize + 2;
    let height = ((y1 - y0) * scale).ceil().min(LARGEST) as usize + 2;
    // Font units, y up, to the picture's pixels, y down, with a pixel of room
    // all round for the edges.
    let place =
        Transform { a: scale, b: 0.0, c: 0.0, d: -scale, e: 1.0 - x0 * scale, f: y1 * scale + 1.0 };

    let painter = Painter { font, width, height, text };
    let mut picture = painter.paint(&tree.paint, &place, 0);
    if let Some(clip) = tree.clip {
        picture.keep_inside(&rectangle(clip).transformed(&place), Rule::Nonzero);
    }
    Some(GlyphPicture {
        pixels: picture.to_rgba(),
        width,
        height,
        pixels_per_em,
        left: x0 * scale - 1.0,
        top: y1 * scale + 1.0,
    })
}

const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// How deep a tree is painted: the font's reader refuses deeper ones, and this
/// is the same bound for the same reason.
const DEEPEST: u8 = 64;

struct Painter<'f, 'a> {
    font: &'f Font<'a>,
    width: usize,
    height: usize,
    text: Color,
}

impl Painter<'_, '_> {
    /// One node as a picture, painted in a space placed on the picture by a
    /// transform.
    fn paint(&self, paint: &Paint, place: &Transform, depth: u8) -> Pixmap {
        let (width, height) = (self.width, self.height);
        if depth > DEEPEST {
            return Pixmap::new(width, height);
        }
        let next = depth + 1;
        match paint {
            Paint::Layers(layers) => {
                let mut out = Pixmap::new(width, height);
                for layer in layers {
                    out.composite(&self.paint(layer, place, next), Mode::SourceOver);
                }
                out
            }
            Paint::Solid(swatch) => {
                let colour = self.swatch(*swatch);
                Pixmap::shaded(width, height, |_, _| colour)
            }
            Paint::Linear { line, from, to, rotation } => {
                // The run goes from the first point to where the second lands
                // on the line square to the third's direction: colours then
                // stay the same along every line parallel to that direction.
                let (p0, p1, p2) = (point(*from), point(*to), point(*rotation));
                let (rx, ry) = (p2.x - p0.x, p2.y - p0.y);
                let (px, py) = (ry, -rx);
                let square = px * px + py * py;
                let (dx, dy) = (p1.x - p0.x, p1.y - p0.y);
                let parallel = (dx * ry - dy * rx).abs() <= f32::EPSILON * (square.max(1.0));
                if square <= f32::EPSILON || dx * dx + dy * dy <= f32::EPSILON || parallel {
                    return Pixmap::new(width, height);
                }
                let share = (dx * px + dy * py) / square;
                let p3 = Point::new(p0.x + px * share, p0.y + py * share);
                self.gradient(place, line, |at| compose::along_line(at, p0, p3))
            }
            Paint::Radial { line, from, to } => {
                let (first, second) = ((point(from.0), from.1), (point(to.0), to.1));
                self.gradient(place, line, |at| compose::between_circles(at, first, second))
            }
            Paint::Sweep { line, centre, start, end } => {
                // Anticlockwise from the first angle, round to the second —
                // the whole way round where the two are the same — and
                // nothing painted outside that.
                let centre = point(*centre);
                let start = start.rem_euclid(360.0);
                let span = (end - start).rem_euclid(360.0);
                let span = if span <= f32::EPSILON { 360.0 } else { span };
                self.gradient(place, line, |at| {
                    let t = compose::round_point(at, centre, start, start + span)?;
                    let t = t.rem_euclid(360.0 / span);
                    (t <= 1.0).then_some(t)
                })
            }
            Paint::Glyph { glyph, paint } => {
                let mut picture = self.paint(paint, place, next);
                match self.font.outline(*glyph).ok().flatten() {
                    Some(outline) => {
                        picture.keep_inside(&path_of(&outline).transformed(place), Rule::Nonzero);
                        picture
                    }
                    None => Pixmap::new(width, height),
                }
            }
            Paint::Clip { clip, paint } => {
                let mut picture = self.paint(paint, place, next);
                picture.keep_inside(&rectangle(*clip).transformed(place), Rule::Nonzero);
                picture
            }
            Paint::Transform { matrix, paint } => {
                let moved = transform(*matrix).then(place);
                self.paint(paint, &moved, next)
            }
            Paint::Composite { source, mode, backdrop } => {
                let mut picture = self.paint(backdrop, place, next);
                picture.composite(&self.paint(source, place, next), mode_of(*mode));
                picture
            }
        }
    }

    /// A gradient as a picture: every pixel taken back into the space the
    /// gradient is written in, asked how far along it is, and given the
    /// colour of the run there.
    fn gradient(
        &self,
        place: &Transform,
        line: &ColourLine,
        along: impl Fn(Point) -> Option<f32>,
    ) -> Pixmap {
        let Some(back) = inverse(place) else { return Pixmap::new(self.width, self.height) };
        let stops = self.stops(line);
        let spread = match line.extend {
            Extend::Pad => Spread::Pad,
            Extend::Repeat => Spread::Repeat,
            Extend::Reflect => Spread::Reflect,
        };
        Pixmap::shaded(self.width, self.height, |x, y| {
            let at = back.apply(Point::new(x, y));
            match along(at) {
                Some(t) => from_linear(compose::colour_along(&stops, t, spread)),
                None => [0.0; 4],
            }
        })
    }

    /// The stops of a run, in order, as premultiplied colours in linear
    /// light — which is what the format says they are mixed in.
    fn stops(&self, line: &ColourLine) -> Vec<(f32, Premultiplied)> {
        let mut stops: Vec<(f32, Premultiplied)> = line
            .stops
            .iter()
            .map(|(offset, swatch)| (*offset, to_linear(self.swatch(*swatch))))
            .collect();
        stops.sort_by(|one, other| one.0.total_cmp(&other.0));
        stops
    }

    /// A colour a paint names, premultiplied.
    fn swatch(&self, swatch: Swatch) -> Premultiplied {
        match swatch {
            Swatch::Colour(colour) => {
                compose::premultiplied(colour.red, colour.green, colour.blue, colour.alpha)
            }
            Swatch::Text { alpha } => {
                let text = self.text;
                let alpha = (f32::from(text.alpha) * alpha).round().clamp(0.0, 255.0) as u8;
                compose::premultiplied(text.red, text.green, text.blue, alpha)
            }
        }
    }
}

/// A colour in the ordinary, non-linear sRGB scale, taken into linear light,
/// still premultiplied.
fn to_linear(colour: Premultiplied) -> Premultiplied {
    let alpha = colour[3];
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let channel = |value: f32| {
        let value = (value / alpha).clamp(0.0, 1.0);
        let linear =
            if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) };
        linear * alpha
    };
    [channel(colour[0]), channel(colour[1]), channel(colour[2]), alpha]
}

/// And back.
fn from_linear(colour: Premultiplied) -> Premultiplied {
    let alpha = colour[3];
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let channel = |value: f32| {
        let value = (value / alpha).clamp(0.0, 1.0);
        let encoded = if value <= 0.003_130_8 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        encoded * alpha
    };
    [channel(colour[0]), channel(colour[1]), channel(colour[2]), alpha]
}

/// How far a tree reaches, in its own space: the box round every outline in
/// it, each where its transforms put it. `None` for a tree of nothing but
/// colours, which reaches everywhere and is painted in the glyph's own box.
fn reach(font: &Font<'_>, paint: &Paint, matrix: [f32; 6]) -> Option<(f32, f32, f32, f32)> {
    let corners = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let mut out: Option<(f32, f32, f32, f32)> = None;
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
            let (px, py) = apply(matrix, x, y);
            out = union(out, Some((px, py, px, py)));
        }
        out
    };
    match paint {
        Paint::Layers(layers) => {
            layers.iter().fold(None, |so_far, layer| union(so_far, reach(font, layer, matrix)))
        }
        Paint::Glyph { glyph, .. } => {
            let outline = font.outline(*glyph).ok().flatten()?;
            let bounds = outline.bounds;
            corners(
                f32::from(bounds.min_x),
                f32::from(bounds.min_y),
                f32::from(bounds.max_x),
                f32::from(bounds.max_y),
            )
        }
        Paint::Clip { clip, .. } => corners(clip.0, clip.1, clip.2, clip.3),
        Paint::Transform { matrix: inner, paint } => {
            reach(font, paint, compose_matrices(matrix, *inner))
        }
        Paint::Composite { source, backdrop, .. } => {
            union(reach(font, source, matrix), reach(font, backdrop, matrix))
        }
        Paint::Solid(_) | Paint::Linear { .. } | Paint::Radial { .. } | Paint::Sweep { .. } => None,
    }
}

fn union(
    one: Option<(f32, f32, f32, f32)>,
    other: Option<(f32, f32, f32, f32)>,
) -> Option<(f32, f32, f32, f32)> {
    match (one, other) {
        (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))),
        (a, b) => a.or(b),
    }
}

/// A point through a matrix of the format's kind.
fn apply(matrix: [f32; 6], x: f32, y: f32) -> (f32, f32) {
    let [xx, yx, xy, yy, dx, dy] = matrix;
    (xx * x + xy * y + dx, yx * x + yy * y + dy)
}

/// The matrix that does `inner` and then `outer`.
fn compose_matrices(outer: [f32; 6], inner: [f32; 6]) -> [f32; 6] {
    let [a, b, c, d, e, f] = outer;
    let [xx, yx, xy, yy, dx, dy] = inner;
    [
        a * xx + c * yx,
        b * xx + d * yx,
        a * xy + c * yy,
        b * xy + d * yy,
        a * dx + c * dy + e,
        b * dx + d * dy + f,
    ]
}

/// A matrix of the format's kind as the rasterizer's.
fn transform(matrix: [f32; 6]) -> Transform {
    let [xx, yx, xy, yy, dx, dy] = matrix;
    Transform { a: xx, b: yx, c: xy, d: yy, e: dx, f: dy }
}

/// The transform that undoes one, when one does.
fn inverse(transform: &Transform) -> Option<Transform> {
    let Transform { a, b, c, d, e, f } = *transform;
    let determinant = a * d - b * c;
    if determinant.abs() <= f32::EPSILON * 1e-3 {
        return None;
    }
    let (ia, ib, ic, id) = (d / determinant, -b / determinant, -c / determinant, a / determinant);
    Some(Transform { a: ia, b: ib, c: ic, d: id, e: -(ia * e + ic * f), f: -(ib * e + id * f) })
}

fn point((x, y): (f32, f32)) -> Point {
    Point::new(x, y)
}

fn rectangle((x0, y0, x1, y1): (f32, f32, f32, f32)) -> Path {
    Path::rectangle(x0, y0, x1 - x0, y1 - y0)
}

/// The format's composite modes, which are the W3C's, as the compositor's.
fn mode_of(mode: CompositeMode) -> Mode {
    match mode {
        CompositeMode::Clear => Mode::Clear,
        CompositeMode::Source => Mode::Source,
        CompositeMode::Destination => Mode::Destination,
        CompositeMode::SourceOver => Mode::SourceOver,
        CompositeMode::DestinationOver => Mode::DestinationOver,
        CompositeMode::SourceIn => Mode::SourceIn,
        CompositeMode::DestinationIn => Mode::DestinationIn,
        CompositeMode::SourceOut => Mode::SourceOut,
        CompositeMode::DestinationOut => Mode::DestinationOut,
        CompositeMode::SourceAtop => Mode::SourceAtop,
        CompositeMode::DestinationAtop => Mode::DestinationAtop,
        CompositeMode::Xor => Mode::Xor,
        CompositeMode::Plus => Mode::Plus,
        CompositeMode::Screen => Mode::Screen,
        CompositeMode::Overlay => Mode::Overlay,
        CompositeMode::Darken => Mode::Darken,
        CompositeMode::Lighten => Mode::Lighten,
        CompositeMode::ColorDodge => Mode::ColorDodge,
        CompositeMode::ColorBurn => Mode::ColorBurn,
        CompositeMode::HardLight => Mode::HardLight,
        CompositeMode::SoftLight => Mode::SoftLight,
        CompositeMode::Difference => Mode::Difference,
        CompositeMode::Exclusion => Mode::Exclusion,
        CompositeMode::Multiply => Mode::Multiply,
        CompositeMode::Hue => Mode::Hue,
        CompositeMode::Saturation => Mode::Saturation,
        CompositeMode::Color => Mode::Color,
        CompositeMode::Luminosity => Mode::Luminosity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transform_and_its_inverse_undo_each_other() {
        let place = Transform { a: 2.0, b: 0.5, c: -1.0, d: 3.0, e: 7.0, f: -4.0 };
        let back = inverse(&place).expect("an inverse");
        let there = place.apply(Point::new(3.0, 5.0));
        let again = back.apply(there);
        assert!((again.x - 3.0).abs() < 1e-4 && (again.y - 5.0).abs() < 1e-4);
    }

    #[test]
    fn linear_light_goes_there_and_back() {
        let colour = [0.25, 0.1, 0.4, 0.5];
        let again = from_linear(to_linear(colour));
        for channel in 0..4 {
            assert!((again[channel] - colour[channel]).abs() < 1e-4);
        }
        // Halfway between black and white in linear light is lighter than
        // halfway in the ordinary scale.
        let middle = from_linear([0.5, 0.5, 0.5, 1.0]);
        assert!(middle[0] > 0.7);
    }

    #[test]
    fn a_matrix_inside_another_is_done_first() {
        let moved = [1.0, 0.0, 0.0, 1.0, 10.0, 0.0];
        let doubled = [2.0, 0.0, 0.0, 2.0, 0.0, 0.0];
        // Moved, then doubled: (1, 0) → (11, 0) → (22, 0).
        assert_eq!(apply(compose_matrices(doubled, moved), 1.0, 0.0), (22.0, 0.0));
    }
}

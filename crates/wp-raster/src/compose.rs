//! Pictures built out of layers: the colours a gradient runs through, and the
//! ways one layer may be put onto another.
//!
//! # Why this is not the canvas
//!
//! The canvas draws one thing over another and nothing else, which is all a
//! page needs: text over paper, a picture over a shading. A colour glyph of
//! the newer kind needs more. It is a tree of paints — a gradient seen
//! through the outline of a glyph, that result multiplied onto another,
//! the whole kept only where a third is — and each step is a picture of its
//! own before it is put onto the next. So a picture here is a layer that can
//! be made, masked and combined, and only the finished one is drawn onto a
//! canvas.
//!
//! The colours are kept *premultiplied* — each channel already multiplied by
//! how opaque the pixel is — in floating point, because that is what every
//! formula for combining layers is written in: the Porter–Duff operators and
//! the blend modes of the W3C's Compositing and Blending, which are the ones
//! a font's `COLR` table and an SVG drawing both name.
//!
//! # Gradients
//!
//! Three shapes of gradient are here, because three are what fonts and
//! drawings ask for: along a line, between two circles, and round a point.
//! Each turns a place into a distance along the run of colours; the run then
//! says what colour that distance is, padded, repeated or reflected past its
//! ends.

use crate::path::{Path, Point};
use crate::raster::{Rasterizer, Rule};

/// A colour with each channel already multiplied by its opacity, all four
/// from nought to one.
pub type Premultiplied = [f32; 4];

/// How a layer is put onto what is under it.
///
/// The first thirteen are Porter and Duff's, and say which of the two
/// layers shows where; the rest are blend modes, which say what colour the
/// two make where both are. See the W3C's Compositing and Blending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Clear,
    Source,
    Destination,
    SourceOver,
    DestinationOver,
    SourceIn,
    DestinationIn,
    SourceOut,
    DestinationOut,
    SourceAtop,
    DestinationAtop,
    Xor,
    Plus,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Multiply,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// What a run of colours does past its ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Spread {
    /// The end colours carry on for ever.
    #[default]
    Pad,
    /// The run starts again.
    Repeat,
    /// The run comes back the way it went.
    Reflect,
}

/// A picture in layers' terms: premultiplied colours, row after row.
#[derive(Clone, Debug, PartialEq)]
pub struct Pixmap {
    width: usize,
    height: usize,
    pixels: Vec<Premultiplied>,
}

impl Pixmap {
    /// A picture of nothing: every pixel transparent.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, pixels: vec![[0.0; 4]; width * height] }
    }

    /// A picture every pixel of which is worked out by a rule, asked about
    /// the middle of each pixel.
    #[must_use]
    pub fn shaded(width: usize, height: usize, colour: impl Fn(f32, f32) -> Premultiplied) -> Self {
        let mut pixels = Vec::with_capacity(width * height);
        for row in 0..height {
            for column in 0..width {
                pixels.push(colour(column as f32 + 0.5, row as f32 + 0.5));
            }
        }
        Self { width, height, pixels }
    }

    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// One pixel, premultiplied.
    #[must_use]
    pub fn pixel(&self, x: usize, y: usize) -> Premultiplied {
        if x >= self.width || y >= self.height {
            return [0.0; 4];
        }
        self.pixels[y * self.width + x]
    }

    /// Keeps the picture only where a path is, as much of each pixel as the
    /// path covers: what seeing a paint through the outline of a glyph is.
    pub fn keep_inside(&mut self, path: &Path, rule: Rule) {
        let mut rasterizer = Rasterizer::new(self.width, self.height);
        rasterizer.fill(path);
        let mask = rasterizer.finish_by(rule);
        for row in 0..self.height {
            for column in 0..self.width {
                let covered = f32::from(mask.at(column, row)) / 255.0;
                let pixel = &mut self.pixels[row * self.width + column];
                for channel in pixel.iter_mut() {
                    *channel *= covered;
                }
            }
        }
    }

    /// Makes every pixel that much more transparent: what an SVG group's
    /// opacity does to what is inside it.
    pub fn fade(&mut self, opacity: f32) {
        let opacity = opacity.clamp(0.0, 1.0);
        for pixel in &mut self.pixels {
            for channel in pixel.iter_mut() {
                *channel *= opacity;
            }
        }
    }

    /// Puts another picture of the same size onto this one, the way a mode
    /// says: this is what is under, the other what goes on top.
    pub fn composite(&mut self, source: &Self, mode: Mode) {
        for (under, over) in self.pixels.iter_mut().zip(&source.pixels) {
            *under = combine(*over, *under, mode);
        }
    }

    /// The picture as bytes of red, green, blue and opacity, not
    /// premultiplied — which is what a canvas and a PNG take.
    #[must_use]
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 4);
        for [red, green, blue, alpha] in &self.pixels {
            let alpha = alpha.clamp(0.0, 1.0);
            let straight = |channel: f32| {
                if alpha <= 0.0 {
                    0
                } else {
                    ((channel / alpha).clamp(0.0, 1.0) * 255.0).round() as u8
                }
            };
            out.extend_from_slice(&[
                straight(*red),
                straight(*green),
                straight(*blue),
                (alpha * 255.0).round() as u8,
            ]);
        }
        out
    }

    /// Whether nothing at all is drawn.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.pixels.iter().all(|pixel| pixel[3] <= 0.0)
    }
}

/// A colour of eight-bit channels, not premultiplied, as premultiplied.
#[must_use]
pub fn premultiplied(red: u8, green: u8, blue: u8, alpha: u8) -> Premultiplied {
    let alpha = f32::from(alpha) / 255.0;
    [
        f32::from(red) / 255.0 * alpha,
        f32::from(green) / 255.0 * alpha,
        f32::from(blue) / 255.0 * alpha,
        alpha,
    ]
}

/// One pixel put onto another, both premultiplied.
#[must_use]
pub fn combine(source: Premultiplied, backdrop: Premultiplied, mode: Mode) -> Premultiplied {
    let (alpha_s, alpha_b) = (source[3], backdrop[3]);
    let porter_duff = |fa: f32, fb: f32| -> Premultiplied {
        let mut out = [0.0; 4];
        for channel in 0..4 {
            out[channel] = (source[channel] * fa + backdrop[channel] * fb).clamp(0.0, 1.0);
        }
        out
    };
    match mode {
        Mode::Clear => [0.0; 4],
        Mode::Source => source,
        Mode::Destination => backdrop,
        Mode::SourceOver => porter_duff(1.0, 1.0 - alpha_s),
        Mode::DestinationOver => porter_duff(1.0 - alpha_b, 1.0),
        Mode::SourceIn => porter_duff(alpha_b, 0.0),
        Mode::DestinationIn => porter_duff(0.0, alpha_s),
        Mode::SourceOut => porter_duff(1.0 - alpha_b, 0.0),
        Mode::DestinationOut => porter_duff(0.0, 1.0 - alpha_s),
        Mode::SourceAtop => porter_duff(alpha_b, 1.0 - alpha_s),
        Mode::DestinationAtop => porter_duff(1.0 - alpha_b, alpha_s),
        Mode::Xor => porter_duff(1.0 - alpha_b, 1.0 - alpha_s),
        Mode::Plus => porter_duff(1.0, 1.0),
        _ => blend(source, backdrop, mode),
    }
}

/// The blend modes: where both layers are, a colour made of the two; where
/// only one is, that one.
fn blend(source: Premultiplied, backdrop: Premultiplied, mode: Mode) -> Premultiplied {
    let (alpha_s, alpha_b) = (source[3], backdrop[3]);
    let straight = |pixel: Premultiplied| -> [f32; 3] {
        if pixel[3] <= 0.0 {
            [0.0; 3]
        } else {
            [pixel[0] / pixel[3], pixel[1] / pixel[3], pixel[2] / pixel[3]]
        }
    };
    let (cs, cb) = (straight(source), straight(backdrop));
    let mixed: [f32; 3] = match mode {
        Mode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        Mode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        Mode::Color => set_lum(cs, lum(cb)),
        Mode::Luminosity => set_lum(cb, lum(cs)),
        _ => [
            separable(cb[0], cs[0], mode),
            separable(cb[1], cs[1], mode),
            separable(cb[2], cs[2], mode),
        ],
    };
    let mut out = [0.0; 4];
    for channel in 0..3 {
        out[channel] = (source[channel] * (1.0 - alpha_b)
            + backdrop[channel] * (1.0 - alpha_s)
            + alpha_s * alpha_b * mixed[channel].clamp(0.0, 1.0))
        .clamp(0.0, 1.0);
    }
    out[3] = (alpha_s + alpha_b * (1.0 - alpha_s)).clamp(0.0, 1.0);
    out
}

/// A blend mode one channel at a time: the backdrop's value and the
/// source's.
fn separable(backdrop: f32, source: f32, mode: Mode) -> f32 {
    let multiply = |b: f32, s: f32| b * s;
    let screen = |b: f32, s: f32| b + s - b * s;
    let hard_light = |b: f32, s: f32| {
        if s <= 0.5 {
            multiply(b, 2.0 * s)
        } else {
            screen(b, 2.0 * s - 1.0)
        }
    };
    match mode {
        Mode::Multiply => multiply(backdrop, source),
        Mode::Screen => screen(backdrop, source),
        Mode::Overlay => hard_light(source, backdrop),
        Mode::Darken => backdrop.min(source),
        Mode::Lighten => backdrop.max(source),
        Mode::ColorDodge => {
            if backdrop <= 0.0 {
                0.0
            } else if source >= 1.0 {
                1.0
            } else {
                (backdrop / (1.0 - source)).min(1.0)
            }
        }
        Mode::ColorBurn => {
            if backdrop >= 1.0 {
                1.0
            } else if source <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - backdrop) / source).min(1.0)
            }
        }
        Mode::HardLight => hard_light(backdrop, source),
        Mode::SoftLight => {
            if source <= 0.5 {
                backdrop - (1.0 - 2.0 * source) * backdrop * (1.0 - backdrop)
            } else {
                let d = if backdrop <= 0.25 {
                    ((16.0 * backdrop - 12.0) * backdrop + 4.0) * backdrop
                } else {
                    backdrop.sqrt()
                };
                backdrop + (2.0 * source - 1.0) * (d - backdrop)
            }
        }
        Mode::Difference => (backdrop - source).abs(),
        Mode::Exclusion => backdrop + source - 2.0 * backdrop * source,
        // Everything else is not a blend mode and never reaches here.
        _ => source,
    }
}

/// How light a colour is, the way the non-separable modes measure it.
fn lum(colour: [f32; 3]) -> f32 {
    0.3 * colour[0] + 0.59 * colour[1] + 0.11 * colour[2]
}

/// A colour moved to a lightness, and brought back into range without
/// changing its hue.
fn set_lum(colour: [f32; 3], lightness: f32) -> [f32; 3] {
    let d = lightness - lum(colour);
    let moved = [colour[0] + d, colour[1] + d, colour[2] + d];
    let l = lum(moved);
    let low = moved[0].min(moved[1]).min(moved[2]);
    let high = moved[0].max(moved[1]).max(moved[2]);
    let mut out = moved;
    if low < 0.0 {
        for channel in &mut out {
            *channel = l + (*channel - l) * l / (l - low).max(f32::EPSILON);
        }
    }
    if high > 1.0 {
        for channel in &mut out {
            *channel = l + (*channel - l) * (1.0 - l) / (high - l).max(f32::EPSILON);
        }
    }
    out
}

/// How saturated a colour is: its highest channel less its lowest.
fn sat(colour: [f32; 3]) -> f32 {
    colour[0].max(colour[1]).max(colour[2]) - colour[0].min(colour[1]).min(colour[2])
}

/// A colour given a saturation, keeping which of its channels is highest.
fn set_sat(colour: [f32; 3], saturation: f32) -> [f32; 3] {
    let mut order = [0usize, 1, 2];
    order.sort_by(|one, other| colour[*one].total_cmp(&colour[*other]));
    let [min, mid, max] = order;
    let mut out = [0.0; 3];
    if colour[max] > colour[min] {
        out[mid] = (colour[mid] - colour[min]) * saturation / (colour[max] - colour[min]);
        out[max] = saturation;
    }
    out
}

// ---------------------------------------------------------------------------
// Gradients
// ---------------------------------------------------------------------------

/// The colour a run of colours has at a distance along it, with what it does
/// past its ends. The stops are distances and premultiplied colours, in
/// order; the colours between two stops are mixed in proportion.
#[must_use]
pub fn colour_along(stops: &[(f32, Premultiplied)], along: f32, spread: Spread) -> Premultiplied {
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else { return [0.0; 4] };
    if !along.is_finite() {
        return [0.0; 4];
    }
    let (start, end) = (first.0, last.0);
    let length = end - start;
    let along = if length <= f32::EPSILON {
        along
    } else {
        match spread {
            Spread::Pad => along,
            Spread::Repeat => start + (along - start).rem_euclid(length),
            Spread::Reflect => {
                let turns = (along - start).rem_euclid(2.0 * length);
                start + if turns > length { 2.0 * length - turns } else { turns }
            }
        }
    };
    if along <= start {
        return first.1;
    }
    if along >= end {
        return last.1;
    }
    for pair in stops.windows(2) {
        let ((from, before), (to, after)) = (pair[0], pair[1]);
        if along >= from && along <= to {
            let share = if to - from <= f32::EPSILON { 1.0 } else { (along - from) / (to - from) };
            let mut out = [0.0; 4];
            for channel in 0..4 {
                out[channel] = before[channel] + (after[channel] - before[channel]) * share;
            }
            return out;
        }
    }
    last.1
}

/// How far along a gradient running from one point to another a place is:
/// nought at the first, one at the second, and the same all along any line
/// square to the two.
#[must_use]
pub fn along_line(place: Point, from: Point, to: Point) -> Option<f32> {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = dx * dx + dy * dy;
    if length <= f32::EPSILON {
        return None;
    }
    Some(((place.x - from.x) * dx + (place.y - from.y) * dy) / length)
}

/// How far along a gradient between two circles a place is: the largest
/// distance at which the circle between the two passes through it, with a
/// radius more than nought. `None` where no such circle does, which is drawn
/// as nothing — and so is everything, when the two circles are one.
///
/// The canvas's `createRadialGradient` and a font's radial paint are this
/// same thing: the circle slides and grows from the first to the second,
/// and beyond, as the distance runs.
#[must_use]
pub fn between_circles(place: Point, first: (Point, f32), second: (Point, f32)) -> Option<f32> {
    let (centre, radius) = first;
    let (cd_x, cd_y) = (second.0.x - centre.x, second.0.y - centre.y);
    let (pd_x, pd_y) = (place.x - centre.x, place.y - centre.y);
    let dr = second.1 - radius;
    let a = cd_x * cd_x + cd_y * cd_y - dr * dr;
    let b = pd_x * cd_x + pd_y * cd_y + radius * dr;
    let c = pd_x * pd_x + pd_y * pd_y - radius * radius;
    // Only where the circle has a radius: the standard paints a circle of
    // radius nought nowhere.
    let usable = |t: f32| radius + t * dr > 0.0;
    if a.abs() <= f32::EPSILON {
        if b.abs() <= f32::EPSILON {
            return None;
        }
        let t = c / (2.0 * b);
        return usable(t).then_some(t);
    }
    let discriminant = b * b - a * c;
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let (one, other) = ((b + root) / a, (b - root) / a);
    let (larger, smaller) = if one >= other { (one, other) } else { (other, one) };
    if usable(larger) {
        Some(larger)
    } else if usable(smaller) {
        Some(smaller)
    } else {
        None
    }
}

/// How far round a gradient swept about a point a place is, with the angles
/// in degrees counted the way the coordinates run: nought at the first angle
/// and one at the second.
#[must_use]
pub fn round_point(place: Point, centre: Point, start: f32, end: f32) -> Option<f32> {
    let span = end - start;
    if span.abs() <= f32::EPSILON {
        return None;
    }
    let angle = (place.y - centre.y).atan2(place.x - centre.x).to_degrees().rem_euclid(360.0);
    Some((angle - start) / span)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Premultiplied = [1.0, 0.0, 0.0, 1.0];
    const BLUE: Premultiplied = [0.0, 0.0, 1.0, 1.0];
    const HALF_GREEN: Premultiplied = [0.0, 0.5, 0.0, 0.5];

    fn close(one: Premultiplied, other: Premultiplied) -> bool {
        one.iter().zip(other).all(|(a, b)| (a - b).abs() < 0.01)
    }

    #[test]
    fn the_porter_duff_operators_keep_what_they_say() {
        assert!(close(combine(RED, BLUE, Mode::SourceOver), RED));
        assert!(close(combine(RED, BLUE, Mode::DestinationOver), BLUE));
        assert!(close(combine(RED, [0.0; 4], Mode::SourceIn), [0.0; 4]));
        assert!(close(combine(RED, BLUE, Mode::SourceIn), RED));
        assert!(close(combine(RED, BLUE, Mode::SourceOut), [0.0; 4]));
        assert!(close(combine(RED, [0.0; 4], Mode::SourceOut), RED));
        assert!(close(combine(RED, BLUE, Mode::Xor), [0.0; 4]));
        assert!(close(combine(RED, BLUE, Mode::Clear), [0.0; 4]));
        // Half a green over red is half green and half red.
        assert!(close(combine(HALF_GREEN, RED, Mode::SourceOver), [0.5, 0.5, 0.0, 1.0]));
        assert!(close(combine(RED, BLUE, Mode::Plus), [1.0, 0.0, 1.0, 1.0]));
    }

    #[test]
    fn the_blend_modes_mix_where_both_are_and_keep_either_where_one_is() {
        let grey = [0.5, 0.5, 0.5, 1.0];
        assert!(close(combine(grey, grey, Mode::Multiply), [0.25, 0.25, 0.25, 1.0]));
        assert!(close(combine(grey, grey, Mode::Screen), [0.75, 0.75, 0.75, 1.0]));
        assert!(close(combine(RED, BLUE, Mode::Darken), [0.0, 0.0, 0.0, 1.0]));
        assert!(close(combine(RED, BLUE, Mode::Lighten), [1.0, 0.0, 1.0, 1.0]));
        assert!(close(combine(RED, BLUE, Mode::Difference), [1.0, 0.0, 1.0, 1.0]));
        // Over nothing, a blend mode is the source.
        assert!(close(combine(RED, [0.0; 4], Mode::Multiply), RED));
        // And nothing over something is the something.
        assert!(close(combine([0.0; 4], BLUE, Mode::Screen), BLUE));
        // The colour of red with the lightness of grey is a red as light as
        // the grey.
        let coloured = combine(RED, grey, Mode::Color);
        assert!((lum([coloured[0], coloured[1], coloured[2]]) - 0.5).abs() < 0.01);
        assert!(coloured[0] > coloured[1] && coloured[0] > coloured[2]);
    }

    #[test]
    fn a_run_of_colours_is_mixed_between_its_stops_and_spread_past_its_ends() {
        let stops = [(0.0, RED), (1.0, BLUE)];
        assert!(close(colour_along(&stops, 0.5, Spread::Pad), [0.5, 0.0, 0.5, 1.0]));
        assert!(close(colour_along(&stops, -3.0, Spread::Pad), RED));
        assert!(close(colour_along(&stops, 7.0, Spread::Pad), BLUE));
        assert!(close(colour_along(&stops, 1.25, Spread::Repeat), [0.75, 0.0, 0.25, 1.0]));
        assert!(close(colour_along(&stops, 1.25, Spread::Reflect), [0.25, 0.0, 0.75, 1.0]));
    }

    #[test]
    fn the_three_shapes_of_gradient_measure_distance_the_way_they_should() {
        let at = |x: f32, y: f32| Point::new(x, y);
        assert_eq!(along_line(at(5.0, 7.0), at(0.0, 0.0), at(10.0, 0.0)), Some(0.5));
        assert_eq!(along_line(at(5.0, 7.0), at(0.0, 0.0), at(0.0, 0.0)), None);
        // Circles about one centre: the distance is how far out a place is.
        let t = between_circles(at(5.0, 0.0), (at(0.0, 0.0), 0.0), (at(0.0, 0.0), 10.0));
        assert!((t.unwrap() - 0.5).abs() < 1e-5);
        // Round a point, counted from the first angle.
        let t = round_point(at(0.0, 1.0), at(0.0, 0.0), 0.0, 360.0).unwrap();
        assert!((t - 0.25).abs() < 1e-5);
    }

    #[test]
    fn a_picture_is_kept_only_where_its_outline_is() {
        let mut picture = Pixmap::shaded(10, 10, |_, _| RED);
        picture.keep_inside(&Path::rectangle(0.0, 0.0, 5.0, 10.0), Rule::Nonzero);
        assert!(close(picture.pixel(2, 5), RED));
        assert!(close(picture.pixel(7, 5), [0.0; 4]));
        let bytes = picture.to_rgba();
        assert_eq!(&bytes[..4], &[255, 0, 0, 255]);
        assert_eq!(&bytes[7 * 4..7 * 4 + 4], &[0, 0, 0, 0]);
    }
}

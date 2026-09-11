//! How one edge of a border is drawn.
//!
//! # Why a style is more than a name
//!
//! Because a dotted border and a solid one of the same width are the same
//! number of pixels of ink and a different thing entirely to look at. Word
//! lists twenty-five line styles and a person picks one by how it looks, so
//! drawing four of them properly and the rest as a plain line is a list where
//! most of the choices do nothing.
//!
//! # What they are made of
//!
//! Plain rectangles, because that is all a decoration is and because it is
//! enough for every one of them. A double line is two lines with a gap; a
//! dotted one is a row of squares; a dash-dot is a row of longer squares with
//! squares between them; a wave is a column of one-pixel marks whose height
//! follows a sine. The three-dimensional ones are two half-bands, one lighter
//! than the border's colour and one darker, which is what makes a flat line
//! look raised or sunken.
//!
//! # Why the side matters
//!
//! For most styles it does not: an edge is a run and a thickness, and whether
//! it goes across or down is the only difference. The bevelled ones are the
//! exception. A box that looks raised is lit from the top left, so its top and
//! left edges are the light ones and its bottom and right edges the dark ones —
//! and an edge that did not know which it was would light all four the same way
//! and look flat.

use wp_docx::model::Border;
use wp_raster::Color;

use crate::layout::{Decoration, Page};

/// Which edge of the box a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Top,
    Bottom,
    Start,
    End,
}

impl Side {
    /// Whether the edge runs across the page rather than down it.
    fn sideways(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }

    /// Whether it is one of the two the light falls on.
    ///
    /// The top and the left, because that is where the light comes from in
    /// every bevelled thing ever drawn on a screen.
    fn lit(self) -> bool {
        matches!(self, Self::Top | Self::Start)
    }

    /// Whether a shadow cast by the box falls beyond this edge.
    ///
    /// The bottom and the right, because Word's shadow falls down and to the
    /// right. The other two edges cast one into the box, where it cannot be
    /// seen.
    fn casts_shadow(self) -> bool {
        matches!(self, Self::Bottom | Self::End)
    }
}

/// How far a shadow falls, as a multiple of the line's thickness.
const SHADOW_REACH: f32 = 2.5;

/// How much of the light is kept in a shadow.
const SHADOW_DARKNESS: f32 = 0.45;

/// How far the two halves of a bevel are moved from the border's own colour.
const BEVEL_SHIFT: f32 = 0.45;

/// Draws one edge of a border, in whatever style it asks for.
///
/// `run` is how long the edge is; `thickness` is how thick, in pixels; `x` and
/// `y` are the corner it starts at, and the band is drawn centred on that line.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_edge(
    page: &mut Page,
    border: &Border,
    side: Side,
    x: f32,
    y: f32,
    run: f32,
    thickness: f32,
    colour: Color,
) {
    // The shadow first, under the line it belongs to.
    if border.shadow && side.casts_shadow() {
        let reach = thickness * SHADOW_REACH;
        draw_shadow(page, side, x, y, run, thickness, reach, darkened(colour, SHADOW_DARKNESS));
    }

    let mut ink = Ink { page, x, y, sideways: side.sideways(), colour };

    // A frame is drawn as a bevel whatever style the line names, because that
    // is what asking for one means: Word's 3-D setting keeps the style for the
    // list and draws the box standing off the page.
    if border.frame {
        bevel(&mut ink, run, thickness, side.lit());
        return;
    }

    match border.style.as_str() {
        // --- Lines, one behind another ------------------------------------
        // The numbers are the widths of line, gap, line, and so on across the
        // band, in whatever units make them add up: the band is shared out
        // among them.
        "double" => parallel(&mut ink, run, thickness, &[1.0, 1.0, 1.0]),
        "triple" => parallel(&mut ink, run, thickness, &[1.0, 1.0, 1.0, 1.0, 1.0]),
        "thinThickSmallGap" => parallel(&mut ink, run, thickness, &[1.0, 1.0, 3.0]),
        "thickThinSmallGap" => parallel(&mut ink, run, thickness, &[3.0, 1.0, 1.0]),
        "thinThickThinSmallGap" => {
            parallel(&mut ink, run, thickness, &[1.0, 1.0, 3.0, 1.0, 1.0]);
        }
        "thinThickMediumGap" => parallel(&mut ink, run, thickness, &[1.0, 2.0, 3.0]),
        "thickThinMediumGap" => parallel(&mut ink, run, thickness, &[3.0, 2.0, 1.0]),
        "thinThickThinMediumGap" => {
            parallel(&mut ink, run, thickness, &[1.0, 2.0, 3.0, 2.0, 1.0]);
        }
        "thinThickLargeGap" => parallel(&mut ink, run, thickness, &[1.0, 3.0, 3.0]),
        "thickThinLargeGap" => parallel(&mut ink, run, thickness, &[3.0, 3.0, 1.0]),
        "thinThickThinLargeGap" => {
            parallel(&mut ink, run, thickness, &[1.0, 3.0, 3.0, 3.0, 1.0]);
        }

        // --- Lines broken along their length ------------------------------
        // The numbers are mark and gap, as multiples of the thickness. A dot
        // is a mark as long as the line is thick, which is what a dot is at
        // any width.
        "dotted" => broken(&mut ink, run, thickness, &[(1.0, 1.0)]),
        "dashed" => broken(&mut ink, run, thickness, &[(4.0, 2.0)]),
        "dashSmallGap" => broken(&mut ink, run, thickness, &[(4.0, 1.0)]),
        "dotDash" => broken(&mut ink, run, thickness, &[(4.0, 2.0), (1.0, 2.0)]),
        "dotDotDash" => {
            broken(&mut ink, run, thickness, &[(4.0, 2.0), (1.0, 2.0), (1.0, 2.0)]);
        }
        // Word draws this one as a dash-dot made of short strokes rather than
        // of squares, which at these widths reads as a tighter dash-dot.
        "dashDotStroked" => broken(&mut ink, run, thickness, &[(3.0, 1.0), (1.0, 1.0)]),

        // --- Lines that are not straight ----------------------------------
        "wave" => wave(&mut ink, run, thickness, 1),
        "doubleWave" => wave(&mut ink, run, thickness, 2),

        // --- Lines drawn to look raised or sunken --------------------------
        "threeDEmboss" | "outset" => bevel(&mut ink, run, thickness, side.lit()),
        "threeDEngrave" | "inset" => bevel(&mut ink, run, thickness, !side.lit()),

        // Everything else is a line: `single`, `thick`, and any style a later
        // version of the format adds. Its width is what makes it heavy or
        // light, which is most of what the styles differ by.
        _ => ink.piece(0.0, run, -thickness / 2.0, thickness),
    }
}

/// Somewhere to put rectangles, with across and down decided once.
struct Ink<'a> {
    page: &'a mut Page,
    x: f32,
    y: f32,
    sideways: bool,
    colour: Color,
}

impl Ink<'_> {
    /// One rectangle of the edge: how far along it starts, how long it is, how
    /// far across the band it sits, and how thick it is there.
    fn piece(&mut self, along: f32, length: f32, offset: f32, weight: f32) {
        self.coloured(along, length, offset, weight, self.colour);
    }

    /// The same, in a colour of its own.
    fn coloured(&mut self, along: f32, length: f32, offset: f32, weight: f32, colour: Color) {
        if length <= 0.0 || weight <= 0.0 {
            return;
        }
        let (x, y, width, height) = if self.sideways {
            (self.x + along, self.y + offset, length, weight)
        } else {
            (self.x + offset, self.y + along, weight, length)
        };
        self.page.decorations.push(Decoration { x, y, width, height, color: colour });
    }
}

/// Lines side by side across the band, sharing it out by the weights given.
///
/// The odd entries are lines and the even ones gaps, starting and ending with a
/// line: `[1, 1, 1]` is a line, a gap and a line of the same width, which is
/// what a double border is.
fn parallel(ink: &mut Ink<'_>, run: f32, thickness: f32, weights: &[f32]) {
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return;
    }
    // Every line at least a pixel, or a fine double border comes out as
    // nothing at all.
    let unit = (thickness / total).max(0.34);
    let mut offset = -thickness / 2.0;
    for (at, weight) in weights.iter().enumerate() {
        let width = unit * weight;
        if at % 2 == 0 {
            ink.piece(0.0, run, offset, width.max(1.0));
        }
        offset += width;
    }
}

/// A line broken along its length, repeating a pattern of marks and gaps.
fn broken(ink: &mut Ink<'_>, run: f32, thickness: f32, pattern: &[(f32, f32)]) {
    let mut along = 0.0f32;
    let mut at = 0usize;
    // A pattern that adds up to nothing would go round for ever.
    let cycle: f32 = pattern.iter().map(|(mark, gap)| (mark + gap) * thickness).sum();
    if cycle <= 0.0 {
        return;
    }

    while along < run {
        let (mark, gap) = pattern[at % pattern.len()];
        let length = (mark * thickness).max(1.0);
        ink.piece(along, length.min(run - along), -thickness / 2.0, thickness);
        along += length + (gap * thickness).max(1.0);
        at += 1;
    }
}

/// A line that rises and falls as it goes.
///
/// Drawn as one-pixel marks whose place across the band follows a sine, which
/// is what a wave is and what makes it read as one at any width. `lines` is how
/// many waves there are: Word's double wave is two, one above the other.
fn wave(ink: &mut Ink<'_>, run: f32, thickness: f32, lines: usize) {
    // Word's wave is about six points from crest to crest at an ordinary line
    // width, and about as tall as the line is thick.
    let wavelength = (thickness * 6.0).max(6.0);
    let amplitude = thickness.max(1.0);
    let mark = thickness.max(1.0);

    let mut along = 0.0f32;
    while along < run {
        let phase = along / wavelength * std::f32::consts::TAU;
        let height = phase.sin() * amplitude;
        for line in 0..lines {
            // The second wave sits below the first by its whole height, so the
            // two are seen apart rather than as one thick one.
            let apart = line as f32 * amplitude * 2.0;
            ink.piece(along, 1.0f32.min(run - along), height + apart - amplitude, mark);
        }
        along += 1.0;
    }
}

/// A band drawn in two halves, one lighter than the border's colour and one
/// darker, so that the box looks raised.
///
/// `lit` says which half catches the light: the near half on the edges the
/// light falls on, the far half on the others. Swapping it is what turns a box
/// that stands up into one that is pressed in.
fn bevel(ink: &mut Ink<'_>, run: f32, thickness: f32, lit: bool) {
    let half = (thickness / 2.0).max(1.0);
    let (near, far) = if lit {
        (lightened(ink.colour, BEVEL_SHIFT), darkened(ink.colour, BEVEL_SHIFT))
    } else {
        (darkened(ink.colour, BEVEL_SHIFT), lightened(ink.colour, BEVEL_SHIFT))
    };
    ink.coloured(0.0, run, -thickness / 2.0, half, near);
    ink.coloured(0.0, run, -thickness / 2.0 + half, half, far);
}

/// The bar a box's shadow casts beyond one of its edges.
#[allow(clippy::too_many_arguments)]
fn draw_shadow(
    page: &mut Page,
    side: Side,
    x: f32,
    y: f32,
    run: f32,
    thickness: f32,
    reach: f32,
    colour: Color,
) {
    // Offset the way the light falls: down and to the right, and along the
    // edge as well, so the two bars meet at the corner rather than crossing.
    let (rect_x, rect_y, width, height) = match side {
        Side::Bottom => (x + reach, y + thickness / 2.0, run, reach),
        Side::End => (x + thickness / 2.0, y + reach, reach, run),
        // The other two cast their shadow into the box, where nothing sees it.
        Side::Top | Side::Start => return,
    };
    page.decorations.push(Decoration {
        x: rect_x,
        y: rect_y,
        width: width.max(0.0),
        height: height.max(0.0),
        color: colour,
    });
}

/// A colour moved towards white.
fn lightened(colour: Color, amount: f32) -> Color {
    let shift = |value: u8| value as f32 + (255.0 - value as f32) * amount;
    Color::rgba(
        shift(colour.red) as u8,
        shift(colour.green) as u8,
        shift(colour.blue) as u8,
        colour.alpha,
    )
}

/// And one moved towards black.
fn darkened(colour: Color, amount: f32) -> Color {
    let shift = |value: u8| value as f32 * (1.0 - amount);
    Color::rgba(
        shift(colour.red) as u8,
        shift(colour.green) as u8,
        shift(colour.blue) as u8,
        colour.alpha,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::Border;

    /// Draws one edge onto an empty page and gives back what it put there.
    fn drawn(style: &str, thickness: f32) -> Vec<Decoration> {
        let border = Border::line(style, 8, None);
        let mut page = Page::default();
        draw_edge(&mut page, &border, Side::Top, 0.0, 0.0, 100.0, thickness, Color::BLACK);
        page.decorations
    }

    #[test]
    fn a_plain_line_is_one_rectangle() {
        assert_eq!(drawn("single", 2.0).len(), 1);
    }

    #[test]
    fn each_of_words_layered_styles_draws_the_lines_it_names() {
        for (style, lines) in [
            ("double", 2),
            ("triple", 3),
            ("thinThickSmallGap", 2),
            ("thickThinSmallGap", 2),
            ("thinThickThinSmallGap", 3),
            ("thinThickMediumGap", 2),
            ("thickThinMediumGap", 2),
            ("thinThickThinMediumGap", 3),
            ("thinThickLargeGap", 2),
            ("thickThinLargeGap", 2),
            ("thinThickThinLargeGap", 3),
        ] {
            assert_eq!(drawn(style, 6.0).len(), lines, "{style}");
        }
    }

    #[test]
    fn a_broken_line_is_many_marks_with_gaps_between_them() {
        let dotted = drawn("dotted", 2.0);
        assert!(dotted.len() > 10, "a dotted line came out as {} marks", dotted.len());
        // Every mark is the same length, and shorter than the run.
        assert!(dotted.iter().all(|mark| mark.width < 100.0));

        // A dash is longer than a dot, so there are fewer of them.
        assert!(drawn("dashed", 2.0).len() < dotted.len());
    }

    #[test]
    fn a_dash_dot_alternates_long_marks_and_short_ones() {
        let marks = drawn("dotDash", 2.0);
        assert!(marks.len() > 4);
        assert!(marks[0].width > marks[1].width, "the dash is not longer than the dot");
        assert!((marks[0].width - marks[2].width).abs() < 0.01, "the pattern does not repeat");
    }

    #[test]
    fn a_wave_goes_up_and_down() {
        let marks = drawn("wave", 2.0);
        let highest = marks.iter().map(|mark| mark.y).fold(f32::MIN, f32::max);
        let lowest = marks.iter().map(|mark| mark.y).fold(f32::MAX, f32::min);
        assert!(highest - lowest > 1.0, "the wave is flat");
    }

    #[test]
    fn a_double_wave_is_two_of_them() {
        assert!(drawn("doubleWave", 2.0).len() > drawn("wave", 2.0).len());
    }

    #[test]
    fn a_bevel_is_two_halves_of_different_colours() {
        let marks = drawn("threeDEmboss", 4.0);
        assert_eq!(marks.len(), 2);
        assert_ne!(marks[0].color, marks[1].color, "both halves are the same colour");
    }

    #[test]
    fn emboss_and_engrave_are_each_other_the_other_way_up() {
        let out = drawn("outset", 4.0);
        let inset = drawn("inset", 4.0);
        assert_eq!(out[0].color, inset[1].color);
        assert_eq!(out[1].color, inset[0].color);
    }

    #[test]
    fn a_shadow_falls_beyond_the_edges_the_light_leaves() {
        let border = Border::line("single", 8, None).with_effect(true, false);
        for (side, casts) in
            [(Side::Top, false), (Side::Start, false), (Side::Bottom, true), (Side::End, true)]
        {
            let mut page = Page::default();
            draw_edge(&mut page, &border, side, 0.0, 0.0, 100.0, 2.0, Color::BLACK);
            // One rectangle for the line, and a second for the shadow where
            // there is one.
            assert_eq!(page.decorations.len(), if casts { 2 } else { 1 }, "{side:?}");
        }
    }

    #[test]
    fn a_frame_is_drawn_as_a_bevel_whatever_line_it_names() {
        let border = Border::line("dotted", 8, None).with_effect(false, true);
        let mut page = Page::default();
        draw_edge(&mut page, &border, Side::Top, 0.0, 0.0, 100.0, 4.0, Color::BLACK);
        assert_eq!(page.decorations.len(), 2, "a framed border is two halves, not a row of dots");
    }

    #[test]
    fn a_style_nobody_here_knows_is_still_a_line() {
        // What a later version of the format would bring.
        assert_eq!(drawn("somethingNew", 2.0).len(), 1);
    }
}

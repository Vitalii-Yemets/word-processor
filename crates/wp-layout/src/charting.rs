//! Drawing a chart: axes, bars, a line or a pie, and the words round them.
//!
//! # Why the chart is drawn here rather than stored as a picture
//!
//! Because the document stores numbers, not a picture — see
//! [`wp_docx::chart`]. Something has to turn the numbers into a chart, and in
//! Word that something is Word. Here it is this.
//!
//! # What it draws
//!
//! The plot area is what is left after the title, the labels along the bottom
//! and the numbers up the side have taken their room. Everything is measured
//! from the box the drawing was given, so a chart made bigger is drawn bigger
//! rather than scaled up from a small one.
//!
//! Colours come from the theme, so a chart in a blue document is blue: the
//! caller passes the accents and they are used in turn, which is what Word does
//! for a chart whose series are not coloured by hand.

use wp_docx::chart::{Chart, Kind};
use wp_raster::{Color, Path, Point};

use crate::layout::{Decoration, PositionedGlyph};

/// How much of the height the title takes, when there is one.
const TITLE_SHARE: f32 = 0.14;
/// How much of the height the names along the bottom take.
const LABEL_SHARE: f32 = 0.12;
/// And how much of the width the numbers up the side take.
const AXIS_SHARE: f32 = 0.14;
/// The room left round the whole thing.
const MARGIN: f32 = 6.0;
/// How much of a slot a bar fills, leaving the rest as the gap between bars.
const BAR_SHARE: f32 = 0.7;
/// How thick the line of a line chart is, and the axes.
const LINE: f32 = 2.0;
/// How many steps the value axis is marked in.
const STEPS: usize = 4;

/// A chart laid out: everything needed to draw it, in page coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChartDrawing {
    /// Bars, axes and gridlines, which are all rectangles.
    pub rules: Vec<Decoration>,
    /// Pie slices and the line of a line chart, which are not.
    pub paths: Vec<(Path, Color)>,
    pub glyphs: Vec<PositionedGlyph>,
}

impl ChartDrawing {
    /// The same drawing moved to where it is going on the page.
    #[must_use]
    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            rules: self
                .rules
                .iter()
                .map(|rule| Decoration { x: rule.x + dx, y: rule.y + dy, ..*rule })
                .collect(),
            paths: self
                .paths
                .iter()
                .map(|(path, color)| {
                    (path.transformed(&wp_raster::Transform::translate(dx, dy)), *color)
                })
                .collect(),
            glyphs: self
                .glyphs
                .iter()
                .map(|glyph| PositionedGlyph {
                    x: glyph.x + dx,
                    baseline: glyph.baseline + dy,
                    ..*glyph
                })
                .collect(),
        }
    }
}
/// What the chart drawing needs from the engine: words turned into glyphs.
pub trait ChartShaper {
    /// Places text with its left edge at `x` and its baseline at `baseline`,
    /// and says how wide it came out.
    fn shape_label(
        &mut self,
        text: &str,
        x: f32,
        baseline: f32,
        size: f32,
        color: Color,
    ) -> (Vec<PositionedGlyph>, f32);
}

/// How a chart is coloured.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    /// The colours the bars and slices are drawn in, used in turn.
    pub accents: Vec<Color>,
    /// The axes, the gridlines and the words.
    pub line: Color,
    pub text: Color,
}

/// Lays a chart out inside a box.
#[must_use]
pub fn draw(
    shaper: &mut dyn ChartShaper,
    chart: &Chart,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    palette: &Palette,
) -> ChartDrawing {
    let mut out = ChartDrawing::default();
    if chart.is_empty() || width <= 0.0 || height <= 0.0 {
        return out;
    }

    let size = (height * 0.07).clamp(7.0, 14.0);
    let mut top = y + MARGIN;
    let bottom = y + height - MARGIN;
    let left = x + MARGIN;
    let right = x + width - MARGIN;

    if !chart.title.is_empty() {
        let baseline = top + size;
        let (glyphs, text_width) =
            shaper.shape_label(&chart.title, 0.0, baseline, size * 1.2, palette.text);
        // Centred over the whole box, which is where a heading goes.
        let shift = ((right - left) - text_width) / 2.0;
        out.glyphs.extend(shifted(glyphs, left + shift.max(0.0)));
        top += height * TITLE_SHARE;
    }

    match chart.kind {
        Kind::Pie => pie(shaper, chart, left, top, right, bottom, size, palette, &mut out),
        Kind::Column => columns(shaper, chart, left, top, right, bottom, size, palette, &mut out),
        Kind::Bar => bars(shaper, chart, left, top, right, bottom, size, palette, &mut out),
        Kind::Line => line(shaper, chart, left, top, right, bottom, size, palette, &mut out),
    }
    out
}

/// Moves glyphs sideways, which is how a label is centred after measuring.
fn shifted(glyphs: Vec<PositionedGlyph>, dx: f32) -> Vec<PositionedGlyph> {
    glyphs.into_iter().map(|glyph| PositionedGlyph { x: glyph.x + dx, ..glyph }).collect()
}

/// The colour of one bar or slice.
fn accent(palette: &Palette, index: usize) -> Color {
    if palette.accents.is_empty() {
        return Color::rgb(0x44, 0x72, 0xC4);
    }
    palette.accents[index % palette.accents.len()]
}

/// A rectangle, as the drawing stores one.
fn rectangle(x: f32, y: f32, width: f32, height: f32, color: Color) -> Decoration {
    Decoration { x, y, width: width.max(0.0), height: height.max(0.0), color }
}

/// The axes and the gridlines of a chart that has them.
#[allow(clippy::too_many_arguments)]
fn frame(
    shaper: &mut dyn ChartShaper,
    largest: f64,
    plot_left: f32,
    plot_top: f32,
    plot_right: f32,
    plot_bottom: f32,
    size: f32,
    palette: &Palette,
    out: &mut ChartDrawing,
    upright: bool,
) {
    // The line along the bottom and the one up the side.
    out.rules.push(rectangle(plot_left, plot_bottom, plot_right - plot_left, LINE, palette.line));
    out.rules.push(rectangle(plot_left, plot_top, LINE, plot_bottom - plot_top, palette.line));

    if largest <= 0.0 {
        return;
    }
    let faint = Color::rgba(palette.line.red, palette.line.green, palette.line.blue, 60);

    for step in 1..=STEPS {
        let share = step as f32 / STEPS as f32;
        let value = largest * f64::from(share);
        let shown = number(value);

        if upright {
            let at = plot_bottom - (plot_bottom - plot_top) * share;
            out.rules.push(rectangle(plot_left, at, plot_right - plot_left, 1.0, faint));
            let (glyphs, width) =
                shaper.shape_label(&shown, 0.0, at + size / 3.0, size, palette.text);
            out.glyphs.extend(shifted(glyphs, plot_left - width - 4.0));
        } else {
            let at = plot_left + (plot_right - plot_left) * share;
            out.rules.push(rectangle(at, plot_top, 1.0, plot_bottom - plot_top, faint));
            let (glyphs, width) =
                shaper.shape_label(&shown, 0.0, plot_bottom + size + 2.0, size, palette.text);
            out.glyphs.extend(shifted(glyphs, at - width / 2.0));
        }
    }
}

/// A number as a chart writes it: no decimals unless it needs them.
fn number(value: f64) -> String {
    if (value - value.round()).abs() < 0.05 {
        return format!("{}", value.round() as i64);
    }
    format!("{value:.1}")
}

/// Upright columns.
#[allow(clippy::too_many_arguments)]
fn columns(
    shaper: &mut dyn ChartShaper,
    chart: &Chart,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    size: f32,
    palette: &Palette,
    out: &mut ChartDrawing,
) {
    let plot_left = left + (right - left) * AXIS_SHARE;
    let plot_bottom = bottom - (bottom - top) * LABEL_SHARE;
    let largest = chart.largest();
    frame(shaper, largest, plot_left, top, right, plot_bottom, size, palette, out, true);
    if largest <= 0.0 {
        return;
    }

    let slot = (right - plot_left) / chart.values.len() as f32;
    let bar = slot * BAR_SHARE;
    for (index, value) in chart.values.iter().enumerate() {
        let share = (*value / largest).clamp(0.0, 1.0) as f32;
        let tall = (plot_bottom - top) * share;
        let x = plot_left + slot * index as f32 + (slot - bar) / 2.0;
        out.rules.push(rectangle(x, plot_bottom - tall, bar, tall, accent(palette, index)));

        if let Some(name) = chart.categories.get(index).filter(|name| !name.is_empty()) {
            let (glyphs, width) =
                shaper.shape_label(name, 0.0, plot_bottom + size + 4.0, size, palette.text);
            out.glyphs.extend(shifted(glyphs, x + (bar - width) / 2.0));
        }
    }
}

/// Bars lying on their side.
#[allow(clippy::too_many_arguments)]
fn bars(
    shaper: &mut dyn ChartShaper,
    chart: &Chart,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    size: f32,
    palette: &Palette,
    out: &mut ChartDrawing,
) {
    let plot_left = left + (right - left) * AXIS_SHARE * 1.4;
    let plot_bottom = bottom - (bottom - top) * LABEL_SHARE;
    let largest = chart.largest();
    frame(shaper, largest, plot_left, top, right, plot_bottom, size, palette, out, false);
    if largest <= 0.0 {
        return;
    }

    let slot = (plot_bottom - top) / chart.values.len() as f32;
    let thick = slot * BAR_SHARE;
    for (index, value) in chart.values.iter().enumerate() {
        let share = (*value / largest).clamp(0.0, 1.0) as f32;
        let wide = (right - plot_left) * share;
        let y = top + slot * index as f32 + (slot - thick) / 2.0;
        out.rules.push(rectangle(plot_left, y, wide, thick, accent(palette, index)));

        if let Some(name) = chart.categories.get(index).filter(|name| !name.is_empty()) {
            let (glyphs, width) =
                shaper.shape_label(name, 0.0, y + thick / 2.0 + size / 3.0, size, palette.text);
            out.glyphs.extend(shifted(glyphs, (plot_left - width - 4.0).max(left)));
        }
    }
}

/// A line through the points.
#[allow(clippy::too_many_arguments)]
fn line(
    shaper: &mut dyn ChartShaper,
    chart: &Chart,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    size: f32,
    palette: &Palette,
    out: &mut ChartDrawing,
) {
    let plot_left = left + (right - left) * AXIS_SHARE;
    let plot_bottom = bottom - (bottom - top) * LABEL_SHARE;
    let largest = chart.largest();
    frame(shaper, largest, plot_left, top, right, plot_bottom, size, palette, out, true);
    if largest <= 0.0 {
        return;
    }

    // One point per value, spread across the plot with half a slot at each end
    // so the first and last are not on the axes.
    let slot = (right - plot_left) / chart.values.len().max(1) as f32;
    let point_at = |index: usize, value: f64| {
        let share = (value / largest).clamp(0.0, 1.0) as f32;
        (plot_left + slot * (index as f32 + 0.5), plot_bottom - (plot_bottom - top) * share)
    };

    let colour = accent(palette, 0);
    let mut previous: Option<(f32, f32)> = None;
    for (index, value) in chart.values.iter().enumerate() {
        let (x, y) = point_at(index, *value);
        if let Some((last_x, last_y)) = previous {
            out.paths.push((thick_line(last_x, last_y, x, y, LINE), colour));
        }
        // A dot at each point, so a single value is still visible.
        out.rules.push(rectangle(x - LINE, y - LINE, LINE * 2.0, LINE * 2.0, colour));
        previous = Some((x, y));

        if let Some(name) = chart.categories.get(index).filter(|name| !name.is_empty()) {
            let (glyphs, width) =
                shaper.shape_label(name, 0.0, plot_bottom + size + 4.0, size, palette.text);
            out.glyphs.extend(shifted(glyphs, x - width / 2.0));
        }
    }
}

/// A line of a given thickness, as a four-cornered shape.
fn thick_line(x1: f32, y1: f32, x2: f32, y2: f32, thickness: f32) -> Path {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let length = (dx * dx + dy * dy).sqrt().max(0.001);
    // At right angles to the line, half a thickness either side.
    let (nx, ny) = (-dy / length * thickness / 2.0, dx / length * thickness / 2.0);

    let mut path = Path::new();
    path.move_to(Point::new(x1 + nx, y1 + ny));
    path.line_to(Point::new(x2 + nx, y2 + ny));
    path.line_to(Point::new(x2 - nx, y2 - ny));
    path.line_to(Point::new(x1 - nx, y1 - ny));
    path.close();
    path
}

/// A pie, divided by how much of the total each value is.
#[allow(clippy::too_many_arguments)]
fn pie(
    shaper: &mut dyn ChartShaper,
    chart: &Chart,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    size: f32,
    palette: &Palette,
    out: &mut ChartDrawing,
) {
    let total = chart.total();
    if total <= 0.0 {
        return;
    }

    // The pie fills the shorter side, with room down the right for the names.
    let room = (right - left) * 0.62;
    let diameter = room.min(bottom - top);
    let radius = diameter / 2.0;
    let centre_x = left + radius;
    let centre_y = top + (bottom - top) / 2.0;

    // From the top, clockwise, which is where a pie starts.
    let mut angle = -core::f32::consts::FRAC_PI_2;
    for (index, value) in chart.values.iter().enumerate() {
        let share = (*value / total) as f32;
        let sweep = share * core::f32::consts::TAU;
        out.paths.push((slice(centre_x, centre_y, radius, angle, sweep), accent(palette, index)));
        angle += sweep;

        if let Some(name) = chart.categories.get(index).filter(|name| !name.is_empty()) {
            // The names down the right, each beside a square of its colour.
            let baseline = top + size * 1.6 * (index as f32 + 1.0);
            let key_x = left + room + 8.0;
            out.rules.push(rectangle(
                key_x,
                baseline - size * 0.7,
                size * 0.7,
                size * 0.7,
                accent(palette, index),
            ));
            let (glyphs, _) = shaper.shape_label(name, key_x + size, baseline, size, palette.text);
            out.glyphs.extend(glyphs);
        }
    }
}

/// One slice of a pie, as a shape.
fn slice(centre_x: f32, centre_y: f32, radius: f32, from: f32, sweep: f32) -> Path {
    let mut path = Path::new();
    path.move_to(Point::new(centre_x, centre_y));

    // Enough straight edges that the curve reads as a curve: one every few
    // degrees, which is finer than a page can show.
    let steps = ((sweep.abs() / 0.08).ceil() as usize).clamp(2, 240);
    for step in 0..=steps {
        let angle = from + sweep * step as f32 / steps as f32;
        path.line_to(Point::new(centre_x + radius * angle.cos(), centre_y + radius * angle.sin()));
    }
    path.close();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::chart::Chart;

    /// A shaper that gives every character the same square, which is enough to
    /// place a label without a font.
    struct Squares;

    impl ChartShaper for Squares {
        fn shape_label(
            &mut self,
            text: &str,
            x: f32,
            baseline: f32,
            size: f32,
            color: Color,
        ) -> (Vec<PositionedGlyph>, f32) {
            let mut glyphs = Vec::new();
            let mut at = x;
            for _ in text.chars() {
                glyphs.push(PositionedGlyph {
                    face: 0,
                    glyph: wp_font::GlyphId(0),
                    x: at,
                    baseline,
                    advance: size,
                    size,
                    color,
                    effect: None,
                    source: wp_docx::TextPosition::new(0, 0),
                    source_length: 0,
                    invisible: false,
                });
                at += size;
            }
            (glyphs, at - x)
        }
    }

    fn palette() -> Palette {
        Palette {
            accents: vec![Color::rgb(0x44, 0x72, 0xC4), Color::rgb(0xED, 0x7D, 0x31)],
            line: Color::BLACK,
            text: Color::BLACK,
        }
    }

    fn drawn(kind: Kind, typed: &str) -> ChartDrawing {
        let chart = Chart::parse(kind, "Sales", typed);
        draw(&mut Squares, &chart, 0.0, 0.0, 400.0, 300.0, &palette())
    }

    #[test]
    fn a_chart_with_no_numbers_draws_nothing() {
        let drawing = drawn(Kind::Column, "");
        assert!(drawing.rules.is_empty() && drawing.paths.is_empty() && drawing.glyphs.is_empty());
    }

    #[test]
    fn a_column_chart_has_a_bar_for_every_number() {
        let drawing = drawn(Kind::Column, "a=1; b=2; c=3");
        // Two axes, four gridlines and three bars.
        let coloured = drawing
            .rules
            .iter()
            .filter(|rule| {
                rule.color == Color::rgb(0x44, 0x72, 0xC4)
                    || rule.color == Color::rgb(0xED, 0x7D, 0x31)
            })
            .count();
        assert_eq!(coloured, 3, "{:?}", drawing.rules);
    }

    #[test]
    fn a_bigger_number_makes_a_taller_column() {
        let drawing = drawn(Kind::Column, "a=1; b=3");
        let bars: Vec<&Decoration> = drawing
            .rules
            .iter()
            .filter(|rule| rule.color != Color::BLACK && rule.color.alpha == 255)
            .collect();
        assert_eq!(bars.len(), 2);
        assert!(bars[1].height > bars[0].height, "{bars:?}");
    }

    #[test]
    fn a_bar_chart_lies_on_its_side() {
        let upright = drawn(Kind::Column, "a=1; b=3");
        let sideways = drawn(Kind::Bar, "a=1; b=3");
        let widest = |drawing: &ChartDrawing| {
            drawing
                .rules
                .iter()
                .filter(|rule| rule.color != Color::BLACK && rule.color.alpha == 255)
                .map(|rule| rule.width)
                .fold(0.0_f32, f32::max)
        };
        assert!(widest(&sideways) > widest(&upright), "the bars did not lie down");
    }

    #[test]
    fn a_line_chart_joins_its_points_up() {
        let drawing = drawn(Kind::Line, "a=1; b=2; c=3");
        // Two joins between three points.
        assert_eq!(drawing.paths.len(), 2, "{:?}", drawing.paths.len());
    }

    #[test]
    fn one_point_needs_no_line_between_anything() {
        let drawing = drawn(Kind::Line, "a=1");
        assert!(drawing.paths.is_empty());
        // But the point itself is still drawn.
        assert!(!drawing.rules.is_empty());
    }

    #[test]
    fn a_pie_has_a_slice_for_every_number_and_no_axes() {
        let drawing = drawn(Kind::Pie, "a=1; b=1; c=2");
        assert_eq!(drawing.paths.len(), 3);
        // The only rectangles are the squares beside the names.
        assert_eq!(drawing.rules.len(), 3, "{:?}", drawing.rules);
    }

    #[test]
    fn a_pie_of_nothing_draws_nothing() {
        let drawing = drawn(Kind::Pie, "a=0; b=0");
        assert!(drawing.paths.is_empty());
    }

    #[test]
    fn the_title_is_drawn_over_the_chart() {
        let with = drawn(Kind::Column, "a=1");
        let without = draw(
            &mut Squares,
            &Chart::parse(Kind::Column, "", "a=1"),
            0.0,
            0.0,
            400.0,
            300.0,
            &palette(),
        );
        assert!(with.glyphs.len() > without.glyphs.len(), "the title was not drawn");
    }

    #[test]
    fn a_chart_stays_inside_the_box_it_was_given() {
        let drawing = drawn(Kind::Column, "a=1; b=2; c=3");
        for rule in &drawing.rules {
            assert!(rule.x >= -1.0 && rule.x + rule.width <= 401.0, "{rule:?}");
            assert!(rule.y >= -1.0 && rule.y + rule.height <= 301.0, "{rule:?}");
        }
    }

    #[test]
    fn a_number_is_written_the_way_a_chart_writes_one() {
        assert_eq!(number(3.0), "3");
        assert_eq!(number(2.5), "2.5");
        assert_eq!(number(0.0), "0");
    }
}

//! Drawing a chart: the plot, the axes, the key, the table and the words.
//!
//! # Why the chart is drawn here rather than stored as a picture
//!
//! Because the document stores numbers, not a picture — see
//! [`wp_docx::chart`]. Something has to turn the numbers into a chart, and in
//! Word that something is Word. Here it is this.
//!
//! # What it draws
//!
//! The box the drawing was given is shared out from the outside in: the
//! title takes the top, the key takes the side it asks for, the table of
//! numbers takes the bottom, and the plot is what is left, with its axes and
//! the words along them inside that. Everything is measured from the box, so
//! a chart made bigger is drawn bigger rather than scaled up from a small one.
//!
//! Each kind of plot is in [`plots`]: columns and bars side by side, stacked
//! or as shares; lines and areas; points and bubbles against two value axes;
//! a radar; a surface seen from above; and a pie or a doughnut. What they
//! share is here: the scale of a value axis, the axes and gridlines, the
//! words on a point, the key, the table.
//!
//! Colours come from the theme, so a chart in a blue document is blue: the
//! caller passes the accents and they are used in turn, which is what Word does
//! for a chart whose series are not coloured by hand. A series or a point the
//! document coloured itself keeps its colour.

use wp_docx::chart::{Axis, Chart, Kind, LegendPosition, Series};
use wp_raster::{Color, Path, Point};

use crate::layout::{Decoration, PositionedGlyph};

mod plots;

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
/// How many steps the value axis is marked in, when the file does not say.
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

/// A rectangle something is drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Frame {
    fn width(self) -> f32 {
        self.right - self.left
    }

    fn height(self) -> f32 {
        self.bottom - self.top
    }
}

/// Everything the drawing of a plot needs to reach: the words, the
/// colours, the size of the writing, and where the drawing goes.
pub(crate) struct Canvas<'a> {
    shaper: &'a mut dyn ChartShaper,
    chart: &'a Chart,
    palette: &'a Palette,
    /// The size of the writing.
    size: f32,
    out: ChartDrawing,
}

impl Canvas<'_> {
    /// Words placed with their left edge at `x`.
    fn words(&mut self, text: &str, x: f32, baseline: f32, size: f32) -> f32 {
        let (glyphs, width) = self.shaper.shape_label(text, x, baseline, size, self.palette.text);
        self.out.glyphs.extend(glyphs);
        width
    }

    /// Words placed with their middle at `x`.
    fn centred(&mut self, text: &str, middle: f32, baseline: f32, size: f32) -> f32 {
        let width = self.measure(text, size);
        self.words(text, middle - width / 2.0, baseline, size)
    }

    /// Words placed with their right edge at `x`.
    fn right_aligned(&mut self, text: &str, right: f32, baseline: f32, size: f32) -> f32 {
        let width = self.measure(text, size);
        self.words(text, right - width, baseline, size)
    }

    /// How wide words would come out.
    fn measure(&mut self, text: &str, size: f32) -> f32 {
        self.shaper.shape_label(text, 0.0, -1000.0, size, self.palette.text).1
    }

    fn rule(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        self.out.rules.push(rectangle(x, y, width, height, color));
    }

    fn path(&mut self, path: Path, color: Color) {
        self.out.paths.push((path, color));
    }

    /// The colour of a series: the one the document chose, or the next of
    /// the theme's.
    fn series_colour(&self, which: usize, series: &Series) -> Color {
        series
            .fill
            .as_deref()
            .and_then(Color::from_hex)
            .unwrap_or_else(|| accent(self.palette, which))
    }

    /// The colour of one point of a series: its own, when the document
    /// chose one; the next of the theme's, when every point is its own
    /// colour; the series' otherwise.
    fn point_colour(&self, which: usize, series: &Series, point: usize) -> Color {
        if let Some((_, fill)) = series.points.iter().find(|(index, _)| *index == point) {
            if let Some(colour) = Color::from_hex(fill) {
                return colour;
            }
        }
        if self.chart.vary_colors {
            return accent(self.palette, point);
        }
        self.series_colour(which, series)
    }

    /// The faint line a gridline is drawn in.
    fn faint(&self) -> Color {
        let line = self.palette.line;
        Color::rgba(line.red, line.green, line.blue, 60)
    }

    /// The words written on one point: what the chart asks for, joined the
    /// way Word joins them.
    fn label_text(&self, series: &Series, point: usize, value: f64, share: f64) -> String {
        let labels = self.chart.labels_of(series);
        let mut pieces = Vec::new();
        if labels.series {
            pieces.push(series.name.clone());
        }
        if labels.category {
            if let Some(name) = self.chart.categories.get(point).filter(|name| !name.is_empty()) {
                pieces.push(name.clone());
            }
        }
        if labels.value {
            pieces.push(formatted(labels.number_format.as_deref(), value));
        }
        if labels.percent {
            pieces.push(wp_docx::numberformat::format("0%", share));
        }
        pieces.join(", ")
    }

    /// Writes the words on a point, centred on a place.
    fn label(&mut self, series: &Series, point: usize, value: f64, share: f64, at: (f32, f32)) {
        if !self.chart.labels_of(series).shows_anything() {
            return;
        }
        let text = self.label_text(series, point, value, share);
        if text.is_empty() {
            return;
        }
        let (middle_x, baseline) = at;
        self.centred(&text, middle_x, baseline, self.size * 0.85);
    }
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
    if chart.is_empty() || width <= 0.0 || height <= 0.0 {
        return ChartDrawing::default();
    }

    let size = (height * 0.07).clamp(7.0, 14.0);
    let mut canvas = Canvas { shaper, chart, palette, size, out: ChartDrawing::default() };
    let mut frame = Frame {
        left: x + MARGIN,
        top: y + MARGIN,
        right: x + width - MARGIN,
        bottom: y + height - MARGIN,
    };

    if !chart.title.is_empty() {
        let baseline = frame.top + size;
        // Centred over the whole box, which is where a heading goes.
        let middle = (frame.left + frame.right) / 2.0;
        canvas.centred(&chart.title, middle, baseline, size * 1.2);
        frame.top += height * TITLE_SHARE;
    }

    // The key takes room from the plot at the side it asks for, so it is
    // measured before anything is drawn and drawn after: a plot laid out
    // over the key would have its bottom row of numbers behind it.
    let entries = key_entries(&canvas);
    let key = chart.legend.filter(|_| !entries.is_empty()).map(|legend| {
        let room = key_room(&mut canvas, &entries);
        let place = frame;
        let drawn = match legend.position {
            LegendPosition::Right | LegendPosition::TopRight => {
                let at = Frame { left: place.right - room.0, ..place };
                if !legend.overlay {
                    frame.right -= room.0 + size;
                }
                at
            }
            LegendPosition::Left => {
                let at = Frame { right: place.left + room.0, ..place };
                if !legend.overlay {
                    frame.left += room.0 + size;
                }
                at
            }
            LegendPosition::Top => {
                let at = Frame { bottom: place.top + room.1, ..place };
                if !legend.overlay {
                    frame.top += room.1 + size * 0.5;
                }
                at
            }
            LegendPosition::Bottom => {
                let at = Frame { top: place.bottom - room.1, ..place };
                if !legend.overlay {
                    frame.bottom -= room.1 + size * 0.5;
                }
                at
            }
        };
        (legend.position, drawn)
    });

    // The table of the numbers takes rows under the plot: one for the
    // categories and one per series, and its rows have to line up with the
    // slots of the plot, so the plot is told how much to leave.
    let table_rows = chart
        .data_table
        .filter(|_| chart.kind.has_axes() && !chart.kind.plots_points())
        .map(|_| chart.series.len() + 1)
        .unwrap_or(0);
    let row_height = size * 1.5;
    let table_height = table_rows as f32 * row_height;
    let table_labels = if table_rows > 0 {
        let widest = chart
            .series
            .iter()
            .map(|series| canvas.measure(&series.name, size))
            .fold(0.0, f32::max);
        widest + size * 1.8
    } else {
        0.0
    };

    let plot = Frame { bottom: frame.bottom - table_height, ..frame };
    let slots = match chart.kind {
        Kind::Pie | Kind::Doughnut => plots::round(&mut canvas, plot),
        Kind::Column => plots::columns(&mut canvas, plot, table_labels),
        Kind::Bar => plots::bars(&mut canvas, plot),
        Kind::Line | Kind::Area => plots::lines(&mut canvas, plot, table_labels),
        Kind::Scatter | Kind::Bubble => plots::points(&mut canvas, plot),
        Kind::Radar => plots::radar(&mut canvas, plot),
        Kind::Surface => plots::surface(&mut canvas, plot, table_labels),
    };

    if table_rows > 0 {
        if let Some(slots) = slots {
            let top = frame.bottom - table_height;
            data_table(&mut canvas, &slots, top, row_height, table_labels);
        }
    }

    if let Some((position, at)) = key {
        key_draw(&mut canvas, &entries, position, at);
    }
    canvas.out
}

/// One entry of the key: what it is called and what colour stands for it.
struct Entry {
    name: String,
    colour: Color,
}

/// What the key names: the series, or for a chart of slices the slices,
/// which are the categories.
fn key_entries(canvas: &Canvas<'_>) -> Vec<Entry> {
    let chart = canvas.chart;
    if chart.kind == Kind::Surface {
        // A surface's key names its bands of height.
        return plots::bands(canvas)
            .into_iter()
            .map(|(name, colour)| Entry { name, colour })
            .collect();
    }
    if chart.kind.is_round() || (chart.vary_colors && chart.series.len() == 1) {
        let Some(series) = chart.series.first() else { return Vec::new() };
        return chart
            .categories
            .iter()
            .enumerate()
            .filter(|(_, name)| !name.is_empty())
            .map(|(index, name)| Entry {
                name: name.clone(),
                colour: canvas.point_colour(0, series, index),
            })
            .collect();
    }
    chart
        .series
        .iter()
        .enumerate()
        .map(|(which, series)| Entry {
            name: series.name.clone(),
            colour: canvas.series_colour(which, series),
        })
        .collect()
}

/// How much room the key needs: as a column, and as a row.
fn key_room(canvas: &mut Canvas<'_>, entries: &[Entry]) -> (f32, f32) {
    let size = canvas.size;
    let widest = entries.iter().map(|entry| canvas.measure(&entry.name, size)).fold(0.0, f32::max);
    (widest + size * 1.6, size * 1.6)
}

/// The key: a square of each entry's colour with its name beside it, down a
/// column at the side or along a row above or below.
fn key_draw(canvas: &mut Canvas<'_>, entries: &[Entry], position: LegendPosition, at: Frame) {
    let size = canvas.size;
    let step = size * 1.6;
    match position {
        LegendPosition::Right | LegendPosition::Left | LegendPosition::TopRight => {
            // Down the side, centred on the plot — or from the top, for a
            // key that asked for the corner.
            let total = entries.len() as f32 * step;
            let mut baseline = if position == LegendPosition::TopRight {
                at.top + size
            } else {
                at.top + (at.height() - total).max(0.0) / 2.0 + size
            };
            for entry in entries {
                canvas.rule(at.left, baseline - size * 0.7, size * 0.7, size * 0.7, entry.colour);
                canvas.words(&entry.name, at.left + size, baseline, size);
                baseline += step;
            }
        }
        LegendPosition::Top | LegendPosition::Bottom => {
            // Along a row, centred: a key that started at the left edge would
            // sit under one end of the plot rather than under the plot.
            let widths: Vec<f32> =
                entries.iter().map(|entry| canvas.measure(&entry.name, size)).collect();
            let total: f32 = widths.iter().map(|width| width + size * 2.4).sum();
            let baseline = at.top + size * 1.2;
            let mut x = at.left + (at.width() - total).max(0.0) / 2.0;
            for (entry, width) in entries.iter().zip(widths) {
                canvas.rule(x, baseline - size * 0.7, size * 0.7, size * 0.7, entry.colour);
                canvas.words(&entry.name, x + size, baseline, size);
                x += width + size * 2.4;
            }
        }
    }
}

/// Where the categories of a plot stand across it, for the table of numbers
/// to line its columns up with: the left edge of the row labels, the edges
/// of the slots, and which way up the values are read.
pub(crate) struct Slots {
    left: f32,
    edges: Vec<f32>,
}

/// The table of the numbers under the plot: the categories along the top
/// row, and a row per series with its name and its numbers under each
/// category.
fn data_table(canvas: &mut Canvas<'_>, slots: &Slots, top: f32, row_height: f32, labels: f32) {
    let chart = canvas.chart;
    let Some(table) = chart.data_table else { return };
    let size = canvas.size;
    let line = canvas.palette.line;
    let faint = canvas.faint();
    let left = slots.left - labels;
    let right = slots.edges.last().copied().unwrap_or(slots.left);
    let rows = chart.series.len() + 1;
    let bottom = top + rows as f32 * row_height;
    let format = chart.value_axis.number_format.clone();

    // The categories along the first row, then a row per series.
    for (index, name) in chart.categories.iter().enumerate() {
        let (Some(from), Some(to)) = (slots.edges.get(index), slots.edges.get(index + 1)) else {
            continue;
        };
        canvas.centred(name, (from + to) / 2.0, top + size * 1.1, size * 0.85);
    }
    for (which, series) in chart.series.iter().enumerate() {
        let row_top = top + (which + 1) as f32 * row_height;
        let baseline = row_top + size * 1.1;
        let mut name_x = left + size * 0.4;
        if table.keys {
            let colour = canvas.series_colour(which, series);
            canvas.rule(name_x, baseline - size * 0.7, size * 0.7, size * 0.7, colour);
            name_x += size;
        }
        canvas.words(&series.name, name_x, baseline, size * 0.85);
        for (index, value) in series.values.iter().enumerate() {
            let (Some(from), Some(to)) = (slots.edges.get(index), slots.edges.get(index + 1))
            else {
                continue;
            };
            let text = formatted(format.as_deref(), *value);
            canvas.centred(&text, (from + to) / 2.0, baseline, size * 0.85);
        }
    }

    if table.horizontal_lines {
        for row in 1..rows {
            let y = top + row as f32 * row_height;
            canvas.rule(left, y, right - left, 1.0, faint);
        }
    }
    if table.vertical_lines {
        for edge in &slots.edges {
            canvas.rule(*edge, top, 1.0, bottom - top, faint);
        }
    }
    if table.outline {
        canvas.rule(left, top, right - left, 1.0, line);
        canvas.rule(left, bottom - 1.0, right - left, 1.0, line);
        canvas.rule(left, top, 1.0, bottom - top, line);
        canvas.rule(right - 1.0, top, 1.0, bottom - top, line);
    }
}

/// The scale of a value axis: where it starts and ends, and how far apart
/// its marks are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Scale {
    min: f64,
    max: f64,
    step: f64,
}

impl Scale {
    /// A scale that reaches from the smallest number to the largest, at
    /// marks a person would choose — unless the file states the ends or the
    /// step, which are then what it says.
    fn spanning(axis: &Axis, smallest: f64, largest: f64) -> Self {
        // A scale that starts at nought unless a number is below it, which
        // is where a column is measured from.
        let mut low = smallest.min(0.0);
        let mut high = largest.max(0.0);
        if high - low <= 0.0 {
            high = low + 1.0;
        }
        let step = axis
            .major_unit
            .filter(|unit| *unit > 0.0)
            .unwrap_or_else(|| nice((high - low) / STEPS as f64));
        low = axis.min.unwrap_or_else(|| (low / step).floor() * step);
        high = axis.max.unwrap_or_else(|| (high / step).ceil() * step);
        if high <= low {
            high = low + step;
        }
        Self { min: low, max: high, step }
    }

    /// The scale of shares: nought to one whole.
    fn shares() -> Self {
        Self { min: 0.0, max: 1.0, step: 0.2 }
    }

    /// How far along the axis a value is, from nought at the start to one at
    /// the end.
    fn along(self, value: f64) -> f32 {
        (((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)) as f32
    }

    /// The values marked along the axis.
    fn marks(self) -> Vec<f64> {
        let mut marks = Vec::new();
        let mut value = self.min;
        // Counted, so a step the file states as tiny is not a million marks.
        while value <= self.max + self.step * 0.001 && marks.len() < 50 {
            marks.push(value);
            value += self.step;
        }
        marks
    }
}

/// A step a person would choose: one, two or five times a power of ten.
fn nice(rough: f64) -> f64 {
    if rough <= 0.0 || !rough.is_finite() {
        return 1.0;
    }
    let power = 10f64.powf(rough.log10().floor());
    let fraction = rough / power;
    let chosen = if fraction <= 1.0 {
        1.0
    } else if fraction <= 2.0 {
        2.0
    } else if fraction <= 5.0 {
        5.0
    } else {
        10.0
    };
    chosen * power
}

/// A number as an axis or a label writes it: the way the file asks, or with
/// no decimals unless it needs them.
fn formatted(code: Option<&str>, value: f64) -> String {
    match code {
        Some(code) if !code.trim().is_empty() && !code.eq_ignore_ascii_case("General") => {
            wp_docx::numberformat::format(code, value)
        }
        _ => number(value),
    }
}

/// A number as a chart writes it: no decimals unless it needs them.
fn number(value: f64) -> String {
    if (value - value.round()).abs() < 0.05 {
        return format!("{}", value.round() as i64);
    }
    format!("{value:.1}")
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

/// A shape through some corners.
fn polygon(corners: &[(f32, f32)]) -> Path {
    let mut path = Path::new();
    for (index, (x, y)) in corners.iter().enumerate() {
        if index == 0 {
            path.move_to(Point::new(*x, *y));
        } else {
            path.line_to(Point::new(*x, *y));
        }
    }
    path.close();
    path
}

/// A circle, as a shape.
fn circle(centre_x: f32, centre_y: f32, radius: f32) -> Path {
    let mut path = Path::new();
    crate::geometry::ellipse(&mut path, centre_x, centre_y, radius, radius);
    path
}

/// The same colour, see-through.
fn translucent(colour: Color, alpha: u8) -> Color {
    Color::rgba(colour.red, colour.green, colour.blue, alpha)
}

/// The colour words are drawn in against a fill: white on a dark one and
/// the text colour on a light one.
fn over(fill: Color, text: Color) -> Color {
    let brightness =
        (u32::from(fill.red) * 299 + u32::from(fill.green) * 587 + u32::from(fill.blue) * 114)
            / 1000;
    if brightness < 140 {
        Color::WHITE
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::chart::{Chart, DataTable, Grouping, Labels, Legend, Series};

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
                    stretch: 1.0,
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

    const BLUE: Color = Color::rgb(0x44, 0x72, 0xC4);
    const ORANGE: Color = Color::rgb(0xED, 0x7D, 0x31);

    fn palette() -> Palette {
        Palette { accents: vec![BLUE, ORANGE], line: Color::BLACK, text: Color::BLACK }
    }

    fn draw_chart(chart: &Chart) -> ChartDrawing {
        draw(&mut Squares, chart, 0.0, 0.0, 400.0, 300.0, &palette())
    }

    fn drawn(kind: Kind, typed: &str) -> ChartDrawing {
        draw_chart(&Chart::parse(kind, "Sales", typed))
    }

    /// The rectangles drawn in a series colour: the bars and the markers.
    fn coloured(drawing: &ChartDrawing) -> Vec<&Decoration> {
        drawing.rules.iter().filter(|rule| rule.color == BLUE || rule.color == ORANGE).collect()
    }

    fn two_series(kind: Kind) -> Chart {
        Chart {
            kind,
            categories: vec!["North".to_owned(), "South".to_owned(), "East".to_owned()],
            series: vec![
                Series {
                    name: "Last".to_owned(),
                    values: vec![3.0, 5.0, 4.0],
                    ..Series::default()
                },
                Series {
                    name: "This".to_owned(),
                    values: vec![4.0, 2.0, 6.0],
                    ..Series::default()
                },
            ],
            ..Chart::default()
        }
    }

    #[test]
    fn a_chart_with_no_numbers_draws_nothing() {
        let drawing = drawn(Kind::Column, "");
        assert!(drawing.rules.is_empty() && drawing.paths.is_empty() && drawing.glyphs.is_empty());
    }

    #[test]
    fn a_column_chart_has_a_bar_for_every_number() {
        let drawing = drawn(Kind::Column, "a=1; b=2; c=3");
        assert_eq!(coloured(&drawing).len(), 3, "{:?}", drawing.rules);
    }

    #[test]
    fn a_bigger_number_makes_a_taller_column() {
        let drawing = drawn(Kind::Column, "a=1; b=3");
        let bars = coloured(&drawing);
        assert_eq!(bars.len(), 2);
        assert!(bars[1].height > bars[0].height, "{bars:?}");
    }

    #[test]
    fn a_bar_chart_lies_on_its_side() {
        let upright = drawn(Kind::Column, "a=1; b=3");
        let sideways = drawn(Kind::Bar, "a=1; b=3");
        let widest = |drawing: &ChartDrawing| {
            coloured(drawing).iter().map(|rule| rule.width).fold(0.0_f32, f32::max)
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
        let without = draw_chart(&Chart::parse(Kind::Column, "", "a=1"));
        assert!(with.glyphs.len() > without.glyphs.len(), "the title was not drawn");
    }

    #[test]
    fn every_kind_stays_inside_the_box_it_was_given() {
        for kind in Kind::ALL {
            let mut chart = two_series(*kind);
            chart.legend = Some(Legend::at(LegendPosition::Right));
            chart.labels = Labels::values();
            if kind.plots_points() {
                for series in &mut chart.series {
                    series.xs = vec![1.0, 2.0, 3.0];
                    series.sizes = vec![1.0, 2.0, 3.0];
                }
            }
            let drawing = draw_chart(&chart);
            for rule in &drawing.rules {
                assert!(rule.x >= -1.0 && rule.x + rule.width <= 401.0, "{kind:?} {rule:?}");
                assert!(rule.y >= -1.0 && rule.y + rule.height <= 301.0, "{kind:?} {rule:?}");
            }
            for (path, _) in &drawing.paths {
                let (left, top, right, bottom) = wp_raster::bounds_of(path).expect("a shape");
                assert!(left >= -1.0 && right <= 401.0, "{kind:?} {left} {right}");
                assert!(top >= -1.0 && bottom <= 301.0, "{kind:?} {top} {bottom}");
            }
            assert!(!drawing.glyphs.is_empty(), "{kind:?} drew no words");
        }
    }

    #[test]
    fn a_number_is_written_the_way_a_chart_writes_one() {
        assert_eq!(number(3.0), "3");
        assert_eq!(number(2.5), "2.5");
        assert_eq!(number(0.0), "0");
    }

    #[test]
    fn a_scale_is_marked_at_numbers_a_person_would_choose() {
        let scale = Scale::spanning(&Axis::default(), 0.0, 7.0);
        assert_eq!(scale.step, 2.0);
        assert_eq!(scale.min, 0.0);
        assert_eq!(scale.max, 8.0);
        assert_eq!(scale.marks(), vec![0.0, 2.0, 4.0, 6.0, 8.0]);

        let stated =
            Axis { min: Some(-5.0), max: Some(50.0), major_unit: Some(25.0), ..Axis::default() };
        let scale = Scale::spanning(&stated, 0.0, 7.0);
        assert_eq!((scale.min, scale.max, scale.step), (-5.0, 50.0, 25.0));
    }

    #[test]
    fn a_negative_number_is_drawn_below_the_nought_line() {
        let drawing = drawn(Kind::Column, "a=3; b=-2");
        let bars = coloured(&drawing);
        assert_eq!(bars.len(), 2);
        // The second bar hangs from where the first stands.
        assert!((bars[0].y + bars[0].height - bars[1].y).abs() < 1.0, "{bars:?}");
    }

    #[test]
    fn stacked_columns_stand_on_one_another_and_shares_reach_the_top() {
        let stacked =
            draw_chart(&Chart { grouping: Grouping::Stacked, ..two_series(Kind::Column) });
        let bars = coloured(&stacked);
        assert_eq!(bars.len(), 6);
        // The second series' first bar stands on the first series' first.
        let (first, second) = (bars[0], bars[1]);
        assert!((first.y - (second.y + second.height)).abs() < 1.0, "{first:?} {second:?}");
        assert!((first.x - second.x).abs() < 0.5, "not in the same slot");

        let shares =
            draw_chart(&Chart { grouping: Grouping::PercentStacked, ..two_series(Kind::Column) });
        let heights: Vec<f32> =
            coloured(&shares).chunks(2).map(|pair| pair[0].height + pair[1].height).collect();
        assert!(heights.windows(2).all(|pair| (pair[0] - pair[1]).abs() < 1.0), "{heights:?}");
    }

    #[test]
    fn an_area_chart_fills_under_its_line() {
        let drawing = draw_chart(&two_series(Kind::Area));
        assert_eq!(drawing.paths.len(), 2, "one shape per series");
    }

    #[test]
    fn a_scatter_chart_has_a_point_for_every_pair_and_a_bubble_a_circle() {
        let chart = Chart::parse(Kind::Scatter, "", "1=3; 2=5; 4=4");
        let drawing = draw_chart(&chart);
        assert_eq!(coloured(&drawing).len(), 3);

        let bubbles = draw_chart(&Chart::parse(Kind::Bubble, "", "1=3:1; 2=5:4"));
        assert_eq!(bubbles.paths.len(), 2);
        let area = |path: &Path| {
            let (l, t, r, b) = wp_raster::bounds_of(path).expect("a bubble");
            (r - l) * (b - t)
        };
        assert!(
            area(&bubbles.paths[1].0) > area(&bubbles.paths[0].0) * 2.0,
            "the bigger bubble is not bigger"
        );
    }

    #[test]
    fn a_radar_draws_a_ring_per_series_and_a_spoke_per_category() {
        let drawing = draw_chart(&two_series(Kind::Radar));
        // Rings for the scale, spokes for the categories, and the series.
        assert!(drawing.paths.len() >= 2 + 3, "{}", drawing.paths.len());
    }

    #[test]
    fn a_surface_is_a_grid_of_bands() {
        let drawing = draw_chart(&two_series(Kind::Surface));
        // A cell per category per series band.
        assert!(
            drawing
                .rules
                .iter()
                .filter(|rule| rule.color.alpha == 255 && rule.width > 5.0 && rule.height > 5.0)
                .count()
                >= 3
        );
    }

    #[test]
    fn a_doughnut_has_a_ring_per_series_with_a_hole() {
        let chart = Chart { hole: 50, ..two_series(Kind::Doughnut) };
        let drawing = draw_chart(&chart);
        assert_eq!(drawing.paths.len(), 6, "a slice per point per ring");
    }

    #[test]
    fn the_key_goes_where_it_is_asked_to() {
        let at = |position| {
            let chart = Chart { legend: Some(Legend::at(position)), ..two_series(Kind::Column) };
            let drawing = draw_chart(&chart);
            let squares: Vec<&Decoration> = drawing
                .rules
                .iter()
                .filter(|rule| (rule.width - rule.height).abs() < 0.01 && rule.width < 12.0)
                .collect();
            let x = squares.iter().map(|square| square.x).fold(0.0, f32::max);
            let y = squares.iter().map(|square| square.y).fold(0.0, f32::max);
            (x, y)
        };
        let (right_x, _) = at(LegendPosition::Right);
        let (left_x, _) = at(LegendPosition::Left);
        let (_, top_y) = at(LegendPosition::Top);
        let (_, bottom_y) = at(LegendPosition::Bottom);
        assert!(right_x > 300.0, "{right_x}");
        assert!(left_x < 60.0, "{left_x}");
        assert!(top_y < 60.0, "{top_y}");
        assert!(bottom_y > 240.0, "{bottom_y}");
    }

    #[test]
    fn the_table_of_numbers_is_drawn_under_the_plot() {
        let with = draw_chart(&Chart {
            data_table: Some(DataTable::default()),
            ..two_series(Kind::Column)
        });
        let without = draw_chart(&two_series(Kind::Column));
        assert!(with.glyphs.len() > without.glyphs.len() + 6, "the numbers were not written");
        assert!(with.rules.len() > without.rules.len(), "the table has no lines");
    }

    #[test]
    fn a_label_says_what_it_is_asked_to() {
        let mut chart = two_series(Kind::Column);
        chart.labels = Labels { value: true, category: true, series: true, ..Labels::default() };
        let canvas = Canvas {
            shaper: &mut Squares,
            chart: &chart,
            palette: &palette(),
            size: 10.0,
            out: ChartDrawing::default(),
        };
        assert_eq!(canvas.label_text(&chart.series[0], 1, 5.0, 0.5), "Last, South, 5");
        let percent = Chart {
            labels: Labels { percent: true, number_format: None, ..Labels::default() },
            ..two_series(Kind::Pie)
        };
        let canvas = Canvas {
            shaper: &mut Squares,
            chart: &percent,
            palette: &palette(),
            size: 10.0,
            out: ChartDrawing::default(),
        };
        assert_eq!(canvas.label_text(&percent.series[0], 0, 3.0, 0.25), "25%");
    }

    #[test]
    fn a_number_format_on_the_axis_writes_money_as_money() {
        let mut chart = two_series(Kind::Column);
        chart.value_axis.number_format = Some("\"$\"#,##0".to_owned());
        chart.series[0].values = vec![1000.0, 2000.0, 3000.0];
        let drawing = draw_chart(&chart);
        // A dollar sign is drawn: the shaper draws one glyph per character,
        // so the axis words are longer than the bare numbers would be.
        let bare = draw_chart(&Chart { value_axis: Axis::default(), ..chart.clone() });
        assert!(drawing.glyphs.len() > bare.glyphs.len());
    }

    #[test]
    fn a_series_coloured_by_the_document_keeps_its_colour() {
        let mut chart = two_series(Kind::Column);
        chart.series[0].fill = Some("FF0000".to_owned());
        chart.series[1].points = vec![(2, "00FF00".to_owned())];
        let drawing = draw_chart(&chart);
        assert!(drawing.rules.iter().any(|rule| rule.color == Color::rgb(255, 0, 0)));
        assert!(drawing.rules.iter().any(|rule| rule.color == Color::rgb(0, 255, 0)));
    }

    #[test]
    fn a_combination_chart_draws_the_line_over_the_columns() {
        let mut chart = two_series(Kind::Column);
        chart.series[1].kind = Some(Kind::Line);
        chart.series[1].secondary = true;
        let drawing = draw_chart(&chart);
        // Three columns, and a line of two joins with its three markers.
        assert_eq!(drawing.paths.len(), 2);
        assert_eq!(coloured(&drawing).len(), 6);
    }

    #[test]
    fn a_pie_label_that_will_not_fit_is_moved_out_on_a_leader_line() {
        let mut chart = Chart::parse(Kind::Pie, "", "big=100; tiny=1");
        chart.labels = Labels { value: true, leader_lines: true, ..Labels::default() };
        let drawing = draw_chart(&chart);
        // Two slices and one leader line.
        assert_eq!(drawing.paths.len(), 3, "{}", drawing.paths.len());
    }
}

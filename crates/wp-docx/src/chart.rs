//! Charts: numbers drawn as columns, bars, lines, areas, slices, points,
//! bubbles, a radar or a surface, and the words round them.
//!
//! # Why the chart is a part of the package
//!
//! Because that is what it is. A chart in a document is not a picture of a
//! chart — it is a part of its own, `word/charts/chart1.xml`, holding the
//! numbers and how to draw them, with the document pointing at it through a
//! relationship. Word rebuilds the picture from the numbers, which is why a
//! chart stays sharp when the page is zoomed and why the numbers can still be
//! read out of the file years later.
//!
//! Writing a picture instead would have been a tenth of the work and would have
//! produced a document where the chart cannot be edited, cannot be recoloured
//! by a theme, and cannot be read by anything that reads numbers.
//!
//! # What a chart is made of
//!
//! Series of numbers, each with a name, against one set of categories — or
//! against x values, for a chart that plots points. How they are drawn is
//! the [`Kind`], and for columns, bars, lines and areas how the series stand
//! to one another is the [`Grouping`]: side by side, stacked, or stacked to
//! the same height with each shown as its share. A series may be drawn as
//! another kind than the chart's, which is what Word calls a combination
//! chart, and may stand against a second value axis down the right.
//!
//! Round the numbers: a title, a key ([`Legend`]) at any of the four sides,
//! the words on each point ([`Labels`] — the number, the category, the
//! series, the share of the whole), a table of the numbers under the plot
//! ([`DataTable`]), and the axes with the scale the file states and the
//! number format it asks for ([`Axis`]).
//!
//! The numbers are written in full — Word calls them the cached values — so
//! the chart draws without the spreadsheet beside it. The spreadsheet is
//! written too, in `word/embeddings`, because it is what Word opens when
//! somebody asks to edit the data: see [`crate::workbook`].
//!
//! Three-dimensional kinds — `c:bar3DChart` and its like — are read as the
//! flat kind they are a picture of and drawn flat, with the same numbers in
//! the same places; Word's perspective is not reproduced.

use wp_xml::tree::Element;

use crate::theme::Theme;
use crate::workbook::Cell;

/// The namespace charts are written in.
pub const CHART_NAMESPACE: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
/// And the one that says a drawing holds a chart.
pub const CHART_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
/// What the package calls a chart part.
pub const CHART_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
/// And the relationship that points at one.
pub const CHART_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";

const A: &str = crate::edit::DRAWING_MAIN;

/// How the numbers are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// Upright columns, which is what "bar chart" usually means to a person.
    #[default]
    Column,
    /// Bars lying on their side, for categories with long names.
    Bar,
    Line,
    Pie,
    /// A pie with a hole in it, which is how Word draws several series as
    /// rings.
    Doughnut,
    /// A line with the room under it filled in.
    Area,
    /// Points at an x and a y, joined or not.
    Scatter,
    /// Points at an x and a y, each drawn as large as a third number says.
    Bubble,
    /// One axis per category, spread round a circle.
    Radar,
    /// The numbers as heights over a grid of categories and series, drawn as
    /// bands of colour: what Word calls a contour, and what its three
    /// dimensional surface is seen from above.
    Surface,
}

impl Kind {
    pub const ALL: &'static [Self] = &[
        Self::Column,
        Self::Bar,
        Self::Line,
        Self::Pie,
        Self::Doughnut,
        Self::Area,
        Self::Scatter,
        Self::Bubble,
        Self::Radar,
        Self::Surface,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Column => "Column",
            Self::Bar => "Bar",
            Self::Line => "Line",
            Self::Pie => "Pie",
            Self::Doughnut => "Doughnut",
            Self::Area => "Area",
            Self::Scatter => "Scatter",
            Self::Bubble => "Bubble",
            Self::Radar => "Radar",
            Self::Surface => "Surface",
        }
    }

    /// The element the chart is drawn by.
    #[must_use]
    fn element(self) -> &'static str {
        match self {
            Self::Column | Self::Bar => "barChart",
            Self::Line => "lineChart",
            Self::Pie => "pieChart",
            Self::Doughnut => "doughnutChart",
            Self::Area => "areaChart",
            Self::Scatter => "scatterChart",
            Self::Bubble => "bubbleChart",
            Self::Radar => "radarChart",
            Self::Surface => "surfaceChart",
        }
    }

    /// The kind an element draws, the three dimensional kinds read as the
    /// flat ones they picture.
    fn from_element(name: &str) -> Option<Self> {
        Some(match name {
            "barChart" | "bar3DChart" => Self::Column,
            "lineChart" | "line3DChart" | "stockChart" => Self::Line,
            "pieChart" | "pie3DChart" | "ofPieChart" => Self::Pie,
            "doughnutChart" => Self::Doughnut,
            "areaChart" | "area3DChart" => Self::Area,
            "scatterChart" => Self::Scatter,
            "bubbleChart" => Self::Bubble,
            "radarChart" => Self::Radar,
            "surfaceChart" | "surface3DChart" => Self::Surface,
            _ => return None,
        })
    }

    /// Which way the bars run, for the kinds that have bars.
    #[must_use]
    fn direction(self) -> Option<&'static str> {
        match self {
            Self::Column => Some("col"),
            Self::Bar => Some("bar"),
            _ => None,
        }
    }

    /// Whether the chart has axes drawn round it.
    #[must_use]
    pub fn has_axes(self) -> bool {
        !matches!(self, Self::Pie | Self::Doughnut)
    }

    /// Whether the series stand to one another in some grouping: side by
    /// side, stacked, or as shares.
    #[must_use]
    pub fn is_grouped(self) -> bool {
        matches!(self, Self::Column | Self::Bar | Self::Line | Self::Area)
    }

    /// Whether the points have an x of their own rather than a category.
    #[must_use]
    pub fn plots_points(self) -> bool {
        matches!(self, Self::Scatter | Self::Bubble)
    }

    /// Whether the chart is drawn as slices of a whole.
    #[must_use]
    pub fn is_round(self) -> bool {
        matches!(self, Self::Pie | Self::Doughnut)
    }
}

/// How the series of a column, bar, line or area chart stand to one another.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Grouping {
    /// Side by side in each category's slot, which is what a chart of
    /// several series means unless it says otherwise.
    #[default]
    Clustered,
    /// One on top of another, so the column is the total.
    Stacked,
    /// Stacked and stretched to the same height, so each is its share.
    PercentStacked,
}

impl Grouping {
    pub const ALL: &'static [Self] = &[Self::Clustered, Self::Stacked, Self::PercentStacked];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Clustered => "Clustered",
            Self::Stacked => "Stacked",
            Self::PercentStacked => "100% Stacked",
        }
    }

    /// The word the format writes: a bar chart says `clustered` where a line
    /// or an area says `standard` for the same thing.
    fn word(self, kind: Kind) -> &'static str {
        match self {
            Self::Clustered if matches!(kind, Kind::Column | Kind::Bar) => "clustered",
            Self::Clustered => "standard",
            Self::Stacked => "stacked",
            Self::PercentStacked => "percentStacked",
        }
    }

    fn from_word(word: &str) -> Self {
        match word {
            "stacked" => Self::Stacked,
            "percentStacked" => Self::PercentStacked,
            _ => Self::Clustered,
        }
    }
}

/// A chart: what it is called, what it counts, and how it is drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chart {
    pub kind: Kind,
    pub grouping: Grouping,
    /// The heading over the chart. Empty for none.
    pub title: String,
    /// The names along the bottom, which every series shares: a chart draws one
    /// set of categories and a number from each series against each of them.
    pub categories: Vec<String>,
    /// The series themselves, in the order the file gives them.
    pub series: Vec<Series>,
    /// Where the key goes, or nothing for a chart drawn without one.
    pub legend: Option<Legend>,
    /// What is written on each point, for every series that does not say
    /// otherwise for itself.
    pub labels: Labels,
    /// The table of the numbers under the plot, when the chart asks for one.
    pub data_table: Option<DataTable>,
    /// The axis the numbers are read against, and the one the categories
    /// stand along.
    pub value_axis: Axis,
    pub category_axis: Axis,
    /// Whether each point is its own colour rather than each series, which
    /// is how a pie is coloured and how a column chart usually is not.
    pub vary_colors: bool,
    /// A doughnut's hole, as a percentage of its width.
    pub hole: u8,
}

/// One run of numbers, with the name a key would call it by.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Series {
    pub name: String,
    /// One number per category. A series with fewer is drawn as far as it
    /// goes, because that is what the file says and not something to make up.
    pub values: Vec<f64>,
    /// The x of each point, for a chart that plots points. Empty otherwise.
    pub xs: Vec<f64>,
    /// How large each bubble is, for a bubble chart. Empty otherwise.
    pub sizes: Vec<f64>,
    /// Drawn as another kind than the chart's: the line over the columns of
    /// a combination chart.
    pub kind: Option<Kind>,
    /// Read against a second value axis, down the right.
    pub secondary: bool,
    /// The colour the document chose for it, as six hex digits, or nothing
    /// for the next of the theme's.
    pub fill: Option<String>,
    /// Points coloured on their own, by their place in the series.
    pub points: Vec<(usize, String)>,
    /// What is written on this series' points, where it differs from the
    /// chart's.
    pub labels: Option<Labels>,
    /// Where in the workbook the numbers are, for Word's own grid.
    pub reference: Option<String>,
}

/// The key: where it sits, and whether it sits over the plot or beside it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Legend {
    pub position: LegendPosition,
    pub overlay: bool,
}

impl Legend {
    #[must_use]
    pub fn at(position: LegendPosition) -> Self {
        Self { position, overlay: false }
    }
}

/// Where the key sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LegendPosition {
    #[default]
    Right,
    Bottom,
    Left,
    Top,
    /// In the top right corner, which is where Word's dialog puts a key it
    /// calls "top right".
    TopRight,
}

impl LegendPosition {
    /// The name the format gives it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Right => "r",
            Self::Bottom => "b",
            Self::Left => "l",
            Self::Top => "t",
            Self::TopRight => "tr",
        }
    }

    /// And reading one back. A place nobody knows is the right-hand side, which
    /// is where the format puts a key that says nothing.
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "b" => Self::Bottom,
            "l" => Self::Left,
            "t" => Self::Top,
            "tr" => Self::TopRight,
            _ => Self::Right,
        }
    }
}

/// What is written on a point.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Labels {
    /// The number itself.
    pub value: bool,
    /// The category the point stands in.
    pub category: bool,
    /// The series the point belongs to.
    pub series: bool,
    /// The point's share of the whole, for a chart drawn as slices.
    pub percent: bool,
    /// A small square of the series' colour beside the words.
    pub key: bool,
    /// A line from a label that had to be moved off its slice back to it.
    pub leader_lines: bool,
    /// How the number is written, as a spreadsheet writes it — see
    /// [`crate::numberformat`]. Nothing means as it is.
    pub number_format: Option<String>,
    /// Where the words sit against the point.
    pub position: Option<LabelPosition>,
}

impl Labels {
    /// Labels showing the number and nothing else.
    #[must_use]
    pub fn values() -> Self {
        Self { value: true, ..Self::default() }
    }

    /// Whether anything is written at all.
    #[must_use]
    pub fn shows_anything(&self) -> bool {
        self.value || self.category || self.series || self.percent
    }
}

/// Where a label sits against its point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelPosition {
    /// Just past the end of the bar, which is where Word puts one unasked.
    OutsideEnd,
    /// Inside the bar at its end.
    InsideEnd,
    Center,
    /// Inside the bar at its base.
    InsideBase,
    /// Wherever it fits, which for a slice is inside when there is room and
    /// outside on a leader line when there is not.
    BestFit,
    Above,
    Below,
    Left,
    Right,
}

impl LabelPosition {
    fn word(self) -> &'static str {
        match self {
            Self::OutsideEnd => "outEnd",
            Self::InsideEnd => "inEnd",
            Self::Center => "ctr",
            Self::InsideBase => "inBase",
            Self::BestFit => "bestFit",
            Self::Above => "t",
            Self::Below => "b",
            Self::Left => "l",
            Self::Right => "r",
        }
    }

    fn from_word(word: &str) -> Option<Self> {
        Some(match word {
            "outEnd" => Self::OutsideEnd,
            "inEnd" => Self::InsideEnd,
            "ctr" => Self::Center,
            "inBase" => Self::InsideBase,
            "bestFit" => Self::BestFit,
            "t" => Self::Above,
            "b" => Self::Below,
            "l" => Self::Left,
            "r" => Self::Right,
            _ => return None,
        })
    }
}

/// The table of the numbers drawn under the plot, one row per series.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataTable {
    /// The square of each series' colour at the start of its row, which is
    /// the key folded into the table.
    pub keys: bool,
    pub horizontal_lines: bool,
    pub vertical_lines: bool,
    pub outline: bool,
}

impl Default for DataTable {
    fn default() -> Self {
        Self { keys: true, horizontal_lines: true, vertical_lines: true, outline: true }
    }
}

/// An axis: the scale the file states for it, how its numbers are written,
/// and whether it is drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Axis {
    /// The ends of the scale, where the file states them. Nothing means
    /// worked out from the numbers.
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// How far apart the marks are, where the file states it.
    pub major_unit: Option<f64>,
    /// How the numbers along it are written — see [`crate::numberformat`].
    pub number_format: Option<String>,
    /// An axis the file has but asks not to draw.
    pub deleted: bool,
}

impl Chart {
    /// Reads a chart out of one line typed as `name=value; name=value`.
    ///
    /// For a chart that plots points the name is the x, and a bubble's
    /// value is `y:size`.
    #[must_use]
    pub fn parse(kind: Kind, title: &str, typed: &str) -> Self {
        let mut categories = Vec::new();
        let mut values = Vec::new();
        let mut xs = Vec::new();
        let mut sizes = Vec::new();
        let number = |text: &str| text.trim().replace(',', ".").parse::<f64>().ok();
        for piece in typed.split(';') {
            let piece = piece.trim();
            if piece.is_empty() {
                continue;
            }
            let (name, rest) = match piece.split_once('=') {
                Some((name, rest)) => (name.trim().to_owned(), rest.trim()),
                // A bare number is a bar with no name, which is better than
                // throwing the number away.
                None => (String::new(), piece),
            };
            let (value, size) = match (kind, rest.split_once(':')) {
                (Kind::Bubble, Some((value, size))) => (number(value), number(size)),
                _ => (number(rest), None),
            };
            let Some(value) = value else { continue };
            if kind.plots_points() {
                xs.push(number(&name).unwrap_or(values.len() as f64 + 1.0));
                sizes.push(size.unwrap_or(1.0));
            }
            categories.push(name);
            values.push(value);
        }
        if kind.plots_points() {
            categories.clear();
        }
        if kind != Kind::Bubble {
            sizes.clear();
        }
        Self {
            kind,
            title: title.trim().to_owned(),
            categories,
            series: vec![Series {
                name: "Series 1".to_owned(),
                values,
                xs,
                sizes,
                ..Series::default()
            }],
            // One series needs no key to tell it from the others, which is
            // what Word decides for the same chart — except a pie, whose key
            // names the slices and without which it says nothing at all.
            legend: kind.is_round().then_some(Legend::at(LegendPosition::Right)),
            vary_colors: kind.is_round(),
            hole: if kind == Kind::Doughnut { 50 } else { 0 },
            ..Self::default()
        }
    }

    /// Whether there is anything to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.series.iter().all(|series| series.values.is_empty())
    }

    /// The largest number anywhere in it, which is what the axis has to reach.
    #[must_use]
    pub fn largest(&self) -> f64 {
        self.series.iter().flat_map(|series| series.values.iter().copied()).fold(0.0, f64::max)
    }

    /// And the smallest, which is nought unless a number is below it.
    #[must_use]
    pub fn smallest(&self) -> f64 {
        self.series.iter().flat_map(|series| series.values.iter().copied()).fold(0.0, f64::min)
    }

    /// The total of the first series, which is what a pie divides up: a pie
    /// of several series would be several pies.
    #[must_use]
    pub fn total(&self) -> f64 {
        self.series.first().map_or(0.0, |series| series.values.iter().sum())
    }

    /// How many points the longest series has, which is how many slots the
    /// categories are drawn in.
    #[must_use]
    pub fn points(&self) -> usize {
        self.series.iter().map(|series| series.values.len()).max().unwrap_or(0)
    }

    /// What is written on a series' points: its own say, or the chart's.
    #[must_use]
    pub fn labels_of<'a>(&'a self, series: &'a Series) -> &'a Labels {
        series.labels.as_ref().unwrap_or(&self.labels)
    }

    /// The kind a series is drawn as.
    #[must_use]
    pub fn kind_of(&self, series: &Series) -> Kind {
        series.kind.unwrap_or(self.kind)
    }

    /// Whether any series stands against the second value axis.
    #[must_use]
    pub fn has_secondary_axis(&self) -> bool {
        self.series.iter().any(|series| series.secondary)
    }

    /// The kinds drawn, the chart's own first, each with the series drawn
    /// that way.
    #[must_use]
    pub fn groups(&self) -> Vec<(Kind, Vec<&Series>)> {
        let mut groups: Vec<(Kind, Vec<&Series>)> = vec![(self.kind, Vec::new())];
        for series in &self.series {
            let kind = self.kind_of(series);
            match groups.iter_mut().find(|(known, _)| *known == kind) {
                Some((_, members)) => members.push(series),
                None => groups.push((kind, vec![series])),
            }
        }
        groups
    }
}

/// Builds the whole of a chart part.
///
/// `workbook` is the relationship to the spreadsheet behind the chart,
/// when it has one.
#[must_use]
pub fn chart_xml(chart: &Chart, workbook: Option<&str>) -> String {
    let mut out = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push('\n');
    out.push_str(&format!(
        r#"<c:chartSpace xmlns:c="{CHART_NAMESPACE}" xmlns:a="{A}" xmlns:r="{rel}">"#,
        rel = crate::edit::RELATIONSHIPS,
    ));
    out.push_str("<c:chart>");

    if !chart.title.is_empty() {
        out.push_str("<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>");
        out.push_str(&escape(&chart.title));
        out.push_str("</a:t></a:r></a:p></c:rich></c:tx><c:overlay val=\"0\"/></c:title>");
        out.push_str("<c:autoTitleDeleted val=\"0\"/>");
    }

    out.push_str("<c:plotArea><c:layout/>");
    // One element per kind drawn, the chart's own first, and a series is
    // numbered by its place among all of them.
    let mut index = 0usize;
    for (kind, members) in chart.groups() {
        let secondary = members.iter().any(|series| series.secondary);
        out.push_str(&format!("<c:{}>", kind.element()));
        if let Some(direction) = kind.direction() {
            out.push_str(&format!("<c:barDir val=\"{direction}\"/>"));
        }
        if kind.is_grouped() {
            out.push_str(&format!("<c:grouping val=\"{}\"/>", chart.grouping.word(kind)));
        }
        if kind == Kind::Scatter {
            out.push_str("<c:scatterStyle val=\"lineMarker\"/>");
        }
        if kind == Kind::Radar {
            out.push_str("<c:radarStyle val=\"marker\"/>");
        }
        if kind == Kind::Surface {
            out.push_str("<c:wireframe val=\"0\"/>");
        }
        if kind != Kind::Surface {
            out.push_str(&format!("<c:varyColors val=\"{}\"/>", u8::from(chart.vary_colors)));
        }
        for series in members {
            out.push_str(&series_xml(chart, series, index, kind));
            index += 1;
        }
        if matches!(kind, Kind::Column | Kind::Bar) && chart.grouping != Grouping::Clustered {
            out.push_str("<c:overlap val=\"100\"/>");
        }
        if kind == Kind::Doughnut {
            out.push_str(&format!("<c:holeSize val=\"{}\"/>", chart.hole.clamp(10, 90)));
        }
        if kind.has_axes() {
            let (first, second) = if secondary { (3, 4) } else { (1, 2) };
            out.push_str(&format!("<c:axId val=\"{first}\"/><c:axId val=\"{second}\"/>"));
            if kind == Kind::Surface {
                out.push_str("<c:axId val=\"5\"/>");
            }
        }
        out.push_str(&format!("</c:{}>", kind.element()));
    }

    if chart.kind.has_axes() {
        out.push_str(&axes_xml(chart, 1, 2, "b", "l", false));
        if chart.has_secondary_axis() {
            // The second value axis stands down the right, against a
            // category axis of its own that is not drawn.
            out.push_str(&axes_xml(chart, 3, 4, "b", "r", true));
        }
        if chart.kind == Kind::Surface {
            out.push_str(
                "<c:serAx><c:axId val=\"5\"/><c:scaling><c:orientation val=\"minMax\"/>\
                 </c:scaling><c:delete val=\"0\"/><c:axPos val=\"b\"/><c:crossAx val=\"2\"/>\
                 </c:serAx>",
            );
        }
    }
    if let Some(table) = chart.data_table {
        out.push_str(&format!(
            "<c:dTable><c:showHorzBorder val=\"{}\"/><c:showVertBorder val=\"{}\"/>\
             <c:showOutline val=\"{}\"/><c:showKeys val=\"{}\"/></c:dTable>",
            u8::from(table.horizontal_lines),
            u8::from(table.vertical_lines),
            u8::from(table.outline),
            u8::from(table.keys),
        ));
    }
    out.push_str("</c:plotArea>");
    // The key, which is what says which series is which. After the plot area
    // and before what is drawn of the plot, which is the order the schema asks
    // for.
    if let Some(legend) = chart.legend {
        out.push_str(&format!(
            "<c:legend><c:legendPos val=\"{}\"/><c:overlay val=\"{}\"/></c:legend>",
            legend.position.word(),
            u8::from(legend.overlay),
        ));
    }
    out.push_str("<c:plotVisOnly val=\"1\"/></c:chart>");
    if let Some(id) = workbook {
        out.push_str(&format!(
            "<c:externalData r:id=\"{}\"><c:autoUpdate val=\"0\"/></c:externalData>",
            escape(id)
        ));
    }
    out.push_str("</c:chartSpace>");
    out
}

/// A pair of axes: the one the categories or x values stand along, and the
/// one the numbers are read against.
fn axes_xml(chart: &Chart, first: u32, second: u32, along: &str, up: &str, hidden: bool) -> String {
    let (along, up) = if chart.kind == Kind::Bar { (up, along) } else { (along, up) };
    let first_element = if chart.kind.plots_points() { "valAx" } else { "catAx" };
    let mut out = format!(
        "<c:{first_element}><c:axId val=\"{first}\"/><c:scaling><c:orientation val=\"minMax\"/>\
         </c:scaling><c:delete val=\"{}\"/><c:axPos val=\"{along}\"/>",
        u8::from(hidden || chart.category_axis.deleted)
    );
    if let Some(code) = &chart.category_axis.number_format {
        out.push_str(&format!("<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>", escape(code)));
    }
    out.push_str(&format!("<c:crossAx val=\"{second}\"/></c:{first_element}>"));

    let axis = &chart.value_axis;
    out.push_str(&format!(
        "<c:valAx><c:axId val=\"{second}\"/><c:scaling><c:orientation val=\"minMax\"/>"
    ));
    if let Some(max) = axis.max {
        out.push_str(&format!("<c:max val=\"{max}\"/>"));
    }
    if let Some(min) = axis.min {
        out.push_str(&format!("<c:min val=\"{min}\"/>"));
    }
    out.push_str(&format!(
        "</c:scaling><c:delete val=\"{}\"/><c:axPos val=\"{up}\"/>",
        u8::from(axis.deleted)
    ));
    if let Some(code) = &axis.number_format {
        out.push_str(&format!("<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>", escape(code)));
    }
    out.push_str(&format!("<c:crossAx val=\"{first}\"/>"));
    if hidden {
        out.push_str("<c:crosses val=\"max\"/>");
    }
    if let Some(unit) = axis.major_unit {
        out.push_str(&format!("<c:majorUnit val=\"{unit}\"/>"));
    }
    out.push_str("</c:valAx>");
    out
}

/// One series: its name, the categories it shares, and its numbers.
///
/// The column it says it came from is its place among the series, which is
/// where [`crate::workbook`] puts it.
fn series_xml(chart: &Chart, series: &Series, index: usize, kind: Kind) -> String {
    use crate::workbook::column_reference;

    let column = index as u32 + 1;
    let mut out = format!("<c:ser><c:idx val=\"{index}\"/><c:order val=\"{index}\"/>");
    out.push_str(&format!(
        "<c:tx><c:strRef><c:f>{}</c:f><c:strCache><c:ptCount val=\"1\"/>",
        column_reference(column, 1, 1)
    ));
    out.push_str(&format!("<c:pt idx=\"0\"><c:v>{}</c:v></c:pt>", escape(&series.name)));
    out.push_str("</c:strCache></c:strRef></c:tx>");

    // The colour the document chose, for the whole series and for any point
    // of its own. A line is coloured by its outline, everything else by its
    // fill.
    let lined = matches!(kind, Kind::Line | Kind::Scatter | Kind::Radar);
    if let Some(fill) = &series.fill {
        out.push_str(&colour_xml(fill, lined));
    }
    for (point, fill) in &series.points {
        out.push_str(&format!("<c:dPt><c:idx val=\"{point}\"/>"));
        if !lined {
            out.push_str("<c:invertIfNegative val=\"0\"/>");
        }
        out.push_str(&colour_xml(fill, lined));
        out.push_str("</c:dPt>");
    }

    // What is written on the points, which the format says per series even
    // though Word asks it of the whole chart.
    let labels = chart.labels_of(series);
    if labels.shows_anything() {
        out.push_str(&labels_xml(labels));
    }

    let rows = (series.values.len() as u32).max(1);
    if kind.plots_points() {
        // Points have an x each rather than a category, and a bubble a size.
        out.push_str(&format!("<c:xVal><c:numRef><c:f>{}</c:f>", column_reference(0, 2, rows + 1)));
        out.push_str(&number_cache(&series.xs));
        out.push_str("</c:numRef></c:xVal>");
        out.push_str(&format!(
            "<c:yVal><c:numRef><c:f>{}</c:f>",
            column_reference(column, 2, rows + 1)
        ));
        out.push_str(&number_cache(&series.values));
        out.push_str("</c:numRef></c:yVal>");
        if kind == Kind::Bubble {
            out.push_str(&format!(
                "<c:bubbleSize><c:numRef><c:f>{}</c:f>",
                column_reference(column + 1, 2, rows + 1)
            ));
            out.push_str(&number_cache(&series.sizes));
            out.push_str("</c:numRef></c:bubbleSize>");
        }
    } else {
        // The names, written out in full so the chart draws without the
        // workbook.
        out.push_str(&format!(
            "<c:cat><c:strRef><c:f>{}</c:f><c:strCache>",
            column_reference(0, 2, chart.categories.len().max(1) as u32 + 1)
        ));
        out.push_str(&format!("<c:ptCount val=\"{}\"/>", chart.categories.len()));
        for (index, name) in chart.categories.iter().enumerate() {
            out.push_str(&format!("<c:pt idx=\"{index}\"><c:v>{}</c:v></c:pt>", escape(name)));
        }
        out.push_str("</c:strCache></c:strRef></c:cat>");

        out.push_str(&format!(
            "<c:val><c:numRef><c:f>{}</c:f>",
            column_reference(column, 2, rows + 1)
        ));
        out.push_str(&number_cache(&series.values));
        out.push_str("</c:numRef></c:val>");
    }
    if lined {
        out.push_str("<c:smooth val=\"0\"/>");
    }
    out.push_str("</c:ser>");
    out
}

/// A run of numbers as a cache.
fn number_cache(values: &[f64]) -> String {
    let mut out = String::from("<c:numCache><c:formatCode>General</c:formatCode>");
    out.push_str(&format!("<c:ptCount val=\"{}\"/>", values.len()));
    for (index, value) in values.iter().enumerate() {
        out.push_str(&format!("<c:pt idx=\"{index}\"><c:v>{value}</c:v></c:pt>"));
    }
    out.push_str("</c:numCache>");
    out
}

/// A colour chosen by the document, as the fill of a shape or the line of
/// one.
fn colour_xml(fill: &str, lined: bool) -> String {
    let colour = format!("<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>", escape(fill));
    if lined {
        format!("<c:spPr><a:ln w=\"28575\">{colour}</a:ln></c:spPr>")
    } else {
        format!("<c:spPr>{colour}</c:spPr>")
    }
}

/// What is written on the points, as the format says it.
fn labels_xml(labels: &Labels) -> String {
    let mut out = String::from("<c:dLbls>");
    if let Some(code) = &labels.number_format {
        out.push_str(&format!("<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>", escape(code)));
    }
    if let Some(position) = labels.position {
        out.push_str(&format!("<c:dLblPos val=\"{}\"/>", position.word()));
    }
    out.push_str(&format!(
        "<c:showLegendKey val=\"{}\"/><c:showVal val=\"{}\"/><c:showCatName val=\"{}\"/>\
         <c:showSerName val=\"{}\"/><c:showPercent val=\"{}\"/><c:showBubbleSize val=\"0\"/>",
        u8::from(labels.key),
        u8::from(labels.value),
        u8::from(labels.category),
        u8::from(labels.series),
        u8::from(labels.percent),
    ));
    if labels.leader_lines {
        out.push_str("<c:showLeaderLines val=\"1\"/>");
    }
    out.push_str("</c:dLbls>");
    out
}

/// The five characters XML will not take as themselves.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// Reads a chart back out of a chart part, its colours resolved against no
/// theme: one that names the theme's colours is drawn in the palette.
#[must_use]
pub fn read_chart(root: &Element) -> Option<Chart> {
    read_chart_with(root, &Theme::default())
}

/// Reads a chart back out of a chart part, the colours it names from the
/// theme resolved against this one.
#[must_use]
pub fn read_chart_with(root: &Element, theme: &Theme) -> Option<Chart> {
    let chart = root.child(Some(CHART_NAMESPACE), "chart")?;
    let plot = chart.child(Some(CHART_NAMESPACE), "plotArea")?;

    // Every kind drawn, in the order the file gives them: the first is the
    // chart's, and a series of any other is drawn as that other.
    let drawn: Vec<(Kind, &Element)> = plot
        .child_elements()
        .filter(|element| element.namespace.as_deref() == Some(CHART_NAMESPACE))
        .filter_map(|element| Kind::from_element(element.local_name()).map(|kind| (kind, element)))
        .collect();
    let (first_kind, first) = *drawn.first()?;
    // A bar chart says which way its bars run, and the element is the same for
    // both, so the direction decides between them.
    let kind = match first_kind {
        Kind::Column => match value_of(first, "barDir") {
            Some("bar") => Kind::Bar,
            _ => Kind::Column,
        },
        other => other,
    };
    let grouping = value_of(first, "grouping").map(Grouping::from_word).unwrap_or_default();
    let primary_axes: Vec<&str> = axis_ids(first);

    // Every series, in the order the file gives them. The categories are the
    // first series' — they are the same for all of them, and a file that
    // disagrees with itself is believed at its first word.
    let mut categories = Vec::new();
    let mut series = Vec::new();
    let mut vary_colors = false;
    let mut hole = 0u8;
    let mut group_labels: Option<Labels> = None;
    for (group_kind, element) in &drawn {
        let group_kind = match group_kind {
            Kind::Column => match value_of(element, "barDir") {
                Some("bar") => Kind::Bar,
                _ => Kind::Column,
            },
            other => *other,
        };
        let secondary = !primary_axes.is_empty() && axis_ids(element) != primary_axes;
        if is_on(element, "varyColors") {
            vary_colors = true;
        }
        if let Some(size) = value_of(element, "holeSize").and_then(|text| text.parse().ok()) {
            hole = size;
        }
        if group_labels.is_none() {
            group_labels = element.child(Some(CHART_NAMESPACE), "dLbls").map(read_labels);
        }
        for member in element.children_named(Some(CHART_NAMESPACE), "ser") {
            let mut one = read_series(member, theme);
            one.kind = (group_kind != kind).then_some(group_kind);
            one.secondary = secondary;
            if categories.is_empty() {
                categories = cached_strings(member.child(Some(CHART_NAMESPACE), "cat"));
            }
            series.push(one);
        }
    }
    if series.is_empty() {
        return None;
    }

    // The names and the numbers are written separately and may not match, so
    // there is one name per point: the extra ones are dropped and the missing
    // ones are blank.
    if !kind.plots_points() {
        let points = series.iter().map(|one| one.values.len()).max().unwrap_or(0);
        categories.resize(points, String::new());
    }

    // What is written on the points is the chart's where every series says
    // the same — or says nothing, and the group says it for them — and each
    // series' own where they differ. A series that says nothing while
    // another speaks has no labels, which is what saying nothing means.
    let all_alike = series.windows(2).all(|pair| pair[0].labels == pair[1].labels);
    let labels = if all_alike {
        series.first().and_then(|one| one.labels.clone()).or(group_labels).unwrap_or_default()
    } else {
        group_labels.unwrap_or_default()
    };
    for one in &mut series {
        let own = one.labels.take().unwrap_or_default();
        if own != labels {
            one.labels = Some(own);
        }
    }

    let legend = chart.child(Some(CHART_NAMESPACE), "legend").map(|element| Legend {
        position: value_of(element, "legendPos")
            .map_or(LegendPosition::Right, LegendPosition::from_word),
        overlay: is_on(element, "overlay"),
    });
    let data_table = plot.child(Some(CHART_NAMESPACE), "dTable").map(|element| DataTable {
        keys: is_on(element, "showKeys"),
        horizontal_lines: is_on(element, "showHorzBorder"),
        vertical_lines: is_on(element, "showVertBorder"),
        outline: is_on(element, "showOutline"),
    });

    // The axes: the ones the first kind is drawn against, which are the
    // primary ones; the first named is the categories' or the x values',
    // the second the numbers'.
    let axis_named = |id: Option<&str>| {
        plot.child_elements().find(|element| {
            element.namespace.as_deref() == Some(CHART_NAMESPACE)
                && element.local_name().ends_with("Ax")
                && value_of(element, "axId") == id
        })
    };
    let category_axis =
        axis_named(primary_axes.first().copied()).map(read_axis).unwrap_or_default();
    let value_axis = axis_named(primary_axes.get(1).copied()).map(read_axis).unwrap_or_default();

    Some(Chart {
        kind,
        grouping,
        title: chart_title(chart),
        categories,
        series,
        legend,
        labels,
        data_table,
        value_axis,
        category_axis,
        vary_colors,
        hole,
    })
}

/// The axes an element draws against, by id.
fn axis_ids(element: &Element) -> Vec<&str> {
    element
        .children_named(Some(CHART_NAMESPACE), "axId")
        .filter_map(|id| id.attribute(None, "val"))
        .collect()
}

/// The `val` of a child, which is how the format writes nearly everything.
fn value_of<'a>(parent: &'a Element, child: &str) -> Option<&'a str> {
    parent.child(Some(CHART_NAMESPACE), child)?.attribute(None, "val")
}

/// Whether a child that means yes or no means yes. A child that is not there
/// means no.
fn is_on(parent: &Element, child: &str) -> bool {
    matches!(value_of(parent, child), Some("1" | "true"))
}

fn read_series(element: &Element, theme: &Theme) -> Series {
    let name = cached_strings(element.child(Some(CHART_NAMESPACE), "tx"))
        .into_iter()
        .next()
        .unwrap_or_default();
    let (values, reference) = match element.child(Some(CHART_NAMESPACE), "val") {
        Some(values) => (cached_numbers(Some(values)), reference_of(values)),
        None => {
            let y = element.child(Some(CHART_NAMESPACE), "yVal");
            (cached_numbers(y), y.and_then(reference_of))
        }
    };
    let xs = cached_numbers(element.child(Some(CHART_NAMESPACE), "xVal"));
    let sizes = cached_numbers(element.child(Some(CHART_NAMESPACE), "bubbleSize"));
    let fill = element
        .child(Some(CHART_NAMESPACE), "spPr")
        .and_then(|properties| crate::diagram::colour_of(properties, theme));
    let points = element
        .children_named(Some(CHART_NAMESPACE), "dPt")
        .filter_map(|point| {
            let index = value_of(point, "idx")?.parse().ok()?;
            let colour = point
                .child(Some(CHART_NAMESPACE), "spPr")
                .and_then(|properties| crate::diagram::colour_of(properties, theme))?;
            Some((index, colour))
        })
        .collect();
    let labels = element.child(Some(CHART_NAMESPACE), "dLbls").map(read_labels);
    Series {
        name,
        values,
        xs,
        sizes,
        kind: None,
        secondary: false,
        fill,
        points,
        labels,
        reference,
    }
}

/// Where a reference says its numbers are in the workbook.
fn reference_of(reference: &Element) -> Option<String> {
    fn search(element: &Element) -> Option<String> {
        if element.is(Some(CHART_NAMESPACE), "f") {
            return Some(element.text_content().trim().to_owned());
        }
        element.child_elements().find_map(search)
    }
    search(reference)
}

fn read_labels(element: &Element) -> Labels {
    Labels {
        value: is_on(element, "showVal"),
        category: is_on(element, "showCatName"),
        series: is_on(element, "showSerName"),
        percent: is_on(element, "showPercent"),
        key: is_on(element, "showLegendKey"),
        leader_lines: is_on(element, "showLeaderLines"),
        number_format: element
            .child(Some(CHART_NAMESPACE), "numFmt")
            .and_then(|format| format.attribute(None, "formatCode"))
            .map(str::to_owned),
        position: value_of(element, "dLblPos").and_then(LabelPosition::from_word),
    }
}

fn read_axis(element: &Element) -> Axis {
    let scaling = element.child(Some(CHART_NAMESPACE), "scaling");
    let number = |parent: Option<&Element>, name: &str| {
        parent.and_then(|parent| value_of(parent, name)).and_then(|text| text.parse().ok())
    };
    Axis {
        min: number(scaling, "min"),
        max: number(scaling, "max"),
        major_unit: number(Some(element), "majorUnit"),
        number_format: element
            .child(Some(CHART_NAMESPACE), "numFmt")
            .and_then(|format| format.attribute(None, "formatCode"))
            .filter(|code| !code.eq_ignore_ascii_case("General"))
            .map(str::to_owned),
        deleted: is_on(element, "delete"),
    }
}

/// Fills in the numbers a chart's caches left out, from the workbook behind
/// it.
///
/// A chart may say only where its numbers are — `Sheet1!$B$2:$B$5` — and
/// leave the values to the workbook; Word reads them from there.
pub fn fill_from_workbook(chart: &mut Chart, root: &Element, sheet: &crate::workbook::Sheet) {
    let Some(plot) = root
        .child(Some(CHART_NAMESPACE), "chart")
        .and_then(|chart| chart.child(Some(CHART_NAMESPACE), "plotArea"))
    else {
        return;
    };
    let numbers = |element: Option<&Element>| -> Vec<f64> {
        element
            .and_then(reference_of)
            .map(|reference| {
                sheet
                    .range(&reference)
                    .iter()
                    .filter_map(|cell| cell.and_then(Cell::number))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut members = plot
        .child_elements()
        .filter(|element| Kind::from_element(element.local_name()).is_some())
        .flat_map(|element| element.children_named(Some(CHART_NAMESPACE), "ser"));
    let mut first_categories: Option<Vec<String>> = None;
    for series in &mut chart.series {
        let Some(element) = members.next() else { break };
        if series.values.is_empty() {
            let values = element
                .child(Some(CHART_NAMESPACE), "val")
                .or_else(|| element.child(Some(CHART_NAMESPACE), "yVal"));
            series.values = numbers(values);
        }
        if series.xs.is_empty() {
            series.xs = numbers(element.child(Some(CHART_NAMESPACE), "xVal"));
        }
        if series.sizes.is_empty() {
            series.sizes = numbers(element.child(Some(CHART_NAMESPACE), "bubbleSize"));
        }
        if series.name.is_empty() {
            if let Some(reference) =
                element.child(Some(CHART_NAMESPACE), "tx").and_then(reference_of)
            {
                if let Some(Some(cell)) = sheet.range(&reference).first() {
                    series.name = cell.text();
                }
            }
        }
        if first_categories.is_none() {
            if let Some(reference) =
                element.child(Some(CHART_NAMESPACE), "cat").and_then(reference_of)
            {
                first_categories = Some(
                    sheet
                        .range(&reference)
                        .iter()
                        .map(|cell| cell.map(|c| c.text()).unwrap_or_default())
                        .collect(),
                );
            }
        }
    }
    if chart.categories.iter().all(String::is_empty) {
        if let Some(names) = first_categories {
            chart.categories = names;
            let points = chart.points();
            chart.categories.resize(points, String::new());
        }
    }
}

/// Whether any of a chart's numbers were left to the workbook.
#[must_use]
pub fn needs_workbook(chart: &Chart) -> bool {
    chart.series.iter().any(|series| {
        series.values.is_empty()
            || (matches!(series.kind, Some(Kind::Scatter | Kind::Bubble)) && series.xs.is_empty())
            || series.name.is_empty()
    }) || (chart.kind.plots_points() && chart.series.iter().any(|series| series.xs.is_empty()))
        || (!chart.kind.plots_points() && chart.categories.iter().all(String::is_empty))
}

/// The heading over a chart, if it has one.
fn chart_title(chart: &Element) -> String {
    let Some(title) = chart.child(Some(CHART_NAMESPACE), "title") else {
        return String::new();
    };
    // The words are inside a rich-text body, which is drawing markup rather
    // than chart markup — so the text is gathered rather than navigated to.
    title.text_content().trim().to_owned()
}

/// The cached names of a reference.
fn cached_strings(reference: Option<&Element>) -> Vec<String> {
    let Some(reference) = reference else { return Vec::new() };
    points(reference, "strCache").into_iter().map(|(_, text)| text).collect()
}

/// And its cached numbers.
fn cached_numbers(reference: Option<&Element>) -> Vec<f64> {
    let Some(reference) = reference else { return Vec::new() };
    points(reference, "numCache")
        .into_iter()
        .filter_map(|(_, text)| text.trim().parse().ok())
        .collect()
}

/// The points of a cache, in the order their indices give.
fn points(reference: &Element, cache: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    collect_cache(reference, cache, &mut found);
    found.sort_by_key(|(index, _)| *index);
    found
}

fn collect_cache(element: &Element, cache: &str, out: &mut Vec<(usize, String)>) {
    if element.is(Some(CHART_NAMESPACE), cache) {
        for point in element.children_named(Some(CHART_NAMESPACE), "pt") {
            let index = point
                .attribute(None, "idx")
                .and_then(|text| text.parse().ok())
                .unwrap_or(out.len());
            let value = point
                .child(Some(CHART_NAMESPACE), "v")
                .map(Element::text_content)
                .unwrap_or_default();
            out.push((index, value));
        }
        return;
    }
    for child in element.child_elements() {
        collect_cache(child, cache, out);
    }
}

impl crate::Document {
    /// Puts a chart at the caret, adding its part to the package — and the
    /// workbook behind it, which is what Word opens to edit the data.
    ///
    /// The size is in English metric units, the same as a picture's: deciding
    /// how much room a chart gets belongs to whoever knows how wide the text
    /// is, which is not this layer.
    pub fn insert_chart(
        &mut self,
        chart: &Chart,
        width_emu: i64,
        height_emu: i64,
    ) -> Result<bool, crate::Error> {
        if chart.is_empty() {
            return Ok(false);
        }

        // A name nothing else in the package has, and the same number for
        // the workbook.
        let mut index = 1usize;
        let (name, workbook_name) = loop {
            let candidate = format!("word/charts/chart{index}.xml");
            let workbook = format!("word/embeddings/Microsoft_Excel_Worksheet{index}.xlsx");
            if self.package().part(&candidate).is_none() && self.package().part(&workbook).is_none()
            {
                break (candidate, workbook);
            }
            index += 1;
        };

        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);

        // The workbook first, so the chart can point at it.
        self.package_mut().add_part(
            &workbook_name,
            crate::workbook::WORKBOOK_CONTENT_TYPE,
            crate::workbook::write_workbook(chart),
        );
        let mut relationships = wp_opc::Relationships::new(&name);
        let workbook_id = relationships
            .add(
                crate::workbook::PACKAGE_RELATIONSHIP,
                &format!("../embeddings/Microsoft_Excel_Worksheet{index}.xlsx"),
                wp_opc::TargetMode::Internal,
            )
            .id
            .clone();
        self.package_mut().set_relationships(&relationships)?;
        self.add_chart_part(&name, chart_xml(chart, Some(&workbook_id)));

        let id = self.point_at_chart(&name)?;
        let prefix = self.prefix();
        let drawing = chart_drawing(&id, width_emu, height_emu, prefix.as_deref());
        let inserted = crate::position::insert_element_at(
            &mut self.tree_mut().root,
            caret,
            drawing,
            prefix.as_deref(),
        );

        if inserted {
            self.set_caret(crate::TextPosition::new(caret.paragraph, caret.offset + 1));
            self.mark_modified();
        }
        Ok(inserted)
    }

    /// The chart a relationship points at, if it points at one: its colours
    /// resolved against the document's theme, and any numbers its caches
    /// left out read from the workbook behind it.
    #[must_use]
    pub fn chart(&self, relationship: &str) -> Option<Chart> {
        let target = self.relationship_target(relationship)?;
        let text = self.package().xml_part(&target).and_then(Result::ok)?;
        let tree = wp_xml::tree::XmlTree::parse(&text).ok()?;
        let mut chart = read_chart_with(&tree.root, &self.theme())?;
        if needs_workbook(&chart) {
            if let Some(sheet) = self.chart_workbook(&target) {
                fill_from_workbook(&mut chart, &tree.root, &sheet);
            }
        }
        Some(chart)
    }

    /// The first sheet of the workbook behind a chart part, if it has one.
    fn chart_workbook(&self, part: &str) -> Option<crate::workbook::Sheet> {
        let relationships = self.package().relationships(part).ok()?;
        let found = relationships.single_by_type(crate::workbook::PACKAGE_RELATIONSHIP)?;
        let target = found.resolved_target(part)?.ok()?;
        let bytes = self.package().part(&target)?;
        crate::workbook::read_first_sheet(bytes)
    }
}

/// The drawing that puts a chart in the line of text.
#[must_use]
fn chart_drawing(
    relationship: &str,
    width_emu: i64,
    height_emu: i64,
    prefix: Option<&str>,
) -> Element {
    let mut drawing =
        Element::new(&crate::edit::name_with(prefix, "drawing"), Some(crate::read::W));

    let mut inline = Element::new("wp:inline", Some(crate::edit::DRAWING_WORDPROCESSING));
    inline
        .declarations
        .push((Some("wp".to_owned()), crate::edit::DRAWING_WORDPROCESSING.to_owned()));
    for side in ["distT", "distB", "distL", "distR"] {
        inline.set_attribute(side, "0");
    }

    let mut extent = Element::new("wp:extent", Some(crate::edit::DRAWING_WORDPROCESSING));
    extent.set_attribute("cx", &width_emu.to_string());
    extent.set_attribute("cy", &height_emu.to_string());
    inline.push_element(extent);

    let mut properties = Element::new("wp:docPr", Some(crate::edit::DRAWING_WORDPROCESSING));
    properties.set_attribute("id", "1");
    properties.set_attribute("name", "Chart 1");
    inline.push_element(properties);

    let mut graphic = Element::new("a:graphic", Some(crate::edit::DRAWING_MAIN));
    graphic.declarations.push((Some("a".to_owned()), crate::edit::DRAWING_MAIN.to_owned()));

    let mut data = Element::new("a:graphicData", Some(crate::edit::DRAWING_MAIN));
    data.set_attribute("uri", CHART_URI);

    // The whole of the chart is in its own part; this only says which.
    let mut reference = Element::new("c:chart", Some(CHART_NAMESPACE));
    reference.declarations.push((Some("c".to_owned()), CHART_NAMESPACE.to_owned()));
    reference.declarations.push((Some("r".to_owned()), crate::edit::RELATIONSHIPS.to_owned()));
    reference.set_namespaced_attribute("r:id", crate::edit::RELATIONSHIPS, relationship);
    data.push_element(reference);

    graphic.push_element(data);
    inline.push_element(graphic);
    drawing.push_element(inline);
    drawing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Chart {
        Chart::parse(Kind::Column, "Sales", "North=10; South=20; East=5")
    }

    fn parsed(xml: &str) -> Chart {
        let tree = wp_xml::tree::XmlTree::parse(xml).expect("the chart parses");
        read_chart(&tree.root).expect("a chart")
    }

    #[test]
    fn every_kind_says_what_it_is_called() {
        for kind in Kind::ALL {
            assert!(!kind.label().is_empty());
        }
    }

    #[test]
    fn a_pie_has_no_axes_and_the_rest_do() {
        assert!(!Kind::Pie.has_axes());
        assert!(!Kind::Doughnut.has_axes());
        assert!(Kind::Column.has_axes());
        assert!(Kind::Bar.has_axes());
        assert!(Kind::Line.has_axes());
        assert!(Kind::Radar.has_axes());
    }

    #[test]
    fn numbers_are_read_out_of_the_line_they_were_typed_on() {
        let chart = sample();
        assert_eq!(chart.categories, vec!["North", "South", "East"]);
        assert_eq!(chart.series[0].values, vec![10.0, 20.0, 5.0]);
        assert_eq!(chart.title, "Sales");
    }

    #[test]
    fn a_number_with_a_comma_for_a_point_is_still_a_number() {
        let chart = Chart::parse(Kind::Column, "", "a=1,5");
        assert_eq!(chart.series[0].values, vec![1.5]);
    }

    #[test]
    fn a_bare_number_is_a_bar_with_no_name() {
        let chart = Chart::parse(Kind::Column, "", "4; 5");
        assert_eq!(chart.series[0].values, vec![4.0, 5.0]);
        assert_eq!(chart.categories, vec!["", ""]);
    }

    #[test]
    fn something_that_is_not_a_number_is_left_out() {
        let chart = Chart::parse(Kind::Column, "", "a=1; b=x; c=3");
        assert_eq!(chart.series[0].values, vec![1.0, 3.0]);
        assert_eq!(chart.categories, vec!["a", "c"]);
    }

    #[test]
    fn nothing_typed_is_nothing_to_draw() {
        assert!(Chart::parse(Kind::Column, "", "").is_empty());
    }

    #[test]
    fn a_scatter_is_typed_as_x_equals_y_and_a_bubble_with_its_size() {
        let scatter = Chart::parse(Kind::Scatter, "", "1=3; 2.5=4");
        assert_eq!(scatter.series[0].xs, vec![1.0, 2.5]);
        assert_eq!(scatter.series[0].values, vec![3.0, 4.0]);
        assert!(scatter.categories.is_empty());

        let bubble = Chart::parse(Kind::Bubble, "", "1=3:10; 2=4:20");
        assert_eq!(bubble.series[0].sizes, vec![10.0, 20.0]);
        assert_eq!(bubble.series[0].values, vec![3.0, 4.0]);
    }

    #[test]
    fn the_largest_and_the_total_are_what_they_say() {
        let chart = sample();
        assert!((chart.largest() - 20.0).abs() < 0.001);
        assert!((chart.total() - 35.0).abs() < 0.001);
        assert!((chart.smallest()).abs() < 0.001);
    }

    #[test]
    fn a_chart_part_is_well_formed_xml() {
        let xml = chart_xml(&sample(), None);
        assert!(wp_xml::tree::XmlTree::parse(&xml).is_ok(), "the chart part does not parse");
    }

    #[test]
    fn every_kind_survives_being_written_and_read_back() {
        for kind in Kind::ALL {
            let mut chart = sample();
            chart.kind = *kind;
            let read = parsed(&chart_xml(&chart, None));
            assert_eq!(read.kind, *kind, "{}", kind.label());
            assert_eq!(read.series[0].values, chart.series[0].values, "{}", kind.label());
            // A chart that plots points has no categories to keep.
            if !kind.plots_points() {
                assert_eq!(read.categories, chart.categories, "{}", kind.label());
            }
        }
    }

    #[test]
    fn every_grouping_survives_being_written_and_read_back() {
        for kind in [Kind::Column, Kind::Bar, Kind::Line, Kind::Area] {
            for grouping in Grouping::ALL {
                let chart = Chart { kind, grouping: *grouping, ..sample() };
                let read = parsed(&chart_xml(&chart, None));
                assert_eq!(read.grouping, *grouping, "{} {}", kind.label(), grouping.label());
                assert_eq!(read.kind, kind);
            }
        }
    }

    #[test]
    fn a_three_dimensional_chart_is_read_as_the_flat_kind_it_pictures() {
        let xml = chart_xml(&sample(), None).replace("barChart", "bar3DChart");
        assert_eq!(parsed(&xml).kind, Kind::Column);
        let xml = chart_xml(&Chart { kind: Kind::Pie, ..sample() }, None)
            .replace("pieChart", "pie3DChart");
        assert_eq!(parsed(&xml).kind, Kind::Pie);
    }

    #[test]
    fn the_title_comes_back_with_the_chart() {
        assert_eq!(parsed(&chart_xml(&sample(), None)).title, "Sales");
    }

    #[test]
    fn a_title_with_a_character_xml_dislikes_still_reads_back() {
        let chart = Chart::parse(Kind::Column, "Profit & loss <2024>", "a=1");
        assert_eq!(parsed(&chart_xml(&chart, None)).title, "Profit & loss <2024>");
    }

    #[test]
    fn a_part_that_is_not_a_chart_is_not_read_as_one() {
        let tree = wp_xml::tree::XmlTree::parse("<hello/>").expect("parsing");
        assert_eq!(read_chart(&tree.root), None);
    }

    /// A chart of two series, read back out of what this program writes.
    fn two_series() -> Chart {
        Chart {
            kind: Kind::Column,
            title: "Sales".to_owned(),
            categories: vec!["North".to_owned(), "South".to_owned()],
            series: vec![
                Series {
                    name: "Last year".to_owned(),
                    values: vec![3.0, 5.0],
                    reference: Some("Sheet1!$B$2:$B$3".to_owned()),
                    ..Series::default()
                },
                Series {
                    name: "This year".to_owned(),
                    values: vec![4.0, 2.0],
                    reference: Some("Sheet1!$C$2:$C$3".to_owned()),
                    ..Series::default()
                },
            ],
            legend: Some(Legend::at(LegendPosition::Bottom)),
            labels: Labels::values(),
            ..Chart::default()
        }
    }

    #[test]
    fn a_chart_of_several_series_survives_being_written_and_read_back() {
        let read = parsed(&chart_xml(&two_series(), None));
        assert_eq!(read, two_series());
    }

    #[test]
    fn every_series_keeps_its_own_name_and_numbers() {
        let read = parsed(&chart_xml(&two_series(), None));
        assert_eq!(read.series.len(), 2, "both series should come back");
        assert_eq!(read.series[1].name, "This year");
        assert_eq!(read.series[1].values, vec![4.0, 2.0]);
        // And the categories are the chart's, not each series'.
        assert_eq!(read.categories, vec!["North".to_owned(), "South".to_owned()]);
    }

    #[test]
    fn the_largest_number_is_the_largest_of_all_of_them() {
        assert!((two_series().largest() - 5.0).abs() < f64::EPSILON);
        assert_eq!(two_series().points(), 2);
    }

    #[test]
    fn a_chart_with_no_key_says_so_by_saying_nothing() {
        let plain = Chart { legend: None, labels: Labels::default(), ..two_series() };
        let written = chart_xml(&plain, None);
        assert!(!written.contains("c:legend"), "a key was written for a chart with none");
        assert!(!written.contains("showVal"), "labels were written for a chart with none");

        let read = parsed(&written);
        assert_eq!(read.legend, None);
        assert!(!read.labels.shows_anything());
    }

    #[test]
    fn one_typed_line_is_still_one_series() {
        let typed = Chart::parse(Kind::Column, "Sales", "North=3; South=5");
        assert_eq!(typed.series.len(), 1);
        assert_eq!(typed.series[0].values, vec![3.0, 5.0]);
        assert_eq!(typed.legend, None);
    }

    #[test]
    fn a_typed_pie_is_given_the_key_that_names_its_slices() {
        let pie = Chart::parse(Kind::Pie, "Sales", "North=3; South=5");
        assert_eq!(pie.legend, Some(Legend::at(LegendPosition::Right)));
        assert!(pie.vary_colors);

        let column = Chart::parse(Kind::Column, "Sales", "North=3; South=5");
        assert_eq!(column.legend, None, "one column series names itself along the bottom");
    }

    #[test]
    fn the_key_keeps_its_place_at_any_side() {
        for position in [
            LegendPosition::Right,
            LegendPosition::Bottom,
            LegendPosition::Left,
            LegendPosition::Top,
            LegendPosition::TopRight,
        ] {
            let chart = Chart { legend: Some(Legend { position, overlay: true }), ..two_series() };
            let read = parsed(&chart_xml(&chart, None));
            assert_eq!(read.legend, Some(Legend { position, overlay: true }));
        }
    }

    #[test]
    fn what_a_label_says_survives_being_written_and_read_back() {
        let labels = Labels {
            value: true,
            category: true,
            series: true,
            percent: true,
            key: false,
            leader_lines: true,
            number_format: Some("0.0%".to_owned()),
            position: Some(LabelPosition::OutsideEnd),
        };
        let chart = Chart { labels: labels.clone(), ..two_series() };
        assert_eq!(parsed(&chart_xml(&chart, None)).labels, labels);
    }

    #[test]
    fn a_series_with_labels_of_its_own_keeps_them_apart_from_the_charts() {
        let mut chart = two_series();
        chart.series[1].labels = Some(Labels { percent: true, ..Labels::default() });
        let read = parsed(&chart_xml(&chart, None));
        assert_eq!(*read.labels_of(&read.series[0]), Labels::values());
        assert_eq!(*read.labels_of(&read.series[1]), Labels { percent: true, ..Labels::default() });

        // And a series that says nothing beside one that speaks has no
        // labels, rather than the other's.
        let mut chart = two_series();
        chart.labels = Labels::default();
        chart.series[1].labels = Some(Labels::values());
        let read = parsed(&chart_xml(&chart, None));
        assert!(!read.labels_of(&read.series[0]).shows_anything());
        assert!(read.labels_of(&read.series[1]).value);
    }

    #[test]
    fn the_data_table_and_the_axes_survive_being_written_and_read_back() {
        let chart = Chart {
            data_table: Some(DataTable { keys: false, ..DataTable::default() }),
            value_axis: Axis {
                min: Some(-5.0),
                max: Some(50.0),
                major_unit: Some(10.0),
                number_format: Some("\"$\"#,##0".to_owned()),
                deleted: false,
            },
            category_axis: Axis { deleted: true, ..Axis::default() },
            ..two_series()
        };
        let read = parsed(&chart_xml(&chart, None));
        assert_eq!(read.data_table, chart.data_table);
        assert_eq!(read.value_axis, chart.value_axis);
        assert!(read.category_axis.deleted);
    }

    #[test]
    fn the_colours_the_document_chose_come_back_as_hex() {
        let mut chart = two_series();
        chart.series[0].fill = Some("FF0000".to_owned());
        chart.series[1].points = vec![(1, "00FF00".to_owned())];
        let read = parsed(&chart_xml(&chart, None));
        assert_eq!(read.series[0].fill.as_deref(), Some("FF0000"));
        assert_eq!(read.series[1].fill, None);
        assert_eq!(read.series[1].points, vec![(1, "00FF00".to_owned())]);
    }

    #[test]
    fn a_colour_named_from_the_theme_is_resolved_against_it() {
        let xml = chart_xml(
            &Chart {
                series: vec![Series {
                    fill: Some("FF0000".to_owned()),
                    ..two_series().series[0].clone()
                }],
                ..two_series()
            },
            None,
        )
        .replace("<a:srgbClr val=\"FF0000\"/>", "<a:schemeClr val=\"accent2\"/>");
        let tree = wp_xml::tree::XmlTree::parse(&xml).expect("the chart parses");
        let theme = Theme::default();
        let read = read_chart_with(&tree.root, &theme).expect("a chart");
        assert_eq!(read.series[0].fill, Some(theme.color(crate::theme::Slot::Accent2)));
    }

    #[test]
    fn a_combination_chart_draws_a_series_as_another_kind_on_the_second_axis() {
        let mut chart = two_series();
        chart.series[1].kind = Some(Kind::Line);
        chart.series[1].secondary = true;
        let written = chart_xml(&chart, None);
        assert!(written.contains("<c:lineChart>"), "no line group was written");
        assert!(written.contains("<c:axId val=\"4\"/>"), "no second axis was written");
        let read = parsed(&written);
        assert_eq!(read.kind, Kind::Column);
        assert_eq!(read.series[1].kind, Some(Kind::Line));
        assert!(read.series[1].secondary, "the line lost its axis");
        assert!(!read.series[0].secondary);
    }

    #[test]
    fn a_scatter_keeps_its_x_values_and_a_bubble_its_sizes() {
        let chart = Chart {
            kind: Kind::Bubble,
            categories: Vec::new(),
            series: vec![Series {
                name: "Cities".to_owned(),
                values: vec![3.0, 5.0],
                xs: vec![1.0, 2.0],
                sizes: vec![10.0, 30.0],
                reference: Some("Sheet1!$B$2:$B$3".to_owned()),
                ..Series::default()
            }],
            ..Chart::default()
        };
        let read = parsed(&chart_xml(&chart, None));
        assert_eq!(read.series[0].xs, vec![1.0, 2.0]);
        assert_eq!(read.series[0].values, vec![3.0, 5.0]);
        assert_eq!(read.series[0].sizes, vec![10.0, 30.0]);
        assert_eq!(read.kind, Kind::Bubble);
    }

    #[test]
    fn a_doughnut_keeps_the_size_of_its_hole() {
        let chart = Chart { kind: Kind::Doughnut, hole: 40, ..two_series() };
        assert_eq!(parsed(&chart_xml(&chart, None)).hole, 40);
    }

    #[test]
    fn numbers_left_to_the_workbook_are_read_from_it() {
        let chart = two_series();
        let sheet = crate::workbook::read_first_sheet(&crate::workbook::write_workbook(&chart))
            .expect("a sheet");
        // The same chart with its caches taken out: only the references.
        let mut stripped = chart_xml(&chart, None);
        while let Some(start) = stripped.find("<c:pt idx=") {
            let end = stripped[start..].find("</c:pt>").expect("a point ends") + start + 7;
            stripped.replace_range(start..end, "");
        }
        let tree = wp_xml::tree::XmlTree::parse(&stripped).expect("the chart parses");
        let mut read = read_chart(&tree.root).expect("a chart");
        assert!(read.series[0].values.is_empty(), "the caches were not taken out");
        assert!(needs_workbook(&read));

        fill_from_workbook(&mut read, &tree.root, &sheet);
        assert_eq!(read.series[0].values, vec![3.0, 5.0]);
        assert_eq!(read.series[1].values, vec![4.0, 2.0]);
        assert_eq!(read.series[1].name, "This year");
        assert_eq!(read.categories, vec!["North", "South"]);
    }
}

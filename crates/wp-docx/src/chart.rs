//! Charts: a few numbers drawn as columns, bars, a line or a pie.
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
//! # What is here and what is not
//!
//! One series of numbers with a name for each, drawn four ways. Not several
//! series, not a second axis, not the sixty other chart types: each of those is
//! its own element with its own rules, and the four here are the four that most
//! charts in most documents are.
//!
//! The numbers are written in full — Word calls them the cached values — so the
//! chart draws without the spreadsheet Word normally embeds beside it. A chart
//! with no spreadsheet cannot have its data edited in Word's own grid, but it
//! draws, prints and reads correctly everywhere.

use wp_xml::tree::Element;

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
}

impl Kind {
    pub const ALL: &'static [Self] = &[Self::Column, Self::Bar, Self::Line, Self::Pie];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Column => "Column",
            Self::Bar => "Bar",
            Self::Line => "Line",
            Self::Pie => "Pie",
        }
    }

    /// The element the chart is drawn by.
    #[must_use]
    fn element(self) -> &'static str {
        match self {
            Self::Column | Self::Bar => "barChart",
            Self::Line => "lineChart",
            Self::Pie => "pieChart",
        }
    }

    /// Which way the bars run, for the kinds that have bars.
    #[must_use]
    fn direction(self) -> Option<&'static str> {
        match self {
            Self::Column => Some("col"),
            Self::Bar => Some("bar"),
            Self::Line | Self::Pie => None,
        }
    }

    /// Whether the chart has axes drawn round it.
    #[must_use]
    pub fn has_axes(self) -> bool {
        self != Self::Pie
    }
}

/// A chart: what it is called, what it counts, and how it is drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chart {
    pub kind: Kind,
    /// The heading over the chart. Empty for none.
    pub title: String,
    /// What the series is called, which is what a legend would say.
    pub series: String,
    /// The names along the bottom, and the numbers they stand for. The two are
    /// the same length: a category with no number is not a category.
    pub categories: Vec<String>,
    pub values: Vec<f64>,
}

impl Chart {
    /// Reads a chart out of one line typed as `name=value; name=value`.
    #[must_use]
    pub fn parse(kind: Kind, title: &str, typed: &str) -> Self {
        let mut categories = Vec::new();
        let mut values = Vec::new();
        for piece in typed.split(';') {
            let piece = piece.trim();
            if piece.is_empty() {
                continue;
            }
            let (name, number) = match piece.split_once('=') {
                Some((name, number)) => (name.trim().to_owned(), number.trim()),
                // A bare number is a bar with no name, which is better than
                // throwing the number away.
                None => (String::new(), piece),
            };
            let Ok(value) = number.replace(',', ".").parse::<f64>() else { continue };
            categories.push(name);
            values.push(value);
        }
        Self {
            kind,
            title: title.trim().to_owned(),
            series: "Series 1".to_owned(),
            categories,
            values,
        }
    }

    /// Whether there is anything to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The largest number in it, which is what the axis has to reach.
    #[must_use]
    pub fn largest(&self) -> f64 {
        self.values.iter().copied().fold(0.0, f64::max)
    }

    /// And the total, which is what a pie divides up.
    #[must_use]
    pub fn total(&self) -> f64 {
        self.values.iter().sum()
    }
}

/// Builds the whole of a chart part.
#[must_use]
pub fn chart_xml(chart: &Chart) -> String {
    let mut out = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    out.push('\n');
    out.push_str(&format!(
        r#"<c:chartSpace xmlns:c="{CHART_NAMESPACE}" xmlns:a="{main}" xmlns:r="{rel}">"#,
        main = crate::edit::DRAWING_MAIN,
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
    out.push_str(&format!("<c:{}>", chart.kind.element()));
    if let Some(direction) = chart.kind.direction() {
        out.push_str(&format!("<c:barDir val=\"{direction}\"/>"));
        out.push_str("<c:grouping val=\"clustered\"/>");
    }
    out.push_str("<c:varyColors val=\"0\"/>");
    out.push_str(&series_xml(chart));
    if chart.kind.has_axes() {
        out.push_str("<c:axId val=\"1\"/><c:axId val=\"2\"/>");
    }
    out.push_str(&format!("</c:{}>", chart.kind.element()));

    if chart.kind.has_axes() {
        // The names along one axis and the numbers up the other.
        out.push_str(
            "<c:catAx><c:axId val=\"1\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling>\
             <c:delete val=\"0\"/><c:axPos val=\"b\"/><c:crossAx val=\"2\"/></c:catAx>\
             <c:valAx><c:axId val=\"2\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling>\
             <c:delete val=\"0\"/><c:axPos val=\"l\"/><c:crossAx val=\"1\"/></c:valAx>",
        );
    }
    out.push_str("</c:plotArea><c:plotVisOnly val=\"1\"/></c:chart></c:chartSpace>");
    out
}

/// The one series: its name, its categories and its numbers.
fn series_xml(chart: &Chart) -> String {
    let mut out = String::from("<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>");
    out.push_str("<c:tx><c:strRef><c:f>Sheet1!$B$1</c:f><c:strCache><c:ptCount val=\"1\"/>");
    out.push_str(&format!("<c:pt idx=\"0\"><c:v>{}</c:v></c:pt>", escape(&chart.series)));
    out.push_str("</c:strCache></c:strRef></c:tx>");

    // The names, written out in full so the chart draws without the workbook.
    out.push_str("<c:cat><c:strRef><c:f>Sheet1!$A$2:$A$");
    out.push_str(&(chart.categories.len() + 1).to_string());
    out.push_str("</c:f><c:strCache>");
    out.push_str(&format!("<c:ptCount val=\"{}\"/>", chart.categories.len()));
    for (index, name) in chart.categories.iter().enumerate() {
        out.push_str(&format!("<c:pt idx=\"{index}\"><c:v>{}</c:v></c:pt>", escape(name)));
    }
    out.push_str("</c:strCache></c:strRef></c:cat>");

    out.push_str("<c:val><c:numRef><c:f>Sheet1!$B$2:$B$");
    out.push_str(&(chart.values.len() + 1).to_string());
    out.push_str("</c:f><c:numCache><c:formatCode>General</c:formatCode>");
    out.push_str(&format!("<c:ptCount val=\"{}\"/>", chart.values.len()));
    for (index, value) in chart.values.iter().enumerate() {
        out.push_str(&format!("<c:pt idx=\"{index}\"><c:v>{value}</c:v></c:pt>"));
    }
    out.push_str("</c:numCache></c:numRef></c:val></c:ser>");
    out
}

/// The five characters XML will not take as themselves.
fn escape(text: &str) -> String {
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

/// Reads a chart back out of a chart part.
#[must_use]
pub fn read_chart(root: &Element) -> Option<Chart> {
    let chart = root.child(Some(CHART_NAMESPACE), "chart")?;
    let plot = chart.child(Some(CHART_NAMESPACE), "plotArea")?;

    let (kind, drawn) = Kind::ALL.iter().find_map(|kind| {
        plot.child(Some(CHART_NAMESPACE), kind.element()).map(|element| (*kind, element))
    })?;
    // A bar chart says which way its bars run, and the element is the same for
    // both, so the direction decides between them.
    let kind = match drawn
        .child(Some(CHART_NAMESPACE), "barDir")
        .and_then(|element| element.attribute(None, "val"))
    {
        Some("bar") => Kind::Bar,
        Some("col") => Kind::Column,
        _ => kind,
    };

    let series = drawn.child(Some(CHART_NAMESPACE), "ser")?;
    let categories = cached_strings(series.child(Some(CHART_NAMESPACE), "cat"));
    let values = cached_numbers(series.child(Some(CHART_NAMESPACE), "val"));
    let name = cached_strings(series.child(Some(CHART_NAMESPACE), "tx"))
        .into_iter()
        .next()
        .unwrap_or_default();

    // The names and the numbers are written separately and may not match, so
    // there is one name per number: the extra ones are dropped and the missing
    // ones are blank.
    let mut categories = categories;
    categories.resize(values.len(), String::new());

    Some(Chart { kind, title: chart_title(chart), series: name, categories, values })
}

/// The heading over a chart, if it has one.
fn chart_title(chart: &Element) -> String {
    let Some(title) = chart.child(Some(CHART_NAMESPACE), "title") else {
        return String::new();
    };
    // The words are inside a rich-text body, which is drawing markup rather
    // than chart markup — so the text is gathered rather than navigated to.
    text_within(title)
}

/// Every scrap of text under an element, in order.
fn text_within(element: &Element) -> String {
    // `text_content` already gathers what is under it, so nothing is walked
    // here: doing both would give the words back once for each level.
    element.text_content().trim().to_owned()
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
    /// Puts a chart at the caret, adding its part to the package.
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

        // A name nothing else in the package has.
        let mut index = 1usize;
        let name = loop {
            let candidate = format!("word/charts/chart{index}.xml");
            if self.package().part(&candidate).is_none() {
                break candidate;
            }
            index += 1;
        };

        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);
        self.add_chart_part(&name, chart_xml(chart));

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

    /// The chart a relationship points at, if it points at one.
    #[must_use]
    pub fn chart(&self, relationship: &str) -> Option<Chart> {
        let target = self.relationship_target(relationship)?;
        let text = self.package().xml_part(&target).and_then(Result::ok)?;
        let tree = wp_xml::tree::XmlTree::parse(&text).ok()?;
        read_chart(&tree.root)
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

    #[test]
    fn every_kind_says_what_it_is_called() {
        for kind in Kind::ALL {
            assert!(!kind.label().is_empty());
        }
    }

    #[test]
    fn a_pie_has_no_axes_and_the_rest_do() {
        assert!(!Kind::Pie.has_axes());
        assert!(Kind::Column.has_axes());
        assert!(Kind::Bar.has_axes());
        assert!(Kind::Line.has_axes());
    }

    #[test]
    fn numbers_are_read_out_of_the_line_they_were_typed_on() {
        let chart = sample();
        assert_eq!(chart.categories, vec!["North", "South", "East"]);
        assert_eq!(chart.values, vec![10.0, 20.0, 5.0]);
        assert_eq!(chart.title, "Sales");
    }

    #[test]
    fn a_number_with_a_comma_for_a_point_is_still_a_number() {
        let chart = Chart::parse(Kind::Column, "", "a=1,5");
        assert_eq!(chart.values, vec![1.5]);
    }

    #[test]
    fn a_bare_number_is_a_bar_with_no_name() {
        let chart = Chart::parse(Kind::Column, "", "4; 5");
        assert_eq!(chart.values, vec![4.0, 5.0]);
        assert_eq!(chart.categories, vec!["", ""]);
    }

    #[test]
    fn something_that_is_not_a_number_is_left_out() {
        let chart = Chart::parse(Kind::Column, "", "a=1; b=hello; c=3");
        assert_eq!(chart.values, vec![1.0, 3.0]);
        assert_eq!(chart.categories, vec!["a", "c"]);
    }

    #[test]
    fn nothing_typed_is_nothing_to_draw() {
        assert!(Chart::parse(Kind::Column, "", "").is_empty());
    }

    #[test]
    fn the_largest_and_the_total_are_what_they_say() {
        let chart = sample();
        assert!((chart.largest() - 20.0).abs() < 0.001);
        assert!((chart.total() - 35.0).abs() < 0.001);
    }

    #[test]
    fn a_chart_part_is_well_formed_xml() {
        let xml = chart_xml(&sample());
        wp_xml::tree::XmlTree::parse(&xml).expect("the chart part should parse");
    }

    #[test]
    fn every_kind_survives_being_written_and_read_back() {
        for kind in Kind::ALL {
            let mut chart = sample();
            chart.kind = *kind;
            let xml = chart_xml(&chart);
            let tree = wp_xml::tree::XmlTree::parse(&xml).expect("parsing");
            let read = read_chart(&tree.root).expect("a chart");
            assert_eq!(read.kind, *kind, "{}", kind.label());
            assert_eq!(read.values, chart.values, "{}", kind.label());
            assert_eq!(read.categories, chart.categories, "{}", kind.label());
        }
    }

    #[test]
    fn the_title_comes_back_with_the_chart() {
        let xml = chart_xml(&sample());
        let tree = wp_xml::tree::XmlTree::parse(&xml).expect("parsing");
        assert_eq!(read_chart(&tree.root).expect("a chart").title, "Sales");
    }

    #[test]
    fn a_title_with_a_character_xml_dislikes_still_reads_back() {
        let chart = Chart::parse(Kind::Column, "Profit & loss <2024>", "a=1");
        let xml = chart_xml(&chart);
        let tree = wp_xml::tree::XmlTree::parse(&xml).expect("parsing");
        assert_eq!(read_chart(&tree.root).expect("a chart").title, "Profit & loss <2024>");
    }

    #[test]
    fn a_part_that_is_not_a_chart_is_not_read_as_one() {
        let tree = wp_xml::tree::XmlTree::parse("<hello/>").expect("parsing");
        assert_eq!(read_chart(&tree.root), None);
    }
}

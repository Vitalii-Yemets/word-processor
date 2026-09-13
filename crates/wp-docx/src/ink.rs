//! Ink: what a document holds when somebody drew on it.
//!
//! # Where ink lives
//!
//! Not in the document part. A run holds `w14:contentPart`, which says almost
//! nothing — a relationship, and sometimes a box to draw in — and the ink
//! itself is a part of its own, written in InkML, which is a W3C format and
//! not Microsoft's. So a document with ink in it holds a piece of somebody
//! else's format, and this reads that.
//!
//! # How a stroke is written
//!
//! As numbers that mostly say how far the pen moved rather than where it is.
//! A trace begins with a point stated outright and goes on in differences:
//! `1439 5623 0,'-4'-1 0,0 0 0` is three points, the second four units left
//! and one up from the first, the third in the same place as the second. The
//! prefix says which kind of number follows — `!` outright, `'` a difference,
//! `"` a difference of differences — and a number with no prefix goes on in
//! whatever kind the last one for that channel was. That last rule is the
//! whole of it: without it every unprefixed number reads as an absolute
//! position and a line of handwriting comes out as a scribble round the
//! origin.
//!
//! The prefix is also a separator, which is why `'-4'-1` is two numbers and
//! not one.
//!
//! # The units
//!
//! Himetric — hundredths of a millimetre — which is what a pen reports and
//! what Word writes. There are 360 English metric units to one of them, and
//! the whole of the ink is turned into those on the way in, because that is
//! what the rest of this program measures a drawing in.

use wp_xml::tree::Element;

use crate::Document;

/// The namespace ink is written in. The W3C's, not Microsoft's.
pub const INKML: &str = "http://www.w3.org/2003/InkML";
/// The namespace the run's part reference is written in.
pub const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
/// What the package calls an ink part.
pub const INK_CONTENT_TYPE: &str = "application/inkml+xml";

/// What the package calls the relationship that reaches an ink part.
///
/// The one Word writes for a content part. Nothing read here depends on it —
/// a relationship is followed by its id, and what kind it is never comes into
/// it — but Word is the reader that does depend on it.
pub const INK_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml";

/// English metric units to one himetric, which is a hundredth of a millimetre.
const EMU_PER_HIMETRIC: f64 = 360.0;

/// One stroke of the pen, from the moment it went down to the moment it came
/// up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stroke {
    /// Six hex digits.
    pub colour: String,
    /// How thick the pen was, in English metric units.
    pub width_emu: i64,
    /// Nothing at all is 0 and invisible is 255. A highlighter is somewhere in
    /// between, which is what lets the words show through it.
    pub transparency: u8,
    /// Whether the tip is a rectangle, which is what a highlighter has and a
    /// pen does not.
    pub flat: bool,
    /// Where the pen went, in English metric units in the ink's own space.
    pub points: Vec<(i64, i64)>,
}

/// Everything somebody drew in one go.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ink {
    pub strokes: Vec<Stroke>,
}

impl Ink {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strokes.iter().all(|stroke| stroke.points.len() < 2)
    }

    /// The rectangle the ink covers, in its own space: left, top, right,
    /// bottom.
    ///
    /// The pen's own width is counted in, because a stroke is as wide as the
    /// pen and not as wide as the line through the middle of it.
    #[must_use]
    pub fn bounds(&self) -> Option<(i64, i64, i64, i64)> {
        let mut bounds: Option<(i64, i64, i64, i64)> = None;
        for stroke in &self.strokes {
            let half = stroke.width_emu / 2;
            for (x, y) in &stroke.points {
                bounds = Some(match bounds {
                    None => (x - half, y - half, x + half, y + half),
                    Some((left, top, right, bottom)) => (
                        left.min(x - half),
                        top.min(y - half),
                        right.max(x + half),
                        bottom.max(y + half),
                    ),
                });
            }
        }
        bounds
    }
}

impl Document {
    /// Puts ink at the caret, in a part of its own.
    ///
    /// How big it is drawn is not asked for: ink is as big as what was drawn,
    /// and the pen was somewhere when it drew it. Returns whether anything
    /// went in, which is false for ink with no stroke in it.
    pub fn insert_ink(&mut self, ink: &Ink) -> Result<bool, crate::Error> {
        let Some((left, top, right, bottom)) = ink.bounds().filter(|_| !ink.is_empty()) else {
            return Ok(false);
        };

        // A name nothing else in the package has.
        let mut index = 1usize;
        let name = loop {
            let candidate = format!("word/ink/ink{index}.xml");
            if self.package().part(&candidate).is_none() {
                break candidate;
            }
            index += 1;
        };

        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);
        self.add_package_part(&name, INK_CONTENT_TYPE, ink_xml(ink));
        let id = self.point_at_part(&name, INK_RELATIONSHIP)?;

        // The run that points at it is an extension, so the document has to
        // say that a reader which does not know it may pass it over.
        crate::edit::declare_extension(&mut self.tree_mut().root, "w14", W14);

        let prefix = self.prefix();
        let part = content_part_element(&id, right - left, bottom - top);
        let inserted = crate::position::insert_element_at(
            &mut self.tree_mut().root,
            caret,
            part,
            prefix.as_deref(),
        );

        if inserted {
            self.set_caret(crate::TextPosition::new(caret.paragraph, caret.offset + 1));
            self.mark_modified();
        }
        Ok(inserted)
    }

    /// The ink a run points at, if the package holds it.
    #[must_use]
    pub fn ink(&self, relationship: &str) -> Option<Ink> {
        let target = self.relationship_target(relationship)?;
        let text = self.package().xml_part(&target).and_then(Result::ok)?;
        let tree = wp_xml::tree::XmlTree::parse(&text).ok()?;
        let ink = read_ink(&tree.root);
        (!ink.is_empty()).then_some(ink)
    }
}

/// What a pen was set to.
#[derive(Clone, Debug)]
struct Brush {
    colour: String,
    width_emu: i64,
    transparency: u8,
    flat: bool,
}

impl Default for Brush {
    fn default() -> Self {
        // What Word draws with when the file says nothing: a black pen a
        // little under a point wide.
        Self { colour: "000000".to_owned(), width_emu: 9_525, transparency: 0, flat: false }
    }
}

/// Reads the ink out of an InkML part.
#[must_use]
pub fn read_ink(root: &Element) -> Ink {
    let mut brushes: Vec<(String, Brush)> = Vec::new();
    collect_brushes(root, &mut brushes);

    let mut contexts: Vec<(String, Vec<Channel>)> = Vec::new();
    collect_contexts(root, &mut contexts);

    let mut traces = Vec::new();
    collect_traces(root, &mut traces);

    let mut strokes = Vec::new();
    for trace in traces {
        let brush = reference(trace, "brushRef")
            .and_then(|id| brushes.iter().find(|(name, _)| *name == id))
            .map(|(_, brush)| brush.clone())
            // One brush and no name is still that brush: a file with a single
            // pen in it need not say which pen each stroke used.
            .or_else(|| (brushes.len() == 1).then(|| brushes[0].1.clone()))
            .unwrap_or_default();

        let channels = reference(trace, "contextRef")
            .and_then(|id| contexts.iter().find(|(name, _)| *name == id))
            .map(|(_, channels)| channels.clone())
            .or_else(|| contexts.first().map(|(_, channels)| channels.clone()))
            .unwrap_or_default();

        let points = decode(&trace.text_content(), &channels);
        if points.len() < 2 {
            continue;
        }
        strokes.push(Stroke {
            colour: brush.colour.clone(),
            width_emu: brush.width_emu,
            transparency: brush.transparency,
            flat: brush.flat,
            points,
        });
    }
    Ink { strokes }
}

/// What a channel of a trace holds, and what it is measured in.
#[derive(Clone, Debug)]
struct Channel {
    name: String,
    /// English metric units to one of whatever this channel counts in.
    scale: f64,
}

/// Every brush anywhere in the part, by the name strokes call it.
fn collect_brushes(element: &Element, out: &mut Vec<(String, Brush)>) {
    if element.local_name() == "brush" {
        let mut brush = Brush::default();
        for property in element.child_elements() {
            if property.local_name() != "brushProperty" {
                continue;
            }
            let (Some(name), Some(value)) =
                (property.attribute(None, "name"), property.attribute(None, "value"))
            else {
                continue;
            };
            let units = property.attribute(None, "units").unwrap_or("himetric");
            match name {
                "color" => brush.colour = value.trim_start_matches('#').to_uppercase(),
                // The pen is as wide as its width, and a pen with a height too
                // is a chisel tip — the narrower of the two is what a line
                // drawn with it is worth at its thinnest.
                "width" | "height" => {
                    let Ok(number) = value.trim().parse::<f64>() else { continue };
                    let emu = (number * measure(units)) as i64;
                    if name == "width" || emu < brush.width_emu {
                        brush.width_emu = emu.max(1);
                    }
                }
                "transparency" => {
                    brush.transparency = value.trim().parse::<u8>().unwrap_or(0);
                }
                "tip" => brush.flat = value != "ellipse",
                // A highlighter says so twice: by its tip, and by drawing with
                // a raster operation that leaves what is under it showing.
                "rasterOp" if value == "maskPen" && brush.transparency == 0 => {
                    brush.transparency = 128;
                }
                _ => {}
            }
        }
        out.push((identifier(element).unwrap_or_default(), brush));
        return;
    }
    for child in element.child_elements() {
        collect_brushes(child, out);
    }
}

/// Every context, with the channels its traces are written in.
fn collect_contexts(element: &Element, out: &mut Vec<(String, Vec<Channel>)>) {
    if element.local_name() == "context" {
        let mut channels = Vec::new();
        gather_channels(element, &mut channels);
        out.push((identifier(element).unwrap_or_default(), channels));
        return;
    }
    for child in element.child_elements() {
        collect_contexts(child, out);
    }
}

/// The channels of a trace format, in the order the numbers come in.
fn gather_channels(element: &Element, out: &mut Vec<Channel>) {
    for child in element.child_elements() {
        if child.local_name() == "channel" {
            out.push(Channel {
                name: child.attribute(None, "name").unwrap_or_default().to_owned(),
                scale: measure(child.attribute(None, "units").unwrap_or("himetric")),
            });
        } else {
            gather_channels(child, out);
        }
    }
}

/// Every trace anywhere in the part, including the ones inside a group.
fn collect_traces<'a>(element: &'a Element, out: &mut Vec<&'a Element>) {
    for child in element.child_elements() {
        if child.local_name() == "trace" {
            out.push(child);
        } else {
            collect_traces(child, out);
        }
    }
}

/// What an element calls itself. `xml:id`, which is the attribute the whole of
/// InkML names things with.
fn identifier(element: &Element) -> Option<String> {
    element
        .attribute_by_name("xml:id")
        .or_else(|| element.attribute(Some("http://www.w3.org/XML/1998/namespace"), "id"))
        .or_else(|| element.attribute(None, "id"))
        .map(str::to_owned)
}

/// What one of `brushRef` and `contextRef` points at, without its hash.
fn reference(element: &Element, name: &str) -> Option<String> {
    element.attribute(None, name).map(|value| value.trim_start_matches('#').to_owned())
}

/// English metric units to one of whatever a channel counts in.
fn measure(units: &str) -> f64 {
    match units {
        "in" => 914_400.0,
        "cm" => 360_000.0,
        "mm" => 36_000.0,
        // A pen that reports in its own units says nothing about how big they
        // are, and himetric is what every such pen this format was written for
        // reports in.
        _ => EMU_PER_HIMETRIC,
    }
}

/// The points of one trace, in English metric units.
fn decode(text: &str, channels: &[Channel]) -> Vec<(i64, i64)> {
    // The first two channels when the file names none: every trace format
    // written by anything begins with where the pen was.
    let across = channels.iter().position(|channel| channel.name == "X").unwrap_or(0);
    let down = channels.iter().position(|channel| channel.name == "Y").unwrap_or(1);
    let scale =
        |index: usize| channels.get(index).map_or(EMU_PER_HIMETRIC, |channel| channel.scale);
    let (across_scale, down_scale) = (scale(across), scale(down));

    let mut state: Vec<Value> = Vec::new();
    let mut points = Vec::new();
    for piece in text.split(',') {
        let numbers = values(piece);
        if numbers.is_empty() {
            continue;
        }
        if state.len() < numbers.len() {
            state.resize(numbers.len(), Value::default());
        }
        for (index, number) in numbers.iter().enumerate() {
            state[index].take(number);
        }
        let (Some(x), Some(y)) = (state.get(across), state.get(down)) else { continue };
        points.push(((x.value * across_scale) as i64, (y.value * down_scale) as i64));
    }
    points
}

/// One channel's running value: what it is now, how it last changed, and which
/// kind of number it is being told in.
#[derive(Clone, Copy, Debug, Default)]
struct Value {
    value: f64,
    delta: f64,
    mode: Mode,
}

/// What a number with no prefix in front of it means.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    /// Where the pen is.
    #[default]
    Outright,
    /// How far it moved.
    Difference,
    /// How much that movement changed, which is how a steady stroke is written
    /// in almost nothing at all.
    SecondDifference,
}

impl Value {
    /// Reads one number of a trace, with whatever prefix it carries.
    fn take(&mut self, token: &str) {
        let (mode, digits) = match token.chars().next() {
            Some('!') => (Some(Mode::Outright), &token[1..]),
            Some('\'') => (Some(Mode::Difference), &token[1..]),
            Some('"') => (Some(Mode::SecondDifference), &token[1..]),
            // A number nobody recorded leaves the channel where it was: the
            // pen was somewhere, the file simply does not say where.
            Some('?') => return,
            // A prefix this does not know still marks a number, and the number
            // is the part after it.
            Some('*') => (None, &token[1..]),
            _ => (None, token),
        };
        if let Some(mode) = mode {
            self.mode = mode;
        }
        let Ok(number) = digits.trim().parse::<f64>() else { return };

        match self.mode {
            Mode::Outright => {
                self.value = number;
                self.delta = 0.0;
            }
            Mode::Difference => {
                self.delta = number;
                self.value += number;
            }
            Mode::SecondDifference => {
                self.delta += number;
                self.value += self.delta;
            }
        }
    }
}

/// The numbers of one point, each with the prefix it came with.
///
/// The prefix separates as well as saying what kind of number follows, so
/// `'-4'-1` is two numbers: a space between them is allowed and not required.
fn values(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (at, character) in text.char_indices() {
        match character {
            '!' | '\'' | '"' | '?' | '*' => {
                if let Some(from) = start.take() {
                    out.push(text[from..at].trim());
                }
                start = Some(at);
            }
            character if character.is_whitespace() => {
                if let Some(from) = start.take() {
                    out.push(text[from..at].trim());
                }
            }
            _ => {
                if start.is_none() {
                    start = Some(at);
                }
            }
        }
    }
    if let Some(from) = start {
        out.push(text[from..].trim());
    }
    out.retain(|token| !token.is_empty());
    out
}

/// Reads what a run says about the ink it holds.
///
/// `w14:contentPart`: which part the ink is in, and how big it is drawn. The
/// box is not always there — a run may point at ink and say nothing about its
/// size, and then the ink is as big as what was drawn.
#[must_use]
pub fn read_reference(element: &Element) -> Option<crate::model::InkReference> {
    let relationship = element
        .attribute(Some(crate::edit::RELATIONSHIPS), "id")
        .or_else(|| element.attribute_by_name("r:id"))?
        .to_owned();

    let mut reference = crate::model::InkReference {
        relationship,
        width_emu: 0,
        height_emu: 0,
        name: String::new(),
        description: String::new(),
    };

    if let Some(transform) = element.child_elements().find(|child| child.local_name() == "xfrm") {
        if let Some(extent) = transform.child_elements().find(|child| child.local_name() == "ext") {
            let number = |name: &str| {
                extent
                    .attribute(None, name)
                    .and_then(|value| value.trim().parse().ok())
                    .unwrap_or(0)
            };
            reference.width_emu = number("cx");
            reference.height_emu = number("cy");
        }
    }
    if let Some(properties) =
        element.child_elements().find(|child| child.local_name() == "nvContentPartPr")
    {
        if let Some(visible) =
            properties.child_elements().find(|child| child.local_name() == "cNvPr")
        {
            reference.name = visible.attribute_by_name("name").unwrap_or_default().to_owned();
            reference.description =
                visible.attribute_by_name("descr").unwrap_or_default().to_owned();
        }
    }
    Some(reference)
}

/// The ink part: the pens, and where each of them went.
///
/// Every number is written outright. The format allows differences, and Word
/// writes them because a stroke is hundreds of points and a difference is two
/// characters where a position is five — but a file of positions says the same
/// thing and says it in the plainest way the format has.
#[must_use]
fn ink_xml(ink: &Ink) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <inkml:ink xmlns:inkml=\"{INKML}\"><inkml:definitions>\
         <inkml:context xml:id=\"ctx0\"><inkml:inkSource xml:id=\"src0\"><inkml:traceFormat>\
         <inkml:channel name=\"X\" type=\"integer\" max=\"32767\" units=\"himetric\"/>\
         <inkml:channel name=\"Y\" type=\"integer\" max=\"32767\" units=\"himetric\"/>\
         </inkml:traceFormat></inkml:inkSource></inkml:context>"
    );

    for (index, stroke) in ink.strokes.iter().enumerate() {
        out.push_str(&format!(
            "<inkml:brush xml:id=\"br{index}\">\
             <inkml:brushProperty name=\"width\" value=\"{width}\" units=\"himetric\"/>\
             <inkml:brushProperty name=\"height\" value=\"{width}\" units=\"himetric\"/>\
             <inkml:brushProperty name=\"color\" value=\"#{colour}\"/>\
             <inkml:brushProperty name=\"transparency\" value=\"{transparency}\"/>\
             <inkml:brushProperty name=\"tip\" value=\"{tip}\"/>\
             </inkml:brush>",
            width = (stroke.width_emu as f64 / EMU_PER_HIMETRIC).round() as i64,
            colour = stroke.colour,
            transparency = stroke.transparency,
            tip = if stroke.flat { "rectangle" } else { "ellipse" },
        ));
    }
    out.push_str("</inkml:definitions>");

    for (index, stroke) in ink.strokes.iter().enumerate() {
        out.push_str(&format!("<inkml:trace contextRef=\"#ctx0\" brushRef=\"#br{index}\">"));
        for (at, (x, y)) in stroke.points.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            let himetric = |emu: i64| (emu as f64 / EMU_PER_HIMETRIC).round() as i64;
            out.push_str(&format!("{} {}", himetric(*x), himetric(*y)));
        }
        out.push_str("</inkml:trace>");
    }

    out.push_str("</inkml:ink>");
    out
}

/// The run's own element: which part the ink is in, and how big it is drawn.
#[must_use]
fn content_part_element(relationship: &str, width_emu: i64, height_emu: i64) -> Element {
    let mut part = Element::new("w14:contentPart", Some(W14));
    part.declarations.push((Some("w14".to_owned()), W14.to_owned()));
    part.declarations.push((Some("r".to_owned()), crate::edit::RELATIONSHIPS.to_owned()));
    part.declarations.push((Some("a".to_owned()), crate::edit::DRAWING_MAIN.to_owned()));
    part.set_namespaced_attribute("r:id", crate::edit::RELATIONSHIPS, relationship);

    let mut properties = Element::new("w14:nvContentPartPr", Some(W14));
    let mut visible = Element::new("w14:cNvPr", Some(W14));
    visible.set_attribute("id", "1");
    visible.set_attribute("name", "Ink");
    properties.push_element(visible);
    properties.push_element(Element::new("w14:cNvContentPartPr", Some(W14)));
    part.push_element(properties);

    let mut transform = Element::new("w14:xfrm", Some(W14));
    let mut offset = Element::new("a:off", Some(crate::edit::DRAWING_MAIN));
    offset.set_attribute("x", "0");
    offset.set_attribute("y", "0");
    transform.push_element(offset);
    let mut extent = Element::new("a:ext", Some(crate::edit::DRAWING_MAIN));
    extent.set_attribute("cx", &width_emu.max(1).to_string());
    extent.set_attribute("cy", &height_emu.max(1).to_string());
    transform.push_element(extent);
    part.push_element(transform);
    part
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(xml: &str) -> Element {
        wp_xml::tree::XmlTree::parse(xml).expect("the part should parse").root
    }

    /// An ink part written the way Word writes one.
    fn sample(trace: &str) -> String {
        format!(
            "<inkml:ink xmlns:inkml=\"{INKML}\"><inkml:definitions>\
             <inkml:context xml:id=\"ctx0\"><inkml:inkSource xml:id=\"src0\">\
             <inkml:traceFormat>\
             <inkml:channel name=\"X\" type=\"integer\" max=\"32767\" units=\"himetric\"/>\
             <inkml:channel name=\"Y\" type=\"integer\" max=\"32767\" units=\"himetric\"/>\
             <inkml:channel name=\"F\" type=\"integer\" max=\"32767\" units=\"dev\"/>\
             </inkml:traceFormat></inkml:inkSource></inkml:context>\
             <inkml:brush xml:id=\"br0\">\
             <inkml:brushProperty name=\"width\" value=\"50\" units=\"himetric\"/>\
             <inkml:brushProperty name=\"height\" value=\"50\" units=\"himetric\"/>\
             <inkml:brushProperty name=\"color\" value=\"#ff0000\"/>\
             </inkml:brush></inkml:definitions>\
             <inkml:trace contextRef=\"#ctx0\" brushRef=\"#br0\">{trace}</inkml:trace></inkml:ink>"
        )
    }

    #[test]
    fn a_stroke_is_read_with_its_colour_and_its_width() {
        let ink = read_ink(&parsed(&sample("100 200 0,200 200 0")));
        assert_eq!(ink.strokes.len(), 1);
        assert_eq!(ink.strokes[0].colour, "FF0000");
        // Fifty himetric is eighteen thousand English metric units, which is
        // about a point and a half.
        assert_eq!(ink.strokes[0].width_emu, 50 * 360);
        assert_eq!(ink.strokes[0].points, vec![(100 * 360, 200 * 360), (200 * 360, 200 * 360)]);
    }

    #[test]
    fn a_number_with_no_prefix_goes_on_in_the_kind_the_last_one_was() {
        // The rule the whole encoding stands on. Word writes the first point
        // outright and everything after it as how far the pen moved, and only
        // the first number of each run carries the mark that says so.
        let ink = read_ink(&parsed(&sample("1000 1000 0,'10'10 0,10 10 0")));
        let points = &ink.strokes[0].points;
        assert_eq!(points.len(), 3);
        assert_eq!(points[1], (1010 * 360, 1010 * 360), "the difference was read as a position");
        assert_eq!(points[2], (1020 * 360, 1020 * 360), "the kind did not carry over");
    }

    #[test]
    fn a_prefix_separates_as_well_as_saying_what_follows() {
        let ink = read_ink(&parsed(&sample("1000 1000 0,'-4'-1 0")));
        assert_eq!(ink.strokes[0].points[1], (996 * 360, 999 * 360));
    }

    #[test]
    fn a_difference_of_differences_is_added_up_twice() {
        // A pen moving steadily writes almost nothing: the second difference
        // of a straight run at a steady speed is zero.
        let ink = read_ink(&parsed(&sample("0 0 0,'10'10 0,\"0\"0 0,0 0 0")));
        let points = &ink.strokes[0].points;
        assert_eq!(points[1], (10 * 360, 10 * 360));
        assert_eq!(points[2], (20 * 360, 20 * 360));
        assert_eq!(points[3], (30 * 360, 30 * 360), "the steady movement stopped");
    }

    #[test]
    fn a_value_stated_outright_again_stops_the_differences() {
        let ink = read_ink(&parsed(&sample("0 0 0,'10'10 0,!500!500 0")));
        assert_eq!(ink.strokes[0].points[2], (500 * 360, 500 * 360));
    }

    #[test]
    fn a_stroke_of_one_point_is_not_a_stroke() {
        let ink = read_ink(&parsed(&sample("100 200 0")));
        assert!(ink.strokes.is_empty());
        assert!(ink.is_empty());
    }

    #[test]
    fn the_channels_say_which_number_is_which() {
        // A trace format that puts the pressure first is read the same: what
        // makes a number an X is the channel it is in and not its place.
        let xml = format!(
            "<inkml:ink xmlns:inkml=\"{INKML}\"><inkml:context xml:id=\"c\">\
             <inkml:traceFormat>\
             <inkml:channel name=\"F\" units=\"dev\"/>\
             <inkml:channel name=\"X\" units=\"himetric\"/>\
             <inkml:channel name=\"Y\" units=\"himetric\"/>\
             </inkml:traceFormat></inkml:context>\
             <inkml:trace contextRef=\"#c\">0 100 200,0 300 400</inkml:trace></inkml:ink>"
        );
        let ink = read_ink(&parsed(&xml));
        assert_eq!(ink.strokes[0].points, vec![(100 * 360, 200 * 360), (300 * 360, 400 * 360)]);
    }

    #[test]
    fn a_highlighter_is_read_as_one() {
        let xml = format!(
            "<inkml:ink xmlns:inkml=\"{INKML}\"><inkml:definitions><inkml:brush xml:id=\"hl\">\
             <inkml:brushProperty name=\"width\" value=\"300\" units=\"himetric\"/>\
             <inkml:brushProperty name=\"color\" value=\"#FFFF00\"/>\
             <inkml:brushProperty name=\"tip\" value=\"rectangle\"/>\
             <inkml:brushProperty name=\"rasterOp\" value=\"maskPen\"/>\
             </inkml:brush></inkml:definitions>\
             <inkml:trace brushRef=\"#hl\">0 0 0,100 0 0</inkml:trace></inkml:ink>"
        );
        let ink = read_ink(&parsed(&xml));
        let stroke = &ink.strokes[0];
        assert!(stroke.flat, "a highlighter has a flat tip");
        assert!(stroke.transparency > 0, "a highlighter you cannot see through hides the words");
        assert_eq!(stroke.colour, "FFFF00");
    }

    #[test]
    fn ink_with_no_pen_named_is_still_ink() {
        let xml = format!(
            "<inkml:ink xmlns:inkml=\"{INKML}\">\
             <inkml:trace>0 0,100 100</inkml:trace></inkml:ink>"
        );
        let ink = read_ink(&parsed(&xml));
        assert_eq!(ink.strokes.len(), 1);
        assert_eq!(ink.strokes[0].colour, "000000", "a pen that says nothing is a black one");
        assert_eq!(ink.strokes[0].points.len(), 2);
    }

    #[test]
    fn every_trace_is_found_wherever_it_sits() {
        // Word puts them in groups, one group per stroke of a word.
        let xml = format!(
            "<inkml:ink xmlns:inkml=\"{INKML}\"><inkml:traceGroup>\
             <inkml:trace>0 0,10 10</inkml:trace>\
             <inkml:traceGroup><inkml:trace>20 20,30 30</inkml:trace></inkml:traceGroup>\
             </inkml:traceGroup></inkml:ink>"
        );
        assert_eq!(read_ink(&parsed(&xml)).strokes.len(), 2);
    }

    #[test]
    fn the_rectangle_ink_covers_counts_the_pen_it_was_drawn_with() {
        let ink = read_ink(&parsed(&sample("100 100 0,200 100 0")));
        let (left, top, right, bottom) = ink.bounds().expect("some ink");
        let half = 50 * 360 / 2;
        assert_eq!(left, 100 * 360 - half);
        assert_eq!(top, 100 * 360 - half);
        assert_eq!(right, 200 * 360 + half);
        assert_eq!(bottom, 100 * 360 + half);
    }

    #[test]
    fn a_run_says_which_part_its_ink_is_in_and_how_big_it_is() {
        let xml = format!(
            "<w14:contentPart xmlns:w14=\"{W14}\" xmlns:r=\"{rel}\" xmlns:a=\"{main}\" \
             r:id=\"rId9\"><w14:nvContentPartPr><w14:cNvPr id=\"3\" name=\"Ink 3\" \
             descr=\"A note\"/></w14:nvContentPartPr>\
             <w14:xfrm><a:off x=\"100\" y=\"200\"/><a:ext cx=\"914400\" cy=\"457200\"/></w14:xfrm>\
             </w14:contentPart>",
            rel = crate::edit::RELATIONSHIPS,
            main = crate::edit::DRAWING_MAIN,
        );
        let reference = read_reference(&parsed(&xml)).expect("a reference");
        assert_eq!(reference.relationship, "rId9");
        assert_eq!(reference.width_emu, 914_400);
        assert_eq!(reference.height_emu, 457_200);
        assert_eq!(reference.name, "Ink 3");
        assert_eq!(reference.description, "A note");
    }

    #[test]
    fn a_run_that_says_nothing_about_the_size_still_says_where_the_ink_is() {
        let xml = format!(
            "<w14:contentPart xmlns:w14=\"{W14}\" xmlns:r=\"{rel}\" r:id=\"rId4\"/>",
            rel = crate::edit::RELATIONSHIPS,
        );
        let reference = read_reference(&parsed(&xml)).expect("a reference");
        assert_eq!(reference.relationship, "rId4");
        assert_eq!(reference.width_emu, 0, "a size nobody stated is not a size");
    }
    #[test]
    fn ink_written_here_reads_back_as_what_was_drawn() {
        let ink = Ink {
            strokes: vec![Stroke {
                colour: "0070C0".to_owned(),
                width_emu: 18_000,
                transparency: 0,
                flat: false,
                points: vec![(0, 0), (36_000, 18_000), (72_000, 0)],
            }],
        };
        let read = read_ink(&parsed(&ink_xml(&ink)));
        assert_eq!(read.strokes.len(), 1);
        assert_eq!(read.strokes[0].colour, "0070C0");
        assert_eq!(read.strokes[0].points, ink.strokes[0].points);
        assert_eq!(read.strokes[0].width_emu, 18_000);
    }

    #[test]
    fn every_pen_is_written_and_every_stroke_keeps_its_own() {
        let ink = Ink {
            strokes: vec![
                Stroke {
                    colour: "FF0000".to_owned(),
                    width_emu: 9_000,
                    transparency: 0,
                    flat: false,
                    points: vec![(0, 0), (1_000, 0)],
                },
                Stroke {
                    colour: "FFFF00".to_owned(),
                    width_emu: 108_000,
                    transparency: 128,
                    flat: true,
                    points: vec![(0, 5_000), (10_000, 5_000)],
                },
            ],
        };
        let read = read_ink(&parsed(&ink_xml(&ink)));
        assert_eq!(read.strokes.len(), 2);
        assert_eq!(read.strokes[1].colour, "FFFF00");
        assert!(read.strokes[1].flat);
        assert_eq!(read.strokes[1].transparency, 128);
        assert_eq!(read.strokes[0].colour, "FF0000");
    }

    #[test]
    fn an_ink_part_is_well_formed() {
        let ink = Ink {
            strokes: vec![Stroke {
                colour: "000000".to_owned(),
                width_emu: 9_525,
                transparency: 0,
                flat: false,
                points: vec![(0, 0), (100, 100)],
            }],
        };
        wp_xml::tree::XmlTree::parse(&ink_xml(&ink)).expect("the part should parse");
    }
}

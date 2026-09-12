//! What a shape is filled with, when it is not one colour.
//!
//! # Why a fill is not a colour
//!
//! Because three quarters of the shapes in a real document are not one colour.
//! Word's own shape styles are gradients; its charts hatch their bars; a shape
//! used as a background is a picture. A model that held a fill as six hex
//! digits could carry none of them, and a document opened here would lose what
//! it was drawn with the moment it was saved.
//!
//! # The shape of a gradient
//!
//! A run of stops, each a colour and how far along it sits, and a direction to
//! run them in. The direction is either an angle — which is what makes a
//! gradient linear — or a shape to run outwards from, which is what makes it
//! radial. The stops are in sixtieths of a thousandth of a degree and hundredths
//! of a per cent, because this is the same format that measures a rotation that
//! way.
//!
//! # The hatchings
//!
//! Fifty-four of them, each a name. They are eight pixels by eight, black on a
//! background, repeated across whatever they fill — which is exactly how they
//! were drawn when they were invented, and why a hatched shape looks the same
//! at any size.

use wp_xml::tree::Element;

/// What fills a shape.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Fill {
    /// Nothing at all: whatever is behind shows through.
    #[default]
    None,
    /// One colour, as six hex digits.
    Solid(String),
    Gradient(Gradient),
    Pattern(Pattern),
}

/// A run of colours, and the direction to run them in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gradient {
    /// Each colour and how far along it sits, in hundredths of a per cent.
    pub stops: Vec<(u32, String)>,
    /// Which way it runs. See [`Direction`].
    pub direction: Direction,
}

/// Which way a gradient runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Along a line at an angle, in sixtieths of a thousandth of a degree
    /// clockwise from pointing right — the same unit a rotation is in.
    Linear(i32),
    /// Outwards from a point, in rings.
    Radial,
    /// Outwards from a point, in rectangles.
    Rectangular,
}

impl Default for Direction {
    fn default() -> Self {
        // Straight down, which is what a gradient with no direction means and
        // what nearly every shape in a document uses.
        Self::Linear(5_400_000)
    }
}

/// A hatching: a named pattern in two colours.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pattern {
    /// The name the format knows it by, such as `ltUpDiag`.
    pub name: String,
    /// The colour the pattern itself is drawn in, and the colour behind it.
    pub foreground: String,
    pub background: String,
}

impl Gradient {
    /// The colour at a point along the run, as six hex digits.
    ///
    /// `along` is nought at the start and one at the end. Between two stops the
    /// two colours are mixed in proportion, which is what makes it a gradient
    /// rather than a set of bands.
    #[must_use]
    pub fn colour_at(&self, along: f32) -> String {
        let along = along.clamp(0.0, 1.0) * 100_000.0;
        if self.stops.is_empty() {
            return "FFFFFF".to_owned();
        }
        let first = &self.stops[0];
        if along <= first.0 as f32 {
            return first.1.clone();
        }
        for pair in self.stops.windows(2) {
            let (before, after) = (&pair[0], &pair[1]);
            if along > after.0 as f32 {
                continue;
            }
            let span = (after.0 - before.0).max(1) as f32;
            let how_far = ((along - before.0 as f32) / span).clamp(0.0, 1.0);
            return mixed(&before.1, &after.1, how_far);
        }
        self.stops.last().map_or_else(|| "FFFFFF".to_owned(), |stop| stop.1.clone())
    }
}

/// Two colours mixed in proportion, as six hex digits.
fn mixed(from: &str, to: &str, how_far: f32) -> String {
    let parse = |text: &str| -> [u8; 3] {
        let digits = text.trim_start_matches('#');
        let byte =
            |at: usize| u8::from_str_radix(digits.get(at..at + 2).unwrap_or("00"), 16).unwrap_or(0);
        [byte(0), byte(2), byte(4)]
    };
    let (one, other) = (parse(from), parse(to));
    // Rounded rather than truncated: halfway between black and white is a
    // hundred and twenty-eight, and a gradient that always took the lower of
    // the two would run a shade dark along its whole length.
    let mix = |index: usize| {
        let value =
            f32::from(one[index]) + (f32::from(other[index]) - f32::from(one[index])) * how_far;
        value.round().clamp(0.0, 255.0) as u8
    };
    format!("{:02X}{:02X}{:02X}", mix(0), mix(1), mix(2))
}

/// Reads whatever a shape's properties say it is filled with.
///
/// The order matters and is the format's: a properties element holds at most
/// one fill, and which one it is is the name of the element.
#[must_use]
pub fn read_fill(properties: &Element) -> Fill {
    for child in properties.child_elements() {
        match child.local_name() {
            "noFill" => return Fill::None,
            "solidFill" => {
                return colour_in(child).map_or(Fill::None, Fill::Solid);
            }
            "gradFill" => return Fill::Gradient(read_gradient(child)),
            "pattFill" => return Fill::Pattern(read_pattern(child)),
            _ => {}
        }
    }
    Fill::None
}

/// The stops and the direction of a gradient.
fn read_gradient(element: &Element) -> Gradient {
    let mut gradient = Gradient::default();

    if let Some(list) = child(element, "gsLst") {
        for stop in list.child_elements() {
            if stop.local_name() != "gs" {
                continue;
            }
            let along = stop
                .attribute_by_name("pos")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0)
                .min(100_000);
            if let Some(colour) = colour_in(stop) {
                gradient.stops.push((along, colour));
            }
        }
    }
    // A file that says nothing about where its stops are has them at the two
    // ends, which is the only reading that draws anything.
    gradient.stops.sort_by_key(|(along, _)| *along);

    if let Some(line) = child(element, "lin") {
        let angle = line
            .attribute_by_name("ang")
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(5_400_000);
        gradient.direction = Direction::Linear(angle);
    } else if let Some(path) = child(element, "path") {
        gradient.direction = match path.attribute_by_name("path") {
            Some("circle") => Direction::Radial,
            // "shape" runs outwards along the shape's own outline, which is a
            // ring for a round shape and a rectangle for a square one. Drawn as
            // a rectangle, which is what it is for the shapes that use it.
            _ => Direction::Rectangular,
        };
    }

    gradient
}

/// The name and two colours of a hatching.
fn read_pattern(element: &Element) -> Pattern {
    Pattern {
        name: element.attribute_by_name("prst").unwrap_or("pct50").to_owned(),
        foreground: child(element, "fgClr")
            .and_then(colour_in)
            .unwrap_or_else(|| "000000".to_owned()),
        background: child(element, "bgClr")
            .and_then(colour_in)
            .unwrap_or_else(|| "FFFFFF".to_owned()),
    }
}

/// The colour under an element, however it is written.
///
/// A colour may be stated outright or named from the theme; a theme colour is
/// left to whoever resolves those, and this answers with what it can see.
fn colour_in(parent: &Element) -> Option<String> {
    fn search(element: &Element) -> Option<String> {
        if element.local_name() == "srgbClr" {
            return element.attribute_by_name("val").map(str::to_uppercase);
        }
        element.child_elements().find_map(search)
    }
    search(parent)
}

fn child<'a>(parent: &'a Element, local: &str) -> Option<&'a Element> {
    parent.child_elements().find(|child| child.local_name() == local)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_xml::tree::XmlTree;

    fn properties(inside: &str) -> Element {
        let text = format!(
            "<a:spPr xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">{inside}</a:spPr>"
        );
        XmlTree::parse(&text).expect("a document").root
    }

    #[test]
    fn a_shape_that_says_it_has_no_fill_has_none() {
        assert_eq!(read_fill(&properties("<a:noFill/>")), Fill::None);
    }

    #[test]
    fn one_colour_is_read_as_one_colour() {
        let element = properties("<a:solidFill><a:srgbClr val=\"4472c4\"/></a:solidFill>");
        assert_eq!(read_fill(&element), Fill::Solid("4472C4".to_owned()));
    }

    #[test]
    fn a_gradient_keeps_its_stops_in_order() {
        let element = properties(
            "<a:gradFill><a:gsLst>\
             <a:gs pos=\"100000\"><a:srgbClr val=\"000000\"/></a:gs>\
             <a:gs pos=\"0\"><a:srgbClr val=\"FFFFFF\"/></a:gs>\
             </a:gsLst><a:lin ang=\"0\"/></a:gradFill>",
        );
        let Fill::Gradient(gradient) = read_fill(&element) else { panic!("not a gradient") };
        assert_eq!(gradient.stops.len(), 2);
        assert_eq!(gradient.stops[0], (0, "FFFFFF".to_owned()), "the first should be the nearest");
        assert_eq!(gradient.direction, Direction::Linear(0));
    }

    #[test]
    fn a_gradient_with_no_direction_runs_down_the_shape() {
        let element = properties(
            "<a:gradFill><a:gsLst><a:gs pos=\"0\"><a:srgbClr val=\"FFFFFF\"/></a:gs></a:gsLst></a:gradFill>",
        );
        let Fill::Gradient(gradient) = read_fill(&element) else { panic!("not a gradient") };
        assert_eq!(gradient.direction, Direction::Linear(5_400_000));
    }

    #[test]
    fn a_gradient_that_runs_outwards_says_which_way() {
        let round = properties("<a:gradFill><a:path path=\"circle\"/></a:gradFill>");
        let Fill::Gradient(gradient) = read_fill(&round) else { panic!("not a gradient") };
        assert_eq!(gradient.direction, Direction::Radial);

        let square = properties("<a:gradFill><a:path path=\"rect\"/></a:gradFill>");
        let Fill::Gradient(gradient) = read_fill(&square) else { panic!("not a gradient") };
        assert_eq!(gradient.direction, Direction::Rectangular);
    }

    #[test]
    fn a_hatching_keeps_its_name_and_both_its_colours() {
        let element = properties(
            "<a:pattFill prst=\"ltUpDiag\">\
             <a:fgClr><a:srgbClr val=\"FF0000\"/></a:fgClr>\
             <a:bgClr><a:srgbClr val=\"00FF00\"/></a:bgClr>\
             </a:pattFill>",
        );
        let Fill::Pattern(pattern) = read_fill(&element) else { panic!("not a pattern") };
        assert_eq!(pattern.name, "ltUpDiag");
        assert_eq!(pattern.foreground, "FF0000");
        assert_eq!(pattern.background, "00FF00");
    }

    #[test]
    fn the_colour_between_two_stops_is_the_two_mixed() {
        let gradient = Gradient {
            stops: vec![(0, "000000".to_owned()), (100_000, "FFFFFF".to_owned())],
            direction: Direction::Linear(0),
        };
        assert_eq!(gradient.colour_at(0.0), "000000");
        assert_eq!(gradient.colour_at(1.0), "FFFFFF");
        assert_eq!(gradient.colour_at(0.5), "808080", "halfway should be halfway");
    }

    #[test]
    fn before_the_first_stop_and_after_the_last_are_the_ends() {
        let gradient = Gradient {
            stops: vec![(25_000, "FF0000".to_owned()), (75_000, "0000FF".to_owned())],
            direction: Direction::Linear(0),
        };
        assert_eq!(gradient.colour_at(0.0), "FF0000");
        assert_eq!(gradient.colour_at(1.0), "0000FF");
        assert_eq!(gradient.colour_at(0.5), "800080", "and the middle is the two mixed");
    }

    #[test]
    fn a_gradient_of_one_stop_is_that_colour_throughout() {
        let gradient = Gradient {
            stops: vec![(50_000, "123456".to_owned())],
            direction: Direction::Linear(0),
        };
        assert_eq!(gradient.colour_at(0.0), "123456");
        assert_eq!(gradient.colour_at(1.0), "123456");
    }

    #[test]
    fn a_gradient_of_nothing_is_white_rather_than_a_panic() {
        assert_eq!(Gradient::default().colour_at(0.5), "FFFFFF");
    }
}

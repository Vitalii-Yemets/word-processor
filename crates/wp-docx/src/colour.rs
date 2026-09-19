//! A colour as a drawing writes it: stated outright, or named from the theme.
//!
//! # Why a colour is not six hex digits
//!
//! Because Word does not write six hex digits for most of the colours in a
//! document. A shape from its gallery is filled with `accent1`; the line round
//! it is `accent1` darkened by half; a glow is `accent1` at forty per cent.
//! Change the theme and every one of them changes, because none of them ever
//! said a colour — they said a name and how far to shift it.
//!
//! A program that resolved the name when it read the shape would draw it right
//! once and then wrong: the theme gallery would change the headings and leave
//! the shapes behind. So a colour here carries what the file said — the base
//! and the shifts written under it — and is resolved where the theme is known,
//! which is when it is drawn.
//!
//! # The shifts
//!
//! DrawingML writes them as children of the colour, each a name and a value in
//! hundred-thousandths: `<a:shade val="50000"/>` is half as bright. They are
//! applied in the order written. Tint and shade work on the components as
//! written; the rest go round through hue, saturation and lightness and back.
//! The names are kept as they are, so a shift this program does not apply
//! still comes back out of the file it went into.

use wp_xml::tree::Element;

use crate::theme::{Slot, Theme};

/// The drawing namespace a colour is written in.
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

/// A colour, as the file says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colour {
    pub base: Base,
    /// The shifts written under it, in the order written.
    pub shifts: Vec<Shift>,
}

/// What a colour starts from before its shifts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Base {
    /// Six hex digits, upper case.
    Rgb(String),
    /// One of the theme's twelve.
    Scheme(Slot),
    /// `phClr`: the colour a theme's format scheme leaves open, for the shape
    /// that takes the style to fill in. Never meaningful outside the theme,
    /// and drawn as the first accent if it ever gets that far.
    Placeholder,
}

/// One shift of a colour: its name as the format writes it, such as `lumMod`,
/// and its value in hundred-thousandths. A shift written without a value —
/// `gray`, `inv`, `comp` — has nought.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shift {
    pub name: String,
    pub value: i32,
}

impl Default for Colour {
    fn default() -> Self {
        Self::rgb("000000")
    }
}

impl Colour {
    /// A colour stated outright.
    #[must_use]
    pub fn rgb(hex: &str) -> Self {
        Self { base: Base::Rgb(hex.trim_start_matches('#').to_uppercase()), shifts: Vec::new() }
    }

    /// One of the theme's colours, as it is.
    #[must_use]
    pub fn scheme(slot: Slot) -> Self {
        Self { base: Base::Scheme(slot), shifts: Vec::new() }
    }

    /// The same colour with one more shift after the ones it has.
    #[must_use]
    pub fn shifted(mut self, name: &str, value: i32) -> Self {
        self.shifts.push(Shift { name: name.to_owned(), value });
        self
    }

    /// Whether it names the theme rather than stating a colour.
    #[must_use]
    pub fn is_named(&self) -> bool {
        !matches!(self.base, Base::Rgb(_))
    }

    /// The six hex digits, when the colour is stated outright and shifted by
    /// nothing — which is what a colour typed into a dialog is.
    #[must_use]
    pub fn hex(&self) -> Option<&str> {
        match &self.base {
            Base::Rgb(hex) if self.shifts.is_empty() => Some(hex),
            _ => None,
        }
    }

    /// What it comes out as against a theme, as six hex digits.
    #[must_use]
    pub fn resolve(&self, theme: &Theme) -> String {
        let base = match &self.base {
            Base::Rgb(hex) => hex.clone(),
            Base::Scheme(slot) => theme.color(*slot),
            Base::Placeholder => theme.color(Slot::Accent1),
        };
        apply(&base, &self.shifts)
    }

    /// This colour with the placeholder filled in: what a theme's style
    /// becomes for a shape that names its colour. The shape's shifts come
    /// first, then the style's own, which is the order the format applies
    /// them in.
    #[must_use]
    pub fn filled_in(&self, with: &Colour) -> Self {
        match self.base {
            Base::Placeholder => {
                let mut shifts = with.shifts.clone();
                shifts.extend(self.shifts.iter().cloned());
                Self { base: with.base.clone(), shifts }
            }
            _ => self.clone(),
        }
    }
}

/// The colour written under an element, wherever it is: a `solidFill` holds
/// it directly, a line holds it under its fill, and the first one found is
/// the one meant.
#[must_use]
pub fn read_colour(parent: &Element) -> Option<Colour> {
    fn search(element: &Element) -> Option<&Element> {
        if matches!(element.local_name(), "srgbClr" | "schemeClr" | "prstClr" | "sysClr") {
            return Some(element);
        }
        element.child_elements().find_map(search)
    }
    read_colour_element(search(parent)?)
}

/// A colour element itself — `a:srgbClr`, `a:schemeClr`, `a:prstClr` or
/// `a:sysClr` — with the shifts under it.
#[must_use]
pub fn read_colour_element(element: &Element) -> Option<Colour> {
    let value = element.attribute_by_name("val");
    let base = match element.local_name() {
        "srgbClr" => Base::Rgb(value?.to_uppercase()),
        "schemeClr" => match value? {
            "phClr" => Base::Placeholder,
            name => Base::Scheme(Slot::from_drawing(name)?),
        },
        "prstClr" => Base::Rgb(preset_colour(value?)?.to_owned()),
        // A system colour carries the last value it was seen with, for anything
        // that cannot ask the system; the two Word writes are black and white.
        "sysClr" => {
            Base::Rgb(element.attribute_by_name("lastClr").map(str::to_uppercase).or_else(
                || match value? {
                    "windowText" => Some("000000".to_owned()),
                    "window" => Some("FFFFFF".to_owned()),
                    _ => None,
                },
            )?)
        }
        _ => return None,
    };
    let shifts = element
        .child_elements()
        .map(|shift| Shift {
            name: shift.local_name().to_owned(),
            value: shift
                .attribute_by_name("val")
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(0),
        })
        .collect();
    Some(Colour { base, shifts })
}

/// The colour as an element: `a:srgbClr` or `a:schemeClr`, with its shifts.
#[must_use]
pub fn colour_element(colour: &Colour) -> Element {
    let mut element = match &colour.base {
        Base::Rgb(hex) => {
            let mut element = Element::new("a:srgbClr", Some(A));
            element.set_attribute("val", hex);
            element
        }
        Base::Scheme(slot) => {
            let mut element = Element::new("a:schemeClr", Some(A));
            element.set_attribute("val", slot.drawing_name());
            element
        }
        Base::Placeholder => {
            let mut element = Element::new("a:schemeClr", Some(A));
            element.set_attribute("val", "phClr");
            element
        }
    };
    for shift in &colour.shifts {
        let mut child = Element::new(&format!("a:{}", shift.name), Some(A));
        if !matches!(shift.name.as_str(), "gray" | "inv" | "comp" | "gamma" | "invGamma") {
            child.set_attribute("val", &shift.value.to_string());
        }
        element.push_element(child);
    }
    element
}

/// The colour wrapped as a solid fill.
#[must_use]
pub fn solid_fill(colour: &Colour) -> Element {
    let mut fill = Element::new("a:solidFill", Some(A));
    fill.push_element(colour_element(colour));
    fill
}

/// A base colour with shifts applied, as six hex digits.
///
/// The shifts are in hundred-thousandths, and a hue in sixty-thousandths of a
/// degree. Shade and tint are worked on the components as they are written
/// rather than in light as it is measured, which is what the format asks for
/// and is a shade off what a photometer would say.
#[must_use]
pub fn apply(base: &str, shifts: &[Shift]) -> String {
    let digits =
        |at: usize| u8::from_str_radix(base.get(at..at + 2).unwrap_or("00"), 16).unwrap_or(0);
    let (mut red, mut green, mut blue) = (digits(0), digits(2), digits(4));

    for shift in shifts {
        let value = shift.value as f32 / 100_000.0;
        match shift.name.as_str() {
            "shade" => {
                let by = value.clamp(0.0, 1.0);
                let darker = |component: u8| (f32::from(component) * by).round() as u8;
                (red, green, blue) = (darker(red), darker(green), darker(blue));
            }
            "tint" => {
                let by = value.clamp(0.0, 1.0);
                let lighter =
                    |component: u8| (f32::from(component) * by + 255.0 * (1.0 - by)).round() as u8;
                (red, green, blue) = (lighter(red), lighter(green), lighter(blue));
            }
            "inv" => (red, green, blue) = (255 - red, 255 - green, 255 - blue),
            "gray" => {
                let grey =
                    (f32::from(red) * 0.299 + f32::from(green) * 0.587 + f32::from(blue) * 0.114)
                        .round() as u8;
                (red, green, blue) = (grey, grey, grey);
            }
            // The rest are said in hue, saturation and lightness, so the
            // colour goes round into those and back again.
            "lum" | "lumMod" | "lumOff" | "sat" | "satMod" | "satOff" | "hue" | "hueOff"
            | "hueMod" | "comp" => {
                let (mut hue, mut saturation, mut lightness) = to_hsl(red, green, blue);
                match shift.name.as_str() {
                    "lum" => lightness = value,
                    "lumMod" => lightness *= value,
                    "lumOff" => lightness += value,
                    "sat" => saturation = value,
                    "satMod" => saturation *= value,
                    "satOff" => saturation += value,
                    // Sixty-thousandths of a degree, and the hundred thousand
                    // above has already divided by a hundred thousand.
                    "hue" => hue = value * 100_000.0 / 60_000.0,
                    "hueOff" => hue += value * 100_000.0 / 60_000.0,
                    "hueMod" => hue *= value,
                    _ => hue += 180.0,
                }
                (red, green, blue) = from_hsl(
                    hue.rem_euclid(360.0),
                    saturation.clamp(0.0, 1.0),
                    lightness.clamp(0.0, 1.0),
                );
            }
            // The transparency, the gamma and the single components change
            // nothing here: a colour is drawn opaque, and the rest are not
            // what any gallery writes.
            _ => {}
        }
    }
    format!("{red:02X}{green:02X}{blue:02X}")
}

/// Hue in degrees, saturation and lightness as fractions.
fn to_hsl(red: u8, green: u8, blue: u8) -> (f32, f32, f32) {
    let (red, green, blue) =
        (f32::from(red) / 255.0, f32::from(green) / 255.0, f32::from(blue) / 255.0);
    let largest = red.max(green).max(blue);
    let smallest = red.min(green).min(blue);
    let lightness = (largest + smallest) / 2.0;
    let span = largest - smallest;
    if span <= f32::EPSILON {
        return (0.0, 0.0, lightness);
    }
    let saturation = if lightness > 0.5 {
        span / (2.0 - largest - smallest)
    } else {
        span / (largest + smallest)
    };
    let hue = if largest == red {
        60.0 * ((green - blue) / span).rem_euclid(6.0)
    } else if largest == green {
        60.0 * ((blue - red) / span + 2.0)
    } else {
        60.0 * ((red - green) / span + 4.0)
    };
    (hue, saturation, lightness)
}

/// And back to the three components.
fn from_hsl(hue: f32, saturation: f32, lightness: f32) -> (u8, u8, u8) {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue / 60.0;
    let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let lift = lightness - chroma / 2.0;
    let component = |value: f32| ((value + lift) * 255.0).round().clamp(0.0, 255.0) as u8;
    (component(r), component(g), component(b))
}

/// The six hex digits one of the format's named colours stands for.
///
/// The names are the web's, which the format took as they were, with `dark`,
/// `light` and `medium` shortened to `dk`, `lt` and `med` in the older
/// spelling. Both spellings are read.
#[must_use]
pub fn preset_colour(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    let full = if let Some(rest) = lower.strip_prefix("dk") {
        format!("dark{rest}")
    } else if let Some(rest) = lower.strip_prefix("lt") {
        format!("light{rest}")
    } else if let Some(rest) = lower.strip_prefix("med") {
        format!("medium{rest}")
    } else {
        lower
    };
    PRESETS.iter().find(|(known, _)| *known == full).map(|(_, hex)| *hex)
}

const PRESETS: &[(&str, &str)] = &[
    ("aliceblue", "F0F8FF"),
    ("antiquewhite", "FAEBD7"),
    ("aqua", "00FFFF"),
    ("aquamarine", "7FFFD4"),
    ("azure", "F0FFFF"),
    ("beige", "F5F5DC"),
    ("bisque", "FFE4C4"),
    ("black", "000000"),
    ("blanchedalmond", "FFEBCD"),
    ("blue", "0000FF"),
    ("blueviolet", "8A2BE2"),
    ("brown", "A52A2A"),
    ("burlywood", "DEB887"),
    ("cadetblue", "5F9EA0"),
    ("chartreuse", "7FFF00"),
    ("chocolate", "D2691E"),
    ("coral", "FF7F50"),
    ("cornflowerblue", "6495ED"),
    ("cornsilk", "FFF8DC"),
    ("crimson", "DC143C"),
    ("cyan", "00FFFF"),
    ("darkblue", "00008B"),
    ("darkcyan", "008B8B"),
    ("darkgoldenrod", "B8860B"),
    ("darkgray", "A9A9A9"),
    ("darkgreen", "006400"),
    ("darkgrey", "A9A9A9"),
    ("darkkhaki", "BDB76B"),
    ("darkmagenta", "8B008B"),
    ("darkolivegreen", "556B2F"),
    ("darkorange", "FF8C00"),
    ("darkorchid", "9932CC"),
    ("darkred", "8B0000"),
    ("darksalmon", "E9967A"),
    ("darkseagreen", "8FBC8F"),
    ("darkslateblue", "483D8B"),
    ("darkslategray", "2F4F4F"),
    ("darkslategrey", "2F4F4F"),
    ("darkturquoise", "00CED1"),
    ("darkviolet", "9400D3"),
    ("deeppink", "FF1493"),
    ("deepskyblue", "00BFFF"),
    ("dimgray", "696969"),
    ("dimgrey", "696969"),
    ("dodgerblue", "1E90FF"),
    ("firebrick", "B22222"),
    ("floralwhite", "FFFAF0"),
    ("forestgreen", "228B22"),
    ("fuchsia", "FF00FF"),
    ("gainsboro", "DCDCDC"),
    ("ghostwhite", "F8F8FF"),
    ("gold", "FFD700"),
    ("goldenrod", "DAA520"),
    ("gray", "808080"),
    ("green", "008000"),
    ("greenyellow", "ADFF2F"),
    ("grey", "808080"),
    ("honeydew", "F0FFF0"),
    ("hotpink", "FF69B4"),
    ("indianred", "CD5C5C"),
    ("indigo", "4B0082"),
    ("ivory", "FFFFF0"),
    ("khaki", "F0E68C"),
    ("lavender", "E6E6FA"),
    ("lavenderblush", "FFF0F5"),
    ("lawngreen", "7CFC00"),
    ("lemonchiffon", "FFFACD"),
    ("lightblue", "ADD8E6"),
    ("lightcoral", "F08080"),
    ("lightcyan", "E0FFFF"),
    ("lightgoldenrodyellow", "FAFAD2"),
    ("lightgray", "D3D3D3"),
    ("lightgreen", "90EE90"),
    ("lightgrey", "D3D3D3"),
    ("lightpink", "FFB6C1"),
    ("lightsalmon", "FFA07A"),
    ("lightseagreen", "20B2AA"),
    ("lightskyblue", "87CEFA"),
    ("lightslategray", "778899"),
    ("lightslategrey", "778899"),
    ("lightsteelblue", "B0C4DE"),
    ("lightyellow", "FFFFE0"),
    ("lime", "00FF00"),
    ("limegreen", "32CD32"),
    ("linen", "FAF0E6"),
    ("magenta", "FF00FF"),
    ("maroon", "800000"),
    ("mediumaquamarine", "66CDAA"),
    ("mediumblue", "0000CD"),
    ("mediumorchid", "BA55D3"),
    ("mediumpurple", "9370DB"),
    ("mediumseagreen", "3CB371"),
    ("mediumslateblue", "7B68EE"),
    ("mediumspringgreen", "00FA9A"),
    ("mediumturquoise", "48D1CC"),
    ("mediumvioletred", "C71585"),
    ("midnightblue", "191970"),
    ("mintcream", "F5FFFA"),
    ("mistyrose", "FFE4E1"),
    ("moccasin", "FFE4B5"),
    ("navajowhite", "FFDEAD"),
    ("navy", "000080"),
    ("oldlace", "FDF5E6"),
    ("olive", "808000"),
    ("olivedrab", "6B8E23"),
    ("orange", "FFA500"),
    ("orangered", "FF4500"),
    ("orchid", "DA70D6"),
    ("palegoldenrod", "EEE8AA"),
    ("palegreen", "98FB98"),
    ("paleturquoise", "AFEEEE"),
    ("palevioletred", "DB7093"),
    ("papayawhip", "FFEFD5"),
    ("peachpuff", "FFDAB9"),
    ("peru", "CD853F"),
    ("pink", "FFC0CB"),
    ("plum", "DDA0DD"),
    ("powderblue", "B0E0E6"),
    ("purple", "800080"),
    ("red", "FF0000"),
    ("rosybrown", "BC8F8F"),
    ("royalblue", "4169E1"),
    ("saddlebrown", "8B4513"),
    ("salmon", "FA8072"),
    ("sandybrown", "F4A460"),
    ("seagreen", "2E8B57"),
    ("seashell", "FFF5EE"),
    ("sienna", "A0522D"),
    ("silver", "C0C0C0"),
    ("skyblue", "87CEEB"),
    ("slateblue", "6A5ACD"),
    ("slategray", "708090"),
    ("slategrey", "708090"),
    ("snow", "FFFAFA"),
    ("springgreen", "00FF7F"),
    ("steelblue", "4682B4"),
    ("tan", "D2B48C"),
    ("teal", "008080"),
    ("thistle", "D8BFD8"),
    ("tomato", "FF6347"),
    ("turquoise", "40E0D0"),
    ("violet", "EE82EE"),
    ("wheat", "F5DEB3"),
    ("white", "FFFFFF"),
    ("whitesmoke", "F5F5F5"),
    ("yellow", "FFFF00"),
    ("yellowgreen", "9ACD32"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use wp_xml::tree::XmlTree;

    fn parsed(xml: &str) -> Element {
        XmlTree::parse(xml).expect("parses").root
    }

    #[test]
    fn a_colour_stated_outright_is_read_and_resolves_to_itself() {
        let element =
            parsed("<a:solidFill xmlns:a=\"x\"><a:srgbClr val=\"4472c4\"/></a:solidFill>");
        let colour = read_colour(&element).expect("a colour");
        assert_eq!(colour, Colour::rgb("4472C4"));
        assert_eq!(colour.hex(), Some("4472C4"));
        assert_eq!(colour.resolve(&Theme::default()), "4472C4");
    }

    #[test]
    fn a_named_colour_carries_its_slot_and_shifts_and_resolves_against_the_theme() {
        let element = parsed(
            "<a:ln xmlns:a=\"x\"><a:solidFill><a:schemeClr val=\"accent1\">\
             <a:shade val=\"50000\"/></a:schemeClr></a:solidFill></a:ln>",
        );
        let colour = read_colour(&element).expect("a colour");
        assert_eq!(colour.base, Base::Scheme(Slot::Accent1));
        assert_eq!(colour.shifts, vec![Shift { name: "shade".to_owned(), value: 50_000 }]);
        assert!(colour.is_named());
        assert_eq!(colour.hex(), None);
        // Half of 4472C4, component by component.
        assert_eq!(colour.resolve(&Theme::default()), "223962");
        let mut other = Theme::default();
        other.colors[4] = "FF0000".to_owned();
        assert_eq!(colour.resolve(&other), "800000", "the colour follows the theme");
    }

    #[test]
    fn the_shifts_lighten_darken_and_turn_the_hue() {
        let lighter = apply(
            "4472C4",
            &[
                Shift { name: "lumMod".to_owned(), value: 60_000 },
                Shift { name: "lumOff".to_owned(), value: 40_000 },
            ],
        );
        assert_ne!(lighter, "4472C4");
        let same = apply("4472C4", &[]);
        assert_eq!(same, "4472C4", "a colour with nothing said under it is itself");
        let tinted = apply("000000", &[Shift { name: "tint".to_owned(), value: 50_000 }]);
        assert_eq!(tinted, "808080");
        let turned = apply("FF0000", &[Shift { name: "comp".to_owned(), value: 0 }]);
        assert_eq!(turned, "00FFFF");
        let grey = apply("FF0000", &[Shift { name: "gray".to_owned(), value: 0 }]);
        assert_eq!(grey, "4C4C4C");
        let inverted = apply("FF0000", &[Shift { name: "inv".to_owned(), value: 0 }]);
        assert_eq!(inverted, "00FFFF");
    }

    #[test]
    fn a_colour_is_written_as_it_was_read() {
        let colour =
            Colour::scheme(Slot::Accent2).shifted("lumMod", 75_000).shifted("alpha", 40_000);
        let element = colour_element(&colour);
        assert_eq!(element.attribute_by_name("val"), Some("accent2"));
        let back = read_colour_element(&element).expect("reads back");
        assert_eq!(back, colour);
        let plain = colour_element(&Colour::rgb("ABCDEF"));
        assert_eq!(plain.local_name(), "srgbClr");
        assert_eq!(read_colour_element(&plain), Some(Colour::rgb("ABCDEF")));
    }

    #[test]
    fn the_named_colours_and_the_system_colours_are_read_as_what_they_are() {
        let black = parsed("<a:prstClr xmlns:a=\"x\" val=\"black\"/>");
        assert_eq!(read_colour_element(&black), Some(Colour::rgb("000000")));
        let blue = parsed("<a:prstClr xmlns:a=\"x\" val=\"dkBlue\"/>");
        assert_eq!(read_colour_element(&blue), Some(Colour::rgb("00008B")));
        let window = parsed("<a:sysClr xmlns:a=\"x\" val=\"window\" lastClr=\"FFFFFF\"/>");
        assert_eq!(read_colour_element(&window), Some(Colour::rgb("FFFFFF")));
        let text = parsed("<a:sysClr xmlns:a=\"x\" val=\"windowText\"/>");
        assert_eq!(read_colour_element(&text), Some(Colour::rgb("000000")));
        assert_eq!(preset_colour("medAquamarine"), Some("66CDAA"));
        assert_eq!(preset_colour("nonsense"), None);
    }

    #[test]
    fn the_placeholder_is_filled_in_with_the_shape_s_colour_and_its_shifts_come_first() {
        let style = Colour {
            base: Base::Placeholder,
            shifts: vec![Shift { name: "tint".to_owned(), value: 67_000 }],
        };
        let mine = Colour::scheme(Slot::Accent3).shifted("shade", 50_000);
        let filled = style.filled_in(&mine);
        assert_eq!(filled.base, Base::Scheme(Slot::Accent3));
        assert_eq!(
            filled.shifts.iter().map(|shift| shift.name.as_str()).collect::<Vec<_>>(),
            vec!["shade", "tint"]
        );
        assert_eq!(mine.filled_in(&Colour::rgb("FF0000")), mine, "only a placeholder is filled in");
    }
}

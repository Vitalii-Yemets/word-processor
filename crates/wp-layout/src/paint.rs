//! What a shape is filled with, worked out ready to draw.
//!
//! # Why the fill is turned into this
//!
//! Because what the document says and what the canvas needs are different
//! things. The document names colours in hex and hatchings by name; the canvas
//! wants a colour for a given pixel. This is the step between: the colours are
//! resolved once, the hatching is turned into the eight-by-eight mask it
//! actually is, and what comes out can answer for any point of the shape
//! without looking anything up again.
//!
//! # The hatchings
//!
//! Two families and a handful of odd ones. The percentages are an ordered
//! dither — the same sixty-four thresholds every one of them uses, compared
//! against how dark the pattern is meant to be — which is why they are worked
//! out rather than written down. The lines and crosses are written down,
//! because each is its own arrangement of rows and columns.

use wp_docx::fills::{Direction, Fill};
use wp_raster::Color;

/// What a shape is filled with.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Paint {
    /// Nothing at all: whatever is behind shows through.
    #[default]
    None,
    Solid(Color),
    /// A run of colours, each with how far along it sits, and which way the run
    /// goes.
    Gradient {
        stops: Vec<(f32, Color)>,
        direction: Direction,
    },
    /// A hatching: eight rows of eight bits, and the two colours it is drawn
    /// in. A set bit is the pattern itself.
    Pattern {
        mask: [u8; 8],
        foreground: Color,
        background: Color,
    },
}

impl Paint {
    /// Works one out from what the document says.
    #[must_use]
    pub fn of(fill: &Fill) -> Self {
        match fill {
            Fill::None => Self::None,
            Fill::Solid(colour) => Color::from_hex(colour).map_or(Self::None, Self::Solid),
            Fill::Gradient(gradient) => {
                let stops: Vec<(f32, Color)> = gradient
                    .stops
                    .iter()
                    .filter_map(|(along, colour)| {
                        Color::from_hex(colour).map(|colour| (*along as f32 / 100_000.0, colour))
                    })
                    .collect();
                if stops.is_empty() {
                    return Self::None;
                }
                Self::Gradient { stops, direction: gradient.direction }
            }
            Fill::Pattern(pattern) => Self::Pattern {
                mask: mask_of(&pattern.name),
                foreground: Color::from_hex(&pattern.foreground).unwrap_or(Color::BLACK),
                background: Color::from_hex(&pattern.background).unwrap_or(Color::WHITE),
            },
        }
    }

    /// Whether it draws anything at all.
    #[must_use]
    pub fn is_nothing(&self) -> bool {
        *self == Self::None
    }

    /// The one colour this is, when it is one colour.
    ///
    /// For the things that want a colour and cannot take a shade: the outline
    /// of a cell, the sample in a gallery. A gradient answers with the colour
    /// at its middle, which is the nearest one colour to it there is.
    #[must_use]
    pub fn colour(&self) -> Option<Color> {
        match self {
            Self::None => None,
            Self::Solid(colour) => Some(*colour),
            Self::Gradient { .. } => Some(self.at(0.5, 0.5)),
            Self::Pattern { foreground, .. } => Some(*foreground),
        }
    }

    /// The colour at a point of the shape, given as fractions of its box.
    #[must_use]
    pub fn at(&self, across: f32, down: f32) -> Color {
        match self {
            Self::None => Color::TRANSPARENT,
            Self::Solid(colour) => *colour,
            Self::Gradient { stops, direction } => {
                let along = match direction {
                    Direction::Linear(angle) => {
                        // How far along the line at that angle the point is,
                        // measured so that nought is the first corner the line
                        // meets and one is the last.
                        let radians = *angle as f32 / 21_600_000.0 * core::f32::consts::TAU;
                        let (sin, cos) = radians.sin_cos();
                        let reach = cos.abs() + sin.abs();
                        let from = ((1.0 - cos.signum()) / 2.0, (1.0 - sin.signum()) / 2.0);
                        ((across - from.0) * cos + (down - from.1) * sin) / reach.max(0.0001)
                    }
                    // Outwards from the middle: how far from it, as a fraction
                    // of the way to the edge.
                    Direction::Radial => {
                        let (x, y) = (across - 0.5, down - 0.5);
                        (x * x + y * y).sqrt() * 2.0
                    }
                    Direction::Rectangular => ((across - 0.5).abs()).max((down - 0.5).abs()) * 2.0,
                };
                colour_along(stops, along.clamp(0.0, 1.0))
            }
            Self::Pattern { .. } => *foreground_of(self),
        }
    }

    /// The colour at a pixel of the canvas.
    ///
    /// A hatching is measured in pixels rather than in fractions of the shape:
    /// it is eight pixels across whatever it fills, which is what makes a
    /// hatched shape look the same at any size.
    #[must_use]
    pub fn at_pixel(&self, x: usize, y: usize, across: f32, down: f32) -> Color {
        match self {
            Self::Pattern { mask, foreground, background } => {
                let row = mask[y % 8];
                let set = (row >> (7 - x % 8)) & 1 == 1;
                if set {
                    *foreground
                } else {
                    *background
                }
            }
            _ => self.at(across, down),
        }
    }
}

fn foreground_of(paint: &Paint) -> &Color {
    match paint {
        Paint::Pattern { foreground, .. } => foreground,
        _ => &Color::BLACK,
    }
}

/// The colour a run of stops has at a point along it.
fn colour_along(stops: &[(f32, Color)], along: f32) -> Color {
    let Some(first) = stops.first() else { return Color::WHITE };
    if along <= first.0 {
        return first.1;
    }
    for pair in stops.windows(2) {
        let (before, after) = (pair[0], pair[1]);
        if along > after.0 {
            continue;
        }
        let span = (after.0 - before.0).max(0.0001);
        let how_far = ((along - before.0) / span).clamp(0.0, 1.0);
        return mixed(before.1, after.1, how_far);
    }
    stops.last().map_or(Color::WHITE, |stop| stop.1)
}

/// Two colours in proportion.
fn mixed(from: Color, to: Color, how_far: f32) -> Color {
    let blend = |one: u8, other: u8| {
        (f32::from(one) + (f32::from(other) - f32::from(one)) * how_far).round().clamp(0.0, 255.0)
            as u8
    };
    Color::rgba(
        blend(from.red, to.red),
        blend(from.green, to.green),
        blend(from.blue, to.blue),
        blend(from.alpha, to.alpha),
    )
}

/// The eight-by-eight mask a named hatching is.
///
/// The percentages are worked out from how dark they are; the rest are written
/// down, because each is its own arrangement. A name this does not know is
/// drawn as the half-and-half dither, which is visibly a hatching of about the
/// right weight rather than a shape pretending to be solid.
#[must_use]
pub fn mask_of(name: &str) -> [u8; 8] {
    if let Some(rest) = name.strip_prefix("pct") {
        if let Ok(percent) = rest.parse::<u32>() {
            return dither(percent);
        }
    }
    match name {
        // Straight lines, at three weights each.
        "horz" => rows(&[0, 0, 0, 0xFF, 0, 0, 0, 0]),
        "ltHorz" => rows(&[0, 0, 0, 0, 0, 0, 0, 0xFF]),
        "dkHorz" => rows(&[0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF]),
        "narHorz" => rows(&[0, 0xFF, 0, 0xFF, 0, 0xFF, 0, 0xFF]),
        "vert" => columns(0b0001_0000),
        "ltVert" => columns(0b0000_0001),
        "dkVert" => columns(0b0011_0011),
        "narVert" => columns(0b0101_0101),
        // Diagonals, up and down, at three weights.
        "ltUpDiag" => rows(&[0x88, 0x44, 0x22, 0x11, 0x88, 0x44, 0x22, 0x11]),
        "ltDnDiag" => rows(&[0x11, 0x22, 0x44, 0x88, 0x11, 0x22, 0x44, 0x88]),
        "dkUpDiag" => rows(&[0xCC, 0x66, 0x33, 0x99, 0xCC, 0x66, 0x33, 0x99]),
        "dkDnDiag" => rows(&[0x99, 0x33, 0x66, 0xCC, 0x99, 0x33, 0x66, 0xCC]),
        "wdUpDiag" => rows(&[0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01]),
        "wdDnDiag" => rows(&[0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80]),
        // And the crosses, which are two of the above at once.
        "cross" => rows(&[0x10, 0x10, 0x10, 0xFF, 0x10, 0x10, 0x10, 0x10]),
        "dkCross" => rows(&[0x11, 0x11, 0xFF, 0x11, 0x11, 0x11, 0xFF, 0x11]),
        "diagCross" => rows(&[0x81, 0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x81]),
        "smCheck" => rows(&[0xCC, 0xCC, 0x33, 0x33, 0xCC, 0xCC, 0x33, 0x33]),
        "lgCheck" => rows(&[0xF0, 0xF0, 0xF0, 0xF0, 0x0F, 0x0F, 0x0F, 0x0F]),
        "openDmnd" => rows(&[0x18, 0x24, 0x42, 0x81, 0x81, 0x42, 0x24, 0x18]),
        "solidDmnd" => rows(&[0x18, 0x3C, 0x7E, 0xFF, 0xFF, 0x7E, 0x3C, 0x18]),
        "trellis" => rows(&[0xFF, 0x44, 0xFF, 0x11, 0xFF, 0x44, 0xFF, 0x11]),
        "shingle" => rows(&[0x80, 0x40, 0x20, 0x1F, 0x02, 0x04, 0x08, 0xF0]),
        "zigZag" => rows(&[0x81, 0x42, 0x24, 0x18, 0x81, 0x42, 0x24, 0x18]),
        "wave" => rows(&[0x00, 0x00, 0x63, 0x94, 0x08, 0x00, 0x00, 0x00]),
        "weave" => rows(&[0x88, 0x54, 0x22, 0x45, 0x88, 0x15, 0x22, 0x51]),
        "plaid" => rows(&[0x55, 0xAA, 0x55, 0xAA, 0x0F, 0x0F, 0x0F, 0x0F]),
        "divot" => rows(&[0x00, 0x10, 0x08, 0x00, 0x00, 0x01, 0x80, 0x00]),
        "dotGrid" => rows(&[0x11, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00]),
        "dotDmnd" => rows(&[0x11, 0x00, 0x44, 0x00, 0x11, 0x00, 0x44, 0x00]),
        "sphere" => rows(&[0x3C, 0x7E, 0xFF, 0xFF, 0xFF, 0xFF, 0x7E, 0x3C]),
        "horzBrick" => rows(&[0xFF, 0x10, 0x10, 0x10, 0xFF, 0x01, 0x01, 0x01]),
        "diagBrick" => rows(&[0x81, 0x42, 0x24, 0x18, 0x10, 0x20, 0x40, 0x80]),
        _ => dither(50),
    }
}

/// Eight rows, as they are written.
fn rows(values: &[u8; 8]) -> [u8; 8] {
    *values
}

/// The same byte eight times, which is how a pattern of upright lines is made.
fn columns(value: u8) -> [u8; 8] {
    [value; 8]
}

/// A dither of a given darkness, as a fraction of a hundred.
///
/// The sixty-four places of an ordered dither, each turned on once the pattern
/// is dark enough to reach it. It is what the percentage hatchings are: the
/// same arrangement at sixteen weights.
fn dither(percent: u32) -> [u8; 8] {
    // The order the sixty-four places fill up in, which is the arrangement that
    // spreads them as evenly as a grid allows.
    const ORDER: [[u8; 8]; 8] = [
        [0, 48, 12, 60, 3, 51, 15, 63],
        [32, 16, 44, 28, 35, 19, 47, 31],
        [8, 56, 4, 52, 11, 59, 7, 55],
        [40, 24, 36, 20, 43, 27, 39, 23],
        [2, 50, 14, 62, 1, 49, 13, 61],
        [34, 18, 46, 30, 33, 17, 45, 29],
        [10, 58, 6, 54, 9, 57, 5, 53],
        [42, 26, 38, 22, 41, 25, 37, 21],
    ];
    let wanted = (percent.min(100) * 64 / 100) as u8;
    let mut mask = [0u8; 8];
    for (row, places) in ORDER.iter().enumerate() {
        for (column, place) in places.iter().enumerate() {
            if *place < wanted {
                mask[row] |= 0x80 >> column;
            }
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::fills::{Gradient, Pattern};

    #[test]
    fn nothing_is_nothing() {
        assert!(Paint::of(&Fill::None).is_nothing());
        assert_eq!(Paint::of(&Fill::None).colour(), None);
    }

    #[test]
    fn one_colour_comes_through_as_that_colour() {
        let paint = Paint::of(&Fill::Solid("4472C4".to_owned()));
        assert_eq!(paint.colour(), Some(Color::rgb(0x44, 0x72, 0xC4)));
        assert_eq!(paint.at(0.0, 0.0), Color::rgb(0x44, 0x72, 0xC4));
        assert_eq!(paint.at(1.0, 1.0), Color::rgb(0x44, 0x72, 0xC4));
    }

    fn black_to_white(direction: Direction) -> Paint {
        Paint::of(&Fill::Gradient(Gradient {
            stops: vec![(0, "000000".to_owned()), (100_000, "FFFFFF".to_owned())],
            direction,
        }))
    }

    #[test]
    fn a_gradient_straight_down_is_dark_at_the_top() {
        let paint = black_to_white(Direction::Linear(5_400_000));
        assert_eq!(paint.at(0.5, 0.0), Color::BLACK, "the top");
        assert_eq!(paint.at(0.5, 1.0), Color::WHITE, "the bottom");
        assert_eq!(paint.at(0.5, 0.5).red, 128, "and halfway between");
    }

    #[test]
    fn a_gradient_straight_across_is_dark_at_the_left() {
        let paint = black_to_white(Direction::Linear(0));
        assert_eq!(paint.at(0.0, 0.5), Color::BLACK, "the left");
        assert_eq!(paint.at(1.0, 0.5), Color::WHITE, "the right");
    }

    #[test]
    fn a_gradient_at_an_angle_runs_corner_to_corner() {
        // Forty-five degrees: from the top left corner to the bottom right.
        let paint = black_to_white(Direction::Linear(2_700_000));
        assert_eq!(paint.at(0.0, 0.0), Color::BLACK, "the corner it starts at");
        assert_eq!(paint.at(1.0, 1.0), Color::WHITE, "the one it ends at");
        assert_eq!(paint.at(0.5, 0.5).red, 128, "and the middle is halfway");
    }

    #[test]
    fn a_gradient_the_other_way_round_starts_at_the_other_end() {
        // Straight up rather than straight down.
        let paint = black_to_white(Direction::Linear(16_200_000));
        assert_eq!(paint.at(0.5, 1.0), Color::BLACK, "the bottom");
        assert_eq!(paint.at(0.5, 0.0), Color::WHITE, "the top");
    }

    #[test]
    fn a_gradient_outwards_is_dark_in_the_middle() {
        let round = black_to_white(Direction::Radial);
        assert_eq!(round.at(0.5, 0.5), Color::BLACK, "the middle");
        assert_eq!(round.at(0.5, 1.0), Color::WHITE, "and the edge");
        // The corner is further out than the edge, so it is clamped to the end.
        assert_eq!(round.at(1.0, 1.0), Color::WHITE);
    }

    #[test]
    fn a_gradient_outwards_in_rectangles_reaches_the_corners() {
        let square = black_to_white(Direction::Rectangular);
        assert_eq!(square.at(0.5, 0.5), Color::BLACK);
        assert_eq!(square.at(1.0, 1.0), Color::WHITE, "the corner is the end of it");
        assert_eq!(square.at(1.0, 0.5), Color::WHITE, "and so is the middle of a side");
    }

    #[test]
    fn a_hatching_is_two_colours_in_a_pattern() {
        let paint = Paint::of(&Fill::Pattern(Pattern {
            name: "ltHorz".to_owned(),
            foreground: "FF0000".to_owned(),
            background: "0000FF".to_owned(),
        }));
        // The light horizontal hatching is one row in eight.
        assert_eq!(paint.at_pixel(3, 7, 0.0, 0.0), Color::rgb(0xFF, 0, 0), "the line");
        assert_eq!(paint.at_pixel(3, 3, 0.0, 0.0), Color::rgb(0, 0, 0xFF), "and between them");
    }

    #[test]
    fn a_hatching_repeats_every_eight_pixels() {
        let paint = Paint::of(&Fill::Pattern(Pattern {
            name: "ltHorz".to_owned(),
            foreground: "FF0000".to_owned(),
            background: "0000FF".to_owned(),
        }));
        assert_eq!(paint.at_pixel(0, 7, 0.0, 0.0), paint.at_pixel(0, 15, 0.0, 0.0));
        assert_eq!(paint.at_pixel(0, 7, 0.0, 0.0), paint.at_pixel(8, 7, 0.0, 0.0));
    }

    #[test]
    fn the_percentages_get_darker_in_order() {
        // Each is darker than the one before it, which is the whole of what
        // the family means.
        let mut before = 0;
        for percent in [5u32, 10, 20, 25, 30, 40, 50, 60, 70, 75, 80, 90] {
            let set: u32 = dither(percent).iter().map(|row| row.count_ones()).sum();
            assert!(set >= before, "pct{percent} is lighter than the one before it");
            before = set;
        }
        assert_eq!(dither(0).iter().map(|row| row.count_ones()).sum::<u32>(), 0);
        assert_eq!(dither(100).iter().map(|row| row.count_ones()).sum::<u32>(), 64);
    }

    #[test]
    fn a_hatching_nobody_knows_is_a_hatching_and_not_a_solid_shape() {
        let unknown = mask_of("herringbone");
        let set: u32 = unknown.iter().map(|row| row.count_ones()).sum();
        assert!(set > 0 && set < 64, "it should be a pattern, not solid and not nothing");
    }

    #[test]
    fn every_hatching_the_format_names_draws_something() {
        for name in [
            "pct5",
            "pct25",
            "pct50",
            "pct75",
            "horz",
            "vert",
            "ltHorz",
            "ltVert",
            "dkHorz",
            "dkVert",
            "narHorz",
            "narVert",
            "ltUpDiag",
            "ltDnDiag",
            "dkUpDiag",
            "dkDnDiag",
            "wdUpDiag",
            "wdDnDiag",
            "cross",
            "dkCross",
            "diagCross",
            "smCheck",
            "lgCheck",
            "openDmnd",
            "solidDmnd",
            "trellis",
            "shingle",
            "zigZag",
            "wave",
            "weave",
            "plaid",
            "divot",
            "dotGrid",
            "dotDmnd",
            "sphere",
            "horzBrick",
            "diagBrick",
        ] {
            let set: u32 = mask_of(name).iter().map(|row| row.count_ones()).sum();
            assert!(set > 0, "{name} draws nothing at all");
            assert!(set < 64, "{name} is solid, which is not a hatching");
        }
    }
}

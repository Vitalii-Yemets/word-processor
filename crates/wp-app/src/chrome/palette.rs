//! The grid of colours the two coloured buttons drop open.
//!
//! # Why the two are not the same list
//!
//! A font colour is any colour at all — the format stores six hex digits. A
//! highlight is not: the format stores a *name* from a fixed list of sixteen,
//! which is exactly why Word's highlighter offers sixteen swatches and its font
//! colour offers a palette with "More Colours" under it. Offering a highlight
//! that the format cannot store would be a lie that only showed up on saving.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

use super::theme::Theme;

/// The side of one swatch and the gap around the grid.
const SWATCH: f32 = 18.0;
const GAP: f32 = 2.0;
const PADDING: f32 = 8.0;
/// Room under the grid for the row that turns the colour off.
const FOOTER: f32 = 24.0;

/// The font colours offered: Word's standard row, then its greys and tints.
///
/// The first entry is "automatic", which is not a colour but an instruction to
/// use whatever reads against the paper.
pub const TEXT_COLORS: &[(&str, Option<&str>)] = &[
    ("Automatic", None),
    ("Black", Some("000000")),
    ("Dark Grey", Some("404040")),
    ("Grey", Some("808080")),
    ("Light Grey", Some("BFBFBF")),
    ("White", Some("FFFFFF")),
    ("Dark Red", Some("C00000")),
    ("Red", Some("FF0000")),
    ("Orange", Some("FFC000")),
    ("Yellow", Some("FFFF00")),
    ("Light Green", Some("92D050")),
    ("Green", Some("00B050")),
    ("Light Blue", Some("00B0F0")),
    ("Blue", Some("0070C0")),
    ("Dark Blue", Some("002060")),
    ("Purple", Some("7030A0")),
];

/// The highlights offered, which are the sixteen names the format allows.
pub const HIGHLIGHTS: &[(&str, Option<&str>)] = &[
    ("No Colour", None),
    ("Yellow", Some("yellow")),
    ("Bright Green", Some("green")),
    ("Turquoise", Some("cyan")),
    ("Pink", Some("magenta")),
    ("Blue", Some("blue")),
    ("Red", Some("red")),
    ("Dark Blue", Some("darkBlue")),
    ("Teal", Some("darkCyan")),
    ("Green", Some("darkGreen")),
    ("Violet", Some("darkMagenta")),
    ("Dark Red", Some("darkRed")),
    ("Dark Yellow", Some("darkYellow")),
    ("Grey 50%", Some("darkGray")),
    ("Grey 25%", Some("lightGray")),
    ("Black", Some("black")),
];

/// How many swatches sit in a row.
const COLUMNS: usize = 8;

/// Which of the three coloured buttons a palette belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The colour of the letters themselves.
    Text,
    /// The band drawn behind them, which the format stores by name.
    Highlight,
    /// The colour behind a whole paragraph.
    Shading,
    /// The colour of the paper itself.
    Page,
}

impl Kind {
    #[must_use]
    fn title(self) -> &'static str {
        match self {
            Self::Text => "Font Colour",
            Self::Highlight => "Highlight",
            Self::Shading => "Shading",
            Self::Page => "Page Colour",
        }
    }
}

/// The palette, and where the pointer is over it.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub kind: Kind,
    left: f32,
    top: f32,
    hovered: Option<usize>,
}

impl Palette {
    #[must_use]
    pub fn new(kind: Kind, left: f32, top: f32) -> Self {
        Self { kind, left, top, hovered: None }
    }

    /// The list this palette is showing.
    #[must_use]
    pub fn entries(&self) -> &'static [(&'static str, Option<&'static str>)] {
        match self.kind {
            // The highlighter offers names because names are all the format
            // will store; the other two offer colours.
            Kind::Highlight => HIGHLIGHTS,
            Kind::Text | Kind::Shading | Kind::Page => TEXT_COLORS,
        }
    }

    #[must_use]
    pub fn width() -> f32 {
        COLUMNS as f32 * (SWATCH + GAP) - GAP + PADDING * 2.0
    }

    #[must_use]
    pub fn height(&self) -> f32 {
        let rows = self.entries().len().div_ceil(COLUMNS);
        rows as f32 * (SWATCH + GAP) - GAP + PADDING * 2.0 + FOOTER
    }

    #[must_use]
    pub fn covers(&self, x: i32, y: i32) -> bool {
        let (x, y) = (x as f32, y as f32);
        x >= self.left
            && x < self.left + Self::width()
            && y >= self.top
            && y < self.top + self.height()
    }

    /// Which swatch a point is on, if any.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        let inside_x = x as f32 - self.left - PADDING;
        let inside_y = y as f32 - self.top - PADDING;
        if inside_x < 0.0 || inside_y < 0.0 {
            return None;
        }
        let column = (inside_x / (SWATCH + GAP)) as usize;
        let row = (inside_y / (SWATCH + GAP)) as usize;
        let index = row * COLUMNS + column;
        (column < COLUMNS && index < self.entries().len()).then_some(index)
    }

    /// Lights up whatever the pointer is over. Returns whether that changed.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let found = self.hit(x, y);
        let changed = found != self.hovered;
        self.hovered = found;
        changed
    }

    pub fn draw(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
    ) {
        let (left, top) = (self.left as i32, self.top as i32);
        let (width, height) = (Self::width() as i32, self.height() as i32);
        canvas.fill_rect(left, top, width, height, theme.pane);
        outline(canvas, left, top, width, height, theme.field_edge);

        for (index, (_, value)) in self.entries().iter().enumerate() {
            let x = self.left + PADDING + (index % COLUMNS) as f32 * (SWATCH + GAP);
            let y = self.top + PADDING + (index / COLUMNS) as f32 * (SWATCH + GAP);

            match value {
                Some(value) => {
                    let colour = swatch_color(value, self.kind).unwrap_or(theme.pane);
                    canvas.fill_rect(x as i32, y as i32, SWATCH as i32, SWATCH as i32, colour);
                }
                // "Automatic" and "no colour" have no colour to show, so they
                // are drawn as an empty box with a line through it.
                None => {
                    canvas.fill_rect(x as i32, y as i32, SWATCH as i32, SWATCH as i32, theme.field);
                    for step in 0..SWATCH as i32 {
                        canvas.fill_rect(x as i32 + step, y as i32 + step, 1, 1, theme.dim_text);
                    }
                }
            }

            let edge = if self.hovered == Some(index) { theme.emphasis } else { theme.field_edge };
            outline(canvas, x as i32, y as i32, SWATCH as i32, SWATCH as i32, edge);
        }

        // The name of whatever the pointer is over, along the bottom, which is
        // how anyone learns what "darkCyan" looks like.
        let name = self
            .hovered
            .and_then(|index| self.entries().get(index))
            .map_or(self.kind.title(), |(name, _)| name);
        let line = engine.simple_line(
            name,
            self.left + PADDING,
            self.top + self.height() - 8.0,
            8.0,
            theme.text,
        );
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
    }
}

/// The colour a swatch shows.
///
/// A font colour is six hex digits; a highlight is one of the fixed names, and
/// the two are not interchangeable.
#[must_use]
pub fn swatch_color(value: &str, kind: Kind) -> Option<Color> {
    match kind {
        Kind::Highlight => highlight_color(value),
        Kind::Text | Kind::Shading | Kind::Page => Color::from_hex(value),
    }
}

/// The colour one of the format's highlight names stands for.
#[must_use]
pub fn highlight_color(name: &str) -> Option<Color> {
    Some(match name {
        "black" => Color::rgb(0x00, 0x00, 0x00),
        "blue" => Color::rgb(0x00, 0x00, 0xFF),
        "cyan" => Color::rgb(0x00, 0xFF, 0xFF),
        "green" => Color::rgb(0x00, 0xFF, 0x00),
        "magenta" => Color::rgb(0xFF, 0x00, 0xFF),
        "red" => Color::rgb(0xFF, 0x00, 0x00),
        "yellow" => Color::rgb(0xFF, 0xFF, 0x00),
        "white" => Color::rgb(0xFF, 0xFF, 0xFF),
        "darkBlue" => Color::rgb(0x00, 0x00, 0x80),
        "darkCyan" => Color::rgb(0x00, 0x80, 0x80),
        "darkGreen" => Color::rgb(0x00, 0x80, 0x00),
        "darkMagenta" => Color::rgb(0x80, 0x00, 0x80),
        "darkRed" => Color::rgb(0x80, 0x00, 0x00),
        "darkYellow" => Color::rgb(0x80, 0x80, 0x00),
        "darkGray" => Color::rgb(0x80, 0x80, 0x80),
        "lightGray" => Color::rgb(0xC0, 0xC0, 0xC0),
        _ => return None,
    })
}

fn outline(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32, colour: Color) {
    canvas.fill_rect(x, y, width, 1, colour);
    canvas.fill_rect(x, y + height - 1, width, 1, colour);
    canvas.fill_rect(x, y, 1, height, colour);
    canvas.fill_rect(x + width - 1, y, 1, height, colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every highlight offered has to be a name the format allows, or saving
    /// would quietly drop it.
    #[test]
    fn every_highlight_is_a_name_the_format_knows() {
        for (label, value) in HIGHLIGHTS {
            if let Some(value) = value {
                assert!(
                    highlight_color(value).is_some(),
                    "{label} names {value:?}, which is not one"
                );
            }
        }
    }

    #[test]
    fn every_font_colour_is_six_hex_digits() {
        for (label, value) in TEXT_COLORS {
            if let Some(value) = value {
                assert!(
                    Color::from_hex(value).is_some(),
                    "{label} is {value:?}, which is not a colour"
                );
            }
        }
    }

    #[test]
    fn a_swatch_can_be_found_by_where_it_was_drawn() {
        let palette = Palette::new(Kind::Text, 100.0, 200.0);
        // The first swatch sits at the padding offset from the corner.
        assert_eq!(palette.hit(100 + PADDING as i32 + 2, 200 + PADDING as i32 + 2), Some(0));
        assert_eq!(palette.hit(90, 200), None);
    }
}

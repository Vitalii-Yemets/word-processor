//! What colour everything is.
//!
//! # Why the page changes colour too
//!
//! A dark window with a white page in the middle of it is not a dark theme; it
//! is a lamp. Word learned this and turns the paper dark as well, and a
//! document's text along with it — because text a document calls "automatic" is
//! not black, it is *the colour that reads against the paper*, and the paper
//! has changed.
//!
//! A colour the document sets on purpose is left exactly as it was: an author
//! who made a heading red meant red, whatever the paper is.

use wp_raster::Color;

/// Which way round the colours go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }
}

/// Every colour the window uses, in one place.
///
/// One structure rather than constants scattered through the drawing code: a
/// theme that can be switched has to be a value, and a colour that is only
/// right in one theme is a bug waiting for the other one.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub mode: Mode,

    /// Behind the pages.
    pub desk: Color,
    /// The paper itself, and the line around it.
    pub page: Color,
    pub page_edge: Color,
    /// What text is drawn in when the document says "automatic".
    pub page_text: Color,
    /// The band behind selected text, and what marks are drawn in.
    pub selection: Color,
    pub caret: Color,
    pub marks: Color,
    /// The lines of a table when the document does not say what colour.
    pub table_line: Color,

    /// The strip at the very top.
    pub title_bar: Color,
    /// The tabs, and the ribbon under them.
    pub tab_strip: Color,
    pub ribbon: Color,
    pub ribbon_edge: Color,
    pub group_separator: Color,

    /// The navigation pane and the status strip.
    pub pane: Color,
    pub pane_edge: Color,
    pub status: Color,

    /// Text on the furniture, and the quieter sort beside it.
    pub text: Color,
    pub dim_text: Color,
    pub disabled_text: Color,

    /// A button under the pointer, and one that is switched on.
    pub hover: Color,
    pub accent: Color,
    /// The strong blue that underlines the open tab and outlines a box with
    /// the keyboard. Strong in both themes, unlike `accent`, which has to be
    /// something text can be read against.
    pub emphasis: Color,
    /// The red a close button turns under the pointer.
    pub danger: Color,

    /// A box that shows a value rather than doing something.
    pub field: Color,
    pub field_edge: Color,

    /// The faint grid the View tab can put over the page.
    pub gridline: Color,

    /// The rulers: the paper, its margins, and the marks on them.
    pub ruler_paper: Color,
    pub ruler_margin: Color,
    pub ruler_tick: Color,
}

impl Theme {
    #[must_use]
    pub fn of(mode: Mode) -> Self {
        match mode {
            Mode::Dark => Self::dark(),
            Mode::Light => Self::light(),
        }
    }

    /// The dark theme, which is what the reference screenshot shows.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            mode: Mode::Dark,

            desk: Color::rgb(0x1E, 0x1E, 0x1E),
            // Lighter than the ribbon above it, so the paper reads as paper and
            // not as more window.
            page: Color::rgb(0x33, 0x33, 0x33),
            page_edge: Color::rgb(0x14, 0x14, 0x14),
            page_text: Color::rgb(0xE6, 0xE6, 0xE6),
            selection: Color::rgb(0x2D, 0x4F, 0x7C),
            caret: Color::rgb(0xE6, 0xE6, 0xE6),
            marks: Color::rgb(0x6E, 0x8A, 0xC8),
            table_line: Color::rgb(0x7A, 0x7A, 0x7A),

            title_bar: Color::rgb(0x1F, 0x1F, 0x1F),
            tab_strip: Color::rgb(0x1F, 0x1F, 0x1F),
            ribbon: Color::rgb(0x2B, 0x2B, 0x2B),
            ribbon_edge: Color::rgb(0x14, 0x14, 0x14),
            group_separator: Color::rgb(0x3C, 0x3C, 0x3C),

            pane: Color::rgb(0x25, 0x25, 0x25),
            pane_edge: Color::rgb(0x16, 0x16, 0x16),
            status: Color::rgb(0x1F, 0x1F, 0x1F),

            text: Color::rgb(0xEA, 0xEA, 0xEA),
            dim_text: Color::rgb(0x9C, 0x9C, 0x9C),
            disabled_text: Color::rgb(0x6A, 0x6A, 0x6A),

            hover: Color::rgb(0x3D, 0x3D, 0x3D),
            accent: Color::rgb(0x2B, 0x57, 0x9A),
            emphasis: Color::rgb(0x59, 0x8B, 0xD6),
            danger: Color::rgb(0xC4, 0x2B, 0x1C),

            field: Color::rgb(0x1A, 0x1A, 0x1A),
            field_edge: Color::rgb(0x4A, 0x4A, 0x4A),

            gridline: Color::rgb(0x45, 0x45, 0x45),
            ruler_paper: Color::rgb(0x5E, 0x5E, 0x5E),
            ruler_margin: Color::rgb(0x38, 0x38, 0x38),
            ruler_tick: Color::rgb(0xA0, 0xA0, 0xA0),
        }
    }

    /// The light theme: white paper, and furniture the colour of Word's own.
    #[must_use]
    pub fn light() -> Self {
        Self {
            mode: Mode::Light,

            desk: Color::rgb(0x8A, 0x8A, 0x8A),
            page: Color::WHITE,
            page_edge: Color::rgb(0x5A, 0x5A, 0x5A),
            page_text: Color::BLACK,
            selection: Color::rgb(0xB4, 0xD5, 0xFE),
            caret: Color::rgb(0x10, 0x50, 0xC0),
            marks: Color::rgb(0x30, 0x50, 0x9A),
            table_line: Color::rgb(0x40, 0x40, 0x40),

            title_bar: Color::rgb(0x1F, 0x3B, 0x63),
            tab_strip: Color::rgb(0x1F, 0x3B, 0x63),
            ribbon: Color::rgb(0xF3, 0xF3, 0xF3),
            ribbon_edge: Color::rgb(0xD0, 0xD0, 0xD0),
            group_separator: Color::rgb(0xD6, 0xD6, 0xD6),

            pane: Color::rgb(0xFA, 0xFA, 0xFA),
            pane_edge: Color::rgb(0xD0, 0xD0, 0xD0),
            status: Color::rgb(0x1F, 0x3B, 0x63),

            text: Color::rgb(0x20, 0x20, 0x20),
            dim_text: Color::rgb(0x5E, 0x5E, 0x5E),
            disabled_text: Color::rgb(0xA6, 0xA6, 0xA6),

            hover: Color::rgb(0xE1, 0xE1, 0xE1),
            accent: Color::rgb(0xC7, 0xDC, 0xF5),
            emphasis: Color::rgb(0x2B, 0x57, 0x9A),
            danger: Color::rgb(0xC4, 0x2B, 0x1C),

            field: Color::WHITE,
            field_edge: Color::rgb(0xB0, 0xB0, 0xB0),

            gridline: Color::rgb(0xDC, 0xDC, 0xDC),
            ruler_paper: Color::WHITE,
            ruler_margin: Color::rgb(0xC4, 0xC4, 0xC4),
            ruler_tick: Color::rgb(0x60, 0x60, 0x60),
        }
    }

    /// What text on the title bar and the status strip is drawn in.
    ///
    /// Those two are dark in both themes — Word's light theme keeps them blue —
    /// so their text is light in both as well.
    #[must_use]
    pub fn bar_text(&self) -> Color {
        match self.mode {
            Mode::Dark => self.text,
            Mode::Light => Color::rgb(0xF0, 0xF0, 0xF0),
        }
    }

    /// The dimmer sort, on those same two strips.
    #[must_use]
    pub fn bar_dim_text(&self) -> Color {
        match self.mode {
            Mode::Dark => self.dim_text,
            Mode::Light => Color::rgb(0xC0, 0xCC, 0xDC),
        }
    }

    /// What a button on the title bar turns under the pointer.
    #[must_use]
    pub fn bar_hover(&self) -> Color {
        match self.mode {
            Mode::Dark => self.hover,
            Mode::Light => Color::rgb(0x33, 0x55, 0x86),
        }
    }

    /// The colour a button that is switched on is filled with, and the text on
    /// it.
    #[must_use]
    pub fn on_accent(&self) -> Color {
        match self.mode {
            Mode::Dark => Color::WHITE,
            Mode::Light => Color::rgb(0x10, 0x20, 0x30),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

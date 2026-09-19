//! The bar across the top of a document that says something about it.
//!
//! # What it is for
//!
//! Word puts one there when the document is not quite what it looks like: it
//! is open read-only, it carries macros that have not been run, it was
//! recovered, it is marked as final. The bar says which, and offers the one
//! thing a person would want to do about it — Edit Anyway, Enable Content.
//!
//! # Why it is furniture rather than a message
//!
//! Because those things are true for as long as the document is open, and a
//! message in the strip along the bottom is gone by the next keystroke. A
//! person who starts typing into a read-only document and finds nothing
//! arriving has to be told why *then*, not told once before they started.
//!
//! So: a strip under the ribbon, in a colour that is not the ribbon's, with
//! one button and a way to shut it. Shutting it is Word's behaviour too, and
//! it does not change what is true — the File page still says so.

use wp_raster::{Canvas, Color};

use wp_layout::{LayoutEngine, Renderer};

use super::theme::Theme;

/// How tall the bar is.
pub const HEIGHT: f32 = 30.0;

/// The room down each side, and how tall the button is.
const PADDING: f32 = 12.0;
const BUTTON_HEIGHT: f32 = 20.0;

/// What a press on the bar hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The one button it offers.
    Button,
    /// The cross at the right-hand end.
    Close,
}

/// Why the bar is there.
///
/// Each says what the bar says and what its button does, so that the two
/// cannot drift apart: a bar reading "open read-only" with a button that
/// enables macros would be a bar nobody could trust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Because {
    /// The document asked to be opened read-only and was. See
    /// [`crate::editor`]'s read-only module.
    ReadOnly,
    /// It carries macros written in Visual Basic, which are kept and not run.
    Macros,
    /// It was put back after the program stopped without saving it.
    Recovered,
}

impl Because {
    /// Every reason a bar can be up, for the catalogue of everything the
    /// program can say.
    pub const ALL: &'static [Self] = &[Self::ReadOnly, Self::Macros, Self::Recovered];

    /// What the bar says.
    #[must_use]
    pub fn said(self) -> &'static str {
        match self {
            Self::ReadOnly => "This document is open read-only.",
            Self::Macros => {
                "This document carries macros. They are disabled until you say otherwise."
            }
            Self::Recovered => "This document was recovered after the program stopped.",
        }
    }

    /// And what its button offers, if it offers one.
    #[must_use]
    pub fn button(self) -> Option<&'static str> {
        match self {
            Self::ReadOnly => Some("Edit Anyway"),
            // Word's own words, and the one decision a person opening a
            // document with macros in it is being asked to make.
            Self::Macros => Some("Enable Content"),
            Self::Recovered => Some("Save As"),
        }
    }
}

/// The bar itself.
#[derive(Clone, Debug)]
pub struct InfoBar {
    pub because: Because,
    /// Where each part ended up when it was last drawn, so a press can be
    /// matched to it.
    placed: Vec<(Hit, f32, f32, f32, f32)>,
}

impl InfoBar {
    #[must_use]
    pub fn new(because: Because) -> Self {
        Self { because, placed: Vec::new() }
    }

    /// The middle of the button, once the bar has been drawn: where a press
    /// on it lands, for a test that presses it.
    #[cfg(test)]
    #[must_use]
    pub fn button_middle(&self) -> Option<(i32, i32)> {
        self.placed.iter().find(|(hit, ..)| *hit == Hit::Button).map(
            |(_, left, top, width, height)| {
                ((left + width / 2.0) as i32, (top + height / 2.0) as i32)
            },
        )
    }

    /// What a point is over, if anything.
    #[must_use]
    pub fn at(&self, x: i32, y: i32) -> Option<Hit> {
        let x = super::mirror::flip(x) as f32;
        let y = y as f32;
        self.placed
            .iter()
            .find(|(_, left, top, width, height)| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(hit, ..)| *hit)
    }

    /// Draws it, and remembers where its parts went.
    ///
    /// Eight things to draw one strip, which is what drawing anything in
    /// this program takes: the canvas, the two halves of the text engine,
    /// where it goes, how wide it is, and the colours.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        top: f32,
        left_edge: f32,
        width: f32,
        theme: &Theme,
    ) {
        self.placed.clear();

        // A colour of its own, because the whole point of the bar is that it
        // is not part of the furniture a person has stopped seeing.
        let band = notice(theme);
        canvas.fill_rect(left_edge as i32, top as i32, width as i32, HEIGHT as i32, band);
        canvas.fill_rect(
            left_edge as i32,
            (top + HEIGHT - 1.0) as i32,
            width as i32,
            1,
            theme.ribbon_edge,
        );

        let middle = top + HEIGHT / 2.0 + 3.0;
        let line = engine.simple_line(
            crate::messages::t(self.because.said()),
            left_edge + PADDING,
            middle,
            8.5,
            theme.text,
        );
        let said_width = line.width - (left_edge + PADDING);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        // The cross first, from the right-hand end, so that the button can be
        // put beside it whatever the message is.
        let close_left = left_edge + width - PADDING - 16.0;
        let close_top = top + (HEIGHT - 16.0) / 2.0;
        cross(canvas, close_left, close_top, 16.0, theme.dim_text);
        self.placed.push((Hit::Close, close_left, close_top, 16.0, 16.0));

        let Some(label) = self.because.button() else { return };
        let label = crate::messages::t(label);
        let measured = engine.simple_line(label, 0.0, 0.0, 8.5, theme.text);
        let button_width = (measured.width + 20.0).max(80.0);
        let button_left =
            (left_edge + PADDING + said_width + PADDING).min(close_left - PADDING - button_width);
        let button_top = top + (HEIGHT - BUTTON_HEIGHT) / 2.0;

        canvas.fill_rect(
            button_left as i32,
            button_top as i32,
            button_width as i32,
            BUTTON_HEIGHT as i32,
            theme.page,
        );
        outline(canvas, button_left, button_top, button_width, BUTTON_HEIGHT, theme.field_edge);
        let line = engine.simple_line(
            label,
            button_left + (button_width - measured.width) / 2.0,
            button_top + BUTTON_HEIGHT / 2.0 + 3.0,
            8.5,
            theme.text,
        );
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        self.placed.push((Hit::Button, button_left, button_top, button_width, BUTTON_HEIGHT));
    }
}

/// The colour of the band: Word's is a pale yellow in the light theme.
///
/// Worked out from the theme rather than written down twice, so that a dark
/// window gets a dark band and the words on it stay readable.
fn notice(theme: &Theme) -> Color {
    match theme.mode {
        super::Mode::Dark => Color::rgb(0x4A, 0x40, 0x22),
        super::Mode::Light => Color::rgb(0xFF, 0xF4, 0xCE),
    }
}

/// A thin cross, the size of a close button.
fn cross(canvas: &mut Canvas, x: f32, y: f32, size: f32, colour: Color) {
    let inset = 4.0;
    let length = size - inset * 2.0;
    for step in 0..(length as i32) {
        canvas.fill_rect((x + inset) as i32 + step, (y + inset) as i32 + step, 1, 1, colour);
        canvas.fill_rect(
            (x + inset) as i32 + step,
            (y + size - inset) as i32 - step - 1,
            1,
            1,
            colour,
        );
    }
}

/// A one-pixel rectangle round something.
fn outline(canvas: &mut Canvas, x: f32, y: f32, width: f32, height: f32, colour: Color) {
    canvas.fill_rect(x as i32, y as i32, width as i32, 1, colour);
    canvas.fill_rect(x as i32, (y + height - 1.0) as i32, width as i32, 1, colour);
    canvas.fill_rect(x as i32, y as i32, 1, height as i32, colour);
    canvas.fill_rect((x + width - 1.0) as i32, y as i32, 1, height as i32, colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_says_something() {
        for because in [Because::ReadOnly, Because::Macros, Because::Recovered] {
            assert!(!because.said().trim().is_empty(), "{because:?} says nothing");
        }
    }

    #[test]
    fn every_bar_offers_the_one_thing_somebody_would_want_to_do_about_it() {
        // And what it offers is what its words are about: the bar that says
        // macros are disabled is the only place they can be enabled, and the
        // one that says the document is read-only is the only place that can
        // be undone. See [`crate::editor::trust`].
        assert_eq!(Because::Macros.button(), Some("Enable Content"));
        assert_eq!(Because::ReadOnly.button(), Some("Edit Anyway"));
        assert_eq!(Because::Recovered.button(), Some("Save As"));
    }

    #[test]
    fn a_point_on_nothing_hits_nothing() {
        let bar = InfoBar::new(Because::ReadOnly);
        assert_eq!(bar.at(10, 10), None, "a bar that has never been drawn has no parts");
    }
}

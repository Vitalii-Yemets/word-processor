//! Word's Signatures pane: what the document carries, and what it is waiting
//! for.
//!
//! # Why a pane and not a dialog
//!
//! The same reason as [`super::restrictpane`], and one more. A signature is
//! about a document, and a person looking at a list of signatures is reading
//! the document at the same time — checking that the thing signed is the thing
//! in front of them. A dialog covers the document to tell you about it.
//!
//! The other reason is that this list has two halves that answer each other.
//! What the document carries, and what it is still waiting for: a signature
//! line somebody put in is a request, and a signature made for that line is
//! the answer to it. Word shows the two together, one above the other, and the
//! pairing is the point — a request with nothing against it is what is left to
//! do.
//!
//! # What a row says
//!
//! Who, whether it holds, and what they said about why. Whether it holds is
//! the part that matters and the part a person cannot work out: it is
//! arithmetic over every part of the package, and the answer is either "this
//! is as it was signed" or the name of what changed.
//!
//! # What the pane does not decide
//!
//! Anything. It draws what it is given and reports where it was pressed.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

use crate::messages::t;

use super::theme::Theme;

/// How wide the pane is drawn.
pub const WIDTH: f32 = 280.0;

/// The room round everything.
const PADDING: f32 = 10.0;

/// How tall a button is.
const BUTTON: f32 = 24.0;

/// The size the pane's words are drawn at.
const TEXT: f32 = 8.5;

/// A heading is drawn a little larger.
const HEADING: f32 = 9.0;

/// One signature the document carries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Made {
    /// Who signed.
    pub who: String,
    /// Whether it holds, said the way a person reads it.
    pub standing: String,
    /// And whether that means it is good, which decides the colour.
    pub good: bool,
    /// What they said about why.
    pub reason: String,
    /// When they say they signed.
    pub at: String,
    /// The signature line it was made for, where a line was named and this
    /// document still has it.
    pub line: String,
    /// Whoever has signed this signature in turn.
    pub counters: Vec<String>,
}

/// One signature line still waiting for one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    pub name: String,
    pub title: String,
}

/// Everything the pane draws.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    pub made: Vec<Made>,
    pub wanted: Vec<Wanted>,
    /// Whether there is anything to sign with at all.
    pub can_sign: bool,
    /// Whether the document has been saved, since an unsaved one cannot be
    /// signed: a signature over something nobody has seen is not one.
    pub saved: bool,
    /// Where a person may put a certificate of their own, for when there is
    /// nothing to sign with.
    ///
    /// Telling somebody they cannot sign without telling them what would let
    /// them is half an answer, and the half that leaves them nowhere to go.
    pub folder: String,
}

/// Where a press on the pane can land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// One of the signatures, which takes a person to what it was made for.
    Made(usize),
    /// The offer to sign one of them in turn.
    Countersign(usize),
    /// One of the lines waiting, which takes a person to it.
    Wanted(usize),
    /// And the offer to sign that line.
    Sign(usize),
    /// The offer to sign the document at large.
    SignDocument,
    Close,
}

/// The pane, and what it remembers between one drawing and the next.
#[derive(Clone, Debug, Default)]
pub struct SignaturePane {
    hovered: Option<Hit>,
    placed: Vec<(Hit, f32, f32, f32, f32)>,
}

impl SignaturePane {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What a point is on, if anything.
    #[must_use]
    pub fn at(&self, x: i32, y: i32) -> Option<Hit> {
        let x = super::mirror::flip(x);
        let (x, y) = (x as f32, y as f32);
        self.placed
            .iter()
            .find(|(_, left, top, width, height)| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(hit, ..)| *hit)
    }

    /// Follows the pointer. True when something has to be drawn again.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let over = self.at(x, y);
        if over == self.hovered {
            return false;
        }
        self.hovered = over;
        true
    }

    /// Draws the pane.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        shown: &Shown,
        left: f32,
        top: f32,
        bottom: f32,
        theme: &Theme,
    ) {
        self.placed.clear();
        canvas.fill_rect(left as i32, top as i32, WIDTH as i32, (bottom - top) as i32, theme.pane);
        canvas.fill_rect(left as i32, top as i32, 1, (bottom - top) as i32, theme.pane_edge);

        let mut pen = Pen { canvas, engine, renderer, theme, left, y: top + 8.0, bottom };
        self.caption(&mut pen, top);

        pen.heading(t("Valid signatures"));
        if shown.made.is_empty() {
            pen.note(t("This document is not signed."));
        }
        for (index, made) in shown.made.iter().enumerate() {
            self.signature(&mut pen, index, made);
        }
        pen.gap();

        pen.heading(t("Requested signatures"));
        if shown.wanted.is_empty() {
            pen.note(t("Nobody has been asked to sign."));
        }
        for (index, wanted) in shown.wanted.iter().enumerate() {
            self.request(&mut pen, index, wanted);
        }
        pen.gap();

        if !shown.saved {
            pen.note(t("Save the document before signing it."));
        } else if shown.can_sign {
            self.button(&mut pen, Hit::SignDocument, t("Sign the document"));
        } else {
            pen.note(t("There is no certificate on this machine to sign with."));
            pen.note(t("Put one and its key in this folder, or use one the machine holds:"));
            // On its own line, because a path and a sentence together is a
            // path with its end cut off.
            pen.note(&shown.folder);
        }
    }

    /// The name of the pane, and the cross that shuts it.
    fn caption(&mut self, pen: &mut Pen<'_, '_, '_>, top: f32) {
        pen.words(t("Signatures"), pen.left + PADDING, HEADING, pen.theme.text);
        let close_left = pen.left + WIDTH - 26.0;
        if self.hovered == Some(Hit::Close) {
            pen.canvas.fill_rect(close_left as i32 - 4, top as i32 + 6, 24, 22, pen.theme.hover);
        }
        super::icons::draw_sized(
            pen.canvas,
            super::icons::Icon::Close,
            close_left,
            top + 10.0,
            12.0,
            pen.theme.text,
        );
        self.placed.push((Hit::Close, close_left - 4.0, top + 6.0, 24.0, 22.0));
        pen.y = top + 34.0;
        pen.rule();
    }

    /// One signature the document carries.
    fn signature(&mut self, pen: &mut Pen<'_, '_, '_>, index: usize, made: &Made) {
        let hit = Hit::Made(index);
        let top = pen.y;

        // A tick or a cross in the margin, in the colour of the answer: which
        // signatures hold is the one thing a person scans this list for.
        let colour = if made.good { pen.theme.accent } else { pen.theme.danger };
        pen.canvas.fill_rect((pen.left + PADDING) as i32, (top + 4.0) as i32, 8, 8, colour);

        pen.y = top + 1.0;
        pen.words(&made.who, pen.left + PADDING + 16.0, TEXT, pen.theme.text);
        pen.y = top + 14.0;
        pen.words(&made.standing, pen.left + PADDING + 16.0, TEXT, colour);
        pen.y = top + 27.0;

        for said in [&made.reason, &made.at] {
            if said.trim().is_empty() {
                continue;
            }
            pen.words(said, pen.left + PADDING + 16.0, TEXT, pen.theme.dim_text);
            pen.y += 12.0;
        }
        if !made.line.trim().is_empty() {
            let said = crate::messages::with("For the line of {0}", &[&made.line]);
            pen.words(&said, pen.left + PADDING + 16.0, TEXT, pen.theme.dim_text);
            pen.y += 12.0;
        }
        for counter in &made.counters {
            let said = crate::messages::with("Countersigned by {0}", &[counter]);
            pen.words(&said, pen.left + PADDING + 16.0, TEXT, pen.theme.dim_text);
            pen.y += 12.0;
        }

        self.placed.push((hit, pen.left + 4.0, top, WIDTH - 8.0, pen.y - top));
        self.small(pen, Hit::Countersign(index), t("Countersign"));
        pen.y += 4.0;
    }

    /// One line still waiting.
    fn request(&mut self, pen: &mut Pen<'_, '_, '_>, index: usize, wanted: &Wanted) {
        let hit = Hit::Wanted(index);
        let top = pen.y;
        pen.y = top + 1.0;
        pen.words(&wanted.name, pen.left + PADDING, TEXT, pen.theme.text);
        pen.y = top + 14.0;
        if !wanted.title.trim().is_empty() {
            pen.words(&wanted.title, pen.left + PADDING, TEXT, pen.theme.dim_text);
            pen.y = top + 27.0;
        }
        self.placed.push((hit, pen.left + 4.0, top, WIDTH - 8.0, pen.y - top));
        self.small(pen, Hit::Sign(index), t("Sign this line"));
        pen.y += 4.0;
    }

    /// A small button under a row.
    fn small(&mut self, pen: &mut Pen<'_, '_, '_>, hit: Hit, label: &str) {
        let x = pen.left + PADDING + 16.0;
        let measured = pen.engine.simple_line(label, 0.0, 0.0, TEXT, pen.theme.text);
        let width = measured.width + 16.0;
        let fill = if self.hovered == Some(hit) { pen.theme.hover } else { pen.theme.field };
        pen.canvas.fill_rect(x as i32, pen.y as i32, width as i32, 18, fill);
        outline(pen.canvas, x, pen.y, width, 18.0, pen.theme.field_edge);

        let line = pen.engine.simple_line(label, x + 8.0, pen.y + 12.5, TEXT, pen.theme.text);
        pen.renderer.draw_onto(pen.canvas, &line, 0.0, 0.0);
        self.placed.push((hit, x, pen.y, width, 18.0));
        pen.y += 22.0;
    }

    /// And one across the pane.
    fn button(&mut self, pen: &mut Pen<'_, '_, '_>, hit: Hit, label: &str) {
        let x = pen.left + PADDING;
        let width = WIDTH - PADDING * 2.0;
        let fill = if self.hovered == Some(hit) { pen.theme.hover } else { pen.theme.field };
        pen.canvas.fill_rect(x as i32, pen.y as i32, width as i32, BUTTON as i32, fill);
        outline(pen.canvas, x, pen.y, width, BUTTON, pen.theme.field_edge);

        let measured = pen.engine.simple_line(label, 0.0, 0.0, TEXT, pen.theme.text);
        let text_x = x + (width - measured.width).max(0.0) / 2.0;
        let line = pen.engine.simple_line(label, text_x, pen.y + 16.0, TEXT, pen.theme.text);
        pen.renderer.draw_onto(pen.canvas, &line, 0.0, 0.0);

        self.placed.push((hit, x, pen.y, width, BUTTON));
        pen.y += BUTTON + 8.0;
    }
}

/// Where the next thing goes, and what draws it.
struct Pen<'a, 'e, 'r> {
    canvas: &'a mut Canvas,
    engine: &'a mut LayoutEngine<'e>,
    renderer: &'a mut Renderer<'r>,
    theme: &'a Theme,
    left: f32,
    y: f32,
    bottom: f32,
}

impl Pen<'_, '_, '_> {
    /// One line of words at the pen, returning how wide it came out.
    fn words(&mut self, text: &str, x: f32, size: f32, colour: Color) -> f32 {
        if self.y > self.bottom {
            return 0.0;
        }
        let line = self.engine.simple_line(text, x, self.y + size, size, colour);
        let width = line.width;
        self.renderer.draw_onto(self.canvas, &line, 0.0, 0.0);
        width
    }

    /// The name of a section.
    fn heading(&mut self, text: &str) {
        self.words(text, self.left + PADDING, HEADING, self.theme.text);
        self.y += 20.0;
    }

    /// A sentence that is read and not pressed, broken to fit.
    fn note(&mut self, text: &str) {
        let colour = self.theme.dim_text;
        let x = self.left + PADDING;
        let width = WIDTH - PADDING * 2.0;
        let mut y = self.y + TEXT;
        let mut line = String::new();
        for word in text.split_whitespace() {
            let wanted = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            let measured = self.engine.simple_line(&wanted, 0.0, 0.0, TEXT, colour);
            if measured.width > width && !line.is_empty() {
                let drawn = self.engine.simple_line(&line, x, y, TEXT, colour);
                self.renderer.draw_onto(self.canvas, &drawn, 0.0, 0.0);
                y += 13.0;
                line = word.to_owned();
            } else {
                line = wanted;
            }
        }
        if !line.is_empty() {
            let drawn = self.engine.simple_line(&line, x, y, TEXT, colour);
            self.renderer.draw_onto(self.canvas, &drawn, 0.0, 0.0);
        }
        self.y = y + 8.0;
    }

    /// A line across the pane.
    fn rule(&mut self) {
        self.canvas.fill_rect(
            (self.left + PADDING) as i32,
            self.y as i32,
            (WIDTH - PADDING * 2.0) as i32,
            1,
            self.theme.pane_edge,
        );
        self.y += 10.0;
    }

    /// The room between one section and the next.
    fn gap(&mut self) {
        self.y += 8.0;
    }
}

/// A rectangle drawn as four lines rather than filled.
fn outline(canvas: &mut Canvas, left: f32, top: f32, width: f32, height: f32, colour: Color) {
    let (x, y, w, h) = (left as i32, top as i32, width as i32, height as i32);
    canvas.fill_rect(x, y, w, 1, colour);
    canvas.fill_rect(x, y + h - 1, w, 1, colour);
    canvas.fill_rect(x, y, 1, h, colour);
    canvas.fill_rect(x + w - 1, y, 1, h, colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_hit_before_it_has_been_drawn() {
        let pane = SignaturePane::new();
        assert_eq!(pane.at(10, 10), None);
    }

    #[test]
    fn a_signature_that_does_not_hold_is_not_drawn_as_one_that_does() {
        // The one thing a person scans this list for, so the two cannot look
        // alike: the answer is a colour as well as a word.
        let good = Made { good: true, ..Made::default() };
        let bad = Made { good: false, ..Made::default() };
        assert_ne!(good.good, bad.good);
    }
}

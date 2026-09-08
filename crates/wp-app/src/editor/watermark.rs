//! Drawing the watermark, and choosing one.
//!
//! # Why it is drawn here rather than laid out
//!
//! A watermark is not part of the text and takes up none of the page: it sits
//! behind everything, the same on every page, and nothing flows around it. So
//! it is not laid out with the document — it is painted onto each sheet as the
//! sheet is drawn, which is also why changing it never re-flows a line.

use wp_docx::watermark::Watermark;
use wp_raster::{Color, Transform};
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// How much of the page's width the word is made to fill.
///
/// Word sizes a watermark to about this much of the printable width, which is
/// what makes a long word small and a short one large without either running
/// off the paper.
const FILL: f32 = 0.72;

/// The size the text is measured at before being scaled to fit.
///
/// Any size would do; a large one keeps the measurement away from the rounding
/// that hinting does at small sizes.
const MEASURED_AT: f32 = 100.0;

/// The angle a diagonal watermark is turned through, in radians.
///
/// Negative because the canvas counts y downwards, so this is the turn that
/// makes the word rise to the right.
const DIAGONAL: f32 = -core::f32::consts::FRAC_PI_4;

impl Editor {
    /// Paints the watermark onto one sheet of paper.
    pub(super) fn draw_watermark(&mut self, left: f32, top: f32, width: f32, height: f32) {
        let Some(watermark) = self.document.watermark() else { return };
        if watermark.text.trim().is_empty() || width <= 0.0 || height <= 0.0 {
            return;
        }

        let colour = Color::from_hex(&watermark.color).unwrap_or(Color::rgb(0xC0, 0xC0, 0xC0));

        // Measured once at a known size, then scaled so the word fills the
        // page. Measuring is the only way to know how wide a word is in a font
        // that has not been chosen yet.
        let measured = self.engine.simple_line(&watermark.text, 0.0, 0.0, MEASURED_AT, colour);
        if measured.width <= 0.0 {
            return;
        }
        // A turned word needs the room the diagonal gives it, which is longer
        // than the page is wide.
        let across =
            if watermark.diagonal { (width * width + height * height).sqrt() } else { width };
        let size = (MEASURED_AT * across * FILL / measured.width).max(1.0);

        let line = self.engine.simple_line(&watermark.text, 0.0, 0.0, size, colour);
        if line.width <= 0.0 {
            return;
        }

        // Centred on the sheet: the line is drawn from its own origin, so it is
        // moved by half its width and half the height of its letters.
        let centre_x = left + width / 2.0;
        let centre_y = top + height / 2.0;
        let place = Transform::translate(centre_x - line.width / 2.0, centre_y + size * 0.35);
        let turn = if watermark.diagonal {
            place.then(&Transform::rotate_about(DIAGONAL, centre_x, centre_y))
        } else {
            place
        };

        self.renderer.draw_transformed(&mut self.canvas, &line, &turn);
    }

    /// Drops open the ready-made watermarks.
    pub(super) fn open_watermarks(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Watermark) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Watermark) else {
            return Response::Ignored;
        };

        let here = self.document.watermark();
        let presets = Watermark::presets();
        let current = here.as_ref().and_then(|found| {
            presets
                .iter()
                .position(|preset| preset.text == found.text && preset.diagonal == found.diagonal)
                .map(|at| at + 1)
        });

        let mut items = vec!["No Watermark".to_owned()];
        items.extend(presets.iter().map(|preset| preset.text.clone()));
        items.extend(presets.iter().map(|preset| format!("{} — across", preset.text)));
        items.push("Custom Watermark…".to_owned());

        self.popup = Some(Popup::new(Choice::Watermark, items, current, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts on whichever watermark was chosen.
    pub(super) fn choose_watermark(&mut self, index: usize) -> Response {
        self.popup = None;
        let presets = Watermark::presets();

        // The list is: nothing, the diagonal ones, the flat ones, then a line
        // for typing something of your own.
        let wanted = match index.checked_sub(1) {
            None => None,
            Some(at) if at < presets.len() => Some(presets[at].clone()),
            Some(at) if at < presets.len() * 2 => {
                Some(Watermark { diagonal: false, ..presets[at - presets.len()].clone() })
            }
            Some(_) => return self.start_watermark(),
        };

        let note = match &wanted {
            None => "Watermark removed".to_owned(),
            Some(watermark) => format!("Watermark: {}", watermark.text),
        };
        let changed = self.document.set_watermark(wanted.as_ref());
        self.needs_redraw = true;
        self.edited(changed, &note)
    }

    /// Opens the strip that takes a watermark of your own.
    pub(super) fn start_watermark(&mut self) -> Response {
        let mut bar = FindBar::for_purpose(Purpose::Watermark);
        if let Some(here) = self.document.watermark() {
            bar.needle = here.text;
        }
        self.find_bar = Some(bar);
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the word to print behind the page, then press Enter")
    }

    /// Puts on the watermark that was typed.
    pub(super) fn finish_watermark(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let text = bar.needle.trim().to_owned();
        self.find_bar = None;
        self.needs_redraw = true;

        if text.is_empty() {
            let changed = self.document.set_watermark(None);
            return self.edited(changed, "Watermark removed");
        }
        // Whatever else was set — the colour, the angle — is kept, so typing a
        // new word does not quietly undo a colour somebody chose.
        let here = self.document.watermark().unwrap_or_default();
        let wanted = Watermark { text: text.clone(), ..here };
        let changed = self.document.set_watermark(Some(&wanted));
        self.edited(changed, &format!("Watermark: {text}"))
    }
}

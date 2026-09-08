//! Charts, from the Insert tab.
//!
//! Two steps: which kind of chart, then the numbers. Word opens a spreadsheet
//! at this point and asks for them in a grid; there is no spreadsheet here, so
//! they are typed as `name=value` pairs — which is the same information and
//! rather quicker for the five or six numbers most charts have.

use wp_docx::chart::{Chart, Kind};
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// How tall a chart is, as a share of how wide it is.
///
/// Three to two, which is the shape Word gives a new chart and the shape most
/// charts read best at.
const SHAPE: f64 = 2.0 / 3.0;

impl Editor {
    /// Drops open the kinds of chart.
    pub(super) fn open_chart(&mut self) -> Response {
        if self.close_popup_if(Choice::Chart) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Chart) else {
            return Response::Ignored;
        };

        let items = Kind::ALL.iter().map(|kind| kind.label().to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Chart, items, None, left, top, 200.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Remembers the kind and asks for the numbers.
    pub(super) fn choose_chart(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(kind) = Kind::ALL.get(index).copied() else { return Response::Ignored };
        self.chart_kind = kind;

        self.find_bar = Some(FindBar::for_purpose(Purpose::Chart));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the numbers: title; name=value; name=value")
    }

    /// Draws the chart.
    pub(super) fn finish_chart(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.clone();
        self.find_bar = None;
        self.needs_redraw = true;

        // The first piece is the title when it holds no number, which is what
        // "Sales; North=10" means and what "North=10" does not.
        let (title, numbers) = match typed.split_once(';') {
            Some((first, rest)) if !first.contains('=') => (first.trim(), rest),
            _ => ("", typed.as_str()),
        };

        let chart = Chart::parse(self.chart_kind, title, numbers);
        if chart.is_empty() {
            return self.report("No numbers were typed, so no chart was drawn");
        }

        // As wide as the text and two thirds as tall, which is what a chart
        // asked for with no size is given.
        let width = self.text_width_emu().max(wp_docx::EMU_PER_INCH * 2);
        let height = (width as f64 * SHAPE) as i64;

        match self.document.insert_chart(&chart, width, height) {
            Ok(inserted) => {
                self.relayout();
                self.reveal_caret();
                let named = self.chart_kind.label();
                self.edited(inserted, &format!("{named} chart, {} points", chart.values.len()))
            }
            Err(error) => self.report(&format!("The chart could not be drawn: {error}")),
        }
    }
}

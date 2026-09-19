//! Charts, from the Insert tab.
//!
//! Two steps: which kind of chart, then the numbers. Word opens a spreadsheet
//! at this point and asks for them in a grid; there is no spreadsheet here, so
//! they are typed as `name=value` pairs — which is the same information and
//! rather quicker for the five or six numbers most charts have. The
//! spreadsheet is written into the document all the same, so that Word's own
//! Edit Data opens on it — see [`wp_docx::workbook`].

use wp_docx::chart::{Chart, Grouping, Kind};
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{Choice, Command, Popup};
use crate::messages::{t, with};

use super::Editor;

/// How tall a chart is, as a share of how wide it is.
///
/// Three to two, which is the shape Word gives a new chart and the shape most
/// charts read best at.
const SHAPE: f64 = 2.0 / 3.0;

/// The charts the list offers, named as Word names them in its gallery: each
/// kind as it is drawn here, and for the kinds that stack, stacked and as
/// shares. A surface is offered as the contour it is drawn as.
pub const PRESETS: &[(Kind, Grouping, &str)] = &[
    (Kind::Column, Grouping::Clustered, "Clustered Column"),
    (Kind::Column, Grouping::Stacked, "Stacked Column"),
    (Kind::Column, Grouping::PercentStacked, "100% Stacked Column"),
    (Kind::Bar, Grouping::Clustered, "Clustered Bar"),
    (Kind::Bar, Grouping::Stacked, "Stacked Bar"),
    (Kind::Bar, Grouping::PercentStacked, "100% Stacked Bar"),
    (Kind::Line, Grouping::Clustered, "Line with Markers"),
    (Kind::Line, Grouping::Stacked, "Stacked Line"),
    (Kind::Area, Grouping::Clustered, "Area"),
    (Kind::Area, Grouping::Stacked, "Stacked Area"),
    (Kind::Pie, Grouping::Clustered, "Pie"),
    (Kind::Doughnut, Grouping::Clustered, "Doughnut"),
    (Kind::Scatter, Grouping::Clustered, "Scatter"),
    (Kind::Bubble, Grouping::Clustered, "Bubble"),
    (Kind::Radar, Grouping::Clustered, "Radar with Markers"),
    (Kind::Surface, Grouping::Clustered, "Contour"),
];

impl Editor {
    /// Drops open the kinds of chart.
    pub(super) fn open_chart(&mut self) -> Response {
        if self.close_popup_if(Choice::Chart) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Chart) else {
            return Response::Ignored;
        };

        let items = PRESETS.iter().map(|(_, _, label)| t(label).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Chart, items, None, left, top, 200.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Remembers the kind and asks for the numbers.
    pub(super) fn choose_chart(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((kind, grouping, _)) = PRESETS.get(index).copied() else {
            return Response::Ignored;
        };
        self.chart_kind = kind;
        self.chart_grouping = grouping;

        self.find_bar = Some(FindBar::for_purpose(Purpose::Chart));
        self.clamp_scroll();
        self.needs_redraw = true;
        // What a point is typed as depends on what a point is: a scatter's
        // has an x rather than a name, and a bubble's a size as well.
        self.report(match kind {
            Kind::Scatter => t("Type the points: title; x=y; x=y"),
            Kind::Bubble => t("Type the points: title; x=y:size; x=y:size"),
            _ => t("Type the numbers: title; name=value; name=value"),
        })
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

        let mut chart = Chart::parse(self.chart_kind, title, numbers);
        chart.grouping = self.chart_grouping;
        if chart.is_empty() {
            return self.report(t("No numbers were typed, so no chart was drawn"));
        }

        // As wide as the text and two thirds as tall, which is what a chart
        // asked for with no size is given.
        let width = self.text_width_emu().max(wp_docx::EMU_PER_INCH * 2);
        let height = (width as f64 * SHAPE) as i64;

        match self.document.insert_chart(&chart, width, height) {
            Ok(inserted) => {
                self.relayout();
                self.reveal_caret();
                let named = PRESETS
                    .iter()
                    .find(|(kind, grouping, _)| {
                        *kind == self.chart_kind && *grouping == self.chart_grouping
                    })
                    .map_or(self.chart_kind.label(), |(_, _, label)| label);
                let note = with("{0} chart, {1} points", &[t(named), &chart.points().to_string()]);
                self.edited(inserted, &note)
            }
            Err(error) => {
                self.report(&with("The chart could not be drawn: {0}", &[&error.to_string()]))
            }
        }
    }
}

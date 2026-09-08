//! The properties of a table, from the Table Layout tab.

use wp_docx::model::Alignment;
use wp_docx::table_properties::CellAlignment;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// What the list offers, in the order it offers it.
///
/// Flat rather than in tabs: Word's dialog has four of them and a person
/// looking for "make the first row a header" has to guess which. One list of
/// what can be changed is shorter to read than four lists of where things are.
const ITEMS: &[&str] = &[
    "Align the table left",
    "Align the table centre",
    "Align the table right",
    "Make the first row a header",
    "Make the first row ordinary",
    "Align the text top",
    "Align the text middle",
    "Align the text bottom",
    "Set the row height…",
    "Let the row find its own height",
];

impl Editor {
    /// Drops open what can be changed about the table at the caret.
    pub(super) fn open_table_properties(&mut self) -> Response {
        if self.close_popup_if(Choice::TableProperty) {
            return Response::Redraw;
        }
        if self.document.table_here().is_none() {
            return self.report("Put the caret in a table first");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::TableProperties) else {
            return Response::Ignored;
        };

        let items = ITEMS.iter().map(|label| (*label).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::TableProperty, items, None, left, top, 300.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Changes whatever was chosen.
    pub(super) fn choose_table_property(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(label) = ITEMS.get(index).copied() else { return Response::Ignored };

        let changed = match index {
            0 => self.document.set_table_alignment(Alignment::Start),
            1 => self.document.set_table_alignment(Alignment::Center),
            2 => self.document.set_table_alignment(Alignment::End),
            3 => self.document.set_table_header_row(true),
            4 => self.document.set_table_header_row(false),
            5 => self.document.set_cell_alignment(CellAlignment::Top),
            6 => self.document.set_cell_alignment(CellAlignment::Middle),
            7 => self.document.set_cell_alignment(CellAlignment::Bottom),
            8 => return self.start_row_height(),
            _ => self.document.set_table_row_height(None),
        };

        self.relayout();
        self.edited(changed, label)
    }

    /// Opens the strip that takes a row height.
    fn start_row_height(&mut self) -> Response {
        let mut bar = FindBar::for_purpose(Purpose::RowHeight);
        if let Some(twips) = self.document.table_row_height() {
            bar.needle = format!("{:.2}", f64::from(twips) / 1440.0);
        }
        self.find_bar = Some(bar);
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the height in inches, then press Enter")
    }

    /// Sets the row height that was typed.
    pub(super) fn finish_row_height(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.trim().replace(',', ".");
        self.find_bar = None;
        self.needs_redraw = true;

        if typed.is_empty() {
            let changed = self.document.set_table_row_height(None);
            self.relayout();
            return self.edited(changed, "The row finds its own height");
        }
        let Ok(inches) = typed.parse::<f64>() else {
            return self.report(&format!("{typed} is not a measurement"));
        };
        // A row of no height at all is a row nobody can put the caret in.
        let twips = (inches * 1440.0).round().clamp(72.0, 22_000.0) as i32;

        let changed = self.document.set_table_row_height(Some(twips));
        self.relayout();
        self.edited(changed, &format!("Row height {inches}\""))
    }
}

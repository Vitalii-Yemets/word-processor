//! The Page Setup group of the Layout tab: margins, orientation, paper, columns
//! and breaks.
//!
//! # Why these are lists and not switches
//!
//! Every one of them is a button that drops open a list in Word, and what the
//! list shows is what the page the caret is on is set to. Stepping round the
//! choices with each press would be quicker to write and wrong to use: a person
//! looking for A5 should see A5, not press Size four times and watch the page
//! change shape under them.
//!
//! # Which page they change
//!
//! The one the caret is in. A document can be cut into sections, and each of
//! them has its own paper, margins and columns — so turning the page on its
//! side turns the section the caret is in and leaves the rest of the document
//! alone, which is what Word does and what makes a landscape table in the
//! middle of a portrait report possible.

use wp_docx::model::BreakKind;
use wp_docx::page::{MARGIN_PRESETS, PAGE_SIZES};
use wp_docx::sections::Start;
use wp_shell::Response;

use crate::chrome::popup::Row;
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// How many columns the text can be told to run down.
///
/// Word's "Left" and "Right" are two columns of unequal width, which the format
/// writes out column by column and this does not do yet.
const COLUMNS: &[(usize, &str)] = &[(1, "One"), (2, "Two"), (3, "Three")];

/// The rows of the Breaks list, in Word's order and with Word's two headings.
///
/// The headings are here rather than in two lists because it is one menu in
/// Word: everything that cuts the text off and starts it again somewhere else.
const BREAKS: &[(&str, Option<Break>)] = &[
    ("Page Breaks", None),
    ("Page", Some(Break::Page)),
    ("Column", Some(Break::Column)),
    ("Text Wrapping", Some(Break::Line)),
    ("Section Breaks", None),
    ("Next Page", Some(Break::Section(Start::NextPage))),
    ("Continuous", Some(Break::Section(Start::Continuous))),
    ("Even Page", Some(Break::Section(Start::EvenPage))),
    ("Odd Page", Some(Break::Section(Start::OddPage))),
];

/// What one row of the Breaks list puts in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Break {
    Page,
    Column,
    Line,
    Section(Start),
}

impl Editor {
    /// Drops open the margins Word offers by name.
    pub(super) fn open_margins(&mut self) -> Response {
        if self.close_popup_if(Choice::Margin) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Margins) else {
            return Response::Ignored;
        };

        let here = self.document.page_margins();
        let current = MARGIN_PRESETS
            .iter()
            .position(|(_, top, right, bottom, left)| (*top, *right, *bottom, *left) == here);
        let items = MARGIN_PRESETS.iter().map(|(name, ..)| (*name).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Margin, items, current, left, top, 200.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Sets the margins of the caret's section.
    pub(super) fn choose_margins(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((name, top, right, bottom, left)) = MARGIN_PRESETS.get(index).copied() else {
            return Response::Ignored;
        };
        let changed = self.document.set_page_margins(top, right, bottom, left);
        self.relayout();
        self.edited(changed, &format!("{name} margins"))
    }

    /// Drops open the two ways round a page can go.
    pub(super) fn open_orientation(&mut self) -> Response {
        if self.close_popup_if(Choice::Orientation) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Orientation) else {
            return Response::Ignored;
        };

        let current = usize::from(self.document.is_landscape());
        let items = vec!["Portrait".to_owned(), "Landscape".to_owned()];
        self.popup = Some(Popup::new(Choice::Orientation, items, Some(current), left, top, 180.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Turns the caret's section, or leaves it as it is.
    pub(super) fn choose_orientation(&mut self, index: usize) -> Response {
        self.popup = None;
        let landscape = index == 1;
        let changed = self.document.set_landscape(landscape);
        self.relayout();
        self.edited(changed, if landscape { "Landscape" } else { "Portrait" })
    }

    /// Drops open the paper sizes.
    pub(super) fn open_page_size(&mut self) -> Response {
        if self.close_popup_if(Choice::Paper) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::PageSize) else {
            return Response::Ignored;
        };

        // A size is quoted portrait-way-round however the page is turned, so
        // the two measurements are put back that way before they are looked up.
        let (width, height) = self.document.page_size();
        let (short, long) = if width > height { (height, width) } else { (width, height) };
        let current = PAGE_SIZES.iter().position(|(_, w, h)| (*w, *h) == (short, long));

        let items = PAGE_SIZES
            .iter()
            .map(|(name, width, height)| {
                format!("{name}  {} × {} mm", millimetres(*width), millimetres(*height))
            })
            .collect();
        self.popup = Some(Popup::new(Choice::Paper, items, current, left, top, 220.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Prints the caret's section on the paper chosen.
    pub(super) fn choose_page_size(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((name, width, height)) = PAGE_SIZES.get(index).copied() else {
            return Response::Ignored;
        };
        let changed = self.document.set_page_size(width, height);
        self.relayout();
        self.edited(changed, name)
    }

    /// Drops open how many columns the text can run down.
    pub(super) fn open_columns(&mut self) -> Response {
        if self.close_popup_if(Choice::Column) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Columns) else {
            return Response::Ignored;
        };

        let (here, _) = self.document.columns();
        let current = COLUMNS.iter().position(|(count, _)| *count == here);
        let items = COLUMNS.iter().map(|(_, name)| (*name).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Column, items, current, left, top, 180.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Runs the caret's section down the number of columns chosen.
    pub(super) fn choose_columns(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((count, name)) = COLUMNS.get(index).copied() else {
            return Response::Ignored;
        };
        // The gap is left as it was: somebody who has set one means it.
        let (_, gap) = self.document.columns();
        let changed = self.document.set_columns(count, gap);
        self.relayout();
        self.edited(changed, &format!("{name} column"))
    }

    /// Drops open the breaks.
    pub(super) fn open_breaks(&mut self) -> Response {
        if self.close_popup_if(Choice::Break) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Breaks) else {
            return Response::Ignored;
        };

        let items = BREAKS.iter().map(|(label, _)| (*label).to_owned()).collect();
        let rows = BREAKS
            .iter()
            .map(|(_, what)| if what.is_none() { Row::heading() } else { Row::default() })
            .collect();
        self.popup = Some(Popup::new(Choice::Break, items, None, left, top, 220.0).with_rows(rows));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts in whichever break was chosen.
    pub(super) fn choose_break(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((label, Some(what))) = BREAKS.get(index).copied() else {
            return Response::Ignored;
        };
        self.put_in_break(what, label)
    }

    /// Puts one break in and says what it was.
    fn put_in_break(&mut self, what: Break, label: &str) -> Response {
        let changed = match what {
            Break::Page => self.document.insert_break(BreakKind::Page),
            Break::Column => self.document.insert_break(BreakKind::Column),
            Break::Line => self.document.insert_break(BreakKind::Line),
            Break::Section(start) => self.document.insert_section_break(start),
        };
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("{label} break"))
    }

    /// A line break, which is what Shift+Enter puts in.
    pub(super) fn press_line_break(&mut self) -> Response {
        self.put_in_break(Break::Line, "Text wrapping")
    }

    /// A column break, which is what Ctrl+Shift+Enter puts in.
    pub(super) fn press_column_break(&mut self) -> Response {
        self.put_in_break(Break::Column, "Column")
    }
}

/// A measurement in twentieths of a point, rounded to whole millimetres.
///
/// What the list shows beside each name, because a paper size is quoted in
/// millimetres everywhere outside the United States and the names alone are no
/// help to somebody who has not met them.
fn millimetres(twips: i32) -> i32 {
    ((twips as f32) * 25.4 / 1440.0).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_four_is_the_size_it_is_printed_as() {
        assert_eq!(millimetres(11_906), 210);
        assert_eq!(millimetres(16_838), 297);
    }

    #[test]
    fn the_headings_are_the_rows_that_offer_nothing() {
        let headings: Vec<usize> = BREAKS
            .iter()
            .enumerate()
            .filter_map(|(index, (_, what))| what.is_none().then_some(index))
            .collect();
        assert_eq!(headings, vec![0, 4]);
    }

    #[test]
    fn every_kind_of_section_break_is_offered() {
        for start in [Start::NextPage, Start::Continuous, Start::EvenPage, Start::OddPage] {
            assert!(
                BREAKS.iter().any(|(_, what)| *what == Some(Break::Section(start))),
                "{start:?} is not in the list"
            );
        }
    }
}

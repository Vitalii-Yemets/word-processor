//! How the pages of the caret's section are numbered.
//!
//! # What Word offers here
//!
//! A dialog: the figures to use, whether to include the chapter number, and
//! whether the section carries on from the one before it or starts again at a
//! number of its own. This is the same choices as a menu, less the chapter
//! numbering, which needs a numbered heading style to count from.
//!
//! It is worth having for one reason: a book's front matter is numbered i, ii,
//! iii and its body starts again at 1, and without this there is no way to say
//! so.

use wp_docx::sections::NumberFormat;
use wp_shell::Response;

use crate::chrome::icons::Icon;
use crate::chrome::popup::{Kind, Row};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// What one line of the menu does to the section's numbering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Figures(NumberFormat),
    /// Carry on from the section before this one.
    Continue,
    /// Start again at one, which is what a body after front matter does.
    Restart,
}

/// The menu, in the order Word's dialog lists things.
const MENU: &[(&str, Option<Change>)] = &[
    ("Number format", None),
    ("1, 2, 3", Some(Change::Figures(NumberFormat::Decimal))),
    ("i, ii, iii", Some(Change::Figures(NumberFormat::LowerRoman))),
    ("I, II, III", Some(Change::Figures(NumberFormat::UpperRoman))),
    ("a, b, c", Some(Change::Figures(NumberFormat::LowerLetter))),
    ("A, B, C", Some(Change::Figures(NumberFormat::UpperLetter))),
    ("", None),
    ("Continue from previous section", Some(Change::Continue)),
    ("Start at 1", Some(Change::Restart)),
];

/// How wide it is drawn.
const WIDTH: f32 = 230.0;

impl Editor {
    /// Drops open how the caret's section numbers its pages.
    pub(super) fn open_page_numbering(&mut self) -> Response {
        if self.close_popup_if(Choice::PageNumbering) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::FormatPageNumbers) else {
            return Response::Ignored;
        };

        let numbering = self.document.page_numbering(self.document.section_here());
        let items = MENU.iter().map(|(label, _)| (*label).to_owned()).collect();
        let rows = MENU
            .iter()
            .map(|(label, change)| match change {
                // A tick against what the section already says.
                Some(Change::Figures(format)) if *format == numbering.format => {
                    Row::new(Kind::Choice, Icon::Accept)
                }
                Some(Change::Continue) if numbering.start.is_none() => {
                    Row::new(Kind::Choice, Icon::Accept)
                }
                Some(Change::Restart) if numbering.start == Some(1) => {
                    Row::new(Kind::Choice, Icon::Accept)
                }
                Some(_) => Row::default(),
                None if label.is_empty() => Row::separator(),
                None => Row::heading(),
            })
            .collect();

        self.popup =
            Some(Popup::new(Choice::PageNumbering, items, None, left, top, WIDTH).with_rows(rows));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Applies whichever line was chosen.
    pub(super) fn choose_page_numbering(&mut self, row: usize) -> Response {
        self.popup = None;
        let Some((label, Some(change))) = MENU.get(row).copied() else {
            return Response::Ignored;
        };

        let mut numbering = self.document.page_numbering(self.document.section_here());
        match change {
            Change::Figures(format) => numbering.format = format,
            Change::Continue => numbering.start = None,
            Change::Restart => numbering.start = Some(1),
        }

        let changed = self.document.set_page_numbering(numbering);
        self.relayout();
        self.edited(changed, label)
    }
}

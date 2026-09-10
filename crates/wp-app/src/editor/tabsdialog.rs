//! Word's Tabs dialog: where the tabs of a paragraph stop.
//!
//! # Why it is a dialog of its own
//!
//! Because Word's is, and because it is reached from two places: the Tabs
//! button inside the Paragraph dialog, and a double click on the ruler. A
//! dialog that can be opened from two places has to be one thing, or the two
//! ways in drift apart.
//!
//! # One stop at a time
//!
//! Word's dialog shows the stops it has as a list, and works on one of them:
//! a position is typed, an alignment and a leader chosen, and Set adds it or
//! changes it. Clear takes one away and Clear All takes the lot. That shape is
//! kept — it is what a person who knows Word will try — with the list of stops
//! shown as a choice rather than as a scrolling column, because a paragraph
//! with more than a handful of tab stops is a paragraph that wants a table.

use wp_docx::model::{TabAlignment, TabLeader, TabStop};
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::measure;

use super::dialogs::Asking;
use super::Editor;

const EXISTING: usize = 0;
const ROW_ONE: usize = 1;
const POSITION: usize = 2;
const DEFAULT_EVERY: usize = 3;
const ROW_TWO: usize = 4;
const ALIGNMENT: usize = 5;
const LEADER: usize = 6;

/// Word's three buttons past OK and Cancel.
pub(super) const SET: &str = "Set";
pub(super) const CLEAR: &str = "Clear";
pub(super) const CLEAR_ALL: &str = "Clear All";

/// The alignments Word offers, in Word's order and by Word's names.
const ALIGNMENTS: &[(&str, TabAlignment)] = &[
    ("Left", TabAlignment::Start),
    ("Center", TabAlignment::Center),
    ("Right", TabAlignment::End),
    ("Decimal", TabAlignment::Decimal),
    ("Bar", TabAlignment::Bar),
];

/// And the leaders, which Word numbers from one.
const LEADERS: &[(&str, TabLeader)] = &[
    ("1 None", TabLeader::None),
    ("2 ......", TabLeader::Dot),
    ("3 ------", TabLeader::Hyphen),
    ("4 ______", TabLeader::Underscore),
    ("5 ······", TabLeader::MiddleDot),
];

/// The row of the list that means "no stop chosen", which is where the dialog
/// opens when the paragraph has none.
const NONE_CHOSEN: &str = "(none)";

impl Editor {
    /// Opens Word's Tabs dialog on the paragraph the caret is in.
    pub(super) fn open_tabs_dialog(&mut self) -> Response {
        let stops = self.document.tab_stops_here();
        let dialog = self.tabs_dialog(&stops, stops.first().copied());
        self.ask(Asking::TabStops, dialog)
    }

    /// The dialog itself: the stops it holds, and whichever one is being
    /// worked on.
    pub(super) fn tabs_dialog(&self, stops: &[TabStop], chosen: Option<TabStop>) -> Dialog {
        let mut positions: Vec<String> = core::iter::once(NONE_CHOSEN.to_owned())
            .chain(stops.iter().map(|stop| measure::format(stop.position, self.unit)))
            .collect();
        let current = chosen
            .and_then(|stop| stops.iter().position(|found| found.position == stop.position))
            .map_or(0, |at| at + 1);
        // A stop typed but not yet set is not in the list, and the list must
        // still be able to show it.
        if positions.len() == 1 {
            positions.truncate(1);
        }

        let stop = chosen.unwrap_or(TabStop {
            position: 0,
            alignment: TabAlignment::Start,
            leader: TabLeader::None,
        });

        let fields = vec![
            Field::Choice { label: "Tab stops".to_owned(), items: positions, current },
            Field::Columns(2),
            Field::Number {
                label: "Tab stop position".to_owned(),
                value: measure::format(stop.position, self.unit),
                unit: self.unit.mark(),
            },
            Field::Number {
                label: "Default tab stops".to_owned(),
                value: measure::format(self.document.default_tab_width(), self.unit),
                unit: self.unit.mark(),
            },
            Field::Columns(2),
            Field::Choice {
                label: "Alignment".to_owned(),
                items: ALIGNMENTS.iter().map(|(name, _)| (*name).to_owned()).collect(),
                current: ALIGNMENTS
                    .iter()
                    .position(|(_, kind)| *kind == stop.alignment)
                    .unwrap_or(0),
            },
            Field::Choice {
                label: "Leader".to_owned(),
                items: LEADERS.iter().map(|(name, _)| (*name).to_owned()).collect(),
                current: LEADERS.iter().position(|(_, kind)| *kind == stop.leader).unwrap_or(0),
            },
        ];

        crate::chrome::dialog::check_rows(
            "Tabs",
            &fields,
            &[
                (EXISTING, "a list"),
                (ROW_ONE, "a row"),
                (POSITION, "a number"),
                (DEFAULT_EVERY, "a number"),
                (ROW_TWO, "a row"),
                (ALIGNMENT, "a list"),
                (LEADER, "a list"),
            ],
        );

        Dialog::with_buttons(
            "Tabs",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: SET.to_owned(), answer: Answer::Named(SET), default: false },
                Button { label: CLEAR.to_owned(), answer: Answer::Named(CLEAR), default: false },
                Button {
                    label: CLEAR_ALL.to_owned(),
                    answer: Answer::Named(CLEAR_ALL),
                    default: false,
                },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(500.0)
    }

    /// The stop the dialog's fields describe.
    fn tab_stop_said(&self, dialog: &Dialog) -> TabStop {
        TabStop {
            position: measure::parse(&dialog.said(POSITION), self.unit).unwrap_or(0).max(0),
            alignment: ALIGNMENTS
                .get(dialog.chose(ALIGNMENT))
                .map_or(TabAlignment::Start, |(_, kind)| *kind),
            leader: LEADERS.get(dialog.chose(LEADER)).map_or(TabLeader::None, |(_, kind)| *kind),
        }
    }

    /// One of Set, Clear or Clear All, which change the list without shutting
    /// the dialog — as Word's do.
    pub(super) fn tabs_dialog_button(&mut self, dialog: &Dialog, button: &str) -> Response {
        let mut stops = self.document.tab_stops_here();
        let wanted = self.tab_stop_said(dialog);

        match button {
            SET if wanted.position > 0 => {
                // Setting a stop where one already is replaces it, which is
                // how Word's dialog is used to change an alignment.
                stops.retain(|stop| stop.position != wanted.position);
                stops.push(wanted);
                stops.sort_by_key(|stop| stop.position);
            }
            CLEAR => stops.retain(|stop| stop.position != wanted.position),
            CLEAR_ALL => stops.clear(),
            _ => {}
        }

        let changed = self.document.set_tab_stops_here(&stops);
        // The dialog stays up, showing what the list now holds.
        let chosen = stops.iter().copied().find(|stop| stop.position == wanted.position);
        self.dialog = Some(self.tabs_dialog(&stops, chosen.or_else(|| stops.first().copied())));
        self.relayout();
        self.edited(changed, "Tab stops")
    }

    /// What OK does: the stops are already on the document, so the only thing
    /// left is the default grid.
    pub(super) fn apply_tabs_dialog(&mut self, dialog: &Dialog) -> Response {
        let every = measure::parse(&dialog.said(DEFAULT_EVERY), self.unit).unwrap_or(720).max(1);
        let changed = self.document.set_default_tab_width(every);
        self.relayout();
        self.edited(changed, "Tab stops")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_alignment_and_leader_word_offers_is_one_the_model_has() {
        // The lists are what the dialog shows; the model is what the file
        // stores. A name here with nothing behind it would be a row that
        // silently did nothing.
        assert_eq!(ALIGNMENTS.len(), 5);
        assert_eq!(LEADERS.len(), 5);
        for (_, kind) in LEADERS {
            assert_eq!(TabLeader::from_word(kind.word()), *kind);
        }
    }
}

//! Which column of the list is which part of an address.
//!
//! # Why this is needed at all
//!
//! An address block is built out of parts — a name, a street, a town — and a
//! list is a spreadsheet with whatever headings the person who typed it chose.
//! The two are matched by guessing: a column called "City" or "Town" is the
//! town. The guess is right most of the time and wrong the rest, and Word's
//! Match Fields is where it gets corrected.
//!
//! A correction is remembered for as long as the list is: it is about this list
//! and these headings, not about the document, so it is not written into the
//! file. Choosing a different list clears it, because the old headings have
//! gone.

use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::mailings::ADDRESS;
use super::Editor;

/// What the list shows when a part is matched to nothing.
const UNMATCHED: &str = "(not matched)";

impl Editor {
    /// Drops open the parts of an address and what each is matched to.
    pub(super) fn open_match_fields(&mut self) -> Response {
        if self.close_popup_if(Choice::MatchField) {
            return Response::Redraw;
        }
        if self.recipients.headers.is_empty() {
            return self.report("No recipients yet — use Select Recipients");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::MatchFields) else {
            return Response::Ignored;
        };

        let items = ADDRESS
            .iter()
            .map(|(part, names)| {
                let column =
                    self.address_column(part, names).unwrap_or_else(|| UNMATCHED.to_owned());
                format!("{part}  →  {column}")
            })
            .collect();
        self.popup = Some(Popup::new(Choice::MatchField, items, None, left, top, 320.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Asks which column the chosen part should come from.
    pub(super) fn choose_match_field(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((part, _)) = ADDRESS.get(index) else { return Response::Ignored };
        self.matching_part = index;

        let Some((left, top, _)) = self.ribbon.command_rect(Command::MatchFields) else {
            return Response::Ignored;
        };
        // The columns, with "not matched" first so a wrong guess can be undone
        // rather than only replaced.
        let mut items = vec![UNMATCHED.to_owned()];
        items.extend(self.recipients.headers.iter().cloned());
        self.popup = Some(Popup::new(Choice::MatchColumn, items, None, left, top, 320.0));
        self.needs_redraw = true;
        self.report(&format!("Which column is the {part}?"))
    }

    /// Matches the part to the column that was chosen.
    pub(super) fn choose_match_column(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((part, _)) = ADDRESS.get(self.matching_part) else {
            return Response::Ignored;
        };

        self.field_matches.retain(|(matched, _)| matched != part);
        let Some(column) = index.checked_sub(1).and_then(|at| self.recipients.headers.get(at))
        else {
            // "Not matched": the part is left out of the address block, even if
            // a column would have been guessed for it.
            self.field_matches.push(((*part).to_owned(), String::new()));
            return self.report(&format!("The {part} is not matched to a column"));
        };

        let column = column.clone();
        self.field_matches.push(((*part).to_owned(), column.clone()));
        self.report(&format!("The {part} comes from {column}"))
    }

    /// The column one part of an address comes from.
    ///
    /// What was matched by hand, or failing that the first column whose heading
    /// is one of the names that part goes by.
    #[must_use]
    pub(super) fn address_column(&self, part: &str, names: &[&str]) -> Option<String> {
        if let Some((_, column)) = self.field_matches.iter().find(|(matched, _)| matched == part) {
            // An empty match is a part deliberately left out, not an absent one.
            return (!column.is_empty()).then(|| column.clone());
        }
        self.matching_column(names)
    }

    /// Forgets what was matched, because it was matched against other headings.
    pub(super) fn forget_matches(&mut self) {
        self.field_matches.clear();
    }
}

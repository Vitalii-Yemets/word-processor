//! Mail merge, from the Mailings tab.
//!
//! # What is kept where
//!
//! The letter is the document, and it holds only the names of the columns it
//! wants. The list of people is a file beside it, read afresh each time — so a
//! list that somebody has added a name to is up to date without the letter
//! being touched. Where that file is, is written into the document's settings,
//! which is where Word writes it too.

use std::path::PathBuf;

use wp_docx::merge::{merge_instruction, Kind, Recipients};
use wp_shell::dialog::FileFilter;
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// The parts of an address block, in the order an envelope is written.
///
/// Word asks which of the list's columns each one is; this looks for the
/// obvious names, because a list written by a person calls the town "Town" or
/// "City" and very little else.
pub(super) const ADDRESS: &[(&str, &[&str])] = &[
    ("name", &["Name", "FullName", "Full Name", "Имя"]),
    ("company", &["Company", "Organisation", "Organization"]),
    ("street", &["Address", "Address1", "Street", "Адрес"]),
    ("town", &["Town", "City", "Город"]),
    ("county", &["County", "State", "Region", "Область"]),
    ("postcode", &["Postcode", "Postal Code", "ZIP", "Индекс"]),
    ("country", &["Country", "Страна"]),
];

impl Editor {
    /// Drops open the kinds of thing a merge can produce.
    pub(super) fn open_merge_kind(&mut self) -> Response {
        if self.close_popup_if(Choice::MergeKind) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::StartMailMerge) else {
            return Response::Ignored;
        };
        let mut items = vec!["Not a merge document".to_owned()];
        items.extend(Kind::ALL.iter().map(|kind| kind.label().to_owned()));

        let current = match self.document.merge_source() {
            None => Some(0),
            Some((kind, _)) => Kind::ALL.iter().position(|entry| *entry == kind).map(|at| at + 1),
        };
        self.popup = Some(Popup::new(Choice::MergeKind, items, current, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Makes the document a merge letter of the kind chosen, or stops it being
    /// one at all.
    pub(super) fn choose_merge_kind(&mut self, index: usize) -> Response {
        self.popup = None;
        let path = self
            .recipient_file
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();

        match index.checked_sub(1) {
            None => {
                let changed = self.document.clear_merge_source();
                self.recipients = Recipients::default();
                self.recipient_file = None;
                self.preview_record = None;
                self.relayout();
                self.edited(changed, "No longer a merge document")
            }
            Some(at) => {
                let Some(kind) = Kind::ALL.get(at).copied() else { return Response::Ignored };
                let changed = self.document.set_merge_source(kind, &path);
                self.edited(changed, &format!("Mail merge: {}", kind.label()))
            }
        }
    }

    /// Asks for the file the recipients are in and reads it.
    pub(super) fn select_recipients(&mut self) -> Response {
        let filters = [
            FileFilter { label: "Comma-separated values", pattern: "*.csv" },
            FileFilter { label: "Text files", pattern: "*.txt" },
            FileFilter { label: "All files", pattern: "*.*" },
        ];
        let Some(path) = wp_shell::dialog::open_file("Select Recipients", &filters) else {
            return Response::Ignored;
        };

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => return self.report(&format!("The list could not be read: {error}")),
        };
        let recipients = Recipients::parse(&bytes);
        if recipients.headers.is_empty() {
            return self.report("That file has no columns in its first row");
        }

        let shown = path.to_string_lossy().into_owned();
        let kind = self.document.merge_source().map_or(Kind::Letters, |(kind, _)| kind);
        self.document.set_merge_source(kind, &shown);
        self.recipients = recipients;
        self.recipient_file = Some(PathBuf::from(&path));
        self.preview_record = None;
        // Matched against the old headings, so they mean nothing now.
        self.forget_matches();
        self.relayout();

        let count = self.recipients.len();
        self.report(&format!("{count} recipients, {} columns", self.recipients.headers.len()))
    }

    /// Shows who is on the list.
    pub(super) fn open_recipient_list(&mut self) -> Response {
        if self.close_popup_if(Choice::Recipient) {
            return Response::Redraw;
        }
        if self.recipients.is_empty() {
            return self.report("No recipients yet — use Select Recipients");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::EditRecipientList) else {
            return Response::Ignored;
        };

        let items = (0..self.recipients.len()).map(|at| self.describe_recipient(at)).collect();
        self.popup =
            Some(Popup::new(Choice::Recipient, items, self.preview_record, left, top, 320.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Shows the letter as it will go to whoever was chosen.
    pub(super) fn choose_recipient(&mut self, index: usize) -> Response {
        self.popup = None;
        if index >= self.recipients.len() {
            return Response::Ignored;
        }
        self.preview_record = Some(index);
        self.relayout();
        self.needs_redraw = true;
        self.report(&format!("Showing {}", self.describe_recipient(index)))
    }

    /// Drops open the columns the list has.
    pub(super) fn open_merge_fields(&mut self) -> Response {
        if self.close_popup_if(Choice::MergeField) {
            return Response::Redraw;
        }
        if self.recipients.headers.is_empty() {
            return self.report("No recipients yet — use Select Recipients");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::InsertMergeField) else {
            return Response::Ignored;
        };
        let items = self.recipients.headers.clone();
        self.popup = Some(Popup::new(Choice::MergeField, items, None, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts a merge field at the caret.
    pub(super) fn choose_merge_field(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(column) = self.recipients.headers.get(index).cloned() else {
            return Response::Ignored;
        };
        let changed = self.insert_merge_field(&column);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("«{column}»"))
    }

    /// Puts a whole address at the caret, one field per line.
    pub(super) fn insert_address_block(&mut self) -> Response {
        if self.recipients.headers.is_empty() {
            return self.report("No recipients yet — use Select Recipients");
        }

        // One gesture: an address is several fields and several line breaks, and
        // one undo should take the whole of it back.
        self.document.begin_gesture();
        let mut written = 0usize;
        for (part, names) in ADDRESS {
            let Some(column) = self.address_column(part, names) else { continue };
            if written > 0 {
                self.document.press_enter();
            }
            self.insert_merge_field(&column);
            written += 1;
        }
        self.document.end_gesture();

        if written == 0 {
            return self.report("The list has no columns an address is made of");
        }
        self.relayout();
        self.reveal_caret();
        self.edited(true, &format!("Address block: {written} lines"))
    }

    /// Puts a greeting at the caret.
    pub(super) fn insert_greeting_line(&mut self) -> Response {
        if self.recipients.headers.is_empty() {
            return self.report("No recipients yet — use Select Recipients");
        }
        let Some(column) = self.address_column(ADDRESS[0].0, ADDRESS[0].1) else {
            return self.report("The list has no column of names");
        };

        self.document.begin_gesture();
        self.document.type_text("Dear ");
        self.insert_merge_field(&column);
        self.document.type_text(",");
        self.document.end_gesture();

        self.relayout();
        self.reveal_caret();
        self.edited(true, "Greeting line")
    }

    /// Shows the letter as it will go out, or back as it was written.
    pub(super) fn toggle_preview(&mut self) -> Response {
        if self.recipients.is_empty() {
            return self.report("No recipients yet — use Select Recipients");
        }
        self.preview_record = match self.preview_record {
            Some(_) => None,
            None => Some(0),
        };
        self.relayout();
        self.needs_redraw = true;

        match self.preview_record {
            Some(at) => self.report(&format!("Showing {}", self.describe_recipient(at))),
            None => self.report("Showing the fields"),
        }
    }

    /// Moves on to the next recipient, or back to the one before.
    pub(super) fn step_recipient(&mut self, forwards: bool) -> Response {
        if self.recipients.is_empty() {
            return self.report("No recipients yet — use Select Recipients");
        }
        let last = self.recipients.len() - 1;
        let at = self.preview_record.unwrap_or(0);
        let next = if forwards { at.saturating_add(1).min(last) } else { at.saturating_sub(1) };

        self.preview_record = Some(next);
        self.relayout();
        self.needs_redraw = true;
        self.report(&format!("{} of {} — {}", next + 1, last + 1, self.describe_recipient(next)))
    }

    /// Says whether the letter asks for anything the list has not got.
    pub(super) fn check_merge(&mut self) -> Response {
        let wanted = self.document.merge_fields();
        if wanted.is_empty() {
            return self.report("This letter has no merge fields in it");
        }
        if self.recipients.headers.is_empty() {
            return self.report("No recipients yet — use Select Recipients");
        }

        let missing = self.document.missing_merge_fields(&self.recipients);
        if missing.is_empty() {
            return self.report(&format!(
                "All {} fields are in the list, {} recipients",
                wanted.len(),
                self.recipients.len()
            ));
        }
        self.report(&format!("The list has no column called {}", missing.join(", ")))
    }

    /// Writes one letter per recipient into a document of its own.
    pub(super) fn finish_merge(&mut self) -> Response {
        if self.recipients.is_empty() {
            return self.report("No recipients yet — use Select Recipients");
        }
        let Some(folder) = wp_shell::dialog::save_file(
            "Finish & Merge",
            &[FileFilter { label: "Word documents", pattern: "*.docx" }],
            self.file.as_deref(),
        ) else {
            return Response::Ignored;
        };

        // One file per person, named after the file that was asked for with the
        // number of the letter on the end. A single document holding every
        // letter is Word's other answer; this is the one that can be printed a
        // letter at a time.
        let stem = folder.with_extension("");
        let mut written = 0usize;
        let mut skipped = 0usize;
        for index in 0..self.recipients.len() {
            let record = self.recipients.record(index);
            // A rule in the letter can say to leave somebody out.
            if self.document.record_is_skipped(&record) {
                skipped += 1;
                continue;
            }

            let mut copy = self.document.clone();
            // The rules first: what a rule says can itself hold merge fields.
            copy.apply_merge_rules(&record, written + 1);
            copy.apply_merge_record(&record);
            let bytes = match copy.save() {
                Ok(bytes) => bytes,
                Err(error) => return self.report(&format!("Letter {index} failed: {error}")),
            };
            written += 1;
            let path = PathBuf::from(format!("{}-{}.docx", stem.to_string_lossy(), written));
            if let Err(error) = std::fs::write(&path, bytes) {
                return self.report(&format!("{} could not be written: {error}", path.display()));
            }
        }

        if skipped > 0 {
            return self.report(&format!("{written} letters written, {skipped} skipped by a rule"));
        }
        self.report(&format!("{written} letters written"))
    }

    /// Puts one merge field in, without laying the document out again.
    fn insert_merge_field(&mut self, column: &str) -> bool {
        // What the field shows before anything works it out: the column's name
        // in the guillemets Word uses, so a letter reads as a letter.
        let shown = format!("«{column}»");
        self.document.insert_field(&merge_instruction(column), &shown)
    }

    /// The list's column whose name is one of those given.
    pub(super) fn matching_column(&self, names: &[&str]) -> Option<String> {
        self.recipients
            .headers
            .iter()
            .find(|header| names.iter().any(|name| header.eq_ignore_ascii_case(name)))
            .cloned()
    }

    /// What to call one recipient in a list.
    fn describe_recipient(&self, index: usize) -> String {
        let record = self.recipients.record(index);
        let shown: Vec<String> = record
            .iter()
            .filter(|(_, value)| !value.trim().is_empty())
            .take(3)
            .map(|(_, value)| value.clone())
            .collect();
        if shown.is_empty() {
            return format!("Recipient {}", index + 1);
        }
        shown.join(", ")
    }
}

impl Editor {
    /// Shades the merge fields, or stops shading them.
    ///
    /// A letter written for everybody looks like a letter written for nobody:
    /// «Name» reads as text until it is shaded, and then it plainly is not.
    pub(super) fn toggle_field_highlight(&mut self) -> Response {
        self.highlight_fields = !self.highlight_fields;
        self.needs_redraw = true;
        self.report(if self.highlight_fields {
            "Merge fields shaded"
        } else {
            "Merge fields no longer shaded"
        })
    }

    /// Draws a band behind every merge field.
    pub(super) fn draw_field_highlight(&mut self) {
        if !self.highlight_fields {
            return;
        }
        let columns = self.document.merge_fields();
        if columns.is_empty() {
            return;
        }

        // Where every merge field sits: the stretches of text a field covers,
        // found by walking the paragraphs the fields are in.
        let mut bands: Vec<(f32, f32, f32, f32)> = Vec::new();
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            for (start, end) in self.document.merge_field_ranges() {
                for (x, y, width, height) in self.pages[index].selection_rects(start, end) {
                    bands.push((origin_x + x, top + y, width, height));
                }
            }
        }

        let colour = self.theme.selection;
        for (x, y, width, height) in bands {
            self.canvas.fill_rect(
                x as i32,
                y as i32,
                width.ceil() as i32,
                height.ceil() as i32,
                colour,
            );
        }
    }
}

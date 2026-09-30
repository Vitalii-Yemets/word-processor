//! Mail merge, from the Mailings tab.
//!
//! # What is kept where
//!
//! The letter is the document, and it holds only the names of the columns it
//! wants. The list of people is a file beside it, read afresh each time — so a
//! list that somebody has added a name to is up to date without the letter
//! being touched. Where that file is, is written into the document's settings,
//! which is where Word writes it too.

use crate::messages::t;
use std::path::PathBuf;

use wp_docx::merge::{merge_instruction, Kind, Recipients};
use wp_shell::dialog::FileFilter;
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field, TreeRow};
use crate::chrome::{Choice, Command, Popup};

use super::dialogs::Asking;

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

    /// Word's Select Recipients: where the people come from.
    pub(super) fn open_recipient_source(&mut self) -> Response {
        self.open_ribbon_menu(Choice::RecipientSource)
    }

    /// Does whichever was chosen.
    pub(super) fn choose_recipient_source(&mut self, index: usize) -> Response {
        self.popup = None;
        match index {
            0 => self.type_a_new_list(),
            1 => self.select_recipients(),
            _ => Response::Ignored,
        }
    }

    /// Asks for the file the recipients are in and reads it.
    pub(super) fn select_recipients(&mut self) -> Response {
        let filters = [
            FileFilter { label: "Comma-separated values", pattern: "*.csv" },
            FileFilter { label: "Text files", pattern: "*.txt" },
            FileFilter { label: "All files", pattern: "*.*" },
        ];
        let Some(path) = wp_shell::dialog::open_file(t("Select Recipients"), &filters) else {
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

/// The columns a new list starts with, which are Word's own.
///
/// A list typed from nothing has to start somewhere, and somewhere is the
/// shape of an address: who, where, and how else to reach them. A column
/// nobody fills in costs nothing — the file simply has an empty one — and a
/// column that is not here can be added by typing its name into the last box.
pub(crate) const NEW_LIST_COLUMNS: &[&str] = &[
    "Title",
    "First Name",
    "Last Name",
    "Company",
    "Address Line 1",
    "City",
    "Postcode",
    "Country",
    "Email",
];

/// Where the list of what has been typed sits, and where the boxes begin.
const FIRST_BOX: usize = 3;

/// The button that puts the person in the boxes on to the list.
pub(super) const ADD_RECIPIENT: &str = "Add";

impl Editor {
    /// Word's Type a New List: a list of people made without a file to start
    /// from.
    ///
    /// What it writes at the end is the delimited file this program already
    /// reads — Word writes a database of its own, which is a second format
    /// for the same nine columns. See [`wp_docx::merge`], where the reason
    /// for reading that one is set out.
    pub(super) fn type_a_new_list(&mut self) -> Response {
        self.typed_recipients = Recipients {
            headers: NEW_LIST_COLUMNS.iter().map(|name| (*name).to_owned()).collect(),
            rows: Vec::new(),
        };
        self.new_list_dialog()
    }

    /// The dialog, with what has been typed so far listed above the boxes.
    fn new_list_dialog(&mut self) -> Response {
        let rows: Vec<TreeRow> = (0..self.typed_recipients.len())
            .map(|at| {
                let record = self.typed_recipients.record(at);
                let said: Vec<String> = record
                    .iter()
                    .filter(|(_, value)| !value.trim().is_empty())
                    .map(|(_, value)| value.clone())
                    .collect();
                TreeRow::plain(&said.join(", "))
            })
            .collect();

        let mut fields = vec![
            Field::note("Type one person, press Add, and type the next."),
            Field::Tree { label: "On the list".to_owned(), rows, current: 0, scroll: 0 },
            Field::Heading("This person".to_owned()),
        ];
        for name in NEW_LIST_COLUMNS {
            fields.push(Field::Text { label: (*name).to_owned(), value: String::new() });
        }

        let dialog = Dialog::with_buttons(
            "New Address List",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button {
                    label: ADD_RECIPIENT.to_owned(),
                    answer: Answer::Named(ADD_RECIPIENT),
                    default: false,
                },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(520.0);
        self.ask(Asking::NewList, dialog)
    }

    /// Puts whoever is in the boxes on to the list.
    pub(super) fn add_typed_recipient(&mut self, dialog: &Dialog) -> Response {
        let values: Vec<(String, String)> = NEW_LIST_COLUMNS
            .iter()
            .enumerate()
            .map(|(at, name)| ((*name).to_owned(), dialog.said(FIRST_BOX + at)))
            .collect();
        if values.iter().all(|(_, value)| value.trim().is_empty()) {
            return self.report(crate::messages::t("Type somebody first"));
        }
        self.typed_recipients.add(&values);
        let count = self.typed_recipients.len();
        self.status = crate::messages::with("{0} on the list", &[&count.to_string()]);
        self.new_list_dialog()
    }

    /// Saves the typed list and uses it for the merge.
    pub(super) fn finish_new_list(&mut self, dialog: &Dialog) -> Response {
        // Whoever is still in the boxes counts: a person who typed the last
        // one and pressed OK meant to include them.
        let values: Vec<(String, String)> = NEW_LIST_COLUMNS
            .iter()
            .enumerate()
            .map(|(at, name)| ((*name).to_owned(), dialog.said(FIRST_BOX + at)))
            .collect();
        if values.iter().any(|(_, value)| !value.trim().is_empty()) {
            self.typed_recipients.add(&values);
        }
        if self.typed_recipients.is_empty() {
            return self.report(crate::messages::t("Nobody was typed, so no list was made"));
        }

        let filters = [FileFilter { label: "Comma-separated files", pattern: "*.csv" }];
        let Some(path) = wp_shell::dialog::save_file(t("Save Address List"), &filters, None) else {
            return self.report(crate::messages::t("The list was not saved, so nothing is merged"));
        };
        // The list the letters are merged from, and perhaps written over an
        // older one: replaced whole or not at all. See [`wp_files`].
        let list = self.typed_recipients.to_delimited();
        if let Err(error) = wp_files::replace_with(&path, list.as_bytes()) {
            return self.report(&format!("Cannot write {}: {error}", path.display()));
        }

        self.recipients = core::mem::take(&mut self.typed_recipients);
        self.left_out.clear();
        self.recipient_file = Some(PathBuf::from(&path));
        self.preview_record = None;
        self.forget_matches();
        // The document keeps being whatever sort of merge it already was:
        // typing a list says who the letters are for, not what they are.
        let kind = self.document.merge_source().map_or(Kind::Letters, |(kind, _)| kind);
        self.document.set_merge_source(kind, &path.to_string_lossy());
        let count = self.recipients.len();
        self.needs_redraw = true;
        self.edited(true, &crate::messages::with("{0} recipients typed", &[&count.to_string()]))
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

/// The four ways a merge can end.
///
/// Word's three — a new document, the printer, mail — and the one this
/// program adds, which is a file for each letter. Word gets to that by
/// merging to a document and saving it a page at a time; a person who wants
/// a hundred files should not have to.
pub(super) fn endings() -> Vec<String> {
    use crate::messages::t;
    vec![
        t("Edit Individual Documents").to_owned(),
        t("Print Documents").to_owned(),
        t("Send E-mail Messages").to_owned(),
        t("One File for Each Letter").to_owned(),
    ]
}

impl Editor {
    /// Drops open the ways a merge can end.
    pub(super) fn open_finishing(&mut self) -> Response {
        if self.recipients.is_empty() {
            return self.report("No recipients yet — use Select Recipients");
        }
        self.open_ribbon_menu(Choice::Finishing)
    }

    /// Does whichever was chosen.
    pub(super) fn choose_finishing(&mut self, index: usize) -> Response {
        self.popup = None;
        match index {
            0 => self.merge_to_a_document(),
            1 => self.merge_to_the_printer(),
            2 => self.merge_to_mail(),
            3 => self.merge_to_files(),
            _ => Response::Ignored,
        }
    }

    /// Every letter, one after another, in one new document.
    ///
    /// Word's Edit Individual Documents, and what a person most often wants:
    /// something to read through before a hundred sheets of paper come out of
    /// the printer.
    pub(super) fn merge_to_a_document(&mut self) -> Response {
        // The letters go where the letter is, this program having one window.
        // So the letter it came from has to be safe first, which is the same
        // question New and Open ask.
        if !self.may_discard() {
            return Response::Ignored;
        }
        let (body, written, skipped) = self.merged_body();
        if written == 0 {
            return self.report("Every recipient was left out");
        }
        let made = match wp_docx::Document::create(&body) {
            Ok(document) => document,
            Err(error) => {
                return self.report(&format!("The letters could not be put together: {error}"))
            }
        };
        let bytes = match made.save() {
            Ok(bytes) => bytes,
            Err(error) => {
                return self.report(&format!("The letters could not be put together: {error}"))
            }
        };
        let Ok(document) = wp_docx::Document::open(&bytes) else {
            return self.report("The letters could not be put together");
        };

        // A new document with no file of its own, which is what Word gives
        // back: the letters are the result, and the letter they came from is
        // untouched.
        self.set_document(document, None);
        self.report(&note(written, skipped, "letters"))
    }

    /// The same, and then the Print page on it.
    ///
    /// Word prints them straight off. This puts them in front of the person
    /// first, which is the same journey with the sheet of paper still in the
    /// tray: a merge that goes wrong goes wrong a hundred times.
    pub(super) fn merge_to_the_printer(&mut self) -> Response {
        let response = self.merge_to_a_document();
        if self.recipients.is_empty() {
            return response;
        }
        self.open_print()
    }

    /// Each letter handed to the machine's mail program.
    ///
    /// The address comes from whichever column the list calls an e-mail
    /// address — see [`super::matching`], which is where a column is matched
    /// to what it means. Nothing is sent: what comes up is a message waiting
    /// for the person to look at and send, which is the only honest thing for
    /// a word processor to do with somebody else's address book.
    pub(super) fn merge_to_mail(&mut self) -> Response {
        let Some(column) = self.address_column("e-mail", EMAIL_COLUMNS) else {
            return self
                .report("No column of e-mail addresses — Match Fields says which column is which");
        };
        let subject = self.document_name();

        let mut sent = 0usize;
        let mut without = 0usize;
        for index in self.included() {
            let record = self.recipients.record(index);
            let Some(address) = self.recipients.value(index, &column).filter(|to| !to.is_empty())
            else {
                without += 1;
                continue;
            };
            let mut copy = self.document.clone();
            copy.apply_merge_rules(&record, sent + 1);
            copy.apply_merge_record(&record);
            if !wp_shell::mail::compose(&address, &subject, &copy.plain_text()) {
                return self.report("This machine has no mail program to hand a letter to");
            }
            sent += 1;
        }

        if without > 0 {
            return self.report(&format!(
                "{sent} letters handed to the mail program, {without} with no address"
            ));
        }
        self.report(&format!("{sent} letters handed to the mail program"))
    }

    /// One file for each letter, named after the file that was asked for.
    pub(super) fn merge_to_files(&mut self) -> Response {
        let Some(chosen) = wp_shell::dialog::save_file(
            t("Finish & Merge"),
            &[FileFilter { label: "Word documents", pattern: "*.docx" }],
            self.file.as_deref(),
        ) else {
            return Response::Ignored;
        };

        let stem = chosen.with_extension("");
        let mut written = 0usize;
        let mut skipped = 0usize;
        for index in 0..self.recipients.len() {
            if !self.is_included(index) {
                skipped += 1;
                continue;
            }
            let record = self.recipients.record(index);
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
            // Each letter is a document, perhaps written over the same
            // letter merged before: see [`wp_files`].
            if let Err(error) = wp_files::replace_with(&path, &bytes) {
                return self.report(&format!("{} could not be written: {error}", path.display()));
            }
        }
        self.report(&note(written, skipped, "letters written"))
    }

    /// Every letter's blocks, one after another, each starting on a new page.
    ///
    /// Gives back what was built, how many letters went in and how many
    /// people were left out.
    fn merged_body(&mut self) -> (wp_docx::model::Body, usize, usize) {
        let mut body = wp_docx::model::Body::default();
        let mut written = 0usize;
        let mut skipped = 0usize;

        for index in 0..self.recipients.len() {
            if !self.is_included(index) {
                skipped += 1;
                continue;
            }
            let record = self.recipients.record(index);
            if self.document.record_is_skipped(&record) {
                skipped += 1;
                continue;
            }

            let mut copy = self.document.clone();
            copy.apply_merge_rules(&record, written + 1);
            copy.apply_merge_record(&record);
            let mut blocks = copy.body().blocks;
            if written > 0 {
                // Each letter starts on a sheet of its own, which is what a
                // letter is. Word writes a section break; a page break is the
                // same thing where the letters share a page setup, and they
                // do — they are copies of one document.
                if let Some(wp_docx::model::Block::Paragraph(first)) = blocks.first_mut() {
                    first.properties.page_break_before = Some(true);
                }
            }
            body.blocks.extend(blocks);
            written += 1;
        }
        (body, written, skipped)
    }

    /// Whether a person is one of those the merge is for.
    pub(super) fn is_included(&self, index: usize) -> bool {
        !self.left_out.contains(&index)
    }

    /// Everybody it is for, in order.
    fn included(&self) -> Vec<usize> {
        (0..self.recipients.len()).filter(|index| self.is_included(*index)).collect()
    }
}

/// The names a list gives the column holding an e-mail address.
const EMAIL_COLUMNS: &[&str] =
    &["E-mail", "Email", "E-mail Address", "Email Address", "Mail", "Почта"];

/// How a merge reports itself, with the people left out counted if there were
/// any.
fn note(written: usize, skipped: usize, what: &str) -> String {
    if skipped > 0 {
        format!("{written} {what}, {skipped} left out")
    } else {
        format!("{written} {what}")
    }
}

impl Editor {
    /// Word's Mail Merge Recipients: everybody on the list, with a tick
    /// against those the letter is for.
    ///
    /// A dialog rather than a list that drops open, because what it is for is
    /// not picking one person but going down the whole list and taking a few
    /// off it. Word's is a dialog too, and a larger one: it sorts, it filters
    /// and it finds duplicates, which is named in the roadmap.
    pub(super) fn open_recipient_list(&mut self) -> Response {
        if self.recipients.is_empty() {
            return self.report("No recipients yet — use Select Recipients");
        }
        self.recipient_dialog(self.preview_record.unwrap_or(0))
    }

    /// The dialog itself, with one row of the list chosen.
    ///
    /// Built again after every button rather than changed in place: sorting
    /// moves every row, and a dialog showing the order from before the sort
    /// would be a list nobody could work with.
    fn recipient_dialog(&mut self, current: usize) -> Response {
        let rows: Vec<TreeRow> = (0..self.recipients.len())
            .map(|at| TreeRow::ticked(0, &self.describe_recipient(at), self.is_included(at)))
            .collect();
        let mut columns = vec![crate::messages::t("Every column").to_owned()];
        columns.extend(self.recipients.headers.iter().cloned());

        let dialog = Dialog::with_buttons(
            "Mail Merge Recipients",
            vec![
                Field::note("The letter goes to everybody ticked."),
                Field::Tree {
                    label: "Recipients".to_owned(),
                    rows,
                    current: current.min(self.recipients.len().saturating_sub(1)),
                    scroll: 0,
                },
                Field::Heading("Sort, filter and check".to_owned()),
                Field::Choice { label: "Column".to_owned(), items: columns, current: 0 },
                Field::Choice {
                    label: "Order".to_owned(),
                    items: vec![
                        crate::messages::t("A to Z").to_owned(),
                        crate::messages::t("Z to A").to_owned(),
                    ],
                    current: 0,
                },
                Field::Text { label: "Holding".to_owned(), value: String::new() },
            ],
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: SORT.to_owned(), answer: Answer::Named(SORT), default: false },
                Button { label: FILTER.to_owned(), answer: Answer::Named(FILTER), default: false },
                Button {
                    label: DUPLICATES.to_owned(),
                    answer: Answer::Named(DUPLICATES),
                    default: false,
                },
                Button {
                    label: VALIDATE.to_owned(),
                    answer: Answer::Named(VALIDATE),
                    default: false,
                },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(640.0);
        self.ask(Asking::Recipients, dialog)
    }

    /// Which column one of the buttons is to work on, or none for all of
    /// them.
    fn chosen_column(&self, dialog: &Dialog) -> Option<String> {
        let at = dialog.chose(RECIPIENT_COLUMN);
        (at > 0).then(|| self.recipients.headers.get(at - 1).cloned()).flatten()
    }

    /// Sort, Filter, Find Duplicates and Validate.
    ///
    /// Each takes the ticks off the dialog first, because a person who
    /// unticked three people and then sorted the list would otherwise find
    /// them ticked again.
    pub(super) fn recipient_button(&mut self, dialog: &Dialog, button: &str) -> Response {
        self.take_recipient_ticks(dialog);
        let column = self.chosen_column(dialog);
        let wanted = dialog.said(RECIPIENT_WANTED);
        let mut current = dialog.chose_row(RECIPIENT_ROWS);

        let said = match button {
            SORT => {
                let Some(column) = column else {
                    return self.report(crate::messages::t("Choose a column to sort by"));
                };
                let order = self.recipients.sorted_by(&column, dialog.chose(RECIPIENT_ORDER) == 0);
                // The ticks go with the rows they were against, or sorting a
                // list would quietly change who gets a letter.
                let left_out: std::collections::BTreeSet<usize> = order
                    .iter()
                    .enumerate()
                    .filter(|(_, was)| self.left_out.contains(was))
                    .map(|(now, _)| now)
                    .collect();
                self.recipients.reorder(&order);
                self.left_out = left_out;
                current = 0;
                crate::messages::with("Sorted by {0}", &[&column])
            }
            FILTER => {
                if wanted.trim().is_empty() {
                    return self.report(crate::messages::t("Type what a row has to hold"));
                }
                let keep = self.recipients.matching(column.as_deref().unwrap_or(""), &wanted);
                let before = self.left_out.len();
                for at in 0..self.recipients.len() {
                    if !keep.contains(&at) {
                        self.left_out.insert(at);
                    }
                }
                current = keep.first().copied().unwrap_or(0);
                crate::messages::with(
                    "{0} left in, {1} left out",
                    &[
                        &keep.len().to_string(),
                        &(self.left_out.len().saturating_sub(before)).to_string(),
                    ],
                )
            }
            DUPLICATES => {
                let copies = self.recipients.duplicates();
                if copies.is_empty() {
                    crate::messages::t("No two of them say the same thing").to_owned()
                } else {
                    current = copies[0];
                    for at in &copies {
                        self.left_out.insert(*at);
                    }
                    crate::messages::with(
                        "{0} left out as copies of somebody already on the list",
                        &[&copies.len().to_string()],
                    )
                }
            }
            VALIDATE => {
                let bad: Vec<usize> = (0..self.recipients.len())
                    .filter(|at| !self.recipients.missing_from(*at).is_empty())
                    .collect();
                match bad.first() {
                    None => {
                        crate::messages::t("Every one of them has a name and an address").to_owned()
                    }
                    Some(first) => {
                        current = *first;
                        let missing = self.recipients.missing_from(*first).join(" and ");
                        crate::messages::with(
                            "{0} of them are short of something: this one has no {1}",
                            &[&bad.len().to_string(), &missing],
                        )
                    }
                }
            }
            _ => return Response::Ignored,
        };

        self.status = said;
        self.recipient_dialog(current)
    }

    /// Reads the ticks out of the dialog and into the list of who is left
    /// out.
    fn take_recipient_ticks(&mut self, dialog: &Dialog) {
        let rows = dialog.tree_rows(RECIPIENT_ROWS);
        if rows.len() != self.recipients.len() {
            return;
        }
        self.left_out.clear();
        for (at, row) in rows.iter().enumerate() {
            if row.tick == Some(false) {
                self.left_out.insert(at);
            }
        }
    }

    /// Takes the ticks back off the dialog.
    pub(super) fn apply_recipient_list(&mut self, dialog: &Dialog) -> Response {
        let Some(Field::Tree { rows, current, .. }) = dialog.fields.get(RECIPIENT_ROWS) else {
            return Response::Ignored;
        };
        self.left_out.clear();
        for (at, row) in rows.iter().enumerate() {
            if row.tick == Some(false) {
                self.left_out.insert(at);
            }
        }
        // The row the keyboard was on is the one the preview shows, which is
        // what makes going down the list and looking at each letter work.
        if *current < self.recipients.len() && self.is_included(*current) {
            self.preview_record = Some(*current);
        }
        self.relayout();
        self.needs_redraw = true;

        let left_out = self.left_out.len();
        if left_out > 0 {
            return self.report(&format!(
                "{} recipients, {left_out} left out",
                self.recipients.len() - left_out
            ));
        }
        self.report(&format!("{} recipients", self.recipients.len()))
    }
}

/// Where the list of people sits in that dialog.
const RECIPIENT_ROWS: usize = 1;
/// And the rest of Word's dialog: which column to work on, which way round,
/// and what to look for.
const RECIPIENT_COLUMN: usize = 3;
const RECIPIENT_ORDER: usize = 4;
const RECIPIENT_WANTED: usize = 5;

/// The buttons under the list, which are Word's four.
pub(super) const SORT: &str = "Sort";
pub(super) const FILTER: &str = "Filter";
pub(super) const DUPLICATES: &str = "Find Duplicates";
pub(super) const VALIDATE: &str = "Validate";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::dialog::Answer;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// A letter with two merge fields in it, and three people to send it to.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Dear ")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        editor.recipients = Recipients::parse(
            b"Name,E-mail\nAda Lovelace,ada@example.com\nGrace Hopper,grace@example.com\nAlan Turing,alan@example.com\n",
        );
        // The field goes after the words, which is where the caret is.
        let end = editor.document.paragraph_text(0).unwrap_or_default().len();
        editor.document.set_caret(wp_docx::TextPosition::new(0, end));
        editor.document.insert_field(&merge_instruction("Name"), "«Name»");
        editor.relayout();
        // Saved, so that merging does not stop to ask about the letter: what
        // that asks is tested where saving is.
        let _ = editor.document.mark_saved();
        editor
    }

    #[test]
    fn the_button_offers_the_three_ways_word_offers_and_one_more() {
        let items = endings();
        assert_eq!(items.len(), 4);
        assert!(items[0].contains("Individual Documents"), "{items:?}");
        assert!(items[1].contains("Print"), "{items:?}");
        assert!(items[2].contains("E-mail"), "{items:?}");
        assert!(items[3].contains("Each Letter"), "{items:?}");
    }

    #[test]
    fn merging_to_a_document_puts_every_letter_in_one() {
        let mut editor = editor();
        editor.merge_to_a_document();

        let text = editor.document.plain_text();
        for name in ["Ada Lovelace", "Grace Hopper", "Alan Turing"] {
            assert!(text.contains(name), "{name} is not in the letters: {text:?}");
        }
        // And the letter that was merged is gone: what is open now is the
        // result, with no file of its own.
        assert!(editor.file.is_none());
    }

    #[test]
    fn each_letter_after_the_first_starts_on_a_new_page() {
        let mut editor = editor();
        editor.merge_to_a_document();

        let mut breaks = 0usize;
        for at in 0..editor.document.paragraph_count() {
            editor.document.set_caret(wp_docx::TextPosition::new(at, 0));
            if editor.document.paragraph_format_here().page_break_before {
                breaks += 1;
            }
        }
        assert_eq!(breaks, 2, "three letters need two breaks between them");
    }

    #[test]
    fn a_recipient_left_out_gets_no_letter() {
        let mut editor = editor();
        editor.left_out.insert(1);
        editor.merge_to_a_document();

        let text = editor.document.plain_text();
        assert!(text.contains("Ada Lovelace"));
        assert!(!text.contains("Grace Hopper"), "somebody left out got a letter: {text:?}");
        assert!(text.contains("Alan Turing"));
    }

    #[test]
    fn the_recipient_dialog_ticks_everybody_and_takes_a_tick_off() {
        let mut editor = editor();
        editor.open_recipient_list();
        let dialog = editor.dialog.as_mut().expect("the dialog");
        let Some(Field::Tree { rows, .. }) = dialog.fields.get_mut(RECIPIENT_ROWS) else {
            panic!("no list of recipients");
        };
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.tick == Some(true)), "somebody started unticked");
        rows[1].tick = Some(false);

        editor.finish_dialog(Answer::Accept);
        assert!(editor.is_included(0));
        assert!(!editor.is_included(1), "the tick did not come off");
        assert!(editor.is_included(2));
        assert!(editor.status.contains("1 left out"), "{}", editor.status);
    }

    #[test]
    fn a_merge_with_nobody_on_the_list_says_so_rather_than_writing_nothing() {
        let mut editor = editor();
        editor.recipients = Recipients::default();
        editor.open_finishing();
        assert!(editor.status.contains("No recipients"), "{}", editor.status);
        assert!(editor.popup.is_none(), "it offered to finish a merge with nobody on it");
    }

    #[test]
    fn everybody_left_out_is_said_rather_than_an_empty_document_made() {
        let mut editor = editor();
        for index in 0..3 {
            editor.left_out.insert(index);
        }
        let before = editor.document.plain_text();
        editor.merge_to_a_document();
        assert_eq!(editor.document.plain_text(), before, "the letter was thrown away");
        assert!(editor.status.contains("left out"), "{}", editor.status);
    }
    /// An editor with four people on its list, two of them the same.
    fn with_recipients() -> Editor {
        let mut editor = editor();
        editor.recipients = Recipients::parse(
            b"Last Name,City\nOgg,Lancre\nNitt,Ankh-Morpork\nogg,lancre\n,Genua\n",
        );
        editor
    }

    /// Opens the dialog and hands back what it holds.
    fn recipients_dialog(editor: &mut Editor) -> Dialog {
        editor.open_recipient_list();
        editor.dialog.clone().expect("the dialog")
    }

    #[test]
    fn the_dialog_offers_word_s_four_buttons_and_the_columns_to_use_them_on() {
        let mut editor = with_recipients();
        let dialog = recipients_dialog(&mut editor);

        for label in [SORT, FILTER, DUPLICATES, VALIDATE] {
            assert!(
                dialog.buttons.iter().any(|button| button.label == label),
                "no {label} button: {:?}",
                dialog.buttons.iter().map(|button| button.label.clone()).collect::<Vec<_>>()
            );
        }
        // Every column, and the one that means all of them.
        let Some(Field::Choice { items, .. }) = dialog.fields.get(RECIPIENT_COLUMN) else {
            panic!("no column to choose")
        };
        assert_eq!(items.len(), 3, "{items:?}");
    }

    #[test]
    fn sorting_reorders_the_list_and_carries_the_ticks_with_it() {
        let mut editor = with_recipients();
        let mut dialog = recipients_dialog(&mut editor);
        // Ogg, who is first, is left out; after sorting by city Ogg is last.
        if let Some(Field::Tree { rows, .. }) = dialog.fields.get_mut(RECIPIENT_ROWS) {
            rows[0].tick = Some(false);
        }
        if let Some(Field::Choice { current, .. }) = dialog.fields.get_mut(RECIPIENT_COLUMN) {
            *current = 2;
        }
        editor.recipient_button(&dialog, SORT);

        let order: Vec<String> = (0..editor.recipients.len())
            .map(|at| editor.recipients.value(at, "City").unwrap_or_default())
            .collect();
        assert_eq!(order, vec!["Ankh-Morpork", "Genua", "Lancre", "lancre"], "{order:?}");
        // Whoever was left out is still the same person, not the same row.
        let left_out: Vec<String> = (0..editor.recipients.len())
            .filter(|at| !editor.is_included(*at))
            .map(|at| editor.recipients.value(at, "Last Name").unwrap_or_default())
            .collect();
        assert_eq!(left_out, vec!["Ogg".to_owned()], "the tick did not travel with the row");
    }

    #[test]
    fn filtering_leaves_out_everybody_the_words_do_not_fit() {
        let mut editor = with_recipients();
        let mut dialog = recipients_dialog(&mut editor);
        if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(RECIPIENT_WANTED) {
            *value = "lancre".to_owned();
        }
        editor.recipient_button(&dialog, FILTER);

        assert!(editor.is_included(0), "Ogg of Lancre was left out");
        assert!(!editor.is_included(1), "Nitt of Ankh-Morpork was left in");
        assert!(editor.is_included(2), "the other Ogg of lancre was left out over its case");
        assert!(editor.status.contains("left in"), "{}", editor.status);
    }

    #[test]
    fn finding_duplicates_leaves_the_copies_out_and_the_first_in() {
        let mut editor = with_recipients();
        let dialog = recipients_dialog(&mut editor);
        editor.recipient_button(&dialog, DUPLICATES);

        assert!(editor.is_included(0), "the original was left out");
        assert!(!editor.is_included(2), "the copy was left in");
        assert!(editor.status.contains("copies"), "{}", editor.status);
    }

    #[test]
    fn validating_says_which_of_them_is_short_of_something() {
        let mut short = with_recipients();
        let dialog = recipients_dialog(&mut short);
        short.recipient_button(&dialog, VALIDATE);
        assert!(short.status.contains("no a name"), "{}", short.status);

        // A list where everybody has both says so.
        let mut whole = editor();
        whole.recipients = Recipients::parse(b"Last Name,City\nOgg,Lancre\n");
        let dialog = recipients_dialog(&mut whole);
        whole.recipient_button(&dialog, VALIDATE);
        assert!(whole.status.contains("has a name and an address"), "{}", whole.status);
    }

    #[test]
    fn a_sort_with_no_column_chosen_says_what_it_wants() {
        let mut editor = with_recipients();
        let dialog = recipients_dialog(&mut editor);
        editor.recipient_button(&dialog, SORT);
        assert!(editor.status.contains("column"), "{}", editor.status);
    }

    #[test]
    fn a_list_typed_from_nothing_gathers_people_one_at_a_time() {
        let mut editor = editor();
        editor.type_a_new_list();
        let mut dialog = editor.dialog.clone().expect("the dialog");
        assert_eq!(dialog.title, "New Address List");

        // The first person: a surname and a city, and the rest left empty.
        if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(FIRST_BOX + 2) {
            *value = "Ogg".to_owned();
        }
        if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(FIRST_BOX + 5) {
            *value = "Lancre".to_owned();
        }
        editor.add_typed_recipient(&dialog);

        assert_eq!(editor.typed_recipients.len(), 1);
        assert_eq!(editor.typed_recipients.value(0, "Last Name").as_deref(), Some("Ogg"));
        assert_eq!(editor.typed_recipients.value(0, "City").as_deref(), Some("Lancre"));
        // And the dialog is up again with the boxes empty and the list above.
        let dialog = editor.dialog.as_ref().expect("asked again");
        assert!(dialog.said(FIRST_BOX + 2).is_empty(), "the boxes kept what was typed");
        assert_eq!(dialog.tree_rows(1).len(), 1, "the list does not show who is on it");
    }

    #[test]
    fn adding_nobody_says_so() {
        let mut editor = editor();
        editor.type_a_new_list();
        let dialog = editor.dialog.clone().expect("the dialog");
        editor.add_typed_recipient(&dialog);
        assert!(editor.typed_recipients.is_empty());
        assert!(editor.status.contains("Type somebody"), "{}", editor.status);
    }

    #[test]
    fn typing_a_list_and_opening_one_are_the_two_ways_to_get_recipients() {
        // Word's menu has three; the third reads another program's address
        // book, which **J5** names as not read. What is here is that the two
        // this program has are reachable and do different things.
        let mut typing = editor();
        typing.choose_recipient_source(0);
        assert_eq!(
            typing.dialog.as_ref().map(|dialog| dialog.title.clone()),
            Some("New Address List".to_owned()),
            "Type a New List opened nothing"
        );

        let mut other = editor();
        assert_eq!(other.choose_recipient_source(9), wp_shell::Response::Ignored);
    }
}

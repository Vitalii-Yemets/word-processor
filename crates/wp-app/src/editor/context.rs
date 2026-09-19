//! The menu the right button opens.
//!
//! # Why what it offers depends on where it was opened
//!
//! Because that is the whole point of it. A right-click on a misspelled word
//! offers the spellings; on a picture, what can be done to a picture; in a
//! table, what can be done to a table. A menu that offered the same twenty
//! things everywhere would be a second ribbon, and nobody would use it.
//!
//! # What Word does that this does not
//!
//! Word shows a small floating toolbar of formatting buttons above the menu,
//! and the menu itself has icons and separators. This is a plain list of the
//! same commands. What matters is that the right button opens something, that
//! what it opens is about what is under the pointer, and that the caret moves
//! to the click first unless the click was inside the selection — all of which
//! is here, because that is the behaviour a person has in their hands.

use wp_shell::Response;

use crate::chrome::icons::Icon;
use crate::chrome::popup::{Kind, Row};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// How wide the menu is drawn.
const WIDTH: f32 = 230.0;

/// One line of the menu.
#[derive(Clone, Debug)]
struct Entry {
    /// What it does, or nothing where the line is only a divider.
    command: Option<Command>,
    label: String,
    icon: Icon,
    kind: Kind,
}

impl Entry {
    /// Something the menu offers.
    fn item(command: Command, label: impl Into<String>, icon: Icon) -> Self {
        Self { command: Some(command), label: label.into(), icon, kind: Kind::Choice }
    }

    /// A line between two groups of it.
    fn line() -> Self {
        Self { command: None, label: String::new(), icon: Icon::None, kind: Kind::Separator }
    }

    /// The same entry, greyed out unless the condition holds.
    ///
    /// Greyed rather than left out, because a menu whose entries move about
    /// depending on what is selected is a menu nobody can learn.
    fn only_if(mut self, applies: bool) -> Self {
        if !applies {
            self.kind = Kind::Disabled;
        }
        self
    }
}

impl Editor {
    /// Opens the menu for whatever is under the pointer.
    pub(super) fn open_context_menu(&mut self, x: i32, y: i32) -> Response {
        // Anything already dropped open is put away first: two menus at once is
        // one more than a person asked for.
        if self.popup.is_some() || self.palette.is_some() || self.table_grid.is_some() {
            self.popup = None;
            self.palette = None;
            self.table_grid = None;
            self.needs_redraw = true;
        }

        // Word moves the caret to the click before opening the menu, unless the
        // click was inside the selection — which is what lets "cut" mean the
        // selection rather than the word that was right-clicked.
        if !self.click_is_inside_selection(x, y) {
            if let Some(position) = self.position_at(x, y) {
                self.document.set_caret(position);
            }
        }

        self.pending_issue = None;
        self.pending_spellings = Vec::new();
        self.pending_synonyms = Vec::new();
        self.pending_word = None;
        let entries = self.context_entries(x, y);
        // The word the checker marked under the pointer, and what it might
        // have been meant as: kept beside the menu so that choosing one knows
        // which word it is for.
        if let Some(issue) = self.position_at(x, y).and_then(|at| self.issue_at(at)) {
            if self.show_proofing {
                self.pending_spellings = if issue.kind.is_spelling() {
                    self.spellings_for(&issue)
                } else {
                    issue.suggestion.iter().cloned().collect()
                };
                self.pending_issue = Some(issue);
            }
        }
        if entries.is_empty() {
            return Response::Ignored;
        }

        let items = entries.iter().map(|entry| entry.label.clone()).collect();
        let rows = entries.iter().map(|entry| Row::new(entry.kind, entry.icon)).collect();
        self.group_commands = entries.iter().map(|entry| entry.command).collect();
        // Under the pointer, which is where a context menu goes — and in
        // the furniture's own coordinates, which is where menus live.
        let left = crate::chrome::mirror::flip(x) as f32;
        self.popup =
            Some(Popup::new(Choice::Context, items, None, left, y as f32, WIDTH).with_rows(rows));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Runs whichever entry was chosen.
    pub(super) fn choose_context_entry(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(command) = self.group_commands.get(index).copied().flatten() else {
            return Response::Ignored;
        };
        self.run(command)
    }

    /// Whether a point is inside the selection, which decides whether the menu
    /// moves the caret first.
    #[must_use]
    fn click_is_inside_selection(&self, x: i32, y: i32) -> bool {
        let Some((start, end)) = self.document.selection() else { return false };
        let Some(at) = self.position_at(x, y) else { return false };
        at >= start && at <= end
    }

    /// What the menu offers where it was opened.
    ///
    /// Each line carries its drawing, and the groups are kept apart by lines
    /// across the menu — which is how Word's is laid out and how anybody's eye
    /// finds "Cut" without reading the whole thing.
    #[must_use]
    fn context_entries(&mut self, x: i32, y: i32) -> Vec<Entry> {
        // A drawing first: a right-click on a picture is about the picture,
        // whatever the text round it is doing.
        if self.drawing_under(x, y).is_some() {
            return vec![
                Entry::item(Command::Cut, "Cut", Icon::Scissors),
                Entry::item(Command::Copy, "Copy", Icon::Copy),
                Entry::line(),
                Entry::item(Command::WrapText, "Wrap Text", Icon::WrapText),
                Entry::item(Command::Position, "Position", Icon::Position),
                Entry::line(),
                Entry::item(Command::BringForward, "Bring Forward", Icon::BringForward),
                Entry::item(Command::BringToFront, "Bring to Front", Icon::BringForward),
                Entry::item(Command::SendBackward, "Send Backward", Icon::SendBackward),
                Entry::item(Command::SendToBack, "Send to Back", Icon::SendBackward),
                Entry::item(Command::SelectionPane, "Selection Pane", Icon::SelectionPane),
            ];
        }

        // Cut and copy need something to work on, and paste needs something to
        // put down. Word shows all three either way and greys the ones that
        // would do nothing, so the menu keeps its shape and its meaning.
        let selected = self.document.selection().is_some();
        let mut entries = vec![
            Entry::item(Command::Cut, "Cut", Icon::Scissors).only_if(selected),
            Entry::item(Command::Copy, "Copy", Icon::Copy).only_if(selected),
            Entry::item(Command::Paste, "Paste", Icon::Clipboard).only_if(self.can_paste()),
        ];

        // Word's right-click offers the paste options themselves and not only
        // Paste, which saves pasting and then changing one's mind. They are
        // there only when there is formatting to decide about: text another
        // program put on the clipboard is words, and there is one way to put
        // words down.
        if self.clipboard_has_formatting() {
            entries.extend([
                Entry::item(Command::PasteKeepSource, "Keep Source Formatting", Icon::Clipboard),
                Entry::item(Command::PasteMerge, "Merge Formatting", Icon::Brush),
                Entry::item(Command::PasteAsPicture, "Picture", Icon::Picture),
                Entry::item(Command::PasteTextOnly, "Keep Text Only", Icon::Letter),
            ]);
        }

        // The spellings for a word the checker does not know, which is what a
        // right-click on a red underline is for: Word puts them at the top,
        // then what else can be done about the word.
        if let Some(issue) = self.position_at(x, y).and_then(|at| self.issue_at(at)) {
            if self.show_proofing {
                let mut spelling: Vec<Entry> = Vec::new();
                if issue.kind.is_spelling() {
                    for (index, offered) in self.spellings_for(&issue).iter().enumerate().take(5) {
                        spelling.push(Entry::item(
                            Command::Correct(index as u8),
                            offered.clone(),
                            Icon::Spelling,
                        ));
                    }
                    if spelling.is_empty() {
                        spelling.push(
                            Entry::item(Command::Spelling, "(no spelling suggestions)", Icon::None)
                                .only_if(false),
                        );
                    }
                    spelling.push(Entry::line());
                    spelling.push(Entry::item(Command::IgnoreAll, "Ignore All", Icon::None));
                    spelling.push(Entry::item(
                        Command::AddToDictionary,
                        "Add to Dictionary",
                        Icon::Spelling,
                    ));
                } else {
                    spelling.push(Entry::item(
                        Command::Spelling,
                        issue.kind.message(),
                        Icon::Spelling,
                    ));
                }
                spelling.push(Entry::line());
                spelling.append(&mut entries);
                entries = spelling;
            }
        }

        // The words that mean what the word under the pointer means, a
        // handful of them, and the rest behind Thesaurus — which is where
        // Word keeps them, one level down.
        if let Some(at) = self.position_at(x, y) {
            let synonyms = self.synonyms_at(at);
            if !synonyms.is_empty() {
                entries.push(Entry::line());
                entries.push(
                    Entry::item(Command::Thesaurus, "Synonyms", Icon::Thesaurus).only_if(false),
                );
                for (index, synonym) in synonyms.iter().enumerate() {
                    entries.push(Entry::item(
                        Command::Synonym(index as u8),
                        format!("    {synonym}"),
                        Icon::None,
                    ));
                }
                entries.push(Entry::item(Command::Thesaurus, "    Thesaurus…", Icon::None));
            }
        }

        // And what it is in another language, which Word offers here too.
        entries.push(Entry::line());
        entries.push(Entry::item(Command::TranslateSelection, "Translate", Icon::Translate));

        entries.extend([
            Entry::line(),
            Entry::item(Command::ChooseFont, "Font…", Icon::Letter),
            Entry::item(Command::ChooseStyle, "Styles", Icon::Themes),
            Entry::item(Command::Bullets, "Bullets", Icon::Bullets),
            Entry::item(Command::Numbering, "Numbering", Icon::Numbering),
            Entry::line(),
            Entry::item(Command::InsertLink, "Link…", Icon::Link),
            Entry::item(Command::NewComment, "New Comment", Icon::NewComment),
        ]);

        // What can be done to a repeating section, when the caret is in one
        // of its items: Word's three.
        if self.in_repeating_item() {
            entries.extend([
                Entry::line(),
                Entry::item(Command::RepeatItemBefore, "Insert Item Before", Icon::Outline),
                Entry::item(Command::RepeatItemAfter, "Insert Item After", Icon::Outline),
                Entry::item(
                    Command::DeleteRepeatItem,
                    "Delete Repeating Section Item",
                    Icon::DeleteRow,
                ),
            ]);
        }

        // And what can be done to a table, when the caret is in one.
        if self.document.table_here().is_some() {
            entries.extend([
                Entry::line(),
                Entry::item(Command::InsertRowBelow, "Insert Row", Icon::InsertRowBelow),
                Entry::item(Command::InsertColumnRight, "Insert Column", Icon::InsertColumnRight),
                Entry::item(Command::DeleteRow, "Delete Row", Icon::DeleteRow),
                Entry::item(Command::DeleteColumn, "Delete Column", Icon::DeleteColumn),
                Entry::item(Command::TableProperties, "Table Properties", Icon::TableProperties),
            ]);
        }
        entries
    }

    /// Whether there is anything to paste.
    #[must_use]
    fn can_paste(&self) -> bool {
        self.clipboard.is_some() || !wp_shell::clipboard::contents().is_empty()
    }
}

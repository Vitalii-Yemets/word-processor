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
#[derive(Clone, Copy, Debug)]
struct Entry {
    /// What it does, or nothing where the line is only a divider.
    command: Option<Command>,
    label: &'static str,
    icon: Icon,
    kind: Kind,
}

impl Entry {
    /// Something the menu offers.
    fn item(command: Command, label: &'static str, icon: Icon) -> Self {
        Self { command: Some(command), label, icon, kind: Kind::Choice }
    }

    /// A line between two groups of it.
    fn line() -> Self {
        Self { command: None, label: "", icon: Icon::None, kind: Kind::Separator }
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

        let entries = self.context_entries(x, y);
        if entries.is_empty() {
            return Response::Ignored;
        }

        let items = entries.iter().map(|entry| entry.label.to_owned()).collect();
        let rows = entries.iter().map(|entry| Row::new(entry.kind, entry.icon)).collect();
        self.group_commands = entries.iter().map(|entry| entry.command).collect();
        // Under the pointer, which is where a context menu goes.
        self.popup = Some(
            Popup::new(Choice::Context, items, None, x as f32, y as f32, WIDTH).with_rows(rows),
        );
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
    fn context_entries(&self, x: i32, y: i32) -> Vec<Entry> {
        // A drawing first: a right-click on a picture is about the picture,
        // whatever the text round it is doing.
        if self.shape_at(x, y).is_some() {
            return vec![
                Entry::item(Command::Cut, "Cut", Icon::Scissors),
                Entry::item(Command::Copy, "Copy", Icon::Copy),
                Entry::line(),
                Entry::item(Command::WrapText, "Wrap Text", Icon::WrapText),
                Entry::item(Command::Position, "Position", Icon::Position),
                Entry::line(),
                Entry::item(Command::BringForward, "Bring Forward", Icon::BringForward),
                Entry::item(Command::SendBackward, "Send Backward", Icon::SendBackward),
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
        // right-click on a red underline is for.
        if self.misspelling_at(x, y) {
            entries.push(Entry::line());
            entries.push(Entry::item(Command::Spelling, "Spelling…", Icon::Spelling));
        }

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
        self.clipboard.is_some() || wp_shell::clipboard::text().is_some_and(|text| !text.is_empty())
    }

    /// Whether the point is on a word the spelling check has marked.
    #[must_use]
    fn misspelling_at(&self, x: i32, y: i32) -> bool {
        if !self.show_proofing {
            return false;
        }
        let Some(at) = self.position_at(x, y) else { return false };
        self.issues.iter().any(|issue| {
            issue.paragraph == at.paragraph && at.offset >= issue.start && at.offset <= issue.end
        })
    }
}

//! Word's Customize Ribbon and Quick Access Toolbar: which commands go where.
//!
//! # Why they are two tabs of Options rather than a dialog of their own
//!
//! Because they are two categories of Word's Options, reached from the same
//! place as everything else in [`super::optionsdialog`]. What Word shows down
//! the left-hand side of that dialog this one shows along the top, which is the
//! one difference, and it is the difference the whole dialog already has.
//!
//! # The shape of a page
//!
//! Two lists side by side, as Word has them: every command on the left, and on
//! the right either the toolbar or the ribbon. What sits between Word's two
//! lists — Add and Remove — and beside its right-hand one — the two arrows, and
//! Reset — is along the bottom here, where every other button in this program's
//! dialogs is. They are drawn only on the two tabs they work on, because a
//! button that changed a list nobody can see would be worse than no button.
//!
//! # Where the working copy lives
//!
//! In [`Editor::editing_chrome`], for the reason the AutoCorrect dialog keeps
//! one: every button changes a list and leaves the dialog standing, which means
//! building it again, and a dialog about to be thrown away is no place to keep
//! what has been changed. Nothing reaches the ribbon until OK is pressed.

use wp_shell::Response;

use crate::chrome::customise::Customisation;
use crate::chrome::dialog::{Dialog, Field, TreeRow};
use crate::chrome::ribbon::{self, Tab};
use crate::chrome::Command;

use super::Editor;

/// Where these two tabs begin in the Options dialog's list of fields.
///
/// Checked where the dialog is built, so a field put in above them fails loudly
/// rather than quietly making every row here point at the wrong thing.
pub(super) const FIRST: usize = 19;

// The Quick Access Toolbar tab.
pub(super) const TAB_QUICK: usize = FIRST;
pub(super) const QUICK_ROW: usize = FIRST + 1;
pub(super) const QUICK_ALL: usize = FIRST + 2;
pub(super) const QUICK_LIST: usize = FIRST + 3;

// The Customize Ribbon tab.
pub(super) const TAB_RIBBON: usize = FIRST + 4;
pub(super) const RIBBON_ROW: usize = FIRST + 5;
pub(super) const RIBBON_ALL: usize = FIRST + 6;
pub(super) const RIBBON_TREE: usize = FIRST + 7;

/// Which tab of Options each of them is.
pub(super) const QUICK_PAGE: usize = 3;
pub(super) const RIBBON_PAGE: usize = 4;

/// Word's buttons beside the two lists.
pub(super) const ADD: &str = "Add";
pub(super) const REMOVE: &str = "Remove";
pub(super) const MOVE_UP: &str = "Move Up";
pub(super) const MOVE_DOWN: &str = "Move Down";
pub(super) const RESET: &str = "Reset";

impl Editor {
    /// The fields of the two tabs, which the Options dialog puts on the end of
    /// its own.
    pub(super) fn customise_fields(&self) -> Vec<Field> {
        vec![
            // --- Quick Access Toolbar ---------------------------------------
            Field::Tab("Quick Access Toolbar".to_owned()),
            Field::Columns(2),
            Field::Tree {
                label: "Choose commands from".to_owned(),
                rows: every_command(),
                current: 0,
                scroll: 0,
            },
            Field::Tree {
                label: "Customize Quick Access Toolbar".to_owned(),
                rows: self
                    .editing_chrome
                    .quick
                    .iter()
                    .filter_map(|command| ribbon::name_of(*command))
                    .map(TreeRow::plain)
                    .collect(),
                current: 0,
                scroll: 0,
            },
            // --- Customize Ribbon -------------------------------------------
            Field::Tab("Customize Ribbon".to_owned()),
            Field::Columns(2),
            Field::Tree {
                label: "Choose commands from".to_owned(),
                rows: every_command(),
                current: 0,
                scroll: 0,
            },
            Field::Tree {
                label: "Customize the Ribbon".to_owned(),
                rows: self.ribbon_rows(),
                current: 0,
                scroll: 0,
            },
        ]
    }

    /// What the two tabs are made of, for the check the dialog makes on itself.
    pub(super) fn customise_kinds() -> Vec<(usize, &'static str)> {
        vec![
            (TAB_QUICK, "a tab"),
            (QUICK_ROW, "a row"),
            (QUICK_ALL, "a list of rows"),
            (QUICK_LIST, "a list of rows"),
            (TAB_RIBBON, "a tab"),
            (RIBBON_ROW, "a row"),
            (RIBBON_ALL, "a list of rows"),
            (RIBBON_TREE, "a list of rows"),
        ]
    }

    /// The ribbon as a tree: a row per tab, a row per group under it with a
    /// tick box, and a row for each command a person added to that group.
    ///
    /// Only the added ones. A command that came with the group is not on the
    /// list because it cannot be taken off — see
    /// [`Customisation::remove_from_group`] for why — and a row whose only
    /// button does nothing to it would be a row that lies.
    fn ribbon_rows(&self) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        for tab in customisable_tabs() {
            rows.push(TreeRow::under(0, tab.label()).opening(false));
            for label in self.editing_chrome.group_order(tab) {
                let added = self.editing_chrome.added_to(tab, label);
                let mut row = TreeRow::ticked(1, label, !self.editing_chrome.is_hidden(tab, label));
                // A triangle only where there is something under it to fold
                // open. One that opened onto nothing would be a triangle that
                // does nothing.
                if !added.is_empty() {
                    // Open, because what was added is what a person came to
                    // look at.
                    row = row.opening(true);
                }
                rows.push(row);
                for command in added {
                    if let Some(name) = ribbon::name_of(command) {
                        rows.push(TreeRow::under(2, name));
                    }
                }
            }
        }
        rows
    }

    /// Reads the tick boxes back onto the working copy.
    ///
    /// The ticks are the one thing these two tabs change inside the dialog
    /// rather than through a button, so they are the one thing that has to be
    /// read out of it before it is built again or thrown away.
    pub(super) fn read_customise_dialog(&mut self, dialog: &Dialog) {
        let mut tab = None;
        for row in dialog.tree_rows(RIBBON_TREE) {
            match row.depth {
                0 => tab = Tab::named(&row.text),
                1 => {
                    if let (Some(tab), Some(on)) = (tab, row.tick) {
                        self.editing_chrome.set_hidden(tab, &row.text, !on);
                    }
                }
                _ => {}
            }
        }
    }

    /// Add, Remove, the two arrows and Reset: none of them answers the dialog.
    pub(super) fn customise_button(&mut self, button: &str) -> Response {
        let Some(dialog) = self.dialog.clone() else { return Response::Ignored };
        self.read_customise_dialog(&dialog);

        if dialog.showing_tab() == QUICK_PAGE {
            self.quick_button(&dialog, button);
        } else {
            self.ribbon_button(&dialog, button);
        }

        let mut rebuilt = self.options_dialog();
        rebuilt.carry_answers_from(&dialog);
        self.dialog = Some(rebuilt);
        self.needs_redraw = true;
        Response::Redraw
    }

    /// One of them, on the toolbar's page.
    fn quick_button(&mut self, dialog: &Dialog, button: &str) {
        let chosen = self.quick_chosen(dialog);
        match button {
            ADD => {
                if let Some(command) = command_chosen(dialog, QUICK_ALL) {
                    self.editing_chrome.add_to_quick(command);
                }
            }
            REMOVE => {
                if let Some(command) = chosen {
                    self.editing_chrome.remove_from_quick(command);
                }
            }
            MOVE_UP | MOVE_DOWN => {
                if let Some(command) = chosen {
                    self.editing_chrome.move_quick(command, button == MOVE_UP);
                }
            }
            RESET => self.editing_chrome.quick = crate::chrome::customise::USUAL_QUICK.to_vec(),
            _ => {}
        }
    }

    /// And on the ribbon's.
    fn ribbon_button(&mut self, dialog: &Dialog, button: &str) {
        let Some((tab, group, command)) = self.ribbon_chosen(dialog) else {
            // Nothing is chosen but Reset, which needs nothing chosen.
            if button == RESET {
                self.reset_ribbon();
            }
            return;
        };
        match button {
            ADD => {
                // A command is added to a group, so a row that is a tab or a
                // command has the group it belongs to added to instead — which
                // is the group the chosen row is under.
                if let (Some(group), Some(adding)) = (group, command_chosen(dialog, RIBBON_ALL)) {
                    self.editing_chrome.add_to_group(tab, group, adding);
                }
            }
            REMOVE => {
                if let (Some(group), Some(command)) = (group, command) {
                    self.editing_chrome.remove_from_group(tab, group, command);
                }
            }
            MOVE_UP | MOVE_DOWN => {
                if let Some(group) = group {
                    self.editing_chrome.move_group(tab, group, button == MOVE_UP);
                }
            }
            RESET => self.reset_ribbon(),
            _ => {}
        }
    }

    /// Puts the ribbon back the way it comes, and leaves the toolbar alone.
    ///
    /// Word's Reset offers both separately; this is the one under the ribbon's
    /// page, and the toolbar has its own under the toolbar's.
    fn reset_ribbon(&mut self) {
        let usual = Customisation::default();
        self.editing_chrome.hidden = usual.hidden;
        self.editing_chrome.order = usual.order;
        self.editing_chrome.added = usual.added;
    }

    /// Which command the toolbar's own list has chosen.
    fn quick_chosen(&self, dialog: &Dialog) -> Option<Command> {
        let at = dialog.chose_row(QUICK_LIST);
        self.editing_chrome.quick.get(at).copied()
    }

    /// What the ribbon's tree has chosen: which tab it is under, which group,
    /// and which command if the row is one.
    fn ribbon_chosen(
        &self,
        dialog: &Dialog,
    ) -> Option<(Tab, Option<&'static str>, Option<Command>)> {
        let rows = dialog.tree_rows(RIBBON_TREE);
        let at = dialog.chose_row(RIBBON_TREE);
        rows.get(at)?;

        // Walked backwards from the chosen row: the group it is in is the last
        // group row above it, and the tab is the last tab row above that.
        let mut group = None;
        let mut tab = None;
        for row in rows[..=at].iter().rev() {
            if row.depth == 1 && group.is_none() {
                group = Some(row.text.clone());
            }
            if row.depth == 0 {
                tab = Tab::named(&row.text);
                break;
            }
        }
        let tab = tab?;

        // The labels are given back as the table's own, so that what is written
        // into the settings is the table's spelling and not the dialog's copy.
        let group = group.and_then(|name| {
            ribbon::groups_of(tab).iter().find(|found| found.label == name).map(|found| found.label)
        });
        let command =
            (rows[at].depth == 2).then(|| ribbon::command_named(&rows[at].text)).flatten();
        Some((tab, group, command))
    }
}

/// The tabs a person may change.
///
/// Every tab of the ribbon except File, which opens the backstage rather than a
/// page of buttons and so has no groups to move or switch off. The two table
/// tabs and the header one are included: they are shown only while a table or a
/// header is being worked on, but what is on them is as much a matter of taste
/// as anything on Home.
fn customisable_tabs() -> Vec<Tab> {
    Tab::ALL.iter().chain(Tab::CONTEXTUAL.iter()).copied().filter(|tab| *tab != Tab::File).collect()
}

/// Every command, as rows of a list.
fn every_command() -> Vec<TreeRow> {
    ribbon::all_commands().into_iter().map(|(_, name)| TreeRow::plain(name)).collect()
}

/// Which command one of the "all commands" lists has chosen.
fn command_chosen(dialog: &Dialog, field: usize) -> Option<Command> {
    let at = dialog.chose_row(field);
    ribbon::all_commands().get(at).map(|(command, _)| *command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Text")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// Opens Options on one of the two tabs.
    fn open(editor: &mut Editor, page: usize) {
        editor.open_options();
        if let Some(dialog) = &mut editor.dialog {
            dialog.show_tab(page);
        }
    }

    /// Puts the keyboard on a row of one of the lists.
    fn choose(editor: &mut Editor, field: usize, row: usize) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Tree { current, .. }) = dialog.fields.get_mut(field) {
                *current = row;
            }
        }
    }

    /// The row of the "all commands" list one command is on.
    fn row_of(command: Command) -> usize {
        ribbon::all_commands().iter().position(|(found, _)| *found == command).expect("a command")
    }

    /// Answers the dialog without letting it write to the settings file.
    fn accept(editor: &mut Editor) {
        let dialog = editor.dialog.take().expect("a dialog");
        editor.asking = None;
        editor.read_customise_dialog(&dialog);
        editor.ribbon.custom = editor.editing_chrome.clone();
    }

    #[test]
    fn a_command_added_to_the_toolbar_is_on_it() {
        let mut editor = editor();
        open(&mut editor, QUICK_PAGE);
        choose(&mut editor, QUICK_ALL, row_of(Command::Print));
        editor.customise_button(ADD);
        accept(&mut editor);

        assert!(editor.ribbon.custom.quick.contains(&Command::Print));
    }

    #[test]
    fn a_command_taken_off_the_toolbar_is_off_it() {
        let mut editor = editor();
        open(&mut editor, QUICK_PAGE);
        // The first row of the toolbar's own list, which is Save.
        choose(&mut editor, QUICK_LIST, 0);
        editor.customise_button(REMOVE);
        accept(&mut editor);

        assert!(!editor.ribbon.custom.quick.contains(&Command::Save));
    }

    #[test]
    fn the_toolbar_can_be_put_back_the_way_it_comes() {
        let mut editor = editor();
        open(&mut editor, QUICK_PAGE);
        choose(&mut editor, QUICK_ALL, row_of(Command::Print));
        editor.customise_button(ADD);
        editor.customise_button(RESET);
        accept(&mut editor);

        assert_eq!(editor.ribbon.custom.quick, crate::chrome::customise::USUAL_QUICK);
    }

    #[test]
    fn a_group_switched_off_leaves_the_ribbon() {
        let mut editor = editor();
        editor.ribbon.tab = Tab::Home;
        open(&mut editor, RIBBON_PAGE);

        // The tree opens with every tab folded shut, so Home's first group is
        // the row after Home itself.
        let rows = editor.dialog.as_ref().expect("a dialog").tree_rows(RIBBON_TREE);
        let at = rows.iter().position(|row| row.depth == 1).expect("a group");
        let label = rows[at].text.clone();
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Tree { rows, current, .. }) = dialog.fields.get_mut(RIBBON_TREE) {
                *current = at;
                rows[at].tick = Some(false);
            }
        }
        accept(&mut editor);

        assert!(editor.ribbon.custom.is_hidden(Tab::Home, &label));
        let shown: Vec<&str> = editor.ribbon.groups().iter().map(|group| group.label).collect();
        assert!(!shown.contains(&label.as_str()), "{label} is still on the ribbon");
    }

    #[test]
    fn a_group_moved_up_is_drawn_before_the_one_it_was_after() {
        let mut editor = editor();
        editor.ribbon.tab = Tab::Home;
        open(&mut editor, RIBBON_PAGE);

        let rows = editor.dialog.as_ref().expect("a dialog").tree_rows(RIBBON_TREE);
        let groups: Vec<usize> =
            rows.iter().enumerate().filter(|(_, row)| row.depth == 1).map(|(at, _)| at).collect();
        let second = rows[groups[1]].text.clone();
        choose(&mut editor, RIBBON_TREE, groups[1]);
        editor.customise_button(MOVE_UP);
        accept(&mut editor);

        let shown: Vec<&str> = editor.ribbon.groups().iter().map(|group| group.label).collect();
        assert_eq!(shown[0], second, "the group did not move to the front");
    }

    #[test]
    fn a_command_added_to_a_group_is_in_it_and_can_be_taken_back_out() {
        let mut editor = editor();
        editor.ribbon.tab = Tab::Home;
        open(&mut editor, RIBBON_PAGE);

        let rows = editor.dialog.as_ref().expect("a dialog").tree_rows(RIBBON_TREE);
        let at = rows.iter().position(|row| row.depth == 1).expect("a group");
        let label = rows[at].text.clone();
        choose(&mut editor, RIBBON_TREE, at);
        choose(&mut editor, RIBBON_ALL, row_of(Command::AddBookmark));
        editor.customise_button(ADD);

        assert_eq!(editor.editing_chrome.added_to(Tab::Home, &label), vec![Command::AddBookmark]);

        // The command is now a row of its own under the group, and Remove takes
        // it off again.
        let rows = editor.dialog.as_ref().expect("a dialog").tree_rows(RIBBON_TREE);
        let command_row = rows.iter().position(|row| row.depth == 2).expect("the command");
        choose(&mut editor, RIBBON_TREE, command_row);
        editor.customise_button(REMOVE);
        accept(&mut editor);

        assert!(editor.ribbon.custom.added_to(Tab::Home, &label).is_empty());
    }

    #[test]
    fn the_ribbon_can_be_put_back_the_way_it_comes() {
        let mut editor = editor();
        editor.ribbon.tab = Tab::Home;
        let before: Vec<&str> = editor.ribbon.groups().iter().map(|group| group.label).collect();

        open(&mut editor, RIBBON_PAGE);
        let rows = editor.dialog.as_ref().expect("a dialog").tree_rows(RIBBON_TREE);
        let groups: Vec<usize> =
            rows.iter().enumerate().filter(|(_, row)| row.depth == 1).map(|(at, _)| at).collect();
        choose(&mut editor, RIBBON_TREE, groups[1]);
        editor.customise_button(MOVE_UP);
        editor.customise_button(RESET);
        accept(&mut editor);

        let after: Vec<&str> = editor.ribbon.groups().iter().map(|group| group.label).collect();
        assert_eq!(after, before);
    }

    #[test]
    fn resetting_the_ribbon_leaves_the_toolbar_alone() {
        // Word's two Resets are separate, and one that took the other's changes
        // with it would be a surprise nobody could undo.
        let mut editor = editor();
        open(&mut editor, QUICK_PAGE);
        choose(&mut editor, QUICK_ALL, row_of(Command::Print));
        editor.customise_button(ADD);

        if let Some(dialog) = &mut editor.dialog {
            dialog.show_tab(RIBBON_PAGE);
        }
        editor.customise_button(RESET);
        accept(&mut editor);

        assert!(editor.ribbon.custom.quick.contains(&Command::Print));
    }
}

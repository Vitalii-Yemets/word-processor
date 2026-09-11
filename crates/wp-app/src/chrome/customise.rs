//! What a person changed about the ribbon and the Quick Access Toolbar.
//!
//! # Changes, not a copy
//!
//! Word writes the whole customised ribbon into a file: every tab, every group,
//! every button, whether it was touched or not. That has a cost nobody notices
//! until the next version — a group added to Word after the file was written
//! does not appear, because the file says what the ribbon is rather than what
//! was changed about it.
//!
//! So what is kept here is the changes: which groups were switched off, which
//! tabs were put in a different order, what was added and where, and what is on
//! the toolbar. [`ribbon::groups_of`] stays the starting point, which means a
//! group added to this program later turns up on the ribbon of somebody who
//! customised it a year ago.
//!
//! # Why the toolbar is the exception
//!
//! It is kept whole, because a person's toolbar *is* the whole of it: Word lets
//! Save be taken off, and a toolbar remembered as "the usual three, less Save"
//! would put Save back the first time the usual three changed.
//!
//! # What is named by label rather than by number
//!
//! A group. The order it sits in changes the moment anything is moved, so a
//! number would point at the wrong group as soon as it was written down. The
//! label does not move, and it is also what a person reads in the settings file.

use std::collections::{BTreeMap, BTreeSet};

use super::ribbon::{self, Item, Tab};
use super::Command;

/// The commands the toolbar carries when nobody has said otherwise.
///
/// Word's three, in Word's order.
pub const USUAL_QUICK: &[Command] = &[Command::Save, Command::Undo, Command::Redo];

/// A group of the ribbon as it is actually shown.
///
/// The table in [`super::ribbon`] is `&'static`, and has to stay that way: it
/// is the description of Word's ribbon and nothing a person does should edit
/// it. This is what comes out of putting one of its groups through the changes
/// below, and it is what the ribbon draws.
#[derive(Clone, Debug)]
pub struct Showing {
    pub label: &'static str,
    pub items: Vec<Item>,
    pub launcher: Option<Command>,
}

/// What a person changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Customisation {
    /// The commands on the Quick Access Toolbar, in order.
    pub quick: Vec<Command>,
    /// The groups that have been switched off, by tab and label.
    pub hidden: BTreeSet<(Tab, String)>,
    /// The tabs whose groups have been put in another order, as their labels in
    /// that order.
    ///
    /// A group the list does not name keeps its place after the ones that are
    /// named — which is what makes a group added by a later version appear at
    /// all, rather than vanishing because an old settings file never heard of
    /// it.
    pub order: BTreeMap<Tab, Vec<String>>,
    /// The commands added to a group, by tab and group label, in the order they
    /// were added.
    pub added: Vec<(Tab, String, Command)>,
}

impl Default for Customisation {
    /// The ribbon as this program draws it, and Word's three quick buttons.
    fn default() -> Self {
        Self {
            quick: USUAL_QUICK.to_vec(),
            hidden: BTreeSet::new(),
            order: BTreeMap::new(),
            added: Vec::new(),
        }
    }
}

impl Customisation {
    /// Whether nothing has been changed, so that nothing need be written down.
    #[must_use]
    pub fn is_usual(&self) -> bool {
        *self == Self::default()
    }

    /// The groups of a tab, in the order they are shown and without the ones
    /// switched off.
    ///
    /// The labels are what is ordered, not the groups: a label that names no
    /// group is passed over, which is how a settings file that mentions a group
    /// this version no longer has stays harmless.
    #[must_use]
    pub fn groups_shown(&self, tab: Tab) -> Vec<Showing> {
        let table = ribbon::groups_of(tab);
        let mut taken = vec![false; table.len()];
        let mut out = Vec::with_capacity(table.len());

        if let Some(order) = self.order.get(&tab) {
            for label in order {
                let Some(at) = table.iter().position(|group| group.label == label) else {
                    continue;
                };
                if taken[at] {
                    continue;
                }
                taken[at] = true;
                if let Some(shown) = self.show(tab, at) {
                    out.push(shown);
                }
            }
        }
        // Whatever the order did not name, in the order the table has it.
        for (at, done) in taken.iter().enumerate() {
            if *done {
                continue;
            }
            if let Some(shown) = self.show(tab, at) {
                out.push(shown);
            }
        }
        out
    }

    /// The labels of a tab's groups in the order they are shown, including the
    /// ones switched off.
    ///
    /// What the customising dialog lists: a group that has been hidden is still
    /// on that list, with its tick box empty, because a list that dropped it
    /// would leave no way of getting it back.
    #[must_use]
    pub fn group_order(&self, tab: Tab) -> Vec<&'static str> {
        let table = ribbon::groups_of(tab);
        let mut taken = vec![false; table.len()];
        let mut out = Vec::with_capacity(table.len());
        if let Some(order) = self.order.get(&tab) {
            for label in order {
                let Some(at) = table.iter().position(|group| group.label == label) else {
                    continue;
                };
                if !taken[at] {
                    taken[at] = true;
                    out.push(table[at].label);
                }
            }
        }
        for (at, group) in table.iter().enumerate() {
            if !taken[at] {
                out.push(group.label);
            }
        }
        out
    }

    /// One group of the table, as it is shown, or nothing if it is switched off.
    fn show(&self, tab: Tab, at: usize) -> Option<Showing> {
        let group = ribbon::groups_of(tab).get(at)?;
        if self.is_hidden(tab, group.label) {
            return None;
        }
        Some(self.showing(tab, group.label, group.items, group.launcher))
    }

    /// A group with whatever has been added to it on the end.
    ///
    /// Added as small buttons, which is what Word does: a command a person put
    /// there is one of a list, not the thing the group is about.
    fn showing(
        &self,
        tab: Tab,
        label: &'static str,
        items: &'static [Item],
        launcher: Option<Command>,
    ) -> Showing {
        let mut items = items.to_vec();
        for (_, _, command) in
            self.added.iter().filter(|(found, name, _)| *found == tab && name == label)
        {
            let Some(name) = ribbon::name_of(*command) else { continue };
            items.push(Item::Break);
            items.push(Item::Small(*command, ribbon::icon_of(*command), name));
        }
        Showing { label, items, launcher }
    }

    /// Whether a group is switched off.
    #[must_use]
    pub fn is_hidden(&self, tab: Tab, label: &str) -> bool {
        self.hidden.iter().any(|(found, name)| *found == tab && name == label)
    }

    /// Switches a group off, or back on.
    pub fn set_hidden(&mut self, tab: Tab, label: &str, hidden: bool) {
        let key = (tab, label.to_owned());
        if hidden {
            self.hidden.insert(key);
        } else {
            self.hidden.remove(&key);
        }
    }

    /// Moves a group one place along the tab it is on.
    ///
    /// Returns where it ended up, so the dialog can keep it chosen: a list that
    /// moved the group and left the keyboard behind would make moving one three
    /// places a game of catch.
    pub fn move_group(&mut self, tab: Tab, label: &str, up: bool) -> usize {
        let mut order: Vec<String> = self.group_order(tab).into_iter().map(str::to_owned).collect();
        let Some(at) = order.iter().position(|found| found == label) else { return 0 };
        let wanted = if up { at.checked_sub(1) } else { (at + 1 < order.len()).then_some(at + 1) };
        let Some(wanted) = wanted else { return at };
        order.swap(at, wanted);
        self.order.insert(tab, order);
        wanted
    }

    /// Adds a command to a group, unless it is already there.
    pub fn add_to_group(&mut self, tab: Tab, label: &str, command: Command) {
        let already = self
            .added
            .iter()
            .any(|(found, name, held)| *found == tab && name == label && *held == command);
        if already {
            return;
        }
        self.added.push((tab, label.to_owned(), command));
    }

    /// Takes one back off, if it was one that was added.
    ///
    /// A command that came with the group cannot be taken off it. Word allows
    /// that; here it would mean the table in [`super::ribbon`] no longer
    /// describing the ribbon, and there is nowhere yet to say "this button of
    /// Word's is not shown". It is named in the roadmap rather than half done.
    pub fn remove_from_group(&mut self, tab: Tab, label: &str, command: Command) -> bool {
        let before = self.added.len();
        self.added
            .retain(|(found, name, held)| !(*found == tab && name == label && *held == command));
        self.added.len() != before
    }

    /// The commands a person added to one group, in order.
    #[must_use]
    pub fn added_to(&self, tab: Tab, label: &str) -> Vec<Command> {
        self.added
            .iter()
            .filter(|(found, name, _)| *found == tab && name == label)
            .map(|(_, _, command)| *command)
            .collect()
    }

    // --- The Quick Access Toolbar -------------------------------------------

    /// Puts a command on the end of the toolbar, unless it is already on it.
    pub fn add_to_quick(&mut self, command: Command) -> usize {
        if let Some(at) = self.quick.iter().position(|found| *found == command) {
            return at;
        }
        self.quick.push(command);
        self.quick.len() - 1
    }

    /// Takes one off.
    pub fn remove_from_quick(&mut self, command: Command) {
        self.quick.retain(|found| *found != command);
    }

    /// Moves one along the toolbar, and says where it ended up.
    pub fn move_quick(&mut self, command: Command, up: bool) -> usize {
        let Some(at) = self.quick.iter().position(|found| *found == command) else { return 0 };
        let wanted =
            if up { at.checked_sub(1) } else { (at + 1 < self.quick.len()).then_some(at + 1) };
        let Some(wanted) = wanted else { return at };
        self.quick.swap(at, wanted);
        wanted
    }
}

// --- Reading and writing the settings file ---------------------------------

/// What the toolbar's line is called.
const QUICK_KEY: &str = "quick";
/// And the three the ribbon uses.
const HIDDEN_KEY: &str = "ribbon-hidden";
const ADDED_KEY: &str = "ribbon-added";
/// This one has the tab's name after it, because a tab is a line of its own.
const ORDER_PREFIX: &str = "ribbon-order.";

/// What separates a tab from a group, and a group from a command.
///
/// A slash, because no tab, group or command is called anything with one in it
/// — and because "Home/Clipboard/Bookmark" reads as a place.
const PART: char = '/';

impl Customisation {
    /// Reads one line of the settings file, and says whether it was one of
    /// these.
    ///
    /// A name that stands for nothing this version has is dropped rather than
    /// kept: it would be a group or a command that cannot be drawn, and the
    /// ribbon is built from the table either way.
    pub fn read_line(&mut self, key: &str, value: &str) -> bool {
        match key {
            QUICK_KEY => {
                self.quick = split(value).filter_map(ribbon::command_named).collect();
                true
            }
            HIDDEN_KEY => {
                self.hidden = split(value)
                    .filter_map(|entry| {
                        let (tab, label) = entry.split_once(PART)?;
                        Some((Tab::named(tab.trim())?, label.trim().to_owned()))
                    })
                    .collect();
                true
            }
            ADDED_KEY => {
                self.added = split(value)
                    .filter_map(|entry| {
                        let (tab, rest) = entry.split_once(PART)?;
                        let (label, command) = rest.rsplit_once(PART)?;
                        Some((
                            Tab::named(tab.trim())?,
                            label.trim().to_owned(),
                            ribbon::command_named(command.trim())?,
                        ))
                    })
                    .collect();
                true
            }
            _ => {
                let Some(name) = key.strip_prefix(ORDER_PREFIX) else { return false };
                let Some(tab) = Tab::named(name.trim()) else { return true };
                self.order.insert(tab, split(value).map(str::to_owned).collect());
                true
            }
        }
    }

    /// Writes the lines back, and nothing at all when nothing was changed.
    ///
    /// Nothing at all on purpose: a settings file that listed the whole ribbon
    /// every time it was saved would be a file nobody could read, and the point
    /// of writing changes rather than a copy is that there are usually none.
    pub fn write_lines(&self, out: &mut String) {
        if self.is_usual() {
            return;
        }

        let names: Vec<&str> =
            self.quick.iter().filter_map(|command| ribbon::name_of(*command)).collect();
        out.push_str(QUICK_KEY);
        out.push_str(" = ");
        out.push_str(&names.join(", "));
        out.push('\n');

        if !self.hidden.is_empty() {
            let entries: Vec<String> = self
                .hidden
                .iter()
                .map(|(tab, label)| format!("{}{PART}{label}", tab.label()))
                .collect();
            out.push_str(HIDDEN_KEY);
            out.push_str(" = ");
            out.push_str(&entries.join(", "));
            out.push('\n');
        }

        if !self.added.is_empty() {
            let entries: Vec<String> = self
                .added
                .iter()
                .filter_map(|(tab, label, command)| {
                    let name = ribbon::name_of(*command)?;
                    Some(format!("{}{PART}{label}{PART}{name}", tab.label()))
                })
                .collect();
            out.push_str(ADDED_KEY);
            out.push_str(" = ");
            out.push_str(&entries.join(", "));
            out.push('\n');
        }

        for (tab, order) in &self.order {
            out.push_str(ORDER_PREFIX);
            out.push_str(tab.label());
            out.push_str(" = ");
            out.push_str(&order.join(", "));
            out.push('\n');
        }
    }
}

/// The entries of a comma-separated line.
fn split(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|entry| !entry.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first tab with more than one group, which is every tab that matters
    /// here; Home is the one a person customises.
    const TAB: Tab = Tab::Home;

    fn labels(custom: &Customisation) -> Vec<&'static str> {
        custom.groups_shown(TAB).into_iter().map(|group| group.label).collect()
    }

    #[test]
    fn nothing_changed_is_the_ribbon_the_table_describes() {
        let custom = Customisation::default();
        let wanted: Vec<&str> = ribbon::groups_of(TAB).iter().map(|group| group.label).collect();
        assert_eq!(labels(&custom), wanted);
        assert!(custom.is_usual());
    }

    #[test]
    fn a_group_switched_off_is_not_shown_and_is_still_on_the_list() {
        let mut custom = Customisation::default();
        custom.set_hidden(TAB, "Clipboard", true);
        assert!(!labels(&custom).contains(&"Clipboard"));
        // Still on the list the dialog shows, or it could never come back.
        assert!(custom.group_order(TAB).contains(&"Clipboard"));

        custom.set_hidden(TAB, "Clipboard", false);
        assert!(labels(&custom).contains(&"Clipboard"));
    }

    #[test]
    fn a_group_moved_stays_moved_and_the_rest_keep_their_order() {
        let mut custom = Customisation::default();
        let before = labels(&custom);
        let second = before[1];
        let at = custom.move_group(TAB, second, true);
        assert_eq!(at, 0);

        let after = labels(&custom);
        assert_eq!(after[0], second);
        assert_eq!(after[1], before[0]);
        assert_eq!(after[2..], before[2..]);
    }

    #[test]
    fn a_group_at_the_end_cannot_be_moved_past_it() {
        let mut custom = Customisation::default();
        let before = labels(&custom);
        let last = *before.last().expect("a group");
        custom.move_group(TAB, last, false);
        assert_eq!(labels(&custom), before);
    }

    #[test]
    fn a_command_added_to_a_group_is_in_it() {
        let mut custom = Customisation::default();
        custom.add_to_group(TAB, "Clipboard", Command::AddBookmark);
        let group = custom
            .groups_shown(TAB)
            .into_iter()
            .find(|group| group.label == "Clipboard")
            .expect("the group");
        assert!(group
            .items
            .iter()
            .any(|item| matches!(item, Item::Small(Command::AddBookmark, ..))));

        // And twice adding it puts it there once.
        custom.add_to_group(TAB, "Clipboard", Command::AddBookmark);
        assert_eq!(custom.added_to(TAB, "Clipboard").len(), 1);

        assert!(custom.remove_from_group(TAB, "Clipboard", Command::AddBookmark));
        assert!(custom.added_to(TAB, "Clipboard").is_empty());
    }

    #[test]
    fn a_label_naming_no_group_is_passed_over() {
        // What an old settings file looks like after a group is renamed: the
        // order still mentions it, and the ribbon must come out whole anyway.
        let mut custom = Customisation::default();
        custom.order.insert(TAB, vec!["Nothing Of The Sort".to_owned(), "Editing".to_owned()]);
        let shown = labels(&custom);
        assert_eq!(shown[0], "Editing");
        assert_eq!(shown.len(), ribbon::groups_of(TAB).len());
    }

    #[test]
    fn the_toolbar_is_a_list_that_can_be_added_to_moved_and_emptied() {
        let mut custom = Customisation::default();
        assert_eq!(custom.quick, USUAL_QUICK);

        custom.add_to_quick(Command::Print);
        assert_eq!(custom.quick.last(), Some(&Command::Print));

        let at = custom.move_quick(Command::Print, true);
        assert_eq!(at, USUAL_QUICK.len() - 1);

        custom.remove_from_quick(Command::Save);
        assert!(!custom.quick.contains(&Command::Save), "Save could not be taken off");
    }

    /// Puts a customisation through the settings file and back.
    fn round_trip(custom: &Customisation) -> Customisation {
        let mut text = String::new();
        custom.write_lines(&mut text);

        let mut read = Customisation::default();
        for line in text.lines() {
            let (key, value) = line.split_once('=').expect("a setting");
            assert!(read.read_line(key.trim(), value.trim()), "{key} was not understood");
        }
        read
    }

    #[test]
    fn nothing_changed_is_written_as_nothing() {
        let mut text = String::new();
        Customisation::default().write_lines(&mut text);
        assert_eq!(text, "");
    }

    #[test]
    fn every_change_survives_being_written_and_read_back() {
        let mut custom = Customisation::default();
        custom.add_to_quick(Command::Print);
        custom.remove_from_quick(Command::Redo);
        custom.set_hidden(TAB, "Clipboard", true);
        custom.set_hidden(Tab::View, "Zoom", true);
        custom.add_to_group(TAB, "Font", Command::AddBookmark);
        custom.move_group(TAB, "Editing", true);

        assert_eq!(round_trip(&custom), custom);
    }

    #[test]
    fn a_name_this_version_does_not_know_is_dropped_rather_than_kept() {
        // What an older settings file looks like after a command is renamed.
        // The line has to be read without the rest of it being lost.
        let mut custom = Customisation::default();
        assert!(custom.read_line("quick", "Save, Nothing Of The Sort, Print"));
        assert_eq!(custom.quick, vec![Command::Save, Command::Print]);

        assert!(custom.read_line("ribbon-hidden", "Nowhere/Something, Home/Clipboard"));
        assert!(custom.is_hidden(TAB, "Clipboard"));
        assert_eq!(custom.hidden.len(), 1);
    }

    #[test]
    fn a_line_that_is_not_ours_is_left_to_somebody_else() {
        let mut custom = Customisation::default();
        assert!(!custom.read_line("dark", "yes"));
        assert!(custom.is_usual());
    }

    #[test]
    fn a_toolbar_emptied_altogether_stays_empty() {
        // The line is written even with nothing on it, because the whole list
        // is what is kept and an absent line means the usual three.
        let mut custom = Customisation::default();
        custom.quick.clear();
        assert!(!custom.is_usual());
        assert_eq!(round_trip(&custom).quick, Vec::<Command>::new());
    }

    #[test]
    fn no_name_the_settings_file_writes_contains_what_separates_them() {
        // The file is a comma-separated list of slash-separated parts, so a
        // comma or a slash in a name would make it unreadable.
        for (_, name) in ribbon::all_commands() {
            assert!(!name.contains(','), "the command {name} has a comma in it");
            assert!(!name.contains(PART), "the command {name} has a {PART} in it");
        }
        for tab in Tab::ALL.iter().chain(Tab::CONTEXTUAL.iter()) {
            assert!(!tab.label().contains(','), "the tab {} has a comma in it", tab.label());
            assert!(!tab.label().contains(PART), "the tab {} has a {PART} in it", tab.label());
            for group in ribbon::groups_of(*tab) {
                assert!(!group.label.contains(','), "the group {} has a comma in it", group.label);
                assert!(
                    !group.label.contains(PART),
                    "the group {} has a {PART} in it",
                    group.label
                );
            }
        }
    }
}

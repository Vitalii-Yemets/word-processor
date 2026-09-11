//! A group of the ribbon shown as one button, because the window is too narrow
//! for all of them.
//!
//! Pressing it lists what is inside, by name. Word draws the group itself in a
//! panel under the button; a list of what the buttons do is the same
//! information, and reaches the same commands.

use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

impl Editor {
    /// Drops open what is inside a group that has been given up to one button.
    pub(super) fn open_group(&mut self, index: usize) -> Response {
        if self.close_popup_if(Choice::Group) {
            return Response::Redraw;
        }
        let command = Command::ExpandGroup(index as u8);
        let Some((left, top, _)) = self.ribbon.command_rect(command) else {
            return Response::Ignored;
        };

        let inside = self.ribbon.group_commands(index);
        if inside.is_empty() {
            return Response::Ignored;
        }
        // A group holding one thing opens it rather than offering a list of
        // one, which is a list nobody needs to read.
        if inside.len() == 1 {
            return self.run(inside[0].0);
        }
        let items = inside.iter().map(|(_, label)| (*label).to_owned()).collect();
        self.group_commands = inside.iter().map(|(command, _)| Some(*command)).collect();

        self.popup = Some(Popup::new(Choice::Group, items, None, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Runs whichever of the group's commands was chosen.
    pub(super) fn choose_group_command(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(command) = self.group_commands.get(index).copied().flatten() else {
            return Response::Ignored;
        };
        self.run(command)
    }
}

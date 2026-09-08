//! Driving the ribbon from the keyboard: Alt, then a letter, then a letter.
//!
//! # The shape of it
//!
//! Alt on its own puts a letter over every tab. Pressing one of those letters
//! opens that tab and puts a letter over everything in it. Pressing one of
//! those runs the command and the letters go. Escape steps back out, Alt puts
//! them away, and so does a click on anything.
//!
//! That is Word's, and it is the reason somebody who knows the program can work
//! it without reaching for the mouse at all.
//!
//! # What is different
//!
//! Word also letters the buttons above the ribbon — save, undo, redo — and
//! their letters are digits. Those three have shortcuts of their own here
//! (Ctrl+S, Ctrl+Z, Ctrl+Y), so nothing is out of reach without them.

use wp_shell::Response;

use crate::chrome::ribbon::Tab;
use crate::chrome::{keytips, tip, Command};

use super::Editor;

/// How far the letters have been followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Level {
    /// A letter over each tab.
    Tabs,
    /// A letter over everything in the tab that is open.
    Commands,
}

impl Editor {
    /// Shows the letters, or puts them away if they are already showing.
    pub(super) fn toggle_key_tips(&mut self) -> Response {
        self.key_tips = match self.key_tips {
            Some(_) => None,
            None => Some(Level::Tabs),
        };
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Whether the letters are showing.
    #[must_use]
    pub(super) fn showing_key_tips(&self) -> bool {
        self.key_tips.is_some()
    }

    /// Takes them away. Returns whether any were showing.
    pub(super) fn hide_key_tips(&mut self) -> bool {
        if self.key_tips.take().is_some() {
            self.needs_redraw = true;
            return true;
        }
        false
    }

    /// Steps back out one level, the way Escape does.
    pub(super) fn leave_key_tips(&mut self) -> Response {
        self.key_tips = match self.key_tips {
            Some(Level::Commands) => Some(Level::Tabs),
            _ => None,
        };
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Follows one letter.
    pub(super) fn press_key_tip(&mut self, letter: char) -> Response {
        let wanted = letter.to_ascii_uppercase().to_string();
        match self.key_tips {
            Some(Level::Tabs) => {
                let Some((tab, ..)) =
                    self.tab_tips().into_iter().find(|(_, letters, ..)| *letters == wanted)
                else {
                    return Response::Ignored;
                };
                self.ribbon.tab = tab;
                self.key_tips = Some(Level::Commands);
                self.needs_redraw = true;
                Response::Redraw
            }
            Some(Level::Commands) => {
                let Some((command, ..)) =
                    self.command_tips().into_iter().find(|(_, letters, ..)| *letters == wanted)
                else {
                    return Response::Ignored;
                };
                self.key_tips = None;
                self.needs_redraw = true;
                self.run(command)
            }
            None => Response::Ignored,
        }
    }

    /// The tabs, their letters, and where each sits.
    #[must_use]
    fn tab_tips(&self) -> Vec<(Tab, String, f32, f32)> {
        // Word's own letters, not worked-out ones: see [`Tab::key_tip`].
        self.ribbon
            .tab_places()
            .into_iter()
            .map(|(tab, left, width)| (tab, tab.key_tip().to_owned(), left, width))
            .collect()
    }

    /// The commands of the open tab, their letters, and where each sits.
    #[must_use]
    fn command_tips(&self) -> Vec<(Command, String, f32, f32, f32, f32)> {
        let places = self.ribbon.command_places();
        let labels: Vec<&str> =
            places.iter().map(|(command, ..)| tip::label_of(*command).unwrap_or("?")).collect();
        let letters = keytips::assign(&labels);
        places
            .into_iter()
            .zip(letters)
            .map(|((command, left, top, width, height), letter)| {
                (command, letter, left, top, width, height)
            })
            .collect()
    }

    /// Draws whichever letters are showing.
    pub(super) fn draw_key_tips(&mut self) {
        let Some(level) = self.key_tips else { return };
        let theme = self.theme;

        // Gathered first, because drawing borrows the same engine the places
        // were worked out with.
        let badges: Vec<(String, f32, f32)> = match level {
            Level::Tabs => {
                let strip = self.ribbon.strip_top();
                self.tab_tips()
                    .into_iter()
                    .map(|(_, letter, left, width)| (letter, left + width / 2.0 - 8.0, strip + 8.0))
                    .collect()
            }
            Level::Commands => self
                .command_tips()
                .into_iter()
                .map(|(_, letter, left, top, width, height)| {
                    (letter, left + width / 2.0 - 8.0, top + height - 14.0)
                })
                .collect(),
        };

        for (letter, x, y) in badges {
            keytips::draw(
                &mut self.canvas,
                &mut self.chrome_engine,
                &mut self.renderer,
                &letter,
                x,
                y,
                &theme,
            );
        }
    }
}

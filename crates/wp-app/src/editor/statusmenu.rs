//! The menu the right button opens on the strip along the bottom.
//!
//! # What Word does
//!
//! Right-click the status bar and a list called "Customize Status Bar" drops
//! open, with a tick beside everything that is showing. Clicking one turns it
//! on or off — and the list stays open, so several can be changed in one go.
//! The choice is remembered between one run of the program and the next.
//!
//! All of that is here. What is not is the right-hand column Word puts in the
//! list, where each line also shows the value it would put on the strip.

use wp_shell::Response;

use crate::chrome::icons::Icon;
use crate::chrome::popup::{Kind, Row};
use crate::chrome::status;
use crate::chrome::{Choice, Popup};

use super::Editor;

/// How wide the list is drawn.
const WIDTH: f32 = 220.0;

impl Editor {
    /// Whether a point is on the strip along the bottom.
    #[must_use]
    pub(super) fn on_status_bar(&self, y: i32) -> bool {
        (y as f32) >= self.view_height as f32 - crate::chrome::STATUS_HEIGHT
    }

    /// Drops open what the strip can show.
    pub(super) fn open_status_menu(&mut self, x: i32, y: i32) -> Response {
        self.popup = None;
        self.palette = None;
        self.table_grid = None;

        let mut items = vec!["Customize Status Bar".to_owned()];
        let mut rows = vec![Row::new(Kind::Heading, Icon::None)];
        for (index, (name, _)) in status::PARTS.iter().enumerate() {
            items.push((*name).to_owned());
            // A tick beside the ones that are showing, which is the whole point
            // of the list.
            let icon = if self.status_shows.at(index) { Icon::Accept } else { Icon::None };
            rows.push(Row::new(Kind::Choice, icon));
        }

        self.status_menu_at = Some((x, y));
        self.popup = Some(
            Popup::new(Choice::StatusBar, items, None, x as f32, y as f32, WIDTH).with_rows(rows),
        );
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Turns one of them on or off, and leaves the list open.
    pub(super) fn choose_status_part(&mut self, index: usize) -> Response {
        // The heading is the first row, so the parts start at one.
        let Some(part) = index.checked_sub(1) else { return Response::Ignored };
        if part >= status::PARTS.len() {
            return Response::Ignored;
        }
        self.status_shows.toggle(part);
        self.settings.status_off = self.status_shows.switched_off();
        self.settings.save();

        // Opened again where it was, because Word's stays up: somebody turning
        // two things on should not have to right-click twice.
        let Some((x, y)) = self.status_menu_at else { return Response::Redraw };
        self.open_status_menu(x, y)
    }
}

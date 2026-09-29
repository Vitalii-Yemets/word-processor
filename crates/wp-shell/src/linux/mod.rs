//! The Linux shell, which is two shells and a choice between them.
//!
//! # Why two
//!
//! Because Linux has two window systems, and a program that spoke only one
//! would be a program half the desktops run through a compatibility layer.
//! X11 is what every older desktop is and what XWayland gives the newer
//! ones; Wayland is what the newer ones are. They have nothing in common
//! on the wire — one is a request-and-reply protocol to a server that owns
//! the screen, the other is a stream of messages to a compositor that owns
//! nothing the client can see — so each is written out in full: see
//! [`xshell`] and [`wayland`].
//!
//! # How the choice is made
//!
//! By asking. `WAYLAND_DISPLAY` names a compositor's socket, and if one
//! answers, that is the desktop the program is on; otherwise it is X.
//! Which means a Wayland desktop gets a Wayland window rather than an
//! XWayland one, and a desktop with neither gets the error the shell
//! already has for a machine with no display.
//!
//! What the desktop provides and neither shell draws itself — the file
//! dialogs, the questions — is asked of the desktop's own dialog program,
//! `zenity` or `kdialog`, whichever is installed; see [`dialogs`]. The
//! same goes for the documents the desktop remembers and which program
//! opens which kind of file; see [`files`].

mod cups;
mod dialogs;
mod files;
pub(crate) mod keys;
mod locale;
mod wayland;
pub(crate) mod x11;
mod xshell;

use std::cell::Cell;

use crate::clipboard::Contents;
use crate::{App, DragEffect, Error, WindowCommand, WindowOptions};

pub(crate) use dialogs::{
    ask_ok_cancel, ask_to_save, ask_yes_no, choose_file, show_error, show_message,
};
pub(crate) use files::{
    associate_kinds, choose_default_programs, install_folder, opens_kind, register_installed,
    remember_document, unregister_installed,
};
pub(crate) use locale::locale;

/// Which window system this program is talking to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Desktop {
    X11,
    Wayland,
}

thread_local! {
    /// Settled once, the first time anything asks, and not again: the
    /// window system does not change under a running program.
    static DESKTOP: Cell<Option<Desktop>> = const { Cell::new(None) };
}

fn desktop() -> Desktop {
    DESKTOP.with(|slot| {
        if let Some(known) = slot.get() {
            return known;
        }
        let chosen = if wayland::is_available() { Desktop::Wayland } else { Desktop::X11 };
        slot.set(Some(chosen));
        chosen
    })
}

/// Does the same thing on whichever desktop this is.
///
/// Written out rather than hidden behind a trait: the two shells are two
/// modules of functions, as the Windows one is, and a trait would mean an
/// object where there is nothing to hold.
macro_rules! on_the_desktop {
    ($($name:ident($($argument:ident: $kind:ty),*) $(-> $result:ty)?;)*) => {
        $(
            pub(crate) fn $name($($argument: $kind),*) $(-> $result)? {
                match desktop() {
                    Desktop::Wayland => wayland::$name($($argument),*),
                    Desktop::X11 => xshell::$name($($argument),*),
                }
            }
        )*
    };
}

on_the_desktop! {
    run(options: WindowOptions, app: Box<dyn App>) -> Result<(), Error>;
    open_window(title: &str) -> bool;
    window_count() -> usize;
    arrange_windows() -> usize;
    is_maximised() -> bool;
    window_command(command: WindowCommand);
    set_window_title(title: &str);
    double_click_millis() -> u32;
    caret_blink_millis() -> Option<u32>;
    system_code_pages() -> (u32, u32);
    place_composition(x: i32, y: i32, height: i32);
    selection_changed();
    set_frame_appearance(dark: bool, border: (u8, u8, u8), caption: (u8, u8, u8));
    start_drag(contents: &Contents) -> DragEffect;
    clipboard_set_contents(contents: &Contents) -> bool;
    clipboard_set_text(text: &str) -> bool;
    clipboard_text() -> Option<String>;
    clipboard_contents() -> Contents;
}

/// A window on the screen, as [`screen_windows`] lists them.
pub(crate) struct ScreenWindow {
    pub(crate) title: String,
    pub(crate) handle: usize,
}

/// A picture of the screen or a window: red, green, blue, alpha.
pub(crate) struct Shot {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) pixels: Vec<u8>,
}

pub(crate) fn screen_windows() -> Vec<ScreenWindow> {
    match desktop() {
        Desktop::Wayland => wayland::screen_windows()
            .into_iter()
            .map(|found| ScreenWindow { title: found.title, handle: found.handle })
            .collect(),
        Desktop::X11 => xshell::screen_windows()
            .into_iter()
            .map(|found| ScreenWindow { title: found.title, handle: found.handle })
            .collect(),
    }
}

pub(crate) fn capture_screen() -> Option<Shot> {
    match desktop() {
        Desktop::Wayland => wayland::capture_screen().map(|shot| Shot {
            width: shot.width,
            height: shot.height,
            pixels: shot.pixels,
        }),
        Desktop::X11 => xshell::capture_screen().map(|shot| Shot {
            width: shot.width,
            height: shot.height,
            pixels: shot.pixels,
        }),
    }
}

pub(crate) fn capture_window(handle: usize) -> Option<Shot> {
    match desktop() {
        Desktop::Wayland => wayland::capture_window(handle).map(|shot| Shot {
            width: shot.width,
            height: shot.height,
            pixels: shot.pixels,
        }),
        Desktop::X11 => xshell::capture_window(handle).map(|shot| Shot {
            width: shot.width,
            height: shot.height,
            pixels: shot.pixels,
        }),
    }
}

/// Hands an address to whichever program the desktop opens it with.
///
/// The same on either: `xdg-open` is the desktop's own, not the window
/// system's.
pub(crate) fn open_in_shell(address: &str) -> bool {
    xshell::open_in_shell(address)
}

// --- Printing ---------------------------------------------------------------
//
// Neither window system has anything to do with printing on Linux: that is
// CUPS, spoken to over its own socket. See [`cups`].

pub(crate) use cups::{
    choose_printer, default_printer_name, finish_document, open_printer_with, print_page,
    printer_names, printer_page, start_document, supports_both_sides,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_is_wayland_where_there_is_a_compositor_and_x_where_there_is_not() {
        // The choice is settled once per thread, so this test does the
        // asking itself rather than through `desktop`.
        let was = std::env::var("WAYLAND_DISPLAY").ok();
        std::env::remove_var("WAYLAND_DISPLAY");
        assert!(!wayland::is_available(), "no display names no compositor");
        std::env::set_var("WAYLAND_DISPLAY", "a-socket-that-is-not-there");
        assert!(!wayland::is_available(), "and a socket nothing answers on is not one either");
        match was {
            Some(value) => std::env::set_var("WAYLAND_DISPLAY", value),
            None => std::env::remove_var("WAYLAND_DISPLAY"),
        }
    }
}

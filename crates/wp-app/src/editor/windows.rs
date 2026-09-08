//! Two windows onto one document.
//!
//! # What a second window is
//!
//! Word's New Window opens another view of the document already open — not a
//! second copy. Typing in one shows up in the other, because there is one
//! document; but each window has its own place in it, its own magnification and
//! its own view mode, because a person opening a second window is doing so to
//! look at two places at once.
//!
//! # How it is done here
//!
//! The same way the split works, one level up. The editor holds one document
//! and one set of "where I am looking" fields, and a window is a saved copy of
//! those fields. The shell says which window it is about to ask about
//! ([`wp_shell::App::switch_window`]), the saved copy is swapped in, and every
//! other line of the program carries on as though there were one window.
//!
//! # What is shared and what is not
//!
//! Shared: the document, the caret, the selection, what is being tracked, the
//! recipients of a merge. Not shared: the scroll, the zoom, the view mode, the
//! split, the rulers and the pane.
//!
//! The caret is shared where Word gives each window its own. That is the same
//! simplification the split makes — see [`super::split`] — and for the same
//! reason: the caret lives in the document, and giving each view one of its own
//! is a change to every command that moves it.

use wp_layout::Page;
use wp_raster::Canvas;
use wp_shell::Response;

use super::views::{Movement, View};
use super::Editor;

/// Where one window is looking, while another window is being answered.
#[derive(Debug)]
pub(super) struct WindowState {
    canvas: Canvas,
    pages: Vec<Page>,
    view_width: usize,
    view_height: usize,
    scroll: f32,
    other_scroll: f32,
    split: Option<f32>,
    active_pane: usize,
    zoom: f32,
    view: View,
    movement: Movement,
    outline_depth: u8,
    show_rulers: bool,
    show_navigation: bool,
    /// Whether the document changed while another window was being answered,
    /// so this one's pages have to be worked out again before it is drawn.
    stale: bool,
}

impl Editor {
    /// Puts the state of the window being answered aside, and takes up
    /// another's.
    pub(super) fn use_window(&mut self, index: usize) {
        if index == self.active_window {
            return;
        }
        self.store_window();
        self.active_window = index;
        self.load_window();
    }

    /// Saves where this window is looking.
    fn store_window(&mut self) {
        let stored = WindowState {
            canvas: core::mem::replace(&mut self.canvas, Canvas::new(1, 1)),
            pages: core::mem::take(&mut self.pages),
            view_width: self.view_width,
            view_height: self.view_height,
            scroll: self.scroll,
            other_scroll: self.other_scroll,
            split: self.split,
            active_pane: self.active_pane,
            zoom: self.zoom,
            view: self.view,
            movement: self.movement,
            outline_depth: self.outline_depth,
            show_rulers: self.show_rulers,
            show_navigation: self.show_navigation,
            stale: false,
        };
        while self.window_states.len() <= self.active_window {
            self.window_states.push(None);
        }
        self.window_states[self.active_window] = Some(stored);
    }

    /// Takes up where the window now being answered was looking.
    fn load_window(&mut self) {
        let Some(Some(stored)) = self.window_states.get_mut(self.active_window).map(Option::take)
        else {
            // A window nobody has saved yet has just opened. It looks at the
            // same place as the one it was opened from — which is what the
            // fields still say — but needs a canvas and pages of its own.
            self.canvas = Canvas::new(1, 1);
            self.relayout();
            self.needs_redraw = true;
            return;
        };

        self.canvas = stored.canvas;
        self.pages = stored.pages;
        self.view_width = stored.view_width;
        self.view_height = stored.view_height;
        self.scroll = stored.scroll;
        self.other_scroll = stored.other_scroll;
        self.split = stored.split;
        self.active_pane = stored.active_pane;
        self.zoom = stored.zoom;
        self.view = stored.view;
        self.movement = stored.movement;
        self.outline_depth = stored.outline_depth;
        self.show_rulers = stored.show_rulers;
        self.show_navigation = stored.show_navigation;
        self.needs_redraw = true;

        // The document may have been changed from the other window while this
        // one was put away, and pages worked out before that are wrong.
        if stored.stale {
            self.relayout();
        }
    }

    /// Says that every window but this one is showing pages worked out before
    /// the document changed.
    ///
    /// Called whenever the document is laid out again, which is after every
    /// edit: the window doing the editing has just been brought up to date, and
    /// the others cannot be until they are looked at.
    pub(super) fn other_windows_are_stale(&mut self) {
        for (index, stored) in self.window_states.iter_mut().enumerate() {
            if index == self.active_window {
                continue;
            }
            if let Some(stored) = stored {
                stored.stale = true;
            }
        }
    }

    /// Opens another window onto this document.
    pub(super) fn new_window(&mut self) -> Response {
        // Nothing to set up: a window with nothing saved for it starts where
        // the one it was opened from is, which is what Word does and what makes
        // the second window useful straight away.
        let title = self.document_name();
        if !wp_shell::open_window(&format!("{title} — Word Processor")) {
            return self.report("Another window could not be opened");
        }
        self.report(&format!("{} windows", wp_shell::window_count().max(2)))
    }

    /// Lays the open windows out side by side.
    pub(super) fn arrange_all(&mut self) -> Response {
        let moved = wp_shell::arrange_windows();
        if moved < 2 {
            return self.report("Only one window is open — use New Window first");
        }
        self.report(&format!("{moved} windows arranged"))
    }

    /// Whether this is the only window left.
    ///
    /// Closing one of several is closing a view, and a view has nothing unsaved
    /// in it; closing the last one is closing the document, and that is when to
    /// ask.
    #[must_use]
    pub(super) fn is_last_window(&self) -> bool {
        wp_shell::window_count() <= 1
    }
}

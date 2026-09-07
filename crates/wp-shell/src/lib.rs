//! The platform shell: a window on the screen and the events it produces.
//!
//! Everything above this crate works in pixels and knows nothing about the
//! operating system. This is the only place that does, and it is deliberately
//! thin: a window, an event loop, and a way to get a finished image onto the
//! screen. Adding a second platform means writing one more file of the same
//! shape, not changing anything above.
//!
//! # Why this crate is allowed to use `unsafe`
//!
//! Every other crate in the project forbids it. Calling into an operating
//! system means calling C functions, and there is no other way to do that. The
//! unsafe surface is kept to the declarations themselves and the few calls that
//! use them, with a safe interface — [`App`] and [`Event`] — on top.
//!
//! No binding library is used. The declarations are written out here against
//! the documented ABI, which is what keeps the project free of dependencies.

#![cfg_attr(not(windows), allow(dead_code))]

use wp_raster::Canvas;

#[cfg(windows)]
mod windows;

/// A key the application reacts to.
///
/// Deliberately small: this is what the shell can report today, not a complete
/// keyboard model. Text input arrives as [`Event::Char`] instead, because the
/// operating system has already done the work of turning keystrokes into
/// characters — including for input methods, where one character can take many
/// keystrokes to compose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Backspace,
    Delete,
    Escape,
    Tab,
}

/// Something that happened to the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The drawing area changed size, in pixels.
    Resized { width: u32, height: u32 },
    /// The wheel turned. Positive scrolls towards the start of the document.
    Scroll { lines: f32 },
    KeyDown(Key),
    /// A character was typed, after the operating system composed it.
    Char(char),
    /// A mouse button went down at a point in the drawing area.
    MouseDown { x: i32, y: i32 },
    /// The window is closing.
    Closing,
}

/// What the shell should do after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    /// Nothing changed; leave the screen alone.
    Ignored,
    /// Redraw the window.
    Redraw,
    /// Close the window.
    Close,
}

/// An application the shell can show.
pub trait App {
    /// Reacts to an event.
    fn handle(&mut self, event: Event) -> Response;

    /// Draws the window contents at the given size.
    ///
    /// The canvas is borrowed rather than returned by value so that an
    /// application can keep one buffer and reuse it, instead of allocating a
    /// window-sized image on every repaint.
    fn draw(&mut self, width: usize, height: usize) -> &Canvas;
}

/// Why a window could not be shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The operating system refused to create the window.
    WindowCreationFailed(String),
    /// This platform has no shell implementation yet.
    UnsupportedPlatform,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::WindowCreationFailed(detail) => write!(f, "cannot create a window: {detail}"),
            Self::UnsupportedPlatform => {
                f.write_str("no window support on this platform yet; X11 and Wayland come later")
            }
        }
    }
}

impl std::error::Error for Error {}

/// How a window should first appear.
#[derive(Clone, Debug)]
pub struct WindowOptions {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self { title: "Word Processor".to_owned(), width: 1000, height: 800 }
    }
}

/// Opens a window and runs until it closes.
///
/// This does not return until the user closes the window, which is how every
/// desktop application works: the operating system owns the loop.
pub fn run(options: WindowOptions, app: Box<dyn App>) -> Result<(), Error> {
    #[cfg(windows)]
    {
        windows::run(options, app)
    }
    #[cfg(not(windows))]
    {
        let _ = (options, app);
        Err(Error::UnsupportedPlatform)
    }
}

/// Whether this build can open a window at all.
#[must_use]
pub fn is_supported() -> bool {
    cfg!(windows)
}

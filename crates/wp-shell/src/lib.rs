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
    /// A letter key, reported in lower case. Only used for shortcuts; ordinary
    /// typing arrives as [`Event::Char`], already composed by the system.
    Letter(char),
    /// A digit key from the number row, for the shortcuts that use one.
    Digit(char),
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
    /// The space bar, which is only reported as a key when a modifier is held.
    ///
    /// An ordinary space arrives as [`Event::Char`] like any other typing;
    /// this is here for the shortcuts that use it — Word's non-breaking space
    /// is Ctrl+Shift+Space, and there is no other way to reach it.
    Space,
}

/// Which modifier keys were held down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Something that happened to the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The drawing area changed size, in pixels.
    Resized {
        width: u32,
        height: u32,
    },
    /// The wheel turned. Positive scrolls towards the start of the document.
    Scroll {
        lines: f32,
        /// What was held while the wheel turned: Ctrl zooms and Shift scrolls
        /// sideways, in this program as in every other.
        modifiers: Modifiers,
    },
    KeyDown {
        key: Key,
        modifiers: Modifiers,
    },
    /// A character was typed, after the operating system composed it.
    Char(char),
    /// A mouse button went down at a point in the drawing area.
    MouseDown {
        x: i32,
        y: i32,
        modifiers: Modifiers,
    },
    /// The pointer moved. `held` is whether the left button is down, which is
    /// what tells a drag apart from an idle wander across the window.
    MouseMove {
        x: i32,
        y: i32,
        held: bool,
        modifiers: Modifiers,
    },
    /// The right button came back up: the place to open a context menu.
    RightClick {
        x: i32,
        y: i32,
        modifiers: Modifiers,
    },
    /// The left button came back up.
    MouseUp {
        x: i32,
        y: i32,
    },
    /// The middle button went down, which starts and stops the scroll that
    /// follows the pointer.
    MiddleClick {
        x: i32,
        y: i32,
    },
    /// The left button was pressed twice in quick succession.
    ///
    /// What counts as quick is the system's business, because it is a setting
    /// the user owns and every other program obeys it.
    DoubleClick {
        x: i32,
        y: i32,
    },
    /// Alt was pressed and let go without anything else being pressed, which
    /// is what asks the ribbon to show the letters that reach its commands.
    MenuKey,
    /// Control was pressed and let go without anything else being pressed.
    ///
    /// Word gives this one gesture: it opens the paste options after a paste,
    /// which is why its little button says "(Ctrl)". It means nothing at any
    /// other time, and a Control held as part of a shortcut is not this.
    ControlKey,
    /// The pointer left the window, so nothing in it is under the pointer any
    /// more. Windows only says so when asked, and it is asked.
    PointerLeft,
    /// The clock ticked. Sent often, and only worth anything to whatever is
    /// measuring time: the blinking caret, and how long the pointer has
    /// rested on a button.
    Tick,
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
    /// Do not carry out what the event was about.
    ///
    /// Only means anything for [`Event::Closing`], where it keeps the window
    /// open — which is what a program has to be able to do when the user
    /// answers "cancel" to being asked about unsaved changes.
    Refuse,
    /// Close the window.
    Close,
}

/// What to do with the window itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    Minimise,
    /// Maximises the window, or puts it back if it already is.
    ToggleMaximise,
    Close,
}

/// Carries out a window command.
pub fn window_command(command: WindowCommand) {
    #[cfg(windows)]
    {
        windows::window_command(command);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// How long the system counts two clicks as one double click, in milliseconds.
///
/// Asked for rather than assumed: it is a setting the person owns, and a third
/// click is only a triple click if it lands inside the same span.
#[must_use]
pub fn double_click_millis() -> u32 {
    #[cfg(windows)]
    {
        windows::double_click_millis()
    }
    #[cfg(not(windows))]
    {
        500
    }
}

/// How long the caret rests on each side of a blink, in milliseconds.
///
/// `None` where the person has asked for a caret that does not blink, which
/// Windows says by returning its "infinite" value — a setting somebody has
/// chosen and a program has no business overriding.
#[must_use]
pub fn caret_blink_millis() -> Option<u32> {
    #[cfg(windows)]
    {
        windows::caret_blink_millis()
    }
    #[cfg(not(windows))]
    {
        Some(530)
    }
}
/// Opens another window onto the same document.
///
/// The application is told which window it is answering for before each event,
/// so one program can show a document in two windows at once — which is what
/// Word's New Window does. Returns whether the window opened.
pub fn open_window(title: &str) -> bool {
    #[cfg(windows)]
    {
        windows::open_window(title)
    }
    #[cfg(not(windows))]
    {
        let _ = title;
        false
    }
}

/// How many windows the program has open.
#[must_use]
pub fn window_count() -> usize {
    #[cfg(windows)]
    {
        windows::window_count()
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Lays the open windows out side by side, filling the screen.
///
/// Word's Arrange All. Returns how many were moved.
pub fn arrange_windows() -> usize {
    #[cfg(windows)]
    {
        windows::arrange_windows()
    }
    #[cfg(not(windows))]
    {
        0
    }
}
/// Whether the window fills the screen, so its button can show which it is.
#[must_use]
pub fn is_maximised() -> bool {
    #[cfg(windows)]
    {
        windows::is_maximised()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// An application the shell can show.
pub trait App {
    /// Reacts to an event.
    fn handle(&mut self, event: Event) -> Response;

    /// Which pointer to show over a point in the window.
    ///
    /// A window that draws its own furniture has to say this for itself: the
    /// system knows only that the whole window is one class, and a class has
    /// one cursor. Without this the I-beam that belongs over the text follows
    /// the pointer onto the ribbon, the rulers and the buttons, where it is
    /// simply wrong.
    fn cursor(&mut self, x: i32, y: i32) -> Cursor {
        let _ = (x, y);
        Cursor::Arrow
    }

    /// Whether a point is on the part of the window that drags it.
    ///
    /// A window that draws its own title bar has to say where that title bar
    /// is, and which parts of it are buttons rather than empty space. The
    /// default is that nothing drags, which is right for an application that
    /// draws no caption of its own.
    fn is_caption(&mut self, x: i32, y: i32) -> bool {
        let _ = (x, y);
        false
    }

    /// Says which window the application is about to answer for.
    ///
    /// Called before every event and every draw. An application with one window
    /// can ignore it; one with several — a second view of the same document —
    /// uses it to put that window's scroll position, zoom and view mode back
    /// before it is asked anything.
    fn switch_window(&mut self, index: usize) {
        let _ = index;
    }

    /// Draws the window contents at the given size.
    ///
    /// The canvas is borrowed rather than returned by value so that an
    /// application can keep one buffer and reuse it, instead of allocating a
    /// window-sized image on every repaint.
    fn draw(&mut self, width: usize, height: usize) -> &Canvas;
}

/// The pointer shapes a window can ask for.
///
/// The four the system provides that a word processor actually needs: the
/// arrow over everything that is pressed, the I-beam over everything that is
/// text, and the two resize bars for the edges a pane is dragged by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Text,
    ResizeHorizontal,
    ResizeVertical,
    Hand,
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

/// Changes the title in the window's caption bar.
///
/// Called when the document being edited changes, so the caption says which
/// file is open — which is where a person looks to find out.
pub fn set_title(title: &str) {
    #[cfg(windows)]
    {
        windows::set_window_title(title);
    }
    #[cfg(not(windows))]
    {
        let _ = title;
    }
}

/// The dialogs the operating system provides.
///
/// Opening and saving go through the system's own dialogs rather than anything
/// drawn here. They are what the user already knows: the same places, the same
/// recent folders, the same keyboard habits as every other program on the
/// machine.
pub mod dialog {
    use std::path::{Path, PathBuf};

    /// One entry in a file dialog's type list.
    #[derive(Clone, Copy, Debug)]
    pub struct FileFilter {
        pub label: &'static str,
        /// A pattern such as `*.docx`, or several separated by semicolons.
        pub pattern: &'static str,
    }

    /// What the user answered when asked about unsaved changes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Answer {
        Yes,
        No,
        Cancel,
    }

    /// Asks for a file to open. `None` means the user cancelled.
    #[must_use]
    pub fn open_file(title: &str, filters: &[FileFilter]) -> Option<PathBuf> {
        #[cfg(windows)]
        {
            crate::windows::choose_file(title, filters, None, false)
        }
        #[cfg(not(windows))]
        {
            let _ = (title, filters);
            None
        }
    }

    /// Asks where to save a file, starting from a suggested name.
    #[must_use]
    pub fn save_file(
        title: &str,
        filters: &[FileFilter],
        suggested: Option<&Path>,
    ) -> Option<PathBuf> {
        #[cfg(windows)]
        {
            crate::windows::choose_file(title, filters, suggested, true)
        }
        #[cfg(not(windows))]
        {
            let _ = (title, filters, suggested);
            None
        }
    }

    /// Asks whether to save changes before throwing them away.
    #[must_use]
    pub fn ask_to_save(name: &str) -> Answer {
        #[cfg(windows)]
        {
            crate::windows::ask_to_save(name)
        }
        #[cfg(not(windows))]
        {
            let _ = name;
            // Without a way to ask, the safe answer is to do nothing rather
            // than to discard the user's work.
            Answer::Cancel
        }
    }

    /// Tells the user something went wrong.
    pub fn show_error(message: &str) {
        #[cfg(windows)]
        {
            crate::windows::show_error(message);
        }
        #[cfg(not(windows))]
        {
            eprintln!("error: {message}");
        }
    }
}

/// Putting a document on paper.
///
/// # Why a page is sent as pixels
///
/// A printer driver takes drawing commands, and a word processor could send it
/// text to draw — but then the printer's own text engine would decide where the
/// letters went, and the page would not be the page on screen. Everything here
/// is laid out and rasterized by this program at the printer's own resolution,
/// so what comes out is what was shown.
pub mod printing {
    use wp_raster::Canvas;

    /// The paper a printer is set up for, in its own dots.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct PageSetup {
        /// The part of the sheet the printer can draw in.
        pub width: usize,
        pub height: usize,
        /// The whole sheet, which is larger by the band the printer holds it in.
        pub paper_width: usize,
        pub paper_height: usize,
        /// Where the printable part begins on the sheet. The printer's own
        /// origin is that corner, so a page drawn as though it were the corner
        /// of the paper comes out shifted by this much.
        pub offset_x: usize,
        pub offset_y: usize,
        pub dpi_x: f32,
        pub dpi_y: f32,
    }

    impl PageSetup {
        /// The band of paper the printer cannot draw in, in its own dots, as
        /// left, top, right and bottom.
        #[must_use]
        pub fn unprintable(&self) -> (f32, f32, f32, f32) {
            let right = self.paper_width.saturating_sub(self.width + self.offset_x);
            let bottom = self.paper_height.saturating_sub(self.height + self.offset_y);
            (self.offset_x as f32, self.offset_y as f32, right as f32, bottom as f32)
        }
    }

    /// A printer, open and ready to be sent pages.
    ///
    /// Dropping one releases it, whether or not the job was finished, because a
    /// printer left open is a queue entry nobody can clear.
    #[derive(Debug)]
    pub struct Printer {
        /// The device context, held as an integer so that this type stays
        /// ordinary and safe outside the platform module.
        device_context: usize,
        started: bool,
        finished: bool,
    }

    impl Printer {
        #[cfg(windows)]
        pub(crate) fn from_device_context(device_context: usize) -> Self {
            Self { device_context, started: false, finished: false }
        }

        /// The paper this printer is set up for.
        #[must_use]
        pub fn page(&self) -> PageSetup {
            #[cfg(windows)]
            {
                crate::windows::printer_page(self.device_context)
            }
            #[cfg(not(windows))]
            {
                PageSetup {
                    width: 1,
                    height: 1,
                    paper_width: 1,
                    paper_height: 1,
                    offset_x: 0,
                    offset_y: 0,
                    dpi_x: 96.0,
                    dpi_y: 96.0,
                }
            }
        }

        /// Begins a job. Everything sent afterwards belongs to it.
        pub fn start(&mut self, name: &str) -> bool {
            #[cfg(windows)]
            {
                self.started = crate::windows::start_document(self.device_context, name);
                self.started
            }
            #[cfg(not(windows))]
            {
                let _ = name;
                false
            }
        }

        /// Sends one page.
        ///
        /// The page is asked for a band at a time rather than as one image: at
        /// a printer's own resolution a whole page runs to well over a hundred
        /// megabytes, and there is no reason to hold one.
        pub fn print_page(
            &mut self,
            width: usize,
            height: usize,
            band: impl FnMut(usize, usize) -> Canvas,
        ) -> bool {
            #[cfg(windows)]
            {
                let mut band = band;
                crate::windows::print_page(self.device_context, width, height, |top, rows| {
                    band(top, rows).to_bgra()
                })
            }
            #[cfg(not(windows))]
            {
                let _ = (width, height, band);
                false
            }
        }

        /// Ends the job, sending it to the queue.
        pub fn finish(&mut self) {
            self.close(true);
        }

        /// Throws the job away instead.
        pub fn cancel(&mut self) {
            self.close(false);
        }

        fn close(&mut self, keep: bool) {
            if self.finished {
                return;
            }
            self.finished = true;
            #[cfg(windows)]
            {
                crate::windows::finish_document(self.device_context, keep && self.started);
            }
            #[cfg(not(windows))]
            {
                let _ = keep;
            }
        }
    }

    impl Drop for Printer {
        fn drop(&mut self) {
            // A printer released only on the happy path is one left open on
            // every other, which is a job stuck in the queue.
            self.close(false);
        }
    }

    /// Every printer this machine can reach, by name.
    ///
    /// Empty where there are none, and on a system with no spooler at all.
    #[must_use]
    pub fn names() -> Vec<String> {
        #[cfg(windows)]
        {
            crate::windows::printer_names()
        }
        #[cfg(not(windows))]
        {
            Vec::new()
        }
    }

    /// The one a document goes to when nobody has said otherwise.
    #[must_use]
    pub fn default_name() -> Option<String> {
        #[cfg(windows)]
        {
            crate::windows::default_printer_name()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    /// Whether a printer can print on both sides of the sheet.
    ///
    /// Word greys the setting out when it cannot, rather than offering
    /// something that will not happen.
    #[must_use]
    pub fn prints_both_sides(name: &str) -> bool {
        #[cfg(windows)]
        {
            crate::windows::supports_both_sides(name)
        }
        #[cfg(not(windows))]
        {
            let _ = name;
            false
        }
    }

    /// Opens a printer by name, ready to be sent pages.
    ///
    /// `both_sides` is `None` for one side, `Some(true)` for a sheet turned
    /// over its long edge — the way a book turns — and `Some(false)` for its
    /// short edge.
    #[must_use]
    pub fn open(name: &str, both_sides: Option<bool>) -> Option<Printer> {
        #[cfg(windows)]
        {
            crate::windows::open_printer_with(name, both_sides)
        }
        #[cfg(not(windows))]
        {
            let _ = (name, both_sides);
            None
        }
    }

    /// Asks which printer to use through the system's own dialog. `None` means
    /// the user cancelled.
    #[must_use]
    pub fn choose() -> Option<Printer> {
        #[cfg(windows)]
        {
            crate::windows::choose_printer()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

/// The system clipboard.
///
/// Cut, copy and paste are how text moves between this program and every other
/// one, so the clipboard belongs in the shell alongside the window: it is the
/// other half of talking to the desktop. Only plain text is carried for now,
/// which is the format every application understands.
pub mod clipboard {
    /// Puts text on the clipboard, replacing what was there.
    ///
    /// Returns whether the system accepted it. Failure is normal rather than
    /// exceptional — another program can hold the clipboard open — so the
    /// caller is told and carries on.
    pub fn set_text(text: &str) -> bool {
        #[cfg(windows)]
        {
            crate::windows::clipboard_set_text(text)
        }
        #[cfg(not(windows))]
        {
            let _ = text;
            false
        }
    }

    /// Reads text from the clipboard, if it holds any.
    #[must_use]
    pub fn text() -> Option<String> {
        #[cfg(windows)]
        {
            crate::windows::clipboard_text()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

/// Taking a picture of what is on the screen.
///
/// Word's Screenshot button offers the windows that are open and drops a
/// picture of the chosen one into the document. Everything below is what that
/// needs: which windows there are, and how to photograph one of them or the
/// whole screen.
pub mod screen {
    /// A window a picture could be taken of.
    #[derive(Clone, Debug)]
    pub struct Window {
        /// What its title bar says, which is how a person recognises it.
        pub title: String,
        /// The system's own name for it. Opaque: pass it back and nothing else.
        pub handle: usize,
    }

    /// A picture, as pixels in red, green, blue, alpha order.
    #[derive(Clone, Debug)]
    pub struct Shot {
        pub width: usize,
        pub height: usize,
        pub pixels: Vec<u8>,
    }

    /// The windows that are open and visible, topmost first.
    ///
    /// Empty where there is no window system to ask, which is the same answer
    /// as "none are open" and needs no special case at the other end.
    #[must_use]
    pub fn windows() -> Vec<Window> {
        #[cfg(windows)]
        {
            crate::windows::screen_windows()
                .into_iter()
                .map(|found| Window { title: found.title, handle: found.handle })
                .collect()
        }
        #[cfg(not(windows))]
        {
            Vec::new()
        }
    }

    /// Photographs every monitor, side by side as the desktop arranges them.
    #[must_use]
    pub fn capture_screen() -> Option<Shot> {
        #[cfg(windows)]
        {
            crate::windows::capture_screen().map(|shot| Shot {
                width: shot.width,
                height: shot.height,
                pixels: shot.pixels,
            })
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    /// Photographs one window, whatever happens to be in front of it.
    #[must_use]
    pub fn capture_window(handle: usize) -> Option<Shot> {
        #[cfg(windows)]
        {
            crate::windows::capture_window(handle).map(|shot| Shot {
                width: shot.width,
                height: shot.height,
                pixels: shot.pixels,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = handle;
            None
        }
    }
}
/// Handing something over to whatever program deals with it.
///
/// A word processor is not a web browser and should not try to be one: a link
/// to a page is given to the desktop, which knows what the person has chosen to
/// open pages with.
pub mod desktop {
    /// Opens an address with whatever program handles it.
    ///
    /// Returns whether the desktop took it. A refusal is not an error worth
    /// stopping for — the address is still in the document, and the person can
    /// still copy it.
    pub fn open(address: &str) -> bool {
        // Anything that is not an address is not handed over. A file path
        // dressed up as a link is exactly how a document talks a program into
        // running something, and this is where that would happen.
        if !is_safe_to_open(address) {
            return false;
        }

        #[cfg(windows)]
        {
            crate::windows::open_in_shell(address)
        }
        #[cfg(not(windows))]
        {
            std::process::Command::new("xdg-open")
                .arg(address)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .is_ok()
        }
    }

    /// Whether an address is one of the kinds worth handing to the desktop.
    ///
    /// Web pages and e-mail, and nothing else. A document can say anything it
    /// likes in a link; it does not get to say `file:` or a scheme of its own
    /// invention and have this program run it.
    #[must_use]
    pub fn is_safe_to_open(address: &str) -> bool {
        let lowered = address.trim().to_lowercase();
        if lowered.contains(['\r', '\n', '\0']) {
            return false;
        }
        ["http://", "https://", "mailto:"].iter().any(|scheme| lowered.starts_with(scheme))
    }

    #[cfg(test)]
    mod tests {
        use super::is_safe_to_open;

        #[test]
        fn pages_and_mail_are_handed_over() {
            assert!(is_safe_to_open("https://example.org"));
            assert!(is_safe_to_open("http://example.org/a?b=c"));
            assert!(is_safe_to_open("mailto:someone@example.org"));
        }

        #[test]
        fn anything_that_could_run_something_is_not() {
            assert!(!is_safe_to_open("file:///c:/windows/system32/cmd.exe"));
            assert!(!is_safe_to_open(r"C:\Windows\System32\cmd.exe"));
            assert!(!is_safe_to_open("javascript:alert(1)"));
            assert!(!is_safe_to_open("ms-msdt:/id"));
            assert!(!is_safe_to_open(""));
        }

        #[test]
        fn a_line_break_cannot_be_smuggled_in() {
            assert!(!is_safe_to_open("https://example.org\r\nsomething else"));
        }
    }
}

/// Tells the desktop what the window looks like, so that the parts it draws
/// itself match the parts this program draws.
///
/// On Windows 11 the rounded corners, the line round the window and the caption
/// that shows while it is dragged all belong to the desktop compositor. It
/// paints them in the system's colours until it is told the window's own, which
/// is why a dark window can end up with a pale border — and only when it is not
/// maximised, because a maximised window has no border.
pub fn set_frame_appearance(dark: bool, border: (u8, u8, u8), caption: (u8, u8, u8)) {
    #[cfg(windows)]
    {
        windows::set_frame_appearance(dark, border, caption);
    }
    #[cfg(not(windows))]
    {
        let _ = (dark, border, caption);
    }
}

#[cfg(test)]
mod printing_tests {
    use super::printing::PageSetup;

    /// A printer that grips a quarter of an inch of the sheet at 600 dots to
    /// the inch: 150 dots at the left and top, and what is left over at the
    /// right and bottom.
    fn office_printer() -> PageSetup {
        PageSetup {
            width: 4660,
            height: 6715,
            paper_width: 4960,
            paper_height: 7015,
            offset_x: 150,
            offset_y: 150,
            dpi_x: 600.0,
            dpi_y: 600.0,
        }
    }

    #[test]
    fn the_band_a_printer_cannot_reach_is_worked_out_from_what_it_reports() {
        let (left, top, right, bottom) = office_printer().unprintable();
        assert_eq!((left, top), (150.0, 150.0));
        assert_eq!((right, bottom), (150.0, 150.0));
    }

    #[test]
    fn a_printer_that_reaches_the_whole_sheet_says_so() {
        let all_of_it = PageSetup {
            width: 4960,
            height: 7015,
            paper_width: 4960,
            paper_height: 7015,
            offset_x: 0,
            offset_y: 0,
            dpi_x: 600.0,
            dpi_y: 600.0,
        };
        assert_eq!(all_of_it.unprintable(), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn a_printer_that_reports_nonsense_does_not_report_a_negative_band() {
        // A driver saying the printable area is larger than the paper is not
        // impossible, and subtracting it must not wrap round.
        let confused = PageSetup { paper_width: 100, paper_height: 100, ..office_printer() };
        let (_, _, right, bottom) = confused.unprintable();
        assert_eq!((right, bottom), (0.0, 0.0));
    }
}

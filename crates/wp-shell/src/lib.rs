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

#![cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]

use wp_raster::Canvas;

// The certificates the machine trusts, which both systems keep somewhere of
// their own.
pub mod certificates;

// Word's own clipboard format, which is a Windows one; built on every
// machine for its tests.
#[cfg(windows)]
mod com;
#[cfg(windows)]
mod dragdrop;
#[cfg(any(windows, test))]
mod embedded;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod uia;
#[cfg(windows)]
mod windows;

/// The module that speaks to the desktop this build runs on. Each offers the
/// same functions under the same names, so the rest of this file asks one
/// place and does not care which.
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(windows)]
use windows as platform;

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
    /// A function key, by its number: F7 is `Function(7)`. Word puts the
    /// spelling check on F7 and the thesaurus on Shift+F7, and a person who
    /// has used Word reaches for them.
    Function(u8),
}

/// Which modifier keys were held down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Something that happened to the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The drawing area changed size, in the application's pixels.
    Resized {
        width: u32,
        height: u32,
    },
    /// How many of the screen's pixels one of the application's is, now:
    /// the window came up on, or moved to, a screen of a given density.
    /// Everything the application draws is scaled by it — see
    /// [`wp_raster::Canvas::set_scale`] — and every coordinate it is given
    /// or gives is in its own pixels. Sent before the first size, and
    /// again whenever the density changes.
    ScaleChanged {
        scale: f32,
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
    /// Text is being composed by an input method — Chinese, Japanese,
    /// Korean — and is not yet in the document: what has been composed so
    /// far, where the caret is in it (in characters), and how each
    /// character stands. Sent again at every change; empty when the
    /// composition is cleared.
    Compose {
        text: String,
        caret: usize,
        attributes: Vec<CompositionAttribute>,
    },
    /// The input method finished a piece of text: it goes into the
    /// document as if typed, and whatever was being composed is over.
    Commit(String),
    /// The composition ended, with whatever was still uncommitted dropped.
    ComposeEnd,
    /// Files from the desktop were dropped on the window, at a point in the
    /// drawing area.
    FilesDropped {
        paths: Vec<std::path::PathBuf>,
        x: i32,
        y: i32,
    },
    /// Something another program is dragging is over the window, at a
    /// point in the drawing area: where it would land if let go.
    DataDragOver {
        x: i32,
        y: i32,
    },
    /// What was being dragged over the window has left it.
    DataDragLeft,
    /// Another program's drag was let go on the window: what it carried, in
    /// every format this program takes, where, and whether Control was held
    /// to ask for a copy rather than a move.
    DataDropped {
        contents: clipboard::Contents,
        x: i32,
        y: i32,
        copying: bool,
    },
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

/// How a character of a composition stands, which is how it is shown:
/// still being typed, converted by the input method, the clause the
/// person is choosing a conversion for, or wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositionAttribute {
    Input,
    Converted,
    Target,
    Error,
}

/// Tells the screen reader that the document's selection moved, so that it
/// reads what the caret is on now. Called whenever the selection changes.
pub fn selection_changed() {
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::selection_changed();
    }
}

/// How a drag this program gave ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragEffect {
    /// Nowhere, or nowhere that took it.
    None,
    /// Another program took a copy: the original stays.
    Copy,
    /// Another program took it as a move: the original is to come out.
    Move,
    /// It landed back in this window, at a point in the drawing area, as a
    /// copy or a move — which the program is told after the fact, since it
    /// was busy giving the drag while the drop happened.
    DroppedOnSelf { x: i32, y: i32, copying: bool },
}

/// Gives the contents to the desktop as a drag, and waits until the drag
/// ends — with a drop somewhere, or with nothing. What the other program
/// did with it is the answer.
#[must_use]
pub fn start_drag(contents: &clipboard::Contents) -> DragEffect {
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::start_drag(contents)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = contents;
        DragEffect::None
    }
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
    /// open, and draws it again: the program has put up its question about
    /// unsaved changes, and closes the window itself, with [`Self::Close`],
    /// once the question is answered — or keeps it, when the answer is
    /// Cancel.
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
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::window_command(command);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
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
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::double_click_millis()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
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
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::caret_blink_millis()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
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
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::open_window(title)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = title;
        false
    }
}

/// How many windows the program has open.
#[must_use]
pub fn window_count() -> usize {
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::window_count()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        0
    }
}

/// Lays the open windows out side by side, filling the screen.
///
/// Word's Arrange All. Returns how many were moved.
pub fn arrange_windows() -> usize {
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::arrange_windows()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        0
    }
}
/// Whether the window fills the screen, so its button can show which it is.
#[must_use]
pub fn is_maximised() -> bool {
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::is_maximised()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
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

    /// What is on the window, for a screen reader: every control a person
    /// could reach, in reading order. See [`accessibility`].
    fn accessible_elements(&mut self) -> Vec<accessibility::Element> {
        Vec::new()
    }

    /// Carries out what pressing an element does.
    fn accessible_invoke(&mut self, id: u64) -> Response {
        let _ = id;
        Response::Ignored
    }

    /// The document's text and selection, for a screen reader to read.
    fn accessible_text(&mut self) -> Option<accessibility::TextState> {
        None
    }

    /// Selects a stretch of the document, by character offsets into the
    /// text [`App::accessible_text`] gives.
    fn accessible_select(&mut self, start: usize, end: usize) -> Response {
        let _ = (start, end);
        Response::Ignored
    }

    /// Where a stretch of the document's text is on the window, as
    /// rectangles in pixels of the drawing area: one per line it covers.
    fn accessible_rects(&mut self, start: usize, end: usize) -> Vec<(i32, i32, i32, i32)> {
        let _ = (start, end);
        Vec::new()
    }

    /// The document's lines as the layout breaks them, each as the
    /// character offsets it starts and ends at. Nothing where the
    /// application has no layout, and then a line is a paragraph.
    fn accessible_lines(&mut self) -> Vec<(usize, usize)> {
        Vec::new()
    }

    /// How the document's text at an offset is set — the font, the size,
    /// bold and the rest — and the stretch around it that is set the same.
    fn accessible_attributes(
        &mut self,
        offset: usize,
    ) -> Option<(accessibility::TextAttributes, usize, usize)> {
        let _ = offset;
        None
    }

    /// Puts a value into an element that holds one — a box's text.
    fn accessible_set_value(&mut self, id: u64, value: &str) -> Response {
        let _ = (id, value);
        Response::Ignored
    }
}

/// What a screen reader is told about the window.
///
/// A window that draws every control itself is a blank rectangle to a
/// screen reader unless it says what is in it. This is what it says: each
/// control with its kind, its name, where it is, what it holds, what it is
/// inside and what it does; and the document as text with a selection in
/// it, broken into lines where the layout breaks it, with how each stretch
/// is set. The Windows shell puts this through UI Automation, and the Linux
/// shells — X11 and Wayland alike — through AT-SPI on the accessibility
/// bus. Each also watches it change, and says so: a dialog opening, the
/// keyboard moving, a message on the status strip.
pub mod accessibility {
    /// The kind of control an element is: what a screen reader calls it and
    /// how it lets a person work it.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Role {
        /// A button that does something when pressed.
        Button,
        /// A button that is on or off.
        Toggle,
        /// One tab of the ribbon.
        TabItem,
        /// The document being edited.
        Document,
        /// Words that only say something.
        #[default]
        Text,
        /// A dialog: the fields and buttons in it are its children.
        Dialog,
        /// A pane beside the page: the navigation pane, the styles.
        Pane,
        /// A menu that has dropped open, and one of its items.
        Menu,
        MenuItem,
        /// A list, and one of its rows.
        List,
        ListItem,
        /// A box that is typed into: its value is what is in it.
        Edit,
        /// A tick box, on when selected.
        CheckBox,
        /// A box with a list under it: its value is what is chosen.
        ComboBox,
        /// A scroll bar: its range says where it is.
        ScrollBar,
        /// A ruler along the page.
        Ruler,
        /// The strip along the bottom: its value is the message it shows.
        StatusBar,
    }

    /// One control on the window.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Element {
        /// The same for the same control from one asking to the next.
        pub id: u64,
        pub role: Role,
        pub name: String,
        /// The letter that reaches it from the keyboard, if one does.
        pub access_key: String,
        /// Left, top, width, height, in pixels of the drawing area.
        pub rect: (i32, i32, i32, i32),
        /// Chosen, for a tab or a list's row; on, for a toggle or a tick box.
        pub selected: bool,
        pub enabled: bool,
        /// Whether keys go to it.
        pub focused: bool,
        /// The element it is inside — a dialog's fields are the dialog's, a
        /// menu's items the menu's — or none, for what is on the window.
        pub parent: Option<u64>,
        /// What it holds, for what holds something: a box's text, a list's
        /// choice, the status strip's message.
        pub value: String,
        /// For what stands somewhere between two ends — a scroll bar — the
        /// ends and where it is: least, most, now.
        pub range: Option<(f32, f32, f32)>,
    }

    /// How a stretch of the document's text is set, as a screen reader asks
    /// for it.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct TextAttributes {
        pub font: String,
        /// In points.
        pub size: f32,
        pub bold: bool,
        pub italic: bool,
        pub underline: bool,
        pub strike: bool,
        /// Red, green and blue, where the text says a colour.
        pub color: Option<(u8, u8, u8)>,
        /// The highlight or shading behind it, where there is one.
        pub background: Option<(u8, u8, u8)>,
    }

    /// The document's text as a screen reader reads it.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct TextState {
        /// The whole text, one line break between paragraphs.
        pub text: String,
        /// The selection, as character offsets into the text; equal when
        /// nothing is selected.
        pub selection: (usize, usize),
    }
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
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::run(options, app)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (options, app);
        Err(Error::UnsupportedPlatform)
    }
}

/// Whether this build can open a window at all.
#[must_use]
pub fn is_supported() -> bool {
    cfg!(any(windows, target_os = "linux"))
}

/// The code pages this machine writes plain text by: the Windows one and the
/// DOS one, by number. What Word's File Conversion dialog means by "Windows
/// (Default)" and "MS-DOS". Off Windows they are the Western European ones,
/// which is what a machine that does not say is taken to be.
#[must_use]
pub fn system_code_pages() -> (u32, u32) {
    #[cfg(any(windows, target_os = "linux"))]
    {
        crate::platform::system_code_pages()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        (1252, 437)
    }
}

/// Tells the input method where the caret is, in pixels of the drawing
/// area: its candidate list opens beside it, and its own windows keep off
/// the text being composed. Called whenever the caret is drawn.
pub fn place_composition(x: i32, y: i32, height: i32) {
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::place_composition(x, y, height);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (x, y, height);
    }
}

/// Changes the title in the window's caption bar.
///
/// Called when the document being edited changes, so the caption says which
/// file is open — which is where a person looks to find out.
pub fn set_title(title: &str) {
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::set_window_title(title);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
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
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::choose_file(title, filters, None, false)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (title, filters);
            None
        }
    }

    /// Asks for a file to open, and says which of the types in the list was
    /// chosen, counted from nought, where the system's dialog tells: a type
    /// may be a way of reading rather than a kind of file, as Word's
    /// "Recover Text from Any File" is.
    #[must_use]
    pub fn open_file_typed(
        title: &str,
        filters: &[FileFilter],
    ) -> Option<(PathBuf, Option<usize>)> {
        #[cfg(windows)]
        {
            crate::platform::choose_file_typed(title, filters, None, false)
        }
        #[cfg(target_os = "linux")]
        {
            crate::platform::choose_file(title, filters, None, false).map(|path| (path, None))
        }
        #[cfg(not(any(windows, target_os = "linux")))]
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
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::choose_file(title, filters, suggested, true)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (title, filters, suggested);
            None
        }
    }

    /// Asks where to save a file, and which of the types in the list it is to
    /// be saved as, counted from nought — where the desktop's dialog says.
    ///
    /// Two types can share an extension, as a web page and a filtered one do,
    /// and then the name alone does not say which was meant. The dialogs of
    /// Linux desktops do not say which type was chosen, and there the answer
    /// is only the name.
    #[must_use]
    pub fn save_file_typed(
        title: &str,
        filters: &[FileFilter],
        suggested: Option<&Path>,
    ) -> Option<(PathBuf, Option<usize>)> {
        #[cfg(windows)]
        {
            crate::platform::choose_file_typed(title, filters, suggested, true)
        }
        #[cfg(target_os = "linux")]
        {
            crate::platform::choose_file(title, filters, suggested, true).map(|path| (path, None))
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (title, filters, suggested);
            None
        }
    }

    /// Asks whether to save changes before throwing them away.
    #[must_use]
    pub fn ask_to_save(name: &str) -> Answer {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::ask_to_save(name)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = name;
            // Without a way to ask, the safe answer is to do nothing rather
            // than to discard the user's work.
            Answer::Cancel
        }
    }

    /// Asks a question with two answers: yes, or anything else.
    #[must_use]
    pub fn ask_yes_no(question: &str) -> bool {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::ask_yes_no(question)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = question;
            false
        }
    }

    /// Tells the user something and asks whether to go on: OK, or cancel.
    #[must_use]
    pub fn ask_ok_cancel(message: &str) -> bool {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::ask_ok_cancel(message)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = message;
            true
        }
    }

    /// Tells the user something went wrong.
    pub fn show_error(message: &str) {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::show_error(message);
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            eprintln!("error: {message}");
        }
    }

    /// Tells the user something that went as it should, with nothing to
    /// answer but OK.
    pub fn show_message(message: &str) {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::show_message(message);
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            println!("{message}");
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
///
/// On Windows the pages go to the spooler through a device context; on Linux
/// they go to CUPS as a PDF of pictures, one a page, over its socket. The
/// same bands, either way.
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
        #[cfg(any(windows, target_os = "linux"))]
        pub(crate) fn from_device_context(device_context: usize) -> Self {
            Self { device_context, started: false, finished: false }
        }

        /// The paper this printer is set up for.
        #[must_use]
        pub fn page(&self) -> PageSetup {
            #[cfg(any(windows, target_os = "linux"))]
            {
                crate::platform::printer_page(self.device_context)
            }
            #[cfg(not(any(windows, target_os = "linux")))]
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
            #[cfg(any(windows, target_os = "linux"))]
            {
                self.started = crate::platform::start_document(self.device_context, name);
                self.started
            }
            #[cfg(not(any(windows, target_os = "linux")))]
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
            #[cfg(any(windows, target_os = "linux"))]
            {
                let mut band = band;
                crate::platform::print_page(self.device_context, width, height, |top, rows| {
                    band(top, rows).to_bgra()
                })
            }
            #[cfg(not(any(windows, target_os = "linux")))]
            {
                let _ = (width, height, band);
                false
            }
        }

        /// Ends the job, sending it to the queue: whether the queue took
        /// it.
        pub fn finish(&mut self) -> bool {
            self.close(true)
        }

        /// Throws the job away instead.
        pub fn cancel(&mut self) {
            self.close(false);
        }

        fn close(&mut self, keep: bool) -> bool {
            if self.finished {
                return false;
            }
            self.finished = true;
            #[cfg(any(windows, target_os = "linux"))]
            {
                crate::platform::finish_document(self.device_context, keep && self.started)
            }
            #[cfg(not(any(windows, target_os = "linux")))]
            {
                let _ = keep;
                false
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
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::printer_names()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Vec::new()
        }
    }

    /// The one a document goes to when nobody has said otherwise.
    #[must_use]
    pub fn default_name() -> Option<String> {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::default_printer_name()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
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
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::supports_both_sides(name)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
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
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::open_printer_with(name, both_sides)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (name, both_sides);
            None
        }
    }

    /// Asks which printer to use through the system's own dialog. `None` means
    /// the user cancelled.
    #[must_use]
    pub fn choose() -> Option<Printer> {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::choose_printer()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            None
        }
    }
}

/// The system clipboard.
///
/// Cut, copy and paste are how text moves between this program and every other
/// one, so the clipboard belongs in the shell alongside the window: it is the
/// other half of talking to the desktop. What is carried is what Word carries:
/// the words as plain text, which every program understands; the same as HTML
/// and as Rich Text, which are how formatting crosses between programs; and a
/// picture as PNG and as a device-independent bitmap, which is how a picture
/// crosses. Each is put on the clipboard under its own format, and a program
/// pasting takes the richest one it knows.
pub mod clipboard {
    /// Everything on the clipboard at once, each in its own format. What is
    /// not there is `None`.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Contents {
        pub text: Option<String>,
        /// The "HTML Format" clipboard format, header and all.
        pub html: Option<Vec<u8>>,
        /// The "Rich Text Format" clipboard format.
        pub rtf: Option<Vec<u8>>,
        /// The "PNG" clipboard format: a PNG file.
        pub png: Option<Vec<u8>>,
        /// `CF_DIB`: a bitmap's information header and pixels, without the
        /// file header a `.bmp` file starts with.
        pub dib: Option<Vec<u8>>,
        /// What was copied as a Word document of its own, a `.docx`
        /// package: Word's own format, which on Windows goes as the
        /// embedded object Word offers beside the rest ("Embed Source",
        /// with its "Object Descriptor"), and comes back out of one Word
        /// put there. Nothing on Linux, where no program asks for it.
        pub document: Option<Vec<u8>>,
    }

    impl Contents {
        /// Whether there is anything at all.
        #[must_use]
        pub fn is_empty(&self) -> bool {
            self.text.as_deref().is_none_or(str::is_empty)
                && self.html.is_none()
                && self.rtf.is_none()
                && self.png.is_none()
                && self.dib.is_none()
                && self.document.is_none()
        }
    }

    /// Puts everything given on the clipboard at once, replacing what was
    /// there. Returns whether the system accepted it.
    pub fn set_contents(contents: &Contents) -> bool {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::clipboard_set_contents(contents)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = contents;
            false
        }
    }

    /// Reads everything on the clipboard that this program can take.
    #[must_use]
    pub fn contents() -> Contents {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::clipboard_contents()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Contents::default()
        }
    }

    /// Puts text on the clipboard, replacing what was there.
    ///
    /// Returns whether the system accepted it. Failure is normal rather than
    /// exceptional — another program can hold the clipboard open — so the
    /// caller is told and carries on.
    pub fn set_text(text: &str) -> bool {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::clipboard_set_text(text)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = text;
            false
        }
    }

    /// Reads text from the clipboard, if it holds any.
    #[must_use]
    pub fn text() -> Option<String> {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::clipboard_text()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
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
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::screen_windows()
                .into_iter()
                .map(|found| Window { title: found.title, handle: found.handle })
                .collect()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Vec::new()
        }
    }

    /// Photographs every monitor, side by side as the desktop arranges them.
    #[must_use]
    pub fn capture_screen() -> Option<Shot> {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::capture_screen().map(|shot| Shot {
                width: shot.width,
                height: shot.height,
                pixels: shot.pixels,
            })
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            None
        }
    }

    /// Whether a person can drag a rectangle out of the screen to be
    /// photographed — Word's Screen Clipping. Where the desktop's portal
    /// does it, on Wayland; not yet on X or Windows, where the program
    /// would have to lay a window of its own over the desktop.
    #[must_use]
    pub fn can_clip() -> bool {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::can_clip_screen()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            false
        }
    }

    /// Lets a person drag a rectangle out of the screen, and photographs
    /// it. Nothing if they cancel, or [`can_clip`] would have said no.
    #[must_use]
    pub fn clip() -> Option<Shot> {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::clip_screen().map(|shot| Shot {
                width: shot.width,
                height: shot.height,
                pixels: shot.pixels,
            })
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            None
        }
    }

    /// Photographs one window, whatever happens to be in front of it.
    #[must_use]
    pub fn capture_window(handle: usize) -> Option<Shot> {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::capture_window(handle).map(|shot| Shot {
                width: shot.width,
                height: shot.height,
                pixels: shot.pixels,
            })
        }
        #[cfg(not(any(windows, target_os = "linux")))]
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

        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::open_in_shell(address)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
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

/// The files a desktop knows this program by.
///
/// Two things, and they are the same thing from two sides. The desktop keeps
/// its own list of documents opened lately — the jump list on the Windows
/// taskbar, the Recent place in a file manager — and a program that opens a
/// document and does not tell it is a program whose documents are missing
/// from everywhere but its own Open page. And the desktop decides which
/// program opens a kind of file: until this one says which kinds it can open,
/// double-clicking a `.docx` cannot reach it and it is not even on the list
/// of programs to open one with.
pub mod files {
    use std::path::Path;

    /// A kind of file this program opens, as the desktop names one.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Kind {
        /// The extension with its dot: `.docx`.
        pub extension: &'static str,
        /// What a person calls it: "Word Document". What Windows shows in
        /// the file's column and in the Open With list.
        pub description: &'static str,
        /// The media type, which is how a Linux desktop names a kind, and
        /// how a file manager's Recent list knows what it is looking at.
        pub media_type: &'static str,
        /// Whether this program asks to be the one that opens the kind, or
        /// only one of the ones that can.
        ///
        /// A word processor that made itself the program for every web page
        /// on the machine is a program nobody can browse with, and one that
        /// took every plain text file from the text editor would be as
        /// unwelcome. Those kinds are registered so that Open With offers
        /// this program; the default is asked for only for documents.
        pub becomes_default: bool,
        /// Whether the kind is a template, which the desktop's own verb makes
        /// a new document from rather than opens: Word's New on a `.dotx`,
        /// with Open beside it for changing the template itself.
        pub is_template: bool,
        /// Whether the desktop's New menu offers an empty one — Explorer's
        /// New ▸ Word Document, which makes a file of no bytes that the
        /// program then opens as a blank document.
        pub in_new_menu: bool,
    }

    /// The kinds this program tells the desktop it opens: the ones the
    /// Options dialog's Make Default registers, and the installer too.
    ///
    /// Word registers every kind it can read, which is what puts it in Open
    /// With for all of them, and asks to be the one that opens the documents
    /// among them. A plain text file and a web page are not documents in
    /// that sense: the machine already has a program for each, and taking
    /// those would be taking something nobody asked to give.
    pub const KINDS: &[Kind] = &[
        Kind {
            extension: ".docx",
            description: "Word Document",
            media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            becomes_default: true,
            is_template: false,
            in_new_menu: true,
        },
        Kind {
            extension: ".docm",
            description: "Word Macro-Enabled Document",
            media_type: "application/vnd.ms-word.document.macroEnabled.12",
            becomes_default: true,
            is_template: false,
            in_new_menu: false,
        },
        Kind {
            extension: ".dotx",
            description: "Word Template",
            media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
            becomes_default: true,
            is_template: true,
            in_new_menu: false,
        },
        Kind {
            extension: ".dotm",
            description: "Word Macro-Enabled Template",
            media_type: "application/vnd.ms-word.template.macroEnabled.12",
            becomes_default: true,
            is_template: true,
            in_new_menu: false,
        },
        Kind {
            extension: ".doc",
            description: "Word 97-2003 Document",
            media_type: "application/msword",
            becomes_default: true,
            is_template: false,
            in_new_menu: false,
        },
        Kind {
            extension: ".rtf",
            description: "Rich Text Format",
            media_type: "application/rtf",
            becomes_default: true,
            is_template: false,
            in_new_menu: false,
        },
        Kind {
            extension: ".odt",
            description: "OpenDocument Text",
            media_type: "application/vnd.oasis.opendocument.text",
            becomes_default: true,
            is_template: false,
            in_new_menu: false,
        },
        Kind {
            extension: ".txt",
            description: "Text Document",
            media_type: "text/plain",
            becomes_default: false,
            is_template: false,
            in_new_menu: false,
        },
        Kind {
            extension: ".htm",
            description: "Web Page",
            media_type: "text/html",
            becomes_default: false,
            is_template: false,
            in_new_menu: false,
        },
        Kind {
            extension: ".html",
            description: "Web Page",
            media_type: "text/html",
            becomes_default: false,
            is_template: false,
            in_new_menu: false,
        },
    ];

    /// The switch that opens a file as File ▸ Open does rather than as a
    /// double-click does: the one difference is a template, which a
    /// double-click makes a new document from and this opens for changing.
    /// The desktop's Open verb on a template carries it.
    pub const OPEN_SWITCH: &str = "--open";

    /// Puts a document on the desktop's own list of documents opened lately.
    ///
    /// Called when one is opened and when one is saved — the same two moments
    /// as the program's own list, because they are the two moments a person
    /// would say they had used the file.
    pub fn remember(path: &Path, media_type: &str) {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::remember_document(path, media_type);
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (path, media_type);
        }
    }

    /// Tells the desktop this program opens these kinds of file.
    ///
    /// Returns whether the desktop took it. What that leaves behind differs:
    /// a Linux desktop lets a program say it is the one to open a kind, and
    /// this makes it so; Windows lets a program say only that it *can*,
    /// because which program opens a kind is the person's to choose and is
    /// kept where a program cannot write it. So on Windows this registers the
    /// kinds — which is what puts the program in Open With, in Default Apps
    /// and in the list a `.docx` offers — and [`choose_defaults`] is how the
    /// person is then taken to the page where they choose.
    pub fn associate(kinds: &[Kind], program_name: &str) -> bool {
        #[cfg(any(windows, target_os = "linux"))]
        {
            let Ok(program) = std::env::current_exe() else { return false };
            crate::platform::associate_kinds(kinds, program_name, &program)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (kinds, program_name);
            false
        }
    }

    /// Whether this program is the one the desktop opens that kind with.
    #[must_use]
    pub fn opens(kind: &Kind) -> bool {
        #[cfg(any(windows, target_os = "linux"))]
        {
            std::env::current_exe().is_ok_and(|program| crate::platform::opens_kind(kind, &program))
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = kind;
            false
        }
    }

    /// Whether the desktop needs the person to choose rather than taking a
    /// program's word for it. True on Windows, false elsewhere.
    #[must_use]
    pub fn defaults_are_chosen_by_hand() -> bool {
        cfg!(windows)
    }

    /// Opens the desktop's own page for choosing which program opens what.
    ///
    /// Returns whether the desktop opened it. This is where Windows sends a
    /// program that has registered its kinds: the choice is made there, in
    /// the page the person already knows, rather than taken from them here.
    pub fn choose_defaults() -> bool {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::choose_default_programs()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            false
        }
    }
}

/// What an installer tells the desktop besides copying the files: which
/// kinds of file the program opens, where the desktop lists it, and how it
/// is taken off again.
///
/// # For one person, without an administrator
///
/// The program goes where the desktop keeps programs a person installed for
/// themselves — on Windows `%LOCALAPPDATA%\Programs`, where Windows' own
/// per-user installers put them, on Linux the data directory — and
/// everything registered is under that person's own keys and files. Nothing
/// asks for an administrator, and nothing another person on the machine has
/// is touched.
pub mod install {
    use std::path::{Path, PathBuf};

    /// What the program is called wherever the desktop lists it.
    pub const PROGRAM_NAME: &str = "Word Processor";

    /// What the uninstaller is started with, by the list of installed
    /// programs and by a person alike.
    pub const UNINSTALL_SWITCH: &str = "--uninstall";

    /// Said as well, the installer or the uninstaller asks nothing and says
    /// nothing, which is what the list's quiet uninstall runs.
    pub const QUIET_SWITCH: &str = "--quiet";

    /// A program installed, as the desktop is told about it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Installed {
        /// The folder the files are in.
        pub folder: PathBuf,
        /// The windowed program, which every kind opens with.
        pub program: PathBuf,
        /// What takes it all off again.
        pub uninstaller: PathBuf,
        /// The version, as the list of installed programs shows it.
        pub version: String,
        /// How many bytes the files take, which the list shows too.
        pub size: u64,
    }

    /// Where a program installed for one person goes, or `None` where the
    /// desktop does not say.
    #[must_use]
    pub fn folder() -> Option<PathBuf> {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::install_folder()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            None
        }
    }

    /// Tells the desktop about an installed program: every kind in
    /// [`crate::files::KINDS`] opening with it — a template's double-click
    /// making a new document, with Open beside it — and its place in the
    /// desktop's menu; on Windows its entry in Apps & features as well.
    /// Returns whether all of it was taken.
    pub fn register(installed: &Installed) -> bool {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::register_installed(installed)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = installed;
            false
        }
    }

    /// Takes back everything [`register`] wrote, leaving what other programs
    /// wrote and what the person chose. Returns whether all of it went.
    pub fn unregister(installed: &Installed) -> bool {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::unregister_installed(installed)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = installed;
            false
        }
    }

    /// Whether the desktop now opens the kind with the installed program.
    #[must_use]
    pub fn opens(kind: &crate::files::Kind, program: &Path) -> bool {
        #[cfg(any(windows, target_os = "linux"))]
        {
            crate::platform::opens_kind(kind, program)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (kind, program);
            false
        }
    }
}

/// What the machine's owner has said about how things are written.
///
/// # Why this is asked rather than decided
///
/// Because a person who has told their computer they are in Germany has
/// already said that a length is in centimetres, that a decimal point is a
/// comma, that paper is A4 and that today is written with the day first.
/// Asking them again in this program's own settings would be asking a
/// question they have answered. Word asks the system for exactly these,
/// and so does this.
///
/// # What is here and what is not
///
/// What the interface needs to write a number, a length, a date and a
/// sheet of paper. Not the language of the interface — that is chosen in
/// Options and lives in the catalogues, because a person may well want a
/// program in English on a machine set to French. Word keeps the two
/// apart in the same way.
pub mod locale {
    /// Which sheet of paper a new document is on.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Paper {
        /// 210 by 297 millimetres, which is everywhere but North America.
        A4,
        /// 8.5 by 11 inches.
        Letter,
    }

    /// The order the parts of a short date are written in.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum DateOrder {
        DayMonthYear,
        MonthDayYear,
        YearMonthDay,
    }

    /// Everything this program asks the machine about.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Locale {
        /// What the place is called: `en-GB`, `de-DE`. Empty where the
        /// machine does not say.
        pub name: String,
        /// Whether lengths are metric.
        pub metric: bool,
        pub paper: Paper,
        /// What stands between the whole part of a number and its fraction.
        pub decimal: char,
        /// What stands between the thousands, if anything does.
        pub thousands: Option<char>,
        pub date_order: DateOrder,
        /// What goes between the parts of a short date.
        pub date_separator: char,
        /// The months and the days, as this machine writes them, from
        /// January and from Monday.
        pub months: Vec<String>,
        pub days: Vec<String>,
        /// Whether the clock has twenty-four hours on it.
        pub twenty_four_hour: bool,
    }

    impl Default for Locale {
        /// What a machine that says nothing is taken to be: the way this
        /// program's own documentation is written.
        fn default() -> Self {
            Self {
                name: String::new(),
                metric: false,
                paper: Paper::Letter,
                decimal: '.',
                thousands: Some(','),
                date_order: DateOrder::MonthDayYear,
                date_separator: '/',
                months: MONTHS.iter().map(|month| (*month).to_owned()).collect(),
                days: DAYS.iter().map(|day| (*day).to_owned()).collect(),
                twenty_four_hour: false,
            }
        }
    }

    /// The English names, for a machine that gives none.
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const DAYS: [&str; 7] =
        ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

    /// What this machine says. Asked once: it does not change while a
    /// program runs, and asking the system costs a call apiece.
    #[must_use]
    pub fn current() -> &'static Locale {
        static ASKED: std::sync::OnceLock<Locale> = std::sync::OnceLock::new();
        ASKED.get_or_init(|| {
            #[cfg(any(windows, target_os = "linux"))]
            {
                crate::platform::locale()
            }
            #[cfg(not(any(windows, target_os = "linux")))]
            {
                Locale::default()
            }
        })
    }
}

/// Bytes nobody can guess.
///
/// # Why a shell crate carries them
///
/// Because no arithmetic makes them. Anything a program can work out from what
/// it already holds is something anybody else can work out too; bytes that
/// cannot be guessed come from the machine, which is watching a keyboard, a
/// disc and a clock that nothing else can see. That makes it a question for the
/// operating system, and the operating system is what this crate is for.
///
/// # What they are for here
///
/// The salt under a password. A salt is what stops two documents locked with
/// the same word carrying the same hash, and it is worth nothing unless it
/// could not have been guessed.
pub mod random {
    /// Fills the slice with bytes from the machine's own source, and says
    /// whether it could.
    ///
    /// A machine that will not give them is a machine that cannot make a salt,
    /// and the caller has to say so rather than make one up: a salt taken from
    /// the clock is a salt that can be worked out again, and writing one would
    /// be claiming a protection this cannot give.
    pub fn fill(bytes: &mut [u8]) -> bool {
        #[cfg(windows)]
        {
            // RtlGenRandom, which every program on Windows ends up at and
            // which the library exports under this name and no other.
            #[link(name = "advapi32")]
            extern "system" {
                fn SystemFunction036(buffer: *mut u8, length: u32) -> u8;
            }
            let Ok(length) = u32::try_from(bytes.len()) else { return false };
            unsafe { SystemFunction036(bytes.as_mut_ptr(), length) != 0 }
        }
        #[cfg(not(windows))]
        {
            use std::io::Read;
            // Every Unix has it, it does not block once the machine has
            // started, and it is where the system's own library reads from.
            std::fs::File::open("/dev/urandom")
                .and_then(|mut source| source.read_exact(bytes))
                .is_ok()
        }
    }

    /// A fresh array of them, or nothing if the machine would not give any.
    #[must_use]
    pub fn bytes<const N: usize>() -> Option<[u8; N]> {
        let mut out = [0u8; N];
        fill(&mut out).then_some(out)
    }
}

/// Handing a letter to whatever the machine uses for mail.
///
/// # Why a word processor does not send mail itself
///
/// Because sending mail means a server, an account, a password and a
/// protocol, and none of those belongs to a document. What Word does is hand
/// the letter to Outlook, which is the machine's mail program; what this does
/// is hand it to whatever the machine's mail program is, which is a question
/// the operating system already answers.
///
/// # What can be handed over, and what cannot
///
/// The address, the subject and the words. Not the formatting, and not an
/// attachment: the way every desktop agrees to open a mail program is a
/// `mailto:` address, and that carries text and nothing else. A merge to mail
/// therefore sends the letter as the words it says. That is a real limitation
/// and it is said here rather than discovered.
pub mod mail {
    /// Opens the machine's mail program with a letter in it, and says whether
    /// it could.
    ///
    /// Nothing is sent: what comes up is a message waiting to be looked at
    /// and sent by the person, which is what handing a letter to a mail
    /// program means and is the only honest thing for a word processor to do
    /// with somebody else's address book.
    pub fn compose(to: &str, subject: &str, body: &str) -> bool {
        let mut url = String::from("mailto:");
        url.push_str(&escaped(to));
        url.push_str("?subject=");
        url.push_str(&escaped(subject));
        url.push_str("&body=");
        url.push_str(&escaped(body));
        open(&url)
    }

    /// Everything that is not a letter or a digit written as its number,
    /// which is what an address has to be to survive being an address.
    ///
    /// The unreserved set of the standard, and nothing else: a space in a
    /// subject is `%20` and a newline in the words is `%0D%0A`, which is
    /// what a mail program expects to find and turn back.
    fn escaped(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for byte in text.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char);
                }
                b'\n' => out.push_str("%0D%0A"),
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        out
    }

    /// Hands it to the desktop, which is where every other address this
    /// program does not open itself goes. See [`crate::desktop::open`],
    /// which is also where the check lives that a program is not talked into
    /// running something by a document.
    fn open(url: &str) -> bool {
        crate::desktop::open(url)
    }

    #[cfg(test)]
    mod tests {
        use super::escaped;

        #[test]
        fn what_has_to_be_written_as_a_number_is() {
            assert_eq!(escaped("a b"), "a%20b");
            assert_eq!(escaped("x&y=z"), "x%26y%3Dz");
            assert_eq!(escaped("one\ntwo"), "one%0D%0Atwo");
            assert_eq!(escaped("a-b_c.d~e"), "a-b_c.d~e", "the unreserved set is left alone");
        }

        #[test]
        fn an_address_survives_being_written() {
            assert_eq!(escaped("somebody@example.com"), "somebody%40example.com");
        }

        #[test]
        fn a_letter_outside_the_alphabet_is_written_as_its_bytes() {
            // Two bytes in UTF-8, so two numbers.
            assert_eq!(escaped("é"), "%C3%A9");
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
    #[cfg(any(windows, target_os = "linux"))]
    {
        platform::set_frame_appearance(dark, border, caption);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
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

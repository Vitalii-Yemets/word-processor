//! The Windows shell, written against the Win32 ABI directly.
//!
//! Every declaration below matches what the operating system documents. Nothing
//! is generated and no binding library is used, which is what keeps the project
//! free of dependencies.
//!
//! The application lives in a thread-local rather than behind a raw pointer
//! stashed in the window. A window procedure is called by the system, so its
//! state has to be reachable from a plain function — but the usual trick of
//! casting a pointer through `SetWindowLongPtr` and back is exactly the sort of
//! thing that is wrong once and then wrong for years. A thread-local is safe,
//! and a window belongs to the thread that created it anyway.

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::Path;

use crate::{App, CompositionAttribute, Error, Event, Key, Modifiers, Response, WindowOptions};

// The parts of this shell that live in their own files, offered under the
// names the Linux shell offers them by.
pub(crate) use crate::dragdrop::start_drag;
pub(crate) use crate::uia::selection_changed;

// --- Types the API uses -----------------------------------------------------

pub(crate) type Handle = *mut c_void;
type WordParam = usize;
type LongParam = isize;
type Result_ = isize;
type WindowProcedure =
    Option<unsafe extern "system" fn(Handle, u32, WordParam, LongParam) -> Result_>;

#[repr(C)]
struct WindowClass {
    size: u32,
    style: u32,
    procedure: WindowProcedure,
    class_extra: i32,
    window_extra: i32,
    instance: Handle,
    icon: Handle,
    cursor: Handle,
    background: Handle,
    menu_name: *const u16,
    class_name: *const u16,
    small_icon: Handle,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Point {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[repr(C)]
struct Message {
    window: Handle,
    message: u32,
    word: WordParam,
    long: LongParam,
    time: u32,
    point: Point,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Rect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

#[repr(C)]
struct PaintStruct {
    device_context: Handle,
    erase: i32,
    paint_area: Rect,
    restore: i32,
    increment_update: i32,
    reserved: [u8; 32],
}

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    image_size: u32,
    x_pixels_per_meter: i32,
    y_pixels_per_meter: i32,
    colors_used: u32,
    colors_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 3],
}

/// What the open and save dialogs are told, laid out as the API documents it.
///
/// Every field is present even where this program has nothing to say about it:
/// the structure's size is passed to the system, which reads it by offset, so a
/// missing field would not be a smaller structure but a misread one.
#[repr(C)]
struct OpenFileName {
    size: u32,
    owner: Handle,
    instance: Handle,
    filter: *const u16,
    custom_filter: *mut u16,
    max_custom_filter: u32,
    filter_index: u32,
    file: *mut u16,
    max_file: u32,
    file_title: *mut u16,
    max_file_title: u32,
    initial_directory: *const u16,
    title: *const u16,
    flags: u32,
    file_offset: u16,
    file_extension: u16,
    default_extension: *const u16,
    custom_data: isize,
    hook: *mut c_void,
    template_name: *const u16,
    reserved_pointer: *mut c_void,
    reserved: u32,
    flags_extended: u32,
}

/// What the print dialog is told, laid out as the API documents it.
#[repr(C)]
struct PrintDialog {
    size: u32,
    owner: Handle,
    device_mode: Handle,
    device_names: Handle,
    device_context: Handle,
    flags: u32,
    from_page: u16,
    to_page: u16,
    min_page: u16,
    max_page: u16,
    copies: u16,
    instance: Handle,
    custom_data: isize,
    print_hook: *mut c_void,
    setup_hook: *mut c_void,
    print_template_name: *const u16,
    setup_template_name: *const u16,
    print_template: Handle,
    setup_template: Handle,
}

/// What a print job is called, which is what a print queue shows.
#[repr(C)]
struct DocumentInfo {
    size: i32,
    name: *const u16,
    output: *const u16,
    data_type: *const u16,
    kind: u32,
}

// --- Constants --------------------------------------------------------------

/// Redraw on either resize, and send double-click messages at all: without
/// `CS_DBLCLKS` a second press arrives as another plain press and the window
/// never learns that it was a double click.
const CLASS_REDRAW_ON_RESIZE: u32 = 0x0001 | 0x0002 | 0x0008;
const STYLE_OVERLAPPED_WINDOW: u32 = 0x00CF_0000;
const USE_DEFAULT_POSITION: i32 = 0x8000_0000_u32 as i32;
const SHOW_NORMAL: i32 = 1;

const MESSAGE_NON_CLIENT_CALC_SIZE: u32 = 0x0083;
const MESSAGE_NON_CLIENT_PAINT: u32 = 0x0085;
const MESSAGE_NON_CLIENT_ACTIVATE: u32 = 0x0086;
const MESSAGE_NON_CLIENT_HIT_TEST: u32 = 0x0084;
const MESSAGE_DESTROY: u32 = 0x0002;
const MESSAGE_SIZE: u32 = 0x0005;
const MESSAGE_PAINT: u32 = 0x000F;
const MESSAGE_ERASE_BACKGROUND: u32 = 0x0014;
const MESSAGE_KEY_DOWN: u32 = 0x0100;
const MESSAGE_KEY_UP: u32 = 0x0101;
const MESSAGE_CHAR: u32 = 0x0102;
const MESSAGE_MOUSE_MOVE: u32 = 0x0200;
const MESSAGE_MOUSE_LEAVE: u32 = 0x02A3;
const MESSAGE_LEFT_BUTTON_DOWN: u32 = 0x0201;
const MESSAGE_LEFT_BUTTON_UP: u32 = 0x0202;
const MESSAGE_LEFT_BUTTON_DOUBLE_CLICK: u32 = 0x0203;
const MESSAGE_RIGHT_BUTTON_DOWN: u32 = 0x0204;
const MESSAGE_RIGHT_BUTTON_UP: u32 = 0x0205;
const MESSAGE_MIDDLE_BUTTON_DOWN: u32 = 0x0207;
const MESSAGE_MOUSE_WHEEL: u32 = 0x020A;
const MESSAGE_SET_CURSOR: u32 = 0x0020;
const MESSAGE_CLOSE: u32 = 0x0010;
const MESSAGE_TIMER: u32 = 0x0113;
/// Alt and the keys pressed with it, which Windows keeps apart from the rest.
const MESSAGE_SYSTEM_KEY_DOWN: u32 = 0x0104;
const MESSAGE_SYSTEM_KEY_UP: u32 = 0x0105;
const MESSAGE_SYSTEM_CHAR: u32 = 0x0106;
/// The system, or a screen reader through it, asking what the window is.
const MESSAGE_GET_OBJECT: u32 = 0x003D;
/// The window moved to a screen of another density, or the density changed.
const MESSAGE_DPI_CHANGED: u32 = 0x02E0;
/// The density of an ordinary screen, which every measurement here is in.
const ORDINARY_DPI: f32 = 96.0;
/// What the system is asked to be: aware of each screen's density, and of
/// changes to it, with the frame drawn to match.
const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
// The input method: a composition starting, changing and ending, the
// context being set, and the requests it makes of the window.
const MESSAGE_IME_START_COMPOSITION: u32 = 0x010D;
const MESSAGE_IME_END_COMPOSITION: u32 = 0x010E;
const MESSAGE_IME_COMPOSITION: u32 = 0x010F;
const MESSAGE_IME_SET_CONTEXT: u32 = 0x0281;
const MESSAGE_IME_REQUEST: u32 = 0x0288;
/// The composition string, its cursor, its attributes, and the result.
const GCS_COMPSTR: isize = 0x0008;
const GCS_COMPATTR: isize = 0x0010;
const GCS_CURSORPOS: isize = 0x0080;
const GCS_RESULTSTR: isize = 0x0800;
/// The bit of the set-context message that would have the input method
/// draw the composition in a window of its own.
const ISC_SHOW_UI_COMPOSITION_WINDOW: isize = -0x8000_0000;
/// The request for where a character of the composition is on screen.
const IMR_QUERY_CHAR_POSITION: usize = 0x0006;
const CFS_POINT: u32 = 0x0002;
const CFS_EXCLUDE: u32 = 0x0080;

/// The one timer the window keeps, and how often it goes off.
///
/// Fast enough that the caret blinks on the beat the system asks for and that a
/// tip appears when the pointer has rested long enough, and slow enough that a
/// window nobody is touching costs next to nothing.
const TICK_TIMER: usize = 1;
const TICK_MILLIS: u32 = 40;

const WHEEL_STEP: f32 = 120.0;
const ARROW_CURSOR: *const u16 = 32512 as *const u16;
const TEXT_CURSOR: *const u16 = 32513 as *const u16;
const RESIZE_HORIZONTAL_CURSOR: *const u16 = 32644 as *const u16;
const RESIZE_VERTICAL_CURSOR: *const u16 = 32645 as *const u16;
const HAND_CURSOR: *const u16 = 32649 as *const u16;

/// The low word of `WM_SETCURSOR`'s long parameter: which part of the window
/// the pointer is over. Only the client area is this program's to decide; the
/// resize edges belong to the system.
const HIT_CLIENT_AREA: isize = 1;
const BITMAP_UNCOMPRESSED: u32 = 0;
const BITMAP_RGB_COLORS: u32 = 0;
const COPY_SOURCE: u32 = 0x00CC_0020;

/// The bit of a mouse message's word parameter that means the left button.
const LEFT_BUTTON_HELD: usize = 0x0001;

/// Clipboard format for UTF-16 text, which is the one every program speaks.
const CLIPBOARD_UNICODE_TEXT: u32 = 13;
/// A bitmap: its information header and pixels, as `CF_DIB`.
const CLIPBOARD_DIB: u32 = 8;
/// Clipboard memory has to be movable, because the system takes ownership.
pub(crate) const MEMORY_MOVEABLE: u32 = 0x0002;

// Flags for the open and save dialogs.
const OFN_OVERWRITE_PROMPT: u32 = 0x0000_0002;
const OFN_HIDE_READ_ONLY: u32 = 0x0000_0004;
/// Leaves the process's working directory alone. Without it the dialog silently
/// changes it, and every relative path the program is later given goes wrong.
const OFN_NO_CHANGE_DIR: u32 = 0x0000_0008;
const OFN_PATH_MUST_EXIST: u32 = 0x0000_0800;
const OFN_FILE_MUST_EXIST: u32 = 0x0000_1000;
const OFN_EXPLORER: u32 = 0x0008_0000;

/// How long a path the dialog may return. Far past `MAX_PATH`, which stopped
/// being the real limit long ago.
const PATH_BUFFER: usize = 32768;

// Message box styles and the answers they give back.
const MB_YES_NO_CANCEL: u32 = 0x0000_0003;
const MB_YES_NO: u32 = 0x0000_0004;
const MB_OK: u32 = 0x0000_0000;
const MB_OK_CANCEL: u32 = 0x0000_0001;
const MB_ICON_INFORMATION: u32 = 0x0000_0040;
const MB_ICON_WARNING: u32 = 0x0000_0030;
const MB_ICON_ERROR: u32 = 0x0000_0010;
const ID_CANCEL: i32 = 2;
const ID_OK: i32 = 1;
const ID_YES: i32 = 6;
const ID_NO: i32 = 7;

// Flags for the print dialog.
/// Hands back a device context for the chosen printer, which is the whole
/// point: without it the dialog only reports a choice.
const PD_RETURN_DC: u32 = 0x0000_0100;
const PD_NO_SELECTION: u32 = 0x0000_0004;
const PD_NO_PAGE_NUMBERS: u32 = 0x0000_0008;
const PD_HIDE_PRINT_TO_FILE: u32 = 0x0010_0000;
/// Lets the printer's own settings decide the number of copies, rather than
/// the program printing each page twice itself.
const PD_USE_DEVICE_MODE_COPIES: u32 = 0x0004_0000;

// What to ask a device context about itself.
const CAPABILITY_HORIZONTAL_PIXELS: i32 = 8;
const CAPABILITY_VERTICAL_PIXELS: i32 = 10;
const CAPABILITY_DPI_X: i32 = 88;
const CAPABILITY_DPI_Y: i32 = 90;
/// The whole sheet, in the device's own dots — larger than the printable area
/// by the band the printer holds the paper in.
const CAPABILITY_PAPER_WIDTH: i32 = 110;
const CAPABILITY_PAPER_HEIGHT: i32 = 111;
/// Where the printable area begins on that sheet. A printer's origin is this
/// corner, not the corner of the paper, and a page drawn without allowing for
/// it comes out shifted by a quarter of an inch.
const CAPABILITY_OFFSET_X: i32 = 112;
const CAPABILITY_OFFSET_Y: i32 = 113;

// What a hit test can say a point is on.
const HIT_CLIENT: Result_ = 1;
const HIT_CAPTION: Result_ = 2;
const HIT_LEFT: Result_ = 10;
const HIT_RIGHT: Result_ = 11;
const HIT_TOP: Result_ = 12;
const HIT_TOP_LEFT: Result_ = 13;
const HIT_TOP_RIGHT: Result_ = 14;
const HIT_BOTTOM: Result_ = 15;
const HIT_BOTTOM_LEFT: Result_ = 16;
const HIT_BOTTOM_RIGHT: Result_ = 17;

/// How wide the invisible band along each edge is, where the pointer resizes
/// the window rather than reaching what is drawn there.
const RESIZE_BORDER: i32 = 6;

// Window states and the commands that change them.
const WINDOW_MAXIMIZED: u32 = 0x0100_0000;

/// Recalculate the frame, leave the size, position, order and focus alone.
///
/// `SWP_FRAMECHANGED` is the one that matters: without it the frame the system
/// worked out while the window was being created stands, and the caption it
/// drew there stays on screen above the one this program draws for itself.
const FRAME_CHANGED: u32 = 0x0001 | 0x0002 | 0x0004 | 0x0010 | 0x0020;
const SHOW_MINIMIZED: i32 = 6;
const SHOW_MAXIMIZED: i32 = 3;
const SHOW_RESTORED: i32 = 9;
/// What the size message says when the window has just been minimised.
const SIZE_MINIMISED: WordParam = 1;
/// Moving and sizing a window, without changing its place in the stack.
const MOVE_AND_SIZE: u32 = 0x0004 | 0x0010;
/// Asking the system for the part of the screen a window may use.
const SPI_GET_WORK_AREA: u32 = 0x0030;

/// How many rows of a page are rasterized at once when printing.
///
/// A page at a printer's own resolution is far too large to hold as one image —
/// A4 at 600 dots to the inch is nearly 140 megabytes. It is drawn in bands
/// instead, which costs nothing in quality and bounds the memory whatever the
/// printer's resolution turns out to be.
const PRINT_BAND_ROWS: usize = 256;

// Virtual key codes for the keys the shell reports.
const KEY_BACKSPACE: u32 = 0x08;
const KEY_TAB: u32 = 0x09;
const KEY_SPACE: u32 = 0x20;
const KEY_ENTER: u32 = 0x0D;
const KEY_ESCAPE: u32 = 0x1B;
const KEY_PAGE_UP: u32 = 0x21;
const KEY_PAGE_DOWN: u32 = 0x22;
const KEY_END: u32 = 0x23;
const KEY_HOME: u32 = 0x24;
const KEY_LEFT: u32 = 0x25;
const KEY_UP: u32 = 0x26;
const KEY_RIGHT: u32 = 0x27;
const KEY_DOWN: u32 = 0x28;
const KEY_DELETE: u32 = 0x2E;
const KEY_CONTROL: i32 = 0x11;
const KEY_SHIFT: i32 = 0x10;
const KEY_ALT: i32 = 0x12;

// --- The system functions used ----------------------------------------------

#[link(name = "user32")]
extern "system" {
    fn RegisterClassExW(class: *const WindowClass) -> u16;
    #[allow(clippy::too_many_arguments)]
    fn CreateWindowExW(
        extended_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        parameter: *mut c_void,
    ) -> Handle;
    fn DefWindowProcW(window: Handle, message: u32, word: WordParam, long: LongParam) -> Result_;
    fn ShowWindow(window: Handle, command: i32) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn SetWindowPos(
        window: Handle,
        after: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    fn UpdateWindow(window: Handle) -> i32;
    fn GetMessageW(message: *mut Message, window: Handle, first: u32, last: u32) -> i32;
    fn TranslateMessage(message: *const Message) -> i32;
    fn DispatchMessageW(message: *const Message) -> Result_;
    fn PostQuitMessage(code: i32);
    fn DestroyWindow(window: Handle) -> i32;
    /// Puts a message in the queue and returns at once, rather than calling the
    /// window procedure there and then. See [`window_command`].
    fn PostMessageW(window: Handle, message: u32, word: WordParam, long: LongParam) -> i32;
    fn BeginPaint(window: Handle, paint: *mut PaintStruct) -> Handle;
    fn EndPaint(window: Handle, paint: *const PaintStruct) -> i32;
    pub(crate) fn InvalidateRect(window: Handle, area: *const Rect, erase: i32) -> i32;
    fn GetClientRect(window: Handle, area: *mut Rect) -> i32;
    fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    fn SetCursor(cursor: Handle) -> Handle;
    fn GetCursorPos(point: *mut Point) -> i32;
    fn GetKeyState(key: i32) -> i16;
    fn GetWindowLongW(window: Handle, index: i32) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn SetProcessDPIAware() -> i32;
    fn GetDoubleClickTime() -> u32;
    /// How long the caret rests on each side of a blink, in milliseconds.
    fn GetCaretBlinkTime() -> u32;
    /// Asks for `MESSAGE_TIMER` every so many milliseconds.
    fn SetTimer(window: Handle, id: usize, interval: u32, callback: *const c_void) -> usize;
    fn KillTimer(window: Handle, id: usize) -> i32;
    /// Asks to be told when the pointer leaves the window, which Windows does
    /// not say unless it is asked.
    fn TrackMouseEvent(track: *mut TrackMouse) -> i32;
    fn SystemParametersInfoW(action: u32, param: u32, data: *mut c_void, update: u32) -> i32;
    pub(crate) fn ScreenToClient(window: Handle, point: *mut Point) -> i32;
    pub(crate) fn ClientToScreen(window: Handle, point: *mut Point) -> i32;
    /// Routes mouse messages to this window even when the pointer leaves it,
    /// which is what lets a selection keep growing during a drag.
    fn SetCapture(window: Handle) -> Handle;
    fn ReleaseCapture() -> i32;
    fn OpenClipboard(owner: Handle) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: Handle) -> Handle;
    fn GetClipboardData(format: u32) -> Handle;
    fn IsClipboardFormatAvailable(format: u32) -> i32;
    fn SetWindowTextW(window: Handle, title: *const u16) -> i32;
    pub(crate) fn RegisterClipboardFormatW(name: *const u16) -> u32;
    fn MessageBoxW(owner: Handle, text: *const u16, caption: *const u16, style: u32) -> i32;
}

/// Where the input method puts its composition window, and where a
/// candidate list keeps away from.
#[repr(C)]
struct CompositionForm {
    style: u32,
    current: Point,
    area: Rect,
}

#[repr(C)]
struct CandidateForm {
    index: u32,
    style: u32,
    current: Point,
    area: Rect,
}

/// The answer to a request for where a character is.
#[repr(C)]
struct CharacterPosition {
    size: u32,
    character: u32,
    point: Point,
    line_height: u32,
    document: Rect,
}

#[link(name = "imm32")]
extern "system" {
    fn ImmGetContext(window: Handle) -> Handle;
    fn ImmReleaseContext(window: Handle, context: Handle) -> i32;
    fn ImmGetCompositionStringW(
        context: Handle,
        index: u32,
        buffer: *mut c_void,
        length: u32,
    ) -> i32;
    fn ImmSetCompositionWindow(context: Handle, form: *const CompositionForm) -> i32;
    fn ImmSetCandidateWindow(context: Handle, form: *const CandidateForm) -> i32;
}

#[link(name = "comdlg32")]
extern "system" {
    fn GetOpenFileNameW(arguments: *mut OpenFileName) -> i32;
    fn GetSaveFileNameW(arguments: *mut OpenFileName) -> i32;
    fn PrintDlgW(arguments: *mut PrintDialog) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    #[allow(clippy::too_many_arguments)]
    fn StretchDIBits(
        device_context: Handle,
        destination_x: i32,
        destination_y: i32,
        destination_width: i32,
        destination_height: i32,
        source_x: i32,
        source_y: i32,
        source_width: i32,
        source_height: i32,
        bits: *const c_void,
        info: *const BitmapInfo,
        usage: u32,
        operation: u32,
    ) -> i32;
    fn GetDeviceCaps(device_context: Handle, index: i32) -> i32;
    fn StartDocW(device_context: Handle, info: *const DocumentInfo) -> i32;
    fn EndDoc(device_context: Handle) -> i32;
    fn StartPage(device_context: Handle) -> i32;
    fn EndPage(device_context: Handle) -> i32;
    fn DeleteDC(device_context: Handle) -> i32;
    fn AbortDoc(device_context: Handle) -> i32;
    fn CreateDCW(
        driver: *const u16,
        device: *const u16,
        output: *const u16,
        mode: *const c_void,
    ) -> Handle;
}

// The print spooler, which is where the printers themselves are listed.
#[link(name = "winspool")]
extern "system" {
    fn GetDefaultPrinterW(name: *mut u16, size: *mut u32) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn EnumPrintersW(
        flags: u32,
        name: *const u16,
        level: u32,
        buffer: *mut u8,
        size: u32,
        needed: *mut u32,
        returned: *mut u32,
    ) -> i32;
    fn OpenPrinterW(name: *const u16, handle: *mut Handle, defaults: *const c_void) -> i32;
    fn ClosePrinter(handle: Handle) -> i32;
    fn DocumentPropertiesW(
        window: Handle,
        printer: Handle,
        device: *const u16,
        out_mode: *mut u8,
        in_mode: *const u8,
        mode: u32,
    ) -> i32;
    fn DeviceCapabilitiesW(
        device: *const u16,
        port: *const u16,
        capability: u16,
        output: *mut u16,
        mode: *const u8,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn LoadLibraryW(name: *const u16) -> Handle;
    fn GetProcAddress(module: Handle, name: *const u8) -> *const c_void;
    fn GetACP() -> u32;
    fn GetOEMCP() -> u32;
    fn GetLastError() -> u32;
    pub(crate) fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
    pub(crate) fn GlobalFree(memory: Handle) -> Handle;
    pub(crate) fn GlobalLock(memory: Handle) -> *mut c_void;
    pub(crate) fn GlobalUnlock(memory: Handle) -> i32;
    pub(crate) fn GlobalSize(memory: Handle) -> usize;
}

// --- The application, reachable from the window procedure -------------------

thread_local! {
    /// The running application.
    ///
    /// The window procedure is called by the system, so it cannot be handed
    /// state as an argument. A thread-local reaches it without casting pointers
    /// through the window, and a window belongs to its creating thread anyway.
    static APPLICATION: RefCell<Option<Box<dyn App>>> = const { RefCell::new(None) };

    /// Every window this thread owns, in the order they were opened.
    ///
    /// A second window on the same document is a second view of it, not a
    /// second program: one application answers for all of them, and is told
    /// which it is answering for before each event. See [`App::switch_window`].
    static WINDOWS: RefCell<Vec<Handle>> = const { RefCell::new(Vec::new()) };

    /// The window the last event came from, so a dialog is owned by the window
    /// the person is looking at rather than always by the first one.
    static WINDOW: std::cell::Cell<Handle> = const { std::cell::Cell::new(core::ptr::null_mut()) };

    /// How many of each window's pixels one of the application's is: the
    /// screen's density over an ordinary screen's. See [`scale_of`].
    static SCALES: RefCell<std::collections::HashMap<usize, f32>> = RefCell::new(std::collections::HashMap::new());

    /// Where the caret is, for the input method: x, y and height in pixels of
    /// the drawing area.
    static CARET: std::cell::Cell<(i32, i32, i32)> = const { std::cell::Cell::new((0, 0, 16)) };

    /// The first half of a character that takes two UTF-16 units — an emoji
    /// from the emoji panel — waiting for its second half.
    static HIGH_SURROGATE: std::cell::Cell<Option<u16>> = const { std::cell::Cell::new(None) };

    /// What the window was asked to do with itself, waiting to be done.
    ///
    /// See [`window_command`] for why it waits, and [`run_pending_command`] for
    /// where the wait ends.
    static PENDING: std::cell::Cell<Option<(Handle, crate::WindowCommand)>> =
        const { std::cell::Cell::new(None) };
}

/// Which view a window is, by the order it was opened in.
fn window_index(window: Handle) -> usize {
    WINDOWS.with(|slot| slot.borrow().iter().position(|found| *found == window).unwrap_or(0))
}

/// Tells the application which window it is about to answer for.
fn point_at(window: Handle) {
    WINDOW.with(|slot| slot.set(window));
    let index = window_index(window);
    with_application(|app| app.switch_window(index));
}

/// Hands the application to a piece of work, unless it is already in hand.
///
/// # Why it can be already in hand
///
/// Because Windows calls the window procedure from inside the calls this
/// program makes. A file dialog runs a message loop of its own and delivers
/// this window's paints while it is open; changing the window's state sends the
/// new size straight back down. Each of those arrives while the application is
/// already answering something else.
///
/// Nothing here can serve two at once, and a program that asked to would stop
/// dead rather than carry on — so the second is turned away instead. What it is
/// is always a message about the window, never a keystroke or a press: those
/// wait in the queue, which is not read again until this one is done with.
pub(crate) fn with_application<R>(work: impl FnOnce(&mut dyn App) -> R) -> Option<R> {
    APPLICATION.with(|slot| {
        let Ok(mut held) = slot.try_borrow_mut() else { return None };
        held.as_mut().map(|app| work(app.as_mut()))
    })
}

/// How long two clicks may be apart and still count as one double click.
pub(crate) fn double_click_millis() -> u32 {
    // SAFETY: the call reads a setting and touches nothing of ours.
    unsafe { GetDoubleClickTime() }
}

/// What `TrackMouseEvent` is told to watch for.
#[repr(C)]
struct TrackMouse {
    size: u32,
    flags: u32,
    window: Handle,
    hover_time: u32,
}

/// Watch for the pointer leaving.
const TRACK_LEAVE: u32 = 0x0002;

/// Asks to be told when the pointer next leaves the window.
///
/// Windows sends the message once and then forgets, so this is asked again on
/// every movement — which costs nothing and is how it is meant to be used.
unsafe fn track_pointer_leaving(window: Handle) {
    let mut track = TrackMouse {
        size: core::mem::size_of::<TrackMouse>() as u32,
        flags: TRACK_LEAVE,
        window,
        hover_time: 0,
    };
    TrackMouseEvent(&raw mut track);
}

/// How long the caret rests on each side of a blink.
pub(crate) fn caret_blink_millis() -> Option<u32> {
    // SAFETY: the call reads a setting and touches nothing of ours.
    let millis = unsafe { GetCaretBlinkTime() };
    // Zero means the call failed; the "infinite" value means the person has
    // asked for a caret that stays put.
    match millis {
        0 => Some(530),
        u32::MAX => None,
        found => Some(found),
    }
}
/// How many windows are open.
pub(crate) fn window_count() -> usize {
    WINDOWS.with(|slot| slot.borrow().len())
}

/// Encodes a string the way the wide-character API expects, with a terminator.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(core::iter::once(0)).collect()
}

/// Registers the window class, which every window of this program shares.
///
/// Registering it twice is not a failure: the second window of a run finds it
/// already there, and that is the answer the system gives.
unsafe fn register_class() -> Result<Handle, Error> {
    let class_name = wide("WordProcessorWindow");
    let instance = GetModuleHandleW(core::ptr::null());

    let class = WindowClass {
        size: core::mem::size_of::<WindowClass>() as u32,
        style: CLASS_REDRAW_ON_RESIZE,
        procedure: Some(window_procedure),
        class_extra: 0,
        window_extra: 0,
        instance,
        // No class cursor at all: the window answers `WM_SETCURSOR` for
        // itself, because which pointer belongs at a point depends on what
        // the program drew there — an I-beam over the text, an arrow over
        // the ribbon, a resize bar over a pane's edge.
        icon: core::ptr::null_mut(),
        cursor: core::ptr::null_mut(),
        // No background brush: the whole window is painted every time, and
        // letting the system erase it first only causes flicker.
        background: core::ptr::null_mut(),
        menu_name: core::ptr::null(),
        class_name: class_name.as_ptr(),
        small_icon: core::ptr::null_mut(),
    };

    if RegisterClassExW(&class) == 0 {
        let code = GetLastError();
        // 1410 means the class is already registered, which happens for every
        // window after the first and is not a failure.
        if code != 1410 {
            return Err(Error::WindowCreationFailed(format!(
                "registering the window class failed with error {code}"
            )));
        }
    }
    Ok(instance)
}

/// Opens one window and remembers it. The size asked for is in the
/// application's pixels; the window is made as many of its screen's as
/// that comes to.
unsafe fn create_window(title: &str, width: u32, height: u32) -> Result<Handle, Error> {
    let instance = register_class()?;
    let class_name = wide("WordProcessorWindow");
    let title = wide(title);
    let system_scale = system_dpi() / ORDINARY_DPI;

    let window = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        STYLE_OVERLAPPED_WINDOW,
        USE_DEFAULT_POSITION,
        USE_DEFAULT_POSITION,
        (width as f32 * system_scale).round() as i32,
        (height as f32 * system_scale).round() as i32,
        core::ptr::null_mut(),
        core::ptr::null_mut(),
        instance,
        core::ptr::null_mut(),
    );

    if window.is_null() {
        return Err(Error::WindowCreationFailed(format!(
            "creating the window failed with error {}",
            GetLastError()
        )));
    }

    WINDOWS.with(|slot| slot.borrow_mut().push(window));
    WINDOW.with(|slot| slot.set(window));
    // The screen the window came up on decides how big everything is drawn.
    let scale = window_dpi(window) / ORDINARY_DPI;
    SCALES.with(|scales| scales.borrow_mut().insert(window as usize, scale));
    deliver(window, Event::ScaleChanged { scale });

    // Ask for the frame to be worked out again now that the window can
    // answer `WM_NCCALCSIZE` for itself. Without this the frame decided
    // during creation stands, and the system caption drawn in it sits above
    // the title bar this program draws — two title bars, one window.
    SetWindowPos(window, core::ptr::null_mut(), 0, 0, 0, 0, FRAME_CHANGED);
    ShowWindow(window, SHOW_NORMAL);

    // The heartbeat the caret blinks on and the tips are timed by.
    SetTimer(window, TICK_TIMER, TICK_MILLIS, core::ptr::null());
    UpdateWindow(window);
    // Text, pictures and files dragged from other programs land here.
    crate::dragdrop::register_window(window);
    Ok(window)
}

/// Opens another window onto the same document.
pub(crate) fn open_window(title: &str) -> bool {
    // SAFETY: the class is already registered and the handle is remembered by
    // `create_window`, which is the only thing that makes one.
    unsafe { create_window(title, 1400, 900).is_ok() }
}

/// Opens the window and runs the event loop.
pub(crate) fn run(options: WindowOptions, app: Box<dyn App>) -> Result<(), Error> {
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(app));
    // SAFETY: called once, before any window, which is when the system
    // allows it.
    unsafe {
        declare_dpi_awareness();
    }

    // SAFETY: every pointer passed below either points at a local that outlives
    // the call, or is null where the API documents null as meaningful.
    unsafe {
        create_window(&options.title, options.width, options.height)?;

        let mut message = core::mem::zeroed::<Message>();
        loop {
            let outcome = GetMessageW(&mut message, core::ptr::null_mut(), 0, 0);
            // Zero means the quit message; negative means an error, and
            // continuing to loop on one would spin forever.
            if outcome <= 0 {
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    WINDOWS.with(|slot| slot.borrow_mut().clear());
    WINDOW.with(|slot| slot.set(core::ptr::null_mut()));
    APPLICATION.with(|slot| slot.borrow_mut().take());
    Ok(())
}

/// Lays the open windows out side by side across the work area.
///
/// Word calls this Arrange All and stacks them; side by side is what two views
/// of one document are actually for — reading one part while writing another —
/// and is what a wide screen has room for.
pub(crate) fn arrange_windows() -> usize {
    let windows = WINDOWS.with(|slot| slot.borrow().clone());
    if windows.is_empty() {
        return 0;
    }

    // SAFETY: the handles are this thread's own windows, and the work area
    // comes back from the system into a local that outlives the call.
    unsafe {
        let mut area = Rect { left: 0, top: 0, right: 0, bottom: 0 };
        // The work area rather than the whole screen, so nothing is put under
        // the taskbar.
        if SystemParametersInfoW(SPI_GET_WORK_AREA, 0, (&raw mut area).cast(), 0) == 0 {
            return 0;
        }

        let count = windows.len() as i32;
        let width = ((area.right - area.left) / count).max(200);
        let height = (area.bottom - area.top).max(200);
        for (index, window) in windows.iter().enumerate() {
            // A maximised window cannot be moved until it is restored.
            if GetWindowLongW(*window, -16) as u32 & WINDOW_MAXIMIZED != 0 {
                ShowWindow(*window, SHOW_RESTORED);
            }
            SetWindowPos(
                *window,
                core::ptr::null_mut(),
                area.left + width * index as i32,
                area.top,
                width,
                height,
                MOVE_AND_SIZE,
            );
        }
        windows.len()
    }
}
/// Tells the input method where the caret is: its composition window, if it
/// insists on one, goes there, and its candidate list keeps clear of the
/// line the caret is on.
pub(crate) fn place_composition(x: i32, y: i32, height: i32) {
    let window = owner_window();
    if window.is_null() {
        return;
    }
    let (x, y) = to_device(window, (x, y));
    let height = (height as f32 * scale_of(window)).round() as i32;
    CARET.with(|slot| slot.set((x, y, height)));
    // SAFETY: the window is one this thread made; the forms outlive the
    // calls, which copy what they need.
    unsafe {
        let context = ImmGetContext(window);
        if context.is_null() {
            return;
        }
        let composition =
            CompositionForm { style: CFS_POINT, current: Point { x, y }, area: Rect::default() };
        ImmSetCompositionWindow(context, &composition);
        let candidate = CandidateForm {
            index: 0,
            style: CFS_EXCLUDE,
            current: Point { x, y: y + height },
            area: Rect { left: x, top: y, right: x + 1, bottom: y + height },
        };
        ImmSetCandidateWindow(context, &candidate);
        ImmReleaseContext(window, context);
    }
}

/// One of the composition's strings, as text.
unsafe fn composition_string(context: Handle, which: isize) -> String {
    let bytes = ImmGetCompositionStringW(context, which as u32, core::ptr::null_mut(), 0);
    if bytes <= 0 {
        return String::new();
    }
    let mut units = vec![0u16; (bytes as usize).div_ceil(2)];
    let got =
        ImmGetCompositionStringW(context, which as u32, units.as_mut_ptr().cast(), bytes as u32);
    units.truncate((got.max(0) as usize) / 2);
    String::from_utf16_lossy(&units)
}

/// How each character of the composition stands, one attribute per
/// character of the text — the input method gives one per UTF-16 unit.
unsafe fn composition_attributes(
    context: Handle,
    units: &[u16],
    which: isize,
) -> Vec<CompositionAttribute> {
    let bytes = ImmGetCompositionStringW(context, which as u32, core::ptr::null_mut(), 0);
    let mut raw = vec![0u8; bytes.max(0) as usize];
    if bytes > 0 {
        ImmGetCompositionStringW(context, which as u32, raw.as_mut_ptr().cast(), bytes as u32);
    }
    let mut attributes = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        let attribute = match raw.get(index).copied().unwrap_or(0) {
            1 | 3 => CompositionAttribute::Target,
            2 => CompositionAttribute::Converted,
            4 => CompositionAttribute::Error,
            _ => CompositionAttribute::Input,
        };
        attributes.push(attribute);
        // A pair of units is one character.
        index += if (0xD800..=0xDBFF).contains(&units[index]) { 2 } else { 1 };
    }
    attributes
}

/// The window this thread owns, for a dialog to be modal to.
pub(crate) fn owner_window() -> Handle {
    WINDOW.with(std::cell::Cell::get)
}

/// Hands an event to the application and acts on what it asks for.
pub(crate) fn deliver(window: Handle, event: Event) -> Result_ {
    // Which of the program's windows this is, so the application can put that
    // window's view back before it answers.
    point_at(window);
    let response = with_application(|app| app.handle(event)).unwrap_or(Response::Ignored);

    match response {
        Response::Redraw => {
            // SAFETY: the window handle comes from the system, which is calling
            // us about that very window.
            unsafe { InvalidateRect(window, core::ptr::null(), 0) };
        }
        Response::Close => {
            unsafe { DestroyWindow(window) };
        }
        Response::Ignored | Response::Refuse => {}
    }

    // Last of all, because the application is no longer in hand here and
    // because what it asks for changes the window itself.
    run_pending_command();
    0
}

thread_local! {
    /// Whether Alt has gone down and nothing has been pressed since.
    ///
    /// Alt on its own is a command — it is what shows the letters over the
    /// ribbon — but Alt held while something else is pressed is a shortcut and
    /// means nothing of the kind. The only way to tell them apart is to watch
    /// what happens between the key going down and coming up again.
    static ALT_ALONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// The same for Control, and for the same reason.
    ///
    /// Word's paste options open when Control is pressed and let go with
    /// nothing in between, which is not the same key as the Control in
    /// Ctrl+S. Telling them apart takes watching what happens while it is
    /// held, exactly as Alt does.
    static CONTROL_ALONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The window procedure, which the system calls for every message.
unsafe extern "system" fn window_procedure(
    window: Handle,
    message: u32,
    word: WordParam,
    long: LongParam,
) -> Result_ {
    match message {
        // The window draws its own caption, so the system is asked to reserve
        // no room for one. Word does the same, and it is why the title bar can
        // hold buttons at all.
        // The whole window is client area: there is no system frame, because the
        // program draws its own. Answered for both forms of the message — the
        // one that asks for a rectangle and the one that asks for a size —
        // because leaving either to the system brings the caption back.
        MESSAGE_NON_CLIENT_CALC_SIZE => {
            if word == 0 {
                return 0;
            }
            // When maximised, Windows sizes the window past the edges of the
            // screen by the width of the frame; without pulling the client area
            // back in by that much, the top of the ribbon would be off-screen.
            if GetWindowLongW(window, -16) as u32 & WINDOW_MAXIMIZED != 0 {
                let dpi = window_dpi(window) as u32;
                let frame_x = metric_for_dpi(32, dpi) + metric_for_dpi(92, dpi);
                let frame_y = metric_for_dpi(33, dpi) + metric_for_dpi(92, dpi);
                let area = long as *mut Rect;
                (*area).left += frame_x;
                (*area).right -= frame_x;
                (*area).top += frame_y;
                (*area).bottom -= frame_y;
            }
            0
        }
        // Nothing outside the client area, so there is no frame to paint. Said
        // explicitly so that a restore or a theme change cannot get one drawn.
        MESSAGE_NON_CLIENT_PAINT => 0,
        // Activating and deactivating repaints the caption, which this window
        // does not have. Passing -1 for the region is what tells the system
        // there is nothing of its own to redraw.
        MESSAGE_NON_CLIENT_ACTIVATE => DefWindowProcW(window, message, word, -1),
        MESSAGE_NON_CLIENT_HIT_TEST => hit_test(window, long),
        MESSAGE_SET_CURSOR if long & 0xFFFF == HIT_CLIENT_AREA => {
            if set_cursor_for_pointer(window) {
                1
            } else {
                DefWindowProcW(window, message, word, long)
            }
        }
        MESSAGE_TIMER if word == TICK_TIMER => deliver(window, Event::Tick),
        MESSAGE_PAINT => {
            paint(window);
            0
        }
        // Claiming the background is already erased stops the system painting
        // it grey a moment before the real contents appear.
        MESSAGE_ERASE_BACKGROUND => 1,
        MESSAGE_SIZE => {
            let width = (long & 0xFFFF) as u32;
            let height = ((long >> 16) & 0xFFFF) as u32;
            // A window on its way to the taskbar reports no size at all. There
            // is nothing to lay a page out in and nobody to show it to, and
            // Word leaves the view as it was until the window comes back — so
            // the size is not passed on, and the one it had is what it has when
            // it is restored.
            if word == SIZE_MINIMISED || width == 0 || height == 0 {
                return 0;
            }
            let scale = scale_of(window);
            let width = (width as f32 / scale).round().max(1.0) as u32;
            let height = (height as f32 / scale).round().max(1.0) as u32;
            deliver(window, Event::Resized { width, height })
        }
        // Moved to a screen of another density: the system says how big the
        // window should now be, and everything in it is drawn again at the
        // new scale.
        MESSAGE_DPI_CHANGED => {
            let dpi = (word & 0xFFFF) as f32;
            let scale = if dpi > 0.0 { dpi / ORDINARY_DPI } else { 1.0 };
            SCALES.with(|scales| scales.borrow_mut().insert(window as usize, scale));
            let suggested = long as *const Rect;
            if !suggested.is_null() {
                let area = *suggested;
                SetWindowPos(
                    window,
                    core::ptr::null_mut(),
                    area.left,
                    area.top,
                    area.right - area.left,
                    area.bottom - area.top,
                    MOVE_AND_SIZE,
                );
            }
            deliver(window, Event::ScaleChanged { scale })
        }
        MESSAGE_MOUSE_WHEEL => {
            // The delta arrives in the high half of the word parameter, as a
            // signed value in units of 120 per notch.
            let delta = ((word >> 16) as u16) as i16;
            deliver(
                window,
                Event::Scroll { lines: f32::from(delta) / WHEEL_STEP, modifiers: modifiers() },
            )
        }
        // Alt on its own shows the letters over the ribbon; Alt with something
        // else is a shortcut and shows nothing. Which it was is only known when
        // the key comes back up.
        MESSAGE_SYSTEM_KEY_DOWN => {
            let code = word as u32;
            ALT_ALONE.with(|alone| alone.set(code == KEY_ALT as u32));
            if code == KEY_ALT as u32 {
                return 0;
            }
            // Alt with another key is a shortcut like any other, so it goes on
            // as one: Windows sends these here rather than with the ordinary
            // keys, which is why Ctrl+Alt+1 would otherwise never arrive.
            match key_from_code(code) {
                Some(key) => deliver(window, Event::KeyDown { key, modifiers: modifiers() }),
                None => DefWindowProcW(window, message, word, long),
            }
        }
        MESSAGE_SYSTEM_KEY_UP if word as i32 == KEY_ALT => {
            if ALT_ALONE.with(|alone| alone.replace(false)) {
                return deliver(window, Event::MenuKey);
            }
            0
        }
        // Swallowed, or Windows sounds the "no such menu" beep at every Alt.
        MESSAGE_SYSTEM_CHAR => 0,
        MESSAGE_KEY_DOWN => {
            ALT_ALONE.with(|alone| alone.set(false));
            // Control going down starts the watch; anything else going down
            // ends it, because then Control is a modifier and not a gesture.
            CONTROL_ALONE.with(|alone| alone.set(word as i32 == KEY_CONTROL));
            match key_from_code(word as u32) {
                Some(key) => deliver(window, Event::KeyDown { key, modifiers: modifiers() }),
                None => DefWindowProcW(window, message, word, long),
            }
        }
        MESSAGE_KEY_UP if word as i32 == KEY_CONTROL => {
            if CONTROL_ALONE.with(|alone| alone.replace(false)) {
                return deliver(window, Event::ControlKey);
            }
            0
        }
        MESSAGE_CHAR => {
            // A character past the basic plane comes as two messages, one
            // half each; the first is kept until the second arrives.
            let unit = word as u16;
            let code = match (HIGH_SURROGATE.with(std::cell::Cell::take), unit) {
                (Some(high), 0xDC00..=0xDFFF) => {
                    0x1_0000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(unit) - 0xDC00)
                }
                (_, 0xD800..=0xDBFF) => {
                    HIGH_SURROGATE.with(|slot| slot.set(Some(unit)));
                    return 0;
                }
                _ => u32::from(unit),
            };
            match char::from_u32(code) {
                // Control characters arrive here too; they are not text.
                Some(character) if !character.is_control() => {
                    deliver(window, Event::Char(character))
                }
                _ => 0,
            }
        }
        // The composition is drawn in the document, so the input method is
        // told not to draw it in a window of its own.
        MESSAGE_IME_SET_CONTEXT => {
            DefWindowProcW(window, message, word, long & !ISC_SHOW_UI_COMPOSITION_WINDOW)
        }
        MESSAGE_IME_START_COMPOSITION => deliver(
            window,
            Event::Compose { text: String::new(), caret: 0, attributes: Vec::new() },
        ),
        MESSAGE_IME_COMPOSITION => {
            let context = ImmGetContext(window);
            if context.is_null() {
                return DefWindowProcW(window, message, word, long);
            }
            if long & GCS_RESULTSTR != 0 {
                let result = composition_string(context, GCS_RESULTSTR);
                if !result.is_empty() {
                    deliver(window, Event::Commit(result));
                }
            }
            if long & GCS_COMPSTR != 0 || long == 0 {
                let text = composition_string(context, GCS_COMPSTR);
                let units: Vec<u16> = text.encode_utf16().collect();
                let cursor = ImmGetCompositionStringW(
                    context,
                    GCS_CURSORPOS as u32,
                    core::ptr::null_mut(),
                    0,
                )
                .max(0) as usize;
                let attributes = composition_attributes(context, &units, GCS_COMPATTR);
                let caret = char::decode_utf16(units.iter().copied().take(cursor)).count();
                deliver(window, Event::Compose { text, caret, attributes });
            }
            ImmReleaseContext(window, context);
            0
        }
        MESSAGE_IME_END_COMPOSITION => deliver(window, Event::ComposeEnd),
        MESSAGE_IME_REQUEST if word == IMR_QUERY_CHAR_POSITION => {
            // Where the character being composed is on screen: the caret,
            // since the composition is drawn at the caret.
            let position = long as *mut CharacterPosition;
            if position.is_null() {
                return 0;
            }
            let (x, y, height) = CARET.with(std::cell::Cell::get);
            let mut point = Point { x, y };
            ClientToScreen(window, &mut point);
            let mut area = Rect::default();
            GetClientRect(window, &mut area);
            let mut corner = Point { x: area.left, y: area.top };
            ClientToScreen(window, &mut corner);
            (*position).point = point;
            (*position).line_height = height.max(1) as u32;
            (*position).document = Rect {
                left: corner.x,
                top: corner.y,
                right: corner.x + area.right - area.left,
                bottom: corner.y + area.bottom - area.top,
            };
            1
        }
        MESSAGE_LEFT_BUTTON_DOWN => {
            let (x, y) = mouse_point(window, long);
            // Capturing the mouse keeps the messages coming even when the
            // pointer is dragged outside the window, so a selection that runs
            // off the edge does not stop growing there.
            SetCapture(window);
            deliver(window, Event::MouseDown { x, y, modifiers: modifiers() })
        }
        MESSAGE_MOUSE_MOVE => {
            let (x, y) = mouse_point(window, long);
            track_pointer_leaving(window);
            deliver(
                window,
                Event::MouseMove {
                    x,
                    y,
                    held: word & LEFT_BUTTON_HELD != 0,
                    modifiers: modifiers(),
                },
            )
        }
        MESSAGE_MOUSE_LEAVE => deliver(window, Event::PointerLeft),
        MESSAGE_MIDDLE_BUTTON_DOWN => {
            let (x, y) = mouse_point(window, long);
            deliver(window, Event::MiddleClick { x, y })
        }
        MESSAGE_LEFT_BUTTON_DOUBLE_CLICK => {
            let (x, y) = mouse_point(window, long);
            deliver(window, Event::DoubleClick { x, y })
        }
        // The right button opens the menu on the way up, which is where
        // Windows puts a context menu and where a person expects it: pressing
        // and thinking better of it should open nothing.
        MESSAGE_RIGHT_BUTTON_DOWN => 0,
        MESSAGE_RIGHT_BUTTON_UP => {
            let (x, y) = mouse_point(window, long);
            deliver(window, Event::RightClick { x, y, modifiers: modifiers() })
        }
        MESSAGE_LEFT_BUTTON_UP => {
            let (x, y) = mouse_point(window, long);
            ReleaseCapture();
            deliver(window, Event::MouseUp { x, y })
        }
        MESSAGE_CLOSE => {
            // The application gets to refuse, which is what lets it ask about
            // unsaved changes and act on "cancel".
            point_at(window);
            let response =
                with_application(|app| app.handle(Event::Closing)).unwrap_or(Response::Ignored);
            if response != Response::Refuse {
                DestroyWindow(window);
            }
            0
        }
        // A screen reader asking for the window's own description of itself.
        MESSAGE_GET_OBJECT if long == crate::uia::ROOT_OBJECT_ID => {
            crate::uia::root_provider(window, word, long)
        }
        MESSAGE_DESTROY => {
            KillTimer(window, TICK_TIMER);
            crate::dragdrop::unregister_window(window);
            crate::uia::forget_window(window);
            SCALES.with(|scales| scales.borrow_mut().remove(&(window as usize)));
            // One window closing is one view closing. The program ends when the
            // last of them goes, not the first.
            WINDOWS.with(|slot| slot.borrow_mut().retain(|found| *found != window));
            if window_count() == 0 {
                PostQuitMessage(0);
            }
            0
        }
        _ => DefWindowProcW(window, message, word, long),
    }
}

/// Asks the application for an image and puts it on the screen.
fn paint(window: Handle) {
    point_at(window);
    // SAFETY: the handle is the window the system is asking us to paint.
    unsafe {
        let mut area = Rect::default();
        GetClientRect(window, &mut area);
        let width = (area.right - area.left).max(1) as usize;
        let height = (area.bottom - area.top).max(1) as usize;

        let mut paint_struct = core::mem::zeroed::<PaintStruct>();
        let device_context = BeginPaint(window, &mut paint_struct);

        let pixels = with_application(|app| app.draw(width, height).to_bgra());

        if let Some(pixels) = pixels {
            // A negative height means the rows run top to bottom, which is the
            // order a canvas stores them in. Without it the image is upside
            // down.
            let info = BitmapInfo {
                header: BitmapInfoHeader {
                    size: core::mem::size_of::<BitmapInfoHeader>() as u32,
                    width: width as i32,
                    height: -(height as i32),
                    planes: 1,
                    bit_count: 32,
                    compression: BITMAP_UNCOMPRESSED,
                    image_size: 0,
                    x_pixels_per_meter: 0,
                    y_pixels_per_meter: 0,
                    colors_used: 0,
                    colors_important: 0,
                },
                colors: [0; 3],
            };

            StretchDIBits(
                device_context,
                0,
                0,
                width as i32,
                height as i32,
                0,
                0,
                width as i32,
                height as i32,
                pixels.as_ptr().cast(),
                &info,
                BITMAP_RGB_COLORS,
                COPY_SOURCE,
            );
        }

        EndPaint(window, &paint_struct);
    }
}

/// Says what part of the window a point is on.
///
/// # Why this has to be answered by hand
///
/// A window that draws its own caption has told the system there is no caption
/// to speak of. The system then has no idea which part of it can be dragged,
/// and which parts resize it — so it asks, on every pointer movement, and the
/// answer here is what makes dragging, snapping, double-click-to-maximise and
/// every resize edge keep working exactly as they do for any other window.
unsafe fn hit_test(window: Handle, long: LongParam) -> Result_ {
    let mut point = Point {
        x: (long & 0xFFFF) as u16 as i16 as i32,
        y: ((long >> 16) & 0xFFFF) as u16 as i16 as i32,
    };
    ScreenToClient(window, &mut point);

    let mut area = Rect::default();
    GetClientRect(window, &mut area);
    let maximised = GetWindowLongW(window, -16) as u32 & WINDOW_MAXIMIZED != 0;

    // A maximised window has no edges to drag: it is already as big as it goes.
    if !maximised {
        let left = point.x < RESIZE_BORDER;
        let right = point.x >= area.right - RESIZE_BORDER;
        let top = point.y < RESIZE_BORDER;
        let bottom = point.y >= area.bottom - RESIZE_BORDER;

        match (top, bottom, left, right) {
            (true, _, true, _) => return HIT_TOP_LEFT,
            (true, _, _, true) => return HIT_TOP_RIGHT,
            (_, true, true, _) => return HIT_BOTTOM_LEFT,
            (_, true, _, true) => return HIT_BOTTOM_RIGHT,
            (true, ..) => return HIT_TOP,
            (_, true, ..) => return HIT_BOTTOM,
            (_, _, true, _) => return HIT_LEFT,
            (_, _, _, true) => return HIT_RIGHT,
            _ => {}
        }
    }

    // The application says which part of what it drew is the caption — the
    // empty stretch of the title bar, and not the buttons on it.
    let (x, y) = to_logical(window, (point.x, point.y));
    let draggable = with_application(|app| app.is_caption(x, y)).unwrap_or(false);

    if draggable {
        HIT_CAPTION
    } else {
        HIT_CLIENT
    }
}

/// Minimises, maximises, restores or closes the window.
///
/// # Why it is remembered rather than carried out
///
/// Because this is called from inside the application, in the middle of the
/// press on the button that asked for it, and two things about that moment
/// make the change impossible to make there.
///
/// The application is borrowed while it answers. Minimising a window makes the
/// system call the window procedure again then and there — with the new size,
/// the new position, and the paint that follows — and every one of those wants
/// to reach the application that is already in hand. Borrowing it twice is
/// where a program stops dead, and the window then goes as though it had been
/// closed.
///
/// And the press has the mouse captured, so that a selection dragged off the
/// edge of the window keeps growing. A window holding the capture is in the
/// middle of a gesture as far as the system is concerned, and a request to
/// minimise or maximise sent to it in that state is declined — which is a
/// button that does nothing at all.
///
/// So the request is remembered, and carried out by
/// [`run_pending_command`] once the application has finished answering and the
/// capture is let go.
pub(crate) fn window_command(command: crate::WindowCommand) {
    let window = owner_window();
    if window.is_null() {
        return;
    }
    PENDING.with(|slot| slot.set(Some((window, command))));
}

/// Carries out the window command the application asked for, if it asked.
///
/// Called after the application has answered an event, which is the first
/// moment at which the window's own state can be changed: see
/// [`window_command`] for what is wrong with the moment before it.
fn run_pending_command() {
    let Some((window, command)) = PENDING.with(std::cell::Cell::take) else { return };
    // SAFETY: the handle is this thread's window. Taking the request out of the
    // slot before acting on it is what keeps the calls below — which deliver
    // the new size straight back down — from finding it again and looping.
    unsafe {
        // The gesture is over: whatever the press captured the mouse for, it is
        // not carrying on through a window that is about to change shape. Held
        // capture is also the reason the system declines to minimise.
        ReleaseCapture();
        match command {
            crate::WindowCommand::Minimise => {
                ShowWindow(window, SHOW_MINIMIZED);
            }
            crate::WindowCommand::ToggleMaximise => {
                let maximised = GetWindowLongW(window, -16) as u32 & WINDOW_MAXIMIZED != 0;
                ShowWindow(window, if maximised { SHOW_RESTORED } else { SHOW_MAXIMIZED });
            }
            crate::WindowCommand::Close => {
                // Through the close message, so the application is asked about
                // unsaved changes exactly as it is for the system's own button.
                PostMessageW(window, MESSAGE_CLOSE, 0, 0);
            }
        }
    }
}

/// Whether the window is maximised, so its button can show which it is.
#[must_use]
pub(crate) fn is_maximised() -> bool {
    let window = owner_window();
    if window.is_null() {
        return false;
    }
    // SAFETY: the handle is this thread's window.
    unsafe { GetWindowLongW(window, -16) as u32 & WINDOW_MAXIMIZED != 0 }
}

/// Unpacks the pointer position a mouse message carries.
///
/// Both halves are signed: a captured drag reports positions outside the window,
/// and reading them as unsigned turns a pointer just off the left edge into one
/// tens of thousands of pixels to the right.
fn mouse_point(window: Handle, long: LongParam) -> (i32, i32) {
    let x = (long & 0xFFFF) as u16 as i16;
    let y = ((long >> 16) & 0xFFFF) as u16 as i16;
    to_logical(window, (i32::from(x), i32::from(y)))
}

/// A point in the window's pixels as one in the application's.
pub(crate) fn to_logical(window: Handle, (x, y): (i32, i32)) -> (i32, i32) {
    let scale = scale_of(window);
    if scale == 1.0 {
        (x, y)
    } else {
        ((x as f32 / scale).round() as i32, (y as f32 / scale).round() as i32)
    }
}

/// A point in the application's pixels as one in the window's.
pub(crate) fn to_device(window: Handle, (x, y): (i32, i32)) -> (i32, i32) {
    let scale = scale_of(window);
    if scale == 1.0 {
        (x, y)
    } else {
        ((x as f32 * scale).round() as i32, (y as f32 * scale).round() as i32)
    }
}

/// How many of a window's pixels one of the application's is.
pub(crate) fn scale_of(window: Handle) -> f32 {
    SCALES.with(|scales| scales.borrow().get(&(window as usize)).copied().unwrap_or(1.0))
}

/// A function of a library this program does not link against, found by name.
///
/// The library is loaded if it is not loaded already and is then left where
/// it is: the pointers taken out of it are kept for the life of the process,
/// and a library that were unloaded under them would turn every one of them
/// into a crash.
pub(crate) unsafe fn library_function(library: &str, name: &[u8]) -> *const c_void {
    let module = LoadLibraryW(wide(library).as_ptr());
    if module.is_null() {
        return core::ptr::null();
    }
    GetProcAddress(module, name.as_ptr())
}

/// A function of user32 that only newer versions of Windows have, found by
/// name so that the program still starts on the older ones.
unsafe fn user32_function(name: &[u8]) -> *const c_void {
    let module = GetModuleHandleW(wide("user32.dll").as_ptr());
    if module.is_null() {
        return core::ptr::null();
    }
    GetProcAddress(module, name.as_ptr())
}

/// Tells the system this program draws for each screen's density itself,
/// so that it is not stretched like a picture on a dense screen. The newer
/// way where there is one, the older where there is not.
unsafe fn declare_dpi_awareness() {
    let newer = user32_function(b"SetProcessDpiAwarenessContext ");
    if !newer.is_null() {
        let set: unsafe extern "system" fn(isize) -> i32 = core::mem::transmute(newer);
        if set(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) != 0 {
            return;
        }
    }
    SetProcessDPIAware();
}

/// The density of the screen a window is on, in dots to the inch.
unsafe fn window_dpi(window: Handle) -> f32 {
    let function = user32_function(b"GetDpiForWindow ");
    if function.is_null() {
        return system_dpi();
    }
    let get: unsafe extern "system" fn(Handle) -> u32 = core::mem::transmute(function);
    let dpi = get(window);
    if dpi == 0 {
        ORDINARY_DPI
    } else {
        dpi as f32
    }
}

/// The density of the main screen.
unsafe fn system_dpi() -> f32 {
    let function = user32_function(b"GetDpiForSystem ");
    if function.is_null() {
        return ORDINARY_DPI;
    }
    let get: unsafe extern "system" fn() -> u32 = core::mem::transmute(function);
    let dpi = get();
    if dpi == 0 {
        ORDINARY_DPI
    } else {
        dpi as f32
    }
}

/// A system metric at a given density, where the system can say.
unsafe fn metric_for_dpi(index: i32, dpi: u32) -> i32 {
    let function = user32_function(b"GetSystemMetricsForDpi ");
    if function.is_null() {
        return GetSystemMetrics(index);
    }
    let get: unsafe extern "system" fn(i32, u32) -> i32 = core::mem::transmute(function);
    get(index, dpi)
}

/// Which modifier keys are held down right now.
///
/// Read at the moment the key arrives rather than tracked separately: the
/// system already knows, and a separately maintained copy goes wrong the first
/// time the window loses focus while a key is held.
fn modifiers() -> Modifiers {
    // The high bit of the state means the key is down.
    let held = |key: i32| unsafe { GetKeyState(key) } < 0;
    Modifiers { control: held(KEY_CONTROL), shift: held(KEY_SHIFT), alt: held(KEY_ALT) }
}

// --- Dialogs ----------------------------------------------------------------

/// Sets the window's caption.
pub(crate) fn set_window_title(title: &str) {
    let window = owner_window();
    if window.is_null() {
        return;
    }
    let text = wide(title);
    // SAFETY: the handle is this thread's window, and the string outlives the
    // call, which copies it.
    unsafe {
        SetWindowTextW(window, text.as_ptr());
    }
}

/// Builds the filter string the dialogs expect.
///
/// Pairs of label and pattern, each ended by a zero, and the whole list ended by
/// another — a shape that predates every convention this program otherwise
/// follows, and has to be produced exactly.
fn filter_string(filters: &[crate::dialog::FileFilter]) -> Vec<u16> {
    let mut out = Vec::new();
    for filter in filters {
        out.extend(filter.label.encode_utf16());
        out.push(0);
        out.extend(filter.pattern.encode_utf16());
        out.push(0);
    }
    out.push(0);
    out
}

/// Shows the system's open or save dialog and returns what was chosen.
pub(crate) fn choose_file(
    title: &str,
    filters: &[crate::dialog::FileFilter],
    suggested: Option<&std::path::Path>,
    saving: bool,
) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let filter = filter_string(filters);
    let title = wide(title);

    // The type list opens on the type the suggested name has, which is how
    // Word's Save As opens on "Word Template" for a template: the list and the
    // name agree, and a person who changes neither gets what they had.
    let starting_filter = suggested
        .and_then(|path| path.extension())
        .and_then(|extension| extension.to_str())
        .and_then(|extension| {
            filters.iter().position(|filter| filter_has_extension(filter.pattern, extension))
        })
        .map_or(1, |index| index as u32 + 1);

    let mut buffer = vec![0u16; PATH_BUFFER];
    if let Some(path) = suggested {
        // The dialog both reads the starting name from this buffer and writes
        // the answer back into it, so the suggestion goes in here.
        let encoded: Vec<u16> = path.as_os_str().encode_wide().collect();
        if encoded.len() < buffer.len() {
            buffer[..encoded.len()].copy_from_slice(&encoded);
        }
    }

    let mut arguments = OpenFileName {
        size: core::mem::size_of::<OpenFileName>() as u32,
        owner: owner_window(),
        instance: core::ptr::null_mut(),
        filter: filter.as_ptr(),
        custom_filter: core::ptr::null_mut(),
        max_custom_filter: 0,
        filter_index: starting_filter,
        file: buffer.as_mut_ptr(),
        max_file: buffer.len() as u32,
        file_title: core::ptr::null_mut(),
        max_file_title: 0,
        initial_directory: core::ptr::null(),
        title: title.as_ptr(),
        flags: OFN_EXPLORER
            | OFN_HIDE_READ_ONLY
            | OFN_NO_CHANGE_DIR
            | OFN_PATH_MUST_EXIST
            | if saving { OFN_OVERWRITE_PROMPT } else { OFN_FILE_MUST_EXIST },
        file_offset: 0,
        file_extension: 0,
        // No extension is filled in by the system: the one the chosen type
        // calls for is put on below, which the system cannot do for a list
        // of several types.
        default_extension: core::ptr::null(),
        custom_data: 0,
        hook: core::ptr::null_mut(),
        template_name: core::ptr::null(),
        reserved_pointer: core::ptr::null_mut(),
        reserved: 0,
        flags_extended: 0,
    };

    // SAFETY: every pointer in the structure points at a local that outlives
    // the call, the buffer is as long as `max_file` claims, and `size` is the
    // structure's real size, which is how the system reads its fields.
    let chosen = unsafe {
        if saving {
            GetSaveFileNameW(&mut arguments)
        } else {
            GetOpenFileNameW(&mut arguments)
        }
    };
    if chosen == 0 {
        // Zero also means an error, but there is nothing useful to say about
        // one here: either way, no file was chosen.
        return None;
    }

    let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    if length == 0 {
        return None;
    }
    let mut path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]));

    // A name typed without an extension gets the extension of the type that
    // was chosen in the list, which is what "Save as type" means.
    if saving && path.extension().is_none() {
        let chosen = filters.get(arguments.filter_index.saturating_sub(1) as usize);
        if let Some(extension) = chosen.and_then(|filter| first_extension(filter.pattern)) {
            path.set_extension(extension);
        }
    }
    Some(path)
}

/// Whether a filter's pattern - `*.docx`, or `*.dotx;*.dotm` - names an
/// extension.
fn filter_has_extension(pattern: &str, extension: &str) -> bool {
    pattern.split(';').any(|one| {
        one.trim().strip_prefix("*.").is_some_and(|named| named.eq_ignore_ascii_case(extension))
    })
}

/// The first extension a filter's pattern names, if it names one rather than
/// everything.
fn first_extension(pattern: &str) -> Option<&str> {
    pattern.split(';').find_map(|one| {
        let named = one.trim().strip_prefix("*.")?;
        (!named.is_empty() && named != "*").then_some(named)
    })
}

/// Asks whether to save changes, and what to do if not.
pub(crate) fn ask_to_save(name: &str) -> crate::dialog::Answer {
    let text = wide(&format!("Save the changes to {name}?"));
    let caption = wide("Word Processor");

    // SAFETY: both strings outlive the call, which copies what it needs.
    let answer = unsafe {
        MessageBoxW(
            owner_window(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_YES_NO_CANCEL | MB_ICON_WARNING,
        )
    };

    match answer {
        ID_YES => crate::dialog::Answer::Yes,
        ID_NO => crate::dialog::Answer::No,
        ID_CANCEL => crate::dialog::Answer::Cancel,
        // A dialog that could not be shown must not be read as permission to
        // throw the user's work away.
        _ => crate::dialog::Answer::Cancel,
    }
}

/// Asks a question with two answers. Anything but yes is no, because a
/// dialog that could not be shown is not consent.
pub(crate) fn ask_yes_no(question: &str) -> bool {
    let text = wide(question);
    let caption = wide("Word Processor");
    // SAFETY: both strings outlive the call, which copies what it needs.
    let answer = unsafe {
        MessageBoxW(owner_window(), text.as_ptr(), caption.as_ptr(), MB_YES_NO | MB_ICON_WARNING)
    };
    answer == ID_YES
}

/// Tells the user something and asks whether to go on. Anything but OK is
/// cancel.
pub(crate) fn ask_ok_cancel(message: &str) -> bool {
    let text = wide(message);
    let caption = wide("Word Processor");
    // SAFETY: both strings outlive the call, which copies what it needs.
    let answer = unsafe {
        MessageBoxW(
            owner_window(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK_CANCEL | MB_ICON_INFORMATION,
        )
    };
    answer == ID_OK
}

/// The code pages this machine writes text by: the Windows one and the DOS
/// one, which are what "Windows (Default)" and "MS-DOS" mean in Word's File
/// Conversion dialog.
pub(crate) fn system_code_pages() -> (u32, u32) {
    // SAFETY: both calls take nothing and return a number.
    unsafe { (GetACP(), GetOEMCP()) }
}

/// Shows a message the user has to acknowledge.
pub(crate) fn show_error(message: &str) {
    let text = wide(message);
    let caption = wide("Word Processor");
    // SAFETY: both strings outlive the call.
    unsafe {
        MessageBoxW(owner_window(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICON_ERROR);
    }
}

// --- Printing ---------------------------------------------------------------

/// The name of every printer this machine can reach.
///
/// Word lists them in the Print page rather than making a person go through the
/// system's own dialog for them, and a person choosing a printer is choosing
/// between names.
pub(crate) fn printer_names() -> Vec<String> {
    /// Printers installed on this machine, and printers on other machines this
    /// one has been connected to.
    const LOCAL: u32 = 0x0000_0002;
    const CONNECTIONS: u32 = 0x0000_0004;
    /// The level of detail asked for: `PRINTER_INFO_4`, which is the name, the
    /// server it lives on, and its attributes — the cheapest one to gather,
    /// because it does not open any of them.
    const LEVEL: u32 = 4;

    let mut needed = 0u32;
    let mut returned = 0u32;

    // SAFETY: the first call is the documented way of asking how much room the
    // answer needs; it is expected to fail and fills in `needed`.
    unsafe {
        EnumPrintersW(
            LOCAL | CONNECTIONS,
            core::ptr::null(),
            LEVEL,
            core::ptr::null_mut(),
            0,
            &mut needed,
            &mut returned,
        );
    }
    if needed == 0 {
        return Vec::new();
    }

    // The answer is a run of structures at the front of the buffer and the
    // strings they point at at the back of it, so it has to be read where it
    // was written.
    let mut buffer = vec![0u8; needed as usize];
    // SAFETY: the buffer is as large as the system asked for.
    let ok = unsafe {
        EnumPrintersW(
            LOCAL | CONNECTIONS,
            core::ptr::null(),
            LEVEL,
            buffer.as_mut_ptr(),
            needed,
            &mut needed,
            &mut returned,
        )
    };
    if ok == 0 {
        return Vec::new();
    }

    let mut names = Vec::with_capacity(returned as usize);
    for index in 0..returned as usize {
        // `PRINTER_INFO_4W` is a pointer to the name, a pointer to the server,
        // and a word of attributes.
        let entry = buffer.as_ptr() as usize + index * core::mem::size_of::<PrinterInfo>();
        // SAFETY: the system wrote `returned` of these into the buffer.
        let info = unsafe { &*(entry as *const PrinterInfo) };
        if info.name.is_null() {
            continue;
        }
        // SAFETY: the pointer is into the same buffer and the string is
        // terminated, as the API promises.
        names.push(unsafe { from_wide(info.name) });
    }
    names
}

/// The printer a document goes to when nobody has said otherwise.
pub(crate) fn default_printer_name() -> Option<String> {
    let mut size = 0u32;
    // SAFETY: asking with no buffer is how the length is found.
    unsafe {
        GetDefaultPrinterW(core::ptr::null_mut(), &mut size);
    }
    if size == 0 {
        return None;
    }

    let mut buffer = vec![0u16; size as usize];
    // SAFETY: the buffer holds `size` characters, which is what was asked for.
    let ok = unsafe { GetDefaultPrinterW(buffer.as_mut_ptr(), &mut size) };
    if ok == 0 {
        return None;
    }
    let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}

/// Where the duplex setting sits inside a `DEVMODE`, and the bit that says it
/// has been set.
///
/// A `DEVMODE` is the structure a printer driver keeps its settings in. Its
/// first hundred and fifty-six bytes are fixed by the operating system and have
/// been since Windows 3, and the driver adds its own on the end. Only two
/// numbers in it are wanted here, so the structure is not declared: the bytes
/// are read and written where the API documents them, and nothing is touched
/// that the driver did not hand over.
mod devmode {
    /// `dmSize`: how much of the structure the operating system owns.
    pub(super) const SIZE_AT: usize = 68;
    /// `dmFields`: which of the settings below the driver should believe.
    pub(super) const FIELDS_AT: usize = 72;
    /// `dmDuplex`, a signed word.
    pub(super) const DUPLEX_AT: usize = 94;
    /// The bit of `dmFields` that says the duplex setting means something.
    pub(super) const FIELD_DUPLEX: u32 = 0x0000_1000;
    /// The whole of the part the operating system owns, which is as far as
    /// anything here reads.
    pub(super) const FIXED_SIZE: usize = 156;
}

/// Whether a printer can print on both sides of the sheet.
pub(crate) fn supports_both_sides(name: &str) -> bool {
    /// `DC_DUPLEX`: one when the printer can, zero when it cannot.
    const DC_DUPLEX: u16 = 7;

    let wide_name = wide(name);
    // SAFETY: the name outlives the call; a null port means the printer's own,
    // and a null mode means its current settings.
    let answer = unsafe {
        DeviceCapabilitiesW(
            wide_name.as_ptr(),
            core::ptr::null(),
            DC_DUPLEX,
            core::ptr::null_mut(),
            core::ptr::null(),
        )
    };
    answer == 1
}

/// Opens a printer by name, set up to print on both sides or on one.
///
/// The setting belongs to the driver rather than to the page image, so it is
/// written into the structure the driver keeps its settings in and handed back
/// with the request for a device context.
pub(crate) fn open_printer_with(
    name: &str,
    both_sides: Option<bool>,
) -> Option<crate::printing::Printer> {
    let wide_name = wide(name);
    let settings = both_sides.and_then(|long_edge| driver_settings(&wide_name, long_edge));
    let mode = settings.as_ref().map_or(core::ptr::null(), |bytes| bytes.as_ptr());

    // SAFETY: the name and the settings outlive the call, and a null driver
    // means "work it out from the name", which is what the API documents.
    let context = unsafe {
        CreateDCW(core::ptr::null(), wide_name.as_ptr(), core::ptr::null(), mode.cast::<c_void>())
    };
    if context.is_null() {
        return None;
    }
    Some(crate::printing::Printer::from_device_context(context as usize))
}

/// The printer's own settings, with the duplex field set.
///
/// `None` when the driver will not give them up, in which case the printer is
/// opened with whatever it is already set to — which is better than refusing to
/// print.
fn driver_settings(wide_name: &[u16], long_edge: bool) -> Option<Vec<u8>> {
    /// `DM_OUT_BUFFER`: fill in the structure handed over.
    const DM_OUT_BUFFER: u32 = 2;
    /// What `dmDuplex` is set to: one for a single side, two for a sheet
    /// turned over its long edge, three for its short one.
    const DUPLEX_VERTICAL: i16 = 2;
    const DUPLEX_HORIZONTAL: i16 = 3;

    let mut printer: Handle = core::ptr::null_mut();
    // SAFETY: the name outlives the call, and the handle is closed below.
    if unsafe { OpenPrinterW(wide_name.as_ptr(), &mut printer, core::ptr::null()) } == 0 {
        return None;
    }

    // How large the structure is, driver's own part included. Asking with no
    // buffer is how the size is found.
    // SAFETY: the printer handle is open and the name outlives the call.
    let needed = unsafe {
        DocumentPropertiesW(
            core::ptr::null_mut(),
            printer,
            wide_name.as_ptr(),
            core::ptr::null_mut(),
            core::ptr::null(),
            0,
        )
    };
    if needed < devmode::FIXED_SIZE as i32 {
        // SAFETY: the handle came from `OpenPrinterW` and is not used again.
        unsafe { ClosePrinter(printer) };
        return None;
    }

    let mut bytes = vec![0u8; needed as usize];
    // SAFETY: the buffer is as large as the driver asked for.
    let filled = unsafe {
        DocumentPropertiesW(
            core::ptr::null_mut(),
            printer,
            wide_name.as_ptr(),
            bytes.as_mut_ptr(),
            core::ptr::null(),
            DM_OUT_BUFFER,
        )
    };
    // SAFETY: the handle came from `OpenPrinterW` and is not used again.
    unsafe { ClosePrinter(printer) };
    if filled < 0 {
        return None;
    }

    // The driver said how much of the structure it filled in; nothing is
    // written past that, whatever the offsets below say.
    let size = u16::from_le_bytes([bytes[devmode::SIZE_AT], bytes[devmode::SIZE_AT + 1]]) as usize;
    if size < devmode::DUPLEX_AT + 2 || bytes.len() < size {
        return None;
    }

    let fields = u32::from_le_bytes([
        bytes[devmode::FIELDS_AT],
        bytes[devmode::FIELDS_AT + 1],
        bytes[devmode::FIELDS_AT + 2],
        bytes[devmode::FIELDS_AT + 3],
    ]) | devmode::FIELD_DUPLEX;
    bytes[devmode::FIELDS_AT..devmode::FIELDS_AT + 4].copy_from_slice(&fields.to_le_bytes());

    let duplex = if long_edge { DUPLEX_VERTICAL } else { DUPLEX_HORIZONTAL };
    bytes[devmode::DUPLEX_AT..devmode::DUPLEX_AT + 2].copy_from_slice(&duplex.to_le_bytes());
    Some(bytes)
}

/// Reads a string the system wrote and terminated.
///
/// # Safety
///
/// The pointer must be to a run of UTF-16 ending in a zero.
unsafe fn from_wide(text: *const u16) -> String {
    let mut length = 0usize;
    while *text.add(length) != 0 {
        length += 1;
    }
    String::from_utf16_lossy(core::slice::from_raw_parts(text, length))
}

/// `PRINTER_INFO_4W`, which is what the cheapest listing hands back.
#[repr(C)]
struct PrinterInfo {
    name: *const u16,
    server: *const u16,
    attributes: u32,
}

/// Asks which printer to use, and opens it.
pub(crate) fn choose_printer() -> Option<crate::printing::Printer> {
    // SAFETY: the structure is zeroed and then filled in, and its size is what
    // the system reads its fields by.
    let mut arguments: PrintDialog = unsafe { core::mem::zeroed() };
    arguments.size = core::mem::size_of::<PrintDialog>() as u32;
    arguments.owner = owner_window();
    arguments.copies = 1;
    // Printing a selection or a range of pages is a later stage; offering the
    // choice and then ignoring it would be worse than not offering it.
    arguments.flags = PD_RETURN_DC
        | PD_NO_SELECTION
        | PD_NO_PAGE_NUMBERS
        | PD_HIDE_PRINT_TO_FILE
        | PD_USE_DEVICE_MODE_COPIES;

    // SAFETY: the structure outlives the call and is filled in by it.
    let chosen = unsafe { PrintDlgW(&mut arguments) };
    if chosen == 0 || arguments.device_context.is_null() {
        return None;
    }

    Some(crate::printing::Printer::from_device_context(arguments.device_context as usize))
}

/// What a printer's paper is, in its own pixels.
pub(crate) fn printer_page(device_context: usize) -> crate::printing::PageSetup {
    let handle = device_context as Handle;
    // SAFETY: the handle came from the print dialog and is not used after the
    // printer is dropped.
    let ask = |index: i32| unsafe { GetDeviceCaps(handle, index) };

    crate::printing::PageSetup {
        width: ask(CAPABILITY_HORIZONTAL_PIXELS).max(1) as usize,
        height: ask(CAPABILITY_VERTICAL_PIXELS).max(1) as usize,
        paper_width: ask(CAPABILITY_PAPER_WIDTH).max(1) as usize,
        paper_height: ask(CAPABILITY_PAPER_HEIGHT).max(1) as usize,
        offset_x: ask(CAPABILITY_OFFSET_X).max(0) as usize,
        offset_y: ask(CAPABILITY_OFFSET_Y).max(0) as usize,
        dpi_x: ask(CAPABILITY_DPI_X).max(1) as f32,
        dpi_y: ask(CAPABILITY_DPI_Y).max(1) as f32,
    }
}

/// Begins a print job.
pub(crate) fn start_document(device_context: usize, name: &str) -> bool {
    let title = wide(name);
    let info = DocumentInfo {
        size: core::mem::size_of::<DocumentInfo>() as i32,
        name: title.as_ptr(),
        output: core::ptr::null(),
        data_type: core::ptr::null(),
        kind: 0,
    };
    // SAFETY: the title outlives the call, which copies what it needs.
    unsafe { StartDocW(device_context as Handle, &info) > 0 }
}

/// Sends one page, drawn as bands so that no whole page is ever in memory.
pub(crate) fn print_page(
    device_context: usize,
    width: usize,
    height: usize,
    mut band: impl FnMut(usize, usize) -> Vec<u8>,
) -> bool {
    let handle = device_context as Handle;

    // SAFETY: the handle is the printer's, and each band's pixels outlive the
    // one call that reads them.
    unsafe {
        if StartPage(handle) <= 0 {
            return false;
        }

        let mut top = 0usize;
        while top < height {
            let rows = PRINT_BAND_ROWS.min(height - top);
            let pixels = band(top, rows);
            if pixels.len() < width * rows * 4 {
                break;
            }

            // A negative height means the rows run top to bottom, which is the
            // order a canvas stores them in.
            let info = BitmapInfo {
                header: BitmapInfoHeader {
                    size: core::mem::size_of::<BitmapInfoHeader>() as u32,
                    width: width as i32,
                    height: -(rows as i32),
                    planes: 1,
                    bit_count: 32,
                    compression: BITMAP_UNCOMPRESSED,
                    image_size: 0,
                    x_pixels_per_meter: 0,
                    y_pixels_per_meter: 0,
                    colors_used: 0,
                    colors_important: 0,
                },
                colors: [0; 3],
            };

            StretchDIBits(
                handle,
                0,
                top as i32,
                width as i32,
                rows as i32,
                0,
                0,
                width as i32,
                rows as i32,
                pixels.as_ptr().cast(),
                &info,
                BITMAP_RGB_COLORS,
                COPY_SOURCE,
            );

            top += rows;
        }

        EndPage(handle) > 0
    }
}

/// Ends a print job and releases the printer.
pub(crate) fn finish_document(device_context: usize, keep: bool) -> bool {
    let handle = device_context as Handle;
    // SAFETY: the handle came from the print dialog and is released here once.
    unsafe {
        // `EndDoc` answers with the job's number, or something below one
        // when the spooler would not take it.
        let taken = if keep { EndDoc(handle) > 0 } else { AbortDoc(handle) > 0 };
        DeleteDC(handle);
        taken
    }
}

// --- The clipboard ----------------------------------------------------------

/// Puts text on the clipboard as UTF-16.
///
/// The clipboard is a shared resource that any program can be holding, so every
/// step here can fail for ordinary reasons. Each failure gives the memory back
/// and returns false rather than leaving the clipboard open, which would lock
/// out every other program on the desktop.
pub(crate) fn clipboard_set_text(text: &str) -> bool {
    let encoded = wide(text);
    let bytes = encoded.len() * core::mem::size_of::<u16>();

    // SAFETY: the handle comes from GlobalAlloc and is written through a
    // pointer from GlobalLock, within the size that was asked for. Once
    // SetClipboardData succeeds the system owns the memory and it is not
    // touched again; until then this function frees it on every failure.
    unsafe {
        if OpenClipboard(core::ptr::null_mut()) == 0 {
            return false;
        }

        let memory = GlobalAlloc(MEMORY_MOVEABLE, bytes);
        if memory.is_null() {
            CloseClipboard();
            return false;
        }

        let destination = GlobalLock(memory).cast::<u16>();
        if destination.is_null() {
            GlobalFree(memory);
            CloseClipboard();
            return false;
        }
        core::ptr::copy_nonoverlapping(encoded.as_ptr(), destination, encoded.len());
        GlobalUnlock(memory);

        EmptyClipboard();
        if SetClipboardData(CLIPBOARD_UNICODE_TEXT, memory).is_null() {
            GlobalFree(memory);
            CloseClipboard();
            return false;
        }

        CloseClipboard();
        true
    }
}

/// Puts several formats on the clipboard at once: the text, and the HTML,
/// Rich Text and picture beside it, each under its own format so that
/// whatever pastes takes the one it knows.
pub(crate) fn clipboard_set_contents(contents: &crate::clipboard::Contents) -> bool {
    // Each format's bytes, with the format it goes under.
    let mut items: Vec<(u32, Vec<u8>)> = Vec::new();
    if let Some(text) = &contents.text {
        let encoded = wide(text);
        let mut bytes = Vec::with_capacity(encoded.len() * 2);
        for unit in encoded {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        items.push((CLIPBOARD_UNICODE_TEXT, bytes));
    }
    // The registered formats, by the names every program registers.
    for (name, bytes) in [
        ("HTML Format", &contents.html),
        ("Rich Text Format", &contents.rtf),
        ("PNG", &contents.png),
    ] {
        if let Some(bytes) = bytes {
            // SAFETY: the name outlives the call.
            let format = unsafe { RegisterClipboardFormatW(wide(name).as_ptr()) };
            if format != 0 {
                let mut bytes = bytes.clone();
                // Text formats end with a nought, which Word looks for.
                if name != "PNG" {
                    bytes.push(0);
                }
                items.push((format, bytes));
            }
        }
    }
    if let Some(dib) = &contents.dib {
        items.push((CLIPBOARD_DIB, dib.clone()));
    }
    if items.is_empty() {
        return false;
    }

    // SAFETY: each handle comes from GlobalAlloc and is written through a
    // pointer from GlobalLock, within the size asked for. Once
    // SetClipboardData takes a handle the system owns it; a handle it did
    // not take is freed here.
    unsafe {
        if OpenClipboard(core::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let mut any = false;
        for (format, bytes) in items {
            let memory = GlobalAlloc(MEMORY_MOVEABLE, bytes.len().max(1));
            if memory.is_null() {
                continue;
            }
            let destination = GlobalLock(memory).cast::<u8>();
            if destination.is_null() {
                GlobalFree(memory);
                continue;
            }
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
            GlobalUnlock(memory);
            if SetClipboardData(format, memory).is_null() {
                GlobalFree(memory);
            } else {
                any = true;
            }
        }
        CloseClipboard();
        any
    }
}

/// Reads every format this program takes off the clipboard.
pub(crate) fn clipboard_contents() -> crate::clipboard::Contents {
    let mut contents = crate::clipboard::Contents {
        text: clipboard_text(),
        ..crate::clipboard::Contents::default()
    };
    // SAFETY: the names outlive the calls; the handles belong to the
    // clipboard and are only read, within the size the system reports.
    unsafe {
        let html = RegisterClipboardFormatW(wide("HTML Format").as_ptr());
        let rtf = RegisterClipboardFormatW(wide("Rich Text Format").as_ptr());
        let png = RegisterClipboardFormatW(wide("PNG").as_ptr());
        if OpenClipboard(core::ptr::null_mut()) == 0 {
            return contents;
        }
        let read = |format: u32| -> Option<Vec<u8>> {
            if format == 0 || IsClipboardFormatAvailable(format) == 0 {
                return None;
            }
            let memory = GetClipboardData(format);
            if memory.is_null() {
                return None;
            }
            let source = GlobalLock(memory).cast::<u8>();
            if source.is_null() {
                return None;
            }
            let size = GlobalSize(memory);
            let bytes = core::slice::from_raw_parts(source, size).to_vec();
            GlobalUnlock(memory);
            Some(bytes)
        };
        // The text formats end at their nought.
        let text_bytes = |bytes: Vec<u8>| {
            let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
            let mut bytes = bytes;
            bytes.truncate(end);
            bytes
        };
        contents.html = read(html).map(text_bytes);
        contents.rtf = read(rtf).map(text_bytes);
        contents.png = read(png);
        contents.dib = read(CLIPBOARD_DIB);
        CloseClipboard();
    }
    contents
}

/// Reads UTF-16 text from the clipboard.
pub(crate) fn clipboard_text() -> Option<String> {
    // SAFETY: the handle belongs to the clipboard and is only read, never
    // freed. The read stops at the terminator or at the end of the block the
    // system reports, so a value without one cannot run off the end.
    unsafe {
        if IsClipboardFormatAvailable(CLIPBOARD_UNICODE_TEXT) == 0 {
            return None;
        }
        if OpenClipboard(core::ptr::null_mut()) == 0 {
            return None;
        }

        let memory = GetClipboardData(CLIPBOARD_UNICODE_TEXT);
        if memory.is_null() {
            CloseClipboard();
            return None;
        }

        let source = GlobalLock(memory).cast::<u16>();
        if source.is_null() {
            CloseClipboard();
            return None;
        }

        let limit = GlobalSize(memory) / core::mem::size_of::<u16>();
        let mut units = Vec::new();
        for index in 0..limit {
            let unit = *source.add(index);
            if unit == 0 {
                break;
            }
            units.push(unit);
        }

        GlobalUnlock(memory);
        CloseClipboard();

        Some(String::from_utf16_lossy(&units))
    }
}

/// Maps a virtual key code to the small set of keys the shell reports.
fn key_from_code(code: u32) -> Option<Key> {
    // Letter keys carry their letter, which is what a shortcut needs.
    if (0x41..=0x5A).contains(&code) {
        return char::from_u32(code + 32).map(Key::Letter);
    }
    // The number row. Its virtual key codes are the ASCII digits themselves.
    if (0x30..=0x39).contains(&code) {
        return char::from_u32(code).map(Key::Digit);
    }
    // The function keys, F1 to F12, which follow one another.
    if (0x70..=0x7B).contains(&code) {
        return Some(Key::Function((code - 0x70 + 1) as u8));
    }
    Some(match code {
        KEY_UP => Key::Up,
        KEY_DOWN => Key::Down,
        KEY_LEFT => Key::Left,
        KEY_RIGHT => Key::Right,
        KEY_PAGE_UP => Key::PageUp,
        KEY_PAGE_DOWN => Key::PageDown,
        KEY_HOME => Key::Home,
        KEY_END => Key::End,
        KEY_ENTER => Key::Enter,
        KEY_BACKSPACE => Key::Backspace,
        KEY_DELETE => Key::Delete,
        KEY_ESCAPE => Key::Escape,
        KEY_TAB => Key::Tab,
        KEY_SPACE => Key::Space,
        _ => return None,
    })
}

/// Puts up whichever pointer the application says belongs where it is.
///
/// Returns whether it handled the message. It does not when the application is
/// not there to ask, which leaves the system to do whatever it would have.
fn set_cursor_for_pointer(window: Handle) -> bool {
    let mut point = Point { x: 0, y: 0 };
    // SAFETY: both take a pointer to a structure this function owns, and a
    // window handle belonging to this thread.
    let inside = unsafe {
        if GetCursorPos(&raw mut point) == 0 {
            return false;
        }
        ScreenToClient(window, &raw mut point) != 0
    };
    if !inside {
        return false;
    }

    let (x, y) = to_logical(window, (point.x, point.y));
    let wanted = with_application(|app| app.cursor(x, y));
    let Some(wanted) = wanted else { return false };

    let name = match wanted {
        crate::Cursor::Arrow => ARROW_CURSOR,
        crate::Cursor::Text => TEXT_CURSOR,
        crate::Cursor::ResizeHorizontal => RESIZE_HORIZONTAL_CURSOR,
        crate::Cursor::ResizeVertical => RESIZE_VERTICAL_CURSOR,
        crate::Cursor::Hand => HAND_CURSOR,
    };

    // SAFETY: a standard cursor name, and the handle it returns, which the
    // system owns and never frees.
    unsafe {
        let cursor = LoadCursorW(core::ptr::null_mut(), name);
        if cursor.is_null() {
            return false;
        }
        SetCursor(cursor);
    }
    true
}

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(
        owner: Handle,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> Handle;
}

/// What `ShellExecuteW` returns below, which is a failure and nothing else.
///
/// The call returns a fake handle rather than a code, and anything above 32
/// means it worked. This is not a mistake in this program: it is the documented
/// return of a function older than the convention it breaks.
const SHELL_EXECUTE_FLOOR: usize = 32;

/// Hands an address to whichever program the desktop opens it with.
pub(crate) fn open_in_shell(address: &str) -> bool {
    let operation = wide("open");
    let file = wide(address);
    // SAFETY: both strings are null-terminated and outlive the call, and the
    // three pointers left null are documented as optional.
    let result = unsafe {
        ShellExecuteW(
            core::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            core::ptr::null(),
            core::ptr::null(),
            SHOW_NORMAL,
        )
    };
    result as usize > SHELL_EXECUTE_FLOOR
}

// --- The files the desktop knows this program by ----------------------------

#[link(name = "advapi32")]
extern "system" {
    fn RegCreateKeyExW(
        key: Handle,
        name: *const u16,
        reserved: u32,
        class: *const u16,
        options: u32,
        access: u32,
        security: *const c_void,
        result: *mut Handle,
        disposition: *mut u32,
    ) -> i32;
    fn RegOpenKeyExW(
        key: Handle,
        name: *const u16,
        options: u32,
        access: u32,
        result: *mut Handle,
    ) -> i32;
    fn RegSetValueExW(
        key: Handle,
        name: *const u16,
        reserved: u32,
        kind: u32,
        data: *const u8,
        length: u32,
    ) -> i32;
    fn RegQueryValueExW(
        key: Handle,
        name: *const u16,
        reserved: *mut u32,
        kind: *mut u32,
        data: *mut u8,
        length: *mut u32,
    ) -> i32;
    fn RegCloseKey(key: Handle) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn SHAddToRecentDocs(flags: u32, path: *const c_void);
    fn SHChangeNotify(event: i32, flags: u32, first: *const c_void, second: *const c_void);
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleFileNameW(module: Handle, name: *mut u16, length: u32) -> u32;
}

/// The two roots used here. `HKEY_CURRENT_USER` is where a program writes what
/// it knows about itself; `HKEY_CLASSES_ROOT` is the merged view of what the
/// person and the machine have between them decided, which is the only honest
/// place to ask what actually opens a file.
const HKEY_CLASSES_ROOT: Handle = 0x8000_0000 as Handle;
const HKEY_CURRENT_USER: Handle = 0x8000_0001 as Handle;
const KEY_READ: u32 = 0x0002_0019;
const KEY_WRITE: u32 = 0x0002_0006;
const REG_SZ: u32 = 1;
const ERROR_SUCCESS: i32 = 0;
/// `SHAddToRecentDocs` takes a wide path when told so.
const SHARD_PATHW: u32 = 3;
/// "The association between a file kind and a program has changed."
const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
const SHCNF_IDLIST: u32 = 0;

/// This program's own file, which every registration has to name.
fn program_path() -> Option<String> {
    let mut buffer = [0u16; 32768];
    // SAFETY: a null module means this program, and the buffer is as long as
    // the length passed with it.
    let length = unsafe {
        GetModuleFileNameW(core::ptr::null_mut(), buffer.as_mut_ptr(), buffer.len() as u32)
    };
    if length == 0 || length as usize >= buffer.len() {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

/// Writes one string value under `HKEY_CURRENT_USER`, making the key.
fn write_string(path: &str, value_name: Option<&str>, value: &str) -> bool {
    let wide_path = wide(path);
    let mut key: Handle = core::ptr::null_mut();
    // SAFETY: a null-terminated name, and a handle written only on success,
    // which is closed below on every path out.
    let made = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide_path.as_ptr(),
            0,
            core::ptr::null(),
            0,
            KEY_WRITE,
            core::ptr::null(),
            &mut key,
            core::ptr::null_mut(),
        )
    };
    if made != ERROR_SUCCESS {
        return false;
    }
    let name = value_name.map(wide);
    let data = wide(value);
    // SAFETY: the data is the string's own bytes, its length counted in bytes
    // including the terminator, which is what REG_SZ means.
    let written = unsafe {
        RegSetValueExW(
            key,
            name.as_ref().map_or(core::ptr::null(), |name| name.as_ptr()),
            0,
            REG_SZ,
            data.as_ptr().cast::<u8>(),
            (data.len() * 2) as u32,
        )
    };
    // SAFETY: the handle came from the call above and is not used again.
    unsafe { RegCloseKey(key) };
    written == ERROR_SUCCESS
}

/// Reads one string value from the merged view of the classes.
fn read_class_string(path: &str, value_name: Option<&str>) -> Option<String> {
    let wide_path = wide(path);
    let mut key: Handle = core::ptr::null_mut();
    // SAFETY: as above; the handle is closed before returning.
    let opened =
        unsafe { RegOpenKeyExW(HKEY_CLASSES_ROOT, wide_path.as_ptr(), 0, KEY_READ, &mut key) };
    if opened != ERROR_SUCCESS {
        return None;
    }
    let name = value_name.map(wide);
    let mut buffer = [0u16; 2048];
    let mut length = (buffer.len() * 2) as u32;
    // SAFETY: the buffer is as long as the length passed with it, and the call
    // writes no more than that.
    let read = unsafe {
        RegQueryValueExW(
            key,
            name.as_ref().map_or(core::ptr::null(), |name| name.as_ptr()),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            buffer.as_mut_ptr().cast::<u8>(),
            &mut length,
        )
    };
    // SAFETY: the handle came from the call above and is not used again.
    unsafe { RegCloseKey(key) };
    if read != ERROR_SUCCESS {
        return None;
    }
    let characters = (length as usize / 2).min(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..characters]);
    Some(text.trim_end_matches('\0').to_owned())
}

/// The program identifier a kind is registered under: one per extension, so
/// that each kind can have its own name and its own icon, as Word's do.
fn program_id(extension: &str) -> String {
    format!("WordProcessor{extension}")
}

pub(crate) fn remember_document(path: &Path, _media_type: &str) {
    let wide_path = wide(&path.display().to_string());
    // SAFETY: a null-terminated path, which is what SHARD_PATHW says the
    // pointer is.
    unsafe { SHAddToRecentDocs(SHARD_PATHW, wide_path.as_ptr().cast::<c_void>()) };
}

pub(crate) fn associate_kinds(kinds: &[crate::files::Kind], program_name: &str) -> bool {
    let Some(program) = program_path() else { return false };
    let command = format!("\"{program}\" \"%1\"");
    let executable = Path::new(&program)
        .file_name()
        .map_or_else(|| program.clone(), |name| name.to_string_lossy().into_owned());
    let mut all = true;

    // The program itself, under the name Windows shows in Open With.
    let application = format!(r"Software\Classes\Applications\{executable}");
    all &= write_string(&application, Some("FriendlyAppName"), program_name);
    all &= write_string(&format!(r"{application}\shell\open\command"), None, &command);

    // What it is capable of, which is what the Default Apps page reads.
    all &=
        write_string(r"Software\WordProcessor\Capabilities", Some("ApplicationName"), program_name);
    all &= write_string(
        r"Software\WordProcessor\Capabilities",
        Some("ApplicationDescription"),
        "Writes and reads Word documents",
    );
    all &= write_string(
        r"Software\RegisteredApplications",
        Some("WordProcessor"),
        r"Software\WordProcessor\Capabilities",
    );

    for kind in kinds {
        let id = program_id(kind.extension);
        let class = format!(r"Software\Classes\{id}");
        all &= write_string(&class, None, kind.description);
        all &= write_string(&format!(r"{class}\DefaultIcon"), None, &format!("{program},0"));
        all &= write_string(&format!(r"{class}\shell\open\command"), None, &command);
        // The kind itself lists the program as one that opens it. Not as the
        // one that does: that is the person's choice, and Windows keeps it
        // where a program cannot write it.
        all &= write_string(
            &format!(r"Software\Classes\{}\OpenWithProgids", kind.extension),
            Some(&id),
            "",
        );
        all &= write_string(&format!(r"{application}\SupportedTypes"), Some(kind.extension), "");
        // The capabilities are what "set this program as the default" acts
        // on, so the kinds it does not ask for are left out of them while
        // still being registered above.
        if kind.becomes_default {
            all &= write_string(
                r"Software\WordProcessor\Capabilities\FileAssociations",
                Some(kind.extension),
                &id,
            );
        }
    }

    // Tell the shell, or Explorer goes on showing the old icons and the old
    // Open With list until it is restarted.
    // SAFETY: the event takes no arguments in this form, which is what the two
    // null pointers say.
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, core::ptr::null(), core::ptr::null());
    }
    all
}

pub(crate) fn opens_kind(kind: &crate::files::Kind) -> bool {
    let Some(program) = program_path() else { return false };
    // What the person chose, if they have chosen; otherwise what the merged
    // view says the extension is.
    let chosen = read_user_choice(kind.extension);
    let id = match chosen {
        Some(id) => id,
        None => match read_class_string(kind.extension, None) {
            Some(id) if !id.is_empty() => id,
            _ => return false,
        },
    };
    let Some(command) = read_class_string(&format!(r"{id}\shell\open\command"), None) else {
        return false;
    };
    command.to_lowercase().contains(&program.to_lowercase())
}

/// Which program the person chose for an extension, where they have chosen.
fn read_user_choice(extension: &str) -> Option<String> {
    let path = format!(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{extension}\UserChoice"
    );
    let wide_path = wide(&path);
    let mut key: Handle = core::ptr::null_mut();
    // SAFETY: as the other registry calls; the handle is closed below.
    let opened =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide_path.as_ptr(), 0, KEY_READ, &mut key) };
    if opened != ERROR_SUCCESS {
        return None;
    }
    let name = wide("ProgId");
    let mut buffer = [0u16; 512];
    let mut length = (buffer.len() * 2) as u32;
    // SAFETY: the buffer is as long as the length passed with it.
    let read = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            buffer.as_mut_ptr().cast::<u8>(),
            &mut length,
        )
    };
    // SAFETY: the handle came from the call above and is not used again.
    unsafe { RegCloseKey(key) };
    if read != ERROR_SUCCESS {
        return None;
    }
    let characters = (length as usize / 2).min(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..characters]).trim_end_matches('\0').to_owned())
}

pub(crate) fn choose_default_programs() -> bool {
    // The Settings page on Windows 10 and 11; the old control panel where
    // there is no Settings to open it.
    open_in_shell("ms-settings:defaultapps")
        || open_in_shell("control.exe /name Microsoft.DefaultPrograms")
}

// --- What the machine says about numbers, lengths, dates and paper --------

#[link(name = "kernel32")]
extern "system" {
    fn GetUserDefaultLocaleName(name: *mut u16, length: i32) -> i32;
    fn GetLocaleInfoEx(locale: *const u16, kind: u32, data: *mut u16, length: i32) -> i32;
}

/// What the system calls the things this asks it for.
const LOCALE_IMEASURE: u32 = 0x0000_000D;
const LOCALE_SDECIMAL: u32 = 0x0000_000E;
const LOCALE_STHOUSAND: u32 = 0x0000_000F;
const LOCALE_ITIME: u32 = 0x0000_0023;
const LOCALE_SSHORTDATE: u32 = 0x0000_001F;
const LOCALE_SMONTHNAME1: u32 = 0x0000_0038;
const LOCALE_SDAYNAME1: u32 = 0x0000_002A;
const LOCALE_IPAPERSIZE: u32 = 0x0000_100A;
/// What `LOCALE_IPAPERSIZE` calls the two sheets this program knows.
const PAPER_LETTER: &str = "1";

/// One of the things the system knows about this machine's settings.
fn locale_string(name: &[u16], kind: u32) -> Option<String> {
    let mut buffer = [0u16; 128];
    // SAFETY: the name is a null-terminated string and the buffer is as
    // long as the length passed with it.
    let written =
        unsafe { GetLocaleInfoEx(name.as_ptr(), kind, buffer.as_mut_ptr(), buffer.len() as i32) };
    if written <= 1 {
        return None;
    }
    // The count includes the terminator.
    Some(String::from_utf16_lossy(&buffer[..written as usize - 1]))
}

pub(crate) fn locale() -> crate::locale::Locale {
    use crate::locale::{DateOrder, Locale, Paper};

    let mut name = [0u16; 85];
    // SAFETY: the buffer is as long as the length passed with it.
    let written = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
    let name: Vec<u16> = if written > 0 { name[..written as usize].to_vec() } else { vec![0] };

    let mut locale = Locale {
        name: String::from_utf16_lossy(&name).trim_end_matches('\0').to_owned(),
        ..Locale::default()
    };
    // "0" is metric; "1" is the measurements of the United States.
    if let Some(measure) = locale_string(&name, LOCALE_IMEASURE) {
        locale.metric = measure.trim() == "0";
    }
    if let Some(decimal) =
        locale_string(&name, LOCALE_SDECIMAL).and_then(|mark| mark.chars().next())
    {
        locale.decimal = decimal;
    }
    locale.thousands = locale_string(&name, LOCALE_STHOUSAND).and_then(|mark| mark.chars().next());
    if let Some(format) = locale_string(&name, LOCALE_SSHORTDATE) {
        if let Some((order, separator)) = date_shape(&format) {
            locale.date_order = order;
            locale.date_separator = separator;
        }
    }
    // "1" is a twenty-four hour clock.
    if let Some(time) = locale_string(&name, LOCALE_ITIME) {
        locale.twenty_four_hour = time.trim() == "1";
    }
    let months: Vec<String> =
        (0..12).filter_map(|month| locale_string(&name, LOCALE_SMONTHNAME1 + month)).collect();
    if months.len() == 12 {
        locale.months = months;
    }
    // The system's list starts on Monday, as this program's does.
    let days: Vec<String> =
        (0..7).filter_map(|day| locale_string(&name, LOCALE_SDAYNAME1 + day)).collect();
    if days.len() == 7 {
        locale.days = days;
    }
    locale.paper = match locale_string(&name, LOCALE_IPAPERSIZE) {
        Some(paper) if paper.trim() == PAPER_LETTER => Paper::Letter,
        Some(_) => Paper::A4,
        // A machine that does not say is taken to be what its measurements
        // say it is.
        None if locale.metric => Paper::A4,
        None => Paper::Letter,
    };
    let _ = DateOrder::MonthDayYear;
    locale
}

/// Which way round a short date goes, and what it is written with.
///
/// Windows writes the format with the letters Word's own field codes use:
/// `dd/MM/yyyy`, `M/d/yyyy`, `yyyy-MM-dd`.
fn date_shape(format: &str) -> Option<(crate::locale::DateOrder, char)> {
    use crate::locale::DateOrder;

    let mut parts: Vec<char> = Vec::new();
    let mut separator = None;
    for character in format.chars() {
        match character {
            'd' | 'M' | 'y' => {
                let part = if character == 'M' { 'm' } else { character };
                if parts.last() != Some(&part) {
                    parts.push(part);
                }
            }
            '\'' => {}
            _ if character.is_whitespace() => {}
            _ if separator.is_none() && !parts.is_empty() => separator = Some(character),
            _ => {}
        }
    }
    let order = match parts.as_slice() {
        ['d', 'm', ..] => DateOrder::DayMonthYear,
        ['m', 'd', ..] => DateOrder::MonthDayYear,
        ['y', ..] => DateOrder::YearMonthDay,
        _ => return None,
    };
    Some((order, separator.unwrap_or('/')))
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(
        window: Handle,
        attribute: u32,
        value: *const c_void,
        size: u32,
    ) -> i32;
}

/// Draw the caption and the border the dark way round.
///
/// Two numbers, not one: the attribute was 19 on the first builds of Windows 10
/// that had it and 20 from build 19041 on. Both are set, and the one that does
/// not exist on a given version fails harmlessly.
const ATTRIBUTE_DARK_MODE: u32 = 20;
const ATTRIBUTE_DARK_MODE_OLD: u32 = 19;
/// The colour of the line the desktop draws round the window. Windows 11 only.
const ATTRIBUTE_BORDER_COLOR: u32 = 34;
/// And of the caption behind it, which shows while the window is being dragged.
const ATTRIBUTE_CAPTION_COLOR: u32 = 35;

/// Tells the desktop what the window looks like.
///
/// # Why this is needed at all
///
/// The window draws its own title bar, so there is nothing left for the desktop
/// to draw — except that on Windows 11 there is: the rounded corners, the line
/// round the edge, and the caption that flashes while a window is dragged. Those
/// belong to the desktop compositor, and it paints them in the system's colours
/// unless it is told otherwise. A dark window with a pale line round it is what
/// happens when nobody tells it, and it shows only when the window is not
/// maximised, because a maximised window has no border to draw.
pub(crate) fn set_frame_appearance(dark: bool, border: (u8, u8, u8), caption: (u8, u8, u8)) {
    let window = WINDOW.with(std::cell::Cell::get);
    if window.is_null() {
        return;
    }

    let dark_value: i32 = i32::from(dark);
    // SAFETY: each call passes a pointer to a local of the size it declares,
    // and a window handle belonging to this thread. An attribute the running
    // version does not know is refused, which is not an error worth acting on.
    unsafe {
        // The old number first, so that on a version where both are known the
        // documented one is the one that stands.
        for attribute in [ATTRIBUTE_DARK_MODE_OLD, ATTRIBUTE_DARK_MODE] {
            DwmSetWindowAttribute(
                window,
                attribute,
                (&raw const dark_value).cast::<c_void>(),
                core::mem::size_of::<i32>() as u32,
            );
        }

        for (attribute, colour) in
            [(ATTRIBUTE_BORDER_COLOR, border), (ATTRIBUTE_CAPTION_COLOR, caption)]
        {
            let value = colour_reference(colour);
            DwmSetWindowAttribute(
                window,
                attribute,
                (&raw const value).cast::<c_void>(),
                core::mem::size_of::<u32>() as u32,
            );
        }
    }
}

/// A colour in the order the desktop wants it: blue, green, red, low byte first.
///
/// Not a mistake and not this program's choice — every colour in the Windows API
/// is written this way round, and passing the familiar order gives a window
/// bordered in the wrong colour with no error at all.
#[must_use]
fn colour_reference((red, green, blue): (u8, u8, u8)) -> u32 {
    u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16)
}

// --- Taking a picture of the screen -----------------------------------------

/// How the system numbers what `GetSystemMetrics` can be asked about.
const SM_XVIRTUALSCREEN: i32 = 76;
const SM_YVIRTUALSCREEN: i32 = 77;
const SM_CXVIRTUALSCREEN: i32 = 78;
const SM_CYVIRTUALSCREEN: i32 = 79;
/// Copy the source over the destination, unchanged.
const SRCCOPY: u32 = 0x00CC_0020;
/// Colours are in the bitmap itself rather than in a palette.
const DIB_RGB_COLORS: u32 = 0;
/// Draw the whole window, including the parts a compositor holds.
const PW_RENDERFULLCONTENT: u32 = 2;
/// Walking the windows from the top of the stacking order downwards.
const GW_HWNDFIRST: u32 = 0;
const GW_HWNDNEXT: u32 = 2;

#[link(name = "user32")]
extern "system" {
    fn GetDesktopWindow() -> Handle;
    fn GetDC(window: Handle) -> Handle;
    fn ReleaseDC(window: Handle, device_context: Handle) -> i32;
    fn GetWindow(window: Handle, relation: u32) -> Handle;
    fn IsWindowVisible(window: Handle) -> i32;
    fn IsIconic(window: Handle) -> i32;
    fn GetWindowTextW(window: Handle, text: *mut u16, count: i32) -> i32;
    fn GetWindowTextLengthW(window: Handle) -> i32;
    fn GetWindowRect(window: Handle, area: *mut Rect) -> i32;
    fn PrintWindow(window: Handle, device_context: Handle, flags: u32) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateCompatibleDC(device_context: Handle) -> Handle;
    fn CreateCompatibleBitmap(device_context: Handle, width: i32, height: i32) -> Handle;
    fn SelectObject(device_context: Handle, object: Handle) -> Handle;
    fn DeleteObject(object: Handle) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn BitBlt(
        destination: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: Handle,
        source_x: i32,
        source_y: i32,
        operation: u32,
    ) -> i32;
    fn GetDIBits(
        device_context: Handle,
        bitmap: Handle,
        first_line: u32,
        lines: u32,
        bits: *mut c_void,
        info: *mut BitmapInfo,
        usage: u32,
    ) -> i32;
}

/// A window that could be photographed, and what it is called.
pub(crate) struct ScreenWindow {
    pub(crate) title: String,
    /// The system handle, kept as a number so that nothing outside this module
    /// can mistake it for something it may use.
    pub(crate) handle: usize,
}

/// The visible windows with titles, topmost first.
///
/// Walked down the stacking order rather than enumerated with a callback: the
/// answer is the same and there is no function pointer to hand out.
pub(crate) fn screen_windows() -> Vec<ScreenWindow> {
    let mut out = Vec::new();
    // SAFETY: every handle comes from the system and is only asked about, never
    // freed. The text buffer is sized from the length the system reports and is
    // passed with that size.
    unsafe {
        let mut window = GetWindow(GetDesktopWindow(), GW_HWNDFIRST);
        let mut guard = 0;
        while !window.is_null() && guard < 500 {
            guard += 1;
            if IsWindowVisible(window) != 0 && IsIconic(window) == 0 {
                let length = GetWindowTextLengthW(window);
                if length > 0 {
                    let mut text = vec![0u16; length as usize + 1];
                    let written = GetWindowTextW(window, text.as_mut_ptr(), length + 1);
                    if written > 0 {
                        let title = String::from_utf16_lossy(&text[..written as usize]);
                        if !title.trim().is_empty() {
                            out.push(ScreenWindow { title, handle: window as usize });
                        }
                    }
                }
            }
            window = GetWindow(window, GW_HWNDNEXT);
        }
    }
    out
}

/// A picture taken of the screen: pixels in red, green, blue, alpha order.
pub(crate) struct Shot {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) pixels: Vec<u8>,
}

/// Photographs everything on every monitor.
pub(crate) fn capture_screen() -> Option<Shot> {
    // SAFETY: the sizes come from the system, and everything the copy allocates
    // is freed inside it.
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        copy_from_screen(left, top, width, height)
    }
}

/// Photographs one window, whatever is in front of it.
pub(crate) fn capture_window(handle: usize) -> Option<Shot> {
    let window = handle as Handle;
    // SAFETY: the handle came from `screen_windows` and is only drawn from.
    // Every object made here is deleted before returning, on both paths.
    unsafe {
        let mut area = Rect { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(window, &mut area) == 0 {
            return None;
        }
        let width = area.right - area.left;
        let height = area.bottom - area.top;
        if width <= 0 || height <= 0 {
            return None;
        }

        let screen = GetDC(core::ptr::null_mut());
        if screen.is_null() {
            return None;
        }
        let memory = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let mut shot = None;
        if !memory.is_null() && !bitmap.is_null() {
            let previous = SelectObject(memory, bitmap);
            // A window that will not draw itself is copied off the screen
            // instead, which is what is in front of the person anyway.
            let drawn = PrintWindow(window, memory, PW_RENDERFULLCONTENT) != 0
                || BitBlt(memory, 0, 0, width, height, screen, area.left, area.top, SRCCOPY) != 0;
            if drawn {
                shot = read_bitmap(memory, bitmap, width, height);
            }
            SelectObject(memory, previous);
        }
        if !bitmap.is_null() {
            DeleteObject(bitmap);
        }
        if !memory.is_null() {
            DeleteDC(memory);
        }
        ReleaseDC(core::ptr::null_mut(), screen);
        shot
    }
}

/// Copies a rectangle of the screen into a picture.
///
/// # Safety
///
/// Called only from this module, with sizes the system reported.
unsafe fn copy_from_screen(left: i32, top: i32, width: i32, height: i32) -> Option<Shot> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let screen = GetDC(core::ptr::null_mut());
    if screen.is_null() {
        return None;
    }

    let memory = CreateCompatibleDC(screen);
    let bitmap = CreateCompatibleBitmap(screen, width, height);
    let mut shot = None;
    if !memory.is_null() && !bitmap.is_null() {
        let previous = SelectObject(memory, bitmap);
        if BitBlt(memory, 0, 0, width, height, screen, left, top, SRCCOPY) != 0 {
            shot = read_bitmap(memory, bitmap, width, height);
        }
        SelectObject(memory, previous);
    }
    if !bitmap.is_null() {
        DeleteObject(bitmap);
    }
    if !memory.is_null() {
        DeleteDC(memory);
    }
    ReleaseDC(core::ptr::null_mut(), screen);
    shot
}

/// Reads a bitmap out of the system and turns it the right way up.
///
/// # Safety
///
/// The bitmap must belong to the device context, and both must outlive the
/// call. Both are true of every caller in this module.
unsafe fn read_bitmap(memory: Handle, bitmap: Handle, width: i32, height: i32) -> Option<Shot> {
    let mut info = BitmapInfo {
        header: BitmapInfoHeader {
            size: core::mem::size_of::<BitmapInfoHeader>() as u32,
            width,
            // Negative, so the rows come back top down rather than bottom up.
            height: -height,
            planes: 1,
            bit_count: 32,
            compression: 0,
            image_size: 0,
            x_pixels_per_meter: 0,
            y_pixels_per_meter: 0,
            colors_used: 0,
            colors_important: 0,
        },
        colors: [0; 3],
    };

    let count = width as usize * height as usize * 4;
    let mut pixels = vec![0u8; count];
    let read = GetDIBits(
        memory,
        bitmap,
        0,
        height as u32,
        pixels.as_mut_ptr().cast::<c_void>(),
        &mut info,
        DIB_RGB_COLORS,
    );
    if read == 0 {
        return None;
    }

    // Windows hands them over blue first, and with the alpha byte left as
    // whatever the window happened to put there, which for a screen is nothing.
    // So the channels are swapped and the picture made solid.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }

    Some(Shot { width: width as usize, height: height as usize, pixels })
}
#[cfg(test)]
mod frame_tests {
    use super::colour_reference;

    #[test]
    fn a_colour_is_written_blue_first() {
        assert_eq!(colour_reference((0x12, 0x34, 0x56)), 0x0056_3412);
    }

    #[test]
    fn black_and_white_come_out_as_themselves() {
        assert_eq!(colour_reference((0, 0, 0)), 0);
        assert_eq!(colour_reference((0xFF, 0xFF, 0xFF)), 0x00FF_FFFF);
    }
}

#[cfg(test)]
mod locale_tests {
    use super::date_shape;
    use crate::locale::DateOrder;

    #[test]
    fn a_short_date_says_which_way_round_it_goes() {
        assert_eq!(date_shape("M/d/yyyy"), Some((DateOrder::MonthDayYear, '/')));
        assert_eq!(date_shape("dd.MM.yyyy"), Some((DateOrder::DayMonthYear, '.')));
        assert_eq!(date_shape("yyyy-MM-dd"), Some((DateOrder::YearMonthDay, '-')));
        assert_eq!(date_shape("nothing of the kind"), None);
    }
}

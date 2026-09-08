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

use crate::{App, Error, Event, Key, Modifiers, Response, WindowOptions};

// --- Types the API uses -----------------------------------------------------

type Handle = *mut c_void;
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
struct Point {
    x: i32,
    y: i32,
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
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
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
/// Clipboard memory has to be movable, because the system takes ownership.
const MEMORY_MOVEABLE: u32 = 0x0002;

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
const MB_OK: u32 = 0x0000_0000;
const MB_ICON_WARNING: u32 = 0x0000_0030;
const MB_ICON_ERROR: u32 = 0x0000_0010;
const ID_CANCEL: i32 = 2;
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
    fn BeginPaint(window: Handle, paint: *mut PaintStruct) -> Handle;
    fn EndPaint(window: Handle, paint: *const PaintStruct) -> i32;
    fn InvalidateRect(window: Handle, area: *const Rect, erase: i32) -> i32;
    fn GetClientRect(window: Handle, area: *mut Rect) -> i32;
    fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    fn SetCursor(cursor: Handle) -> Handle;
    fn GetCursorPos(point: *mut Point) -> i32;
    fn GetKeyState(key: i32) -> i16;
    fn GetWindowLongW(window: Handle, index: i32) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
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
    fn ScreenToClient(window: Handle, point: *mut Point) -> i32;
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
    fn MessageBoxW(owner: Handle, text: *const u16, caption: *const u16, style: u32) -> i32;
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
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn GetLastError() -> u32;
    fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
    fn GlobalFree(memory: Handle) -> Handle;
    fn GlobalLock(memory: Handle) -> *mut c_void;
    fn GlobalUnlock(memory: Handle) -> i32;
    fn GlobalSize(memory: Handle) -> usize;
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
}

/// Which view a window is, by the order it was opened in.
fn window_index(window: Handle) -> usize {
    WINDOWS.with(|slot| slot.borrow().iter().position(|found| *found == window).unwrap_or(0))
}

/// Tells the application which window it is about to answer for.
fn point_at(window: Handle) {
    WINDOW.with(|slot| slot.set(window));
    let index = window_index(window);
    APPLICATION.with(|slot| {
        if let Some(app) = slot.borrow_mut().as_mut() {
            app.switch_window(index);
        }
    });
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
fn wide(text: &str) -> Vec<u16> {
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

/// Opens one window and remembers it.
unsafe fn create_window(title: &str, width: u32, height: u32) -> Result<Handle, Error> {
    let instance = register_class()?;
    let class_name = wide("WordProcessorWindow");
    let title = wide(title);

    let window = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        STYLE_OVERLAPPED_WINDOW,
        USE_DEFAULT_POSITION,
        USE_DEFAULT_POSITION,
        width as i32,
        height as i32,
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

    // Ask for the frame to be worked out again now that the window can
    // answer `WM_NCCALCSIZE` for itself. Without this the frame decided
    // during creation stands, and the system caption drawn in it sits above
    // the title bar this program draws — two title bars, one window.
    SetWindowPos(window, core::ptr::null_mut(), 0, 0, 0, 0, FRAME_CHANGED);
    ShowWindow(window, SHOW_NORMAL);

    // The heartbeat the caret blinks on and the tips are timed by.
    SetTimer(window, TICK_TIMER, TICK_MILLIS, core::ptr::null());
    UpdateWindow(window);
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
/// The window this thread owns, for a dialog to be modal to.
fn owner_window() -> Handle {
    WINDOW.with(std::cell::Cell::get)
}

/// Hands an event to the application and acts on what it asks for.
fn deliver(window: Handle, event: Event) -> Result_ {
    // Which of the program's windows this is, so the application can put that    // window's view back before it answers.    point_at(window);
    let response = APPLICATION.with(|slot| match slot.borrow_mut().as_mut() {
        Some(app) => app.handle(event),
        None => Response::Ignored,
    });

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
                let frame_x = GetSystemMetrics(32) + GetSystemMetrics(92);
                let frame_y = GetSystemMetrics(33) + GetSystemMetrics(92);
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
            deliver(window, Event::Resized { width, height })
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
            match key_from_code(word as u32) {
                Some(key) => deliver(window, Event::KeyDown { key, modifiers: modifiers() }),
                None => DefWindowProcW(window, message, word, long),
            }
        }
        MESSAGE_CHAR => match char::from_u32(word as u32) {
            // Control characters arrive here too; they are not text.
            Some(character) if !character.is_control() => deliver(window, Event::Char(character)),
            _ => 0,
        },
        MESSAGE_LEFT_BUTTON_DOWN => {
            let (x, y) = mouse_point(long);
            // Capturing the mouse keeps the messages coming even when the
            // pointer is dragged outside the window, so a selection that runs
            // off the edge does not stop growing there.
            SetCapture(window);
            deliver(window, Event::MouseDown { x, y, modifiers: modifiers() })
        }
        MESSAGE_MOUSE_MOVE => {
            let (x, y) = mouse_point(long);
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
            let (x, y) = mouse_point(long);
            deliver(window, Event::MiddleClick { x, y })
        }
        MESSAGE_LEFT_BUTTON_DOUBLE_CLICK => {
            let (x, y) = mouse_point(long);
            deliver(window, Event::DoubleClick { x, y })
        }
        // The right button opens the menu on the way up, which is where
        // Windows puts a context menu and where a person expects it: pressing
        // and thinking better of it should open nothing.
        MESSAGE_RIGHT_BUTTON_DOWN => 0,
        MESSAGE_RIGHT_BUTTON_UP => {
            let (x, y) = mouse_point(long);
            deliver(window, Event::RightClick { x, y, modifiers: modifiers() })
        }
        MESSAGE_LEFT_BUTTON_UP => {
            let (x, y) = mouse_point(long);
            ReleaseCapture();
            deliver(window, Event::MouseUp { x, y })
        }
        MESSAGE_CLOSE => {
            // The application gets to refuse, which is what lets it ask about
            // unsaved changes and act on "cancel".
            point_at(window);
            let response = APPLICATION.with(|slot| match slot.borrow_mut().as_mut() {
                Some(app) => app.handle(Event::Closing),
                None => Response::Ignored,
            });
            if response != Response::Refuse {
                DestroyWindow(window);
            }
            0
        }
        MESSAGE_DESTROY => {
            KillTimer(window, TICK_TIMER);
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

        let pixels = APPLICATION
            .with(|slot| slot.borrow_mut().as_mut().map(|app| app.draw(width, height).to_bgra()));

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
    let draggable = APPLICATION.with(|slot| match slot.borrow_mut().as_mut() {
        Some(app) => app.is_caption(point.x, point.y),
        None => false,
    });

    if draggable {
        HIT_CAPTION
    } else {
        HIT_CLIENT
    }
}

/// Minimises, maximises, restores or closes the window.
pub(crate) fn window_command(command: crate::WindowCommand) {
    let window = owner_window();
    if window.is_null() {
        return;
    }
    // SAFETY: the handle is this thread's window.
    unsafe {
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
                DefWindowProcW(window, MESSAGE_CLOSE, 0, 0);
            }
        }
        // The caption is drawn by the application, so a change of state has to
        // repaint it.
        InvalidateRect(window, core::ptr::null(), 0);
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
fn mouse_point(long: LongParam) -> (i32, i32) {
    let x = (long & 0xFFFF) as u16 as i16;
    let y = ((long >> 16) & 0xFFFF) as u16 as i16;
    (i32::from(x), i32::from(y))
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
    // The extension added when the user types a name without one.
    let default_extension = wide("docx");

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
        filter_index: 1,
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
        default_extension: default_extension.as_ptr(),
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
    Some(std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length])))
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
pub(crate) fn finish_document(device_context: usize, keep: bool) {
    let handle = device_context as Handle;
    // SAFETY: the handle came from the print dialog and is released here once.
    unsafe {
        if keep {
            EndDoc(handle);
        } else {
            AbortDoc(handle);
        }
        DeleteDC(handle);
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

    let wanted =
        APPLICATION.with(|slot| slot.borrow_mut().as_mut().map(|app| app.cursor(point.x, point.y)));
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

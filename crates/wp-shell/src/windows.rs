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

// --- Constants --------------------------------------------------------------

const CLASS_REDRAW_ON_RESIZE: u32 = 0x0001 | 0x0002;
const STYLE_OVERLAPPED_WINDOW: u32 = 0x00CF_0000;
const USE_DEFAULT_POSITION: i32 = 0x8000_0000_u32 as i32;
const SHOW_NORMAL: i32 = 1;

const MESSAGE_DESTROY: u32 = 0x0002;
const MESSAGE_SIZE: u32 = 0x0005;
const MESSAGE_PAINT: u32 = 0x000F;
const MESSAGE_ERASE_BACKGROUND: u32 = 0x0014;
const MESSAGE_KEY_DOWN: u32 = 0x0100;
const MESSAGE_CHAR: u32 = 0x0102;
const MESSAGE_LEFT_BUTTON_DOWN: u32 = 0x0201;
const MESSAGE_MOUSE_WHEEL: u32 = 0x020A;
const MESSAGE_CLOSE: u32 = 0x0010;

const WHEEL_STEP: f32 = 120.0;
const ARROW_CURSOR: *const u16 = 32512 as *const u16;
const BITMAP_UNCOMPRESSED: u32 = 0;
const BITMAP_RGB_COLORS: u32 = 0;
const COPY_SOURCE: u32 = 0x00CC_0020;

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
    fn GetKeyState(key: i32) -> i16;
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
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn GetLastError() -> u32;
}

// --- The application, reachable from the window procedure -------------------

thread_local! {
    /// The running application.
    ///
    /// The window procedure is called by the system, so it cannot be handed
    /// state as an argument. A thread-local reaches it without casting pointers
    /// through the window, and a window belongs to its creating thread anyway.
    static APPLICATION: RefCell<Option<Box<dyn App>>> = const { RefCell::new(None) };
}

/// Encodes a string the way the wide-character API expects, with a terminator.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(core::iter::once(0)).collect()
}

/// Opens the window and runs the event loop.
pub(crate) fn run(options: WindowOptions, app: Box<dyn App>) -> Result<(), Error> {
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(app));

    let class_name = wide("WordProcessorWindow");
    let title = wide(&options.title);

    // SAFETY: every pointer passed below either points at a local that outlives
    // the call, or is null where the API documents null as meaningful.
    unsafe {
        let instance = GetModuleHandleW(core::ptr::null());

        let class = WindowClass {
            size: core::mem::size_of::<WindowClass>() as u32,
            style: CLASS_REDRAW_ON_RESIZE,
            procedure: Some(window_procedure),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: core::ptr::null_mut(),
            cursor: LoadCursorW(core::ptr::null_mut(), ARROW_CURSOR),
            // No background brush: the whole window is painted every time, and
            // letting the system erase it first only causes flicker.
            background: core::ptr::null_mut(),
            menu_name: core::ptr::null(),
            class_name: class_name.as_ptr(),
            small_icon: core::ptr::null_mut(),
        };

        if RegisterClassExW(&class) == 0 {
            let code = GetLastError();
            // 1410 means the class is already registered, which happens if a
            // window is opened twice in one process and is not a failure.
            if code != 1410 {
                return Err(Error::WindowCreationFailed(format!(
                    "registering the window class failed with error {code}"
                )));
            }
        }

        let window = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            STYLE_OVERLAPPED_WINDOW,
            USE_DEFAULT_POSITION,
            USE_DEFAULT_POSITION,
            options.width as i32,
            options.height as i32,
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

        ShowWindow(window, SHOW_NORMAL);
        UpdateWindow(window);

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

    APPLICATION.with(|slot| slot.borrow_mut().take());
    Ok(())
}

/// Hands an event to the application and acts on what it asks for.
fn deliver(window: Handle, event: Event) -> Result_ {
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
        Response::Ignored => {}
    }
    0
}

/// The window procedure, which the system calls for every message.
unsafe extern "system" fn window_procedure(
    window: Handle,
    message: u32,
    word: WordParam,
    long: LongParam,
) -> Result_ {
    match message {
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
            deliver(window, Event::Scroll { lines: f32::from(delta) / WHEEL_STEP })
        }
        MESSAGE_KEY_DOWN => match key_from_code(word as u32) {
            Some(key) => deliver(window, Event::KeyDown { key, modifiers: modifiers() }),
            None => DefWindowProcW(window, message, word, long),
        },
        MESSAGE_CHAR => match char::from_u32(word as u32) {
            // Control characters arrive here too; they are not text.
            Some(character) if !character.is_control() => {
                deliver(window, Event::Char(character))
            }
            _ => 0,
        },
        MESSAGE_LEFT_BUTTON_DOWN => {
            let x = (long & 0xFFFF) as i16 as i32;
            let y = ((long >> 16) & 0xFFFF) as i16 as i32;
            deliver(window, Event::MouseDown { x, y, modifiers: modifiers() })
        }
        MESSAGE_CLOSE => {
            deliver(window, Event::Closing);
            DestroyWindow(window);
            0
        }
        MESSAGE_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(window, message, word, long),
    }
}

/// Asks the application for an image and puts it on the screen.
fn paint(window: Handle) {
    // SAFETY: the handle is the window the system is asking us to paint.
    unsafe {
        let mut area = Rect::default();
        GetClientRect(window, &mut area);
        let width = (area.right - area.left).max(1) as usize;
        let height = (area.bottom - area.top).max(1) as usize;

        let mut paint_struct = core::mem::zeroed::<PaintStruct>();
        let device_context = BeginPaint(window, &mut paint_struct);

        let pixels = APPLICATION.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .map(|app| app.draw(width, height).to_bgra())
        });

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

/// Which modifier keys are held down right now.
///
/// Read at the moment the key arrives rather than tracked separately: the
/// system already knows, and a separately maintained copy goes wrong the first
/// time the window loses focus while a key is held.
fn modifiers() -> Modifiers {
    // The high bit of the state means the key is down.
    let held = |key: i32| unsafe { GetKeyState(key) } < 0;
    Modifiers {
        control: held(KEY_CONTROL),
        shift: held(KEY_SHIFT),
        alt: held(KEY_ALT),
    }
}

/// Maps a virtual key code to the small set of keys the shell reports.
fn key_from_code(code: u32) -> Option<Key> {
    // Letter keys carry their letter, which is what a shortcut needs.
    if (0x41..=0x5A).contains(&code) {
        return char::from_u32(code + 32).map(Key::Letter);
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

//! The X11 shell: a window on an X server, spoken to directly.
//!
//! The same shape as the Windows shell — a window, an event loop, a way to
//! get a finished image onto the screen, the clipboard, the pointer — with
//! the X protocol under it instead of Win32. See [`x11`] for the protocol
//! and [`crate::App`] for what the window asks of the application. This is
//! one of the two Linux shells; the other speaks Wayland, and
//! [`super`] is where the choice between them is made.
//!
//! The window manager's frame is switched off, since the program draws its
//! own title bar, and the window is moved and sized through the manager's
//! own messages for it.

use super::keys;
use super::x11;
use super::xim;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::clipboard::Contents;
use crate::{
    App, Cursor, DragEffect, Error, Event, Modifiers, Response, WindowCommand, WindowOptions,
};
use x11::{Connection, Failure, Packet};

/// How often the clock ticks for the application, in milliseconds.
const TICK_MILLIS: u64 = 40;
/// How close to an edge the pointer starts a resize.
const RESIZE_BORDER: i32 = 6;
/// The density everything is measured in.
const ORDINARY_DPI: f32 = 96.0;

/// One window of this program.
struct Window {
    id: u32,
    gc: u32,
    /// The window's own pixels.
    width: u32,
    height: u32,
    scale: f32,
    cursor: Cursor,
    dirty: bool,
    /// The last press, for telling a double click.
    last_press: Option<(u32, i32, i32)>,
}

/// The atoms the shell speaks of.
struct Atoms {
    wm_protocols: u32,
    wm_delete_window: u32,
    wm_change_state: u32,
    net_wm_name: u32,
    net_wm_state: u32,
    net_wm_state_maximized_vert: u32,
    net_wm_state_maximized_horz: u32,
    net_wm_moveresize: u32,
    net_client_list_stacking: u32,
    motif_wm_hints: u32,
    utf8_string: u32,
    clipboard: u32,
    targets: u32,
    text: u32,
    text_plain_utf8: u32,
    text_html: u32,
    text_rtf: u32,
    image_png: u32,
    incr: u32,
    own_property: u32,
    xdnd_aware: u32,
    xdnd_enter: u32,
    xdnd_position: u32,
    xdnd_status: u32,
    xdnd_leave: u32,
    xdnd_drop: u32,
    xdnd_finished: u32,
    xdnd_selection: u32,
    xdnd_type_list: u32,
    xdnd_action_copy: u32,
    xdnd_action_move: u32,
    text_uri_list: u32,
    text_plain: u32,
}

/// Everything the shell holds while it runs.
struct State {
    connection: Connection,
    atoms: Atoms,
    windows: Vec<Window>,
    cursors: HashMap<u8, u32>,
    cursor_font: u32,
    /// Keysyms per keycode, and the keysyms in rows.
    keymap: (usize, Vec<u32>),
    /// What this program put on the clipboard, while it owns the
    /// selection.
    owned: Option<Contents>,
    /// Whether Alt, or Control, went down and nothing else has since.
    alt_alone: bool,
    control_alone: bool,
    scale: f32,
    /// The input method the keys go through, where one is running.
    input_method: Option<InputMethod>,
    /// Another program's drag, while it is over one of the windows.
    incoming: Option<Incoming>,
    /// What this program is dragging, while it is, for the program it is
    /// let go on to ask for.
    dragged: Option<Contents>,
    /// What is being handed over in pieces, to programs that asked for
    /// more than one property should carry.
    transfers: Vec<Transfer>,
}

/// How much of a selection goes in one property. Anything bigger is handed
/// over in pieces of this size — `INCR`, in the conventions' own word — so
/// that no request to the server has to carry the lot, and a program on a
/// server without big requests can take it too.
const INCR_CHUNK: usize = 256 * 1024;

/// How long a program taking a selection in pieces may leave a piece
/// untaken before it is given up on.
const INCR_PATIENCE: Duration = Duration::from_secs(30);

/// A selection being handed over in pieces: to whom, in which property,
/// as what, and how far it has got.
#[derive(Debug)]
struct Transfer {
    requestor: u32,
    property: u32,
    kind: u32,
    data: Vec<u8>,
    sent: usize,
    /// Whether the empty piece that says the end has been written.
    ended: bool,
    last: Instant,
}

/// The version of the drag and drop protocol spoken: the fifth, in which
/// the drop's target says what it did with what it was given.
const XDND_VERSION: u32 = 5;

/// A drag another program is giving, while it is over one of this
/// program's windows.
#[derive(Debug)]
struct Incoming {
    source: u32,
    window: u32,
    /// Whether what is dragged is files, said as their addresses.
    files: bool,
    /// The formats it is offered in.
    types: Vec<u32>,
    /// Whether this program takes any of them.
    accepted: bool,
    /// What the source asked for: a copy, or a move.
    action: u32,
    /// Where it is, in the application's pixels.
    at: (i32, i32),
}

/// The line to an X input method server, and the conversation on it: see
/// [`xim`] for the protocol. The line itself is X's own — client messages
/// between two windows, one each, with window properties for what is too
/// long for a message.
struct InputMethod {
    protocol: xim::Xim,
    /// The server's window, which owns the name it is known by.
    server: u32,
    /// The window the server talks on, once it has said which.
    line: Option<u32>,
    /// This program's end of the line: a window never shown.
    own: u32,
    xconnect: u32,
    message: u32,
    more: u32,
    /// The pieces of a message the server sent in several.
    pieces: Vec<u8>,
    /// Which of the properties a long message goes in next.
    next_property: u32,
    /// When the key the server has not answered went to it.
    asked_at: Option<Instant>,
}

/// How long a key sent to the input method may go unanswered before the
/// input method is taken to be gone and the keys are typed without it.
const INPUT_METHOD_PATIENCE: Duration = Duration::from_secs(3);

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static APPLICATION: RefCell<Option<Box<dyn App>>> = const { RefCell::new(None) };
    /// The window the last event came from.
    static WINDOW: Cell<u32> = const { Cell::new(0) };
}

fn with_state<R>(work: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|slot| {
        let Ok(mut held) = slot.try_borrow_mut() else { return None };
        held.as_mut().map(work)
    })
}

fn with_application<R>(work: impl FnOnce(&mut dyn App) -> R) -> Option<R> {
    APPLICATION.with(|slot| {
        let Ok(mut held) = slot.try_borrow_mut() else { return None };
        held.as_mut().map(|app| work(app.as_mut()))
    })
}

fn failed(failure: &Failure) -> Error {
    Error::WindowCreationFailed(failure.to_string())
}

impl State {
    fn connect() -> Result<Self, Failure> {
        let mut connection = Connection::open()?;
        let mut atom = |name: &str| connection.atom(name);
        let atoms = Atoms {
            wm_protocols: atom("WM_PROTOCOLS")?,
            wm_delete_window: atom("WM_DELETE_WINDOW")?,
            wm_change_state: atom("WM_CHANGE_STATE")?,
            net_wm_name: atom("_NET_WM_NAME")?,
            net_wm_state: atom("_NET_WM_STATE")?,
            net_wm_state_maximized_vert: atom("_NET_WM_STATE_MAXIMIZED_VERT")?,
            net_wm_state_maximized_horz: atom("_NET_WM_STATE_MAXIMIZED_HORZ")?,
            net_wm_moveresize: atom("_NET_WM_MOVERESIZE")?,
            net_client_list_stacking: atom("_NET_CLIENT_LIST_STACKING")?,
            motif_wm_hints: atom("_MOTIF_WM_HINTS")?,
            utf8_string: atom("UTF8_STRING")?,
            clipboard: atom("CLIPBOARD")?,
            targets: atom("TARGETS")?,
            text: atom("TEXT")?,
            text_plain_utf8: atom("text/plain;charset=utf-8")?,
            text_html: atom("text/html")?,
            text_rtf: atom("text/rtf")?,
            image_png: atom("image/png")?,
            incr: atom("INCR")?,
            own_property: atom("WORD_PROCESSOR_SELECTION")?,
            xdnd_aware: atom("XdndAware")?,
            xdnd_enter: atom("XdndEnter")?,
            xdnd_position: atom("XdndPosition")?,
            xdnd_status: atom("XdndStatus")?,
            xdnd_leave: atom("XdndLeave")?,
            xdnd_drop: atom("XdndDrop")?,
            xdnd_finished: atom("XdndFinished")?,
            xdnd_selection: atom("XdndSelection")?,
            xdnd_type_list: atom("XdndTypeList")?,
            xdnd_action_copy: atom("XdndActionCopy")?,
            xdnd_action_move: atom("XdndActionMove")?,
            text_uri_list: atom("text/uri-list")?,
            text_plain: atom("text/plain")?,
        };
        let keymap = connection.keyboard_mapping()?;
        let cursor_font = connection.open_font("cursor")?;
        let scale = screen_scale(&mut connection);
        Ok(Self {
            connection,
            atoms,
            windows: Vec::new(),
            cursors: HashMap::new(),
            cursor_font,
            keymap,
            owned: None,
            alt_alone: false,
            control_alone: false,
            scale,
            input_method: None,
            incoming: None,
            dragged: None,
            transfers: Vec::new(),
        })
    }

    fn window_index(&self, id: u32) -> Option<usize> {
        self.windows.iter().position(|window| window.id == id)
    }

    /// Makes a window, sized in the application's pixels, with its frame
    /// switched off and the manager told what it needs to know.
    fn create_window(&mut self, title: &str, width: u32, height: u32) -> Result<u32, Failure> {
        let scale = self.scale;
        let device_width = (width as f32 * scale).round() as u32;
        let device_height = (height as f32 * scale).round() as u32;
        let id = self.connection.create_window(device_width as u16, device_height as u16)?;
        let gc = self.connection.create_gc(id)?;
        let atoms = &self.atoms;
        let connection = &mut self.connection;
        connection.set_property_atoms(id, atoms.wm_protocols, &[atoms.wm_delete_window])?;
        // No frame from the manager: the program draws its own title bar.
        connection.set_property(id, atoms.motif_wm_hints, atoms.motif_wm_hints, 32, &{
            let mut hints = Vec::new();
            for word in [2u32, 0, 0, 0, 0] {
                hints.extend_from_slice(&word.to_le_bytes());
            }
            hints
        })?;
        connection.set_property(id, x11::ATOM_WM_NAME, x11::ATOM_STRING, 8, title.as_bytes())?;
        connection.set_property(id, atoms.net_wm_name, atoms.utf8_string, 8, title.as_bytes())?;
        // Takes drops, in the fifth version of the protocol.
        connection.set_property(
            id,
            atoms.xdnd_aware,
            x11::ATOM_ATOM,
            32,
            &XDND_VERSION.to_le_bytes(),
        )?;
        let class = connection.atom("WM_CLASS")?;
        connection.set_property(
            id,
            class,
            x11::ATOM_STRING,
            8,
            b"word-processor\0WordProcessor\0",
        )?;
        connection.map_window(id)?;
        connection.flush()?;
        self.windows.push(Window {
            id,
            gc,
            width: device_width,
            height: device_height,
            scale,
            cursor: Cursor::Arrow,
            dirty: true,
            last_press: None,
        });
        Ok(id)
    }

    /// The pointer shape for a cursor, made once.
    fn cursor_id(&mut self, cursor: Cursor) -> Result<u32, Failure> {
        let glyph: u16 = match cursor {
            Cursor::Arrow => 68,
            Cursor::Text => 152,
            Cursor::ResizeHorizontal => 108,
            Cursor::ResizeVertical => 116,
            Cursor::Hand => 60,
        };
        let key = glyph as u8;
        if let Some(id) = self.cursors.get(&key) {
            return Ok(*id);
        }
        let id = self.connection.glyph_cursor(self.cursor_font, glyph)?;
        self.cursors.insert(key, id);
        Ok(id)
    }

    /// The keysym a key press means, with the modifiers held.
    fn keysym(&self, keycode: u8, state: u16) -> u32 {
        keysym_of(&self.keymap, self.connection.setup.min_keycode, keycode, state)
    }
}

/// The keysym a keycode gives under the modifiers held: the shifted one
/// with Shift, the third level with AltGr, and Caps Lock turning letters.
fn keysym_of(keymap: &(usize, Vec<u32>), min: u8, keycode: u8, state: u16) -> u32 {
    let (per, keysyms) = keymap;
    let per = *per;
    if per == 0 || keycode < min {
        return 0;
    }
    let row = usize::from(keycode - min) * per;
    let at = |column: usize| keysyms.get(row + column).copied().unwrap_or(0);
    let shift = state & 0x1 != 0;
    let lock = state & 0x2 != 0;
    let level3 = state & 0x80 != 0 && per >= 4;
    let (base, shifted) = if level3 { (at(2), at(3)) } else { (at(0), at(1)) };
    let base = if base == 0 { at(0) } else { base };
    let mut sym = if shift {
        if shifted == 0 {
            upper(base)
        } else {
            shifted
        }
    } else {
        base
    };
    // Caps Lock turns letters and nothing else.
    if lock && keys::char_of(sym).is_some_and(char::is_alphabetic) {
        sym = if shift { lower(base) } else { upper(base) };
    }
    sym
}

/// A keysym's upper-case letter, where it is a letter.
fn upper(keysym: u32) -> u32 {
    match keys::char_of(keysym).and_then(|c| c.to_uppercase().next()) {
        Some(c) if (c as u32) < 0x100 => c as u32,
        Some(c) => 0x0100_0000 + c as u32,
        None => keysym,
    }
}

fn lower(keysym: u32) -> u32 {
    match keys::char_of(keysym).and_then(|c| c.to_lowercase().next()) {
        Some(c) if (c as u32) < 0x100 => c as u32,
        Some(c) => 0x0100_0000 + c as u32,
        None => keysym,
    }
}

/// The screen's density as a scale: what `Xft.dpi` in the root window's
/// resources says, else an ordinary screen.
fn screen_scale(connection: &mut Connection) -> f32 {
    let root = connection.setup.root;
    if let Ok(atom) = connection.atom("RESOURCE_MANAGER") {
        if let Ok((_, _, bytes)) = connection.get_property(root, atom, false) {
            let text = String::from_utf8_lossy(&bytes);
            for line in text.lines() {
                if let Some(value) = line.strip_prefix("Xft.dpi:") {
                    if let Ok(dpi) = value.trim().parse::<f32>() {
                        if dpi > 0.0 {
                            return (dpi / ORDINARY_DPI).clamp(0.5, 4.0);
                        }
                    }
                }
            }
        }
    }
    1.0
}

/// A point in a window's pixels as one in the application's.
fn to_logical(scale: f32, x: i32, y: i32) -> (i32, i32) {
    ((x as f32 / scale).round() as i32, (y as f32 / scale).round() as i32)
}

fn modifiers_of(state: u16) -> Modifiers {
    Modifiers { control: state & 0x4 != 0, shift: state & 0x1 != 0, alt: state & 0x8 != 0 }
}

/// Hands an event to the application for a window, and acts on the answer.
fn deliver(window: u32, event: Event) -> Response {
    WINDOW.with(|slot| slot.set(window));
    let index = with_state(|state| state.window_index(window)).flatten();
    if let Some(index) = index {
        with_application(|app| app.switch_window(index));
    }
    let response = with_application(|app| app.handle(event)).unwrap_or(Response::Ignored);
    match response {
        // Refused is a question put up in the window, which has to be
        // drawn before it can be answered.
        Response::Redraw | Response::Refuse => {
            with_state(|state| {
                if let Some(index) = state.window_index(window) {
                    state.windows[index].dirty = true;
                }
            });
        }
        Response::Close => close_window(window),
        Response::Ignored => {}
    }
    response
}

fn close_window(window: u32) {
    let actions = with_state(|state| {
        let _ = state.connection.destroy_window(window);
        let _ = state.connection.flush();
        state.windows.retain(|found| found.id != window);
        state
            .input_method
            .as_mut()
            .map(|method| method.protocol.remove_window(window))
            .unwrap_or_default()
    })
    .unwrap_or_default();
    perform(actions);
}

/// Opens the window and runs the event loop.
pub(crate) fn run(options: WindowOptions, app: Box<dyn App>) -> Result<(), Error> {
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(app));
    let state = State::connect().map_err(|failure| failed(&failure))?;
    STATE.with(|slot| *slot.borrow_mut() = Some(state));
    let scale = with_state(|state| state.scale).unwrap_or(1.0);
    let window =
        with_state(|state| state.create_window(&options.title, options.width, options.height))
            .ok_or_else(|| Error::WindowCreationFailed("the shell is not running".to_owned()))?
            .map_err(|failure| failed(&failure))?;
    WINDOW.with(|slot| slot.set(window));
    deliver(window, Event::ScaleChanged { scale });
    deliver(window, Event::Resized { width: options.width, height: options.height });
    start_input_method(window);

    let mut last_tick = Instant::now();
    let mut accessible = false;
    loop {
        // The display and the accessibility bus are waited on together, so
        // that whichever speaks is answered at once; see [`super::wait`].
        let tick = Duration::from_millis(TICK_MILLIS);
        let until_tick = tick.saturating_sub(last_tick.elapsed());
        let display = with_state(|state| (state.connection.raw_fd(), state.connection.buffered()));
        let Some((display_fd, buffered)) = display else { break };
        let ready = buffered || {
            let mut fds = vec![display_fd];
            fds.extend(super::atspi::bus_fd());
            super::wait::readable(&fds, until_tick)[0]
        };
        let packet = if ready {
            with_state(|state| state.connection.next_event(Duration::from_millis(1)))
        } else {
            Some(Ok(None))
        };
        match packet {
            Some(Ok(Some(packet))) => handle_packet(&packet),
            Some(Ok(None)) => {}
            Some(Err(_)) | None => break,
        }
        if last_tick.elapsed() >= Duration::from_millis(TICK_MILLIS) {
            last_tick = Instant::now();
            let ids: Vec<u32> = with_state(|state| state.windows.iter().map(|w| w.id).collect())
                .unwrap_or_default();
            for id in ids {
                deliver(id, Event::Tick);
            }
            check_input_method();
            with_state(forget_stalled_transfers);
            // Joined once the window is up, so that finding the bus does
            // not keep the window from coming up.
            if !accessible {
                accessible = true;
                super::atspi::start();
            }
        }
        if with_state(|state| state.windows.is_empty()).unwrap_or(true) {
            break;
        }
        super::atspi::pump(&mut Reading(WINDOW.with(Cell::get)));
        present_dirty();
    }

    STATE.with(|slot| slot.borrow_mut().take());
    APPLICATION.with(|slot| slot.borrow_mut().take());
    Ok(())
}

/// A window as a screen reader reads it: see [`super::atspi`].
struct Reading(u32);

impl Reading {
    fn application<R>(&self, work: impl FnOnce(&mut dyn App) -> R) -> Option<R> {
        let index = with_state(|state| state.window_index(self.0)).flatten();
        if let Some(index) = index {
            with_application(|app| app.switch_window(index));
        }
        with_application(work)
    }

    /// What pressing or selecting asked for, done.
    fn answered(&self, response: Option<Response>) {
        if response == Some(Response::Redraw) {
            with_state(|state| {
                if let Some(index) = state.window_index(self.0) {
                    state.windows[index].dirty = true;
                }
            });
        }
    }
}

impl super::atspi::Window for Reading {
    fn title(&mut self) -> String {
        with_state(|state| title_of(state, self.0)).unwrap_or_default()
    }

    fn origin(&mut self) -> (i32, i32) {
        with_state(|state| state.connection.translate(self.0, 0, 0).ok())
            .flatten()
            .unwrap_or((0, 0))
    }

    fn size(&mut self) -> (i32, i32) {
        with_state(|state| {
            state.window_index(self.0).map(|index| {
                let window = &state.windows[index];
                (window.width as i32, window.height as i32)
            })
        })
        .flatten()
        .unwrap_or((0, 0))
    }

    fn scale(&mut self) -> f32 {
        with_state(|state| state.window_index(self.0).map(|index| state.windows[index].scale))
            .flatten()
            .unwrap_or(1.0)
    }

    fn elements(&mut self) -> Vec<crate::accessibility::Element> {
        self.application(|app| app.accessible_elements()).unwrap_or_default()
    }

    fn text(&mut self) -> Option<crate::accessibility::TextState> {
        self.application(|app| app.accessible_text()).flatten()
    }

    fn rects(&mut self, start: usize, end: usize) -> Vec<(i32, i32, i32, i32)> {
        self.application(|app| app.accessible_rects(start, end)).unwrap_or_default()
    }

    fn invoke(&mut self, id: u64) {
        let response = self.application(|app| app.accessible_invoke(id));
        self.answered(response);
    }

    fn select(&mut self, start: usize, end: usize) {
        let response = self.application(|app| app.accessible_select(start, end));
        self.answered(response);
    }

    fn lines(&mut self) -> Vec<(usize, usize)> {
        self.application(|app| app.accessible_lines()).unwrap_or_default()
    }

    fn attributes(
        &mut self,
        offset: usize,
    ) -> Option<(crate::accessibility::TextAttributes, usize, usize)> {
        self.application(|app| app.accessible_attributes(offset)).flatten()
    }

    fn set_value(&mut self, id: u64, value: &str) {
        let response = self.application(|app| app.accessible_set_value(id, value));
        self.answered(response);
    }
}

/// Draws every window that asked to be drawn again.
fn present_dirty() {
    let dirty: Vec<(u32, u32, u32)> = with_state(|state| {
        state
            .windows
            .iter_mut()
            .filter(|window| window.dirty && window.width > 0 && window.height > 0)
            .map(|window| {
                window.dirty = false;
                (window.id, window.width, window.height)
            })
            .collect()
    })
    .unwrap_or_default();
    for (id, width, height) in dirty {
        present(id, width, height);
    }
}

/// Draws one window: the application's canvas, put on the window as the
/// server's own pixels.
fn present(id: u32, width: u32, height: u32) {
    let index = with_state(|state| state.window_index(id)).flatten();
    if let Some(index) = index {
        with_application(|app| app.switch_window(index));
    }
    WINDOW.with(|slot| slot.set(id));
    let Some(pixels) =
        with_application(|app| app.draw(width as usize, height as usize).pixels().to_vec())
    else {
        return;
    };
    with_state(|state| {
        let rows = server_pixels(&state.connection.setup, &pixels, width as usize, height as usize);
        let gc = state.windows.iter().find(|window| window.id == id).map_or(0, |window| window.gc);
        let _ = state.connection.put_image(id, gc, width, height, &rows);
    });
}

/// The canvas's red, green, blue, alpha as the server's pixel format.
fn server_pixels(setup: &x11::Setup, pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let bytes_per_pixel = usize::from(setup.bits_per_pixel / 8).max(1);
    let pad = usize::from(setup.scanline_pad / 8).max(1);
    let row_bytes = (width * bytes_per_pixel).div_ceil(pad) * pad;
    let shift = |mask: u32| mask.trailing_zeros();
    let (red, green, blue) =
        (shift(setup.red_mask), shift(setup.green_mask), shift(setup.blue_mask));
    let mut out = vec![0u8; row_bytes * height];
    for y in 0..height {
        for x in 0..width {
            let at = (y * width + x) * 4;
            let Some(pixel) = pixels.get(at..at + 4) else { continue };
            let value = (u32::from(pixel[0]) << red)
                | (u32::from(pixel[1]) << green)
                | (u32::from(pixel[2]) << blue);
            let bytes =
                if setup.image_byte_order_msb { value.to_be_bytes() } else { value.to_le_bytes() };
            let start = y * row_bytes + x * bytes_per_pixel;
            match bytes_per_pixel {
                4 => out[start..start + 4].copy_from_slice(&bytes),
                3 => {
                    // Three bytes a pixel, in the byte order's order.
                    if setup.image_byte_order_msb {
                        out[start..start + 3].copy_from_slice(&bytes[1..4]);
                    } else {
                        out[start..start + 3].copy_from_slice(&bytes[..3]);
                    }
                }
                2 => out[start..start + 2].copy_from_slice(&(value as u16).to_le_bytes()),
                _ => out[start] = value as u8,
            }
        }
    }
    out
}

/// The server's pixels as red, green, blue, alpha.
fn canvas_pixels(setup: &x11::Setup, rows: &[u8], width: usize, height: usize) -> Vec<u8> {
    let bytes_per_pixel = usize::from(setup.bits_per_pixel / 8).max(1);
    let pad = usize::from(setup.scanline_pad / 8).max(1);
    let row_bytes = (width * bytes_per_pixel).div_ceil(pad) * pad;
    let channel = |value: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 0;
        }
        let bits = mask.count_ones();
        let taken = (value & mask) >> mask.trailing_zeros();
        if bits >= 8 {
            (taken >> (bits - 8)) as u8
        } else {
            ((taken << (8 - bits)) | (taken >> (2 * bits).saturating_sub(8))) as u8
        }
    };
    let mut out = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let start = y * row_bytes + x * bytes_per_pixel;
            let mut value = 0u32;
            for offset in 0..bytes_per_pixel.min(4) {
                let byte = u32::from(rows.get(start + offset).copied().unwrap_or(0));
                value |= if setup.image_byte_order_msb {
                    byte << (8 * (bytes_per_pixel - 1 - offset))
                } else {
                    byte << (8 * offset)
                };
            }
            let at = (y * width + x) * 4;
            out[at] = channel(value, setup.red_mask);
            out[at + 1] = channel(value, setup.green_mask);
            out[at + 2] = channel(value, setup.blue_mask);
            out[at + 3] = 255;
        }
    }
    out
}

/// Acts on one packet from the server.
fn handle_packet(packet: &Packet) {
    let kind = packet.kind();
    let window = match kind {
        x11::KEY_PRESS
        | x11::KEY_RELEASE
        | x11::BUTTON_PRESS
        | x11::BUTTON_RELEASE
        | x11::MOTION_NOTIFY
        | x11::LEAVE_NOTIFY => packet.u32_at(12),
        x11::CLIENT_MESSAGE if is_input_method_window(packet.u32_at(4)) => {
            input_method_message(packet);
            return;
        }
        x11::DESTROY_NOTIFY if is_input_method_server(packet.u32_at(8)) => {
            input_method_gone();
            return;
        }
        x11::EXPOSE | x11::CLIENT_MESSAGE | x11::FOCUS_IN | x11::FOCUS_OUT => packet.u32_at(4),
        x11::CONFIGURE_NOTIFY | x11::DESTROY_NOTIFY => packet.u32_at(8),
        x11::SELECTION_REQUEST => {
            answer_selection_request(packet);
            return;
        }
        x11::SELECTION_CLEAR => {
            with_state(|state| state.owned = None);
            return;
        }
        x11::PROPERTY_NOTIFY => {
            // The property a piece was written into, taken: the next.
            if packet.u8_at(16) == PROPERTY_DELETED {
                with_state(|state| next_piece(state, packet.u32_at(4), packet.u32_at(8)));
            }
            return;
        }
        x11::MAPPING_NOTIFY => {
            with_state(|state| {
                if let Ok(keymap) = state.connection.keyboard_mapping() {
                    state.keymap = keymap;
                }
            });
            return;
        }
        _ => return,
    };
    let Some((index, scale)) = with_state(|state| {
        state.window_index(window).map(|index| (index, state.windows[index].scale))
    })
    .flatten() else {
        return;
    };
    match kind {
        x11::KEY_PRESS => key_press(window, packet),
        x11::KEY_RELEASE => key_release(window, packet),
        x11::BUTTON_PRESS => button_press(window, index, scale, packet),
        x11::BUTTON_RELEASE => {
            let (x, y) =
                to_logical(scale, i32::from(packet.i16_at(24)), i32::from(packet.i16_at(26)));
            match packet.detail() {
                1 => deliver(window, Event::MouseUp { x, y }),
                3 => deliver(
                    window,
                    Event::RightClick { x, y, modifiers: modifiers_of(packet.u16_at(28)) },
                ),
                _ => Response::Ignored,
            };
        }
        x11::MOTION_NOTIFY => {
            let state = packet.u16_at(28);
            let (x, y) =
                to_logical(scale, i32::from(packet.i16_at(24)), i32::from(packet.i16_at(26)));
            update_cursor(window, x, y);
            deliver(
                window,
                Event::MouseMove { x, y, held: state & 0x100 != 0, modifiers: modifiers_of(state) },
            );
        }
        x11::LEAVE_NOTIFY => {
            deliver(window, Event::PointerLeft);
        }
        x11::EXPOSE => {
            if packet.u16_at(16) == 0 {
                with_state(|state| state.windows[index].dirty = true);
            }
        }
        x11::CONFIGURE_NOTIFY => {
            let width = u32::from(packet.u16_at(20));
            let height = u32::from(packet.u16_at(22));
            let changed = with_state(|state| {
                let window = &mut state.windows[index];
                let changed = window.width != width || window.height != height;
                window.width = width;
                window.height = height;
                window.dirty |= changed;
                changed
            })
            .unwrap_or(false);
            if changed {
                let logical_width = (width as f32 / scale).round().max(1.0) as u32;
                let logical_height = (height as f32 / scale).round().max(1.0) as u32;
                deliver(window, Event::Resized { width: logical_width, height: logical_height });
            }
        }
        x11::CLIENT_MESSAGE if is_drop_message(packet.u32_at(8)) => {
            drop_message(window, scale, packet);
        }
        x11::CLIENT_MESSAGE => {
            let (protocols, delete) =
                with_state(|state| (state.atoms.wm_protocols, state.atoms.wm_delete_window))
                    .unwrap_or((0, 0));
            let asked = packet.u32_at(8) == protocols && packet.u32_at(12) == delete;
            if asked && deliver(window, Event::Closing) != Response::Refuse {
                close_window(window);
            }
        }
        x11::DESTROY_NOTIFY => {
            with_state(|state| state.windows.retain(|found| found.id != window));
        }
        x11::FOCUS_IN | x11::FOCUS_OUT => {
            let on = kind == x11::FOCUS_IN;
            let actions = with_state(|state| {
                state
                    .input_method
                    .as_mut()
                    .map(|method| method.protocol.focus(window, on))
                    .unwrap_or_default()
            })
            .unwrap_or_default();
            perform(actions);
        }
        _ => {}
    }
}

fn key_press(window: u32, packet: &Packet) {
    if through_input_method(window, packet, true) {
        return;
    }
    typed(window, packet);
}

/// A key press handled here: the input method did not take it, or gave it
/// back, or there is none.
fn typed(window: u32, packet: &Packet) {
    let state = packet.u16_at(28);
    let Some(keysym) = with_state(|s| s.keysym(packet.detail(), state)) else { return };
    if keys::is_alt(keysym) {
        with_state(|s| s.alt_alone = true);
        return;
    }
    if keys::is_control(keysym) {
        with_state(|s| s.control_alone = true);
        return;
    }
    with_state(|s| {
        s.alt_alone = false;
        s.control_alone = false;
    });
    if keys::is_modifier(keysym) {
        return;
    }
    let modifiers = modifiers_of(state);
    if let Some(key) = keys::key_of(keysym) {
        deliver(window, Event::KeyDown { key, modifiers });
    }
    if !modifiers.control && !modifiers.alt {
        if let Some(character) = keys::char_of(keysym) {
            if !character.is_control() {
                deliver(window, Event::Char(character));
            }
        }
    }
}

fn key_release(window: u32, packet: &Packet) {
    if through_input_method(window, packet, false) {
        return;
    }
    released(window, packet);
}

fn released(window: u32, packet: &Packet) {
    let state = packet.u16_at(28);
    let Some(keysym) = with_state(|s| s.keysym(packet.detail(), state)) else { return };
    if keys::is_alt(keysym)
        && with_state(|s| core::mem::replace(&mut s.alt_alone, false)).unwrap_or(false)
    {
        deliver(window, Event::MenuKey);
    }
    if keys::is_control(keysym)
        && with_state(|s| core::mem::replace(&mut s.control_alone, false)).unwrap_or(false)
    {
        deliver(window, Event::ControlKey);
    }
}

fn button_press(window: u32, index: usize, scale: f32, packet: &Packet) {
    let state = packet.u16_at(28);
    let time = packet.u32_at(4);
    let device = (i32::from(packet.i16_at(24)), i32::from(packet.i16_at(26)));
    let (x, y) = to_logical(scale, device.0, device.1);
    let modifiers = modifiers_of(state);
    match packet.detail() {
        1 => {
            // The edges size the window and the caption moves it, through the
            // manager, which takes the drag from here.
            if let Some(direction) = resize_direction(index, device) {
                begin_move_resize(window, packet, direction);
                return;
            }
            let caption = with_application(|app| app.is_caption(x, y)).unwrap_or(false);
            if caption {
                begin_move_resize(window, packet, 8);
                return;
            }
            let double = with_state(|s| {
                let held = &mut s.windows[index].last_press;
                let double = held.is_some_and(|(then, px, py)| {
                    time.wrapping_sub(then) <= double_click_millis()
                        && (px - x).abs() < 4
                        && (py - y).abs() < 4
                });
                *held = if double { None } else { Some((time, x, y)) };
                double
            })
            .unwrap_or(false);
            if double {
                deliver(window, Event::DoubleClick { x, y });
            } else {
                deliver(window, Event::MouseDown { x, y, modifiers });
            }
        }
        2 => {
            deliver(window, Event::MiddleClick { x, y });
        }
        4 | 5 => {
            let lines = if packet.detail() == 4 { 1.0 } else { -1.0 };
            deliver(window, Event::Scroll { lines, modifiers });
        }
        6 | 7 => {
            let lines = if packet.detail() == 6 { 1.0 } else { -1.0 };
            let sideways = Modifiers { shift: true, ..modifiers };
            deliver(window, Event::Scroll { lines, modifiers: sideways });
        }
        _ => {}
    }
}

/// Which edge or corner of the window the pointer is at, as the manager
/// numbers them, if it is at one.
fn resize_direction(index: usize, (x, y): (i32, i32)) -> Option<u32> {
    let (width, height, maximised) = with_state(|s| {
        let window = &s.windows[index];
        (window.width as i32, window.height as i32, false)
    })?;
    if maximised {
        return None;
    }
    let left = x < RESIZE_BORDER;
    let right = x >= width - RESIZE_BORDER;
    let top = y < RESIZE_BORDER;
    let bottom = y >= height - RESIZE_BORDER;
    Some(match (top, bottom, left, right) {
        (true, _, true, _) => 0,
        (true, _, _, true) => 2,
        (_, true, true, _) => 6,
        (_, true, _, true) => 4,
        (true, ..) => 1,
        (_, true, ..) => 5,
        (_, _, true, _) => 7,
        (_, _, _, true) => 3,
        _ => return None,
    })
}

/// Asks the manager to move or size the window with the pointer.
fn begin_move_resize(window: u32, packet: &Packet, direction: u32) {
    with_state(|state| {
        let root_x = i32::from(packet.i16_at(20)) as u32;
        let root_y = i32::from(packet.i16_at(22)) as u32;
        let kind = state.atoms.net_wm_moveresize;
        let root = state.connection.setup.root;
        let event =
            state.connection.client_message(window, kind, [root_x, root_y, direction, 1, 1]);
        let _ = state.connection.send_event(root, x11::SUBSTRUCTURE_MASK, &event);
        let _ = state.connection.flush();
    });
}

/// Sets the pointer shape for where the pointer is.
fn update_cursor(window: u32, x: i32, y: i32) {
    let Some(wanted) = with_application(|app| app.cursor(x, y)) else { return };
    with_state(|state| {
        let Some(index) = state.window_index(window) else { return };
        if state.windows[index].cursor == wanted {
            return;
        }
        state.windows[index].cursor = wanted;
        if let Ok(id) = state.cursor_id(wanted) {
            let _ = state.connection.set_cursor(window, id);
        }
    });
}

// --- Windows ------------------------------------------------------------------

pub(crate) fn open_window(title: &str) -> bool {
    let made = with_state(|state| state.create_window(title, 1400, 900));
    match made {
        Some(Ok(window)) => {
            let scale = with_state(|state| state.scale).unwrap_or(1.0);
            deliver(window, Event::ScaleChanged { scale });
            deliver(window, Event::Resized { width: 1400, height: 900 });
            let actions = with_state(|state| {
                state
                    .input_method
                    .as_mut()
                    .map(|method| method.protocol.add_window(window))
                    .unwrap_or_default()
            })
            .unwrap_or_default();
            perform(actions);
            true
        }
        _ => false,
    }
}

pub(crate) fn window_count() -> usize {
    with_state(|state| state.windows.len()).unwrap_or(0)
}

/// Lays the windows out side by side across the screen.
pub(crate) fn arrange_windows() -> usize {
    with_state(|state| {
        let count = state.windows.len();
        if count == 0 {
            return 0;
        }
        let screen_width = u32::from(state.connection.setup.screen_width);
        let screen_height = u32::from(state.connection.setup.screen_height);
        let width = screen_width / count as u32;
        let ids: Vec<u32> = state.windows.iter().map(|w| w.id).collect();
        for (index, id) in ids.iter().enumerate() {
            let _ = state.connection.configure_window(
                *id,
                Some((index as u32 * width) as i32),
                Some(0),
                Some(width),
                Some(screen_height),
            );
        }
        let _ = state.connection.flush();
        count
    })
    .unwrap_or(0)
}

pub(crate) fn is_maximised() -> bool {
    let window = WINDOW.with(Cell::get);
    with_state(|state| {
        let atoms = &state.atoms;
        let Ok((_, _, bytes)) = state.connection.get_property(window, atoms.net_wm_state, false)
        else {
            return false;
        };
        bytes
            .chunks_exact(4)
            .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
            .any(|atom| {
                atom == atoms.net_wm_state_maximized_horz
                    || atom == atoms.net_wm_state_maximized_vert
            })
    })
    .unwrap_or(false)
}

pub(crate) fn window_command(command: WindowCommand) {
    let window = WINDOW.with(Cell::get);
    match command {
        WindowCommand::Close => {
            // Through the same message the window manager sends, so the
            // application is asked about unsaved changes exactly as it is
            // for the frame's own button — and asked on the way round,
            // since it may be busy answering an event as it asks.
            with_state(|state| {
                let (protocols, delete) = (state.atoms.wm_protocols, state.atoms.wm_delete_window);
                let event =
                    state.connection.client_message(window, protocols, [delete, 0, 0, 0, 0]);
                let _ = state.connection.send_event(window, 0, &event);
                let _ = state.connection.flush();
            });
        }
        WindowCommand::Minimise => {
            with_state(|state| {
                let root = state.connection.setup.root;
                let kind = state.atoms.wm_change_state;
                let event = state.connection.client_message(window, kind, [3, 0, 0, 0, 0]);
                let _ = state.connection.send_event(root, x11::SUBSTRUCTURE_MASK, &event);
                let _ = state.connection.flush();
            });
        }
        WindowCommand::ToggleMaximise => {
            with_state(|state| {
                let root = state.connection.setup.root;
                let (kind, vert, horz) = (
                    state.atoms.net_wm_state,
                    state.atoms.net_wm_state_maximized_vert,
                    state.atoms.net_wm_state_maximized_horz,
                );
                let event = state.connection.client_message(window, kind, [2, vert, horz, 1, 0]);
                let _ = state.connection.send_event(root, x11::SUBSTRUCTURE_MASK, &event);
                let _ = state.connection.flush();
            });
        }
    }
}

pub(crate) fn set_window_title(title: &str) {
    let window = WINDOW.with(Cell::get);
    with_state(|state| {
        let (name, utf8) = (state.atoms.net_wm_name, state.atoms.utf8_string);
        let _ = state.connection.set_property(
            window,
            x11::ATOM_WM_NAME,
            x11::ATOM_STRING,
            8,
            title.as_bytes(),
        );
        let _ = state.connection.set_property(window, name, utf8, 8, title.as_bytes());
        let _ = state.connection.flush();
    });
}

pub(crate) fn double_click_millis() -> u32 {
    400
}

pub(crate) fn caret_blink_millis() -> Option<u32> {
    Some(530)
}

pub(crate) fn system_code_pages() -> (u32, u32) {
    (1252, 437)
}

/// There is no input method here yet, so there is nothing to place.
/// Tells the input method where the caret is, so that its list of
/// candidates opens beside it: in the window's own pixels, at the foot of
/// the caret.
pub(crate) fn place_composition(x: i32, y: i32, height: i32) {
    let window = WINDOW.with(Cell::get);
    let actions = with_state(|state| {
        let scale = state.window_index(window).map_or(state.scale, |i| state.windows[i].scale);
        let method = state.input_method.as_mut()?;
        let device = |value: i32| (value as f32 * scale).round().clamp(-32768.0, 32767.0) as i16;
        Some(method.protocol.spot(window, device(x), device(y + height)))
    })
    .flatten()
    .unwrap_or_default();
    perform(actions);
}

// --- The input method -----------------------------------------------------------

/// The name `XMODIFIERS` gives the input method: `@im=ibus` names `ibus`.
/// Nothing where it names none, which is what every X program takes it to
/// mean, and nothing for `none`.
fn input_method_name(modifiers: &str) -> Option<String> {
    let at = modifiers.find("@im=")?;
    let rest = &modifiers[at + 4..];
    let name = rest.split('@').next().unwrap_or("").trim();
    (!name.is_empty() && name != "none").then(|| name.to_owned())
}

/// The locale the program runs in, as the input method is opened in it.
fn locale_name() -> String {
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| "C".to_owned())
}

/// Finds the input method `XMODIFIERS` names and opens the line to it:
/// the server's window is the owner of its name, a window of this
/// program's own is the other end, and the server is asked to answer with
/// the window it will talk on. Everything after goes through the event
/// loop as it arrives.
fn start_input_method(window: u32) {
    let Some(name) = std::env::var("XMODIFIERS").ok().and_then(|value| input_method_name(&value))
    else {
        return;
    };
    with_state(|state| {
        let connection = &mut state.connection;
        let Ok(selection) = connection.atom(&format!("@server={name}")) else { return };
        let server = match connection.selection_owner(selection) {
            Ok(owner) if owner != 0 => owner,
            _ => return,
        };
        let (Ok(xconnect), Ok(message), Ok(more)) = (
            connection.atom("_XIM_XCONNECT"),
            connection.atom("_XIM_PROTOCOL"),
            connection.atom("_XIM_MOREDATA"),
        ) else {
            return;
        };
        let Ok(own) = connection.create_window(1, 1) else { return };
        // Told when the server's window goes, which is the server gone.
        let _ = connection.select_input(server, 0x0002_0000);
        let event = connection.client_message(server, xconnect, [own, 0, 0, 0, 0]);
        let _ = connection.send_event(server, 0, &event);
        let _ = connection.flush();
        let mut protocol = xim::Xim::new(&locale_name());
        let _ = protocol.add_window(window);
        state.input_method = Some(InputMethod {
            protocol,
            server,
            line: None,
            own,
            xconnect,
            message,
            more,
            pieces: Vec::new(),
            next_property: 0,
            asked_at: None,
        });
    });
}

fn is_input_method_window(window: u32) -> bool {
    with_state(|state| state.input_method.as_ref().is_some_and(|method| method.own == window))
        .unwrap_or(false)
}

fn is_input_method_server(window: u32) -> bool {
    with_state(|state| {
        state
            .input_method
            .as_ref()
            .is_some_and(|method| method.server == window || method.line == Some(window))
    })
    .unwrap_or(false)
}

/// A client message on this program's end of the line: the server's
/// answer to the opening, or a message, whole or in pieces, in the
/// message itself or in a property it names.
fn input_method_message(packet: &Packet) {
    let actions = with_state(|state| {
        let State { connection, input_method, .. } = state;
        let Some(method) = input_method.as_mut() else { return Vec::new() };
        let kind = packet.u32_at(8);
        let data = packet.bytes.get(12..32).unwrap_or(&[]);
        if kind == method.xconnect {
            method.line = Some(packet.u32_at(12));
            // Told when this window goes as well, if it is not the other.
            let _ = connection.select_input(packet.u32_at(12), 0x0002_0000);
            return vec![xim::Action::Send(method.protocol.connect())];
        }
        if kind == method.more {
            method.pieces.extend_from_slice(data);
            return Vec::new();
        }
        if kind != method.message {
            return Vec::new();
        }
        if packet.detail() == 32 {
            let length = packet.u32_at(12) as usize;
            let property = packet.u32_at(16);
            if let Ok((kind, format, bytes)) = connection.get_property(method.own, property, true) {
                let taken = length.min(bytes.len());
                method.pieces.extend_from_slice(&bytes[..taken]);
                // A server adds each message to the end of the property,
                // and says of each how long it is: what is past this one is
                // the next, and goes back for the message that names it.
                if taken < bytes.len() {
                    let _ = connection.set_property(
                        method.own,
                        property,
                        kind,
                        format,
                        &bytes[taken..],
                    );
                }
            }
        } else {
            method.pieces.extend_from_slice(data);
        }
        // Whole messages, one after another; what is left over is the
        // padding of the last client message.
        let pieces = std::mem::take(&mut method.pieces);
        let mut actions = Vec::new();
        let mut at = 0;
        while at + 4 <= pieces.len() {
            let length = 4 + usize::from(u16::from_le_bytes([pieces[at + 2], pieces[at + 3]])) * 4;
            if at + length > pieces.len() {
                break;
            }
            actions.extend(method.protocol.receive(&pieces[at..at + length]));
            at += length;
            if pieces[at..].iter().all(|&byte| byte == 0) {
                break;
            }
        }
        // The server is there: a key still waiting has been waited for
        // from now.
        method.asked_at = method.protocol.is_waiting().then(Instant::now);
        actions
    })
    .unwrap_or_default();
    perform(actions);
    // A server that refused what had to be agreed is no input method.
    let failed = with_state(|state| {
        state.input_method.as_ref().is_some_and(|method| method.protocol.has_failed())
    })
    .unwrap_or(false);
    if failed {
        input_method_gone();
    }
}

/// Does what the conversation asks: sends, delivers, or types a key the
/// input method gave back.
fn perform(actions: Vec<xim::Action>) {
    for action in actions {
        match action {
            xim::Action::Send(bytes) => {
                with_state(|state| send_to_input_method(state, &bytes));
            }
            xim::Action::Deliver(window, event) => {
                deliver(window, event);
            }
            xim::Action::Key(window, event) => {
                let packet = Packet { bytes: event.to_vec() };
                if packet.kind() == x11::KEY_PRESS {
                    typed(window, &packet);
                } else if packet.kind() == x11::KEY_RELEASE {
                    released(window, &packet);
                }
            }
        }
    }
}

/// Puts a message on the line: in one client message where it fits, which
/// is twenty bytes; otherwise in a property on the server's window, with a
/// client message saying which and how long.
fn send_to_input_method(state: &mut State, bytes: &[u8]) {
    let State { connection, input_method, .. } = state;
    let Some(method) = input_method.as_mut() else { return };
    let Some(line) = method.line else { return };
    if bytes.len() <= 20 {
        let mut event = [0u8; 32];
        event[0] = x11::CLIENT_MESSAGE;
        event[1] = 8;
        event[4..8].copy_from_slice(&line.to_le_bytes());
        event[8..12].copy_from_slice(&method.message.to_le_bytes());
        event[12..12 + bytes.len()].copy_from_slice(bytes);
        let _ = connection.send_event(line, 0, &event);
    } else {
        // A property of its own for each message in flight, so that one
        // the server has yet to read is not written over.
        let name = format!("_WORD_PROCESSOR_XIM_{}", method.next_property % 16);
        method.next_property = method.next_property.wrapping_add(1);
        let Ok(property) = connection.atom(&name) else { return };
        let _ = connection.set_property(line, property, x11::ATOM_STRING, 8, bytes);
        let event = connection.client_message(
            line,
            method.message,
            [bytes.len() as u32, property, 0, 0, 0],
        );
        let _ = connection.send_event(line, 0, &event);
    }
    let _ = connection.flush();
}

/// A key through the input method, if there is one and it takes the key.
/// Whether it did: a key it took comes back if it is not wanted.
fn through_input_method(window: u32, packet: &Packet, press: bool) -> bool {
    let bits = packet.u16_at(28);
    let outcome = with_state(|state| {
        let keysym = state.keysym(packet.detail(), bits);
        let method = state.input_method.as_mut()?;
        let mut event = [0u8; 32];
        event.copy_from_slice(packet.bytes.get(..32)?);
        let (taken, actions) = method.protocol.key(window, &event, keysym, bits, press);
        if method.protocol.is_waiting() && method.asked_at.is_none() {
            method.asked_at = Some(Instant::now());
        }
        Some((taken, actions))
    })
    .flatten();
    let Some((taken, actions)) = outcome else { return false };
    perform(actions);
    taken
}

/// An input method that has not answered a key for too long is taken to be
/// gone, and the keys that waited on it are typed.
fn check_input_method() {
    let late = with_state(|state| {
        state
            .input_method
            .as_ref()
            .and_then(|method| method.asked_at)
            .is_some_and(|asked| asked.elapsed() > INPUT_METHOD_PATIENCE)
    })
    .unwrap_or(false);
    if late {
        input_method_gone();
    }
}

/// The input method went away: typing goes on without it, the keys that
/// waited for it included.
fn input_method_gone() {
    let actions = with_state(|state| {
        let mut method = state.input_method.take()?;
        let _ = state.connection.destroy_window(method.own);
        let _ = state.connection.flush();
        Some(method.protocol.abandon())
    })
    .flatten()
    .unwrap_or_default();
    // Whatever was being composed will not be finished now.
    let ids: Vec<u32> = with_state(|state| state.windows.iter().map(|window| window.id).collect())
        .unwrap_or_default();
    for id in ids {
        deliver(id, Event::ComposeEnd);
    }
    perform(actions);
}

/// The screen reader is told on the loop's next turn: see [`super::atspi`].
pub(crate) fn selection_changed() {
    super::atspi::note_selection_changed();
}

pub(crate) fn set_frame_appearance(_dark: bool, _border: (u8, u8, u8), _caption: (u8, u8, u8)) {}

// --- Dragging and dropping ---------------------------------------------------------
//
// XDND, the protocol every X desktop drags by: client messages between the
// program giving the drag and the window it is over — the drag has come,
// where it is now, whether it would be taken and how, it has gone, it was
// let go — and the thing itself handed over as a selection of its own,
// `XdndSelection`, in whichever format the taker asks for.

/// Whether a client message is one of a drag's.
fn is_drop_message(kind: u32) -> bool {
    with_state(|state| {
        let atoms = &state.atoms;
        [atoms.xdnd_enter, atoms.xdnd_position, atoms.xdnd_leave, atoms.xdnd_drop].contains(&kind)
    })
    .unwrap_or(false)
}

/// A client message word: the protocol's `data.l[index]`.
fn word(packet: &Packet, index: usize) -> u32 {
    packet.u32_at(12 + index * 4)
}

/// The formats a drop is taken in, text first as the richest of them is
/// asked for afterwards.
fn taken_formats(atoms: &Atoms) -> [u32; 7] {
    [
        atoms.utf8_string,
        atoms.text_plain_utf8,
        atoms.text_plain,
        x11::ATOM_STRING,
        atoms.text_html,
        atoms.text_rtf,
        atoms.image_png,
    ]
}

/// Another program's drag, over one of this program's windows.
fn drop_message(window: u32, scale: f32, packet: &Packet) {
    let kind = packet.u32_at(8);
    let event = with_state(|state| {
        let atoms = &state.atoms;
        if kind == atoms.xdnd_enter {
            let source = word(packet, 0);
            let types: Vec<u32> = if word(packet, 1) & 1 != 0 {
                let list = atoms.xdnd_type_list;
                state
                    .connection
                    .get_property(source, list, false)
                    .map(|(_, _, bytes)| {
                        bytes
                            .chunks_exact(4)
                            .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                (2..5).map(|index| word(packet, index)).filter(|atom| *atom != 0).collect()
            };
            let atoms = &state.atoms;
            let files = types.contains(&atoms.text_uri_list);
            let accepted = files || taken_formats(atoms).iter().any(|atom| types.contains(atom));
            state.incoming = Some(Incoming {
                source,
                window,
                files,
                types,
                accepted,
                action: atoms.xdnd_action_copy,
                at: (0, 0),
            });
            return None;
        }
        if kind == atoms.xdnd_position {
            let (copy, moving, status) =
                (atoms.xdnd_action_copy, atoms.xdnd_action_move, atoms.xdnd_status);
            let root = word(packet, 2);
            let asked = word(packet, 4);
            let (left, top) = state.connection.translate(window, 0, 0).unwrap_or((0, 0));
            let x = (root >> 16) as i16 as i32 - left;
            let y = (root & 0xFFFF) as i16 as i32 - top;
            let at = to_logical(scale, x, y);
            // Said of the window it came into, or of none.
            let incoming = state.incoming.as_mut().filter(|incoming| incoming.window == window)?;
            incoming.at = at;
            // A move where one is asked for; a copy otherwise, which is
            // every other action a source could name.
            incoming.action = if asked == moving { moving } else { copy };
            let (source, accepted, action, files) =
                (incoming.source, incoming.accepted, incoming.action, incoming.files);
            let flags = if accepted { 0b11 } else { 0b10 };
            let answer = state.connection.client_message(
                source,
                status,
                [window, flags, 0, 0, if accepted { action } else { 0 }],
            );
            let _ = state.connection.send_event(source, 0, &answer);
            let _ = state.connection.flush();
            return (accepted && !files).then_some(Event::DataDragOver { x: at.0, y: at.1 });
        }
        if kind == atoms.xdnd_leave {
            let incoming = state.incoming.take()?;
            return (incoming.accepted && !incoming.files).then_some(Event::DataDragLeft);
        }
        if kind == atoms.xdnd_drop {
            let time = word(packet, 2);
            return dropped(state, window, time);
        }
        None
    })
    .flatten();
    if let Some(event) = event {
        deliver(window, event);
    }
}

/// The drag was let go on a window: what it carried is asked for, in the
/// formats this program takes, and the source told it is done with.
fn dropped(state: &mut State, window: u32, time: u32) -> Option<Event> {
    let incoming = state.incoming.take()?;
    let finished = state.atoms.xdnd_finished;
    let selection = state.atoms.xdnd_selection;
    let mut event = None;
    if incoming.accepted {
        let (x, y) = incoming.at;
        if incoming.files {
            let list = state.atoms.text_uri_list;
            let paths = fetch_from(state, window, selection, list, time)
                .map(|bytes| super::files::paths_of_uri_list(&String::from_utf8_lossy(&bytes)))
                .unwrap_or_default();
            if !paths.is_empty() {
                event = Some(Event::FilesDropped { paths, x, y });
            }
        } else {
            let offered = |atom: u32| incoming.types.contains(&atom);
            let [utf8, plain_utf8, plain, string, html, rtf, png] = taken_formats(&state.atoms);
            let mut text = None;
            for format in [utf8, plain_utf8, plain] {
                if text.is_none() && offered(format) {
                    text = fetch_from(state, window, selection, format, time)
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                }
            }
            if text.is_none() && offered(string) {
                text = fetch_from(state, window, selection, string, time)
                    .map(|bytes| bytes.iter().map(|b| char::from(*b)).collect());
            }
            let mut fetch = |format: u32| {
                offered(format)
                    .then(|| fetch_from(state, window, selection, format, time))
                    .flatten()
            };
            let contents = Contents {
                text,
                html: fetch(html),
                rtf: fetch(rtf),
                png: fetch(png),
                dib: None,
                document: None,
            };
            if !contents.is_empty() {
                event = Some(Event::DataDropped {
                    contents,
                    x,
                    y,
                    copying: incoming.action != state.atoms.xdnd_action_move,
                });
            }
        }
    }
    let success = u32::from(event.is_some());
    let action = if event.is_some() { incoming.action } else { 0 };
    let answer =
        state.connection.client_message(incoming.source, finished, [window, success, action, 0, 0]);
    let _ = state.connection.send_event(incoming.source, 0, &answer);
    let _ = state.connection.flush();
    event
}

/// A window that takes drops, under a point on the screen, and the
/// version of the protocol it speaks: the deepest window there that says it
/// takes them, which is the program's own window under a window manager's
/// frame.
fn aware_window_at(state: &mut State, x: i16, y: i16) -> Option<(u32, u32)> {
    let aware = state.atoms.xdnd_aware;
    let mut current = state.connection.setup.root;
    for _ in 0..16 {
        let child = state.connection.child_at(current, x, y).ok()?;
        if child == 0 {
            return None;
        }
        if let Ok((kind, 32, bytes)) = state.connection.get_property(child, aware, false) {
            if kind == x11::ATOM_ATOM && bytes.len() >= 4 {
                return Some((child, u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])));
            }
        }
        current = child;
    }
    None
}

/// The window a drag is over, as the drag sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Over {
    window: u32,
    version: u32,
    /// Whether it is one of this program's own windows.
    own: bool,
}

/// Gives the contents to the desktop as a drag, until it is let go or
/// given up. The window's own button press has the pointer already; it is
/// taken outright for the length of the drag, so that the drag follows it
/// across the screen.
pub(crate) fn start_drag(contents: &Contents) -> DragEffect {
    let window = WINDOW.with(Cell::get);
    let (effect, kept) = with_state(|state| drag_out(state, window, contents))
        .unwrap_or((DragEffect::None, Vec::new()));
    // What arrived meanwhile and was not the drag's goes back to the loop.
    with_state(|state| {
        for packet in kept.into_iter().rev() {
            state.connection.push_front(packet);
        }
    });
    effect
}

fn drag_out(state: &mut State, window: u32, contents: &Contents) -> (DragEffect, Vec<Packet>) {
    let atoms = &state.atoms;
    let (selection, type_list, enter, position, status, leave, drop, finished) = (
        atoms.xdnd_selection,
        atoms.xdnd_type_list,
        atoms.xdnd_enter,
        atoms.xdnd_position,
        atoms.xdnd_status,
        atoms.xdnd_leave,
        atoms.xdnd_drop,
        atoms.xdnd_finished,
    );
    let (copy_action, move_action) = (atoms.xdnd_action_copy, atoms.xdnd_action_move);
    let mut types = Vec::new();
    if contents.text.is_some() {
        types.extend([
            atoms.utf8_string,
            atoms.text_plain_utf8,
            atoms.text_plain,
            x11::ATOM_STRING,
        ]);
    }
    if contents.html.is_some() {
        types.push(atoms.text_html);
    }
    if contents.rtf.is_some() {
        types.push(atoms.text_rtf);
    }
    if contents.png.is_some() {
        types.push(atoms.image_png);
    }
    if types.is_empty() {
        return (DragEffect::None, Vec::new());
    }
    state.dragged = Some(contents.clone());
    let connection = &mut state.connection;
    let list: Vec<u8> = types.iter().flat_map(|atom| atom.to_le_bytes()).collect();
    let _ = connection.set_property(window, type_list, x11::ATOM_ATOM, 32, &list);
    let _ = connection.set_selection_owner(selection, window);
    // Motion and the button's release, wherever the pointer goes.
    let cursor = state.cursor_id(Cursor::Hand).unwrap_or(0);
    let _ = state.connection.grab_pointer(window, 0x0008 | 0x0040, cursor);
    let _ = state.connection.flush();

    let mut over: Option<Over> = None;
    let mut accepted = false;
    let mut accepted_action = 0u32;
    let mut awaiting_status = false;
    let mut pending_position: Option<(u32, u32)> = None;
    let mut copying = false;
    let mut last_root = (0i16, 0i16);
    let mut kept = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut effect = DragEffect::None;

    // The messages to the window under the pointer.
    let send = |state: &mut State, to: u32, kind: u32, data: [u32; 5]| {
        let event = state.connection.client_message(to, kind, data);
        let _ = state.connection.send_event(to, 0, &event);
        let _ = state.connection.flush();
    };
    let enter_data = |version: u32| {
        let more = u32::from(types.len() > 3);
        let first = |index: usize| types.get(index).copied().unwrap_or(0);
        [window, (version.min(XDND_VERSION) << 24) | more, first(0), first(1), first(2)]
    };

    while Instant::now() < deadline {
        let Ok(Some(packet)) = state.connection.read_packet(Some(Duration::from_millis(50))) else {
            continue;
        };
        match packet.kind() {
            x11::MOTION_NOTIFY => {
                let (root_x, root_y) = (packet.i16_at(20), packet.i16_at(22));
                last_root = (root_x, root_y);
                copying = packet.u16_at(28) & 0x4 != 0;
                let time = packet.u32_at(4);
                let found = aware_window_at(state, root_x, root_y).map(|(found, version)| Over {
                    window: found,
                    version,
                    own: state.window_index(found).is_some(),
                });
                if found.map(|o| o.window) != over.map(|o| o.window) {
                    if let Some(old) = over.filter(|old| !old.own) {
                        send(state, old.window, leave, [window, 0, 0, 0, 0]);
                    }
                    accepted = false;
                    awaiting_status = false;
                    pending_position = None;
                    if let Some(new) = found.filter(|new| !new.own && new.version >= 3) {
                        send(state, new.window, enter, enter_data(new.version));
                    }
                    over = found.filter(|new| new.own || new.version >= 3);
                }
                if let Some(target) = over.filter(|target| !target.own) {
                    let place = ((root_x as u16 as u32) << 16) | (root_y as u16 as u32);
                    let action = if copying { copy_action } else { move_action };
                    if awaiting_status {
                        // One position at a time: the next goes when this
                        // one is answered.
                        pending_position = Some((place, time));
                    } else {
                        send(state, target.window, position, [window, 0, place, time, action]);
                        awaiting_status = true;
                    }
                }
            }
            x11::CLIENT_MESSAGE if packet.u32_at(8) == status => {
                accepted = word(&packet, 1) & 1 != 0;
                accepted_action = word(&packet, 4);
                awaiting_status = false;
                if let (Some(target), Some((place, time))) = (over, pending_position.take()) {
                    let action = if copying { copy_action } else { move_action };
                    send(state, target.window, position, [window, 0, place, time, action]);
                    awaiting_status = true;
                }
            }
            x11::BUTTON_RELEASE => {
                let time = packet.u32_at(4);
                match over {
                    Some(target) if target.own => {
                        let index = state.window_index(target.window).unwrap_or(0);
                        let scale = state.windows[index].scale;
                        let (left, top) =
                            state.connection.translate(target.window, 0, 0).unwrap_or((0, 0));
                        let (x, y) = to_logical(
                            scale,
                            i32::from(last_root.0) - left,
                            i32::from(last_root.1) - top,
                        );
                        effect = DragEffect::DroppedOnSelf { x, y, copying };
                    }
                    Some(target) if accepted => {
                        send(state, target.window, drop, [window, 0, time, 0, 0]);
                        effect = wait_for_finish(state, finished, accepted_action, &mut kept);
                    }
                    Some(target) => send(state, target.window, leave, [window, 0, 0, 0, 0]),
                    None => {}
                }
                break;
            }
            x11::KEY_PRESS => {
                // Escape gives the drag up.
                let keysym = state.keysym(packet.detail(), packet.u16_at(28));
                if keys::key_of(keysym) == Some(crate::Key::Escape) {
                    if let Some(target) = over.filter(|target| !target.own) {
                        send(state, target.window, leave, [window, 0, 0, 0, 0]);
                    }
                    break;
                }
            }
            x11::SELECTION_REQUEST => answer_selection_request_in(state, &packet),
            _ => kept.push(packet),
        }
    }
    let _ = state.connection.ungrab_pointer();
    let _ = state.connection.flush();
    state.dragged = None;
    (effect, kept)
}

/// After the drop, the target asks for what it was given and then says it
/// is finished — and, in the fifth version, what it did: took a copy, or
/// took it as a move. Waited for a while, answering its requests meanwhile.
fn wait_for_finish(
    state: &mut State,
    finished: u32,
    accepted_action: u32,
    kept: &mut Vec<Packet>,
) -> DragEffect {
    let move_action = state.atoms.xdnd_action_move;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let Ok(Some(packet)) = state.connection.read_packet(Some(Duration::from_millis(50))) else {
            continue;
        };
        match packet.kind() {
            x11::SELECTION_REQUEST => answer_selection_request_in(state, &packet),
            x11::CLIENT_MESSAGE if packet.u32_at(8) == finished => {
                let flags = word(&packet, 1);
                let action = word(&packet, 2);
                // A target of the fourth version says nothing of what it
                // did: what it said it would do stands.
                let (success, action) =
                    if action == 0 { (true, accepted_action) } else { (flags & 1 != 0, action) };
                return match (success, action == move_action) {
                    (false, _) => DragEffect::None,
                    (true, true) => DragEffect::Move,
                    (true, false) => DragEffect::Copy,
                };
            }
            _ => kept.push(packet),
        }
    }
    DragEffect::None
}

// --- The clipboard ------------------------------------------------------------

pub(crate) fn clipboard_set_contents(contents: &Contents) -> bool {
    let window = WINDOW.with(Cell::get);
    with_state(|state| {
        let clipboard = state.atoms.clipboard;
        if state.connection.set_selection_owner(clipboard, window).is_err() {
            return false;
        }
        let _ = state.connection.flush();
        let owner = state.connection.selection_owner(clipboard).unwrap_or(0);
        if owner != window {
            return false;
        }
        state.owned = Some(contents.clone());
        true
    })
    .unwrap_or(false)
}

pub(crate) fn clipboard_set_text(text: &str) -> bool {
    clipboard_set_contents(&Contents { text: Some(text.to_owned()), ..Contents::default() })
}

pub(crate) fn clipboard_text() -> Option<String> {
    clipboard_contents().text
}

/// What the clipboard holds: this program's own, or what its owner gives
/// for each format asked.
pub(crate) fn clipboard_contents() -> Contents {
    if let Some(owned) = with_state(|state| state.owned.clone()).flatten() {
        return owned;
    }
    let window = WINDOW.with(Cell::get);
    with_state(|state| {
        let clipboard = state.atoms.clipboard;
        if state.connection.selection_owner(clipboard).unwrap_or(0) == 0 {
            return Contents::default();
        }
        let atoms = (
            state.atoms.utf8_string,
            state.atoms.text_html,
            state.atoms.text_rtf,
            state.atoms.image_png,
            x11::ATOM_STRING,
        );
        let text = fetch_selection(state, window, atoms.0)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .or_else(|| {
                fetch_selection(state, window, atoms.4)
                    .map(|bytes| bytes.iter().map(|b| char::from(*b)).collect())
            });
        Contents {
            text,
            html: fetch_selection(state, window, atoms.1),
            rtf: fetch_selection(state, window, atoms.2),
            png: fetch_selection(state, window, atoms.3),
            dib: None,
            document: None,
        }
    })
    .unwrap_or_default()
}

/// Asks the clipboard's owner for one format, and waits for the answer.
fn fetch_selection(state: &mut State, window: u32, target: u32) -> Option<Vec<u8>> {
    let clipboard = state.atoms.clipboard;
    fetch_from(state, window, clipboard, target, 0)
}

/// Asks a selection's owner for one format as of a moment, and waits for
/// the answer.
fn fetch_from(
    state: &mut State,
    window: u32,
    selection: u32,
    target: u32,
    time: u32,
) -> Option<Vec<u8>> {
    let (property, incr) = (state.atoms.own_property, state.atoms.incr);
    state.connection.convert_selection(window, selection, target, property, time).ok()?;
    state.connection.flush().ok()?;
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut kept = Vec::new();
    let mut answered = None;
    while Instant::now() < deadline {
        let Ok(Some(packet)) = state.connection.read_packet(Some(Duration::from_millis(100)))
        else {
            continue;
        };
        if packet.kind() == x11::SELECTION_NOTIFY
            && packet.u32_at(8) == window
            && packet.u32_at(16) == target
        {
            answered = Some(packet.u32_at(20));
            break;
        }
        if packet.kind() == x11::SELECTION_REQUEST {
            answer_selection_request_in(state, &packet);
            continue;
        }
        kept.push(packet);
    }
    for packet in kept.into_iter().rev() {
        state.connection.push_front(packet);
    }
    let given = answered?;
    if given == 0 {
        return None;
    }
    let (kind, _, bytes) = state.connection.get_property(window, property, true).ok()?;
    if kind == incr {
        // Too big for one property, so handed over in pieces: taking the
        // property off, which was just done, asks for the first, and each
        // piece taken asks for the next, until an empty one says that was
        // all.
        return take_pieces(state, window, property);
    }
    Some(bytes)
}

/// The pieces of a selection handed over with `INCR`, put together. Each
/// arrives as the property being written again, which the window hears of
/// since it asks for its property changes; a piece is waited for as long
/// as the owner keeps writing them.
fn take_pieces(state: &mut State, window: u32, property: u32) -> Option<Vec<u8>> {
    let mut whole = Vec::new();
    let mut kept = Vec::new();
    let mut last = Instant::now();
    let taken = loop {
        if last.elapsed() > Duration::from_secs(2) {
            break None;
        }
        let Ok(Some(packet)) = state.connection.read_packet(Some(Duration::from_millis(100)))
        else {
            continue;
        };
        let written = packet.kind() == x11::PROPERTY_NOTIFY
            && packet.u32_at(4) == window
            && packet.u32_at(8) == property
            && packet.u8_at(16) != PROPERTY_DELETED;
        if !written {
            if packet.kind() == x11::SELECTION_REQUEST {
                answer_selection_request_in(state, &packet);
            } else if packet.kind() == x11::PROPERTY_NOTIFY && packet.u8_at(16) == PROPERTY_DELETED
            {
                // Something of this program's own being handed over in
                // pieces meanwhile goes on.
                next_piece(state, packet.u32_at(4), packet.u32_at(8));
            } else {
                kept.push(packet);
            }
            continue;
        }
        last = Instant::now();
        let Ok((kind, _, piece)) = state.connection.get_property(window, property, true) else {
            break None;
        };
        let _ = state.connection.flush();
        // Told of a writing that is no longer there — the INCR itself,
        // heard before the answer and already taken — and not a piece: a
        // piece, even the empty last one, has a type.
        if kind == 0 {
            continue;
        }
        if piece.is_empty() {
            break Some(core::mem::take(&mut whole));
        }
        whole.extend_from_slice(&piece);
    };
    for packet in kept.into_iter().rev() {
        state.connection.push_front(packet);
    }
    taken
}

fn answer_selection_request(packet: &Packet) {
    with_state(|state| answer_selection_request_in(state, packet));
}

/// Gives another program what it asks for from the clipboard, or from what
/// is being dragged.
fn answer_selection_request_in(state: &mut State, packet: &Packet) {
    let time = packet.u32_at(4);
    let requestor = packet.u32_at(12);
    let selection = packet.u32_at(16);
    let target = packet.u32_at(20);
    let mut property = packet.u32_at(24);
    if property == 0 {
        property = target;
    }
    let atoms = &state.atoms;
    let owned =
        if selection == atoms.xdnd_selection { state.dragged.clone() } else { state.owned.clone() };
    let mut given = 0u32;
    if let Some(contents) = owned {
        let mut offered: Vec<u32> = vec![atoms.targets];
        if contents.text.is_some() {
            offered.extend([
                atoms.utf8_string,
                x11::ATOM_STRING,
                atoms.text,
                atoms.text_plain_utf8,
                atoms.text_plain,
            ]);
        }
        if contents.html.is_some() {
            offered.push(atoms.text_html);
        }
        if contents.rtf.is_some() {
            offered.push(atoms.text_rtf);
        }
        if contents.png.is_some() {
            offered.push(atoms.image_png);
        }
        let answer: Option<(u32, u8, Vec<u8>)> = if target == atoms.targets {
            Some((x11::ATOM_ATOM, 32, offered.iter().flat_map(|atom| atom.to_le_bytes()).collect()))
        } else if target == atoms.utf8_string
            || target == atoms.text
            || target == atoms.text_plain_utf8
            || target == atoms.text_plain
        {
            // Plain text on a desktop whose every locale is UTF-8 now.
            let kind = if target == atoms.utf8_string { atoms.utf8_string } else { target };
            contents.text.as_ref().map(|text| (kind, 8, text.as_bytes().to_vec()))
        } else if target == x11::ATOM_STRING {
            contents.text.as_ref().map(|text| {
                (
                    x11::ATOM_STRING,
                    8,
                    text.chars().map(|c| if (c as u32) < 0x100 { c as u8 } else { b'?' }).collect(),
                )
            })
        } else if target == atoms.text_html {
            contents.html.as_ref().map(|html| (atoms.text_html, 8, html_body(html)))
        } else if target == atoms.text_rtf {
            contents.rtf.clone().map(|rtf| (atoms.text_rtf, 8, rtf))
        } else if target == atoms.image_png {
            contents.png.clone().map(|png| (atoms.image_png, 8, png))
        } else {
            None
        };
        let incr = atoms.incr;
        if let Some((kind, format, data)) = answer {
            if format == 8 && data.len() > INCR_CHUNK {
                // Too big for one property: the size first, as INCR, and
                // then a piece each time the program takes the last. It is
                // told of its property being taken by asking for its
                // window's property changes.
                let size = u32::try_from(data.len()).unwrap_or(u32::MAX).to_le_bytes();
                let ours = state.window_index(requestor).is_some();
                let started = (ours
                    || state.connection.select_input(requestor, PROPERTY_CHANGE).is_ok())
                    && state.connection.set_property(requestor, property, incr, 32, &size).is_ok();
                if started {
                    state.transfers.retain(|transfer| {
                        (transfer.requestor, transfer.property) != (requestor, property)
                    });
                    state.transfers.push(Transfer {
                        requestor,
                        property,
                        kind,
                        data,
                        sent: 0,
                        ended: false,
                        last: Instant::now(),
                    });
                    given = property;
                }
            } else if state
                .connection
                .set_property(requestor, property, kind, format, &data)
                .is_ok()
            {
                given = property;
            }
        }
    }
    let mut event = [0u8; 32];
    event[0] = x11::SELECTION_NOTIFY;
    event[4..8].copy_from_slice(&time.to_le_bytes());
    event[8..12].copy_from_slice(&requestor.to_le_bytes());
    event[12..16].copy_from_slice(&selection.to_le_bytes());
    event[16..20].copy_from_slice(&target.to_le_bytes());
    event[20..24].copy_from_slice(&given.to_le_bytes());
    let _ = state.connection.send_event(requestor, 0, &event);
    let _ = state.connection.flush();
}

/// The event mask that tells of a window's properties changing.
const PROPERTY_CHANGE: u32 = 0x0040_0000;

/// A property notification's state when the property was taken off.
const PROPERTY_DELETED: u8 = 1;

/// Writes the next piece of a selection being handed over, now that the
/// program taking it has taken the last: the rest in pieces, then an empty
/// one to say that was all, after whose taking the transfer is over.
fn next_piece(state: &mut State, requestor: u32, property: u32) {
    let Some(at) = state
        .transfers
        .iter()
        .position(|transfer| (transfer.requestor, transfer.property) == (requestor, property))
    else {
        return;
    };
    let transfer = &mut state.transfers[at];
    transfer.last = Instant::now();
    if transfer.ended {
        state.transfers.remove(at);
        // The window is no longer watched, unless something else is still
        // being handed to it.
        if !state.transfers.iter().any(|other| other.requestor == requestor)
            && state.window_index(requestor).is_none()
        {
            let _ = state.connection.select_input(requestor, 0);
        }
        let _ = state.connection.flush();
        return;
    }
    let end = (transfer.sent + INCR_CHUNK).min(transfer.data.len());
    let piece = transfer.data[transfer.sent..end].to_vec();
    transfer.sent = end;
    transfer.ended = piece.is_empty();
    let kind = transfer.kind;
    let _ = state.connection.set_property(requestor, property, kind, 8, &piece);
    let _ = state.connection.flush();
}

/// Gives up on selections handed over in pieces that have stopped being
/// taken — the program that asked has gone, or forgotten.
fn forget_stalled_transfers(state: &mut State) {
    let stalled: Vec<u32> = state
        .transfers
        .iter()
        .filter(|transfer| transfer.last.elapsed() > INCR_PATIENCE)
        .map(|transfer| transfer.requestor)
        .collect();
    if stalled.is_empty() {
        return;
    }
    state.transfers.retain(|transfer| transfer.last.elapsed() <= INCR_PATIENCE);
    for requestor in stalled {
        if !state.transfers.iter().any(|other| other.requestor == requestor)
            && state.window_index(requestor).is_none()
        {
            let _ = state.connection.select_input(requestor, 0);
        }
    }
}

/// The page of an "HTML Format" payload, which is what a program on this
/// desktop wants under `text/html`: what follows the header of offsets.
fn html_body(payload: &[u8]) -> Vec<u8> {
    let at = payload.iter().position(|b| *b == b'<').unwrap_or(0);
    payload[at..].to_vec()
}

// --- The screen -----------------------------------------------------------------

pub(crate) fn screen_windows() -> Vec<ScreenWindow> {
    with_state(|state| {
        let root = state.connection.setup.root;
        let list = state.atoms.net_client_list_stacking;
        // The window manager's own list, bottom to top; where there is no
        // window manager to keep one, the server's tree of the screen,
        // which is in the same order.
        let mut ids: Vec<u32> = state
            .connection
            .get_property(root, list, false)
            .map(|(_, _, bytes)| {
                bytes
                    .chunks_exact(4)
                    .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
                    .collect()
            })
            .unwrap_or_default();
        if ids.is_empty() {
            ids = state
                .connection
                .query_tree(root)
                .unwrap_or_default()
                .into_iter()
                .filter(|id| state.connection.is_viewable(*id).unwrap_or(false))
                .collect();
        }
        let mut windows = Vec::new();
        // Topmost first, which is the end of the stacking list.
        for id in ids.iter().rev() {
            let title = title_of(state, *id);
            if !title.is_empty() {
                windows.push(ScreenWindow { title, handle: *id as usize });
            }
        }
        windows
    })
    .unwrap_or_default()
}

/// What a window's title bar says: its name as the desktop writes it, or
/// as the protocol first had it.
fn title_of(state: &mut State, id: u32) -> String {
    let name = state.atoms.net_wm_name;
    state
        .connection
        .get_property(id, name, false)
        .ok()
        .filter(|(_, _, bytes)| !bytes.is_empty())
        .or_else(|| state.connection.get_property(id, x11::ATOM_WM_NAME, false).ok())
        .map(|(_, _, bytes)| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
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

pub(crate) fn capture_screen() -> Option<Shot> {
    with_state(|state| {
        let root = state.connection.setup.root;
        let (width, height) =
            (state.connection.setup.screen_width, state.connection.setup.screen_height);
        let rows = state.connection.get_image(root, 0, 0, width, height).ok()?;
        let pixels =
            canvas_pixels(&state.connection.setup, &rows, usize::from(width), usize::from(height));
        Some(Shot { width: usize::from(width), height: usize::from(height), pixels })
    })
    .flatten()
}

pub(crate) fn capture_window(handle: usize) -> Option<Shot> {
    with_state(|state| {
        let window = handle as u32;
        let (_, _, width, height) = state.connection.geometry(window).ok()?;
        let rows = state.connection.get_image(window, 0, 0, width as u16, height as u16).ok()?;
        let pixels = canvas_pixels(&state.connection.setup, &rows, width as usize, height as usize);
        Some(Shot { width: width as usize, height: height as usize, pixels })
    })
    .flatten()
}

pub(crate) fn open_in_shell(address: &str) -> bool {
    std::process::Command::new("xdg-open")
        .arg(address)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> x11::Setup {
        x11::Setup {
            resource_base: 0,
            resource_mask: 0,
            max_request_length: 65535,
            image_byte_order_msb: false,
            min_keycode: 8,
            max_keycode: 255,
            root: 1,
            root_visual: 1,
            root_depth: 24,
            black: 0,
            white: 0xFF_FFFF,
            screen_width: 100,
            screen_height: 100,
            screen_width_mm: 30,
            bits_per_pixel: 32,
            scanline_pad: 32,
            red_mask: 0xFF_0000,
            green_mask: 0xFF00,
            blue_mask: 0xFF,
        }
    }

    #[test]
    fn pixels_go_to_the_server_in_its_order_and_come_back() {
        let setup = setup();
        let pixels = [10u8, 20, 30, 255, 40, 50, 60, 255];
        let rows = server_pixels(&setup, &pixels, 2, 1);
        assert_eq!(rows, [30, 20, 10, 0, 60, 50, 40, 0]);
        assert_eq!(canvas_pixels(&setup, &rows, 2, 1), pixels);
        let mut big_endian = setup;
        big_endian.image_byte_order_msb = true;
        let rows = server_pixels(&big_endian, &pixels, 2, 1);
        assert_eq!(rows[..4], [0, 10, 20, 30]);
        assert_eq!(canvas_pixels(&big_endian, &rows, 2, 1), pixels);
    }

    #[test]
    fn a_keysym_is_read_off_the_map_with_shift_and_lock() {
        let mut keysyms = vec![0u32; 4 * 2];
        keysyms[0] = 0x61; // a
        keysyms[1] = 0x41; // A
        keysyms[4] = 0x31; // 1
        keysyms[5] = 0x21; // !
        let keymap = (4, keysyms);
        assert_eq!(keysym_of(&keymap, 8, 8, 0), 0x61);
        assert_eq!(keysym_of(&keymap, 8, 8, 0x1), 0x41);
        assert_eq!(keysym_of(&keymap, 8, 8, 0x2), 0x41, "lock gives the capital");
        assert_eq!(
            keysym_of(&keymap, 8, 8, 0x3),
            0x61,
            "shift with lock gives the small letter back"
        );
        assert_eq!(keysym_of(&keymap, 8, 9, 0x2), 0x31, "lock does not turn a digit");
        assert_eq!(keysym_of(&keymap, 8, 9, 0x1), 0x21);
        assert_eq!(keysym_of(&keymap, 8, 7, 0), 0, "a keycode below the first gives nothing");
    }
}

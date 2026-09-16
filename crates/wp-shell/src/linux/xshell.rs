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
}

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
        Response::Redraw => {
            with_state(|state| {
                if let Some(index) = state.window_index(window) {
                    state.windows[index].dirty = true;
                }
            });
        }
        Response::Close => close_window(window),
        Response::Ignored | Response::Refuse => {}
    }
    response
}

fn close_window(window: u32) {
    with_state(|state| {
        let _ = state.connection.destroy_window(window);
        let _ = state.connection.flush();
        state.windows.retain(|found| found.id != window);
    });
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

    let mut last_tick = Instant::now();
    loop {
        let packet =
            with_state(|state| state.connection.next_event(Duration::from_millis(TICK_MILLIS)));
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
        }
        if with_state(|state| state.windows.is_empty()).unwrap_or(true) {
            break;
        }
        present_dirty();
    }

    STATE.with(|slot| slot.borrow_mut().take());
    APPLICATION.with(|slot| slot.borrow_mut().take());
    Ok(())
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
        x11::EXPOSE | x11::CLIENT_MESSAGE | x11::FOCUS_OUT => packet.u32_at(4),
        x11::CONFIGURE_NOTIFY | x11::DESTROY_NOTIFY => packet.u32_at(8),
        x11::SELECTION_REQUEST => {
            answer_selection_request(packet);
            return;
        }
        x11::SELECTION_CLEAR => {
            with_state(|state| state.owned = None);
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
        _ => {}
    }
}

fn key_press(window: u32, packet: &Packet) {
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
pub(crate) fn place_composition(_x: i32, _y: i32, _height: i32) {}

pub(crate) fn selection_changed() {}

pub(crate) fn set_frame_appearance(_dark: bool, _border: (u8, u8, u8), _caption: (u8, u8, u8)) {}

pub(crate) fn start_drag(_contents: &Contents) -> DragEffect {
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
        }
    })
    .unwrap_or_default()
}

/// Asks the clipboard's owner for one format, and waits for the answer.
fn fetch_selection(state: &mut State, window: u32, target: u32) -> Option<Vec<u8>> {
    let (clipboard, property, incr) =
        (state.atoms.clipboard, state.atoms.own_property, state.atoms.incr);
    state.connection.convert_selection(window, clipboard, target, property).ok()?;
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
        // Handed over in pieces, which this does not take.
        return None;
    }
    Some(bytes)
}

fn answer_selection_request(packet: &Packet) {
    with_state(|state| answer_selection_request_in(state, packet));
}

/// Gives another program what it asks for from the clipboard.
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
    let owned = state.owned.clone();
    let mut given = 0u32;
    if let Some(contents) = owned {
        let mut offered: Vec<u32> = vec![atoms.targets];
        if contents.text.is_some() {
            offered.extend([
                atoms.utf8_string,
                x11::ATOM_STRING,
                atoms.text,
                atoms.text_plain_utf8,
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
        {
            contents.text.as_ref().map(|text| (atoms.utf8_string, 8, text.as_bytes().to_vec()))
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
        if let Some((kind, format, data)) = answer {
            if state.connection.set_property(requestor, property, kind, format, &data).is_ok() {
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

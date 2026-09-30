//! The Wayland shell: a window on a compositor, spoken to directly.
//!
//! The same shape as the other two shells — a window, an event loop, a way
//! to get a finished image onto the screen, the clipboard, the pointer —
//! with the Wayland protocol under it. See [`wire`] for the protocol,
//! [`protocol`] for what the numbers on it mean, and [`crate::App`] for
//! what the window asks of the application.
//!
//! # How a window comes up
//!
//! The compositor lists what it offers; the program takes the handful it
//! needs and asks for a surface, gives that surface a window role, and
//! waits to be told how big the window is to be. Nothing is drawn until
//! that answer comes: on Wayland the compositor decides the size, and a
//! program that drew before being told would be drawing the wrong size.
//! From then on it is a loop: draw into memory the compositor can see,
//! say which part changed, and commit.
//!
//! # What is different from X
//!
//! There is no server to ask about anything. A client cannot see another
//! window, cannot read the screen, and cannot put its window where it
//! likes — all three on purpose, and all three are why a screenshot on
//! Wayland goes through the desktop's own portal rather than through the
//! program. What the program draws, it draws; the rest it asks for and is
//! told.

pub(crate) mod keymap;
pub(crate) mod protocol;
pub(crate) mod shm;
pub(crate) mod wire;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use wp_raster::Canvas;

use super::keys;
use crate::clipboard::Contents;
use crate::{
    App, CompositionAttribute, Cursor, DragEffect, Error, Event, Modifiers, Response,
    WindowCommand, WindowOptions,
};

use protocol as p;
use wire::{Connection, Failure, Message, Request};

/// How often the application is given a tick, in milliseconds.
const TICK_MILLIS: u64 = 40;

/// The interfaces the program asked for and the compositor gave.
#[derive(Debug, Default)]
struct Globals {
    compositor: u32,
    shm: u32,
    wm_base: u32,
    seat: u32,
    output: u32,
    data_device_manager: u32,
    fractional_manager: u32,
    text_input_manager: u32,
}

/// One buffer of the window's pixels, and whether the compositor has
/// finished with it.
#[derive(Debug)]
struct Buffer {
    id: u32,
    memory: shm::Shared,
    width: u32,
    height: u32,
    /// False while the compositor is showing it.
    free: bool,
}

/// A window: the surface, the role that makes it a window, and the pixels.
#[derive(Debug)]
struct Window {
    surface: u32,
    xdg_surface: u32,
    toplevel: u32,
    /// The size the compositor has settled on, in its own pixels.
    width: u32,
    height: u32,
    scale: f32,
    /// The configure that has not been answered yet.
    pending: Option<u32>,
    configured: bool,
    maximised: bool,
    dirty: bool,
    buffers: Vec<Buffer>,
    /// The object the compositor says the window's scale through, where it
    /// has one to say it with.
    fractional: u32,
    /// What the window is called, as a screen reader is told.
    title: String,
}

/// The input method's line to this program: text-input, version 3.
///
/// On Wayland the input method is the compositor's to run, and the keys
/// go to it before they come here — those it does not want come on as
/// keys. What this program does is say, for the window with the keyboard,
/// that it takes text and where its caret is; and take what the input
/// method sends: the text being composed and the text committed, a batch
/// at a time, each batch ended by `done`.
#[derive(Debug, Default)]
struct TextInput {
    /// The object, where the compositor has the interface.
    id: u32,
    /// The window the input method is typing into, while one is.
    surface: Option<u32>,
    /// Where the caret was last said to be, in the window's own units.
    rectangle: Option<(i32, i32, i32, i32)>,
    /// What has come since the last `done`.
    preedit: Option<(String, i32, i32)>,
    commit: Option<String>,
    /// Whether text being composed is showing.
    composing: bool,
}

/// Everything the shell holds while it runs.
struct State {
    connection: Connection,
    globals: Globals,
    windows: Vec<Window>,
    keyboard: u32,
    pointer: u32,
    data_device: u32,
    keymap: keymap::Keymap,
    /// What the modifiers were at the last `modifiers` event.
    modifiers: u32,
    /// The last serial from anything the person did, which the compositor
    /// wants back before it will move a window or take the clipboard.
    serial: u32,
    /// Where the pointer is, in the window's own pixels.
    pointer_at: (f32, f32),
    pointer_serial: u32,
    /// Which window the pointer is over.
    pointer_window: Option<u32>,
    held: bool,
    /// The last press, for working out a double click.
    last_press: Option<(Instant, f32, f32)>,
    /// Alt and Control pressed and let go with nothing in between.
    alt_alone: bool,
    control_alone: bool,
    /// The cursor, as a surface of its own — which is the only way a
    /// Wayland client can have one.
    cursor_surface: u32,
    cursor_buffer: Option<Buffer>,
    cursor_shape: Option<Cursor>,
    /// What this program has put on the clipboard, and the object the
    /// compositor takes it from.
    offered: Option<Contents>,
    data_source: u32,
    /// What another program has put there: the object, and the formats.
    offer: u32,
    offer_formats: Vec<String>,
    /// The offer being built, before the compositor says what it is for.
    building: HashMap<u32, Vec<String>>,
    scale: f32,
    /// Set when the compositor asks the program to go away.
    closing: bool,
    /// The input method's line to the window with the keyboard.
    text_input: TextInput,
    /// A drag over one of the windows, while it is.
    drag_over: Option<DragOver>,
    /// This program's own drag, while it is being given.
    dragging: Option<Dragging>,
}

/// A drag over one of this program's windows: the offer it comes as, and
/// what this program makes of it.
#[derive(Debug)]
struct DragOver {
    offer: u32,
    surface: u32,
    /// The formats it can be had in.
    formats: Vec<String>,
    /// Whether it is files, said as their addresses.
    files: bool,
    /// Whether this program takes any of the formats.
    accepted: bool,
    /// What the compositor settled on between the two programs: a copy or
    /// a move.
    action: u32,
    /// Where it is, in the window's own units.
    at: (i32, i32),
    /// Whether it is this program's own drag, come back over its window.
    own: bool,
}

/// This program's drag while it is being given.
#[derive(Debug)]
struct Dragging {
    source: u32,
    contents: Contents,
    action: u32,
    dropped: bool,
    finished: bool,
    cancelled: bool,
    /// Where it was let go, when that was on one of this program's windows.
    landed: Option<(u32, i32, i32)>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static APPLICATION: RefCell<Option<Box<dyn App>>> = const { RefCell::new(None) };
    /// Which window the application is answering for.
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

/// Whether there is a compositor to talk to.
///
/// Asked before the X shell is tried: a desktop that runs both answers
/// here, and a program that took X on a Wayland desktop would be a program
/// drawing through a compatibility layer for no reason.
pub(crate) fn is_available() -> bool {
    Connection::open().is_ok()
}

impl State {
    /// Connects, finds out what the compositor offers, and takes it.
    fn connect() -> Result<Self, Failure> {
        let mut connection = Connection::open()?;
        let registry = connection.make_id();
        connection
            .send(&Request::new(p::wl_display::ID, p::wl_display::GET_REGISTRY).uint(registry))?;

        // The compositor answers with everything it has; the round trip is
        // how the program knows the list has ended.
        let mut globals = Globals::default();
        let mut offered: Vec<(u32, String, u32)> = Vec::new();
        connection.round_trip(p::wl_display::ID)?;
        for message in connection.take_pending() {
            if message.object == registry && message.opcode == p::wl_registry::GLOBAL {
                let mut arguments = message.arguments();
                let name = arguments.uint();
                let interface = arguments.string();
                let version = arguments.uint();
                offered.push((name, interface, version));
            }
        }

        let bind =
            |connection: &mut Connection, wanted: &str, version: u32| -> Result<u32, Failure> {
                let Some((name, interface, theirs)) =
                    offered.iter().find(|(_, interface, _)| interface == wanted)
                else {
                    return Ok(0);
                };
                let id = connection.make_id();
                let version = version.min(*theirs);
                connection.send(
                    &Request::new(registry, p::wl_registry::BIND)
                        .uint(*name)
                        .string(interface)
                        .uint(version)
                        .uint(id),
                )?;
                Ok(id)
            };
        for (wanted, version) in p::WANTED {
            let id = bind(&mut connection, wanted, *version)?;
            match *wanted {
                "wl_compositor" => globals.compositor = id,
                "wl_shm" => globals.shm = id,
                "xdg_wm_base" => globals.wm_base = id,
                "wl_seat" => globals.seat = id,
                "wl_output" => globals.output = id,
                "wl_data_device_manager" => globals.data_device_manager = id,
                "wp_fractional_scale_manager_v1" => globals.fractional_manager = id,
                "zwp_text_input_manager_v3" => globals.text_input_manager = id,
                _ => {}
            }
        }
        if globals.compositor == 0 || globals.shm == 0 || globals.wm_base == 0 {
            return Err(Failure::Protocol(
                "the compositor does not offer a window this program can use".to_owned(),
            ));
        }

        let mut state = Self {
            connection,
            globals,
            windows: Vec::new(),
            keyboard: 0,
            pointer: 0,
            data_device: 0,
            keymap: keymap::Keymap::default(),
            modifiers: 0,
            serial: 0,
            pointer_at: (0.0, 0.0),
            pointer_serial: 0,
            pointer_window: None,
            held: false,
            last_press: None,
            alt_alone: false,
            control_alone: false,
            cursor_surface: 0,
            cursor_buffer: None,
            cursor_shape: None,
            offered: None,
            data_source: 0,
            offer: 0,
            offer_formats: Vec::new(),
            building: HashMap::new(),
            scale: 1.0,
            closing: false,
            text_input: TextInput::default(),
            drag_over: None,
            dragging: None,
        };

        // The keyboard and the pointer are asked for in `take_seat`, once
        // the seat has said which of them it has.
        if state.globals.data_device_manager != 0 && state.globals.seat != 0 {
            state.data_device = state.connection.make_id();
            let request = Request::new(
                state.globals.data_device_manager,
                p::wl_data_device_manager::GET_DATA_DEVICE,
            )
            .uint(state.data_device)
            .uint(state.globals.seat);
            state.connection.send(&request)?;
        }
        if state.globals.text_input_manager != 0 && state.globals.seat != 0 {
            state.text_input.id = state.connection.make_id();
            let request = Request::new(
                state.globals.text_input_manager,
                p::zwp_text_input_manager_v3::GET_TEXT_INPUT,
            )
            .uint(state.text_input.id)
            .uint(state.globals.seat);
            state.connection.send(&request)?;
        }
        state.cursor_surface = state.connection.make_id();
        let request = Request::new(state.globals.compositor, p::wl_compositor::CREATE_SURFACE)
            .uint(state.cursor_surface);
        state.connection.send(&request)?;
        state.connection.round_trip(p::wl_display::ID)?;
        let pending = state.connection.take_pending();
        for message in pending {
            state.handle(&message);
        }
        Ok(state)
    }

    /// Makes a window and asks for it to be shown.
    fn create_window(&mut self, title: &str, width: u32, height: u32) -> Result<u32, Failure> {
        let surface = self.connection.make_id();
        self.connection.send(
            &Request::new(self.globals.compositor, p::wl_compositor::CREATE_SURFACE).uint(surface),
        )?;
        let xdg_surface = self.connection.make_id();
        self.connection.send(
            &Request::new(self.globals.wm_base, p::xdg_wm_base::GET_XDG_SURFACE)
                .uint(xdg_surface)
                .uint(surface),
        )?;
        let toplevel = self.connection.make_id();
        self.connection
            .send(&Request::new(xdg_surface, p::xdg_surface::GET_TOPLEVEL).uint(toplevel))?;
        self.connection.send(&Request::new(toplevel, p::xdg_toplevel::SET_TITLE).string(title))?;
        // What the desktop knows this program by: the same name as the
        // desktop entry, so that the window carries the right icon and is
        // grouped with the right thing in a task bar.
        self.connection
            .send(&Request::new(toplevel, p::xdg_toplevel::SET_APP_ID).string("word-processor"))?;
        let mut fractional = 0;
        if self.globals.fractional_manager != 0 {
            fractional = self.connection.make_id();
            let request = Request::new(
                self.globals.fractional_manager,
                p::wp_fractional_scale_manager_v1::GET_FRACTIONAL_SCALE,
            )
            .uint(fractional)
            .uint(surface);
            self.connection.send(&request)?;
        }
        // Nothing may be drawn until the compositor has said how big.
        self.connection.send(&Request::new(surface, p::wl_surface::COMMIT))?;

        let scale = self.scale;
        self.windows.push(Window {
            surface,
            xdg_surface,
            toplevel,
            width: (width as f32 * scale).round() as u32,
            height: (height as f32 * scale).round() as u32,
            scale,
            pending: None,
            configured: false,
            maximised: false,
            dirty: true,
            buffers: Vec::new(),
            fractional,
            title: title.to_owned(),
        });
        Ok(surface)
    }

    fn window_index(&self, surface: u32) -> Option<usize> {
        self.windows.iter().position(|window| window.surface == surface)
    }

    /// The window an object belongs to, whichever of its objects it is.
    fn window_of(&self, object: u32) -> Option<usize> {
        self.windows.iter().position(|window| {
            window.surface == object || window.xdg_surface == object || window.toplevel == object
        })
    }

    /// A buffer of the right size that the compositor is not showing.
    fn free_buffer(&mut self, index: usize) -> Option<usize> {
        let (width, height) = (self.windows[index].width.max(1), self.windows[index].height.max(1));
        // Any buffer of another size is of no use now, and the compositor
        // is told so rather than left holding it.
        let stale: Vec<u32> = self.windows[index]
            .buffers
            .iter()
            .filter(|buffer| buffer.free && (buffer.width != width || buffer.height != height))
            .map(|buffer| buffer.id)
            .collect();
        for id in stale {
            let _ = self.connection.send(&Request::new(id, p::wl_buffer::DESTROY));
        }
        self.windows[index]
            .buffers
            .retain(|buffer| buffer.width == width && buffer.height == height || !buffer.free);
        if let Some(at) = self.windows[index]
            .buffers
            .iter()
            .position(|buffer| buffer.free && buffer.width == width && buffer.height == height)
        {
            return Some(at);
        }
        // Two is enough: one being shown while the next is drawn.
        if self.windows[index].buffers.len() >= 2 {
            return None;
        }
        let length = width as usize * height as usize * 4;
        let memory = shm::Shared::new(length).ok()?;
        let pool = self.connection.make_id();
        let request = Request::new(self.globals.shm, p::wl_shm::CREATE_POOL)
            .uint(pool)
            .int(length as i32)
            .with_fd(memory.fd());
        self.connection.send(&request).ok()?;
        let id = self.connection.make_id();
        let request = Request::new(pool, p::wl_shm_pool::CREATE_BUFFER)
            .uint(id)
            .int(0)
            .int(width as i32)
            .int(height as i32)
            .int(width as i32 * 4)
            .uint(p::wl_shm::XRGB8888);
        self.connection.send(&request).ok()?;
        let _ = self.connection.send(&Request::new(pool, p::wl_shm_pool::DESTROY));
        self.windows[index].buffers.push(Buffer { id, memory, width, height, free: true });
        Some(self.windows[index].buffers.len() - 1)
    }

    /// Takes the keyboard and the pointer the seat says it has.
    ///
    /// Asked for rather than assumed: a seat with no keyboard — a tablet,
    /// a kiosk — answers a request for one with a protocol error, and the
    /// program would be shut down for asking.
    fn take_seat(&mut self, capabilities: u32) {
        // A keyboard that has gone is an object that answers for nothing:
        // it is let go, so that the next one to arrive is asked for
        // afresh. Which is what happens when a keyboard is unplugged, and
        // what a compositor with no devices at all does every time one
        // appears.
        if capabilities & p::wl_seat::KEYBOARD == 0 && self.keyboard != 0 {
            let _ = self.connection.send(&Request::new(self.keyboard, p::wl_keyboard::RELEASE));
            self.keyboard = 0;
        }
        if capabilities & p::wl_seat::POINTER == 0 && self.pointer != 0 {
            let _ = self.connection.send(&Request::new(self.pointer, p::wl_pointer::RELEASE));
            self.pointer = 0;
            self.cursor_shape = None;
        }
        if capabilities & p::wl_seat::KEYBOARD != 0 && self.keyboard == 0 {
            self.keyboard = self.connection.make_id();
            let request =
                Request::new(self.globals.seat, p::wl_seat::GET_KEYBOARD).uint(self.keyboard);
            let _ = self.connection.send(&request);
        }
        if capabilities & p::wl_seat::POINTER != 0 && self.pointer == 0 {
            self.pointer = self.connection.make_id();
            let request =
                Request::new(self.globals.seat, p::wl_seat::GET_POINTER).uint(self.pointer);
            let _ = self.connection.send(&request);
        }
    }

    /// Answers a ping, so the compositor knows the program is alive.
    fn pong(&mut self, serial: u32) {
        let _ = self
            .connection
            .send(&Request::new(self.globals.wm_base, p::xdg_wm_base::PONG).uint(serial));
    }
}

// --- Running ----------------------------------------------------------------

/// Opens the window and runs until it closes.
pub(crate) fn run(options: WindowOptions, app: Box<dyn App>) -> Result<(), Error> {
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(app));
    let mut state = State::connect().map_err(|failure| failed(&failure))?;
    let window = state
        .create_window(&options.title, options.width, options.height)
        .map_err(|failure| failed(&failure))?;
    let scale = state.scale;
    STATE.with(|slot| *slot.borrow_mut() = Some(state));
    WINDOW.with(|slot| slot.set(window));
    deliver(window, Event::ScaleChanged { scale });
    // The desktop's portal, which is the only way a program here may
    // photograph the screen, woken now and asked what it offers, so that
    // the Screenshot list neither waits for it to start nor opens without
    // what it could have offered.
    super::portal::warm_up();

    let mut last_tick = Instant::now();
    let mut accessible = false;
    loop {
        // The compositor and the accessibility bus are waited on together,
        // so that whichever speaks is answered at once; see
        // [`super::wait`].
        let tick = Duration::from_millis(TICK_MILLIS);
        let until_tick = tick.saturating_sub(last_tick.elapsed());
        let display = with_state(|state| (state.connection.raw_fd(), state.connection.buffered()));
        let Some((display_fd, buffered)) = display else { break };
        let ready = buffered || {
            let mut fds = vec![display_fd];
            fds.extend(super::atspi::bus_fd());
            super::wait::readable(&fds, until_tick)[0]
        };
        let message = if ready {
            with_state(|state| state.connection.next_message(Duration::ZERO))
        } else {
            Some(Ok(None))
        };
        match message {
            Some(Ok(Some(message))) => {
                with_state(|state| state.handle(&message));
                deliver_pending();
                deliver_handed_on();
                deliver_input();
                follow_pointer();
            }
            Some(Ok(None)) => {}
            Some(Err(_)) | None => break,
        }
        if last_tick.elapsed() >= Duration::from_millis(TICK_MILLIS) {
            last_tick = Instant::now();
            let ids: Vec<u32> =
                with_state(|state| state.windows.iter().map(|window| window.surface).collect())
                    .unwrap_or_default();
            for id in ids {
                deliver(id, Event::Tick);
            }
            // Joined once the window is up, so that finding the bus does
            // not keep the window from coming up.
            if !accessible {
                accessible = true;
                super::atspi::start();
            }
        }
        super::atspi::pump(&mut Reading(WINDOW.with(Cell::get)));
        // Being asked to close is a question for the application, whether
        // it came from the compositor or from the program's own button.
        if with_state(|state| core::mem::replace(&mut state.closing, false)) == Some(true) {
            let window = WINDOW.with(Cell::get);
            if deliver(window, Event::Closing) != Response::Refuse {
                break;
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

/// A window as a screen reader reads it: see [`super::atspi`]. Where it is
/// on the screen is the one thing Wayland does not tell a program, so its
/// places are given as the window's own.
struct Reading(u32);

impl Reading {
    fn application<R>(&self, work: impl FnOnce(&mut dyn App) -> R) -> Option<R> {
        let index = with_state(|state| state.window_index(self.0)).flatten();
        if let Some(index) = index {
            with_application(|app| app.switch_window(index));
        }
        with_application(work)
    }

    fn answered(&self, response: Option<Response>) {
        if response == Some(Response::Redraw) {
            with_state(|state| {
                if let Some(index) = state.window_index(self.0) {
                    state.windows[index].dirty = true;
                }
            });
        }
    }

    fn window<R>(&self, read: impl FnOnce(&Window) -> R) -> Option<R> {
        with_state(|state| state.window_index(self.0).map(|index| read(&state.windows[index])))
            .flatten()
    }
}

impl super::atspi::Window for Reading {
    fn title(&mut self) -> String {
        self.window(|window| window.title.clone()).unwrap_or_default()
    }

    fn origin(&mut self) -> (i32, i32) {
        (0, 0)
    }

    fn size(&mut self) -> (i32, i32) {
        self.window(|window| (window.width as i32, window.height as i32)).unwrap_or((0, 0))
    }

    fn scale(&mut self) -> f32 {
        self.window(|window| window.scale).unwrap_or(1.0)
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

/// Hands an event to the application for a window, and acts on the answer.
fn deliver(surface: u32, event: Event) -> Response {
    WINDOW.with(|slot| slot.set(surface));
    let index = with_state(|state| state.window_index(surface)).flatten();
    if let Some(index) = index {
        with_application(|app| app.switch_window(index));
    }
    let response = with_application(|app| app.handle(event)).unwrap_or(Response::Ignored);
    match response {
        Response::Redraw => {
            with_state(|state| {
                if let Some(index) = state.window_index(surface) {
                    state.windows[index].dirty = true;
                }
            });
        }
        Response::Close => close_window(surface),
        Response::Ignored | Response::Refuse => {}
    }
    response
}

fn close_window(surface: u32) {
    with_state(|state| {
        let Some(index) = state.window_index(surface) else { return };
        let window = state.windows.remove(index);
        let _ = state.connection.send(&Request::new(window.toplevel, p::xdg_toplevel::DESTROY));
        let _ = state.connection.send(&Request::new(window.xdg_surface, p::xdg_surface::DESTROY));
        let _ = state.connection.send(&Request::new(window.surface, p::wl_surface::DESTROY));
    });
}

/// Draws every window that asked to be drawn again.
fn present_dirty() {
    let dirty: Vec<u32> = with_state(|state| {
        state
            .windows
            .iter()
            .filter(|window| window.dirty && window.configured && window.width > 0)
            .map(|window| window.surface)
            .collect()
    })
    .unwrap_or_default();
    for surface in dirty {
        present(surface);
    }
}

/// Draws one window: the application's canvas, into memory the compositor
/// reads, and then the word that it has changed.
fn present(surface: u32) {
    let Some(Some(index)) = with_state(|state| state.window_index(surface)) else { return };
    with_application(|app| app.switch_window(index));
    WINDOW.with(|slot| slot.set(surface));

    let Some((width, height, scale)) = with_state(|state| {
        let window = &state.windows[index];
        (window.width, window.height, window.scale)
    }) else {
        return;
    };
    let logical_width = (width as f32 / scale).round().max(1.0) as usize;
    let logical_height = (height as f32 / scale).round().max(1.0) as usize;
    let Some(pixels) =
        with_application(|app| app.draw(logical_width, logical_height).pixels().to_vec())
    else {
        return;
    };

    with_state(|state| {
        let Some(at) = state.free_buffer(index) else { return };
        let window = &mut state.windows[index];
        let buffer = &mut window.buffers[at];
        let wanted = buffer.width as usize * buffer.height as usize * 4;
        if pixels.len() >= wanted {
            shm::to_compositor(&pixels[..wanted], &mut buffer.memory.bytes()[..wanted]);
        }
        buffer.free = false;
        let (id, buffer_width, buffer_height) = (buffer.id, buffer.width, buffer.height);
        window.dirty = false;
        let surface = window.surface;
        let _ = state
            .connection
            .send(&Request::new(surface, p::wl_surface::ATTACH).uint(id).int(0).int(0));
        let _ = state.connection.send(
            &Request::new(surface, p::wl_surface::DAMAGE_BUFFER)
                .int(0)
                .int(0)
                .int(buffer_width as i32)
                .int(buffer_height as i32),
        );
        let _ = state.connection.send(&Request::new(surface, p::wl_surface::COMMIT));
    });
}

// --- What the compositor says -----------------------------------------------

impl State {
    /// Deals with one message from the compositor.
    fn handle(&mut self, message: &Message) {
        let mut arguments = message.arguments();
        if message.object == p::wl_display::ID && message.opcode == p::wl_display::ERROR {
            let _object = arguments.uint();
            let _code = arguments.uint();
            let _why = arguments.string();
            return;
        }
        if message.object == self.globals.wm_base && message.opcode == p::xdg_wm_base::PING {
            let serial = arguments.uint();
            self.pong(serial);
            return;
        }
        if message.object == self.globals.seat && message.opcode == p::wl_seat::CAPABILITIES {
            let capabilities = arguments.uint();
            self.take_seat(capabilities);
            return;
        }
        if message.object == self.keyboard && self.keyboard != 0 {
            self.keyboard_event(message);
            return;
        }
        if message.object == self.text_input.id && self.text_input.id != 0 {
            self.text_input_event(message);
            return;
        }
        if message.object == self.pointer && self.pointer != 0 {
            self.pointer_event(message);
            return;
        }
        let dragged_over = self.drag_over.as_ref().is_some_and(|over| over.offer == message.object);
        if message.object == self.data_device
            || self.building.contains_key(&message.object)
            || dragged_over
        {
            self.clipboard_event(message);
            return;
        }
        if self.dragging.as_ref().is_some_and(|dragging| dragging.source == message.object) {
            self.drag_source_event(message);
            return;
        }
        if message.object == self.data_source {
            self.source_event(message);
            return;
        }
        if message.object == self.globals.output && message.opcode == p::wl_output::SCALE {
            let scale = arguments.int().max(1) as f32;
            self.set_scale(scale);
            return;
        }
        // A buffer the compositor has finished with can be drawn into
        // again — which is what keeps a window being drawn at all, so it
        // is looked for by name rather than left to fall through.
        if message.opcode == p::wl_buffer::RELEASE {
            for window in &mut self.windows {
                for buffer in &mut window.buffers {
                    if buffer.id == message.object {
                        buffer.free = true;
                        return;
                    }
                }
            }
        }
        if let Some(index) = self.window_of(message.object) {
            self.window_event(index, message);
            return;
        }
        // The object the compositor says a fraction of a scale through.
        let wanted = self.windows.iter().any(|window| window.fractional == message.object);
        if wanted && message.opcode == p::wp_fractional_scale_v1::PREFERRED_SCALE {
            // In hundred-and-twentieths, which is how the protocol says a
            // fraction without saying a float.
            let scale = arguments.uint() as f32 / 120.0;
            if scale > 0.0 {
                self.set_scale(scale);
            }
        }
    }

    fn set_scale(&mut self, scale: f32) {
        let scale = scale.clamp(0.5, 4.0);
        if (scale - self.scale).abs() < 0.01 {
            return;
        }
        self.scale = scale;
        for window in &mut self.windows {
            window.scale = scale;
            window.dirty = true;
        }
        let surface = self.windows.first().map(|window| window.surface);
        if let Some(surface) = surface {
            // The window is drawn at the compositor's own scale, and the
            // surface is told so — otherwise the compositor would take the
            // picture for one at scale 1 and blow it up.
            let whole = scale.round().max(1.0) as i32;
            let _ = self
                .connection
                .send(&Request::new(surface, p::wl_surface::SET_BUFFER_SCALE).int(whole));
            SCALE_CHANGED.with(|slot| slot.set(true));
        }
    }

    /// The window's own messages: how big it is to be, and being asked to
    /// go away.
    fn window_event(&mut self, index: usize, message: &Message) {
        let mut arguments = message.arguments();
        let window = &mut self.windows[index];
        if message.object == window.xdg_surface && message.opcode == p::xdg_surface::CONFIGURE {
            let serial = arguments.uint();
            window.pending = Some(serial);
            let (surface, xdg_surface) = (window.surface, window.xdg_surface);
            let _ = self
                .connection
                .send(&Request::new(xdg_surface, p::xdg_surface::ACK_CONFIGURE).uint(serial));
            let first = !self.windows[index].configured;
            self.windows[index].configured = true;
            self.windows[index].dirty = true;
            let (width, height, scale) = {
                let window = &self.windows[index];
                (window.width, window.height, window.scale)
            };
            if first {
                let whole = scale.round().max(1.0) as i32;
                let _ = self
                    .connection
                    .send(&Request::new(surface, p::wl_surface::SET_BUFFER_SCALE).int(whole));
            }
            RESIZED.with(|slot| {
                slot.set(Some((
                    surface,
                    (width as f32 / scale).round().max(1.0) as u32,
                    (height as f32 / scale).round().max(1.0) as u32,
                )));
            });
            return;
        }
        if message.object == window.toplevel {
            match message.opcode {
                p::xdg_toplevel::CONFIGURE => {
                    let width = arguments.int();
                    let height = arguments.int();
                    let states = arguments.array();
                    let maximised = states.chunks_exact(4).any(|four| {
                        u32::from_ne_bytes([four[0], four[1], four[2], four[3]])
                            == p::xdg_toplevel::STATE_MAXIMIZED
                    });
                    window.maximised = maximised;
                    // Zero means "as big as you like", which is the size
                    // the program asked for.
                    if width > 0 && height > 0 {
                        let scale = window.scale;
                        let wanted =
                            ((width as f32 * scale) as u32, (height as f32 * scale) as u32);
                        if (window.width, window.height) != wanted {
                            window.width = wanted.0;
                            window.height = wanted.1;
                            window.dirty = true;
                        }
                    }
                }
                p::xdg_toplevel::CLOSE => self.closing = true,
                _ => {}
            }
        }
    }
}

thread_local! {
    /// A resize the compositor asked for, to be handed to the application
    /// once the message has been dealt with — the application cannot be
    /// called while the state is borrowed.
    static RESIZED: Cell<Option<(u32, u32, u32)>> = const { Cell::new(None) };
    static SCALE_CHANGED: Cell<bool> = const { Cell::new(false) };
}

/// Hands on whatever the last message asked for.
fn deliver_pending() {
    if SCALE_CHANGED.with(Cell::take) {
        let scale = with_state(|state| state.scale).unwrap_or(1.0);
        let window = WINDOW.with(Cell::get);
        deliver(window, Event::ScaleChanged { scale });
    }
    if let Some((surface, width, height)) = RESIZED.with(Cell::take) {
        deliver(surface, Event::Resized { width, height });
    }
}

// --- The keyboard -----------------------------------------------------------

/// The modifier bits, as every keyboard on Linux numbers them.
const SHIFT: u32 = 1;
const CAPS: u32 = 2;
const CONTROL: u32 = 4;
const ALT: u32 = 8;
const LEVEL3: u32 = 128;

impl State {
    fn keyboard_event(&mut self, message: &Message) {
        let mut arguments = message.arguments();
        match message.opcode {
            p::wl_keyboard::KEYMAP => {
                let format = arguments.uint();
                let _size = arguments.uint();
                let Some(fd) = self.connection.take_fd() else { return };
                if format != p::wl_keyboard::FORMAT_XKB_V1 {
                    return;
                }
                let bytes = shm::read_fd(fd);
                let text = String::from_utf8_lossy(&bytes);
                let keymap = keymap::Keymap::parse(&text);
                if !keymap.is_empty() {
                    self.keymap = keymap;
                }
            }
            p::wl_keyboard::MODIFIERS => {
                let serial = arguments.uint();
                self.serial = serial;
                let depressed = arguments.uint();
                let _latched = arguments.uint();
                let locked = arguments.uint();
                self.modifiers = depressed | (locked & CAPS);
            }
            p::wl_keyboard::KEY => {
                let serial = arguments.uint();
                self.serial = serial;
                let _time = arguments.uint();
                // The compositor counts keys as the input layer does, and
                // the keymap counts them as X does: eight apart.
                let keycode = arguments.uint() + 8;
                let pressed = arguments.uint() == p::wl_keyboard::PRESSED;
                let keysym = self.keysym(keycode);
                KEY_EVENT.with(|slot| slot.set(Some((keysym, pressed))));
            }
            p::wl_keyboard::ENTER | p::wl_keyboard::LEAVE => {
                let serial = arguments.uint();
                self.serial = serial;
            }
            _ => {}
        }
    }

    /// What a key means with the modifiers held.
    fn keysym(&self, keycode: u32) -> u32 {
        self.keymap.keysym(
            keycode,
            self.modifiers & SHIFT != 0,
            self.modifiers & CAPS != 0,
            self.modifiers & LEVEL3 != 0,
        )
    }

    fn modifiers_now(&self) -> Modifiers {
        Modifiers {
            control: self.modifiers & CONTROL != 0,
            shift: self.modifiers & SHIFT != 0,
            alt: self.modifiers & ALT != 0,
        }
    }
}

thread_local! {
    /// A key press or release, handed on once the state is free again.
    static KEY_EVENT: Cell<Option<(u32, bool)>> = const { Cell::new(None) };
    static POINTER_EVENT: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
}

/// Gives the application whatever the keyboard and the pointer said.
fn deliver_input() {
    take_hold_of_the_window();
    let window = WINDOW.with(Cell::get);
    if let Some((keysym, pressed)) = KEY_EVENT.with(Cell::take) {
        if pressed {
            key_pressed(window, keysym);
        } else {
            key_released(window, keysym);
        }
    }
    let events: Vec<Event> = POINTER_EVENT.with(|slot| core::mem::take(&mut *slot.borrow_mut()));
    for event in events {
        deliver(window, event);
    }
}

fn key_pressed(window: u32, keysym: u32) {
    if keys::is_alt(keysym) {
        with_state(|state| state.alt_alone = true);
        return;
    }
    if keys::is_control(keysym) {
        with_state(|state| state.control_alone = true);
        return;
    }
    with_state(|state| {
        state.alt_alone = false;
        state.control_alone = false;
    });
    if keys::is_modifier(keysym) {
        return;
    }
    let modifiers = with_state(|state| state.modifiers_now()).unwrap_or_default();
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

fn key_released(window: u32, keysym: u32) {
    // Alt and Control on their own are the menu key and the one that
    // shows the key tips, as they are on Windows.
    if keys::is_alt(keysym)
        && with_state(|state| core::mem::replace(&mut state.alt_alone, false)).unwrap_or(false)
    {
        deliver(window, Event::MenuKey);
    }
    if keys::is_control(keysym)
        && with_state(|state| core::mem::replace(&mut state.control_alone, false)).unwrap_or(false)
    {
        deliver(window, Event::ControlKey);
    }
}

// --- The input method -------------------------------------------------------

impl State {
    fn text_input_event(&mut self, message: &Message) {
        let mut arguments = message.arguments();
        let id = self.text_input.id;
        match message.opcode {
            // The window has the keyboard: it takes text, ordinary text, and
            // its caret is where it was last said to be.
            p::zwp_text_input_v3::ENTER => {
                let surface = arguments.uint();
                self.text_input.surface = Some(surface);
                self.text_input.preedit = None;
                self.text_input.commit = None;
                let _ = self.connection.send(&Request::new(id, p::zwp_text_input_v3::ENABLE));
                let _ = self.connection.send(
                    &Request::new(id, p::zwp_text_input_v3::SET_CONTENT_TYPE)
                        .uint(p::zwp_text_input_v3::HINT_NONE)
                        .uint(p::zwp_text_input_v3::PURPOSE_NORMAL),
                );
                if let Some(rectangle) = self.text_input.rectangle {
                    self.send_cursor_rectangle(rectangle);
                }
                let _ = self.connection.send(&Request::new(id, p::zwp_text_input_v3::COMMIT));
            }
            p::zwp_text_input_v3::LEAVE => {
                let surface = arguments.uint();
                let _ = self.connection.send(&Request::new(id, p::zwp_text_input_v3::DISABLE));
                let _ = self.connection.send(&Request::new(id, p::zwp_text_input_v3::COMMIT));
                self.text_input.surface = None;
                self.text_input.preedit = None;
                self.text_input.commit = None;
                // Whatever was being composed there will not be finished.
                if core::mem::take(&mut self.text_input.composing) {
                    HANDED_ON.with(|slot| slot.borrow_mut().push((surface, Event::ComposeEnd)));
                }
            }
            p::zwp_text_input_v3::PREEDIT_STRING => {
                let text = arguments.string();
                let begin = arguments.int();
                let end = arguments.int();
                self.text_input.preedit = Some((text, begin, end));
            }
            p::zwp_text_input_v3::COMMIT_STRING => {
                self.text_input.commit = Some(arguments.string());
            }
            // The batch is over: the text committed goes in, and the text
            // being composed is whatever the batch said — nothing, if it
            // said nothing.
            p::zwp_text_input_v3::DONE => {
                let commit = self.text_input.commit.take().filter(|text| !text.is_empty());
                let preedit = self.text_input.preedit.take().filter(|(text, ..)| !text.is_empty());
                let Some(surface) = self.text_input.surface else { return };
                let mut events = Vec::new();
                if let Some(text) = commit {
                    events.push(Event::Commit(text));
                    self.text_input.composing = false;
                }
                match preedit {
                    Some((text, begin, end)) => {
                        events.push(composition(text, begin, end));
                        self.text_input.composing = true;
                    }
                    None if self.text_input.composing => {
                        events.push(Event::ComposeEnd);
                        self.text_input.composing = false;
                    }
                    None => {}
                }
                HANDED_ON.with(|slot| {
                    slot.borrow_mut().extend(events.into_iter().map(|event| (surface, event)));
                });
            }
            _ => {}
        }
    }

    fn send_cursor_rectangle(&mut self, (x, y, width, height): (i32, i32, i32, i32)) {
        let request = Request::new(self.text_input.id, p::zwp_text_input_v3::SET_CURSOR_RECTANGLE)
            .int(x)
            .int(y)
            .int(width)
            .int(height);
        let _ = self.connection.send(&request);
    }
}

/// The text being composed, as the window is given it. The input method
/// says where its cursor is as a range of bytes of the text — nothing
/// where it shows none — and a range that is not empty is the part being
/// chosen for.
fn composition(text: String, begin: i32, end: i32) -> Event {
    let starts: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    let count = starts.len();
    let character_at = |byte: i32| -> usize {
        usize::try_from(byte)
            .map_or(count, |byte| starts.iter().take_while(|&&at| at < byte).count())
    };
    let (from, to) = (character_at(begin), character_at(end));
    let attributes = (0..count)
        .map(|index| {
            if from < to && (from..to).contains(&index) {
                CompositionAttribute::Target
            } else {
                CompositionAttribute::Input
            }
        })
        .collect();
    Event::Compose { text, caret: to, attributes }
}

thread_local! {
    /// What the input method and the drags of other programs sent, for the
    /// window it was sent for, handed on once the state is free again.
    static HANDED_ON: RefCell<Vec<(u32, Event)>> = const { RefCell::new(Vec::new()) };
}

fn deliver_handed_on() {
    let events: Vec<(u32, Event)> = HANDED_ON.with(|slot| core::mem::take(&mut *slot.borrow_mut()));
    for (surface, event) in events {
        deliver(surface, event);
    }
}

// --- The pointer ------------------------------------------------------------

impl State {
    fn pointer_event(&mut self, message: &Message) {
        let mut arguments = message.arguments();
        match message.opcode {
            p::wl_pointer::ENTER => {
                let serial = arguments.uint();
                self.serial = serial;
                self.pointer_serial = serial;
                let surface = arguments.uint();
                let x = arguments.fixed();
                let y = arguments.fixed();
                self.pointer_window = Some(surface);
                self.pointer_at = (x, y);
                // The cursor is the client's on Wayland: a window that says
                // nothing has no cursor at all over it.
                self.cursor_shape = None;
                self.set_cursor(Cursor::Arrow);
            }
            p::wl_pointer::LEAVE => {
                let serial = arguments.uint();
                self.serial = serial;
                self.pointer_window = None;
                POINTER_EVENT.with(|slot| slot.borrow_mut().push(Event::PointerLeft));
            }
            p::wl_pointer::MOTION => {
                let _time = arguments.uint();
                let x = arguments.fixed();
                let y = arguments.fixed();
                self.pointer_at = (x, y);
                let (x, y) = self.logical_pointer();
                let modifiers = self.modifiers_now();
                let held = self.held;
                POINTER_EVENT.with(|slot| {
                    slot.borrow_mut().push(Event::MouseMove { x, y, held, modifiers });
                });
                MOVED.with(|slot| slot.set(true));
            }
            p::wl_pointer::BUTTON => {
                let serial = arguments.uint();
                self.serial = serial;
                self.pointer_serial = serial;
                let _time = arguments.uint();
                let button = arguments.uint();
                let pressed = arguments.uint() == p::wl_pointer::PRESSED;
                self.button(button, pressed);
            }
            p::wl_pointer::AXIS => {
                let _time = arguments.uint();
                let axis = arguments.uint();
                let value = arguments.fixed();
                if axis == p::wl_pointer::AXIS_VERTICAL {
                    // The protocol measures a wheel in the same units as a
                    // scroll bar, and fifteen of them are one notch.
                    let lines = -value / 15.0;
                    let modifiers = self.modifiers_now();
                    POINTER_EVENT.with(|slot| {
                        slot.borrow_mut().push(Event::Scroll { lines, modifiers });
                    });
                }
            }
            _ => {}
        }
    }

    /// Which edges the pointer is on, if it is on any: the four sides and
    /// the four corners, in the band a person can reasonably hit.
    fn resize_edges(&self, x: i32, y: i32) -> Option<u32> {
        const BAND: i32 = 4;
        let index = self.pointer_window.and_then(|surface| self.window_index(surface))?;
        let window = &self.windows[index];
        if window.maximised {
            return None;
        }
        let width = (window.width as f32 / window.scale).round() as i32;
        let height = (window.height as f32 / window.scale).round() as i32;
        let mut edges = 0;
        if x <= BAND {
            edges |= p::xdg_toplevel::RESIZE_LEFT;
        } else if x >= width - BAND {
            edges |= p::xdg_toplevel::RESIZE_RIGHT;
        }
        if y <= BAND {
            edges |= p::xdg_toplevel::RESIZE_TOP;
        } else if y >= height - BAND {
            edges |= p::xdg_toplevel::RESIZE_BOTTOM;
        }
        (edges != 0).then_some(edges)
    }

    /// Where the pointer is in the program's own pixels.
    fn logical_pointer(&self) -> (i32, i32) {
        let scale = self.scale;
        let _ = scale;
        // The pointer is already in the surface's own coordinates, which
        // are the program's: the buffer is scaled, not the surface.
        (self.pointer_at.0.round() as i32, self.pointer_at.1.round() as i32)
    }

    fn button(&mut self, button: u32, pressed: bool) {
        let (x, y) = self.logical_pointer();
        let modifiers = self.modifiers_now();
        let mut events: Vec<Event> = Vec::new();
        match (button, pressed) {
            (p::wl_pointer::BUTTON_LEFT, true) => {
                // The window has no frame of the compositor's — the
                // program draws its own — so the edges and the caption are
                // where the person takes hold of it, and the compositor is
                // asked to do the moving.
                if let Some(edges) = self.resize_edges(x, y) {
                    RESIZE_FROM.with(|slot| slot.set(Some(edges)));
                    return;
                }
                CAPTION_AT.with(|slot| slot.set(Some((x, y))));
                self.held = true;
                let now = Instant::now();
                let double = self.last_press.is_some_and(|(at, was_x, was_y)| {
                    at.elapsed() < Duration::from_millis(u64::from(double_click_millis()))
                        && (was_x - x as f32).abs() < 4.0
                        && (was_y - y as f32).abs() < 4.0
                });
                self.last_press = Some((now, x as f32, y as f32));
                events.push(if double {
                    Event::DoubleClick { x, y }
                } else {
                    Event::MouseDown { x, y, modifiers }
                });
            }
            (p::wl_pointer::BUTTON_LEFT, false) => {
                self.held = false;
                events.push(Event::MouseUp { x, y });
            }
            (p::wl_pointer::BUTTON_RIGHT, false) => {
                events.push(Event::RightClick { x, y, modifiers });
            }
            (p::wl_pointer::BUTTON_MIDDLE, false) => {
                events.push(Event::MiddleClick { x, y });
            }
            _ => {}
        }
        POINTER_EVENT.with(|slot| slot.borrow_mut().extend(events));
    }
}

thread_local! {
    /// Whether the pointer moved, so the cursor can be asked for again.
    static MOVED: Cell<bool> = const { Cell::new(false) };
    /// A press on an edge, which the compositor is to turn into a resize.
    static RESIZE_FROM: Cell<Option<u32>> = const { Cell::new(None) };
    /// A press that may have been on the caption, which only the
    /// application can say — and it cannot be asked while the state is
    /// borrowed, so it is asked afterwards.
    static CAPTION_AT: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
}

/// Acts on a press that was on an edge or on the caption.
fn take_hold_of_the_window() {
    if let Some(edges) = RESIZE_FROM.with(Cell::take) {
        begin_move_or_resize(edges);
        CAPTION_AT.with(|slot| slot.set(None));
        POINTER_EVENT.with(|slot| slot.borrow_mut().clear());
        return;
    }
    let Some((x, y)) = CAPTION_AT.with(Cell::take) else { return };
    let window = WINDOW.with(Cell::get);
    let index = with_state(|state| state.window_index(window)).flatten();
    if let Some(index) = index {
        with_application(|app| app.switch_window(index));
    }
    if with_application(|app| app.is_caption(x, y)) == Some(true) {
        // The press belongs to the compositor now: the program does not
        // also treat it as a click in the document.
        POINTER_EVENT.with(|slot| slot.borrow_mut().clear());
        begin_move_or_resize(0);
    }
}

/// Asks the application which cursor belongs where the pointer is, and
/// tells the compositor.
fn follow_pointer() {
    if !MOVED.with(Cell::take) {
        return;
    }
    let Some((x, y)) = with_state(|state| state.logical_pointer()) else { return };
    let window = WINDOW.with(Cell::get);
    let index = with_state(|state| state.window_index(window)).flatten();
    if let Some(index) = index {
        with_application(|app| app.switch_window(index));
    }
    let Some(wanted) = with_application(|app| app.cursor(x, y)) else { return };
    with_state(|state| state.set_cursor(wanted));
}

impl State {
    /// Puts a cursor on the pointer.
    ///
    /// A Wayland client draws its own: there is no cursor to ask the
    /// compositor for, only a surface to hand it. The shapes are drawn
    /// here, in the one place that knows what each one means.
    fn set_cursor(&mut self, wanted: Cursor) {
        if self.cursor_shape == Some(wanted) || self.pointer == 0 {
            return;
        }
        self.cursor_shape = Some(wanted);
        let (canvas, hot_x, hot_y) = cursor_picture(wanted);
        let (width, height) = (canvas.pixel_width() as u32, canvas.pixel_height() as u32);
        let length = width as usize * height as usize * 4;
        let Ok(mut memory) = shm::Shared::new(length) else { return };
        // A cursor has a hole in it: what is not the arrow is not drawn at
        // all, which is what the alpha the canvas keeps says.
        let pixels = canvas.pixels();
        for (source, destination) in pixels.chunks_exact(4).zip(memory.bytes().chunks_exact_mut(4))
        {
            let alpha = u32::from(source[3]);
            let shade = |value: u8| ((u32::from(value) * alpha) / 255) as u8;
            destination[0] = shade(source[2]);
            destination[1] = shade(source[1]);
            destination[2] = shade(source[0]);
            destination[3] = source[3];
        }
        let pool = self.connection.make_id();
        let request = Request::new(self.globals.shm, p::wl_shm::CREATE_POOL)
            .uint(pool)
            .int(length as i32)
            .with_fd(memory.fd());
        if self.connection.send(&request).is_err() {
            return;
        }
        let id = self.connection.make_id();
        let request = Request::new(pool, p::wl_shm_pool::CREATE_BUFFER)
            .uint(id)
            .int(0)
            .int(width as i32)
            .int(height as i32)
            .int(width as i32 * 4)
            // A cursor has to be drawn with what is behind it showing
            // through, so this is the one buffer with an alpha channel.
            .uint(0);
        if self.connection.send(&request).is_err() {
            return;
        }
        let surface = self.cursor_surface;
        let _ = self
            .connection
            .send(&Request::new(surface, p::wl_surface::ATTACH).uint(id).int(0).int(0));
        let _ = self.connection.send(
            &Request::new(surface, p::wl_surface::DAMAGE_BUFFER)
                .int(0)
                .int(0)
                .int(width as i32)
                .int(height as i32),
        );
        let _ = self.connection.send(&Request::new(surface, p::wl_surface::COMMIT));
        let serial = self.pointer_serial;
        let _ = self.connection.send(
            &Request::new(self.pointer, p::wl_pointer::SET_CURSOR)
                .uint(serial)
                .uint(surface)
                .int(hot_x)
                .int(hot_y),
        );
        // The old buffer is left to the compositor, which lets it go when
        // it is done; the new one is what the pointer wears now.
        let _ = self.connection.send(&Request::new(pool, p::wl_shm_pool::DESTROY));
        if let Some(old) =
            self.cursor_buffer.replace(Buffer { id, memory, width, height, free: false })
        {
            let _ = self.connection.send(&Request::new(old.id, p::wl_buffer::DESTROY));
        }
    }
}

/// The cursor shapes, drawn rather than fetched.
///
/// X has a font of them and Windows has them built in; Wayland has
/// neither — a client draws its own or shows none. These are the classic
/// shapes: a white arrow with a black edge, an I-beam, the resize arrows
/// and a hand, at the size a pointer has been since pointers began.
fn cursor_picture(cursor: Cursor) -> (Canvas, i32, i32) {
    use wp_raster::Color;
    const WHITE: Color = Color::rgba(255, 255, 255, 255);
    const BLACK: Color = Color::rgba(0, 0, 0, 255);
    let mut canvas = Canvas::new(24, 24);
    canvas.clear(Color::rgba(0, 0, 0, 0));
    let mut ink = |x: i32, y: i32, colour: Color| {
        if (0..24).contains(&x) && (0..24).contains(&y) {
            canvas.fill_rect(x, y, 1, 1, colour);
        }
    };
    match cursor {
        Cursor::Text => {
            // An I-beam: a bar with a serif at each end.
            for y in 3..21 {
                ink(11, y, BLACK);
                ink(12, y, WHITE);
            }
            for x in 8..16 {
                ink(x, 3, BLACK);
                ink(x, 20, BLACK);
                ink(x, 4, WHITE);
                ink(x, 19, WHITE);
            }
            (canvas, 12, 12)
        }
        Cursor::ResizeHorizontal | Cursor::ResizeVertical => {
            let (dx, dy) = if cursor == Cursor::ResizeHorizontal { (1.0, 0.0) } else { (0.0, 1.0) };
            for step in -9..=9 {
                let x = 12 + (dx * step as f32) as i32;
                let y = 12 + (dy * step as f32) as i32;
                for (ox, oy) in [(0, 0), (1, 0), (0, 1)] {
                    ink(x + ox, y + oy, if ox == 0 && oy == 0 { WHITE } else { BLACK });
                }
            }
            // The heads at each end.
            for end in [-9, 9] {
                let x = 12 + (dx * end as f32) as i32;
                let y = 12 + (dy * end as f32) as i32;
                for spread in 1..4 {
                    ink(x - (dy * spread as f32) as i32, y - (dx * spread as f32) as i32, BLACK);
                    ink(x + (dy * spread as f32) as i32, y + (dx * spread as f32) as i32, BLACK);
                }
            }
            (canvas, 12, 12)
        }
        Cursor::Hand => {
            for y in 6..18 {
                for x in 8..16 {
                    let edge = x == 8 || x == 15 || y == 6 || y == 17;
                    ink(x, y, if edge { BLACK } else { WHITE });
                }
            }
            for y in 2..7 {
                ink(11, y, BLACK);
                ink(12, y, WHITE);
                ink(13, y, BLACK);
            }
            (canvas, 12, 4)
        }
        Cursor::Arrow => {
            // The arrow every desktop has: a triangle with a tail.
            for y in 0..17 {
                let width = (y + 1).min(11);
                for x in 0..width {
                    let edge = x == 0 || x == width - 1 || y == 16;
                    ink(x, y, if edge { BLACK } else { WHITE });
                }
            }
            for step in 0..7 {
                ink(6 + step / 2, 12 + step, BLACK);
                ink(7 + step / 2, 12 + step, WHITE);
                ink(8 + step / 2, 12 + step, BLACK);
            }
            (canvas, 0, 0)
        }
    }
}

// --- The clipboard ----------------------------------------------------------

/// What the formats are called where a program hands them over.
const TEXT: &str = "text/plain;charset=utf-8";
const HTML: &str = "text/html";
const RTF: &str = "text/rtf";
const PNG: &str = "image/png";
/// Files, as a list of their addresses.
const URI_LIST: &str = "text/uri-list";

/// Writes what another program asked for into the end of the pipe the
/// compositor handed over, and closes it. The other program is reading;
/// if it has stopped, the write fails and that is all.
fn write_to(fd: std::os::fd::OwnedFd, bytes: &[u8]) {
    use std::io::Write;
    let mut file = std::fs::File::from(fd);
    let _ = file.write_all(bytes);
}

/// Reads one end of a pair until the other program closes its end, or
/// stops writing for too long: what has arrived by then is what there is.
fn read_until_closed(reader: &mut std::os::unix::net::UnixStream) -> Option<Vec<u8>> {
    use std::io::Read;
    let _ = reader.set_read_timeout(Some(Duration::from_millis(1500)));
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut bytes = Vec::new();
    while Instant::now() < deadline {
        let mut chunk = [0u8; 4096];
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => bytes.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    (!bytes.is_empty()).then_some(bytes)
}

impl State {
    fn clipboard_event(&mut self, message: &Message) {
        let mut arguments = message.arguments();
        if message.object == self.data_device {
            match message.opcode {
                p::wl_data_device::DATA_OFFER => {
                    let id = arguments.uint();
                    self.building.insert(id, Vec::new());
                }
                p::wl_data_device::SELECTION => {
                    let id = arguments.uint();
                    // The one before it is no longer anybody's.
                    if self.offer != 0 && self.offer != id {
                        let _ = self
                            .connection
                            .send(&Request::new(self.offer, p::wl_data_offer::DESTROY));
                    }
                    self.offer_formats = self.building.remove(&id).unwrap_or_default();
                    self.offer = id;
                }
                p::wl_data_device::ENTER => {
                    let serial = arguments.uint();
                    let surface = arguments.uint();
                    let x = arguments.fixed();
                    let y = arguments.fixed();
                    let offer = arguments.uint();
                    self.drag_entered(serial, surface, (x.round() as i32, y.round() as i32), offer);
                }
                p::wl_data_device::MOTION => {
                    let _time = arguments.uint();
                    let x = arguments.fixed().round() as i32;
                    let y = arguments.fixed().round() as i32;
                    if let Some(over) = &mut self.drag_over {
                        over.at = (x, y);
                        if over.accepted && !over.files && !over.own {
                            let event = Event::DataDragOver { x, y };
                            HANDED_ON.with(|slot| slot.borrow_mut().push((over.surface, event)));
                        }
                    }
                }
                p::wl_data_device::LEAVE => {
                    if let Some(over) = self.drag_over.take() {
                        if over.accepted && !over.files && !over.own {
                            let event = Event::DataDragLeft;
                            HANDED_ON.with(|slot| slot.borrow_mut().push((over.surface, event)));
                        }
                        let _ = self
                            .connection
                            .send(&Request::new(over.offer, p::wl_data_offer::DESTROY));
                    }
                }
                p::wl_data_device::DROP => self.drag_dropped(),
                _ => {}
            }
            return;
        }
        // What the compositor settled on for a drag over a window.
        if message.opcode == p::wl_data_offer::ACTION {
            if let Some(over) = self.drag_over.as_mut().filter(|over| over.offer == message.object)
            {
                over.action = arguments.uint();
            }
            return;
        }
        // An offer saying what it can be read as.
        if message.opcode == p::wl_data_offer::OFFER {
            let format = arguments.string();
            if let Some(formats) = self.building.get_mut(&message.object) {
                formats.push(format);
            } else if message.object == self.offer {
                self.offer_formats.push(format);
            }
        }
    }

    /// The compositor asking for what this program put on the clipboard.
    fn source_event(&mut self, message: &Message) {
        let mut arguments = message.arguments();
        match message.opcode {
            p::wl_data_source::SEND => {
                let format = arguments.string();
                let Some(fd) = self.connection.take_fd() else { return };
                let Some(contents) = &self.offered else { return };
                let bytes = match format.as_str() {
                    TEXT => contents.text.clone().map(String::into_bytes),
                    HTML => contents.html.clone(),
                    RTF => contents.rtf.clone(),
                    PNG => contents.png.clone(),
                    _ => contents.text.clone().map(String::into_bytes),
                };
                write_to(fd, bytes.as_deref().unwrap_or_default());
            }
            p::wl_data_source::CANCELLED => {
                // Somebody else owns the clipboard now.
                let _ = self
                    .connection
                    .send(&Request::new(self.data_source, p::wl_data_source::DESTROY));
                self.data_source = 0;
                self.offered = None;
            }
            _ => {}
        }
    }

    /// A drag came over a window: what it offers, whether this program
    /// takes it, and — to the compositor — which format it would ask for and
    /// that a move or a copy would both do, a move rather.
    fn drag_entered(&mut self, serial: u32, surface: u32, at: (i32, i32), offer: u32) {
        if let Some(old) = self.drag_over.take() {
            let _ = self.connection.send(&Request::new(old.offer, p::wl_data_offer::DESTROY));
        }
        let formats = self.building.remove(&offer).unwrap_or_default();
        let own = self.dragging.is_some();
        let files = !own && formats.iter().any(|format| format == URI_LIST);
        let wanted = if files {
            Some(URI_LIST)
        } else {
            [TEXT, "text/plain", "UTF8_STRING", HTML, RTF, PNG]
                .into_iter()
                .find(|wanted| formats.iter().any(|format| format == wanted))
        };
        let accepted = wanted.is_some();
        let mut request = Request::new(offer, p::wl_data_offer::ACCEPT).uint(serial);
        request = match wanted {
            Some(format) => request.string(format),
            // No format: the protocol's null string.
            None => request.uint(0),
        };
        let _ = self.connection.send(&request);
        let _ = self.connection.send(
            &Request::new(offer, p::wl_data_offer::SET_ACTIONS)
                .uint(if accepted { p::dnd_action::COPY | p::dnd_action::MOVE } else { 0 })
                .uint(if accepted { p::dnd_action::MOVE } else { 0 }),
        );
        if accepted && !files && !own {
            let event = Event::DataDragOver { x: at.0, y: at.1 };
            HANDED_ON.with(|slot| slot.borrow_mut().push((surface, event)));
        }
        self.drag_over = Some(DragOver {
            offer,
            surface,
            formats,
            files,
            accepted,
            action: p::dnd_action::MOVE,
            at,
            own,
        });
    }

    /// The drag was let go on a window. Another program's: what it carries
    /// is read in the formats this program takes, and the offer finished.
    /// This program's own, come back: where it landed is noted for the drag
    /// to say, and nothing is read — the program giving it is this one, busy
    /// giving it.
    fn drag_dropped(&mut self) {
        let Some(over) = self.drag_over.take() else { return };
        let (x, y) = over.at;
        if over.own {
            if let Some(dragging) = &mut self.dragging {
                dragging.landed = Some((over.surface, x, y));
            }
        } else if over.accepted {
            let offered = |format: &str| over.formats.iter().any(|known| known == format);
            let event = if over.files {
                let list = self.read_offer(over.offer, URI_LIST).unwrap_or_default();
                let paths = super::files::paths_of_uri_list(&String::from_utf8_lossy(&list));
                (!paths.is_empty()).then_some(Event::FilesDropped { paths, x, y })
            } else {
                let mut read = |format: &str| {
                    offered(format).then(|| self.read_offer(over.offer, format)).flatten()
                };
                let text = [TEXT, "text/plain", "UTF8_STRING"]
                    .into_iter()
                    .find_map(&mut read)
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                let contents = Contents {
                    text,
                    html: read(HTML),
                    rtf: read(RTF),
                    png: read(PNG),
                    dib: None,
                    document: None,
                };
                (!contents.is_empty()).then_some(Event::DataDropped {
                    contents,
                    x,
                    y,
                    copying: over.action != p::dnd_action::MOVE,
                })
            };
            if let Some(event) = event {
                HANDED_ON.with(|slot| slot.borrow_mut().push((over.surface, event)));
            }
        }
        if over.accepted {
            let _ = self.connection.send(&Request::new(over.offer, p::wl_data_offer::FINISH));
        }
        let _ = self.connection.send(&Request::new(over.offer, p::wl_data_offer::DESTROY));
    }

    /// The compositor, about this program's own drag: which format the
    /// window under it would take, what it wants the data in, what was
    /// settled on, and how it ended.
    fn drag_source_event(&mut self, message: &Message) {
        let mut arguments = message.arguments();
        match message.opcode {
            p::wl_data_source::SEND => {
                let format = arguments.string();
                let Some(fd) = self.connection.take_fd() else { return };
                let Some(dragging) = &self.dragging else { return };
                let contents = &dragging.contents;
                let bytes = match format.as_str() {
                    HTML => contents.html.clone(),
                    RTF => contents.rtf.clone(),
                    PNG => contents.png.clone(),
                    _ => contents.text.clone().map(String::into_bytes),
                };
                write_to(fd, bytes.as_deref().unwrap_or_default());
            }
            p::wl_data_source::ACTION => {
                if let Some(dragging) = &mut self.dragging {
                    dragging.action = arguments.uint();
                }
            }
            p::wl_data_source::DND_DROP_PERFORMED => {
                if let Some(dragging) = &mut self.dragging {
                    dragging.dropped = true;
                }
            }
            p::wl_data_source::DND_FINISHED => {
                if let Some(dragging) = &mut self.dragging {
                    dragging.finished = true;
                }
            }
            p::wl_data_source::CANCELLED => {
                if let Some(dragging) = &mut self.dragging {
                    dragging.cancelled = true;
                }
            }
            _ => {}
        }
    }

    /// Reads a drag's offer in one format.
    fn read_offer(&mut self, offer: u32, format: &str) -> Option<Vec<u8>> {
        let (mut reader, writer) = std::os::unix::net::UnixStream::pair().ok()?;
        use std::os::fd::AsRawFd;
        let request = Request::new(offer, p::wl_data_offer::RECEIVE)
            .string(format)
            .with_fd(writer.as_raw_fd());
        self.connection.send(&request).ok()?;
        drop(writer);
        read_until_closed(&mut reader)
    }

    /// Reads what another program has put on the clipboard, in one format.
    fn paste(&mut self, format: &str) -> Option<Vec<u8>> {
        if self.offer == 0 || !self.offer_formats.iter().any(|known| known == format) {
            return None;
        }
        // A pair of joined sockets rather than a pipe: the compositor
        // writes into the end it is given and this program reads the
        // other, which is what a pipe would do — and a socket can be made
        // without reaching past the standard library.
        let (mut reader, writer) = std::os::unix::net::UnixStream::pair().ok()?;
        use std::os::fd::AsRawFd;
        let request = Request::new(self.offer, p::wl_data_offer::RECEIVE)
            .string(format)
            .with_fd(writer.as_raw_fd());
        self.connection.send(&request).ok()?;
        // This program must let go of its end of the pair, or the read
        // below would wait for itself to write.
        drop(writer);
        read_until_closed(&mut reader)
    }

    /// Puts something on the clipboard, in every format it has.
    fn copy(&mut self, contents: &Contents) -> bool {
        if self.data_device == 0 || self.globals.data_device_manager == 0 {
            return false;
        }
        if self.data_source != 0 {
            let _ =
                self.connection.send(&Request::new(self.data_source, p::wl_data_source::DESTROY));
        }
        let source = self.connection.make_id();
        let request = Request::new(
            self.globals.data_device_manager,
            p::wl_data_device_manager::CREATE_DATA_SOURCE,
        )
        .uint(source);
        if self.connection.send(&request).is_err() {
            return false;
        }
        let offer = |state: &mut Self, format: &str| {
            let _ = state
                .connection
                .send(&Request::new(source, p::wl_data_source::OFFER).string(format));
        };
        if contents.text.is_some() {
            offer(self, TEXT);
            offer(self, "text/plain");
            offer(self, "TEXT");
            offer(self, "STRING");
            offer(self, "UTF8_STRING");
        }
        if contents.html.is_some() {
            offer(self, HTML);
        }
        if contents.rtf.is_some() {
            offer(self, RTF);
        }
        if contents.png.is_some() {
            offer(self, PNG);
        }
        let serial = self.serial;
        let request = Request::new(self.data_device, p::wl_data_device::SET_SELECTION)
            .uint(source)
            .uint(serial);
        if self.connection.send(&request).is_err() {
            return false;
        }
        self.data_source = source;
        self.offered = Some(contents.clone());
        true
    }
}

// --- What the rest of the shell asks for ------------------------------------

pub(crate) fn open_window(title: &str) -> bool {
    let made = with_state(|state| state.create_window(title, 1400, 900));
    matches!(made, Some(Ok(_)))
}

pub(crate) fn window_count() -> usize {
    with_state(|state| state.windows.len()).unwrap_or(0)
}

/// Laying windows out side by side is the compositor's business on
/// Wayland: a client cannot put its own window anywhere, by design, and
/// the desktop's portal has no interface for it. So it is asked of the
/// compositor in the compositor's own language, where that is one this
/// program speaks — sway's; see [`super::sway`]. Elsewhere nothing moves.
pub(crate) fn arrange_windows() -> usize {
    super::sway::arrange(std::process::id()).unwrap_or(0)
}

pub(crate) fn is_maximised() -> bool {
    let window = WINDOW.with(Cell::get);
    with_state(|state| {
        state.window_index(window).is_some_and(|index| state.windows[index].maximised)
    })
    .unwrap_or(false)
}

pub(crate) fn window_command(command: WindowCommand) {
    let window = WINDOW.with(Cell::get);
    with_state(|state| {
        let Some(index) = state.window_index(window) else { return };
        let (toplevel, maximised) = (state.windows[index].toplevel, state.windows[index].maximised);
        match command {
            WindowCommand::Close => state.closing = true,
            WindowCommand::Minimise => {
                let _ =
                    state.connection.send(&Request::new(toplevel, p::xdg_toplevel::SET_MINIMIZED));
            }
            WindowCommand::ToggleMaximise => {
                let opcode = if maximised {
                    p::xdg_toplevel::UNSET_MAXIMIZED
                } else {
                    p::xdg_toplevel::SET_MAXIMIZED
                };
                let _ = state.connection.send(&Request::new(toplevel, opcode));
            }
        }
    });
}

/// Starts the compositor moving or resizing the window, which is the only
/// way it happens: the program says "the person took hold of me here".
pub(crate) fn begin_move_or_resize(edges: u32) {
    let window = WINDOW.with(Cell::get);
    with_state(|state| {
        let Some(index) = state.window_index(window) else { return };
        let toplevel = state.windows[index].toplevel;
        let (seat, serial) = (state.globals.seat, state.pointer_serial);
        if seat == 0 {
            return;
        }
        let request = if edges == 0 {
            Request::new(toplevel, p::xdg_toplevel::MOVE).uint(seat).uint(serial)
        } else {
            Request::new(toplevel, p::xdg_toplevel::RESIZE).uint(seat).uint(serial).uint(edges)
        };
        let _ = state.connection.send(&request);
    });
}

pub(crate) fn set_window_title(title: &str) {
    let window = WINDOW.with(Cell::get);
    with_state(|state| {
        let Some(index) = state.window_index(window) else { return };
        let toplevel = state.windows[index].toplevel;
        title.clone_into(&mut state.windows[index].title);
        let _ = state
            .connection
            .send(&Request::new(toplevel, p::xdg_toplevel::SET_TITLE).string(title));
    });
}

pub(crate) fn double_click_millis() -> u32 {
    // The compositor does not say, and there is no setting to read: this is
    // the figure every desktop starts at.
    400
}

pub(crate) fn caret_blink_millis() -> Option<u32> {
    Some(530)
}

pub(crate) fn system_code_pages() -> (u32, u32) {
    (1252, 437)
}

/// Tells the input method where the caret is, in the window's own units,
/// so that its list of candidates opens beside it. Said only when it moved,
/// and to the input method only while a window has the keyboard.
pub(crate) fn place_composition(x: i32, y: i32, height: i32) {
    with_state(|state| {
        let rectangle = (x, y, 1, height.max(1));
        if state.text_input.id == 0 || state.text_input.rectangle == Some(rectangle) {
            return;
        }
        state.text_input.rectangle = Some(rectangle);
        if state.text_input.surface.is_none() {
            return;
        }
        state.send_cursor_rectangle(rectangle);
        let id = state.text_input.id;
        let _ = state.connection.send(&Request::new(id, p::zwp_text_input_v3::COMMIT));
    });
}

/// The screen reader is told on the loop's next turn: see [`super::atspi`].
pub(crate) fn selection_changed() {
    super::atspi::note_selection_changed();
}

pub(crate) fn set_frame_appearance(_dark: bool, _border: (u8, u8, u8), _caption: (u8, u8, u8)) {}

/// Gives the contents to the compositor as a drag from the window the
/// button went down in, and follows it until it is let go or given up —
/// the compositor carries it, and says where it went and what was done
/// with it.
pub(crate) fn start_drag(contents: &Contents) -> DragEffect {
    let surface = WINDOW.with(Cell::get);
    let started = with_state(|state| state.begin_drag(surface, contents)).unwrap_or(false);
    if !started {
        return DragEffect::None;
    }
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let message =
            with_state(|state| state.connection.next_message(Duration::from_millis(TICK_MILLIS)));
        match message {
            Some(Ok(Some(message))) => {
                with_state(|state| state.handle(&message));
            }
            Some(Ok(None)) => {}
            Some(Err(_)) | None => break,
        }
        let over = with_state(|state| {
            state.dragging.as_ref().is_none_or(|dragging| {
                dragging.cancelled
                    || dragging.finished
                    || (dragging.landed.is_some() && dragging.dropped)
            })
        })
        .unwrap_or(true);
        if over || Instant::now() > deadline {
            break;
        }
    }
    with_state(|state| {
        let Some(dragging) = state.dragging.take() else { return DragEffect::None };
        let _ = state.connection.send(&Request::new(dragging.source, p::wl_data_source::DESTROY));
        if let Some((_, x, y)) = dragging.landed {
            return DragEffect::DroppedOnSelf {
                x,
                y,
                copying: dragging.action == p::dnd_action::COPY,
            };
        }
        if dragging.cancelled || !dragging.dropped {
            return DragEffect::None;
        }
        if dragging.action == p::dnd_action::MOVE {
            DragEffect::Move
        } else {
            DragEffect::Copy
        }
    })
    .unwrap_or(DragEffect::None)
}

impl State {
    /// Offers the contents in every format they have, and asks the
    /// compositor to start the drag — which it does only for the button
    /// press it last told this program of.
    fn begin_drag(&mut self, surface: u32, contents: &Contents) -> bool {
        if self.data_device == 0 || self.globals.data_device_manager == 0 || contents.is_empty() {
            return false;
        }
        let source = self.connection.make_id();
        let request = Request::new(
            self.globals.data_device_manager,
            p::wl_data_device_manager::CREATE_DATA_SOURCE,
        )
        .uint(source);
        if self.connection.send(&request).is_err() {
            return false;
        }
        let mut formats = Vec::new();
        if contents.text.is_some() {
            formats.extend([TEXT, "text/plain", "UTF8_STRING", "TEXT", "STRING"]);
        }
        if contents.html.is_some() {
            formats.push(HTML);
        }
        if contents.rtf.is_some() {
            formats.push(RTF);
        }
        if contents.png.is_some() {
            formats.push(PNG);
        }
        for format in formats {
            let _ = self
                .connection
                .send(&Request::new(source, p::wl_data_source::OFFER).string(format));
        }
        let _ = self.connection.send(
            &Request::new(source, p::wl_data_source::SET_ACTIONS)
                .uint(p::dnd_action::COPY | p::dnd_action::MOVE),
        );
        let request = Request::new(self.data_device, p::wl_data_device::START_DRAG)
            .uint(source)
            .uint(surface)
            // No picture under the pointer: the compositor's own.
            .uint(0)
            .uint(self.pointer_serial);
        if self.connection.send(&request).is_err() {
            return false;
        }
        self.dragging = Some(Dragging {
            source,
            contents: contents.clone(),
            action: 0,
            dropped: false,
            finished: false,
            cancelled: false,
            landed: None,
        });
        true
    }
}

pub(crate) fn clipboard_set_contents(contents: &Contents) -> bool {
    with_state(|state| state.copy(contents)).unwrap_or(false)
}

pub(crate) fn clipboard_set_text(text: &str) -> bool {
    let contents = Contents { text: Some(text.to_owned()), ..Contents::default() };
    clipboard_set_contents(&contents)
}

pub(crate) fn clipboard_text() -> Option<String> {
    // What this program put there is not offered back to it by the
    // compositor, so it answers from what it kept.
    if let Some(Some(text)) =
        with_state(|state| state.offered.as_ref().and_then(|contents| contents.text.clone()))
    {
        return Some(text);
    }
    let bytes = with_state(|state| {
        state
            .paste(TEXT)
            .or_else(|| state.paste("text/plain"))
            .or_else(|| state.paste("UTF8_STRING"))
    })
    .flatten()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

pub(crate) fn clipboard_contents() -> Contents {
    if let Some(Some(contents)) = with_state(|state| state.offered.clone()) {
        return contents;
    }
    with_state(|state| Contents {
        text: state
            .paste(TEXT)
            .or_else(|| state.paste("text/plain"))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
        html: state.paste(HTML),
        rtf: state.paste(RTF),
        png: state.paste(PNG),
        dib: None,
        document: None,
    })
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

/// A Wayland client cannot see another program's window, or list them —
/// that is the protocol's own rule — so there are none to photograph one
/// of. The whole screen, and a rectangle dragged out of it, are asked of
/// the desktop's portal instead; see [`super::portal`].
pub(crate) fn screen_windows() -> Vec<ScreenWindow> {
    Vec::new()
}

pub(crate) fn capture_window(_handle: usize) -> Option<Shot> {
    None
}

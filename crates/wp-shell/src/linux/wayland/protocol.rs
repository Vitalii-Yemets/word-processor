//! What the numbers on the wire mean.
//!
//! Wayland's interfaces are described in XML files, and most programs turn
//! those into code with a generator. There is nothing here to generate
//! from: what this program uses is a dozen interfaces and perhaps forty
//! messages of them, and the opcodes are the order the messages are
//! written in the description — so they are written out, with the name of
//! each beside its number. What is not here is what this program does not
//! say or listen for.
//!
//! The names are the protocol's own, so that anything found in Wayland's
//! documentation can be found here under the same name.

/// The compositor's own object, which every connection starts with.
pub(crate) mod wl_display {
    pub(crate) const ID: u32 = 1;
    // Requests.
    pub(crate) const GET_REGISTRY: u16 = 1;
    // Events.
    pub(crate) const ERROR: u16 = 0;
}

/// What the compositor offers, and how a program asks for one.
pub(crate) mod wl_registry {
    pub(crate) const BIND: u16 = 0;
    pub(crate) const GLOBAL: u16 = 0;
}

pub(crate) mod wl_compositor {
    pub(crate) const CREATE_SURFACE: u16 = 0;
}

pub(crate) mod wl_surface {
    pub(crate) const DESTROY: u16 = 0;
    pub(crate) const ATTACH: u16 = 1;
    pub(crate) const COMMIT: u16 = 6;
    pub(crate) const SET_BUFFER_SCALE: u16 = 8;
    pub(crate) const DAMAGE_BUFFER: u16 = 9;
}

pub(crate) mod wl_shm {
    pub(crate) const CREATE_POOL: u16 = 0;
    /// Blue, green, red and a byte that is ignored — the only format every
    /// compositor must take, and the order this program's canvas is in.
    pub(crate) const XRGB8888: u32 = 1;
}

pub(crate) mod wl_shm_pool {
    pub(crate) const CREATE_BUFFER: u16 = 0;
    pub(crate) const DESTROY: u16 = 1;
}

pub(crate) mod wl_buffer {
    pub(crate) const DESTROY: u16 = 0;
    pub(crate) const RELEASE: u16 = 0;
}

/// The window itself: the shell protocol every desktop agrees on.
pub(crate) mod xdg_wm_base {
    pub(crate) const GET_XDG_SURFACE: u16 = 2;
    pub(crate) const PONG: u16 = 3;
    // Events.
    pub(crate) const PING: u16 = 0;
}

pub(crate) mod xdg_surface {
    pub(crate) const DESTROY: u16 = 0;
    pub(crate) const GET_TOPLEVEL: u16 = 1;
    pub(crate) const ACK_CONFIGURE: u16 = 4;
    // Events.
    pub(crate) const CONFIGURE: u16 = 0;
}

pub(crate) mod xdg_toplevel {
    pub(crate) const DESTROY: u16 = 0;
    pub(crate) const SET_TITLE: u16 = 2;
    pub(crate) const SET_APP_ID: u16 = 3;
    pub(crate) const MOVE: u16 = 5;
    pub(crate) const RESIZE: u16 = 6;
    pub(crate) const SET_MAXIMIZED: u16 = 9;
    pub(crate) const UNSET_MAXIMIZED: u16 = 10;
    pub(crate) const SET_MINIMIZED: u16 = 13;
    // Events.
    pub(crate) const CONFIGURE: u16 = 0;
    pub(crate) const CLOSE: u16 = 1;

    /// Which edge is being dragged, as `resize` numbers them.
    pub(crate) const RESIZE_TOP: u32 = 1;
    pub(crate) const RESIZE_BOTTOM: u32 = 2;
    pub(crate) const RESIZE_LEFT: u32 = 4;
    pub(crate) const RESIZE_RIGHT: u32 = 8;

    /// What the compositor says the window is now, in `configure`.
    pub(crate) const STATE_MAXIMIZED: u32 = 1;
}

pub(crate) mod wl_seat {
    pub(crate) const GET_POINTER: u16 = 0;
    pub(crate) const GET_KEYBOARD: u16 = 1;
    // Events.
    pub(crate) const CAPABILITIES: u16 = 0;

    /// What the seat says it has, which is what may be asked for.
    pub(crate) const POINTER: u32 = 1;
    pub(crate) const KEYBOARD: u32 = 2;
}

pub(crate) mod wl_pointer {
    pub(crate) const SET_CURSOR: u16 = 0;
    pub(crate) const RELEASE: u16 = 1;
    // Events.
    pub(crate) const ENTER: u16 = 0;
    pub(crate) const LEAVE: u16 = 1;
    pub(crate) const MOTION: u16 = 2;
    pub(crate) const BUTTON: u16 = 3;
    pub(crate) const AXIS: u16 = 4;

    /// The buttons, as Linux's input layer numbers them.
    pub(crate) const BUTTON_LEFT: u32 = 0x110;
    pub(crate) const BUTTON_RIGHT: u32 = 0x111;
    pub(crate) const BUTTON_MIDDLE: u32 = 0x112;
    pub(crate) const PRESSED: u32 = 1;
    pub(crate) const AXIS_VERTICAL: u32 = 0;
}

pub(crate) mod wl_keyboard {
    pub(crate) const RELEASE: u16 = 0;
    // Events.
    pub(crate) const KEYMAP: u16 = 0;
    pub(crate) const ENTER: u16 = 1;
    pub(crate) const LEAVE: u16 = 2;
    pub(crate) const KEY: u16 = 3;
    pub(crate) const MODIFIERS: u16 = 4;

    pub(crate) const PRESSED: u32 = 1;
    /// The keymap as a file of xkb's own text format, which is the only
    /// kind a compositor sends.
    pub(crate) const FORMAT_XKB_V1: u32 = 1;
}

pub(crate) mod wl_data_device_manager {
    pub(crate) const CREATE_DATA_SOURCE: u16 = 0;
    pub(crate) const GET_DATA_DEVICE: u16 = 1;
}

pub(crate) mod wl_data_device {
    pub(crate) const START_DRAG: u16 = 0;
    pub(crate) const SET_SELECTION: u16 = 1;
    // Events.
    pub(crate) const DATA_OFFER: u16 = 0;
    pub(crate) const ENTER: u16 = 1;
    pub(crate) const LEAVE: u16 = 2;
    pub(crate) const MOTION: u16 = 3;
    pub(crate) const DROP: u16 = 4;
    pub(crate) const SELECTION: u16 = 5;
}

pub(crate) mod wl_data_offer {
    pub(crate) const ACCEPT: u16 = 0;
    pub(crate) const RECEIVE: u16 = 1;
    pub(crate) const DESTROY: u16 = 2;
    pub(crate) const FINISH: u16 = 3;
    pub(crate) const SET_ACTIONS: u16 = 4;
    // Events.
    pub(crate) const OFFER: u16 = 0;
    pub(crate) const ACTION: u16 = 2;
}

pub(crate) mod wl_data_source {
    pub(crate) const OFFER: u16 = 0;
    pub(crate) const DESTROY: u16 = 1;
    pub(crate) const SET_ACTIONS: u16 = 2;
    // Events.
    pub(crate) const SEND: u16 = 1;
    pub(crate) const CANCELLED: u16 = 2;
    pub(crate) const DND_DROP_PERFORMED: u16 = 3;
    pub(crate) const DND_FINISHED: u16 = 4;
    pub(crate) const ACTION: u16 = 5;
}

/// What a drag may do with what it carries, as the data device numbers it.
pub(crate) mod dnd_action {
    pub(crate) const COPY: u32 = 1;
    pub(crate) const MOVE: u32 = 2;
}

pub(crate) mod wl_output {
    // Events.
    pub(crate) const SCALE: u16 = 3;
}

/// The scale a compositor would rather the window drew itself at, as a
/// fraction rather than a whole number.
pub(crate) mod wp_fractional_scale_manager_v1 {
    pub(crate) const GET_FRACTIONAL_SCALE: u16 = 1;
}

pub(crate) mod wp_fractional_scale_v1 {
    // Events.
    pub(crate) const PREFERRED_SCALE: u16 = 0;
}

/// The input method's side of the keyboard: text composed, and text
/// committed, said to the window that has the keyboard.
pub(crate) mod zwp_text_input_manager_v3 {
    pub(crate) const GET_TEXT_INPUT: u16 = 1;
}

pub(crate) mod zwp_text_input_v3 {
    pub(crate) const ENABLE: u16 = 1;
    pub(crate) const DISABLE: u16 = 2;
    pub(crate) const SET_CONTENT_TYPE: u16 = 5;
    pub(crate) const SET_CURSOR_RECTANGLE: u16 = 6;
    pub(crate) const COMMIT: u16 = 7;
    // Events.
    pub(crate) const ENTER: u16 = 0;
    pub(crate) const LEAVE: u16 = 1;
    pub(crate) const PREEDIT_STRING: u16 = 2;
    pub(crate) const COMMIT_STRING: u16 = 3;
    pub(crate) const DONE: u16 = 5;

    /// What the text is for: nothing hinted, and ordinary text — which is
    /// what a document is.
    pub(crate) const HINT_NONE: u32 = 0;
    pub(crate) const PURPOSE_NORMAL: u32 = 0;
}

/// What the interfaces are called where the compositor lists them, with the
/// version this program speaks.
pub(crate) const WANTED: &[(&str, u32)] = &[
    ("wl_compositor", 4),
    ("wl_shm", 1),
    ("xdg_wm_base", 2),
    ("wl_seat", 5),
    ("wl_output", 2),
    ("wl_data_device_manager", 3),
    ("wp_fractional_scale_manager_v1", 1),
    ("zwp_text_input_manager_v3", 1),
];

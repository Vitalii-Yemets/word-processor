//! The X protocol, spoken over the socket.
//!
//! An X server is a program at the other end of a socket, and everything a
//! window does — being made, being shown, being drawn on, being told about
//! the keyboard and the pointer — is a message to it or from it, laid out
//! byte by byte the way the protocol says. Nothing more is needed to put a
//! window on a Linux desktop, so nothing more is used: no Xlib, no xcb, no
//! library of any kind. What follows is the little of the protocol this
//! program speaks, written out against the specification.
//!
//! Requests go out on the socket; replies and events come back on it, and
//! since a reply is waited for while events keep arriving, the events that
//! arrive in the meantime are kept aside for the loop to take afterwards.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// Why the server could not be reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    NoDisplay,
    Socket(String),
    Refused(String),
    Protocol(String),
}

impl core::fmt::Display for Failure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoDisplay => {
                f.write_str("DISPLAY is not set: there is no X server to open a window on")
            }
            Self::Socket(what) => write!(f, "cannot reach the X server: {what}"),
            Self::Refused(what) => write!(f, "the X server refused the connection: {what}"),
            Self::Protocol(what) => write!(f, "the X server answered wrongly: {what}"),
        }
    }
}

/// What the server said of itself when the connection was made.
#[derive(Clone, Debug)]
pub(crate) struct Setup {
    pub(crate) resource_base: u32,
    pub(crate) resource_mask: u32,
    /// In four-byte units.
    pub(crate) max_request_length: u32,
    pub(crate) image_byte_order_msb: bool,
    pub(crate) min_keycode: u8,
    pub(crate) max_keycode: u8,
    pub(crate) root: u32,
    pub(crate) root_visual: u32,
    pub(crate) root_depth: u8,
    pub(crate) black: u32,
    pub(crate) white: u32,
    pub(crate) screen_width: u16,
    pub(crate) screen_height: u16,
    pub(crate) screen_width_mm: u16,
    /// The bits per pixel of the root depth's format.
    pub(crate) bits_per_pixel: u8,
    pub(crate) scanline_pad: u8,
    /// The root visual's colour masks.
    pub(crate) red_mask: u32,
    pub(crate) green_mask: u32,
    pub(crate) blue_mask: u32,
}

/// A packet from the server: an event, or a reply with its body.
#[derive(Clone, Debug)]
pub(crate) struct Packet {
    pub(crate) bytes: Vec<u8>,
}

impl Packet {
    pub(crate) fn kind(&self) -> u8 {
        self.bytes.first().copied().unwrap_or(0) & 0x7F
    }

    pub(crate) fn detail(&self) -> u8 {
        self.bytes.get(1).copied().unwrap_or(0)
    }

    pub(crate) fn u8_at(&self, at: usize) -> u8 {
        self.bytes.get(at).copied().unwrap_or(0)
    }

    pub(crate) fn u16_at(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.u8_at(at), self.u8_at(at + 1)])
    }

    pub(crate) fn i16_at(&self, at: usize) -> i16 {
        self.u16_at(at) as i16
    }

    pub(crate) fn u32_at(&self, at: usize) -> u32 {
        u32::from_le_bytes([
            self.u8_at(at),
            self.u8_at(at + 1),
            self.u8_at(at + 2),
            self.u8_at(at + 3),
        ])
    }
}

/// The connection to the server.
pub(crate) struct Connection {
    stream: UnixStream,
    pub(crate) setup: Setup,
    /// The sequence number of the last request sent.
    sequence: u16,
    next_resource: u32,
    /// Events read while a reply was waited for.
    pending: VecDeque<Packet>,
    /// Bytes read off the socket and not yet made into a packet: a read
    /// that ran out of time part way through one keeps what it got.
    inbox: Vec<u8>,
    atoms: HashMap<String, u32>,
    atom_names: HashMap<u32, String>,
    /// Whether the server takes requests longer than the protocol's own
    /// limit, which a whole window's pixels need.
    big_requests: bool,
}

// Opcodes.
const CREATE_WINDOW: u8 = 1;
const CHANGE_WINDOW_ATTRIBUTES: u8 = 2;
const GET_WINDOW_ATTRIBUTES: u8 = 3;
const DESTROY_WINDOW: u8 = 4;
const MAP_WINDOW: u8 = 8;
const CONFIGURE_WINDOW: u8 = 12;
const GET_GEOMETRY: u8 = 14;
const QUERY_TREE: u8 = 15;
const INTERN_ATOM: u8 = 16;
const CHANGE_PROPERTY: u8 = 18;
const GET_PROPERTY: u8 = 20;
const SET_SELECTION_OWNER: u8 = 22;
const GET_SELECTION_OWNER: u8 = 23;
const CONVERT_SELECTION: u8 = 24;
const SEND_EVENT: u8 = 25;
const GRAB_POINTER: u8 = 26;
const UNGRAB_POINTER: u8 = 27;
const TRANSLATE_COORDINATES: u8 = 40;
const OPEN_FONT: u8 = 45;
const CREATE_GC: u8 = 55;
const PUT_IMAGE: u8 = 72;
const GET_IMAGE: u8 = 73;
const CREATE_GLYPH_CURSOR: u8 = 94;
const QUERY_EXTENSION: u8 = 98;
const GET_KEYBOARD_MAPPING: u8 = 101;

/// What the server's packets are.
pub(crate) const ERROR: u8 = 0;
pub(crate) const REPLY: u8 = 1;
pub(crate) const KEY_PRESS: u8 = 2;
pub(crate) const KEY_RELEASE: u8 = 3;
pub(crate) const BUTTON_PRESS: u8 = 4;
pub(crate) const BUTTON_RELEASE: u8 = 5;
pub(crate) const MOTION_NOTIFY: u8 = 6;
pub(crate) const LEAVE_NOTIFY: u8 = 8;
pub(crate) const FOCUS_IN: u8 = 9;
pub(crate) const FOCUS_OUT: u8 = 10;
pub(crate) const EXPOSE: u8 = 12;
pub(crate) const DESTROY_NOTIFY: u8 = 17;
pub(crate) const CONFIGURE_NOTIFY: u8 = 22;
pub(crate) const SELECTION_CLEAR: u8 = 29;
pub(crate) const SELECTION_REQUEST: u8 = 30;
pub(crate) const SELECTION_NOTIFY: u8 = 31;
pub(crate) const CLIENT_MESSAGE: u8 = 33;
pub(crate) const MAPPING_NOTIFY: u8 = 34;

/// The events a window asks to be told of.
pub(crate) const EVENT_MASK: u32 = 0x0000_0001 // KeyPress
    | 0x0000_0002 // KeyRelease
    | 0x0000_0004 // ButtonPress
    | 0x0000_0008 // ButtonRelease
    | 0x0000_0020 // LeaveWindow
    | 0x0000_0040 // PointerMotion
    | 0x0000_8000 // Exposure
    | 0x0002_0000 // StructureNotify
    | 0x0020_0000 // FocusChange
    | 0x0040_0000; // PropertyChange

/// The masks a message to the window manager is sent with.
pub(crate) const SUBSTRUCTURE_MASK: u32 = 0x0008_0000 | 0x0010_0000;

pub(crate) const ATOM_ATOM: u32 = 4;
pub(crate) const ATOM_STRING: u32 = 31;
pub(crate) const ATOM_WM_NAME: u32 = 39;

fn pad4(length: usize) -> usize {
    (4 - length % 4) % 4
}

/// A request being put together.
struct Request {
    bytes: Vec<u8>,
}

impl Request {
    fn new(opcode: u8, detail: u8) -> Self {
        Self { bytes: vec![opcode, detail, 0, 0] }
    }

    fn u8(mut self, value: u8) -> Self {
        self.bytes.push(value);
        self
    }

    fn u16(mut self, value: u16) -> Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn i16(self, value: i16) -> Self {
        self.u16(value as u16)
    }

    fn u32(mut self, value: u32) -> Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn pad(mut self, count: usize) -> Self {
        self.bytes.extend(std::iter::repeat_n(0, count));
        self
    }

    fn bytes(mut self, data: &[u8]) -> Self {
        self.bytes.extend_from_slice(data);
        let padding = pad4(data.len());
        self.bytes.extend(std::iter::repeat_n(0, padding));
        self
    }

    /// The finished request, with its length filled in — the long way when
    /// it is longer than the short length can say.
    fn finish(mut self, big: bool) -> Vec<u8> {
        let units = self.bytes.len() / 4;
        if units <= 0xFFFF {
            self.bytes[2..4].copy_from_slice(&(units as u16).to_le_bytes());
            self.bytes
        } else if big {
            let mut out = Vec::with_capacity(self.bytes.len() + 4);
            out.extend_from_slice(&self.bytes[..2]);
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(&((units + 1) as u32).to_le_bytes());
            out.extend_from_slice(&self.bytes[4..]);
            out
        } else {
            self.bytes
        }
    }
}

impl Connection {
    /// Connects to the server `DISPLAY` names.
    pub(crate) fn open() -> Result<Self, Failure> {
        let display = std::env::var("DISPLAY").map_err(|_| Failure::NoDisplay)?;
        let (host, number) = parse_display(&display).ok_or(Failure::NoDisplay)?;
        let path = if host.is_empty() || host == "unix" {
            format!("/tmp/.X11-unix/X{number}")
        } else {
            return Err(Failure::Socket(format!("only a local display is reached, not {host}")));
        };
        let stream =
            UnixStream::connect(&path).map_err(|error| Failure::Socket(error.to_string()))?;
        let mut connection = Self {
            stream,
            setup: Setup {
                resource_base: 0,
                resource_mask: 0,
                max_request_length: 0,
                image_byte_order_msb: false,
                min_keycode: 8,
                max_keycode: 255,
                root: 0,
                root_visual: 0,
                root_depth: 24,
                black: 0,
                white: 0xFF_FFFF,
                screen_width: 0,
                screen_height: 0,
                screen_width_mm: 0,
                bits_per_pixel: 32,
                scanline_pad: 32,
                red_mask: 0xFF_0000,
                green_mask: 0xFF00,
                blue_mask: 0xFF,
            },
            sequence: 0,
            next_resource: 0,
            pending: VecDeque::new(),
            inbox: Vec::new(),
            atoms: HashMap::new(),
            atom_names: HashMap::new(),
            big_requests: false,
        };
        let cookie = authority_cookie(number);
        match connection.handshake(cookie.as_deref()) {
            Ok(()) => {}
            Err(Failure::Refused(_)) if cookie.is_some() => {
                // Without the cookie, which a server that trusts local
                // connections accepts.
                connection.stream = UnixStream::connect(&path)
                    .map_err(|error| Failure::Socket(error.to_string()))?;
                connection.handshake(None)?;
            }
            Err(error) => return Err(error),
        }
        connection.enable_big_requests();
        Ok(connection)
    }

    fn handshake(&mut self, cookie: Option<&[u8]>) -> Result<(), Failure> {
        let name: &[u8] = if cookie.is_some() { b"MIT-MAGIC-COOKIE-1" } else { b"" };
        let data = cookie.unwrap_or(&[]);
        let mut request = vec![b'l', 0, 11, 0, 0, 0];
        request.extend_from_slice(&(name.len() as u16).to_le_bytes());
        request.extend_from_slice(&(data.len() as u16).to_le_bytes());
        request.extend_from_slice(&[0, 0]);
        request.extend_from_slice(name);
        request.extend(std::iter::repeat_n(0, pad4(name.len())));
        request.extend_from_slice(data);
        request.extend(std::iter::repeat_n(0, pad4(data.len())));
        self.stream.write_all(&request).map_err(|error| Failure::Socket(error.to_string()))?;

        let mut head = [0u8; 8];
        self.stream.read_exact(&mut head).map_err(|error| Failure::Socket(error.to_string()))?;
        let length = usize::from(u16::from_le_bytes([head[6], head[7]])) * 4;
        let mut body = vec![0u8; length];
        self.stream.read_exact(&mut body).map_err(|error| Failure::Socket(error.to_string()))?;
        match head[0] {
            1 => {}
            0 => {
                let reason_length = usize::from(head[1]).min(body.len());
                let reason = String::from_utf8_lossy(&body[..reason_length]).into_owned();
                return Err(Failure::Refused(reason));
            }
            _ => {
                return Err(Failure::Refused("authentication is wanted in another way".to_owned()))
            }
        }
        self.parse_setup(&body)
    }

    fn parse_setup(&mut self, body: &[u8]) -> Result<(), Failure> {
        let packet = Packet { bytes: body.to_vec() };
        let at = |offset: usize| packet.u32_at(offset);
        self.setup.resource_base = at(4);
        self.setup.resource_mask = at(8);
        let vendor_length = usize::from(packet.u16_at(16));
        self.setup.max_request_length = u32::from(packet.u16_at(18));
        let screens = usize::from(packet.u8_at(20));
        let formats = usize::from(packet.u8_at(21));
        self.setup.image_byte_order_msb = packet.u8_at(22) == 1;
        self.setup.min_keycode = packet.u8_at(26);
        self.setup.max_keycode = packet.u8_at(27);
        let mut cursor = 32 + vendor_length + pad4(vendor_length);
        let mut format_of: HashMap<u8, (u8, u8)> = HashMap::new();
        for _ in 0..formats {
            format_of
                .insert(packet.u8_at(cursor), (packet.u8_at(cursor + 1), packet.u8_at(cursor + 2)));
            cursor += 8;
        }
        if screens == 0 {
            return Err(Failure::Protocol("no screen".to_owned()));
        }
        // The first screen is the one used.
        self.setup.root = packet.u32_at(cursor);
        self.setup.white = packet.u32_at(cursor + 8);
        self.setup.black = packet.u32_at(cursor + 12);
        self.setup.screen_width = packet.u16_at(cursor + 20);
        self.setup.screen_height = packet.u16_at(cursor + 22);
        self.setup.screen_width_mm = packet.u16_at(cursor + 24);
        self.setup.root_visual = packet.u32_at(cursor + 32);
        self.setup.root_depth = packet.u8_at(cursor + 38);
        let depths = usize::from(packet.u8_at(cursor + 39));
        cursor += 40;
        for _ in 0..depths {
            let depth = packet.u8_at(cursor);
            let visuals = usize::from(packet.u16_at(cursor + 2));
            cursor += 8;
            for _ in 0..visuals {
                if packet.u32_at(cursor) == self.setup.root_visual && depth == self.setup.root_depth
                {
                    self.setup.red_mask = packet.u32_at(cursor + 8);
                    self.setup.green_mask = packet.u32_at(cursor + 12);
                    self.setup.blue_mask = packet.u32_at(cursor + 16);
                }
                cursor += 24;
            }
        }
        if let Some((bits, pad)) = format_of.get(&self.setup.root_depth) {
            self.setup.bits_per_pixel = *bits;
            self.setup.scanline_pad = *pad;
        }
        Ok(())
    }

    /// Asks for requests longer than the protocol's own limit, which the
    /// pixels of a whole window need in one piece.
    fn enable_big_requests(&mut self) {
        let Ok(Some(opcode)) = self.query_extension("BIG-REQUESTS") else { return };
        let request = Request::new(opcode, 0).finish(false);
        if let Ok(reply) = self.request_with_reply(&request) {
            let most = reply.u32_at(8);
            if most > self.setup.max_request_length {
                self.setup.max_request_length = most;
                self.big_requests = true;
            }
        }
    }

    /// An extension's major opcode, if the server has it.
    pub(crate) fn query_extension(&mut self, name: &str) -> Result<Option<u8>, Failure> {
        let request = Request::new(QUERY_EXTENSION, 0)
            .u16(name.len() as u16)
            .pad(2)
            .bytes(name.as_bytes())
            .finish(false);
        let reply = self.request_with_reply(&request)?;
        Ok((reply.u8_at(8) == 1).then(|| reply.u8_at(9)))
    }

    /// A resource id nothing else has.
    pub(crate) fn make_id(&mut self) -> u32 {
        let id = self.setup.resource_base | (self.next_resource & self.setup.resource_mask);
        self.next_resource += 1;
        id
    }

    fn send(&mut self, request: &[u8]) -> Result<u16, Failure> {
        self.sequence = self.sequence.wrapping_add(1);
        self.stream.write_all(request).map_err(|error| Failure::Socket(error.to_string()))?;
        Ok(self.sequence)
    }

    /// Sends a request that has no reply.
    pub(crate) fn request(&mut self, request: &[u8]) -> Result<(), Failure> {
        self.send(request).map(|_| ())
    }

    /// Sends a request and waits for its reply, keeping aside the events
    /// that arrive first.
    pub(crate) fn request_with_reply(&mut self, request: &[u8]) -> Result<Packet, Failure> {
        let sequence = self.send(request)?;
        self.stream.flush().map_err(|error| Failure::Socket(error.to_string()))?;
        loop {
            // Off the socket, not out of what is kept aside: an event kept
            // aside is not the reply, and would only go round again.
            let packet =
                self.read_raw(None)?.ok_or_else(|| Failure::Socket("closed".to_owned()))?;
            match packet.kind() {
                REPLY if packet.u16_at(2) == sequence => return Ok(packet),
                ERROR if packet.u16_at(2) == sequence => {
                    return Err(Failure::Protocol(format!(
                        "error {} for request {}",
                        packet.u8_at(1),
                        packet.u8_at(10)
                    )));
                }
                ERROR | REPLY => {}
                _ => self.pending.push_back(packet),
            }
        }
    }

    /// The next packet from the server: one kept aside, or one read, waiting
    /// at most the given time. Nothing when the wait ran out.
    pub(crate) fn read_packet(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<Option<Packet>, Failure> {
        if let Some(packet) = self.pending.pop_front() {
            return Ok(Some(packet));
        }
        self.read_raw(timeout)
    }

    /// The next packet off the socket, waiting at most the given time.
    /// Nothing when the wait ran out — and what was read of a packet by
    /// then stays in the inbox for the next call.
    fn read_raw(&mut self, timeout: Option<Duration>) -> Result<Option<Packet>, Failure> {
        if !self.fill(32, timeout)? {
            return Ok(None);
        }
        let mut length = 32;
        if self.inbox[0] & 0x7F == REPLY {
            // A reply may carry more after its first thirty-two bytes.
            let head = &self.inbox;
            length += u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize * 4;
            if !self.fill(length, None)? {
                return Ok(None);
            }
        }
        let bytes = self.inbox.drain(..length).collect();
        Ok(Some(Packet { bytes }))
    }

    /// Reads until the inbox holds at least the bytes wanted. Whether it
    /// does by the time the wait ran out.
    fn fill(&mut self, wanted: usize, timeout: Option<Duration>) -> Result<bool, Failure> {
        if self.inbox.len() >= wanted {
            return Ok(true);
        }
        self.stream
            .set_read_timeout(timeout)
            .map_err(|error| Failure::Socket(error.to_string()))?;
        let mut chunk = [0u8; 4096];
        while self.inbox.len() < wanted {
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(Failure::Socket("closed".to_owned())),
                Ok(count) => self.inbox.extend_from_slice(&chunk[..count]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(false);
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(Failure::Socket(error.to_string())),
            }
        }
        Ok(true)
    }

    /// The next event, waiting at most the given time.
    pub(crate) fn next_event(&mut self, timeout: Duration) -> Result<Option<Packet>, Failure> {
        loop {
            let Some(packet) = self.read_packet(Some(timeout))? else { return Ok(None) };
            match packet.kind() {
                REPLY | ERROR => continue,
                _ => return Ok(Some(packet)),
            }
        }
    }

    /// Puts a packet back at the front of those kept aside, for a reader
    /// that took one it was not looking for.
    pub(crate) fn push_front(&mut self, packet: Packet) {
        self.pending.push_front(packet);
    }

    pub(crate) fn flush(&mut self) -> Result<(), Failure> {
        self.stream.flush().map_err(|error| Failure::Socket(error.to_string()))
    }

    // --- Atoms -----------------------------------------------------------------

    /// The atom of a name, interned if the server has not seen it.
    pub(crate) fn atom(&mut self, name: &str) -> Result<u32, Failure> {
        if let Some(atom) = self.atoms.get(name) {
            return Ok(*atom);
        }
        let request = Request::new(INTERN_ATOM, 0)
            .u16(name.len() as u16)
            .pad(2)
            .bytes(name.as_bytes())
            .finish(false);
        let reply = self.request_with_reply(&request)?;
        let atom = reply.u32_at(8);
        self.atoms.insert(name.to_owned(), atom);
        self.atom_names.insert(atom, name.to_owned());
        Ok(atom)
    }

    // --- Windows ---------------------------------------------------------------

    /// Makes a window on the root, of the root's depth and visual.
    pub(crate) fn create_window(&mut self, width: u16, height: u16) -> Result<u32, Failure> {
        let id = self.make_id();
        let request = Request::new(CREATE_WINDOW, 0)
            .u32(id)
            .u32(self.setup.root)
            .i16(0)
            .i16(0)
            .u16(width.max(1))
            .u16(height.max(1))
            .u16(0)
            .u16(1)
            .u32(0)
            .u32(0x0000_0002 | 0x0000_0800)
            .u32(self.setup.white)
            .u32(EVENT_MASK)
            .finish(false);
        self.request(&request)?;
        Ok(id)
    }

    pub(crate) fn map_window(&mut self, window: u32) -> Result<(), Failure> {
        self.request(&Request::new(MAP_WINDOW, 0).u32(window).finish(false))
    }

    pub(crate) fn destroy_window(&mut self, window: u32) -> Result<(), Failure> {
        self.request(&Request::new(DESTROY_WINDOW, 0).u32(window).finish(false))
    }

    /// Moves and sizes a window.
    pub(crate) fn configure_window(
        &mut self,
        window: u32,
        x: Option<i32>,
        y: Option<i32>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<(), Failure> {
        let mut mask = 0u16;
        let mut values = Vec::new();
        for (bit, value) in [(1u16, x), (2, y)] {
            if let Some(value) = value {
                mask |= bit;
                values.push(value as u32);
            }
        }
        for (bit, value) in [(4u16, width), (8, height)] {
            if let Some(value) = value {
                mask |= bit;
                values.push(value.max(1));
            }
        }
        let mut request = Request::new(CONFIGURE_WINDOW, 0).u32(window).u16(mask).pad(2);
        for value in values {
            request = request.u32(value);
        }
        self.request(&request.finish(false))
    }

    /// Asks to be told of a window's events — another program's window
    /// too, whose events every client may ask for separately.
    pub(crate) fn select_input(&mut self, window: u32, mask: u32) -> Result<(), Failure> {
        let request = Request::new(CHANGE_WINDOW_ATTRIBUTES, 0)
            .u32(window)
            .u32(0x0000_0800)
            .u32(mask)
            .finish(false);
        self.request(&request)
    }

    /// Sets the pointer a window shows.
    pub(crate) fn set_cursor(&mut self, window: u32, cursor: u32) -> Result<(), Failure> {
        let request = Request::new(CHANGE_WINDOW_ATTRIBUTES, 0)
            .u32(window)
            .u32(0x0000_4000)
            .u32(cursor)
            .finish(false);
        self.request(&request)
    }

    /// A pointer shape from the cursor font, by the glyph's number.
    pub(crate) fn glyph_cursor(&mut self, font: u32, glyph: u16) -> Result<u32, Failure> {
        let id = self.make_id();
        let request = Request::new(CREATE_GLYPH_CURSOR, 0)
            .u32(id)
            .u32(font)
            .u32(font)
            .u16(glyph)
            .u16(glyph + 1)
            .u16(0)
            .u16(0)
            .u16(0)
            .u16(0xFFFF)
            .u16(0xFFFF)
            .u16(0xFFFF)
            .finish(false);
        self.request(&request)?;
        Ok(id)
    }

    pub(crate) fn open_font(&mut self, name: &str) -> Result<u32, Failure> {
        let id = self.make_id();
        let request = Request::new(OPEN_FONT, 0)
            .u32(id)
            .u16(name.len() as u16)
            .pad(2)
            .bytes(name.as_bytes())
            .finish(false);
        self.request(&request)?;
        Ok(id)
    }

    pub(crate) fn create_gc(&mut self, drawable: u32) -> Result<u32, Failure> {
        let id = self.make_id();
        let request = Request::new(CREATE_GC, 0).u32(id).u32(drawable).u32(0).finish(false);
        self.request(&request)?;
        Ok(id)
    }

    /// The window's place and size on the screen.
    pub(crate) fn geometry(&mut self, window: u32) -> Result<(i32, i32, u32, u32), Failure> {
        let reply =
            self.request_with_reply(&Request::new(GET_GEOMETRY, 0).u32(window).finish(false))?;
        let (x, y) = self.translate(window, 0, 0)?;
        Ok((x, y, u32::from(reply.u16_at(16)), u32::from(reply.u16_at(18))))
    }

    /// The children of a window, bottom to top.
    pub(crate) fn query_tree(&mut self, window: u32) -> Result<Vec<u32>, Failure> {
        let reply =
            self.request_with_reply(&Request::new(QUERY_TREE, 0).u32(window).finish(false))?;
        let count = usize::from(reply.u16_at(16));
        Ok((0..count).map(|index| reply.u32_at(32 + index * 4)).collect())
    }

    /// Whether a window is mapped, and every window above it is too.
    pub(crate) fn is_viewable(&mut self, window: u32) -> Result<bool, Failure> {
        let request = Request::new(GET_WINDOW_ATTRIBUTES, 0).u32(window).finish(false);
        let reply = self.request_with_reply(&request)?;
        Ok(reply.u8_at(26) == 2)
    }

    /// A point in a window as one on the root.
    pub(crate) fn translate(&mut self, window: u32, x: i16, y: i16) -> Result<(i32, i32), Failure> {
        let request = Request::new(TRANSLATE_COORDINATES, 0)
            .u32(window)
            .u32(self.setup.root)
            .i16(x)
            .i16(y)
            .finish(false);
        let reply = self.request_with_reply(&request)?;
        Ok((i32::from(reply.i16_at(12)), i32::from(reply.i16_at(14))))
    }

    // --- Properties -------------------------------------------------------------

    /// Sets a property to bytes of a format.
    pub(crate) fn set_property(
        &mut self,
        window: u32,
        property: u32,
        kind: u32,
        format: u8,
        data: &[u8],
    ) -> Result<(), Failure> {
        let units = data.len() / usize::from(format / 8).max(1);
        let request = Request::new(CHANGE_PROPERTY, 0)
            .u32(window)
            .u32(property)
            .u32(kind)
            .u8(format)
            .pad(3)
            .u32(units as u32)
            .bytes(data)
            .finish(self.big_requests);
        self.request(&request)
    }

    pub(crate) fn set_property_atoms(
        &mut self,
        window: u32,
        property: u32,
        atoms: &[u32],
    ) -> Result<(), Failure> {
        let mut data = Vec::with_capacity(atoms.len() * 4);
        for atom in atoms {
            data.extend_from_slice(&atom.to_le_bytes());
        }
        self.set_property(window, property, ATOM_ATOM, 32, &data)
    }

    /// A property's bytes and its type, taking it off the window if asked.
    pub(crate) fn get_property(
        &mut self,
        window: u32,
        property: u32,
        delete: bool,
    ) -> Result<(u32, u8, Vec<u8>), Failure> {
        let mut out = Vec::new();
        let mut offset = 0u32;
        let (kind, format) = loop {
            let request = Request::new(GET_PROPERTY, u8::from(delete))
                .u32(window)
                .u32(property)
                .u32(0)
                .u32(offset)
                .u32(0x0010_0000)
                .finish(false);
            let reply = self.request_with_reply(&request)?;
            let format = reply.u8_at(1);
            let kind = reply.u32_at(8);
            let after = reply.u32_at(12);
            let units = reply.u32_at(16) as usize;
            let bytes = units * usize::from(format / 8).max(1);
            let end = (32 + bytes).min(reply.bytes.len());
            out.extend_from_slice(&reply.bytes[32..end]);
            if after == 0 || bytes == 0 {
                break (kind, format);
            }
            offset += (bytes / 4) as u32;
        };
        Ok((kind, format, out))
    }

    // --- Selections ------------------------------------------------------------

    pub(crate) fn set_selection_owner(
        &mut self,
        selection: u32,
        owner: u32,
    ) -> Result<(), Failure> {
        let request =
            Request::new(SET_SELECTION_OWNER, 0).u32(owner).u32(selection).u32(0).finish(false);
        self.request(&request)
    }

    pub(crate) fn selection_owner(&mut self, selection: u32) -> Result<u32, Failure> {
        let reply = self.request_with_reply(
            &Request::new(GET_SELECTION_OWNER, 0).u32(selection).finish(false),
        )?;
        Ok(reply.u32_at(8))
    }

    /// Asks a selection's owner for it in a format, as of a moment — nought
    /// for now; a drop names the moment it was let go.
    pub(crate) fn convert_selection(
        &mut self,
        requestor: u32,
        selection: u32,
        target: u32,
        property: u32,
        time: u32,
    ) -> Result<(), Failure> {
        let request = Request::new(CONVERT_SELECTION, 0)
            .u32(requestor)
            .u32(selection)
            .u32(target)
            .u32(property)
            .u32(time)
            .finish(false);
        self.request(&request)
    }

    // --- The pointer ---------------------------------------------------------------

    /// Takes the pointer for a window: its motion and its buttons come
    /// there, wherever it is on the screen, until let go. Whether the
    /// server gave it.
    pub(crate) fn grab_pointer(
        &mut self,
        window: u32,
        mask: u16,
        cursor: u32,
    ) -> Result<bool, Failure> {
        let request = Request::new(GRAB_POINTER, 0)
            .u32(window)
            .u16(mask)
            // Neither the pointer nor the keyboard frozen.
            .u8(1)
            .u8(1)
            .u32(0)
            .u32(cursor)
            .u32(0)
            .finish(false);
        let reply = self.request_with_reply(&request)?;
        Ok(reply.u8_at(1) == 0)
    }

    pub(crate) fn ungrab_pointer(&mut self) -> Result<(), Failure> {
        self.request(&Request::new(UNGRAB_POINTER, 0).u32(0).finish(false))
    }

    /// The child of a window that a point on the screen is in, if any.
    pub(crate) fn child_at(&mut self, window: u32, x: i16, y: i16) -> Result<u32, Failure> {
        let request = Request::new(TRANSLATE_COORDINATES, 0)
            .u32(self.setup.root)
            .u32(window)
            .i16(x)
            .i16(y)
            .finish(false);
        let reply = self.request_with_reply(&request)?;
        Ok(reply.u32_at(8))
    }

    /// Sends a thirty-two byte event to a window.
    pub(crate) fn send_event(
        &mut self,
        destination: u32,
        mask: u32,
        event: &[u8; 32],
    ) -> Result<(), Failure> {
        let mut request = Request::new(SEND_EVENT, 0).u32(destination).u32(mask);
        request.bytes.extend_from_slice(event);
        self.request(&request.finish(false))
    }

    /// A client message of five thirty-two bit words, as the window manager
    /// takes them.
    pub(crate) fn client_message(&mut self, window: u32, kind: u32, data: [u32; 5]) -> [u8; 32] {
        let mut event = [0u8; 32];
        event[0] = CLIENT_MESSAGE;
        event[1] = 32;
        event[4..8].copy_from_slice(&window.to_le_bytes());
        event[8..12].copy_from_slice(&kind.to_le_bytes());
        for (index, word) in data.iter().enumerate() {
            event[12 + index * 4..16 + index * 4].copy_from_slice(&word.to_le_bytes());
        }
        event
    }

    // --- Pictures ----------------------------------------------------------------

    /// Puts pixels on a window: rows of the root depth's format, in bands no
    /// longer than a request may be.
    pub(crate) fn put_image(
        &mut self,
        drawable: u32,
        gc: u32,
        width: u32,
        height: u32,
        rows: &[u8],
    ) -> Result<(), Failure> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let bytes_per_row = rows.len() / height as usize;
        let most_bytes = (self.setup.max_request_length as usize * 4).saturating_sub(64);
        let rows_per_band = (most_bytes / bytes_per_row.max(1)).clamp(1, height as usize);
        let mut top = 0usize;
        while top < height as usize {
            let count = rows_per_band.min(height as usize - top);
            let band = &rows[top * bytes_per_row..(top + count) * bytes_per_row];
            let request = Request::new(PUT_IMAGE, 2)
                .u32(drawable)
                .u32(gc)
                .u16(width as u16)
                .u16(count as u16)
                .i16(0)
                .i16(top as i16)
                .u8(0)
                .u8(self.setup.root_depth)
                .pad(2)
                .bytes(band)
                .finish(self.big_requests);
            self.request(&request)?;
            top += count;
        }
        self.flush()
    }

    /// The pixels of a drawable, as the server keeps them.
    pub(crate) fn get_image(
        &mut self,
        drawable: u32,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    ) -> Result<Vec<u8>, Failure> {
        let request = Request::new(GET_IMAGE, 2)
            .u32(drawable)
            .i16(x)
            .i16(y)
            .u16(width)
            .u16(height)
            .u32(0xFFFF_FFFF)
            .finish(false);
        let reply = self.request_with_reply(&request)?;
        Ok(reply.bytes[32..].to_vec())
    }

    // --- The keyboard -------------------------------------------------------------

    /// The keysyms of every keycode, in rows of the same width.
    pub(crate) fn keyboard_mapping(&mut self) -> Result<(usize, Vec<u32>), Failure> {
        let first = self.setup.min_keycode;
        let count = self.setup.max_keycode.saturating_sub(first).saturating_add(1);
        let request =
            Request::new(GET_KEYBOARD_MAPPING, 0).u8(first).u8(count).pad(2).finish(false);
        let reply = self.request_with_reply(&request)?;
        let per_keycode = usize::from(reply.u8_at(1));
        let keysyms: Vec<u32> = reply.bytes[32..]
            .chunks_exact(4)
            .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
            .collect();
        Ok((per_keycode, keysyms))
    }
}

/// The host and display number of a `DISPLAY` value such as `:0`, `:1.0`
/// or `unix:0`.
fn parse_display(display: &str) -> Option<(String, u32)> {
    let (host, rest) = display.rsplit_once(':')?;
    let number = rest.split('.').next()?.parse().ok()?;
    Some((host.to_owned(), number))
}

/// The cookie the server wants for this display, from the authority file.
fn authority_cookie(display: u32) -> Option<Vec<u8>> {
    let path = std::env::var("XAUTHORITY")
        .map(std::path::PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME").map(|home| std::path::Path::new(&home).join(".Xauthority"))
        })
        .ok()?;
    let bytes = std::fs::read(path).ok()?;
    let mut at = 0usize;
    let wanted = display.to_string();
    loop {
        if at + 2 > bytes.len() {
            return None;
        }
        at += 2;
        let _address = authority_field(&bytes, &mut at)?;
        let number = authority_field(&bytes, &mut at)?;
        let name = authority_field(&bytes, &mut at)?;
        let data = authority_field(&bytes, &mut at)?;
        if (number.is_empty() || number == wanted.as_bytes()) && name == b"MIT-MAGIC-COOKIE-1" {
            return Some(data);
        }
    }
}

/// One counted field of an authority file: a big-endian length, then that
/// many bytes.
fn authority_field(bytes: &[u8], at: &mut usize) -> Option<Vec<u8>> {
    let length = usize::from(u16::from_be_bytes([*bytes.get(*at)?, *bytes.get(*at + 1)?]));
    *at += 2;
    let value = bytes.get(*at..*at + length)?.to_vec();
    *at += length;
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_display_name_gives_its_number() {
        assert_eq!(parse_display(":0"), Some((String::new(), 0)));
        assert_eq!(parse_display(":99.0"), Some((String::new(), 99)));
        assert_eq!(parse_display("unix:1"), Some(("unix".to_owned(), 1)));
        assert_eq!(parse_display("nonsense"), None);
    }

    #[test]
    fn a_request_is_laid_out_in_four_byte_units_with_its_length_in_front() {
        let request = Request::new(INTERN_ATOM, 0).u16(3).pad(2).bytes(b"ATM").finish(false);
        assert_eq!(request, vec![16, 0, 3, 0, 3, 0, 0, 0, b'A', b'T', b'M', 0]);
    }

    #[test]
    fn a_long_request_takes_the_long_length() {
        let request = Request::new(PUT_IMAGE, 2).bytes(&vec![0u8; 300_000]).finish(true);
        assert_eq!(request[2..4], [0, 0]);
        let units = u32::from_le_bytes([request[4], request[5], request[6], request[7]]);
        assert_eq!(units as usize * 4, request.len());
    }
}

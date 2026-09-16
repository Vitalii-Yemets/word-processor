//! The Wayland wire protocol: objects, messages, and the socket they cross.
//!
//! # What the protocol is
//!
//! A stream of messages over a Unix socket. Each is an object identifier,
//! an opcode and a length, then the arguments: integers, strings, arrays,
//! new object identifiers, and file descriptors — which do not go in the
//! message at all but alongside it, as ancillary data. Everything is in
//! the machine's own byte order and padded to four bytes. That is the
//! whole of it; there is no handshake and no versioning on the wire, only
//! an agreement about what the numbers mean, which is what
//! [`super::protocol`] holds.
//!
//! # Why the two calls against the C library
//!
//! A file descriptor cannot be written into a stream; it is passed with
//! `sendmsg` and received with `recvmsg` as a control message, and the
//! standard library has no way to say that yet. Those two calls, and the
//! four that make and map a shared file, are the whole of what this
//! program declares against the C library — the same rule the Windows
//! shell follows with Win32: the operating system's own interface,
//! declared here rather than taken from somebody else's crate.

use std::ffi::c_void;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// What went wrong, in words a person could be shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// There is no compositor to talk to.
    NoDisplay,
    Socket(String),
    /// The compositor said the program did something it should not have.
    Protocol(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDisplay => write!(out, "no Wayland compositor to connect to"),
            Self::Socket(why) => write!(out, "the compositor's socket: {why}"),
            Self::Protocol(why) => write!(out, "the Wayland protocol: {why}"),
        }
    }
}

// --- The calls the standard library does not make ---------------------------

#[repr(C)]
struct IoVec {
    base: *mut c_void,
    length: usize,
}

#[repr(C)]
struct MsgHdr {
    name: *mut c_void,
    name_length: u32,
    _pad: u32,
    iov: *mut IoVec,
    iov_length: usize,
    control: *mut c_void,
    control_length: usize,
    flags: i32,
    _pad2: u32,
}

#[repr(C)]
struct CmsgHdr {
    length: usize,
    level: i32,
    kind: i32,
}

extern "C" {
    fn sendmsg(socket: i32, message: *const MsgHdr, flags: i32) -> isize;
    fn recvmsg(socket: i32, message: *mut MsgHdr, flags: i32) -> isize;
}

const SOL_SOCKET: i32 = 1;
const SCM_RIGHTS: i32 = 1;
/// `MSG_DONTWAIT | MSG_CMSG_CLOEXEC`: never block in the reader, and do not
/// leak a received descriptor into a program this one starts.
const RECEIVE_FLAGS: i32 = 0x40 | 0x4000_0000;
const MSG_NOSIGNAL: i32 = 0x4000;

/// How many descriptors can arrive with one read. Four is more than any
/// message this program takes carries; the keymap and a paste are one each.
const MOST_FDS: usize = 4;

/// The alignment a control message is written at, which is the machine's
/// own word and not the protocol's four bytes.
fn align(length: usize) -> usize {
    (length + core::mem::size_of::<usize>() - 1) & !(core::mem::size_of::<usize>() - 1)
}

// --- Messages ---------------------------------------------------------------

/// One message, as it stands on the wire: which object, which opcode, and
/// the arguments as bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Message {
    pub(crate) object: u32,
    pub(crate) opcode: u16,
    pub(crate) body: Vec<u8>,
}

impl Message {
    /// A reader over the arguments.
    pub(crate) fn arguments(&self) -> Arguments<'_> {
        Arguments { bytes: &self.body, at: 0 }
    }
}

/// The arguments of a message, read in the order the interface says they
/// are in. A message read wrongly gives zeroes rather than panicking: the
/// compositor is another program, and a shell that fell over on a message
/// it did not expect would be a shell that fell over.
pub(crate) struct Arguments<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Arguments<'_> {
    pub(crate) fn uint(&mut self) -> u32 {
        let Some(four) = self.bytes.get(self.at..self.at + 4) else { return 0 };
        self.at += 4;
        u32::from_ne_bytes([four[0], four[1], four[2], four[3]])
    }

    pub(crate) fn int(&mut self) -> i32 {
        self.uint() as i32
    }

    /// A fixed-point number: twenty-four bits of whole and eight of
    /// fraction, which is how Wayland says where the pointer is.
    pub(crate) fn fixed(&mut self) -> f32 {
        self.int() as f32 / 256.0
    }

    pub(crate) fn string(&mut self) -> String {
        let bytes = self.array();
        // The length counts the terminator the string is written with.
        let text = bytes.strip_suffix(&[0]).unwrap_or(&bytes);
        String::from_utf8_lossy(text).into_owned()
    }

    pub(crate) fn array(&mut self) -> Vec<u8> {
        let length = self.uint() as usize;
        let Some(bytes) = self.bytes.get(self.at..self.at + length) else { return Vec::new() };
        let bytes = bytes.to_vec();
        // The protocol pads every argument out to four bytes.
        self.at += (length + 3) & !3;
        bytes
    }
}

/// A message being built, argument by argument.
#[derive(Debug)]
pub(crate) struct Request {
    object: u32,
    opcode: u16,
    body: Vec<u8>,
    /// A descriptor to send alongside it, where the request carries one.
    fd: Option<RawFd>,
}

impl Request {
    pub(crate) fn new(object: u32, opcode: u16) -> Self {
        Self { object, opcode, body: Vec::new(), fd: None }
    }

    #[must_use]
    pub(crate) fn uint(mut self, value: u32) -> Self {
        self.body.extend_from_slice(&value.to_ne_bytes());
        self
    }

    #[must_use]
    pub(crate) fn int(self, value: i32) -> Self {
        self.uint(value as u32)
    }

    /// A string, written with its terminator and padded out to four bytes,
    /// which is what the protocol means by a string.
    #[must_use]
    pub(crate) fn string(mut self, text: &str) -> Self {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        let length = bytes.len();
        self.body.extend_from_slice(&(length as u32).to_ne_bytes());
        self.body.extend_from_slice(&bytes);
        while self.body.len() % 4 != 0 {
            self.body.push(0);
        }
        let _ = length;
        self
    }

    /// The descriptor this request hands over.
    #[must_use]
    pub(crate) fn with_fd(mut self, fd: RawFd) -> Self {
        self.fd = Some(fd);
        self
    }

    /// The bytes of the message: the object, the opcode and the length in
    /// the same word, then the arguments.
    pub(crate) fn bytes(&self) -> Vec<u8> {
        let length = 8 + self.body.len();
        let mut out = Vec::with_capacity(length);
        out.extend_from_slice(&self.object.to_ne_bytes());
        out.extend_from_slice(&(((length as u32) << 16) | u32::from(self.opcode)).to_ne_bytes());
        out.extend_from_slice(&self.body);
        out
    }
}

// --- The connection ---------------------------------------------------------

/// The socket to the compositor, and the objects made on it.
pub(crate) struct Connection {
    stream: UnixStream,
    /// Bytes read and not yet made into messages.
    inbox: Vec<u8>,
    /// Messages read while waiting for an answer to something else.
    pending: Vec<Message>,
    /// Descriptors that arrived and have not been claimed by a message.
    fds: Vec<OwnedFd>,
    /// The next identifier to give a new object. The compositor owns the
    /// numbers above `0xff00_0000`; everything below is the program's.
    next_id: u32,
}

impl Connection {
    /// Opens the socket the environment names.
    ///
    /// `WAYLAND_DISPLAY` is the socket's name, in `XDG_RUNTIME_DIR`, or an
    /// absolute path of its own. Without it there is no compositor, which
    /// is not a failure: it means this is an X desktop.
    pub(crate) fn open() -> Result<Self, Failure> {
        let path = socket_path().ok_or(Failure::NoDisplay)?;
        let stream = UnixStream::connect(&path)
            .map_err(|error| Failure::Socket(format!("{}: {error}", path.display())))?;
        stream.set_nonblocking(true).map_err(|error| Failure::Socket(error.to_string()))?;
        Ok(Self { stream, inbox: Vec::new(), pending: Vec::new(), fds: Vec::new(), next_id: 2 })
    }

    /// The next identifier for an object this program makes.
    pub(crate) fn make_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Sends a request, with its descriptor if it has one.
    pub(crate) fn send(&mut self, request: &Request) -> Result<(), Failure> {
        let bytes = request.bytes();
        let mut iov = IoVec { base: bytes.as_ptr() as *mut c_void, length: bytes.len() };
        let mut control = [0u8; 32];
        let (control_pointer, control_length) = match request.fd {
            Some(fd) => {
                let payload = core::mem::size_of::<RawFd>();
                let header_size = core::mem::size_of::<CmsgHdr>();
                let header =
                    CmsgHdr { length: header_size + payload, level: SOL_SOCKET, kind: SCM_RIGHTS };
                // SAFETY: the buffer is larger than one header and one
                // descriptor, which is all that is written into it.
                unsafe {
                    core::ptr::write_unaligned(control.as_mut_ptr().cast::<CmsgHdr>(), header);
                    core::ptr::write_unaligned(
                        control.as_mut_ptr().add(header_size).cast::<RawFd>(),
                        fd,
                    );
                }
                (control.as_mut_ptr().cast::<c_void>(), align(header_size + payload))
            }
            None => (core::ptr::null_mut(), 0),
        };
        let message = MsgHdr {
            name: core::ptr::null_mut(),
            name_length: 0,
            _pad: 0,
            iov: &mut iov,
            iov_length: 1,
            control: control_pointer,
            control_length,
            flags: 0,
            _pad2: 0,
        };
        // SAFETY: every pointer in the message points at something alive
        // until the call returns.
        let sent = unsafe { sendmsg(self.stream.as_raw_fd(), &message, MSG_NOSIGNAL) };
        if sent < 0 {
            return Err(Failure::Socket(io::Error::last_os_error().to_string()));
        }
        Ok(())
    }

    /// Reads whatever the compositor has sent, without waiting.
    ///
    /// Whether anything arrived. The messages are left in the inbox for
    /// [`Self::next_message`].
    fn fill(&mut self) -> Result<bool, Failure> {
        let mut bytes = [0u8; 4096];
        let mut iov = IoVec { base: bytes.as_mut_ptr().cast::<c_void>(), length: bytes.len() };
        let mut control = [0u8; 256];
        let mut message = MsgHdr {
            name: core::ptr::null_mut(),
            name_length: 0,
            _pad: 0,
            iov: &mut iov,
            iov_length: 1,
            control: control.as_mut_ptr().cast::<c_void>(),
            control_length: control.len(),
            flags: 0,
            _pad2: 0,
        };
        // SAFETY: as above; the buffers outlive the call and their lengths
        // are the lengths passed.
        let read = unsafe { recvmsg(self.stream.as_raw_fd(), &mut message, RECEIVE_FLAGS) };
        if read < 0 {
            let error = io::Error::last_os_error();
            return match error.kind() {
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => Ok(false),
                _ => Err(Failure::Socket(error.to_string())),
            };
        }
        if read == 0 {
            return Err(Failure::Socket("the compositor closed the connection".to_owned()));
        }
        self.inbox.extend_from_slice(&bytes[..read as usize]);
        self.take_fds(&control, message.control_length);
        Ok(true)
    }

    /// The descriptors that came with a read, taken into this program.
    fn take_fds(&mut self, control: &[u8], length: usize) {
        let header_size = core::mem::size_of::<CmsgHdr>();
        let mut at = 0usize;
        while at + header_size <= length.min(control.len()) {
            // SAFETY: the bytes were written by the kernel as a control
            // message and are read back at the alignment they were written.
            let header: CmsgHdr =
                unsafe { core::ptr::read_unaligned(control.as_ptr().add(at).cast::<CmsgHdr>()) };
            if header.length < header_size || at + header.length > control.len() {
                break;
            }
            if header.level == SOL_SOCKET && header.kind == SCM_RIGHTS {
                let payload = header.length - header_size;
                let count = (payload / core::mem::size_of::<RawFd>()).min(MOST_FDS);
                for index in 0..count {
                    // SAFETY: the kernel wrote `count` descriptors here,
                    // and each is taken over exactly once.
                    let fd: RawFd = unsafe {
                        core::ptr::read_unaligned(
                            control.as_ptr().add(at + header_size).cast::<RawFd>().add(index),
                        )
                    };
                    // SAFETY: the descriptor is ours now; nothing else in
                    // this program holds it.
                    self.fds.push(unsafe { <OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(fd) });
                }
            }
            at += align(header.length);
        }
    }

    /// The next message, reading from the socket if the inbox is empty.
    ///
    /// Nothing when nothing has arrived by the time the wait runs out.
    pub(crate) fn next_message(&mut self, timeout: Duration) -> Result<Option<Message>, Failure> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(message) = self.take_message() {
                return Ok(Some(message));
            }
            if !self.fill()? {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                // The socket is not ready. Waiting a moment costs a
                // thousandth of a second and keeps the loop from spinning
                // the processor for nothing.
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
        }
    }

    /// A message out of the inbox, if a whole one is there.
    fn take_message(&mut self) -> Option<Message> {
        if self.inbox.len() < 8 {
            return None;
        }
        let object =
            u32::from_ne_bytes([self.inbox[0], self.inbox[1], self.inbox[2], self.inbox[3]]);
        let second =
            u32::from_ne_bytes([self.inbox[4], self.inbox[5], self.inbox[6], self.inbox[7]]);
        let length = (second >> 16) as usize;
        let opcode = (second & 0xFFFF) as u16;
        if length < 8 || self.inbox.len() < length {
            return None;
        }
        let body = self.inbox[8..length].to_vec();
        self.inbox.drain(..length);
        Some(Message { object, opcode, body })
    }

    /// The descriptor that came with the message being handled.
    pub(crate) fn take_fd(&mut self) -> Option<OwnedFd> {
        if self.fds.is_empty() {
            return None;
        }
        Some(self.fds.remove(0))
    }

    /// Sends everything and waits for the compositor to answer whatever was
    /// asked before this moment.
    ///
    /// The protocol has no reply to a request; this is how a client asks
    /// "are you done with what I have sent?" — the compositor answers the
    /// `sync` with `done`, and everything before it has been dealt with by
    /// then. Every setting-up step waits on one.
    pub(crate) fn round_trip(&mut self, display: u32) -> Result<(), Failure> {
        let callback = self.make_id();
        self.send(&Request::new(display, 0).uint(callback))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if Instant::now() > deadline {
                return Err(Failure::Protocol("the compositor did not answer".to_owned()));
            }
            let Some(message) = self.next_message(Duration::from_millis(50))? else { continue };
            if message.object == callback {
                return Ok(());
            }
            self.pending.push(message);
        }
    }

    /// Messages read while waiting for something else, to be dealt with
    /// once the waiting is over.
    pub(crate) fn take_pending(&mut self) -> Vec<Message> {
        core::mem::take(&mut self.pending)
    }
}

/// Where the compositor's socket is.
fn socket_path() -> Option<PathBuf> {
    let display = std::env::var("WAYLAND_DISPLAY").ok().filter(|name| !name.is_empty())?;
    let path = PathBuf::from(&display);
    if path.is_absolute() {
        return Some(path);
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok().filter(|dir| !dir.is_empty())?;
    Some(PathBuf::from(runtime).join(display))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_the_object_the_length_and_the_opcode_then_the_arguments() {
        let request = Request::new(3, 5).uint(7).int(-2);
        let bytes = request.bytes();
        assert_eq!(bytes.len(), 16);
        assert_eq!(u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]), 3);
        let second = u32::from_ne_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        assert_eq!(second & 0xFFFF, 5, "the opcode");
        assert_eq!(second >> 16, 16, "the length, counting the header");
        assert_eq!(u32::from_ne_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]), 7);
        assert_eq!(u32::from_ne_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as i32, -2);
    }

    #[test]
    fn a_string_carries_its_terminator_and_is_padded_to_four_bytes() {
        let bytes = Request::new(1, 0).string("abc").bytes();
        // Four bytes of length, then "abc\0" — which is already a multiple
        // of four, so nothing is added.
        assert_eq!(bytes.len(), 8 + 4 + 4);
        assert_eq!(u32::from_ne_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]), 4);
        assert_eq!(&bytes[12..16], b"abc\0");

        // And one that is not: "ab\0" is three, padded to four.
        let bytes = Request::new(1, 0).string("ab").bytes();
        assert_eq!(bytes.len(), 8 + 4 + 4);
        assert_eq!(u32::from_ne_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]), 3);
        assert_eq!(&bytes[12..16], b"ab\0\0");
    }

    #[test]
    fn the_arguments_come_back_as_they_were_written() {
        let request = Request::new(1, 0).uint(42).string("hello");
        let message = Message { object: 1, opcode: 0, body: request.body.clone() };
        let mut arguments = message.arguments();
        assert_eq!(arguments.uint(), 42);
        assert_eq!(arguments.string(), "hello");
        assert_eq!(arguments.uint(), 0, "past the end is nothing rather than a panic");
    }

    #[test]
    fn what_the_compositor_sends_is_read_the_way_it_writes_it() {
        // A fixed-point number and an array, as they stand on the wire:
        // the compositor sends both and this program never writes one.
        let mut body = Vec::new();
        body.extend_from_slice(&(384i32).to_ne_bytes()); // 1.5, in 256ths
        body.extend_from_slice(&(3u32).to_ne_bytes());
        body.extend_from_slice(&[1, 2, 3, 0]); // three bytes, padded to four
        let message = Message { object: 1, opcode: 0, body };
        let mut arguments = message.arguments();
        assert!((arguments.fixed() - 1.5).abs() < 0.01);
        assert_eq!(arguments.array(), vec![1, 2, 3]);
    }

    #[test]
    fn a_display_name_says_where_the_socket_is() {
        // The environment belongs to the whole program, so this test puts
        // back what it found.
        let (was_display, was_runtime) =
            (std::env::var("WAYLAND_DISPLAY").ok(), std::env::var("XDG_RUNTIME_DIR").ok());
        std::env::set_var("WAYLAND_DISPLAY", "wayland-1");
        std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
        assert_eq!(socket_path(), Some(PathBuf::from("/run/user/1000/wayland-1")));
        std::env::set_var("WAYLAND_DISPLAY", "/tmp/somewhere/wayland-9");
        assert_eq!(socket_path(), Some(PathBuf::from("/tmp/somewhere/wayland-9")));
        std::env::remove_var("WAYLAND_DISPLAY");
        assert_eq!(socket_path(), None, "no display is no compositor");
        match was_display {
            Some(value) => std::env::set_var("WAYLAND_DISPLAY", value),
            None => std::env::remove_var("WAYLAND_DISPLAY"),
        }
        match was_runtime {
            Some(value) => std::env::set_var("XDG_RUNTIME_DIR", value),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }
    }
}

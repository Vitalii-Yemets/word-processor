//! D-Bus, spoken over its socket.
//!
//! # What it is
//!
//! The message bus the programs of a Linux desktop talk to each other on: a
//! daemon at the end of a socket, and messages — calls, replies, errors and
//! signals — addressed by a bus name, an object path, an interface and a
//! member, with their arguments in a binary format of the protocol's own,
//! typed by a signature. What a screen reader is told of a program goes
//! over one, the accessibility bus, and so does what a program asks of the
//! desktop's portal.
//!
//! # What is here
//!
//! The wire format both ways, for every type the protocol has but the file
//! descriptor; the authentication a local socket asks for — EXTERNAL, the
//! user the socket already knows this program runs as; and a connection
//! that sends, waits for a reply while keeping what else arrives meanwhile,
//! and hands over whatever has arrived without waiting. Written out against
//! the D-Bus specification; no library.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

/// A value of any of the protocol's types.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Byte(u8),
    Bool(bool),
    Int16(i16),
    Uint16(u16),
    Int32(i32),
    Uint32(u32),
    Int64(i64),
    Uint64(u64),
    Double(f64),
    Str(String),
    Path(String),
    Signature(String),
    /// The signature of the elements, and the elements — which an empty
    /// array needs, having none to say it.
    Array(String, Vec<Value>),
    Struct(Vec<Value>),
    Variant(Box<Value>),
    DictEntry(Box<Value>, Box<Value>),
}

impl Value {
    /// The value's type, as a signature writes it.
    pub(crate) fn signature(&self) -> String {
        match self {
            Self::Byte(_) => "y".to_owned(),
            Self::Bool(_) => "b".to_owned(),
            Self::Int16(_) => "n".to_owned(),
            Self::Uint16(_) => "q".to_owned(),
            Self::Int32(_) => "i".to_owned(),
            Self::Uint32(_) => "u".to_owned(),
            Self::Int64(_) => "x".to_owned(),
            Self::Uint64(_) => "t".to_owned(),
            Self::Double(_) => "d".to_owned(),
            Self::Str(_) => "s".to_owned(),
            Self::Path(_) => "o".to_owned(),
            Self::Signature(_) => "g".to_owned(),
            Self::Array(element, _) => format!("a{element}"),
            Self::Struct(fields) => {
                format!("({})", fields.iter().map(Self::signature).collect::<String>())
            }
            Self::Variant(_) => "v".to_owned(),
            Self::DictEntry(key, value) => format!("{{{}{}}}", key.signature(), value.signature()),
        }
    }

    pub(crate) fn str(s: &str) -> Self {
        Self::Str(s.to_owned())
    }

    pub(crate) fn path(s: &str) -> Self {
        Self::Path(s.to_owned())
    }

    /// A string, a path or a signature, as text.
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) | Self::Path(text) | Self::Signature(text) => Some(text),
            _ => None,
        }
    }

    /// Any of the integer types, as a wide one.
    pub(crate) fn as_i64(&self) -> Option<i64> {
        match *self {
            Self::Byte(value) => Some(i64::from(value)),
            Self::Int16(value) => Some(i64::from(value)),
            Self::Uint16(value) => Some(i64::from(value)),
            Self::Int32(value) => Some(i64::from(value)),
            Self::Uint32(value) => Some(i64::from(value)),
            Self::Int64(value) => Some(value),
            Self::Uint64(value) => i64::try_from(value).ok(),
            Self::Bool(value) => Some(i64::from(value)),
            _ => None,
        }
    }

    /// A variant's value, or the value itself.
    pub(crate) fn inner(&self) -> &Self {
        match self {
            Self::Variant(inner) => inner.inner(),
            other => other,
        }
    }

    /// A struct's fields or an array's elements.
    pub(crate) fn items(&self) -> &[Self] {
        match self {
            Self::Struct(items) | Self::Array(_, items) => items,
            _ => &[],
        }
    }
}

/// How a type is aligned, by the first character of its signature.
fn alignment(code: u8) -> usize {
    match code {
        b'n' | b'q' => 2,
        b'b' | b'i' | b'u' | b's' | b'o' | b'a' | b'h' => 4,
        b'x' | b't' | b'd' | b'(' | b'{' => 8,
        _ => 1,
    }
}

/// How long the first complete type of a signature is.
fn single_type_length(signature: &[u8]) -> Option<usize> {
    match *signature.first()? {
        b'a' => Some(1 + single_type_length(&signature[1..])?),
        open @ (b'(' | b'{') => {
            let close = if open == b'(' { b')' } else { b'}' };
            let mut at = 1;
            while *signature.get(at)? != close {
                at += single_type_length(&signature[at..])?;
            }
            Some(at + 1)
        }
        _ => Some(1),
    }
}

/// A signature, as the complete types it is made of.
pub(crate) fn split_signature(signature: &str) -> Option<Vec<&str>> {
    let bytes = signature.as_bytes();
    let mut types = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let length = single_type_length(&bytes[at..])?;
        types.push(&signature[at..at + length]);
        at += length;
    }
    Some(types)
}

/// Writes values, least significant byte first, aligned from the start.
struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn pad(&mut self, alignment: usize) {
        while self.bytes.len() % alignment != 0 {
            self.bytes.push(0);
        }
    }

    fn u32(&mut self, value: u32) {
        self.pad(4);
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn value(&mut self, value: &Value) {
        match value {
            Value::Byte(byte) => self.bytes.push(*byte),
            Value::Bool(truth) => self.u32(u32::from(*truth)),
            Value::Int16(number) => {
                self.pad(2);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::Uint16(number) => {
                self.pad(2);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::Int32(number) => {
                self.pad(4);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::Uint32(number) => self.u32(*number),
            Value::Int64(number) => {
                self.pad(8);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::Uint64(number) => {
                self.pad(8);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::Double(number) => {
                self.pad(8);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::Str(text) | Value::Path(text) => {
                self.u32(text.len() as u32);
                self.bytes.extend_from_slice(text.as_bytes());
                self.bytes.push(0);
            }
            Value::Signature(text) => {
                self.bytes.push(text.len() as u8);
                self.bytes.extend_from_slice(text.as_bytes());
                self.bytes.push(0);
            }
            Value::Array(element, items) => {
                self.u32(0);
                let length_at = self.bytes.len() - 4;
                // The length counts the elements and not the padding before
                // the first of them.
                self.pad(alignment(element.as_bytes().first().copied().unwrap_or(b'y')));
                let start = self.bytes.len();
                for item in items {
                    self.value(item);
                }
                let length = (self.bytes.len() - start) as u32;
                self.bytes[length_at..length_at + 4].copy_from_slice(&length.to_le_bytes());
            }
            Value::Struct(fields) => {
                self.pad(8);
                for field in fields {
                    self.value(field);
                }
            }
            Value::Variant(inner) => {
                self.value(&Value::Signature(inner.signature()));
                self.value(inner);
            }
            Value::DictEntry(key, item) => {
                self.pad(8);
                self.value(key);
                self.value(item);
            }
        }
    }
}

/// Reads values, in the byte order the message says, aligned from where
/// the reader starts — which is a place aligned to eight in the message.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    big: bool,
}

impl<'a> Reader<'a> {
    fn align(&mut self, alignment: usize) {
        self.at = self.at.div_ceil(alignment) * alignment;
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.at..self.at.checked_add(count)?)?;
        self.at += count;
        Some(slice)
    }

    fn fixed<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.align(N);
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        if self.big {
            out.reverse();
        }
        Some(out)
    }

    fn u32(&mut self) -> Option<u32> {
        self.fixed::<4>().map(u32::from_le_bytes)
    }

    fn value(&mut self, signature: &str) -> Option<Value> {
        let code = *signature.as_bytes().first()?;
        Some(match code {
            b'y' => Value::Byte(self.take(1)?[0]),
            b'b' => Value::Bool(self.u32()? != 0),
            b'n' => Value::Int16(i16::from_le_bytes(self.fixed::<2>()?)),
            b'q' => Value::Uint16(u16::from_le_bytes(self.fixed::<2>()?)),
            b'i' => Value::Int32(i32::from_le_bytes(self.fixed::<4>()?)),
            b'u' | b'h' => Value::Uint32(self.u32()?),
            b'x' => Value::Int64(i64::from_le_bytes(self.fixed::<8>()?)),
            b't' => Value::Uint64(u64::from_le_bytes(self.fixed::<8>()?)),
            b'd' => Value::Double(f64::from_le_bytes(self.fixed::<8>()?)),
            b's' | b'o' => {
                let length = self.u32()? as usize;
                let text = String::from_utf8_lossy(self.take(length)?).into_owned();
                self.take(1)?;
                if code == b's' {
                    Value::Str(text)
                } else {
                    Value::Path(text)
                }
            }
            b'g' => {
                let length = usize::from(self.take(1)?[0]);
                let text = String::from_utf8_lossy(self.take(length)?).into_owned();
                self.take(1)?;
                Value::Signature(text)
            }
            b'a' => {
                let length = self.u32()? as usize;
                let element_length = single_type_length(&signature.as_bytes()[1..])?;
                let element = &signature[1..1 + element_length];
                self.align(alignment(element.as_bytes()[0]));
                let end = self.at.checked_add(length)?;
                if end > self.bytes.len() {
                    return None;
                }
                let mut items = Vec::new();
                while self.at < end {
                    items.push(self.value(element)?);
                }
                Value::Array(element.to_owned(), items)
            }
            b'(' => {
                self.align(8);
                let inner = &signature[1..single_type_length(signature.as_bytes())? - 1];
                let mut fields = Vec::new();
                for field in split_signature(inner)? {
                    fields.push(self.value(field)?);
                }
                Value::Struct(fields)
            }
            b'{' => {
                self.align(8);
                let inner = &signature[1..single_type_length(signature.as_bytes())? - 1];
                let parts = split_signature(inner)?;
                let key = self.value(parts.first()?)?;
                let item = self.value(parts.get(1)?)?;
                Value::DictEntry(Box::new(key), Box::new(item))
            }
            b'v' => {
                let Value::Signature(inner) = self.value("g")? else { return None };
                Value::Variant(Box::new(self.value(&inner)?))
            }
            _ => return None,
        })
    }
}

/// What a message is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Kind {
    #[default]
    Call,
    Return,
    Error,
    Signal,
}

impl Kind {
    fn number(self) -> u8 {
        match self {
            Self::Call => 1,
            Self::Return => 2,
            Self::Error => 3,
            Self::Signal => 4,
        }
    }

    fn of(number: u8) -> Option<Self> {
        Some(match number {
            1 => Self::Call,
            2 => Self::Return,
            3 => Self::Error,
            4 => Self::Signal,
            _ => return None,
        })
    }
}

/// A message, and where it is going or came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Message {
    pub(crate) kind: Kind,
    /// Whether no reply is wanted.
    pub(crate) no_reply: bool,
    pub(crate) serial: u32,
    pub(crate) path: Option<String>,
    pub(crate) interface: Option<String>,
    pub(crate) member: Option<String>,
    pub(crate) error_name: Option<String>,
    pub(crate) reply_serial: Option<u32>,
    pub(crate) destination: Option<String>,
    pub(crate) sender: Option<String>,
    pub(crate) body: Vec<Value>,
}

impl Message {
    /// A method call.
    pub(crate) fn call(
        destination: &str,
        path: &str,
        interface: &str,
        member: &str,
        body: Vec<Value>,
    ) -> Self {
        Self {
            kind: Kind::Call,
            path: Some(path.to_owned()),
            interface: Some(interface.to_owned()),
            member: Some(member.to_owned()),
            destination: Some(destination.to_owned()),
            body,
            ..Self::default()
        }
    }

    /// A signal, to whoever listens for it.
    pub(crate) fn signal(path: &str, interface: &str, member: &str, body: Vec<Value>) -> Self {
        Self {
            kind: Kind::Signal,
            no_reply: true,
            path: Some(path.to_owned()),
            interface: Some(interface.to_owned()),
            member: Some(member.to_owned()),
            body,
            ..Self::default()
        }
    }

    /// The answer to a call.
    pub(crate) fn reply(&self, body: Vec<Value>) -> Self {
        Self {
            kind: Kind::Return,
            no_reply: true,
            reply_serial: Some(self.serial),
            destination: self.sender.clone(),
            body,
            ..Self::default()
        }
    }

    /// A call refused, with the protocol's name for why and words for it.
    pub(crate) fn error(&self, name: &str, why: &str) -> Self {
        Self {
            kind: Kind::Error,
            no_reply: true,
            error_name: Some(name.to_owned()),
            reply_serial: Some(self.serial),
            destination: self.sender.clone(),
            body: vec![Value::str(why)],
            ..Self::default()
        }
    }

    /// The signature of the body.
    pub(crate) fn signature(&self) -> String {
        self.body.iter().map(Value::signature).collect()
    }

    /// The message on the wire, with a serial of its own.
    pub(crate) fn encode(&self, serial: u32) -> Vec<u8> {
        let mut body = Writer { bytes: Vec::new() };
        for value in &self.body {
            body.value(value);
        }
        let mut fields = Vec::new();
        let mut field = |code: u8, value: Value| {
            fields.push(Value::Struct(vec![Value::Byte(code), Value::Variant(Box::new(value))]));
        };
        if let Some(path) = &self.path {
            field(1, Value::path(path));
        }
        if let Some(interface) = &self.interface {
            field(2, Value::str(interface));
        }
        if let Some(member) = &self.member {
            field(3, Value::str(member));
        }
        if let Some(name) = &self.error_name {
            field(4, Value::str(name));
        }
        if let Some(reply) = self.reply_serial {
            field(5, Value::Uint32(reply));
        }
        if let Some(destination) = &self.destination {
            field(6, Value::str(destination));
        }
        let signature = self.signature();
        if !signature.is_empty() {
            field(8, Value::Signature(signature));
        }
        let mut header = Writer { bytes: Vec::new() };
        header.bytes.extend_from_slice(&[b'l', self.kind.number(), u8::from(self.no_reply), 1]);
        header.u32(body.bytes.len() as u32);
        header.u32(serial);
        header.value(&Value::Array("(yv)".to_owned(), fields));
        header.pad(8);
        header.bytes.extend_from_slice(&body.bytes);
        header.bytes
    }

    /// A whole message off the front of what has arrived, and how many
    /// bytes it took; nothing while it has not all arrived. A message that
    /// cannot be read is skipped as a message of no kind.
    pub(crate) fn decode(bytes: &[u8]) -> Option<(Option<Self>, usize)> {
        if bytes.len() < 16 {
            return None;
        }
        let big = match bytes[0] {
            b'l' => false,
            b'B' => true,
            _ => return Some((None, bytes.len())),
        };
        let word = |at: usize| {
            let mut four = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
            if big {
                four.reverse();
            }
            u32::from_le_bytes(four) as usize
        };
        let body_length = word(4);
        let fields_length = word(12);
        let header_end = (16 + fields_length).div_ceil(8) * 8;
        let total = header_end + body_length;
        if bytes.len() < total {
            return None;
        }
        let mut reader = Reader { bytes: &bytes[..header_end], at: 12, big };
        let mut message = Self {
            kind: Kind::of(bytes[1]).unwrap_or_default(),
            no_reply: bytes[2] & 1 != 0,
            serial: word(8) as u32,
            ..Self::default()
        };
        let mut signature = String::new();
        let Some(fields) = reader.value("a(yv)") else { return Some((None, total)) };
        for field in fields.items() {
            let [code, value] = field.items() else { continue };
            let text = value.inner().as_str().map(str::to_owned);
            match code {
                Value::Byte(1) => message.path = text,
                Value::Byte(2) => message.interface = text,
                Value::Byte(3) => message.member = text,
                Value::Byte(4) => message.error_name = text,
                Value::Byte(5) => {
                    message.reply_serial = value.inner().as_i64().map(|serial| serial as u32);
                }
                Value::Byte(6) => message.destination = text,
                Value::Byte(7) => message.sender = text,
                Value::Byte(8) => signature = text.unwrap_or_default(),
                _ => {}
            }
        }
        let mut body = Reader { bytes: &bytes[header_end..total], at: 0, big };
        let Some(types) = split_signature(&signature) else { return Some((None, total)) };
        for kind in types {
            let Some(value) = body.value(kind) else { return Some((None, total)) };
            message.body.push(value);
        }
        Some((Some(message), total))
    }
}

/// Why a bus could not be reached.
#[derive(Debug)]
pub(crate) struct Failure(pub(crate) String);

/// What a call comes to when nothing has answered it in time.
const NO_ANSWER: &str = "no answer in time";

impl Failure {
    /// Whether nothing answered in time — which says nothing of what the
    /// answer would have been, where an error in answer does.
    pub(crate) fn is_no_answer(&self) -> bool {
        self.0 == NO_ANSWER
    }
}

impl core::fmt::Display for Failure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A connection to a bus.
#[derive(Debug)]
pub(crate) struct Connection {
    stream: UnixStream,
    serial: u32,
    inbox: Vec<u8>,
    /// Messages that arrived while a reply was waited for.
    pending: VecDeque<Message>,
    /// The name the bus gave this connection.
    pub(crate) name: String,
    /// Whether the bus has gone: a socket that has closed is always ready
    /// to be read, and waiting on it would be no wait at all.
    closed: bool,
}

/// The session bus's address: what the desktop put in the environment, or
/// the one a user session keeps in the runtime directory.
pub(crate) fn session_address() -> Option<String> {
    if let Ok(address) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        if !address.is_empty() {
            return Some(address);
        }
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let path = std::path::Path::new(&runtime).join("bus");
    path.exists().then(|| format!("unix:path={}", path.display()))
}

/// The socket an address names: the first of its entries that is a local
/// one, by path or by an abstract name.
fn socket_of(address: &str) -> Option<UnixStream> {
    for entry in address.split(';') {
        let Some(rest) = entry.strip_prefix("unix:") else { continue };
        for pair in rest.split(',') {
            let Some((key, value)) = pair.split_once('=') else { continue };
            let value = unescape_address(value);
            match key {
                "path" => {
                    if let Ok(stream) = UnixStream::connect(&value) {
                        return Some(stream);
                    }
                }
                "abstract" => {
                    use std::os::linux::net::SocketAddrExt;
                    let Ok(name) = std::os::unix::net::SocketAddr::from_abstract_name(&value)
                    else {
                        continue;
                    };
                    if let Ok(stream) = UnixStream::connect_addr(&name) {
                        return Some(stream);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// An address value's `%XX` escapes, undone.
fn unescape_address(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            let digit = |byte: u8| char::from(byte).to_digit(16);
            if let (Some(high), Some(low)) = (digit(bytes[at + 1]), digit(bytes[at + 2])) {
                out.push((high * 16 + low) as u8);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The user this program runs as, which the process's own directory in
/// `/proc` belongs to.
fn user_id() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map_or(0, |metadata| metadata.uid())
}

impl Connection {
    /// Connects to a bus, says who this is, and takes a name on it.
    pub(crate) fn open(address: &str) -> Result<Self, Failure> {
        let mut stream =
            socket_of(address).ok_or_else(|| Failure(format!("cannot reach {address}")))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        // Who this is: the user, which the bus can see for itself on a
        // local socket, written as the digits of its number in hex.
        let hex: String = user_id().to_string().bytes().map(|byte| format!("{byte:02x}")).collect();
        let hello = format!("\0AUTH EXTERNAL {hex}\r\n");
        stream.write_all(hello.as_bytes()).map_err(|error| Failure(error.to_string()))?;
        let mut answer = Vec::new();
        let mut byte = [0u8; 1];
        while !answer.ends_with(b"\r\n") {
            match stream.read(&mut byte) {
                Ok(1) => answer.push(byte[0]),
                _ => return Err(Failure("the bus did not answer".to_owned())),
            }
        }
        if !answer.starts_with(b"OK") {
            return Err(Failure(format!(
                "the bus refused: {}",
                String::from_utf8_lossy(&answer).trim()
            )));
        }
        stream.write_all(b"BEGIN\r\n").map_err(|error| Failure(error.to_string()))?;
        let mut connection = Self {
            stream,
            serial: 0,
            inbox: Vec::new(),
            pending: VecDeque::new(),
            name: String::new(),
            closed: false,
        };
        let reply = connection.call(
            &Message::call(
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "Hello",
                Vec::new(),
            ),
            Duration::from_secs(5),
        )?;
        connection.name = reply.body.first().and_then(Value::as_str).unwrap_or_default().to_owned();
        Ok(connection)
    }

    /// Sends a message. Its serial.
    pub(crate) fn send(&mut self, message: &Message) -> Result<u32, Failure> {
        self.serial = self.serial.wrapping_add(1).max(1);
        let bytes = message.encode(self.serial);
        self.stream.write_all(&bytes).map_err(|error| Failure(error.to_string()))?;
        Ok(self.serial)
    }

    /// Sends a call and waits for its answer; what else arrives meanwhile is
    /// kept for [`Self::poll`]. An error from the other end is a failure
    /// saying so.
    pub(crate) fn call(&mut self, message: &Message, wait: Duration) -> Result<Message, Failure> {
        let serial = self.send(message)?;
        let deadline = Instant::now() + wait;
        loop {
            while let Some((decoded, used)) = Message::decode(&self.inbox) {
                self.inbox.drain(..used);
                let Some(decoded) = decoded else { continue };
                if decoded.reply_serial == Some(serial) {
                    if decoded.kind == Kind::Error {
                        let why = decoded.body.first().and_then(Value::as_str).unwrap_or("");
                        return Err(Failure(format!(
                            "{}: {why}",
                            decoded.error_name.as_deref().unwrap_or("an error")
                        )));
                    }
                    return Ok(decoded);
                }
                self.pending.push_back(decoded);
            }
            let left = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| Failure(NO_ANSWER.to_owned()))?;
            self.read_some(left.max(Duration::from_millis(1)))?;
        }
    }

    /// Waits for a message the caller is looking for — a signal, as a rule,
    /// that answers something asked earlier — keeping whatever else arrives
    /// for [`Self::poll`]. Nothing if the time runs out or the bus goes.
    pub(crate) fn wait_for(
        &mut self,
        wait: Duration,
        wanted: impl Fn(&Message) -> bool,
    ) -> Option<Message> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(at) = self.pending.iter().position(&wanted) {
                return self.pending.remove(at);
            }
            while let Some((decoded, used)) = Message::decode(&self.inbox) {
                self.inbox.drain(..used);
                let Some(decoded) = decoded else { continue };
                if wanted(&decoded) {
                    return Some(decoded);
                }
                self.pending.push_back(decoded);
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            self.read_some(left.clamp(Duration::from_millis(1), Duration::from_millis(250)))
                .ok()?;
        }
    }

    /// The socket, for waiting on it beside another — while there is a bus
    /// at the other end of it; see [`super::wait`].
    pub(crate) fn raw_fd(&self) -> Option<std::os::fd::RawFd> {
        (!self.closed).then(|| std::os::fd::AsRawFd::as_raw_fd(&self.stream))
    }

    /// Whatever has arrived, without waiting for more.
    pub(crate) fn poll(&mut self) -> Vec<Message> {
        if !self.closed && self.read_some(Duration::from_millis(1)).is_err() {
            self.closed = true;
        }
        while let Some((decoded, used)) = Message::decode(&self.inbox) {
            self.inbox.drain(..used);
            if let Some(decoded) = decoded {
                self.pending.push_back(decoded);
            }
        }
        self.pending.drain(..).collect()
    }

    /// Reads what the socket has, waiting at most so long for the first of
    /// it.
    fn read_some(&mut self, wait: Duration) -> Result<(), Failure> {
        let _ = self.stream.set_read_timeout(Some(wait));
        let mut chunk = [0u8; 8192];
        match self.stream.read(&mut chunk) {
            Ok(0) => Err(Failure("the bus closed the connection".to_owned())),
            Ok(count) => {
                self.inbox.extend_from_slice(&chunk[..count]);
                Ok(())
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(Failure(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_is_split_into_its_complete_types() {
        assert_eq!(split_signature("sa{sv}(so)u"), Some(vec!["s", "a{sv}", "(so)", "u"]));
        assert_eq!(split_signature("a(ua(so))"), Some(vec!["a(ua(so))"]));
        assert_eq!(split_signature("(s"), None, "an open struct is not a type");
    }

    #[test]
    fn a_message_is_read_back_as_it_was_written() {
        let message = Message::call(
            "org.a11y.atspi.Registry",
            "/org/a11y/atspi/accessible/root",
            "org.a11y.atspi.Socket",
            "Embed",
            vec![
                Value::Struct(vec![Value::str(":1.7"), Value::path("/a/b")]),
                Value::Array(
                    "{sv}".to_owned(),
                    vec![Value::DictEntry(
                        Box::new(Value::str("key")),
                        Box::new(Value::Variant(Box::new(Value::Int32(-3)))),
                    )],
                ),
                Value::Array("u".to_owned(), vec![Value::Uint32(1), Value::Uint32(2)]),
                Value::Array("s".to_owned(), Vec::new()),
                Value::Byte(9),
                Value::Double(1.5),
                Value::Bool(true),
                Value::Int64(-1),
            ],
        );
        let bytes = message.encode(42);
        let (read, used) = Message::decode(&bytes).expect("whole");
        assert_eq!(used, bytes.len());
        let read = read.expect("readable");
        assert_eq!(read.serial, 42);
        assert_eq!(read.path, message.path);
        assert_eq!(read.member, message.member);
        assert_eq!(read.destination, message.destination);
        assert_eq!(read.body, message.body);
        assert_eq!(read.signature(), "(so)a{sv}auasydbx");
        // Half a message is not a message yet.
        assert!(Message::decode(&bytes[..bytes.len() - 1]).is_none());
    }

    #[test]
    fn values_are_aligned_as_the_specification_says() {
        // A byte, then a 32-bit number, which starts at the next fourth
        // byte; then an array of 64-bit numbers, whose length is not counted
        // in the padding before its first element.
        let mut writer = Writer { bytes: Vec::new() };
        writer.value(&Value::Byte(1));
        writer.value(&Value::Uint32(2));
        writer.value(&Value::Array("t".to_owned(), vec![Value::Uint64(3)]));
        assert_eq!(
            writer.bytes,
            vec![1, 0, 0, 0, 2, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn an_address_names_its_socket_escapes_and_all() {
        assert_eq!(unescape_address("/run/user/0/at%2dspi/bus"), "/run/user/0/at-spi/bus");
        assert!(socket_of("tcp:host=localhost,port=1").is_none());
    }
}

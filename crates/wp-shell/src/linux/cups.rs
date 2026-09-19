//! Printing on Linux: CUPS, spoken to in its own protocol.
//!
//! # What CUPS is, from here
//!
//! A server with a socket. Everything a printer on Linux is asked — which
//! printers there are, what paper one is set up for, take this job — goes
//! to that socket as an HTTP request carrying an IPP message ([RFC 8010]
//! for the bytes, [RFC 8011] for the operations, and CUPS's own two for
//! listing printers and naming the default). No library is linked: the
//! socket is opened, the bytes are written and read, as the X server is
//! spoken to next door.
//!
//! # Why the pages go as a PDF
//!
//! The rest of the shell sends a printer its pages as pixels, a band at a
//! time, so that what comes out is what was shown — see
//! [`crate::printing`]. CUPS takes a document, not a device context, and
//! the one document format every CUPS takes is PDF. So the bands are
//! gathered into pages and the pages into a PDF of pictures, one picture a
//! page at the printer's own resolution, and that is the job. The same
//! pixels, in an envelope the queue understands.
//!
//! # What the paper is
//!
//! Asked of the printer: its default medium, named the way the standard
//! names media — `iso_a4_210x297mm`, `na_letter_8.5x11in`, the size in the
//! name — its default resolution, and the smallest margins it says it can
//! keep to, in hundredths of a millimetre. A printer that cannot be asked
//! is a printer that cannot be opened, and the program says so rather than
//! guessing a paper.
//!
//! [RFC 8010]: https://www.rfc-editor.org/rfc/rfc8010
//! [RFC 8011]: https://www.rfc-editor.org/rfc/rfc8011

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::printing::{PageSetup, Printer};

/// The IPP operations used, by number.
const PRINT_JOB: u16 = 0x0002;
const GET_PRINTER_ATTRIBUTES: u16 = 0x000B;
const CUPS_GET_DEFAULT: u16 = 0x4001;
const CUPS_GET_PRINTERS: u16 = 0x4002;

/// The tags of the values sent and read.
mod tag {
    pub const OPERATION_ATTRIBUTES: u8 = 0x01;
    pub const JOB_ATTRIBUTES: u8 = 0x02;
    pub const END: u8 = 0x03;
    pub const INTEGER: u8 = 0x21;
    pub const BOOLEAN: u8 = 0x22;
    pub const ENUM: u8 = 0x23;
    pub const RESOLUTION: u8 = 0x32;
    pub const BEGIN_COLLECTION: u8 = 0x34;
    pub const END_COLLECTION: u8 = 0x37;
    pub const NAME: u8 = 0x42;
    pub const KEYWORD: u8 = 0x44;
    pub const URI: u8 = 0x45;
    pub const CHARSET: u8 = 0x47;
    pub const LANGUAGE: u8 = 0x48;
    pub const MIME_TYPE: u8 = 0x49;
}

/// How long to wait for the server: a question is quick, a job is not.
const QUESTION: Duration = Duration::from_secs(5);
const UPLOAD: Duration = Duration::from_secs(120);

/// How many rows of a page go into one picture. A page at six hundred
/// dots to the inch is a hundred megabytes of pixels, and a strip is a few.
const STRIP: usize = 256;

// --- The connection --------------------------------------------------------

/// Where the server is: a socket file, or a host and port.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Server {
    Socket(String),
    Address(String),
}

thread_local! {
    /// A server named by a test, which stands in for the machine's own.
    static TEST_SERVER: RefCell<Option<Server>> = const { RefCell::new(None) };
    /// The jobs open on this thread, by the handle the printer carries.
    static JOBS: RefCell<HashMap<usize, Job>> = RefCell::new(HashMap::new());
    static NEXT_HANDLE: RefCell<usize> = const { RefCell::new(1) };
}

/// Names a server for the tests on this thread: `host:port`.
#[cfg(test)]
pub(crate) fn use_server(address: &str) {
    TEST_SERVER.with(|slot| *slot.borrow_mut() = Some(Server::Address(address.to_owned())));
}

/// The servers to try, in order: the one named, or the machine's own by
/// its socket, then by its port.
fn servers() -> Vec<Server> {
    if let Some(named) = TEST_SERVER.with(|slot| slot.borrow().clone()) {
        return vec![named];
    }
    if let Ok(named) = std::env::var("CUPS_SERVER") {
        let named = named.trim().to_owned();
        if !named.is_empty() {
            return if named.starts_with('/') {
                vec![Server::Socket(named)]
            } else if named.contains(':') {
                vec![Server::Address(named)]
            } else {
                vec![Server::Address(format!("{named}:631"))]
            };
        }
    }
    vec![
        Server::Socket("/run/cups/cups.sock".to_owned()),
        Server::Socket("/var/run/cups/cups.sock".to_owned()),
        Server::Address("localhost:631".to_owned()),
    ]
}

/// A stream to the server, whichever kind it is.
enum Stream {
    Unix(UnixStream),
    Tcp(TcpStream),
}

impl Stream {
    fn connect(server: &Server, timeout: Duration) -> Option<Self> {
        match server {
            Server::Socket(path) => {
                let stream = UnixStream::connect(path).ok()?;
                stream.set_read_timeout(Some(timeout)).ok()?;
                stream.set_write_timeout(Some(timeout)).ok()?;
                Some(Self::Unix(stream))
            }
            Server::Address(address) => {
                let stream = TcpStream::connect(address).ok()?;
                stream.set_read_timeout(Some(timeout)).ok()?;
                stream.set_write_timeout(Some(timeout)).ok()?;
                Some(Self::Tcp(stream))
            }
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Unix(stream) => stream.write_all(bytes),
            Self::Tcp(stream) => stream.write_all(bytes),
        }
    }

    fn read_to_end(&mut self, out: &mut Vec<u8>) -> std::io::Result<usize> {
        match self {
            Self::Unix(stream) => stream.read_to_end(out),
            Self::Tcp(stream) => stream.read_to_end(out),
        }
    }
}

/// Sends one IPP message to a path on the server and gives back the
/// message it answered with, or nothing if no server answered.
fn exchange(path: &str, message: &[u8], timeout: Duration) -> Option<Vec<u8>> {
    for server in servers() {
        let Some(mut stream) = Stream::connect(&server, timeout) else { continue };
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/ipp\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            message.len()
        );
        if stream.write_all(request.as_bytes()).is_err() || stream.write_all(message).is_err() {
            continue;
        }
        let mut raw = Vec::new();
        // A server that closes the connection ends the read, which is what
        // `Connection: close` asked for; one that does not is waited for as
        // long as the timeout allows, and what came is what is read.
        let _ = stream.read_to_end(&mut raw);
        if let Some(body) = http_body(&raw) {
            return Some(body);
        }
    }
    None
}

/// The body of an HTTP response, whether it came with a length or in
/// chunks, or nothing for a status that is not success.
fn http_body(raw: &[u8]) -> Option<Vec<u8>> {
    let end = find(raw, b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..end]);
    let mut lines = head.lines();
    let status = lines.next()?;
    let code: u16 = status.split_whitespace().nth(1)?.parse().ok()?;
    if code != 200 {
        return None;
    }
    let mut chunked = false;
    let mut length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        if name == "transfer-encoding" && value.eq_ignore_ascii_case("chunked") {
            chunked = true;
        } else if name == "content-length" {
            length = value.parse::<usize>().ok();
        }
    }
    let body = &raw[end + 4..];
    if chunked {
        return Some(unchunk(body));
    }
    match length {
        Some(length) => Some(body.get(..length.min(body.len()))?.to_vec()),
        None => Some(body.to_vec()),
    }
}

/// Joins the chunks of a chunked body.
fn unchunk(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < body.len() {
        let Some(line_end) = find(&body[at..], b"\r\n") else { break };
        let size_text = String::from_utf8_lossy(&body[at..at + line_end]);
        let size_text = size_text.split(';').next().unwrap_or_default().trim();
        let Ok(size) = usize::from_str_radix(size_text, 16) else { break };
        if size == 0 {
            break;
        }
        let start = at + line_end + 2;
        let end = (start + size).min(body.len());
        out.extend_from_slice(&body[start..end]);
        at = end + 2;
    }
    out
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

// --- The messages ---------------------------------------------------------

/// A message being built: the header, then the attributes in groups.
struct Message {
    bytes: Vec<u8>,
}

impl Message {
    /// A request of this operation, numbered.
    fn request(operation: u16, request_id: u32) -> Self {
        let mut bytes = vec![2, 0];
        bytes.extend_from_slice(&operation.to_be_bytes());
        bytes.extend_from_slice(&request_id.to_be_bytes());
        Self { bytes }
    }

    fn group(&mut self, group: u8) -> &mut Self {
        self.bytes.push(group);
        self
    }

    /// One attribute with one value.
    fn value(&mut self, value_tag: u8, name: &str, value: &[u8]) -> &mut Self {
        self.bytes.push(value_tag);
        self.bytes.extend_from_slice(&(name.len() as u16).to_be_bytes());
        self.bytes.extend_from_slice(name.as_bytes());
        self.bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
        self.bytes.extend_from_slice(value);
        self
    }

    /// Another value of the attribute just written, which has no name of
    /// its own.
    fn another(&mut self, value_tag: u8, value: &[u8]) -> &mut Self {
        self.value(value_tag, "", value)
    }

    fn text(&mut self, value_tag: u8, name: &str, value: &str) -> &mut Self {
        self.value(value_tag, name, value.as_bytes())
    }

    fn integer(&mut self, name: &str, value: i32) -> &mut Self {
        self.value(tag::INTEGER, name, &value.to_be_bytes())
    }

    /// The three every request begins with: the charset, the language,
    /// and which printer.
    fn opening(&mut self, printer_uri: &str) -> &mut Self {
        self.group(tag::OPERATION_ATTRIBUTES)
            .text(tag::CHARSET, "attributes-charset", "utf-8")
            .text(tag::LANGUAGE, "attributes-natural-language", "en")
            .text(tag::URI, "printer-uri", printer_uri)
    }

    fn finish(mut self) -> Vec<u8> {
        self.bytes.push(tag::END);
        self.bytes
    }
}

/// One value read out of a response.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Number(i32),
    Words(String),
    /// Dots across, dots down, and the unit: 3 for an inch, 4 for a
    /// centimetre.
    Resolution(i32, i32, u8),
    Other,
}

impl Value {
    fn words(&self) -> Option<&str> {
        match self {
            Self::Words(words) => Some(words),
            _ => None,
        }
    }

    fn number(&self) -> Option<i32> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }
}

/// A response, read: its status, and every attribute of every group by
/// name, in order, with all of its values.
#[derive(Debug, Default)]
struct Reply {
    status: u16,
    attributes: Vec<(u8, String, Vec<Value>)>,
}

impl Reply {
    fn read(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        let status = u16::from_be_bytes([bytes[2], bytes[3]]);
        let mut reply = Self { status, attributes: Vec::new() };
        let mut at = 8usize;
        let mut group = 0u8;
        let mut depth = 0usize;
        while at < bytes.len() {
            let value_tag = bytes[at];
            at += 1;
            if value_tag == tag::END {
                break;
            }
            if value_tag < 0x10 {
                group = value_tag;
                continue;
            }
            let name_length =
                usize::from(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]));
            at += 2;
            let name = String::from_utf8_lossy(bytes.get(at..at + name_length)?).into_owned();
            at += name_length;
            let value_length =
                usize::from(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]));
            at += 2;
            let raw = bytes.get(at..at + value_length)?;
            at += value_length;
            // A collection's members are stepped over: nothing asked for
            // here lives in one.
            match value_tag {
                tag::BEGIN_COLLECTION => {
                    depth += 1;
                    continue;
                }
                tag::END_COLLECTION => {
                    depth = depth.saturating_sub(1);
                    continue;
                }
                _ if depth > 0 => continue,
                _ => {}
            }
            let value = match value_tag {
                tag::INTEGER | tag::ENUM if raw.len() == 4 => {
                    Value::Number(i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]))
                }
                tag::BOOLEAN if raw.len() == 1 => Value::Number(i32::from(raw[0])),
                tag::RESOLUTION if raw.len() == 9 => Value::Resolution(
                    i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]),
                    i32::from_be_bytes([raw[4], raw[5], raw[6], raw[7]]),
                    raw[8],
                ),
                0x40..=0x4F => Value::Words(String::from_utf8_lossy(raw).into_owned()),
                _ => Value::Other,
            };
            if name.is_empty() {
                if let Some(last) = reply.attributes.last_mut() {
                    last.2.push(value);
                }
            } else {
                reply.attributes.push((group, name, vec![value]));
            }
        }
        Some(reply)
    }

    fn ok(&self) -> bool {
        self.status < 0x0100
    }

    /// Every value of the attribute of this name, across the groups.
    fn all(&self, name: &str) -> Vec<&Value> {
        self.attributes
            .iter()
            .filter(|(_, held, _)| held == name)
            .flat_map(|(_, _, values)| values.iter())
            .collect()
    }

    fn first(&self, name: &str) -> Option<&Value> {
        self.all(name).into_iter().next()
    }
}

// --- The questions --------------------------------------------------------

/// The address a printer of this name is spoken to at.
fn printer_uri(name: &str) -> String {
    format!("ipp://localhost/printers/{name}")
}

/// A fresh request number.
fn request_id() -> u32 {
    NEXT_HANDLE.with(|slot| {
        let mut held = slot.borrow_mut();
        *held += 1;
        *held as u32
    })
}

/// Every printer the server knows, by name.
pub(crate) fn printer_names() -> Vec<String> {
    let mut message = Message::request(CUPS_GET_PRINTERS, request_id());
    message
        .group(tag::OPERATION_ATTRIBUTES)
        .text(tag::CHARSET, "attributes-charset", "utf-8")
        .text(tag::LANGUAGE, "attributes-natural-language", "en")
        .text(tag::KEYWORD, "requested-attributes", "printer-name");
    let Some(raw) = exchange("/", &message.finish(), QUESTION) else { return Vec::new() };
    let Some(reply) = Reply::read(&raw) else { return Vec::new() };
    let mut names: Vec<String> = reply
        .all("printer-name")
        .into_iter()
        .filter_map(|value| value.words())
        .map(str::to_owned)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The printer jobs go to when nobody says otherwise.
pub(crate) fn default_printer_name() -> Option<String> {
    let mut message = Message::request(CUPS_GET_DEFAULT, request_id());
    message
        .group(tag::OPERATION_ATTRIBUTES)
        .text(tag::CHARSET, "attributes-charset", "utf-8")
        .text(tag::LANGUAGE, "attributes-natural-language", "en")
        .text(tag::KEYWORD, "requested-attributes", "printer-name");
    let raw = exchange("/", &message.finish(), QUESTION)?;
    let reply = Reply::read(&raw)?;
    reply.first("printer-name")?.words().map(str::to_owned)
}

/// What a printer says about itself, for the few things asked.
fn printer_attributes(name: &str) -> Option<Reply> {
    let mut message = Message::request(GET_PRINTER_ATTRIBUTES, request_id());
    message
        .opening(&printer_uri(name))
        .text(tag::KEYWORD, "requested-attributes", "sides-supported")
        .another(tag::KEYWORD, b"media-default")
        .another(tag::KEYWORD, b"printer-resolution-default")
        .another(tag::KEYWORD, b"media-left-margin-supported")
        .another(tag::KEYWORD, b"media-right-margin-supported")
        .another(tag::KEYWORD, b"media-top-margin-supported")
        .another(tag::KEYWORD, b"media-bottom-margin-supported");
    let raw = exchange(&format!("/printers/{name}"), &message.finish(), QUESTION)?;
    let reply = Reply::read(&raw)?;
    reply.ok().then_some(reply)
}

/// Whether a printer can turn the sheet over.
pub(crate) fn supports_both_sides(name: &str) -> bool {
    printer_attributes(name).is_some_and(|reply| {
        reply
            .all("sides-supported")
            .iter()
            .any(|value| value.words() == Some("two-sided-long-edge"))
    })
}

/// The size a medium's name carries: `iso_a4_210x297mm`,
/// `na_letter_8.5x11in`, as width and height in hundredths of a millimetre.
fn medium_size(name: &str) -> Option<(u32, u32)> {
    let size = name.rsplit('_').next()?;
    let (numbers, unit) = if let Some(rest) = size.strip_suffix("mm") {
        (rest, 100.0)
    } else if let Some(rest) = size.strip_suffix("in") {
        (rest, 2540.0)
    } else {
        return None;
    };
    let (width, height) = numbers.split_once('x')?;
    let width: f64 = width.parse().ok()?;
    let height: f64 = height.parse().ok()?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(((width * unit).round() as u32, (height * unit).round() as u32))
}

/// The smallest margin a printer offers on one side, in hundredths of a
/// millimetre; nought where it says nothing.
fn smallest_margin(reply: &Reply, name: &str) -> u32 {
    reply
        .all(name)
        .iter()
        .filter_map(|value| value.number())
        .filter(|value| *value >= 0)
        .min()
        .map_or(0, |value| value as u32)
}

/// The paper a printer is set up for, from what it says about itself.
fn page_setup(reply: &Reply) -> Option<PageSetup> {
    let medium = reply.first("media-default")?.words()?;
    let (paper_width, paper_height) = medium_size(medium)?;
    let (dpi_x, dpi_y) = match reply.first("printer-resolution-default") {
        Some(Value::Resolution(across, down, 3)) => (*across as f32, *down as f32),
        Some(Value::Resolution(across, down, 4)) => (*across as f32 * 2.54, *down as f32 * 2.54),
        _ => (300.0, 300.0),
    };
    let dots_x =
        |hundredths: u32| (f64::from(hundredths) / 2540.0 * f64::from(dpi_x)).round() as usize;
    let dots_y =
        |hundredths: u32| (f64::from(hundredths) / 2540.0 * f64::from(dpi_y)).round() as usize;
    let left = smallest_margin(reply, "media-left-margin-supported");
    let right = smallest_margin(reply, "media-right-margin-supported");
    let top = smallest_margin(reply, "media-top-margin-supported");
    let bottom = smallest_margin(reply, "media-bottom-margin-supported");
    let paper_width_dots = dots_x(paper_width);
    let paper_height_dots = dots_y(paper_height);
    Some(PageSetup {
        width: paper_width_dots.saturating_sub(dots_x(left) + dots_x(right)).max(1),
        height: paper_height_dots.saturating_sub(dots_y(top) + dots_y(bottom)).max(1),
        paper_width: paper_width_dots,
        paper_height: paper_height_dots,
        offset_x: dots_x(left),
        offset_y: dots_y(top),
        dpi_x,
        dpi_y,
    })
}

// --- The job ---------------------------------------------------------------

/// One picture of a strip of a page: where it starts, how many rows, and
/// its pixels, compressed.
struct Strip {
    top: usize,
    rows: usize,
    compressed: Vec<u8>,
}

/// One page of a job, as strips.
struct Page {
    width: usize,
    strips: Vec<Strip>,
}

/// A job being made: the printer, the paper, and the pages so far.
struct Job {
    printer: String,
    page: PageSetup,
    both_sides: Option<bool>,
    name: String,
    pages: Vec<Page>,
}

/// Opens a printer: asks it about its paper, and keeps a job for it.
pub(crate) fn open_printer_with(name: &str, both_sides: Option<bool>) -> Option<Printer> {
    let reply = printer_attributes(name)?;
    let page = page_setup(&reply)?;
    let handle = NEXT_HANDLE.with(|slot| {
        let mut held = slot.borrow_mut();
        *held += 1;
        *held
    });
    JOBS.with(|jobs| {
        jobs.borrow_mut().insert(
            handle,
            Job {
                printer: name.to_owned(),
                page,
                both_sides,
                name: String::new(),
                pages: Vec::new(),
            },
        );
    });
    Some(Printer::from_device_context(handle))
}

/// There is no system dialog to choose a printer with, so the default is
/// the choice, as it is when nobody says otherwise.
pub(crate) fn choose_printer() -> Option<Printer> {
    let name = default_printer_name()?;
    open_printer_with(&name, None)
}

pub(crate) fn printer_page(handle: usize) -> PageSetup {
    JOBS.with(|jobs| jobs.borrow().get(&handle).map(|job| job.page)).unwrap_or(PageSetup {
        width: 1,
        height: 1,
        paper_width: 1,
        paper_height: 1,
        offset_x: 0,
        offset_y: 0,
        dpi_x: 96.0,
        dpi_y: 96.0,
    })
}

pub(crate) fn start_document(handle: usize, name: &str) -> bool {
    JOBS.with(|jobs| {
        let mut jobs = jobs.borrow_mut();
        let Some(job) = jobs.get_mut(&handle) else { return false };
        job.name = name.to_owned();
        true
    })
}

/// Takes one page, a band at a time, into strips of compressed pixels.
pub(crate) fn print_page(
    handle: usize,
    width: usize,
    height: usize,
    mut band: impl FnMut(usize, usize) -> Vec<u8>,
) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    let mut strips = Vec::new();
    let mut top = 0usize;
    while top < height {
        let rows = STRIP.min(height - top);
        let bgra = band(top, rows);
        if bgra.len() < width * rows * 4 {
            return false;
        }
        let mut rgb = Vec::with_capacity(width * rows * 3);
        for pixel in bgra[..width * rows * 4].chunks_exact(4) {
            rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
        strips.push(Strip { top, rows, compressed: wp_deflate::compress_zlib(&rgb) });
        top += rows;
    }
    JOBS.with(|jobs| {
        let mut jobs = jobs.borrow_mut();
        let Some(job) = jobs.get_mut(&handle) else { return false };
        job.pages.push(Page { width, strips });
        true
    })
}

/// Sends the job, or throws it away: whether the queue took it.
pub(crate) fn finish_document(handle: usize, keep: bool) -> bool {
    let Some(job) = JOBS.with(|jobs| jobs.borrow_mut().remove(&handle)) else { return false };
    if !keep || job.pages.is_empty() {
        return false;
    }
    let document = pdf_of(&job);
    let mut message = Message::request(PRINT_JOB, request_id());
    message
        .opening(&printer_uri(&job.printer))
        .text(tag::NAME, "requesting-user-name", &whoami())
        .text(tag::NAME, "job-name", if job.name.is_empty() { "Document" } else { &job.name })
        .text(tag::MIME_TYPE, "document-format", "application/pdf")
        .group(tag::JOB_ATTRIBUTES)
        .integer("copies", 1)
        .text(
            tag::KEYWORD,
            "sides",
            match job.both_sides {
                None => "one-sided",
                Some(true) => "two-sided-long-edge",
                Some(false) => "two-sided-short-edge",
            },
        );
    let mut bytes = message.finish();
    bytes.extend_from_slice(&document);
    exchange(&format!("/printers/{}", job.printer), &bytes, UPLOAD)
        .and_then(|raw| Reply::read(&raw))
        .is_some_and(|reply| reply.ok())
}

/// Who is printing, for the queue to say.
fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_owned())
}

// --- The document -----------------------------------------------------------

/// The job as a PDF: a page for each page, each a picture of its strips
/// laid on the paper where the printable part is.
fn pdf_of(job: &Job) -> Vec<u8> {
    let paper = job.page;
    let points_x = |dots: usize| dots as f64 * 72.0 / f64::from(paper.dpi_x);
    let points_y = |dots: usize| dots as f64 * 72.0 / f64::from(paper.dpi_y);
    let paper_width = points_x(paper.paper_width);
    let paper_height = points_y(paper.paper_height);

    let mut objects: Vec<Vec<u8>> = Vec::new();
    // 1 is the catalogue and 2 the page tree; both are written once the
    // pages are numbered.
    objects.push(Vec::new());
    objects.push(Vec::new());
    let mut page_numbers = Vec::new();

    for page in &job.pages {
        // The strips, each a picture of its own.
        let mut resources = String::from("<< /XObject <<");
        let mut content = String::new();
        let mut strip_numbers = Vec::new();
        for (index, strip) in page.strips.iter().enumerate() {
            let mut image = format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB \
                 /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
                page.width,
                strip.rows,
                strip.compressed.len()
            )
            .into_bytes();
            image.extend_from_slice(&strip.compressed);
            image.extend_from_slice(b"\nendstream");
            objects.push(image);
            let number = objects.len();
            strip_numbers.push(number);
            resources.push_str(&format!(" /Im{index} {number} 0 R"));
            // Where the strip goes: the printable part's corner, then this
            // strip's rows down from its top. PDF measures from the bottom.
            let x = points_x(paper.offset_x);
            let y = paper_height - points_y(paper.offset_y + strip.top + strip.rows);
            content.push_str(&format!(
                "q {:.4} 0 0 {:.4} {:.4} {:.4} cm /Im{index} Do Q\n",
                points_x(page.width),
                points_y(strip.rows),
                x,
                y
            ));
        }
        resources.push_str(" >> >>");
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend_from_slice(content.as_bytes());
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
        let content_number = objects.len();
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {paper_width:.4} {paper_height:.4}] \
                 /Resources {resources} /Contents {content_number} 0 R >>"
            )
            .into_bytes(),
        );
        page_numbers.push(objects.len());
    }

    objects[0] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    let kids: Vec<String> = page_numbers.iter().map(|number| format!("{number} 0 R")).collect();
    objects[1] =
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), page_numbers.len())
            .into_bytes();

    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_request_is_laid_out_as_the_rfc_says() {
        // Get-Printer-Attributes for one printer, byte by byte: the version,
        // the operation, the number, the group, and the three attributes
        // every request opens with.
        let mut message = Message::request(GET_PRINTER_ATTRIBUTES, 7);
        message.opening("ipp://localhost/printers/Office");
        let bytes = message.finish();
        let mut wanted = vec![0x02, 0x00, 0x00, 0x0B, 0x00, 0x00, 0x00, 0x07, 0x01];
        for (tag, name, value) in [
            (0x47u8, "attributes-charset", "utf-8"),
            (0x48, "attributes-natural-language", "en"),
            (0x45, "printer-uri", "ipp://localhost/printers/Office"),
        ] {
            wanted.push(tag);
            wanted.extend_from_slice(&(name.len() as u16).to_be_bytes());
            wanted.extend_from_slice(name.as_bytes());
            wanted.extend_from_slice(&(value.len() as u16).to_be_bytes());
            wanted.extend_from_slice(value.as_bytes());
        }
        wanted.push(0x03);
        assert_eq!(bytes, wanted);
    }

    /// A reply as a server would write it: successful, with these
    /// attributes in the printer group.
    fn reply_with(attributes: &[(u8, &str, Vec<Vec<u8>>)]) -> Vec<u8> {
        let mut bytes = vec![0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x04];
        for (tag, name, values) in attributes {
            for (index, value) in values.iter().enumerate() {
                bytes.push(*tag);
                let name = if index == 0 { *name } else { "" };
                bytes.extend_from_slice(&(name.len() as u16).to_be_bytes());
                bytes.extend_from_slice(name.as_bytes());
                bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
                bytes.extend_from_slice(value);
            }
        }
        bytes.push(0x03);
        bytes
    }

    fn resolution(across: i32, down: i32, unit: u8) -> Vec<u8> {
        let mut out = across.to_be_bytes().to_vec();
        out.extend_from_slice(&down.to_be_bytes());
        out.push(unit);
        out
    }

    #[test]
    fn a_reply_is_read_with_every_value_of_a_set_and_the_collections_stepped_over() {
        let mut bytes = reply_with(&[
            (0x42, "printer-name", vec![b"Office".to_vec()]),
            (0x44, "sides-supported", vec![b"one-sided".to_vec(), b"two-sided-long-edge".to_vec()]),
            (0x32, "printer-resolution-default", vec![resolution(600, 600, 3)]),
            (
                0x21,
                "media-left-margin-supported",
                vec![423i32.to_be_bytes().to_vec(), 0i32.to_be_bytes().to_vec()],
            ),
        ]);
        // A collection in the middle, whose members must not be read as
        // attributes of their own.
        let end = bytes.pop();
        bytes.push(0x34);
        bytes.extend_from_slice(&[0x00, 0x09]);
        bytes.extend_from_slice(b"media-col");
        bytes.extend_from_slice(&[0x00, 0x00]);
        bytes.push(0x4A);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x0A]);
        bytes.extend_from_slice(b"media-size");
        bytes.push(0x37);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        bytes.push(end.unwrap());

        let reply = Reply::read(&bytes).expect("a reply");
        assert!(reply.ok());
        assert_eq!(reply.first("printer-name").and_then(Value::words), Some("Office"));
        assert_eq!(reply.all("sides-supported").len(), 2);
        assert_eq!(
            reply.first("printer-resolution-default"),
            Some(&Value::Resolution(600, 600, 3))
        );
        assert_eq!(smallest_margin(&reply, "media-left-margin-supported"), 0);
        assert!(reply.first("media-size").is_none(), "a member was read as an attribute");
    }

    #[test]
    fn a_medium_carries_its_size_in_its_name() {
        assert_eq!(medium_size("iso_a4_210x297mm"), Some((21000, 29700)));
        assert_eq!(medium_size("na_letter_8.5x11in"), Some((21590, 27940)));
        assert_eq!(medium_size("om_odd_100x200mm"), Some((10000, 20000)));
        assert_eq!(medium_size("iso_a4"), None);
    }

    #[test]
    fn the_paper_is_the_medium_at_the_resolution_less_the_margins() {
        let bytes = reply_with(&[
            (0x44, "media-default", vec![b"iso_a4_210x297mm".to_vec()]),
            (0x32, "printer-resolution-default", vec![resolution(600, 600, 3)]),
            (0x21, "media-left-margin-supported", vec![423i32.to_be_bytes().to_vec()]),
            (0x21, "media-right-margin-supported", vec![423i32.to_be_bytes().to_vec()]),
            (0x21, "media-top-margin-supported", vec![423i32.to_be_bytes().to_vec()]),
            (0x21, "media-bottom-margin-supported", vec![423i32.to_be_bytes().to_vec()]),
        ]);
        let page = page_setup(&Reply::read(&bytes).expect("a reply")).expect("a page");
        assert_eq!((page.paper_width, page.paper_height), (4961, 7016));
        assert_eq!((page.offset_x, page.offset_y), (100, 100));
        assert_eq!((page.width, page.height), (4761, 6816));
        assert_eq!(page.unprintable(), (100.0, 100.0, 100.0, 100.0));
    }

    #[test]
    fn a_chunked_body_is_joined() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        assert_eq!(http_body(raw).as_deref(), Some(&b"abcde"[..]));
        let raw = b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n";
        assert_eq!(http_body(raw), None);
    }

    /// What the pretend server was asked, and the document it was sent.
    #[derive(Default)]
    struct Seen {
        operations: Vec<u16>,
        document: Vec<u8>,
    }

    /// A server that answers as CUPS with one printer would, for as many
    /// requests as it is told to expect.
    fn pretend_cups(expected: usize) -> (String, Arc<Mutex<Seen>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("the address").to_string();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let recorded = Arc::clone(&seen);
        std::thread::spawn(move || {
            for _ in 0..expected {
                let Ok((mut stream, _)) = listener.accept() else { break };
                let mut raw = Vec::new();
                let mut buffer = [0u8; 8192];
                // The headers, then as much body as the length says.
                let body_length = loop {
                    let Ok(read) = stream.read(&mut buffer) else { break 0 };
                    if read == 0 {
                        break 0;
                    }
                    raw.extend_from_slice(&buffer[..read]);
                    if let Some(end) = find(&raw, b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&raw[..end]).to_string();
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        while raw.len() < end + 4 + length {
                            let Ok(read) = stream.read(&mut buffer) else { break };
                            if read == 0 {
                                break;
                            }
                            raw.extend_from_slice(&buffer[..read]);
                        }
                        break length;
                    }
                };
                let end = find(&raw, b"\r\n\r\n").unwrap_or(0) + 4;
                let body = &raw[end..(end + body_length).min(raw.len())];
                let operation = u16::from_be_bytes([body[2], body[3]]);
                let answer = match operation {
                    CUPS_GET_PRINTERS | CUPS_GET_DEFAULT => {
                        reply_with(&[(0x42, "printer-name", vec![b"Office".to_vec()])])
                    }
                    GET_PRINTER_ATTRIBUTES => reply_with(&[
                        (
                            0x44,
                            "sides-supported",
                            vec![b"one-sided".to_vec(), b"two-sided-long-edge".to_vec()],
                        ),
                        (0x44, "media-default", vec![b"na_letter_8.5x11in".to_vec()]),
                        (0x32, "printer-resolution-default", vec![resolution(300, 300, 3)]),
                    ]),
                    PRINT_JOB => {
                        // The document follows the end-of-attributes tag.
                        let data_at = body
                            .iter()
                            .position(|byte| *byte == 0x03)
                            .map_or(body.len(), |at| at + 1);
                        recorded.lock().expect("the record").document = body[data_at..].to_vec();
                        reply_with(&[(0x21, "job-id", vec![42i32.to_be_bytes().to_vec()])])
                    }
                    _ => reply_with(&[]),
                };
                recorded.lock().expect("the record").operations.push(operation);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\n\r\n",
                    answer.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&answer);
            }
        });
        (address, seen)
    }

    #[test]
    fn a_job_goes_to_the_server_as_a_pdf_of_its_pages() {
        let (address, seen) = pretend_cups(5);
        use_server(&address);

        assert_eq!(printer_names(), vec!["Office".to_owned()]);
        assert_eq!(default_printer_name().as_deref(), Some("Office"));
        assert!(supports_both_sides("Office"));

        let mut printer = open_printer_with("Office", Some(true)).expect("the printer");
        let page = printer.page();
        assert_eq!((page.paper_width, page.paper_height), (2550, 3300));
        assert!((page.dpi_x - 300.0).abs() < f32::EPSILON);
        assert!(printer.start("Letter"));

        // Two pages, each drawn as bands of one colour.
        for shade in [0x20u8, 0xC0] {
            let sent = printer.print_page(page.width, 600, |_, rows| {
                let mut canvas = wp_raster::Canvas::new(page.width, rows);
                canvas.fill_rect(
                    0,
                    0,
                    page.width as i32,
                    rows as i32,
                    wp_raster::Color::rgb(shade, shade, shade),
                );
                canvas
            });
            assert!(sent);
        }
        assert!(printer.finish(), "the queue did not take the job");

        let seen = seen.lock().expect("the record");
        assert_eq!(seen.operations.last(), Some(&PRINT_JOB));
        let document = &seen.document;
        assert!(document.starts_with(b"%PDF-1.4"), "not a PDF");
        let text = String::from_utf8_lossy(document);
        assert!(text.contains("/Count 2"), "not two pages");
        assert!(text.contains("/MediaBox [0 0 612.0000 792.0000]"), "{}", &text[..300]);

        // The cross-reference table points at every object, which is what
        // a reader that is not this program holds a PDF to. Counted in
        // bytes: the pictures are not text.
        let startxref =
            document.windows(10).rposition(|window| window == b"startxref\n").expect("startxref")
                + 10;
        let tail = String::from_utf8_lossy(&document[startxref..]).to_string();
        let xref: usize =
            tail.lines().next().expect("the offset").trim().parse().expect("a number");
        assert!(document[xref..].starts_with(b"xref\n"));
        let table = String::from_utf8_lossy(&document[xref..]).to_string();
        let count: usize = table
            .lines()
            .nth(1)
            .expect("the count")
            .split(' ')
            .nth(1)
            .expect("count")
            .parse()
            .expect("a number");
        for (number, line) in table.lines().skip(3).take(count - 1).enumerate() {
            let offset: usize = line[..10].parse().expect("an offset");
            let wanted = format!("{} 0 obj\n", number + 1);
            assert!(
                document[offset..].starts_with(wanted.as_bytes()),
                "object {} is not where the table says",
                number + 1
            );
        }
    }

    #[test]
    fn with_no_server_there_are_no_printers_and_nothing_is_opened() {
        // A port nobody listens on.
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("the address").to_string();
        drop(listener);
        use_server(&address);
        assert!(printer_names().is_empty());
        assert_eq!(default_printer_name(), None);
        assert!(open_printer_with("Office", None).is_none());
        assert!(choose_printer().is_none());
    }
}

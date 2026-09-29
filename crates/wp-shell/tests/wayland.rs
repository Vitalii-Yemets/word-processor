//! The Wayland shell against a real compositor.
//!
//! Sway will run without a screen — its headless backend draws into memory
//! — which is all a test needs: a window opens on it, paints, is
//! photographed by `grim` (another client, so what it sees is what the
//! compositor is showing rather than what this program thinks it drew),
//! typed into by `wtype`, and closed. Where the machine has no sway the
//! tests pass without proving anything, and say so on the way out.
#![cfg(target_os = "linux")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use wp_raster::{Canvas, Color};
use wp_shell::{App, Event, Response, WindowOptions};

/// One test on a compositor at a time: which compositor a program talks to
/// is named by the environment, and the environment is the whole program's.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A compositor with no screen, for the length of a test.
struct Compositor {
    child: Child,
    runtime: PathBuf,
    display: String,
}

impl Compositor {
    /// Starts one, as somebody who is not root.
    ///
    /// Sway refuses to run as root — it will not start where it cannot
    /// drop privileges — and everything in the build container is root,
    /// so it is started as the user the image keeps for this. The socket
    /// it makes belongs to that user; this program, being root, may still
    /// connect to it, which is the whole of why that works.
    fn start(name: &str, width: u32, height: u32) -> Option<Self> {
        Self::start_with(name, width, height, "")
    }

    /// Starts one with lines of its configuration added.
    fn start_with(name: &str, width: u32, height: u32, extra: &str) -> Option<Self> {
        let runtime =
            std::env::temp_dir().join(format!("wp-wayland-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&runtime);
        std::fs::create_dir_all(&runtime).ok()?;
        let config = runtime.join("config");
        let mut file = std::fs::File::create(&config).ok()?;
        // No frame of the compositor's own: the program draws its own
        // title bar, and a picture with sway's border on it would be a
        // picture of sway.
        writeln!(
            file,
            "default_border none
default_floating_border none
gaps inner 0
output HEADLESS-1 resolution {width}x{height}
{extra}"
        )
        .ok()?;
        drop(file);
        if !Command::new("chown")
            .args(["-R", COMPOSITOR_USER])
            .arg(&runtime)
            .status()
            .is_ok_and(|status| status.success())
        {
            return None;
        }

        let started_as = format!(
            "XDG_RUNTIME_DIR={} WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 XDG_SESSION_TYPE=wayland WAYLAND_DEBUG=server exec sway -d -c {}",
            runtime.display(),
            config.display()
        );
        let child = Command::new("su")
            .args([COMPOSITOR_USER, "-s", "/bin/sh", "-c", &started_as])
            .env_remove("DISPLAY")
            .stdout(Stdio::from(std::fs::File::create("/tmp/sway-test.log").ok()?))
            .stderr(Stdio::from(std::fs::File::create("/tmp/sway-test-err.log").ok()?))
            .spawn()
            .ok()?;
        let mut compositor = Self { child, runtime: runtime.clone(), display: String::new() };
        // The compositor chooses the socket's name itself, taking the
        // first that is free: what it is has to be looked for rather than
        // told.
        let started = Instant::now();
        loop {
            if let Some(found) = socket_in(&runtime) {
                compositor.display = found;
                break;
            }
            if started.elapsed() > Duration::from_secs(20) {
                compositor.stop();
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // The socket exists a moment before the compositor is ready to
        // answer on it.
        std::thread::sleep(Duration::from_millis(500));
        std::env::set_var("XDG_RUNTIME_DIR", &runtime);
        std::env::set_var("WAYLAND_DISPLAY", &compositor.display);
        Some(compositor)
    }

    /// Runs one of the compositor's own clients, in its own environment.
    fn client(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("WAYLAND_DISPLAY", &self.display)
            .env_remove("DISPLAY");
        command
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.runtime);
        // The environment is the whole program's: a compositor that has
        // stopped must not be what the next test in this binary talks to.
        std::env::remove_var("WAYLAND_DISPLAY");
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Who the compositor runs as. See [`Compositor::start`].
const COMPOSITOR_USER: &str = "compositor";

/// The name of the compositor's socket, which is `wayland-` and a number.
fn socket_in(runtime: &Path) -> Option<String> {
    let entries = std::fs::read_dir(runtime).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("wayland-") && !name.ends_with(".lock") {
            return Some(name);
        }
    }
    None
}

/// What `grim` photographed, as red, green and blue.
fn photograph(compositor: &Compositor) -> Option<(usize, usize, Vec<u8>)> {
    let output = compositor.client("grim").args(["-t", "ppm", "-"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    read_ppm(&output.stdout)
}

/// A picture as `grim` writes one with `-t ppm`: "P6", the size, the
/// largest value, and then three bytes a pixel.
fn read_ppm(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut fields = Vec::new();
    let mut at = 0usize;
    while fields.len() < 4 && at < bytes.len() {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if bytes.get(at) == Some(&b'#') {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
    }
    if fields.first().map(String::as_str) != Some("P6") {
        return None;
    }
    let width: usize = fields.get(1)?.parse().ok()?;
    let height: usize = fields.get(2)?.parse().ok()?;
    at += 1;
    let pixels = bytes.get(at..at + width * height * 3)?.to_vec();
    Some((width, height, pixels))
}

/// Writes a photograph where the proofs go, when somewhere was named.
fn keep_proof(name: &str, width: usize, height: usize, rgb: &[u8]) {
    let Ok(directory) = std::env::var("WP_PROOFS") else { return };
    let mut canvas = Canvas::new(width, height);
    let rgba: Vec<u8> =
        rgb.chunks_exact(3).flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255]).collect();
    canvas.draw_pixels(&rgba, width, height, 0, 0, width, height);
    let _ = std::fs::create_dir_all(&directory);
    let _ = std::fs::write(Path::new(&directory).join(name), wp_raster::encode_png(&canvas));
}

/// What the program saw while its window was up.
#[derive(Default)]
struct Seen {
    ticks: u32,
    sizes: Vec<(u32, u32)>,
    photographed: Option<(usize, usize, Vec<u8>)>,
    typed: String,
    keys: Vec<wp_shell::Key>,
    pasted: Option<String>,
}

/// A program that paints its window, photographs it, is typed into, and
/// closes itself.
struct Painter {
    canvas: Canvas,
    seen: std::rc::Rc<std::cell::RefCell<Seen>>,
    display: String,
    runtime: PathBuf,
}

/// The tick the picture is taken on, the two the typing is done on, and
/// the one the window closes on. Far enough apart for the compositor to
/// have dealt with what came before.
const PHOTOGRAPH_AT: u32 = 12;
const TYPE_AT: u32 = 24;
const CLOSE_AT: u32 = 44;

impl App for Painter {
    fn handle(&mut self, event: Event) -> Response {
        let mut seen = self.seen.borrow_mut();
        match event {
            Event::Resized { width, height } => {
                seen.sizes.push((width, height));
                Response::Redraw
            }
            Event::Char(character) => {
                seen.typed.push(character);
                Response::Ignored
            }
            Event::KeyDown { key, .. } => {
                seen.keys.push(key);
                Response::Ignored
            }
            Event::Tick => {
                seen.ticks += 1;
                if seen.ticks == PHOTOGRAPH_AT {
                    let compositor = Compositor {
                        // Not started here: this is the one already running,
                        // named so that its own clients can be run.
                        child: Command::new("true").spawn().expect("a process that does nothing"),
                        runtime: self.runtime.clone(),
                        display: self.display.clone(),
                    };
                    seen.photographed = photograph(&compositor);
                    // The clipboard, while the window is up to own it.
                    assert!(wp_shell::clipboard::set_text("carried by the compositor"));
                    seen.pasted = wp_shell::clipboard::text();
                    core::mem::forget(compositor);
                }
                if seen.ticks == TYPE_AT {
                    // Two spaces before the word on purpose. A compositor
                    // with no devices has no keyboard until this very
                    // program makes one, and the first key is pressed in
                    // the same instant: nobody is listening for it yet.
                    // What is being proved is that the keys arrive, so the
                    // sacrifice is made deliberately rather than by
                    // leaving a letter of the word to chance.
                    // Started rather than waited for: the program has to go
                    // on reading while the keys are being typed, or the
                    // keyboard would appear and be used while it was not
                    // listening.
                    let typed = Command::new("wtype")
                        .args(["-s", "120"])
                        .arg("  Wayland")
                        .env("XDG_RUNTIME_DIR", &self.runtime)
                        .env("WAYLAND_DISPLAY", &self.display)
                        .env_remove("DISPLAY")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                    if let Err(error) = typed {
                        eprintln!("wtype could not run: {error}");
                    }
                }
                if seen.ticks >= CLOSE_AT {
                    return Response::Close;
                }
                Response::Ignored
            }
            _ => Response::Ignored,
        }
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.canvas = Canvas::new(width, height);
        self.canvas.clear(Color::rgb(40, 80, 200));
        self.canvas.fill_rect(30, 30, 140, 90, Color::rgb(240, 200, 40));
        &self.canvas
    }
}

/// How much of a picture is one colour.
fn share_of(rgb: &[u8], colour: (u8, u8, u8)) -> f32 {
    let matching =
        rgb.chunks_exact(3).filter(|pixel| (pixel[0], pixel[1], pixel[2]) == colour).count();
    matching as f32 / (rgb.len() / 3) as f32
}

#[test]
fn a_window_opens_paints_takes_typing_and_closes_on_a_real_compositor() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(compositor) = Compositor::start("window", 1000, 700) else {
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Seen::default()));
    let painter = Painter {
        canvas: Canvas::new(1, 1),
        seen: std::rc::Rc::clone(&seen),
        display: compositor.display.clone(),
        runtime: compositor.runtime.clone(),
    };
    let options = WindowOptions { title: "Painter".to_owned(), width: 600, height: 400 };
    wp_shell::run(options, Box::new(painter)).expect("the window opens on the compositor");

    let seen = std::rc::Rc::try_unwrap(seen).ok().expect("the shell let go").into_inner();
    assert!(seen.ticks >= CLOSE_AT, "the loop ran to the end: {} ticks", seen.ticks);
    assert!(!seen.sizes.is_empty(), "the compositor said how big the window is to be");
    // Sway tiles a window to fill the screen, so the size is the screen's
    // rather than the one asked for — which is the point: on Wayland the
    // compositor decides, and the program draws what it is told.
    assert_eq!(seen.sizes.last(), Some(&(1000, 700)), "sizes seen: {:?}", seen.sizes);

    let (width, height, rgb) = seen.photographed.expect("grim photographed the screen");
    assert_eq!((width, height), (1000, 700));
    keep_proof("wayland-window.png", width, height, &rgb);
    let at = |x: usize, y: usize| {
        let base = (y * width + x) * 3;
        (rgb[base], rgb[base + 1], rgb[base + 2])
    };
    assert_eq!(at(10, 10), (40, 80, 200), "the window's own paint reached the compositor");
    assert_eq!(at(100, 80), (240, 200, 40), "and so did the square drawn on it");
    assert!(share_of(&rgb, (40, 80, 200)) > 0.5, "the window fills the screen");

    assert_eq!(seen.typed.trim_start(), "Wayland", "what was typed into the window arrived");
    assert_eq!(
        seen.pasted.as_deref(),
        Some("carried by the compositor"),
        "and text put on the clipboard comes back"
    );
}

// --- The test's own clients of the compositor --------------------------------

/// A client of the compositor spoken to on the wire, for the test's own
/// input method and its own pointer: messages of an object, an opcode and a
/// length, then arguments, the way the protocol says.
mod client {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::time::{Duration, Instant};

    pub(super) struct Wire {
        stream: UnixStream,
        inbox: Vec<u8>,
        next: u32,
    }

    pub(super) fn uint(value: u32) -> Vec<u8> {
        value.to_ne_bytes().to_vec()
    }

    pub(super) fn int(value: i32) -> Vec<u8> {
        uint(value as u32)
    }

    pub(super) fn string(text: &str) -> Vec<u8> {
        let mut bytes = uint(text.len() as u32 + 1);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        bytes
    }

    /// Reads a string argument at the start of a body.
    fn string_at(body: &[u8]) -> String {
        let length = u32::from_ne_bytes([body[0], body[1], body[2], body[3]]) as usize;
        String::from_utf8_lossy(&body[4..4 + length.saturating_sub(1)]).into_owned()
    }

    impl Wire {
        /// Connects, and binds the globals asked for by their interface at
        /// the version given; their objects, in the order asked.
        pub(super) fn connect(
            runtime: &Path,
            display: &str,
            wanted: &[(&str, u32)],
        ) -> Result<(Self, Vec<u32>), String> {
            let stream = UnixStream::connect(runtime.join(display))
                .map_err(|error| format!("cannot reach the compositor: {error}"))?;
            let mut wire = Self { stream, inbox: Vec::new(), next: 4 };
            let until = Instant::now() + Duration::from_secs(10);
            // The registry, and a round trip for the list of what is offered.
            wire.send(1, 1, &[uint(2)]);
            wire.send(1, 0, &[uint(3)]);
            let mut offered = Vec::new();
            loop {
                let (object, opcode, body) =
                    wire.event(until).ok_or("the compositor did not answer")?;
                if object == 2 && opcode == 0 {
                    let name = u32::from_ne_bytes([body[0], body[1], body[2], body[3]]);
                    offered.push((name, string_at(&body[4..])));
                }
                if object == 3 {
                    break;
                }
            }
            let mut objects = Vec::new();
            for (interface, version) in wanted {
                let name = offered
                    .iter()
                    .find(|(_, offered)| offered == interface)
                    .map(|(name, _)| *name)
                    .ok_or_else(|| format!("the compositor has no {interface}"))?;
                let id = wire.new_id();
                wire.send(2, 0, &[uint(name), string(interface), uint(*version), uint(id)]);
                objects.push(id);
            }
            Ok((wire, objects))
        }

        pub(super) fn new_id(&mut self) -> u32 {
            let id = self.next;
            self.next += 1;
            id
        }

        pub(super) fn send(&mut self, object: u32, opcode: u16, arguments: &[Vec<u8>]) {
            let body: Vec<u8> = arguments.concat();
            let mut message = uint(object);
            message.extend(uint(((8 + body.len() as u32) << 16) | u32::from(opcode)));
            message.extend(body);
            let _ = self.stream.write_all(&message);
        }

        /// The next event, or nothing once the wait is over.
        pub(super) fn event(&mut self, until: Instant) -> Option<(u32, u16, Vec<u8>)> {
            loop {
                if self.inbox.len() >= 8 {
                    let word = |at: usize| {
                        u32::from_ne_bytes([
                            self.inbox[at],
                            self.inbox[at + 1],
                            self.inbox[at + 2],
                            self.inbox[at + 3],
                        ])
                    };
                    let (object, header) = (word(0), word(4));
                    let length = (header >> 16) as usize;
                    if length >= 8 && self.inbox.len() >= length {
                        let body = self.inbox[8..length].to_vec();
                        self.inbox.drain(..length);
                        return Some((object, (header & 0xFFFF) as u16, body));
                    }
                }
                let left = until.checked_duration_since(Instant::now())?;
                let _ = self.stream.set_read_timeout(Some(left.max(Duration::from_millis(1))));
                let mut chunk = [0u8; 4096];
                match self.stream.read(&mut chunk) {
                    Ok(0) => return None,
                    Ok(count) => self.inbox.extend_from_slice(&chunk[..count]),
                    Err(_) if Instant::now() < until => {}
                    Err(_) => return None,
                }
            }
        }
    }
}

// --- Composing through the compositor's input method -------------------------

/// An input method of the test's own, which is the other end the
/// compositor relays between: it tells the compositor it composes text,
/// and when a window asks for text it composes 한글 into it a letter at a
/// time, as a Korean input method would, committing each syllable when the
/// next begins. What is under test is the window's end; this only drives,
/// the way `wtype` drives the keyboard, and the compositor between them is
/// sway's own relay.
mod input_method {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::client::{int, string, uint, Wire};

    /// Runs the input method until it has composed its word, or the time is
    /// up; what went wrong, where something did.
    pub(super) fn compose(runtime: &Path, display: &str) -> Result<(), String> {
        let (mut wire, objects) =
            Wire::connect(runtime, display, &[("wl_seat", 1), ("zwp_input_method_manager_v2", 1)])?;
        let (seat, manager) = (objects[0], objects[1]);
        let until = Instant::now() + Duration::from_secs(30);
        // The input method for the seat.
        let method = wire.new_id();
        wire.send(manager, 0, &[uint(seat), uint(method)]);

        let mut done = 0u32;
        let mut active = false;
        loop {
            let (object, opcode, _) =
                wire.event(until).ok_or("no window asked for text in time")?;
            if object != method {
                continue;
            }
            match opcode {
                0 => active = true,
                1 => active = false,
                5 => {
                    done += 1;
                    if active {
                        break;
                    }
                }
                6 => return Err("another input method has the seat".to_owned()),
                _ => {}
            }
        }
        // The word, as a Korean input method builds it. Each step is one
        // batch: what is committed, then what is being composed and where
        // its cursor is — a range of bytes of it, which where it is not
        // empty is the part being chosen for.
        let steps: [(&str, &str, i32, i32); 7] = [
            ("", "ㅎ", 3, 3),
            ("", "하", 3, 3),
            ("", "한", 3, 3),
            ("한", "ㄱ", 3, 3),
            ("", "그", 3, 3),
            ("", "글", 0, 3),
            ("글", "", -1, -1),
        ];
        for (commit, preedit, begin, end) in steps {
            if !commit.is_empty() {
                wire.send(method, 0, &[string(commit)]);
            }
            if !preedit.is_empty() {
                wire.send(method, 1, &[string(preedit), int(begin), int(end)]);
            }
            wire.send(method, 3, &[uint(done)]);
            std::thread::sleep(Duration::from_millis(150));
            // Whatever the compositor said meanwhile is counted, so that
            // the next batch names the state it follows.
            while let Some((object, opcode, _)) =
                wire.event(Instant::now() + Duration::from_millis(20))
            {
                if object == method && opcode == 5 {
                    done += 1;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(300));
        Ok(())
    }
}

/// What the program was given by the input method.
#[derive(Default)]
struct Composed {
    shown: Vec<(String, usize, Vec<wp_shell::CompositionAttribute>)>,
    committed: Vec<String>,
    ended: u32,
    typed: String,
}

/// A program that tells the input method where its caret is, keeps what it
/// is given, and closes once the word is in or the time is up.
struct Writer {
    canvas: Canvas,
    seen: std::rc::Rc<std::cell::RefCell<Composed>>,
    started: Instant,
}

impl App for Writer {
    fn handle(&mut self, event: Event) -> Response {
        let mut seen = self.seen.borrow_mut();
        match event {
            Event::Compose { text, caret, attributes } => {
                seen.shown.push((text, caret, attributes));
                Response::Redraw
            }
            Event::Commit(text) => {
                seen.committed.push(text);
                Response::Redraw
            }
            Event::ComposeEnd => {
                seen.ended += 1;
                Response::Ignored
            }
            Event::Char(character) => {
                seen.typed.push(character);
                Response::Ignored
            }
            Event::Tick => {
                let finished = seen.committed.concat() == "한글";
                if finished || self.started.elapsed() > Duration::from_secs(30) {
                    return Response::Close;
                }
                Response::Ignored
            }
            _ => Response::Ignored,
        }
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.canvas = Canvas::new(width, height);
        self.canvas.clear(Color::WHITE);
        wp_shell::place_composition(120, 80, 18);
        &self.canvas
    }
}

#[test]
fn text_composed_by_the_compositors_input_method_is_shown_and_committed() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(compositor) = Compositor::start("compose", 800, 600) else {
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    let method = {
        let runtime = compositor.runtime.clone();
        let display = compositor.display.clone();
        std::thread::spawn(move || input_method::compose(&runtime, &display))
    };
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Composed::default()));
    let writer = Writer {
        canvas: Canvas::new(1, 1),
        seen: std::rc::Rc::clone(&seen),
        started: Instant::now(),
    };
    let options = WindowOptions { title: "Writer".to_owned(), width: 600, height: 400 };
    wp_shell::run(options, Box::new(writer)).expect("the window opens on the compositor");
    let drove = method.join().expect("the input method ran");
    let log = std::fs::read_to_string("/tmp/sway-test-err.log").unwrap_or_default();

    let seen = std::rc::Rc::try_unwrap(seen).ok().expect("the shell let go").into_inner();
    drove.expect("the input method composed its word");
    assert_eq!(
        seen.committed,
        vec!["한".to_owned(), "글".to_owned()],
        "each syllable is committed as the next begins: shown {:?}",
        seen.shown
    );
    let shown: Vec<&str> = seen.shown.iter().map(|(text, _, _)| text.as_str()).collect();
    assert_eq!(shown, ["ㅎ", "하", "한", "ㄱ", "그", "글"], "the syllable is shown as it grows");
    assert!(seen
        .shown
        .iter()
        .take(5)
        .all(|(text, caret, attributes)| *caret == text.chars().count()
            && attributes.iter().all(|mark| *mark == wp_shell::CompositionAttribute::Input)));
    assert_eq!(
        seen.shown[5],
        ("글".to_owned(), 1, vec![wp_shell::CompositionAttribute::Target]),
        "and a range of the text being chosen for is marked as such"
    );
    assert!(seen.typed.is_empty(), "nothing arrives as typed: {:?}", seen.typed);
    assert!(
        log.contains("set_cursor_rectangle(120, 80, 1, 18)"),
        "the caret's place reached the compositor"
    );
    assert!(log.contains(".enable()"), "and text was asked for");
}

// --- Dragging and dropping between programs ---------------------------------

/// A pointer of the test's own — the compositor's virtual pointer, which is
/// how a program without a mouse moves one — so that a drag can be made the
/// way a hand makes it: pressed on one window, carried, let go on another.
mod pointer {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::client::{uint, Wire};

    pub(super) struct Pointer {
        wire: Wire,
        id: u32,
        extent: (u32, u32),
        started: Instant,
    }

    impl Pointer {
        pub(super) fn new(
            runtime: &Path,
            display: &str,
            width: u32,
            height: u32,
        ) -> Result<Self, String> {
            let (mut wire, objects) = Wire::connect(
                runtime,
                display,
                &[("wl_seat", 1), ("zwlr_virtual_pointer_manager_v1", 1)],
            )?;
            let id = wire.new_id();
            wire.send(objects[1], 0, &[uint(objects[0]), uint(id)]);
            Ok(Self { wire, id, extent: (width, height), started: Instant::now() })
        }

        fn time(&self) -> u32 {
            self.started.elapsed().as_millis() as u32
        }

        fn move_to(&mut self, (x, y): (i32, i32)) {
            let time = self.time();
            let (width, height) = self.extent;
            self.wire.send(
                self.id,
                1,
                &[uint(time), uint(x as u32), uint(y as u32), uint(width), uint(height)],
            );
            self.wire.send(self.id, 4, &[]);
        }

        fn button(&mut self, pressed: bool) {
            let time = self.time();
            self.wire.send(self.id, 2, &[uint(time), uint(0x110), uint(u32::from(pressed))]);
            self.wire.send(self.id, 4, &[]);
        }

        /// Pressed at one point, carried a step at a time to another, and
        /// let go there.
        pub(super) fn drag(&mut self, from: (i32, i32), to: (i32, i32)) {
            self.move_to(from);
            std::thread::sleep(Duration::from_millis(300));
            self.button(true);
            for step in 1..=12 {
                std::thread::sleep(Duration::from_millis(80));
                self.move_to((
                    from.0 + (to.0 - from.0) * step / 12,
                    from.1 + (to.1 - from.1) * step / 12,
                ));
            }
            std::thread::sleep(Duration::from_millis(400));
            self.button(false);
        }
    }
}

/// Where the windows go: the peer at the right, this program's at the
/// left, both floating so that where each is is known.
const PLACES: &str = "for_window [title=\"^dnd peer\"] floating enable, move absolute position 500 0, resize set 300 300
for_window [title=\"^Hand \"] floating enable, move absolute position 0 0, resize set 400 300
";

/// The peer, as a client of the compositor, and what it says.
fn peer(
    compositor: &Compositor,
    arguments: &[&str],
) -> Option<(Child, std::sync::mpsc::Receiver<String>)> {
    use std::io::{BufRead, BufReader};
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/dnd-peer.py");
    let mut child = compositor
        .client("python3")
        .arg(script)
        .args(arguments)
        .env("GDK_BACKEND", "wayland")
        .env("NO_AT_BRIDGE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let out = child.stdout.take()?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = sender.send(line);
        }
    });
    let first = receiver.recv_timeout(Duration::from_secs(15)).ok();
    if first.as_deref() != Some("ready") {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    Some((child, receiver))
}

fn gtk_is_here() -> bool {
    Command::new("python3")
        .args(["-c", "import gi; gi.require_version('Gtk', '3.0'); from gi.repository import Gtk"])
        .status()
        .is_ok_and(|status| status.success())
}

/// What the program saw.
#[derive(Default)]
struct Handled {
    over: Vec<(i32, i32)>,
    dropped: Option<(wp_shell::clipboard::Contents, i32, i32, bool)>,
    files: Option<(Vec<PathBuf>, i32, i32)>,
    effect: Option<wp_shell::DragEffect>,
}

/// A program that takes drops, or gives a drag once the button is held
/// and the pointer moves, and closes when it has what it came for.
struct Hand {
    canvas: Canvas,
    seen: std::rc::Rc<std::cell::RefCell<Handled>>,
    gives: Option<wp_shell::clipboard::Contents>,
    pressed: bool,
    started: Instant,
    done_at: Option<Instant>,
}

impl App for Hand {
    fn handle(&mut self, event: Event) -> Response {
        match event {
            Event::DataDragOver { x, y } => self.seen.borrow_mut().over.push((x, y)),
            Event::DataDropped { contents, x, y, copying } => {
                self.seen.borrow_mut().dropped = Some((contents, x, y, copying));
                self.done_at = Some(Instant::now());
            }
            Event::FilesDropped { paths, x, y } => {
                self.seen.borrow_mut().files = Some((paths, x, y));
                self.done_at = Some(Instant::now());
            }
            Event::MouseDown { .. } => self.pressed = true,
            Event::MouseMove { held: true, .. } if self.pressed => {
                self.pressed = false;
                if let Some(contents) = self.gives.take() {
                    let effect = wp_shell::start_drag(&contents);
                    self.seen.borrow_mut().effect = Some(effect);
                    self.done_at = Some(Instant::now());
                }
            }
            Event::Tick => {
                let settled =
                    self.done_at.is_some_and(|at| at.elapsed() > Duration::from_millis(1500));
                if settled || self.started.elapsed() > Duration::from_secs(40) {
                    return Response::Close;
                }
            }
            _ => {}
        }
        Response::Ignored
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.canvas = Canvas::new(width, height);
        self.canvas.clear(Color::WHITE);
        &self.canvas
    }
}

/// Runs this program's window, with the test's pointer making one drag
/// once the windows are up; what the program saw.
fn drag_on(
    compositor: &Compositor,
    title: &str,
    gives: Option<wp_shell::clipboard::Contents>,
    from: (i32, i32),
    to: (i32, i32),
) -> Handled {
    let mover = {
        let runtime = compositor.runtime.clone();
        let display = compositor.display.clone();
        std::thread::spawn(move || {
            let mut pointer = pointer::Pointer::new(&runtime, &display, 1000, 600)
                .expect("the compositor gives the test a pointer");
            // The window mapped, placed and drawn.
            std::thread::sleep(Duration::from_millis(2500));
            pointer.drag(from, to);
            std::thread::sleep(Duration::from_millis(3000));
        })
    };
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Handled::default()));
    let hand = Hand {
        canvas: Canvas::new(1, 1),
        seen: std::rc::Rc::clone(&seen),
        gives,
        pressed: false,
        started: Instant::now(),
        done_at: None,
    };
    let options = WindowOptions { title: format!("Hand {title}"), width: 400, height: 300 };
    wp_shell::run(options, Box::new(hand)).expect("the window opens on the compositor");
    mover.join().expect("the pointer moved");
    std::rc::Rc::try_unwrap(seen).ok().expect("the shell let go").into_inner()
}

/// What the peer said, until it has said nothing for a moment.
fn said(receiver: &std::sync::mpsc::Receiver<String>) -> Vec<String> {
    let mut lines = Vec::new();
    while let Ok(line) = receiver.recv_timeout(Duration::from_secs(3)) {
        lines.push(line);
    }
    lines
}

#[test]
fn text_dragged_from_another_program_is_dropped_on_the_window() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !gtk_is_here() {
        eprintln!("skipped: GTK is not on this machine");
        return;
    }
    let Some(compositor) = Compositor::start_with("drop-text", 1000, 600, PLACES) else {
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    let (mut peer, told) =
        peer(&compositor, &["source", "text", "dragged from GTK"]).expect("the peer is up");
    let seen = drag_on(&compositor, "taker", None, (650, 150), (200, 120));
    let told = said(&told);
    let _ = peer.kill();
    let _ = peer.wait();

    let (contents, x, y, _) = seen.dropped.expect("the drop arrived");
    assert_eq!(contents.text.as_deref(), Some("dragged from GTK"));
    assert_eq!((x, y), (200, 120), "where it was let go");
    assert!(!seen.over.is_empty(), "the place it would land was followed on the way");
    assert!(
        told.iter().any(|line| line.starts_with("action ") && line != "action none"),
        "and the giver was told it was taken: {told:?}"
    );
}

#[test]
fn a_file_dragged_from_another_program_arrives_as_its_path_on_the_window() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !gtk_is_here() {
        eprintln!("skipped: GTK is not on this machine");
        return;
    }
    let Some(compositor) = Compositor::start_with("drop-file", 1000, 600, PLACES) else {
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    let file = std::env::temp_dir().join(format!("wp wayland dnd {}.txt", std::process::id()));
    std::fs::write(&file, b"a file").expect("a file to drag");
    let (mut peer, _told) = peer(&compositor, &["source", "file", &file.display().to_string()])
        .expect("the peer is up");
    let seen = drag_on(&compositor, "taker of files", None, (650, 150), (150, 100));
    let _ = peer.kill();
    let _ = peer.wait();
    let _ = std::fs::remove_file(&file);

    let (paths, x, y) = seen.files.expect("the files arrived");
    assert_eq!(paths, vec![file]);
    assert_eq!((x, y), (150, 100));
}

#[test]
fn text_dragged_out_of_the_window_is_taken_by_another_program() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !gtk_is_here() {
        eprintln!("skipped: GTK is not on this machine");
        return;
    }
    let Some(compositor) = Compositor::start_with("drag-out", 1000, 600, PLACES) else {
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    let (mut peer, told) = peer(&compositor, &["target"]).expect("the peer is up");
    let contents = wp_shell::clipboard::Contents {
        text: Some("dragged out of the shell".to_owned()),
        ..wp_shell::clipboard::Contents::default()
    };
    let seen = drag_on(&compositor, "giver", Some(contents), (150, 150), (650, 150));
    let told = said(&told);
    let _ = peer.kill();
    let _ = peer.wait();

    assert!(
        told.contains(&"text dragged out of the shell".to_owned()),
        "the other program took the text: {told:?}"
    );
    assert!(
        matches!(seen.effect, Some(wp_shell::DragEffect::Move | wp_shell::DragEffect::Copy)),
        "and the drag says it was taken: {:?}",
        seen.effect
    );
}

#[test]
fn a_drag_let_go_on_its_own_window_says_where_on_the_compositor() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(compositor) = Compositor::start_with("drag-home", 1000, 600, PLACES) else {
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    let contents = wp_shell::clipboard::Contents {
        text: Some("stays home".to_owned()),
        ..wp_shell::clipboard::Contents::default()
    };
    let seen = drag_on(&compositor, "home", Some(contents), (60, 60), (300, 220));
    assert!(
        matches!(seen.effect, Some(wp_shell::DragEffect::DroppedOnSelf { x: 300, y: 220, .. })),
        "{:?}",
        seen.effect
    );
}

// --- What a screen reader is told -------------------------------------------

/// A window that says it has a toggle, a button and a document, and does
/// what it is asked; closes once the reader is done.
struct Readable {
    canvas: Canvas,
    invoked: std::rc::Rc<std::cell::RefCell<Vec<u64>>>,
    bold: bool,
    selection: (usize, usize),
    finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
    started: Instant,
}

impl App for Readable {
    fn handle(&mut self, event: Event) -> Response {
        let finished = self.finished.load(std::sync::atomic::Ordering::SeqCst);
        if event == Event::Tick && (finished || self.started.elapsed() > Duration::from_secs(60)) {
            return Response::Close;
        }
        Response::Ignored
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.canvas = Canvas::new(width, height);
        self.canvas.clear(Color::WHITE);
        &self.canvas
    }

    fn accessible_elements(&mut self) -> Vec<wp_shell::accessibility::Element> {
        use wp_shell::accessibility::{Element, Role};
        let element = |id: u64, role: Role, name: &str, selected: bool, focused: bool| Element {
            id,
            role,
            name: name.to_owned(),
            access_key: String::new(),
            rect: (100, 200, 60, 24),
            selected,
            enabled: true,
            focused,
            ..Element::default()
        };
        vec![
            element(3, Role::Toggle, "Bold", self.bold, false),
            element(4, Role::Button, "Save", false, false),
            element(5, Role::Document, "Document", false, true),
        ]
    }

    fn accessible_invoke(&mut self, id: u64) -> Response {
        self.invoked.borrow_mut().push(id);
        if id == 3 {
            self.bold = !self.bold;
        }
        Response::Redraw
    }

    fn accessible_text(&mut self) -> Option<wp_shell::accessibility::TextState> {
        Some(wp_shell::accessibility::TextState {
            text: "Hello world. Second paragraph".to_owned(),
            selection: self.selection,
        })
    }

    fn accessible_select(&mut self, start: usize, end: usize) -> Response {
        self.selection = (start, end);
        wp_shell::selection_changed();
        Response::Redraw
    }

    fn accessible_rects(&mut self, start: usize, end: usize) -> Vec<(i32, i32, i32, i32)> {
        (start..end).map(|at| (100 + 10 * at as i32, 200, 10, 20)).collect()
    }
}

#[test]
fn a_screen_reader_reads_the_window_on_the_compositor() {
    use std::io::BufRead;
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let atspi = Command::new("python3")
        .args([
            "-c",
            "import gi; gi.require_version('Atspi', '2.0'); from gi.repository import Atspi",
        ])
        .status()
        .is_ok_and(|status| status.success());
    let Ok(mut bus) = Command::new("dbus-daemon")
        .args(["--session", "--print-address=1", "--nofork", "--nopidfile"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        eprintln!("skipped: no session bus on this machine");
        return;
    };
    let mut address = String::new();
    let _ =
        std::io::BufReader::new(bus.stdout.take().expect("the address")).read_line(&mut address);
    let address = address.trim().to_owned();
    let stop_bus = |mut bus: Child| {
        let _ = bus.kill();
        let _ = bus.wait();
    };
    if !atspi {
        stop_bus(bus);
        eprintln!("skipped: no Atspi on this machine");
        return;
    }
    let Some(compositor) = Compositor::start("read", 800, 600) else {
        stop_bus(bus);
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &address);
    let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let finished = std::sync::Arc::clone(&finished);
        let address = address.clone();
        std::thread::spawn(move || {
            let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/atspi-reader.py");
            let output = Command::new("python3")
                .arg(script)
                .args(["Word Processor", "Bold"])
                .env("DBUS_SESSION_BUS_ADDRESS", &address)
                .stderr(Stdio::null())
                .output();
            finished.store(true, std::sync::atomic::Ordering::SeqCst);
            output.map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        })
    };
    let invoked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let readable = Readable {
        canvas: Canvas::new(1, 1),
        invoked: std::rc::Rc::clone(&invoked),
        bold: false,
        selection: (6, 11),
        finished,
        started: Instant::now(),
    };
    let options = WindowOptions { title: "Readable".to_owned(), width: 500, height: 300 };
    wp_shell::run(options, Box::new(readable)).expect("the window opens on the compositor");
    let said = reader.join().expect("the reader ran").expect("the reader's output");
    std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
    drop(compositor);
    stop_bus(bus);
    let has = |wanted: &str| said.lines().any(|line| line.starts_with(wanted));

    assert!(has("node 0 application | Word Processor"), "{said}");
    assert!(has("node 1 frame | Readable"), "{said}");
    assert!(has("node 2 toggle button | Bold"), "{said}");
    assert!(has("node 2 document text | Document | focused"), "{said}");
    assert!(has("text Hello world. Second paragraph"), "{said}");
    assert!(has("selection 6 11"), "{said}");
    assert!(has("word world 6 11"), "{said}");
    assert!(has("extents 100 200 10 20"), "{said}");
    assert!(has("checked yes"), "{said}");
    assert!(has("event caret 5"), "{said}");
    assert_eq!(*invoked.borrow(), vec![3]);
}

/// What a program saw of the desktop's portal while its window was up.
#[derive(Default)]
struct Portrayed {
    ticks: u32,
    offered: bool,
    screen: Option<(usize, usize, Vec<u8>)>,
    clipped: Option<(usize, usize, Vec<u8>)>,
}

/// A window of one colour that asks the desktop's portal for a picture of
/// the screen, then for a rectangle dragged out of it, and closes.
struct Portrait {
    canvas: Canvas,
    seen: std::rc::Rc<std::cell::RefCell<Portrayed>>,
    runtime: PathBuf,
    display: String,
}

/// The colour the window is painted, which the pictures are looked at for.
const PORTRAIT: (u8, u8, u8) = (200, 30, 90);

impl App for Portrait {
    fn handle(&mut self, event: Event) -> Response {
        let Event::Tick = event else { return Response::Ignored };
        let mut seen = self.seen.borrow_mut();
        seen.ticks += 1;
        let as_rgb = |shot: wp_shell::screen::Shot| {
            let rgb = shot.pixels.chunks_exact(4).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]);
            (shot.width, shot.height, rgb.collect())
        };
        if seen.ticks == 15 {
            seen.offered = wp_shell::screen::can_clip();
            seen.screen = wp_shell::screen::capture_screen().map(as_rgb);
        }
        if seen.ticks == 20 {
            // The rectangle is dragged by another client while this one
            // waits for the portal's answer, as a person's hand would drag
            // it: slurp draws over the screen and takes the pointer.
            let (runtime, display) = (self.runtime.clone(), self.display.clone());
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(2500));
                if let Ok(mut hand) = pointer::Pointer::new(&runtime, &display, 640, 480) {
                    hand.drag((50, 40), (250, 190));
                    std::thread::sleep(Duration::from_millis(3000));
                }
            });
            seen.clipped = wp_shell::screen::clip().map(as_rgb);
        }
        if seen.ticks >= 25 {
            return Response::Close;
        }
        Response::Ignored
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.canvas = Canvas::new(width, height);
        self.canvas.clear(Color::rgb(PORTRAIT.0, PORTRAIT.1, PORTRAIT.2));
        &self.canvas
    }
}

/// The session bus a desktop would have, with the environment the
/// portal's programs are started into when the first call wakes them: the
/// compositor to draw on and photograph, and which desktop this is, which
/// is how the portal chooses sway's half of itself.
fn portal_bus(compositor: &Compositor) -> Option<(Child, String)> {
    use std::io::BufRead;
    let mut bus = Command::new("dbus-daemon")
        .args(["--session", "--print-address=1", "--nofork", "--nopidfile"])
        .env("XDG_RUNTIME_DIR", &compositor.runtime)
        .env("WAYLAND_DISPLAY", &compositor.display)
        .env("XDG_CURRENT_DESKTOP", "sway")
        .env_remove("DISPLAY")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut address = String::new();
    let _ = std::io::BufReader::new(bus.stdout.take()?).read_line(&mut address);
    Some((bus, address.trim().to_owned()))
}

/// The screen photographed through the desktop's portal, and a rectangle
/// dragged out of it: the two things Word's Screenshot button does that a
/// Wayland client may not do for itself. Held to xdg-desktop-portal and
/// its wlr half, which photograph with grim and let the rectangle be
/// dragged with slurp — the rectangle dragged here by the test's own
/// pointer.
#[test]
fn the_screen_is_photographed_and_clipped_through_the_desktop_s_portal() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !Path::new("/usr/libexec/xdg-desktop-portal-wlr").exists() {
        eprintln!("skipped: no portal for sway on this machine");
        return;
    }
    let places = "for_window [title=\"^Portrait\"] floating enable, move absolute position 0 0, resize set 400 300\n";
    let Some(compositor) = Compositor::start_with("portal", 640, 480, places) else {
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    let Some((mut bus, address)) = portal_bus(&compositor) else {
        eprintln!("skipped: no session bus on this machine");
        return;
    };
    // The portal's screen recording half will not start without PipeWire.
    let pipewire = compositor
        .client("pipewire")
        .env("DBUS_SESSION_BUS_ADDRESS", &address)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    std::thread::sleep(Duration::from_millis(500));
    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &address);

    let seen = std::rc::Rc::new(std::cell::RefCell::new(Portrayed::default()));
    let portrait = Portrait {
        canvas: Canvas::new(1, 1),
        seen: std::rc::Rc::clone(&seen),
        runtime: compositor.runtime.clone(),
        display: compositor.display.clone(),
    };
    let options = WindowOptions { title: "Portrait".to_owned(), width: 400, height: 300 };
    wp_shell::run(options, Box::new(portrait)).expect("the window opens on the compositor");

    std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
    let _ = Command::new("pkill").arg("-f").arg("xdg-desktop-portal").status();
    if let Ok(mut pipewire) = pipewire {
        let _ = pipewire.kill();
        let _ = pipewire.wait();
    }
    let _ = bus.kill();
    let _ = bus.wait();
    drop(compositor);

    let seen = seen.borrow();
    assert!(seen.offered, "the portal did not say it takes screenshots");
    let (width, height, rgb) = seen.screen.as_ref().expect("the whole screen, from the portal");
    keep_proof("portal-screen", *width, *height, rgb);
    assert_eq!((*width, *height), (640, 480), "the whole of the compositor's output");
    let share = share_of(rgb, PORTRAIT);
    let expected = (400.0 * 300.0) / (640.0 * 480.0);
    assert!(
        (share - expected).abs() < 0.05,
        "the window is {share} of the picture, not {expected}"
    );

    let (width, height, rgb) = seen.clipped.as_ref().expect("the rectangle, from the portal");
    keep_proof("portal-clipping", *width, *height, rgb);
    assert!(
        (199..=202).contains(width) && (149..=152).contains(height),
        "the rectangle dragged out is 200 by 150, not {width} by {height}"
    );
    assert!(share_of(rgb, PORTRAIT) > 0.95, "the rectangle is of the window");
}

/// What sway says of the program's windows, one line each: whether each
/// is tiled or floating, how its parent lays it out, which parent that is,
/// and where it is — read by Python's own JSON reader, so that what is
/// checked is sway's account and not this program's reading of it.
fn sway_windows(compositor: &Compositor, socket: &Path, pid: u32) -> Vec<String> {
    let script = format!(
        "import json, sys
def walk(node, parent):
    if node.get('pid') == {pid} and node.get('type') in ('con', 'floating_con'):
        r = node['rect']
        print(node['type'], parent['layout'], parent['id'], r['x'], r['y'], r['width'], r['height'])
    for child in node.get('nodes', []) + node.get('floating_nodes', []):
        walk(child, node)
walk(json.load(sys.stdin), {{'layout': '-', 'id': 0}})"
    );
    let tree =
        compositor.client("swaymsg").args(["-t", "get_tree"]).env("SWAYSOCK", socket).output();
    let Ok(tree) = tree else { return Vec::new() };
    let mut python = Command::new("python3")
        .args(["-c", &script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("python");
    let _ = python.stdin.take().expect("its input").write_all(&tree.stdout);
    let output = python.wait_with_output().expect("python's answer");
    String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()
}

/// Two windows, scattered, and then side by side.
struct Arranging {
    canvas: Canvas,
    ticks: u32,
    compositor: (PathBuf, String),
    socket: PathBuf,
    before: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    after: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    arranged: std::rc::Rc<std::cell::Cell<usize>>,
}

impl App for Arranging {
    fn handle(&mut self, event: Event) -> Response {
        let Event::Tick = event else { return Response::Ignored };
        self.ticks += 1;
        let compositor = Compositor {
            // Not started here: the one already running, named so that its
            // own clients can be run.
            child: Command::new("true").spawn().expect("a process that does nothing"),
            runtime: self.compositor.0.clone(),
            display: self.compositor.1.clone(),
        };
        let pid = std::process::id();
        match self.ticks {
            2 => assert!(wp_shell::open_window("Arranging, second")),
            12 => {
                // Scattered: both floating, one over the other.
                let _ = compositor
                    .client("swaymsg")
                    .arg(format!("[pid={pid}] floating enable"))
                    .env("SWAYSOCK", &self.socket)
                    .stdout(Stdio::null())
                    .status();
            }
            20 => *self.before.borrow_mut() = sway_windows(&compositor, &self.socket, pid),
            22 => self.arranged.set(wp_shell::arrange_windows()),
            30 => *self.after.borrow_mut() = sway_windows(&compositor, &self.socket, pid),
            _ => {}
        }
        core::mem::forget(compositor);
        if self.ticks >= 32 {
            return Response::Close;
        }
        Response::Ignored
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.canvas = Canvas::new(width, height);
        self.canvas.clear(Color::rgb(90, 160, 60));
        &self.canvas
    }
}

/// Word's Arrange All on sway: the program's windows, floating one over
/// the other, made tiled and laid side by side across the screen — asked
/// of sway in its own language, since neither the protocol nor the portal
/// lets a client place its windows, and read back from sway by swaymsg.
#[test]
fn the_windows_are_put_side_by_side_by_asking_sway() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(compositor) = Compositor::start("arrange", 800, 600) else {
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    // Sway names its socket for itself, in the runtime folder.
    let socket = std::fs::read_dir(&compositor.runtime)
        .ok()
        .and_then(|entries| {
            entries.flatten().map(|entry| entry.path()).find(|path| {
                path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("sway-ipc."))
            })
        })
        .expect("sway's socket");
    std::env::set_var("SWAYSOCK", &socket);

    let before = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let after = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let arranged = std::rc::Rc::new(std::cell::Cell::new(0));
    let arranging = Arranging {
        canvas: Canvas::new(1, 1),
        ticks: 0,
        compositor: (compositor.runtime.clone(), compositor.display.clone()),
        socket: socket.clone(),
        before: std::rc::Rc::clone(&before),
        after: std::rc::Rc::clone(&after),
        arranged: std::rc::Rc::clone(&arranged),
    };
    let options = WindowOptions { title: "Arranging".to_owned(), width: 500, height: 300 };
    wp_shell::run(options, Box::new(arranging)).expect("the window opens on the compositor");
    std::env::remove_var("SWAYSOCK");
    drop(compositor);

    let (before, after) = (before.borrow(), after.borrow());
    assert_eq!(before.len(), 2, "two windows: {before:?}");
    assert!(before.iter().all(|line| line.starts_with("floating_con")), "scattered: {before:?}");
    assert_eq!(arranged.get(), 2, "both were arranged");
    assert_eq!(after.len(), 2, "{after:?}");
    let fields: Vec<Vec<&str>> = after.iter().map(|line| line.split(' ').collect()).collect();
    for window in &fields {
        assert_eq!(window[0], "con", "tiled: {after:?}");
        assert_eq!(window[1], "splith", "laid out across: {after:?}");
    }
    assert_eq!(fields[0][2], fields[1][2], "in one container: {after:?}");
    let number = |text: &str| text.parse::<i32>().expect("a number");
    let (left, right) = (&fields[0], &fields[1]);
    assert_eq!(
        number(left[4]),
        number(right[4]),
        "side by side, not one above the other: {after:?}"
    );
    assert_eq!(
        number(left[3]) + number(left[5]),
        number(right[3]),
        "the second where the first ends"
    );
    assert!((number(left[5]) - number(right[5])).abs() <= 2, "the same width: {after:?}");
    assert_eq!(number(left[5]) + number(right[5]), 800, "across the whole screen: {after:?}");
}

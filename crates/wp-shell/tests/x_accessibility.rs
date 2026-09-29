//! What a screen reader on Linux is told, read by a screen reader's library.
//!
//! A session bus of the test's own, on which the desktop's accessibility bus
//! is started as a desktop starts it; the X shell's window on Xvfb, saying
//! what it has; and `tools/atspi-reader.py`, which reads it through Atspi —
//! the library Orca is written on — as a screen reader would: the tree, the
//! document's text and caret and selection, a word, a character's place, a
//! button pressed, a selection made and the caret's move heard. Where the
//! machine has no Xvfb, no bus or no Atspi the test passes without proving
//! anything, and says so.
#![cfg(target_os = "linux")]

use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use wp_raster::{Canvas, Color};
use wp_shell::accessibility::{Element, Role, TextState};
use wp_shell::{App, Event, Response, WindowOptions};

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// What the program was asked to do.
#[derive(Default)]
struct Asked {
    invoked: Vec<u64>,
    selected: Vec<(usize, usize)>,
}

/// A window that says it has two tabs, a toggle, a button, a document and a
/// line of words, and does what it is asked.
struct Readable {
    canvas: Canvas,
    asked: Rc<RefCell<Asked>>,
    bold: bool,
    selection: (usize, usize),
    finished: Arc<AtomicBool>,
    started: Instant,
}

const TEXT: &str = "Hello world. Second paragraph\nThird line";

impl App for Readable {
    fn handle(&mut self, event: Event) -> Response {
        if event == Event::Tick
            && (self.finished.load(Ordering::SeqCst)
                || self.started.elapsed() > Duration::from_secs(60))
        {
            return Response::Close;
        }
        Response::Ignored
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.canvas = Canvas::new(width, height);
        self.canvas.clear(Color::WHITE);
        &self.canvas
    }

    fn accessible_elements(&mut self) -> Vec<Element> {
        let element =
            |id: u64, role: Role, name: &str, key: &str, x: i32, selected: bool| Element {
                id,
                role,
                name: name.to_owned(),
                access_key: key.to_owned(),
                rect: (x, 10, 60, 24),
                selected,
                enabled: true,
                focused: false,
            };
        vec![
            element(1, Role::TabItem, "Home", "H", 10, true),
            element(2, Role::TabItem, "Insert", "N", 80, false),
            element(3, Role::Toggle, "Bold", "1", 10, self.bold),
            element(4, Role::Button, "Save", "S", 80, false),
            Element {
                id: 5,
                role: Role::Document,
                name: "Document".to_owned(),
                access_key: String::new(),
                rect: (100, 200, 300, 60),
                selected: false,
                enabled: true,
                focused: true,
            },
            element(6, Role::Text, "Page 1 of 1", "", 10, false),
        ]
    }

    fn accessible_invoke(&mut self, id: u64) -> Response {
        self.asked.borrow_mut().invoked.push(id);
        if id == 3 {
            self.bold = !self.bold;
        }
        Response::Redraw
    }

    fn accessible_text(&mut self) -> Option<TextState> {
        Some(TextState { text: TEXT.to_owned(), selection: self.selection })
    }

    fn accessible_select(&mut self, start: usize, end: usize) -> Response {
        self.asked.borrow_mut().selected.push((start, end));
        self.selection = (start, end);
        wp_shell::selection_changed();
        Response::Redraw
    }

    fn accessible_rects(&mut self, start: usize, end: usize) -> Vec<(i32, i32, i32, i32)> {
        (start..end).map(|at| (100 + 10 * at as i32, 200, 10, 20)).collect()
    }
}

#[test]
fn a_screen_reader_reads_the_window_presses_its_button_and_hears_the_caret() {
    let display = ":90";
    let socket = "/tmp/.X11-unix/X90";
    let _ = std::fs::remove_file("/tmp/.X90-lock");
    let _ = std::fs::remove_file(socket);
    let Ok(server) = Command::new("Xvfb")
        .args([display, "-screen", "0", "800x600x24", "-nolisten", "tcp"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    let _server = Running(server);
    let started = Instant::now();
    while !Path::new(socket).exists() && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let atspi = Command::new("python3")
        .args([
            "-c",
            "import gi; gi.require_version('Atspi', '2.0'); from gi.repository import Atspi",
        ])
        .status()
        .is_ok_and(|status| status.success());
    // A session bus of the test's own, which starts the accessibility bus
    // when it is asked for it, as a desktop's does.
    let bus = Command::new("dbus-daemon")
        .args(["--session", "--print-address=1", "--nofork", "--nopidfile"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let (Ok(mut bus), true) = (bus, atspi) else {
        eprintln!("skipped: no session bus or no Atspi on this machine");
        return;
    };
    let mut address = String::new();
    let _ = BufReader::new(bus.stdout.take().expect("the bus's address")).read_line(&mut address);
    let _bus = Running(bus);
    let address = address.trim().to_owned();
    std::env::remove_var("WAYLAND_DISPLAY");
    std::env::remove_var("XMODIFIERS");
    std::env::set_var("DISPLAY", display);
    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &address);

    let finished = Arc::new(AtomicBool::new(false));
    let reader = {
        let finished = Arc::clone(&finished);
        std::thread::spawn(move || {
            let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/atspi-reader.py");
            let output = Command::new("python3")
                .arg(script)
                .args(["Word Processor", "Bold"])
                .env("DBUS_SESSION_BUS_ADDRESS", &address)
                .env("DISPLAY", display)
                .stderr(Stdio::null())
                .output();
            finished.store(true, Ordering::SeqCst);
            output.map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        })
    };
    let asked = Rc::new(RefCell::new(Asked::default()));
    let readable = Readable {
        canvas: Canvas::new(1, 1),
        asked: Rc::clone(&asked),
        bold: false,
        selection: (6, 11),
        finished,
        started: Instant::now(),
    };
    let options = WindowOptions { title: "Readable".to_owned(), width: 500, height: 300 };
    wp_shell::run(options, Box::new(readable)).expect("the window opens on the virtual server");
    let said = reader.join().expect("the reader ran").expect("the reader's output");
    std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
    let lines: Vec<&str> = said.lines().collect();
    let has = |wanted: &str| lines.iter().any(|line| line.starts_with(wanted));

    assert!(!has("missing"), "the application is on the desktop: {said}");
    assert!(has("node 0 application | Word Processor"), "{said}");
    assert!(has("node 1 frame | Readable"), "the window, by its title: {said}");
    assert!(has("node 2 page tab | Home | selected"), "a tab, chosen: {said}");
    assert!(has("node 2 page tab | Insert | enabled"), "and one not: {said}");
    assert!(has("node 2 toggle button | Bold | enabled"), "a toggle, off: {said}");
    assert!(has("node 2 push button | Save"), "a button: {said}");
    assert!(
        has("node 2 document text | Document | focused,enabled,editable,multi_line"),
        "the document, with the keyboard: {said}"
    );
    assert!(has("node 2 label | Page 1 of 1"), "and the words: {said}");
    assert!(has(&format!("text {}", TEXT.lines().next().unwrap())), "its text: {said}");
    assert!(has("caret 11"), "the caret at the selection's end: {said}");
    assert!(has("selection 6 11"), "the selection: {said}");
    assert!(has("word world 6 11"), "the word at an offset: {said}");
    assert!(has("extents 100 200 10 20"), "a character's place in the window: {said}");
    assert!(has("action press"), "{said}");
    assert!(has("checked yes"), "pressed, the toggle is on, and the reader knows: {said}");
    assert!(has("event caret 5"), "the caret's move after a selection is heard: {said}");
    assert!(has("done"), "{said}");

    let asked = asked.borrow();
    assert_eq!(asked.invoked, vec![3], "Bold was pressed, and nothing else");
    assert_eq!(asked.selected, vec![(0, 5)], "the reader selected the first word");
}

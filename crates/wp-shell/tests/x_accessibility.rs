//! What a screen reader on Linux is told, read by a screen reader's library.
//!
//! A session bus of the test's own, on which the desktop's accessibility bus
//! is started as a desktop starts it; the X shell's window on Xvfb, saying
//! what it has; and `tools/atspi-reader.py`, which reads it through Atspi —
//! the library Orca is written on — as a screen reader would: the tree with
//! what each control holds, the document's text and caret and selection, a
//! word, a line as the layout broke it, how a stretch is set, a character's
//! place, a button pressed, a box written, a selection made and the caret's
//! move heard, and a dialog opened with the message it leaves on the status
//! strip. Where the machine has no Xvfb, no bus or no Atspi the test passes
//! without proving anything, and says so.
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
use wp_shell::accessibility::{Element, Role, TextAttributes, TextState};
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
    written: Vec<(u64, String)>,
}

/// A window that says it has tabs, a toggle, buttons, a box, a list box, a
/// ruler, a document, a scroll bar, a pane with a list, a status strip —
/// and, once Save is pressed, a dialog — and does what it is asked.
struct Readable {
    canvas: Canvas,
    asked: Rc<RefCell<Asked>>,
    bold: bool,
    selection: (usize, usize),
    indent: String,
    saving: bool,
    status: String,
    finished: Arc<AtomicBool>,
    started: Instant,
}

const TEXT: &str = "Hello world. Second paragraph\nThird line";

/// The ids, so that pressing and writing can be told apart.
const SAVE: u64 = 4;
const INDENT: u64 = 7;
const DIALOG: u64 = 20;

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
        let element = |id: u64, role: Role, name: &str, x: i32, y: i32| Element {
            id,
            role,
            name: name.to_owned(),
            rect: (x, y, 60, 24),
            enabled: true,
            ..Element::default()
        };
        let mut elements = vec![
            Element {
                selected: true,
                access_key: "H".to_owned(),
                ..element(1, Role::TabItem, "Home", 10, 10)
            },
            element(2, Role::TabItem, "Insert", 80, 10),
            Element {
                selected: self.bold,
                access_key: "1".to_owned(),
                ..element(3, Role::Toggle, "Bold", 10, 40)
            },
            element(SAVE, Role::Button, "Save", 80, 40),
            Element { value: "Calibri".to_owned(), ..element(6, Role::ComboBox, "Font", 150, 40) },
            Element {
                value: self.indent.clone(),
                ..element(INDENT, Role::Edit, "Left indent", 220, 40)
            },
            element(8, Role::Ruler, "Horizontal ruler", 100, 180),
            Element {
                rect: (100, 200, 300, 60),
                focused: !self.saving,
                ..element(5, Role::Document, "Document", 0, 0)
            },
            Element {
                range: Some((0.0, 100.0, 25.0)),
                ..element(9, Role::ScrollBar, "Vertical scroll bar", 480, 200)
            },
            element(10, Role::Pane, "Navigation", 0, 200),
            Element { parent: Some(10), ..element(11, Role::List, "Headings", 0, 220) },
            Element {
                parent: Some(11),
                selected: true,
                ..element(12, Role::ListItem, "Introduction", 0, 240)
            },
            Element { parent: Some(11), ..element(13, Role::ListItem, "Method", 0, 260) },
            Element {
                value: self.status.clone(),
                ..element(14, Role::StatusBar, "Status bar", 0, 280)
            },
        ];
        if self.saving {
            elements.push(element(DIALOG, Role::Dialog, "Save As", 100, 60));
            elements.push(Element {
                parent: Some(DIALOG),
                value: "Letter.docx".to_owned(),
                focused: true,
                ..element(21, Role::Edit, "File name", 110, 80)
            });
            elements.push(Element {
                parent: Some(DIALOG),
                selected: true,
                ..element(22, Role::CheckBox, "Keep a copy", 110, 110)
            });
            elements.push(Element {
                parent: Some(DIALOG),
                ..element(23, Role::Button, "OK", 110, 140)
            });
        }
        elements
    }

    fn accessible_invoke(&mut self, id: u64) -> Response {
        self.asked.borrow_mut().invoked.push(id);
        if id == 3 {
            self.bold = !self.bold;
        }
        if id == SAVE {
            self.saving = true;
            "Saved".clone_into(&mut self.status);
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

    fn accessible_lines(&mut self) -> Vec<(usize, usize)> {
        vec![(0, 13), (13, 30), (30, 40)]
    }

    fn accessible_attributes(&mut self, offset: usize) -> Option<(TextAttributes, usize, usize)> {
        let bold = TextAttributes {
            font: "Calibri".to_owned(),
            size: 11.0,
            bold: true,
            color: Some((192, 0, 0)),
            ..TextAttributes::default()
        };
        let plain =
            TextAttributes { font: "Calibri".to_owned(), size: 11.0, ..TextAttributes::default() };
        Some(if offset < 5 { (bold, 0, 5) } else { (plain, 5, 40) })
    }

    fn accessible_set_value(&mut self, id: u64, value: &str) -> Response {
        self.asked.borrow_mut().written.push((id, value.to_owned()));
        if id == INDENT {
            value.clone_into(&mut self.indent);
        }
        Response::Redraw
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
                .args(["Word Processor", "Bold", "Save", "Left indent", "2 cm"])
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
        indent: "0 cm".to_owned(),
        saving: false,
        status: "Ready".to_owned(),
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
        has("node 2 combo box | Font | enabled | Calibri"),
        "a list box and its choice: {said}"
    );
    assert!(
        has("node 2 entry | Left indent | enabled,editable | 0 cm"),
        "a box and its text: {said}"
    );
    assert!(has("node 2 ruler | Horizontal ruler"), "a ruler: {said}");
    assert!(
        has("node 2 document text | Document | focused,enabled,editable,multi_line"),
        "the document, with the keyboard: {said}"
    );
    assert!(has("node 2 scroll bar | Vertical scroll bar | enabled | 25"), "where it is: {said}");
    assert!(has("node 2 panel | Navigation"), "a pane: {said}");
    assert!(has("node 3 list | Headings"), "with its list inside it: {said}");
    assert!(has("node 4 list item | Introduction | selected"), "and the rows inside that: {said}");
    assert!(has("node 4 list item | Method | enabled"), "{said}");
    assert!(has("node 2 status bar | Status bar | enabled | Ready"), "the status strip: {said}");
    assert!(has(&format!("text {}", TEXT.lines().next().unwrap())), "its text: {said}");
    assert!(has("caret 11"), "the caret at the selection's end: {said}");
    assert!(has("selection 6 11"), "the selection: {said}");
    assert!(has("word world 6 11"), "the word at an offset: {said}");
    assert!(has("line 13 30"), "the line as the layout broke it: {said}");
    assert!(
        has("attributes 0 5 family-name=Calibri,fg-color=192,0,0,size=11,strikethrough=false,style=normal,underline=none,weight=700"),
        "how the first word is set: {said}"
    );
    assert!(has("extents 100 200 10 20"), "a character's place in the window: {said}");
    assert!(has("action press"), "{said}");
    assert!(has("checked yes"), "pressed, the toggle is on, and the reader knows: {said}");
    assert!(has("event caret 5"), "the caret's move after a selection is heard: {said}");
    assert!(has("written 2 cm"), "a box written, and read back: {said}");
    assert!(has("event window activate Save As"), "a dialog opening is heard: {said}");
    assert!(has("event focused File name"), "and the keyboard going into it: {said}");
    assert!(has("event text insert Saved"), "and the status strip's message: {said}");
    assert!(has("dialog 1 entry | File name | focused,enabled,editable | Letter.docx"), "{said}");
    assert!(has("dialog 1 check box | Keep a copy | checked"), "{said}");
    assert!(has("dialog 1 push button | OK"), "{said}");
    assert!(has("done"), "{said}");

    let asked = asked.borrow();
    assert_eq!(asked.invoked, vec![3, SAVE], "Bold and Save were pressed, and nothing else");
    assert_eq!(asked.selected, vec![(0, 5)], "the reader selected the first word");
    assert_eq!(asked.written, vec![(INDENT, "2 cm".to_owned())], "and wrote into the box");
}

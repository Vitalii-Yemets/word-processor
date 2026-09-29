//! Dragging and dropping between the X shell and another program.
//!
//! The other program is GTK's — `tools/dnd-peer.py`, a window that gives a
//! drag or takes a drop and says what it was — so the protocol on the
//! other end is GTK's reading of XDND, not this program's. xdotool moves
//! the pointer and presses the button through the X server's test
//! extension, as a person's hand would. Where the machine has no Xvfb,
//! xdotool or GTK the tests pass without proving anything, and say so.
#![cfg(target_os = "linux")]

use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use wp_raster::{Canvas, Color};
use wp_shell::clipboard::Contents;
use wp_shell::{App, DragEffect, Event, Response, WindowOptions};

/// One test on a display at a time: the display is the whole program's.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A process for the length of a test.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_server(number: u32) -> Option<Running> {
    let display = format!(":{number}");
    let socket = format!("/tmp/.X11-unix/X{number}");
    let _ = std::fs::remove_file(format!("/tmp/.X{number}-lock"));
    let _ = std::fs::remove_file(&socket);
    let child = Command::new("Xvfb")
        .args([&display, "-screen", "0", "1000x600x24", "-nolisten", "tcp"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let server = Running(child);
    let started = Instant::now();
    while !Path::new(&socket).exists() {
        if started.elapsed() > Duration::from_secs(10) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    std::env::remove_var("WAYLAND_DISPLAY");
    std::env::remove_var("XMODIFIERS");
    std::env::set_var("DISPLAY", &display);
    Some(server)
}

/// The peer, at the screen's right, and what it says, line by line.
fn peer(arguments: &[&str]) -> Option<(Running, mpsc::Receiver<String>)> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/dnd-peer.py");
    let mut child = Command::new("python3")
        .arg(script)
        .args(arguments)
        .env("NO_AT_BRIDGE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let out = child.stdout.take()?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = sender.send(line);
        }
    });
    let running = Running(child);
    // Up, or not there at all.
    let first = receiver.recv_timeout(Duration::from_secs(15)).ok()?;
    (first == "ready").then_some((running, receiver))
}

fn tools_are_here() -> bool {
    let xdotool = Command::new("xdotool").arg("version").output().is_ok();
    let gtk = Command::new("python3")
        .args(["-c", "import gi; gi.require_version('Gtk', '3.0'); from gi.repository import Gtk"])
        .status()
        .is_ok_and(|status| status.success());
    xdotool && gtk
}

/// Moves the pointer from one point to another with the button held, a
/// step at a time, as a hand does — and lets go.
fn drag(from: (i32, i32), to: (i32, i32)) {
    let run = |arguments: &[String]| {
        let _ = Command::new("xdotool").args(arguments).output();
    };
    run(&["mousemove".into(), from.0.to_string(), from.1.to_string()]);
    std::thread::sleep(Duration::from_millis(300));
    run(&["mousedown".into(), "1".into()]);
    for step in 1..=12 {
        let x = from.0 + (to.0 - from.0) * step / 12;
        let y = from.1 + (to.1 - from.1) * step / 12;
        std::thread::sleep(Duration::from_millis(80));
        run(&["mousemove".into(), x.to_string(), y.to_string()]);
    }
    std::thread::sleep(Duration::from_millis(400));
    run(&["mouseup".into(), "1".into()]);
}

/// Waits for the shell's window to be on the screen.
fn window_is_up(title: &str) {
    let _ = Command::new("xdotool").args(["search", "--sync", "--name", title]).output();
    std::thread::sleep(Duration::from_millis(500));
}

/// What the program saw.
#[derive(Default)]
struct Seen {
    over: Vec<(i32, i32)>,
    left: u32,
    dropped: Option<(Contents, i32, i32, bool)>,
    files: Option<(Vec<PathBuf>, i32, i32)>,
    effect: Option<DragEffect>,
}

/// A program that takes drops, or gives a drag once the button is held
/// and the pointer moves, and closes when it has what it came for.
struct Hand {
    canvas: Canvas,
    seen: Rc<RefCell<Seen>>,
    gives: Option<Contents>,
    pressed: bool,
    started: Instant,
    done_at: Option<Instant>,
}

impl Hand {
    fn new(seen: &Rc<RefCell<Seen>>, gives: Option<Contents>) -> Self {
        Self {
            canvas: Canvas::new(1, 1),
            seen: Rc::clone(seen),
            gives,
            pressed: false,
            started: Instant::now(),
            done_at: None,
        }
    }
}

impl App for Hand {
    fn handle(&mut self, event: Event) -> Response {
        match event {
            Event::DataDragOver { x, y } => self.seen.borrow_mut().over.push((x, y)),
            Event::DataDragLeft => self.seen.borrow_mut().left += 1,
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

/// Runs the shell's window with a hand on the mouse meanwhile.
fn run_with(title: &str, hand: Hand, moves: impl FnOnce() + Send + 'static) {
    let title_owned = title.to_owned();
    let mover = std::thread::spawn(move || {
        window_is_up(&title_owned);
        moves();
    });
    let options = WindowOptions { title: title.to_owned(), width: 400, height: 300 };
    wp_shell::run(options, Box::new(hand)).expect("the window opens on the virtual server");
    mover.join().expect("the pointer was moved");
}

/// What the peer said, until it has said nothing for a moment.
fn said(receiver: &mpsc::Receiver<String>) -> Vec<String> {
    let mut lines = Vec::new();
    while let Ok(line) = receiver.recv_timeout(Duration::from_secs(3)) {
        lines.push(line);
    }
    lines
}

#[test]
fn text_dragged_from_another_program_is_dropped_where_it_is_let_go() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(_server) = start_server(84) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    if !tools_are_here() {
        eprintln!("skipped: xdotool or GTK is not on this machine");
        return;
    }
    let (_peer, told) = peer(&["source", "text", "dragged from GTK"]).expect("the peer is up");
    let seen = Rc::new(RefCell::new(Seen::default()));
    run_with("Taker", Hand::new(&seen, None), || drag((650, 150), (200, 120)));

    let seen = seen.borrow();
    let (contents, x, y, copying) = seen.dropped.clone().expect("the drop arrived");
    assert_eq!(contents.text.as_deref(), Some("dragged from GTK"));
    assert_eq!((x, y), (200, 120), "where it was let go");
    assert!(copying, "GTK asks for a copy unless told otherwise");
    assert!(!seen.over.is_empty(), "the place it would land was followed on the way");
    assert!(seen.over.iter().all(|(x, _)| *x < 400), "and only while it was over the window");
    let told = said(&told);
    assert!(told.contains(&"action copy".to_owned()), "and the giver was told: {told:?}");
}

#[test]
fn a_file_dragged_from_another_program_arrives_as_its_path() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(_server) = start_server(85) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    if !tools_are_here() {
        eprintln!("skipped: xdotool or GTK is not on this machine");
        return;
    }
    let file = std::env::temp_dir().join(format!("wp dnd {}.txt", std::process::id()));
    std::fs::write(&file, b"a file").expect("a file to drag");
    let (_peer, _told) =
        peer(&["source", "file", &file.display().to_string()]).expect("the peer is up");
    let seen = Rc::new(RefCell::new(Seen::default()));
    run_with("Taker of files", Hand::new(&seen, None), || drag((650, 150), (150, 100)));
    let _ = std::fs::remove_file(&file);

    let seen = seen.borrow();
    let (paths, x, y) = seen.files.clone().expect("the files arrived");
    assert_eq!(paths, vec![file], "the path, its space unescaped");
    assert_eq!((x, y), (150, 100));
    assert!(seen.dropped.is_none() && seen.over.is_empty(), "files are not data");
}

#[test]
fn text_dragged_out_is_taken_by_another_program_as_a_move() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(_server) = start_server(86) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    if !tools_are_here() {
        eprintln!("skipped: xdotool or GTK is not on this machine");
        return;
    }
    let (_peer, told) = peer(&["target"]).expect("the peer is up");
    let seen = Rc::new(RefCell::new(Seen::default()));
    let contents = Contents {
        text: Some("dragged out of the shell".to_owned()),
        html: Some(b"<p>dragged out of the shell</p>".to_vec()),
        ..Contents::default()
    };
    run_with("Giver", Hand::new(&seen, Some(contents)), || drag((150, 150), (650, 150)));

    let told = said(&told);
    assert!(
        told.contains(&"text dragged out of the shell".to_owned()),
        "the other program took the text: {told:?}"
    );
    assert_eq!(
        seen.borrow().effect,
        Some(DragEffect::Move),
        "and said it moved it, so the original is to come out"
    );
}

#[test]
fn a_drag_let_go_on_its_own_window_says_where() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(_server) = start_server(87) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    if Command::new("xdotool").arg("version").output().is_err() {
        eprintln!("skipped: xdotool is not on this machine");
        return;
    }
    let seen = Rc::new(RefCell::new(Seen::default()));
    let contents = Contents { text: Some("stays home".to_owned()), ..Contents::default() };
    run_with("Home", Hand::new(&seen, Some(contents)), || drag((60, 60), (300, 220)));
    assert_eq!(
        seen.borrow().effect,
        Some(DragEffect::DroppedOnSelf { x: 300, y: 220, copying: false })
    );
}

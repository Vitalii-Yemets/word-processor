//! The X11 shell's clipboard against another program's, for what is too big
//! to go in one piece.
//!
//! A selection bigger than one property should carry goes in pieces —
//! `INCR`, in the conventions' own word: the owner says how big it is, and
//! writes a piece each time the program asking takes the last. xclip is the
//! other program, and both ways round are held to it: a selection xclip
//! hands over in pieces taken whole, and one this program hands over in
//! pieces taken whole by xclip. Where the machine has no Xvfb or no xclip
//! the tests pass without proving anything, and say so on the way out.
#![cfg(target_os = "linux")]

use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use wp_raster::{Canvas, Color};
use wp_shell::{App, Event, Response, WindowOptions};

/// One test on the display at a time: which display a program talks to is
/// named by the environment, and the environment is the whole program's.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// The display the tests here use.
const DISPLAY: &str = ":95";

/// A virtual X server for the length of a test.
struct Server {
    child: Child,
}

impl Server {
    fn start() -> Option<Self> {
        let socket = "/tmp/.X11-unix/X95";
        let _ = std::fs::remove_file("/tmp/.X95-lock");
        let _ = std::fs::remove_file(socket);
        let child = Command::new("Xvfb")
            .args([DISPLAY, "-screen", "0", "640x480x24", "-nolisten", "tcp"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut server = Self { child };
        let started = Instant::now();
        while !Path::new(socket).exists() {
            if started.elapsed() > Duration::from_secs(10) {
                server.stop();
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::set_var("DISPLAY", DISPLAY);
        Some(server)
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

fn xclip_is_here() -> bool {
    Command::new("xclip").arg("-version").stderr(Stdio::null()).status().is_ok()
}

/// Three megabytes of text, every line different, so that a piece lost,
/// doubled or put in the wrong place shows.
fn big_text() -> String {
    (0..60_000).map(|line| format!("Line {line:06} of the text on the clipboard.\n")).collect()
}

/// A window that, on a given tick, does one thing with the clipboard, and
/// closes once told that what else was going on has finished.
struct Clipboarding {
    canvas: Canvas,
    ticks: u32,
    on_tick: Box<dyn FnMut(u32)>,
    finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
    started: Instant,
}

impl App for Clipboarding {
    fn handle(&mut self, event: Event) -> Response {
        let Event::Tick = event else { return Response::Ignored };
        self.ticks += 1;
        (self.on_tick)(self.ticks);
        let finished = self.finished.load(std::sync::atomic::Ordering::SeqCst);
        if finished || self.started.elapsed() > Duration::from_secs(60) {
            return Response::Close;
        }
        Response::Ignored
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.canvas = Canvas::new(width, height);
        self.canvas.clear(Color::rgb(250, 250, 250));
        &self.canvas
    }
}

#[test]
fn a_big_selection_another_program_hands_over_in_pieces_is_taken_whole() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !xclip_is_here() {
        eprintln!("skipped: no xclip on this machine");
        return;
    }
    let Some(_server) = Server::start() else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    let text = big_text();
    // xclip owns the clipboard with it, and stays to hand it over; a
    // selection this big it hands over in pieces.
    let mut owner = Command::new("xclip")
        .args(["-selection", "clipboard", "-i", "-quiet"])
        .env("DISPLAY", DISPLAY)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("xclip");
    owner.stdin.take().expect("its input").write_all(text.as_bytes()).expect("writing");
    std::thread::sleep(Duration::from_millis(500));

    let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let taken = std::rc::Rc::new(std::cell::RefCell::new(None));
    let on_tick = {
        let (finished, taken) = (std::sync::Arc::clone(&finished), std::rc::Rc::clone(&taken));
        Box::new(move |tick: u32| {
            if tick == 5 {
                *taken.borrow_mut() = Some(wp_shell::clipboard::text());
                finished.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        })
    };
    let window = Clipboarding {
        canvas: Canvas::new(1, 1),
        ticks: 0,
        on_tick,
        finished,
        started: Instant::now(),
    };
    let options = WindowOptions { title: "Clipboard".to_owned(), width: 320, height: 200 };
    wp_shell::run(options, Box::new(window)).expect("the window opens");
    let _ = owner.kill();
    let _ = owner.wait();

    let taken = taken.borrow_mut().take().expect("the clipboard was asked");
    let taken = taken.expect("the clipboard held something");
    assert_eq!(taken.len(), text.len(), "all of it, and no more");
    assert!(taken == text, "every piece in its place");
}

#[test]
fn a_big_selection_this_program_hands_over_in_pieces_is_taken_whole() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !xclip_is_here() {
        eprintln!("skipped: no xclip on this machine");
        return;
    }
    let Some(_server) = Server::start() else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    let text = big_text();
    let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (sender, received) = std::sync::mpsc::channel();
    let on_tick = {
        let text = text.clone();
        let finished = std::sync::Arc::clone(&finished);
        Box::new(move |tick: u32| {
            if tick == 5 {
                assert!(wp_shell::clipboard::set_text(&text));
            }
            if tick == 8 {
                // Taken by another program while this one goes on
                // answering: the pieces are written as the window's loop
                // hears each one taken.
                let (finished, sender) = (std::sync::Arc::clone(&finished), sender.clone());
                std::thread::spawn(move || {
                    let taken = Command::new("xclip")
                        .args(["-selection", "clipboard", "-o"])
                        .env("DISPLAY", DISPLAY)
                        .stderr(Stdio::null())
                        .output();
                    let targets = Command::new("xclip")
                        .args(["-selection", "clipboard", "-o", "-t", "TARGETS"])
                        .env("DISPLAY", DISPLAY)
                        .stderr(Stdio::null())
                        .output();
                    let _ = sender.send((taken, targets));
                    finished.store(true, std::sync::atomic::Ordering::SeqCst);
                });
            }
        })
    };
    let window = Clipboarding {
        canvas: Canvas::new(1, 1),
        ticks: 0,
        on_tick,
        finished,
        started: Instant::now(),
    };
    let options = WindowOptions { title: "Clipboard".to_owned(), width: 320, height: 200 };
    wp_shell::run(options, Box::new(window)).expect("the window opens");

    let (taken, targets) = received.recv_timeout(Duration::from_secs(5)).expect("xclip ran");
    let taken = taken.expect("xclip's output");
    assert!(taken.status.success(), "xclip took it");
    assert_eq!(taken.stdout.len(), text.len(), "all of it, and no more");
    assert!(taken.stdout == text.as_bytes(), "every piece in its place");
    let targets = String::from_utf8_lossy(&targets.expect("the targets").stdout).into_owned();
    assert!(targets.lines().any(|line| line == "UTF8_STRING"), "{targets}");
}

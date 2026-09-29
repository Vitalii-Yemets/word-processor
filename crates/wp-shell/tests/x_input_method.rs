//! The X shell typing through a real input method.
//!
//! uim-xim is an X input method server, and byeoru the Korean input method
//! it runs: Shift and Space switch it on, and on the two-set layout the
//! keys g k s r m f spell 한글 a letter at a time — the syllable being
//! built shown as it grows, each finished syllable committed. xdotool
//! presses the keys through the X server's test extension, so they arrive
//! as a person's would, and the shell is the program they arrive at. Where
//! the machine has no Xvfb, uim-xim or xdotool the test passes without
//! proving anything, and says so on the way out.
#![cfg(target_os = "linux")]

use std::cell::RefCell;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use wp_raster::{Canvas, Color};
use wp_shell::{App, CompositionAttribute, Event, Response, WindowOptions};

/// A process for the length of a test.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A virtual X server, and the display it is.
fn start_server(number: u32) -> Option<(Running, String)> {
    let display = format!(":{number}");
    let socket = format!("/tmp/.X11-unix/X{number}");
    let _ = std::fs::remove_file(format!("/tmp/.X{number}-lock"));
    let _ = std::fs::remove_file(&socket);
    let child = Command::new("Xvfb")
        .args([&display, "-screen", "0", "800x600x24", "-nolisten", "tcp"])
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
    Some((server, display))
}

fn installed(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// What the program was given.
#[derive(Default)]
struct Seen {
    composed: Vec<(String, usize, Vec<CompositionAttribute>)>,
    committed: Vec<String>,
    typed: Vec<char>,
    ended: u32,
}

/// A program that keeps what it is given, and closes once the word is in
/// or the time is up.
struct Typist {
    canvas: Canvas,
    seen: Rc<RefCell<Seen>>,
    started: Instant,
    finished_at: Option<Instant>,
}

impl App for Typist {
    fn handle(&mut self, event: Event) -> Response {
        let mut seen = self.seen.borrow_mut();
        match event {
            Event::Compose { text, caret, attributes } => {
                seen.composed.push((text, caret, attributes));
                Response::Redraw
            }
            Event::Commit(text) => {
                seen.committed.push(text);
                Response::Redraw
            }
            Event::Char(character) => {
                seen.typed.push(character);
                Response::Redraw
            }
            Event::ComposeEnd => {
                seen.ended += 1;
                Response::Ignored
            }
            Event::Tick => {
                // A moment after the space, which comes after the word, so
                // that whatever the space brings is seen too.
                if self.finished_at.is_none() && seen.typed.contains(&' ') {
                    self.finished_at = Some(Instant::now());
                }
                let settled =
                    self.finished_at.is_some_and(|at| at.elapsed() > Duration::from_millis(800));
                if settled || self.started.elapsed() > Duration::from_secs(40) {
                    return Response::Close;
                }
                Response::Ignored
            }
            Event::Closing => Response::Close,
            _ => Response::Ignored,
        }
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.canvas = Canvas::new(width, height);
        self.canvas.clear(Color::WHITE);
        // Where the caret is, so that the input method can put its list
        // beside it — which is the shell telling the server.
        wp_shell::place_composition(120, 80, 18);
        &self.canvas
    }
}

/// Runs xdotool, and what it printed.
fn xdotool(display: &str, arguments: &[&str]) -> String {
    Command::new("xdotool")
        .args(arguments)
        .env("DISPLAY", display)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

#[test]
fn korean_is_composed_where_it_goes_and_committed_through_the_input_method() {
    let Some((_server, display)) = start_server(83) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    if !installed("xdotool") || Command::new("uim-xim").arg("--list").output().is_err() {
        eprintln!("skipped: uim-xim or xdotool is not on this machine");
        return;
    }
    let _method = Running(
        Command::new("uim-xim")
            .arg("--engine=byeoru")
            .env("DISPLAY", &display)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the input method server starts"),
    );
    // The server puts its name on the root window once it is up; a second
    // is ample, and two are given.
    std::thread::sleep(Duration::from_secs(2));
    std::env::remove_var("WAYLAND_DISPLAY");
    std::env::set_var("DISPLAY", &display);
    std::env::set_var("XMODIFIERS", "@im=uim");
    std::env::set_var("LC_CTYPE", "ko_KR.UTF-8");

    // The person: finds the window once it is up, gives it the keyboard,
    // switches the input method on, and types.
    let keys = {
        let display = display.clone();
        std::thread::spawn(move || {
            let window = xdotool(&display, &["search", "--sync", "--name", "Korean typist"]);
            let window = window.lines().next().unwrap_or("").to_owned();
            xdotool(&display, &["windowfocus", "--sync", &window]);
            std::thread::sleep(Duration::from_millis(500));
            xdotool(&display, &["key", "--delay", "200", "shift+space"]);
            xdotool(&display, &["type", "--delay", "200", "gksrmf"]);
            xdotool(&display, &["key", "--delay", "200", "space"]);
        })
    };

    let seen = Rc::new(RefCell::new(Seen::default()));
    let typist = Typist {
        canvas: Canvas::new(1, 1),
        seen: Rc::clone(&seen),
        started: Instant::now(),
        finished_at: None,
    };
    let options = WindowOptions { title: "Korean typist".to_owned(), width: 400, height: 300 };
    wp_shell::run(options, Box::new(typist)).expect("the window opens on the virtual server");
    keys.join().expect("the keys were pressed");

    let seen = Rc::try_unwrap(seen).ok().expect("the shell let go of the program").into_inner();
    let committed: String = seen.committed.concat();
    assert_eq!(
        committed.trim_end(),
        "한글",
        "the word the keys spell is committed, syllable by syllable: {:?}, typed {:?}, composed {:?}",
        seen.committed,
        seen.typed,
        seen.composed
    );
    assert!(
        !seen.typed.iter().any(|character| "gksrmf".contains(*character)),
        "and none of the letters that spelled it arrive as typed: {:?}",
        seen.typed
    );
    // The first syllable as it is built: the consonant, then with its
    // vowel, then with the consonant that closes it.
    let shown: Vec<&str> = seen.composed.iter().map(|(text, _, _)| text.as_str()).collect();
    let grows = ["ㅎ", "하", "한"]
        .iter()
        .scan(0, |from, wanted| {
            let found = shown[*from..].iter().position(|text| text == wanted);
            *from += found.map_or(shown.len(), |at| at + 1);
            Some(found.is_some())
        })
        .all(|found| found);
    assert!(grows, "the syllable being built is shown where it goes as it grows: {shown:?}");
    assert!(shown.contains(&"그"), "and the second: {shown:?}");
    assert!(
        seen.composed.iter().all(|(text, caret, attributes)| *caret <= text.chars().count()
            && attributes.len() == text.chars().count()),
        "each composition says where its caret is and how each character stands: {:?}",
        seen.composed
    );
    assert!(seen.typed.contains(&' '), "the space the input method did not want is typed");
}

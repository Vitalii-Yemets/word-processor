//! The editor itself on a real X server.
//!
//! The shell has its own tests against Xvfb; this one puts the whole
//! editor — ribbon, rulers, page, fonts — through the same window, reads
//! the picture back, and closes it the way the frame's button would. Where
//! the machine has no Xvfb the test passes without proving anything, and
//! says so on the way out.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use wp_docx::Document;
use wp_layout::FontLibrary;
use wp_raster::Canvas;
use wp_shell::screen::Shot;
use wp_shell::{accessibility, App, Cursor, Event, Response, WindowCommand, WindowOptions};

use crate::editor::Editor;
use crate::sample;

/// One test on a display at a time: which display a program talks to is
/// named by the environment, and the environment is the whole program's.
/// The compositor's tests hold it too.
pub(crate) static ONE_DISPLAY_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds [`ONE_DISPLAY_AT_A_TIME`], whatever a test before panicked with.
pub(crate) fn one_display_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    ONE_DISPLAY_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A virtual X server for the length of a test.
struct Server {
    child: Child,
}

impl Server {
    fn start(number: u32, width: u32, height: u32) -> Option<Self> {
        let display = format!(":{number}");
        let socket = format!("/tmp/.X11-unix/X{number}");
        let _ = std::fs::remove_file(format!("/tmp/.X{number}-lock"));
        let _ = std::fs::remove_file(&socket);
        let child = Command::new("Xvfb")
            .args([&display, "-screen", "0", &format!("{width}x{height}x24"), "-nolisten", "tcp"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut server = Self { child };
        let started = Instant::now();
        while !Path::new(&socket).exists() {
            if started.elapsed() > Duration::from_secs(10) {
                server.stop();
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // An X server is not a Wayland compositor: whatever a test before
        // this one left in the environment is not what this one uses.
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::set_var("DISPLAY", display);
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

/// What the test saw while the editor was up.
#[derive(Default)]
struct Seen {
    ticks: u32,
    shot: Option<Shot>,
    closed_on_request: bool,
}

/// The editor with a clock on it: it photographs the screen once the
/// window has been up long enough to be painted, then asks its own window
/// to close — through the window manager's channel, as the close button
/// does — and everything else goes to the editor untouched.
struct Timed {
    editor: Editor,
    seen: Rc<RefCell<Seen>>,
}

const PHOTOGRAPH_AT: u32 = 25;
const GIVE_UP_AT: u32 = 150;

impl App for Timed {
    fn handle(&mut self, event: Event) -> Response {
        match event {
            Event::Tick => {
                let mut seen = self.seen.borrow_mut();
                seen.ticks += 1;
                if seen.ticks == PHOTOGRAPH_AT {
                    seen.shot = wp_shell::screen::capture_screen();
                    wp_shell::window_command(WindowCommand::Close);
                }
                if seen.ticks > GIVE_UP_AT {
                    return Response::Close;
                }
            }
            Event::Closing => {
                let response = self.editor.handle(event);
                self.seen.borrow_mut().closed_on_request = response != Response::Refuse;
                return response;
            }
            _ => {}
        }
        self.editor.handle(event)
    }

    fn cursor(&mut self, x: i32, y: i32) -> Cursor {
        self.editor.cursor(x, y)
    }

    fn is_caption(&mut self, x: i32, y: i32) -> bool {
        self.editor.is_caption(x, y)
    }

    fn switch_window(&mut self, index: usize) {
        self.editor.switch_window(index);
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.editor.draw(width, height)
    }

    fn accessible_elements(&mut self) -> Vec<accessibility::Element> {
        self.editor.accessible_elements()
    }

    fn accessible_invoke(&mut self, id: u64) -> Response {
        self.editor.accessible_invoke(id)
    }

    fn accessible_text(&mut self) -> Option<accessibility::TextState> {
        self.editor.accessible_text()
    }

    fn accessible_select(&mut self, start: usize, end: usize) -> Response {
        self.editor.accessible_select(start, end)
    }

    fn accessible_rects(&mut self, start: usize, end: usize) -> Vec<(i32, i32, i32, i32)> {
        self.editor.accessible_rects(start, end)
    }
}

/// How much of a picture is one colour.
fn share_of(shot: &Shot, colour: (u8, u8, u8)) -> f32 {
    let matching = shot
        .pixels
        .chunks_exact(4)
        .filter(|pixel| (pixel[0], pixel[1], pixel[2]) == colour)
        .count();
    matching as f32 / (shot.width * shot.height) as f32
}

/// The colours a band of rows holds, most common first.
fn colours_in_rows(shot: &Shot, rows: std::ops::Range<usize>) -> Vec<((u8, u8, u8), usize)> {
    let mut counts = std::collections::HashMap::new();
    for y in rows {
        for x in 0..shot.width {
            let at = (y * shot.width + x) * 4;
            let pixel = &shot.pixels[at..at + 4];
            *counts.entry((pixel[0], pixel[1], pixel[2])).or_insert(0usize) += 1;
        }
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    counts
}

#[test]
fn the_editor_comes_up_on_a_real_server_and_closes_from_its_frame() {
    let _display = one_display_at_a_time();
    let Some(_server) = Server::start(96, 1400, 900) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    let document = Document::create(&sample::welcome_document()).expect("the sample document");
    let editor = Editor::opened(library, document, None::<PathBuf>);
    let seen = Rc::new(RefCell::new(Seen::default()));
    let timed = Timed { editor, seen: Rc::clone(&seen) };
    let options =
        WindowOptions { title: "Document — Word Processor".to_owned(), width: 1400, height: 900 };
    wp_shell::run(options, Box::new(timed))
        .expect("the editor's window opens on the virtual server");

    let seen = Rc::try_unwrap(seen).ok().expect("the shell let go of the editor").into_inner();
    assert!(seen.closed_on_request, "an unchanged document closes when its frame asks");
    assert!(
        (PHOTOGRAPH_AT..=PHOTOGRAPH_AT + 10).contains(&seen.ticks),
        "closed on the request, not the timeout: {} ticks",
        seen.ticks
    );
    let shot = seen.shot.expect("the screen was photographed");
    assert_eq!((shot.width, shot.height), (1400, 900));
    if let Ok(directory) = std::env::var("WP_PROOFS") {
        let mut canvas = Canvas::new(shot.width, shot.height);
        canvas.draw_pixels(&shot.pixels, shot.width, shot.height, 0, 0, shot.width, shot.height);
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::write(
            Path::new(&directory).join("linux-editor.png"),
            wp_raster::encode_png(&canvas),
        );
    }
    // The page of the welcome document sits on the desk around it, in
    // whichever theme the settings say; nothing of the bare server's black
    // shows through.
    let theme = crate::chrome::theme::Theme::of(crate::chrome::theme::Mode::Dark);
    let page = (theme.page.red, theme.page.green, theme.page.blue);
    let light = crate::chrome::theme::Theme::of(crate::chrome::theme::Mode::Light);
    let light_page = (light.page.red, light.page.green, light.page.blue);
    let paper = share_of(&shot, page).max(share_of(&shot, light_page));
    assert!(paper > 0.25, "the page is on the screen: {paper} of it is paper");
    assert!(share_of(&shot, (0, 0, 0)) < 0.02, "the window covers the screen");
    // The caption bar along the top is drawn by the editor, not the server.
    let top = colours_in_rows(&shot, 0..8);
    assert_ne!(top[0].0, (0, 0, 0), "the caption bar is painted: {:?}", &top[..top.len().min(3)]);
}

/// The editor with a word typed into it through an input method: it keeps
/// the document's text once the word is there, and closes.
struct Composing {
    editor: Editor,
    text: Rc<RefCell<Option<String>>>,
    composed: Rc<RefCell<bool>>,
    started: Instant,
}

impl App for Composing {
    fn handle(&mut self, event: Event) -> Response {
        if matches!(&event, Event::Compose { text, .. } if !text.is_empty()) {
            *self.composed.borrow_mut() = true;
        }
        if event == Event::Tick {
            let text = self.editor.document.plain_text();
            if text.contains("한글") || self.started.elapsed() > Duration::from_secs(60) {
                *self.text.borrow_mut() = Some(text);
                return Response::Close;
            }
        }
        self.editor.handle(event)
    }

    fn cursor(&mut self, x: i32, y: i32) -> Cursor {
        self.editor.cursor(x, y)
    }

    fn is_caption(&mut self, x: i32, y: i32) -> bool {
        self.editor.is_caption(x, y)
    }

    fn switch_window(&mut self, index: usize) {
        self.editor.switch_window(index);
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.editor.draw(width, height)
    }
}

/// Korean typed on a real X server through a real input method — uim-xim
/// and its Korean method, the keys pressed through the server's test
/// extension — goes into the document: the chain from the key to the page,
/// with nothing of it this program's own but the program.
#[test]
fn korean_typed_through_an_input_method_goes_into_the_document() {
    let _display = one_display_at_a_time();
    let Some(_server) = Server::start(88, 1400, 900) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    let display = ":88";
    let tools = Command::new("xdotool").arg("version").output().is_ok()
        && Command::new("uim-xim").arg("--list").output().is_ok();
    if !tools {
        eprintln!("skipped: uim-xim or xdotool is not on this machine");
        return;
    }
    let mut method = Command::new("uim-xim")
        .arg("--engine=byeoru")
        .env("DISPLAY", display)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the input method server starts");
    std::thread::sleep(Duration::from_secs(2));
    std::env::set_var("XMODIFIERS", "@im=uim");
    std::env::set_var("LC_CTYPE", "ko_KR.UTF-8");

    let keys = std::thread::spawn(move || {
        let run = |arguments: &[&str]| {
            Command::new("xdotool")
                .args(arguments)
                .env("DISPLAY", display)
                .output()
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
                .unwrap_or_default()
        };
        let window = run(&["search", "--sync", "--name", "Word Processor"]);
        let window = window.lines().next().unwrap_or("").to_owned();
        run(&["windowfocus", "--sync", &window]);
        // The editor draws its first page before it takes keys in earnest.
        std::thread::sleep(Duration::from_secs(3));
        run(&["key", "--delay", "250", "shift+space"]);
        run(&["type", "--delay", "250", "gksrmf"]);
        run(&["key", "--delay", "250", "space"]);
    });

    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    let mut body = wp_docx::model::Body::default();
    body.blocks.push(wp_docx::model::Block::Paragraph(wp_docx::model::Paragraph::default()));
    let document = Document::create(&body).expect("a blank document");
    let editor = Editor::opened(library, document, None::<PathBuf>);
    let text = Rc::new(RefCell::new(None));
    let composed = Rc::new(RefCell::new(false));
    let composing = Composing {
        editor,
        text: Rc::clone(&text),
        composed: Rc::clone(&composed),
        started: Instant::now(),
    };
    let options =
        WindowOptions { title: "Document — Word Processor".to_owned(), width: 1400, height: 900 };
    wp_shell::run(options, Box::new(composing)).expect("the editor's window opens");
    keys.join().expect("the keys were pressed");
    let _ = method.kill();
    let _ = method.wait();
    std::env::remove_var("XMODIFIERS");

    let text = text.borrow().clone().unwrap_or_default();
    assert!(text.contains("한글"), "the word is in the document: {text:?}");
    assert!(!text.contains("gksrmf"), "and not the letters that spelled it: {text:?}");
    assert!(*composed.borrow(), "the syllable was shown in the document as it was built");
}

/// The editor with something dropped on it from another program: it keeps
/// the document's text once the drop is in, and closes.
struct Dropping {
    editor: Editor,
    text: Rc<RefCell<Option<String>>>,
    started: Instant,
    dropped_at: Option<Instant>,
}

impl App for Dropping {
    fn handle(&mut self, event: Event) -> Response {
        if matches!(event, Event::DataDropped { .. }) {
            self.dropped_at = Some(Instant::now());
        }
        if event == Event::Tick {
            let settled = self.dropped_at.is_some_and(|at| at.elapsed() > Duration::from_secs(1));
            if settled || self.started.elapsed() > Duration::from_secs(60) {
                *self.text.borrow_mut() = Some(self.editor.document.plain_text());
                return Response::Close;
            }
        }
        self.editor.handle(event)
    }

    fn cursor(&mut self, x: i32, y: i32) -> Cursor {
        self.editor.cursor(x, y)
    }

    fn is_caption(&mut self, x: i32, y: i32) -> bool {
        self.editor.is_caption(x, y)
    }

    fn switch_window(&mut self, index: usize) {
        self.editor.switch_window(index);
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.editor.draw(width, height)
    }
}

/// Text dragged out of another program — GTK's, speaking XDND itself —
/// and let go on the page goes into the document.
#[test]
fn text_dragged_from_another_program_goes_into_the_document() {
    let _display = one_display_at_a_time();
    let Some(_server) = Server::start(89, 1400, 900) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    let gtk = Command::new("python3")
        .args(["-c", "import gi; gi.require_version('Gtk', '3.0'); from gi.repository import Gtk"])
        .status()
        .is_ok_and(|status| status.success());
    if !gtk || Command::new("xdotool").arg("version").output().is_err() {
        eprintln!("skipped: GTK or xdotool is not on this machine");
        return;
    }
    std::env::remove_var("XMODIFIERS");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/dnd-peer.py");
    // The giver at the screen's right, over the editor's window.
    let mut peer = Command::new("python3")
        .arg(script)
        .args(["source", "text", "dropped in from GTK"])
        .env("DND_PEER_X", "1050")
        .env("NO_AT_BRIDGE", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the peer starts");

    let hand = std::thread::spawn(|| {
        let run = |arguments: &[String]| {
            let _ = Command::new("xdotool").args(arguments).env("DISPLAY", ":89").output();
        };
        let _ = Command::new("xdotool")
            .args(["search", "--sync", "--name", "Word Processor"])
            .env("DISPLAY", ":89")
            .output();
        // The editor draws its first page, and the peer comes up above it.
        std::thread::sleep(Duration::from_secs(4));
        let (from, to) = ((1200, 150), (500, 400));
        run(&["mousemove".into(), from.0.to_string(), from.1.to_string()]);
        std::thread::sleep(Duration::from_millis(300));
        run(&["mousedown".into(), "1".into()]);
        for step in 1..=14 {
            std::thread::sleep(Duration::from_millis(100));
            let x = from.0 + (to.0 - from.0) * step / 14;
            let y = from.1 + (to.1 - from.1) * step / 14;
            run(&["mousemove".into(), x.to_string(), y.to_string()]);
        }
        std::thread::sleep(Duration::from_millis(500));
        run(&["mouseup".into(), "1".into()]);
    });

    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    let mut body = wp_docx::model::Body::default();
    body.blocks.push(wp_docx::model::Block::Paragraph(wp_docx::model::Paragraph::default()));
    let document = Document::create(&body).expect("a blank document");
    let editor = Editor::opened(library, document, None::<PathBuf>);
    let text = Rc::new(RefCell::new(None));
    let dropping =
        Dropping { editor, text: Rc::clone(&text), started: Instant::now(), dropped_at: None };
    let options =
        WindowOptions { title: "Document — Word Processor".to_owned(), width: 1400, height: 900 };
    wp_shell::run(options, Box::new(dropping)).expect("the editor's window opens");
    hand.join().expect("the pointer moved");
    let _ = peer.kill();
    let _ = peer.wait();

    let text = text.borrow().clone().unwrap_or_default();
    assert!(text.contains("dropped in from GTK"), "the drop is in the document: {text:?}");
}

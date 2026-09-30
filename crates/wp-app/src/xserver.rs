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
use std::sync::{Arc, Mutex, MutexGuard};
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

/// How long a test on a display gives any one step to happen. On a quiet
/// machine every step takes a moment; on one busy with the rest of the
/// suite, some have taken several seconds, and a step that has not happened
/// in a minute is not going to.
pub(crate) const STEP_BOUND: Duration = Duration::from_secs(60);

/// Waits, up to [`STEP_BOUND`], for something to be so; if it is not, says
/// which step it was, what it waited for, and what there was instead.
fn within<T>(
    step: &str,
    mut ready: impl FnMut() -> Option<T>,
    instead: impl Fn() -> String,
) -> Result<T, String> {
    let started = Instant::now();
    loop {
        if let Some(found) = ready() {
            return Ok(found);
        }
        if started.elapsed() > STEP_BOUND {
            return Err(format!("waited {STEP_BOUND:?} for {step}; {}", instead()));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Whether a selection has an owner on a display — asked of the X server
/// over a line of the test's own, since nothing on the image says it. An
/// input method takes the selection of its name once programs can find it,
/// and the shell looks for that owner as its window comes up.
fn selection_owned(display: u32, name: &str) -> bool {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    let Ok(mut line) = UnixStream::connect(format!("/tmp/.X11-unix/X{display}")) else {
        return false;
    };
    let _ = line.set_read_timeout(Some(Duration::from_secs(5)));
    // Least significant byte first, protocol 11.0, and no authorisation,
    // which Xvfb asks none of.
    if line.write_all(&[b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err() {
        return false;
    }
    let mut head = [0u8; 8];
    if line.read_exact(&mut head).is_err() || head[0] != 1 {
        return false;
    }
    let mut setup = vec![0u8; usize::from(u16::from_le_bytes([head[6], head[7]])) * 4];
    if line.read_exact(&mut setup).is_err() {
        return false;
    }
    let ask = |line: &mut UnixStream, request: &[u8]| -> Option<u32> {
        line.write_all(request).ok()?;
        let mut reply = [0u8; 32];
        line.read_exact(&mut reply).ok()?;
        (reply[0] == 1).then(|| u32::from_le_bytes([reply[8], reply[9], reply[10], reply[11]]))
    };
    // InternAtom for the selection's atom, only if somebody has named it —
    // an atom nobody has named is a selection nobody owns — and then
    // GetSelectionOwner.
    let padded = name.len().div_ceil(4) * 4;
    let mut intern = vec![16, 1];
    intern.extend_from_slice(&((2 + padded / 4) as u16).to_le_bytes());
    intern.extend_from_slice(&(name.len() as u16).to_le_bytes());
    intern.extend_from_slice(&[0, 0]);
    intern.extend_from_slice(name.as_bytes());
    intern.resize(8 + padded, 0);
    let Some(atom) = ask(&mut line, &intern).filter(|&atom| atom != 0) else { return false };
    let mut owner = vec![23, 0, 2, 0];
    owner.extend_from_slice(&atom.to_le_bytes());
    ask(&mut line, &owner).is_some_and(|owner| owner != 0)
}

/// What the person typing Korean can see of the editor.
#[derive(Default)]
struct Page {
    /// Whether the window has been drawn.
    drawn: bool,
    /// The syllable being built, as the input method last showed it, and
    /// whether one was ever shown.
    composing: String,
    composed: bool,
    /// The document's text, the syllable being built included.
    text: String,
    /// Whether the person has finished, and the window is to close.
    finished: bool,
}

fn look(page: &Mutex<Page>) -> MutexGuard<'_, Page> {
    page.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The editor with a word typed into it through an input method: it shows
/// the person what they are typing, and closes once they have finished.
struct Composing {
    editor: Editor,
    page: Arc<Mutex<Page>>,
}

impl App for Composing {
    fn handle(&mut self, event: Event) -> Response {
        if event == Event::Tick && look(&self.page).finished {
            return Response::Close;
        }
        let composing = match &event {
            Event::Compose { text, .. } => Some(text.clone()),
            Event::ComposeEnd => Some(String::new()),
            _ => None,
        };
        let response = self.editor.handle(event);
        let text = self.editor.document.plain_text();
        let mut page = look(&self.page);
        if let Some(composing) = composing {
            page.composed |= !composing.is_empty();
            page.composing = composing;
        }
        page.text = text;
        response
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
        look(&self.page).drawn = true;
        self.editor.draw(width, height)
    }
}

/// The keys that spell 한글 on the two-set layout, and what the editor
/// shows once each has been through the input method: the syllable being
/// built, and what the document holds by then, that syllable included and
/// a space at the end not counted.
const KOREAN: [(&str, &str, &str); 7] = [
    ("g", "ㅎ", "ㅎ"),
    ("k", "하", "하"),
    ("s", "한", "한"),
    // Not a final consonant: 한 is finished, and ㄱ begins the next.
    ("r", "ㄱ", "한ㄱ"),
    ("m", "그", "한그"),
    ("f", "글", "한글"),
    // Space finishes 글, and goes in after it.
    ("space", "", "한글"),
];

/// The person at the keyboard: they find the editor's window, give it the
/// keyboard, switch the input method on and type 한글 — each key once the
/// editor shows what the one before did, as a person looks at what they
/// type. The keys used to go at a fixed pace, three seconds after the
/// window was found; on a busy machine uim-xim fell behind, answered a key
/// before drawing what it did, and gave the space back ahead of the
/// syllable it was to follow: "한 글".
fn type_korean(display: &str, page: &Mutex<Page>) -> Result<(), String> {
    let xdotool = |arguments: &[&str]| {
        Command::new("xdotool")
            .args(arguments)
            .env("DISPLAY", display)
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .unwrap_or_default()
    };
    let instead = || {
        let page = look(page);
        format!("the editor shows {:?} being built, in {:?}", page.composing, page.text)
    };
    let window = within(
        "the editor's window, found by its name",
        || xdotool(&["search", "--name", "Word Processor"]).lines().next().map(str::to_owned),
        || "no window of that name".to_owned(),
    )?;
    within(
        "the editor's window with the keyboard",
        || {
            xdotool(&["windowfocus", &window]);
            (xdotool(&["getwindowfocus"]) == window).then_some(())
        },
        || format!("the keyboard is with {}", xdotool(&["getwindowfocus"])),
    )?;
    within("the window drawn", || look(page).drawn.then_some(()), instead)?;
    // Shift and Space switch the method on; that it did shows with the
    // first letter.
    xdotool(&["key", "shift+space"]);
    for (key, composing, text) in KOREAN {
        xdotool(&["key", key]);
        within(
            &format!("{key} to show {composing:?} being built, in {text:?}"),
            || {
                let page = look(page);
                (page.composing == composing && page.text.trim_end() == text).then_some(())
            },
            instead,
        )?;
    }
    Ok(())
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
    // The shell looks for the input method once, as its window comes up;
    // what is waited for is the input method taking its name, where it used
    // to be two seconds, which on a busy machine it had not.
    let registered = within(
        "uim-xim to take the name @server=uim",
        || selection_owned(88, "@server=uim").then_some(()),
        || "nobody owns it".to_owned(),
    );
    if let Err(why) = registered {
        let _ = method.kill();
        let _ = method.wait();
        panic!("{why}");
    }
    std::env::set_var("XMODIFIERS", "@im=uim");
    std::env::set_var("LC_CTYPE", "ko_KR.UTF-8");

    let page = Arc::new(Mutex::new(Page::default()));
    let typist = {
        let page = Arc::clone(&page);
        std::thread::spawn(move || {
            let typed = type_korean(display, &page);
            look(&page).finished = true;
            typed
        })
    };

    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    let mut body = wp_docx::model::Body::default();
    body.blocks.push(wp_docx::model::Block::Paragraph(wp_docx::model::Paragraph::default()));
    let document = Document::create(&body).expect("a blank document");
    let editor = Editor::opened(library, document, None::<PathBuf>);
    let composing = Composing { editor, page: Arc::clone(&page) };
    let options =
        WindowOptions { title: "Document — Word Processor".to_owned(), width: 1400, height: 900 };
    wp_shell::run(options, Box::new(composing)).expect("the editor's window opens");
    let typed = typist.join().expect("the keys were pressed");
    let _ = method.kill();
    let _ = method.wait();
    std::env::remove_var("XMODIFIERS");

    if let Err(why) = typed {
        panic!("{why}");
    }
    let page = look(&page);
    assert!(page.text.contains("한글"), "the word is in the document: {:?}", page.text);
    assert!(!page.text.contains("gksrmf"), "and not the letters that spelled it: {:?}", page.text);
    assert!(page.composed, "the syllable was shown in the document as it was built");
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

/// The editor as a screen reader reads it, closing once the reader is done.
struct ReadAloud {
    editor: Editor,
    finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
    started: Instant,
    /// Whether the find strip has been opened yet.
    searching: bool,
}

impl App for ReadAloud {
    fn handle(&mut self, event: Event) -> Response {
        let finished = self.finished.load(std::sync::atomic::Ordering::SeqCst);
        if event == Event::Tick && (finished || self.started.elapsed() > Duration::from_secs(90)) {
            return Response::Close;
        }
        // The find strip is opened at once, the way a person would with
        // Ctrl+F, so that there is a box on the window to be written into —
        // as soon as the window has its size, which is before the first
        // tick, and so before a screen reader can ask what the window holds
        // and be told of a strip not yet drawn.
        let sized = matches!(event, Event::Resized { .. });
        let response = self.editor.handle(event);
        if sized && !self.searching {
            self.searching = true;
            let control = wp_shell::Modifiers { control: true, ..wp_shell::Modifiers::default() };
            self.editor
                .handle(Event::KeyDown { key: wp_shell::Key::Letter('f'), modifiers: control });
            return Response::Redraw;
        }
        response
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

    fn accessible_lines(&mut self) -> Vec<(usize, usize)> {
        self.editor.accessible_lines()
    }

    fn accessible_attributes(
        &mut self,
        offset: usize,
    ) -> Option<(accessibility::TextAttributes, usize, usize)> {
        self.editor.accessible_attributes(offset)
    }

    fn accessible_set_value(&mut self, id: u64, value: &str) -> Response {
        self.editor.accessible_set_value(id, value)
    }
}

/// The whole editor read through AT-SPI by a screen reader's library: the
/// ribbon's tabs, buttons and boxes, the find strip and the status strip,
/// the document's text by lines and by how it is set, a box written into,
/// and a dialog opening — what is heard, and what the dialog holds.
#[test]
fn a_screen_reader_reads_the_editor_through_at_spi() {
    let _display = one_display_at_a_time();
    let Some(_server) = Server::start(91, 1400, 900) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    let atspi = Command::new("python3")
        .args([
            "-c",
            "import gi; gi.require_version('Atspi', '2.0'); from gi.repository import Atspi",
        ])
        .status()
        .is_ok_and(|status| status.success());
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
    let _ = std::io::BufRead::read_line(
        &mut std::io::BufReader::new(bus.stdout.take().expect("the address")),
        &mut address,
    );
    let address = address.trim().to_owned();
    std::env::remove_var("XMODIFIERS");
    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &address);

    let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let finished = std::sync::Arc::clone(&finished);
        let address = address.clone();
        std::thread::spawn(move || {
            let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/atspi-reader.py");
            let output = Command::new("python3")
                .arg(script)
                .args(["Word Processor", "Italic", "Paragraph", "Search document", "aloud"])
                .env("DBUS_SESSION_BUS_ADDRESS", &address)
                .env("DISPLAY", ":91")
                .stderr(Stdio::null())
                .output();
            finished.store(true, std::sync::atomic::Ordering::SeqCst);
            output.map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        })
    };

    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    let mut body = wp_docx::model::Body::default();
    body.blocks.push(wp_docx::model::Block::Paragraph(wp_docx::model::Paragraph::text(
        "Read aloud on Linux.",
    )));
    let document = Document::create(&body).expect("a document");
    let editor = Editor::opened(library, document, None::<PathBuf>);
    let reading = ReadAloud { editor, finished, started: Instant::now(), searching: false };
    let options =
        WindowOptions { title: "Document — Word Processor".to_owned(), width: 1400, height: 900 };
    wp_shell::run(options, Box::new(reading)).expect("the editor's window opens");
    let said = reader.join().expect("the reader ran").expect("the reader's output");
    std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
    let _ = bus.kill();
    let _ = bus.wait();
    let has = |wanted: &str| said.lines().any(|line| line.starts_with(wanted));

    assert!(has("node 0 application | Word Processor"), "{said}");
    assert!(has("node 1 frame | Document — Word Processor"), "{said}");
    assert!(has("node 2 page tab | Home | selected"), "the Home tab, chosen: {said}");
    assert!(has("node 2 page tab | Insert"), "{said}");
    assert!(has("node 2 toggle button | Italic"), "{said}");
    assert!(has("text Read aloud on Linux."), "the document's text: {said}");
    assert!(has("pressed"), "{said}");
    // The boxes as boxes, with what is in them.
    assert!(has("node 2 combo box | Font Size | enabled | 11"), "the size box: {said}");
    assert!(has("node 2 combo box | Font | enabled |"), "the font box: {said}");
    // The find strip, with the keyboard in its box, and the status strip.
    assert!(has("node 2 panel | Find"), "{said}");
    assert!(has("node 3 entry | Find | focused,enabled,editable"), "{said}");
    assert!(has("node 2 status bar | Status Bar"), "{said}");
    assert!(has("node 3 label | Page 1 of 1, 4 words"), "what the strip says: {said}");
    // Lines as the layout broke them, and how the text is set.
    assert!(has("line 0 20"), "{said}");
    assert!(has("attributes 0 20 "), "{said}");
    assert!(said.contains("weight=400"), "{said}");
    // A box written into: the navigation pane's search.
    assert!(has("written aloud"), "{said}");
    // A dialog opening is heard, and what it holds is read.
    assert!(has("event window activate Paragraph"), "{said}");
    assert!(has("event focused"), "{said}");
    assert!(has("dialog 0 dialog | Paragraph"), "{said}");
    assert!(has("dialog 1 push button | OK"), "{said}");
    assert!(has("done"), "{said}");
}

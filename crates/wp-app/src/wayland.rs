//! The editor itself on a Wayland compositor.
//!
//! The shell has its own tests against sway; this one puts the whole
//! editor — ribbon, rulers, page, fonts — on the same compositor, types
//! into the document, photographs the screen through another client, and
//! closes the window. Where the machine has no sway the test passes
//! without proving anything, and says so on the way out.

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use wp_docx::Document;
use wp_layout::FontLibrary;
use wp_raster::Canvas;
use wp_shell::{accessibility, App, Cursor, Event, Response, WindowCommand, WindowOptions};

use crate::editor::Editor;
use crate::xserver::STEP_BOUND;

/// Who the compositor runs as: sway will not run as root, and everything
/// in the build container is root.
const COMPOSITOR_USER: &str = "compositor";

/// A compositor with no screen, for the length of a test.
struct Compositor {
    child: Child,
    runtime: PathBuf,
    display: String,
}

impl Compositor {
    fn start(width: u32, height: u32) -> Option<Self> {
        let runtime = std::env::temp_dir().join(format!("wp-wayland-app-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&runtime);
        std::fs::create_dir_all(&runtime).ok()?;
        let config = runtime.join("config");
        let mut file = std::fs::File::create(&config).ok()?;
        writeln!(
            file,
            "default_border none\ndefault_floating_border none\ngaps inner 0\noutput HEADLESS-1 resolution {width}x{height}\n"
        )
        .ok()?;
        drop(file);
        Command::new("chown").args(["-R", COMPOSITOR_USER]).arg(&runtime).status().ok()?;

        let started_as = format!(
            "XDG_RUNTIME_DIR={} WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 XDG_SESSION_TYPE=wayland exec sway -c {}",
            runtime.display(),
            config.display()
        );
        let child = Command::new("su")
            .args([COMPOSITOR_USER, "-s", "/bin/sh", "-c", &started_as])
            .env_remove("DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut compositor = Self { child, runtime: runtime.clone(), display: String::new() };
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
        std::thread::sleep(Duration::from_millis(500));
        std::env::set_var("XDG_RUNTIME_DIR", &runtime);
        std::env::set_var("WAYLAND_DISPLAY", &compositor.display);
        Some(compositor)
    }

    fn stop(&mut self) {
        // The child is su, and killing it leaves the sway it started still
        // running — one for every test, found there long after. Sway is
        // found by the one thing on its command line that is this
        // compositor's alone: the path of its configuration.
        let _ = Command::new("pkill").arg("-f").arg(self.runtime.join("config")).status();
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

/// What the editor is to have typed into it. A function and not a string
/// constant, which is the shape [`crate::messagelist`] reads as a message
/// the interface shows.
fn words() -> &'static str {
    "Hello from Wayland"
}

/// The steps of the typing test, each named by what it waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Typing {
    /// The window drawn, to be typed into.
    Drawn,
    /// A key from the person's keyboard reaching the editor.
    Keyboard,
    /// Everything typed reaching the editor.
    Typed,
    /// The window drawn with it, to be photographed.
    Shown,
    /// The first request to close answered: no, with the typing unsaved.
    Refused,
    /// The second, with the document saved, answered: yes.
    Closed,
}

impl Typing {
    /// What the step waits for, in the words of the message that says it
    /// did not come.
    fn waits_for(self) -> &'static str {
        match self {
            Self::Drawn => "the window drawn",
            Self::Keyboard => "a key from wtype's keyboard reaching the editor",
            Self::Typed => "everything wtype typed reaching the editor",
            Self::Shown => "the window drawn with what was typed",
            Self::Refused => "the request to close answered while the typing is unsaved",
            Self::Closed => "the request to close answered once the document is saved",
        }
    }
}

/// What the test saw while the editor was up.
#[derive(Default)]
struct Seen {
    typed: String,
    shot: Option<(usize, usize, Vec<u8>)>,
    /// What the document held when the picture was taken.
    document_text: String,
    asked_to_close: u32,
    closed_on_request: bool,
    /// The step that did not happen in time, and what there was instead.
    gave_up: Option<String>,
}

/// The editor typed into from another client, photographed, and asked to
/// close — each once what comes before it has happened, and never by a
/// count of ticks: on a machine busy with the rest of the suite, a count
/// that is plenty on a quiet one is not.
struct Timed {
    editor: Editor,
    seen: Rc<RefCell<Seen>>,
    runtime: PathBuf,
    display: String,
    /// Which step it is on, and when that began.
    step: Typing,
    began: Instant,
    /// How many times the window has been drawn, and how many times by the
    /// last key that reached it.
    drawn: u32,
    drawn_by_the_last_key: u32,
    /// The person's keyboard: a wtype that types nothing and waits, so that
    /// the seat has a keyboard from first to last. A wtype that types goes
    /// when it has typed, and its keyboard with it; with no other, the seat
    /// would have none in between, and the editor would let its own go and
    /// take one up afresh for every wtype, missing whatever came first.
    keyboard: Option<Child>,
    /// The wtypes that type, the last of them the one typing now, and when
    /// it was started.
    typists: Vec<Child>,
    pressed: Option<Instant>,
}

impl Timed {
    fn next(&mut self, step: Typing) {
        self.step = step;
        self.began = Instant::now();
    }

    fn wtype(&self) -> Command {
        let mut wtype = Command::new("wtype");
        wtype
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("WAYLAND_DISPLAY", &self.display)
            .env_remove("DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        wtype
    }

    /// Types something, through a wtype of its own.
    fn type_in(&mut self, text: &str) {
        if let Ok(typist) = self.wtype().arg(text).spawn() {
            self.typists.push(typist);
        }
        self.pressed = Some(Instant::now());
    }

    /// Whether the last wtype started has typed everything and gone.
    fn typist_gone(&mut self) -> bool {
        self.typists.last_mut().is_none_or(|typist| !matches!(typist.try_wait(), Ok(None)))
    }

    /// Takes the step it is on, if what it waits for has happened; closes
    /// the window, saying which step it was, if that has not happened in
    /// [`STEP_BOUND`].
    fn step(&mut self) -> Option<Response> {
        if self.began.elapsed() > STEP_BOUND {
            let typed = self.seen.borrow().typed.clone();
            self.seen.borrow_mut().gave_up = Some(format!(
                "{:?} waited {STEP_BOUND:?} for {}; the editor had {typed:?} typed into it",
                self.step,
                self.step.waits_for()
            ));
            return Some(Response::Close);
        }
        let typist_gone = self.typist_gone();
        match self.step {
            Typing::Drawn if self.drawn > 0 => {
                // Ten minutes asleep: longer than the test, which stops it.
                self.keyboard = self.wtype().args(["-s", "600000"]).spawn().ok();
                self.next(Typing::Keyboard);
            }
            // A compositor with no devices has no keyboard until wtype makes
            // one, and a key pressed as it appears is pressed before the
            // editor has taken the keyboard up: a space at a time, until one
            // arrives. Two spaces typed first, in the same wtype as the
            // words, used to be all the editor was given to be ready.
            Typing::Keyboard if self.seen.borrow().typed.contains(' ') => {
                self.type_in(words());
                self.next(Typing::Typed);
            }
            Typing::Keyboard
                if self.pressed.is_none_or(|at| at.elapsed() > Duration::from_millis(250))
                    && typist_gone =>
            {
                self.type_in(" ");
            }
            Typing::Typed if self.seen.borrow().typed.trim_start() == words() => {
                self.next(Typing::Shown);
            }
            Typing::Shown if self.drawn > self.drawn_by_the_last_key => {
                let shot = photograph(&self.runtime, &self.display);
                let text = self.editor.document.paragraph_text(0).unwrap_or_default();
                let mut seen = self.seen.borrow_mut();
                seen.shot = shot;
                seen.document_text = text;
                drop(seen);
                wp_shell::window_command(WindowCommand::Close);
                self.next(Typing::Refused);
            }
            // The first request is refused: there is typing in the document
            // that is not on disk, and the question about that cannot be
            // asked where there is no dialog program — so the window stays,
            // which is the safe answer and the right one. Saved, it closes.
            Typing::Refused if self.seen.borrow().asked_to_close == 1 => {
                let _ = self.editor.document.mark_saved();
                wp_shell::window_command(WindowCommand::Close);
                self.next(Typing::Closed);
            }
            _ => {}
        }
        None
    }
}

impl Drop for Timed {
    fn drop(&mut self) {
        for mut wtype in self.keyboard.take().into_iter().chain(self.typists.drain(..)) {
            let _ = wtype.kill();
            let _ = wtype.wait();
        }
    }
}

impl App for Timed {
    fn handle(&mut self, event: Event) -> Response {
        match event {
            Event::Tick => {
                if let Some(response) = self.step() {
                    return response;
                }
            }
            Event::Closing => {
                let response = self.editor.handle(event);
                let mut seen = self.seen.borrow_mut();
                seen.asked_to_close += 1;
                seen.closed_on_request = response != Response::Refuse;
                return response;
            }
            Event::Char(character) => {
                self.seen.borrow_mut().typed.push(character);
                self.drawn_by_the_last_key = self.drawn;
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
        self.drawn += 1;
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

/// What `grim` photographed, as red, green and blue.
fn photograph(runtime: &Path, display: &str) -> Option<(usize, usize, Vec<u8>)> {
    let output = Command::new("grim")
        .args(["-t", "ppm", "-"])
        .env("XDG_RUNTIME_DIR", runtime)
        .env("WAYLAND_DISPLAY", display)
        .env_remove("DISPLAY")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    read_ppm(&output.stdout)
}

fn read_ppm(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut fields = Vec::new();
    let mut at = 0usize;
    while fields.len() < 4 && at < bytes.len() {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
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
    Some((width, height, bytes.get(at..at + width * height * 3)?.to_vec()))
}

/// How much of a picture is one colour.
fn share_of(rgb: &[u8], colour: (u8, u8, u8)) -> f32 {
    let matching =
        rgb.chunks_exact(3).filter(|pixel| (pixel[0], pixel[1], pixel[2]) == colour).count();
    matching as f32 / (rgb.len() / 3) as f32
}

#[test]
fn the_editor_comes_up_on_a_compositor_takes_typing_and_closes() {
    let _display = crate::xserver::one_display_at_a_time();
    let Some(compositor) = Compositor::start(1400, 900) else {
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    // One empty paragraph, as a new document has: somewhere for the caret
    // to be, and so somewhere for the typing to go.
    let mut body = wp_docx::model::Body::default();
    body.blocks.push(wp_docx::model::Block::Paragraph(wp_docx::model::Paragraph::default()));
    let document = Document::create(&body).expect("a document to type into");
    let editor = Editor::opened(library, document, None::<PathBuf>);
    let seen = Rc::new(RefCell::new(Seen::default()));
    let timed = Timed {
        editor,
        seen: Rc::clone(&seen),
        runtime: compositor.runtime.clone(),
        display: compositor.display.clone(),
        step: Typing::Drawn,
        began: Instant::now(),
        drawn: 0,
        drawn_by_the_last_key: 0,
        keyboard: None,
        typists: Vec::new(),
        pressed: None,
    };
    let options =
        WindowOptions { title: "Document — Word Processor".to_owned(), width: 1400, height: 900 };
    wp_shell::run(options, Box::new(timed)).expect("the editor's window opens on the compositor");

    let seen = Rc::try_unwrap(seen).ok().expect("the shell let go of the editor").into_inner();
    assert!(seen.gave_up.is_none(), "{}", seen.gave_up.as_deref().unwrap_or_default());
    assert_eq!(seen.asked_to_close, 2, "asked twice: refused while unsaved, then agreed");
    assert!(seen.closed_on_request, "a saved document closes when it is asked to");

    assert_eq!(seen.typed.trim_start(), words(), "what was typed reached the editor");
    let (width, height, rgb) = seen.shot.expect("the screen was photographed");
    assert!(
        seen.document_text.contains(words()),
        "and went into the document: {:?}",
        seen.document_text
    );
    assert_eq!((width, height), (1400, 900));
    if let Ok(directory) = std::env::var("WP_PROOFS") {
        let mut canvas = Canvas::new(width, height);
        let rgba: Vec<u8> =
            rgb.chunks_exact(3).flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255]).collect();
        canvas.draw_pixels(&rgba, width, height, 0, 0, width, height);
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::write(
            Path::new(&directory).join("wayland-editor.png"),
            wp_raster::encode_png(&canvas),
        );
    }
    // The page of the document, on the desk the theme paints, and nothing
    // of the compositor's own background showing through.
    let theme = crate::chrome::theme::Theme::of(crate::chrome::theme::Mode::Dark);
    let page = (theme.page.red, theme.page.green, theme.page.blue);
    let light = crate::chrome::theme::Theme::of(crate::chrome::theme::Mode::Light);
    let light_page = (light.page.red, light.page.green, light.page.blue);
    let paper = share_of(&rgb, page).max(share_of(&rgb, light_page));
    assert!(paper > 0.2, "the page is on the screen: {paper} of it is paper");
}

/// The steps of the screenshot test, each named by what it waits for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Step {
    /// The window drawn, with the ribbon's Insert tab on it to press.
    #[default]
    InsertTab,
    /// The Insert tab drawn, with the Screenshot button on it to press.
    ScreenshotButton,
    /// The list drawn, offering the whole screen, which is pressed.
    WholeScreenOffered,
    /// The portal's answer, which the status strip says.
    WholeScreenAnswered,
    /// The picture drawn in the document, and Screenshot pressed again.
    ScreenshotAgain,
    /// The list drawn again, offering Screen Clipping, which is pressed.
    ClippingOffered,
    /// The portal's answer, once the person has pressed Escape at slurp.
    ClippingAnswered,
    /// Everything has been seen.
    Done,
}

impl Step {
    /// What the step waits for, in the words of the message that says it
    /// did not come.
    fn waits_for(self) -> &'static str {
        match self {
            Self::InsertTab => "the window drawn, with the Insert tab on the ribbon",
            Self::ScreenshotButton => "the Insert tab drawn, with the Screenshot button on it",
            Self::WholeScreenOffered => "the Screenshot list drawn, offering the whole screen",
            Self::WholeScreenAnswered => "the portal's answer for the whole screen, in the status",
            Self::ScreenshotAgain => "the window drawn with the picture, and Screenshot on it",
            Self::ClippingOffered => "the Screenshot list drawn, offering Screen Clipping",
            Self::ClippingAnswered => "the portal's answer for the clipping, in the status",
            Self::Done => "nothing",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::InsertTab => Self::ScreenshotButton,
            Self::ScreenshotButton => Self::WholeScreenOffered,
            Self::WholeScreenOffered => Self::WholeScreenAnswered,
            Self::WholeScreenAnswered => Self::ScreenshotAgain,
            Self::ScreenshotAgain => Self::ClippingOffered,
            Self::ClippingOffered => Self::ClippingAnswered,
            Self::ClippingAnswered | Self::Done => Self::Done,
        }
    }
}

/// What the editor said as it took its screenshots.
#[derive(Default)]
struct Shots {
    /// Which step it is on, when that began, and how many times the window
    /// had been drawn and what the status strip said then: a step waits for
    /// the window to be drawn again, or for the status strip to say
    /// something else.
    step: Step,
    began: Option<Instant>,
    drawn_then: u32,
    status_then: String,
    /// How many times the window has been drawn.
    drawn: u32,
    offered: Vec<String>,
    whole: String,
    picture: bool,
    /// Whether the window had been drawn again, without the list, by the
    /// time the whole screen was photographed.
    drawn_before_the_shutter: bool,
    cancelled: String,
    /// How long the clipping was waited for, and what became of the Escape
    /// pressed at slurp.
    waited: Duration,
    escape: Option<Escape>,
    /// The step that did not happen in time, and what there was instead.
    gave_up: Option<String>,
}

/// The editor pressed through what it tells a screen reader, as a person
/// who cannot see the ribbon would press it: the Insert tab, Screenshot,
/// and one of what that offers.
///
/// Each step waits for what it needs — the window drawn with what is to be
/// pressed on it, or the portal's answer said in the status strip — and
/// not for a count of ticks: on a machine busy with the rest of the suite a
/// count that is plenty on a quiet one is not.
struct Photographing {
    editor: Editor,
    shots: Rc<RefCell<Shots>>,
    runtime: PathBuf,
    display: String,
    /// The person at slurp, once Screen Clipping has been pressed, and the
    /// word that tells them the clipping has been answered.
    person: Option<(Arc<AtomicBool>, std::thread::JoinHandle<Escape>)>,
}

impl Photographing {
    /// Presses the element of a kind with a name.
    fn press(&mut self, role: accessibility::Role, name: &str) -> bool {
        let found = self
            .editor
            .accessible_elements()
            .into_iter()
            .find(|element| element.role == role && element.name == name);
        found.is_some_and(|element| self.editor.accessible_invoke(element.id) != Response::Ignored)
    }

    /// What the list that is down offers.
    fn menu(&mut self) -> Vec<String> {
        let elements = self.editor.accessible_elements();
        elements
            .into_iter()
            .filter(|element| element.role == accessibility::Role::MenuItem)
            .map(|element| element.name)
            .collect()
    }

    /// What the status strip says.
    fn status(&mut self) -> String {
        let elements = self.editor.accessible_elements();
        let strip = elements.iter().find(|element| element.role == accessibility::Role::StatusBar);
        strip.map(|element| element.value.clone()).unwrap_or_default()
    }

    /// On to the next step, noting how the window and the status strip are
    /// as it begins.
    fn next(&mut self) {
        let status = self.status();
        let mut shots = self.shots.borrow_mut();
        shots.step = shots.step.next();
        shots.began = Some(Instant::now());
        shots.drawn_then = shots.drawn;
        shots.status_then = status;
    }

    /// Gives up on a step that has not happened in [`STEP_BOUND`], saying
    /// which it was, what it waited for, and what there was instead.
    fn gives_up(&mut self) -> bool {
        let (step, began) = {
            let mut shots = self.shots.borrow_mut();
            (shots.step, *shots.began.get_or_insert_with(Instant::now))
        };
        if began.elapsed() < STEP_BOUND {
            return false;
        }
        let (offered, status) = (self.menu(), self.status());
        self.shots.borrow_mut().gave_up = Some(format!(
            "{step:?} waited {STEP_BOUND:?} for {}; the list offers {offered:?}, and the \
             status strip says {status:?}",
            step.waits_for()
        ));
        true
    }

    /// Presses what the step is there to press, once the window has been
    /// drawn with it: a person presses what they can see. Whether anything
    /// was pressed.
    fn press_when_drawn(&mut self) -> bool {
        use accessibility::Role;
        let (step, drawn) = {
            let shots = self.shots.borrow();
            (shots.step, shots.drawn > shots.drawn_then)
        };
        if !drawn {
            return false;
        }
        let pressed = match step {
            Step::InsertTab => self.press(Role::TabItem, "Insert"),
            Step::ScreenshotButton | Step::ScreenshotAgain => {
                self.press(Role::Button, "Screenshot")
            }
            Step::WholeScreenOffered
                if self.menu().iter().any(|item| item == "The whole screen") =>
            {
                let offered = self.menu();
                self.shots.borrow_mut().offered = offered;
                self.press(Role::MenuItem, "The whole screen")
            }
            Step::ClippingOffered if self.menu().iter().any(|item| item == "Screen Clipping") => {
                if self.person.is_none() {
                    // Any slurp already running is some earlier run's.
                    let before = slurps();
                    let answered = Arc::new(AtomicBool::new(false));
                    let heard = Arc::clone(&answered);
                    let (runtime, display) = (self.runtime.clone(), self.display.clone());
                    let person = std::thread::spawn(move || {
                        the_person_at_slurp(&runtime, &display, &before, &heard)
                    });
                    self.person = Some((answered, person));
                }
                self.press(Role::MenuItem, "Screen Clipping")
            }
            _ => false,
        };
        if pressed {
            self.next();
        }
        pressed
    }

    /// Sees whether the portal's answer, which a step may be waiting for,
    /// came with the tick just handed on: the status strip saying something
    /// it did not say when the step began.
    fn see_the_answer(&mut self, drawn_before_the_tick: u32, took: Duration) {
        let (step, said) = {
            let shots = self.shots.borrow();
            (shots.step, shots.status_then.clone())
        };
        if !matches!(step, Step::WholeScreenAnswered | Step::ClippingAnswered) {
            return;
        }
        let status = self.status();
        if status == said {
            return;
        }
        if step == Step::WholeScreenAnswered {
            let picture = self.editor.document.drawing_at(wp_docx::TextPosition::new(0, 0));
            let mut shots = self.shots.borrow_mut();
            shots.whole = status;
            shots.picture = picture;
            shots.drawn_before_the_shutter = drawn_before_the_tick > shots.drawn_then;
        } else {
            let escape = self.person.take().map(|(answered, person)| {
                answered.store(true, Ordering::SeqCst);
                person.join().unwrap_or(Escape::NoSlurp)
            });
            let mut shots = self.shots.borrow_mut();
            shots.cancelled = status;
            shots.waited = took;
            shots.escape = escape;
        }
        self.next();
    }
}

impl App for Photographing {
    fn handle(&mut self, event: Event) -> Response {
        if event != Event::Tick {
            return self.editor.handle(event);
        }
        let done = self.shots.borrow().step == Step::Done;
        if done || self.gives_up() {
            return Response::Close;
        }
        // Pressed before the tick is handed on, in the same turn of the
        // shell's loop, as a click can be: whatever the press changed has
        // not been drawn when the editor has the tick.
        let pressed = self.press_when_drawn();
        let drawn = self.shots.borrow().drawn;
        let started = Instant::now();
        let response = self.editor.handle(event);
        self.see_the_answer(drawn, started.elapsed());
        // What a press changed is drawn, as the shell draws what a screen
        // reader's press changed.
        if pressed {
            Response::Redraw
        } else {
            response
        }
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
        self.shots.borrow_mut().drawn += 1;
        self.editor.draw(width, height)
    }
}

impl Drop for Photographing {
    fn drop(&mut self) {
        // A test that gave up does not leave the person pressing Escape at
        // a compositor that has gone.
        if let Some((answered, _)) = &self.person {
            answered.store(true, Ordering::SeqCst);
        }
    }
}

/// What became of the person's Escape at slurp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Escape {
    /// slurp went, after Escape had been pressed this many times.
    Heeded(u32),
    /// No slurp came up for the clipping.
    NoSlurp,
    /// slurp stayed through all the pressing, and was stopped.
    Ignored,
}

/// The person at slurp, which the portal starts for the rectangle to be
/// dragged out: they wait for it to come up, look at the screen a while,
/// think better of it and press Escape — again and again until slurp has
/// gone, because slurp is running a while before it has the keyboard, and
/// an Escape pressed before then goes to the editor. Escape used to be
/// pressed once, at a fixed time after the clipping was chosen, and on a
/// busy machine that was too soon: slurp then waited the portal's five
/// minutes for a person who had long since given up.
fn the_person_at_slurp(
    runtime: &Path,
    display: &str,
    before: &[u32],
    answered: &AtomicBool,
) -> Escape {
    let waiting = Instant::now();
    let slurp = loop {
        if let Some(found) = slurps().into_iter().find(|pid| !before.contains(pid)) {
            break found;
        }
        if answered.load(Ordering::SeqCst) || waiting.elapsed() > STEP_BOUND {
            return Escape::NoSlurp;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // Long enough to show that the editor waits for the person, and does
    // not give up on the rectangle by a clock of its own.
    std::thread::sleep(Duration::from_secs(2));
    let pressing = Instant::now();
    let mut presses = 0;
    while is_slurp(slurp) {
        if answered.load(Ordering::SeqCst) || pressing.elapsed() > STEP_BOUND {
            let _ = Command::new("kill").arg(slurp.to_string()).status();
            return Escape::Ignored;
        }
        let _ = Command::new("wtype")
            .args(["-k", "Escape"])
            .env("XDG_RUNTIME_DIR", runtime)
            .env("WAYLAND_DISPLAY", display)
            .env_remove("DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        presses += 1;
        let pressed = Instant::now();
        while is_slurp(slurp) && pressed.elapsed() < Duration::from_millis(500) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    Escape::Heeded(presses)
}

/// Every slurp that is running, by its process number.
fn slurps() -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&process| is_slurp(process))
        .collect()
}

/// Whether a process is slurp and still running. Its `stat` names the
/// program in brackets and then its state, which is `Z` once it has ended
/// and not yet been waited for.
fn is_slurp(process: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{process}/stat")) else {
        return false;
    };
    let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else { return false };
    let state = stat[close + 1..].trim_start().chars().next();
    &stat[open + 1..close] == "slurp" && state.is_some_and(|state| !matches!(state, 'Z' | 'X'))
}

/// Word's Screenshot button on Wayland: the whole screen asked of the
/// desktop's portal and put in the document as a picture, and Screen
/// Clipping offered — the rectangle for the person to drag out, which here
/// they cancel. Held to xdg-desktop-portal and its wlr half, which
/// photograph with grim and let the rectangle be dragged with slurp.
#[test]
fn the_editor_takes_a_screenshot_through_the_desktop_s_portal() {
    use std::io::BufRead;
    let _display = crate::xserver::one_display_at_a_time();
    if !Path::new("/usr/libexec/xdg-desktop-portal-wlr").exists() {
        eprintln!("skipped: no portal for sway on this machine");
        return;
    }
    let Some(compositor) = Compositor::start(1600, 900) else {
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    // The session bus the portal's programs are started into when the
    // first call wakes them, knowing the compositor and that it is sway. A
    // bus of the test's own, so that the portal is cold every time: the
    // shell wakes it as the window comes up, and on a busy machine the
    // Screenshot list opens while it is still starting.
    let Ok(mut bus) = Command::new("dbus-daemon")
        .args(["--session", "--print-address=1", "--nofork", "--nopidfile"])
        .env("XDG_CURRENT_DESKTOP", "sway")
        .env_remove("DISPLAY")
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
    let pipewire = Command::new("pipewire")
        .env("DBUS_SESSION_BUS_ADDRESS", &address)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    // The portal's wlr half will not start without pipewire, and without
    // it the portal takes no screenshots at all: what is waited for is the
    // socket pipewire listens on, rather than a length of time.
    let listening = compositor.runtime.join("pipewire-0");
    let waiting = Instant::now();
    while pipewire.is_ok() && !listening.exists() && waiting.elapsed() < STEP_BOUND {
        std::thread::sleep(Duration::from_millis(20));
    }
    let pipewire_listened = listening.exists();
    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &address);

    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    let mut body = wp_docx::model::Body::default();
    body.blocks.push(wp_docx::model::Block::Paragraph(wp_docx::model::Paragraph::default()));
    let document = Document::create(&body).expect("a document");
    let shots = Rc::new(RefCell::new(Shots::default()));
    let photographing = Photographing {
        editor: Editor::opened(library, document, None::<PathBuf>),
        shots: Rc::clone(&shots),
        runtime: compositor.runtime.clone(),
        display: compositor.display.clone(),
        person: None,
    };
    let options =
        WindowOptions { title: "Document — Word Processor".to_owned(), width: 1600, height: 900 };
    wp_shell::run(options, Box::new(photographing)).expect("the editor's window opens");

    std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
    let _ = Command::new("pkill").arg("-f").arg("xdg-desktop-portal").status();
    if let Ok(mut pipewire) = pipewire {
        let _ = pipewire.kill();
        let _ = pipewire.wait();
    }
    let _ = bus.kill();
    let _ = bus.wait();
    drop(compositor);

    let shots = shots.borrow();
    assert!(pipewire_listened, "pipewire came up, which the portal's wlr half needs");
    assert!(shots.gave_up.is_none(), "{}", shots.gave_up.as_deref().unwrap_or_default());
    assert_eq!(
        shots.offered,
        ["The whole screen", "Screen Clipping"],
        "the screen, no windows — Wayland lists none — and the clipping"
    );
    assert_eq!(shots.whole, "Screenshot, 1600 by 900", "the whole screen went in");
    assert!(shots.picture, "as a picture in the document");
    assert!(
        shots.drawn_before_the_shutter,
        "photographed once the window had been drawn without the list"
    );
    assert!(
        matches!(shots.escape, Some(Escape::Heeded(_))),
        "slurp came up, and went when the person pressed Escape: {:?}",
        shots.escape
    );
    assert_eq!(shots.cancelled, "No picture was taken", "and the clipping was cancelled");
    assert!(
        shots.waited >= Duration::from_secs(2),
        "by the person, after slurp had waited for them: {:?}",
        shots.waited
    );
}

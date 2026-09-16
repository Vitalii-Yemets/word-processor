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
"
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

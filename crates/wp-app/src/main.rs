//! The windowed application.
//!
//! It opens a `.docx`, lays it out, shows the pages, and lets them be edited.
//! Everything on screen is drawn by this project: the archive is unpacked, the
//! XML parsed, the fonts read, the outlines rasterized and the window filled,
//! without a line of third-party code.
//!
//! Editing goes through the document model, which changes only the nodes it
//! must. A file opened here keeps everything this program does not model — a
//! chart, a content control, a colleague's tracked change — even after it has
//! been typed in and saved.

// A release build has no console, which is what a windowed program should look
// like. A debug build keeps one, since that is where diagnostics go.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod actions;
mod autocorrect;
mod chrome;
mod editor;
mod locale;
mod measure;
pub mod messagelist;
pub mod messages;
#[cfg(test)]
mod mirrored;
mod names;
mod sample;
mod settings;
#[cfg(all(test, target_os = "linux"))]
mod wayland;
#[cfg(all(test, target_os = "linux"))]
mod xserver;

use std::path::{Path, PathBuf};

use editor::Editor;
use wp_docx::Document;
use wp_layout::FontLibrary;
use wp_shell::{App, Event, WindowOptions};

fn main() -> std::process::ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();

    // A picture of the window, written to a file instead of shown on a screen.
    // The whole interface is drawn by this program onto a canvas, so it can be
    // drawn without a window at all — which is how it gets checked on a machine
    // that has no display.
    let outcome = match arguments.first().map(String::as_str) {
        Some("--picture") => picture(&arguments[1..]),
        // The desktop's Open on a template, as against its New: the file
        // opened as File ▸ Open opens it.
        Some(wp_shell::files::OPEN_SWITCH) => start(arguments.get(1).map(String::as_str), true),
        _ => start(arguments.first().map(String::as_str), false),
    };

    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Draws the window into a PNG rather than onto a screen.
fn picture(arguments: &[String]) -> Result<(), String> {
    let [document_path, image_path, rest @ ..] = arguments else {
        return Err("usage: --picture <document.docx|-> <image.png> [width height] [option...]
\n             options: light, tab=<file|home|insert|layout|view>,
             file=<info|new|open|saveas|export>, nonav, marks"
            .to_owned());
    };
    let width: usize = rest
        .first()
        .map_or(Ok(1400), |value| value.parse())
        .map_err(|error: std::num::ParseIntError| format!("width is not a number: {error}"))?;
    let height: usize = rest
        .get(1)
        .map_or(Ok(900), |value| value.parse())
        .map_err(|error: std::num::ParseIntError| format!("height is not a number: {error}"))?;

    let library = font_library()?;
    let document = if document_path == "-" {
        Document::create(&sample::welcome_document())
            .map_err(|error| format!("cannot build the sample document: {error}"))?
    } else {
        let bytes = std::fs::read(document_path)
            .map_err(|error| format!("cannot read {document_path}: {error}"))?;
        Document::open(&bytes).map_err(|error| format!("cannot open {document_path}: {error}"))?
    };

    let mut editor = Editor::opened(library, document, None);
    // The size comes first: an option that puts something on the screen has to
    // know how big the screen is before it can decide where.
    editor.handle(Event::Resized { width: width as u32, height: height as u32 });
    // Drawn once before the options are applied, because some of them ask the
    // ribbon where one of its buttons ended up, and a ribbon that has never
    // been drawn does not know.
    editor.draw(width, height);

    // The options exist because this is the only way the interface can be
    // looked at on a machine with no display, and a picture of one theme and
    // one tab would leave most of it unseen.
    for option in rest.iter().skip(2) {
        // Drawn again between one option and the next, because an option may
        // depend on where the last one put something: `menu=accept` hangs a
        // list under a button that `tab=review` has only just brought out.
        editor.draw(width, height);
        editor.set_view_option(option)?;
    }

    let canvas = editor.draw(width, height);

    std::fs::write(image_path, wp_raster::encode_png(canvas))
        .map_err(|error| format!("cannot write {image_path}: {error}"))
}

/// The fonts on this machine.
///
/// Given the lifetime of the process: the layout engine and the renderer both
/// borrow from it for as long as the window is open, and threading a lifetime
/// through every type would buy nothing, because nothing is freed anyway.
fn font_library() -> Result<&'static FontLibrary, String> {
    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    if library.is_empty() {
        return Err("no usable fonts were found on this machine".to_owned());
    }
    Ok(library)
}

fn start(path: Option<&str>, as_opened: bool) -> Result<(), String> {
    if !wp_shell::is_supported() {
        return Err(
            "this build has no window support; Windows, and Linux under X11 or Wayland, are the desktops it runs on".to_owned()
        );
    }

    let library = font_library()?;
    // What is read once the window is up, the way the Open command reads it,
    // with a blank document standing in until then. A text file has no
    // package to open and is read through the same dialog the Open command
    // uses; a page likewise; a PDF too, since Word says what it is about to
    // do to one first, and the message needs a window to belong to. A file
    // of no bytes is what the desktop's New menu makes, and is a blank
    // document that saves to it. And anything the desktop's Open asked for
    // on a template, which File ▸ Open opens as the template itself.
    let later = path.filter(|path| {
        let path = Path::new(path);
        as_opened
            || editor::is_text_path(path)
            || editor::is_web_path(path)
            || editor::is_pdf_path(path)
            || std::fs::metadata(path).is_ok_and(|metadata| metadata.len() == 0)
    });
    let (document, file, title) = match path {
        Some(path) if later.is_some() => {
            let document = Document::create(&wp_docx::model::Body::default())
                .map_err(|error| format!("cannot make a document: {error}"))?;
            (document, None, file_name(path))
        }
        Some(path) => {
            let bytes =
                std::fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))?;
            if editor::is_doc_path(Path::new(path)) {
                let document =
                    wp_doc::open(&bytes).map_err(|error| format!("cannot open {path}: {error}"))?;
                (document, Some(PathBuf::from(path)), file_name(path))
            } else if editor::is_rtf_path(Path::new(path)) {
                let document =
                    wp_rtf::open(&bytes).map_err(|error| format!("cannot open {path}: {error}"))?;
                (document, Some(PathBuf::from(path)), file_name(path))
            } else if editor::is_odt_path(Path::new(path)) {
                let document =
                    wp_odt::open(&bytes).map_err(|error| format!("cannot open {path}: {error}"))?;
                (document, Some(PathBuf::from(path)), file_name(path))
            } else if editor::is_template_path(Path::new(path)) {
                // A template given to the program is a document to make from
                // it, which is what Word does with one double-clicked: the
                // template stays as it was, and the new document is untitled.
                let document = Document::from_template(&bytes, Some(path))
                    .map_err(|error| format!("cannot open {path}: {error}"))?;
                (document, None, "Document".to_owned())
            } else {
                // A document that says so takes its template's styles as it
                // opens, however it is opened.
                let document = Document::open(&bytes)
                    .map(editor::with_template_styles)
                    .map_err(|error| format!("cannot open {path}: {error}"))?;
                (document, Some(PathBuf::from(path)), file_name(path))
            }
        }
        None => {
            // Started with no file, so there is something to look at rather than
            // an empty window.
            let document = Document::create(&sample::welcome_document())
                .map_err(|error| format!("cannot build the sample document: {error}"))?;
            (document, None, "Document".to_owned())
        }
    };

    let mut editor = Editor::opened(library, document, file);
    // Anything a run that did not end left behind, offered before the person
    // has touched anything — which is the only moment at which it is news.
    editor.show_recovered(editor::autorecover::found());
    if let Some(path) = later {
        editor.open_path(Path::new(path));
    }
    // The window comes up the way it was left rather than the way it starts.
    editor.apply_settings(settings::Settings::load());
    let options =
        WindowOptions { title: format!("{title} — Word Processor"), width: 1400, height: 900 };

    wp_shell::run(options, Box::new(editor)).map_err(|error| error.to_string())
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path).to_owned()
}

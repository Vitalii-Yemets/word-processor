//! The Help tab: training, what is new, and where to report a problem.
//!
//! # Why these open documents rather than web pages
//!
//! Because this is a word processor. Everything it has to say about itself is
//! text with headings and lists in it, which is exactly what it is for — and a
//! guide that opens in the program it describes can be read without leaving it,
//! searched with the program's own search, and printed.
//!
//! Feedback is the exception: a bug report has to reach somebody, and that
//! somebody is not on this machine. It goes to the desktop, which knows what
//! the person opens web pages with.

use wp_docx::model::{Block, Body, Paragraph, ParagraphProperties, Run};
use wp_docx::Document;
use wp_shell::Response;

use super::Editor;

/// Where a problem with the program is reported.
const ISSUES: &str = "https://github.com/Vitalii-Yemets/word-processor/issues";

impl Editor {
    /// Opens the guide to using the program.
    pub(super) fn show_training(&mut self) -> Response {
        self.open_written(training(), "Training")
    }

    /// Opens the list of what this version can do.
    pub(super) fn show_whats_new(&mut self) -> Response {
        self.open_written(whats_new(), "What's New")
    }

    /// Hands the place to report a problem to the desktop.
    pub(super) fn send_feedback(&mut self) -> Response {
        if wp_shell::desktop::open(ISSUES) {
            return self.report(&format!("Opened {ISSUES}"));
        }
        // Refused, or there is no desktop to ask. The address is still worth
        // saying, because it can be typed.
        self.report(&format!("Report problems at {ISSUES}"))
    }

    /// Puts a written-here document in the window, keeping the open one safe.
    fn open_written(&mut self, body: Body, name: &str) -> Response {
        if !self.may_discard() {
            return Response::Ignored;
        }
        match Document::create(&body) {
            Ok(document) => {
                self.set_document(document, None);
                self.report(name)
            }
            Err(error) => self.report(&format!("{name} could not be opened: {error}")),
        }
    }
}

/// A heading of the given level.
fn heading(level: u8, text: &str) -> Block {
    Block::Paragraph(Paragraph {
        properties: ParagraphProperties {
            style: Some(format!("Heading{level}")),
            ..ParagraphProperties::default()
        },
        runs: vec![Run::text(text)],
    })
}

/// An ordinary paragraph.
fn line(text: &str) -> Block {
    Block::Paragraph(Paragraph::text(text))
}

/// One step of a guide: what to press, and what it does.
fn step(keys: &str, what: &str) -> Block {
    Block::Paragraph(Paragraph {
        properties: ParagraphProperties {
            indent_start: Some(360),
            space_after: Some(60),
            ..ParagraphProperties::default()
        },
        runs: vec![
            Run {
                properties: wp_docx::model::RunProperties {
                    bold: Some(true),
                    ..wp_docx::model::RunProperties::default()
                },
                content: vec![wp_docx::model::RunContent::Text(format!("{keys}  "))],
                field: None,
                revision: None,
            },
            Run::text(what),
        ],
    })
}

/// The guide.
fn training() -> Body {
    let mut body = Body::default();
    body.blocks.push(heading(1, "Using this word processor"));
    body.blocks.push(line(
        "Everything below works on this document. Type into it, try the keys, \
         and close it without saving when you are done.",
    ));

    body.blocks.push(heading(2, "Writing"));
    body.blocks.push(step("Ctrl+B, Ctrl+I, Ctrl+U", "bold, italic, underline"));
    body.blocks.push(step("Ctrl+Alt+1 to 6", "make the paragraph a heading of that level"));
    body.blocks.push(step("Ctrl+Shift+N", "make it ordinary text again"));
    body.blocks.push(step("Ctrl+Left, Ctrl+Right", "move a whole word at a time"));
    body.blocks.push(step("Ctrl+Backspace", "delete the word before the caret"));
    body.blocks.push(step("Ctrl+Z, Ctrl+Y", "undo and redo"));

    body.blocks.push(heading(2, "Finding things"));
    body.blocks.push(step("Ctrl+F", "find text; Enter steps through the matches"));
    body.blocks.push(step("Ctrl+H", "find and replace"));
    body.blocks.push(step("View ▸ Navigation", "the headings of the document, to jump about by"));

    body.blocks.push(heading(2, "Laying out the page"));
    body.blocks.push(line(
        "The rulers along the top and side are draggable: the grey ends set the margins \
         and the markers set the paragraph indents.",
    ));
    body.blocks.push(step("Layout ▸ Margins, Size, Columns", "the shape of the paper"));
    body.blocks.push(step("Insert ▸ Header, Footer, Page Number", "what repeats on every page"));

    body.blocks.push(heading(2, "Working with other people"));
    body.blocks.push(step("Review ▸ Track Changes", "record what is changed, and by whom"));
    body.blocks.push(step("Review ▸ New Comment", "leave a note in the margin"));
    body.blocks.push(step("Review ▸ Compare", "show what changed between two documents"));
    body.blocks.push(step("Review ▸ Block Authors", "lock the selection while you work on it"));

    body.blocks.push(heading(2, "Looking at the document"));
    body.blocks.push(step("View ▸ Outline", "the headings alone, a level at a time"));
    body.blocks.push(step("View ▸ Side to Side", "turn the pages across instead of down"));
    body.blocks.push(step("View ▸ Split", "two views of the same document, scrolled apart"));
    body.blocks.push(step("Ctrl+wheel", "zoom in and out"));
    body
}

/// The list of what the program does.
fn whats_new() -> Body {
    let mut body = Body::default();
    body.blocks.push(heading(1, "What this version does"));
    body.blocks.push(line(
        "A word processor written from nothing: the archive, the XML, the fonts, \
         the rasterizer, the layout and the window are all in this program. \
         It reads and writes the same .docx files Word does.",
    ));

    body.blocks.push(heading(2, "The document"));
    body.blocks.push(line(
        "Styles and themes, headings, lists, tables, pictures, shapes and text boxes, \
         headers and footers, footnotes and endnotes, bookmarks and cross-references, \
         a table of contents, an index, a table of authorities, captions and citations.",
    ));

    body.blocks.push(heading(2, "Writing"));
    body.blocks.push(line(
        "Tracked changes and comments, spelling against a word list you supply, \
         word count, an accessibility check, and comparing two documents.",
    ));

    body.blocks.push(heading(2, "New in this version"));
    body.blocks.push(line("Text effects: shadow, outline, glow and reflection."));
    body.blocks.push(line("Envelopes and sheets of labels."));
    body.blocks.push(line("Match Fields, for telling a mail merge which column is which."));
    body.blocks.push(line("Outline view, with the levels shown one at a time."));
    body.blocks.push(line("Side to Side page movement, and Split for two views at once."));
    body.blocks.push(line("Screenshots of the desktop or of one window."));
    body.blocks.push(line("Block Authors, to lock part of a document while you work on it."));
    body.blocks.push(line(
        "The window now remembers how it was left: the theme, the rulers, \
         the navigation pane and the zoom.",
    ));

    body.blocks.push(heading(2, "Not here yet"));
    body.blocks.push(line(
        "SmartArt, charts, equations, embedded objects and macros. \
         Translation needs a service to translate with, and this program talks to nobody.",
    ));
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_is_a_document_that_can_be_made_and_saved() {
        let document = Document::create(&training()).expect("a document");
        document.save().expect("saving");
        assert!(document.plain_text().contains("Ctrl+B"));
    }

    #[test]
    fn what_is_new_is_a_document_that_can_be_made_and_saved() {
        let document = Document::create(&whats_new()).expect("a document");
        document.save().expect("saving");
        assert!(document.plain_text().contains("Text effects"));
    }

    #[test]
    fn both_start_with_a_heading() {
        for body in [training(), whats_new()] {
            let Block::Paragraph(first) = &body.blocks[0] else { panic!("a paragraph") };
            assert_eq!(first.properties.style.as_deref(), Some("Heading1"));
        }
    }

    #[test]
    fn the_place_to_report_a_problem_is_one_the_desktop_would_take() {
        assert!(wp_shell::desktop::is_safe_to_open(ISSUES), "{ISSUES}");
    }
}

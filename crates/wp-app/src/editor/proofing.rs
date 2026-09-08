//! What is wrong with the writing, underlined and walked through.
//!
//! # Why the underlining is drawn here and not laid out
//!
//! Because a mistake is not part of the document. It is something noticed about
//! the document, and it changes as the words change without a single character
//! moving. So the marks are drawn over the page from the same ranges the
//! selection is drawn from, and the layout knows nothing about them.

use wp_docx::proofing::{Dictionary, Issue, Kind};
use wp_docx::TextPosition;
use wp_shell::dialog::FileFilter;
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

impl Editor {
    /// Works out what is wrong with the writing, if it is being shown.
    ///
    /// Kept beside the pages rather than worked out while drawing, because it
    /// is the same answer for every frame until the document changes.
    pub(super) fn recheck_proofing(&mut self) {
        if !self.show_proofing {
            self.issues = Vec::new();
            return;
        }
        self.issues = self.document.proofing_issues(&self.dictionary);
    }

    /// Shows or hides the underlining.
    pub(super) fn toggle_proofing(&mut self) -> Response {
        self.show_proofing = !self.show_proofing;
        self.recheck_proofing();
        self.needs_redraw = true;

        if !self.show_proofing {
            return self.report("Proofing marks hidden");
        }
        let count = self.issues.len();
        self.report(&match count {
            0 => "Nothing to correct".to_owned(),
            1 => "One thing to look at".to_owned(),
            many => format!("{many} things to look at"),
        })
    }

    /// Goes to the next thing wrong and offers what to do about it.
    pub(super) fn next_issue(&mut self) -> Response {
        if !self.show_proofing {
            self.show_proofing = true;
            self.recheck_proofing();
        }
        if self.issues.is_empty() {
            return self.report("Nothing to correct");
        }

        let caret = self.document.caret();
        let next = self
            .issues
            .iter()
            .find(|issue| (issue.paragraph, issue.start) > (caret.paragraph, caret.offset))
            .or_else(|| self.issues.first())
            .cloned();
        let Some(issue) = next else { return Response::Ignored };

        // The mistake is selected, so what the correction replaces is plain to
        // see before it is accepted.
        self.document.move_caret(TextPosition::new(issue.paragraph, issue.start), false);
        self.document.move_caret(TextPosition::new(issue.paragraph, issue.end), true);
        self.reveal_caret();
        self.needs_redraw = true;

        match &issue.suggestion {
            Some(_) => self.open_correction(&issue),
            None => self.report(&format!("{}: {}", issue.kind.message(), issue.text)),
        }
    }

    /// Drops open what can be done about one mistake.
    fn open_correction(&mut self, issue: &Issue) -> Response {
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Spelling) else {
            return Response::Ignored;
        };

        let mut items = Vec::new();
        if let Some(suggestion) = &issue.suggestion {
            items.push(if suggestion.is_empty() {
                "Delete".to_owned()
            } else {
                format!("Change to “{suggestion}”")
            });
        }
        items.push("Ignore".to_owned());
        if issue.kind.is_spelling() {
            items.push("Add to Dictionary".to_owned());
        }

        self.pending_issue = Some(issue.clone());
        self.popup = Some(Popup::new(Choice::Correction, items, None, left, top, 300.0));
        self.needs_redraw = true;
        self.report(issue.kind.message())
    }

    /// Does what was chosen about the mistake.
    pub(super) fn choose_correction(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(issue) = self.pending_issue.take() else { return Response::Ignored };

        // The list is: the correction if there is one, then Ignore, then adding
        // the word when it is a spelling.
        let has_suggestion = issue.suggestion.is_some();
        let at = if has_suggestion { index } else { index + 1 };

        match at {
            0 => {
                let Some(suggestion) = issue.suggestion else { return Response::Ignored };
                let changed = self.document.paste(&suggestion);
                self.relayout();
                self.recheck_proofing();
                self.reveal_caret();
                self.edited(changed, "Corrected")
            }
            1 => {
                self.document.clear_selection();
                self.needs_redraw = true;
                self.report("Left as it is")
            }
            _ => {
                self.dictionary.add(&issue.text);
                self.document.clear_selection();
                self.recheck_proofing();
                self.needs_redraw = true;
                self.report(&format!("{} added to the dictionary", issue.text))
            }
        }
    }

    /// Reads a list of words to check spelling against.
    pub(super) fn load_dictionary(&mut self) -> Response {
        let filters = [
            FileFilter { label: "Word lists", pattern: "*.dic;*.txt" },
            FileFilter { label: "All files", pattern: "*.*" },
        ];
        let Some(path) = wp_shell::dialog::open_file("Open a word list", &filters) else {
            return Response::Ignored;
        };

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => return self.report(&format!("The list could not be read: {error}")),
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        self.dictionary = Dictionary::parse(&text);
        self.recheck_proofing();
        self.needs_redraw = true;

        self.report(&format!("{} words loaded", self.dictionary.len()))
    }

    /// Draws a wavy line under everything that is wrong.
    pub(super) fn draw_proofing_marks(&mut self) {
        if self.issues.is_empty() {
            return;
        }

        // Worked out first: drawing holds the canvas, and the pages cannot be
        // asked anything while it does.
        let mut marks: Vec<(f32, f32, f32, bool)> = Vec::new();
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            if top > self.view_height as f32 || top + self.pages[index].height < 0.0 {
                continue;
            }

            for issue in &self.issues {
                let start = TextPosition::new(issue.paragraph, issue.start);
                let end = TextPosition::new(issue.paragraph, issue.end);
                for (x, y, width, height) in self.pages[index].selection_rects(start, end) {
                    marks.push((
                        origin_x + x,
                        top + y + height - 1.0,
                        width,
                        issue.kind.is_spelling(),
                    ));
                }
            }
        }

        for (x, y, width, spelling) in marks {
            let colour = if spelling { self.theme.danger } else { self.theme.marks };
            squiggle(&mut self.canvas, x, y, width, colour);
        }
    }
}

/// Draws a wavy line, which is how a mistake is marked and has been for thirty
/// years.
///
/// Two pixels up, two pixels down, over and over: any smoother and it is a
/// line, any coarser and it is a row of dots.
fn squiggle(canvas: &mut wp_raster::Canvas, x: f32, y: f32, width: f32, colour: wp_raster::Color) {
    if width <= 0.0 {
        return;
    }
    let steps = width as i32;
    for step in 0..steps {
        // Up, up, down, down — a triangle wave four pixels long.
        let lift = match step % 4 {
            0 | 2 => 0,
            1 => -1,
            _ => 1,
        };
        canvas.fill_rect(x as i32 + step, y as i32 + lift, 1, 1, colour);
    }
}

/// Kept so this module names what it walks over.
const _: fn(Kind) -> bool = Kind::is_spelling;

impl Editor {
    /// Drops open what would stop somebody reading the document.
    pub(super) fn open_accessibility(&mut self) -> Response {
        if self.close_popup_if(Choice::Accessibility) {
            return Response::Redraw;
        }
        let findings = self.document.accessibility_findings();
        if findings.is_empty() {
            return self.report("Nothing found — the document reads well");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::CheckAccessibility) else {
            return Response::Ignored;
        };

        let items = findings
            .iter()
            .map(|finding| format!("{}: {}", finding.severity.label(), finding.problem))
            .collect();
        self.accessibility = findings;
        self.popup = Some(Popup::new(Choice::Accessibility, items, None, left, top, 380.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Goes to whatever was chosen and says what to do about it.
    pub(super) fn choose_accessibility(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(finding) = self.accessibility.get(index).cloned() else {
            return Response::Ignored;
        };

        if let Some(paragraph) = finding.paragraph {
            self.document.set_caret(TextPosition::new(paragraph, 0));
            self.reveal_caret();
        }
        self.needs_redraw = true;
        self.report(finding.advice)
    }
}

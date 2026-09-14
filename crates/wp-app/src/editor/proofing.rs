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
        // Looked for the first time anything is checked rather than at
        // startup: reading fifty thousand words is not work to do before the
        // window is even shown.
        self.find_dictionary();
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

    /// Finds a dictionary on this machine for the language being written in.
    ///
    /// No dictionary is shipped with this program: they are data with their own
    /// licences, exactly as typefaces are. What can be checked is whatever the
    /// machine already has — on Linux whatever was installed beside
    /// LibreOffice, on Windows whatever the reader has put beside the program —
    /// and nothing at all where there is none, which is the honest answer for a
    /// program with no words.
    pub(super) fn find_dictionary(&mut self) {
        if self.dictionary_searched {
            return;
        }
        self.dictionary_searched = true;

        let installed = wp_dict::installed();
        if installed.is_empty() {
            return;
        }

        // The language the writing is in, where one of them is for it. The
        // tag is written `en-GB` in a document and `en_GB` in a file name, and
        // a dictionary for the language without the country will do where
        // there is none for the country.
        let wanted = self.document.language_here().replace('-', "_").to_lowercase();
        let base = wanted.split('_').next().unwrap_or_default().to_owned();
        let chosen = installed
            .iter()
            .find(|(name, _)| name.to_lowercase() == wanted)
            .or_else(|| {
                installed.iter().find(|(name, _)| {
                    name.split('_').next().unwrap_or_default().eq_ignore_ascii_case(&base)
                })
            })
            .or_else(|| installed.first());

        let Some((name, path)) = chosen else { return };
        match wp_dict::read_pair(path) {
            Ok(words) => {
                self.dictionary = Dictionary::from_words(words);
                self.dictionary_name = Some(name.clone());
            }
            Err(_) => self.dictionary_name = None,
        }
    }

    /// Opens a dictionary by hand.
    ///
    /// A real one is two files — the word list and the affix rules that say
    /// what forms its words take — and choosing the word list finds the rules
    /// beside it. A plain list of words is still read as a list of words,
    /// because somebody's own list of names and jargon is exactly that.
    pub(super) fn load_dictionary(&mut self) -> Response {
        let filters = [
            FileFilter { label: "Dictionaries", pattern: "*.dic;*.txt" },
            FileFilter { label: "All files", pattern: "*.*" },
        ];
        let Some(path) = wp_shell::dialog::open_file("Open a dictionary", &filters) else {
            return Response::Ignored;
        };

        self.dictionary_searched = true;
        let rules = path.with_extension("aff");
        if rules.exists() {
            return match wp_dict::read_pair(&path) {
                Ok(words) => {
                    self.dictionary = Dictionary::from_words(words);
                    self.dictionary_name =
                        path.file_stem().map(|name| name.to_string_lossy().into_owned());
                    self.recheck_proofing();
                    self.needs_redraw = true;
                    let name = self.dictionary_name.clone().unwrap_or_default();
                    self.report(&format!("{name}: {} words and their forms", self.dictionary.len()))
                }
                Err(error) => self.report(&format!("{error}")),
            };
        }

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => return self.report(&format!("The list could not be read: {error}")),
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        self.dictionary = Dictionary::parse(&text);
        self.dictionary_name = None;
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

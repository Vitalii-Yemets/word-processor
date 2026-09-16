//! What is wrong with the writing, underlined and walked through.
//!
//! # Why the underlining is drawn here and not laid out
//!
//! Because a mistake is not part of the document. It is something noticed about
//! the document, and it changes as the words change without a single character
//! moving. So the marks are drawn over the page from the same ranges the
//! selection is drawn from, and the layout knows nothing about them.

use crate::messages::t;
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
        self.find_dictionaries();
        self.issues =
            self.document.proofing_issues_cached(&self.dictionaries, &mut self.proofing_cache);
        // What the reader said to leave alone, taken out after the checking
        // rather than before it, so that the cache holds the whole answer.
        if !self.ignored_findings.is_empty() {
            self.issues.retain(|issue| {
                issue.kind.is_spelling()
                    || !self
                        .ignored_findings
                        .contains(&(issue.kind.message().to_owned(), issue.text.clone()))
            });
        }
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

        if issue.suggestion.is_some() || issue.kind.is_spelling() {
            self.open_correction(&issue)
        } else {
            self.report(&format!("{}: {}", issue.kind.message(), issue.text))
        }
    }

    /// Drops open what can be done about one mistake.
    fn open_correction(&mut self, issue: &Issue) -> Response {
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Spelling) else {
            return Response::Ignored;
        };

        // The spellings on offer first, as Word's pane lists them; then what
        // else can be done. For a mistake that is not a spelling there is one
        // correction, or none, and no dictionary to add to.
        self.pending_spellings = if issue.kind.is_spelling() {
            self.spellings_for(issue)
        } else {
            issue.suggestion.iter().cloned().collect()
        };
        let mut items: Vec<String> = self
            .pending_spellings
            .iter()
            .map(|spelling| {
                if spelling.is_empty() {
                    "Delete".to_owned()
                } else {
                    format!("Change to “{spelling}”")
                }
            })
            .collect();
        items.push("Ignore".to_owned());
        if issue.kind.is_spelling() {
            items.push("Ignore All".to_owned());
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
        let Some(issue) = self.pending_issue.clone() else { return Response::Ignored };

        // The list is: the spellings on offer, then Ignore, then — for a
        // spelling — Ignore All and adding the word.
        let offered = self.pending_spellings.len();
        if index < offered {
            return self.take_spelling(index);
        }
        self.pending_issue = None;
        match index - offered {
            0 if !issue.kind.is_spelling() => self.ignore_finding(&issue),
            0 => {
                self.document.clear_selection();
                self.needs_redraw = true;
                self.report("Left as it is")
            }
            1 if issue.kind.is_spelling() => self.ignore_everywhere(&issue.text),
            _ => self.add_to_dictionary(&issue.text),
        }
    }

    /// Finds dictionaries on this machine for the languages the document is
    /// written in.
    ///
    /// No dictionary is shipped with this program: they are data with their own
    /// licences, exactly as typefaces are. What can be checked is whatever the
    /// machine already has — on Linux whatever was installed beside
    /// LibreOffice, on Windows whatever the reader has put beside the program —
    /// and nothing at all where there is none, which is the honest answer for a
    /// program with no words.
    ///
    /// Asked again whenever the document is checked, because a run marked in
    /// another language may have been pasted in since; but each language is
    /// looked for once, so a language with no dictionary costs nothing after
    /// the first time.
    pub(super) fn find_dictionaries(&mut self) {
        let mut wanted = self.document.languages_used();
        wanted.push(self.document.language_here());

        for tag in wanted {
            let key = tag.replace('_', "-").to_lowercase();
            if self.dictionaries.has_language(&key) || self.languages_searched.contains(&key) {
                continue;
            }
            self.languages_searched.insert(key.clone());

            // The tag is written `en-GB` in a document and `en_GB` in a file
            // name, and a dictionary for the language without the country will
            // do where there is none for the country.
            let installed = wp_dict::installed();
            let base = key.split('-').next().unwrap_or_default().to_owned();
            let found = installed
                .iter()
                .find(|(name, _)| name.replace('_', "-").to_lowercase() == key)
                .or_else(|| {
                    installed.iter().find(|(name, _)| {
                        name.split('_').next().unwrap_or_default().eq_ignore_ascii_case(&base)
                    })
                });
            let Some((name, path)) = found else { continue };
            if let Ok(words) = wp_dict::read_pair(path) {
                let mut dictionary = Dictionary::from_words(words);
                for word in &self.custom_words {
                    dictionary.add(word);
                }
                self.dictionaries.insert(&name.replace('_', "-"), dictionary);
                self.proofing_cache.clear();
            }
        }
    }

    /// Reads the reader's own dictionary: the words "Add to Dictionary" has
    /// added over the years, kept beside the settings and outliving every
    /// document.
    pub(super) fn load_custom_dictionary(&mut self) {
        let Some(path) = Self::custom_dictionary_path() else { return };
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        for line in text.lines() {
            let word = line.trim();
            if !word.is_empty() {
                self.custom_words.push(word.to_owned());
            }
        }
    }

    /// Where the reader's own dictionary lives: beside the settings, as Word
    /// keeps its `CUSTOM.DIC` beside its own.
    fn custom_dictionary_path() -> Option<std::path::PathBuf> {
        Some(crate::settings::Settings::path()?.with_file_name("custom.dic"))
    }

    /// Adds a word to the dictionary for good.
    ///
    /// To the file as well as to memory, because "Add to Dictionary" means
    /// every document from now on and not this one until it is closed.
    pub(super) fn add_to_dictionary(&mut self, word: &str) -> Response {
        self.dictionaries.add(word);
        self.proofing_cache.clear();
        if !self.custom_words.iter().any(|held| held == word) {
            self.custom_words.push(word.to_owned());
            if let Some(path) = Self::custom_dictionary_path() {
                if let Some(directory) = path.parent() {
                    let _ = std::fs::create_dir_all(directory);
                }
                let _ = std::fs::write(&path, self.custom_words.join("\n") + "\n");
            }
        }
        self.document.clear_selection();
        self.recheck_proofing();
        self.needs_redraw = true;
        self.report(&format!("{word} added to the dictionary"))
    }

    /// Leaves a word alone everywhere in this document, for as long as it is
    /// open.
    pub(super) fn ignore_everywhere(&mut self, word: &str) -> Response {
        self.dictionaries.ignore(word);
        self.proofing_cache.clear();
        self.document.clear_selection();
        self.recheck_proofing();
        self.needs_redraw = true;
        self.report(&format!("{word} ignored"))
    }

    /// The mistake under a point, if the checker has marked one there.
    #[must_use]
    pub(super) fn issue_at(&self, at: TextPosition) -> Option<Issue> {
        self.issues
            .iter()
            .find(|issue| {
                issue.paragraph == at.paragraph
                    && at.offset >= issue.start
                    && at.offset <= issue.end
            })
            .cloned()
    }

    /// What the writer probably meant by a misspelled word, in the language
    /// it is written in.
    #[must_use]
    pub(super) fn spellings_for(&self, issue: &Issue) -> Vec<String> {
        if !issue.kind.is_spelling() {
            return Vec::new();
        }
        let language = self.document.language_at(TextPosition::new(issue.paragraph, issue.start));
        self.dictionaries.suggest(&issue.text, &language)
    }

    /// Puts a spelling in place of the mistake it was offered for.
    pub(super) fn take_spelling(&mut self, index: usize) -> Response {
        let Some(issue) = self.pending_issue.take() else { return Response::Ignored };
        let Some(spelling) = self.pending_spellings.get(index).cloned() else {
            return Response::Ignored;
        };
        self.document.move_caret(TextPosition::new(issue.paragraph, issue.start), false);
        self.document.move_caret(TextPosition::new(issue.paragraph, issue.end), true);
        let changed = self.document.paste(&spelling);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, "Corrected")
    }

    /// Opens a dictionary by hand.
    ///
    /// A real one is two files — the word list and the affix rules that say
    /// what forms its words take — and choosing the word list finds the rules
    /// beside it. It is filed under the language its name says, `en_GB.dic`
    /// being English, so that text in that language is checked against it. A
    /// plain list of words is still read as a list of words, because somebody's
    /// own list of names and jargon is exactly that, and is used for whatever
    /// language has nothing better.
    pub(super) fn load_dictionary(&mut self) -> Response {
        let filters = [
            FileFilter { label: "Dictionaries", pattern: "*.dic;*.txt" },
            FileFilter { label: "All files", pattern: "*.*" },
        ];
        let Some(path) = wp_shell::dialog::open_file(t("Open a dictionary"), &filters) else {
            return Response::Ignored;
        };
        let name = path.file_stem().map(|name| name.to_string_lossy().into_owned());

        if path.with_extension("aff").exists() {
            return match wp_dict::read_pair(&path) {
                Ok(words) => {
                    let mut dictionary = Dictionary::from_words(words);
                    for word in &self.custom_words {
                        dictionary.add(word);
                    }
                    let count = dictionary.len();
                    let tag = name.clone().unwrap_or_default().replace('_', "-");
                    self.dictionaries.insert(&tag, dictionary);
                    self.proofing_cache.clear();
                    self.recheck_proofing();
                    self.needs_redraw = true;
                    self.report(&format!("{tag}: {count} words and their forms"))
                }
                Err(error) => self.report(&format!("{error}")),
            };
        }

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => return self.report(&format!("The list could not be read: {error}")),
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let mut dictionary = Dictionary::parse(&text);
        for word in &self.custom_words {
            dictionary.add(word);
        }
        let count = dictionary.len();
        self.dictionaries.insert("", dictionary);
        self.proofing_cache.clear();
        self.recheck_proofing();
        self.needs_redraw = true;

        self.report(&format!("{count} words loaded"))
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
            // Red for a spelling and blue for grammar, which is Word's pair and
            // is how a glance tells which is which.
            let colour = if spelling { self.theme.danger } else { self.theme.grammar };
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

impl Editor {
    /// Leaves the word under the menu alone everywhere.
    pub(super) fn ignore_pending(&mut self) -> Response {
        let Some(issue) = self.pending_issue.take() else { return Response::Ignored };
        if issue.kind.is_spelling() {
            self.ignore_everywhere(&issue.text)
        } else {
            self.ignore_finding(&issue)
        }
    }

    /// Leaves a mistake of grammar alone: the same words in the same place
    /// are not marked again while the document is open.
    pub(super) fn ignore_finding(&mut self, issue: &Issue) -> Response {
        self.ignored_findings.insert((issue.kind.message().to_owned(), issue.text.clone()));
        self.document.clear_selection();
        self.recheck_proofing();
        self.needs_redraw = true;
        self.report("Left as it is")
    }

    /// Adds the word under the menu to the dictionary.
    pub(super) fn add_pending(&mut self) -> Response {
        let Some(issue) = self.pending_issue.take() else { return Response::Ignored };
        self.add_to_dictionary(&issue.text)
    }
}

//! Recording what was done, and doing it again.
//!
//! # What this is, and what Word's macros are
//!
//! Word's macros are Visual Basic. A macro there is a program with variables
//! and loops that can reach into the document object model and do anything at
//! all — and that is also why opening a document with macros in it is a
//! security question, and why most people who press Record Macro want none of
//! it. They want the four things they just did, done again.
//!
//! That is what this records: the buttons pressed and the words typed, in
//! order, replayed the same way. No language, nothing to run but what a person
//! did with their own hands. A document cannot carry one — macros live with the
//! program, in [`crate::settings`] — so opening a file from anywhere can never
//! run anything.
//!
//! # How a command is written down
//!
//! By the name on its button. The ribbon already names every command in
//! English, so that is where the name comes from and where it is read back —
//! see [`crate::chrome::ribbon::name_of`]. A command with no button is not
//! recorded, because there would be nothing to call it.

use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{ribbon, Choice, Command, Popup};
use crate::messages::t;

use super::Editor;

/// One thing that was done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// A button was pressed.
    Run(String),
    /// Something was typed.
    Type(String),
}

impl Step {
    /// How the step is written in the settings file.
    fn written(&self) -> String {
        match self {
            Self::Run(name) => format!("run {name}"),
            Self::Type(text) => format!("type {text}"),
        }
    }

    /// And read back. Anything else is skipped rather than guessed at.
    fn parse(text: &str) -> Option<Self> {
        if let Some(name) = text.strip_prefix("run ") {
            return Some(Self::Run(name.to_owned()));
        }
        text.strip_prefix("type ").map(|typed| Self::Type(typed.to_owned()))
    }
}

/// Steps are separated by this in the settings file, which is a character no
/// button name and no typed line contains.
const BETWEEN: char = '\u{1}';

/// Writes a recording down as one line.
#[must_use]
pub(super) fn write_steps(steps: &[Step]) -> String {
    steps.iter().map(Step::written).collect::<Vec<_>>().join(&BETWEEN.to_string())
}

/// And reads it back.
#[must_use]
pub(super) fn read_steps(line: &str) -> Vec<Step> {
    line.split(BETWEEN).filter_map(Step::parse).collect()
}

/// How many steps one macro may hold.
///
/// A recording somebody forgot to stop should not grow without end, and a
/// hundred steps is longer than any macro anybody records by hand.
const LONGEST: usize = 100;

impl Editor {
    /// Drops open the macros, and the way to record another.
    pub(super) fn open_macros(&mut self) -> Response {
        if self.close_popup_if(Choice::Macro) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Macros) else {
            return Response::Ignored;
        };

        let mut items = Vec::new();
        if self.recording.is_some() {
            items.push("Stop recording and save it…".to_owned());
            items.push("Throw the recording away".to_owned());
        } else {
            items.push("Record a macro".to_owned());
        }
        self.macro_names = self.settings.macro_names();
        items.extend(self.macro_names.iter().map(|name| format!("Run: {name}")));
        // A document with Visual Basic in it must say so here, of all
        // places: a person who opens this list and sees only what they
        // recorded would read it as a document with no macros in it.
        if self.carries_macros {
            items.push(t("This document also carries Visual Basic macros").to_owned());
        }

        self.popup = Some(Popup::new(Choice::Macro, items, None, left, top, 300.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Starts, stops or runs, depending on what was chosen.
    pub(super) fn choose_macro(&mut self, index: usize) -> Response {
        self.popup = None;

        let heading = if self.recording.is_some() { 2 } else { 1 };
        if index >= heading {
            let Some(name) = self.macro_names.get(index - heading).cloned() else {
                // Past the recorded ones is the line about the document's
                // own macros, which is there to be read rather than run.
                return self.report(
                    "This document carries Visual Basic macros. They are kept as they are and not run.",
                );
            };
            return self.play_macro(&name);
        }

        if self.recording.is_none() {
            self.recording = Some(Vec::new());
            return self.report("Recording — press Macros again to stop");
        }
        if index == 1 {
            self.recording = None;
            return self.report("The recording was thrown away");
        }

        // Nothing was done, so there is nothing to name.
        if self.recording.as_ref().is_some_and(Vec::is_empty) {
            self.recording = None;
            return self.report("Nothing was recorded");
        }
        self.find_bar = Some(FindBar::for_purpose(Purpose::Macro));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type a name for the macro, then press Enter")
    }

    /// Saves what was recorded under the name that was typed.
    pub(super) fn finish_macro(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let name = bar.needle.trim().to_owned();
        self.find_bar = None;
        self.needs_redraw = true;

        let Some(steps) = self.recording.take() else { return Response::Ignored };
        if name.is_empty() {
            return self.report("Nothing was named, so the recording was thrown away");
        }

        self.settings.set_macro(&name, &write_steps(&steps));
        self.settings.save();
        self.report(&format!("Macro {name}: {} steps", steps.len()))
    }

    /// Does again what was recorded.
    fn play_macro(&mut self, name: &str) -> Response {
        let Some(line) = self.settings.macro_steps(name) else {
            return self.report(&format!("There is no macro called {name}"));
        };
        let steps = read_steps(&line);
        if steps.is_empty() {
            return self.report(&format!("{name} has nothing in it"));
        }

        // Playing is not recording: a macro that recorded itself would grow
        // every time it ran.
        let held = self.recording.take();
        let mut done = 0usize;
        for step in &steps {
            match step {
                Step::Run(button) => {
                    if let Some(command) = ribbon::command_named(button) {
                        self.run(command);
                        done += 1;
                    }
                }
                Step::Type(text) => {
                    for character in text.chars() {
                        self.type_character(character);
                    }
                    done += 1;
                }
            }
        }
        self.recording = held;

        self.relayout();
        self.reveal_caret();
        self.needs_redraw = true;
        self.report(&format!("{name}: {done} steps"))
    }

    /// Writes a button press into the recording, if one is running.
    pub(super) fn record_command(&mut self, command: Command) {
        // The Macros button itself is how recording is stopped, so recording it
        // would put "stop recording" inside every macro.
        if command == Command::Macros {
            return;
        }
        let Some(name) = ribbon::name_of(command) else { return };
        let Some(steps) = &mut self.recording else { return };
        if steps.len() >= LONGEST {
            return;
        }
        steps.push(Step::Run(name.to_owned()));
    }

    /// And a typed character, joined onto the last run of typing.
    pub(super) fn record_typing(&mut self, character: char) {
        let Some(steps) = &mut self.recording else { return };
        if let Some(Step::Type(text)) = steps.last_mut() {
            text.push(character);
            return;
        }
        if steps.len() >= LONGEST {
            return;
        }
        steps.push(Step::Type(character.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_survives_being_written_and_read_back() {
        let steps = vec![
            Step::Run("Bold".to_owned()),
            Step::Type("Hello".to_owned()),
            Step::Run("Save".to_owned()),
        ];
        assert_eq!(read_steps(&write_steps(&steps)), steps);
    }

    #[test]
    fn a_step_nobody_here_understands_is_skipped_rather_than_guessed_at() {
        let read = read_steps(&format!("run Bold{BETWEEN}fly to the moon{BETWEEN}type hi"));
        assert_eq!(read, vec![Step::Run("Bold".to_owned()), Step::Type("hi".to_owned())]);
    }

    #[test]
    fn typed_text_with_spaces_in_it_comes_back_whole() {
        let steps = vec![Step::Type("a sentence, with punctuation".to_owned())];
        assert_eq!(read_steps(&write_steps(&steps)), steps);
    }

    #[test]
    fn nothing_recorded_reads_back_as_nothing() {
        assert!(read_steps("").is_empty());
    }
}

#[cfg(test)]
mod document_macros {
    use crate::chrome::Command;
    use wp_docx::kinds::Kind;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use crate::editor::Editor;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// A document carrying a Visual Basic project, made the way a test can.
    fn with_macros() -> Document {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("one")));
        let mut document = Document::create(&body).expect("a document");
        document.set_kind(Kind::MacroEnabledDocument);

        let bytes = document.save().expect("saving");
        let mut package = wp_opc::Package::open(&bytes).expect("a package");
        package.add_part(
            "word/vbaProject.bin",
            "application/vnd.ms-office.vbaProject",
            vec![0xD0, 0xCF, 0x11, 0xE0, 1, 2, 3, 4],
        );
        let mut relationships = package.relationships("word/document.xml").expect("relationships");
        relationships.add(
            "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
            "vbaProject.bin",
            wp_opc::TargetMode::Internal,
        );
        package.set_relationships(&relationships).expect("writing them");
        let bytes = package.save().expect("saving the package");
        Document::open(&bytes).expect("reopening")
    }

    fn editor(document: Document) -> Editor {
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    #[test]
    fn a_document_with_visual_basic_in_it_says_so_on_the_macro_list() {
        let mut editor = editor(with_macros());
        editor.set_view_option("tab=view").expect("the View tab");
        editor.draw(1400, 900);
        editor.run(Command::Macros);

        let popup = editor.popup.as_ref().expect("the list");
        let lines: Vec<String> =
            (0..8).filter_map(|at| popup.item(at).map(str::to_owned)).collect();
        assert!(
            lines.iter().any(|line| line.contains("Visual Basic")),
            "the list says nothing about the document's own macros: {lines:?}"
        );
    }

    #[test]
    fn a_document_without_them_says_nothing_about_them() {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("one")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let mut editor = editor(Document::open(&bytes).expect("reopening"));
        editor.set_view_option("tab=view").expect("the View tab");
        editor.draw(1400, 900);
        editor.run(Command::Macros);

        let popup = editor.popup.as_ref().expect("the list");
        let lines: Vec<String> =
            (0..8).filter_map(|at| popup.item(at).map(str::to_owned)).collect();
        assert!(
            !lines.iter().any(|line| line.contains("Visual Basic")),
            "it says a document has macros when it has none: {lines:?}"
        );
    }

    #[test]
    fn pressing_that_line_says_what_happens_to_them() {
        let mut editor = editor(with_macros());
        editor.set_view_option("tab=view").expect("the View tab");
        editor.draw(1400, 900);
        editor.run(Command::Macros);
        // The line about the document's own macros is the last one.
        let popup = editor.popup.as_ref().expect("the list");
        let last = (0..8).filter(|at| popup.item(*at).is_some()).count() - 1;
        editor.choose_macro(last);

        assert!(editor.status.contains("not run"), "{}", editor.status);
        assert!(editor.status.contains("kept"), "{}", editor.status);
    }
}

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

use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{ribbon, Choice, Command, Popup};
use crate::messages::t;

use super::dialogs::Asking;

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

/// The button on the macro dialog that runs it.
///
/// Word's own Macros dialog has one, and this program now has something
/// behind it. Nothing else in the program reaches the interpreter: a document
/// that is opened runs nothing at all, and the gate that will let it is a
/// later item. See [`super::Editor::run_macro`].
pub(super) const RUN: &str = "Run";

/// Which field of the macro dialog holds the module's text, which is the last
/// of them however many there are in front of it.
fn source_field(dialog: &Dialog) -> usize {
    dialog.fields.len().saturating_sub(1)
}

/// How many steps one macro may hold.
///
/// A recording somebody forgot to stop should not grow without end, and a
/// hundred steps is longer than any macro anybody records by hand.
const LONGEST: usize = 100;

/// The Visual Basic project a document carries, read.
///
/// A project that will not read is not an error anybody can act on — the
/// document still carries it, it is still written back untouched, and the
/// list still says so. What is lost is the names, and saying nothing about
/// them is better than saying something wrong about them.
#[must_use]
pub(super) fn project_of(document: &wp_docx::Document) -> Option<wp_vba::Project> {
    let bytes = document.package().part("word/vbaProject.bin")?;
    wp_vba::Project::open(bytes).ok()
}

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
        self.document_macros = self.vba.as_ref().map(wp_vba::Project::macros).unwrap_or_default();
        if self.carries_macros {
            items.push(t("This document also carries Visual Basic macros").to_owned());
            // By name, with the module they are in, which is how Word's own
            // dialog lists them and the only way to tell two `Hello`s apart.
            // Shown rather than run: running one is a later item, and a line
            // that said Run and did not would be worse than no line at all.
            items.extend(
                self.document_macros.iter().map(|one| format!("Show: {}", one.qualified())),
            );
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
            let at = index - heading;
            if let Some(name) = self.macro_names.get(at).cloned() {
                return self.play_macro(&name);
            }
            // Past the recorded ones is the line about the document's own
            // macros, which is there to be read rather than run, and then one
            // line for each of them.
            let at = at - self.macro_names.len();
            if at == 0 {
                return self.report(
                    "This document carries Visual Basic macros. They are kept as they are and not run.",
                );
            }
            let Some(wanted) = self.document_macros.get(at - 1).cloned() else {
                return Response::Ignored;
            };
            return self.show_macro(&wanted);
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

    /// Shows what one of the document's own macros says.
    ///
    /// The whole module, because that is what a macro is written in and what
    /// Word's own editor opens: the lines above it declare what it uses, and
    /// a macro read without them is a macro read out of context. The box
    /// opens at the line that declares the one chosen.
    fn show_macro(&mut self, wanted: &wp_vba::Macro) -> Response {
        let Some(module) = self.vba.as_ref().and_then(|vba| vba.module(&wanted.module)) else {
            return Response::Ignored;
        };
        let lines: Vec<String> = module.source.lines().map(str::to_owned).collect();
        let at = module
            .procedures()
            .iter()
            .find(|one| one.name == wanted.name)
            .map_or(1, |one| one.line)
            - 1;

        let mut fields = vec![
            Field::Said { label: "Macro name".to_owned(), value: wanted.qualified() },
            Field::Said {
                label: "In".to_owned(),
                value: format!("{} ({})", module.name, module.kind.label()),
            },
        ];
        // A line this program could not read is worth saying out loud, where
        // Word's own editor would say it: the line number is what a person
        // needs to find it, and a module shown without the warning would look
        // like one that was understood.
        if let Some(complaint) = module.read().1.first() {
            fields.push(Field::Said {
                label: "Compile error".to_owned(),
                value: complaint.to_string(),
            });
        }
        fields.push(Field::Lines { label: "Source".to_owned(), lines, scroll: 0 });
        let dialog = Dialog::with_buttons(
            "Macro",
            fields,
            vec![
                Button { label: RUN.to_owned(), answer: Answer::Named(RUN), default: false },
                Button { label: "Close".to_owned(), answer: Answer::Accept, default: true },
            ],
        );
        self.showing_macro = Some(wanted.clone());
        let mut dialog = dialog;
        let source = source_field(&dialog);
        dialog.show_line(source, at);
        self.ask(Asking::Macro, dialog)
    }

    /// Runs one of the document's own macros.
    ///
    /// Reachable from here and from nowhere else. Opening a document runs
    /// nothing, and every other way in is behind the gate that is a later
    /// item; this is the one a person asked for by opening the list, finding
    /// the macro and pressing the button.
    ///
    /// What it can do is the language and the document: the text, the
    /// paragraphs, the styles, the formatting, find and replace. What it
    /// cannot do it says by name and stops, because a macro told `Empty` for
    /// a property nobody modelled will carry on and write the wrong thing.
    pub(super) fn run_macro(&mut self, wanted: &wp_vba::Macro) -> Response {
        let Some(module) = self.vba.as_ref().and_then(|vba| vba.module(&wanted.module)).cloned()
        else {
            return Response::Ignored;
        };
        self.dialog = None;
        self.asking = None;
        self.needs_redraw = true;

        let (program, complaints) = wp_vba::run::Program::read(&module.source);
        if let Some(complaint) = complaints.first() {
            return self.report(&crate::messages::with(
                "{0} stopped: {1}",
                &[&wanted.qualified(), &complaint.to_string()],
            ));
        }

        // The document as the macro sees it, which is what it runs against.
        let (answer, mut said) = {
            let mut model = super::objects::Model::new(self);
            let answer = program.run(&wanted.name, Vec::new(), &mut model);
            // What the macro showed is shown afterwards rather than in the
            // middle of it: a message box put up while a macro is running
            // would have to stop the macro to wait for an answer, and
            // stopping in the middle is what the debugger is for, which is a
            // later item.
            (answer, model.said())
        };
        self.relayout();
        self.reveal_caret();
        if let Err(fault) = answer {
            said.push(fault.to_string());
            if !said.is_empty() {
                self.show_what_a_macro_said(wanted, said);
            }
            return self.report(&crate::messages::with(
                "{0} stopped: {1}",
                &[&wanted.qualified(), &fault.to_string()],
            ));
        }
        if !said.is_empty() {
            self.show_what_a_macro_said(wanted, said);
        }
        self.report(&crate::messages::with("{0} ran", &[&wanted.qualified()]))
    }

    /// What a macro showed while it ran.
    fn show_what_a_macro_said(&mut self, wanted: &wp_vba::Macro, said: Vec<String>) {
        let dialog = Dialog::message(
            "Macro",
            vec![
                Field::Said { label: "Macro name".to_owned(), value: wanted.qualified() },
                Field::Lines { label: "Said".to_owned(), lines: said, scroll: 0 },
            ],
        );
        let _ = self.ask(Asking::Macro, dialog);
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
    use crate::chrome::dialog::Field;
    use crate::chrome::Command;
    use wp_docx::kinds::Kind;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

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

    /// The same, carrying a project this program can actually read.
    fn with_a_project() -> Document {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("one")));
        let mut document = Document::create(&body).expect("a document");
        document.set_kind(Kind::MacroEnabledDocument);

        let bytes = document.save().expect("saving");
        let mut package = wp_opc::Package::open(&bytes).expect("a package");
        package.add_part(
            "word/vbaProject.bin",
            "application/vnd.ms-office.vbaProject",
            wp_vba::example(&[
                ("Module1", SOURCE),
                ("ThisDocument", "Attribute VB_Name = \"ThisDocument\"\r\n"),
            ]),
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

    /// A module with one macro in it, and enough lines above and below that
    /// the box showing it cannot show all of them at once.
    const SOURCE: &str = "Attribute VB_Name = \"Module1\"\r\n\
         Option Explicit\r\n\
         \r\n\
         Private Sub NotAMacro(ByVal n As Long)\r\n\
         End Sub\r\n\
         \r\n\
         ' six\r\n\
         ' seven\r\n\
         ' eight\r\n\
         ' nine\r\n\
         ' ten\r\n\
         Public Sub Hello()\r\n    \
             MsgBox \"Hello\"\r\n\
         End Sub\r\n\
         ' fifteen\r\n\
         ' sixteen\r\n";

    fn lines_of(editor: &Editor) -> (Vec<String>, usize) {
        let dialog = editor.dialog.as_ref().expect("the dialog");
        match dialog.fields.iter().find(|field| matches!(field, Field::Lines { .. })) {
            Some(Field::Lines { lines, scroll, .. }) => (lines.clone(), *scroll),
            _ => panic!("the dialog shows no source at all"),
        }
    }

    #[test]
    fn the_macros_a_document_carries_are_listed_by_module_and_name() {
        // Word's own dialog lists them by name; two modules may each have a
        // Hello, so the module is named as well.
        let mut editor = editor(with_a_project());
        editor.set_view_option("tab=view").expect("the View tab");
        editor.draw(1400, 900);
        editor.run(Command::Macros);

        let popup = editor.popup.as_ref().expect("the list");
        let lines: Vec<String> =
            (0..8).filter_map(|at| popup.item(at).map(str::to_owned)).collect();
        assert!(
            lines.iter().any(|line| line.contains("Module1.Hello")),
            "the document's own macro is not on the list: {lines:?}"
        );
        // Not the private one, and not the one that wants an argument: those
        // are not macros anybody could run from a list.
        assert!(
            !lines.iter().any(|line| line.contains("NotAMacro")),
            "a private procedure was offered: {lines:?}"
        );
    }

    #[test]
    fn choosing_one_shows_the_module_it_is_written_in() {
        let mut editor = editor(with_a_project());
        editor.set_view_option("tab=view").expect("the View tab");
        editor.draw(1400, 900);
        editor.run(Command::Macros);

        let popup = editor.popup.as_ref().expect("the list");
        let at = (0..8)
            .find(|at| popup.item(*at).is_some_and(|line| line.contains("Module1.Hello")))
            .expect("the line");
        editor.choose_macro(at);

        let (lines, scroll) = lines_of(&editor);
        assert!(lines.iter().any(|line| line.contains("MsgBox")), "{lines:?}");
        assert!(
            lines.iter().any(|line| line.contains("Option Explicit")),
            "the module is shown whole, not the macro alone: {lines:?}"
        );
        // Opened at the line that declares the one chosen, which is past the
        // eight the box can show.
        assert!(scroll > 0, "a macro on line twelve was shown from the top");
        assert!(scroll <= 11, "the declaration was scrolled past: {scroll}");
    }

    #[test]
    fn the_source_box_reaches_the_lines_below_the_ones_it_shows() {
        // The other half of showing a macro: a box that shows eight lines of
        // sixteen and cannot reach the ninth is hiding what was asked for.
        let mut editor = editor(with_a_project());
        editor.set_view_option("tab=view").expect("the View tab");
        editor.draw(1400, 900);
        editor.run(Command::Macros);
        let popup = editor.popup.as_ref().expect("the list");
        let at = (0..8)
            .find(|at| popup.item(*at).is_some_and(|line| line.contains("Module1.Hello")))
            .expect("the line");
        editor.choose_macro(at);

        // The box of lines is the only thing in that dialog the keyboard
        // can land on, so it already has it.
        let (lines, was) = lines_of(&editor);
        editor.handle(Event::KeyDown { key: Key::Home, modifiers: Modifiers::default() });
        let (_, home) = lines_of(&editor);
        assert_eq!(home, 0, "Home did not reach the first line, from {was}");

        editor.handle(Event::KeyDown { key: Key::End, modifiers: Modifiers::default() });
        let (_, end) = lines_of(&editor);
        assert_eq!(
            end,
            lines.len() - crate::chrome::dialog::LINES_SHOWN,
            "End did not reach the last"
        );

        editor.handle(Event::KeyDown { key: Key::Up, modifiers: Modifiers::default() });
        let (_, up) = lines_of(&editor);
        assert_eq!(up, end - 1, "the arrows do not move it a line at a time");
    }

    /// Opens the list, finds a macro by name and shows it.
    fn showing(editor: &mut Editor, named: &str) {
        editor.set_view_option("tab=view").expect("the View tab");
        editor.draw(1400, 900);
        editor.run(Command::Macros);
        let popup = editor.popup.as_ref().expect("the list");
        let at = (0..8)
            .find(|at| popup.item(*at).is_some_and(|line| line.contains(named)))
            .expect("the line");
        editor.choose_macro(at);
    }

    #[test]
    fn the_run_button_runs_the_one_that_is_showing() {
        // The only way into the interpreter this program has: a person
        // opened the list, found the macro and pressed the button.
        let mut editor = editor(with_a_project());
        editor.vba = wp_vba::Project::open(&wp_vba::example(&[(
            "Module1",
            "Public Sub Hello()\r\n    MsgBox \"Ran \" & (6 * 7)\r\nEnd Sub\r\n",
        )]))
        .ok();
        showing(&mut editor, "Module1.Hello");

        let dialog = editor.dialog.clone().expect("the dialog");
        assert!(
            dialog.buttons.iter().any(|button| button.label == "Run"),
            "there is no way to run it"
        );
        editor.finish_dialog(crate::chrome::dialog::Answer::Named(super::RUN));

        assert!(editor.status.contains("ran"), "{}", editor.status);
        // And what it showed is shown, because a message box put up in the
        // middle of a macro would have to stop it to wait for an answer.
        let (said, _) = lines_of(&editor);
        assert!(said.iter().any(|line| line == "Ran 42"), "{said:?}");
    }

    /// An editor holding a document of three paragraphs and a macro that is
    /// whatever the test needs.
    fn with_macro(source: &str) -> Editor {
        let mut body = Body::default();
        for line in ["one", "two", "three"] {
            body.blocks.push(Block::Paragraph(Paragraph::text(line)));
        }
        let mut document = Document::create(&body).expect("a document");
        document.set_kind(Kind::MacroEnabledDocument);
        let bytes = document.save().expect("saving");
        let mut package = wp_opc::Package::open(&bytes).expect("a package");
        package.add_part(
            "word/vbaProject.bin",
            "application/vnd.ms-office.vbaProject",
            wp_vba::example(&[("Module1", source)]),
        );
        let mut relationships = package.relationships("word/document.xml").expect("relationships");
        relationships.add(
            "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
            "vbaProject.bin",
            wp_opc::TargetMode::Internal,
        );
        package.set_relationships(&relationships).expect("writing them");
        let bytes = package.save().expect("saving the package");
        editor(Document::open(&bytes).expect("reopening"))
    }

    /// Runs the macro called `Hello` and gives back what it showed.
    fn ran(editor: &mut Editor) -> Vec<String> {
        showing(editor, "Module1.Hello");
        editor.finish_dialog(crate::chrome::dialog::Answer::Named(super::RUN));
        let said = editor.dialog.as_ref().map(|_| lines_of(editor).0).unwrap_or_default();
        assert!(!editor.status.contains("stopped"), "{}", editor.status);
        said
    }

    #[test]
    fn a_macro_types_into_the_document_the_way_a_person_would() {
        let mut editor =
            with_macro("Public Sub Hello()\r\n    Selection.TypeText \"typed \"\r\nEnd Sub\r\n");
        ran(&mut editor);
        assert!(
            editor.document.plain_text().starts_with("typed one"),
            "{}",
            editor.document.plain_text()
        );
    }

    #[test]
    fn a_macro_reads_the_document_it_is_in() {
        let mut editor = with_macro(
            "Public Sub Hello()\r\n\
             \x20   MsgBox ActiveDocument.Paragraphs.Count & \":\" & _\r\n\
             \x20       ActiveDocument.Paragraphs(2).Range.Text\r\n\
             End Sub\r\n",
        );
        let said = ran(&mut editor);
        assert!(said.iter().any(|line| line == "3:two"), "{said:?}");
    }

    #[test]
    fn a_macro_walks_the_paragraphs_and_changes_one() {
        let mut editor = with_macro(
            "Public Sub Hello()\r\n\
             \x20   Dim p As Object, count As Long\r\n\
             \x20   For Each p In ActiveDocument.Paragraphs\r\n\
             \x20       count = count + 1\r\n\
             \x20   Next p\r\n\
             \x20   ActiveDocument.Paragraphs(1).Range.Text = \"first\"\r\n\
             \x20   MsgBox count\r\n\
             End Sub\r\n",
        );
        let said = ran(&mut editor);
        assert!(said.iter().any(|line| line == "3"), "{said:?}");
        assert!(
            editor.document.plain_text().starts_with("first"),
            "{}",
            editor.document.plain_text()
        );
    }

    #[test]
    fn a_macro_sets_a_style_and_the_document_keeps_it() {
        let mut editor = with_macro(
            "Public Sub Hello()\r\n    ActiveDocument.Paragraphs(1).Style = \"Heading1\"\r\nEnd Sub\r\n",
        );
        ran(&mut editor);
        assert_eq!(editor.document.style_of(0).as_deref(), Some("Heading1"));
    }

    #[test]
    fn a_macro_finds_and_replaces_as_word_writes_it() {
        // Named arguments, which is how every real macro writes this line.
        let mut editor = with_macro(
            "Public Sub Hello()\r\n\
             \x20   Selection.Find.Execute FindText:=\"two\", ReplaceWith:=\"deux\", _\r\n\
             \x20       Replace:=wdReplaceAll\r\n\
             End Sub\r\n",
        );
        ran(&mut editor);
        assert!(editor.document.plain_text().contains("deux"), "{}", editor.document.plain_text());
        assert!(!editor.document.plain_text().contains("two"), "{}", editor.document.plain_text());
    }

    #[test]
    fn a_macro_turns_bold_on_through_the_font() {
        let mut editor = with_macro(
            "Public Sub Hello()\r\n\
             \x20   ActiveDocument.Paragraphs(1).Range.Select\r\n\
             \x20   Selection.Font.Bold = True\r\n\
             \x20   MsgBox Selection.Font.Bold\r\n\
             End Sub\r\n",
        );
        let said = ran(&mut editor);
        assert!(said.iter().any(|line| line == "True"), "{said:?}");
    }

    #[test]
    fn a_range_counts_characters_the_way_word_counts_them() {
        // Each paragraph mark is one character, so the second paragraph
        // starts four along from the first in a document that begins "one".
        let mut editor = with_macro(
            "Public Sub Hello()\r\n\
             \x20   MsgBox ActiveDocument.Paragraphs(2).Range.Start & \",\" & _\r\n\
             \x20       ActiveDocument.Content.End\r\n\
             End Sub\r\n",
        );
        let said = ran(&mut editor);
        assert!(said.iter().any(|line| line == "4,13"), "{said:?}");
    }

    #[test]
    fn a_property_nobody_modelled_stops_the_macro_and_names_itself() {
        // The rule the whole object model is written under.
        let mut editor =
            with_macro("Public Sub Hello()\r\n    Selection.Shading.Texture = 1\r\nEnd Sub\r\n");
        showing(&mut editor, "Module1.Hello");
        editor.finish_dialog(crate::chrome::dialog::Answer::Named(super::RUN));
        assert!(editor.status.contains("stopped"), "{}", editor.status);
        assert!(editor.status.contains("Shading"), "it did not say which: {}", editor.status);
    }

    #[test]
    fn a_macro_that_stops_leaves_what_it_had_not_reached_alone() {
        // Half a macro is half a macro: what it did before it stopped stays
        // done, as it does in Word, and what came after it does not happen.
        let mut editor = with_macro(
            "Public Sub Hello()\r\n\
             \x20   Selection.TypeText \"done \"\r\n\
             \x20   Selection.Shading.Texture = 1\r\n\
             \x20   Selection.TypeText \"never\"\r\n\
             End Sub\r\n",
        );
        showing(&mut editor, "Module1.Hello");
        editor.finish_dialog(crate::chrome::dialog::Answer::Named(super::RUN));

        assert!(editor.status.contains("stopped"), "{}", editor.status);
        let text = editor.document.plain_text();
        assert!(text.starts_with("done one"), "{text}");
        assert!(!text.contains("never"), "{text}");
    }

    #[test]
    fn a_line_the_parser_could_not_read_is_said_where_word_would_say_it() {
        // Word's editor points at the line and says what it wanted. A module
        // shown without that warning would look like one that was read.
        let mut editor = editor(with_a_project());
        editor.set_view_option("tab=view").expect("the View tab");
        editor.draw(1400, 900);
        editor.vba = wp_vba::Project::open(&wp_vba::example(&[(
            "Module1",
            "Public Sub Hello()\r\n    ]] nonsense\r\nEnd Sub\r\n",
        )]))
        .ok();
        editor.run(Command::Macros);

        let popup = editor.popup.as_ref().expect("the list");
        let at = (0..8)
            .find(|at| popup.item(*at).is_some_and(|line| line.contains("Module1.Hello")))
            .expect("the line");
        editor.choose_macro(at);

        let dialog = editor.dialog.as_ref().expect("the dialog");
        let said: Vec<String> = dialog
            .fields
            .iter()
            .filter_map(|field| match field {
                Field::Said { value, .. } => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert!(
            said.iter().any(|value| value.contains("line 2")),
            "nothing said which line could not be read: {said:?}"
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

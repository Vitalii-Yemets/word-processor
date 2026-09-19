//! The events a document raises, and the macros that answer them.
//!
//! # Two names for one moment
//!
//! Word has two ways of running a macro when something happens to a
//! document, and both are older than anybody who uses them cares. The first
//! is an *auto macro*: a `Sub AutoOpen` in any module, or a `Main` in a
//! module called `AutoOpen`, which runs when the document is opened; and
//! `AutoNew` and `AutoClose` likewise. The second is an *event procedure*:
//! `Private Sub Document_Open()` in the document's own module,
//! `ThisDocument`, and `Document_New` and `Document_Close` beside it. A
//! document may have both, and then both run.
//!
//! Which runs first is not written down where this program could read it,
//! and is not asserted here: the auto macro goes first, and the roadmap
//! says that this is a choice and not a fact.
//!
//! # Through the one gate
//!
//! Nothing here runs unless [`Editor::macros_allowed`] says so — the same
//! question the Run button and F5 ask, with the same answers. That is what
//! makes a document that arrives in the post safe to open: its `AutoOpen`
//! is a macro like any other, and no macro runs until somebody says so.
//! Pressing Enable Content is that somebody saying so, and the document's
//! opening macros run then, which is when Word runs them too.
//!
//! # The controls
//!
//! A document's own module may also answer for its content controls:
//! `Document_ContentControlOnEnter` when the caret goes into one and
//! `Document_ContentControlOnExit` when it leaves, which may say `Cancel`
//! and keep the caret where it was. The document is watched for the caret
//! crossing a control's edge only while it has one of those two procedures
//! and its macros may run, because looking is not free and a document
//! with nothing to say need not be looked at.

use std::path::Path;

use wp_vba::value::{Handle, Value};

use super::trust::Allowed;
use super::Editor;

/// The moments a document has a macro for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    /// An existing document was opened.
    Opened,
    /// A new document was made from a template.
    Created,
    /// The document is being closed.
    Closing,
}

impl Moment {
    /// The auto macro's name, and the event procedure's.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Opened => ("AutoOpen", "Document_Open"),
            Self::Created => ("AutoNew", "Document_New"),
            Self::Closing => ("AutoClose", "Document_Close"),
        }
    }
}

/// Where an auto macro is in a project, if it is: `Sub AutoOpen` in any
/// module, or `Main` in a module called `AutoOpen`.
fn auto_macro(project: &wp_vba::Project, auto: &str) -> Option<String> {
    for module in &project.modules {
        if module.kind != wp_vba::Kind::Standard {
            continue;
        }
        let wanted = if module.name.eq_ignore_ascii_case(auto) { "Main" } else { auto };
        if module.procedures().iter().any(|procedure| {
            procedure.name.eq_ignore_ascii_case(wanted)
                && procedure.sort == wp_vba::Sort::Sub
                && !procedure.takes_arguments
        }) {
            return Some(format!("{}.{wanted}", module.name));
        }
    }
    None
}

/// Where an event procedure is: in the document's own module.
fn document_event(project: &wp_vba::Project, event: &str) -> Option<String> {
    project
        .modules
        .iter()
        .filter(|module| module.kind == wp_vba::Kind::Document)
        .find(|module| {
            module.procedures().iter().any(|procedure| procedure.name.eq_ignore_ascii_case(event))
        })
        .map(|module| format!("{}.{event}", module.name))
}

impl Editor {
    /// Raises a moment of the document: its auto macro, then its event
    /// procedure, if it has them and if its macros may run.
    pub(super) fn raise(&mut self, moment: Moment) {
        if matches!(self.macros_allowed(), Allowed::No(_)) {
            return;
        }
        let Some(project) = &self.vba else { return };
        let (auto, event) = moment.names();
        let names: Vec<String> =
            auto_macro(project, auto).into_iter().chain(document_event(project, event)).collect();
        for name in names {
            self.launch(&name, Vec::new(), true);
        }
    }

    /// Raises `AutoNew` and `Document_New` from the template a new document
    /// was just made from, whose macros are the template's and not the
    /// document's.
    ///
    /// Whether they may run is the template's question: the folder it is
    /// in, the signature it carries. The document is new and has neither.
    pub(super) fn raise_from_template(&mut self, template: &[u8], path: &Path) {
        let Ok(document) = wp_docx::Document::open(template) else { return };
        let Some(project) = super::macros::project_of(&document) else { return };
        if matches!(self.allowed_for(Some(path), &document), Allowed::No(_)) {
            return;
        }
        let (auto, event) = Moment::Created.names();
        let names: Vec<String> =
            auto_macro(&project, auto).into_iter().chain(document_event(&project, event)).collect();
        if names.is_empty() {
            return;
        }
        // The template's project stands in for the document's while its
        // macros run against the new document, and is put back after: the
        // macro has its own copy of the project from the moment it starts.
        let held = self.vba.replace(project);
        for name in names {
            self.launch(&name, Vec::new(), true);
        }
        self.vba = held;
    }

    /// Works out whether the document has anything to say about its
    /// content controls, which decides whether the caret is watched.
    pub(super) fn refresh_control_watch(&mut self) {
        self.watching_controls = self.vba.as_ref().is_some_and(|project| {
            project.modules.iter().any(|module| {
                module.kind == wp_vba::Kind::Document
                    && module.source.contains("Document_ContentControlOn")
            })
        });
        self.control_here = None;
    }

    /// After anything that may have moved the caret: whether it has gone
    /// into a content control or out of one, and the document's answer.
    pub(super) fn notice_control_change(&mut self) {
        if !self.watching_controls || self.debugger.is_some() {
            return;
        }
        let caret = self.document.caret();
        if self.last_caret_seen == Some(caret) {
            return;
        }
        self.last_caret_seen = Some(caret);
        if matches!(self.macros_allowed(), Allowed::No(_)) {
            return;
        }
        let controls = self.document.controls();
        let now = controls
            .iter()
            .position(|control| control.start == caret)
            .or_else(|| controls.iter().position(|control| control.covers(caret)));
        let was = self.control_here;
        if now == was {
            return;
        }
        let Some(project) = &self.vba else { return };
        let exit = document_event(project, "Document_ContentControlOnExit");
        let enter = document_event(project, "Document_ContentControlOnEnter");

        if let (Some(was), Some(name)) = (was, exit) {
            let argument = Value::Object(Handle::of("ContentControl", was as u64));
            self.launch(&name, vec![argument, Value::Boolean(false)], true);
            // `Cancel` keeps the caret in the control it was leaving.
            let cancelled = self
                .macro_left()
                .and_then(|left| left.get(1).map(|cancel| cancel.truth().unwrap_or(false)))
                .unwrap_or(false);
            if cancelled {
                if let Some(control) = controls.get(was) {
                    let back = control.end;
                    self.document.set_caret(back);
                    self.last_caret_seen = Some(back);
                    self.needs_redraw = true;
                }
                return;
            }
        }
        self.control_here = now;
        if let (Some(now), Some(name)) = (now, enter) {
            let argument = Value::Object(Handle::of("ContentControl", now as u64));
            self.launch(&name, vec![argument], true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::kinds::Kind;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    use crate::editor::trust::Trusting;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// A document carrying the given project, with one paragraph of text.
    fn carrying(modules: &[(&str, &str)]) -> Document {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("untouched")));
        let mut document = Document::create(&body).expect("a document");
        document.set_kind(Kind::MacroEnabledDocument);
        let bytes = document.save().expect("saving");

        let mut package = wp_opc::Package::open(&bytes).expect("a package");
        package.add_part(
            "word/vbaProject.bin",
            "application/vnd.ms-office.vbaProject",
            wp_vba::example(modules),
        );
        let mut relationships = package.relationships("word/document.xml").expect("relationships");
        relationships.add(
            "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
            "vbaProject.bin",
            wp_opc::TargetMode::Internal,
        );
        package.set_relationships(&relationships).expect("writing them");
        Document::open(&package.save().expect("saving the package")).expect("reopening")
    }

    fn editor() -> Editor {
        let blank = Document::create(&Body::default()).expect("a blank document");
        let mut editor = Editor::new(library(), blank, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    const BOTH: &[(&str, &str)] = &[
        (
            "Module1",
            "Public Sub AutoOpen()\r\n    Selection.TypeText \"auto \"\r\nEnd Sub\r\n\
             Public Sub AutoClose()\r\n    Selection.TypeText \"bye \"\r\nEnd Sub\r\n",
        ),
        (
            "ThisDocument",
            "Private Sub Document_Open()\r\n    Selection.TypeText \"event \"\r\nEnd Sub\r\n\
             Private Sub Document_Close()\r\n    Selection.TypeText \"closed \"\r\nEnd Sub\r\n",
        ),
    ];

    #[test]
    fn nothing_runs_on_opening_until_content_is_enabled_and_then_both_do() {
        let mut editor = editor();
        editor.set_document(carrying(BOTH), None);
        editor.raise(Moment::Opened);
        assert_eq!(editor.document.plain_text().trim(), "untouched", "it ran unasked");

        // Enable Content is the moment the opening macros run: the auto
        // macro, then the event procedure.
        editor.enable_macros_here();
        assert_eq!(editor.document.plain_text().trim(), "auto event untouched");
    }

    #[test]
    fn a_trusted_document_runs_its_opening_macros_when_it_is_opened() {
        let mut editor = editor();
        editor.settings.macro_trust = Some(Trusting::Everything.name().to_owned());
        editor.set_document(carrying(BOTH), None);
        editor.raise(Moment::Opened);
        assert_eq!(editor.document.plain_text().trim(), "auto event untouched");
    }

    #[test]
    fn closing_runs_the_closing_macros_before_the_document_goes() {
        let mut editor = editor();
        editor.settings.macro_trust = Some(Trusting::Everything.name().to_owned());
        editor.set_document(carrying(BOTH), None);
        editor.raise(Moment::Closing);
        assert_eq!(editor.document.plain_text().trim(), "bye closed untouched");
    }

    #[test]
    fn an_auto_macro_may_be_main_in_a_module_of_its_own_name() {
        let mut editor = editor();
        editor.settings.macro_trust = Some(Trusting::Everything.name().to_owned());
        editor.set_document(
            carrying(&[(
                "AutoOpen",
                "Sub Main()\r\n    Selection.TypeText \"main \"\r\nEnd Sub\r\n",
            )]),
            None,
        );
        editor.raise(Moment::Opened);
        assert_eq!(editor.document.plain_text().trim(), "main untouched");
    }

    #[test]
    fn a_macro_that_stops_says_so_on_the_status_line_and_the_rest_still_runs() {
        let mut editor = editor();
        editor.settings.macro_trust = Some(Trusting::Everything.name().to_owned());
        editor.set_document(
            carrying(&[
                ("Module1", "Sub AutoOpen()\r\n    Err.Raise 7\r\nEnd Sub\r\n"),
                (
                    "ThisDocument",
                    "Private Sub Document_Open()\r\n    Selection.TypeText \"still \"\r\nEnd Sub\r\n",
                ),
            ]),
            None,
        );
        editor.raise(Moment::Opened);
        assert!(editor.status.contains("AutoOpen"), "{}", editor.status);
        assert!(editor.status.contains("stopped"), "{}", editor.status);
        assert_eq!(editor.document.plain_text().trim(), "still untouched");
    }

    /// A document with a content control, and a module that answers for it.
    fn with_control(code: &str) -> Document {
        let mut document = carrying(&[("ThisDocument", code)]);
        document.set_caret(TextPosition::new(0, 0));
        assert!(document.insert_control(wp_docx::controls::ControlKind::PlainText, "Name", &[]));
        let control = document.control_at(TextPosition::new(0, 0)).expect("the control");
        assert!(document.set_control_properties(control.start, "Name", "who", false, false));
        document
    }

    const CONTROL_EVENTS: &str = "Private Sub Document_ContentControlOnEnter(ByVal cc As ContentControl)\r\n\
         \x20   ActiveDocument.Paragraphs.Add\r\n\
         \x20   ActiveDocument.Paragraphs(ActiveDocument.Paragraphs.Count).Range.Text = \"in \" & cc.Title\r\n\
         End Sub\r\n\
         Private Sub Document_ContentControlOnExit(ByVal cc As ContentControl, Cancel As Boolean)\r\n\
         \x20   ActiveDocument.Paragraphs.Add\r\n\
         \x20   ActiveDocument.Paragraphs(ActiveDocument.Paragraphs.Count).Range.Text = \"out \" & cc.Tag\r\n\
         \x20   If cc.Range.Text = \"stay\" Then Cancel = True\r\n\
         End Sub\r\n";

    #[test]
    fn the_caret_going_into_a_control_and_out_again_is_told_to_the_document() {
        let mut editor = editor();
        editor.settings.macro_trust = Some(Trusting::Everything.name().to_owned());
        editor.set_document(with_control(CONTROL_EVENTS), None);
        assert!(editor.watching_controls, "the document is not being watched");

        // The caret starts outside; Home takes it to the control.
        let last = editor.document.paragraph_count() - 1;
        editor.document.set_caret(TextPosition::new(last, 0));
        editor.handle(Event::KeyDown { key: Key::Home, modifiers: Modifiers::default() });
        editor.document.set_caret(TextPosition::new(0, 0));
        editor.handle(Event::Tick);
        let text = editor.document.plain_text();
        assert!(text.contains("in Name"), "{text}");

        // And out again, past the end of the paragraph the control is in.
        let end = editor.document.paragraph_text(0).unwrap_or_default().chars().count();
        editor.document.set_caret(TextPosition::new(0, end));
        editor.handle(Event::Tick);
        let text = editor.document.plain_text();
        assert!(text.contains("out who"), "{text}");
    }

    #[test]
    fn cancelling_the_exit_keeps_the_caret_in_the_control() {
        let mut editor = editor();
        editor.settings.macro_trust = Some(Trusting::Everything.name().to_owned());
        let mut document = with_control(CONTROL_EVENTS);
        let control = document.control_at(TextPosition::new(0, 0)).expect("the control");
        document.set_control_text(control.start, "stay");
        editor.set_document(document, None);

        editor.document.set_caret(TextPosition::new(0, 0));
        editor.handle(Event::Tick);
        let control = editor.document.control_at(TextPosition::new(0, 0)).expect("the control");
        let end = editor.document.paragraph_text(0).unwrap_or_default().chars().count();
        editor.document.set_caret(TextPosition::new(0, end));
        editor.handle(Event::Tick);
        assert!(
            control.covers(editor.document.caret()),
            "the caret left: {:?}",
            editor.document.caret()
        );
    }

    #[test]
    fn a_document_with_nothing_to_say_is_not_watched() {
        let mut editor = editor();
        editor.set_document(with_control("Private Sub Document_Open()\r\nEnd Sub\r\n"), None);
        assert!(!editor.watching_controls);
    }
}

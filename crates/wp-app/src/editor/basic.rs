//! What the Visual Basic editor does.
//!
//! The page itself is [`crate::chrome::basicpane`]; this is what typing in it
//! changes, what F5 and F8 do, and how what was typed gets back into the
//! document.
//!
//! # Writing it back
//!
//! A project is a compound file inside the document: a `dir` stream naming
//! the modules and one stream per module, each holding a cache Word keeps for
//! itself and then the text, compressed. Writing an edited module back means
//! keeping everything else exactly as it was — the cache, the other streams,
//! the project's own settings — and putting new compressed text after the
//! cache. Nothing else is touched, because everything else belongs to a
//! program that is not this one.
//!
//! # Running from here
//!
//! F5 runs the procedure the caret is in; F8 runs it a statement at a time;
//! F9 puts a breakpoint on the line the caret is on. All three go through
//! [`super::debugger`], which is where stopping in the middle happens. The
//! Immediate window does not: a line typed there is run to the end straight
//! away, because there is nothing to step through in one line.

use std::collections::BTreeSet;

use wp_shell::{Key, Response};

use crate::chrome::basicpane::{BasicPane, Pressed};
use crate::messages::{t, with};

use super::debugger::Debugger;
use super::objects::Model;
use super::Editor;

impl Editor {
    /// Whether the window is showing the Visual Basic editor.
    #[must_use]
    pub(super) fn editing_basic(&self) -> bool {
        self.basic.is_some()
    }

    /// Opens it, on the document's own project.
    pub(super) fn open_basic(&mut self) -> Response {
        self.popup = None;
        let project = self.vba.as_ref();
        let name = project.map_or_else(|| "VBAProject".to_owned(), |vba| vba.name.clone());
        let modules: Vec<String> = project
            .map(|vba| vba.modules.iter().map(|module| module.name.clone()).collect())
            .unwrap_or_default();

        let mut pane = BasicPane::new(&name, modules);
        // A document with no project at all is a new project: one module,
        // empty, which is what Word gives somebody who opens the editor on a
        // document that has never had a macro.
        let source = self
            .vba
            .as_ref()
            .and_then(|vba| vba.modules.first())
            .map(|module| module.source.clone());
        match source {
            Some(source) => pane.show(&source),
            None => {
                pane.modules = vec!["Module1".to_owned()];
                pane.show("Attribute VB_Name = \"Module1\"\r\nOption Explicit\r\n\r\n");
            }
        }
        self.basic = Some(pane);
        self.needs_redraw = true;
        self.report("Visual Basic Editor — F5 runs, F8 steps, F9 marks a line, Esc closes")
    }

    /// Shuts it, keeping whatever was typed.
    pub(super) fn close_basic(&mut self) -> Response {
        self.keep_basic();
        self.basic = None;
        self.debugger = None;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Draws the page.
    pub(super) fn draw_basic_page(&mut self) {
        let (width, height) = (self.view_width, self.view_height);
        let Some(mut pane) = self.basic.take() else { return };
        let theme = self.theme;
        let top = crate::chrome::TITLE_HEIGHT;
        pane.draw(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            &theme,
            top,
            width,
            height,
        );
        self.basic = Some(pane);
    }

    /// A key while it is up.
    #[allow(clippy::too_many_lines)]
    pub(super) fn basic_key(&mut self, key: Key, shift: bool, control: bool) -> Response {
        let Some(pane) = &mut self.basic else { return Response::Ignored };
        match key {
            Key::Escape => return self.close_basic(),
            Key::Function(5) => {
                if shift {
                    self.stop_macro();
                    return Response::Redraw;
                }
                return self.start_macro(false);
            }
            Key::Function(8) => return self.start_macro(true),
            Key::Function(9) => {
                let wanted = pane.caret.0 + 1;
                if !pane.breakpoints.remove(&wanted) {
                    // On the line the caret is on, or the next one with a
                    // statement on it: a breakpoint on a blank line or on a
                    // comment is one the macro would never reach, and Word
                    // moves it down for the same reason.
                    let text = pane.text();
                    let line =
                        breakable(&text).into_iter().find(|line| *line >= wanted).unwrap_or(wanted);
                    pane.breakpoints.insert(line);
                }
                if let Some(debugger) = &mut self.debugger {
                    debugger.breakpoints = self
                        .basic
                        .as_ref()
                        .map(|pane| pane.breakpoints.clone())
                        .unwrap_or_default();
                }
                self.needs_redraw = true;
                return Response::Redraw;
            }
            Key::Tab => {
                let typing = !pane.in_immediate;
                pane.in_immediate = false;
                if typing {
                    // Four spaces, which is what the editor puts in.
                    for _ in 0..4 {
                        self.basic_character(' ');
                    }
                }
                self.needs_redraw = true;
                return Response::Redraw;
            }
            _ => {}
        }

        if pane.in_immediate {
            match key {
                Key::Enter => return self.run_immediate(),
                Key::Backspace => {
                    pane.immediate.pop();
                }
                _ => return Response::Ignored,
            }
            self.needs_redraw = true;
            return Response::Redraw;
        }

        let line = pane.caret.0;
        let column = pane.caret.1;
        match key {
            Key::Left => {
                if column > 0 {
                    pane.caret.1 -= 1;
                } else if line > 0 {
                    pane.caret = (line - 1, pane.lines[line - 1].chars().count());
                }
            }
            Key::Right => {
                if column < pane.lines[line].chars().count() {
                    pane.caret.1 += 1;
                } else if line + 1 < pane.lines.len() {
                    pane.caret = (line + 1, 0);
                }
            }
            Key::Up => pane.caret.0 = line.saturating_sub(1),
            Key::Down => pane.caret.0 = (line + 1).min(pane.lines.len() - 1),
            Key::Home => pane.caret.1 = 0,
            Key::End => pane.caret.1 = pane.lines[line].chars().count(),
            Key::PageUp => pane.caret.0 = line.saturating_sub(pane.room),
            Key::PageDown => pane.caret.0 = (line + pane.room).min(pane.lines.len() - 1),
            Key::Enter => {
                let letters: Vec<char> = pane.lines[line].chars().collect();
                let rest: String = letters[column.min(letters.len())..].iter().collect();
                pane.lines[line] = letters[..column.min(letters.len())].iter().collect();
                // The new line starts where the last one did, which is what
                // every code editor does and what nobody thanks it for.
                let indent: String = pane.lines[line]
                    .chars()
                    .take_while(|letter| *letter == ' ' || *letter == '\t')
                    .collect();
                pane.lines.insert(line + 1, format!("{indent}{rest}"));
                pane.caret = (line + 1, indent.chars().count());
            }
            Key::Backspace => {
                if column > 0 {
                    let mut letters: Vec<char> = pane.lines[line].chars().collect();
                    letters.remove(column - 1);
                    pane.lines[line] = letters.into_iter().collect();
                    pane.caret.1 -= 1;
                } else if line > 0 {
                    let joined = pane.lines.remove(line);
                    let length = pane.lines[line - 1].chars().count();
                    pane.lines[line - 1].push_str(&joined);
                    pane.caret = (line - 1, length);
                }
            }
            Key::Delete => {
                let length = pane.lines[line].chars().count();
                if column < length {
                    let mut letters: Vec<char> = pane.lines[line].chars().collect();
                    letters.remove(column);
                    pane.lines[line] = letters.into_iter().collect();
                } else if line + 1 < pane.lines.len() {
                    let joined = pane.lines.remove(line + 1);
                    pane.lines[line].push_str(&joined);
                }
            }
            Key::Letter('s') if control => {
                self.keep_basic();
                return self.report("The project was written back into the document");
            }
            _ => return Response::Ignored,
        }
        if let Some(pane) = &mut self.basic {
            pane.settle();
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// A letter typed into it.
    pub(super) fn basic_character(&mut self, letter: char) -> Response {
        if letter.is_control() {
            return Response::Ignored;
        }
        let Some(pane) = &mut self.basic else { return Response::Ignored };
        if pane.in_immediate {
            pane.immediate.push(letter);
        } else {
            let (line, column) = pane.caret;
            let mut letters: Vec<char> = pane.lines[line].chars().collect();
            letters.insert(column.min(letters.len()), letter);
            pane.lines[line] = letters.into_iter().collect();
            pane.caret.1 = column + 1;
        }
        pane.settle();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// A press on it.
    pub(super) fn basic_press(&mut self, x: i32, y: i32, width: usize, height: usize) -> Response {
        let Some(pane) = &mut self.basic else { return Response::Ignored };
        #[allow(clippy::cast_precision_loss)]
        let (fx, fy) = (x as f32, y as f32);
        #[allow(clippy::cast_precision_loss)]
        let what = pane.at(fx, fy, width as f32, height as f32);
        match what {
            Pressed::Close => return self.close_basic(),
            Pressed::Module(index) => {
                self.keep_basic();
                self.show_module(index);
            }
            Pressed::Margin(line) => {
                if !pane.breakpoints.remove(&line) {
                    pane.breakpoints.insert(line);
                }
                if let Some(debugger) = &mut self.debugger {
                    debugger.breakpoints = pane.breakpoints.clone();
                }
            }
            Pressed::Code(line, _) => {
                pane.in_immediate = false;
                pane.caret.0 = line;
                let mut pane = self.basic.take().expect("the pane");
                pane.caret.1 = pane.column_at(&mut self.chrome_engine, line, fx);
                pane.settle();
                self.basic = Some(pane);
            }
            Pressed::Immediate => {
                pane.in_immediate = true;
            }
            Pressed::Nothing => {}
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// The wheel over it.
    pub(super) fn basic_scroll(&mut self, lines: f32) -> Response {
        let Some(pane) = &mut self.basic else { return Response::Ignored };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let step = lines.abs().max(1.0) as usize;
        if lines > 0.0 {
            pane.scroll = pane.scroll.saturating_sub(step);
        } else {
            pane.scroll = (pane.scroll + step).min(pane.lines.len().saturating_sub(1));
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Shows another module, keeping what was typed into this one.
    fn show_module(&mut self, index: usize) {
        let source = self
            .vba
            .as_ref()
            .and_then(|vba| vba.modules.get(index))
            .map(|module| module.source.clone());
        let Some(pane) = &mut self.basic else { return };
        pane.showing = index;
        match source {
            Some(source) => pane.show(&source),
            None => pane.show(""),
        }
    }

    /// Puts what was typed back into the project, and the project back into
    /// the document.
    fn keep_basic(&mut self) {
        let Some(pane) = &self.basic else { return };
        let showing = pane.showing;
        let text = pane.text();
        let Some(vba) = &mut self.vba else {
            // A project this program never read is one it must not write:
            // the document keeps whatever it had.
            return;
        };
        let Some(module) = vba.modules.get_mut(showing) else { return };
        if module.source == text {
            return;
        }
        module.source = text;

        let stream = module.stream.clone();
        let source = module.source.clone();
        let code_page = vba.code_page;
        if let Some(bytes) = self.rebuilt_project(&stream, &source, code_page) {
            self.document.set_macro_project(bytes);
        }
    }

    /// The project's bytes with one module's text changed.
    ///
    /// Everything else is carried through exactly: the cache in front of the
    /// text, the `dir` stream that says where it begins, the other modules,
    /// and the streams beside the storage that hold the project's settings.
    fn rebuilt_project(&self, stream: &str, source: &str, code_page: u16) -> Option<Vec<u8>> {
        let bytes = self.document.package().part("word/vbaProject.bin")?;
        let file = wp_ole::CompoundFile::open(bytes.to_vec()).ok()?;

        let encoding = wp_text::Encoding::code_page(u32::from(code_page))
            .unwrap_or(wp_text::Encoding::CodePage(1252));
        let (written, _) = encoding.encode(source, true);

        let mut inside: Vec<wp_ole::Item> = Vec::new();
        let mut outside: Vec<wp_ole::Item> = Vec::new();
        for entry in file.entries() {
            if entry.kind != wp_ole::EntryKind::Stream {
                continue;
            }
            let held = file.walk(&["VBA", &entry.name]);
            let in_vba = held.is_some();
            let Some(mut contents) = held.or_else(|| file.walk(&[&entry.name])) else { continue };

            if in_vba && entry.name.eq_ignore_ascii_case(stream) {
                // The `dir` stream says where the text starts, and what is in
                // front of it is Word's own cache.
                let offset = self
                    .vba
                    .as_ref()?
                    .modules
                    .iter()
                    .find(|module| module.stream.eq_ignore_ascii_case(stream))?
                    .offset;
                if offset > contents.len() {
                    return None;
                }
                let mut made = contents[..offset].to_vec();
                made.extend_from_slice(&wp_vba::compress::compress(&written));
                contents = made;
            }
            let item = wp_ole::Item::stream(&entry.name, contents);
            if in_vba {
                inside.push(item);
            } else {
                outside.push(item);
            }
        }

        let mut builder = wp_ole::Builder::new();
        builder.item(wp_ole::Item::storage("VBA", inside));
        for item in outside {
            builder.item(item);
        }
        Some(builder.build())
    }

    // --- Running --------------------------------------------------------

    /// Runs the procedure the caret is in, or carries on the one that is
    /// stopped.
    fn start_macro(&mut self, stepping: bool) -> Response {
        // While one is stopped, F5 is "carry on" and F8 is "one more
        // statement". Starting the macro again instead would do everything it
        // had already done a second time.
        if let Some(debugger) = &mut self.debugger {
            if debugger.stopped.is_some() {
                debugger.go(stepping);
                self.pump_macro();
                return Response::Redraw;
            }
            if debugger.running() {
                return self.report("That macro is still running");
            }
        }
        // The gate every way of running a macro goes through: see
        // [`super::trust`].
        if let super::trust::Allowed::No(why) = self.macros_allowed() {
            return self.report(&why);
        }
        self.keep_basic();
        let Some(pane) = &self.basic else { return Response::Ignored };
        let text = pane.text();
        let breakpoints = pane.breakpoints.clone();
        let line = pane.caret.0 + 1;

        let Some(name) = procedure_at(&text, line) else {
            return self.report("Put the caret in a Sub or a Function, and F5 runs that one");
        };
        self.debugger = Some(Debugger::start(&text, &name, breakpoints, stepping));
        self.pump_macro();
        Response::Redraw
    }

    /// Stops one that is running, which is Word's Reset.
    fn stop_macro(&mut self) {
        if let Some(debugger) = &mut self.debugger {
            debugger.reset();
        }
        if let Some(pane) = &mut self.basic {
            pane.stopped = None;
        }
    }

    /// Answers whatever the running macro has asked, and says whether
    /// anything changed.
    pub(super) fn pump_macro(&mut self) -> bool {
        let Some(mut debugger) = self.debugger.take() else { return false };
        debugger.pump(self);

        if let Some(pane) = &mut self.basic {
            pane.stopped = debugger.stopped;
            pane.watched =
                debugger.watched.iter().map(|(name, value)| (name.clone(), shown(value))).collect();
        }

        let ended = debugger.finished();
        let said = debugger.said.clone();
        let name = debugger.name.clone();
        match ended {
            Some(answer) => {
                if let Some(pane) = &mut self.basic {
                    pane.stopped = None;
                    pane.answers.extend(said);
                    match &answer {
                        Ok(_) => pane.answers.push(with("{0} ran", &[&name])),
                        Err(fault) => {
                            pane.answers
                                .push(with("{0} stopped: {1}", &[&name, &fault.to_string()]));
                        }
                    }
                }
                self.relayout();
                self.reveal_caret();
                self.needs_redraw = true;
                true
            }
            None => {
                self.debugger = Some(debugger);
                self.needs_redraw = true;
                true
            }
        }
    }

    /// Runs the line typed into the Immediate window.
    fn run_immediate(&mut self) -> Response {
        let Some(pane) = &mut self.basic else { return Response::Ignored };
        let typed = pane.immediate.trim().to_owned();
        pane.immediate.clear();
        if typed.is_empty() {
            return Response::Redraw;
        }
        pane.answers.push(format!("> {typed}"));
        // A line typed here is Visual Basic running against the document, so
        // it is the same question as F5 and goes through the same gate.
        if let super::trust::Allowed::No(why) = self.macros_allowed() {
            if let Some(pane) = &mut self.basic {
                pane.answers.push(why);
            }
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // A line beginning with `?` asks for a value, as it does in Word's
        // own Immediate window; anything else is a statement to run.
        let asking = typed.starts_with('?');
        let line = typed.trim_start_matches('?').trim().to_owned();
        let source = if asking {
            format!("Function __Immediate()\r\n__Immediate = {line}\r\nEnd Function\r\n")
        } else {
            format!("Sub __Immediate()\r\n{line}\r\nEnd Sub\r\n")
        };

        let (program, complaints) = wp_vba::run::Program::read(&source);
        if let Some(complaint) = complaints.first() {
            let said = complaint.said.clone();
            if let Some(pane) = &mut self.basic {
                pane.answers.push(said);
            }
            self.needs_redraw = true;
            return Response::Redraw;
        }

        let mut model = Model::default();
        let answer = {
            let mut bound = model.on(self);
            program.run("__Immediate", Vec::new(), &mut bound)
        };
        let mut said = model.said();
        said.push(match &answer {
            Ok(value) if asking => shown(value),
            Ok(_) => t("Done").to_owned(),
            Err(fault) => fault.to_string(),
        });
        if let Some(pane) = &mut self.basic {
            pane.answers.extend(said);
        }
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }
}

/// A value as the Immediate window and the Watch show one.
fn shown(value: &wp_vba::value::Value) -> String {
    match value {
        wp_vba::value::Value::Text(text) => format!("\"{text}\""),
        wp_vba::value::Value::Object(handle) => format!("[{}]", handle.kind),
        other => other.text().unwrap_or_else(|_| other.type_name().to_owned()),
    }
}

/// Which procedure a line of a module is inside.
#[must_use]
pub fn procedure_at(source: &str, line: usize) -> Option<String> {
    let (tree, _) = wp_vba::parse::parse(source);
    for node in tree.every(wp_vba::tree::Part::Procedure) {
        let start = node.line();
        let length = node.written().matches('\n').count();
        if line >= start && line <= start + length {
            return node
                .children()
                .iter()
                .filter_map(|child| child.token())
                .find(|token| {
                    token.kind == wp_vba::lex::Kind::Word && !wp_vba::tree::is_keyword(&token.text)
                })
                .map(|token| token.text.clone());
        }
    }
    None
}

/// Which lines of a module may have a breakpoint on them: the ones with a
/// statement, which is what the debugger can stop before.
#[must_use]
pub fn breakable(source: &str) -> BTreeSet<usize> {
    let (tree, _) = wp_vba::parse::parse(source);
    let mut out = BTreeSet::new();
    for node in tree.every(wp_vba::tree::Part::Body) {
        for statement in node.children() {
            if statement.part().is_some() && statement.line() > 0 {
                out.insert(statement.line());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use wp_docx::kinds::Kind;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// A document of three paragraphs carrying one module.
    fn document_with(source: &str) -> Document {
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
            wp_vba::example(&[("Module1", source), ("Module2", "' the other one\r\n")]),
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

    /// An editor with that document, its Visual Basic editor open.
    fn editing(source: &str) -> Editor {
        let mut editor = Editor::new(library(), document_with(source), None);
        editor.handle(Event::Resized { width: 1200, height: 800 });
        editor.draw(1200, 800);
        // Enabled first, as a person enables them: nothing here runs a macro
        // until somebody has said so. See [`super::trust`].
        editor.enable_macros_here();
        editor.open_basic();
        editor
    }

    fn keys(editor: &mut Editor, key: Key) {
        editor.handle(Event::KeyDown { key, modifiers: Modifiers::default() });
    }

    fn types(editor: &mut Editor, text: &str) {
        for letter in text.chars() {
            editor.handle(Event::Char(letter));
        }
    }

    /// Ticks until the macro stops, finishes, or too long has gone by.
    fn settle(editor: &mut Editor) {
        for _ in 0..200 {
            editor.handle(Event::Tick);
            let stopped = editor.basic.as_ref().and_then(|pane| pane.stopped).is_some();
            if stopped || editor.debugger.is_none() {
                return;
            }
        }
    }

    fn pane(editor: &Editor) -> &BasicPane {
        editor.basic.as_ref().expect("the editor is open")
    }

    const HELLO: &str = "Attribute VB_Name = \"Module1\"\r\n\
         Option Explicit\r\n\
         \r\n\
         Public Sub Hello()\r\n\
         \x20   Dim total As Long\r\n\
         \x20   total = 6 * 7\r\n\
         \x20   MsgBox total\r\n\
         End Sub\r\n";

    #[test]
    fn it_opens_on_the_project_the_document_carries() {
        let editor = editing(HELLO);
        assert_eq!(pane(&editor).modules, vec!["Module1".to_owned(), "Module2".to_owned()]);
        assert!(pane(&editor).lines[0].contains("VB_Name"), "{:?}", pane(&editor).lines[0]);
        assert!(pane(&editor).lines.iter().any(|line| line.contains("MsgBox total")));
    }

    #[test]
    fn what_is_typed_goes_in_where_the_caret_is() {
        let mut editor = editing(HELLO);
        if let Some(pane) = &mut editor.basic {
            pane.caret = (2, 0);
        }
        types(&mut editor, "' a new line");
        keys(&mut editor, Key::Enter);
        types(&mut editor, "' and another");

        let lines = &pane(&editor).lines;
        assert_eq!(lines[2], "' a new line");
        assert_eq!(lines[3], "' and another");

        // And Backspace takes it back, joining two lines where it has to.
        keys(&mut editor, Key::Home);
        keys(&mut editor, Key::Backspace);
        assert_eq!(pane(&editor).lines[2], "' a new line' and another");
    }

    #[test]
    fn what_was_typed_is_written_back_into_the_documents_own_project() {
        // The half of this item that outlives the window: a macro edited here
        // has to be there when the document is opened again, and everything
        // else in the project has to be exactly as it was.
        let mut editor = editing(HELLO);
        if let Some(pane) = &mut editor.basic {
            pane.caret = (2, 0);
        }
        types(&mut editor, "' written back");
        editor.close_basic();

        let bytes =
            editor.document.package().part("word/vbaProject.bin").expect("the project").to_vec();
        let project = wp_vba::Project::open(&bytes).expect("a project again");
        let module = project.module("Module1").expect("the module");
        assert!(module.source.contains("' written back"), "{}", module.source);
        assert!(module.source.contains("MsgBox total"), "the rest of it went missing");
        assert_eq!(
            project.module("Module2").expect("the other").source.trim_end(),
            "' the other one",
            "the module nobody touched was changed"
        );
        // And the document knows it has changed, so that it is offered to be
        // saved.
        assert!(editor.document.is_modified());
    }

    #[test]
    fn f5_runs_the_procedure_the_caret_is_in() {
        let mut editor = editing(HELLO);
        if let Some(pane) = &mut editor.basic {
            pane.caret = (5, 0);
        }
        keys(&mut editor, Key::Function(5));
        settle(&mut editor);

        let answers = &pane(&editor).answers;
        assert!(answers.iter().any(|line| line == "42"), "{answers:?}");
        assert!(answers.iter().any(|line| line.contains("ran")), "{answers:?}");
    }

    #[test]
    fn a_breakpoint_stops_it_and_f5_carries_on() {
        // The heart of a debugger: stopped in the middle, the line said, what
        // the variables hold said, and then on to the end.
        let mut editor = editing(HELLO);
        if let Some(pane) = &mut editor.basic {
            // The line that adds up, which is the sixth.
            pane.caret = (5, 0);
        }
        keys(&mut editor, Key::Function(9));
        assert!(pane(&editor).breakpoints.contains(&6), "{:?}", pane(&editor).breakpoints);

        keys(&mut editor, Key::Function(5));
        settle(&mut editor);
        assert_eq!(pane(&editor).stopped, Some(6), "it did not stop on the line");
        assert!(
            pane(&editor).watched.iter().any(|(name, _)| name == "total"),
            "{:?}",
            pane(&editor).watched
        );

        keys(&mut editor, Key::Function(5));
        settle(&mut editor);
        assert_eq!(pane(&editor).stopped, None, "it did not carry on");
        assert!(
            pane(&editor).answers.iter().any(|line| line == "42"),
            "{:?}",
            pane(&editor).answers
        );
    }

    #[test]
    fn f8_goes_one_statement_at_a_time() {
        let mut editor = editing(HELLO);
        if let Some(pane) = &mut editor.basic {
            pane.caret = (3, 0);
        }
        keys(&mut editor, Key::Function(8));
        settle(&mut editor);
        let first = pane(&editor).stopped.expect("it stopped somewhere");

        keys(&mut editor, Key::Function(8));
        settle(&mut editor);
        let second = pane(&editor).stopped.expect("it stopped again");
        assert!(second > first, "it did not move on: {first} then {second}");
    }

    #[test]
    fn a_breakpoint_goes_on_a_line_the_macro_can_reach() {
        // A blank line is one the macro never runs, so Word moves the
        // breakpoint down to the next statement and so does this.
        let mut editor = editing(HELLO);
        if let Some(pane) = &mut editor.basic {
            pane.caret = (2, 0);
        }
        keys(&mut editor, Key::Function(9));
        assert!(
            pane(&editor).breakpoints.iter().all(|line| *line > 3),
            "{:?}",
            pane(&editor).breakpoints
        );
    }

    #[test]
    fn the_immediate_window_answers_a_question_and_runs_a_line() {
        let mut editor = editing(HELLO);
        if let Some(pane) = &mut editor.basic {
            pane.in_immediate = true;
        }
        types(&mut editor, "?1 + 1");
        keys(&mut editor, Key::Enter);
        assert!(
            pane(&editor).answers.iter().any(|line| line == "2"),
            "{:?}",
            pane(&editor).answers
        );

        types(&mut editor, "Selection.TypeText \"typed\"");
        keys(&mut editor, Key::Enter);
        assert!(
            editor.document.plain_text().starts_with("typed"),
            "{}",
            editor.document.plain_text()
        );
    }

    #[test]
    fn a_line_that_will_not_read_says_so_rather_than_running_something_else() {
        let mut editor = editing(HELLO);
        if let Some(pane) = &mut editor.basic {
            pane.in_immediate = true;
        }
        types(&mut editor, "?]]");
        keys(&mut editor, Key::Enter);
        assert!(
            pane(&editor).answers.iter().any(|line| line.contains("Expected")),
            "{:?}",
            pane(&editor).answers
        );
    }

    #[test]
    fn the_editor_is_left_by_the_cross_as_well_as_by_the_key() {
        let mut editor = editing(HELLO);
        let (left, top, _) = pane(&editor).cross(1200.0);
        editor.handle(Event::MouseDown {
            x: left as i32 + 4,
            y: top as i32 + 4,
            modifiers: Modifiers::default(),
        });
        assert!(editor.basic.is_none(), "the cross did not shut it");

        editor.open_basic();
        keys(&mut editor, Key::Escape);
        assert!(editor.basic.is_none(), "Escape did not shut it");
    }

    #[test]
    fn which_procedure_a_line_is_in() {
        assert_eq!(procedure_at(HELLO, 6).as_deref(), Some("Hello"));
        assert_eq!(procedure_at(HELLO, 2), None, "a line above them all is in none");
    }
}

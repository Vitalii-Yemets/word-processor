//! What a macro sees when it looks at the document.
//!
//! # The part a macro is actually written against
//!
//! Nobody writes `Left(s, 5)` in a Word macro for its own sake; they write
//! `Selection.TypeText`, `ActiveDocument.Paragraphs(1).Range.Text`,
//! `.Font.Bold = True`. The language is the floor and this is the building.
//!
//! # A property this program cannot answer says so and stops
//!
//! That is the rule this whole thing is written under, and it is the
//! difference between a program a macro can be trusted with and one it
//! cannot. Word has some thousands of properties; this has what is below.
//! Everything else is refused by name — "Range.Shading is not something this
//! program does" — because a macro that reads a property and is quietly told
//! `Empty` will carry on and write the wrong thing into the document, and
//! nobody will know which property it was.
//!
//! # Where a range is
//!
//! Word counts characters from the start of the document, with each
//! paragraph mark counting as one; this program counts paragraphs and offsets
//! inside them. The two are translated at the edge — see [`Model::offset_of`]
//! and [`Model::position_at`] — so that a macro doing arithmetic on `.Start`
//! and `.End` gets the numbers it expects.
//!
//! # Nothing here is translated
//!
//! The names are the language's: a macro says `Selection` in every country,
//! and what this says when it refuses something is said to whoever wrote the
//! macro, in the English the rest of the language's errors are written in.
//! The message catalogue leaves this module alone for that reason — see
//! [`crate::messagelist`].
//!
//! # What editing goes through
//!
//! The document's own editing, with the selection moved to where the macro is
//! working and put back afterwards. That is not an implementation detail
//! worth hiding: it means a macro's changes are one undo step like anybody
//! else's, and that everything a macro does is something a person could have
//! done by hand.

use wp_docx::model::Alignment;
use wp_docx::CharacterFormat;
use wp_docx::TextPosition;
use wp_vba::library::Host;
use wp_vba::value::{Fault, Given, Handle, Value};

use super::Editor;

/// How many characters come before a byte offset of a paragraph's text.
fn chars_before(text: &str, byte: usize) -> usize {
    text.char_indices().take_while(|(at, _)| *at < byte).count()
}

/// The byte offset of the character this many characters in.
fn byte_of(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(at, _)| at)
}

/// The text between two byte offsets, whole characters only.
fn between(text: &str, from: usize, to: usize) -> &str {
    let from = text.char_indices().map(|(at, _)| at).find(|at| *at >= from).unwrap_or(text.len());
    let to = text.char_indices().map(|(at, _)| at).find(|at| *at >= to).unwrap_or(text.len());
    text.get(from..to.max(from)).unwrap_or_default()
}

/// The kinds of thing a macro can hold on to.
mod kind {
    pub const APPLICATION: &str = "Application";
    pub const DOCUMENTS: &str = "Documents";
    pub const DOCUMENT: &str = "Document";
    pub const SELECTION: &str = "Selection";
    pub const RANGE: &str = "Range";
    pub const PARAGRAPHS: &str = "Paragraphs";
    pub const PARAGRAPH: &str = "Paragraph";
    pub const FONT: &str = "Font";
    pub const STYLES: &str = "Styles";
    pub const STYLE: &str = "Style";
    pub const BOOKMARKS: &str = "Bookmarks";
    pub const BOOKMARK: &str = "Bookmark";
    pub const TABLES: &str = "Tables";
    pub const TABLE: &str = "Table";
    pub const COMMENTS: &str = "Comments";
    pub const COMMENT: &str = "Comment";
    pub const FIND: &str = "Find";
    pub const CONTENT_CONTROLS: &str = "ContentControls";
    pub const CONTENT_CONTROL: &str = "ContentControl";
}

/// Word's `WdContentControlType`, for what `.Type` answers.
fn control_type(kind: wp_docx::controls::ControlKind) -> i64 {
    use wp_docx::controls::ControlKind;
    match kind {
        ControlKind::RichText => 0,
        ControlKind::PlainText => 1,
        ControlKind::ComboBox => 3,
        ControlKind::DropDown => 4,
        ControlKind::Date => 6,
        ControlKind::CheckBox => 8,
        ControlKind::Picture => 2,
    }
}

/// What a macro is holding on to, and what it has said.
///
/// Kept apart from the document itself so that it outlives one question: a
/// macro being stepped through asks one thing, waits, and asks another, and
/// the ranges it took hold of have to still be there when it comes back.
#[derive(Clone, Debug, Default)]
pub struct Model {
    /// The stretches the macro is holding, by the number in their handle.
    ranges: Vec<(TextPosition, TextPosition)>,
    /// What the macro asked to show, and what it printed.
    pub shown: Vec<String>,
    pub printed: Vec<String>,
    /// What a `Find` has been told to look for, by the range it belongs to.
    finding: Vec<(u64, String, String)>,
    /// The ranges that are a content control's inside, by range and by
    /// where the control starts: writing into one goes through the control
    /// rather than round it.
    inside_controls: Vec<(u64, TextPosition)>,
}

impl Model {
    /// Everything the macro said while it ran.
    #[must_use]
    pub fn said(&self) -> Vec<String> {
        let mut out = self.shown.clone();
        out.extend(self.printed.clone());
        out
    }

    /// The model against a document, which is what answers a macro.
    pub fn on<'a>(&'a mut self, editor: &'a mut Editor) -> Bound<'a> {
        Bound { model: self, editor }
    }
}

/// The document as a macro sees it, for as long as one question takes.
pub struct Bound<'a> {
    model: &'a mut Model,
    editor: &'a mut Editor,
}

impl core::fmt::Debug for Bound<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Bound").field("ranges", &self.model.ranges.len()).finish_non_exhaustive()
    }
}

impl Bound<'_> {
    // --- Where things are ------------------------------------------------

    /// How many characters there are before a position.
    ///
    /// Word counts the paragraph mark, so the first character of the second
    /// paragraph is one past the end of the first.
    fn offset_of(&self, position: TextPosition) -> i64 {
        let mut total = 0usize;
        for index in 0..position.paragraph {
            total +=
                self.editor.document.paragraph_text(index).unwrap_or_default().chars().count() + 1;
        }
        // The document's offset is in bytes and Word's is in characters,
        // which are not the same thing past the first accented letter.
        let text = self.editor.document.paragraph_text(position.paragraph).unwrap_or_default();
        #[allow(clippy::cast_possible_wrap)]
        {
            (total + chars_before(&text, position.offset)) as i64
        }
    }

    /// And the other way about, clamped to the document.
    fn position_at(&self, offset: i64) -> TextPosition {
        let mut left = offset.max(0) as usize;
        let count = self.editor.document.paragraph_count();
        for index in 0..count {
            let text = self.editor.document.paragraph_text(index).unwrap_or_default();
            let length = text.chars().count();
            if left <= length {
                return TextPosition::new(index, byte_of(&text, left));
            }
            left -= length + 1;
        }
        let last = count.saturating_sub(1);
        let length = self.editor.document.paragraph_text(last).unwrap_or_default().len();
        TextPosition::new(last, length)
    }

    /// The whole document, as a stretch.
    fn everything(&self) -> (TextPosition, TextPosition) {
        let last = self.editor.document.paragraph_count().saturating_sub(1);
        let length = self.editor.document.paragraph_text(last).unwrap_or_default().len();
        (TextPosition::new(0, 0), TextPosition::new(last, length))
    }

    /// A handle for a stretch of the document.
    fn range(&mut self, stretch: (TextPosition, TextPosition)) -> Value {
        self.model.ranges.push(stretch);
        #[allow(clippy::cast_possible_truncation)]
        Value::Object(Handle::of(kind::RANGE, self.model.ranges.len() as u64 - 1))
    }

    /// What a handle stands for: a held range, or the selection as it is now.
    fn stretch(&self, object: &Handle) -> Result<(TextPosition, TextPosition), Fault> {
        match object.kind.as_str() {
            kind::SELECTION => Ok(self.editor.document.selection().unwrap_or_else(|| {
                let caret = self.editor.document.caret();
                (caret, caret)
            })),
            kind::DOCUMENT => Ok(self.everything()),
            kind::PARAGRAPH => {
                let index = object.id as usize;
                let length = self.editor.document.paragraph_text(index).unwrap_or_default().len();
                Ok((TextPosition::new(index, 0), TextPosition::new(index, length)))
            }
            kind::RANGE | kind::FIND | kind::FONT => self
                .model
                .ranges
                .get(object.id as usize)
                .copied()
                .ok_or_else(|| Fault::saying(91, "That range is no longer there")),
            other => Err(Fault::saying(424, &format!("{other} is not a stretch of the document"))),
        }
    }

    /// The text of a stretch, with a paragraph mark between paragraphs as
    /// Word has it.
    fn text_of(&self, (start, end): (TextPosition, TextPosition)) -> String {
        let mut out = String::new();
        for index in start.paragraph..=end.paragraph.min(self.editor.document.paragraph_count()) {
            let Some(text) = self.editor.document.paragraph_text(index) else { break };
            let from = if index == start.paragraph { start.offset } else { 0 };
            let to = if index == end.paragraph { end.offset } else { text.len() };
            out.push_str(between(&text, from, to));
            if index < end.paragraph {
                out.push('\r');
            }
        }
        out
    }

    /// Does something with the selection put where the macro is working, and
    /// puts the selection back afterwards where the document is unchanged.
    fn at<T>(
        &mut self,
        stretch: (TextPosition, TextPosition),
        what: impl FnOnce(&mut Editor) -> T,
    ) -> T {
        let held = self.editor.document.selection();
        let caret = self.editor.document.caret();
        self.editor.document.set_selections(&[stretch]);
        let answer = what(self.editor);
        // Where the edit left the document longer or shorter, putting the
        // selection back by its old numbers would put it somewhere else; the
        // document's own rule then applies, which is that the caret stays
        // where the edit left it.
        if let Some(held) = held {
            self.editor.document.set_selections(&[held]);
        } else {
            self.editor.document.set_caret(caret);
        }
        answer
    }

    /// The same, leaving the selection where the edit left it, which is what
    /// Word does when a macro edits through `Selection`.
    fn through<T>(
        &mut self,
        stretch: (TextPosition, TextPosition),
        what: impl FnOnce(&mut Editor) -> T,
    ) -> T {
        self.editor.document.set_selections(&[stretch]);
        what(self.editor)
    }

    /// A collection, or the one of it the brackets asked for.
    ///
    /// `ActiveDocument.Paragraphs` is a collection and
    /// `ActiveDocument.Paragraphs(2)` is the second paragraph: the property
    /// itself takes nothing, and the brackets belong to what it gives back.
    /// Word tells the two apart by its type library; here, a member that
    /// hands back a collection hands the brackets on to it.
    fn one_of(&mut self, collection: Value, given: &[Given]) -> Result<Value, Fault> {
        if given.is_empty() {
            return Ok(collection);
        }
        let Value::Object(handle) = &collection else { return Ok(collection) };
        let handle = handle.clone();
        self.member(&handle, "Item", given)
    }

    /// Everything the document has been changed by, in one step.
    fn done(&mut self) {
        self.editor.relayout();
        self.editor.needs_redraw = true;
    }

    /// The content controls in a stretch, by their number in the document.
    fn controls_in(&self, (start, end): (TextPosition, TextPosition)) -> Vec<usize> {
        self.editor
            .document
            .controls()
            .iter()
            .enumerate()
            .filter(|(_, control)| control.start >= start && control.start <= end)
            .map(|(at, _)| at)
            .collect()
    }

    /// One content control, by its number.
    fn control(&self, id: u64) -> Result<wp_docx::controls::Control, Fault> {
        self.editor
            .document
            .controls()
            .get(id as usize)
            .cloned()
            .ok_or_else(|| Fault::saying(91, "That content control is no longer in the document"))
    }
}

impl Host for Bound<'_> {
    fn message(&mut self, text: &str, _buttons: i64, _title: &str) -> i64 {
        self.model.shown.push(text.to_owned());
        1
    }

    fn ask(&mut self, _prompt: &str, _title: &str, default: &str) -> Option<String> {
        (!default.is_empty()).then(|| default.to_owned())
    }

    fn note(&mut self, text: &str) {
        self.model.printed.push(text.to_owned());
    }

    fn root(&mut self, name: &str) -> Option<Value> {
        Some(match name.to_ascii_lowercase().as_str() {
            "application" => Value::Object(Handle::of(kind::APPLICATION, 0)),
            "activedocument" | "thisdocument" => Value::Object(Handle::of(kind::DOCUMENT, 0)),
            "documents" => Value::Object(Handle::of(kind::DOCUMENTS, 0)),
            "selection" => Value::Object(Handle::of(kind::SELECTION, 0)),
            _ => return None,
        })
    }

    fn as_text(&mut self, object: &Handle) -> Result<String, Fault> {
        Ok(self.text_of(self.stretch(object)?))
    }

    fn items(&mut self, object: &Handle) -> Result<Vec<Value>, Fault> {
        let mut out = Vec::new();
        match object.kind.as_str() {
            kind::DOCUMENTS => out.push(Value::Object(Handle::of(kind::DOCUMENT, 0))),
            kind::PARAGRAPHS => {
                let (start, end) = self.stretch(&Handle::of(kind::RANGE, object.id))?;
                for index in start.paragraph..=end.paragraph {
                    out.push(Value::Object(Handle::of(kind::PARAGRAPH, index as u64)));
                }
            }
            kind::BOOKMARKS => {
                for (at, _) in self.editor.document.bookmarks().iter().enumerate() {
                    out.push(Value::Object(Handle::of(kind::BOOKMARK, at as u64)));
                }
            }
            kind::COMMENTS => {
                for (at, _) in self.editor.document.comments().iter().enumerate() {
                    out.push(Value::Object(Handle::of(kind::COMMENT, at as u64)));
                }
            }
            kind::STYLES => {
                for (at, _) in self.editor.document.styles().all().iter().enumerate() {
                    out.push(Value::Object(Handle::of(kind::STYLE, at as u64)));
                }
            }
            kind::CONTENT_CONTROLS => {
                let stretch = self.stretch(&Handle::of(kind::RANGE, object.id))?;
                for at in self.controls_in(stretch) {
                    out.push(Value::Object(Handle::of(kind::CONTENT_CONTROL, at as u64)));
                }
            }
            other => {
                return Err(Fault::saying(
                    438,
                    &format!("{other} is not something this program can walk through"),
                ))
            }
        }
        Ok(out)
    }

    #[allow(clippy::too_many_lines)]
    fn member(&mut self, object: &Handle, member: &str, given: &[Given]) -> Result<Value, Fault> {
        let asked = member.to_ascii_lowercase();
        let first = Given::find(given, "Index", 0).cloned();

        match (object.kind.as_str(), asked.as_str()) {
            // --- Application -------------------------------------------
            (kind::APPLICATION, "name") => Ok(Value::Text("Word Processor".to_owned())),
            (kind::APPLICATION, "version") => Ok(Value::Text(env!("CARGO_PKG_VERSION").to_owned())),
            (kind::APPLICATION, "visible") => Ok(Value::Boolean(true)),
            (kind::APPLICATION, "activedocument") => {
                Ok(Value::Object(Handle::of(kind::DOCUMENT, 0)))
            }
            (kind::APPLICATION, "selection") => Ok(Value::Object(Handle::of(kind::SELECTION, 0))),
            (kind::APPLICATION, "documents") => {
                let collection = Value::Object(Handle::of(kind::DOCUMENTS, 0));
                self.one_of(collection, given)
            }

            // --- Documents ---------------------------------------------
            (kind::DOCUMENTS, "count") => Ok(Value::Long(1)),
            (kind::DOCUMENTS, "item") => Ok(Value::Object(Handle::of(kind::DOCUMENT, 0))),

            // --- Document ----------------------------------------------
            // What the document is called is what its file is called, and a
            // document nobody has saved has the name Word gives one.
            (kind::DOCUMENT, "name") => Ok(Value::Text(
                self.editor
                    .file
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Document1".to_owned()),
            )),
            (kind::DOCUMENT, "fullname") => Ok(Value::Text(
                self.editor
                    .file
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Document1".to_owned()),
            )),
            (kind::DOCUMENT, "path") => Ok(Value::Text(
                self.editor
                    .file
                    .as_ref()
                    .and_then(|path| path.parent())
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )),
            (kind::DOCUMENT, "saved") => Ok(Value::Boolean(!self.editor.document.is_modified())),
            (kind::DOCUMENT, "content") => {
                let whole = self.everything();
                Ok(self.range(whole))
            }
            (kind::DOCUMENT, "range") => {
                let start = first.map_or(Ok(0), |value| value.whole())?;
                let end = Given::find(given, "End", 1).cloned().map_or_else(
                    || Ok(self.offset_of(self.everything().1)),
                    |value| value.whole(),
                )?;
                let stretch = (self.position_at(start), self.position_at(end));
                Ok(self.range(stretch))
            }
            (kind::DOCUMENT, "save") => {
                // A document nobody has given a name to cannot be saved
                // without asking where, and asking is a dialog a macro
                // cannot be stopped for.
                if self.editor.file.is_none() {
                    return Err(Fault::saying(
                        5,
                        "This document has never been saved, so a macro cannot save it without being asked where",
                    ));
                }
                if !self.editor.save_now() {
                    return Err(Fault::saying(5, "The document could not be saved"));
                }
                Ok(Value::Empty)
            }

            // --- Collections that hang off a document or a range -------
            (kind::DOCUMENT | kind::RANGE | kind::SELECTION | kind::PARAGRAPH, "paragraphs") => {
                let stretch = self.stretch(object)?;
                self.model.ranges.push(stretch);
                #[allow(clippy::cast_possible_truncation)]
                let collection =
                    Value::Object(Handle::of(kind::PARAGRAPHS, self.model.ranges.len() as u64 - 1));
                self.one_of(collection, given)
            }
            (kind::DOCUMENT, "bookmarks") => {
                let collection = Value::Object(Handle::of(kind::BOOKMARKS, 0));
                self.one_of(collection, given)
            }
            (kind::DOCUMENT, "comments") => {
                let collection = Value::Object(Handle::of(kind::COMMENTS, 0));
                self.one_of(collection, given)
            }
            (kind::DOCUMENT, "styles") => {
                let collection = Value::Object(Handle::of(kind::STYLES, 0));
                self.one_of(collection, given)
            }
            (kind::DOCUMENT, "tables") => {
                let collection = Value::Object(Handle::of(kind::TABLES, 0));
                self.one_of(collection, given)
            }
            (kind::DOCUMENT | kind::RANGE | kind::SELECTION, "contentcontrols") => {
                let stretch = self.stretch(object)?;
                self.model.ranges.push(stretch);
                #[allow(clippy::cast_possible_truncation)]
                let collection = Value::Object(Handle::of(
                    kind::CONTENT_CONTROLS,
                    self.model.ranges.len() as u64 - 1,
                ));
                self.one_of(collection, given)
            }

            // --- Content controls ---------------------------------------
            (kind::CONTENT_CONTROLS, "count") => {
                let stretch = self.stretch(&Handle::of(kind::RANGE, object.id))?;
                Ok(Value::Long(self.controls_in(stretch).len() as i64))
            }
            (kind::CONTENT_CONTROLS, "item") => {
                let stretch = self.stretch(&Handle::of(kind::RANGE, object.id))?;
                let which = first.map_or(Ok(1), |value| value.whole())?;
                let inside = self.controls_in(stretch);
                let at = usize::try_from(which - 1).map_err(|_| Fault::of(9))?;
                let found = inside.get(at).copied().ok_or_else(|| Fault::of(9))?;
                Ok(Value::Object(Handle::of(kind::CONTENT_CONTROL, found as u64)))
            }
            (kind::CONTENT_CONTROL, "title") => Ok(Value::Text(self.control(object.id)?.alias)),
            (kind::CONTENT_CONTROL, "tag") => Ok(Value::Text(self.control(object.id)?.tag)),
            (kind::CONTENT_CONTROL, "type") => {
                Ok(Value::Long(control_type(self.control(object.id)?.kind)))
            }
            (kind::CONTENT_CONTROL, "checked") => {
                Ok(Value::Boolean(self.control(object.id)?.checked))
            }
            (kind::CONTENT_CONTROL, "lockcontents") => {
                Ok(Value::Boolean(self.control(object.id)?.locked_edit))
            }
            (kind::CONTENT_CONTROL, "lockcontentcontrol") => {
                Ok(Value::Boolean(self.control(object.id)?.locked_delete))
            }
            (kind::CONTENT_CONTROL, "range") => {
                let control = self.control(object.id)?;
                let range = self.range((control.start, control.end));
                if let Value::Object(handle) = &range {
                    self.model.inside_controls.push((handle.id, control.start));
                }
                Ok(range)
            }

            // --- Paragraphs ---------------------------------------------
            (kind::PARAGRAPHS, "count") => {
                let (start, end) = self.stretch(&Handle::of(kind::RANGE, object.id))?;
                Ok(Value::Long((end.paragraph - start.paragraph + 1) as i64))
            }
            (kind::PARAGRAPHS, "item") => {
                let (start, end) = self.stretch(&Handle::of(kind::RANGE, object.id))?;
                let which = first.map_or(Ok(1), |value| value.whole())?;
                let index = start.paragraph as i64 + which - 1;
                if which < 1 || index > end.paragraph as i64 {
                    return Err(Fault::of(9));
                }
                Ok(Value::Object(Handle::of(kind::PARAGRAPH, index as u64)))
            }
            (kind::PARAGRAPHS, "add") => {
                let at = self.editor.document.paragraph_count();
                self.editor.document.append_paragraph(&wp_docx::model::Paragraph::text(""));
                self.done();
                Ok(Value::Object(Handle::of(kind::PARAGRAPH, at as u64)))
            }

            // --- Paragraph ----------------------------------------------
            (kind::PARAGRAPH, "range") => {
                let stretch = self.stretch(object)?;
                Ok(self.range(stretch))
            }
            (kind::PARAGRAPH, "style") => {
                let style = self.editor.document.style_of(object.id as usize);
                Ok(Value::Text(style.unwrap_or_else(|| "Normal".to_owned())))
            }
            (kind::PARAGRAPH, "alignment") => {
                let stretch = self.stretch(object)?;
                let alignment = self.at(stretch, |editor| editor.document.alignment_here());
                Ok(Value::Long(match alignment {
                    Alignment::Start => 0,
                    Alignment::Center => 1,
                    Alignment::End => 2,
                    Alignment::Both => 3,
                }))
            }

            // --- Range and Selection ------------------------------------
            (kind::RANGE | kind::SELECTION | kind::PARAGRAPH | kind::DOCUMENT, "text") => {
                Ok(Value::Text(self.text_of(self.stretch(object)?)))
            }
            (kind::RANGE | kind::SELECTION | kind::PARAGRAPH, "start") => {
                Ok(Value::Long(self.offset_of(self.stretch(object)?.0)))
            }
            (kind::RANGE | kind::SELECTION | kind::PARAGRAPH, "end") => {
                Ok(Value::Long(self.offset_of(self.stretch(object)?.1)))
            }
            (kind::SELECTION, "range") => {
                let stretch = self.stretch(object)?;
                Ok(self.range(stretch))
            }
            (kind::RANGE | kind::SELECTION, "font") => {
                let stretch = self.stretch(object)?;
                self.model.ranges.push(stretch);
                #[allow(clippy::cast_possible_truncation)]
                Ok(Value::Object(Handle::of(kind::FONT, self.model.ranges.len() as u64 - 1)))
            }
            (kind::RANGE | kind::SELECTION, "find") => {
                let stretch = self.stretch(object)?;
                self.model.ranges.push(stretch);
                #[allow(clippy::cast_possible_truncation)]
                Ok(Value::Object(Handle::of(kind::FIND, self.model.ranges.len() as u64 - 1)))
            }
            (kind::RANGE | kind::SELECTION, "style") => {
                let stretch = self.stretch(object)?;
                let style = self.at(stretch, |editor| editor.document.style_here());
                Ok(Value::Text(style.unwrap_or_else(|| "Normal".to_owned())))
            }
            (kind::RANGE | kind::SELECTION, "bold" | "italic" | "underline") => {
                let stretch = self.stretch(object)?;
                let format = format_named(&asked)?;
                let on = self.at(stretch, |editor| editor.document.format_is_on(format));
                Ok(Value::Boolean(on))
            }
            (kind::RANGE | kind::SELECTION, "insertafter") => {
                let (_, end) = self.stretch(object)?;
                let text = Given::find(given, "Text", 0).cloned().unwrap_or(Value::Empty).text()?;
                self.through((end, end), |editor| editor.document.type_text(&text));
                self.done();
                Ok(Value::Empty)
            }
            (kind::RANGE | kind::SELECTION, "insertbefore") => {
                let (start, _) = self.stretch(object)?;
                let text = Given::find(given, "Text", 0).cloned().unwrap_or(Value::Empty).text()?;
                self.through((start, start), |editor| editor.document.type_text(&text));
                self.done();
                Ok(Value::Empty)
            }
            (kind::RANGE | kind::SELECTION | kind::PARAGRAPH, "delete") => {
                let stretch = self.stretch(object)?;
                self.through(stretch, |editor| editor.document.delete_selection());
                self.done();
                Ok(Value::Empty)
            }
            (kind::RANGE | kind::PARAGRAPH, "select") => {
                let stretch = self.stretch(object)?;
                self.editor.document.set_selections(&[stretch]);
                self.editor.needs_redraw = true;
                Ok(Value::Empty)
            }
            (kind::RANGE | kind::SELECTION, "collapse") => {
                let (start, end) = self.stretch(object)?;
                // Word's wdCollapseEnd is nought and wdCollapseStart is one.
                let to_start = Given::find(given, "Direction", 0)
                    .cloned()
                    .map_or(Ok(true), |value| value.whole().map(|number| number != 0))?;
                let at = if to_start { start } else { end };
                if object.kind == kind::SELECTION {
                    self.editor.document.set_caret(at);
                } else {
                    self.model.ranges[object.id as usize] = (at, at);
                }
                Ok(Value::Empty)
            }
            (kind::SELECTION, "typetext") => {
                let text = Given::find(given, "Text", 0).cloned().unwrap_or(Value::Empty).text()?;
                self.editor.document.type_text(&text);
                self.done();
                Ok(Value::Empty)
            }
            (kind::SELECTION, "typeparagraph") => {
                self.editor.document.press_enter();
                self.done();
                Ok(Value::Empty)
            }
            (kind::SELECTION, "homekey") => {
                self.editor.document.set_caret(TextPosition::new(0, 0));
                Ok(Value::Empty)
            }
            (kind::SELECTION, "endkey") => {
                let (_, end) = self.everything();
                self.editor.document.set_caret(end);
                Ok(Value::Empty)
            }

            // --- Font ----------------------------------------------------
            (kind::FONT, "name") => {
                let stretch = self.stretch(object)?;
                let name = self.at(stretch, |editor| editor.document.font_here());
                Ok(Value::Text(name.unwrap_or_default()))
            }
            (kind::FONT, "size") => {
                let stretch = self.stretch(object)?;
                let size = self.at(stretch, |editor| editor.document.size_here());
                Ok(Value::Double(f64::from(size)))
            }
            (kind::FONT, "bold" | "italic" | "underline") => {
                let stretch = self.stretch(object)?;
                let format = format_named(&asked)?;
                let on = self.at(stretch, |editor| editor.document.format_is_on(format));
                Ok(Value::Boolean(on))
            }

            // --- Styles ---------------------------------------------------
            (kind::STYLES, "count") => {
                Ok(Value::Long(self.editor.document.styles().all().len() as i64))
            }
            (kind::STYLES, "item") => {
                let wanted = first.unwrap_or(Value::Empty);
                let styles = self.editor.document.styles();
                let at = match &wanted {
                    Value::Text(name) => styles
                        .all()
                        .iter()
                        .position(|style| {
                            style.id.eq_ignore_ascii_case(name)
                                || style
                                    .name
                                    .as_deref()
                                    .is_some_and(|had| had.eq_ignore_ascii_case(name))
                        })
                        .ok_or_else(|| Fault::of(9))?,
                    other => usize::try_from(other.whole()? - 1).map_err(|_| Fault::of(9))?,
                };
                if at >= styles.all().len() {
                    return Err(Fault::of(9));
                }
                Ok(Value::Object(Handle::of(kind::STYLE, at as u64)))
            }
            (kind::STYLE, "namelocal" | "name") => {
                let styles = self.editor.document.styles();
                let style = styles.all().get(object.id as usize).ok_or_else(|| Fault::of(9))?;
                Ok(Value::Text(style.name.clone().unwrap_or_else(|| style.id.clone())))
            }

            // --- Bookmarks -------------------------------------------------
            (kind::BOOKMARKS, "count") => {
                Ok(Value::Long(self.editor.document.bookmarks().len() as i64))
            }
            (kind::BOOKMARKS, "exists") => {
                let name = first.unwrap_or(Value::Empty).text()?;
                Ok(Value::Boolean(
                    self.editor
                        .document
                        .bookmarks()
                        .iter()
                        .any(|bookmark| bookmark.name.eq_ignore_ascii_case(&name)),
                ))
            }
            (kind::BOOKMARKS, "item") => {
                let wanted = first.unwrap_or(Value::Empty);
                let bookmarks = self.editor.document.bookmarks();
                let at = match &wanted {
                    Value::Text(name) => bookmarks
                        .iter()
                        .position(|bookmark| bookmark.name.eq_ignore_ascii_case(name))
                        .ok_or_else(|| Fault::of(9))?,
                    other => usize::try_from(other.whole()? - 1).map_err(|_| Fault::of(9))?,
                };
                if at >= bookmarks.len() {
                    return Err(Fault::of(9));
                }
                Ok(Value::Object(Handle::of(kind::BOOKMARK, at as u64)))
            }
            (kind::BOOKMARKS, "add") => {
                let name = first.unwrap_or(Value::Empty).text()?;
                if !self.editor.document.add_bookmark(&name) {
                    return Err(Fault::saying(5, "That bookmark could not be added"));
                }
                self.done();
                let at = self
                    .editor
                    .document
                    .bookmarks()
                    .iter()
                    .position(|bookmark| bookmark.name.eq_ignore_ascii_case(&name))
                    .unwrap_or_default();
                Ok(Value::Object(Handle::of(kind::BOOKMARK, at as u64)))
            }
            (kind::BOOKMARK, "name") => {
                let bookmarks = self.editor.document.bookmarks();
                let bookmark = bookmarks.get(object.id as usize).ok_or_else(|| Fault::of(9))?;
                Ok(Value::Text(bookmark.name.clone()))
            }
            (kind::BOOKMARK, "range") => {
                let bookmarks = self.editor.document.bookmarks();
                let bookmark = bookmarks.get(object.id as usize).ok_or_else(|| Fault::of(9))?;
                let stretch = bookmark.range;
                Ok(self.range(stretch))
            }

            // --- Comments ---------------------------------------------------
            (kind::COMMENTS, "count") => {
                Ok(Value::Long(self.editor.document.comments().len() as i64))
            }
            (kind::COMMENTS, "item") => {
                let which = first.map_or(Ok(1), |value| value.whole())?;
                let at = usize::try_from(which - 1).map_err(|_| Fault::of(9))?;
                if at >= self.editor.document.comments().len() {
                    return Err(Fault::of(9));
                }
                Ok(Value::Object(Handle::of(kind::COMMENT, at as u64)))
            }
            (kind::COMMENT, "author" | "initial" | "range") => {
                let comments = self.editor.document.comments();
                let comment = comments.get(object.id as usize).ok_or_else(|| Fault::of(9))?;
                match asked.as_str() {
                    "author" => Ok(Value::Text(comment.author.clone())),
                    "initial" => Ok(Value::Text(comment.initials.clone())),
                    _ => {
                        let stretch = comment.range.ok_or_else(|| Fault::of(9))?;
                        Ok(self.range(stretch))
                    }
                }
            }

            // --- Tables ------------------------------------------------------
            (kind::TABLES, "count") => Ok(Value::Long(self.tables().len() as i64)),
            (kind::TABLES, "item") => {
                let which = first.map_or(Ok(1), |value| value.whole())?;
                let at = usize::try_from(which - 1).map_err(|_| Fault::of(9))?;
                let tables = self.tables();
                if at >= tables.len() {
                    return Err(Fault::of(9));
                }
                Ok(Value::Object(Handle::of(kind::TABLE, tables[at] as u64)))
            }
            (kind::TABLE, "rows" | "columns") => {
                let at = self
                    .editor
                    .document
                    .table_at(object.id as usize)
                    .ok_or_else(|| Fault::of(9))?;
                Ok(Value::Long(if asked == "rows" { at.rows as i64 } else { at.columns as i64 }))
            }

            // --- Find ---------------------------------------------------------
            (kind::FIND, "text") => {
                let held = self.model.finding.iter().find(|(id, _, _)| *id == object.id);
                Ok(Value::Text(held.map(|(_, text, _)| text.clone()).unwrap_or_default()))
            }
            (kind::FIND, "execute") => self.execute_find(object, given),

            _ => Err(Fault::saying(
                438,
                &format!("{}.{member} is not something this program does yet", object.kind),
            )),
        }
    }

    fn set_member(&mut self, object: &Handle, member: &str, value: Value) -> Result<(), Fault> {
        let asked = member.to_ascii_lowercase();
        match (object.kind.as_str(), asked.as_str()) {
            // --- Content controls ---------------------------------------
            (kind::CONTENT_CONTROL, "title" | "tag" | "lockcontents" | "lockcontentcontrol") => {
                let control = self.control(object.id)?;
                let (mut alias, mut tag) = (control.alias.clone(), control.tag.clone());
                let (mut locked_delete, mut locked_edit) =
                    (control.locked_delete, control.locked_edit);
                match asked.as_str() {
                    "title" => alias = value.text()?,
                    "tag" => tag = value.text()?,
                    "lockcontents" => locked_edit = value.truth()?,
                    _ => locked_delete = value.truth()?,
                }
                if !self.editor.document.set_control_properties(
                    control.start,
                    &alias,
                    &tag,
                    locked_delete,
                    locked_edit,
                ) {
                    return Err(Fault::saying(5, "That content control could not be changed"));
                }
                self.done();
                Ok(())
            }
            (kind::CONTENT_CONTROL, "checked") => {
                let control = self.control(object.id)?;
                if !self.editor.document.set_control_checked(control.start, value.truth()?) {
                    return Err(Fault::saying(5, "Only a check box content control can be ticked"));
                }
                self.done();
                Ok(())
            }
            // A range that is a control's inside is written through the
            // control, which keeps the control round it.
            (kind::RANGE, "text")
                if self.model.inside_controls.iter().any(|(id, _)| *id == object.id) =>
            {
                let start = self
                    .model
                    .inside_controls
                    .iter()
                    .find(|(id, _)| *id == object.id)
                    .map(|(_, start)| *start)
                    .ok_or_else(|| Fault::of(91))?;
                if !self.editor.document.set_control_text(start, &value.text()?) {
                    return Err(Fault::saying(5, "That content control takes no text"));
                }
                self.done();
                Ok(())
            }
            (kind::RANGE | kind::SELECTION | kind::PARAGRAPH, "text") => {
                let stretch = self.stretch(object)?;
                let text = value.text()?;
                self.through(stretch, |editor| {
                    editor.document.delete_selection();
                    editor.document.type_text(&text);
                });
                self.done();
                Ok(())
            }
            (kind::RANGE | kind::SELECTION | kind::PARAGRAPH, "style") => {
                let stretch = self.stretch(object)?;
                let wanted = value.text()?;
                let known = self
                    .editor
                    .document
                    .styles()
                    .all()
                    .iter()
                    .find(|style| {
                        style.id.eq_ignore_ascii_case(&wanted)
                            || style
                                .name
                                .as_deref()
                                .is_some_and(|had| had.eq_ignore_ascii_case(&wanted))
                    })
                    .map(|style| style.id.clone())
                    .ok_or_else(|| {
                        Fault::saying(5, &format!("This document has no style called {wanted}"))
                    })?;
                self.at(stretch, |editor| {
                    editor.document.set_paragraph_style_here(Some(&known));
                });
                self.done();
                Ok(())
            }
            (kind::RANGE | kind::SELECTION | kind::FONT, "bold" | "italic" | "underline") => {
                let stretch = self.stretch(object)?;
                let format = format_named(&asked)?;
                let on = value.truth()?;
                self.at(stretch, |editor| editor.document.set_format(format, on));
                self.done();
                Ok(())
            }
            (kind::FONT, "name") => {
                let stretch = self.stretch(object)?;
                let name = value.text()?;
                self.at(stretch, |editor| editor.document.set_font(&name));
                self.done();
                Ok(())
            }
            (kind::FONT, "size") => {
                let stretch = self.stretch(object)?;
                #[allow(clippy::cast_possible_truncation)]
                let points = value.number()? as f32;
                self.at(stretch, |editor| editor.document.set_size(points));
                self.done();
                Ok(())
            }
            (kind::PARAGRAPH, "alignment") => {
                let stretch = self.stretch(object)?;
                let alignment = match value.whole()? {
                    1 => Alignment::Center,
                    2 => Alignment::End,
                    3 => Alignment::Both,
                    _ => Alignment::Start,
                };
                self.at(stretch, |editor| editor.document.set_alignment_here(alignment));
                self.done();
                Ok(())
            }
            (kind::SELECTION, "start" | "end") => {
                let at = self.position_at(value.whole()?);
                let (start, end) = self.stretch(object)?;
                let stretch =
                    if asked == "start" { (at, end.max(at)) } else { (start.min(at), at) };
                self.editor.document.set_selections(&[stretch]);
                Ok(())
            }
            (kind::RANGE, "start" | "end") => {
                let at = self.position_at(value.whole()?);
                let (start, end) = self.stretch(object)?;
                self.model.ranges[object.id as usize] =
                    if asked == "start" { (at, end.max(at)) } else { (start.min(at), at) };
                Ok(())
            }
            (kind::FIND, "text") => {
                let text = value.text()?;
                self.remember_find(object.id, Some(text), None);
                Ok(())
            }
            (kind::FIND, "replacement") => Err(Fault::saying(
                438,
                "Find.Replacement is set through Execute here: give it ReplaceWith",
            )),
            _ => Err(Fault::saying(
                438,
                &format!("{}.{member} is not something this program sets yet", object.kind),
            )),
        }
    }
}

impl Bound<'_> {
    /// Which paragraphs begin a table.
    fn tables(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut seen: Vec<Vec<usize>> = Vec::new();
        for index in 0..self.editor.document.paragraph_count() {
            if let Some(at) = self.editor.document.table_at(index) {
                if !seen.contains(&at.table) {
                    seen.push(at.table);
                    out.push(index);
                }
            }
        }
        out
    }

    /// What a `Find` has been told, kept by the range it belongs to.
    fn remember_find(&mut self, id: u64, text: Option<String>, replacement: Option<String>) {
        if let Some(held) = self.model.finding.iter_mut().find(|(had, _, _)| *had == id) {
            if let Some(text) = text {
                held.1 = text;
            }
            if let Some(replacement) = replacement {
                held.2 = replacement;
            }
            return;
        }
        self.model.finding.push((id, text.unwrap_or_default(), replacement.unwrap_or_default()));
    }

    /// `Find.Execute`, which both finds and replaces depending on what it is
    /// given — as Word's does.
    fn execute_find(&mut self, object: &Handle, given: &[Given]) -> Result<Value, Fault> {
        let wanted = match Given::find(given, "FindText", 0) {
            Some(value) => value.text()?,
            None => self
                .model
                .finding
                .iter()
                .find(|(id, _, _)| *id == object.id)
                .map(|(_, text, _)| text.clone())
                .unwrap_or_default(),
        };
        if wanted.is_empty() {
            return Err(Fault::saying(5, "Find was given nothing to look for"));
        }
        let replacement = Given::find(given, "ReplaceWith", 1).cloned();
        let replace_all =
            Given::find(given, "Replace", 2).map_or(Ok(0), |value| value.whole()).unwrap_or(0);
        let matching = wp_docx::search::Matching {
            whole_word: false,
            match_case: Given::find(given, "MatchCase", 3)
                .map_or(Ok(false), |value| value.truth())?,
        };

        match replacement {
            Some(replacement) => {
                let with = replacement.text()?;
                // Word's wdReplaceAll is two and wdReplaceOne is one, and
                // the difference matters to a macro that means one.
                if replace_all >= 2 {
                    let changed = self.editor.document.replace_matching(&wanted, &with, matching);
                    self.done();
                    return Ok(Value::Boolean(changed > 0));
                }
                let Some((at, length)) =
                    self.editor.document.find_all(&wanted, matching).first().copied()
                else {
                    return Ok(Value::Boolean(false));
                };
                let end = TextPosition::new(at.paragraph, at.offset + length);
                self.through((at, end), |editor| {
                    editor.document.delete_selection();
                    editor.document.type_text(&with);
                });
                self.done();
                Ok(Value::Boolean(true))
            }
            None => {
                let found = self.editor.document.find_all(&wanted, matching);
                if let Some((at, length)) = found.first().copied() {
                    let end = TextPosition::new(at.paragraph, at.offset + length);
                    self.editor.document.set_selections(&[(at, end)]);
                    self.editor.needs_redraw = true;
                }
                Ok(Value::Boolean(!found.is_empty()))
            }
        }
    }
}

/// The format a name stands for.
fn format_named(name: &str) -> Result<CharacterFormat, Fault> {
    Ok(match name {
        "bold" => CharacterFormat::Bold,
        "italic" => CharacterFormat::Italic,
        "underline" => CharacterFormat::Underline,
        _ => return Err(Fault::of(438)),
    })
}

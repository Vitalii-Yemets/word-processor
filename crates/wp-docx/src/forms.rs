//! The fields of a form, and where a person may type in one.
//!
//! # What a form is in this format
//!
//! A field written the long way — see [`crate::fields`] — whose begin marker
//! carries a `w:ffData` child. That child says what sort of field it is, what
//! it is called, and what may be put in it; the runs between the `separate`
//! marker and the `end` marker are the answer, and they are what a person
//! typing into the form changes.
//!
//! ```text
//! <w:r><w:fldChar w:fldCharType="begin"><w:ffData>
//!        <w:name w:val="Surname"/><w:enabled/>
//!        <w:textInput><w:default w:val="—"/></w:textInput>
//! </w:ffData></w:fldChar></w:r>
//! <w:r><w:instrText> FORMTEXT </w:instrText></w:r>
//! <w:r><w:fldChar w:fldCharType="separate"/></w:r>
//! <w:r><w:t>Habgood</w:t></w:r>              ← the answer
//! <w:r><w:fldChar w:fldCharType="end"/></w:r>
//! ```
//!
//! # Why it is here rather than in the editor
//!
//! Because the question it answers is a question about the document: *where in
//! this text may somebody type when the document is protected for forms?* The
//! answer is a list of stretches, worked out from the file, and the program in
//! front of the person has only to ask.
//!
//! # What is not here
//!
//! Making one. This program reads the form fields a document already has and
//! lets them be filled in; it has no Developer tab and no command that puts a
//! text box, a tick box or a drop-down into a document. That is named in the
//! roadmap rather than left to be discovered.
//!
//! Nor is the behaviour of a tick box or a drop-down: this program can say
//! that one is there and what it holds, and a person filling in the form can
//! change the text of a text field, which is the one of the three whose answer
//! is text.

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::{read, Document, TextPosition};

/// Which of the three sorts of field it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormKind {
    /// A box somebody types into.
    Text,
    /// A box that is ticked or not.
    CheckBox,
    /// One of a list.
    DropDown,
}

impl FormKind {
    fn of(data: &Element) -> Self {
        for child in data.child_elements() {
            if child.namespace.as_deref() != Some(read::W) {
                continue;
            }
            match child.local_name() {
                "checkBox" => return Self::CheckBox,
                "ddList" => return Self::DropDown,
                "textInput" => return Self::Text,
                _ => {}
            }
        }
        // A `w:ffData` naming none of the three is a text field, which is
        // what the schema's own default says and what Word shows.
        Self::Text
    }
}

/// One field of a form, and where its answer sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormField {
    /// What it is called — the name a macro or a mail merge would use. Empty
    /// where the document did not name it, which is allowed.
    pub name: String,
    pub kind: FormKind,
    /// Whether it may be filled in at all. Word's tick box for this is
    /// "Fill-in enabled", and a field with it off is shown and not typed in.
    pub enabled: bool,
    /// Where the answer starts: just after the `separate` marker.
    pub start: TextPosition,
    /// And where it ends: just before the `end` marker.
    pub end: TextPosition,
    /// What a drop-down offers, in order. Empty for the other two.
    pub items: Vec<String>,
    /// Whether a tick box is ticked. False for the other two.
    pub checked: bool,
    /// Which of a drop-down's entries is chosen, counted from nought.
    pub chosen: usize,
}

impl FormField {
    /// Whether a position is inside the answer.
    ///
    /// Both ends count. A form field whose answer is empty is one position
    /// wide, and it has to be typeable or an empty form could not be filled
    /// in at all.
    #[must_use]
    pub fn covers(&self, position: TextPosition) -> bool {
        position >= self.start && position <= self.end
    }
}

impl Document {
    /// Every form field in the body, in the order they appear.
    #[must_use]
    pub fn form_fields(&self) -> Vec<FormField> {
        let mut found = Vec::new();
        for (index, paragraph) in self.paragraph_elements().into_iter().enumerate() {
            let mut offset = 0usize;
            let mut open: Vec<Option<Opening>> = Vec::new();
            walk(paragraph, index, &mut offset, &mut open, &mut found);
        }
        found
    }

    /// The form field a position is inside, if it is inside one.
    ///
    /// Two fields side by side share the position between them — it is the
    /// end of one and the start of the next — so a rule is needed, and the
    /// rule is that a position belongs to the field beginning there. A person
    /// who clicks or tabs to a place between two boxes is going into the
    /// second, not back into the first.
    #[must_use]
    pub fn form_field_at(&self, position: TextPosition) -> Option<FormField> {
        let fields = self.form_fields();
        fields
            .iter()
            .find(|field| field.start == position)
            .or_else(|| fields.iter().find(|field| field.covers(position)))
            .cloned()
    }

    /// Whether a section is one of those a form protection closes.
    ///
    /// Word's Restrict Editing has a Select Sections link beside the forms
    /// mode: a document may be a form in one section and free text in
    /// another, and the section says which it is. A section that does not say
    /// is closed, because the restriction was put on the document and a
    /// section has to ask to be let out of it.
    #[must_use]
    pub fn section_is_form_protected(&self, section: usize) -> bool {
        let Some(properties) = crate::sections::properties_of(&self.tree().root, section) else {
            return true;
        };
        properties.child(Some(read::W), "formProt").is_none_or(read::on_off)
    }
}

/// What a tick box shows when it is ticked, and when it is not.
///
/// Word draws the box itself from the field; a program that only draws text
/// draws these, which is what every other reader of the format shows and what
/// prints on paper. The characters are the ballot box and the ballot box with
/// a cross in it, which are in every font that has a box at all.
pub const TICKED: char = '\u{2612}';
pub const UNTICKED: char = '\u{2610}';

impl Document {
    /// Puts a form field in at the caret.
    ///
    /// `items` is what a drop-down offers and is ignored by the other two.
    /// Returns whether it went in.
    pub fn insert_form_field(&mut self, kind: FormKind, name: &str, items: &[String]) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(path) = crate::position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some(paragraph) =
            crate::edit::element_at_path_mut(&mut self.tree_to_edit().root, &path)
        else {
            return false;
        };
        crate::format::split_runs_at_offset(paragraph, caret.offset);
        let at = past_any_field_end(
            paragraph,
            crate::edit::child_position_at_offset(paragraph, caret.offset),
        );

        let shown = match kind {
            FormKind::Text => String::new(),
            FormKind::CheckBox => UNTICKED.to_string(),
            FormKind::DropDown => items.first().cloned().unwrap_or_default(),
        };
        for (step, element) in
            field_runs(kind, name, items, &shown, prefix.as_deref()).into_iter().enumerate()
        {
            paragraph.insert_element(at + step, element);
        }

        // The caret goes past the field, which is where somebody who put one
        // in wants to carry on typing.
        self.set_caret(TextPosition::new(caret.paragraph, caret.offset + shown.len()));
        self.note_change();
        true
    }

    /// Ticks or unticks the tick box at a position.
    pub fn set_check_box(&mut self, at: TextPosition, on: bool) -> bool {
        let Some(field) = self.form_field_at(at) else { return false };
        if field.kind != FormKind::CheckBox || !field.enabled {
            return false;
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(path) = crate::position::paragraph_path(&self.tree().root, field.start.paragraph)
        else {
            return false;
        };
        let Some(paragraph) =
            crate::edit::element_at_path_mut(&mut self.tree_to_edit().root, &path)
        else {
            return false;
        };
        set_ff_flag(paragraph, &field.name, "checkBox", "checked", on, prefix.as_deref());
        replace_result(
            paragraph,
            &field,
            &if on { TICKED.to_string() } else { UNTICKED.to_string() },
        );

        self.note_change();
        true
    }

    /// Picks one of a drop-down's entries.
    pub fn choose_form_item(&mut self, at: TextPosition, index: usize) -> bool {
        let Some(field) = self.form_field_at(at) else { return false };
        if field.kind != FormKind::DropDown || !field.enabled {
            return false;
        }
        let Some(chosen) = field.items.get(index).cloned() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(path) = crate::position::paragraph_path(&self.tree().root, field.start.paragraph)
        else {
            return false;
        };
        let Some(paragraph) =
            crate::edit::element_at_path_mut(&mut self.tree_to_edit().root, &path)
        else {
            return false;
        };
        set_ff_value(
            paragraph,
            &field.name,
            "ddList",
            "result",
            &index.to_string(),
            prefix.as_deref(),
        );
        replace_result(paragraph, &field, &chosen);

        self.note_change();
        true
    }
}

/// Where a new field may go, given where the caret said.
///
/// A field is five runs and nothing may be put between them. The caret at the
/// end of a field's answer lands on the run holding the closing marker, and a
/// field written there would be a field inside a field — which reads back in
/// the wrong order and is not what anybody meant. So the position walks past
/// any marker it is standing on.
fn past_any_field_end(paragraph: &Element, mut at: usize) -> usize {
    while let Some(child) = paragraph.children.get(at).and_then(|node| node.as_element()) {
        let marker = child.child(Some(read::W), "fldChar").is_some()
            || child.child(Some(read::W), "instrText").is_some();
        if !marker {
            break;
        }
        at += 1;
    }
    at
}

/// The five runs a form field is written as.
fn field_runs(
    kind: FormKind,
    name: &str,
    items: &[String],
    shown: &str,
    prefix: Option<&str>,
) -> Vec<Element> {
    let named = |local: &str| crate::edit::name_with(prefix, local);
    let valued = |local: &str, value: &str| {
        let mut element = Element::new(&named(local), Some(read::W));
        element.set_namespaced_attribute(&named("val"), read::W, value);
        element
    };

    let mut data = Element::new(&named("ffData"), Some(read::W));
    data.push_element(valued("name", name));
    data.push_element(Element::new(&named("enabled"), Some(read::W)));
    match kind {
        FormKind::Text => {
            data.push_element(Element::new(&named("textInput"), Some(read::W)));
        }
        FormKind::CheckBox => {
            let mut box_of = Element::new(&named("checkBox"), Some(read::W));
            box_of.push_element(Element::new(&named("sizeAuto"), Some(read::W)));
            box_of.push_element(valued("default", "0"));
            box_of.push_element(valued("checked", "0"));
            data.push_element(box_of);
        }
        FormKind::DropDown => {
            let mut list = Element::new(&named("ddList"), Some(read::W));
            list.push_element(valued("result", "0"));
            for item in items {
                list.push_element(valued("listEntry", item));
            }
            data.push_element(list);
        }
    }

    let mut begin = Element::new(&named("fldChar"), Some(read::W));
    begin.set_namespaced_attribute(&named("fldCharType"), read::W, "begin");
    begin.push_element(data);
    let mut separate = Element::new(&named("fldChar"), Some(read::W));
    separate.set_namespaced_attribute(&named("fldCharType"), read::W, "separate");
    let mut end = Element::new(&named("fldChar"), Some(read::W));
    end.set_namespaced_attribute(&named("fldCharType"), read::W, "end");

    let run_with = |inside: Element| {
        let mut run = Element::new(&named("r"), Some(read::W));
        run.push_element(inside);
        run
    };
    let mut instruction = Element::new(&named("instrText"), Some(read::W));
    crate::edit::preserve_space_if_needed(&mut instruction, " FORMTEXT ");
    instruction.set_text(match kind {
        FormKind::Text => " FORMTEXT ",
        FormKind::CheckBox => " FORMCHECKBOX ",
        FormKind::DropDown => " FORMDROPDOWN ",
    });

    let mut result = Element::new(&named("r"), Some(read::W));
    let mut text = Element::new(&named("t"), Some(read::W));
    crate::edit::preserve_space_if_needed(&mut text, shown);
    text.set_text(shown);
    result.push_element(text);

    vec![run_with(begin), run_with(instruction), run_with(separate), result, run_with(end)]
}

/// Sets an on/off value inside a field's `w:ffData`.
fn set_ff_flag(
    paragraph: &mut Element,
    name: &str,
    inside: &str,
    local: &str,
    on: bool,
    prefix: Option<&str>,
) {
    set_ff_value(paragraph, name, inside, local, if on { "1" } else { "0" }, prefix);
}

/// And a value of any kind.
fn set_ff_value(
    paragraph: &mut Element,
    name: &str,
    inside: &str,
    local: &str,
    value: &str,
    prefix: Option<&str>,
) {
    let Some(data) = find_ff_data_mut(paragraph, name) else { return };
    let Some(part) = data.child_mut(Some(read::W), inside) else { return };
    let named = crate::edit::name_with(prefix, local);
    match part.child_mut(Some(read::W), local) {
        Some(element) => {
            element.set_namespaced_attribute(
                &crate::edit::name_with(prefix, "val"),
                read::W,
                value,
            );
        }
        None => {
            let mut element = Element::new(&named, Some(read::W));
            element.set_namespaced_attribute(
                &crate::edit::name_with(prefix, "val"),
                read::W,
                value,
            );
            part.push_element(element);
        }
    }
}

/// The `w:ffData` of the field with this name, to be written into.
fn find_ff_data_mut<'a>(element: &'a mut Element, name: &str) -> Option<&'a mut Element> {
    let mut found = None;
    for (index, child) in element.children.iter().enumerate() {
        let Some(child) = child.as_element() else { continue };
        if let Some(data) = child.child(Some(read::W), "ffData") {
            let said = data
                .child(Some(read::W), "name")
                .and_then(|child| child.attribute(Some(read::W), "val"))
                .unwrap_or_default();
            if said == name {
                found = Some(index);
                break;
            }
        }
    }
    match found {
        Some(index) => element.children[index]
            .as_element_mut()
            .and_then(|child| child.child_mut(Some(read::W), "ffData")),
        None => {
            // Not at this level, so inside something at this level.
            for child in &mut element.children {
                if let Some(child) = child.as_element_mut() {
                    if let Some(found) = find_ff_data_mut(child, name) {
                        return Some(found);
                    }
                }
            }
            None
        }
    }
}

/// Writes new words into a field's answer, taking the old ones out.
///
/// The answer is whatever lies between the `separate` marker and the `end`
/// one, which is exactly what [`FormField::start`] and [`FormField::end`]
/// say.
fn replace_result(paragraph: &mut Element, field: &FormField, text: &str) {
    let mut offset = 0usize;
    let mut writing = false;
    let mut done = false;
    write_result_within(paragraph, field, &mut offset, &mut writing, &mut done, text);
}

fn write_result_within(
    element: &mut Element,
    field: &FormField,
    offset: &mut usize,
    writing: &mut bool,
    done: &mut bool,
    text: &str,
) {
    let mut index = 0usize;
    while index < element.children.len() {
        let Some(child) = element.children[index].as_element() else {
            index += 1;
            continue;
        };
        if child.namespace.as_deref() != Some(read::W) {
            index += 1;
            continue;
        }
        match child.local_name() {
            "fldChar" => {
                let word = child.attribute(Some(read::W), "fldCharType").unwrap_or("begin");
                if word == "separate" && *offset == field.start.offset {
                    *writing = true;
                } else if word == "end" && *writing {
                    *writing = false;
                    *done = true;
                }
                index += 1;
            }
            "t" => {
                let length = child.text_content().len();
                if *writing && !*done {
                    // The first run of the answer takes the new words; the
                    // rest are emptied, so that what was there is gone.
                    if let Some(child) = element.children[index].as_element_mut() {
                        let wanted = if *offset == field.start.offset { text } else { "" };
                        crate::edit::preserve_space_if_needed(child, wanted);
                        child.set_text(wanted);
                    }
                }
                *offset += length;
                index += 1;
            }
            "tab" | "br" | "cr" => {
                *offset += 1;
                index += 1;
            }
            "instrText" => index += 1,
            _ => {
                if let Some(child) = element.children[index].as_element_mut() {
                    write_result_within(child, field, offset, writing, done, text);
                }
                index += 1;
            }
        }
    }
}

/// A field whose begin marker has been seen but whose answer has not ended.
#[derive(Clone, Debug)]
struct Opening {
    name: String,
    kind: FormKind,
    enabled: bool,
    items: Vec<String>,
    checked: bool,
    chosen: usize,
    /// Where the answer started, once the `separate` marker has been passed.
    start: Option<TextPosition>,
}

/// Walks a paragraph, counting characters and matching up the markers.
fn walk(
    element: &Element,
    paragraph: usize,
    offset: &mut usize,
    open: &mut Vec<Option<Opening>>,
    found: &mut Vec<FormField>,
) {
    for node in &element.children {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(read::W) {
            continue;
        }

        match child.local_name() {
            "fldChar" => {
                let word = child.attribute(Some(read::W), "fldCharType").unwrap_or("begin");
                match crate::fields::Marker::from_word(word) {
                    // Only a begin carrying `w:ffData` opens a form field.
                    // Every other field in the document is pushed as nothing
                    // so that the markers still pair up.
                    Some(crate::fields::Marker::Begin) => {
                        open.push(child.child(Some(read::W), "ffData").map(opening));
                    }
                    Some(crate::fields::Marker::Separate) => {
                        if let Some(Some(field)) = open.last_mut() {
                            field.start = Some(TextPosition::new(paragraph, *offset));
                        }
                    }
                    Some(crate::fields::Marker::End) => {
                        if let Some(Some(field)) = open.pop() {
                            // A field with no `separate` marker has no answer
                            // and nothing to type into.
                            if let Some(start) = field.start {
                                found.push(FormField {
                                    name: field.name,
                                    kind: field.kind,
                                    enabled: field.enabled,
                                    start,
                                    end: TextPosition::new(paragraph, *offset),
                                    items: field.items,
                                    checked: field.checked,
                                    chosen: field.chosen,
                                });
                            }
                        }
                    }
                    None => {}
                }
            }
            "t" => *offset += child.text_content().len(),
            "tab" | "br" | "cr" => *offset += 1,
            // The instruction is not part of the text and is not counted,
            // which is what every other reader of this document does too.
            "instrText" => {}
            _ => walk(child, paragraph, offset, open, found),
        }
    }
}

/// Reads a `w:ffData`.
fn opening(data: &Element) -> Opening {
    let said = |local: &str| {
        data.child(Some(read::W), local)
            .and_then(|child| child.attribute(Some(read::W), "val"))
            .unwrap_or_default()
            .to_owned()
    };
    let items = data
        .child(Some(read::W), "ddList")
        .map(|list| {
            list.child_elements()
                .filter(|child| child.is(Some(read::W), "listEntry"))
                .map(|child| child.attribute(Some(read::W), "val").unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default();
    Opening {
        name: said("name"),
        checked: data
            .child(Some(read::W), "checkBox")
            .and_then(|box_of| box_of.child(Some(read::W), "checked"))
            .is_some_and(read::on_off),
        chosen: data
            .child(Some(read::W), "ddList")
            .and_then(|list| list.child(Some(read::W), "result"))
            .and_then(|result| result.attribute(Some(read::W), "val"))
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        kind: FormKind::of(data),
        // An absent `w:enabled` means enabled: the schema's default is on,
        // and a form whose fields were all dead by default would be absurd.
        enabled: data.child(Some(read::W), "enabled").is_none_or(read::on_off),
        items,
        start: None,
    }
}

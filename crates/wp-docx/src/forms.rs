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
    #[must_use]
    pub fn form_field_at(&self, position: TextPosition) -> Option<FormField> {
        self.form_fields().into_iter().find(|field| field.covers(position))
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

/// A field whose begin marker has been seen but whose answer has not ended.
#[derive(Clone, Debug)]
struct Opening {
    name: String,
    kind: FormKind,
    enabled: bool,
    items: Vec<String>,
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
        kind: FormKind::of(data),
        // An absent `w:enabled` means enabled: the schema's default is on,
        // and a form whose fields were all dead by default would be absurd.
        enabled: data.child(Some(read::W), "enabled").is_none_or(read::on_off),
        items,
        start: None,
    }
}

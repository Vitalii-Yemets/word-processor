//! Footnotes and endnotes.
//!
//! # What a note is made of
//!
//! The same three pieces a comment has, arranged differently:
//!
//! * `word/footnotes.xml` holds the text of every note, each with a number.
//! * A run in the document holds `w:footnoteReference`, which is the little
//!   raised number a reader sees.
//! * `w:sectPr` says nothing at all — a note belongs to the text, not the page.
//!   Which page it is printed at the bottom of is worked out when the document
//!   is laid out, not stored.
//!
//! # The two notes that are not notes
//!
//! Every footnotes part starts with two entries that are not footnotes: number
//! −1 is the separator rule drawn above the notes, and number 0 is the one used
//! when a note carries over onto the next page. Word writes them, expects them,
//! and numbers real notes from 1. Leaving them out produces a file Word opens
//! and then quietly repairs.

use wp_opc::TargetMode;
use wp_xml::tree::{Element, Node, XmlTree};

use crate::history::EditKind;
use crate::model::{Paragraph, Run, RunContent, RunProperties, VerticalAlignment};
use crate::{edit, position, read, Document, Error, TextPosition};

/// Which of the two kinds a call is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Printed at the bottom of the page the mark is on.
    Footnote,
    /// Collected at the end of the document.
    Endnote,
}

impl Kind {
    #[must_use]
    fn content_type(self) -> &'static str {
        match self {
            Self::Footnote => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"
            }
            Self::Endnote => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"
            }
        }
    }

    #[must_use]
    fn relationship(self) -> &'static str {
        match self {
            Self::Footnote => {
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes"
            }
            Self::Endnote => {
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes"
            }
        }
    }

    /// The root element of the part.
    #[must_use]
    fn root(self) -> &'static str {
        match self {
            Self::Footnote => "footnotes",
            Self::Endnote => "endnotes",
        }
    }

    /// One entry in it.
    #[must_use]
    fn entry(self) -> &'static str {
        match self {
            Self::Footnote => "footnote",
            Self::Endnote => "endnote",
        }
    }

    #[must_use]
    fn part(self) -> &'static str {
        match self {
            Self::Footnote => "word/footnotes.xml",
            Self::Endnote => "word/endnotes.xml",
        }
    }

    #[must_use]
    pub fn is_endnote(self) -> bool {
        self == Self::Endnote
    }
}

/// One note, and where its mark sits in the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: i32,
    pub kind: Kind,
    /// What the note says.
    pub text: String,
    /// Where the mark that points at it is, when the document still has one.
    pub mark: Option<TextPosition>,
    /// What number the reader sees, counting the marks in reading order.
    pub number: usize,
}

impl Document {
    /// Every note of one kind, in the order their marks appear.
    #[must_use]
    pub fn notes(&self, kind: Kind) -> Vec<Note> {
        let Some(root) = self.notes_root(kind) else { return Vec::new() };
        let marks = self.note_marks(kind);

        let mut out: Vec<Note> = root
            .children_named(Some(read::W), kind.entry())
            .filter_map(|element| {
                let id: i32 = element.attribute(Some(read::W), "id")?.parse().ok()?;
                // The separator and the continuation separator are not notes.
                if id < 1 {
                    return None;
                }
                Some(Note {
                    id,
                    kind,
                    text: read::read_part(element).plain_text().trim().to_owned(),
                    mark: marks.get(&id).copied(),
                    number: 0,
                })
            })
            .collect();

        out.sort_by_key(|note| {
            note.mark.map_or((usize::MAX, usize::MAX), |at| (at.paragraph, at.offset))
        });
        // The number a reader sees is the position in reading order, not the id
        // — a document edited for years has notes numbered every which way.
        for (position, note) in out.iter_mut().enumerate() {
            note.number = position + 1;
        }
        out
    }

    /// Adds a note at the caret and returns the number it was given.
    pub fn add_note(&mut self, kind: Kind, text: &str) -> Result<i32, Error> {
        let caret = self.caret();
        // Putting the mark in and raising it are two changes to the tree and
        // one act to a person, so they are one step to undo.
        self.begin_gesture();
        self.record(EditKind::Structural, caret, false);

        let id = self.notes(kind).iter().map(|note| note.id).max().unwrap_or(0) + 1;
        self.write_note(kind, id, text)?;

        // The mark goes into the text as one character, the way a tab or a
        // picture does — so the caret can stand either side of it and
        // Backspace can take it away.
        let prefix = self.prefix();
        let local = if kind.is_endnote() { "endnoteReference" } else { "footnoteReference" };
        let mut element = Element::new(&edit::name_with(prefix.as_deref(), local), Some(read::W));
        element.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "id"),
            read::W,
            &id.to_string(),
        );

        if !position::insert_element_at(&mut self.tree.root, caret, element, prefix.as_deref()) {
            self.end_gesture();
            return Ok(id);
        }

        // Raised, the way Word raises it. Applied to the one character it
        // occupies rather than written onto the run, because the run it landed
        // in may hold text either side of it.
        let after = TextPosition::new(caret.paragraph, caret.offset + 1);
        self.set_caret(caret);
        self.extend_selection_to(after);
        self.set_format(crate::CharacterFormat::Superscript, true);
        self.clear_selection();

        self.set_caret(after);
        self.mark_modified();
        self.end_gesture();
        Ok(id)
    }

    /// Takes a note out, mark and all.
    pub fn delete_note(&mut self, kind: Kind, id: i32) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let mut changed = false;
        for index in 0..self.paragraph_count() {
            let Some(path) = position::paragraph_path(&self.tree().root, index) else { continue };
            let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path)
            else {
                continue;
            };
            changed |= remove_marks(paragraph, kind, id);
        }

        if let Some(mut root) = self.notes_root(kind) {
            let wanted = id.to_string();
            let before = root.children.len();
            root.children.retain(|node| match node {
                Node::Element(element) => {
                    !(element.is(Some(read::W), kind.entry())
                        && element.attribute(Some(read::W), "id") == Some(wanted.as_str()))
                }
                _ => true,
            });
            if root.children.len() != before {
                changed |= self.save_notes_root(kind, root).is_ok();
            }
        }

        if changed {
            self.mark_modified();
        }
        changed
    }

    /// Where each note's mark sits in the text.
    fn note_marks(&self, kind: Kind) -> std::collections::HashMap<i32, TextPosition> {
        let mut found = std::collections::HashMap::new();
        for (index, paragraph) in self.paragraph_elements().into_iter().enumerate() {
            let mut offset = 0usize;
            walk_marks(paragraph, kind, &mut offset, &mut found, index);
        }
        found
    }

    /// The part holding the notes of one kind, if there is one.
    #[must_use]
    pub fn notes_part(&self, kind: Kind) -> Option<String> {
        let relationships = self.package().relationships(self.main_part()).ok()?;
        let relationship = relationships.single_by_type(kind.relationship())?;
        relationship.resolved_target(self.main_part())?.ok()
    }

    /// The root element of that part, read afresh.
    fn notes_root(&self, kind: Kind) -> Option<Element> {
        let part = self.notes_part(kind)?;
        let text = self.package().xml_part(&part)?.ok()?;
        XmlTree::parse(&text).ok().map(|tree| tree.root)
    }

    /// Adds one note to the part, making the part if there is not one.
    fn write_note(&mut self, kind: Kind, id: i32, text: &str) -> Result<(), Error> {
        let mut root = match self.notes_root(kind) {
            Some(root) => root,
            None => fresh_part(kind),
        };

        let mut note = Element::new(&format!("w:{}", kind.entry()), Some(read::W));
        note.set_namespaced_attribute("w:id", read::W, &id.to_string());

        // The note starts with the same raised number the text carries, which
        // is what makes it look like a footnote rather than a stray paragraph.
        for (number, line) in text.split('\n').enumerate() {
            let mut paragraph = Paragraph::text(line);
            if number == 0 {
                paragraph.runs.insert(
                    0,
                    Run {
                        properties: RunProperties {
                            vertical_align: Some(VerticalAlignment::Superscript),
                            ..RunProperties::default()
                        },
                        content: vec![RunContent::NoteReference { id, endnote: kind.is_endnote() }],
                        field: None,
                        revision: None,
                        format_change: None,
                    },
                );
                paragraph.runs.insert(1, Run::text(" "));
            }
            note.push_element(edit::paragraph_element(&paragraph, Some("w")));
        }
        root.push_element(note);

        self.save_notes_root(kind, root)
    }

    /// Writes the part back, adding the relationship the first time.
    fn save_notes_root(&mut self, kind: Kind, root: Element) -> Result<(), Error> {
        let tree = XmlTree {
            standalone: Some(true),
            has_declaration: true,
            doctype: None,
            before_root: Vec::new(),
            root,
            after_root: Vec::new(),
        };
        let xml =
            tree.to_xml().map_err(|source| Error::Xml { part: kind.part().to_owned(), source })?;

        let part = self.notes_part(kind).unwrap_or_else(|| kind.part().to_owned());
        self.package_mut().add_part(&part, kind.content_type(), xml.into_bytes());

        let main_part = self.main_part().to_owned();
        let mut relationships = self
            .package()
            .relationships(&main_part)
            .unwrap_or_else(|_| wp_opc::Relationships::new(&main_part));
        if relationships.single_by_type(kind.relationship()).is_none() {
            let target = part.strip_prefix("word/").unwrap_or(&part).to_owned();
            relationships.add(kind.relationship(), &target, TargetMode::Internal);
            self.package_mut().set_relationships(&relationships)?;
        }
        Ok(())
    }
}

/// A notes part with the two entries every one of them has to start with.
fn fresh_part(kind: Kind) -> Element {
    let mut root = Element::new(&format!("w:{}", kind.root()), Some(read::W));
    root.declarations.push((Some("w".to_owned()), read::W.to_owned()));

    for (id, sort) in [(-1, "separator"), (0, "continuationSeparator")] {
        let mut entry = Element::new(&format!("w:{}", kind.entry()), Some(read::W));
        entry.set_namespaced_attribute("w:type", read::W, sort);
        entry.set_namespaced_attribute("w:id", read::W, &id.to_string());

        let mut paragraph = Element::new("w:p", Some(read::W));
        let mut run = Element::new("w:r", Some(read::W));
        run.push_element(Element::new(
            &format!(
                "w:{}",
                if sort == "separator" { "separator" } else { "continuationSeparator" }
            ),
            Some(read::W),
        ));
        paragraph.push_element(run);
        entry.push_element(paragraph);
        root.push_element(entry);
    }
    root
}

/// Finds every note mark in a paragraph, with the offset it sits at.
fn walk_marks(
    element: &Element,
    kind: Kind,
    offset: &mut usize,
    found: &mut std::collections::HashMap<i32, TextPosition>,
    paragraph: usize,
) {
    for node in &element.children {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(read::W) || child.local_name() == "del" {
            continue;
        }

        let wanted = if kind.is_endnote() { "endnoteReference" } else { "footnoteReference" };
        if child.local_name() == wanted {
            if let Some(id) =
                child.attribute(Some(read::W), "id").and_then(|text| text.parse().ok())
            {
                found.insert(id, TextPosition::new(paragraph, *offset));
            }
            *offset += 1;
            continue;
        }
        if child.local_name() == "t" {
            *offset += child.text_content().len();
        } else if let Some(text) = edit::atomic_text(child) {
            *offset += text.len();
        } else {
            walk_marks(child, kind, offset, found, paragraph);
        }
    }
}

/// Takes one note's mark out of a paragraph.
///
/// The mark sits inside whatever run it was typed into, so it is the element
/// that goes rather than the run — and the run goes only if the mark was all it
/// held.
fn remove_marks(paragraph: &mut Element, kind: Kind, id: i32) -> bool {
    let wanted = id.to_string();
    let local = if kind.is_endnote() { "endnoteReference" } else { "footnoteReference" };
    let mut removed = false;

    for node in &mut paragraph.children {
        let Some(run) = node.as_element_mut() else { continue };
        let before = run.children.len();
        run.children.retain(|child| {
            let Node::Element(child) = child else { return true };
            !(child.is(Some(read::W), local)
                && child.attribute(Some(read::W), "id") == Some(wanted.as_str()))
        });
        removed |= run.children.len() != before;
    }
    if !removed {
        return false;
    }

    // A run left holding nothing but its properties draws nothing and is only
    // clutter in the file.
    paragraph.children.retain(|node| {
        let Node::Element(run) = node else { return true };
        if !run.is(Some(read::W), "r") {
            return true;
        }
        run.child_elements().any(|child| child.local_name() != "rPr")
    });
    true
}

impl Document {
    /// The body of one note, for something that needs to lay it out.
    #[must_use]
    pub fn note_body(&self, kind: Kind, id: i32) -> Option<crate::model::Body> {
        let root = self.notes_root(kind)?;
        let wanted = id.to_string();
        let entry = root
            .children_named(Some(read::W), kind.entry())
            .find(|element| element.attribute(Some(read::W), "id") == Some(wanted.as_str()))?;
        Some(read::read_part(entry))
    }
}

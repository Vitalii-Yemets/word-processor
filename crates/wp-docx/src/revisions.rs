//! Tracked changes: recording them, accepting them and rejecting them.
//!
//! # What accepting and rejecting actually are
//!
//! Four operations, and it is worth writing them down because three of them are
//! easy to get backwards:
//!
//! | | accept | reject |
//! |---|---|---|
//! | `w:ins` | unwrap it, the text stays | remove it, text and all |
//! | `w:del` | remove it, text and all | unwrap it, `w:delText` becomes `w:t` |
//!
//! Accepting an insertion and rejecting a deletion are the same move: take the
//! wrapper off and keep the words. Rejecting an insertion and accepting a
//! deletion are the same move too: take the whole thing out.

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::model::RevisionKind;
use crate::{edit, read, Document};

/// Whether the recorded changes are being kept or thrown away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Accept,
    Reject,
}

/// Who made a change and when, for the changes this program records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reviser {
    pub author: String,
    /// An ISO 8601 timestamp. Supplied rather than worked out here so that a
    /// test can pin it and two runs of the same edit produce the same file.
    pub date: String,
}

impl Default for Reviser {
    fn default() -> Self {
        Self { author: "Author".to_owned(), date: "1970-01-01T00:00:00Z".to_owned() }
    }
}

impl Document {
    /// Whether the document asks for changes to be recorded.
    ///
    /// Answered from what was read when the document was opened, because this
    /// is asked on every keystroke.
    #[must_use]
    pub fn tracking_changes(&self) -> bool {
        self.tracking
    }

    /// Whose name goes on the changes this program records.
    pub fn set_reviser(&mut self, reviser: Reviser) {
        self.reviser = reviser;
    }

    /// Reads the setting out of the package, which is done once on opening.
    pub(crate) fn read_tracking_setting(&self) -> bool {
        let Some(settings) = self.settings_root() else { return false };
        settings
            .child(Some(read::W), "trackChanges")
            .is_some_and(|element| element.attribute(Some(read::W), "val") != Some("0"))
    }

    /// Turns the recording of changes on or off.
    pub fn set_tracking_changes(&mut self, on: bool) -> bool {
        if !self.set_setting_flag("trackChanges", on) {
            return false;
        }
        self.tracking = on;
        true
    }

    /// How many tracked changes the document holds.
    #[must_use]
    pub fn revision_count(&self) -> usize {
        let mut count = 0usize;
        count_revisions(&self.tree().root, &mut count);
        count
    }

    /// Accepts or rejects every tracked change in the document.
    pub fn resolve_all_revisions(&mut self, decision: Decision) -> usize {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let mut resolved = 0usize;
        resolve_within(&mut self.tree_mut().root, decision, None, prefix.as_deref(), &mut resolved);
        if resolved > 0 {
            self.clamp_caret();
            self.mark_modified();
        }
        resolved
    }

    /// Accepts or rejects the tracked changes touching the caret's paragraph.
    ///
    /// Word works on the change the caret is in, or the next one; working on
    /// the paragraph is close enough to be useful and simple enough to be
    /// right.
    pub fn resolve_revisions_here(&mut self, decision: Decision) -> usize {
        let Some(path) = crate::position::paragraph_path(&self.tree().root, self.caret().paragraph)
        else {
            return 0;
        };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let mut resolved = 0usize;
        if let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) {
            resolve_within(paragraph, decision, None, prefix.as_deref(), &mut resolved);
        }
        if resolved > 0 {
            self.clamp_caret();
            self.mark_modified();
        }
        resolved
    }

    /// Puts the caret somewhere that still exists.
    pub(crate) fn clamp_caret(&mut self) {
        let count = self.paragraph_count().max(1);
        let caret = self.caret();
        let paragraph = caret.paragraph.min(count - 1);
        let length = self.paragraph_text(paragraph).map_or(0, |text| text.len());
        self.set_caret(crate::TextPosition::new(paragraph, caret.offset.min(length)));
    }
}

/// Counts every `w:ins` and `w:del` under an element.
fn count_revisions(element: &Element, count: &mut usize) {
    for child in element.child_elements() {
        if child.namespace.as_deref() == Some(read::W)
            && matches!(child.local_name(), "ins" | "del")
        {
            *count += 1;
        }
        count_revisions(child, count);
    }
}

/// Carries out a decision on every tracked change under an element.
fn resolve_within(
    element: &mut Element,
    decision: Decision,
    _parent: Option<RevisionKind>,
    prefix: Option<&str>,
    resolved: &mut usize,
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

        let kind = match child.local_name() {
            "ins" => Some(RevisionKind::Inserted),
            "del" => Some(RevisionKind::Deleted),
            _ => None,
        };

        let Some(kind) = kind else {
            if let Some(child) = element.children[index].as_element_mut() {
                resolve_within(child, decision, None, prefix, resolved);
            }
            index += 1;
            continue;
        };

        *resolved += 1;
        let keep = matches!(
            (kind, decision),
            (RevisionKind::Inserted, Decision::Accept) | (RevisionKind::Deleted, Decision::Reject)
        );

        if !keep {
            element.children.remove(index);
            continue;
        }

        // Kept: the wrapper comes off and its runs take its place. Deleted text
        // that is being kept was written as `w:delText` and has to become
        // ordinary text again.
        let Node::Element(wrapper) = element.children.remove(index) else { continue };
        let mut inner = wrapper.children;
        if kind == RevisionKind::Deleted {
            for node in &mut inner {
                if let Some(run) = node.as_element_mut() {
                    undelete_text(run, prefix);
                }
            }
        }
        let count = inner.len();
        for (offset, node) in inner.into_iter().enumerate() {
            element.children.insert(index + offset, node);
        }
        index += count;
    }
}

/// Turns `w:delText` back into `w:t` throughout a run.
fn undelete_text(element: &mut Element, prefix: Option<&str>) {
    if element.namespace.as_deref() == Some(read::W) && element.local_name() == "delText" {
        element.name = edit::name_with(prefix, "t");
        return;
    }
    for node in &mut element.children {
        if let Some(child) = node.as_element_mut() {
            undelete_text(child, prefix);
        }
    }
}

impl Document {
    /// Types text in, recording it as an insertion somebody made.
    ///
    /// The text goes into a run of its own inside a `w:ins`, which is why the
    /// runs either side have to be cut apart first: a wrapper cannot start in
    /// the middle of a run.
    pub(crate) fn insert_tracked(
        &mut self,
        at: crate::TextPosition,
        text: &str,
        reviser: &Reviser,
    ) -> bool {
        if text.is_empty() {
            return false;
        }
        let prefix = self.prefix();
        let id = self.next_revision_id();
        let Some(path) = crate::position::paragraph_path(&self.tree().root, at.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };

        crate::format::split_runs_at_offset(paragraph, at.offset);
        let position = edit::child_position_at_offset(paragraph, at.offset);

        // The new text takes the formatting of whatever it is being typed into,
        // which is what happens when tracking is off as well.
        let inherited = paragraph
            .children
            .iter()
            .take(position)
            .filter_map(|node| node.as_element())
            .rev()
            .find(|child| child.is(Some(read::W), "r"))
            .and_then(|run| run.child(Some(read::W), "rPr"))
            .cloned();

        let mut run = Element::new(&edit::name_with(prefix.as_deref(), "r"), Some(read::W));
        if let Some(properties) = inherited {
            run.push_element(properties);
        }
        let mut node = Element::new(&edit::name_with(prefix.as_deref(), "t"), Some(read::W));
        node.set_text(text);
        node.set_namespaced_attribute("xml:space", edit::XML_NAMESPACE, "preserve");
        run.push_element(node);

        paragraph.insert_element(
            position,
            wrap(run, RevisionKind::Inserted, id, reviser, prefix.as_deref()),
        );
        self.mark_modified();
        true
    }

    /// Marks a stretch of one paragraph as deleted rather than removing it.
    pub(crate) fn delete_tracked(
        &mut self,
        paragraph_index: usize,
        start: usize,
        end: usize,
        reviser: &Reviser,
    ) -> bool {
        if end <= start {
            return false;
        }
        let prefix = self.prefix();
        let id = self.next_revision_id();
        let Some(path) = crate::position::paragraph_path(&self.tree().root, paragraph_index) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };

        // Cut at both ends, so every run is now wholly inside the range or
        // wholly outside it.
        crate::format::split_runs_at_offset(paragraph, start);
        crate::format::split_runs_at_offset(paragraph, end);

        // Which children fall inside, worked out before anything moves.
        let mut inside = Vec::new();
        let mut offset = 0usize;
        for (index, node) in paragraph.children.iter().enumerate() {
            let Some(child) = node.as_element() else { continue };
            if child.namespace.as_deref() != Some(read::W) || child.local_name() == "del" {
                continue;
            }
            let length = edit::measured_length(child);
            if length > 0 && offset >= start && offset + length <= end {
                inside.push(index);
            }
            offset += length;
        }
        if inside.is_empty() {
            return false;
        }

        // Taken out from the back so the earlier indices stay valid, then put
        // back in order inside one wrapper.
        let mut taken = Vec::new();
        for index in inside.iter().rev() {
            taken.push(paragraph.children.remove(*index));
        }
        taken.reverse();

        let at = inside[0];
        let mut wrapper = Element::new(&edit::name_with(prefix.as_deref(), "del"), Some(read::W));
        stamp(&mut wrapper, id, reviser, prefix.as_deref());
        for node in taken {
            if let Node::Element(mut run) = node {
                rename_text_to_deleted(&mut run, prefix.as_deref());
                wrapper.push_element(run);
            }
        }
        paragraph.insert_element(at, wrapper);

        self.mark_modified();
        true
    }

    /// A number no tracked change in the document is using.
    fn next_revision_id(&self) -> i32 {
        let mut highest = 0i32;
        highest_revision_id(&self.tree().root, &mut highest);
        highest + 1
    }
}

/// Puts a run inside a `w:ins` or a `w:del`.
fn wrap(
    run: Element,
    kind: RevisionKind,
    id: i32,
    reviser: &Reviser,
    prefix: Option<&str>,
) -> Element {
    let mut wrapper = Element::new(&edit::name_with(prefix, kind.element()), Some(read::W));
    stamp(&mut wrapper, id, reviser, prefix);
    wrapper.push_element(run);
    wrapper
}

/// Writes who made a change and when onto its wrapper.
fn stamp(wrapper: &mut Element, id: i32, reviser: &Reviser, prefix: Option<&str>) {
    wrapper.set_namespaced_attribute(&edit::name_with(prefix, "id"), read::W, &id.to_string());
    wrapper.set_namespaced_attribute(&edit::name_with(prefix, "author"), read::W, &reviser.author);
    wrapper.set_namespaced_attribute(&edit::name_with(prefix, "date"), read::W, &reviser.date);
}

/// Turns `w:t` into `w:delText` throughout a run being marked as deleted.
fn rename_text_to_deleted(element: &mut Element, prefix: Option<&str>) {
    if element.namespace.as_deref() == Some(read::W) && element.local_name() == "t" {
        element.name = edit::name_with(prefix, "delText");
        return;
    }
    for node in &mut element.children {
        if let Some(child) = node.as_element_mut() {
            rename_text_to_deleted(child, prefix);
        }
    }
}

/// The largest revision number anywhere under an element.
fn highest_revision_id(element: &Element, highest: &mut i32) {
    for child in element.child_elements() {
        if child.namespace.as_deref() == Some(read::W)
            && matches!(child.local_name(), "ins" | "del")
        {
            if let Some(id) =
                child.attribute(Some(read::W), "id").and_then(|text| text.parse::<i32>().ok())
            {
                *highest = (*highest).max(id);
            }
        }
        highest_revision_id(child, highest);
    }
}

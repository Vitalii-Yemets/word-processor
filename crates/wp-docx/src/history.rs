//! Undo and redo.
//!
//! # Why whole snapshots
//!
//! The alternative is to record an inverse for every operation — a deletion for
//! each insertion, and so on. That is smaller, and it is also the design where a
//! single missed case silently corrupts a document three undos later, because
//! the inverse was very slightly wrong. Keeping a copy of the tree cannot be
//! wrong: what comes back is exactly what was there.
//!
//! It costs memory, so the history is bounded. A document large enough for that
//! to matter is a document where correctness matters more.
//!
//! # Why steps are merged
//!
//! One undo per keystroke is not undo, it is a typing replay. Consecutive
//! typing is therefore folded into one step, and the fold is broken at a space
//! — so undo takes back a word at a time, which is what a person means by it.

use wp_xml::tree::XmlTree;

use crate::position::TextPosition;

/// What sort of change was made, so like changes can be merged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Text was typed in.
    Typing,
    /// Text was removed a character at a time.
    Deleting,
    /// Anything that changes the shape of the document, which is never merged.
    Structural,
}

/// One recoverable state of the document.
#[derive(Clone, Debug)]
struct Step {
    tree: XmlTree,
    caret: TextPosition,
    modified: bool,
    kind: EditKind,
    /// Which part of the package the tree belongs to.
    ///
    /// A header lives in a part of its own, and editing one swaps which part
    /// is loaded. Undo has to put the right tree back into the right part —
    /// and switch to that part first, which is what Word does when undo
    /// reaches back past the moment a header was opened.
    part: String,
    /// Where the change ended, so the next one can be tested for contiguity.
    ends_at: TextPosition,
}

/// How many steps are kept.
///
/// Far more than anyone undoes in practice, and bounded so that a long session
/// on a large document does not grow without limit.
const DEFAULT_LIMIT: usize = 200;

/// The undo and redo stacks.
#[derive(Clone, Debug)]
pub(crate) struct History {
    past: Vec<Step>,
    future: Vec<Step>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self { past: Vec::new(), future: Vec::new(), limit: DEFAULT_LIMIT }
    }
}

impl History {
    pub(crate) fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub(crate) fn depth(&self) -> usize {
        self.past.len()
    }

    /// Records the state before a change.
    ///
    /// `caret` is where the caret was before it, and `ends_at` where the change
    /// will leave it; together they decide whether this change continues the
    /// last one or starts a new step.
    #[allow(clippy::too_many_arguments, reason = "a step is a tree, a place, a kind and a part")]
    pub(crate) fn record(
        &mut self,
        tree: &XmlTree,
        part: &str,
        caret: TextPosition,
        modified: bool,
        kind: EditKind,
        ends_at: TextPosition,
        mergeable: bool,
    ) {
        // Anything new makes the redo stack meaningless: it described a future
        // that no longer follows from the present.
        self.future.clear();

        if mergeable {
            if let Some(last) = self.past.last_mut() {
                // A continuation of the same kind of change, starting exactly
                // where the last one ended, is part of the same step — and in
                // the same part of the package, or it is not a continuation of
                // anything.
                if last.kind == kind && last.ends_at == caret && last.part == part {
                    last.ends_at = ends_at;
                    return;
                }
            }
        }

        self.past.push(Step {
            tree: tree.clone(),
            part: part.to_owned(),
            caret,
            modified,
            kind,
            ends_at,
        });
        if self.past.len() > self.limit {
            self.past.remove(0);
        }
    }

    /// Steps back, given the current state to keep for redo.
    ///
    /// What comes back is the tree, which part of the package it belongs to,
    /// where the caret was and whether the document had been changed.
    pub(crate) fn undo(
        &mut self,
        current_tree: &XmlTree,
        current_part: &str,
        current_caret: TextPosition,
        current_modified: bool,
    ) -> Option<(XmlTree, String, TextPosition, bool)> {
        let step = self.past.pop()?;
        self.future.push(Step {
            tree: current_tree.clone(),
            part: current_part.to_owned(),
            caret: current_caret,
            modified: current_modified,
            kind: step.kind,
            ends_at: step.ends_at,
        });
        Some((step.tree, step.part, step.caret, step.modified))
    }

    /// Steps forward again.
    pub(crate) fn redo(
        &mut self,
        current_tree: &XmlTree,
        current_part: &str,
        current_caret: TextPosition,
        current_modified: bool,
    ) -> Option<(XmlTree, String, TextPosition, bool)> {
        let step = self.future.pop()?;
        self.past.push(Step {
            tree: current_tree.clone(),
            part: current_part.to_owned(),
            caret: current_caret,
            modified: current_modified,
            kind: step.kind,
            ends_at: step.ends_at,
        });
        Some((step.tree, step.part, step.caret, step.modified))
    }

    /// Ends the current step, so the next change starts a new one.
    ///
    /// Called when something happens that is not itself an edit — the caret is
    /// moved, the document is saved — because typing after that is a new
    /// thought, not a continuation.
    pub(crate) fn break_merge(&mut self) {
        if let Some(last) = self.past.last_mut() {
            // An impossible position, so nothing can be contiguous with it.
            last.ends_at = TextPosition::new(usize::MAX, usize::MAX);
        }
    }
}

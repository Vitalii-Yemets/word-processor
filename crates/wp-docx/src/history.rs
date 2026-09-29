//! Undo and redo.
//!
//! # Why states rather than inverses
//!
//! The alternative is to record an inverse for every operation — a deletion for
//! each insertion, and so on. That is smaller, and it is also the design where a
//! single missed case silently corrupts a document three undos later, because
//! the inverse was very slightly wrong. Keeping a copy of what was there cannot
//! be wrong: what comes back is exactly what was there.
//!
//! # Why not always the whole document
//!
//! Because a copy of the whole element tree is fourteen megabytes on a
//! thousand-page document, and a person typing makes a step every word. That is
//! a gigabyte in a minute of typing, which is not a price worth paying for a
//! guarantee that a smaller copy gives just as well.
//!
//! Typing and deleting change one paragraph and nothing else. So those steps
//! keep that one paragraph, exactly as it was, and put it back where it was.
//! It is the same guarantee — a state, not an inverse — for a thousandth of the
//! memory. Anything that changes the shape of the document keeps the whole
//! tree, because anything is where it might have changed.
//!
//! # Why steps are merged
//!
//! One undo per keystroke is not undo, it is a typing replay. Consecutive
//! typing is therefore folded into one step, and the fold is broken at a space
//! — so undo takes back a word at a time, which is what a person means by it.
//!
//! # Why "saved" is a place in the history and not a flag
//!
//! Whether a document has changed is a question about two states: the one on
//! the screen and the one on disk. A flag kept with each step answers it for
//! the save that was current when the step was made, and a save moves that
//! answer without touching the steps: undo straight after saving put back
//! "unchanged" beside a tree that was no longer the saved one, and the next
//! save wrote the old file. So every state the history can come back to has a
//! number, the state on disk is one of those numbers, and the document has
//! changed exactly when the present number is not that one — whichever way
//! the history went to get there.

use wp_xml::tree::{Element, XmlTree};

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

/// What a step keeps of the document, so that it can be put back.
#[derive(Clone, Debug)]
pub(crate) enum Kept {
    /// The whole tree, for a change that could have touched any of it.
    Whole(XmlTree),
    /// One paragraph as it was, and where it sat.
    Paragraph { index: usize, element: Box<Element> },
    /// The whole tree and some parts of the package as they were, for a
    /// change that lives in the parts as much as in the tree: a diagram's
    /// words are in a part of their own, and undoing an edit to them has to
    /// put that part back.
    WithParts { tree: XmlTree, parts: Vec<(String, Vec<u8>)> },
}

/// One recoverable state of the document.
#[derive(Clone, Debug)]
struct Step {
    kept: Kept,
    caret: TextPosition,
    /// The number of the state this is, so that coming back to it is coming
    /// back to its number — which is what says whether it is the one on disk.
    revision: u64,
    kind: EditKind,
    /// Which part of the package the state belongs to.
    ///
    /// A header lives in a part of its own, and editing one swaps which part
    /// is loaded. Undo has to put the right state back into the right part —
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
    /// The number of the state the document is in.
    revision: u64,
    /// The last number given out. None is given out twice, so two states
    /// with the same number are the same state.
    issued: u64,
    /// The number of the state that is on disk.
    saved: u64,
}

impl Default for History {
    fn default() -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            limit: DEFAULT_LIMIT,
            revision: 0,
            issued: 0,
            saved: 0,
        }
    }
}

impl History {
    /// Gives the present state a number of its own, because it has changed
    /// since it was last given one.
    pub(crate) fn advance(&mut self) {
        self.issued += 1;
        self.revision = self.issued;
    }

    /// Says the present state is the one on disk.
    pub(crate) fn mark_saved(&mut self) {
        self.saved = self.revision;
    }

    /// Whether the present state is the one on disk.
    pub(crate) fn is_at_saved(&self) -> bool {
        self.revision == self.saved
    }

    /// Says the state on disk can no longer be come back to.
    ///
    /// For a change no step records. Undo takes back what the history kept,
    /// and this it did not keep, so whatever state undo reaches still has
    /// the change in it — and a state with a change in it that the file has
    /// not got is not the file, whatever number it once had. The saved number
    /// becomes one no state is ever given (numbers count up from nothing and
    /// never reach it) until the next save names another.
    pub(crate) fn lose_saved(&mut self) {
        self.saved = u64::MAX;
    }

    /// The parts of the package the last step keeps besides the tree, whose
    /// writes that step can therefore put back.
    pub(crate) fn parts_kept_by_last_step(&self) -> Vec<&str> {
        match self.past.last().map(|step| &step.kept) {
            Some(Kept::WithParts { parts, .. }) => {
                parts.iter().map(|(name, _)| name.as_str()).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Forgets every step, and keeps the numbers.
    ///
    /// What is forgotten is the way back, not where the document stands: a
    /// document that was not the saved one before is still not the saved one
    /// after, and a number handed out again later would make two different
    /// states look alike.
    pub(crate) fn forget(&mut self) {
        self.past.clear();
        self.future.clear();
    }

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
    /// last one or starts a new step. The state kept is the present one, so it
    /// is kept under the present number, which the caller has brought up to
    /// date with [`Self::advance`] if the state had changed since.
    pub(crate) fn record(
        &mut self,
        kept: Kept,
        part: &str,
        caret: TextPosition,
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
            kept,
            part: part.to_owned(),
            caret,
            revision: self.revision,
            kind,
            ends_at,
        });
        if self.past.len() > self.limit {
            self.past.remove(0);
        }
    }

    /// What the next step back keeps, and the part of the package it was
    /// kept from, so the caller can go to that part and keep the same of it
    /// for redo.
    pub(crate) fn next_undo(&self) -> Option<(&Kept, &str)> {
        self.past.last().map(|step| (&step.kept, step.part.as_str()))
    }

    /// The same, for a step forward.
    pub(crate) fn next_redo(&self) -> Option<(&Kept, &str)> {
        self.future.last().map(|step| (&step.kept, step.part.as_str()))
    }

    /// Steps back, given the present state to keep for redo.
    ///
    /// The present state is the step's own part, which the caller has gone to
    /// already, and the present number is its number; what comes back is the
    /// state to put back and where the caret was, and the present number is
    /// then the step's.
    pub(crate) fn undo(
        &mut self,
        now: Kept,
        current_part: &str,
        current_caret: TextPosition,
    ) -> Option<(Kept, TextPosition)> {
        let step = self.past.pop()?;
        self.future.push(Step {
            kept: now,
            part: current_part.to_owned(),
            caret: current_caret,
            revision: self.revision,
            kind: step.kind,
            ends_at: step.ends_at,
        });
        self.revision = step.revision;
        Some((step.kept, step.caret))
    }

    /// Steps forward again, the same way.
    pub(crate) fn redo(
        &mut self,
        now: Kept,
        current_part: &str,
        current_caret: TextPosition,
    ) -> Option<(Kept, TextPosition)> {
        let step = self.future.pop()?;
        self.past.push(Step {
            kept: now,
            part: current_part.to_owned(),
            caret: current_caret,
            revision: self.revision,
            kind: step.kind,
            ends_at: step.ends_at,
        });
        self.revision = step.revision;
        Some((step.kept, step.caret))
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

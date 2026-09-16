//! Comparing two documents, and writing the difference as tracked changes.
//!
//! # What a comparison is
//!
//! Not a report. Word's Compare produces a *document*: the original, with
//! everything the second version added marked as an insertion and everything it
//! removed marked as a deletion, by an author called after the comparison. It
//! is then read, and accepted or rejected, with the same commands as any other
//! set of changes — which is the point, because a reviewer should not have to
//! learn a second way of doing the same thing.
//!
//! So this does not invent any machinery. It works out what changed and then
//! makes those changes with tracking switched on.
//!
//! # How the difference is found
//!
//! Twice over. First by paragraph, to find which paragraphs are the same in
//! both; then, within a paragraph that changed, by word — because "the cat sat"
//! becoming "the cat sat down" is one word added, not a whole paragraph
//! replaced, and marking it as the latter makes the comparison useless.

use wp_xml::tree::Element;

use crate::revisions::Reviser;
use crate::{edit, position, read, Document, TextPosition};

/// What a comparison is to take notice of.
///
/// Word's Compare dialog is a list of tick boxes, and every one of them is a
/// question about what counts as a difference. A person comparing a draft
/// against its retyped copy does not want to be told that a space became two;
/// one comparing a contract does. So they are asked rather than decided.
///
/// The defaults are Word's: everything on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Whether a paragraph carried from one place to another is one move
    /// rather than a deletion and an insertion.
    pub moves: bool,
    /// Whether a paragraph nobody retyped but somebody re-styled has changed.
    pub formatting: bool,
    /// Whether "The" becoming "the" is a change.
    pub case: bool,
    /// Whether one space becoming two is.
    pub white_space: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { moves: true, formatting: true, case: true, white_space: true }
    }
}

impl Options {
    /// What two pieces of text are compared as.
    ///
    /// The text itself where everything is being taken notice of, and
    /// otherwise the same text with what is being ignored flattened out of
    /// it. The original is still what gets written into the document: this is
    /// only what decides whether two pieces are the same.
    #[must_use]
    pub fn key(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut last_was_space = false;
        for character in text.chars() {
            if !self.white_space && character.is_whitespace() {
                if !last_was_space {
                    out.push(' ');
                }
                last_was_space = true;
                continue;
            }
            last_was_space = false;
            if self.case {
                out.push(character);
            } else {
                out.extend(character.to_lowercase());
            }
        }
        if !self.white_space {
            return out.trim().to_owned();
        }
        out
    }
}

/// One step of turning the first sequence into the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The same in both, at these two places.
    Keep(usize, usize),
    /// In the second only, at this place in it.
    Insert(usize),
    /// In the first only, at this place in it.
    Delete(usize),
}

/// How large a comparison this will work out exactly.
///
/// The table is one cell per pair, so a thousand paragraphs against a thousand
/// is a million cells — which is fine, and ten thousand against ten thousand is
/// not. Past this the two are treated as wholly different, which is a poor
/// comparison but a fast one, and it is said rather than hidden.
const LARGEST: usize = 4_000;

/// The difference between two sequences, as the steps that turn one into the
/// other.
///
/// The longest common subsequence, which is what every diff is: the most that
/// can be kept, so the least is marked as changed.
#[must_use]
pub fn difference<T: PartialEq>(first: &[T], second: &[T]) -> Vec<Step> {
    if first.is_empty() {
        return (0..second.len()).map(Step::Insert).collect();
    }
    if second.is_empty() {
        return (0..first.len()).map(Step::Delete).collect();
    }
    if first.len().saturating_mul(second.len()) > LARGEST * LARGEST {
        let mut steps: Vec<Step> = (0..first.len()).map(Step::Delete).collect();
        steps.extend((0..second.len()).map(Step::Insert));
        return steps;
    }

    // How much of the two can be kept, counted from the end backwards. Cell
    // (i, j) is the answer for `first[i..]` against `second[j..]`.
    let width = second.len() + 1;
    let mut table = vec![0u32; (first.len() + 1) * width];
    for i in (0..first.len()).rev() {
        for j in (0..second.len()).rev() {
            table[i * width + j] = if first[i] == second[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }

    // Walked forwards, taking whichever step the table says loses nothing.
    let mut steps = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < first.len() && j < second.len() {
        if first[i] == second[j] {
            steps.push(Step::Keep(i, j));
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            steps.push(Step::Delete(i));
            i += 1;
        } else {
            steps.push(Step::Insert(j));
            j += 1;
        }
    }
    while i < first.len() {
        steps.push(Step::Delete(i));
        i += 1;
    }
    while j < second.len() {
        steps.push(Step::Insert(j));
        j += 1;
    }
    steps
}

/// Splits a paragraph into the pieces a comparison works on.
///
/// Words with the space that follows them, so that inserting a word marks the
/// word and its space rather than leaving two words run together.
#[must_use]
pub fn pieces(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_space = false;

    for character in text.chars() {
        let is_space = character.is_whitespace();
        if is_space {
            in_space = true;
            current.push(character);
            continue;
        }
        if in_space {
            out.push(core::mem::take(&mut current));
            in_space = false;
        }
        current.push(character);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// What is to be done to one paragraph of the original.
///
/// The difference alone is not enough: a paragraph that was *edited* comes back
/// from it as a deletion and an insertion side by side, and marking it that way
/// would say the whole paragraph was replaced when one word changed. So a
/// deletion standing next to an insertion is read as a change to that
/// paragraph, and only what is left over is a whole paragraph added or removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// The same in both.
    Same(usize, usize),
    /// This paragraph of the original becomes that paragraph of the revision.
    Change(usize, usize),
    /// In the original only.
    Removed(usize),
    /// In the revision only.
    Added(usize),
    /// In the original only, and the same words turn up somewhere else in the
    /// revision: this is where they were. The number ties the two halves of
    /// one move together.
    MovedFrom(usize, u32),
    /// And this is where they went.
    MovedTo(usize, u32),
}

/// Turns a difference into what is to be done, pairing changes up.
#[must_use]
pub fn edits(steps: &[Step]) -> Vec<Edit> {
    let mut out = Vec::new();
    let mut at = 0usize;

    while at < steps.len() {
        match steps[at] {
            Step::Keep(mine, theirs) => {
                out.push(Edit::Same(mine, theirs));
                at += 1;
            }
            _ => {
                // A run of deletions and insertions in any order. They are
                // gathered together and then paired off, because a diff may
                // report them either way round.
                let start = at;
                while at < steps.len() && !matches!(steps[at], Step::Keep(_, _)) {
                    at += 1;
                }
                let removed: Vec<usize> = steps[start..at]
                    .iter()
                    .filter_map(|step| match step {
                        Step::Delete(index) => Some(*index),
                        _ => None,
                    })
                    .collect();
                let added: Vec<usize> = steps[start..at]
                    .iter()
                    .filter_map(|step| match step {
                        Step::Insert(index) => Some(*index),
                        _ => None,
                    })
                    .collect();

                let paired = removed.len().min(added.len());
                for index in 0..paired {
                    out.push(Edit::Change(removed[index], added[index]));
                }
                for index in removed.iter().skip(paired) {
                    out.push(Edit::Removed(*index));
                }
                for index in added.iter().skip(paired) {
                    out.push(Edit::Added(*index));
                }
            }
        }
    }
    out
}

/// Pairs a paragraph that went with the same paragraph that arrived.
///
/// A paragraph carried from one place to another comes out of a difference as
/// a deletion and an insertion that happen to say the same thing. Word calls
/// that a move and marks it as one, and for a good reason: a reviewer reading
/// "this paragraph was deleted" and, four pages later, "this paragraph was
/// added" has to work out for themselves that it is the same paragraph.
///
/// Only whole paragraphs, and only where the words are exactly what the
/// options say counts as the same. Word will call a moved sentence a move;
/// this is the case that matters and the one that cannot be a coincidence.
#[must_use]
pub fn paired_moves(plan: &[Edit], mine: &[String], theirs: &[String]) -> Vec<Edit> {
    let mut out = plan.to_vec();
    let mut name = 1u32;

    for from in 0..out.len() {
        let Edit::Removed(mine_at) = out[from] else { continue };
        let Some(text) = mine.get(mine_at) else { continue };
        // An empty paragraph matches every other empty paragraph, and calling
        // that a move would be calling every blank line a move.
        if text.trim().is_empty() {
            continue;
        }
        // The first that has not been paired already: once one is, it is no
        // longer an addition and cannot be found twice.
        let found = (0..out.len())
            .find(|to| matches!(out[*to], Edit::Added(index) if theirs.get(index) == Some(text)));
        let Some(to) = found else { continue };
        let Edit::Added(theirs_at) = out[to] else { continue };
        out[from] = Edit::MovedFrom(mine_at, name);
        out[to] = Edit::MovedTo(theirs_at, name);
        name += 1;
    }
    out
}

impl Document {
    /// Turns this document into a comparison of itself with another.
    ///
    /// Afterwards it holds everything both versions have, with what the other
    /// added marked as inserted and what it dropped marked as deleted. Returns
    /// how many changes were marked.
    pub fn compare_with(&mut self, revised: &Document, author: &str) -> usize {
        self.compare_with_options(revised, author, Options::default())
    }

    /// The same, taking notice of what it is asked to.
    pub fn compare_with_options(
        &mut self,
        revised: &Document,
        author: &str,
        options: Options,
    ) -> usize {
        let mine: Vec<String> = (0..self.paragraph_count())
            .map(|at| self.paragraph_text(at).unwrap_or_default())
            .collect();
        let theirs: Vec<String> = (0..revised.paragraph_count())
            .map(|at| revised.paragraph_text(at).unwrap_or_default())
            .collect();
        // Compared as what the options say to compare, and written back as
        // what was actually there.
        let my_keys: Vec<String> = mine.iter().map(|text| options.key(text)).collect();
        let their_keys: Vec<String> = theirs.iter().map(|text| options.key(text)).collect();
        let plan = edits(&difference(&my_keys, &their_keys));
        let plan = if options.moves { paired_moves(&plan, &my_keys, &their_keys) } else { plan };

        let was_tracking = self.tracking_changes();
        let was_reviser = self.reviser.clone();
        self.set_reviser(Reviser { author: author.to_owned(), ..was_reviser.clone() });
        // One comparison is one thing done, so it is one thing to undo.
        self.begin_gesture();
        self.set_tracking_changes(true);
        // Set on the field as well: the setting is read from the package once
        // when the document is opened and kept, so writing it is not enough.
        self.tracking = true;

        // Worked backwards, so that changing one paragraph never moves the
        // paragraphs still to be changed.
        let mut marked = 0usize;
        for (at, edit) in plan.iter().enumerate().rev() {
            match edit {
                Edit::Same(mine_at, theirs_at) => {
                    if options.formatting {
                        marked += self.mark_formatting(*mine_at, revised, *theirs_at);
                    }
                }
                Edit::Change(mine_at, theirs_at) => {
                    marked += self.mark_paragraph_difference(*mine_at, &theirs[*theirs_at]);
                    if options.formatting {
                        marked += self.mark_formatting(*mine_at, revised, *theirs_at);
                    }
                }
                Edit::Removed(mine_at) => {
                    marked += self.mark_paragraph_deleted(*mine_at);
                }
                Edit::Added(theirs_at) => {
                    marked += self.add_paragraph_tracked(&plan[..at], &theirs[*theirs_at]).0;
                }
                Edit::MovedFrom(mine_at, name) => {
                    marked += self.mark_paragraph_deleted(*mine_at);
                    self.rename_tracked(*mine_at, "del", "moveFrom", *name);
                }
                Edit::MovedTo(theirs_at, name) => {
                    let (added, into) =
                        self.add_paragraph_tracked(&plan[..at], &theirs[*theirs_at]);
                    if added > 0 {
                        self.rename_tracked(into, "ins", "moveTo", *name);
                    }
                    marked += added;
                }
            }
        }

        self.end_gesture();
        self.set_tracking_changes(was_tracking);
        self.tracking = was_tracking;
        self.set_reviser(was_reviser);
        marked
    }

    /// Marks the difference between one paragraph and its new text.
    pub(crate) fn mark_paragraph_difference(&mut self, paragraph: usize, wanted: &str) -> usize {
        let current = self.paragraph_text(paragraph).unwrap_or_default();
        if current == wanted {
            return 0;
        }

        let mine = pieces(&current);
        let theirs = pieces(wanted);
        let steps = difference(&mine, &theirs);

        // Where each piece begins, so a step can be turned into an offset.
        let mut starts = Vec::with_capacity(mine.len() + 1);
        let mut at = 0usize;
        for piece in &mine {
            starts.push(at);
            at += piece.len();
        }
        starts.push(at);

        // How far into the original each step has reached, so an insertion
        // knows where it belongs. Worked out forwards, used backwards.
        let mut reached = Vec::with_capacity(steps.len());
        let mut consumed = 0usize;
        for step in &steps {
            reached.push(consumed);
            if matches!(step, Step::Keep(_, _) | Step::Delete(_)) {
                consumed += 1;
            }
        }

        let mut marked = 0usize;
        for (index, step) in steps.iter().enumerate().rev() {
            match step {
                Step::Keep(_, _) => {}
                Step::Delete(piece) => {
                    let (from, to) = (starts[*piece], starts[*piece + 1]);
                    self.set_caret(TextPosition::new(paragraph, from));
                    self.extend_selection_to(TextPosition::new(paragraph, to));
                    if self.delete_selection() {
                        marked += 1;
                    }
                }
                Step::Insert(piece) => {
                    let offset = starts.get(reached[index]).copied().unwrap_or(at);
                    self.set_caret(TextPosition::new(paragraph, offset));
                    self.clear_selection();
                    if self.type_text(&theirs[*piece]) {
                        marked += 1;
                    }
                }
            }
        }
        marked
    }

    /// Marks the difference between one paragraph's formatting and another's.
    ///
    /// A paragraph nobody retyped and somebody re-styled is a change, and one
    /// a comparison that only read the words would miss entirely. What is
    /// compared is what the paragraph itself says — its style, its alignment,
    /// its indents, its spacing — and what each of its runs says, where the
    /// runs line up.
    ///
    /// The old properties are kept in a `w:pPrChange` or a `w:rPrChange`,
    /// which is what rejecting the change puts back.
    fn mark_formatting(&mut self, at: usize, revised: &Document, theirs: usize) -> usize {
        let Some(wanted) =
            revised.paragraph_elements().get(theirs).map(|element| (*element).clone())
        else {
            return 0;
        };
        let prefix = self.prefix();
        let Some(path) = position::paragraph_path(&self.tree().root, at) else { return 0 };
        let reviser = self.reviser.clone();
        let id = self.next_revision_id();
        let Some(mine) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return 0;
        };

        let mut marked = 0usize;
        if crate::format::note_properties_change(
            mine,
            "pPr",
            wanted.child(Some(read::W), "pPr"),
            &reviser,
            id,
            prefix.as_deref(),
        ) {
            marked += 1;
        }

        // The runs, where there are the same number of them saying the same
        // words. Anything else is a paragraph whose text changed, and the
        // words are what the rest of the comparison is for.
        let mut theirs_runs: Vec<Element> =
            wanted.child_elements().filter(|child| child.is(Some(read::W), "r")).cloned().collect();
        let mine_runs = mine
            .child_elements()
            .filter(|child| child.is(Some(read::W), "r"))
            .map(|run| run.text_content())
            .collect::<Vec<String>>();
        if mine_runs.len() == theirs_runs.len()
            && mine_runs.iter().zip(&theirs_runs).all(|(text, run)| *text == run.text_content())
        {
            let mut which = 0usize;
            for run in mine.child_elements_mut() {
                if !run.is(Some(read::W), "r") {
                    continue;
                }
                let wanted = theirs_runs[which].child(Some(read::W), "rPr").cloned();
                if crate::format::note_properties_change(
                    run,
                    "rPr",
                    wanted.as_ref(),
                    &reviser,
                    id,
                    prefix.as_deref(),
                ) {
                    marked += 1;
                }
                which += 1;
            }
        }
        theirs_runs.clear();

        if marked > 0 {
            self.mark_modified();
        }
        marked
    }

    /// Turns a tracked deletion or insertion into the half of a move.
    ///
    /// The marking is done by the ordinary machinery — a move is a deletion
    /// in one place and an insertion in another, and the format says as much
    /// by writing them the same way with another name. So this renames what
    /// was written and puts the marks round it that say which move it is.
    fn rename_tracked(&mut self, paragraph: usize, from: &str, to: &str, name: u32) {
        let prefix = self.prefix();
        let Some(path) = position::paragraph_path(&self.tree().root, paragraph) else { return };
        let Some(element) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return;
        };

        let mut found = false;
        for child in element.child_elements_mut() {
            if child.is(Some(read::W), from) {
                child.name = edit::name_with(prefix.as_deref(), to);
                found = true;
            }
        }
        if !found {
            return;
        }

        // Word brackets a move with marks naming it, so that a reader can
        // tell which "moved from" belongs to which "moved to".
        let marks = (format!("{to}RangeStart"), format!("{to}RangeEnd"));
        let mut start = Element::new(&edit::name_with(prefix.as_deref(), &marks.0), Some(read::W));
        start.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "id"),
            read::W,
            &name.to_string(),
        );
        start.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "name"),
            read::W,
            &format!("move{name}"),
        );
        let mut end = Element::new(&edit::name_with(prefix.as_deref(), &marks.1), Some(read::W));
        end.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "id"),
            read::W,
            &name.to_string(),
        );

        // Inside the paragraph, round everything in it: a move of a whole
        // paragraph is what this marks.
        let at = usize::from(element.child(Some(read::W), "pPr").is_some());
        element.insert_element(at, start);
        element.push_element(end);
        self.mark_modified();
    }

    /// Marks a whole paragraph as deleted.
    pub(crate) fn mark_paragraph_deleted(&mut self, paragraph: usize) -> usize {
        let text = self.paragraph_text(paragraph).unwrap_or_default();
        if text.is_empty() {
            return 0;
        }
        self.set_caret(TextPosition::new(paragraph, 0));
        self.extend_selection_to(TextPosition::new(paragraph, text.len()));
        usize::from(self.delete_selection())
    }

    /// Puts in a paragraph the other version has and this one does not.
    ///
    /// `earlier` is the plan up to this point, which says which paragraph of
    /// the original the new one belongs after.
    /// Says how much was marked, and which paragraph it went into.
    fn add_paragraph_tracked(&mut self, earlier: &[Edit], text: &str) -> (usize, usize) {
        let after = earlier
            .iter()
            .filter_map(|edit| match edit {
                Edit::Same(mine, _)
                | Edit::Change(mine, _)
                | Edit::Removed(mine)
                | Edit::MovedFrom(mine, _) => Some(*mine),
                Edit::Added(_) | Edit::MovedTo(_, _) => None,
            })
            .max();

        match after {
            Some(paragraph) => {
                let end = self.paragraph_text(paragraph).unwrap_or_default().len();
                self.set_caret(TextPosition::new(paragraph, end));
                self.clear_selection();
                self.press_enter();
                (usize::from(self.type_text(text)), paragraph + 1)
            }
            None => {
                // Before everything: written at the start, then a break after
                // it to push the original down.
                self.set_caret(TextPosition::new(0, 0));
                self.clear_selection();
                let written = self.type_text(text);
                self.press_enter();
                (usize::from(written), 0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(first: &[&str], second: &[&str]) -> Vec<Step> {
        difference(first, second)
    }

    #[test]
    fn two_sequences_the_same_are_all_kept() {
        let found = steps(&["a", "b", "c"], &["a", "b", "c"]);
        assert!(found.iter().all(|step| matches!(step, Step::Keep(_, _))));
        assert_eq!(found.len(), 3);
    }

    #[test]
    fn something_added_at_the_end_is_one_insertion() {
        let found = steps(&["a", "b"], &["a", "b", "c"]);
        assert_eq!(found.len(), 3);
        assert_eq!(found[2], Step::Insert(2));
    }

    #[test]
    fn something_taken_from_the_middle_is_one_deletion() {
        let found = steps(&["a", "b", "c"], &["a", "c"]);
        assert_eq!(found.iter().filter(|step| matches!(step, Step::Delete(_))).count(), 1);
        assert_eq!(found[1], Step::Delete(1));
    }

    #[test]
    fn something_changed_is_a_deletion_and_an_insertion() {
        let found = steps(&["a", "b", "c"], &["a", "x", "c"]);
        assert_eq!(found.iter().filter(|step| matches!(step, Step::Delete(_))).count(), 1);
        assert_eq!(found.iter().filter(|step| matches!(step, Step::Insert(_))).count(), 1);
        assert_eq!(found.iter().filter(|step| matches!(step, Step::Keep(_, _))).count(), 2);
    }

    #[test]
    fn comparing_with_nothing_deletes_everything() {
        let found = steps(&["a", "b"], &[]);
        assert_eq!(found, vec![Step::Delete(0), Step::Delete(1)]);
    }

    #[test]
    fn comparing_nothing_with_something_inserts_everything() {
        let found = steps(&[], &["a", "b"]);
        assert_eq!(found, vec![Step::Insert(0), Step::Insert(1)]);
    }

    #[test]
    fn the_most_that_can_be_kept_is_kept() {
        // Moving a word should not be read as replacing the whole sequence.
        let found = steps(&["the", "cat", "sat"], &["the", "cat", "sat", "down"]);
        assert_eq!(found.iter().filter(|step| matches!(step, Step::Keep(_, _))).count(), 3);
    }

    #[test]
    fn a_paragraph_changed_is_one_change_and_not_a_removal_and_an_addition() {
        let steps = difference(&["one", "two"], &["one", "TWO"]);
        let plan = edits(&steps);
        assert_eq!(plan, vec![Edit::Same(0, 0), Edit::Change(1, 1)]);
    }

    #[test]
    fn what_is_left_over_after_pairing_is_added_or_removed() {
        // Two paragraphs become one: one is a change, the other a removal.
        let plan = edits(&difference(&["a", "b", "c"], &["a", "X"]));
        assert!(plan.contains(&Edit::Same(0, 0)));
        assert_eq!(plan.iter().filter(|edit| matches!(edit, Edit::Change(_, _))).count(), 1);
        assert_eq!(plan.iter().filter(|edit| matches!(edit, Edit::Removed(_))).count(), 1);
    }

    #[test]
    fn a_paragraph_added_with_nothing_removed_stays_an_addition() {
        let plan = edits(&difference(&["a"], &["a", "b"]));
        assert_eq!(plan, vec![Edit::Same(0, 0), Edit::Added(1)]);
    }

    #[test]
    fn nothing_changed_is_all_the_same() {
        let plan = edits(&difference(&["a", "b"], &["a", "b"]));
        assert_eq!(plan, vec![Edit::Same(0, 0), Edit::Same(1, 1)]);
    }

    #[test]
    fn a_paragraph_splits_into_words_with_the_spaces_that_follow_them() {
        assert_eq!(pieces("the cat sat"), vec!["the ", "cat ", "sat"]);
    }

    #[test]
    fn a_run_of_spaces_stays_with_the_word_before_it() {
        assert_eq!(pieces("a  b"), vec!["a  ", "b"]);
    }

    #[test]
    fn splitting_nothing_gives_nothing() {
        assert!(pieces("").is_empty());
    }

    #[test]
    fn the_pieces_join_back_into_what_they_came_from() {
        for text in ["the cat sat", "a  b", " leading", "trailing ", "one"] {
            assert_eq!(pieces(text).concat(), text, "{text:?} did not survive");
        }
    }
}

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

use crate::revisions::Reviser;
use crate::{Document, TextPosition};

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

impl Document {
    /// Turns this document into a comparison of itself with another.
    ///
    /// Afterwards it holds everything both versions have, with what the other
    /// added marked as inserted and what it dropped marked as deleted. Returns
    /// how many changes were marked.
    pub fn compare_with(&mut self, revised: &Document, author: &str) -> usize {
        let mine: Vec<String> = (0..self.paragraph_count())
            .map(|at| self.paragraph_text(at).unwrap_or_default())
            .collect();
        let theirs: Vec<String> = (0..revised.paragraph_count())
            .map(|at| revised.paragraph_text(at).unwrap_or_default())
            .collect();
        let plan = edits(&difference(&mine, &theirs));

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
                Edit::Same(_, _) => {}
                Edit::Change(mine_at, theirs_at) => {
                    marked += self.compare_paragraph(*mine_at, &theirs[*theirs_at]);
                }
                Edit::Removed(mine_at) => {
                    marked += self.delete_paragraph_tracked(*mine_at);
                }
                Edit::Added(theirs_at) => {
                    marked += self.add_paragraph_tracked(&plan[..at], &theirs[*theirs_at]);
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
    fn compare_paragraph(&mut self, paragraph: usize, wanted: &str) -> usize {
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

    /// Marks a whole paragraph as deleted.
    fn delete_paragraph_tracked(&mut self, paragraph: usize) -> usize {
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
    fn add_paragraph_tracked(&mut self, earlier: &[Edit], text: &str) -> usize {
        let after = earlier
            .iter()
            .filter_map(|edit| match edit {
                Edit::Same(mine, _) | Edit::Change(mine, _) | Edit::Removed(mine) => Some(*mine),
                Edit::Added(_) => None,
            })
            .max();

        match after {
            Some(paragraph) => {
                let end = self.paragraph_text(paragraph).unwrap_or_default().len();
                self.set_caret(TextPosition::new(paragraph, end));
                self.clear_selection();
                self.press_enter();
                usize::from(self.type_text(text))
            }
            None => {
                // Before everything: written at the start, then a break after
                // it to push the original down.
                self.set_caret(TextPosition::new(0, 0));
                self.clear_selection();
                let written = self.type_text(text);
                self.press_enter();
                usize::from(written)
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

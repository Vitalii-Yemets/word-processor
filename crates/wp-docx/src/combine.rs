//! Combining two revisions of one document: the three-way merge.
//!
//! # What this is and what [`crate::compare`] is
//!
//! Comparing takes two documents and says how the second differs from the
//! first. Combining takes *three*: the original, and two copies of it that
//! two people edited without seeing each other's work. What comes out holds
//! both sets of changes, each marked with the name of whoever made it, so
//! that one person can read the two together and take what they want.
//!
//! Word calls it Combine and offers it beside Compare. The name the older
//! literature uses is a three-way merge, and the third way is the point: with
//! only the two revisions there is no telling whether a sentence is in one
//! and not the other because somebody added it or because somebody took it
//! away. The original settles that.
//!
//! # What happens where they disagree
//!
//! Both changes go in, and the place is counted as a conflict. There is no
//! right answer for a program to pick — two people wrote two different things
//! and only a person can say which — so what this does is put them side by
//! side as two tracked changes and say how many such places there are. That
//! is what Word does, and it is the only honest thing to do: a merge that
//! quietly chose one would be a merge that lost work without saying so.
//!
//! # What it works on
//!
//! Paragraphs, and words inside a paragraph, which is what [`crate::compare`]
//! works on. A change to the *formatting* of a paragraph that neither author
//! changed the words of is not seen; that is named in the roadmap.

use crate::compare::{difference, edits, Edit};
use crate::revisions::Reviser;
use crate::{Document, TextPosition};

/// What came of combining.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Combined {
    /// How many changes were marked, both authors' together.
    pub changes: usize,
    /// How many places the two authors changed in different ways. Both
    /// changes are in the document; somebody has to choose.
    pub conflicts: usize,
}

/// What one author did to one paragraph of the original.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Did {
    /// Left it alone.
    Nothing,
    /// Wrote it differently.
    Wrote(String),
    /// Took it out.
    Removed,
}

impl Document {
    /// Combines two revisions of this document into it.
    ///
    /// `self` is the original both revisions were made from; it is left
    /// holding everything both of them did, as tracked changes named after
    /// their authors.
    pub fn combine(
        &mut self,
        mine: &Document,
        theirs: &Document,
        authors: (&str, &str),
    ) -> Combined {
        let original = paragraphs(self);
        let ours = paragraphs(mine);
        let yours = paragraphs(theirs);

        let (did_ours, added_ours) = what_was_done(original.len(), &ours, &original);
        let (did_yours, added_yours) = what_was_done(original.len(), &yours, &original);

        let was_tracking = self.tracking_changes();
        let was_reviser = self.reviser.clone();
        // One combining is one thing done, so it is one thing to undo.
        self.begin_gesture();
        self.set_tracking_changes(true);
        // Set on the field as well: the setting is read from the package once
        // when the document is opened and kept, so writing it is not enough.
        self.tracking = true;

        let mut out = Combined::default();

        // The paragraphs of the original, backwards, so that changing one
        // never moves the ones still to be done. Nothing here adds or takes
        // away a paragraph — a tracked deletion leaves the text where it is,
        // marked — so the numbering holds.
        for at in (0..original.len()).rev() {
            let ours = did_ours.get(at).cloned().unwrap_or(Did::Nothing);
            let yours = did_yours.get(at).cloned().unwrap_or(Did::Nothing);
            match (&ours, &yours) {
                (Did::Nothing, Did::Nothing) => {}
                (Did::Nothing, other) => {
                    out.changes += self.apply(at, other, authors.1, &was_reviser);
                }
                (one, Did::Nothing) => {
                    out.changes += self.apply(at, one, authors.0, &was_reviser);
                }
                // The same thing twice is one thing, and the first to have
                // done it is the one it is marked as.
                (one, other) if one == other => {
                    out.changes += self.apply(at, one, authors.0, &was_reviser);
                }
                // Two different things. Both go in, and somebody has to
                // choose.
                (one, other) => {
                    out.conflicts += 1;
                    out.changes += self.apply(at, one, authors.0, &was_reviser);
                    if let Did::Wrote(text) = other {
                        out.changes += self.add_after(at, text, authors.1, &was_reviser);
                    } else {
                        out.changes += self.apply(at, other, authors.1, &was_reviser);
                    }
                }
            }
        }

        // Then the paragraphs each of them added, backwards by where they
        // belong, so that one addition does not move the next one's place.
        let mut additions: Vec<(Option<usize>, &String, &str)> = Vec::new();
        for (anchor, text) in &added_ours {
            additions.push((*anchor, text, authors.0));
        }
        for (anchor, text) in &added_yours {
            additions.push((*anchor, text, authors.1));
        }
        // By where they go, and the second author's after the first's at the
        // same place, which is what putting them in one at a time from the
        // end gives.
        additions.sort_by_key(|(anchor, _, _)| anchor.map_or(0, |at| at + 1));
        for (anchor, text, author) in additions.into_iter().rev() {
            out.changes += match anchor {
                Some(at) => self.add_after(at, text, author, &was_reviser),
                None => self.add_first(text, author, &was_reviser),
            };
        }

        self.end_gesture();
        self.set_tracking_changes(was_tracking);
        self.tracking = was_tracking;
        self.set_reviser(was_reviser);
        out
    }

    /// Does to one paragraph what one author did to it.
    fn apply(&mut self, at: usize, did: &Did, author: &str, was: &Reviser) -> usize {
        self.set_reviser(Reviser { author: author.to_owned(), ..was.clone() });
        match did {
            Did::Nothing => 0,
            Did::Wrote(text) => self.mark_paragraph_difference(at, text),
            Did::Removed => self.mark_paragraph_deleted(at),
        }
    }

    /// Puts a paragraph in after another, marked as this author's.
    fn add_after(&mut self, at: usize, text: &str, author: &str, was: &Reviser) -> usize {
        self.set_reviser(Reviser { author: author.to_owned(), ..was.clone() });
        let end = self.paragraph_text(at).unwrap_or_default().len();
        self.set_caret(TextPosition::new(at, end));
        self.clear_selection();
        self.press_enter();
        usize::from(self.type_text(text))
    }

    /// And one before everything.
    fn add_first(&mut self, text: &str, author: &str, was: &Reviser) -> usize {
        self.set_reviser(Reviser { author: author.to_owned(), ..was.clone() });
        self.set_caret(TextPosition::new(0, 0));
        self.clear_selection();
        let written = self.type_text(text);
        self.press_enter();
        usize::from(written)
    }
}

/// Every paragraph of a document, as text.
fn paragraphs(document: &Document) -> Vec<String> {
    (0..document.paragraph_count())
        .map(|at| document.paragraph_text(at).unwrap_or_default())
        .collect()
}

/// What one author did to each paragraph of the original, and what they
/// added that was not in it.
fn what_was_done(
    length: usize,
    revised: &[String],
    original: &[String],
) -> (Vec<Did>, Vec<(Option<usize>, String)>) {
    let plan = edits(&difference(original, revised));
    let mut did = vec![Did::Nothing; length];
    let mut added = Vec::new();
    let mut after: Option<usize> = None;

    for edit in &plan {
        match edit {
            Edit::Same(at, _) => after = Some(*at),
            Edit::Change(at, theirs) => {
                if let Some(slot) = did.get_mut(*at) {
                    *slot = Did::Wrote(revised[*theirs].clone());
                }
                after = Some(*at);
            }
            Edit::Removed(at) => {
                if let Some(slot) = did.get_mut(*at) {
                    *slot = Did::Removed;
                }
                after = Some(*at);
            }
            Edit::Added(theirs) | Edit::MovedTo(theirs, _) => {
                added.push((after, revised[*theirs].clone()));
            }
            // A combining does not pair moves up - it has no options and
            // asks for none - but the plan it works from is the same sort of
            // plan, so the two halves are read as what they are made of.
            Edit::MovedFrom(at, _) => {
                if let Some(slot) = did.get_mut(*at) {
                    *slot = Did::Removed;
                }
                after = Some(*at);
            }
        }
    }
    (did, added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_each_author_did_is_read_off_the_difference() {
        let original =
            vec!["one".to_owned(), "two".to_owned(), "three".to_owned(), "four".to_owned()];
        // The second changed, the third dropped, one added at the end.
        let revised =
            vec!["one".to_owned(), "TWO".to_owned(), "four".to_owned(), "five".to_owned()];
        let (did, added) = what_was_done(original.len(), &revised, &original);

        assert_eq!(did[0], Did::Nothing);
        assert_eq!(did[1], Did::Wrote("TWO".to_owned()));
        assert_eq!(did[2], Did::Removed);
        assert_eq!(did[3], Did::Nothing);
        assert_eq!(added, vec![(Some(3), "five".to_owned())]);
    }

    #[test]
    fn a_paragraph_added_at_the_very_front_has_nothing_to_go_after() {
        let original = vec!["one".to_owned()];
        let revised = vec!["nought".to_owned(), "one".to_owned()];
        let (_, added) = what_was_done(original.len(), &revised, &original);
        assert_eq!(added, vec![(None, "nought".to_owned())]);
    }

    #[test]
    fn nothing_done_is_nothing_recorded() {
        let original = vec!["one".to_owned(), "two".to_owned()];
        let (did, added) = what_was_done(original.len(), &original, &original);
        assert!(did.iter().all(|what| *what == Did::Nothing));
        assert!(added.is_empty());
    }
}

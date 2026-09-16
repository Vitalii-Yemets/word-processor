//! Two people editing one document, and what comes of putting their work
//! together.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::Document;

fn document(lines: &[&str]) -> Document {
    let mut body = Body::default();
    for line in lines {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// The text as a reader sees it with the changes shown, which is what the
/// document says after a merge.
fn shown(document: &Document) -> Vec<String> {
    (0..document.paragraph_count())
        .map(|at| document.paragraph_text(at).unwrap_or_default())
        .collect()
}

#[test]
fn two_people_changing_different_paragraphs_get_both_changes() {
    let mut original = document(&["one", "two", "three"]);
    let mine = document(&["ONE", "two", "three"]);
    let theirs = document(&["one", "two", "THREE"]);

    let combined = original.combine(&mine, &theirs, ("Ada", "Grace"));
    assert_eq!(combined.conflicts, 0, "they did not touch the same thing");
    assert!(combined.changes > 0);

    let text = shown(&original);
    assert!(text[0].contains("ONE"), "{text:?}");
    assert!(text[2].contains("THREE"), "{text:?}");
    // And both are marked, by the two of them.
    let authors: Vec<String> = original.changes().into_iter().map(|change| change.author).collect();
    assert!(authors.contains(&"Ada".to_owned()), "{authors:?}");
    assert!(authors.contains(&"Grace".to_owned()), "{authors:?}");
}

#[test]
fn two_people_writing_the_same_thing_is_one_change_and_no_conflict() {
    let mut original = document(&["one", "two"]);
    let same = document(&["one", "TWO"]);
    let also = document(&["one", "TWO"]);

    let combined = original.combine(&same, &also, ("Ada", "Grace"));
    assert_eq!(combined.conflicts, 0);
    assert!(shown(&original)[1].contains("TWO"));
    // Marked once, and as the first of them.
    let authors: Vec<String> = original.changes().into_iter().map(|change| change.author).collect();
    assert!(authors.contains(&"Ada".to_owned()), "{authors:?}");
    assert!(!authors.contains(&"Grace".to_owned()), "{authors:?}: it was marked twice");
}

#[test]
fn two_people_writing_different_things_in_one_place_is_a_conflict_with_both_kept() {
    let mut original = document(&["the cat sat"]);
    let mine = document(&["the cat sat down"]);
    let theirs = document(&["the dog sat"]);

    let combined = original.combine(&mine, &theirs, ("Ada", "Grace"));
    assert_eq!(combined.conflicts, 1, "the two of them wrote over each other");

    // Both are in the document, so that a person can choose.
    let whole = shown(&original).join("\n");
    assert!(whole.contains("down"), "one author's work was lost: {whole:?}");
    assert!(whole.contains("dog"), "the other author's work was lost: {whole:?}");
}

#[test]
fn a_paragraph_added_by_each_of_them_is_two_additions() {
    let mut original = document(&["one"]);
    let mine = document(&["one", "mine"]);
    let theirs = document(&["one", "theirs"]);

    let combined = original.combine(&mine, &theirs, ("Ada", "Grace"));
    assert_eq!(combined.conflicts, 0, "adding beside each other is not a conflict");
    let whole = shown(&original).join("\n");
    assert!(whole.contains("mine"), "{whole:?}");
    assert!(whole.contains("theirs"), "{whole:?}");
}

#[test]
fn one_taking_a_paragraph_out_while_the_other_leaves_it_alone() {
    let mut original = document(&["one", "two", "three"]);
    let mine = document(&["one", "three"]);
    let theirs = document(&["one", "two", "three"]);

    let combined = original.combine(&mine, &theirs, ("Ada", "Grace"));
    assert_eq!(combined.conflicts, 0);
    // The paragraph is still there, marked as deleted, which is what a
    // tracked deletion is.
    assert_eq!(original.paragraph_count(), 3);
    let deletions: Vec<String> = original
        .changes()
        .into_iter()
        .filter(|change| change.kind == wp_docx::revisions::ChangeKind::Deletion)
        .map(|change| change.author)
        .collect();
    assert_eq!(deletions, vec!["Ada".to_owned()], "{deletions:?}");
}

#[test]
fn one_taking_a_paragraph_out_while_the_other_rewrites_it_is_a_conflict() {
    let mut original = document(&["one", "two", "three"]);
    let mine = document(&["one", "three"]);
    let theirs = document(&["one", "TWO", "three"]);

    let combined = original.combine(&mine, &theirs, ("Ada", "Grace"));
    assert_eq!(combined.conflicts, 1, "a deletion against a rewrite is a disagreement");
}

#[test]
fn nothing_changed_by_either_is_nothing_to_combine() {
    let mut original = document(&["one", "two"]);
    let mine = document(&["one", "two"]);
    let theirs = document(&["one", "two"]);

    let combined = original.combine(&mine, &theirs, ("Ada", "Grace"));
    assert_eq!(combined, wp_docx::combine::Combined { changes: 0, conflicts: 0 });
    assert!(original.changes().is_empty());
}

#[test]
fn what_was_combined_survives_being_saved_and_opened_again() {
    let mut original = document(&["one", "two", "three"]);
    let mine = document(&["ONE", "two", "three"]);
    let theirs = document(&["one", "two", "THREE"]);
    original.combine(&mine, &theirs, ("Ada", "Grace"));

    let bytes = original.save().expect("saving");
    let reopened = Document::open(&bytes).expect("reopening");
    let authors: Vec<String> = reopened.changes().into_iter().map(|change| change.author).collect();
    assert!(authors.contains(&"Ada".to_owned()), "{authors:?}");
    assert!(authors.contains(&"Grace".to_owned()), "{authors:?}");
}

#[test]
fn combining_is_one_thing_to_undo() {
    let mut original = document(&["one", "two", "three"]);
    let mine = document(&["ONE", "two", "three"]);
    let theirs = document(&["one", "two", "THREE"]);
    let before = shown(&original);

    original.combine(&mine, &theirs, ("Ada", "Grace"));
    assert_ne!(shown(&original), before);
    assert!(original.undo());
    assert_eq!(shown(&original), before, "undoing left half a merge behind");
    assert!(original.changes().is_empty());
}

//! Editing at a place in the text, as a person points at it.
//!
//! Everything here works in terms of a paragraph and an offset into its text.
//! That is what a caret is, what a mouse click resolves to, and what a search
//! result is — so it is the vocabulary the editing layer speaks, rather than the
//! runs and elements underneath.
//!
//! Like the rest of the editing code, an operation touches only the nodes it
//! must. A paragraph holding a bookmark, a comment anchor or a tracked change
//! keeps them, because nothing walks over them.

use wp_xml::tree::{Element, Node};

use crate::edit::{
    collect_text_pieces, element_at_path_mut, name_with, preserve_space_if_needed, XML_NAMESPACE,
};
use crate::read::W;

/// A place in the document, as a person would point at it.
///
/// Paragraphs are numbered in reading order, including the ones inside table
/// cells, so this agrees with what the layout puts on the page. The offset is in
/// bytes into that paragraph's text, which is the unit a search result and a
/// caret already work in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPosition {
    pub paragraph: usize,
    pub offset: usize,
}

impl TextPosition {
    #[must_use]
    pub fn new(paragraph: usize, offset: usize) -> Self {
        Self { paragraph, offset }
    }
}

/// How many paragraphs the document has, in reading order.
#[must_use]
pub fn paragraph_count(root: &Element) -> usize {
    let mut counter = 0usize;
    count_paragraphs(root, &mut counter);
    counter
}

fn count_paragraphs(element: &Element, counter: &mut usize) {
    for child in element.child_elements() {
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        if child.is(Some(W), "p") {
            *counter += 1;
        } else {
            count_paragraphs(child, counter);
        }
    }
}

/// The path to the nth paragraph, in reading order.
///
/// A path rather than a reference: two paragraphs cannot be borrowed mutably at
/// once, and an index into a flattened list would go stale the moment one was
/// inserted.
#[must_use]
pub fn paragraph_path(root: &Element, index: usize) -> Option<Vec<usize>> {
    let mut found = None;
    let mut counter = 0usize;
    let mut path = Vec::new();
    walk_paragraphs(root, &mut path, &mut counter, index, &mut found);
    found
}

fn walk_paragraphs(
    element: &Element,
    path: &mut Vec<usize>,
    counter: &mut usize,
    wanted: usize,
    found: &mut Option<Vec<usize>>,
) {
    for (child_index, node) in element.children.iter().enumerate() {
        if found.is_some() {
            return;
        }
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(W) {
            continue;
        }

        path.push(child_index);
        if child.is(Some(W), "p") {
            if *counter == wanted {
                *found = Some(path.clone());
            }
            *counter += 1;
        } else {
            walk_paragraphs(child, path, counter, wanted, found);
        }
        path.pop();
    }
}

/// The text of one paragraph, measured the way a [`TextPosition`] offset is.
#[must_use]
pub fn paragraph_text(paragraph: &Element) -> String {
    collect_text_pieces(paragraph).into_iter().map(|piece| piece.text).collect()
}

/// The text of the nth paragraph.
#[must_use]
pub fn text_of(root: &Element, index: usize) -> Option<String> {
    let path = paragraph_path(root, index)?;
    let mut current = root;
    for step in &path {
        current = current.children.get(*step)?.as_element()?;
    }
    Some(paragraph_text(current))
}

/// Inserts text at a position. Returns whether anything was inserted.
pub fn insert_text(root: &mut Element, at: TextPosition, text: &str, prefix: Option<&str>) -> bool {
    if text.is_empty() {
        return false;
    }
    let Some(path) = paragraph_path(root, at.paragraph) else {
        return false;
    };
    let Some(paragraph) = element_at_path_mut(root, &path) else {
        return false;
    };

    let pieces = collect_text_pieces(paragraph);

    // An empty paragraph has nowhere to put text, so it gets a run.
    if pieces.is_empty() {
        let mut run = Element::new(&name_with(prefix, "r"), Some(W));
        let mut node = Element::new(&name_with(prefix, "t"), Some(W));
        node.set_text(text);
        node.set_namespaced_attribute("xml:space", XML_NAMESPACE, "preserve");
        run.push_element(node);
        // After the paragraph properties, which must stay first.
        let position = usize::from(paragraph.child(Some(W), "pPr").is_some());
        paragraph.insert_element(position, run);
        return true;
    }

    // The piece the offset falls in. At a boundary the earlier piece wins, so
    // typing at the end of a bold word stays bold rather than jumping style.
    let target = pieces
        .iter()
        .find(|piece| at.offset >= piece.start && at.offset <= piece.start + piece.text.len())
        .or_else(|| pieces.last());
    let Some(target) = target else { return false };

    let local = at.offset.saturating_sub(target.start).min(target.text.len());
    if !target.text.is_char_boundary(local) {
        return false;
    }

    let mut updated = target.text.clone();
    updated.insert_str(local, text);

    let piece_path = target.path.clone();
    match element_at_path_mut(paragraph, &piece_path) {
        Some(element) => {
            element.set_text(&updated);
            preserve_space_if_needed(element, &updated);
            true
        }
        None => false,
    }
}

/// Removes a stretch of text from one paragraph.
pub fn delete_range(root: &mut Element, paragraph_index: usize, start: usize, end: usize) -> bool {
    if end <= start {
        return false;
    }
    let Some(path) = paragraph_path(root, paragraph_index) else {
        return false;
    };
    let Some(paragraph) = element_at_path_mut(root, &path) else {
        return false;
    };

    let pieces = collect_text_pieces(paragraph);
    let mut changed = false;

    for piece in &pieces {
        let piece_end = piece.start + piece.text.len();
        if piece_end <= start || piece.start >= end {
            continue;
        }

        let from = start.saturating_sub(piece.start).min(piece.text.len());
        let to = end.saturating_sub(piece.start).min(piece.text.len());
        if from >= to || !piece.text.is_char_boundary(from) || !piece.text.is_char_boundary(to) {
            continue;
        }

        let mut updated = piece.text.clone();
        updated.replace_range(from..to, "");

        if let Some(element) = element_at_path_mut(paragraph, &piece.path) {
            element.set_text(&updated);
            preserve_space_if_needed(element, &updated);
            changed = true;
        }
    }

    changed
}

/// Splits a paragraph in two, as pressing Enter does.
///
/// The new paragraph keeps the original's properties, and a run cut in the
/// middle keeps its formatting on both sides — otherwise pressing Enter in the
/// middle of a bold word would leave half of it unstyled.
pub fn split_paragraph(root: &mut Element, at: TextPosition, prefix: Option<&str>) -> bool {
    let Some(path) = paragraph_path(root, at.paragraph) else {
        return false;
    };
    let Some(paragraph) = element_at_path_mut(root, &path) else {
        return false;
    };

    let mut tail = Element::new(&name_with(prefix, "p"), Some(W));
    if let Some(properties) = paragraph.child(Some(W), "pPr") {
        tail.push_element(properties.clone());
    }

    // Where each child run sits in the paragraph's text.
    let mut spans = Vec::with_capacity(paragraph.children.len());
    let mut running = 0usize;
    for node in &paragraph.children {
        let length = match node.as_element() {
            Some(element) if element.is(Some(W), "r") => paragraph_text(element).len(),
            _ => 0,
        };
        spans.push((running, running + length));
        running += length;
    }

    // Examined from the end, so removing one run does not move the others.
    let mut moved: Vec<Element> = Vec::new();
    for index in (0..paragraph.children.len()).rev() {
        let is_run = paragraph.children[index]
            .as_element()
            .is_some_and(|element| element.is(Some(W), "r"));
        if !is_run {
            continue;
        }
        let (run_start, run_end) = spans[index];

        if run_start >= at.offset {
            if let Node::Element(run) = paragraph.children.remove(index) {
                moved.push(run);
            }
        } else if run_end > at.offset {
            // Cut in the middle: copy the run, then keep opposite halves.
            let local = at.offset - run_start;
            let Some(original) = paragraph.children[index].as_element() else { continue };
            let mut copy = original.clone();
            truncate_run(&mut copy, local, true);

            if let Some(run) = paragraph.children[index].as_element_mut() {
                truncate_run(run, local, false);
            }
            moved.push(copy);
        }
    }

    moved.reverse();
    for run in moved {
        tail.push_element(run);
    }

    let Some((position, parent_path)) = path.split_last() else {
        return false;
    };
    let Some(parent) = element_at_path_mut(root, parent_path) else {
        return false;
    };
    parent.insert_element(position + 1, tail);
    true
}

/// Keeps either the first `offset` bytes of a run, or everything after them.
fn truncate_run(run: &mut Element, offset: usize, keep_tail: bool) {
    for piece in collect_text_pieces(run) {
        let piece_end = piece.start + piece.text.len();
        let local = offset.saturating_sub(piece.start).min(piece.text.len());
        if !piece.text.is_char_boundary(local) {
            continue;
        }

        let updated = if keep_tail {
            if piece_end <= offset {
                String::new()
            } else {
                piece.text[local..].to_owned()
            }
        } else if piece.start >= offset {
            String::new()
        } else {
            piece.text[..local].to_owned()
        };

        if let Some(element) = element_at_path_mut(run, &piece.path) {
            element.set_text(&updated);
            preserve_space_if_needed(element, &updated);
        }
    }
}

/// Joins a paragraph onto the one before it, as Backspace at the start does.
///
/// Refused across a table cell boundary: pressing Backspace should not move
/// text from one cell into another.
pub fn merge_with_previous(root: &mut Element, paragraph_index: usize) -> bool {
    if paragraph_index == 0 {
        return false;
    }
    let Some(path) = paragraph_path(root, paragraph_index) else {
        return false;
    };
    let Some(previous_path) = paragraph_path(root, paragraph_index - 1) else {
        return false;
    };
    if path.len() != previous_path.len()
        || path[..path.len() - 1] != previous_path[..previous_path.len() - 1]
    {
        return false;
    }

    let Some((position, parent_path)) = path.split_last() else {
        return false;
    };
    let parent_path = parent_path.to_vec();
    let position = *position;

    let Some(parent) = element_at_path_mut(root, &parent_path) else {
        return false;
    };
    let Node::Element(removed) = parent.children.remove(position) else {
        return false;
    };

    let runs: Vec<Element> = removed
        .children
        .into_iter()
        .filter_map(|node| match node {
            Node::Element(element) if element.is(Some(W), "r") => Some(element),
            _ => None,
        })
        .collect();

    let Some(previous) = element_at_path_mut(root, &previous_path) else {
        return false;
    };
    for run in runs {
        previous.push_element(run);
    }
    true
}

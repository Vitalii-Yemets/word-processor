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
    collect_text_pieces, element_at_path_mut, name_with, preserve_space_if_needed, TextPiece,
    XML_NAMESPACE,
};
use crate::model::BreakKind;
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

/// Every paragraph of the document, in reading order.
///
/// One walk. Asking for them one at a time means walking from the root once
/// per paragraph, and a loop over all of them then costs the square of the
/// document's length — which on a thousand pages is seconds rather than
/// milliseconds.
#[must_use]
pub fn paragraphs(root: &Element) -> Vec<&Element> {
    let mut found = Vec::new();
    gather_paragraphs(root, &mut found);
    found
}

fn gather_paragraphs<'a>(element: &'a Element, found: &mut Vec<&'a Element>) {
    for child in element.child_elements() {
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        if child.is(Some(W), "p") {
            found.push(child);
        } else {
            gather_paragraphs(child, found);
        }
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
    // Only text can be typed into: a tab or a break is an element, so text
    // landing beside one goes into the nearest piece that can hold it.
    let target = pieces
        .iter()
        // Neither a tab nor the answer of a field is text a person types into.
        .filter(|piece| !piece.atomic && !piece.in_field)
        .find(|piece| at.offset >= piece.start && at.offset <= piece.start + piece.text.len());
    let Some(target) = target else {
        // Beside a field, the text goes outside it rather than into its
        // answer.
        if insert_beside_field(paragraph, &pieces, at.offset, text, prefix) {
            return true;
        }
        // The offset is beside a tab or a break rather than inside any text, so
        // the new text needs a place of its own next to it. Falling back to the
        // nearest text element instead would put the text on the wrong side.
        return insert_beside_atomic(paragraph, &pieces, at.offset, text, prefix);
    };

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

/// Puts text beside a field rather than inside it.
///
/// A field's text is the answer something worked out, and it is replaced whole
/// the next time anything works the field out — so text typed at the end of a
/// page number belongs *after* the number, in a run of its own, not inside the
/// field where it would be thrown away.
fn insert_beside_field(
    paragraph: &mut Element,
    pieces: &[TextPiece],
    offset: usize,
    text: &str,
    prefix: Option<&str>,
) -> bool {
    let Some(piece) = pieces.iter().find(|piece| {
        piece.in_field && offset >= piece.start && offset <= piece.start + piece.text.len()
    }) else {
        return false;
    };
    let after = offset >= piece.start + piece.text.len();

    // Where the field itself is: the shortest prefix of the piece's path that
    // names a `fldSimple`. A field can sit inside a hyperlink, so the depth is
    // not fixed.
    let mut field_depth = None;
    for depth in 1..=piece.path.len() {
        let at = element_at_path_mut(paragraph, &piece.path[..depth]);
        if at.is_some_and(|element| element.is(Some(W), "fldSimple")) {
            field_depth = Some(depth);
            break;
        }
    }
    let Some(depth) = field_depth else { return false };

    let index = piece.path[depth - 1];
    let parent_path = &piece.path[..depth - 1];
    let Some(parent) = element_at_path_mut(paragraph, parent_path) else { return false };

    let mut run = Element::new(&name_with(prefix, "r"), Some(W));
    let mut node = Element::new(&name_with(prefix, "t"), Some(W));
    node.set_text(text);
    node.set_namespaced_attribute("xml:space", XML_NAMESPACE, "preserve");
    run.push_element(node);

    parent.insert_element(if after { index + 1 } else { index }, run);
    true
}

/// Puts text into a paragraph beside a tab or a break rather than inside text.
///
/// Reached when no text element covers the offset — the caret is sitting
/// against a tab. The new text goes on the side of it the offset names, so
/// typing after a tab lands after the tab and not before it.
fn insert_beside_atomic(
    paragraph: &mut Element,
    pieces: &[TextPiece],
    offset: usize,
    text: &str,
    prefix: Option<&str>,
) -> bool {
    // The first element at or after the offset is what the new text goes before.
    let before = pieces.iter().find(|piece| piece.start >= offset);
    let (path, after) = match before {
        Some(piece) => (piece.path.clone(), false),
        None => match pieces.last() {
            Some(piece) => (piece.path.clone(), true),
            None => return false,
        },
    };

    let Some((index, parent_path)) = path.split_last() else { return false };
    let position = index + usize::from(after);
    let parent_path = parent_path.to_vec();
    let Some(parent) = element_at_path_mut(paragraph, &parent_path) else {
        return false;
    };

    let mut node = Element::new(&name_with(prefix, "t"), Some(W));
    node.set_text(text);
    node.set_namespaced_attribute("xml:space", XML_NAMESPACE, "preserve");
    parent.insert_element(position, node);
    true
}

/// Inserts a break element at a position, as a page break is.
///
/// Like a tab, the break goes inside the run the offset falls in, so it takes
/// that run's formatting and does not cut the paragraph into pieces.
pub fn insert_break(
    root: &mut Element,
    at: TextPosition,
    kind: BreakKind,
    prefix: Option<&str>,
) -> bool {
    let mut element = Element::new(&name_with(prefix, "br"), Some(W));
    match kind {
        BreakKind::Line => {}
        BreakKind::Page => element.set_namespaced_attribute(&name_with(prefix, "type"), W, "page"),
        BreakKind::Column => {
            element.set_namespaced_attribute(&name_with(prefix, "type"), W, "column");
        }
    }
    insert_atomic(root, at, element, prefix)
}

/// Inserts an element that stands for one character, such as a drawing.
///
/// Public because a picture is built above this layer — it needs a package part
/// and a relationship, which this module knows nothing about — but has to be
/// put into the text the same way a tab is.
pub fn insert_element_at(
    root: &mut Element,
    at: TextPosition,
    element: Element,
    prefix: Option<&str>,
) -> bool {
    insert_atomic(root, at, element, prefix)
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
    let mut removed: Vec<Vec<usize>> = Vec::new();

    for piece in &pieces {
        let piece_end = piece.start + piece.text.len();
        if piece_end <= start || piece.start >= end {
            continue;
        }

        // A tab or a break is one character that is either deleted whole or not
        // at all: there is no half of it to keep.
        if piece.atomic {
            if piece.start >= start && piece_end <= end {
                removed.push(piece.path.clone());
                changed = true;
            }
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

    // From the back, so removing one element does not move the others.
    for path in removed.iter().rev() {
        let Some((index, parent_path)) = path.split_last() else { continue };
        let index = *index;
        let parent_path = parent_path.to_vec();
        if let Some(parent) = element_at_path_mut(paragraph, &parent_path) {
            if index < parent.children.len() {
                parent.children.remove(index);
            }
        }
    }

    changed
}

/// Inserts a tab element at a position.
///
/// The tab goes inside the run the offset falls in, so it takes that run's
/// formatting rather than starting a new one — which is what a tab typed in the
/// middle of a bold sentence should do.
pub fn insert_tab(root: &mut Element, at: TextPosition, prefix: Option<&str>) -> bool {
    insert_atomic(root, at, Element::new(&name_with(prefix, "tab"), Some(W)), prefix)
}

/// Inserts an element that stands for one character of the text.
///
/// A tab and a break are both written this way: inside the run the offset
/// falls in, splitting its text only when the offset lands in the middle of
/// some.
fn insert_atomic(
    root: &mut Element,
    at: TextPosition,
    element: Element,
    prefix: Option<&str>,
) -> bool {
    let Some(path) = paragraph_path(root, at.paragraph) else {
        return false;
    };
    let Some(paragraph) = element_at_path_mut(root, &path) else {
        return false;
    };

    let pieces = collect_text_pieces(paragraph);

    // An empty paragraph has nowhere to put it, so it gets a run.
    if pieces.is_empty() {
        let mut run = Element::new(&name_with(prefix, "r"), Some(W));
        run.push_element(element);
        let position = usize::from(paragraph.child(Some(W), "pPr").is_some());
        paragraph.insert_element(position, run);
        return true;
    }

    let target = pieces
        .iter()
        .find(|piece| at.offset >= piece.start && at.offset <= piece.start + piece.text.len())
        .or_else(|| pieces.last());
    let Some(target) = target else { return false };

    let local = at.offset.saturating_sub(target.start).min(target.text.len());
    let Some((index, parent_path)) = target.path.split_last() else {
        return false;
    };
    let index = *index;
    let parent_path = parent_path.to_vec();

    // Splitting the text element in two only when the tab lands inside it.
    let tail = if target.atomic || local == 0 || local == target.text.len() {
        None
    } else {
        if !target.text.is_char_boundary(local) {
            return false;
        }
        Some((target.text[..local].to_owned(), target.text[local..].to_owned()))
    };
    let after = tail.is_some() || local > 0;

    let Some(parent) = element_at_path_mut(paragraph, &parent_path) else {
        return false;
    };

    if let Some((head, rest)) = tail {
        if let Some(element) = parent.children.get_mut(index).and_then(Node::as_element_mut) {
            element.set_text(&head);
            preserve_space_if_needed(element, &head);
        }
        let mut node = Element::new(&name_with(prefix, "t"), Some(W));
        node.set_text(&rest);
        node.set_namespaced_attribute("xml:space", XML_NAMESPACE, "preserve");
        parent.insert_element(index + 1, node);
    }

    parent.insert_element(index + usize::from(after), element);
    true
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
        let is_run =
            paragraph.children[index].as_element().is_some_and(|element| element.is(Some(W), "r"));
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
            let Some(original) = paragraph.children[index].as_element() else {
                continue;
            };
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
    let mut removed: Vec<Vec<usize>> = Vec::new();

    for piece in collect_text_pieces(run) {
        let piece_end = piece.start + piece.text.len();

        // A tab or a break belongs wholly to one side of the cut.
        if piece.atomic {
            let goes_to_tail = piece.start >= offset;
            if goes_to_tail != keep_tail {
                removed.push(piece.path.clone());
            }
            continue;
        }

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

    // From the back, so removing one element does not move the others.
    for path in removed.iter().rev() {
        let Some((index, parent_path)) = path.split_last() else { continue };
        let index = *index;
        let parent_path = parent_path.to_vec();
        if let Some(parent) = element_at_path_mut(run, &parent_path) {
            if index < parent.children.len() {
                parent.children.remove(index);
            }
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

/// Removes a paragraph outright.
///
/// Used when a selection covers a whole paragraph: its text is not deleted so
/// much as the paragraph itself stops existing.
pub fn remove_paragraph(root: &mut Element, index: usize) -> bool {
    let Some(path) = paragraph_path(root, index) else {
        return false;
    };
    let Some((position, parent_path)) = path.split_last() else {
        return false;
    };
    let parent_path = parent_path.to_vec();
    let position = *position;

    let Some(parent) = element_at_path_mut(root, &parent_path) else {
        return false;
    };
    if position >= parent.children.len() {
        return false;
    }
    parent.children.remove(position);
    true
}

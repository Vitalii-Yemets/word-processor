//! The content controls that hold paragraphs rather than words: the
//! repeating section, and the building-block gallery.
//!
//! # Two shapes of control
//!
//! [`crate::controls`] is the control written inside a paragraph, round a
//! few words. This is the other shape: a `w:sdt` among the paragraphs of
//! the body, with paragraphs inside it. The reader has always stepped
//! through one to the paragraphs in it — see [`crate::read`] — so a document
//! that carries one lays out; what is here is knowing it is there, and
//! doing the two things Word does with them.
//!
//! # The repeating section
//!
//! ```text
//! <w:sdt><w:sdtPr><w15:repeatingSection/></w:sdtPr><w:sdtContent>
//!   <w:sdt><w:sdtPr><w15:repeatingSectionItem/></w:sdtPr><w:sdtContent>
//!     <w:p>…</w:p>
//!   </w:sdtContent></w:sdt>
//! </w:sdtContent></w:sdt>
//! ```
//!
//! A section is a list of items, each an `sdt` of its own, and repeating is
//! copying an item: Word's plus at the corner of an item puts a copy of it
//! after it, content and all, and its menu takes one away. Both are in a
//! namespace Word added in 2013, written with its own declaration on the
//! element that uses it, as the tick box in [`crate::controls`] is.
//!
//! # The building-block gallery
//!
//! `w:docPartList` in the properties, naming a gallery and a category; the
//! control offers what is filed there and holds whichever was chosen. The
//! blocks are [`crate::blocks`]'s, in the template they live in, and the
//! choosing is the program's; here the control is put in, holding the words
//! that ask for a choice, and filled with the paragraphs of the block.

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::model::Block;
use crate::{edit, position, read, Document, TextPosition};

/// The namespace Word added in 2013, which is where a repeating section
/// lives.
pub const W15: &str = "http://schemas.microsoft.com/office/word/2012/wordml";

/// What a block-level control is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// The section that repeats, which holds the items.
    RepeatingSection,
    /// One item of it.
    RepeatingItem,
    /// A control that offers a gallery of building blocks.
    Gallery { gallery: String, category: String },
    /// Any other control round paragraphs: a rich text control at block
    /// level, which Word makes when one is put round a whole paragraph.
    Other,
}

/// One block-level control, and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockControl {
    pub kind: BlockKind,
    pub alias: String,
    pub tag: String,
    /// The paragraphs it holds, by number: the first and the last.
    pub first: usize,
    pub last: usize,
    /// The way down from the root to the `w:sdt`, as the indices of the
    /// children to step through.
    pub path: Vec<usize>,
    /// Whether it is showing its placeholder rather than an answer.
    pub placeholder: bool,
}

impl BlockControl {
    /// Whether a paragraph is one of its.
    #[must_use]
    pub fn holds(&self, paragraph: usize) -> bool {
        paragraph >= self.first && paragraph <= self.last
    }
}

/// The words a gallery control shows before anything is chosen.
pub const CHOOSE_A_BLOCK: &str = "Choose a building block.";

impl Document {
    /// Every block-level control, outer before inner, in document order.
    #[must_use]
    pub fn block_controls(&self) -> Vec<BlockControl> {
        let mut found = Vec::new();
        let mut counter = 0usize;
        let mut path = Vec::new();
        walk(&self.tree().root, &mut path, &mut counter, &mut found);
        found
    }

    /// The innermost block-level control a paragraph is in.
    #[must_use]
    pub fn block_control_at(&self, paragraph: usize) -> Option<BlockControl> {
        self.block_controls().into_iter().filter(|control| control.holds(paragraph)).last()
    }

    /// The repeating-section item a paragraph is in, if it is in one.
    #[must_use]
    pub fn repeating_item_at(&self, paragraph: usize) -> Option<BlockControl> {
        self.block_controls()
            .into_iter()
            .filter(|control| control.kind == BlockKind::RepeatingItem && control.holds(paragraph))
            .last()
    }

    /// Puts a repeating section round the paragraphs the selection covers,
    /// or round the paragraph the caret is in, as one item.
    pub fn insert_repeating_section(&mut self) -> bool {
        let (start, end) = self.selection().unwrap_or_else(|| (self.caret(), self.caret()));
        let (first, last) = (start.paragraph, end.paragraph.max(start.paragraph));
        // The paragraphs have to be siblings: a section round half a table,
        // or round a paragraph inside another control and one outside it,
        // is not something the format can say.
        let Some(first_path) = position::paragraph_path(&self.tree().root, first) else {
            return false;
        };
        let Some(last_path) = position::paragraph_path(&self.tree().root, last) else {
            return false;
        };
        if first_path.len() != last_path.len()
            || first_path[..first_path.len() - 1] != last_path[..last_path.len() - 1]
        {
            return false;
        }
        self.record(EditKind::Structural, self.caret(), false);
        let prefix = self.prefix();
        let named = |local: &str| edit::name_with(prefix.as_deref(), local);

        let parent_path = &first_path[..first_path.len() - 1];
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, parent_path) else {
            return false;
        };
        let from = first_path[first_path.len() - 1];
        let to = last_path[last_path.len() - 1];
        let taken: Vec<Node> = parent.children.drain(from..=to).collect();

        let mut item_content = Element::new(&named("sdtContent"), Some(read::W));
        item_content.children = taken;
        let mut item = Element::new(&named("sdt"), Some(read::W));
        item.push_element(properties_with(&named("sdtPr"), "w15:repeatingSectionItem"));
        item.push_element(item_content);

        let mut content = Element::new(&named("sdtContent"), Some(read::W));
        content.push_element(item);
        let mut section = Element::new(&named("sdt"), Some(read::W));
        section.push_element(properties_with(&named("sdtPr"), "w15:repeatingSection"));
        section.push_element(content);
        parent.insert_element(from, section);

        self.mark_modified();
        true
    }

    /// Puts a copy of the item a paragraph is in after it — or before it —
    /// content and all, which is what Word's plus does.
    pub fn add_repeating_item(&mut self, paragraph: usize, after: bool) -> bool {
        let Some(item) = self.repeating_item_at(paragraph) else { return false };
        self.record(EditKind::Structural, self.caret(), false);
        let (parent_path, at) = item.path.split_at(item.path.len() - 1);
        let at = at[0];
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, parent_path) else {
            return false;
        };
        let Some(copy) = parent.children.get(at).and_then(Node::as_element).cloned() else {
            return false;
        };
        parent.insert_element(if after { at + 1 } else { at }, copy);
        // The caret goes to the start of the new item, which is where a
        // person who pressed the plus wants to type.
        let new_first = if after { item.last + 1 } else { item.first };
        self.set_caret(TextPosition::new(new_first, 0));
        self.mark_modified();
        true
    }

    /// Takes the item a paragraph is in out of its section, unless it is
    /// the only one: a section with nothing in it is a section nobody can
    /// find to add to, and Word keeps the last item too.
    pub fn remove_repeating_item(&mut self, paragraph: usize) -> bool {
        let Some(item) = self.repeating_item_at(paragraph) else { return false };
        let siblings = self
            .block_controls()
            .into_iter()
            .filter(|other| {
                other.kind == BlockKind::RepeatingItem
                    && other.path.len() == item.path.len()
                    && other.path[..other.path.len() - 1] == item.path[..item.path.len() - 1]
            })
            .count();
        if siblings <= 1 {
            return false;
        }
        self.record(EditKind::Structural, self.caret(), false);
        let (parent_path, at) = item.path.split_at(item.path.len() - 1);
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, parent_path) else {
            return false;
        };
        if at[0] >= parent.children.len() {
            return false;
        }
        parent.children.remove(at[0]);
        // The caret stays in the section: in the item that took the
        // removed one's place, or the one before it when the removed one
        // was last.
        let next = item.first.min(self.paragraph_count() - 1);
        let landing =
            if self.repeating_item_at(next).is_some() { next } else { next.saturating_sub(1) };
        self.set_caret(TextPosition::new(landing, 0));
        self.mark_modified();
        true
    }

    /// Puts a building-block gallery control in: round the paragraph the
    /// caret is in where that paragraph is empty, and after it otherwise,
    /// holding the words that ask for a choice.
    pub fn insert_gallery_control(&mut self, gallery: &str, category: &str) -> bool {
        let caret = self.caret();
        let Some(path) = position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let named = |local: &str| edit::name_with(prefix.as_deref(), local);
        let empty = self.paragraph_text(caret.paragraph).is_none_or(|text| text.is_empty());

        let mut properties = Element::new(&named("sdtPr"), Some(read::W));
        properties.push_element(Element::new(&named("showingPlcHdr"), Some(read::W)));
        let mut list = Element::new(&named("docPartList"), Some(read::W));
        for (local, value) in [("docPartGallery", gallery), ("docPartCategory", category)] {
            let mut element = Element::new(&named(local), Some(read::W));
            element.set_namespaced_attribute(&named("val"), read::W, value);
            list.push_element(element);
        }
        properties.push_element(list);

        let mut paragraph = Element::new(&named("p"), Some(read::W));
        let mut run = Element::new(&named("r"), Some(read::W));
        let mut text = Element::new(&named("t"), Some(read::W));
        text.set_text(CHOOSE_A_BLOCK);
        run.push_element(text);
        paragraph.push_element(run);

        let mut content = Element::new(&named("sdtContent"), Some(read::W));
        content.push_element(paragraph);
        let mut control = Element::new(&named("sdt"), Some(read::W));
        control.push_element(properties);
        control.push_element(content);

        let (parent_path, at) = path.split_at(path.len() - 1);
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, parent_path) else {
            return false;
        };
        let at = at[0];
        let put_at = if empty {
            // The empty paragraph gives way to the control's own.
            parent.children.remove(at);
            at
        } else {
            at + 1
        };
        parent.insert_element(put_at, control);
        let new_paragraph = if empty { caret.paragraph } else { caret.paragraph + 1 };
        self.set_caret(TextPosition::new(new_paragraph, 0));
        self.mark_modified();
        true
    }

    /// Puts a block's paragraphs into the gallery control a paragraph is
    /// in, in place of whatever it held.
    pub fn fill_gallery_control(&mut self, paragraph: usize, blocks: &[Block]) -> bool {
        let Some(control) = self.block_control_at(paragraph) else { return false };
        if !matches!(control.kind, BlockKind::Gallery { .. }) {
            return false;
        }
        let end = self.paragraph_text(control.last).map_or(0, |text| text.len());
        self.set_selections(&[(
            TextPosition::new(control.first, 0),
            TextPosition::new(control.last, end),
        )]);
        if self.paste_blocks(blocks) {
            // A block chosen is an answer, so the control stops asking.
            if let Some(properties) =
                edit::element_at_path_mut(&mut self.tree_mut().root, &control.path)
                    .and_then(|sdt| sdt.child_mut(Some(read::W), "sdtPr"))
            {
                properties.remove_children_named(Some(read::W), "showingPlcHdr");
            }
            return true;
        }
        self.set_caret(TextPosition::new(control.first, 0));
        false
    }
}

/// `w:sdtPr` holding one element of the 2013 namespace, declared on it.
fn properties_with(name: &str, w15_element: &str) -> Element {
    let mut properties = Element::new(name, Some(read::W));
    let mut marker = Element::new(w15_element, Some(W15));
    marker.declarations.push((Some("w15".to_owned()), W15.to_owned()));
    properties.push_element(marker);
    properties
}

/// Walks the tree, numbering paragraphs and noting every `w:sdt` that holds
/// paragraphs rather than runs.
fn walk(
    element: &Element,
    path: &mut Vec<usize>,
    counter: &mut usize,
    found: &mut Vec<BlockControl>,
) {
    for (index, node) in element.children.iter().enumerate() {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(read::W) {
            continue;
        }
        path.push(index);
        match child.local_name() {
            "p" => *counter += 1,
            "sdt" if holds_blocks(child) => {
                let first = *counter;
                let place = found.len();
                found.push(BlockControl {
                    kind: kind_of(child.child(Some(read::W), "sdtPr")),
                    alias: said(child.child(Some(read::W), "sdtPr"), "alias"),
                    tag: said(child.child(Some(read::W), "sdtPr"), "tag"),
                    first,
                    last: first,
                    path: path.clone(),
                    placeholder: read::showing_placeholder(child),
                });
                if let Some(content) = child.child(Some(read::W), "sdtContent") {
                    path.push(child.position_of(Some(read::W), "sdtContent").unwrap_or(0));
                    walk(content, path, counter, found);
                    path.pop();
                }
                // A control holding no paragraph at all covers none.
                found[place].last = counter.saturating_sub(1).max(first);
            }
            _ => walk(child, path, counter, found),
        }
        path.pop();
    }
}

/// Whether an `sdt` holds paragraphs: its content has a paragraph, a table
/// or another such control in it, rather than runs.
fn holds_blocks(control: &Element) -> bool {
    control.child(Some(read::W), "sdtContent").is_some_and(|content| {
        content.child_elements().any(|child| {
            child.is(Some(read::W), "p")
                || child.is(Some(read::W), "tbl")
                || (child.is(Some(read::W), "sdt") && holds_blocks(child))
        })
    })
}

/// Which kind the properties name.
fn kind_of(properties: Option<&Element>) -> BlockKind {
    let Some(properties) = properties else { return BlockKind::Other };
    if properties.child(Some(W15), "repeatingSection").is_some() {
        return BlockKind::RepeatingSection;
    }
    if properties.child(Some(W15), "repeatingSectionItem").is_some() {
        return BlockKind::RepeatingItem;
    }
    if let Some(list) = properties.child(Some(read::W), "docPartList") {
        let value = |local: &str| {
            list.child(Some(read::W), local)
                .and_then(|child| child.attribute(Some(read::W), "val"))
                .unwrap_or_default()
                .to_owned()
        };
        return BlockKind::Gallery {
            gallery: value("docPartGallery"),
            category: value("docPartCategory"),
        };
    }
    BlockKind::Other
}

/// What a property says, by its `w:val`.
fn said(properties: Option<&Element>, local: &str) -> String {
    properties
        .and_then(|properties| properties.child(Some(read::W), local))
        .and_then(|child| child.attribute(Some(read::W), "val"))
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Body, Paragraph};

    fn document(lines: &[&str]) -> Document {
        let mut body = Body::default();
        for line in lines {
            body.blocks.push(Block::Paragraph(Paragraph::text(line)));
        }
        Document::create(&body).expect("a document")
    }

    #[test]
    fn a_repeating_section_is_put_round_the_selection_as_one_item() {
        let mut document = document(&["Name", "Street", "Town", "After"]);
        document.set_selections(&[(TextPosition::new(1, 0), TextPosition::new(2, 4))]);
        assert!(document.insert_repeating_section());

        let controls = document.block_controls();
        assert_eq!(controls.len(), 2, "{controls:?}");
        assert_eq!(controls[0].kind, BlockKind::RepeatingSection);
        assert_eq!((controls[0].first, controls[0].last), (1, 2));
        assert_eq!(controls[1].kind, BlockKind::RepeatingItem);
        assert_eq!((controls[1].first, controls[1].last), (1, 2));
        assert_eq!(document.paragraph_count(), 4);
        assert_eq!(document.plain_text(), "Name\nStreet\nTown\nAfter");

        // And the file says so the way Word says it.
        let again = Document::open(&document.save().expect("saving")).expect("reopening");
        let xml = again.package().xml_part("word/document.xml").expect("the part").expect("xml");
        assert!(xml.contains("<w15:repeatingSection"), "{xml}");
        assert!(xml.contains("xmlns:w15=\"http://schemas.microsoft.com/office/word/2012/wordml\""));
        assert_eq!(again.block_controls().len(), 2);
    }

    #[test]
    fn an_item_is_repeated_by_copying_it_and_the_last_one_is_kept() {
        let mut document = document(&["Name", "Street", "After"]);
        document.set_selections(&[(TextPosition::new(0, 0), TextPosition::new(1, 6))]);
        assert!(document.insert_repeating_section());

        assert!(document.add_repeating_item(1, true));
        assert_eq!(document.plain_text(), "Name\nStreet\nName\nStreet\nAfter");
        assert_eq!(document.caret(), TextPosition::new(2, 0));
        let items: Vec<(usize, usize)> = document
            .block_controls()
            .iter()
            .filter(|control| control.kind == BlockKind::RepeatingItem)
            .map(|control| (control.first, control.last))
            .collect();
        assert_eq!(items, [(0, 1), (2, 3)]);
        assert_eq!(document.repeating_item_at(3).map(|item| item.first), Some(2));
        assert_eq!(document.repeating_item_at(4), None);

        assert!(document.remove_repeating_item(0));
        assert_eq!(document.plain_text(), "Name\nStreet\nAfter");
        assert!(!document.remove_repeating_item(0), "the last item was taken out");
        assert_eq!(document.block_controls().len(), 2);
    }

    #[test]
    fn a_section_cannot_go_round_paragraphs_that_are_not_side_by_side() {
        let mut document = document(&["Outside", "Inside", "After"]);
        document.set_selections(&[(TextPosition::new(1, 0), TextPosition::new(1, 0))]);
        assert!(document.insert_repeating_section());
        // From outside the section to inside it.
        document.set_selections(&[(TextPosition::new(0, 0), TextPosition::new(1, 0))]);
        assert!(!document.insert_repeating_section());
    }

    #[test]
    fn a_gallery_control_asks_for_a_choice_and_takes_the_block_chosen() {
        let mut document = document(&["Before", ""]);
        document.set_caret(TextPosition::new(1, 0));
        assert!(document.insert_gallery_control("Quick Parts", "General"));
        assert_eq!(document.plain_text(), "Before\nChoose a building block.");
        let control = document.block_control_at(1).expect("the control");
        assert_eq!(
            control.kind,
            BlockKind::Gallery {
                gallery: "Quick Parts".to_owned(),
                category: "General".to_owned()
            }
        );

        let chosen = vec![
            Block::Paragraph(Paragraph::text("Yours faithfully,")),
            Block::Paragraph(Paragraph::text("A. Habgood")),
        ];
        assert!(document.fill_gallery_control(1, &chosen));
        assert_eq!(document.plain_text(), "Before\nYours faithfully,\nA. Habgood");
        let control = document.block_control_at(2).expect("the control, grown");
        assert_eq!((control.first, control.last), (1, 2));

        // Chosen again, the old choice goes.
        assert!(document.fill_gallery_control(2, &[Block::Paragraph(Paragraph::text("Regards"))]));
        assert_eq!(document.plain_text(), "Before\nRegards");

        let again = Document::open(&document.save().expect("saving")).expect("reopening");
        assert!(matches!(
            again.block_control_at(1).map(|c| c.kind),
            Some(BlockKind::Gallery { .. })
        ));
    }

    #[test]
    fn a_gallery_control_after_a_paragraph_with_words_goes_below_it() {
        let mut document = document(&["Before"]);
        document.set_caret(TextPosition::new(0, 3));
        assert!(document.insert_gallery_control("Quick Parts", "General"));
        assert_eq!(document.plain_text(), "Before\nChoose a building block.");
        assert_eq!(document.caret(), TextPosition::new(1, 0));
    }
}

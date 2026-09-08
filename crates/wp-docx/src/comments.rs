//! Comments: the notes people leave on a document without changing it.
//!
//! # Where a comment actually lives
//!
//! In three places at once, and all three are needed:
//!
//! * `word/comments.xml` holds the text of every comment, each with an id.
//! * The document marks the stretch the comment is about with
//!   `w:commentRangeStart` and `w:commentRangeEnd`, both carrying that id.
//! * A run holding `w:commentReference` says where the little mark goes.
//!
//! The anchors take up no room in the text at all — they sit between runs and
//! carry no characters — which is exactly why a comment can be attached to a
//! range without moving a single caret position.

use wp_opc::TargetMode;
use wp_xml::tree::{Element, Node, XmlTree};

use crate::history::EditKind;
use crate::model::Body;
use crate::{edit, format, position, read, Document, Error, TextPosition};

/// Content type of the comments part.
const COMMENTS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";

/// Relationship type of the comments part.
const COMMENTS_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";

/// One comment, and the stretch of text it is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub id: i32,
    pub author: String,
    pub initials: String,
    /// The date as the file records it, which is an ISO 8601 timestamp.
    pub date: String,
    /// What the comment says, with its paragraphs joined by newlines.
    pub text: String,
    /// Where the commented stretch begins and ends.
    ///
    /// `None` when the document has a comment in the part but no anchor for it,
    /// which happens in files other programs have edited.
    pub range: Option<(TextPosition, TextPosition)>,
}

impl Document {
    /// Every comment on the document, in the order their anchors appear.
    #[must_use]
    pub fn comments(&self) -> Vec<Comment> {
        let Some(part) = self.comments_part() else { return Vec::new() };
        let Some(Ok(text)) = self.package().xml_part(&part) else { return Vec::new() };
        let Ok(tree) = XmlTree::parse(&text) else { return Vec::new() };

        let ranges = self.comment_ranges();
        let mut out: Vec<Comment> = tree
            .root
            .children_named(Some(read::W), "comment")
            .filter_map(|element| {
                let id = element.attribute(Some(read::W), "id")?.parse().ok()?;
                let body = read::read_part(element);
                Some(Comment {
                    id,
                    author: attribute(element, "author"),
                    initials: attribute(element, "initials"),
                    date: attribute(element, "date"),
                    text: body.plain_text(),
                    range: ranges.get(&id).copied(),
                })
            })
            .collect();

        // In the order the reader meets them, which is the order they should be
        // listed and stepped through.
        out.sort_by_key(|comment| {
            comment
                .range
                .map_or((usize::MAX, usize::MAX), |(start, _)| (start.paragraph, start.offset))
        });
        out
    }

    /// The name of the part holding the comments, if there is one.
    #[must_use]
    pub fn comments_part(&self) -> Option<String> {
        let relationships = self.package().relationships(self.main_part()).ok()?;
        let relationship = relationships.single_by_type(COMMENTS_RELATIONSHIP)?;
        relationship.resolved_target(self.main_part())?.ok()
    }

    /// Where every comment's anchors sit in the text.
    fn comment_ranges(&self) -> std::collections::HashMap<i32, (TextPosition, TextPosition)> {
        let mut starts = std::collections::HashMap::new();
        let mut ends = std::collections::HashMap::new();

        for index in 0..self.paragraph_count() {
            let Some(paragraph) = self.paragraph_element(index) else { continue };
            for (id, offset, is_start) in anchors_in(paragraph) {
                let position = TextPosition::new(index, offset);
                if is_start {
                    starts.entry(id).or_insert(position);
                } else {
                    ends.insert(id, position);
                }
            }
        }

        starts
            .into_iter()
            .filter_map(|(id, start)| ends.get(&id).map(|end| (id, (start, *end))))
            .collect()
    }

    /// Attaches a comment to the selection, or to the word at the caret.
    ///
    /// Returns the id it was given, which is what a later delete needs.
    pub fn add_comment(&mut self, text: &str, author: &str, date: &str) -> Result<i32, Error> {
        let (start, end) = match self.selection() {
            Some(range) => range,
            // Word anchors a comment on nothing at all rather than refusing,
            // and shows it against the caret. Same here.
            None => (self.caret(), self.caret()),
        };
        if start.paragraph != end.paragraph {
            // A range across paragraphs needs an anchor in each, which is more
            // than this does; the comment goes on the first paragraph's part.
            return self.add_comment_to(
                text,
                author,
                date,
                start,
                TextPosition::new(
                    start.paragraph,
                    self.paragraph_text(start.paragraph).map_or(start.offset, |line| line.len()),
                ),
            );
        }
        self.add_comment_to(text, author, date, start, end)
    }

    fn add_comment_to(
        &mut self,
        text: &str,
        author: &str,
        date: &str,
        start: TextPosition,
        end: TextPosition,
    ) -> Result<i32, Error> {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let id = self.comments().iter().map(|comment| comment.id).max().unwrap_or(-1) + 1;
        let prefix = self.prefix();

        // The anchors go into the paragraph in reverse order, so the first one
        // does not shift the place the second one has to go.
        self.insert_anchor(start.paragraph, end.offset, id, false, prefix.as_deref());
        self.insert_anchor(start.paragraph, start.offset, id, true, prefix.as_deref());

        self.write_comment_part(id, text, author, date)?;
        self.mark_modified();
        Ok(id)
    }

    /// Puts one anchor into a paragraph at a text offset.
    fn insert_anchor(
        &mut self,
        paragraph_index: usize,
        offset: usize,
        id: i32,
        start: bool,
        prefix: Option<&str>,
    ) {
        let Some(path) = position::paragraph_path(&self.tree().root, paragraph_index) else {
            return;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return;
        };

        // A run has to be cut in two where the anchor goes, or the anchor could
        // only ever land between runs.
        format::split_runs_at_offset(paragraph, offset);

        let local = if start { "commentRangeStart" } else { "commentRangeEnd" };
        let mut anchor = Element::new(&edit::name_with(prefix, local), Some(read::W));
        anchor.set_namespaced_attribute(&edit::name_with(prefix, "id"), read::W, &id.to_string());

        let position = edit::child_position_at_offset(paragraph, offset);
        paragraph.insert_element(position, anchor);

        // The end anchor is followed by the run that carries the mark a reader
        // clicks on. Without it Word shows the comment but not where it is.
        if !start {
            let mut run = Element::new(&edit::name_with(prefix, "r"), Some(read::W));
            let mut properties = Element::new(&edit::name_with(prefix, "rPr"), Some(read::W));
            properties.push_element(Element::new(
                &edit::name_with(prefix, "commentReference"),
                Some(read::W),
            ));
            let mut reference =
                Element::new(&edit::name_with(prefix, "commentReference"), Some(read::W));
            reference.set_namespaced_attribute(
                &edit::name_with(prefix, "id"),
                read::W,
                &id.to_string(),
            );
            let _ = properties;
            run.push_element(reference);
            paragraph.insert_element(position + 1, run);
        }
    }

    /// Takes a comment off the document.
    pub fn delete_comment(&mut self, id: i32) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let mut changed = false;
        for index in 0..self.paragraph_count() {
            let Some(path) = position::paragraph_path(&self.tree().root, index) else { continue };
            let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path)
            else {
                continue;
            };
            changed |= remove_anchors(paragraph, id);
        }

        changed |= self.remove_from_comment_part(id);
        if changed {
            self.mark_modified();
        }
        changed
    }

    /// Adds one comment to the part, making the part if there is not one.
    fn write_comment_part(
        &mut self,
        id: i32,
        text: &str,
        author: &str,
        date: &str,
    ) -> Result<(), Error> {
        let mut root = self.comments_root();

        let mut comment = Element::new("w:comment", Some(read::W));
        comment.set_namespaced_attribute("w:id", read::W, &id.to_string());
        comment.set_namespaced_attribute("w:author", read::W, author);
        comment.set_namespaced_attribute("w:initials", read::W, &initials_of(author));
        comment.set_namespaced_attribute("w:date", read::W, date);

        // One paragraph per line, so a comment can be more than a sentence.
        for line in text.split('\n') {
            let paragraph = crate::model::Paragraph::text(line);
            comment.push_element(edit::paragraph_element(&paragraph, Some("w")));
        }
        root.push_element(comment);

        self.save_comments_root(root)
    }

    /// Removes one comment from the part.
    fn remove_from_comment_part(&mut self, id: i32) -> bool {
        let mut root = self.comments_root();
        let wanted = id.to_string();
        let before = root.children.len();

        root.children.retain(|node| match node {
            Node::Element(element) => {
                !(element.is(Some(read::W), "comment")
                    && element.attribute(Some(read::W), "id") == Some(wanted.as_str()))
            }
            _ => true,
        });

        if root.children.len() == before {
            return false;
        }
        self.save_comments_root(root).is_ok()
    }

    /// The `w:comments` element, read from the part or made afresh.
    fn comments_root(&self) -> Element {
        if let Some(part) = self.comments_part() {
            if let Some(Ok(text)) = self.package().xml_part(&part) {
                if let Ok(tree) = XmlTree::parse(&text) {
                    return tree.root;
                }
            }
        }

        let mut root = Element::new("w:comments", Some(read::W));
        root.declarations.push((Some("w".to_owned()), read::W.to_owned()));
        root
    }

    /// Writes the part back, adding the relationship the first time.
    fn save_comments_root(&mut self, root: Element) -> Result<(), Error> {
        let tree = XmlTree {
            standalone: Some(true),
            has_declaration: true,
            doctype: None,
            before_root: Vec::new(),
            root,
            after_root: Vec::new(),
        };
        let xml = tree
            .to_xml()
            .map_err(|source| Error::Xml { part: "word/comments.xml".to_owned(), source })?;

        let part = self.comments_part().unwrap_or_else(|| "word/comments.xml".to_owned());
        self.package_mut().add_part(&part, COMMENTS_CONTENT_TYPE, xml.into_bytes());

        let main_part = self.main_part().to_owned();
        let mut relationships = self
            .package()
            .relationships(&main_part)
            .unwrap_or_else(|_| wp_opc::Relationships::new(&main_part));
        if relationships.single_by_type(COMMENTS_RELATIONSHIP).is_none() {
            let target = part.strip_prefix("word/").unwrap_or(&part).to_owned();
            relationships.add(COMMENTS_RELATIONSHIP, &target, TargetMode::Internal);
            self.package_mut().set_relationships(&relationships)?;
        }
        Ok(())
    }
}

/// The author's initials, which Word shows on the mark in the text.
fn initials_of(author: &str) -> String {
    author
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .flat_map(char::to_uppercase)
        .take(3)
        .collect()
}

fn attribute(element: &Element, name: &str) -> String {
    element.attribute(Some(read::W), name).unwrap_or_default().to_owned()
}

/// Every comment anchor in a paragraph, with the text offset it sits at.
///
/// Reported as `(id, offset, is_start)`.
fn anchors_in(paragraph: &Element) -> Vec<(i32, usize, bool)> {
    let mut found = Vec::new();
    let mut offset = 0usize;
    walk_anchors(paragraph, &mut offset, &mut found);
    found
}

fn walk_anchors(element: &Element, offset: &mut usize, found: &mut Vec<(i32, usize, bool)>) {
    for node in &element.children {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(read::W) {
            continue;
        }
        // Deleted text is not part of what the document says, so it is not
        // counted — the same rule the rest of the reader follows.
        if child.local_name() == "del" {
            continue;
        }

        match child.local_name() {
            "commentRangeStart" | "commentRangeEnd" => {
                if let Some(id) =
                    child.attribute(Some(read::W), "id").and_then(|text| text.parse().ok())
                {
                    found.push((id, *offset, child.local_name() == "commentRangeStart"));
                }
            }
            "t" => *offset += child.text_content().len(),
            _ => {
                if let Some(text) = edit::atomic_text(child) {
                    *offset += text.len();
                } else {
                    walk_anchors(child, offset, found);
                }
            }
        }
    }
}

/// Takes a comment's anchors and its reference mark out of a paragraph.
fn remove_anchors(paragraph: &mut Element, id: i32) -> bool {
    let wanted = id.to_string();
    let before = paragraph.children.len();

    paragraph.children.retain(|node| {
        let Node::Element(element) = node else { return true };
        if element.namespace.as_deref() != Some(read::W) {
            return true;
        }
        match element.local_name() {
            "commentRangeStart" | "commentRangeEnd" => {
                element.attribute(Some(read::W), "id") != Some(wanted.as_str())
            }
            // The run that carries only the mark goes with them; a run that
            // also holds text stays, because the text is the document's.
            "r" => !is_only_reference(element, &wanted),
            _ => true,
        }
    });

    paragraph.children.len() != before
}

/// Whether a run holds nothing but the mark for one comment.
fn is_only_reference(run: &Element, id: &str) -> bool {
    let mut saw_reference = false;
    for node in &run.children {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(read::W) {
            return false;
        }
        match child.local_name() {
            "rPr" => {}
            "commentReference" => {
                if child.attribute(Some(read::W), "id") != Some(id) {
                    return false;
                }
                saw_reference = true;
            }
            _ => return false,
        }
    }
    saw_reference
}

/// Kept so this module can name what it reads out of a comment part.
const _: fn(&Element) -> Body = read::read_part;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_are_the_first_letters_of_the_names() {
        assert_eq!(initials_of("Ada Lovelace"), "AL");
        assert_eq!(initials_of("ada"), "A");
        assert_eq!(initials_of(""), "");
        assert_eq!(initials_of("One Two Three Four"), "OTT");
    }
}

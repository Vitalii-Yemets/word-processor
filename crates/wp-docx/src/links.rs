//! Hyperlinks.
//!
//! # Two kinds that look the same
//!
//! A link to somewhere else in the document names a bookmark, and carries it in
//! `w:anchor` — the address is right there in the text. A link out of the
//! document names a relationship, and carries only its id in `r:id`; the
//! address itself lives in the relationships part beside the document.
//!
//! The second arrangement is the reason a `.docx` can be checked for what it
//! points at without reading the text, and the reason a link survives being
//! edited: the text can be rewritten to say anything, and it still goes where
//! the relationship says.
//!
//! # What wrapping means
//!
//! `w:hyperlink` is not a property of a run. It is an element that *holds*
//! runs, like a field does, so making a link means moving the runs that were
//! selected inside a new element rather than setting a flag on them.

use wp_opc::TargetMode;
use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::model::Underline;
use crate::{edit, position, read, Document, TextPosition};

/// Relationship type of a link out of the document.
const RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";

/// The character style Word gives a link, and the colour it comes out.
const STYLE: &str = "Hyperlink";

/// The namespace a relationship id is named in.
const R: &str = read::RELATIONSHIPS;
const COLOR: &str = "0563C1";

/// Where a link goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    /// An address outside the document — a web page, a file, an e-mail.
    Address(String),
    /// A bookmark inside it.
    Place(String),
}

impl Destination {
    /// What to show for it when there is nothing to show.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Address(address) | Self::Place(address) => address,
        }
    }

    /// Reads what somebody typed, deciding which kind it is.
    ///
    /// A bare word is a place in the document; anything with a scheme, a dot in
    /// it or an `@` is an address. This is the guess Word makes as you type,
    /// and it is right nearly always.
    #[must_use]
    pub fn parse(typed: &str) -> Self {
        let typed = typed.trim();
        if let Some(place) = typed.strip_prefix('#') {
            return Self::Place(place.to_owned());
        }
        let looks_like_an_address = typed.contains("://")
            || typed.starts_with("mailto:")
            || typed.contains('@')
            || typed.contains('.') && !typed.contains(' ');
        if !looks_like_an_address {
            return Self::Place(typed.to_owned());
        }
        Self::Address(with_scheme(typed))
    }
}

/// One link in the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub destination: Destination,
    /// What a reader sees.
    pub text: String,
    pub paragraph: usize,
    /// Where in the paragraph it starts and ends.
    pub range: (usize, usize),
}

impl Document {
    /// Makes the selection a link, or puts one in when nothing is selected.
    ///
    /// Returns whether anything changed. With nothing selected the address is
    /// typed in as the text, which is what Word does and what a person nearly
    /// always wants.
    pub fn add_hyperlink(&mut self, typed: &str, shown: &str) -> bool {
        let destination = Destination::parse(typed);
        if destination.label().is_empty() {
            return false;
        }

        let caret = self.caret();
        let selection = self.selection();

        // Nothing selected: the words go in first, then they are wrapped.
        if selection.is_none() {
            let text = if shown.trim().is_empty() { destination.label() } else { shown };
            let text = text.to_owned();
            if text.is_empty() {
                return false;
            }
            self.record(EditKind::Structural, caret, false);
            if !self.paste(&text) {
                return false;
            }
            let start = TextPosition::new(caret.paragraph, caret.offset);
            let end = TextPosition::new(caret.paragraph, caret.offset + text.len());
            return self.wrap_as_link(start, end, &destination, false);
        }

        let (start, end) = selection.unwrap_or((caret, caret));
        if start.paragraph != end.paragraph {
            // A link across paragraphs would have to be several links; Word
            // makes one per paragraph, and so does this.
            self.record(EditKind::Structural, caret, false);
            let mut changed = false;
            for index in start.paragraph..=end.paragraph {
                let text = self.paragraph_text(index).unwrap_or_default();
                let from = if index == start.paragraph { start.offset } else { 0 };
                let to = if index == end.paragraph { end.offset } else { text.len() };
                if from < to {
                    changed |= self.wrap_as_link(
                        TextPosition::new(index, from),
                        TextPosition::new(index, to),
                        &destination,
                        false,
                    );
                }
            }
            return changed;
        }

        self.wrap_as_link(start, end, &destination, true)
    }

    /// Takes the link off whatever the caret is in, leaving the words.
    pub fn remove_hyperlink(&mut self) -> bool {
        let caret = self.caret();
        let Some(path) = position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        self.record(EditKind::Structural, caret, false);
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };

        // The link's runs take the place of the link, so not one character of
        // the text moves.
        let mut unwrapped = false;
        let mut children = Vec::with_capacity(paragraph.children.len());
        for node in core::mem::take(&mut paragraph.children) {
            match node {
                Node::Element(element) if is_link(&element) => {
                    unwrapped = true;
                    for child in element.children {
                        children.push(child);
                    }
                }
                other => children.push(other),
            }
        }
        paragraph.children = children;

        if !unwrapped {
            return false;
        }
        self.mark_modified();
        true
    }

    /// Every link in the document, in reading order.
    #[must_use]
    pub fn hyperlinks(&self) -> Vec<Link> {
        let mut out = Vec::new();
        for (index, paragraph) in self.paragraph_elements().into_iter().enumerate() {
            let targets = self.link_targets(index);
            let mut offset = 0usize;
            let mut seen = 0usize;
            walk_links(paragraph, index, &mut offset, &targets, &mut seen, &mut out);
        }
        out
    }

    /// The link the caret is inside, if it is inside one.
    #[must_use]
    pub fn hyperlink_here(&self) -> Option<Link> {
        let caret = self.caret();
        self.hyperlinks().into_iter().find(|link| {
            link.paragraph == caret.paragraph
                && caret.offset >= link.range.0
                && caret.offset <= link.range.1
        })
    }

    /// Wraps a range of one paragraph in a link element.
    ///
    /// `record` says whether to note the change for undo, which the callers
    /// that have already noted one do not want done twice.
    fn wrap_as_link(
        &mut self,
        start: TextPosition,
        end: TextPosition,
        destination: &Destination,
        record: bool,
    ) -> bool {
        if start.offset >= end.offset {
            return false;
        }
        if record {
            self.record(EditKind::Structural, self.caret(), false);
        }

        // An address needs a relationship before the element can name it.
        let id = match destination {
            Destination::Address(address) => match self.link_relationship(address) {
                Some(id) => Some(id),
                None => return false,
            },
            Destination::Place(_) => None,
        };

        let prefix = self.prefix();
        let Some(path) = position::paragraph_path(&self.tree().root, start.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };

        // Split first so the ends of the link fall between runs rather than
        // through the middle of one.
        crate::format::split_runs_at_offset(paragraph, start.offset);
        crate::format::split_runs_at_offset(paragraph, end.offset);
        // Never as far back as the paragraph properties, which sit before every
        // run and belong to the paragraph rather than to anything in it.
        let mut from = edit::child_position_at_offset(paragraph, start.offset);
        while paragraph
            .children
            .get(from)
            .and_then(Node::as_element)
            .is_some_and(|child| child.is(Some(read::W), "pPr"))
        {
            from += 1;
        }
        let to = edit::child_position_at_offset(paragraph, end.offset);
        if from >= to || to > paragraph.children.len() {
            return false;
        }

        let taken: Vec<Node> = paragraph.children.drain(from..to).collect();
        let mut link =
            Element::new(&edit::name_with(prefix.as_deref(), "hyperlink"), Some(read::W));
        match (&id, destination) {
            (Some(id), _) => {
                // The prefix is declared on the element itself, because a
                // document written from nothing has no reason to have declared
                // it on the root before its first link.
                link.declarations.push((Some("r".to_owned()), R.to_owned()));
                link.set_namespaced_attribute("r:id", R, id);
            }
            (None, Destination::Place(place)) => link.set_namespaced_attribute(
                &edit::name_with(prefix.as_deref(), "anchor"),
                read::W,
                place,
            ),
            (None, Destination::Address(_)) => {}
        }
        for node in taken {
            link.children.push(node);
        }
        style_as_link(&mut link, prefix.as_deref());
        paragraph.insert_element(from, link);

        self.set_caret(end);
        self.mark_modified();
        true
    }

    /// The relationship id for an address, making one if there is not one.
    fn link_relationship(&mut self, address: &str) -> Option<String> {
        let main_part = self.main_part().to_owned();
        let mut relationships = self
            .package()
            .relationships(&main_part)
            .unwrap_or_else(|_| wp_opc::Relationships::new(&main_part));

        // The same address twice is the same relationship, which is what keeps
        // a document that links to one place a hundred times from carrying a
        // hundred entries.
        if let Some(existing) =
            relationships.by_type(RELATIONSHIP).find(|relationship| relationship.target == address)
        {
            return Some(existing.id.clone());
        }

        let id = relationships.add(RELATIONSHIP, address, TargetMode::External).id.clone();
        self.package_mut().set_relationships(&relationships).ok()?;
        Some(id)
    }

    /// Where each link in a paragraph goes, in the order they appear.
    fn link_targets(&self, paragraph: usize) -> Vec<Destination> {
        let Some(element) = self.paragraph_element(paragraph) else { return Vec::new() };
        let relationships = self.package().relationships(self.main_part()).ok();

        let mut out = Vec::new();
        collect_targets(element, relationships.as_ref(), &mut out);
        out
    }
}

/// Adds the scheme a person leaves off.
fn with_scheme(typed: &str) -> String {
    if typed.contains("://") || typed.starts_with("mailto:") {
        return typed.to_owned();
    }
    if typed.contains('@') {
        return format!("mailto:{typed}");
    }
    format!("https://{typed}")
}

/// Whether an element is a link.
fn is_link(element: &Element) -> bool {
    element.namespace.as_deref() == Some(read::W) && element.local_name() == "hyperlink"
}

/// Gives every run in a link the look of one.
///
/// Both the style and the colour, because a document whose styles part does not
/// define `Hyperlink` would otherwise show a link as ordinary text.
fn style_as_link(link: &mut Element, prefix: Option<&str>) {
    for run in link.child_elements_mut() {
        if run.namespace.as_deref() != Some(read::W) || run.local_name() != "r" {
            continue;
        }
        let properties = match run.position_of(Some(read::W), "rPr") {
            Some(index) => run.children[index].as_element_mut(),
            None => {
                let element = Element::new(&edit::name_with(prefix, "rPr"), Some(read::W));
                run.insert_element(0, element);
                run.children.first_mut().and_then(Node::as_element_mut)
            }
        };
        let Some(properties) = properties else { continue };

        let mut style = Element::new(&edit::name_with(prefix, "rStyle"), Some(read::W));
        style.set_namespaced_attribute(&edit::name_with(prefix, "val"), read::W, STYLE);
        let mut color = Element::new(&edit::name_with(prefix, "color"), Some(read::W));
        color.set_namespaced_attribute(&edit::name_with(prefix, "val"), read::W, COLOR);
        let mut underline = Element::new(&edit::name_with(prefix, "u"), Some(read::W));
        underline.set_namespaced_attribute(
            &edit::name_with(prefix, "val"),
            read::W,
            Underline::Single.to_attribute(),
        );

        properties.remove_children_named(Some(read::W), "rStyle");
        properties.remove_children_named(Some(read::W), "color");
        properties.remove_children_named(Some(read::W), "u");
        properties.insert_element(0, style);
        properties.push_element(color);
        properties.push_element(underline);
    }
}

/// Where each link in an element goes.
fn collect_targets(
    element: &Element,
    relationships: Option<&wp_opc::Relationships>,
    out: &mut Vec<Destination>,
) {
    for child in element.child_elements() {
        if is_link(child) {
            if let Some(place) = child.attribute(Some(read::W), "anchor") {
                out.push(Destination::Place(place.to_owned()));
            } else if let Some(id) = child.attribute(Some(R), "id") {
                let address = relationships
                    .and_then(|relationships| relationships.by_id(id))
                    .map(|relationship| relationship.target.clone())
                    .unwrap_or_default();
                out.push(Destination::Address(address));
            } else {
                out.push(Destination::Address(String::new()));
            }
        }
        collect_targets(child, relationships, out);
    }
}

/// Finds every link in a paragraph, with where it sits in the text.
fn walk_links(
    element: &Element,
    paragraph: usize,
    offset: &mut usize,
    targets: &[Destination],
    seen: &mut usize,
    out: &mut Vec<Link>,
) {
    for node in &element.children {
        let Some(child) = node.as_element() else { continue };
        if is_link(child) {
            let start = *offset;
            let mut inner = *offset;
            walk_links(child, paragraph, &mut inner, targets, seen, out);
            let destination =
                targets.get(*seen).cloned().unwrap_or(Destination::Address(String::new()));
            *seen += 1;
            out.push(Link {
                destination,
                text: child.text_content(),
                paragraph,
                range: (start, inner),
            });
            *offset = inner;
            continue;
        }
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "r" {
            *offset += edit::measured_length(child);
            continue;
        }
        walk_links(child, paragraph, offset, targets, seen, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_word_is_a_place_in_the_document() {
        assert_eq!(Destination::parse("Chapter"), Destination::Place("Chapter".to_owned()));
        assert_eq!(Destination::parse("#Top"), Destination::Place("Top".to_owned()));
    }

    #[test]
    fn an_address_is_recognised_however_it_is_typed() {
        assert_eq!(
            Destination::parse("example.org"),
            Destination::Address("https://example.org".to_owned())
        );
        assert_eq!(
            Destination::parse("https://example.org/a"),
            Destination::Address("https://example.org/a".to_owned())
        );
        assert_eq!(
            Destination::parse("someone@example.org"),
            Destination::Address("mailto:someone@example.org".to_owned())
        );
    }

    #[test]
    fn words_with_a_space_in_them_are_not_addresses() {
        assert_eq!(
            Destination::parse("See chapter two."),
            Destination::Place("See chapter two.".to_owned())
        );
    }
}

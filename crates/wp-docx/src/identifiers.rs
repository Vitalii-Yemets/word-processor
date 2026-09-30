//! The number each drawing in a document is known by, and its name.
//!
//! # Which number, and how far it has to be unique
//!
//! Every drawing — a picture, a shape, a chart, a diagram, a group, ink — is
//! wrapped in a `wp:inline` or a `wp:anchor`, and that wrapper's `wp:docPr`
//! carries an `id`. The standard makes it unique in the whole document: two
//! objects with the same one make the document non-conformant (ECMA-376 Part 1,
//! §20.4.2.5). It is also what anything pointing at one drawing among several
//! has to go by. Every drawing here used to be written with `id="1"`.
//!
//! Word keeps one count for the whole document. The templates that come with
//! Office number the drawings of the body, the headers and the footers from
//! it, one after another across all of them, and give the shapes inside a
//! group — whose `wps:cNvPr/@id` is what a connector names — numbers from the
//! same count. A picture's own `pic:cNvPr` Word writes as 0: it names nothing
//! and nothing names it. The glossary, the building blocks, is a document of
//! its own, and Word counts there again from wherever it likes; its numbers
//! are counted here all the same, which costs nothing and keeps a block put
//! into the document from arriving with a number the document already has.
//!
//! So a new drawing takes one more than the highest `docPr` or `cNvPr` in any
//! part that can hold one — the part being edited, and every other body,
//! header, footer, note, comment and the glossary — and nothing is renumbered
//! but what is new. A drawing kept where it was keeps its number; a copy
//! pasted, a group made and one taken apart, a part written from a model all
//! count as new.
//!
//! A shape has a number of its own besides, its `wps:cNvPr/@id`, which is
//! what a connector names it by, in a group or out of one. A shape nobody
//! has numbered yet is written with 0 there, and takes the next number of the
//! same count after its drawing's — every new shape used to be 1, and two of
//! them grouped were two members a connector could not tell apart. A shape
//! that has a number keeps it: a connector may be naming it.
//!
//! Ink in the line of text has no `wp:docPr`: it is a `w14:contentPart` alone,
//! and its `w14:cNvPr` is the number it is known by — which Word writes from
//! the same count.
//!
//! # The name
//!
//! Word names a drawing after its number and what it is: "Picture 3",
//! "Chart 4", "Shape 6", "Group 8". A new drawing is named so here, in place
//! of the name its writer gave it for want of a number; a name somebody chose
//! is kept. A pasted copy keeps the name it came with — it is the same
//! drawing, somewhere else — unless the document already has a drawing of
//! that name, and then it takes the one its new number gives it.

use std::collections::HashSet;

use wp_xml::tree::{Element, XmlTree};

use crate::edit::DRAWING_WORDPROCESSING;
use crate::effects::W14;
use crate::Document;

/// The kinds of part that can hold a drawing: the four kinds of main
/// document, a header, a footer, the notes, the comments and the glossary.
const PARTS_WITH_DRAWINGS: &[&str] = &[
    wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
    wp_opc::MAIN_DOCUMENT_MACRO_CONTENT_TYPE,
    wp_opc::MAIN_DOCUMENT_TEMPLATE_CONTENT_TYPE,
    wp_opc::MAIN_DOCUMENT_MACRO_TEMPLATE_CONTENT_TYPE,
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml",
    GLOSSARY,
];

/// The glossary's kind, whose numbers count and whose names do not: it is a
/// document of its own, and a name there is no other drawing's.
const GLOSSARY: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.glossary+xml";

/// The names the writers give a drawing that has no number yet, and Word's
/// for what the drawing is — each followed by the number, when there is one.
const WORDS: &[&str] =
    &["Picture", "Chart", "Diagram", "Shape", "Text Box", "Ink", "Group", "Video", "Drawing"];

impl Document {
    /// A number no drawing anywhere in the document has: one more than the
    /// highest.
    #[must_use]
    pub(crate) fn next_drawing_id(&self) -> u32 {
        self.known_drawings().0.saturating_add(1).max(self.drawing_floor)
    }

    /// The highest number any drawing in the document has, and the names its
    /// drawings go by.
    fn known_drawings(&self) -> (u32, HashSet<String>) {
        // The part being edited is its tree, which is newer than whatever the
        // package holds under its name.
        let mut highest = highest_id(&self.tree.root);
        let mut names = HashSet::new();
        names_in(&self.tree.root, false, &mut names);
        for entry in self.package.content_parts() {
            let kind = self.package.content_type(&entry.name);
            if entry.name.eq_ignore_ascii_case(&self.main_part)
                || !kind.is_some_and(|kind| PARTS_WITH_DRAWINGS.contains(&kind))
            {
                continue;
            }
            let Some(Ok(text)) = self.package.xml_part(&entry.name) else { continue };
            // Read as a tree only when there is a number in it to find.
            if !text.contains("docPr") && !text.contains("cNvPr") {
                continue;
            }
            if let Ok(tree) = XmlTree::parse(&text) {
                highest = highest.max(highest_id(&tree.root));
                if kind != Some(GLOSSARY) {
                    names_in(&tree.root, false, &mut names);
                }
            }
        }
        (highest, names)
    }

    /// How the drawings of something new to the document are numbered and
    /// named: from above every number it has, among the names it has.
    pub(crate) fn drawing_numbering(&self) -> Numbering {
        let (highest, names) = self.known_drawings();
        let next = highest.saturating_add(1).max(self.drawing_floor);
        Numbering { next, names, copies: false }
    }

    /// Gives every drawing in something about to go into the document a
    /// number of its own, above every number the document has, and the name
    /// the number gives it.
    ///
    /// Asked of whatever is new, before it goes in: a drawing inserted, a
    /// group made, a part written from a model.
    pub(crate) fn number_drawings(&self, elements: &mut [Element]) {
        self.drawing_numbering().number_all(elements);
    }

    /// The same, for drawings that were somewhere else before: a copy pasted,
    /// and the drawings a group is taken apart into. Each keeps the name it
    /// had unless the document already has a drawing of that name.
    pub(crate) fn number_copies(&self, elements: &mut [Element]) {
        let mut numbering = self.drawing_numbering();
        numbering.copies = true;
        numbering.number_all(elements);
    }

    /// [`Self::number_drawings`] for one element, given back.
    pub(crate) fn numbered(&self, mut element: Element) -> Element {
        self.number_drawings(core::slice::from_mut(&mut element));
        element
    }

    /// Gives every shape in a copy being put down a number of its own, and
    /// points the connectors in the copy at the new numbers.
    ///
    /// A copy is its elements as they were, and a shape's own number with
    /// them: a pair of shapes pasted beside the pair they were copied from
    /// were four shapes of two numbers, and a connector could not tell which
    /// of two it was fastened to. Word numbers a pasted copy's shapes again,
    /// and so does this — every `cNvPr` of a shape, and of every member of a
    /// group — and a connector in the same copy that named one of them names
    /// its new number. One that named a shape outside the copy goes on naming
    /// it.
    ///
    /// All of a copy at once, whatever paragraphs and notes it is spread over,
    /// because a connector in one may name a shape in another; and the
    /// numbers held back from the rest of the paste, which is numbered as it
    /// goes in and would otherwise be handed them again.
    pub(crate) fn renumber_copied_shapes(&mut self, elements: Vec<&mut Element>) {
        let mut next = self.next_drawing_id();
        let mut renamed = std::collections::HashMap::new();
        let mut elements = elements;
        for element in &mut elements {
            renumber_shapes(element, false, &mut next, &mut renamed);
        }
        for element in &mut elements {
            repoint_connectors(element, &renamed);
        }
        self.drawing_floor = self.drawing_floor.max(next);
    }
}

/// The numbers and the names drawings are being given, and the names already
/// taken.
pub(crate) struct Numbering {
    next: u32,
    names: HashSet<String>,
    /// Whether what is numbered is copies, which keep their names.
    copies: bool,
}

impl Numbering {
    /// For a document being made, which has nothing in it yet.
    pub(crate) fn fresh() -> Self {
        Self { next: 1, names: HashSet::new(), copies: false }
    }

    /// Numbers and names every drawing in some elements, from above what they
    /// carry inside themselves as well: a group made of drawings taken out of
    /// the text holds their shapes, whose numbers come from the same count
    /// and are no longer anywhere else.
    pub(crate) fn number_all(&mut self, elements: &mut [Element]) {
        let inside = elements.iter().map(highest_own_id).max().unwrap_or(0);
        self.next = self.next.max(inside.saturating_add(1));
        for element in elements {
            self.number(element);
        }
    }

    /// Numbers and names every drawing at or under an element.
    pub(crate) fn number(&mut self, element: &mut Element) {
        let namespace = element.namespace.as_deref();
        let local = element.local_name();
        if namespace == Some(DRAWING_WORDPROCESSING) && matches!(local, "inline" | "anchor") {
            self.wrapper(element);
            return;
        }
        // Ink in the line, which is known by its content part's own.
        if namespace == Some(W14) && local == "cNvPr" {
            let number = self.take();
            let before = element.attribute_by_name("name").unwrap_or_default().to_owned();
            let name = self.name_for(&before, "Ink", number);
            element.set_attribute("id", &number.to_string());
            element.set_attribute("name", &name);
            return;
        }
        for child in element.child_elements_mut() {
            self.number(child);
        }
    }

    /// One drawing: its wrapper's number and name, then what is inside it.
    fn wrapper(&mut self, wrapper: &mut Element) {
        let kind = kind_of(wrapper);
        let number = self.take();
        let mut named = None;
        if let Some(properties) = wrapper.child_elements_mut().find(|child| {
            child.namespace.as_deref() == Some(DRAWING_WORDPROCESSING)
                && child.local_name() == "docPr"
        }) {
            let before = properties.attribute_by_name("name").unwrap_or_default().to_owned();
            let name = self.name_for(&before, kind, number);
            properties.set_attribute("id", &number.to_string());
            properties.set_attribute("name", &name);
            named = Some((before, name));
        }
        for child in wrapper.child_elements_mut() {
            if child.local_name() != "docPr" {
                self.inside(child, named.as_ref(), false);
            }
        }
    }

    /// What is inside a drawing: the shape it is, the shapes of a group, and
    /// any drawing in the words of a text box.
    fn inside(&mut self, element: &mut Element, drawing: Option<&(String, String)>, grouped: bool) {
        let namespace = element.namespace.as_deref();
        let local = element.local_name();
        if namespace == Some(DRAWING_WORDPROCESSING) && matches!(local, "inline" | "anchor") {
            self.wrapper(element);
            return;
        }
        if namespace == Some(crate::shapes::WPS) && local == "cNvPr" {
            self.shape(element, drawing, grouped);
            return;
        }
        // The content part of floating ink is known by the wrapper's number.
        if namespace == Some(W14) && local == "cNvPr" {
            return;
        }
        let grouped = grouped || matches!(local, "wgp" | "grpSp");
        for child in element.child_elements_mut() {
            self.inside(child, drawing, grouped);
        }
    }

    /// A shape's own number and name. One with a number keeps it. One without
    /// takes the next; and its name follows the drawing's when the shape is
    /// the drawing, or its own number when it is one of a group's.
    fn shape(&mut self, own: &mut Element, drawing: Option<&(String, String)>, grouped: bool) {
        let had = own.attribute_by_name("id").and_then(|id| id.trim().parse::<u32>().ok());
        let fresh = had.is_none_or(|id| id == 0);
        let number = if fresh { self.take() } else { had.unwrap_or(0) };
        own.set_attribute("id", &number.to_string());

        let name = own.attribute_by_name("name").unwrap_or_default().trim().to_owned();
        let placeholder = name.is_empty() || stem_of(&name).is_some();
        if !grouped {
            if let Some((before, after)) = drawing {
                if placeholder || name == before.trim() {
                    own.set_attribute("name", after);
                }
            }
        } else if fresh && placeholder {
            let named = format!("{} {number}", stem_of(&name).unwrap_or("Shape"));
            own.set_attribute("name", &named);
        }
    }

    /// The next number, taken.
    fn take(&mut self) -> u32 {
        let number = self.next;
        self.next = self.next.saturating_add(1);
        number
    }

    /// The name a drawing numbered `number` goes by, having been called
    /// `before`, and taken.
    ///
    /// A new drawing's name is its number's unless somebody chose it; a
    /// copy's is the one it came with. Either gives way to the number's when
    /// the document already has a drawing of that name.
    fn name_for(&mut self, before: &str, kind: &str, number: u32) -> String {
        let before = before.trim();
        let stem = stem_of(before);
        let chosen = !before.is_empty() && (self.copies || stem.is_none());
        let name = if chosen && !self.names.contains(before) {
            before.to_owned()
        } else {
            // After the number; and in the rare case a kept name already is
            // that, the next free one, so that no two are called the same.
            let stem = stem.unwrap_or(kind);
            let mut at = number;
            loop {
                let candidate = format!("{stem} {at}");
                if !self.names.contains(&candidate) {
                    break candidate;
                }
                at = at.saturating_add(1);
            }
        };
        self.names.insert(name.clone());
        name
    }
}

/// The word a name is made of, when it is one of the names the writers give
/// and Word's for what a drawing is — alone or followed by a number.
fn stem_of(name: &str) -> Option<&'static str> {
    let name = name.trim();
    WORDS.iter().copied().find(|word| {
        name == *word
            || name.strip_prefix(word).and_then(|rest| rest.strip_prefix(' ')).is_some_and(|rest| {
                !rest.is_empty() && rest.chars().all(|character| character.is_ascii_digit())
            })
    })
}

/// What a drawing is, for its name: from what its graphic says it holds.
fn kind_of(wrapper: &Element) -> &'static str {
    fn uri(element: &Element) -> Option<&str> {
        if element.local_name() == "graphicData" {
            return element.attribute_by_name("uri");
        }
        element.child_elements().find_map(uri)
    }
    match uri(wrapper) {
        Some(crate::chart::CHART_URI) => "Chart",
        Some(crate::diagram::DIAGRAM_URI) => "Diagram",
        Some(crate::shapes::WPS) => "Shape",
        Some(crate::group::WPG) => "Group",
        Some(crate::ink::INK_GRAPHIC) => "Ink",
        Some(uri) if uri.ends_with("/picture") => "Picture",
        _ => "Drawing",
    }
}

/// The names the drawings at or under an element go by: each wrapper's
/// `wp:docPr`, and the content part of ink in the line.
fn names_in(element: &Element, in_wrapper: bool, out: &mut HashSet<String>) {
    let namespace = element.namespace.as_deref();
    let local = element.local_name();
    let named = (namespace == Some(DRAWING_WORDPROCESSING) && local == "docPr")
        || (namespace == Some(W14) && local == "cNvPr" && !in_wrapper);
    if named {
        if let Some(name) = element.attribute_by_name("name") {
            out.insert(name.trim().to_owned());
        }
        return;
    }
    let in_wrapper = in_wrapper
        || (namespace == Some(DRAWING_WORDPROCESSING) && matches!(local, "inline" | "anchor"));
    for child in element.child_elements() {
        names_in(child, in_wrapper, out);
    }
}

/// Gives every shape at or under an element the next number — each
/// `wps:cNvPr`, and each `cNvPr` inside a group — and says which number each
/// had. A picture's own `pic:cNvPr` out of a group is not a shape's and
/// stays as Word writes it, 0; ink's is the drawing's, and is numbered as
/// the drawing is.
fn renumber_shapes(
    element: &mut Element,
    grouped: bool,
    next: &mut u32,
    renamed: &mut std::collections::HashMap<u32, u32>,
) {
    let local = element.local_name();
    let namespace = element.namespace.as_deref();
    if local == "cNvPr"
        && namespace != Some(W14)
        && (grouped || namespace == Some(crate::shapes::WPS))
    {
        let number = *next;
        *next = next.saturating_add(1);
        let had = element.attribute_by_name("id").and_then(|id| id.trim().parse::<u32>().ok());
        // The first of a number is the one a connector is taken to mean,
        // should a copy carry two shapes of one number.
        if let Some(had) = had.filter(|had| *had != 0) {
            renamed.entry(had).or_insert(number);
        }
        element.set_attribute("id", &number.to_string());
        return;
    }
    let grouped = grouped || matches!(local, "wgp" | "grpSp");
    for child in element.child_elements_mut() {
        renumber_shapes(child, grouped, next, renamed);
    }
}

/// Points each end of each connector at or under an element at the new
/// number of the shape it named, when that shape was given one.
fn repoint_connectors(element: &mut Element, renamed: &std::collections::HashMap<u32, u32>) {
    if matches!(element.local_name(), "stCxn" | "endCxn") {
        let named = element.attribute_by_name("id").and_then(|id| id.trim().parse::<u32>().ok());
        if let Some(number) = named.and_then(|named| renamed.get(&named)) {
            element.set_attribute("id", &number.to_string());
        }
        return;
    }
    for child in element.child_elements_mut() {
        repoint_connectors(child, renamed);
    }
}

/// The highest drawing number at or under an element: every `docPr` and every
/// `cNvPr`, the numbers Word counts from one count.
#[must_use]
pub(crate) fn highest_id(element: &Element) -> u32 {
    let own = if matches!(element.local_name(), "docPr" | "cNvPr") {
        element.attribute_by_name("id").and_then(|id| id.trim().parse().ok()).unwrap_or(0)
    } else {
        0
    };
    element.child_elements().map(highest_id).fold(own, u32::max)
}

/// The highest number a shape or a picture inside some new content already
/// has: the `cNvPr`s, which are kept, and not the `docPr`s, which are given
/// again, nor ink's, which is given again too.
fn highest_own_id(element: &Element) -> u32 {
    let own = if element.local_name() == "cNvPr" && element.namespace.as_deref() != Some(W14) {
        element.attribute_by_name("id").and_then(|id| id.trim().parse().ok()).unwrap_or(0)
    } else {
        0
    };
    element.child_elements().map(highest_own_id).fold(own, u32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawing(id: &str, name: &str) -> Element {
        let mut drawing = Element::new("w:drawing", Some(crate::read::W));
        let mut inline = Element::new("wp:inline", Some(DRAWING_WORDPROCESSING));
        let mut properties = Element::new("wp:docPr", Some(DRAWING_WORDPROCESSING));
        properties.set_attribute("id", id);
        properties.set_attribute("name", name);
        inline.push_element(properties);
        drawing.push_element(inline);
        drawing
    }

    fn named(element: &Element, out: &mut Vec<(String, String)>) {
        if element.local_name() == "docPr" {
            let attribute = |name| element.attribute_by_name(name).unwrap_or_default().to_owned();
            out.push((attribute("id"), attribute("name")));
        }
        for child in element.child_elements() {
            named(child, out);
        }
    }

    #[test]
    fn every_drawing_under_an_element_gets_the_next_number_and_its_name() {
        let mut paragraph = Element::new("w:p", Some(crate::read::W));
        paragraph.push_element(drawing("1", "Picture 1"));
        paragraph.push_element(drawing("1", "Chosen"));
        let mut numbering = Numbering { next: 7, ..Numbering::fresh() };
        numbering.number(&mut paragraph);
        let mut found = Vec::new();
        named(&paragraph, &mut found);
        let pair = |id: &str, name: &str| (id.to_owned(), name.to_owned());
        assert_eq!(found, [pair("7", "Picture 7"), pair("8", "Chosen")]);
        assert_eq!(numbering.next, 9);
    }

    #[test]
    fn a_copy_keeps_its_name_unless_the_document_has_it() {
        let mut numbering =
            Numbering { next: 5, names: HashSet::from(["Picture 2".to_owned()]), copies: true };
        let mut kept = drawing("3", "Picture 3");
        numbering.number(&mut kept);
        let mut clashing = drawing("2", "Picture 2");
        numbering.number(&mut clashing);
        let mut found = Vec::new();
        named(&kept, &mut found);
        named(&clashing, &mut found);
        let pair = |id: &str, name: &str| (id.to_owned(), name.to_owned());
        assert_eq!(found, [pair("5", "Picture 3"), pair("6", "Picture 6")]);
    }

    #[test]
    fn a_name_the_writers_give_is_told_from_one_somebody_chose() {
        assert_eq!(stem_of("Picture 1"), Some("Picture"));
        assert_eq!(stem_of("Text Box"), Some("Text Box"));
        assert_eq!(stem_of("Shape 12"), Some("Shape"));
        assert_eq!(stem_of("Shape of things"), None);
        assert_eq!(stem_of("Company logo"), None);
    }

    #[test]
    fn the_highest_counts_a_shape_in_a_group_as_well_as_a_drawing() {
        let mut paragraph = Element::new("w:p", Some(crate::read::W));
        paragraph.push_element(drawing("4", "Group 4"));
        let mut member = Element::new("wps:cNvPr", Some(crate::shapes::WPS));
        member.set_attribute("id", "11");
        paragraph.push_element(member);
        assert_eq!(highest_id(&paragraph), 11);
    }

    #[test]
    fn a_copied_group_numbers_its_members_again_and_its_connector_follows() {
        let shapes = crate::shapes::WPS;
        let member = |id: &str| {
            let mut shape = Element::new("wps:wsp", Some(shapes));
            let mut own = Element::new("wps:cNvPr", Some(shapes));
            own.set_attribute("id", id);
            shape.push_element(own);
            shape
        };
        let end = |local: &str, id: &str| {
            let mut end = Element::new(&format!("a:{local}"), Some(crate::shapes::A));
            end.set_attribute("id", id);
            end
        };
        let mut connector = member("4");
        let mut fastened = Element::new("wps:cNvCnPr", Some(shapes));
        fastened.push_element(end("stCxn", "2"));
        fastened.push_element(end("endCxn", "9"));
        connector.push_element(fastened);
        let mut group = Element::new("wpg:wgp", Some(crate::group::WPG));
        for child in [member("2"), member("3"), connector] {
            group.push_element(child);
        }

        let mut next = 20;
        let mut renamed = std::collections::HashMap::new();
        renumber_shapes(&mut group, false, &mut next, &mut renamed);
        repoint_connectors(&mut group, &renamed);

        let mut own = Vec::new();
        let mut ends = Vec::new();
        fn walk(element: &Element, own: &mut Vec<String>, ends: &mut Vec<String>) {
            let id = element.attribute_by_name("id").unwrap_or_default().to_owned();
            match element.local_name() {
                "cNvPr" => own.push(id),
                "stCxn" | "endCxn" => ends.push(id),
                _ => {}
            }
            for child in element.child_elements() {
                walk(child, own, ends);
            }
        }
        walk(&group, &mut own, &mut ends);
        assert_eq!(own, ["20", "21", "22"]);
        // The member it named is 20 now; the shape outside the copy, 9,
        // it goes on naming.
        assert_eq!(ends, ["20", "9"]);
    }

    #[test]
    fn ink_in_a_wrapper_is_numbered_by_the_wrapper_alone() {
        let mut floating = drawing("1", "Ink 1");
        let mut part = Element::new("w14:contentPart", Some(W14));
        let mut own = Element::new("w14:cNvPr", Some(W14));
        own.set_attribute("id", "1");
        own.set_attribute("name", "Ink");
        part.push_element(own);
        floating.child_elements_mut().next().expect("the wrapper").push_element(part.clone());

        let mut numbering = Numbering { next: 5, ..Numbering::fresh() };
        numbering.number(&mut floating);
        assert_eq!(numbering.next, 6, "one drawing, one number");

        let mut numbering = Numbering { next: 5, ..Numbering::fresh() };
        numbering.number(&mut part);
        let own = part.child(Some(W14), "cNvPr").expect("the number");
        assert_eq!(own.attribute_by_name("id"), Some("5"), "ink in the line has no wrapper");
        assert_eq!(own.attribute_by_name("name"), Some("Ink 5"));
    }
}

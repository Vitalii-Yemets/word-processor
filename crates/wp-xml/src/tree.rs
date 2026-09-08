//! A document tree that keeps everything it was given.
//!
//! The pull parser is the right shape for reading a document once. Editing needs
//! something else: a structure that can be changed in place and written back
//! with **only the changed parts different**.
//!
//! That is the whole point of this module. A real `.docx` contains far more than
//! any one program models — tracked changes from a colleague, a chart, a content
//! control, a field nobody has implemented. A model that understood only what it
//! knew about would throw the rest away the moment the user pressed save. Here
//! every element, attribute, comment and stretch of whitespace is held as it
//! arrived, and an edit touches one node and leaves its neighbours untouched.
//!
//! # Example
//!
//! ```
//! use wp_xml::tree::XmlTree;
//!
//! let source = r#"<a xmlns="urn:x"><b keep="yes"/><c>text</c></a>"#;
//! let mut tree = XmlTree::parse(source)?;
//!
//! // Change one attribute; everything else is written back untouched.
//! tree.root.child_mut(Some("urn:x"), "b").unwrap().set_attribute("keep", "no");
//!
//! assert_eq!(tree.to_xml()?, r#"<a xmlns="urn:x"><b keep="no"/><c>text</c></a>"#);
//! # Ok::<(), wp_xml::Error>(())
//! ```

use crate::{Error, ErrorKind, Event, Position, Reader, Writer};

/// An attribute, kept with the prefix it was written with.
///
/// The written name is preserved rather than regenerated. Rewriting `w:val` as
/// `ns0:val` is equivalent XML, but it would make every saved file differ from
/// the original everywhere, which hides the change the user actually made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    /// The name as written, prefix included.
    pub name: String,
    /// Resolved namespace. `None` for an unprefixed attribute, which belongs to
    /// no namespace at all.
    pub namespace: Option<String>,
    pub value: String,
}

/// Anything that can sit inside an element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
    CData(String),
    Comment(String),
    ProcessingInstruction { target: String, data: String },
}

impl Node {
    /// The element, if this node is one.
    #[must_use]
    pub fn as_element(&self) -> Option<&Element> {
        match self {
            Self::Element(element) => Some(element),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_element_mut(&mut self) -> Option<&mut Element> {
        match self {
            Self::Element(element) => Some(element),
            _ => None,
        }
    }
}

/// An element and everything under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    /// The name as written, prefix included, such as `w:pPr`.
    pub name: String,
    /// Resolved namespace, from the prefix or from the default namespace.
    pub namespace: Option<String>,
    pub attributes: Vec<Attribute>,
    /// Namespace declarations written on this element, so it can be serialized
    /// exactly as it arrived.
    pub declarations: Vec<(Option<String>, String)>,
    pub children: Vec<Node>,
    /// Whether the source wrote `<x/>` rather than `<x></x>`. Semantically the
    /// same, but keeping it means an untouched document is written back as it
    /// came.
    pub empty_form: bool,
}

impl Element {
    /// A new element with no children, written in the `<x/>` form.
    #[must_use]
    pub fn new(name: &str, namespace: Option<&str>) -> Self {
        Self {
            name: name.to_owned(),
            namespace: namespace.map(str::to_owned),
            attributes: Vec::new(),
            declarations: Vec::new(),
            children: Vec::new(),
            empty_form: true,
        }
    }

    /// The part of the name after the prefix.
    #[must_use]
    pub fn local_name(&self) -> &str {
        match self.name.split_once(':') {
            Some((_, local)) => local,
            None => &self.name,
        }
    }

    /// The prefix, if the name has one.
    #[must_use]
    pub fn prefix(&self) -> Option<&str> {
        self.name.split_once(':').map(|(prefix, _)| prefix)
    }

    /// Whether this is the given element.
    #[must_use]
    pub fn is(&self, namespace: Option<&str>, local: &str) -> bool {
        self.local_name() == local && self.namespace.as_deref() == namespace
    }

    /// The value of an attribute, by namespace and local name.
    #[must_use]
    pub fn attribute(&self, namespace: Option<&str>, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| {
                attribute.namespace.as_deref() == namespace && local_of(&attribute.name) == local
            })
            .map(|attribute| attribute.value.as_str())
    }

    /// The value of an attribute by its written name.
    #[must_use]
    pub fn attribute_by_name(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .map(|attribute| attribute.value.as_str())
    }

    /// Sets an attribute by written name, adding it if absent.
    ///
    /// The namespace is inherited from an existing attribute of the same name,
    /// so an edit never silently moves an attribute out of its namespace.
    pub fn set_attribute(&mut self, name: &str, value: &str) {
        match self.attributes.iter_mut().find(|attribute| attribute.name == name) {
            Some(attribute) => attribute.value = value.to_owned(),
            None => self.attributes.push(Attribute {
                name: name.to_owned(),
                namespace: None,
                value: value.to_owned(),
            }),
        }
    }

    /// Sets an attribute that belongs to a namespace.
    pub fn set_namespaced_attribute(&mut self, name: &str, namespace: &str, value: &str) {
        match self.attributes.iter_mut().find(|attribute| attribute.name == name) {
            Some(attribute) => {
                attribute.namespace = Some(namespace.to_owned());
                attribute.value = value.to_owned();
            }
            None => self.attributes.push(Attribute {
                name: name.to_owned(),
                namespace: Some(namespace.to_owned()),
                value: value.to_owned(),
            }),
        }
    }

    /// Removes an attribute by written name.
    pub fn remove_attribute(&mut self, name: &str) {
        self.attributes.retain(|attribute| attribute.name != name);
    }

    /// Direct child elements.
    pub fn child_elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(Node::as_element)
    }

    /// Direct child elements, mutably.
    pub fn child_elements_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children.iter_mut().filter_map(Node::as_element_mut)
    }

    /// The first direct child with the given name.
    #[must_use]
    pub fn child(&self, namespace: Option<&str>, local: &str) -> Option<&Element> {
        self.child_elements().find(|element| element.is(namespace, local))
    }

    /// The first direct child with the given name, mutably.
    pub fn child_mut(&mut self, namespace: Option<&str>, local: &str) -> Option<&mut Element> {
        self.child_elements_mut().find(|element| element.is(namespace, local))
    }

    /// Every direct child with the given name.
    pub fn children_named<'a>(
        &'a self,
        namespace: Option<&'a str>,
        local: &'a str,
    ) -> impl Iterator<Item = &'a Element> {
        self.child_elements().filter(move |element| element.is(namespace, local))
    }

    /// The position of the first direct child with the given name.
    #[must_use]
    pub fn position_of(&self, namespace: Option<&str>, local: &str) -> Option<usize> {
        self.children
            .iter()
            .position(|node| node.as_element().is_some_and(|element| element.is(namespace, local)))
    }

    /// Removes every direct child with the given name.
    pub fn remove_children_named(&mut self, namespace: Option<&str>, local: &str) {
        self.children
            .retain(|node| !node.as_element().is_some_and(|element| element.is(namespace, local)));
    }

    /// Appends a child element.
    pub fn push_element(&mut self, element: Element) {
        self.children.push(Node::Element(element));
        self.empty_form = false;
    }

    /// Inserts a child at a position.
    pub fn insert_element(&mut self, index: usize, element: Element) {
        let index = index.min(self.children.len());
        self.children.insert(index, Node::Element(element));
        self.empty_form = false;
    }

    /// Replaces all children with a single stretch of text.
    pub fn set_text(&mut self, text: &str) {
        self.children.clear();
        if !text.is_empty() {
            self.children.push(Node::Text(text.to_owned()));
            self.empty_form = false;
        }
    }

    /// All text under this element, markup ignored.
    #[must_use]
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        collect_text(self, &mut out);
        out
    }
}

fn collect_text(element: &Element, out: &mut String) {
    for node in &element.children {
        match node {
            Node::Text(text) | Node::CData(text) => out.push_str(text),
            Node::Element(child) => collect_text(child, out),
            _ => {}
        }
    }
}

/// The local part of a written name.
fn local_of(name: &str) -> &str {
    match name.split_once(':') {
        Some((_, local)) => local,
        None => name,
    }
}

/// A whole XML document, including what sits outside the root element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlTree {
    /// Whether the declaration said `standalone`, and what it said.
    pub standalone: Option<bool>,
    /// Whether the source had an XML declaration at all.
    pub has_declaration: bool,
    /// A document type declaration, kept verbatim and never acted on.
    pub doctype: Option<String>,
    /// Comments, processing instructions and whitespace before the root.
    pub before_root: Vec<Node>,
    pub root: Element,
    /// The same, after the root.
    pub after_root: Vec<Node>,
}

impl XmlTree {
    /// Builds a tree from XML text.
    pub fn parse(source: &str) -> Result<Self, Error> {
        let mut reader = Reader::new(source);

        let mut standalone = None;
        let mut has_declaration = false;
        let mut doctype = None;
        let mut before_root = Vec::new();
        let mut after_root = Vec::new();
        let mut root: Option<Element> = None;
        // Elements currently open, innermost last.
        let mut stack: Vec<Element> = Vec::new();

        while let Some(event) = reader.next_event() {
            match event? {
                Event::Declaration { standalone: value, .. } => {
                    has_declaration = true;
                    standalone = value;
                }
                Event::DocType { content } => doctype = Some(content.to_owned()),
                Event::Start(tag) => stack.push(element_from(&tag, false)),
                Event::Empty(tag) => {
                    let element = element_from(&tag, true);
                    place(
                        &mut stack,
                        Node::Element(element),
                        &mut before_root,
                        &mut after_root,
                        &mut root,
                    );
                }
                Event::End(_) => {
                    let finished = stack.pop().expect("the reader guarantees matched tags");
                    place(
                        &mut stack,
                        Node::Element(finished),
                        &mut before_root,
                        &mut after_root,
                        &mut root,
                    );
                }
                Event::Text(text) => {
                    place(
                        &mut stack,
                        Node::Text(text.into_owned()),
                        &mut before_root,
                        &mut after_root,
                        &mut root,
                    );
                }
                Event::CData(text) => {
                    place(
                        &mut stack,
                        Node::CData(text.to_owned()),
                        &mut before_root,
                        &mut after_root,
                        &mut root,
                    );
                }
                Event::Comment(text) => {
                    place(
                        &mut stack,
                        Node::Comment(text.to_owned()),
                        &mut before_root,
                        &mut after_root,
                        &mut root,
                    );
                }
                Event::ProcessingInstruction { target, data } => {
                    let node = Node::ProcessingInstruction {
                        target: target.to_owned(),
                        data: data.to_owned(),
                    };
                    place(&mut stack, node, &mut before_root, &mut after_root, &mut root);
                }
            }
        }

        // The reader refuses a document without exactly one root, so by the time
        // it finishes without error there is one.
        let root = root.ok_or(Error {
            kind: ErrorKind::RootElementCount(0),
            position: Position { offset: 0, line: 1, column: 1 },
        })?;

        Ok(Self { standalone, has_declaration, doctype, before_root, root, after_root })
    }

    /// Writes the tree back out as XML.
    pub fn to_xml(&self) -> Result<String, Error> {
        let mut writer = Writer::with_capacity(4096);

        if self.has_declaration {
            writer.write_declaration(self.standalone);
        }
        if let Some(doctype) = &self.doctype {
            writer.write_doctype(doctype);
        }
        for node in &self.before_root {
            write_node(&mut writer, node)?;
        }
        write_element(&mut writer, &self.root)?;
        for node in &self.after_root {
            write_node(&mut writer, node)?;
        }

        writer.finish()
    }
}

/// Adds a finished node to whatever is currently open.
fn place(
    stack: &mut [Element],
    node: Node,
    before_root: &mut Vec<Node>,
    after_root: &mut Vec<Node>,
    root: &mut Option<Element>,
) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
        return;
    }
    // Nothing is open, so this is at the top level.
    match node {
        Node::Element(element) => *root = Some(element),
        other => {
            if root.is_some() {
                after_root.push(other);
            } else {
                before_root.push(other);
            }
        }
    }
}

fn element_from(tag: &crate::StartTag<'_>, empty_form: bool) -> Element {
    Element {
        name: tag.name.to_written(),
        namespace: tag.namespace.map(str::to_owned),
        attributes: tag
            .attributes
            .iter()
            .map(|attribute| Attribute {
                name: attribute.name.to_written(),
                namespace: attribute.namespace.map(str::to_owned),
                value: attribute.value.to_string(),
            })
            .collect(),
        declarations: tag
            .declarations
            .iter()
            .map(|(prefix, uri)| (prefix.map(str::to_owned), (*uri).to_owned()))
            .collect(),
        children: Vec::new(),
        empty_form,
    }
}

fn write_node(writer: &mut Writer, node: &Node) -> Result<(), Error> {
    match node {
        Node::Element(element) => write_element(writer, element),
        Node::Text(text) => {
            writer.write_text(text);
            Ok(())
        }
        Node::CData(text) => writer.write_cdata(text),
        Node::Comment(text) => writer.write_comment(text),
        Node::ProcessingInstruction { target, data } => {
            writer.write_processing_instruction(target, data)
        }
    }
}

fn write_element(writer: &mut Writer, element: &Element) -> Result<(), Error> {
    // Namespace declarations are written as attributes, before the rest, which
    // is where documents put them.
    let mut attributes: Vec<(String, String)> =
        Vec::with_capacity(element.declarations.len() + element.attributes.len());
    for (prefix, uri) in &element.declarations {
        let name = match prefix {
            Some(prefix) => format!("xmlns:{prefix}"),
            None => "xmlns".to_owned(),
        };
        attributes.push((name, uri.clone()));
    }
    for attribute in &element.attributes {
        attributes.push((attribute.name.clone(), attribute.value.clone()));
    }
    let borrowed: Vec<(&str, &str)> =
        attributes.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();

    if element.children.is_empty() && element.empty_form {
        return writer.write_empty(&element.name, &borrowed);
    }

    writer.write_start(&element.name, &borrowed)?;
    for child in &element.children {
        write_node(writer, child)?;
    }
    writer.write_end(&element.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_survives_parsing_and_writing() {
        for source in [
            "<a/>",
            "<a></a>",
            "<a>text</a>",
            "<a b=\"1\" c=\"2\"><d/></a>",
            "<a xmlns=\"urn:default\" xmlns:p=\"urn:p\"><p:b p:c=\"v\"/></a>",
            "<a><!-- comment --><b/><?pi data?></a>",
            "<a><![CDATA[raw <text>]]></a>",
            "<a>многоязычный 文書 نص</a>",
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><a/>",
        ] {
            let tree = XmlTree::parse(source).unwrap_or_else(|e| panic!("{source:?}: {e}"));
            assert_eq!(tree.to_xml().unwrap(), source, "changed on the way back");
        }
    }

    #[test]
    fn an_edit_leaves_its_neighbours_untouched() {
        let source =
            "<r xmlns=\"urn:x\"><keep a=\"1\"/><change a=\"1\"/><keep2><deep/></keep2></r>";
        let mut tree = XmlTree::parse(source).unwrap();

        tree.root.child_mut(Some("urn:x"), "change").unwrap().set_attribute("a", "2");

        assert_eq!(
            tree.to_xml().unwrap(),
            "<r xmlns=\"urn:x\"><keep a=\"1\"/><change a=\"2\"/><keep2><deep/></keep2></r>"
        );
    }

    #[test]
    fn unknown_content_is_carried_through_an_edit() {
        // Stands in for what a real document holds: a content control, a chart,
        // a revision. None of it is understood, all of it must come back.
        let source = "<body xmlns=\"urn:x\">\
            <p><t>old</t></p>\
            <unknown><nested attr=\"kept\"><!-- note --><deep>data</deep></nested></unknown>\
            </body>";
        let mut tree = XmlTree::parse(source).unwrap();

        let paragraph = tree.root.child_mut(Some("urn:x"), "p").unwrap();
        paragraph.child_mut(Some("urn:x"), "t").unwrap().set_text("new");

        let written = tree.to_xml().unwrap();
        assert!(written.contains("<t>new</t>"), "the edit did not apply");
        assert!(
            written.contains(
                "<unknown><nested attr=\"kept\"><!-- note --><deep>data</deep></nested></unknown>"
            ),
            "unknown content was altered: {written}"
        );
    }

    #[test]
    fn whitespace_between_elements_is_kept() {
        // Indentation is text content as far as XML is concerned. Dropping it
        // would rewrite every line of a formatted document.
        let source = "<a>\n  <b/>\n  <c>text</c>\n</a>";
        assert_eq!(XmlTree::parse(source).unwrap().to_xml().unwrap(), source);
    }

    #[test]
    fn elements_are_found_by_namespace_not_by_prefix() {
        let source = "<a xmlns:p=\"urn:x\"><p:b/></a>";
        let tree = XmlTree::parse(source).unwrap();

        assert!(tree.root.child(Some("urn:x"), "b").is_some());
        // The same name in another namespace is a different element.
        assert!(tree.root.child(Some("urn:other"), "b").is_none());
        assert!(tree.root.child(None, "b").is_none());
    }

    #[test]
    fn text_content_gathers_everything_below() {
        let tree = XmlTree::parse("<a>one<b> two</b><c><d> three</d></c></a>").unwrap();
        assert_eq!(tree.root.text_content(), "one two three");
    }
}

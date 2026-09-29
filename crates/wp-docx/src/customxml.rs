//! Custom XML parts, and the content controls bound to their nodes.
//!
//! # Two views of one thing
//!
//! A document can carry data as well as words: an XML part of its own under
//! `customXml/`, with any shape its author likes — an order, a patient, an
//! invoice. A content control can then be *bound* to one node of it, and the
//! two are one thing seen twice: the control shows what the node says, and
//! writing in the control writes the node. A program that fills the data in
//! gets a document that reads as prose; a person who edits the prose leaves
//! data a program can read back. That is what Word's XML Mapping pane is
//! for, and this is the storage under it.
//!
//! # How a part is stored
//!
//! Three things in the package for each part, which is how Word writes it
//! and what Word expects to find:
//!
//! - `customXml/item1.xml` — the data itself, whatever it is;
//! - `customXml/itemProps1.xml` — a *datastore item* with an identifier, a
//!   GUID, which is how a control names the part it is bound to, since a
//!   part may be renumbered and a GUID may not;
//! - `customXml/_rels/item1.xml.rels` — the relationship from the one to the
//!   other.
//!
//! The document's own relationships reach the item, by the same relationship
//! type an ink part uses; the parts under `customXml/` are the data ones.
//!
//! # How a control is bound
//!
//! ```text
//! <w:dataBinding w:prefixMappings="xmlns:ns0='http://example.com/order'"
//!                w:xpath="/ns0:order[1]/ns0:customer[1]"
//!                w:storeItemID="{7A9E3C10-...}"/>
//! ```
//!
//! An XPath, with the prefixes it uses declared beside it, and the GUID of
//! the part. The paths Word writes are of one shape — every step an element
//! by name with its number among its namesakes, and at most an attribute at
//! the end — and that shape is what is followed here. A path of another
//! shape is not followed, and a control bound by one is a control this
//! program leaves as it found it.

use wp_opc::{Relationships, TargetMode};
use wp_xml::tree::{Element, XmlTree};

use crate::{read, Document, TextPosition};

/// The relationship type from the document to a custom XML part, which is
/// also the one an ink part is reached by.
pub const CUSTOM_XML: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml";
/// And from the part to its datastore item.
pub const CUSTOM_XML_PROPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps";
/// What the package calls a datastore item.
pub const PROPS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.customXmlProperties+xml";
/// The namespace a datastore item is written in.
pub const DATASTORE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/customXml";

/// How a control is bound: the path, the prefixes it uses, and the part.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Binding {
    /// `xmlns:ns0='http://…' xmlns:ns1='…'`, as Word writes it.
    pub prefixes: String,
    /// `/ns0:order[1]/ns0:customer[1]`, or with `/@attribute` at the end.
    pub xpath: String,
    /// The GUID of the part, braces included.
    pub store_item: String,
}

impl Binding {
    /// The prefixes as pairs: which prefix means which namespace.
    #[must_use]
    pub fn mappings(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut rest = self.prefixes.as_str();
        while let Some(at) = rest.find("xmlns:") {
            rest = &rest[at + 6..];
            let Some(equals) = rest.find('=') else { break };
            let prefix = rest[..equals].trim().to_owned();
            rest = rest[equals + 1..].trim_start();
            let Some(quote) = rest.chars().next().filter(|c| *c == '\'' || *c == '"') else {
                break;
            };
            rest = &rest[1..];
            let Some(end) = rest.find(quote) else { break };
            out.push((prefix, rest[..end].to_owned()));
            rest = &rest[end + 1..];
        }
        out
    }

    /// The binding for a node of a part: the path down to it, with every
    /// namespace on the way given a prefix.
    #[must_use]
    pub fn to_node(part: &CustomXml, node: &NodeRow) -> Self {
        let mut namespaces: Vec<String> = Vec::new();
        let mut prefix_of = |namespace: Option<&str>| -> String {
            let Some(namespace) = namespace else { return String::new() };
            let at = match namespaces.iter().position(|known| known == namespace) {
                Some(at) => at,
                None => {
                    namespaces.push(namespace.to_owned());
                    namespaces.len() - 1
                }
            };
            format!("ns{at}:")
        };

        let mut xpath = String::new();
        let mut element = &part.root;
        xpath.push('/');
        xpath.push_str(&prefix_of(element.namespace.as_deref()));
        xpath.push_str(element.local_name());
        xpath.push_str("[1]");
        for at in &node.path {
            let Some(child) = element.children.get(*at).and_then(|node| node.as_element()) else {
                break;
            };
            // Its number among the children of the same name before it.
            let number = element.children[..*at]
                .iter()
                .filter_map(|node| node.as_element())
                .filter(|other| {
                    other.namespace == child.namespace && other.local_name() == child.local_name()
                })
                .count()
                + 1;
            xpath.push('/');
            xpath.push_str(&prefix_of(child.namespace.as_deref()));
            xpath.push_str(child.local_name());
            xpath.push_str(&format!("[{number}]"));
            element = child;
        }
        if let Some(attribute) = &node.attribute {
            xpath.push_str("/@");
            xpath.push_str(attribute);
        }
        let prefixes = namespaces
            .iter()
            .enumerate()
            .map(|(at, namespace)| format!("xmlns:ns{at}='{namespace}'"))
            .collect::<Vec<_>>()
            .join(" ");
        Self { prefixes, xpath, store_item: part.id.clone() }
    }
}

/// One step of a path: an element by name and number, or an attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Step {
    Element { namespace: Option<String>, local: String, number: usize },
    Attribute(String),
}

/// The steps of a path, or nothing for a path of a shape not followed.
fn steps(binding: &Binding) -> Option<Vec<Step>> {
    let mappings = binding.mappings();
    let mut out = Vec::new();
    for piece in binding.xpath.trim().trim_start_matches('/').split('/') {
        if piece.is_empty() {
            return None;
        }
        if let Some(attribute) = piece.strip_prefix('@') {
            out.push(Step::Attribute(attribute.to_owned()));
            continue;
        }
        let (name, number) = match piece.split_once('[') {
            Some((name, rest)) => {
                let digits = rest.strip_suffix(']')?;
                (name, digits.trim().parse::<usize>().ok()?)
            }
            None => (piece, 1),
        };
        let (namespace, local) = match name.split_once(':') {
            Some((prefix, local)) => {
                let namespace = mappings
                    .iter()
                    .find(|(known, _)| known == prefix)
                    .map(|(_, namespace)| namespace.clone())?;
                (Some(namespace), local)
            }
            None => (None, name),
        };
        if local.is_empty() || number == 0 {
            return None;
        }
        out.push(Step::Element { namespace, local: local.to_owned(), number });
    }
    (!out.is_empty()).then_some(out)
}

/// Where a path leads in a tree: the indices down through the children,
/// and the attribute at the end if there is one.
fn locate(root: &Element, binding: &Binding) -> Option<(Vec<usize>, Option<String>)> {
    let steps = steps(binding)?;
    let mut steps = steps.into_iter();
    // The first step is the root itself.
    match steps.next()? {
        Step::Element { namespace, local, number } => {
            if root.namespace != namespace || root.local_name() != local || number != 1 {
                return None;
            }
        }
        Step::Attribute(_) => return None,
    }
    let mut path = Vec::new();
    let mut element = root;
    for step in steps {
        match step {
            Step::Attribute(name) => return Some((path, Some(name))),
            Step::Element { namespace, local, number } => {
                let mut seen = 0usize;
                let mut found = None;
                for (at, child) in element.children.iter().enumerate() {
                    let Some(child) = child.as_element() else { continue };
                    if child.namespace == namespace && child.local_name() == local {
                        seen += 1;
                        if seen == number {
                            found = Some((at, child));
                            break;
                        }
                    }
                }
                let (at, child) = found?;
                path.push(at);
                element = child;
            }
        }
    }
    Some((path, None))
}

/// The element a path of child indices leads to.
fn element_at<'a>(root: &'a Element, path: &[usize]) -> Option<&'a Element> {
    let mut element = root;
    for at in path {
        element = element.children.get(*at)?.as_element()?;
    }
    Some(element)
}

fn element_at_mut<'a>(root: &'a mut Element, path: &[usize]) -> Option<&'a mut Element> {
    let mut element = root;
    for at in path {
        element = element.children.get_mut(*at)?.as_element_mut()?;
    }
    Some(element)
}

/// One row of a part's tree, as a pane lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeRow {
    /// How deep, from the root at nought.
    pub depth: usize,
    /// The indices down through the children to the element; empty for the
    /// root.
    pub path: Vec<usize>,
    /// The element's name, or the attribute's.
    pub name: String,
    /// The attribute, for a row that is one of the element's attributes.
    pub attribute: Option<String>,
    /// What it says: an attribute's value, or an element's own text where
    /// it has no elements inside it.
    pub value: String,
}

/// One custom XML part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomXml {
    /// Its name in the package: `customXml/item1.xml`.
    pub part: String,
    /// Its datastore item's GUID, braces included; empty where it has none,
    /// in which case nothing can be bound to it.
    pub id: String,
    pub root: Element,
}

impl CustomXml {
    /// What a person calls it: the root's namespace, or its name.
    #[must_use]
    pub fn label(&self) -> String {
        self.root.namespace.clone().unwrap_or_else(|| self.root.local_name().to_owned())
    }

    /// What a bound node says, if the path leads anywhere.
    #[must_use]
    pub fn text_at(&self, binding: &Binding) -> Option<String> {
        let (path, attribute) = locate(&self.root, binding)?;
        let element = element_at(&self.root, &path)?;
        match attribute {
            Some(name) => element.attribute_by_name(&name).map(str::to_owned).or_else(|| {
                element
                    .attributes
                    .iter()
                    .find(|attribute| attribute.name.rsplit(':').next() == Some(name.as_str()))
                    .map(|attribute| attribute.value.clone())
            }),
            None => Some(element.text_content()),
        }
    }

    /// Every node, top to bottom, as a pane lists them.
    #[must_use]
    pub fn rows(&self) -> Vec<NodeRow> {
        let mut out = Vec::new();
        list(&self.root, 0, &mut Vec::new(), &mut out);
        out
    }
}

/// Lists an element, its attributes and its children, depth first.
fn list(element: &Element, depth: usize, path: &mut Vec<usize>, out: &mut Vec<NodeRow>) {
    let has_elements = element.child_elements().next().is_some();
    out.push(NodeRow {
        depth,
        path: path.clone(),
        name: element.local_name().to_owned(),
        attribute: None,
        value: if has_elements { String::new() } else { element.text_content() },
    });
    for attribute in &element.attributes {
        if attribute.name.starts_with("xmlns") {
            continue;
        }
        out.push(NodeRow {
            depth: depth + 1,
            path: path.clone(),
            name: attribute.name.clone(),
            attribute: Some(attribute.name.clone()),
            value: attribute.value.clone(),
        });
    }
    for (at, child) in element.children.iter().enumerate() {
        let Some(child) = child.as_element() else { continue };
        path.push(at);
        list(child, depth + 1, path, out);
        path.pop();
    }
}

/// A GUID for a new datastore item, as Word writes one: braces and capitals.
///
/// Made from the clock, the process and a count, hashed, with the bits set
/// that mark a random GUID; nothing here needs it to be more than unlike
/// every other one in the same package.
fn fresh_guid(salt: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or(0);
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    let mut message = nanos.to_le_bytes().to_vec();
    message.extend_from_slice(&count.to_le_bytes());
    message.extend_from_slice(&std::process::id().to_le_bytes());
    message.extend_from_slice(salt.as_bytes());
    let mut bytes = wp_hash::sha1(&message);
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let hex: Vec<String> = bytes.iter().take(16).map(|byte| format!("{byte:02X}")).collect();
    format!(
        "{{{}-{}-{}-{}-{}}}",
        hex[0..4].concat(),
        hex[4..6].concat(),
        hex[6..8].concat(),
        hex[8..10].concat(),
        hex[10..16].concat()
    )
}

impl Document {
    /// The custom XML parts the document carries, in the order the
    /// document reaches them.
    #[must_use]
    pub fn custom_xml_parts(&self) -> Vec<CustomXml> {
        let main = self.document_part.clone();
        let Ok(relationships) = self.package().relationships(&main) else { return Vec::new() };
        let mut out = Vec::new();
        for relationship in relationships.by_type(CUSTOM_XML) {
            let Some(Ok(part)) = relationship.resolved_target(&main) else { continue };
            // The data parts are the ones under `customXml/`; an ink part is
            // reached by the same relationship and lives elsewhere.
            if !part.starts_with("customXml/") {
                continue;
            }
            let Some(Ok(xml)) = self.package().xml_part(&part) else { continue };
            let Ok(tree) = XmlTree::parse(&xml) else { continue };
            out.push(CustomXml { id: self.datastore_id(&part), part, root: tree.root });
        }
        out
    }

    /// The GUID of a part's datastore item, or nothing.
    fn datastore_id(&self, part: &str) -> String {
        let Ok(relationships) = self.package().relationships(part) else { return String::new() };
        let Some(props) = relationships.single_by_type(CUSTOM_XML_PROPS) else {
            return String::new();
        };
        let Some(Ok(props)) = props.resolved_target(part) else { return String::new() };
        let Some(Ok(xml)) = self.package().xml_part(&props) else { return String::new() };
        let Ok(tree) = XmlTree::parse(&xml) else { return String::new() };
        tree.root
            .attribute(Some(DATASTORE), "itemID")
            .or_else(|| tree.root.attribute_by_name("ds:itemID"))
            .unwrap_or_default()
            .to_owned()
    }

    /// The part of a GUID, if the document has it.
    #[must_use]
    pub fn custom_xml_part(&self, id: &str) -> Option<CustomXml> {
        self.custom_xml_parts().into_iter().find(|part| part.id.eq_ignore_ascii_case(id))
    }

    /// Adds a part holding this XML, with a datastore item of its own, and
    /// gives back the item's GUID. XML that does not parse is refused.
    pub fn add_custom_xml(&mut self, xml: &str) -> Option<String> {
        XmlTree::parse(xml).ok()?;
        let number = (1..)
            .find(|number| self.package().part(&format!("customXml/item{number}.xml")).is_none())?;
        let item = format!("customXml/item{number}.xml");
        let props = format!("customXml/itemProps{number}.xml");
        let id = fresh_guid(&item);

        let main = self.document_part.clone();
        let mut relationships = self.package().relationships(&main).ok()?;
        relationships.add(CUSTOM_XML, &format!("../{item}"), TargetMode::Internal);
        self.package_mut().set_relationships(&relationships).ok()?;

        self.package_mut().add_part_with_default_type(
            &item,
            "xml",
            "application/xml",
            xml.as_bytes().to_vec(),
        );
        let written = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\r\n\
             <ds:datastoreItem ds:itemID=\"{id}\" xmlns:ds=\"{DATASTORE}\"><ds:schemaRefs/></ds:datastoreItem>"
        );
        self.package_mut().add_part(&props, PROPS_CONTENT_TYPE, written.into_bytes());
        let mut own = Relationships::new(&item);
        own.add(CUSTOM_XML_PROPS, &format!("itemProps{number}.xml"), TargetMode::Internal);
        self.package_mut().set_relationships(&own).ok()?;
        self.note_change();
        Some(id)
    }

    /// Takes a part out, with its datastore item and the relationships to
    /// both. The controls bound to it are left as they are, showing what
    /// they last showed, which is what Word does too.
    pub fn remove_custom_xml(&mut self, id: &str) -> bool {
        let Some(part) = self.custom_xml_part(id) else { return false };
        let main = self.document_part.clone();
        let Ok(mut relationships) = self.package().relationships(&main) else { return false };
        let doomed: Vec<String> = relationships
            .by_type(CUSTOM_XML)
            .filter(|relationship| {
                matches!(relationship.resolved_target(&main), Some(Ok(target)) if target == part.part)
            })
            .map(|relationship| relationship.id.clone())
            .collect();
        for id in doomed {
            relationships.remove(&id);
        }
        if self.package_mut().set_relationships(&relationships).is_err() {
            return false;
        }
        if let Ok(own) = self.package().relationships(&part.part) {
            for relationship in own.by_type(CUSTOM_XML_PROPS) {
                if let Some(Ok(props)) = relationship.resolved_target(&part.part) {
                    self.package_mut().remove_part(&props);
                }
            }
            let empty = Relationships::new(&part.part);
            let _ = self.package_mut().set_relationships(&empty);
        }
        self.package_mut().remove_part(&part.part);
        self.note_change();
        true
    }

    /// What a binding's node says.
    #[must_use]
    pub fn custom_xml_text(&self, binding: &Binding) -> Option<String> {
        self.custom_xml_part(&binding.store_item)?.text_at(binding)
    }

    /// Writes a binding's node, and the part it is in.
    pub fn set_custom_xml_text(&mut self, binding: &Binding, text: &str) -> bool {
        let Some(part) = self.custom_xml_part(&binding.store_item) else { return false };
        let Some(Ok(xml)) = self.package().xml_part(&part.part) else { return false };
        let Ok(mut tree) = XmlTree::parse(&xml) else { return false };
        let Some((path, attribute)) = locate(&tree.root, binding) else { return false };
        let Some(element) = element_at_mut(&mut tree.root, &path) else { return false };
        match attribute {
            Some(name) => {
                let written = element
                    .attributes
                    .iter()
                    .find(|attribute| {
                        attribute.name == name
                            || attribute.name.rsplit(':').next() == Some(name.as_str())
                    })
                    .map(|attribute| attribute.name.clone())
                    .unwrap_or(name);
                element.set_attribute(&written, text);
            }
            None => {
                if element.text_content() == text {
                    return true;
                }
                element.set_text(text);
            }
        }
        let Ok(written) = tree.to_xml() else { return false };
        self.package_mut().set_part(&part.part, written.into_bytes());
        self.note_change();
        true
    }

    /// Binds the control at a position to a node, and shows what the node
    /// says.
    pub fn bind_control(&mut self, at: TextPosition, binding: &Binding) -> bool {
        let Some(control) = self.control_at(at) else { return false };
        let bound = binding.clone();
        let changed = self.change_control(&control, move |properties, _, prefix| {
            let named = |local: &str| crate::edit::name_with(prefix, local);
            properties.remove_children_named(Some(read::W), "dataBinding");
            let mut element = Element::new(&named("dataBinding"), Some(read::W));
            element.set_namespaced_attribute(&named("prefixMappings"), read::W, &bound.prefixes);
            element.set_namespaced_attribute(&named("xpath"), read::W, &bound.xpath);
            element.set_namespaced_attribute(&named("storeItemID"), read::W, &bound.store_item);
            // Before the element that says what kind the control is, which
            // the schema puts last.
            let kind_at = properties
                .children
                .iter()
                .position(|child| {
                    child.as_element().is_some_and(|child| {
                        matches!(
                            child.local_name(),
                            "text" | "comboBox" | "dropDownList" | "date" | "checkbox" | "richText"
                        )
                    })
                })
                .unwrap_or(properties.children.len());
            properties.insert_element(kind_at, element);
        });
        if !changed {
            return false;
        }
        if let Some(text) = self.custom_xml_text(binding) {
            self.show_bound(control.start, &text);
        }
        true
    }

    /// Puts a node's text into a control, in the way its kind takes it.
    fn show_bound(&mut self, start: TextPosition, text: &str) {
        let Some(control) = self.control_at(start) else { return };
        if control.kind == crate::controls::ControlKind::CheckBox {
            let on = matches!(text.trim(), "true" | "1");
            if control.checked != on {
                self.set_control_checked(start, on);
            }
            return;
        }
        let shown = self.control_text(&control);
        if shown != text {
            self.set_control_text(start, text);
        }
    }

    /// What a control shows now.
    #[must_use]
    pub fn control_text(&self, control: &crate::controls::Control) -> String {
        let text = self.paragraph_text(control.start.paragraph).unwrap_or_default();
        // The offsets are bytes, and only whole characters are taken.
        let boundary = |wanted: usize| {
            text.char_indices().map(|(at, _)| at).find(|at| *at >= wanted).unwrap_or(text.len())
        };
        let from = boundary(control.start.offset);
        let to = boundary(control.end.offset).max(from);
        text.get(from..to).unwrap_or_default().to_owned()
    }

    /// Brings every bound control to what its node says, which is what Word
    /// does when a document is opened. Not an edit: the document is as it
    /// was opened, and there is nothing to undo.
    pub fn refresh_bound_controls(&mut self) -> usize {
        let was_modified = self.is_modified();
        let mut changed = 0usize;
        for control in self.controls() {
            let Some(binding) = &control.binding else { continue };
            let Some(text) = self.custom_xml_text(binding) else { continue };
            let before = self.control_text(&control);
            self.show_bound(control.start, &text);
            if self.control_at(control.start).is_some_and(|now| self.control_text(&now) != before) {
                changed += 1;
            }
        }
        if changed > 0 {
            self.forget_history();
        }
        // A document that was as it is on disk still counts as that: what was
        // brought up to date is what the file says, read the way Word reads
        // it. One that was changed stays changed, which it is anyway.
        if !was_modified {
            self.count_as_saved();
        }
        changed
    }

    /// Writes what every bound control shows into its node, which is the
    /// other half of the binding, and gives back how many nodes changed.
    pub fn store_bound_controls(&mut self) -> usize {
        let mut changed = 0usize;
        for control in self.controls() {
            let Some(binding) = control.binding.clone() else { continue };
            let text = if control.kind == crate::controls::ControlKind::CheckBox {
                if control.checked {
                    "true".to_owned()
                } else {
                    "false".to_owned()
                }
            } else {
                self.control_text(&control)
            };
            if self.custom_xml_text(&binding).is_some_and(|held| held == text) {
                continue;
            }
            if self.set_custom_xml_text(&binding, &text) {
                changed += 1;
            }
        }
        changed
    }

    /// Writes the control at a position into its node, if it is bound.
    pub fn store_bound_control(&mut self, at: TextPosition) -> bool {
        let Some(control) = self.control_at(at) else { return false };
        let Some(binding) = control.binding.clone() else { return false };
        let text = if control.kind == crate::controls::ControlKind::CheckBox {
            if control.checked {
                "true".to_owned()
            } else {
                "false".to_owned()
            }
        } else {
            self.control_text(&control)
        };
        if self.custom_xml_text(&binding).is_some_and(|held| held == text) {
            return false;
        }
        self.set_custom_xml_text(&binding, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::ControlKind;
    use crate::model::{Block, Body, Paragraph};

    const ORDER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\
        <o:order xmlns:o=\"http://example.com/order\" o:number=\"41\">\r\n  \
        <o:customer>Habgood</o:customer>\r\n  <o:item>Nails</o:item>\r\n  \
        <o:item>Glue</o:item>\r\n</o:order>";

    fn document() -> Document {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Dear ")));
        Document::create(&body).expect("a document")
    }

    #[test]
    fn a_part_is_added_with_its_datastore_item_and_read_back() {
        let mut document = document();
        let id = document.add_custom_xml(ORDER).expect("the part");
        assert!(id.starts_with('{') && id.ends_with('}') && id.len() == 38, "{id}");

        let parts = document.custom_xml_parts();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].part, "customXml/item1.xml");
        assert_eq!(parts[0].id, id);
        assert_eq!(parts[0].label(), "http://example.com/order");

        // The package has what Word looks for: the item, its properties,
        // the relationship between them, and the content type.
        let package = document.package();
        assert!(package.part("customXml/itemProps1.xml").is_some());
        assert_eq!(package.content_type("customXml/itemProps1.xml"), Some(PROPS_CONTENT_TYPE));
        let own = package.relationships("customXml/item1.xml").expect("its relationships");
        assert!(own.single_by_type(CUSTOM_XML_PROPS).is_some());

        // And it survives a save and a reopening.
        let bytes = document.save().expect("saving");
        let again = Document::open(&bytes).expect("reopening");
        assert_eq!(again.custom_xml_parts()[0].id, id);
        let second = again.custom_xml_parts();
        assert_eq!(second[0].rows().len(), 5, "{:?}", second[0].rows());
    }

    #[test]
    fn a_path_leads_to_an_element_by_number_and_to_an_attribute() {
        let mut document = document();
        let id = document.add_custom_xml(ORDER).expect("the part");
        let binding = |xpath: &str| Binding {
            prefixes: "xmlns:o='http://example.com/order'".to_owned(),
            xpath: xpath.to_owned(),
            store_item: id.clone(),
        };
        assert_eq!(
            document.custom_xml_text(&binding("/o:order[1]/o:customer[1]")).as_deref(),
            Some("Habgood")
        );
        assert_eq!(
            document.custom_xml_text(&binding("/o:order[1]/o:item[2]")).as_deref(),
            Some("Glue")
        );
        assert_eq!(
            document.custom_xml_text(&binding("/o:order[1]/@o:number")).as_deref(),
            Some("41")
        );
        assert_eq!(document.custom_xml_text(&binding("/o:order[1]/o:item[3]")), None);
        assert_eq!(document.custom_xml_text(&binding("/o:nothing[1]")), None);
        // A prefix the mappings do not declare leads nowhere.
        let unknown = Binding { prefixes: String::new(), ..binding("/o:order[1]") };
        assert_eq!(document.custom_xml_text(&unknown), None);
    }

    #[test]
    fn a_bound_control_shows_the_node_and_writing_in_it_writes_the_node() {
        let mut document = document();
        let id = document.add_custom_xml(ORDER).expect("the part");
        let end = document.paragraph_text(0).unwrap_or_default().chars().count();
        document.set_caret(TextPosition::new(0, end));
        assert!(document.insert_control(ControlKind::PlainText, "Customer", &[]));
        let control = document.controls().pop().expect("the control");

        let part = document.custom_xml_parts().pop().expect("the part");
        assert_eq!(part.id, id);
        let rows = part.rows();
        let customer = rows.iter().find(|row| row.name == "customer").expect("the row");
        let binding = Binding::to_node(&part, customer);
        assert_eq!(binding.xpath, "/ns0:order[1]/ns0:customer[1]");
        assert_eq!(binding.prefixes, "xmlns:ns0='http://example.com/order'");
        // A path counts elements and not the text between them.
        let glue = rows.iter().find(|row| row.value == "Glue").expect("the row");
        assert_eq!(Binding::to_node(&part, glue).xpath, "/ns0:order[1]/ns0:item[2]");

        assert!(document.bind_control(control.start, &binding));
        assert_eq!(document.plain_text().trim(), "Dear Habgood");
        let control = document.controls().pop().expect("the control");
        assert_eq!(control.binding.as_ref(), Some(&binding));

        // Writing in the control writes the node.
        assert!(document.set_control_text(control.start, "Okafor"));
        assert!(document.store_bound_control(control.start));
        assert_eq!(document.custom_xml_text(&binding).as_deref(), Some("Okafor"));

        // And the other way about: the node changed under the document, the
        // control follows when the document is refreshed, as on opening.
        assert!(document.set_custom_xml_text(&binding, "Mbeki"));
        assert_eq!(document.refresh_bound_controls(), 1);
        assert_eq!(document.plain_text().trim(), "Dear Mbeki");

        // All of it survives a save and a reopening.
        let bytes = document.save().expect("saving");
        let again = Document::open(&bytes).expect("reopening");
        let control = again.controls().pop().expect("the control again");
        assert_eq!(
            control.binding.as_ref().map(|held| held.xpath.as_str()),
            Some(binding.xpath.as_str())
        );
        assert_eq!(again.custom_xml_text(&binding).as_deref(), Some("Mbeki"));
    }

    #[test]
    fn a_tick_box_is_bound_to_true_and_false() {
        let mut document = document();
        let id = document
            .add_custom_xml("<f:form xmlns:f=\"urn:form\"><f:agreed>true</f:agreed></f:form>")
            .expect("the part");
        document.set_caret(TextPosition::new(0, 0));
        assert!(document.insert_control(ControlKind::CheckBox, "", &[]));
        let control = document.controls().pop().expect("the control");
        let binding = Binding {
            prefixes: "xmlns:f='urn:form'".to_owned(),
            xpath: "/f:form[1]/f:agreed[1]".to_owned(),
            store_item: id,
        };
        assert!(document.bind_control(control.start, &binding));
        assert!(document.controls()[0].checked, "the box did not follow the node");
        assert!(document.set_control_checked(control.start, false));
        assert!(document.store_bound_control(control.start));
        assert_eq!(document.custom_xml_text(&binding).as_deref(), Some("false"));
    }

    #[test]
    fn a_part_taken_out_takes_its_item_and_relationships_with_it() {
        let mut document = document();
        let id = document.add_custom_xml(ORDER).expect("the part");
        assert!(document.remove_custom_xml(&id));
        assert!(document.custom_xml_parts().is_empty());
        assert!(document.package().part("customXml/item1.xml").is_none());
        assert!(document.package().part("customXml/itemProps1.xml").is_none());
        assert!(!document.remove_custom_xml(&id));
        // The next part added takes the number that is free.
        document.add_custom_xml(ORDER).expect("again");
        assert_eq!(document.custom_xml_parts()[0].part, "customXml/item1.xml");
    }

    #[test]
    fn xml_that_does_not_parse_is_refused() {
        let mut document = document();
        assert!(document.add_custom_xml("<order><customer></order>").is_none());
        assert!(document.custom_xml_parts().is_empty());
    }
}

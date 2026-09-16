//! Canonical XML: one way of writing a piece of XML, so that it can be
//! signed.
//!
//! # Why a signature needs this
//!
//! A signature is over bytes, and the same XML can be written as many
//! different runs of bytes. `<a x="1" y="2"/>` and `<a y="2" x="1"></a>` say
//! exactly the same thing and hash to different numbers. A document that went
//! through any program at all between being signed and being checked would
//! fail, not because anybody changed it but because the program wrote its
//! attributes in another order.
//!
//! So the standard lays down one way: attributes in a fixed order, empty
//! elements written open and shut, one set of escapes, no comments, and every
//! namespace the element inherits written out on it.
//!
//! # Which of the two this is
//!
//! Canonical XML 1.0, without comments, which is the one Office names in
//! every signature it writes. Its distinguishing rule is that a piece cut out
//! of a document carries its whole namespace context with it: `<SignedInfo>`
//! inside a `<Signature xmlns="…">` is written with that `xmlns` on it,
//! because otherwise the piece would not mean what it meant in the document.
//!
//! The other one — exclusive canonicalisation — writes only the namespaces
//! the piece actually uses. It is not here because nothing this program reads
//! or writes asks for it, and a canonicalisation that is never exercised is a
//! canonicalisation that is quietly wrong.

use std::collections::BTreeMap;

use wp_xml::tree::{Element, Node};

/// The name the standard goes by, as a signature writes it.
pub const NAME: &str = "http://www.w3.org/TR/2001/REC-xml-c14n-20010315";

/// One element and everything under it, written the one way.
///
/// `inherited` is the namespaces in scope from the element's ancestors, which
/// a caller that has the whole document works out with [`context`].
#[must_use]
pub fn canonical(element: &Element, inherited: &[(Option<String>, String)]) -> String {
    let mut rendered: BTreeMap<Option<String>, String> = BTreeMap::new();
    let mut scope: BTreeMap<Option<String>, String> = BTreeMap::new();
    for (prefix, uri) in inherited {
        scope.insert(prefix.clone(), uri.clone());
    }
    let mut out = String::new();
    write(element, &scope, &mut rendered.clone(), &mut out);
    let _ = &mut rendered;
    out
}

/// The namespaces in scope at an element, gathered walking down to it.
///
/// Given as a path of ancestors, outermost first, not counting the element
/// itself.
#[must_use]
pub fn context(ancestors: &[&Element]) -> Vec<(Option<String>, String)> {
    let mut scope: BTreeMap<Option<String>, String> = BTreeMap::new();
    for ancestor in ancestors {
        for (prefix, uri) in &ancestor.declarations {
            scope.insert(prefix.clone(), uri.clone());
        }
    }
    scope.into_iter().collect()
}

/// Writes one element, given what is in scope and what the output ancestors
/// have already declared.
fn write(
    element: &Element,
    scope: &BTreeMap<Option<String>, String>,
    rendered: &mut BTreeMap<Option<String>, String>,
    out: &mut String,
) {
    // What is in scope here: what was in scope outside, with this element's
    // own declarations on top.
    let mut here = scope.clone();
    for (prefix, uri) in &element.declarations {
        here.insert(prefix.clone(), uri.clone());
    }

    // What has to be written: every declaration in scope that the output
    // ancestors have not already made, in the standard's order — the default
    // first, then by prefix.
    let mut to_write: Vec<(&Option<String>, &String)> = Vec::new();
    for (prefix, uri) in &here {
        if rendered.get(prefix) == Some(uri) {
            continue;
        }
        // A default namespace of nothing is only worth writing where an
        // ancestor wrote a real one to be undone.
        if prefix.is_none() && uri.is_empty() && !rendered.contains_key(&None) {
            continue;
        }
        to_write.push((prefix, uri));
    }
    to_write.sort_by(|(left, _), (right, _)| match (left, right) {
        (None, None) => core::cmp::Ordering::Equal,
        (None, Some(_)) => core::cmp::Ordering::Less,
        (Some(_), None) => core::cmp::Ordering::Greater,
        (Some(one), Some(other)) => one.cmp(other),
    });

    out.push('<');
    out.push_str(&element.name);
    for (prefix, uri) in &to_write {
        match prefix {
            None => out.push_str(" xmlns=\""),
            Some(prefix) => {
                out.push_str(" xmlns:");
                out.push_str(prefix);
                out.push_str("=\"");
            }
        }
        out.push_str(&escaped_attribute(uri));
        out.push('"');
    }

    // The attributes, in the standard's order: those in no namespace first,
    // in the order of their names, then the rest by namespace and then name.
    let mut attributes: Vec<_> =
        element.attributes.iter().filter(|attribute| !is_a_declaration(&attribute.name)).collect();
    attributes.sort_by(|left, right| {
        let namespace = left
            .namespace
            .as_deref()
            .unwrap_or_default()
            .cmp(right.namespace.as_deref().unwrap_or_default());
        namespace.then_with(|| local_of(&left.name).cmp(local_of(&right.name)))
    });
    for attribute in attributes {
        out.push(' ');
        out.push_str(&attribute.name);
        out.push_str("=\"");
        out.push_str(&escaped_attribute(&attribute.value));
        out.push('"');
    }
    out.push('>');

    // Everything inside. An element written `<x/>` is written `<x></x>` here,
    // which is what falling through to the closing tag does.
    let mut passed_on = rendered.clone();
    for (prefix, uri) in to_write {
        passed_on.insert(prefix.clone(), uri.clone());
    }
    for child in &element.children {
        match child {
            Node::Element(child) => write(child, &here, &mut passed_on.clone(), out),
            Node::Text(text) | Node::CData(text) => out.push_str(&escaped_text(text)),
            // Without comments, which is the variant every signature names.
            Node::Comment(_) => {}
            Node::ProcessingInstruction { target, data } => {
                out.push_str("<?");
                out.push_str(target);
                if !data.is_empty() {
                    out.push(' ');
                    out.push_str(data);
                }
                out.push_str("?>");
            }
        }
    }

    out.push_str("</");
    out.push_str(&element.name);
    out.push('>');
}

/// Whether an attribute is really a namespace declaration.
fn is_a_declaration(name: &str) -> bool {
    name == "xmlns" || name.starts_with("xmlns:")
}

fn local_of(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Text, escaped as the standard says.
///
/// The greater-than sign is escaped here and not in an attribute, and a
/// carriage return is escaped in both — which looks arbitrary and is not: a
/// carriage return that went through unescaped would be turned into a line
/// feed by the next parser to read it, and the bytes would no longer be the
/// bytes that were signed.
fn escaped_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            other => out.push(other),
        }
    }
    out
}

fn escaped_attribute(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_xml::tree::XmlTree;

    fn of(source: &str) -> String {
        let tree = XmlTree::parse(source).expect("valid XML");
        canonical(&tree.root, &[])
    }

    #[test]
    fn an_empty_element_is_written_open_and_shut() {
        assert_eq!(of("<a/>"), "<a></a>");
        assert_eq!(of("<a></a>"), "<a></a>");
        assert_eq!(of("<a>  </a>"), "<a>  </a>", "space inside an element is its text");
    }

    #[test]
    fn attributes_come_out_in_one_order_whatever_order_they_went_in() {
        assert_eq!(of(r#"<a y="2" x="1"/>"#), r#"<a x="1" y="2"></a>"#);
        assert_eq!(of(r#"<a x="1" y="2"/>"#), r#"<a x="1" y="2"></a>"#);
    }

    #[test]
    fn a_namespace_is_written_before_the_attributes_and_the_default_first() {
        let source = r#"<a:root z="1" xmlns:a="urn:a" xmlns="urn:d" a:y="2"/>"#;
        assert_eq!(of(source), r#"<a:root xmlns="urn:d" xmlns:a="urn:a" z="1" a:y="2"></a:root>"#);
    }

    #[test]
    fn a_namespace_the_parent_already_declared_is_not_declared_again() {
        let source = r#"<root xmlns="urn:d"><child xmlns="urn:d"><leaf/></child></root>"#;
        assert_eq!(of(source), r#"<root xmlns="urn:d"><child><leaf></leaf></child></root>"#);
    }

    #[test]
    fn a_piece_cut_out_of_a_document_carries_its_namespaces_with_it() {
        let tree = XmlTree::parse(
            r#"<Signature xmlns="urn:dsig"><SignedInfo><X/></SignedInfo></Signature>"#,
        )
        .expect("valid XML");
        let inside = tree.root.child_elements().next().expect("the SignedInfo");
        let scope = context(&[&tree.root]);
        assert_eq!(
            canonical(inside, &scope),
            r#"<SignedInfo xmlns="urn:dsig"><X></X></SignedInfo>"#,
            "the piece does not say what it meant in the document"
        );
    }

    #[test]
    fn a_namespace_nothing_inside_uses_is_carried_across_all_the_same() {
        // What makes this the inclusive canonicalisation and not the other.
        let tree =
            XmlTree::parse(r#"<r xmlns:unused="urn:u" xmlns="urn:d"><c/></r>"#).expect("valid XML");
        let inside = tree.root.child_elements().next().expect("the child");
        let scope = context(&[&tree.root]);
        assert_eq!(canonical(inside, &scope), r#"<c xmlns="urn:d" xmlns:unused="urn:u"></c>"#);
    }

    #[test]
    fn the_escapes_are_the_ones_the_standard_names() {
        assert_eq!(of("<a>&lt;&amp;&gt;</a>"), "<a>&lt;&amp;&gt;</a>");
        assert_eq!(of(r#"<a x="&quot;&amp;&lt;"/>"#), r#"<a x="&quot;&amp;&lt;"></a>"#);
        // A tab in an attribute is escaped; in text it is not.
        assert_eq!(of("<a x=\"&#9;\">\t</a>"), "<a x=\"&#x9;\">\t</a>");
        // A greater-than is escaped in text and left alone in an attribute.
        assert_eq!(of(r#"<a x="&gt;">&gt;</a>"#), r#"<a x=">">&gt;</a>"#);
    }

    #[test]
    fn a_comment_is_left_out() {
        assert_eq!(of("<a><!-- nothing --><b/></a>"), "<a><b></b></a>");
    }
}

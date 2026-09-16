//! Canonical XML, held to somebody else's idea of it.
//!
//! `xmllint` is in the build image and implements the same standard. A
//! canonicalisation that only agreed with itself would be no use at all: the
//! whole point of it is that two different programs, given the same document,
//! write the same bytes.

use std::io::Write;
use std::process::{Command, Stdio};

/// What xmllint makes of a document.
fn xmllint(source: &str) -> String {
    let mut child = Command::new("xmllint")
        .args(["--c14n", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("xmllint is in the build image");
    child.stdin.take().expect("its input").write_all(source.as_bytes()).expect("writing");
    let output = child.wait_with_output().expect("waiting");
    assert!(
        output.status.success(),
        "xmllint refused {source:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("canonical XML is UTF-8")
}

fn ours(source: &str) -> String {
    let tree = wp_xml::tree::XmlTree::parse(source).expect("valid XML");
    wp_sign::c14n::canonical(&tree.root, &[])
}

/// Every shape that has ever been got wrong.
const DOCUMENTS: &[&str] = &[
    r#"<a/>"#,
    r#"<a></a>"#,
    r#"<a>text</a>"#,
    r#"<a> spaced  text </a>"#,
    r#"<a y="2" x="1"/>"#,
    r#"<a b="1" a="2" c="3"><b/><c x="1"/></a>"#,
    r#"<a xmlns="urn:d"><b/></a>"#,
    r#"<a xmlns="urn:d"><b xmlns="urn:d"><c/></b></a>"#,
    r#"<a xmlns="urn:d"><b xmlns="urn:e"><c/></b></a>"#,
    r#"<p:a xmlns:p="urn:p" xmlns:q="urn:q" q:x="1" p:y="2" z="3"/>"#,
    r#"<p:a xmlns:p="urn:p"><p:b><q:c xmlns:q="urn:q"/></p:b></p:a>"#,
    r#"<a xmlns:unused="urn:u"><b/></a>"#,
    r#"<a>&lt;&amp;&gt;&quot;</a>"#,
    r#"<a x="&lt;&amp;&quot;&#9;&#10;"/>"#,
    r#"<a><![CDATA[some <raw> text]]></a>"#,
    r#"<a><?target some data?><b/></a>"#,
    r#"<a xmlns="urn:d" xmlns:p="urn:p"><p:b><c/></p:b></a>"#,
    // The shape a signature actually has.
    r##"<Signature xmlns="http://www.w3.org/2000/09/xmldsig#"><SignedInfo><CanonicalizationMethod Algorithm="http://www.w3.org/TR/2001/REC-xml-c14n-20010315"/><SignatureMethod Algorithm="http://www.w3.org/2001/04/xmldsig-more#rsa-sha256"/><Reference URI="#idPackageObject" Type="http://www.w3.org/2000/09/xmldsig#Object"><DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/><DigestValue>abc=</DigestValue></Reference></SignedInfo></Signature>"##,
];

#[test]
fn every_document_comes_out_the_way_xmllint_writes_it() {
    for source in DOCUMENTS {
        assert_eq!(ours(source), xmllint(source), "canonicalising {source}");
    }
}

/// The case the whole thing exists for: a piece cut out of a document,
/// carrying the namespaces it inherited.
///
/// xmllint has no way of being asked for a subtree with a given namespace
/// context, so the same thing is put to it the only way it can be: as a
/// document whose root is that piece, with the inherited namespaces written
/// on it. That is what the canonical form of such a piece is *defined* to be,
/// and it is what this program has to produce from a piece still inside its
/// document.
#[test]
fn a_piece_of_a_document_is_written_as_that_piece_standing_alone() {
    let whole = r##"<Signature xmlns="http://www.w3.org/2000/09/xmldsig#" xmlns:m="urn:office"><SignedInfo><Reference URI="#x"><DigestValue>abc=</DigestValue></Reference></SignedInfo></Signature>"##;
    let alone = r##"<SignedInfo xmlns="http://www.w3.org/2000/09/xmldsig#" xmlns:m="urn:office"><Reference URI="#x"><DigestValue>abc=</DigestValue></Reference></SignedInfo>"##;

    let tree = wp_xml::tree::XmlTree::parse(whole).expect("valid XML");
    let inside = tree.root.child_elements().next().expect("the SignedInfo");
    let scope = wp_sign::c14n::context(&[&tree.root]);
    assert_eq!(wp_sign::c14n::canonical(inside, &scope), xmllint(alone));
}

/// The one place the two tools do not agree, and why.
///
/// `xmllint --c14n` writes the comments; the variant a signature names
/// does not. Both are Canonical XML 1.0 — the standard defines the two —
/// so what is put to xmllint here is the same document with the comments
/// already gone, which is what the without-comments form is defined to be.
#[test]
fn a_comment_is_left_out_and_nothing_else_changes() {
    let with = r#"<a><!-- a comment --><b/><!-- another --><c x="1"/></a>"#;
    let without = r#"<a><b/><c x="1"/></a>"#;
    assert_eq!(ours(with), xmllint(without));
}

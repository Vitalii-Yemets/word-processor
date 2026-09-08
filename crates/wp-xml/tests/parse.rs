//! Tests for the XML reader and writer.

use std::borrow::Cow;

use wp_xml::{Error, ErrorKind, Event, Reader, Writer};

/// A fragment shaped like the body of a real `document.xml`.
const DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>
    <w:p>
      <w:pPr><w:pStyle w:val="Heading1"/><w:jc w:val="center"/></w:pPr>
      <w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">Hello, </w:t></w:r>
      <w:r><w:t>world</w:t></w:r>
    </w:p>
    <w:p><w:r><w:t>Многоязычный текст: 文書, نص, मूल</w:t></w:r></w:p>
  </w:body>
</w:document>"#;

const WORDPROCESSING_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn parse(input: &str) -> Result<Vec<Event<'_>>, Error> {
    Reader::new(input).into_events()
}

#[test]
fn reads_a_document_and_its_text() {
    let text = Reader::text_content(DOCUMENT).unwrap();
    assert!(text.contains("Hello, "));
    assert!(text.contains("world"));
    assert!(text.contains("Многоязычный текст: 文書, نص, मूल"));
}

#[test]
fn resolves_namespaces_from_prefixes() {
    let events = parse(DOCUMENT).unwrap();

    let document = events
        .iter()
        .find_map(|event| match event {
            Event::Start(tag) if tag.name.local == "document" => Some(tag),
            _ => None,
        })
        .expect("the root element should be present");

    assert_eq!(document.name.prefix, Some("w"));
    assert_eq!(document.namespace, Some(WORDPROCESSING_NAMESPACE));

    // The prefix stays available, because a document has to be written back the
    // way it came rather than renamed to something equivalent.
    assert_eq!(document.name.to_written(), "w:document");
}

#[test]
fn attribute_lookup_works_by_namespace_and_by_written_name() {
    let events = parse(DOCUMENT).unwrap();
    let style = events
        .iter()
        .find_map(|event| match event {
            Event::Empty(tag) if tag.name.local == "pStyle" => Some(tag),
            _ => None,
        })
        .expect("w:pStyle should be present");

    assert_eq!(style.attribute(Some(WORDPROCESSING_NAMESPACE), "val"), Some("Heading1"));
    assert_eq!(style.attribute_by_written_name("w:val"), Some("Heading1"));
    // An unprefixed name is a different attribute, not a fallback.
    assert_eq!(style.attribute(None, "val"), None);
}

#[test]
fn the_reserved_xml_prefix_needs_no_declaration() {
    let events = parse(DOCUMENT).unwrap();
    let text_element = events
        .iter()
        .find_map(|event| match event {
            Event::Start(tag) if tag.name.local == "t" => Some(tag),
            _ => None,
        })
        .expect("w:t should be present");

    assert_eq!(text_element.attribute(Some(wp_xml::XML_NAMESPACE), "space"), Some("preserve"));
}

#[test]
fn whitespace_in_content_is_reported_exactly() {
    // With xml:space="preserve" the trailing space is content. A parser that
    // trimmed it would silently join two words in the finished document.
    let events = parse(r#"<w:t xmlns:w="urn:w" xml:space="preserve">Hello, </w:t>"#).unwrap();
    let text: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            Event::Text(text) => Some(text.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(text, ["Hello, "]);
}

#[test]
fn text_without_entities_borrows_from_the_input() {
    let events = parse("<t>plain content</t>").unwrap();
    assert!(matches!(&events[0], Event::Start(_)));
    assert!(
        matches!(&events[1], Event::Text(Cow::Borrowed(_))),
        "unchanged text should not be copied"
    );
}

#[test]
fn distinguishes_empty_elements_from_pairs() {
    let events = parse("<a><b/><c></c></a>").unwrap();
    assert!(matches!(events[1], Event::Empty(_)));
    assert!(matches!(events[2], Event::Start(_)));
    assert!(matches!(events[3], Event::End(_)));
}

#[test]
fn reads_comments_cdata_and_processing_instructions() {
    let source = "<?custom target?><!-- a note --><a><![CDATA[raw <not> markup]]></a>";
    let events = parse(source).unwrap();

    assert!(matches!(events[0], Event::ProcessingInstruction { target: "custom", data: "target" }));
    assert!(matches!(events[1], Event::Comment(" a note ")));
    assert!(matches!(events[3], Event::CData("raw <not> markup")));
}

#[test]
fn reads_the_declaration() {
    let events = parse(DOCUMENT).unwrap();
    assert_eq!(
        events[0],
        Event::Declaration { version: "1.0", encoding: Some("UTF-8"), standalone: Some(true) }
    );
}

// --- Round-tripping --------------------------------------------------------

/// Parses, writes back, and parses again, requiring the two event streams to
/// agree.
///
/// Byte equality would be the wrong test: XML lets a document be written several
/// equivalent ways, and the parser is required to normalize line endings and
/// attribute whitespace. What must not change is what the document *says*.
fn assert_roundtrips(source: &str) {
    let original = parse(source).unwrap_or_else(|error| panic!("parsing {source:?}: {error}"));

    let mut writer = Writer::new();
    for event in &original {
        writer.write_event(event).unwrap_or_else(|error| panic!("writing {event:?}: {error}"));
    }
    let rewritten = writer.finish().expect("all elements should be closed");

    let reparsed = parse(&rewritten)
        .unwrap_or_else(|error| panic!("re-parsing our own output failed: {error}\n{rewritten}"));

    assert_eq!(original, reparsed, "round trip changed the document\nrewritten: {rewritten}");
}

#[test]
fn documents_survive_a_round_trip() {
    for source in [
        DOCUMENT,
        "<a/>",
        "<a></a>",
        "<a>text</a>",
        "<a b=\"1\" c=\"2\"/>",
        "<a xmlns=\"urn:default\"><b/></a>",
        "<a xmlns:p=\"urn:p\"><p:b p:c=\"value\"/></a>",
        "<a>&amp; &lt; &gt; &quot; &apos;</a>",
        "<a><![CDATA[literal <tags> & ampersands]]></a>",
        "<a><!-- comment --><?pi data?><b/></a>",
        "<a>многоязычный 文書 نص 🖋</a>",
        "<a b=\"значение\"/>",
        "<w:t xmlns:w=\"urn:w\" xml:space=\"preserve\">  spaced  </w:t>",
    ] {
        assert_roundtrips(source);
    }
}

#[test]
fn writer_produces_the_expected_markup() {
    let mut writer = Writer::new();
    writer.write_declaration(Some(true));
    writer.write_start("w:p", &[]).unwrap();
    writer.write_start("w:r", &[]).unwrap();
    writer.write_start("w:t", &[("xml:space", "preserve")]).unwrap();
    writer.write_text("a < b & c");
    writer.write_end("w:t").unwrap();
    writer.write_end("w:r").unwrap();
    writer.write_end("w:p").unwrap();

    assert_eq!(
        writer.finish().unwrap(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:p><w:r><w:t xml:space=\"preserve\">a &lt; b &amp; c</w:t></w:r></w:p>"
    );
}

#[test]
fn writer_refuses_a_mismatched_end_tag() {
    let mut writer = Writer::new();
    writer.write_start("a", &[]).unwrap();
    assert!(matches!(writer.write_end("b").unwrap_err().kind, ErrorKind::MismatchedEndTag { .. }));
}

#[test]
fn writer_refuses_to_finish_with_elements_open() {
    let mut writer = Writer::new();
    writer.write_start("a", &[]).unwrap();
    assert!(matches!(writer.finish().unwrap_err().kind, ErrorKind::UnclosedElements(_)));
}

// --- Rejecting malformed input ---------------------------------------------

/// Asserts that parsing fails, and returns the error for further checks.
fn expect_error(source: &str) -> Error {
    match parse(source) {
        Ok(events) => panic!("should have been rejected: {source:?}\nparsed as {events:?}"),
        Err(error) => error,
    }
}

#[test]
fn rejects_mismatched_tags() {
    assert!(matches!(expect_error("<a><b></a></b>").kind, ErrorKind::MismatchedEndTag { .. }));
    assert!(matches!(expect_error("<a><b></a>").kind, ErrorKind::MismatchedEndTag { .. }));
    assert!(matches!(expect_error("<a>").kind, ErrorKind::UnclosedElements(_)));
    assert!(matches!(expect_error("</a>").kind, ErrorKind::UnexpectedEndTag(_)));
}

#[test]
fn requires_exactly_one_root_element() {
    assert!(matches!(expect_error("<a/><b/>").kind, ErrorKind::RootElementCount(2)));
    assert!(matches!(expect_error("").kind, ErrorKind::RootElementCount(0)));
    assert!(matches!(expect_error("<!-- only a comment -->").kind, ErrorKind::RootElementCount(0)));
    // Whitespace and comments around the root are fine.
    assert!(parse("  <!-- before --> <a/> <!-- after -->  ").is_ok());
}

#[test]
fn rejects_text_outside_the_root_element() {
    assert!(matches!(expect_error("stray text<a/>").kind, ErrorKind::UnexpectedCharacter { .. }));
}

#[test]
fn rejects_undeclared_prefixes() {
    assert!(matches!(
        expect_error("<w:p/>").kind,
        ErrorKind::UndeclaredPrefix(prefix) if prefix == "w"
    ));
    // A declaration only reaches the element it is on and what is inside it.
    assert!(matches!(
        expect_error("<a><b xmlns:p=\"urn:p\"/><p:c/></a>").kind,
        ErrorKind::UndeclaredPrefix(_)
    ));
}

#[test]
fn rejects_duplicate_attributes() {
    assert!(matches!(expect_error("<a b=\"1\" b=\"2\"/>").kind, ErrorKind::DuplicateAttribute(_)));
    // Two prefixes for one namespace still name the same attribute.
    assert!(matches!(
        expect_error("<a xmlns:p=\"urn:x\" xmlns:q=\"urn:x\" p:b=\"1\" q:b=\"2\"/>").kind,
        ErrorKind::DuplicateAttribute(_)
    ));
}

#[test]
fn rejects_illegal_namespace_declarations() {
    for source in [
        "<a xmlns:xmlns=\"urn:x\"/>",
        "<a xmlns:xml=\"urn:wrong\"/>",
        "<a xmlns:p=\"\"/>",
        "<a xmlns:p=\"http://www.w3.org/2000/xmlns/\"/>",
    ] {
        assert!(
            matches!(expect_error(source).kind, ErrorKind::IllegalNamespaceDeclaration(_)),
            "should have been rejected: {source}"
        );
    }
}

#[test]
fn rejects_malformed_markup() {
    for source in [
        "<a b/>",              // attribute with no value
        "<a b=unquoted/>",     // value without quotes
        "<a b=\"unclosed/>",   // value quote never closed
        "<a",                  // tag never closed
        "<1a/>",               // name cannot start with a digit
        "<a b=\"1\"c=\"2\"/>", // no whitespace between attributes
        "<!-- a -- b -->",     // "--" inside a comment
        "<a><![CDATA[unterminated</a>",
        "<a>]]></a>", // the literal "]]>" in content
    ] {
        expect_error(source);
    }
}

#[test]
fn rejects_unknown_entities() {
    // &nbsp; is HTML, not XML. Silently accepting it would put a character in
    // the document that the source never said.
    assert!(matches!(expect_error("<a>&nbsp;</a>").kind, ErrorKind::UnknownEntity(_)));
    assert!(matches!(expect_error("<a>&whatever;</a>").kind, ErrorKind::UnknownEntity(_)));
}

#[test]
fn does_not_expand_entities_declared_in_a_doctype() {
    // The "billion laughs" attack: nested entity definitions that expand
    // exponentially. Custom entities are never expanded, so the reference is
    // simply unknown and the document is refused.
    let bomb = r#"<!DOCTYPE lolz [
        <!ENTITY lol "lol">
        <!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
        <!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">
    ]>
    <lolz>&lol3;</lolz>"#;

    assert!(matches!(expect_error(bomb).kind, ErrorKind::UnknownEntity(_)));
}

#[test]
fn does_not_resolve_external_entities() {
    // The other classic: an external entity pointed at a local file. Nothing is
    // fetched, and the reference is refused.
    let attack = r#"<!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
        <foo>&xxe;</foo>"#;

    assert!(matches!(expect_error(attack).kind, ErrorKind::UnknownEntity(_)));
}

#[test]
fn errors_carry_a_useful_position() {
    let error = expect_error("<a>\n  <b>\n</a>");
    assert!(matches!(error.kind, ErrorKind::MismatchedEndTag { .. }));
    assert_eq!(error.position.line, 3);
    assert_eq!(error.position.column, 1);
}

#[test]
fn arbitrary_input_never_panics() {
    // Fragments assembled from pieces of XML syntax, most of them nonsense.
    let pieces = [
        "<",
        ">",
        "/",
        "?",
        "!",
        "-",
        "[",
        "]",
        "&",
        ";",
        "\"",
        "'",
        "=",
        "a",
        " ",
        "\n",
        ":",
        "<a",
        "</",
        "<!--",
        "]]>",
        "<![CDATA[",
        "<?xml",
        "&#x",
        "\u{1F600}",
    ];

    let mut state = 0x1234_5678u64;
    for _ in 0..20_000 {
        let mut source = String::new();
        let length = (state % 12) + 1;
        for _ in 0..length {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            source.push_str(pieces[(state >> 33) as usize % pieces.len()]);
        }
        // Any outcome is fine; a panic is not.
        let _ = parse(&source);
    }
}

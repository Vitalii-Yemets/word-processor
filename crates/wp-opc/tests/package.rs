//! Tests for the package layer.

use wp_opc::{Package, Relationships, TargetMode};
use wp_zip::{Compression, DosDateTime, ZipWriter};

const MAIN_DOCUMENT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#;

/// Builds a package the way a real producer would, and hands back its bytes.
fn sample_package() -> Vec<u8> {
    let mut package = Package::empty();
    package.add_part(
        "word/document.xml",
        wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
        MAIN_DOCUMENT.to_vec(),
    );

    let mut root = Relationships::new("");
    root.add(wp_opc::OFFICE_DOCUMENT_RELATIONSHIP, "word/document.xml", TargetMode::Internal);
    package.set_relationships(&root).unwrap();

    package.save().unwrap()
}

#[test]
fn follows_the_relationship_to_the_main_document() {
    let package = Package::open(&sample_package()).unwrap();
    assert_eq!(package.main_document_part().unwrap(), "word/document.xml");
}

#[test]
fn falls_back_to_the_content_type_when_there_is_no_relationship() {
    // Some producers omit the package relationship. The content type names the
    // part just as definitely, so the document is still openable.
    let mut package = Package::empty();
    package.add_part(
        "word/document.xml",
        wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
        MAIN_DOCUMENT.to_vec(),
    );
    let bytes = package.save().unwrap();

    let reopened = Package::open(&bytes).unwrap();
    assert_eq!(reopened.main_document_part().unwrap(), "word/document.xml");
}

#[test]
fn a_package_with_no_main_document_is_reported_as_such() {
    let mut package = Package::empty();
    package.add_part("notes.txt", "text/plain", b"just a note".to_vec());
    let bytes = package.save().unwrap();

    assert_eq!(
        Package::open(&bytes).unwrap().main_document_part(),
        Err(wp_opc::Error::NoMainDocument)
    );
}

#[test]
fn a_file_that_is_not_a_package_is_refused() {
    // A perfectly good archive, but with no content types stream it is not a
    // package and nothing in it can be identified.
    let mut writer = ZipWriter::new();
    writer.add("hello.txt", b"content").unwrap();
    let archive = writer.finish().unwrap();

    assert_eq!(Package::open(&archive).unwrap_err(), wp_opc::Error::MissingContentTypes);
}

#[test]
fn parts_the_program_does_not_understand_are_carried_through_untouched() {
    // The promise the whole design rests on. A document holds far more than any
    // one program models — a macro project, an embedded font, a chart, someone
    // else's tracked changes. Saving must not quietly drop any of it.
    let unknown_parts: &[(&str, &[u8])] = &[
        ("word/vbaProject.bin", &[0xD0, 0xCF, 0x11, 0xE0, 0x00, 0x01, 0x02, 0x03]),
        ("word/fonts/font1.odttf", &[0x4F, 0x54, 0x54, 0x4F, 0xFF, 0xFE]),
        ("customXml/item1.xml", b"<root><unmodelled/></root>"),
        ("word/charts/chart1.xml", b"<c:chart xmlns:c=\"urn:chart\"/>"),
        ("word/media/image1.png", &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
    ];

    let mut package = Package::empty();
    package.add_part(
        "word/document.xml",
        wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
        MAIN_DOCUMENT.to_vec(),
    );
    for (name, data) in unknown_parts {
        package.add_part(name, "application/octet-stream", data.to_vec());
    }

    let mut root = Relationships::new("");
    root.add(wp_opc::OFFICE_DOCUMENT_RELATIONSHIP, "word/document.xml", TargetMode::Internal);
    package.set_relationships(&root).unwrap();

    let original = package.save().unwrap();

    // Open and save without touching anything, as an editor would when a user
    // opens a document and presses save.
    let reopened = Package::open(&original).unwrap();
    let saved = reopened.save().unwrap();

    assert_eq!(saved, original, "an untouched package changed on save");

    let after = Package::open(&saved).unwrap();
    for (name, data) in unknown_parts {
        assert_eq!(after.part(name), Some(*data), "part {name:?} did not survive");
    }
}

#[test]
fn entry_order_and_compression_are_preserved() {
    // Preserving these is what makes byte-identical saving possible at all.
    let mut writer = ZipWriter::new();
    writer.add("[Content_Types].xml", CONTENT_TYPES.as_bytes()).unwrap();
    writer.add_stored("word/media/image1.png", &[0x89, 0x50, 0x4E, 0x47]).unwrap();
    writer
        .add_with("word/document.xml", MAIN_DOCUMENT, Compression::Deflate, DosDateTime::EPOCH)
        .unwrap();
    let original = writer.finish().unwrap();

    let package = Package::open(&original).unwrap();
    let names: Vec<&str> = package.entries().iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["[Content_Types].xml", "word/media/image1.png", "word/document.xml"]);

    assert_eq!(package.entries()[1].compression, Compression::Stored);
    assert_eq!(package.entries()[2].compression, Compression::Deflate);
    assert_eq!(package.save().unwrap(), original);
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="png" ContentType="image/png"/>
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

#[test]
fn a_relationship_cannot_point_outside_the_package() {
    // A crafted document must not be able to name a file on the machine that
    // opens it.
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="rId1" Type="urn:test" Target="../../../etc/passwd"/>
    </Relationships>"#;

    let mut package = Package::empty();
    package.add_part(
        "word/document.xml",
        wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
        MAIN_DOCUMENT.to_vec(),
    );
    package.set_part("word/_rels/document.xml.rels", rels.as_bytes().to_vec());

    let relationships = package.relationships("word/document.xml").unwrap();
    let escaping = relationships.by_id("rId1").unwrap();

    assert!(matches!(
        escaping.resolved_target("word/document.xml"),
        Some(Err(wp_opc::Error::InvalidTarget { .. }))
    ));
}

#[test]
fn validation_reports_a_part_with_no_declared_type() {
    let mut package = Package::empty();
    package.add_part(
        "word/document.xml",
        wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
        MAIN_DOCUMENT.to_vec(),
    );
    // Added without declaring a type, which makes the package invalid.
    package.set_part("word/mystery.bin", vec![1, 2, 3]);

    let problems = package.validate();
    assert!(
        problems.iter().any(|problem| matches!(
            problem,
            wp_opc::Error::MissingPart(name) if name == "word/mystery.bin"
        )),
        "expected the undeclared part to be reported, got {problems:?}"
    );
}

#[test]
fn validation_reports_a_declared_part_that_is_absent() {
    let mut package = Package::empty();
    package.add_part(
        "word/document.xml",
        wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
        MAIN_DOCUMENT.to_vec(),
    );
    // Declared, then the file itself removed from under the declaration.
    let mut content_types = package.content_types().clone();
    content_types.set_override("word/ghost.xml", "application/xml");
    package.set_content_types(content_types);

    let problems = package.validate();
    assert!(
        problems.iter().any(|problem| matches!(
            problem,
            wp_opc::Error::MissingPart(name) if name == "word/ghost.xml"
        )),
        "expected the missing part to be reported, got {problems:?}"
    );
}

#[test]
fn part_lookup_ignores_ascii_case_and_the_leading_slash() {
    let package = Package::open(&sample_package()).unwrap();

    assert!(package.part("word/document.xml").is_some());
    assert!(package.part("/word/document.xml").is_some());
    assert!(package.part("WORD/DOCUMENT.XML").is_some());
}

#[test]
fn a_part_with_no_relationships_simply_has_none() {
    // Not an error: most parts declare no relationships at all.
    let package = Package::open(&sample_package()).unwrap();
    let relationships = package.relationships("word/document.xml").unwrap();
    assert!(relationships.all().is_empty());
}

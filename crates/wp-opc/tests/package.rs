//! Tests for the package layer.

/// Archives put together by hand, which no writer of this program made: see
/// the file itself.
#[path = "../../wp-zip/tests/foreign/mod.rs"]
mod foreign;

use wp_opc::{Package, Relationships, TargetMode};
use wp_zip::{Compression, DosDateTime, ZipArchive, ZipWriter};

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

#[test]
fn changing_the_content_types_is_a_write_like_any_other() {
    // The count is how a program holding something worked out from the parts
    // tells whether they changed, and the declarations are a part: a package
    // whose main part became a template's is not the package it was.
    let mut package = Package::open(&sample_package()).unwrap();
    let before = package.generation();

    let mut content_types = package.content_types().clone();
    content_types.set_override("word/document.xml", wp_opc::MAIN_DOCUMENT_TEMPLATE_CONTENT_TYPE);
    package.set_content_types(content_types);

    assert!(package.generation() > before, "the declarations changed and the count did not");
    let written: Vec<&str> = package.written_since(before).collect();
    assert_eq!(written, ["[Content_Types].xml"]);
}

#[test]
fn what_was_written_since_a_generation_is_named() {
    let mut package = Package::open(&sample_package()).unwrap();
    assert_eq!(package.written_since(0).count(), 0, "nothing is written by opening");

    package.set_part("word/document.xml", MAIN_DOCUMENT.to_vec());
    let after_the_document = package.generation();
    package.add_part("word/extra.xml", "application/xml", b"<x/>".to_vec());

    let mut since: Vec<&str> = package.written_since(after_the_document).collect();
    since.sort_unstable();
    assert_eq!(since, ["[Content_Types].xml", "word/extra.xml"]);

    package.remove_part("word/extra.xml");
    assert!(package.written_since(after_the_document).any(|name| name == "word/extra.xml"));
    assert!(!package.written_since(package.generation()).any(|_| true), "nothing after now");
}

#[test]
fn a_part_whose_type_is_declared_already_leaves_the_declarations_alone() {
    // Writing the declarations again the same would still count as writing
    // them, and would put this program's spelling of them over the one the
    // producer wrote.
    let mut package = Package::open(&sample_package()).unwrap();
    let declared = package.part("[Content_Types].xml").unwrap().to_vec();
    let before = package.generation();

    package.add_part(
        "word/document.xml",
        wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
        MAIN_DOCUMENT.to_vec(),
    );

    let written: Vec<&str> = package.written_since(before).collect();
    assert_eq!(written, ["word/document.xml"]);
    assert_eq!(package.part("[Content_Types].xml").unwrap(), declared.as_slice());
}

// --- What nothing reaches ----------------------------------------------------

const IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

/// A package whose document reaches a picture, and beside it a part nothing
/// reaches, which itself reaches another part nothing else does.
fn with_a_stray_part() -> Package {
    let mut package = Package::open(&sample_package()).unwrap();
    package.add_part("word/media/image1.png", "image/png", b"picture".to_vec());
    let mut own = package.relationships("word/document.xml").unwrap();
    own.add(IMAGE, "media/image1.png", TargetMode::Internal);
    own.add(IMAGE, "https://example.com/elsewhere.png", TargetMode::External);
    package.set_relationships(&own).unwrap();

    package.add_part("word/stray.xml", "application/xml", b"<stray/>".to_vec());
    package.add_part("word/media/behind.png", "image/png", b"behind".to_vec());
    let mut stray = Relationships::new("word/stray.xml");
    stray.add(IMAGE, "media/behind.png", TargetMode::Internal);
    package.set_relationships(&stray).unwrap();
    package
}

#[test]
fn a_part_nothing_reaches_goes_with_what_only_it_reaches() {
    let mut package = with_a_stray_part();
    assert!(package.prune_unreachable(|_, _, _| true));

    assert!(package.part("word/document.xml").is_some());
    assert!(package.part("word/media/image1.png").is_some(), "a part reached was taken out");
    assert!(package.part("word/stray.xml").is_none());
    assert!(package.part("word/_rels/stray.xml.rels").is_none());
    assert!(package.part("word/media/behind.png").is_none());
    // Nothing is left declared that is not there, and an address outside
    // the package is left alone.
    assert!(package.validate().is_empty(), "{:?}", package.validate());
    assert_eq!(package.relationships("word/document.xml").unwrap().all().len(), 2);
}

#[test]
fn a_relationship_that_no_longer_counts_goes_and_so_does_its_part() {
    let mut package = with_a_stray_part();
    assert!(package
        .prune_unreachable(|_, _, relationship| { relationship.target != "media/image1.png" }));
    assert!(package.part("word/media/image1.png").is_none());
    let left = package.relationships("word/document.xml").unwrap();
    assert!(left.all().iter().all(|relationship| relationship.target != "media/image1.png"));
    assert_eq!(left.all().len(), 1, "the address outside the package is not a part, and stays");
}

#[test]
fn nothing_goes_when_the_walk_does_not_come_to_the_document() {
    // No relationships of the package's own: nothing can say what is
    // reached, and taking everything out would lose the document.
    let mut package = with_a_stray_part();
    package.remove_part("_rels/.rels");
    assert!(!package.prune_unreachable(|_, _, _| true));
    assert!(package.part("word/document.xml").is_some());
    assert!(package.part("word/stray.xml").is_some());
}

#[test]
fn a_package_where_everything_is_reached_is_left_as_it_was() {
    let mut package = Package::open(&sample_package()).unwrap();
    let before = package.generation();
    assert!(!package.prune_unreachable(|_, _, _| true));
    assert_eq!(package.generation(), before, "something was written");
}

#[test]
fn a_walk_goes_up_out_of_a_folder_and_on_from_a_part_that_is_not_xml() {
    // Custom XML is reached from the document by `../customXml/item1.xml`
    // and reaches its datastore item from its own relationships; macros
    // are bytes, and reach their data all the same.
    let mut package = with_a_stray_part();
    let reach = |package: &mut Package, source: &str, target: &str| {
        let mut relationships = package.relationships(source).unwrap();
        relationships.add("urn:kind", target, TargetMode::Internal);
        package.set_relationships(&relationships).unwrap();
    };
    package.add_part("customXml/item1.xml", "application/xml", b"<data/>".to_vec());
    package.add_part("customXml/itemProps1.xml", "application/xml", b"<props/>".to_vec());
    reach(&mut package, "word/document.xml", "../customXml/item1.xml");
    reach(&mut package, "customXml/item1.xml", "itemProps1.xml");
    package.add_part("word/vbaProject.bin", "application/octet-stream", vec![0xD0, 0xCF, 0x11]);
    package.add_part("word/vbaData.xml", "application/xml", b"<data/>".to_vec());
    reach(&mut package, "word/document.xml", "vbaProject.bin");
    reach(&mut package, "word/vbaProject.bin", "vbaData.xml");

    assert!(package.prune_unreachable(|_, _, _| true));
    for kept in [
        "customXml/item1.xml",
        "customXml/_rels/item1.xml.rels",
        "customXml/itemProps1.xml",
        "word/vbaProject.bin",
        "word/_rels/vbaProject.bin.rels",
        "word/vbaData.xml",
    ] {
        assert!(package.part(kept).is_some(), "{kept} was taken out");
    }
    assert!(package.part("word/stray.xml").is_none());
}

#[test]
fn a_part_is_reached_whichever_way_its_name_is_spelled() {
    // A target is a URI and writes a space as %20; the archive may hold the
    // name either way, and both are the one part.
    let mut package = with_a_stray_part();
    package.add_part("word/media/a picture.png", "image/png", b"one".to_vec());
    package.add_part("word/media/another%20picture.png", "image/png", b"two".to_vec());
    let mut own = package.relationships("word/document.xml").unwrap();
    own.add(IMAGE, "media/a%20picture.png", TargetMode::Internal);
    own.add(IMAGE, "media/another picture.png", TargetMode::Internal);
    package.set_relationships(&own).unwrap();

    assert!(package.prune_unreachable(|_, _, _| true));
    assert!(package.part("word/media/a picture.png").is_some());
    assert!(package.part("word/media/another%20picture.png").is_some());
    assert!(package.part("word/stray.xml").is_none());
}

// --- A file another program wrote ---------------------------------------------

/// A package as another program might write it: the content types neither
/// first nor compressed and described after their data, the parts in an
/// order of its own — a folder entry among them — stored and compressed,
/// stamped nought or 2019, with extra fields, comments of their own and a
/// descriptor without its signature, and a comment on the whole.
fn written_elsewhere() -> Vec<u8> {
    let parts = foreign::document_parts("Written by hand");
    let [types, package_relationships, document, document_relationships, styles, core] =
        <[foreign::Entry; 6]>::try_from(parts).unwrap();
    foreign::archive(
        &[
            document.with_extra_fields().with_comment("the text"),
            styles.stored().at(0, 0),
            types.stored().described(true),
            foreign::Entry::new("docProps/", b"").stored(),
            core.described(false).with_extra_fields(),
            package_relationships,
            document_relationships.with_comment("its relationships"),
        ],
        b"written by another program",
    )
}

/// The same parts with the content types first, every one stored.
fn written_elsewhere_types_first() -> Vec<u8> {
    let parts: Vec<foreign::Entry> =
        foreign::document_parts("Stored").into_iter().map(foreign::Entry::stored).collect();
    foreign::archive(&parts, b"")
}

/// And every part in Zip64, half of them with descriptors.
fn written_elsewhere_in_zip64() -> Vec<u8> {
    let parts: Vec<foreign::Entry> = foreign::document_parts("Zip64")
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let entry = entry.zip64();
            if index % 2 == 0 {
                entry.described(true)
            } else {
                entry
            }
        })
        .collect();
    foreign::archive(&parts, b"zip64")
}

#[test]
fn a_package_nothing_changed_is_saved_as_the_file_it_was() {
    for (what, original) in [
        ("content types in the middle", written_elsewhere()),
        ("content types first", written_elsewhere_types_first()),
        ("Zip64", written_elsewhere_in_zip64()),
    ] {
        let package = Package::open(&original).unwrap();
        assert_eq!(package.main_document_part().unwrap(), "word/document.xml", "{what}");
        assert_eq!(package.save().unwrap(), original, "{what}: the file came back different");
    }
}

#[test]
fn a_part_written_back_as_it_was_leaves_the_file_as_it_was() {
    // Written is not changed: the same bytes put back are the file's bytes.
    let original = written_elsewhere();
    let mut package = Package::open(&original).unwrap();
    let document = package.part("word/document.xml").unwrap().to_vec();
    package.set_part("word/document.xml", document);
    let types = package.content_types().clone();
    package.set_content_types(types);
    assert_eq!(package.save().unwrap(), original);
}

/// Where each entry of an archive starts, taken out of its central
/// directory header, so that two headers can be held against each other.
fn without_offset(record: &[u8]) -> Vec<u8> {
    let mut record = record.to_vec();
    record[42..46].fill(0);
    record
}

#[test]
fn an_edited_package_copies_every_part_it_did_not_write() {
    let original = written_elsewhere();
    let mut package = Package::open(&original).unwrap();
    let edited = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Edited</w:t></w:r></w:p></w:body></w:document>"#;
    package.set_part("word/document.xml", edited.to_vec());
    // A part of a type the package already declares, so the declarations
    // are left as they were.
    package.set_part("customXml/item1.xml", b"<data/>".to_vec());
    let saved = package.save().unwrap();

    let before = ZipArchive::open(&original).unwrap();
    let after = ZipArchive::open(&saved).unwrap();
    let names = |archive: &ZipArchive| -> Vec<String> {
        archive.entries().iter().map(|entry| entry.name.clone()).collect()
    };
    let mut expected = names(&before);
    expected.push("customXml/item1.xml".to_owned());
    assert_eq!(names(&after), expected, "the file's order was not kept, the new part last");
    assert_eq!(after.comment(), b"written by another program");

    for entry in before.entries() {
        let name = &entry.name;
        let kept = after.entry(name).unwrap_or_else(|| panic!("{name} was lost"));
        if name == "word/document.xml" {
            assert_ne!(after.local_record(kept).unwrap(), before.local_record(entry).unwrap());
            assert_eq!(after.read(kept).unwrap(), edited);
            continue;
        }
        assert_eq!(
            after.local_record(kept).unwrap(),
            before.local_record(entry).unwrap(),
            "{name}: not copied as it was stored"
        );
        assert_eq!(
            without_offset(after.central_record(kept).unwrap()),
            without_offset(before.central_record(entry).unwrap()),
            "{name}: the central directory says something else of it"
        );
    }
    assert_eq!(after.read_by_name("customXml/item1.xml").unwrap().unwrap(), b"<data/>");
    let reopened = Package::open(&saved).unwrap();
    assert_eq!(reopened.part("word/document.xml"), Some(edited.as_slice()));
}

#[test]
fn a_part_taken_out_is_left_out_and_the_rest_are_copied() {
    // Taking a part out takes its declaration out too, so the content types
    // are written again; everything else is as the file had it.
    let original = written_elsewhere_in_zip64();
    let mut package = Package::open(&original).unwrap();
    package.remove_part("docProps/core.xml");
    let saved = package.save().unwrap();

    let before = ZipArchive::open(&original).unwrap();
    let after = ZipArchive::open(&saved).unwrap();
    assert!(after.entry("docProps/core.xml").is_none(), "the part taken out is in the file");
    assert_eq!(after.entries().len(), before.entries().len() - 1);
    for entry in before.entries().iter().filter(|entry| entry.name != "docProps/core.xml") {
        let name = &entry.name;
        let kept = after.entry(name).unwrap_or_else(|| panic!("{name} was lost"));
        let same = after.local_record(kept).unwrap() == before.local_record(entry).unwrap();
        assert_eq!(same, name != "[Content_Types].xml", "{name}");
        assert_eq!(after.read(kept).unwrap(), package.part(name).unwrap(), "{name}");
    }
}

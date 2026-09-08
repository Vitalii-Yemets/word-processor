//! Tests for the ZIP reader and writer.

use wp_zip::{Compression, DosDateTime, Error, NameEncoding, ZipArchive, ZipWriter};

/// Builds a small archive resembling the shape of a `.docx` package.
fn sample_package() -> Vec<u8> {
    let mut writer = ZipWriter::new();
    writer.add("[Content_Types].xml", CONTENT_TYPES.as_bytes()).unwrap();
    writer.add("_rels/.rels", RELATIONSHIPS.as_bytes()).unwrap();
    writer.add("word/document.xml", DOCUMENT.as_bytes()).unwrap();
    writer.add_stored("word/media/image1.bin", &[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();
    writer.finish().unwrap()
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#;

const RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;

const DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document><w:body><w:p><w:r><w:t>Hello</w:t></w:r></w:p></w:body></w:document>"#;

#[test]
fn written_archive_reads_back_identically() {
    let bytes = sample_package();
    let archive = ZipArchive::open(&bytes).unwrap();

    let names: Vec<&str> = archive.entries().iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(
        names,
        ["[Content_Types].xml", "_rels/.rels", "word/document.xml", "word/media/image1.bin"]
    );

    assert_eq!(archive.read_by_name("word/document.xml").unwrap().unwrap(), DOCUMENT.as_bytes());
    assert_eq!(
        archive.read_by_name("word/media/image1.bin").unwrap().unwrap(),
        [0xDE, 0xAD, 0xBE, 0xEF]
    );
}

#[test]
fn compression_method_is_preserved() {
    let bytes = sample_package();
    let archive = ZipArchive::open(&bytes).unwrap();

    assert_eq!(archive.entry("word/document.xml").unwrap().compression, Compression::Deflate);
    assert_eq!(archive.entry("word/media/image1.bin").unwrap().compression, Compression::Stored);
}

#[test]
fn writing_is_deterministic() {
    // Saving the same document twice must produce identical bytes; without this
    // no round-trip test could prove that a save changed nothing.
    assert_eq!(sample_package(), sample_package());
}

#[test]
fn empty_archive_is_valid() {
    let bytes = ZipWriter::new().finish().unwrap();
    let archive = ZipArchive::open(&bytes).unwrap();
    assert!(archive.entries().is_empty());
}

#[test]
fn empty_entries_roundtrip() {
    let mut writer = ZipWriter::new();
    writer.add("empty.xml", b"").unwrap();
    writer.add_stored("empty-stored.bin", b"").unwrap();
    let bytes = writer.finish().unwrap();

    let archive = ZipArchive::open(&bytes).unwrap();
    assert_eq!(archive.read_by_name("empty.xml").unwrap().unwrap(), b"");
    assert_eq!(archive.read_by_name("empty-stored.bin").unwrap().unwrap(), b"");
}

#[test]
fn non_ascii_names_roundtrip() {
    // Part names in a package can carry any Unicode; the UTF-8 flag has to be
    // set for them and honoured on the way back.
    let names = ["word/документ.xml", "word/文書.xml", "word/مستند.xml", "word/média/imagen.png"];

    let mut writer = ZipWriter::new();
    for (index, name) in names.iter().enumerate() {
        writer.add(name, format!("content {index}").as_bytes()).unwrap();
    }
    let bytes = writer.finish().unwrap();

    let archive = ZipArchive::open(&bytes).unwrap();
    for (index, name) in names.iter().enumerate() {
        let entry = archive.entry(name).unwrap_or_else(|| panic!("missing entry {name:?}"));
        assert_eq!(
            entry.name_encoding,
            NameEncoding::Utf8Declared,
            "{name:?} should carry the UTF-8 flag"
        );
        assert_eq!(archive.read(entry).unwrap(), format!("content {index}").as_bytes());
    }
}

#[test]
fn ascii_names_are_written_without_the_utf8_flag() {
    // An ASCII name reads identically under either interpretation, so the flag
    // is left clear for the benefit of older tools.
    let bytes = sample_package();
    let archive = ZipArchive::open(&bytes).unwrap();
    for entry in archive.entries() {
        assert_eq!(
            entry.name_encoding,
            NameEncoding::Utf8Detected,
            "{:?} should not need the UTF-8 flag",
            entry.name
        );
    }
}

#[test]
fn timestamps_roundtrip() {
    let stamp = DosDateTime::new(2026, 9, 7, 14, 35, 46);
    let mut writer = ZipWriter::new();
    writer.add_with("dated.xml", b"content", Compression::Deflate, stamp).unwrap();
    let bytes = writer.finish().unwrap();

    let archive = ZipArchive::open(&bytes).unwrap();
    let entry = archive.entry("dated.xml").unwrap();
    assert_eq!(entry.last_modified, stamp);
    assert_eq!(entry.last_modified.year(), 2026);
    assert_eq!(entry.last_modified.second(), 46);
}

#[test]
fn duplicate_names_are_refused_when_writing() {
    let mut writer = ZipWriter::new();
    writer.add("word/document.xml", b"first").unwrap();
    assert_eq!(
        writer.add("word/document.xml", b"second"),
        Err(Error::DuplicateName("word/document.xml".to_owned()))
    );
}

#[test]
fn unsafe_names_are_refused_when_writing() {
    let mut writer = ZipWriter::new();
    for name in ["../escape.xml", "/absolute.xml", "word\\document.xml"] {
        assert!(writer.add(name, b"content").is_err(), "should have been refused: {name:?}");
    }
}

#[test]
fn a_non_archive_is_rejected() {
    assert_eq!(ZipArchive::open(b"not a zip file at all").unwrap_err(), Error::NotAnArchive);
    assert_eq!(ZipArchive::open(b"").unwrap_err(), Error::NotAnArchive);
    // A PDF, offered by mistake: right kind of file, wrong format.
    assert_eq!(
        ZipArchive::open(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n").unwrap_err(),
        Error::NotAnArchive
    );
}

#[test]
fn corrupted_content_fails_the_checksum() {
    let mut bytes = sample_package();
    let archive = ZipArchive::open(&bytes).unwrap();
    let entry = archive.entry("word/media/image1.bin").unwrap().clone();

    // The stored entry's bytes appear verbatim, so they can be found and damaged.
    let position = bytes
        .windows(4)
        .position(|window| window == [0xDE, 0xAD, 0xBE, 0xEF])
        .expect("stored payload should be present verbatim");
    bytes[position] ^= 0xFF;

    let archive = ZipArchive::open(&bytes).unwrap();
    assert_eq!(
        archive.read(&entry),
        Err(Error::ChecksumMismatch { entry: "word/media/image1.bin".to_owned() })
    );
}

#[test]
fn truncated_archives_are_rejected_without_panicking() {
    let bytes = sample_package();
    for cut in 0..bytes.len() {
        // Some prefixes may still parse; only a panic is unacceptable.
        if let Ok(archive) = ZipArchive::open(&bytes[..cut]) {
            for entry in archive.entries() {
                let _ = archive.read(entry);
            }
        }
    }
}

#[test]
fn corrupted_archives_are_rejected_without_panicking() {
    let bytes = sample_package();
    for index in 0..bytes.len() {
        for bit in [0u8, 3, 7] {
            let mut damaged = bytes.clone();
            damaged[index] ^= 1 << bit;
            if let Ok(archive) = ZipArchive::open(&damaged) {
                for entry in archive.entries() {
                    let _ = archive.read(entry);
                }
            }
        }
    }
}

#[test]
fn entry_size_limit_stops_a_bomb() {
    let mut writer = ZipWriter::new();
    writer.add("bomb.bin", &vec![0u8; 4 << 20]).unwrap();
    let bytes = writer.finish().unwrap();

    let archive = ZipArchive::open(&bytes).unwrap().with_max_entry_size(1024);
    let entry = archive.entry("bomb.bin").unwrap();
    assert!(matches!(archive.read(entry), Err(Error::TooLarge(_))));

    // The same archive reads fine when the limit allows it.
    let archive = ZipArchive::open(&bytes).unwrap();
    assert_eq!(archive.read(archive.entry("bomb.bin").unwrap()).unwrap().len(), 4 << 20);
}

#[test]
fn many_entries_roundtrip() {
    // Comfortably more than a real document has, and enough to catch an offset
    // arithmetic mistake in the central directory.
    let mut writer = ZipWriter::new();
    for index in 0..2_000 {
        writer.add(&format!("word/part{index}.xml"), format!("<p>{index}</p>").as_bytes()).unwrap();
    }
    let bytes = writer.finish().unwrap();

    let archive = ZipArchive::open(&bytes).unwrap();
    assert_eq!(archive.entries().len(), 2_000);
    for index in [0, 1, 999, 1_999] {
        let name = format!("word/part{index}.xml");
        assert_eq!(
            archive.read_by_name(&name).unwrap().unwrap(),
            format!("<p>{index}</p>").as_bytes()
        );
    }
}

#[test]
fn large_entry_crosses_the_zip64_threshold_for_counts() {
    // More than 65535 entries forces Zip64 end records, because the classic
    // end-of-central-directory has only a 16-bit entry count.
    let mut writer = ZipWriter::new();
    for index in 0..70_000u32 {
        writer.add_stored(&format!("p{index}"), b"x").unwrap();
    }
    let bytes = writer.finish().unwrap();

    let archive = ZipArchive::open(&bytes).unwrap();
    assert_eq!(archive.entries().len(), 70_000);
    assert_eq!(archive.read_by_name("p69999").unwrap().unwrap(), b"x");
}

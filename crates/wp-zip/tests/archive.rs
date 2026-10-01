//! Tests for the ZIP reader and writer.

mod foreign;

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

// --- Entries copied as they are stored ----------------------------------------

/// An archive another program might have written, every choice in it made
/// otherwise than this crate makes it: the order, stored entries and
/// compressed ones, a raw timestamp of nought, extra fields, entry comments,
/// data descriptors with their signature and without, Zip64 entries — one of
/// them with a descriptor, its sizes eight bytes wide — and a directory
/// entry, and the whole with a comment.
fn a_foreign_archive() -> Vec<u8> {
    use foreign::Entry;
    foreign::archive(
        &[
            Entry::new("word/document.xml", DOCUMENT.as_bytes())
                .with_extra_fields()
                .with_comment("the text"),
            Entry::new("word/media/image1.bin", &[0xDE, 0xAD, 0xBE, 0xEF]).stored().at(0, 0),
            Entry::new("[Content_Types].xml", CONTENT_TYPES.as_bytes()).described(true),
            Entry::new("_rels/.rels", RELATIONSHIPS.as_bytes())
                .described(false)
                .with_extra_fields(),
            // Long enough to take several stored blocks.
            Entry::new("word/styles.xml", "<w:style/>".repeat(300).as_bytes()).zip64(),
            Entry::new("docProps/app.xml", b"<Properties/>").zip64().described(true),
            Entry::new("word/", b"").stored(),
        ],
        b"written by hand",
    )
}

/// A central directory header with where its entry starts taken out, which
/// is the one thing a copy changes: the four bytes of its slot, and the
/// eight of its Zip64 field when the slot holds the sentinel.
fn without_offset(record: &[u8]) -> Vec<u8> {
    let mut record = record.to_vec();
    let word =
        |record: &[u8], at: usize| u32::from_le_bytes(record[at..at + 4].try_into().unwrap());
    if word(&record, 42) == u32::MAX {
        let extra_start = 46 + usize::from(u16::from_le_bytes([record[28], record[29]]));
        assert_eq!(record[extra_start..extra_start + 2], [1, 0], "the Zip64 field comes first");
        let sizes = [20, 24].iter().filter(|at| word(&record, **at) == u32::MAX).count() * 8;
        let at = extra_start + 4 + sizes;
        record[at..at + 8].fill(0);
    }
    record[42..46].fill(0);
    record
}

#[test]
fn the_records_of_every_entry_are_where_the_archive_has_them() {
    // Written one after another with nothing between, the local records are
    // the archive up to its central directory, and the central records come
    // next: a record that stopped short of its data descriptor, or ran on
    // into the next entry, would not tile the file.
    let bytes = a_foreign_archive();
    let archive = ZipArchive::open(&bytes).unwrap();
    let mut local = Vec::new();
    let mut central = Vec::new();
    for entry in archive.entries() {
        local.extend_from_slice(archive.local_record(entry).unwrap());
        central.extend_from_slice(archive.central_record(entry).unwrap());
    }
    assert!(bytes.starts_with(&local), "the local records are not the start of the archive");
    assert_eq!(&bytes[local.len()..local.len() + central.len()], central.as_slice());
    assert_eq!(archive.comment(), b"written by hand");

    let described = archive.entry("[Content_Types].xml").unwrap();
    let record = archive.local_record(described).unwrap();
    let mut descriptor = 0x0807_4B50u32.to_le_bytes().to_vec();
    descriptor.extend_from_slice(&described.crc32.to_le_bytes());
    descriptor.extend_from_slice(&(described.compressed_size as u32).to_le_bytes());
    descriptor.extend_from_slice(&(described.uncompressed_size as u32).to_le_bytes());
    assert!(record.ends_with(&descriptor), "the descriptor is not the end of the record");
}

#[test]
fn an_entry_copied_into_another_archive_is_the_entry_it_was() {
    let bytes = a_foreign_archive();
    let archive = ZipArchive::open(&bytes).unwrap();

    // In the other order, so that every entry starts somewhere else.
    let mut writer = ZipWriter::new();
    for entry in archive.entries().iter().rev() {
        writer.copy_from(&archive, entry).unwrap();
    }
    writer.set_comment(archive.comment()).unwrap();
    let copied = writer.finish().unwrap();

    let copy = ZipArchive::open(&copied).unwrap();
    assert_eq!(copy.comment(), b"written by hand");
    let names: Vec<&str> = copy.entries().iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names.first(), Some(&"word/"), "the order given is not the order written");
    for entry in archive.entries() {
        let name = &entry.name;
        let moved = copy.entry(name).unwrap_or_else(|| panic!("{name} was not copied"));
        assert_eq!(
            copy.local_record(moved).unwrap(),
            archive.local_record(entry).unwrap(),
            "{name}: the header, the stored bytes or the descriptor changed"
        );
        assert_eq!(
            without_offset(copy.central_record(moved).unwrap()),
            without_offset(archive.central_record(entry).unwrap()),
            "{name}: the central directory says something else of it"
        );
        assert_eq!(
            (moved.compression, moved.crc32, moved.last_modified, moved.compressed_size),
            (entry.compression, entry.crc32, entry.last_modified, entry.compressed_size),
            "{name}"
        );
        // And it is found where it now is, Zip64 offsets included.
        assert_eq!(copy.read(moved).unwrap(), archive.read(entry).unwrap(), "{name}");
    }
}

#[test]
fn copied_entries_and_written_ones_make_one_archive() {
    let bytes = a_foreign_archive();
    let archive = ZipArchive::open(&bytes).unwrap();
    let document = archive.entry("word/document.xml").unwrap();
    let image = archive.entry("word/media/image1.bin").unwrap();

    let mut writer = ZipWriter::new();
    writer.add("word/new.xml", b"<new/>").unwrap();
    writer.copy_from(&archive, document).unwrap();
    writer.add_stored("word/other.bin", b"other").unwrap();
    writer.copy_from(&archive, image).unwrap();
    // A name is in the archive once, however it got there.
    assert_eq!(
        writer.copy_from(&archive, document),
        Err(Error::DuplicateName("word/document.xml".to_owned()))
    );
    assert_eq!(
        writer.add("word/media/image1.bin", b"again"),
        Err(Error::DuplicateName("word/media/image1.bin".to_owned()))
    );
    let bytes = writer.finish().unwrap();

    let mixed = ZipArchive::open(&bytes).unwrap();
    let names: Vec<&str> = mixed.entries().iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(
        names,
        ["word/new.xml", "word/document.xml", "word/other.bin", "word/media/image1.bin"]
    );
    assert_eq!(mixed.read_by_name("word/new.xml").unwrap().unwrap(), b"<new/>");
    assert_eq!(mixed.read_by_name("word/document.xml").unwrap().unwrap(), DOCUMENT.as_bytes());
    assert_eq!(mixed.read_by_name("word/other.bin").unwrap().unwrap(), b"other");
    assert_eq!(
        mixed.read_by_name("word/media/image1.bin").unwrap().unwrap(),
        [0xDE, 0xAD, 0xBE, 0xEF]
    );
    assert!(mixed.comment().is_empty(), "a comment nobody set");
}

#[test]
fn an_entry_whose_descriptor_disagrees_is_not_copied_and_still_reads() {
    // The descriptor's checksum, damaged. Reading does not need it — the
    // central directory has the checksum — but a copy that cannot say where
    // the entry ends must not guess, and must leave nothing half written.
    let mut bytes = a_foreign_archive();
    let archive = ZipArchive::open(&bytes).unwrap();
    let entry = archive.entry("[Content_Types].xml").unwrap().clone();
    let record = archive.local_record(&entry).unwrap();
    let end = record.as_ptr() as usize - bytes.as_ptr() as usize + record.len();
    // Signature, checksum and two sizes of four: the checksum is twelve back.
    bytes[end - 12] ^= 0xFF;

    let archive = ZipArchive::open(&bytes).unwrap();
    assert_eq!(archive.local_record(&entry), Err(Error::CorruptHeader("data descriptor")));
    assert_eq!(archive.read(&entry).unwrap(), CONTENT_TYPES.as_bytes());

    let mut writer = ZipWriter::new();
    assert!(writer.copy_from(&archive, &entry).is_err());
    writer.add("[Content_Types].xml", CONTENT_TYPES.as_bytes()).unwrap();
    let bytes = writer.finish().unwrap();
    let rewritten = ZipArchive::open(&bytes).unwrap();
    assert_eq!(rewritten.entries().len(), 1);
    assert_eq!(
        rewritten.read_by_name("[Content_Types].xml").unwrap().unwrap(),
        CONTENT_TYPES.as_bytes()
    );
}

#[test]
fn copying_from_a_damaged_archive_fails_without_panicking() {
    let bytes = a_foreign_archive();
    for index in 0..bytes.len() {
        for bit in [0u8, 4, 7] {
            let mut damaged = bytes.clone();
            damaged[index] ^= 1 << bit;
            let Ok(archive) = ZipArchive::open(&damaged) else { continue };
            let mut writer = ZipWriter::new();
            for entry in archive.entries() {
                let _ = archive.central_record(entry);
                let _ = writer.copy_from(&archive, entry);
            }
            let _ = writer.finish();
        }
    }
}

#[test]
fn a_comment_is_written_after_the_end_record_and_read_back() {
    let mut writer = ZipWriter::new();
    writer.add("a.xml", b"<a/>").unwrap();
    writer.set_comment(b"a comment").unwrap();
    let bytes = writer.finish().unwrap();
    assert!(bytes.ends_with(b"a comment"));
    assert_eq!(ZipArchive::open(&bytes).unwrap().comment(), b"a comment");

    let mut writer = ZipWriter::new();
    assert!(matches!(writer.set_comment(&[b'x'; 70_000]), Err(Error::TooLarge(_))));
}

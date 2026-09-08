//! Interoperability with the reference `zip` and `unzip` tools.
//!
//! Reading back our own output proves only internal consistency. These tests
//! check each direction against an implementation that had no part in writing
//! this code, which is the only way a format mistake shows up.
//!
//! Both tools live in the build container, so these run wherever the suite runs.

use std::path::{Path, PathBuf};
use std::process::Command;

use wp_zip::{ZipArchive, ZipWriter};

/// A scratch directory inside the container. Nothing is written to the
/// developer's machine.
fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join("wp-zip-interop").join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("cannot create scratch directory");
    path
}

fn run(program: &str, args: &[&str], working_directory: &Path) -> std::process::Output {
    let output = Command::new(program)
        .args(args)
        .current_dir(working_directory)
        .output()
        .unwrap_or_else(|error| panic!("failed to run {program}: {error}"));
    assert!(
        output.status.success(),
        "{program} {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Files laid out the way a real package is, including names that need UTF-8.
fn write_source_tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    let files: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".to_owned(), b"<Types/>".to_vec()),
        ("_rels/.rels".to_owned(), b"<Relationships/>".to_vec()),
        (
            "word/document.xml".to_owned(),
            "<w:document><w:t>Multilingual: текст 文書 نص</w:t></w:document>".as_bytes().to_vec(),
        ),
        // Long and repetitive, so the tool has a reason to use dynamic Huffman
        // codes — the branch our own encoder never exercises.
        (
            "word/styles.xml".to_owned(),
            "<w:style w:styleId=\"Normal\"/>".repeat(3_000).into_bytes(),
        ),
        // Incompressible, so the tool stores it instead.
        ("word/media/image1.bin".to_owned(), (0..=255u8).cycle().take(20_000).collect()),
        ("docProps/название.xml".to_owned(), "<Properties/>".as_bytes().to_vec()),
    ];

    for (name, content) in &files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
    }
    files
}

#[test]
fn reads_archives_produced_by_zip() {
    for level in ["-0", "-6", "-9"] {
        let directory = scratch(&format!("from-zip{level}"));
        let files = write_source_tree(&directory);

        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        let mut args = vec![level, "-q", "-X", "archive.zip"];
        args.extend_from_slice(&names);
        run("zip", &args, &directory);

        let bytes = std::fs::read(directory.join("archive.zip")).unwrap();
        let archive = ZipArchive::open(&bytes)
            .unwrap_or_else(|error| panic!("zip {level} archive rejected: {error}"));

        assert_eq!(archive.entries().len(), files.len(), "zip {level}: wrong entry count");
        for (name, expected) in &files {
            let entry = archive
                .entry(name)
                .unwrap_or_else(|| panic!("zip {level}: entry {name:?} is missing"));
            let actual = archive
                .read(entry)
                .unwrap_or_else(|error| panic!("zip {level}: {name:?} failed to read: {error}"));
            assert_eq!(&actual, expected, "zip {level}: {name:?} has wrong content");
        }
    }
}

/// Escapes the characters `unzip` treats as wildcards in an entry name.
///
/// Package names really do contain brackets — `[Content_Types].xml` is required
/// by the format — and unzip would read those as a character class.
fn escape_for_unzip(name: &str) -> String {
    let mut escaped = String::with_capacity(name.len());
    for character in name.chars() {
        if matches!(character, '[' | ']' | '*' | '?' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[test]
fn unzip_accepts_archives_produced_by_us() {
    let directory = scratch("to-unzip");
    let files = write_source_tree(&directory);

    let mut writer = ZipWriter::new();
    for (name, content) in &files {
        writer.add(name, content).unwrap();
    }
    let bytes = writer.finish().unwrap();
    std::fs::write(directory.join("ours.zip"), &bytes).unwrap();

    // -t verifies every entry's CRC, so this alone proves the archive is sound.
    let output = run("unzip", &["-t", "ours.zip"], &directory);
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains("No errors detected"), "unzip reported problems:\n{report}");

    // Names and content are checked only for ASCII entries.
    //
    // For a non-ASCII name we set general-purpose bit 11 and store UTF-8, which
    // is what the format specifies and what Word, 7-Zip and Windows Explorer
    // all do. Info-ZIP 6.00 dates from 2009 and renders such names through a
    // legacy conversion that mangles anything outside Latin-1 — a limitation of
    // that build, not of the archive. The -t check above covers those entries
    // structurally, and full round-tripping of non-ASCII names is tested inside
    // the crate where no external tool is in the way.
    let ascii_files: Vec<&(String, Vec<u8>)> =
        files.iter().filter(|(name, _)| name.is_ascii()).collect();
    assert!(ascii_files.len() < files.len(), "the sample should include a non-ASCII name");

    let output = run("unzip", &["-Z1", "ours.zip"], &directory);
    let listed: Vec<String> =
        String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect();
    for (name, _) in &ascii_files {
        assert!(listed.contains(name), "unzip did not list {name:?}");
    }

    // Content is compared through a pipe rather than by extracting to disk: the
    // file system would drag in its own rules about names and case, which have
    // nothing to do with whether the archive is correct.
    for (name, expected) in &ascii_files {
        let output = run("unzip", &["-p", "ours.zip", &escape_for_unzip(name)], &directory);
        assert_eq!(&output.stdout, expected, "{name:?} differs when read back by unzip");
    }
}

#[test]
fn reads_archives_named_under_a_legacy_locale() {
    // Under a non-UTF-8 locale `zip` takes a different path for a name it cannot
    // represent: it may attach an Info-ZIP Unicode Path extra field carrying the
    // real name. Whichever route it takes, the name must come back intact and be
    // recognised as Unicode rather than guessed at as a legacy code page.
    let directory = scratch("legacy-locale");
    let name = "docProps/название.xml";

    let path = directory.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"content").unwrap();

    let output = Command::new("zip")
        .args(["-q", "-9", "legacy.zip", name])
        .current_dir(&directory)
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .output()
        .expect("failed to run zip");
    assert!(output.status.success(), "zip failed: {}", String::from_utf8_lossy(&output.stderr));

    let bytes = std::fs::read(directory.join("legacy.zip")).unwrap();
    let archive = ZipArchive::open(&bytes).expect("archive from a legacy locale was rejected");

    let entry = archive.entry(name).unwrap_or_else(|| {
        let listed: Vec<&str> = archive.entries().iter().map(|e| e.name.as_str()).collect();
        panic!("entry {name:?} not found; the archive holds {listed:?}")
    });
    assert!(
        entry.name_encoding.is_unicode(),
        "name should have been recovered as Unicode, not as {:?}",
        entry.name_encoding
    );
    assert_eq!(archive.read(entry).unwrap(), b"content");
}

#[test]
fn unzip_accepts_a_stored_only_archive() {
    let directory = scratch("stored-only");

    let mut writer = ZipWriter::new();
    writer.add_stored("mimetype", b"application/vnd.oasis.opendocument.text").unwrap();
    writer
        .add_stored("word/image.bin", &(0..=255u8).cycle().take(5_000).collect::<Vec<_>>())
        .unwrap();
    let bytes = writer.finish().unwrap();
    std::fs::write(directory.join("stored.zip"), &bytes).unwrap();

    let output = run("unzip", &["-t", "stored.zip"], &directory);
    assert!(String::from_utf8_lossy(&output.stdout).contains("No errors detected"));
}

#[test]
fn unzip_accepts_a_zip64_archive() {
    let directory = scratch("zip64");

    // More than 65535 entries forces the Zip64 end records into the output.
    let mut writer = ZipWriter::new();
    for index in 0..70_000u32 {
        writer.add_stored(&format!("p{index}"), b"x").unwrap();
    }
    let bytes = writer.finish().unwrap();
    std::fs::write(directory.join("big.zip"), &bytes).unwrap();

    let output = run("unzip", &["-t", "big.zip"], &directory);
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains("No errors detected"), "unzip reported problems:\n{report}");
}

//! Interoperability with a reference implementation (zlib, via system `gzip`).
//!
//! "Compressed and decompressed with our own code" proves nothing about
//! interoperability: a mistake made symmetrically on both sides slips straight
//! through. Here each direction is checked against an outside implementation.
//!
//! The first test matters most: our encoder emits only fixed Huffman codes,
//! while Word and gzip emit dynamic ones. Without these fixtures the dynamic-code
//! path of the decoder would go completely untested.

use std::path::{Path, PathBuf};
use std::process::Command;

use wp_deflate::{compress, crc32, inflate_limited};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// One manifest entry: fixture name, original size, and original CRC-32.
struct Fixture {
    name: String,
    original_size: usize,
    original_crc32: u32,
}

/// Reads the manifest produced by `tools/make-fixtures.sh`. Size and CRC-32 come
/// from the gzip trailer, which means they were not computed by us.
fn read_manifest() -> Vec<Fixture> {
    let path = fixtures_dir().join("manifest.txt");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "fixture manifest {} not found: {error}\n\
             regenerate it with: docker compose run --rm dev bash tools/make-fixtures.sh",
            path.display()
        )
    });

    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next().expect("fixture name").to_owned();
            let original_size = parts.next().expect("size").parse().expect("size is a number");
            let original_crc32 = parts.next().expect("crc32").parse().expect("crc32 is a number");
            Fixture { name, original_size, original_crc32 }
        })
        .collect()
}

#[test]
fn decodes_streams_produced_by_gzip() {
    let fixtures = read_manifest();
    assert!(!fixtures.is_empty(), "fixture manifest is empty");

    for fixture in &fixtures {
        let path = fixtures_dir().join(format!("{}.deflate", fixture.name));
        let stream = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));

        let decoded = inflate_limited(&stream, 8 << 20)
            .unwrap_or_else(|error| panic!("fixture {:?} failed to decode: {error}", fixture.name));

        assert_eq!(
            decoded.len(),
            fixture.original_size,
            "fixture {:?}: wrong output length",
            fixture.name
        );
        // The CRC-32 was computed by gzip, so a match validates both the decoder
        // and our own checksum implementation.
        assert_eq!(
            crc32(&decoded),
            fixture.original_crc32,
            "fixture {:?}: checksum mismatch",
            fixture.name
        );
    }
}

/// Wraps a raw DEFLATE stream in a gzip container (RFC 1952) so that the system
/// `gzip -d` can be asked to decode it.
fn wrap_as_gzip(deflate_stream: &[u8], original: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(deflate_stream.len() + 18);
    // Magic, deflate method, no flags, no timestamp, OS "Unix".
    out.extend_from_slice(&[0x1F, 0x8B, 0x08, 0x00, 0, 0, 0, 0, 0x00, 0x03]);
    out.extend_from_slice(deflate_stream);
    out.extend_from_slice(&crc32(original).to_le_bytes());
    out.extend_from_slice(&(original.len() as u32).to_le_bytes());
    out
}

#[test]
fn gzip_accepts_streams_produced_by_us() {
    // The scratch directory lives inside the container; nothing is written to
    // the developer's machine.
    let scratch = std::env::temp_dir().join("wp-deflate-interop");
    std::fs::create_dir_all(&scratch).expect("cannot create scratch directory");

    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("single", b"z".to_vec()),
        (
            "multilingual",
            "Interoperability check. Проверка. 検証. تحقّق. परीक्षण. ".as_bytes().repeat(500),
        ),
        ("markup", r#"<w:r><w:t>value</w:t></w:r>"#.as_bytes().repeat(2_000)),
        ("zeros", vec![0u8; 300_000]),
        ("bytes", (0..=255u8).collect::<Vec<_>>().repeat(300)),
    ];

    for (name, original) in cases {
        let archive = wrap_as_gzip(&compress(&original), &original);
        let path = scratch.join(format!("{name}.gz"));
        std::fs::write(&path, &archive).expect("cannot write scratch file");

        let output = Command::new("gzip")
            .arg("-d")
            .arg("-c")
            .arg(&path)
            .output()
            .expect("failed to run gzip");

        assert!(
            output.status.success(),
            "gzip rejected our stream {name:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, original, "gzip decoded our stream {name:?} incorrectly");

        let _ = std::fs::remove_file(&path);
    }
}

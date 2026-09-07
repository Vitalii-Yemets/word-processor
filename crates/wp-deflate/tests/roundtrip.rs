//! Tests for the DEFLATE encoder and decoder.
//!
//! Robustness gets particular attention: the decoder will be handed files from
//! the internet and from email, so on malformed input it must return an error
//! rather than panic.

use wp_deflate::{compress, compress_stored, compress_zlib, inflate, inflate_limited, inflate_zlib};

/// Deterministic pseudo-random byte source. Tests must be reproducible, so no
/// system entropy is used.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_byte(&mut self) -> u8 {
        // Constants from Numerical Recipes.
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u8
    }

    fn bytes(&mut self, count: usize) -> Vec<u8> {
        (0..count).map(|_| self.next_byte()).collect()
    }
}

/// Text covering the writing systems the editor has to handle, so the byte
/// patterns under test are the ones real documents actually contain: Latin,
/// Cyrillic, Greek, Arabic and Hebrew (right-to-left), Devanagari and Thai
/// (complex shaping), CJK, Korean, and characters outside the Basic
/// Multilingual Plane.
const MULTILINGUAL_SAMPLE: &str = concat!(
    "The quick brown fox jumps over the lazy dog. ",
    "Съешь же ещё этих мягких французских булок. ",
    "Ταχίστη αλώπηξ βαφής ψημένη γη. ",
    "نص حكيم له سر قاطع وذو شأن عظيم مكتوب على ثوب أخضر. ",
    "דג סקרן שט בים מאוכזב ולפתע מצא חברה. ",
    "वह क्षमा और साहस का प्रतीक है। ",
    "เป็นมนุษย์สุดประเสริฐเลิศคุณค่า ",
    "永和九年，歲在癸丑，暮春之初，會于會稽山陰之蘭亭。 ",
    "다람쥐 헌 쳇바퀴에 타고파. ",
    "𝕬𝖓𝖉 𝖆𝖘𝖙𝖗𝖆𝖑 𝖕𝖑𝖆𝖓𝖊 𝖙𝖊𝖝𝖙 🌍🖋️📄",
);

/// Inputs that exercise meaningfully different paths through the encoder.
fn samples() -> Vec<(&'static str, Vec<u8>)> {
    let mut random = Lcg::new(0x5EED);

    vec![
        ("empty", Vec::new()),
        ("single byte", b"x".to_vec()),
        ("short text", b"Hello, world!".to_vec()),
        // A repeated single byte exercises distance-1 back-references, i.e.
        // overlapping copies — the most common source of decoder bugs.
        ("one thousand zeros", vec![0u8; 1000]),
        ("long repeat", b"abcabcabc".repeat(500)),
        // Close to what a real document.xml looks like.
        (
            "markup",
            r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p>"#
                .as_bytes()
                .repeat(200),
        ),
        // Multi-byte UTF-8 across many scripts: byte statistics differ sharply
        // from ASCII and stress different parts of the code alphabet.
        ("multilingual text", MULTILINGUAL_SAMPLE.as_bytes().repeat(60)),
        ("every byte value", (0..=255u8).collect::<Vec<_>>().repeat(40)),
        // Incompressible data: the encoder must fall back to stored blocks.
        ("random data", random.bytes(50_000)),
        // More than one stored block (LEN is a 16-bit field).
        ("over 64 KiB", random.bytes(200_000)),
        ("300 KiB of text", b"The quick brown fox jumps over the lazy dog. ".repeat(7_000)),
    ]
}

#[test]
fn compress_then_inflate_returns_the_original() {
    for (name, data) in samples() {
        let packed = compress(&data);
        let unpacked = inflate(&packed)
            .unwrap_or_else(|error| panic!("sample {name:?} failed to decompress: {error}"));
        assert_eq!(unpacked, data, "sample {name:?} decompressed incorrectly");
    }
}

#[test]
fn stored_blocks_roundtrip() {
    for (name, data) in samples() {
        let packed = compress_stored(&data);
        let unpacked = inflate(&packed)
            .unwrap_or_else(|error| panic!("stored blocks, sample {name:?}: {error}"));
        assert_eq!(unpacked, data, "sample {name:?}");
    }
}

#[test]
fn zlib_wrapper_roundtrips_and_checks_adler32() {
    for (name, data) in samples() {
        let packed = compress_zlib(&data);
        let unpacked = inflate_zlib(&packed, usize::MAX)
            .unwrap_or_else(|error| panic!("zlib, sample {name:?}: {error}"));
        assert_eq!(unpacked, data, "sample {name:?}");
    }
}

#[test]
fn zlib_rejects_a_broken_checksum() {
    let packed = compress_zlib(b"proof that the checksum is actually verified");
    let mut damaged = packed.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 0xFF;

    assert_eq!(inflate_zlib(&damaged, usize::MAX), Err(wp_deflate::Error::ChecksumMismatch));
}

#[test]
fn compression_actually_shrinks_repetitive_data() {
    let data = b"The quick brown fox jumps over the lazy dog. ".repeat(1_000);
    let packed = compress(&data);
    assert!(
        packed.len() * 10 < data.len(),
        "expected better than 10:1 compression, got {} from {}",
        packed.len(),
        data.len()
    );
}

#[test]
fn output_never_meaningfully_exceeds_the_input() {
    // Fixed codes inflate incompressible data, so the encoder is required to
    // switch to stored blocks.
    let data = Lcg::new(1).bytes(100_000);
    let packed = compress(&data);
    let overhead = packed.len() - data.len();
    assert!(overhead <= 16, "overhead of {overhead} bytes is too large");
}

#[test]
fn output_limit_stops_a_decompression_bomb() {
    // A megabyte of zeros compresses to a few kilobytes; this is exactly how
    // "zip bombs" work. The limit has to trigger before memory runs out.
    let data = vec![0u8; 1 << 20];
    let packed = compress(&data);
    // Fixed codes spend 13 bits per 258-byte match, i.e. roughly 6.6 KB per
    // megabyte of zeros — about 160:1. Dynamic codes will do an order of
    // magnitude better, and this bound can drop once they land.
    assert!(packed.len() < 8_192, "bomb sample came out too large: {} bytes", packed.len());

    assert_eq!(inflate_limited(&packed, 4_096), Err(wp_deflate::Error::OutputLimitExceeded));
    // With a sufficient limit, the very same data decompresses fine.
    assert_eq!(inflate_limited(&packed, 1 << 20).unwrap().len(), data.len());
}

#[test]
fn truncated_streams_are_rejected_without_panicking() {
    let data = MULTILINGUAL_SAMPLE.as_bytes().repeat(20);
    let packed = compress(&data);

    for cut in 0..packed.len() {
        // The outcome may be an error, or — for very short prefixes — a
        // correctly decoded partial result. Only a panic is unacceptable.
        let _ = inflate_limited(&packed[..cut], 1 << 20);
    }
}

#[test]
fn corrupted_streams_are_rejected_without_panicking() {
    let data = "<w:document><w:body><w:p><w:r><w:t>text</w:t></w:r></w:p></w:body></w:document>"
        .as_bytes()
        .repeat(20);
    let packed = compress(&data);

    for index in 0..packed.len() {
        for bit in 0..8 {
            let mut damaged = packed.clone();
            damaged[index] ^= 1 << bit;
            let _ = inflate_limited(&damaged, 1 << 20);
        }
    }
}

#[test]
fn arbitrary_bytes_are_rejected_without_panicking() {
    let mut random = Lcg::new(0xC0FFEE);
    for _ in 0..2_000 {
        let length = (usize::from(random.next_byte()) % 64) + 1;
        let noise = random.bytes(length);
        let _ = inflate_limited(&noise, 1 << 16);
    }
}

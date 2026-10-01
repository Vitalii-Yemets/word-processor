//! Zip archives put together by hand, the way another program writes them,
//! for the tests that need a file this program did not make.
//!
//! A file this program wrote proves nothing about keeping a file's bytes: the
//! same writer twice gives the same bytes whether anything was kept or not.
//! So nothing here calls the code under test. The checksum is worked out
//! here; a compressed entry is a DEFLATE stream of stored blocks, which every
//! reader inflates and no compressor that compresses ever writes, so an entry
//! that comes back with the same bytes was copied and not compressed again;
//! and every choice a producer makes — the order, the method, the timestamp,
//! extra fields, a data descriptor, Zip64, comments, attributes — is made
//! otherwise than this program makes it.
//!
//! One file, included by path from the tests of every crate that asks the
//! question, each of which uses its own part of it.

#![allow(dead_code)]

const SIGNATURE_LOCAL_HEADER: u32 = 0x0403_4B50;
const SIGNATURE_CENTRAL_HEADER: u32 = 0x0201_4B50;
const SIGNATURE_DATA_DESCRIPTOR: u32 = 0x0807_4B50;
const SIGNATURE_END: u32 = 0x0605_4B50;
const SIGNATURE_ZIP64_END: u32 = 0x0606_4B50;
const SIGNATURE_ZIP64_LOCATOR: u32 = 0x0706_4B50;

/// One entry, and the choices its producer made in writing it.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
    /// Method 8, as stored blocks of at most a kilobyte, rather than method 0.
    pub deflated: bool,
    /// The checksum and the sizes after the data rather than in the local
    /// header, with the descriptor's optional signature or without it.
    pub descriptor: Option<bool>,
    /// The sizes as sentinels, the real ones in a Zip64 field in both
    /// headers, and the offset in the central one's field too.
    pub zip64: bool,
    /// The MS-DOS date and time, as raw as the producer wrote them.
    pub date: u16,
    pub time: u16,
    /// Extra fields of the local header, and the central one's, which a
    /// producer is free to make different.
    pub local_extra: Vec<u8>,
    pub central_extra: Vec<u8>,
    /// The entry's comment in the central directory.
    pub comment: Vec<u8>,
}

impl Entry {
    /// An entry deflated, stamped 17 May 2019 at 13:37:42, and nothing else.
    pub fn new(name: &str, data: &[u8]) -> Self {
        Self {
            name: name.to_owned(),
            data: data.to_vec(),
            deflated: true,
            descriptor: None,
            zip64: false,
            date: ((2019 - 1980) << 9) | (5 << 5) | 17,
            time: (13 << 11) | (37 << 5) | 21,
            local_extra: Vec::new(),
            central_extra: Vec::new(),
            comment: Vec::new(),
        }
    }

    /// Stored, method 0.
    #[must_use]
    pub fn stored(mut self) -> Self {
        self.deflated = false;
        self
    }

    /// With a data descriptor, signed or not.
    #[must_use]
    pub fn described(mut self, signed: bool) -> Self {
        self.descriptor = Some(signed);
        self
    }

    /// With every size and the offset in Zip64 fields.
    #[must_use]
    pub fn zip64(mut self) -> Self {
        self.zip64 = true;
        self
    }

    /// Stamped with a raw date and time — nought for both is what some
    /// producers write, and no calendar has it.
    #[must_use]
    pub fn at(mut self, date: u16, time: u16) -> Self {
        self.date = date;
        self.time = time;
        self
    }

    /// With the extended timestamp Info-ZIP writes, the local one carrying a
    /// field of a producer's own besides, which the central one does not.
    #[must_use]
    pub fn with_extra_fields(mut self) -> Self {
        let timestamp = |extra: &mut Vec<u8>| {
            extra.extend_from_slice(&0x5455u16.to_le_bytes());
            extra.extend_from_slice(&5u16.to_le_bytes());
            extra.push(1);
            extra.extend_from_slice(&1_558_100_262u32.to_le_bytes());
        };
        timestamp(&mut self.local_extra);
        self.local_extra.extend_from_slice(&0xCAFEu16.to_le_bytes());
        self.local_extra.extend_from_slice(&3u16.to_le_bytes());
        self.local_extra.extend_from_slice(b"pad");
        timestamp(&mut self.central_extra);
        self
    }

    /// With a comment in the central directory.
    #[must_use]
    pub fn with_comment(mut self, comment: &str) -> Self {
        self.comment = comment.as_bytes().to_vec();
        self
    }

    /// The bytes the entry's data is stored as.
    pub fn payload(&self) -> Vec<u8> {
        if self.deflated {
            stored_blocks(&self.data)
        } else {
            self.data.clone()
        }
    }
}

/// CRC-32 as ZIP has it, worked out bit by bit.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 { crc >> 1 } else { (crc >> 1) ^ 0xEDB8_8320 };
        }
    }
    !crc
}

/// DEFLATE as stored blocks of at most a kilobyte: valid, and nothing a
/// compressor that compresses would write.
fn stored_blocks(data: &[u8]) -> Vec<u8> {
    let chunks: Vec<&[u8]> = if data.is_empty() { vec![&[]] } else { data.chunks(1024).collect() };
    let mut out = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        // BFINAL on the last, BTYPE 00, and the rest of the byte skipped.
        out.push(u8::from(index + 1 == chunks.len()));
        let length = chunk.len() as u16;
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// The archive: the entries in the order given, made by "UNIX" in the
/// central directory, with the Zip64 end records when any entry is Zip64,
/// and the comment after the end record.
pub fn archive(entries: &[Entry], comment: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = out.len() as u64;
        let payload = entry.payload();
        let crc = crc32(&entry.data);
        let (compressed, uncompressed) = (payload.len() as u64, entry.data.len() as u64);
        let method = if entry.deflated { 8 } else { 0 };
        let version = if entry.zip64 { 45 } else { 20 };
        let flags = if entry.descriptor.is_some() { 1 << 3 } else { 0 };

        let mut local_extra = Vec::new();
        if entry.zip64 {
            // A producer writing to a stream does not know the sizes yet,
            // and says nought until the descriptor.
            let known = entry.descriptor.is_none();
            put16(&mut local_extra, 1);
            put16(&mut local_extra, 16);
            put64(&mut local_extra, if known { uncompressed } else { 0 });
            put64(&mut local_extra, if known { compressed } else { 0 });
        }
        local_extra.extend_from_slice(&entry.local_extra);

        put32(&mut out, SIGNATURE_LOCAL_HEADER);
        put16(&mut out, version);
        put16(&mut out, flags);
        put16(&mut out, method);
        put16(&mut out, entry.time);
        put16(&mut out, entry.date);
        if entry.descriptor.is_some() {
            put32(&mut out, 0);
            put32(&mut out, 0);
            put32(&mut out, 0);
        } else if entry.zip64 {
            put32(&mut out, crc);
            put32(&mut out, u32::MAX);
            put32(&mut out, u32::MAX);
        } else {
            put32(&mut out, crc);
            put32(&mut out, compressed as u32);
            put32(&mut out, uncompressed as u32);
        }
        put16(&mut out, entry.name.len() as u16);
        put16(&mut out, local_extra.len() as u16);
        out.extend_from_slice(entry.name.as_bytes());
        out.extend_from_slice(&local_extra);
        out.extend_from_slice(&payload);
        if let Some(signed) = entry.descriptor {
            if signed {
                put32(&mut out, SIGNATURE_DATA_DESCRIPTOR);
            }
            put32(&mut out, crc);
            if entry.zip64 {
                put64(&mut out, compressed);
                put64(&mut out, uncompressed);
            } else {
                put32(&mut out, compressed as u32);
                put32(&mut out, uncompressed as u32);
            }
        }

        let mut central_extra = Vec::new();
        if entry.zip64 {
            put16(&mut central_extra, 1);
            put16(&mut central_extra, 24);
            put64(&mut central_extra, uncompressed);
            put64(&mut central_extra, compressed);
            put64(&mut central_extra, offset);
        }
        central_extra.extend_from_slice(&entry.central_extra);

        put32(&mut central, SIGNATURE_CENTRAL_HEADER);
        put16(&mut central, (3 << 8) | 30);
        put16(&mut central, version);
        put16(&mut central, flags);
        put16(&mut central, method);
        put16(&mut central, entry.time);
        put16(&mut central, entry.date);
        put32(&mut central, crc);
        put32(&mut central, if entry.zip64 { u32::MAX } else { compressed as u32 });
        put32(&mut central, if entry.zip64 { u32::MAX } else { uncompressed as u32 });
        put16(&mut central, entry.name.len() as u16);
        put16(&mut central, central_extra.len() as u16);
        put16(&mut central, entry.comment.len() as u16);
        put16(&mut central, 0); // the disk it starts on
        put16(&mut central, 1); // internal attributes: text
        put32(&mut central, 0o100_644 << 16); // external: a file, rw-r--r--
        put32(&mut central, if entry.zip64 { u32::MAX } else { offset as u32 });
        central.extend_from_slice(entry.name.as_bytes());
        central.extend_from_slice(&central_extra);
        central.extend_from_slice(&entry.comment);
    }

    let directory_offset = out.len() as u64;
    let directory_size = central.len() as u64;
    out.extend_from_slice(&central);
    let count = entries.len() as u64;
    let zip64 = entries.iter().any(|entry| entry.zip64);
    if zip64 {
        let record = out.len() as u64;
        put32(&mut out, SIGNATURE_ZIP64_END);
        put64(&mut out, 44);
        put16(&mut out, 45);
        put16(&mut out, 45);
        put32(&mut out, 0);
        put32(&mut out, 0);
        put64(&mut out, count);
        put64(&mut out, count);
        put64(&mut out, directory_size);
        put64(&mut out, directory_offset);
        put32(&mut out, SIGNATURE_ZIP64_LOCATOR);
        put32(&mut out, 0);
        put64(&mut out, record);
        put32(&mut out, 1);
    }
    put32(&mut out, SIGNATURE_END);
    put16(&mut out, 0);
    put16(&mut out, 0);
    put16(&mut out, if zip64 { u16::MAX } else { count as u16 });
    put16(&mut out, if zip64 { u16::MAX } else { count as u16 });
    put32(&mut out, directory_size as u32);
    put32(&mut out, if zip64 { u32::MAX } else { directory_offset as u32 });
    put16(&mut out, comment.len() as u16);
    out.extend_from_slice(comment);
    out
}

/// The parts of a small Word document, as entries: the content types, the
/// package's relationships, the document with one paragraph saying what is
/// given, its relationships and styles, and the core properties.
///
/// Every part is a part Word reads; what the tests vary is how the archive
/// around them is written.
pub fn document_parts(text: &str) -> Vec<Entry> {
    let content_types = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        "\n",
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
        r#"<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#,
        r#"<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>"#,
        r#"</Types>"#,
    );
    let package_relationships = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        "\n",
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>"#,
        r#"</Relationships>"#,
    );
    let document = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            "\n",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
            r#"<w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p>"#,
            r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
            r#"<w:pgMar w:top="1134" w:right="1134" w:bottom="1134" w:left="1134" w:header="709" w:footer="709" w:gutter="0"/>"#,
            r#"</w:sectPr></w:body></w:document>"#,
        ),
        text
    );
    let document_relationships = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        "\n",
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
        r#"</Relationships>"#,
    );
    let styles = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        "\n",
        r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>"#,
        r#"</w:styles>"#,
    );
    let core = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        "\n",
        r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
        r#"<dc:title>Written by hand</dc:title></cp:coreProperties>"#,
    );
    vec![
        Entry::new("[Content_Types].xml", content_types.as_bytes()),
        Entry::new("_rels/.rels", package_relationships.as_bytes()),
        Entry::new("word/document.xml", document.as_bytes()),
        Entry::new("word/_rels/document.xml.rels", document_relationships.as_bytes()),
        Entry::new("word/styles.xml", styles.as_bytes()),
        Entry::new("docProps/core.xml", core.as_bytes()),
    ]
}

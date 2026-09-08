//! Writing ZIP archives.
//!
//! Output is deterministic: the same parts written in the same order produce
//! byte-identical archives. Timestamps therefore default to a fixed value rather
//! than the current clock. For a document editor this matters — it makes saved
//! files comparable, which is what lets a round-trip test prove that opening and
//! saving a document changed nothing.

use crate::{check_name, Compression, DosDateTime, Error};

const SIGNATURE_LOCAL_HEADER: u32 = 0x0403_4B50;
const SIGNATURE_CENTRAL_HEADER: u32 = 0x0201_4B50;
const SIGNATURE_END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4B50;
const SIGNATURE_ZIP64_END_OF_CENTRAL_DIRECTORY: u32 = 0x0606_4B50;
const SIGNATURE_ZIP64_LOCATOR: u32 = 0x0706_4B50;

const EXTRA_FIELD_ZIP64: u16 = 0x0001;

/// Minimum version needed to extract: 2.0, the version that introduced deflate.
const VERSION_DEFLATE: u16 = 20;
/// Minimum version needed to extract an entry using Zip64: 4.5.
const VERSION_ZIP64: u16 = 45;

const FLAG_UTF8_NAME: u16 = 1 << 11;

/// Above this a 32-bit size or offset field cannot hold the value.
const MAX_32_BIT: u64 = 0xFFFF_FFFF;
/// Above this the 16-bit entry count in the end record cannot hold the value.
const MAX_16_BIT_COUNT: usize = 0xFFFF;

/// What the central directory needs to remember about an entry already written.
#[derive(Debug)]
struct PendingEntry {
    name: String,
    compression: Compression,
    modified: DosDateTime,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    name_is_utf8: bool,
}

/// Builds a ZIP archive in memory.
#[derive(Debug, Default)]
pub struct ZipWriter {
    out: Vec<u8>,
    entries: Vec<PendingEntry>,
}

impl ZipWriter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry, compressing it with DEFLATE.
    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<(), Error> {
        self.add_with(name, data, Compression::Deflate, DosDateTime::EPOCH)
    }

    /// Adds an entry stored without compression. Appropriate for content that is
    /// already compressed, such as embedded PNG or JPEG images.
    pub fn add_stored(&mut self, name: &str, data: &[u8]) -> Result<(), Error> {
        self.add_with(name, data, Compression::Stored, DosDateTime::EPOCH)
    }

    /// Adds an entry, choosing compression and timestamp explicitly.
    pub fn add_with(
        &mut self,
        name: &str,
        data: &[u8],
        compression: Compression,
        modified: DosDateTime,
    ) -> Result<(), Error> {
        check_name(name)?;
        if self.entries.iter().any(|entry| entry.name == name) {
            return Err(Error::DuplicateName(name.to_owned()));
        }

        let payload = match compression {
            Compression::Stored => data.to_vec(),
            Compression::Deflate => wp_deflate::compress(data),
        };

        let uncompressed_size = data.len() as u64;
        let compressed_size = payload.len() as u64;
        let local_header_offset = self.out.len() as u64;

        // Bit 11 says the name is UTF-8. It is set only when the name actually
        // needs it: an ASCII name is identical under either interpretation, and
        // leaving the flag clear keeps the archive readable by older tools.
        let name_is_utf8 = !name.is_ascii();
        let flags = if name_is_utf8 { FLAG_UTF8_NAME } else { 0 };

        let needs_zip64 = uncompressed_size > MAX_32_BIT || compressed_size > MAX_32_BIT;
        let version = if needs_zip64 { VERSION_ZIP64 } else { VERSION_DEFLATE };

        let crc32 = wp_deflate::crc32(data);
        let name_bytes = name.as_bytes();
        if name_bytes.len() > usize::from(u16::MAX) {
            return Err(Error::TooLarge("entry name longer than 65535 bytes"));
        }

        self.write_u32(SIGNATURE_LOCAL_HEADER);
        self.write_u16(version);
        self.write_u16(flags);
        self.write_u16(compression.to_code());
        self.write_u16(modified.time);
        self.write_u16(modified.date);
        self.write_u32(crc32);
        if needs_zip64 {
            // Sentinels here; the true sizes go in the extra field below.
            self.write_u32(u32::MAX);
            self.write_u32(u32::MAX);
        } else {
            self.write_u32(compressed_size as u32);
            self.write_u32(uncompressed_size as u32);
        }
        self.write_u16(name_bytes.len() as u16);
        // A local header's Zip64 field always carries both sizes, in this order.
        self.write_u16(if needs_zip64 { 20 } else { 0 });
        self.out.extend_from_slice(name_bytes);
        if needs_zip64 {
            self.write_u16(EXTRA_FIELD_ZIP64);
            self.write_u16(16);
            self.write_u64(uncompressed_size);
            self.write_u64(compressed_size);
        }

        self.out.extend_from_slice(&payload);

        self.entries.push(PendingEntry {
            name: name.to_owned(),
            compression,
            modified,
            crc32,
            compressed_size,
            uncompressed_size,
            local_header_offset,
            name_is_utf8,
        });

        Ok(())
    }

    /// Writes the central directory and returns the finished archive.
    pub fn finish(mut self) -> Result<Vec<u8>, Error> {
        let directory_offset = self.out.len() as u64;

        let entries = std::mem::take(&mut self.entries);
        for entry in &entries {
            self.write_central_header(entry)?;
        }

        let directory_size = self.out.len() as u64 - directory_offset;
        let entry_count = entries.len();

        // Zip64 end records are written only when a value genuinely does not fit
        // the classic structure. Emitting them unconditionally would make every
        // small document unreadable to older tools for no benefit.
        let needs_zip64 = entry_count > MAX_16_BIT_COUNT
            || directory_offset > MAX_32_BIT
            || directory_size > MAX_32_BIT;

        if needs_zip64 {
            let zip64_end_offset = self.out.len() as u64;

            self.write_u32(SIGNATURE_ZIP64_END_OF_CENTRAL_DIRECTORY);
            self.write_u64(44); // size of this record, excluding the first 12 bytes
            self.write_u16(VERSION_ZIP64); // version made by
            self.write_u16(VERSION_ZIP64); // version needed
            self.write_u32(0); // this disk
            self.write_u32(0); // disk holding the central directory
            self.write_u64(entry_count as u64);
            self.write_u64(entry_count as u64);
            self.write_u64(directory_size);
            self.write_u64(directory_offset);

            self.write_u32(SIGNATURE_ZIP64_LOCATOR);
            self.write_u32(0); // disk holding the Zip64 end record
            self.write_u64(zip64_end_offset);
            self.write_u32(1); // total number of disks
        }

        self.write_u32(SIGNATURE_END_OF_CENTRAL_DIRECTORY);
        self.write_u16(0); // this disk
        self.write_u16(0); // disk holding the central directory
        self.write_u16(entry_count.min(MAX_16_BIT_COUNT) as u16);
        self.write_u16(entry_count.min(MAX_16_BIT_COUNT) as u16);
        self.write_u32(directory_size.min(MAX_32_BIT) as u32);
        self.write_u32(directory_offset.min(MAX_32_BIT) as u32);
        self.write_u16(0); // archive comment length

        Ok(self.out)
    }

    fn write_central_header(&mut self, entry: &PendingEntry) -> Result<(), Error> {
        // Each oversized value is replaced by a sentinel and moved into the
        // Zip64 extra field, in the order the format prescribes.
        let uncompressed_overflows = entry.uncompressed_size > MAX_32_BIT;
        let compressed_overflows = entry.compressed_size > MAX_32_BIT;
        let offset_overflows = entry.local_header_offset > MAX_32_BIT;
        let needs_zip64 = uncompressed_overflows || compressed_overflows || offset_overflows;

        let mut extra_size = 0usize;
        if uncompressed_overflows {
            extra_size += 8;
        }
        if compressed_overflows {
            extra_size += 8;
        }
        if offset_overflows {
            extra_size += 8;
        }

        let version = if needs_zip64 { VERSION_ZIP64 } else { VERSION_DEFLATE };
        let flags = if entry.name_is_utf8 { FLAG_UTF8_NAME } else { 0 };
        let name_bytes = entry.name.as_bytes();

        self.write_u32(SIGNATURE_CENTRAL_HEADER);
        self.write_u16(version); // version made by
        self.write_u16(version); // version needed to extract
        self.write_u16(flags);
        self.write_u16(entry.compression.to_code());
        self.write_u16(entry.modified.time);
        self.write_u16(entry.modified.date);
        self.write_u32(entry.crc32);
        self.write_u32(if compressed_overflows { u32::MAX } else { entry.compressed_size as u32 });
        self.write_u32(if uncompressed_overflows {
            u32::MAX
        } else {
            entry.uncompressed_size as u32
        });
        self.write_u16(name_bytes.len() as u16);
        self.write_u16(if needs_zip64 { (extra_size + 4) as u16 } else { 0 });
        self.write_u16(0); // comment length
        self.write_u16(0); // disk on which the entry starts
        self.write_u16(0); // internal attributes
        self.write_u32(0); // external attributes
        self.write_u32(if offset_overflows { u32::MAX } else { entry.local_header_offset as u32 });
        self.out.extend_from_slice(name_bytes);

        if needs_zip64 {
            self.write_u16(EXTRA_FIELD_ZIP64);
            self.write_u16(extra_size as u16);
            if uncompressed_overflows {
                self.write_u64(entry.uncompressed_size);
            }
            if compressed_overflows {
                self.write_u64(entry.compressed_size);
            }
            if offset_overflows {
                self.write_u64(entry.local_header_offset);
            }
        }

        Ok(())
    }

    fn write_u16(&mut self, value: u16) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u64(&mut self, value: u64) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }
}

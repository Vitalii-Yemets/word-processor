//! Writing ZIP archives.
//!
//! Output is deterministic: the same parts written in the same order produce
//! byte-identical archives. Timestamps therefore default to a fixed value rather
//! than the current clock. For a document editor this matters — it makes saved
//! files comparable, which is what lets a round-trip test prove that opening and
//! saving a document changed nothing.

use crate::{
    check_name, read_u16, read_u32, zip64_field, Compression, DosDateTime, Error, ZipArchive,
    ZipEntry,
};

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

/// Fixed part of a central directory header, before the variable-length fields.
const CENTRAL_HEADER_SIZE: usize = 46;
/// Where a central directory header keeps the offset of its local header.
const CENTRAL_OFFSET_FIELD: usize = 42;

/// An entry already in the archive, as the central directory will list it.
#[derive(Debug)]
enum Pending {
    /// Written here, so its header is made from what is known of it.
    Written(PendingEntry),
    /// Copied from another archive, so its header is the one that archive
    /// had, pointed at where the entry starts in this one.
    Copied { name: String, record: Vec<u8>, local_header_offset: u64 },
}

impl Pending {
    fn name(&self) -> &str {
        match self {
            Self::Written(entry) => &entry.name,
            Self::Copied { name, .. } => name,
        }
    }
}

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
    entries: Vec<Pending>,
    comment: Vec<u8>,
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
        self.check_unique(name)?;

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

        self.entries.push(Pending::Written(PendingEntry {
            name: name.to_owned(),
            compression,
            modified,
            crc32,
            compressed_size,
            uncompressed_size,
            local_header_offset,
            name_is_utf8,
        }));

        Ok(())
    }

    /// Adds an entry of another archive as that archive stores it.
    ///
    /// Nothing is decompressed or compressed again: the local header, the
    /// compressed bytes and any data descriptor after them go in as they
    /// were, and so does the entry's header in the central directory — its
    /// attributes, its extra fields, its comment — with only where the entry
    /// starts changed. An entry nobody changed is then the entry its
    /// producer wrote, and not this crate's compression of the same content,
    /// which is a different stream of bytes.
    ///
    /// Nothing is written when the entry's records cannot be found whole, so
    /// a caller can fall back to [`Self::add_with`] on an error.
    pub fn copy_from(&mut self, archive: &ZipArchive<'_>, entry: &ZipEntry) -> Result<(), Error> {
        self.check_unique(&entry.name)?;
        let local = archive.local_record(entry)?;
        let record = archive.central_record(entry)?.to_vec();
        // Read before anything is written, so that a header that cannot be
        // pointed elsewhere fails here and not when the archive is finished.
        pointed_at(&record, 0)?;

        let local_header_offset = self.out.len() as u64;
        self.out.extend_from_slice(local);
        self.entries.push(Pending::Copied {
            name: entry.name.clone(),
            record,
            local_header_offset,
        });
        Ok(())
    }

    /// Sets the archive's comment, written after its end record.
    pub fn set_comment(&mut self, comment: &[u8]) -> Result<(), Error> {
        if comment.len() > usize::from(u16::MAX) {
            return Err(Error::TooLarge("archive comment longer than 65535 bytes"));
        }
        self.comment = comment.to_vec();
        Ok(())
    }

    /// Refuses a name already in the archive.
    fn check_unique(&self, name: &str) -> Result<(), Error> {
        if self.entries.iter().any(|entry| entry.name() == name) {
            return Err(Error::DuplicateName(name.to_owned()));
        }
        Ok(())
    }

    /// Writes the central directory and returns the finished archive.
    pub fn finish(mut self) -> Result<Vec<u8>, Error> {
        let directory_offset = self.out.len() as u64;

        let entries = std::mem::take(&mut self.entries);
        for entry in &entries {
            match entry {
                Pending::Written(entry) => self.write_central_header(entry)?,
                Pending::Copied { record, local_header_offset, .. } => {
                    let record = pointed_at(record, *local_header_offset)?;
                    self.out.extend_from_slice(&record);
                }
            }
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
        // No longer than a 16-bit length can say: see [`Self::set_comment`].
        let comment = std::mem::take(&mut self.comment);
        self.write_u16(comment.len() as u16);
        self.out.extend_from_slice(&comment);

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

/// A central directory header copied from another archive, pointed at where
/// its entry starts in this one.
///
/// Only the offset changes, and where it is kept is the header's own
/// choice: in the Zip64 extra field when the header put the sentinel in the
/// offset's slot, in the slot otherwise. A slot too small for where the
/// entry now is gives way to the Zip64 field as the format says — the
/// sentinel in the slot and the offset in the field, after whichever sizes
/// the field already carries — so that a copied entry may land past 4 GiB
/// as a written one may.
fn pointed_at(record: &[u8], offset: u64) -> Result<Vec<u8>, Error> {
    let mut record = record.to_vec();
    let name_length = usize::from(read_u16(&record, 28)?);
    let extra_length = usize::from(read_u16(&record, 30)?);
    let extra_start = CENTRAL_HEADER_SIZE + name_length;
    let extra = record.get(extra_start..extra_start + extra_length).ok_or(Error::Truncated)?;
    let field = zip64_field(extra).map(|(at, size)| (extra_start + at, size));
    // The Zip64 field holds only the values whose slots overflowed, in a
    // fixed order, and the offset comes after the two sizes.
    let mut before_offset = 0usize;
    for slot in [20, 24] {
        if read_u32(&record, slot)? == u32::MAX {
            before_offset += 8;
        }
    }

    if read_u32(&record, CENTRAL_OFFSET_FIELD)? == u32::MAX {
        let missing = Error::CorruptHeader("missing Zip64 extended information field");
        let (field, size) = field.ok_or(missing)?;
        if before_offset + 8 > size {
            return Err(Error::CorruptHeader("Zip64 extended information field too short"));
        }
        let at = field + 4 + before_offset;
        record[at..at + 8].copy_from_slice(&offset.to_le_bytes());
        return Ok(record);
    }
    if offset <= MAX_32_BIT {
        record[CENTRAL_OFFSET_FIELD..CENTRAL_OFFSET_FIELD + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
        return Ok(record);
    }

    record[CENTRAL_OFFSET_FIELD..CENTRAL_OFFSET_FIELD + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let (at, grown) = match field {
        Some((field, size)) => {
            let longer = u16::try_from(size + 8)
                .map_err(|_| Error::TooLarge("Zip64 extended information field"))?;
            record[field + 2..field + 4].copy_from_slice(&longer.to_le_bytes());
            (field + 4 + before_offset.min(size), 8)
        }
        None => {
            let at = extra_start + extra_length;
            let mut header = EXTRA_FIELD_ZIP64.to_le_bytes().to_vec();
            header.extend_from_slice(&8u16.to_le_bytes());
            record.splice(at..at, header);
            (at + 4, 12)
        }
    };
    record.splice(at..at, offset.to_le_bytes());
    let extra_length = u16::try_from(extra_length + grown)
        .map_err(|_| Error::TooLarge("extra fields longer than 65535 bytes"))?;
    record[30..32].copy_from_slice(&extra_length.to_le_bytes());
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The central directory header of a small entry, as this crate writes it.
    fn record() -> Vec<u8> {
        let mut writer = ZipWriter::new();
        writer.add("word/document.xml", b"<w:document/>").unwrap();
        let bytes = writer.finish().unwrap();
        let archive = ZipArchive::open(&bytes).unwrap();
        archive.central_record(&archive.entries()[0]).unwrap().to_vec()
    }

    fn u16_at(record: &[u8], at: usize) -> u16 {
        read_u16(record, at).unwrap()
    }

    fn u32_at(record: &[u8], at: usize) -> u32 {
        read_u32(record, at).unwrap()
    }

    fn u64_at(record: &[u8], at: usize) -> u64 {
        crate::read_u64(record, at).unwrap()
    }

    #[test]
    fn a_copied_header_says_where_its_entry_now_starts_and_nothing_else() {
        let before = record();
        let after = pointed_at(&before, 0x1234).unwrap();
        assert_eq!(u32_at(&after, CENTRAL_OFFSET_FIELD), 0x1234);
        assert_eq!(after[..CENTRAL_OFFSET_FIELD], before[..CENTRAL_OFFSET_FIELD]);
        assert_eq!(after[CENTRAL_OFFSET_FIELD + 4..], before[CENTRAL_OFFSET_FIELD + 4..]);
    }

    #[test]
    fn a_copied_header_past_four_gigabytes_takes_a_zip64_field() {
        let before = record();
        let far = 5 << 30;
        let after = pointed_at(&before, far).unwrap();

        assert_eq!(u32_at(&after, CENTRAL_OFFSET_FIELD), u32::MAX);
        let name_length = usize::from(u16_at(&after, 28));
        assert_eq!(u16_at(&after, 30), 12, "the field and its header");
        let field = CENTRAL_HEADER_SIZE + name_length;
        assert_eq!(u16_at(&after, field), EXTRA_FIELD_ZIP64);
        assert_eq!(u16_at(&after, field + 2), 8);
        assert_eq!(u64_at(&after, field + 4), far);
        assert_eq!(after[CENTRAL_HEADER_SIZE..field], before[CENTRAL_HEADER_SIZE..field]);
    }

    /// A header whose sizes overflowed, with them in its Zip64 field and,
    /// when asked, the offset too.
    fn zip64_record(with_offset: bool) -> Vec<u8> {
        let mut record = record();
        let name_length = usize::from(u16_at(&record, 28));
        record[20..28].copy_from_slice(&[0xFF; 8]);
        let mut field = EXTRA_FIELD_ZIP64.to_le_bytes().to_vec();
        field.extend_from_slice(&(if with_offset { 24u16 } else { 16 }).to_le_bytes());
        field.extend_from_slice(&13u64.to_le_bytes());
        field.extend_from_slice(&15u64.to_le_bytes());
        if with_offset {
            record[CENTRAL_OFFSET_FIELD..CENTRAL_OFFSET_FIELD + 4].copy_from_slice(&[0xFF; 4]);
            field.extend_from_slice(&7u64.to_le_bytes());
        }
        record[30..32].copy_from_slice(&(field.len() as u16).to_le_bytes());
        let at = CENTRAL_HEADER_SIZE + name_length;
        record.splice(at..at, field);
        record
    }

    #[test]
    fn an_offset_kept_in_the_zip64_field_is_changed_there() {
        let before = zip64_record(true);
        let after = pointed_at(&before, 0x99).unwrap();
        assert_eq!(after.len(), before.len());
        let field = CENTRAL_HEADER_SIZE + usize::from(u16_at(&after, 28));
        assert_eq!(u64_at(&after, field + 4), 13, "the uncompressed size moved");
        assert_eq!(u64_at(&after, field + 12), 15, "the compressed size moved");
        assert_eq!(u64_at(&after, field + 20), 0x99);
        assert_eq!(u32_at(&after, CENTRAL_OFFSET_FIELD), u32::MAX);
    }

    #[test]
    fn an_offset_that_overflows_goes_after_the_sizes_in_the_zip64_field() {
        let before = zip64_record(false);
        let far = 6 << 30;
        let after = pointed_at(&before, far).unwrap();
        let field = CENTRAL_HEADER_SIZE + usize::from(u16_at(&after, 28));
        assert_eq!(u16_at(&after, field + 2), 24);
        assert_eq!(u16_at(&after, 30), 28);
        assert_eq!(u64_at(&after, field + 4), 13);
        assert_eq!(u64_at(&after, field + 12), 15);
        assert_eq!(u64_at(&after, field + 20), far);
        assert_eq!(u32_at(&after, CENTRAL_OFFSET_FIELD), u32::MAX);
    }
}

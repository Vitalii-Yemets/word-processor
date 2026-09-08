//! Reading ZIP archives.
//!
//! The central directory at the end of the file is treated as authoritative,
//! which is what every ZIP implementation does and what the format intends. Data
//! is located through it, and the local header is consulted only for the two
//! fields that decide where an entry's bytes actually begin.

use crate::{
    check_name, cp437, read_u16, read_u32, read_u64, Compression, DosDateTime, Error, NameEncoding,
    ZipEntry,
};

const SIGNATURE_LOCAL_HEADER: u32 = 0x0403_4B50;
const SIGNATURE_CENTRAL_HEADER: u32 = 0x0201_4B50;
const SIGNATURE_END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4B50;
const SIGNATURE_ZIP64_END_OF_CENTRAL_DIRECTORY: u32 = 0x0606_4B50;
const SIGNATURE_ZIP64_LOCATOR: u32 = 0x0706_4B50;

/// Fixed part of an end-of-central-directory record, before the comment.
const END_OF_CENTRAL_DIRECTORY_SIZE: usize = 22;
/// Fixed part of a central directory header, before the variable-length fields.
const CENTRAL_HEADER_SIZE: usize = 46;
/// Fixed part of a local file header, before the variable-length fields.
const LOCAL_HEADER_SIZE: usize = 30;
/// Size of the Zip64 end-of-central-directory locator.
const ZIP64_LOCATOR_SIZE: usize = 20;

/// The archive comment is a 16-bit length, so the record starts at most this far
/// from the end of the file.
const MAX_COMMENT_SIZE: usize = 0xFFFF;

/// Value that marks a 32-bit field as "see the Zip64 extra field".
const ZIP64_SENTINEL_32: u32 = 0xFFFF_FFFF;
/// The 16-bit equivalent, used for disk numbers.
const ZIP64_SENTINEL_16: u16 = 0xFFFF;

/// Header id of the Zip64 extended information extra field.
const EXTRA_FIELD_ZIP64: u16 = 0x0001;

/// Header id of the Info-ZIP Unicode Path extra field, which carries a UTF-8
/// name alongside a legacy one.
const EXTRA_FIELD_UNICODE_PATH: u16 = 0x7075;

/// General-purpose bit flags that matter here.
const FLAG_ENCRYPTED: u16 = 1 << 0;
const FLAG_DATA_DESCRIPTOR: u16 = 1 << 3;
const FLAG_UTF8_NAME: u16 = 1 << 11;

/// Ceiling on the decompressed size of a single entry, in bytes.
///
/// Without a ceiling, an archive could declare an enormous uncompressed size and
/// have us allocate it. Half a gigabyte is far beyond any legitimate part of a
/// document while still leaving obvious attacks no room.
pub const DEFAULT_MAX_ENTRY_SIZE: u64 = 512 * 1024 * 1024;

/// A ZIP archive held entirely in memory.
///
/// Documents are read whole rather than streamed: parts reference each other in
/// both directions, so random access is needed anyway, and an office document
/// that does not fit in memory is not a document anyone is editing.
#[derive(Clone, Debug)]
pub struct ZipArchive<'a> {
    data: &'a [u8],
    entries: Vec<ZipEntry>,
    max_entry_size: u64,
}

impl<'a> ZipArchive<'a> {
    /// Parses the archive's directory. Entry data is decompressed only when
    /// [`Self::read`] asks for it.
    pub fn open(data: &'a [u8]) -> Result<Self, Error> {
        let directory = Directory::locate(data)?;
        let entries = parse_central_directory(data, &directory)?;

        // Duplicate names are a known attack: different tools resolve them
        // differently, so one program acts on a part another never sees. A
        // package cannot legitimately contain two parts with the same name.
        let mut sorted: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        sorted.sort_unstable();
        if let Some(pair) = sorted.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(Error::DuplicateName(pair[0].to_owned()));
        }

        Ok(Self { data, entries, max_entry_size: DEFAULT_MAX_ENTRY_SIZE })
    }

    /// Overrides the per-entry decompressed size ceiling.
    #[must_use]
    pub fn with_max_entry_size(mut self, limit: u64) -> Self {
        self.max_entry_size = limit;
        self
    }

    /// Every entry, in the order the central directory lists them.
    #[must_use]
    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    /// Finds an entry by its exact name.
    #[must_use]
    pub fn entry(&self, name: &str) -> Option<&ZipEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    /// Decompresses an entry and verifies its length and CRC-32.
    ///
    /// The checksum is checked every time rather than trusted, because a
    /// mismatch means the file is damaged and the caller must not act on the
    /// contents.
    pub fn read(&self, entry: &ZipEntry) -> Result<Vec<u8>, Error> {
        if entry.uncompressed_size > self.max_entry_size {
            return Err(Error::TooLarge("entry exceeds the decompressed size limit"));
        }

        let start = self.data_offset(entry)?;
        let end = start
            .checked_add(usize::try_from(entry.compressed_size).map_err(|_| Error::Truncated)?)
            .ok_or(Error::Truncated)?;
        let raw = self.data.get(start..end).ok_or(Error::Truncated)?;

        let limit = usize::try_from(entry.uncompressed_size).map_err(|_| Error::Truncated)?;
        let content = match entry.compression {
            Compression::Stored => raw.to_vec(),
            Compression::Deflate => wp_deflate::inflate_limited(raw, limit)?,
        };

        if content.len() as u64 != entry.uncompressed_size {
            return Err(Error::SizeMismatch {
                entry: entry.name.clone(),
                expected: entry.uncompressed_size,
                actual: content.len() as u64,
            });
        }
        if wp_deflate::crc32(&content) != entry.crc32 {
            return Err(Error::ChecksumMismatch { entry: entry.name.clone() });
        }

        Ok(content)
    }

    /// Convenience: find an entry by name and read it.
    pub fn read_by_name(&self, name: &str) -> Option<Result<Vec<u8>, Error>> {
        self.entry(name).map(|entry| self.read(entry))
    }

    /// Where an entry's data begins.
    ///
    /// The central directory records the offset of the local header, not of the
    /// data. Only the local header knows how long its own name and extra fields
    /// are, and those lengths may differ from the central directory's — so it has
    /// to be read.
    fn data_offset(&self, entry: &ZipEntry) -> Result<usize, Error> {
        let header = usize::try_from(entry.local_header_offset).map_err(|_| Error::Truncated)?;

        if read_u32(self.data, header)? != SIGNATURE_LOCAL_HEADER {
            return Err(Error::CorruptHeader("local file header signature"));
        }
        let name_length = usize::from(read_u16(self.data, header + 26)?);
        let extra_length = usize::from(read_u16(self.data, header + 28)?);

        header.checked_add(LOCAL_HEADER_SIZE + name_length + extra_length).ok_or(Error::Truncated)
    }
}

/// Where the central directory is and how many entries it holds.
struct Directory {
    offset: u64,
    entry_count: u64,
}

impl Directory {
    /// Finds the central directory, following the Zip64 records when present.
    fn locate(data: &[u8]) -> Result<Self, Error> {
        let end_offset = find_end_of_central_directory(data)?;

        if read_u16(data, end_offset + 4)? != 0 || read_u16(data, end_offset + 6)? != 0 {
            return Err(Error::MultiVolume);
        }

        let entry_count = u64::from(read_u16(data, end_offset + 10)?);
        let offset = u64::from(read_u32(data, end_offset + 16)?);

        // A sentinel in either field means the real values live in the Zip64
        // record, which sits immediately before the locator.
        let needs_zip64 =
            entry_count == u64::from(ZIP64_SENTINEL_16) || offset == u64::from(ZIP64_SENTINEL_32);
        if !needs_zip64 {
            return Ok(Self { offset, entry_count });
        }

        let locator_offset = end_offset.checked_sub(ZIP64_LOCATOR_SIZE).ok_or(Error::Truncated)?;
        if read_u32(data, locator_offset)? != SIGNATURE_ZIP64_LOCATOR {
            return Err(Error::CorruptHeader("Zip64 end-of-central-directory locator"));
        }

        let record_offset =
            usize::try_from(read_u64(data, locator_offset + 8)?).map_err(|_| Error::Truncated)?;
        if read_u32(data, record_offset)? != SIGNATURE_ZIP64_END_OF_CENTRAL_DIRECTORY {
            return Err(Error::CorruptHeader("Zip64 end-of-central-directory record"));
        }

        Ok(Self {
            entry_count: read_u64(data, record_offset + 32)?,
            offset: read_u64(data, record_offset + 48)?,
        })
    }
}

/// Scans backwards for the end-of-central-directory signature.
///
/// The record is last in the file but is followed by a comment of up to 64 KiB,
/// so its position is not fixed. Scanning from the end finds the last valid
/// record, which is the one that describes the archive.
fn find_end_of_central_directory(data: &[u8]) -> Result<usize, Error> {
    if data.len() < END_OF_CENTRAL_DIRECTORY_SIZE {
        return Err(Error::NotAnArchive);
    }

    let furthest_start = data.len() - END_OF_CENTRAL_DIRECTORY_SIZE;
    let search_limit = furthest_start.saturating_sub(MAX_COMMENT_SIZE);

    for offset in (search_limit..=furthest_start).rev() {
        if read_u32(data, offset)? != SIGNATURE_END_OF_CENTRAL_DIRECTORY {
            continue;
        }
        // The comment length must account for exactly the rest of the file;
        // otherwise this is a coincidental byte sequence inside entry data.
        let comment_length = usize::from(read_u16(data, offset + 20)?);
        if offset + END_OF_CENTRAL_DIRECTORY_SIZE + comment_length == data.len() {
            return Ok(offset);
        }
    }

    Err(Error::NotAnArchive)
}

/// Walks the central directory and builds the entry list.
fn parse_central_directory(data: &[u8], directory: &Directory) -> Result<Vec<ZipEntry>, Error> {
    let mut offset = usize::try_from(directory.offset).map_err(|_| Error::Truncated)?;

    // Cap the allocation by what the file could physically contain, so a bogus
    // entry count cannot make us reserve gigabytes up front.
    let plausible_maximum = data.len() / CENTRAL_HEADER_SIZE + 1;
    let capacity =
        usize::try_from(directory.entry_count).unwrap_or(plausible_maximum).min(plausible_maximum);
    let mut entries = Vec::with_capacity(capacity);

    for _ in 0..directory.entry_count {
        if read_u32(data, offset)? != SIGNATURE_CENTRAL_HEADER {
            return Err(Error::CorruptHeader("central directory header signature"));
        }

        let flags = read_u16(data, offset + 8)?;
        if flags & FLAG_ENCRYPTED != 0 {
            return Err(Error::Encrypted);
        }

        let method = read_u16(data, offset + 10)?;
        let compression = match method {
            0 => Compression::Stored,
            8 => Compression::Deflate,
            other => return Err(Error::UnsupportedCompression(other)),
        };

        let name_length = usize::from(read_u16(data, offset + 28)?);
        let extra_length = usize::from(read_u16(data, offset + 30)?);
        let comment_length = usize::from(read_u16(data, offset + 32)?);

        let name_start = offset + CENTRAL_HEADER_SIZE;
        let name_bytes = data.get(name_start..name_start + name_length).ok_or(Error::Truncated)?;

        let extra_start = name_start + name_length;
        let extra = data.get(extra_start..extra_start + extra_length).ok_or(Error::Truncated)?;

        let (name, name_encoding) = decode_entry_name(name_bytes, flags, extra)?;
        check_name(&name)?;

        let mut entry = ZipEntry {
            name,
            compression,
            crc32: read_u32(data, offset + 16)?,
            compressed_size: u64::from(read_u32(data, offset + 20)?),
            uncompressed_size: u64::from(read_u32(data, offset + 24)?),
            last_modified: DosDateTime::from_raw(
                read_u16(data, offset + 14)?,
                read_u16(data, offset + 12)?,
            ),
            local_header_offset: u64::from(read_u32(data, offset + 42)?),
            name_encoding,
        };

        // Sizes are only trustworthy in the local header when no data descriptor
        // is used; the central directory always has the final values, so nothing
        // extra is needed here beyond noting why the flag is ignored.
        let _ = flags & FLAG_DATA_DESCRIPTOR;

        apply_zip64_extra_field(extra, &mut entry, data, offset)?;

        entries.push(entry);
        offset = extra_start + extra_length + comment_length;
    }

    Ok(entries)
}

/// Turns the stored name bytes into text.
///
/// The format's own rule — UTF-8 when bit 11 is set, the system code page
/// otherwise — is not enough in practice. Info-ZIP's `zip` writes UTF-8 bytes
/// and leaves the flag clear, and it is far from alone, so an archive from an
/// ordinary command line would come out mangled if the rule were applied
/// literally. The fallbacks below are what every practical ZIP reader does.
fn decode_entry_name(
    raw: &[u8],
    flags: u16,
    extra: &[u8],
) -> Result<(String, NameEncoding), Error> {
    if flags & FLAG_UTF8_NAME != 0 {
        let name = String::from_utf8(raw.to_vec()).map_err(|_| Error::InvalidNameEncoding)?;
        return Ok((name, NameEncoding::Utf8Declared));
    }

    if let Some(name) = unicode_path_extra_field(extra, raw) {
        return Ok((name, NameEncoding::Utf8FromExtraField));
    }

    // Valid UTF-8 is taken at face value. A CP437 name that also happens to be
    // valid multi-byte UTF-8 is possible in principle but not in practice, and
    // guessing the other way would break far more archives than it fixed.
    if let Ok(name) = core::str::from_utf8(raw) {
        return Ok((name.to_owned(), NameEncoding::Utf8Detected));
    }

    Ok((cp437::decode(raw), NameEncoding::CodePage437))
}

/// Extracts the UTF-8 name from an Info-ZIP Unicode Path extra field, if one is
/// present and still applies to this entry.
///
/// The field embeds a CRC-32 of the header name it was created for. If another
/// tool later renamed the entry without updating the field, that checksum no
/// longer matches and the field is stale — using it would silently resurrect an
/// old name, so it is ignored instead.
fn unicode_path_extra_field(extra: &[u8], raw_name: &[u8]) -> Option<String> {
    let mut cursor = 0usize;

    while cursor + 4 <= extra.len() {
        let field_id = u16::from_le_bytes([extra[cursor], extra[cursor + 1]]);
        let field_size = usize::from(u16::from_le_bytes([extra[cursor + 2], extra[cursor + 3]]));
        let body_start = cursor + 4;
        let body = extra.get(body_start..body_start + field_size)?;

        // Version byte, then the CRC-32, then the name: at least five bytes.
        if field_id == EXTRA_FIELD_UNICODE_PATH && body.len() >= 5 && body[0] == 1 {
            let recorded = u32::from_le_bytes([body[1], body[2], body[3], body[4]]);
            if wp_deflate::crc32(raw_name) == recorded {
                if let Ok(text) = core::str::from_utf8(&body[5..]) {
                    return Some(text.to_owned());
                }
            }
        }

        cursor = body_start + field_size;
    }

    None
}

/// Replaces sentinel values with the real ones from the Zip64 extra field.
///
/// The field carries only those values whose 32-bit slot overflowed, in a fixed
/// order, so which ones are present has to be inferred from the sentinels.
fn apply_zip64_extra_field(
    extra: &[u8],
    entry: &mut ZipEntry,
    data: &[u8],
    header_offset: usize,
) -> Result<(), Error> {
    let uncompressed_overflowed = read_u32(data, header_offset + 24)? == ZIP64_SENTINEL_32;
    let compressed_overflowed = read_u32(data, header_offset + 20)? == ZIP64_SENTINEL_32;
    let offset_overflowed = read_u32(data, header_offset + 42)? == ZIP64_SENTINEL_32;

    if !(uncompressed_overflowed || compressed_overflowed || offset_overflowed) {
        return Ok(());
    }

    let mut cursor = 0usize;
    while cursor + 4 <= extra.len() {
        let field_id = read_u16(extra, cursor)?;
        let field_size = usize::from(read_u16(extra, cursor + 2)?);
        let body_start = cursor + 4;
        let body = extra.get(body_start..body_start + field_size).ok_or(Error::Truncated)?;

        if field_id == EXTRA_FIELD_ZIP64 {
            let mut at = 0usize;
            let mut take = |target: &mut u64| -> Result<(), Error> {
                *target = read_u64(body, at)?;
                at += 8;
                Ok(())
            };
            if uncompressed_overflowed {
                take(&mut entry.uncompressed_size)?;
            }
            if compressed_overflowed {
                take(&mut entry.compressed_size)?;
            }
            if offset_overflowed {
                take(&mut entry.local_header_offset)?;
            }
            return Ok(());
        }

        cursor = body_start + field_size;
    }

    Err(Error::CorruptHeader("missing Zip64 extended information field"))
}

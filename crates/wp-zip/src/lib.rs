//! ZIP archive reading and writing.
//!
//! A `.docx` file is a ZIP archive, so this is the layer that turns one into a
//! set of named parts. The implementation follows the PKWARE APPNOTE format
//! specification, including the Zip64 extensions that documents with many or
//! large parts require.
//!
//! Scope is deliberately limited to what office documents actually use: stored
//! and deflated entries, single-volume archives, no encryption. Anything else
//! is reported as an error rather than guessed at.
//!
//! # Example
//!
//! ```no_run
//! use wp_zip::ZipArchive;
//!
//! let bytes = std::fs::read("document.docx")?;
//! let archive = ZipArchive::open(&bytes)?;
//! for entry in archive.entries() {
//!     println!("{} ({} bytes)", entry.name, entry.uncompressed_size);
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod cp437;
mod read;
mod write;

pub use read::ZipArchive;
pub use write::ZipWriter;

/// How an entry's data is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    /// Stored verbatim. Used for data that will not compress, and required for
    /// the `mimetype` entry of some OPC-adjacent formats.
    Stored,
    /// Compressed with DEFLATE. What nearly every entry of a `.docx` uses.
    Deflate,
}

impl Compression {
    fn to_code(self) -> u16 {
        match self {
            Self::Stored => 0,
            Self::Deflate => 8,
        }
    }
}

/// A timestamp in the MS-DOS format ZIP uses: two-second resolution, no time
/// zone, and no dates before 1980.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DosDateTime {
    date: u16,
    time: u16,
}

impl DosDateTime {
    /// The earliest timestamp the format can express, 1980-01-01 00:00:00.
    ///
    /// This is the default for archives we write. Using a fixed value keeps
    /// output reproducible: saving the same document twice produces identical
    /// bytes, which makes changes to a file reviewable.
    pub const EPOCH: Self = Self { date: 0x0021, time: 0 };

    /// Builds a timestamp from calendar components, clamping to what the format
    /// can represent. Seconds are rounded down to an even number.
    #[must_use]
    pub fn new(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Self {
        let year = year.clamp(1980, 2107) - 1980;
        let month = u16::from(month.clamp(1, 12));
        let day = u16::from(day.clamp(1, 31));
        let hour = u16::from(hour.min(23));
        let minute = u16::from(minute.min(59));
        let second = u16::from(second.min(59)) / 2;

        Self { date: (year << 9) | (month << 5) | day, time: (hour << 11) | (minute << 5) | second }
    }

    fn from_raw(date: u16, time: u16) -> Self {
        Self { date, time }
    }

    #[must_use]
    pub fn year(self) -> u16 {
        1980 + (self.date >> 9)
    }

    #[must_use]
    pub fn month(self) -> u8 {
        ((self.date >> 5) & 0x0F) as u8
    }

    #[must_use]
    pub fn day(self) -> u8 {
        (self.date & 0x1F) as u8
    }

    #[must_use]
    pub fn hour(self) -> u8 {
        (self.time >> 11) as u8
    }

    #[must_use]
    pub fn minute(self) -> u8 {
        ((self.time >> 5) & 0x3F) as u8
    }

    #[must_use]
    pub fn second(self) -> u8 {
        ((self.time & 0x1F) * 2) as u8
    }
}

impl Default for DosDateTime {
    fn default() -> Self {
        Self::EPOCH
    }
}

/// How an entry's name had to be decoded.
///
/// ZIP has no single answer for this. The format says a name is UTF-8 only when
/// bit 11 of the general-purpose flags is set, and otherwise belongs to the
/// archiving system's code page — but Info-ZIP's `zip`, among others, writes
/// UTF-8 bytes and leaves the flag clear. Recording which rule applied makes the
/// guesswork visible instead of hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameEncoding {
    /// Bit 11 was set: the name is UTF-8 by declaration.
    Utf8Declared,
    /// An Info-ZIP Unicode Path extra field supplied the name.
    Utf8FromExtraField,
    /// The flag was clear, but the bytes are valid UTF-8 and were taken as such.
    /// This also covers plain ASCII names, which read identically either way.
    Utf8Detected,
    /// The bytes are not valid UTF-8, so they were decoded as code page 437.
    CodePage437,
}

impl NameEncoding {
    /// Whether the name came from a Unicode source rather than a guess at a
    /// legacy code page.
    #[must_use]
    pub fn is_unicode(self) -> bool {
        !matches!(self, Self::CodePage437)
    }
}

/// One entry in an archive's central directory.
#[derive(Clone, Debug)]
pub struct ZipEntry {
    /// Path within the archive, always using forward slashes.
    pub name: String,
    /// How the data is stored.
    pub compression: Compression,
    /// CRC-32 of the uncompressed data, as recorded in the archive.
    pub crc32: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub last_modified: DosDateTime,
    /// Byte offset of this entry's local header.
    pub(crate) local_header_offset: u64,
    /// Which rule was used to turn the stored name bytes into text.
    pub name_encoding: NameEncoding,
}

impl ZipEntry {
    /// Whether the entry describes a directory rather than a file.
    ///
    /// ZIP has no real directory concept; the convention is a trailing slash and
    /// zero-length content.
    #[must_use]
    pub fn is_directory(&self) -> bool {
        self.name.ends_with('/')
    }
}

/// Why an archive could not be read or written.
///
/// English by design: these are diagnostics for developers and logs. Text shown
/// to the user is produced by the localized presentation layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// No end-of-central-directory record was found — this is not a ZIP file.
    NotAnArchive,
    /// The file ends in the middle of a structure it declares.
    Truncated,
    /// A required signature was not where the format says it should be.
    CorruptHeader(&'static str),
    /// The archive spans several volumes, which office documents never do.
    MultiVolume,
    /// The entry is encrypted. Password-protected documents are not yet handled.
    Encrypted,
    /// The entry uses a compression method outside the supported set.
    UnsupportedCompression(u16),
    /// Decompression of an entry failed.
    Inflate(wp_deflate::Error),
    /// The decompressed data does not match the recorded CRC-32.
    ChecksumMismatch { entry: String },
    /// The decompressed data is not the recorded length.
    SizeMismatch { entry: String, expected: u64, actual: u64 },
    /// The entry name is unusable: absolute, escaping the archive root, or
    /// containing characters that are illegal in a path.
    UnsafeName(String),
    /// The entry name is not valid UTF-8 even though it is flagged as such.
    InvalidNameEncoding,
    /// The archive exceeds a limit of the format itself.
    TooLarge(&'static str),
    /// Two entries in the archive share a name.
    DuplicateName(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotAnArchive => {
                f.write_str("not a ZIP archive: no end-of-central-directory record")
            }
            Self::Truncated => f.write_str("archive ends in the middle of a structure"),
            Self::CorruptHeader(what) => write!(f, "corrupt {what}"),
            Self::MultiVolume => f.write_str("multi-volume archives are not supported"),
            Self::Encrypted => f.write_str("the entry is encrypted"),
            Self::UnsupportedCompression(method) => {
                write!(f, "unsupported compression method {method}")
            }
            Self::Inflate(error) => write!(f, "decompression failed: {error}"),
            Self::ChecksumMismatch { entry } => write!(f, "checksum mismatch in entry {entry:?}"),
            Self::SizeMismatch { entry, expected, actual } => {
                write!(f, "entry {entry:?} is {actual} bytes, expected {expected}")
            }
            Self::UnsafeName(name) => write!(f, "unsafe entry name {name:?}"),
            Self::InvalidNameEncoding => f.write_str("entry name is not valid UTF-8"),
            Self::TooLarge(what) => write!(f, "archive exceeds a format limit: {what}"),
            Self::DuplicateName(name) => write!(f, "duplicate entry name {name:?}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<wp_deflate::Error> for Error {
    fn from(error: wp_deflate::Error) -> Self {
        Self::Inflate(error)
    }
}

/// Rejects entry names that would be dangerous to act on.
///
/// A ZIP entry name is attacker-controlled data. The classic attack ("zip slip")
/// uses `..` segments or an absolute path to make an extractor write outside the
/// destination directory. Names are checked once, here, so no caller has to
/// remember to do it.
pub(crate) fn check_name(name: &str) -> Result<(), Error> {
    let unsafe_name = || Error::UnsafeName(name.to_owned());

    if name.is_empty() {
        return Err(unsafe_name());
    }
    // Backslashes are not separators in ZIP, but Windows would treat them as
    // such, so a name containing one is ambiguous and rejected.
    if name.starts_with('/') || name.contains('\\') {
        return Err(unsafe_name());
    }
    // A drive letter or a UNC path smuggled in as a relative-looking name.
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        return Err(unsafe_name());
    }
    if name.contains('\0') {
        return Err(unsafe_name());
    }
    for segment in name.split('/') {
        if segment == ".." {
            return Err(unsafe_name());
        }
    }
    Ok(())
}

/// Reads a little-endian `u16` at `offset`.
pub(crate) fn read_u16(data: &[u8], offset: usize) -> Result<u16, Error> {
    let bytes = data.get(offset..offset + 2).ok_or(Error::Truncated)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Reads a little-endian `u32` at `offset`.
pub(crate) fn read_u32(data: &[u8], offset: usize) -> Result<u32, Error> {
    let bytes = data.get(offset..offset + 4).ok_or(Error::Truncated)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Reads a little-endian `u64` at `offset`.
pub(crate) fn read_u64(data: &[u8], offset: usize) -> Result<u64, Error> {
    let bytes = data.get(offset..offset + 8).ok_or(Error::Truncated)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_timestamp_roundtrips() {
        let stamp = DosDateTime::new(2026, 9, 7, 14, 35, 47);
        assert_eq!(stamp.year(), 2026);
        assert_eq!(stamp.month(), 9);
        assert_eq!(stamp.day(), 7);
        assert_eq!(stamp.hour(), 14);
        assert_eq!(stamp.minute(), 35);
        // Two-second resolution: the odd second is rounded down.
        assert_eq!(stamp.second(), 46);
    }

    #[test]
    fn dos_timestamp_epoch_is_the_format_minimum() {
        let epoch = DosDateTime::EPOCH;
        assert_eq!((epoch.year(), epoch.month(), epoch.day()), (1980, 1, 1));
        assert_eq!((epoch.hour(), epoch.minute(), epoch.second()), (0, 0, 0));
    }

    #[test]
    fn dangerous_names_are_rejected() {
        for name in [
            "",
            "/etc/passwd",
            "../outside.xml",
            "word/../../escape.xml",
            "C:/windows/system32",
            "word\\document.xml",
            "word/doc\0ument.xml",
        ] {
            assert!(check_name(name).is_err(), "should have been rejected: {name:?}");
        }
    }

    #[test]
    fn ordinary_names_are_accepted() {
        for name in [
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/media/image1.png",
            "word/",
            "docProps/app.xml",
            // A leading "..text" is not a parent-directory segment.
            "word/..text.xml",
        ] {
            assert!(check_name(name).is_ok(), "should have been accepted: {name:?}");
        }
    }
}

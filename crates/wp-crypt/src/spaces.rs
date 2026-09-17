//! The `\006DataSpaces` storage: what an encrypted document says about how it
//! was encrypted, apart from the encryption itself.
//!
//! # What it is for
//!
//! The format does not say "this file is encrypted". It says "the stream
//! called `EncryptedPackage` has had a transform applied to it, the transform
//! is called this, and here is what that transform needs to know" — because
//! the same machinery carries the rights-management path, where the transform
//! is a different one and the key comes from a server.
//!
//! So a file that says nothing about its data spaces is a file that says
//! nothing about what was done to its package. Office writes the storage;
//! this program did not, and a reader stricter than Word would have been
//! within its rights to refuse the result.
//!
//! # What is in it
//!
//! Four little structures, all of the same two shapes — a length and then
//! that many bytes, and a string written as its length in bytes followed by
//! UTF-16 padded out to a multiple of four:
//!
//! ```text
//! \006DataSpaces
//! ├── Version                     which version of this machinery wrote it
//! ├── DataSpaceMap                which stream is in which data space
//! ├── DataSpaceInfo
//! │   └── StrongEncryptionDataSpace    which transforms that space applies
//! └── TransformInfo
//!     └── StrongEncryptionTransform
//!         └── \006Primary               what the transform is
//! ```
//!
//! # What this does not do
//!
//! Decide anything. Nothing in here changes a single byte of the document:
//! the storage is a description, and the description of a file this program
//! wrote is the same every time. It is written because the format says it is
//! there, and read because a file whose transform is not the one this program
//! understands is a file it must not claim to have opened.

use wp_ole::Item;

/// The name of the storage, which begins with a control character: the format
/// marks its own structures that way so that they cannot be confused with
/// anything a person named.
pub(crate) const STORAGE: &str = "\u{6}DataSpaces";

/// What the transform is called, and the name of the space that applies it.
const SPACE: &str = "StrongEncryptionDataSpace";
const TRANSFORM: &str = "StrongEncryptionTransform";
/// The identifier the format gives the transform that is a password.
const TRANSFORM_ID: &str = "{FF9A3F03-56EF-4613-BDD5-5A41C1D07246}";

/// The stream inside a transform's storage, marked the same way.
const PRIMARY: &str = "\u{6}Primary";

/// Everything the storage holds, ready to be put into a compound file.
#[must_use]
pub(crate) fn storage() -> Item {
    Item::storage(
        STORAGE,
        vec![
            Item::stream("Version", version()),
            Item::stream("DataSpaceMap", map()),
            Item::storage("DataSpaceInfo", vec![Item::stream(SPACE, space())]),
            Item::storage(
                "TransformInfo",
                vec![Item::storage(TRANSFORM, vec![Item::stream(PRIMARY, primary())])],
            ),
        ],
    )
}

/// Which version of this machinery wrote the file, and which can read it.
fn version() -> Vec<u8> {
    let mut out = string("Microsoft.Container.DataSpaces");
    // Reader, updater and writer, each a major and a minor. All ones: there
    // has only ever been one version of this.
    for _ in 0..3 {
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

/// Which stream is in which data space: one entry, and it is the package.
fn map() -> Vec<u8> {
    let mut entry = Vec::new();
    // One reference, and it is to a stream rather than to a storage.
    entry.extend_from_slice(&1u32.to_le_bytes());
    entry.extend_from_slice(&0u32.to_le_bytes());
    entry.extend_from_slice(&string(crate::PACKAGE));
    entry.extend_from_slice(&string(SPACE));

    let mut out = Vec::new();
    // How long this header is, and how many entries follow it.
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    // Each entry says its own length, which counts the length itself.
    out.extend_from_slice(&((entry.len() + 4) as u32).to_le_bytes());
    out.extend_from_slice(&entry);
    out
}

/// What that space does: one transform, by name.
fn space() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&string(TRANSFORM));
    out
}

/// And what the transform is: the one that means a password.
fn primary() -> Vec<u8> {
    let mut header = Vec::new();
    // The sort of transform this is: one, which the format calls "other" and
    // uses for every transform anybody has written.
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&string(TRANSFORM_ID));
    header.extend_from_slice(&string("Microsoft.Container.EncryptionTransform"));
    for _ in 0..3 {
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes());
    }

    let mut out = Vec::new();
    // The header's own length, which counts the length itself.
    out.extend_from_slice(&((header.len() + 4) as u32).to_le_bytes());
    out.extend_from_slice(&header);
    // What the encryption is called, how big its blocks are and how it
    // chains them: all empty, because the agile description says all three
    // and saying them twice is two places to disagree.
    out.extend_from_slice(&string(""));
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

/// A string as these structures write one: its length in bytes, then UTF-16,
/// padded out to a multiple of four.
fn string(text: &str) -> Vec<u8> {
    let mut units: Vec<u8> = Vec::new();
    for unit in text.encode_utf16() {
        units.extend_from_slice(&unit.to_le_bytes());
    }
    let mut out = (units.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&units);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// Whether a file's data spaces say it is a password and not something else.
///
/// A file with no such storage is taken at its word: Word writes one, and
/// Word opens files without one, and a document this program wrote before
/// this was written has none. What is refused is a file that names a
/// transform this program does not understand — a rights-managed one, whose
/// key comes from a server nobody here can ask.
#[must_use]
pub(crate) fn understood(file: &wp_ole::CompoundFile) -> bool {
    let Some(bytes) = file.walk(&[STORAGE, "TransformInfo", TRANSFORM, PRIMARY]) else {
        // No storage at all, or one that does not name this transform. The
        // second is the interesting case and is covered below.
        return file.walk(&[STORAGE, "DataSpaceMap"]).is_none();
    };
    read_string(&bytes, 8).is_some_and(|name| name == TRANSFORM_ID)
}

/// One of those strings, read back from a place in some bytes.
fn read_string(bytes: &[u8], at: usize) -> Option<String> {
    let length = bytes
        .get(at..at + 4)
        .map(|slice| u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as usize)?;
    let text = bytes.get(at + 4..at + 4 + length)?;
    let units: Vec<u16> =
        text.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    String::from_utf16(&units).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_is_its_length_then_utf16_padded_to_four() {
        // "ab" is four bytes of text, so four and four with no padding.
        assert_eq!(string("ab"), vec![4, 0, 0, 0, b'a', 0, b'b', 0]);
        // "abc" is six, which is padded out to eight.
        let written = string("abc");
        assert_eq!(written.len(), 12);
        assert_eq!(&written[..4], &[6, 0, 0, 0]);
        assert_eq!(&written[10..], &[0, 0], "it was not padded");
        // And nothing at all is a length of nought and no text.
        assert_eq!(string(""), vec![0, 0, 0, 0]);
    }

    #[test]
    fn a_string_reads_back_as_what_was_written() {
        let written = string("Microsoft.Container.DataSpaces");
        assert_eq!(read_string(&written, 0).as_deref(), Some("Microsoft.Container.DataSpaces"));
    }

    #[test]
    fn the_map_names_the_package_and_the_space_it_is_in() {
        let bytes = map();
        // The header says eight and one: eight bytes of header, one entry.
        assert_eq!(&bytes[..8], &[8, 0, 0, 0, 1, 0, 0, 0]);
        // Then the entry's length, its one reference, and the two names.
        let entry_length = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        assert_eq!(entry_length, bytes.len() - 8, "the entry does not fill the stream");
        assert_eq!(read_string(&bytes, 20).as_deref(), Some(crate::PACKAGE));
    }

    #[test]
    fn the_transform_is_the_one_that_means_a_password() {
        let bytes = primary();
        assert_eq!(read_string(&bytes, 8).as_deref(), Some(TRANSFORM_ID));
        assert_eq!(
            read_string(&bytes, 8 + 4 + 76).as_deref(),
            Some("Microsoft.Container.EncryptionTransform"),
            "the transform's name does not follow its identifier"
        );
    }
}

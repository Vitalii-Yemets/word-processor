//! Names from the `name` table.
//!
//! A font carries its names several times over, once per platform and language.
//! The Windows records are UTF-16, the old Macintosh ones are in a single-byte
//! encoding, and a font localized into several languages has a record for each.
//! An English Windows record is preferred, because that is the name a document
//! refers to when it says `w:ascii="Times New Roman"`.

use crate::read::Reader;

/// Name identifier for the family, such as "Cambria".
pub(crate) const FAMILY: u16 = 1;
/// Name identifier for the full name, such as "Cambria Bold".
pub(crate) const FULL: u16 = 4;

/// Finds a name, preferring an English Windows record.
pub(crate) fn find(data: &[u8], offset: usize, wanted: u16) -> Option<String> {
    let mut reader = Reader::at(data, offset).ok()?;
    let _format = reader.u16().ok()?;
    let count = reader.u16().ok()?;
    let strings_at = offset + usize::from(reader.u16().ok()?);

    let mut best: Option<(u8, String)> = None;

    for _ in 0..count {
        let platform = reader.u16().ok()?;
        let encoding = reader.u16().ok()?;
        let language = reader.u16().ok()?;
        let name_id = reader.u16().ok()?;
        let length = usize::from(reader.u16().ok()?);
        let string_offset = usize::from(reader.u16().ok()?);

        if name_id != wanted {
            continue;
        }

        let start = strings_at.checked_add(string_offset)?;
        let Ok(mut string_reader) = Reader::at(data, start) else {
            continue;
        };
        let Ok(bytes) = string_reader.bytes(length) else {
            continue;
        };

        // Ranked so that the record a document would name wins.
        let (rank, decoded) = match (platform, encoding, language) {
            // Windows, Unicode, US English.
            (3, 1 | 10, 0x0409) => (4, decode_utf16(bytes)),
            (3, 1 | 10, _) => (3, decode_utf16(bytes)),
            (0, _, _) => (2, decode_utf16(bytes)),
            // Macintosh Roman, which is ASCII for every name we care about.
            (1, 0, _) => (1, decode_mac_roman(bytes)),
            _ => continue,
        };

        let Some(decoded) = decoded else { continue };
        if best.as_ref().is_none_or(|(best_rank, _)| rank > *best_rank) {
            best = Some((rank, decoded));
        }
    }

    best.map(|(_, name)| name)
}

/// Decodes a UTF-16 big-endian name.
fn decode_utf16(bytes: &[u8]) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> =
        bytes.chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect();
    String::from_utf16(&units).ok()
}

/// Decodes a Macintosh Roman name.
///
/// Only the ASCII range is mapped. Everything above it is replaced rather than
/// guessed at: the alternative is a name with wrong characters in it, which is
/// worse than a visibly incomplete one.
fn decode_mac_roman(bytes: &[u8]) -> Option<String> {
    Some(
        bytes
            .iter()
            .map(|&byte| if byte < 0x80 { byte as char } else { '\u{FFFD}' })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a `name` table with one Windows English family record.
    fn table_with(name: &str) -> Vec<u8> {
        let encoded: Vec<u8> =
            name.encode_utf16().flat_map(|unit| unit.to_be_bytes()).collect();

        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_be_bytes()); // format
        out.extend_from_slice(&1u16.to_be_bytes()); // count
        out.extend_from_slice(&18u16.to_be_bytes()); // stringOffset

        out.extend_from_slice(&3u16.to_be_bytes()); // platform: Windows
        out.extend_from_slice(&1u16.to_be_bytes()); // encoding: Unicode
        out.extend_from_slice(&0x0409u16.to_be_bytes()); // language: US English
        out.extend_from_slice(&FAMILY.to_be_bytes());
        out.extend_from_slice(&(encoded.len() as u16).to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // offset within the strings

        out.extend_from_slice(&encoded);
        out
    }

    #[test]
    fn reads_a_windows_name() {
        let data = table_with("Times New Roman");
        assert_eq!(find(&data, 0, FAMILY).as_deref(), Some("Times New Roman"));
    }

    #[test]
    fn a_name_that_is_not_there_is_none() {
        let data = table_with("Cambria");
        assert_eq!(find(&data, 0, FULL), None);
    }

    #[test]
    fn names_in_other_scripts_decode_correctly() {
        let data = table_with("Шрифт 文字");
        assert_eq!(find(&data, 0, FAMILY).as_deref(), Some("Шрифт 文字"));
    }

    #[test]
    fn a_truncated_table_is_not_a_panic() {
        let data = table_with("Cambria");
        for cut in 0..data.len() {
            let _ = find(&data[..cut], 0, FAMILY);
        }
    }
}

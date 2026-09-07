//! Code page 437, the legacy encoding of ZIP entry names.
//!
//! An entry name is UTF-8 only when bit 11 of the general-purpose flags is set.
//! Older tools leave it clear, and the name is then in the archiving system's
//! code page — for which the format specification names CP437. Interpreting
//! those bytes as UTF-8 would either fail or silently mangle the name, so they
//! are decoded properly instead.
//!
//! Only decoding is needed: names we write are always UTF-8 with the flag set.

/// Unicode characters for byte values 0x80..=0xFF. Bytes below 0x80 are ASCII
/// and map to themselves.
const HIGH_HALF: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', //
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', //
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', //
    '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', //
    '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', //
    '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀', //
    'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', //
    '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{00A0}',
];

/// Decodes bytes as CP437. Every byte value has a mapping, so this never fails.
pub(crate) fn decode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| {
            if byte < 0x80 {
                byte as char
            } else {
                HIGH_HALF[usize::from(byte) - 0x80]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passes_through() {
        assert_eq!(decode(b"word/document.xml"), "word/document.xml");
        assert_eq!(decode(b"[Content_Types].xml"), "[Content_Types].xml");
    }

    #[test]
    fn high_bytes_decode_to_their_cp437_characters() {
        assert_eq!(decode(&[0x80]), "Ç");
        assert_eq!(decode(&[0xE1]), "ß");
        assert_eq!(decode(&[0xFF]), "\u{00A0}");
        assert_eq!(decode(&[0x81, 0x94, 0x9A]), "üöÜ");
    }

    #[test]
    fn every_byte_has_a_mapping() {
        for byte in 0u8..=255 {
            assert_eq!(decode(&[byte]).chars().count(), 1);
        }
    }
}

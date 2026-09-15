//! From an X keysym to what the application understands: a key, or a
//! character.
//!
//! X names every key with a keysym. The Latin ones are their own
//! characters; the rest of Unicode is the code point with a bit set; and
//! the keys that are not characters — arrows, function keys, Enter — have
//! numbers of their own in the range beginning at `0xFF00`.

use crate::Key;

/// The key a keysym is, of the ones the application listens for.
pub(crate) fn key_of(keysym: u32) -> Option<Key> {
    Some(match keysym {
        0x0061..=0x007A => Key::Letter(char::from(keysym as u8)),
        0x0041..=0x005A => Key::Letter(char::from(keysym as u8 + 32)),
        0x0030..=0x0039 => Key::Digit(char::from(keysym as u8)),
        0xFF08 => Key::Backspace,
        0xFF09 | 0xFE20 => Key::Tab,
        0xFF0D | 0xFF8D => Key::Enter,
        0xFF1B => Key::Escape,
        0xFF50 | 0xFF95 => Key::Home,
        0xFF51 | 0xFF96 => Key::Left,
        0xFF52 | 0xFF97 => Key::Up,
        0xFF53 | 0xFF98 => Key::Right,
        0xFF54 | 0xFF99 => Key::Down,
        0xFF55 | 0xFF9A => Key::PageUp,
        0xFF56 | 0xFF9B => Key::PageDown,
        0xFF57 | 0xFF9C => Key::End,
        0xFFFF | 0xFF9F => Key::Delete,
        0x0020 => Key::Space,
        0xFFBE..=0xFFC9 => Key::Function((keysym - 0xFFBE + 1) as u8),
        _ => return None,
    })
}

/// The character a keysym types, if it types one.
pub(crate) fn char_of(keysym: u32) -> Option<char> {
    match keysym {
        0x0020..=0x007E | 0x00A0..=0x00FF => char::from_u32(keysym),
        0xFF0D | 0xFF8D => Some('\r'),
        0xFF09 => Some('\t'),
        // The keypad's digits and signs.
        0xFFAA => Some('*'),
        0xFFAB => Some('+'),
        0xFFAC => Some(','),
        0xFFAD => Some('-'),
        0xFFAE => Some('.'),
        0xFFAF => Some('/'),
        0xFFB0..=0xFFB9 => char::from_u32(keysym - 0xFFB0 + u32::from(b'0')),
        // Unicode keysyms carry the code point with a bit set.
        0x0100_0000..=0x0110_FFFF => char::from_u32(keysym - 0x0100_0000),
        _ => legacy_char(keysym),
    }
}

/// The keysyms of the other Latin, Greek and Cyrillic sets, which predate
/// the Unicode ones and which a keyboard layout may still give.
fn legacy_char(keysym: u32) -> Option<char> {
    let code = match keysym {
        // Latin-2, the part a Central European layout types.
        0x01A1 => 0x0104,
        0x01A2 => 0x02D8,
        0x01A3 => 0x0141,
        0x01A5 => 0x013D,
        0x01A6 => 0x015A,
        0x01A9 => 0x0160,
        0x01AA => 0x015E,
        0x01AB => 0x0164,
        0x01AC => 0x0179,
        0x01AE => 0x017D,
        0x01AF => 0x017B,
        0x01B1 => 0x0105,
        0x01B2 => 0x02DB,
        0x01B3 => 0x0142,
        0x01B5 => 0x013E,
        0x01B6 => 0x015B,
        0x01B7 => 0x02C7,
        0x01B9 => 0x0161,
        0x01BA => 0x015F,
        0x01BB => 0x0165,
        0x01BC => 0x017A,
        0x01BD => 0x02DD,
        0x01BE => 0x017E,
        0x01BF => 0x017C,
        0x01C0 => 0x0154,
        0x01C3 => 0x0102,
        0x01C5 => 0x0139,
        0x01C6 => 0x0106,
        0x01C8 => 0x010C,
        0x01CA => 0x0118,
        0x01CC => 0x011A,
        0x01CF => 0x010E,
        0x01D0 => 0x0110,
        0x01D1 => 0x0143,
        0x01D2 => 0x0147,
        0x01D5 => 0x0150,
        0x01D8 => 0x0158,
        0x01D9 => 0x016E,
        0x01DB => 0x0170,
        0x01DE => 0x0162,
        0x01E0 => 0x0155,
        0x01E3 => 0x0103,
        0x01E5 => 0x013A,
        0x01E6 => 0x0107,
        0x01E8 => 0x010D,
        0x01EA => 0x0119,
        0x01EC => 0x011B,
        0x01EF => 0x010F,
        0x01F0 => 0x0111,
        0x01F1 => 0x0144,
        0x01F2 => 0x0148,
        0x01F5 => 0x0151,
        0x01F8 => 0x0159,
        0x01F9 => 0x016F,
        0x01FB => 0x0171,
        0x01FE => 0x0163,
        0x01FF => 0x02D9,
        // Greek.
        0x07C1..=0x07D9 => keysym - 0x07C1 + 0x0391,
        0x07E1..=0x07F9 => keysym - 0x07E1 + 0x03B1,
        // Cyrillic, whose keysyms are not in Unicode's order.
        0x06A1 => 0x0452,
        0x06A2 => 0x0453,
        0x06A3 => 0x0451,
        0x06A4 => 0x0454,
        0x06A5 => 0x0455,
        0x06A6 => 0x0456,
        0x06A7 => 0x0457,
        0x06A8 => 0x0458,
        0x06A9 => 0x0459,
        0x06AA => 0x045A,
        0x06AB => 0x045B,
        0x06AC => 0x045C,
        0x06AD => 0x0491,
        0x06AE => 0x045E,
        0x06AF => 0x045F,
        0x06B1 => 0x0402,
        0x06B2 => 0x0403,
        0x06B3 => 0x0401,
        0x06B4 => 0x0404,
        0x06B5 => 0x0405,
        0x06B6 => 0x0406,
        0x06B7 => 0x0407,
        0x06B8 => 0x0408,
        0x06B9 => 0x0409,
        0x06BA => 0x040A,
        0x06BB => 0x040B,
        0x06BC => 0x040C,
        0x06BD => 0x0490,
        0x06BE => 0x040E,
        0x06BF => 0x040F,
        0x06C0..=0x06DF => CYRILLIC_LOWER[(keysym - 0x06C0) as usize],
        0x06E0..=0x06FF => CYRILLIC_LOWER[(keysym - 0x06E0) as usize] - 0x20,
        _ => return None,
    };
    char::from_u32(code)
}

/// The lower-case Cyrillic letters in keysym order, which follows the old
/// KOI8 layout rather than the alphabet.
const CYRILLIC_LOWER: [u32; 32] = [
    0x044E, 0x0430, 0x0431, 0x0446, 0x0434, 0x0435, 0x0444, 0x0433, 0x0445, 0x0438, 0x0439, 0x043A,
    0x043B, 0x043C, 0x043D, 0x043E, 0x043F, 0x044F, 0x0440, 0x0441, 0x0442, 0x0443, 0x0436, 0x0432,
    0x044C, 0x044B, 0x0437, 0x0448, 0x044D, 0x0449, 0x0447, 0x044A,
];

/// The modifier keys themselves, which type nothing.
pub(crate) fn is_modifier(keysym: u32) -> bool {
    matches!(keysym, 0xFFE1..=0xFFEE | 0xFF7E | 0xFE03)
}

pub(crate) fn is_alt(keysym: u32) -> bool {
    matches!(keysym, 0xFFE9 | 0xFFEA)
}

pub(crate) fn is_control(keysym: u32) -> bool {
    matches!(keysym, 0xFFE3 | 0xFFE4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keysyms_are_keys_and_characters() {
        assert_eq!(key_of(0x0061), Some(Key::Letter('a')));
        assert_eq!(key_of(0x0041), Some(Key::Letter('a')));
        assert_eq!(key_of(0xFF51), Some(Key::Left));
        assert_eq!(key_of(0xFFC4), Some(Key::Function(7)));
        assert_eq!(char_of(0x0041), Some('A'));
        assert_eq!(char_of(0x00E9), Some('é'));
        assert_eq!(char_of(0x0100_0416), Some('Ж'));
        assert_eq!(char_of(0x06D6), Some('ж'));
        assert_eq!(char_of(0x06F6), Some('Ж'));
        assert_eq!(char_of(0x07E1), Some('α'));
        assert_eq!(char_of(0xFFB7), Some('7'));
        assert_eq!(char_of(0xFF51), None);
        assert!(is_modifier(0xFFE1));
    }
}

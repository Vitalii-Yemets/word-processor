//! From the bytes a font is shown to the characters they mean.
//!
//! A simple font takes one byte a character, through an encoding — the
//! standard one, Windows', the Macintosh's — with the font's own changes
//! on top, given as glyph names. A composite font takes one to four bytes
//! a character through a CMap, which says how many bytes and which
//! character id. Either may carry a ToUnicode CMap, the table saying which
//! text each code was written for, which is the one thing a program
//! copying text out of a file can trust.

use std::collections::HashMap;

use super::object::{Lexer, Object};

/// A CMap: which byte lengths codes come in, and where each code goes —
/// to a character id, or to text.
#[derive(Clone, Debug, Default)]
pub struct CMap {
    /// Byte length, low, high.
    codespaces: Vec<(usize, u32, u32)>,
    single: HashMap<u32, u32>,
    ranges: Vec<(u32, u32, u32)>,
    text: HashMap<u32, String>,
    text_ranges: Vec<(u32, u32, String)>,
    /// The widest code any mapping used, for a map that states no
    /// codespace.
    widest: usize,
    pub vertical: bool,
    /// For the predefined Unicode CMaps: the form the codes are Unicode
    /// in, and the id of the space, where the collection's Latin starts.
    unicode: Option<(Form, u32)>,
}

/// The forms of Unicode the predefined `Uni` CMaps take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    Ucs2,
    Utf16,
    Utf8,
    Utf32,
}

impl CMap {
    /// The identity: two bytes a code, the code its own id.
    #[must_use]
    pub fn identity() -> Self {
        Self {
            codespaces: vec![(2, 0, 0xFFFF)],
            ranges: vec![(0, 0xFFFF, 0)],
            ..Default::default()
        }
    }

    /// A predefined CMap by name: the identities, and the Unicode ones of
    /// the CJK collections, whose codes are the characters themselves.
    /// The rest, the older national encodings, are read as two bytes a
    /// code so that a ToUnicode table can still name them.
    #[must_use]
    pub fn predefined(name: &str) -> Self {
        let form = if !name.starts_with("Uni") {
            None
        } else if name.contains("UCS2") {
            Some(Form::Ucs2)
        } else if name.contains("UTF16") {
            Some(Form::Utf16)
        } else if name.contains("UTF8") {
            Some(Form::Utf8)
        } else if name.contains("UTF32") {
            Some(Form::Utf32)
        } else {
            None
        };
        let mut map = match form {
            None => Self::identity(),
            Some(form) => {
                let codespaces = match form {
                    Form::Ucs2 => vec![(2, 0, 0xFFFF)],
                    Form::Utf16 => {
                        vec![(2, 0, 0xD7FF), (4, 0xD800_DC00, 0xDBFF_DFFF), (2, 0xE000, 0xFFFF)]
                    }
                    Form::Utf8 => vec![
                        (1, 0, 0x7F),
                        (2, 0xC280, 0xDFBF),
                        (3, 0xE0_8080, 0xEF_BFBF),
                        (4, 0xF080_8080, 0xF48F_BFBF),
                    ],
                    Form::Utf32 => vec![(4, 0, 0x10_FFFF)],
                };
                // Every Adobe CJK collection has the printable ASCII from
                // id 1, proportional; Japanese has it half-width from 231
                // too, which the "HW" maps use.
                let space = if name.contains("-HW-") { 231 } else { 1 };
                Self { codespaces, unicode: Some((form, space)), ..Default::default() }
            }
        };
        map.vertical = name.ends_with("-V");
        map
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.single.is_empty()
            && self.ranges.is_empty()
            && self.text.is_empty()
            && self.text_ranges.is_empty()
    }

    /// Reads an embedded CMap, in the PostScript-like syntax.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Self {
        let mut map = Self::default();
        let mut lexer = Lexer::new(bytes);
        let mut stack: Vec<Object> = Vec::new();
        while let Some(object) = lexer.next_object() {
            let Object::Operator(word) = object else {
                stack.push(object);
                if stack.len() > 64 {
                    stack.remove(0);
                }
                continue;
            };
            match word.as_str() {
                "begincodespacerange" => {
                    let items = read_until(&mut lexer, "endcodespacerange");
                    for pair in items.chunks_exact(2) {
                        if let (Some(low), Some(high)) = (pair[0].as_string(), pair[1].as_string())
                        {
                            let length = low.len().clamp(1, 4);
                            map.codespaces.push((length, code_of(low), code_of(high)));
                        }
                    }
                }
                "begincidrange" => {
                    let items = read_until(&mut lexer, "endcidrange");
                    for triple in items.chunks_exact(3) {
                        if let (Some(low), Some(high), Some(cid)) =
                            (triple[0].as_string(), triple[1].as_string(), triple[2].as_integer())
                        {
                            map.note_length(low.len());
                            map.ranges.push((code_of(low), code_of(high), cid.max(0) as u32));
                        }
                    }
                }
                "begincidchar" => {
                    let items = read_until(&mut lexer, "endcidchar");
                    for pair in items.chunks_exact(2) {
                        if let (Some(code), Some(cid)) = (pair[0].as_string(), pair[1].as_integer())
                        {
                            map.note_length(code.len());
                            map.single.insert(code_of(code), cid.max(0) as u32);
                        }
                    }
                }
                "beginbfchar" => {
                    let items = read_until(&mut lexer, "endbfchar");
                    for pair in items.chunks_exact(2) {
                        let Some(code) = pair[0].as_string() else { continue };
                        map.note_length(code.len());
                        match &pair[1] {
                            Object::String(text) => {
                                map.text.insert(code_of(code), utf16_of(text));
                            }
                            Object::Name(name) => {
                                if let Some(c) = glyph_char(name) {
                                    map.text.insert(code_of(code), c.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "beginbfrange" => {
                    let items = read_until(&mut lexer, "endbfrange");
                    for triple in items.chunks_exact(3) {
                        let (Some(low), Some(high)) =
                            (triple[0].as_string(), triple[1].as_string())
                        else {
                            continue;
                        };
                        map.note_length(low.len());
                        let (low, high) = (code_of(low), code_of(high));
                        match &triple[2] {
                            Object::String(text) => {
                                map.text_ranges.push((low, high, utf16_of(text)));
                            }
                            Object::Array(texts) => {
                                for (offset, text) in texts.iter().enumerate() {
                                    if let Some(text) = text.as_string() {
                                        map.text.insert(low + offset as u32, utf16_of(text));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "usecmap" => {
                    if let Some(Object::Name(name)) = stack.last() {
                        if name.starts_with("Identity") {
                            let identity = Self::identity();
                            map.codespaces.extend(identity.codespaces);
                            map.ranges.extend(identity.ranges);
                        }
                    }
                }
                "endcmap" => break,
                _ => {}
            }
        }
        if map.codespaces.is_empty() {
            // No codespace said: one byte, unless the mappings are wider.
            let widest = map.widest.max(1);
            map.codespaces.push((
                widest,
                0,
                if widest >= 4 { u32::MAX } else { (1 << (8 * widest)) - 1 },
            ));
        }
        map
    }

    fn note_length(&mut self, length: usize) {
        self.widest = self.widest.max(length.clamp(1, 4));
    }

    /// The next code in the bytes: the code and how many bytes it took.
    /// A code no codespace holds takes the shortest codespace's length, so
    /// that the reading stays in step.
    #[must_use]
    pub fn next_code(&self, bytes: &[u8]) -> (u32, usize) {
        for length in 1..=4 {
            if bytes.len() < length {
                break;
            }
            let code = code_of(&bytes[..length]);
            if self
                .codespaces
                .iter()
                .any(|&(l, low, high)| l == length && code >= low && code <= high)
            {
                return (code, length);
            }
        }
        // Partial match on the first byte decides the length, as the
        // standard says; otherwise the shortest.
        let first = u32::from(bytes[0]);
        let length = self
            .codespaces
            .iter()
            .find(|&&(l, low, high)| {
                let shift = 8 * (l - 1);
                first >= (low >> shift) && first <= (high >> shift)
            })
            .map(|&(l, _, _)| l)
            .or_else(|| self.codespaces.iter().map(|&(l, _, _)| l).min())
            .unwrap_or(1)
            .min(bytes.len())
            .max(1);
        (code_of(&bytes[..length]), length)
    }

    /// The character id of a code.
    #[must_use]
    pub fn cid(&self, code: u32) -> u32 {
        if let Some((form, space)) = self.unicode {
            // The collection's ids for the rest are its own tables', which
            // this does not carry: they take the font's default width.
            return match unicode_of(form, code) {
                Some(c @ ' '..='~') => u32::from(c) - 0x20 + space,
                _ => 0,
            };
        }
        if let Some(&cid) = self.single.get(&code) {
            return cid;
        }
        for &(low, high, cid) in &self.ranges {
            if code >= low && code <= high {
                return cid + (code - low);
            }
        }
        0
    }

    /// The text of a code, when the map says.
    #[must_use]
    pub fn text(&self, code: u32) -> Option<String> {
        if let Some(text) = self.text.get(&code) {
            return Some(text.clone());
        }
        for (low, high, base) in &self.text_ranges {
            if code >= *low && code <= *high {
                let offset = code - low;
                // The last character of the base steps up with the code.
                let mut chars: Vec<char> = base.chars().collect();
                if let Some(last) = chars.pop() {
                    let stepped = char::from_u32(u32::from(last) + offset).unwrap_or(last);
                    chars.push(stepped);
                }
                return Some(chars.into_iter().collect());
            }
        }
        self.unicode.and_then(|(form, _)| unicode_of(form, code)).map(String::from)
    }

    /// Whether the map has any text in it.
    #[must_use]
    pub fn has_text(&self) -> bool {
        !self.text.is_empty() || !self.text_ranges.is_empty()
    }
}

/// Reads objects up to an operator.
fn read_until(lexer: &mut Lexer<'_>, end: &str) -> Vec<Object> {
    let mut items = Vec::new();
    while let Some(object) = lexer.next_object() {
        match object {
            Object::Operator(word) if word == end => break,
            Object::Operator(_) => {}
            other => items.push(other),
        }
    }
    items
}

fn code_of(bytes: &[u8]) -> u32 {
    bytes.iter().take(4).fold(0u32, |acc, &b| (acc << 8) | u32::from(b))
}

/// The character a code of a Unicode CMap is.
fn unicode_of(form: Form, code: u32) -> Option<char> {
    match form {
        Form::Ucs2 | Form::Utf32 => char::from_u32(code),
        Form::Utf16 if code > 0xFFFF => {
            let (high, low) = (code >> 16, code & 0xFFFF);
            if !(0xD800..0xDC00).contains(&high) || !(0xDC00..0xE000).contains(&low) {
                return None;
            }
            char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00))
        }
        Form::Utf16 => char::from_u32(code),
        Form::Utf8 => {
            let bytes = code.to_be_bytes();
            let start = bytes.iter().position(|&b| b != 0).unwrap_or(3);
            std::str::from_utf8(&bytes[start..]).ok()?.chars().next()
        }
    }
}

/// A destination string: UTF-16BE, as ToUnicode maps write them — or a
/// single byte when a producer wrote one.
fn utf16_of(bytes: &[u8]) -> String {
    if bytes.len() % 2 != 0 {
        return bytes.iter().map(|&b| char::from(b)).collect();
    }
    let units: Vec<u16> =
        bytes.chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect();
    String::from_utf16_lossy(&units)
}

// --- Encodings --------------------------------------------------------------

/// The base encodings a simple font may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseEncoding {
    Standard,
    WinAnsi,
    MacRoman,
    /// The Symbol font's own.
    Symbol,
    /// Whatever the font has built in, taken as Latin-1.
    Builtin,
}

impl BaseEncoding {
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "StandardEncoding" => Some(Self::Standard),
            "WinAnsiEncoding" => Some(Self::WinAnsi),
            "MacRomanEncoding" => Some(Self::MacRoman),
            "MacExpertEncoding" => Some(Self::Standard),
            _ => None,
        }
    }

    /// The character of a code under this encoding.
    #[must_use]
    pub fn char_of(self, code: u8) -> Option<char> {
        match self {
            Self::WinAnsi => {
                if code < 0x80 {
                    return Some(char::from(code));
                }
                let decoded = wp_text::Encoding::CodePage(1252).decode(&[code]);
                let c = decoded.chars().next().unwrap_or('\u{FFFD}');
                Some(match c {
                    '\u{FFFD}' => '\u{2022}',
                    '\u{A0}' => ' ',
                    '\u{AD}' => '-',
                    other => other,
                })
            }
            Self::Standard => Some(match code {
                0x27 => '\u{2019}',
                0x60 => '\u{2018}',
                0x00..=0x7E => char::from(code),
                _ => STANDARD_UPPER.iter().find(|(c, _)| *c == code).map(|(_, ch)| *ch)?,
            }),
            Self::MacRoman => Some(if code < 0x80 {
                char::from(code)
            } else {
                MAC_ROMAN_UPPER[usize::from(code - 0x80)]
            }),
            Self::Symbol => symbol_char(code),
            Self::Builtin => {
                if code < 0x80 {
                    Some(char::from(code))
                } else {
                    Self::WinAnsi.char_of(code)
                }
            }
        }
    }
}

/// The standard encoding's upper half, the codes that have characters.
const STANDARD_UPPER: &[(u8, char)] = &[
    (0xA1, '\u{A1}'),
    (0xA2, '\u{A2}'),
    (0xA3, '\u{A3}'),
    (0xA4, '\u{2044}'),
    (0xA5, '\u{A5}'),
    (0xA6, '\u{192}'),
    (0xA7, '\u{A7}'),
    (0xA8, '\u{A4}'),
    (0xA9, '\''),
    (0xAA, '\u{201C}'),
    (0xAB, '\u{AB}'),
    (0xAC, '\u{2039}'),
    (0xAD, '\u{203A}'),
    (0xAE, '\u{FB01}'),
    (0xAF, '\u{FB02}'),
    (0xB1, '\u{2013}'),
    (0xB2, '\u{2020}'),
    (0xB3, '\u{2021}'),
    (0xB4, '\u{B7}'),
    (0xB6, '\u{B6}'),
    (0xB7, '\u{2022}'),
    (0xB8, '\u{201A}'),
    (0xB9, '\u{201E}'),
    (0xBA, '\u{201D}'),
    (0xBB, '\u{BB}'),
    (0xBC, '\u{2026}'),
    (0xBD, '\u{2030}'),
    (0xBF, '\u{BF}'),
    (0xC1, '`'),
    (0xC2, '\u{B4}'),
    (0xC3, '\u{2C6}'),
    (0xC4, '\u{2DC}'),
    (0xC5, '\u{AF}'),
    (0xC6, '\u{2D8}'),
    (0xC7, '\u{2D9}'),
    (0xC8, '\u{A8}'),
    (0xCA, '\u{2DA}'),
    (0xCB, '\u{B8}'),
    (0xCD, '\u{2DD}'),
    (0xCE, '\u{2DB}'),
    (0xCF, '\u{2C7}'),
    (0xD0, '\u{2014}'),
    (0xE1, '\u{C6}'),
    (0xE3, '\u{AA}'),
    (0xE8, '\u{141}'),
    (0xE9, '\u{D8}'),
    (0xEA, '\u{152}'),
    (0xEB, '\u{BA}'),
    (0xF1, '\u{E6}'),
    (0xF5, '\u{131}'),
    (0xF8, '\u{142}'),
    (0xF9, '\u{F8}'),
    (0xFA, '\u{153}'),
    (0xFB, '\u{DF}'),
];

/// The Macintosh Roman encoding's upper half, as the format defines it.
const MAC_ROMAN_UPPER: [char; 128] = [
    'Ä', 'Å', 'Ç', 'É', 'Ñ', 'Ö', 'Ü', 'á', 'à', 'â', 'ä', 'ã', 'å', 'ç', 'é', 'è', //
    'ê', 'ë', 'í', 'ì', 'î', 'ï', 'ñ', 'ó', 'ò', 'ô', 'ö', 'õ', 'ú', 'ù', 'û', 'ü', //
    '†', '°', '¢', '£', '§', '•', '¶', 'ß', '®', '©', '™', '´', '¨', '≠', 'Æ', 'Ø', //
    '∞', '±', '≤', '≥', '¥', 'µ', '∂', '∑', '∏', 'π', '∫', 'ª', 'º', 'Ω', 'æ', 'ø', //
    '¿', '¡', '¬', '√', 'ƒ', '≈', '∆', '«', '»', '…', ' ', 'À', 'Ã', 'Õ', 'Œ', 'œ', //
    '–', '—', '“', '”', '‘', '’', '÷', '◊', 'ÿ', 'Ÿ', '⁄', '¤', '‹', '›', 'ﬁ', 'ﬂ', //
    '‡', '·', '‚', '„', '‰', 'Â', 'Ê', 'Á', 'Ë', 'È', 'Í', 'Î', 'Ï', 'Ì', 'Ó', 'Ô', //
    '\u{F8FF}', 'Ò', 'Ú', 'Û', 'Ù', 'ı', 'ˆ', '˜', '¯', '˘', '˙', '˚', '¸', '˝', '˛', 'ˇ',
];

/// The Symbol font's encoding: Greek where the Latin letters are, and the
/// signs a formula or a bullet needs.
fn symbol_char(code: u8) -> Option<char> {
    Some(match code {
        0x20 => ' ',
        0x21 => '!',
        0x22 => '\u{2200}',
        0x23 => '#',
        0x24 => '\u{2203}',
        0x25 => '%',
        0x26 => '&',
        0x27 => '\u{220B}',
        0x28 => '(',
        0x29 => ')',
        0x2A => '\u{2217}',
        0x2B => '+',
        0x2C => ',',
        0x2D => '\u{2212}',
        0x2E => '.',
        0x2F => '/',
        0x30..=0x39 => char::from(code),
        0x3A => ':',
        0x3B => ';',
        0x3C => '<',
        0x3D => '=',
        0x3E => '>',
        0x3F => '?',
        0x40 => '\u{2245}',
        0x41 => 'Α',
        0x42 => 'Β',
        0x43 => 'Χ',
        0x44 => 'Δ',
        0x45 => 'Ε',
        0x46 => 'Φ',
        0x47 => 'Γ',
        0x48 => 'Η',
        0x49 => 'Ι',
        0x4A => 'ϑ',
        0x4B => 'Κ',
        0x4C => 'Λ',
        0x4D => 'Μ',
        0x4E => 'Ν',
        0x4F => 'Ο',
        0x50 => 'Π',
        0x51 => 'Θ',
        0x52 => 'Ρ',
        0x53 => 'Σ',
        0x54 => 'Τ',
        0x55 => 'Υ',
        0x56 => 'ς',
        0x57 => 'Ω',
        0x58 => 'Ξ',
        0x59 => 'Ψ',
        0x5A => 'Ζ',
        0x5B => '[',
        0x5C => '\u{2234}',
        0x5D => ']',
        0x5E => '\u{22A5}',
        0x5F => '_',
        0x60 => '\u{F8E5}',
        0x61 => 'α',
        0x62 => 'β',
        0x63 => 'χ',
        0x64 => 'δ',
        0x65 => 'ε',
        0x66 => 'φ',
        0x67 => 'γ',
        0x68 => 'η',
        0x69 => 'ι',
        0x6A => 'ϕ',
        0x6B => 'κ',
        0x6C => 'λ',
        0x6D => 'μ',
        0x6E => 'ν',
        0x6F => 'ο',
        0x70 => 'π',
        0x71 => 'θ',
        0x72 => 'ρ',
        0x73 => 'σ',
        0x74 => 'τ',
        0x75 => 'υ',
        0x76 => 'ϖ',
        0x77 => 'ω',
        0x78 => 'ξ',
        0x79 => 'ψ',
        0x7A => 'ζ',
        0x7B => '{',
        0x7C => '|',
        0x7D => '}',
        0x7E => '\u{223C}',
        0xA0 => '\u{20AC}',
        0xA1 => 'ϒ',
        0xA2 => '\u{2032}',
        0xA3 => '\u{2264}',
        0xA4 => '\u{2044}',
        0xA5 => '\u{221E}',
        0xA6 => 'ƒ',
        0xA7 => '\u{2663}',
        0xA8 => '\u{2666}',
        0xA9 => '\u{2665}',
        0xAA => '\u{2660}',
        0xAB => '\u{2194}',
        0xAC => '\u{2190}',
        0xAD => '\u{2191}',
        0xAE => '\u{2192}',
        0xAF => '\u{2193}',
        0xB0 => '°',
        0xB1 => '±',
        0xB2 => '\u{2033}',
        0xB3 => '\u{2265}',
        0xB4 => '×',
        0xB5 => '\u{221D}',
        0xB6 => '\u{2202}',
        0xB7 => '\u{2022}',
        0xB8 => '÷',
        0xB9 => '\u{2260}',
        0xBA => '\u{2261}',
        0xBB => '\u{2248}',
        0xBC => '\u{2026}',
        0xBD => '\u{23D0}',
        0xBE => '\u{23AF}',
        0xBF => '\u{21B5}',
        0xC0 => '\u{2135}',
        0xC1 => '\u{2111}',
        0xC2 => '\u{211C}',
        0xC3 => '\u{2118}',
        0xC4 => '\u{2297}',
        0xC5 => '\u{2295}',
        0xC6 => '\u{2205}',
        0xC7 => '\u{2229}',
        0xC8 => '\u{222A}',
        0xC9 => '\u{2283}',
        0xCA => '\u{2287}',
        0xCB => '\u{2284}',
        0xCC => '\u{2282}',
        0xCD => '\u{2286}',
        0xCE => '\u{2208}',
        0xCF => '\u{2209}',
        0xD0 => '\u{2220}',
        0xD1 => '\u{2207}',
        0xD2 => '®',
        0xD3 => '©',
        0xD4 => '™',
        0xD5 => '\u{220F}',
        0xD6 => '\u{221A}',
        0xD7 => '\u{22C5}',
        0xD8 => '¬',
        0xD9 => '\u{2227}',
        0xDA => '\u{2228}',
        0xDB => '\u{21D4}',
        0xDC => '\u{21D0}',
        0xDD => '\u{21D1}',
        0xDE => '\u{21D2}',
        0xDF => '\u{21D3}',
        0xE0 => '\u{25CA}',
        0xE1 => '\u{2329}',
        0xE5 => '\u{2211}',
        0xF1 => '\u{232A}',
        0xF2 => '\u{222B}',
        _ => return None,
    })
}

/// The character a code under a symbol font meant, when the font is one
/// of the two Word uses for bullets and the code is in the private range
/// the font's own cmap puts it in.
#[must_use]
pub fn private_use_bullet(c: char) -> Option<char> {
    Some(match c {
        '\u{F0B7}' | '\u{F0A7}' | '\u{F06C}' => '\u{2022}',
        '\u{F06E}' => '\u{25A0}',
        '\u{F071}' | '\u{F0A8}' => '\u{25A1}',
        '\u{F076}' => '\u{2756}',
        '\u{F0D8}' => '\u{27A2}',
        '\u{F0FC}' => '\u{2713}',
        '\u{F06F}' => '\u{25CB}',
        '\u{F0A1}' => '\u{25CF}',
        '\u{F02D}' => '\u{2013}',
        _ => return None,
    })
}

/// The character a glyph name stands for: the Adobe list's common names,
/// or the `uniXXXX` and `uXXXX` forms that spell the code out.
#[must_use]
pub fn glyph_char(name: &str) -> Option<char> {
    let name = name.split('.').next().unwrap_or(name);
    if let Some(hex) = name.strip_prefix("uni") {
        if hex.len() >= 4 {
            return u32::from_str_radix(&hex[..4], 16).ok().and_then(char::from_u32);
        }
    }
    if let Some(hex) = name.strip_prefix('u') {
        if (4..=6).contains(&hex.len()) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
        }
    }
    if name.chars().count() == 1 {
        return name.chars().next();
    }
    GLYPH_LIST.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// The glyph names a Latin document uses, from the Adobe Glyph List.
const GLYPH_LIST: &[(&str, char)] = &[
    ("space", ' '),
    ("exclam", '!'),
    ("quotedbl", '"'),
    ("numbersign", '#'),
    ("dollar", '$'),
    ("percent", '%'),
    ("ampersand", '&'),
    ("quotesingle", '\''),
    ("parenleft", '('),
    ("parenright", ')'),
    ("asterisk", '*'),
    ("plus", '+'),
    ("comma", ','),
    ("hyphen", '-'),
    ("period", '.'),
    ("slash", '/'),
    ("zero", '0'),
    ("one", '1'),
    ("two", '2'),
    ("three", '3'),
    ("four", '4'),
    ("five", '5'),
    ("six", '6'),
    ("seven", '7'),
    ("eight", '8'),
    ("nine", '9'),
    ("colon", ':'),
    ("semicolon", ';'),
    ("less", '<'),
    ("equal", '='),
    ("greater", '>'),
    ("question", '?'),
    ("at", '@'),
    ("bracketleft", '['),
    ("backslash", '\\'),
    ("bracketright", ']'),
    ("asciicircum", '^'),
    ("underscore", '_'),
    ("grave", '`'),
    ("braceleft", '{'),
    ("bar", '|'),
    ("braceright", '}'),
    ("asciitilde", '~'),
    ("exclamdown", '¡'),
    ("cent", '¢'),
    ("sterling", '£'),
    ("currency", '¤'),
    ("yen", '¥'),
    ("brokenbar", '¦'),
    ("section", '§'),
    ("dieresis", '¨'),
    ("copyright", '©'),
    ("ordfeminine", 'ª'),
    ("guillemotleft", '«'),
    ("logicalnot", '¬'),
    ("registered", '®'),
    ("macron", '¯'),
    ("degree", '°'),
    ("plusminus", '±'),
    ("twosuperior", '²'),
    ("threesuperior", '³'),
    ("acute", '´'),
    ("mu", 'µ'),
    ("paragraph", '¶'),
    ("periodcentered", '·'),
    ("cedilla", '¸'),
    ("onesuperior", '¹'),
    ("ordmasculine", 'º'),
    ("guillemotright", '»'),
    ("onequarter", '¼'),
    ("onehalf", '½'),
    ("threequarters", '¾'),
    ("questiondown", '¿'),
    ("Agrave", 'À'),
    ("Aacute", 'Á'),
    ("Acircumflex", 'Â'),
    ("Atilde", 'Ã'),
    ("Adieresis", 'Ä'),
    ("Aring", 'Å'),
    ("AE", 'Æ'),
    ("Ccedilla", 'Ç'),
    ("Egrave", 'È'),
    ("Eacute", 'É'),
    ("Ecircumflex", 'Ê'),
    ("Edieresis", 'Ë'),
    ("Igrave", 'Ì'),
    ("Iacute", 'Í'),
    ("Icircumflex", 'Î'),
    ("Idieresis", 'Ï'),
    ("Eth", 'Ð'),
    ("Ntilde", 'Ñ'),
    ("Ograve", 'Ò'),
    ("Oacute", 'Ó'),
    ("Ocircumflex", 'Ô'),
    ("Otilde", 'Õ'),
    ("Odieresis", 'Ö'),
    ("multiply", '×'),
    ("Oslash", 'Ø'),
    ("Ugrave", 'Ù'),
    ("Uacute", 'Ú'),
    ("Ucircumflex", 'Û'),
    ("Udieresis", 'Ü'),
    ("Yacute", 'Ý'),
    ("Thorn", 'Þ'),
    ("germandbls", 'ß'),
    ("agrave", 'à'),
    ("aacute", 'á'),
    ("acircumflex", 'â'),
    ("atilde", 'ã'),
    ("adieresis", 'ä'),
    ("aring", 'å'),
    ("ae", 'æ'),
    ("ccedilla", 'ç'),
    ("egrave", 'è'),
    ("eacute", 'é'),
    ("ecircumflex", 'ê'),
    ("edieresis", 'ë'),
    ("igrave", 'ì'),
    ("iacute", 'í'),
    ("icircumflex", 'î'),
    ("idieresis", 'ï'),
    ("eth", 'ð'),
    ("ntilde", 'ñ'),
    ("ograve", 'ò'),
    ("oacute", 'ó'),
    ("ocircumflex", 'ô'),
    ("otilde", 'õ'),
    ("odieresis", 'ö'),
    ("divide", '÷'),
    ("oslash", 'ø'),
    ("ugrave", 'ù'),
    ("uacute", 'ú'),
    ("ucircumflex", 'û'),
    ("udieresis", 'ü'),
    ("yacute", 'ý'),
    ("thorn", 'þ'),
    ("ydieresis", 'ÿ'),
    ("quoteleft", '\u{2018}'),
    ("quoteright", '\u{2019}'),
    ("quotesinglbase", '\u{201A}'),
    ("quotedblleft", '\u{201C}'),
    ("quotedblright", '\u{201D}'),
    ("quotedblbase", '\u{201E}'),
    ("endash", '\u{2013}'),
    ("emdash", '\u{2014}'),
    ("bullet", '\u{2022}'),
    ("ellipsis", '\u{2026}'),
    ("dagger", '\u{2020}'),
    ("daggerdbl", '\u{2021}'),
    ("perthousand", '\u{2030}'),
    ("guilsinglleft", '\u{2039}'),
    ("guilsinglright", '\u{203A}'),
    ("fi", '\u{FB01}'),
    ("fl", '\u{FB02}'),
    ("ff", '\u{FB00}'),
    ("ffi", '\u{FB03}'),
    ("ffl", '\u{FB04}'),
    ("Euro", '\u{20AC}'),
    ("trademark", '\u{2122}'),
    ("florin", '\u{192}'),
    ("circumflex", '\u{2C6}'),
    ("tilde", '\u{2DC}'),
    ("caron", '\u{2C7}'),
    ("breve", '\u{2D8}'),
    ("dotaccent", '\u{2D9}'),
    ("ring", '\u{2DA}'),
    ("ogonek", '\u{2DB}'),
    ("hungarumlaut", '\u{2DD}'),
    ("Scaron", 'Š'),
    ("scaron", 'š'),
    ("Zcaron", 'Ž'),
    ("zcaron", 'ž'),
    ("OE", 'Œ'),
    ("oe", 'œ'),
    ("Ydieresis", 'Ÿ'),
    ("Lslash", 'Ł'),
    ("lslash", 'ł'),
    ("dotlessi", 'ı'),
    ("fraction", '\u{2044}'),
    ("minus", '\u{2212}'),
    ("Delta", '\u{2206}'),
    ("Omega", '\u{2126}'),
    ("pi", 'π'),
    ("summation", '\u{2211}'),
    ("product", '\u{220F}'),
    ("radical", '\u{221A}'),
    ("infinity", '\u{221E}'),
    ("notequal", '\u{2260}'),
    ("lessequal", '\u{2264}'),
    ("greaterequal", '\u{2265}'),
    ("approxequal", '\u{2248}'),
    ("partialdiff", '\u{2202}'),
    ("integral", '\u{222B}'),
    ("lozenge", '\u{25CA}'),
    ("apple", '\u{F8FF}'),
    ("nbspace", '\u{A0}'),
    ("sfthyphen", '\u{AD}'),
    ("Amacron", 'Ā'),
    ("amacron", 'ā'),
    ("Abreve", 'Ă'),
    ("abreve", 'ă'),
    ("Aogonek", 'Ą'),
    ("aogonek", 'ą'),
    ("Cacute", 'Ć'),
    ("cacute", 'ć'),
    ("Ccaron", 'Č'),
    ("ccaron", 'č'),
    ("Dcaron", 'Ď'),
    ("dcaron", 'ď'),
    ("Dcroat", 'Đ'),
    ("dcroat", 'đ'),
    ("Emacron", 'Ē'),
    ("emacron", 'ē'),
    ("Eogonek", 'Ę'),
    ("eogonek", 'ę'),
    ("Ecaron", 'Ě'),
    ("ecaron", 'ě'),
    ("Gbreve", 'Ğ'),
    ("gbreve", 'ğ'),
    ("Idotaccent", 'İ'),
    ("Lacute", 'Ĺ'),
    ("lacute", 'ĺ'),
    ("Lcaron", 'Ľ'),
    ("lcaron", 'ľ'),
    ("Nacute", 'Ń'),
    ("nacute", 'ń'),
    ("Ncaron", 'Ň'),
    ("ncaron", 'ň'),
    ("Ohungarumlaut", 'Ő'),
    ("ohungarumlaut", 'ő'),
    ("Racute", 'Ŕ'),
    ("racute", 'ŕ'),
    ("Rcaron", 'Ř'),
    ("rcaron", 'ř'),
    ("Sacute", 'Ś'),
    ("sacute", 'ś'),
    ("Scedilla", 'Ş'),
    ("scedilla", 'ş'),
    ("Tcaron", 'Ť'),
    ("tcaron", 'ť'),
    ("Uring", 'Ů'),
    ("uring", 'ů'),
    ("Uhungarumlaut", 'Ű'),
    ("uhungarumlaut", 'ű'),
    ("Zacute", 'Ź'),
    ("zacute", 'ź'),
    ("Zdotaccent", 'Ż'),
    ("zdotaccent", 'ż'),
    ("afii10017", 'А'),
    ("afii10018", 'Б'),
    ("afii10019", 'В'),
    ("afii10020", 'Г'),
    ("afii10021", 'Д'),
    ("afii10022", 'Е'),
    ("afii10023", 'Ё'),
    ("afii10024", 'Ж'),
    ("afii10025", 'З'),
    ("afii10026", 'И'),
    ("afii10027", 'Й'),
    ("afii10028", 'К'),
    ("afii10029", 'Л'),
    ("afii10030", 'М'),
    ("afii10031", 'Н'),
    ("afii10032", 'О'),
    ("afii10033", 'П'),
    ("afii10034", 'Р'),
    ("afii10035", 'С'),
    ("afii10036", 'Т'),
    ("afii10037", 'У'),
    ("afii10038", 'Ф'),
    ("afii10039", 'Х'),
    ("afii10040", 'Ц'),
    ("afii10041", 'Ч'),
    ("afii10042", 'Ш'),
    ("afii10043", 'Щ'),
    ("afii10044", 'Ъ'),
    ("afii10045", 'Ы'),
    ("afii10046", 'Ь'),
    ("afii10047", 'Э'),
    ("afii10048", 'Ю'),
    ("afii10049", 'Я'),
    ("afii10065", 'а'),
    ("afii10066", 'б'),
    ("afii10067", 'в'),
    ("afii10068", 'г'),
    ("afii10069", 'д'),
    ("afii10070", 'е'),
    ("afii10071", 'ё'),
    ("afii10072", 'ж'),
    ("afii10073", 'з'),
    ("afii10074", 'и'),
    ("afii10075", 'й'),
    ("afii10076", 'к'),
    ("afii10077", 'л'),
    ("afii10078", 'м'),
    ("afii10079", 'н'),
    ("afii10080", 'о'),
    ("afii10081", 'п'),
    ("afii10082", 'р'),
    ("afii10083", 'с'),
    ("afii10084", 'т'),
    ("afii10085", 'у'),
    ("afii10086", 'ф'),
    ("afii10087", 'х'),
    ("afii10088", 'ц'),
    ("afii10089", 'ч'),
    ("afii10090", 'ш'),
    ("afii10091", 'щ'),
    ("afii10092", 'ъ'),
    ("afii10093", 'ы'),
    ("afii10094", 'ь'),
    ("afii10095", 'э'),
    ("afii10096", 'ю'),
    ("afii10097", 'я'),
    ("afii00208", '\u{2015}'),
    ("afii61352", '\u{2116}'),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unicode_cmaps_are_the_characters_themselves() {
        let ucs2 = CMap::predefined("UniGB-UCS2-H");
        assert_eq!(ucs2.next_code(&[0x4E, 0x2D, 0x00, 0x41]), (0x4E2D, 2));
        assert_eq!(ucs2.text(0x4E2D).as_deref(), Some("\u{4E2D}"));
        // Its Latin from id 1, the space first.
        assert_eq!(ucs2.cid(0x0041), 34);
        assert!(!ucs2.vertical);
        let utf16 = CMap::predefined("UniJIS-UTF16-V");
        assert!(utf16.vertical);
        let face = [0xD8, 0x3D, 0xDE, 0x00];
        assert_eq!(utf16.next_code(&face), (0xD83D_DE00, 4));
        assert_eq!(utf16.text(0xD83D_DE00).as_deref(), Some("\u{1F600}"));
        assert_eq!(CMap::predefined("UniJIS-UCS2-HW-H").cid(0x0020), 231);
        let utf8 = CMap::predefined("UniKS-UTF8-H");
        assert_eq!(utf8.next_code("\u{E9}x".as_bytes()), (0xC3A9, 2));
        assert_eq!(utf8.text(0xC3A9).as_deref(), Some("\u{E9}"));
        assert_eq!(utf8.next_code(b"x"), (0x78, 1));
        assert_eq!(CMap::predefined("UniCNS-UTF32-H").text(0x1F600).as_deref(), Some("\u{1F600}"));
    }

    #[test]
    fn a_tounicode_map_gives_text_back() {
        let map = CMap::parse(
            b"/CIDInit /ProcSet findresource begin begincmap\n1 begincodespacerange <0000> <FFFF> endcodespacerange\n2 beginbfchar <0003> <0020> <0024> <0041> endbfchar\n1 beginbfrange <0044> <0046> <0061> endbfrange\n1 beginbfrange <0050> <0051> [<00660069> <0066006C>] endbfrange\nendcmap",
        );
        assert_eq!(map.next_code(&[0x00, 0x24]), (0x24, 2));
        assert_eq!(map.text(0x24).as_deref(), Some("A"));
        assert_eq!(map.text(0x45).as_deref(), Some("b"));
        assert_eq!(map.text(0x51).as_deref(), Some("fl"));
        assert_eq!(map.text(0x99), None);
    }

    #[test]
    fn a_cid_map_takes_mixed_byte_lengths() {
        let map = CMap::parse(
            b"2 begincodespacerange <00> <80> <8140> <9ffc> endcodespacerange\n1 begincidrange <20> <7e> 1 endcidrange\n1 begincidchar <8140> 633 endcidchar",
        );
        assert_eq!(map.next_code(&[0x41, 0x81, 0x40]), (0x41, 1));
        assert_eq!(map.cid(0x41), 34);
        assert_eq!(map.next_code(&[0x81, 0x40]), (0x8140, 2));
        assert_eq!(map.cid(0x8140), 633);
    }

    #[test]
    fn the_encodings_give_their_characters() {
        assert_eq!(BaseEncoding::WinAnsi.char_of(0x93), Some('\u{201C}'));
        assert_eq!(BaseEncoding::MacRoman.char_of(0xD2), Some('\u{201C}'));
        assert_eq!(BaseEncoding::Standard.char_of(0xAA), Some('\u{201C}'));
        assert_eq!(BaseEncoding::Standard.char_of(0x27), Some('\u{2019}'));
        assert_eq!(BaseEncoding::Symbol.char_of(0xB7), Some('\u{2022}'));
        assert_eq!(BaseEncoding::Symbol.char_of(0x61), Some('α'));
    }

    #[test]
    fn glyph_names_are_characters() {
        assert_eq!(glyph_char("eacute"), Some('é'));
        assert_eq!(glyph_char("uni2022"), Some('\u{2022}'));
        assert_eq!(glyph_char("u1F600"), Some('\u{1F600}'));
        assert_eq!(glyph_char("fi.alt"), Some('\u{FB01}'));
        assert_eq!(glyph_char("g123"), None);
    }
}

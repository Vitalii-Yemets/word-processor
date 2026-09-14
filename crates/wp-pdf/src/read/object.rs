//! The objects a PDF is made of, and reading them out of bytes.
//!
//! Eight kinds: null, booleans, numbers, strings, names, arrays,
//! dictionaries and streams — and a reference, `12 0 R`, standing for an
//! object kept elsewhere in the file. A content stream is the same syntax
//! with operators between the operands, so the one lexer reads both.

use std::collections::BTreeMap;

/// A dictionary: names to objects, in name order.
pub type Dictionary = BTreeMap<String, Object>;

/// A stream: a dictionary and the bytes it describes, as they are in the
/// file — decoded elsewhere, because decoding may need other objects.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    pub dictionary: Dictionary,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Object {
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u8>),
    Name(String),
    Array(Vec<Object>),
    Dictionary(Dictionary),
    Stream(Box<Stream>),
    Reference(u32, u16),
    /// A keyword that is not an object: an operator in a content stream, or
    /// `obj`, `endobj`, `stream` between objects.
    Operator(String),
}

impl Object {
    #[must_use]
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_integer(&self) -> Option<i64> {
        self.as_number().filter(|n| n.is_finite()).map(|n| n as i64)
    }

    #[must_use]
    pub fn as_name(&self) -> Option<&str> {
        match self {
            Self::Name(name) => Some(name),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_string(&self) -> Option<&[u8]> {
        match self {
            Self::String(bytes) => Some(bytes),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_array(&self) -> Option<&[Object]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The dictionary of a dictionary or of a stream.
    #[must_use]
    pub fn as_dictionary(&self) -> Option<&Dictionary> {
        match self {
            Self::Dictionary(dictionary) => Some(dictionary),
            Self::Stream(stream) => Some(&stream.dictionary),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_stream(&self) -> Option<&Stream> {
        match self {
            Self::Stream(stream) => Some(stream),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_reference(&self) -> Option<(u32, u16)> {
        match self {
            Self::Reference(number, generation) => Some((*number, *generation)),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

/// Whether a byte is one of the six the format counts as white space.
#[must_use]
pub fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b'\0' | b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

/// Whether a byte ends a token: white space or one of the delimiters.
#[must_use]
pub fn is_delimiter(byte: u8) -> bool {
    matches!(byte, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

fn is_regular(byte: u8) -> bool {
    !is_whitespace(byte) && !is_delimiter(byte)
}

/// Reads objects out of bytes, one after another.
pub struct Lexer<'a> {
    pub bytes: &'a [u8],
    pub at: usize,
}

impl<'a> Lexer<'a> {
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    #[must_use]
    pub fn from(bytes: &'a [u8], at: usize) -> Self {
        Self { bytes, at: at.min(bytes.len()) }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    /// Steps over white space and comments.
    pub fn skip_whitespace(&mut self) {
        while let Some(byte) = self.peek() {
            if is_whitespace(byte) {
                self.at += 1;
            } else if byte == b'%' {
                while let Some(byte) = self.peek() {
                    if byte == b'\n' || byte == b'\r' {
                        break;
                    }
                    self.at += 1;
                }
            } else {
                break;
            }
        }
    }

    /// The next object, or nothing at the end. A keyword comes back as an
    /// operator; a reference `n g R` as one object.
    pub fn next_object(&mut self) -> Option<Object> {
        self.skip_whitespace();
        let byte = self.peek()?;
        match byte {
            b'/' => {
                self.at += 1;
                Some(Object::Name(self.name()))
            }
            b'(' => {
                self.at += 1;
                Some(Object::String(self.literal_string()))
            }
            b'<' => {
                if self.bytes.get(self.at + 1) == Some(&b'<') {
                    self.at += 2;
                    Some(self.dictionary_or_stream())
                } else {
                    self.at += 1;
                    Some(Object::String(self.hex_string()))
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_whitespace();
                    match self.peek() {
                        None => break,
                        Some(b']') => {
                            self.at += 1;
                            break;
                        }
                        Some(_) => match self.next_object() {
                            Some(Object::Operator(word))
                                if word == "endobj" || word == "endstream" =>
                            {
                                break
                            }
                            Some(Object::Operator(_)) => {}
                            Some(item) => items.push(item),
                            None => break,
                        },
                    }
                }
                Some(Object::Array(items))
            }
            b']' | b'>' | b')' | b'{' | b'}' => {
                // Stray delimiters are stepped over rather than stopping
                // the reading: a damaged file still has text in it.
                self.at += 1;
                self.next_object()
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => Some(self.number_or_reference()),
            _ => {
                let start = self.at;
                while let Some(byte) = self.peek() {
                    if !is_regular(byte) {
                        break;
                    }
                    self.at += 1;
                }
                if self.at == start {
                    self.at += 1;
                    return self.next_object();
                }
                let word = String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned();
                Some(match word.as_str() {
                    "true" => Object::Bool(true),
                    "false" => Object::Bool(false),
                    "null" => Object::Null,
                    _ => Object::Operator(word),
                })
            }
        }
    }

    fn name(&mut self) -> String {
        let mut name = Vec::new();
        while let Some(byte) = self.peek() {
            if !is_regular(byte) {
                break;
            }
            self.at += 1;
            if byte == b'#' {
                let hex = |b: Option<&u8>| b.and_then(|b| (*b as char).to_digit(16));
                if let (Some(high), Some(low)) =
                    (hex(self.bytes.get(self.at)), hex(self.bytes.get(self.at + 1)))
                {
                    name.push((high * 16 + low) as u8);
                    self.at += 2;
                    continue;
                }
            }
            name.push(byte);
        }
        String::from_utf8_lossy(&name).into_owned()
    }

    fn literal_string(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut depth = 1;
        while let Some(byte) = self.peek() {
            self.at += 1;
            match byte {
                b'\\' => {
                    let Some(next) = self.peek() else { break };
                    self.at += 1;
                    match next {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'\n' => {}
                        b'\r' => {
                            if self.peek() == Some(b'\n') {
                                self.at += 1;
                            }
                        }
                        b'0'..=b'7' => {
                            let mut value = u32::from(next - b'0');
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(digit @ b'0'..=b'7') => {
                                        value = value * 8 + u32::from(digit - b'0');
                                        self.at += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push((value & 0xFF) as u8);
                        }
                        other => out.push(other),
                    }
                }
                b'(' => {
                    depth += 1;
                    out.push(byte);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(byte);
                }
                _ => out.push(byte),
            }
        }
        out
    }

    fn hex_string(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut high: Option<u8> = None;
        while let Some(byte) = self.peek() {
            self.at += 1;
            if byte == b'>' {
                break;
            }
            let Some(digit) = (byte as char).to_digit(16) else { continue };
            match high.take() {
                Some(high) => out.push(high * 16 + digit as u8),
                None => high = Some(digit as u8),
            }
        }
        if let Some(high) = high {
            out.push(high * 16);
        }
        out
    }

    fn dictionary_or_stream(&mut self) -> Object {
        let mut dictionary = Dictionary::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                None => break,
                Some(b'>') => {
                    self.at += 1;
                    if self.peek() == Some(b'>') {
                        self.at += 1;
                    }
                    break;
                }
                Some(_) => {}
            }
            let Some(key) = self.next_object() else { break };
            let Object::Name(key) = key else {
                if matches!(&key, Object::Operator(word) if word == "endobj" || word == "stream") {
                    // A dictionary cut short: give back what it had.
                    self.at -= word_length(&key);
                    break;
                }
                continue;
            };
            let Some(value) = self.next_object() else { break };
            if let Object::Operator(word) = &value {
                if word == "endobj" || word == "stream" {
                    self.at -= word_length(&value);
                    break;
                }
                continue;
            }
            dictionary.insert(key, value);
        }
        // A stream follows its dictionary directly.
        let saved = self.at;
        self.skip_whitespace();
        if self.bytes[self.at..].starts_with(b"stream") {
            self.at += 6;
            if self.peek() == Some(b'\r') {
                self.at += 1;
            }
            if self.peek() == Some(b'\n') {
                self.at += 1;
            }
            let data = self.stream_data(&dictionary);
            return Object::Stream(Box::new(Stream { dictionary, data }));
        }
        self.at = saved;
        Object::Dictionary(dictionary)
    }

    /// The bytes of a stream: as many as `/Length` says when that is a
    /// number and `endstream` really follows them, or else up to the
    /// `endstream` found by looking — a length given by reference or given
    /// wrongly is common enough.
    fn stream_data(&mut self, dictionary: &Dictionary) -> Vec<u8> {
        let start = self.at;
        if let Some(length) = dictionary.get("Length").and_then(Object::as_integer) {
            if let Ok(length) = usize::try_from(length) {
                let end = start + length;
                if end <= self.bytes.len() {
                    let mut probe = Lexer::from(self.bytes, end);
                    probe.skip_whitespace();
                    if probe.bytes[probe.at..].starts_with(b"endstream") {
                        self.at = probe.at + 9;
                        return self.bytes[start..end].to_vec();
                    }
                }
            }
        }
        let rest = &self.bytes[start..];
        let mut end = find(rest, b"endstream").unwrap_or(rest.len());
        self.at = start + end + 9.min(rest.len() - end);
        // The line end before the keyword is not part of the data.
        if end > 0 && rest[end - 1] == b'\n' {
            end -= 1;
        }
        if end > 0 && rest[end - 1] == b'\r' {
            end -= 1;
        }
        rest[..end].to_vec()
    }

    fn number_or_reference(&mut self) -> Object {
        let start = self.at;
        let number = self.number();
        // Two whole numbers and an R are one reference.
        if number >= 0.0 && number.fract() == 0.0 && !self.bytes[start..self.at].contains(&b'.') {
            let saved = self.at;
            self.skip_whitespace();
            let generation_start = self.at;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
            if self.at > generation_start {
                let generation: u16 = std::str::from_utf8(&self.bytes[generation_start..self.at])
                    .ok()
                    .and_then(|text| text.parse().ok())
                    .unwrap_or(0);
                self.skip_whitespace();
                if self.peek() == Some(b'R')
                    && self.bytes.get(self.at + 1).is_none_or(|b| !is_regular(*b))
                {
                    self.at += 1;
                    return Object::Reference(number as u32, generation);
                }
            }
            self.at = saved;
        }
        Object::Number(number)
    }

    fn number(&mut self) -> f64 {
        let start = self.at;
        while let Some(byte) = self.peek() {
            if !matches!(byte, b'+' | b'-' | b'.' | b'0'..=b'9' | b'e' | b'E') {
                break;
            }
            self.at += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or("0");
        // A number like `--5` or `3.4.5` is written by some producers; take
        // what makes sense of it.
        let cleaned: String = {
            let mut seen_point = false;
            let mut out = String::new();
            for (index, c) in text.chars().enumerate() {
                match c {
                    '-' if out.is_empty() => out.push('-'),
                    '-' | '+' => {
                        if index > 0 && out.chars().all(|c| c == '-') {
                            continue;
                        }
                        break;
                    }
                    '.' if !seen_point => {
                        seen_point = true;
                        out.push('.');
                    }
                    '.' => break,
                    'e' | 'E' => break,
                    digit => out.push(digit),
                }
            }
            out
        };
        if cleaned == "-" || cleaned == "." || cleaned == "-." || cleaned.is_empty() {
            return 0.0;
        }
        cleaned.parse().unwrap_or(0.0)
    }

    /// Reads `n g obj` at the current place and returns the numbers, or
    /// nothing if that is not what is there.
    pub fn object_header(&mut self) -> Option<(u32, u16)> {
        self.skip_whitespace();
        let number = self.integer()?;
        self.skip_whitespace();
        let generation = self.integer()?;
        self.skip_whitespace();
        if !self.bytes[self.at..].starts_with(b"obj") {
            return None;
        }
        self.at += 3;
        Some((u32::try_from(number).ok()?, u16::try_from(generation).ok()?))
    }

    fn integer(&mut self) -> Option<i64> {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.at]).ok()?.parse().ok()
    }
}

fn word_length(object: &Object) -> usize {
    match object {
        Object::Operator(word) => word.len(),
        _ => 0,
    }
}

/// Where `needle` first occurs in `haystack`.
#[must_use]
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Where `needle` last occurs in `haystack`.
#[must_use]
pub fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|window| window == needle)
}

/// A string as text: UTF-16 with a byte order mark, UTF-8 with one, or
/// else the format's own encoding, which is Latin-1 with a few of its own
/// in the upper half.
#[must_use]
pub fn text_of(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> =
            rest.chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> =
            rest.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    bytes.iter().map(|&byte| pdf_doc_char(byte)).collect()
}

/// PDFDocEncoding: Latin-1, with the quotes, dashes, bullet and ligatures
/// in the range Latin-1 leaves for control codes.
fn pdf_doc_char(byte: u8) -> char {
    match byte {
        0x80 => '\u{2022}',
        0x81 => '\u{2020}',
        0x82 => '\u{2021}',
        0x83 => '\u{2026}',
        0x84 => '\u{2014}',
        0x85 => '\u{2013}',
        0x86 => '\u{0192}',
        0x87 => '\u{2044}',
        0x88 => '\u{2039}',
        0x89 => '\u{203A}',
        0x8A => '\u{2212}',
        0x8B => '\u{2030}',
        0x8C => '\u{201E}',
        0x8D => '\u{201C}',
        0x8E => '\u{201D}',
        0x8F => '\u{2018}',
        0x90 => '\u{2019}',
        0x91 => '\u{201A}',
        0x92 => '\u{2122}',
        0x93 => '\u{FB01}',
        0x94 => '\u{FB02}',
        0x95 => '\u{0141}',
        0x96 => '\u{0152}',
        0x97 => '\u{0160}',
        0x98 => '\u{0178}',
        0x99 => '\u{017D}',
        0x9A => '\u{0131}',
        0x9B => '\u{0142}',
        0x9C => '\u{0153}',
        0x9D => '\u{0161}',
        0x9E => '\u{017E}',
        0xA0 => '\u{20AC}',
        other => char::from(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_of_object_reads() {
        let mut lexer = Lexer::new(
            b"<< /Type /Page /Kids [1 0 R 2 0 R] /Count 2 /N#61me (a\\)b) /Hex <414> /F 1.5 /T true /Z null >>",
        );
        let Some(Object::Dictionary(dictionary)) = lexer.next_object() else { panic!() };
        assert_eq!(dictionary.get("Type"), Some(&Object::Name("Page".into())));
        assert_eq!(
            dictionary.get("Kids"),
            Some(&Object::Array(vec![Object::Reference(1, 0), Object::Reference(2, 0)]))
        );
        assert_eq!(dictionary.get("Count"), Some(&Object::Number(2.0)));
        assert_eq!(dictionary.get("Name"), Some(&Object::String(b"a)b".to_vec())));
        assert_eq!(dictionary.get("Hex"), Some(&Object::String(vec![0x41, 0x40])));
        assert_eq!(dictionary.get("F"), Some(&Object::Number(1.5)));
        assert_eq!(dictionary.get("T"), Some(&Object::Bool(true)));
        assert_eq!(dictionary.get("Z"), Some(&Object::Null));
    }

    #[test]
    fn a_stream_takes_its_length_or_finds_its_end() {
        let mut lexer = Lexer::new(b"<< /Length 5 >>\nstream\nhello\nendstream");
        let Some(Object::Stream(stream)) = lexer.next_object() else { panic!() };
        assert_eq!(stream.data, b"hello");
        let mut lexer = Lexer::new(b"<< /Length 9 0 R >>\r\nstream\r\nhello\r\nendstream");
        let Some(Object::Stream(stream)) = lexer.next_object() else { panic!() };
        assert_eq!(stream.data, b"hello");
    }

    #[test]
    fn operators_come_between_operands() {
        let mut lexer = Lexer::new(b"BT /F1 12 Tf 72 700 Td (Hi) Tj ET");
        let mut words = Vec::new();
        while let Some(object) = lexer.next_object() {
            if let Object::Operator(word) = object {
                words.push(word);
            }
        }
        assert_eq!(words, ["BT", "Tf", "Td", "Tj", "ET"]);
    }

    #[test]
    fn text_strings_come_in_three_encodings() {
        assert_eq!(text_of(&[0xFE, 0xFF, 0x00, 0x41, 0x04, 0x16]), "A\u{416}");
        assert_eq!(text_of(b"\xEF\xBB\xBFcaf\xC3\xA9"), "caf\u{e9}");
        assert_eq!(text_of(b"caf\xE9 \x80 \x84"), "caf\u{e9} \u{2022} \u{2014}");
    }
}

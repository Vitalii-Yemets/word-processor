//! Reading JSON, which is what sway answers in.
//!
//! Only reading, and only as much as the answers need: objects, arrays,
//! strings with their escapes, numbers, and the three words. An object's
//! members are kept in the order they came, which is all a reader of one
//! answer needs and is cheaper than a map.

/// A value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// How deep values may be nested: an answer deeper than this is not one
/// sway would give, and reading it would only risk the stack.
const DEEPEST: usize = 256;

impl Json {
    /// A whole text as one value, or nothing if it is not JSON.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut reader = Reader { bytes: text.as_bytes(), at: 0 };
        let value = reader.value(0)?;
        reader.space();
        (reader.at == reader.bytes.len()).then_some(value)
    }

    /// An object's member.
    pub(crate) fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(members) => {
                members.iter().find(|(name, _)| name == key).map(|(_, value)| value)
            }
            _ => None,
        }
    }

    /// An array's elements, or none.
    pub(crate) fn elements(&self) -> &[Self] {
        match self {
            Self::Array(elements) => elements,
            _ => &[],
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            _ => None,
        }
    }

    /// A number that is a whole one.
    pub(crate) fn as_i64(&self) -> Option<i64> {
        match *self {
            Self::Number(number) if number.fract() == 0.0 && number.abs() < 9.0e15 => {
                Some(number as i64)
            }
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match *self {
            Self::Bool(value) => Some(value),
            _ => None,
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while self
            .bytes
            .get(self.at)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Option<()> {
        self.space();
        (self.bytes.get(self.at) == Some(&byte)).then(|| self.at += 1)
    }

    fn word(&mut self, word: &str, value: Json) -> Option<Json> {
        self.bytes[self.at..].starts_with(word.as_bytes()).then(|| {
            self.at += word.len();
            value
        })
    }

    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > DEEPEST {
            return None;
        }
        self.space();
        match *self.bytes.get(self.at)? {
            b'{' => {
                self.at += 1;
                let mut members = Vec::new();
                if self.eat(b'}').is_some() {
                    return Some(Json::Object(members));
                }
                loop {
                    self.space();
                    let name = self.string()?;
                    self.eat(b':')?;
                    members.push((name, self.value(depth + 1)?));
                    if self.eat(b',').is_none() {
                        self.eat(b'}')?;
                        return Some(Json::Object(members));
                    }
                }
            }
            b'[' => {
                self.at += 1;
                let mut elements = Vec::new();
                if self.eat(b']').is_some() {
                    return Some(Json::Array(elements));
                }
                loop {
                    elements.push(self.value(depth + 1)?);
                    if self.eat(b',').is_none() {
                        self.eat(b']')?;
                        return Some(Json::Array(elements));
                    }
                }
            }
            b'"' => self.string().map(Json::Str),
            b't' => self.word("true", Json::Bool(true)),
            b'f' => self.word("false", Json::Bool(false)),
            b'n' => self.word("null", Json::Null),
            _ => self.number(),
        }
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E')
        }) {
            self.at += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).ok()?;
        text.parse::<f64>().ok().map(Json::Number)
    }

    /// A string, its escapes undone — a pair of escaped halves of a
    /// character outside the first plane made one character again.
    fn string(&mut self) -> Option<String> {
        if self.bytes.get(self.at) != Some(&b'"') {
            return None;
        }
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let byte = *self.bytes.get(self.at)?;
            self.at += 1;
            match byte {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let escaped = *self.bytes.get(self.at)?;
                    self.at += 1;
                    let character = match escaped {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let high = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&high) {
                                self.eat_exact(b"\\u")?;
                                let low = self.hex4()?;
                                0x10000 + ((high - 0xD800) << 10) + (low.checked_sub(0xDC00)?)
                            } else {
                                high
                            };
                            char::from_u32(code)?
                        }
                        _ => return None,
                    };
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                }
                other => out.push(other),
            }
        }
    }

    fn eat_exact(&mut self, expected: &[u8]) -> Option<()> {
        self.bytes[self.at..].starts_with(expected).then(|| self.at += expected.len())
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.bytes.get(self.at..self.at + 4)?;
        self.at += 4;
        u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_of_sway_s_is_read() {
        let text = r#"{"id": 4, "type": "con", "name": "Doc \u00e9 \ud83d\ude00", "pid": 1234,
            "focused": false, "rect": {"x": 0, "y": -1.5e1}, "nodes": [], "marks": [null]}"#;
        let value = Json::parse(text).expect("JSON");
        assert_eq!(value.get("id").and_then(Json::as_i64), Some(4));
        assert_eq!(value.get("name").and_then(Json::as_str), Some("Doc é 😀"));
        assert_eq!(value.get("focused").and_then(Json::as_bool), Some(false));
        assert_eq!(value.get("rect").and_then(|rect| rect.get("y")), Some(&Json::Number(-15.0)));
        assert!(value.get("nodes").is_some_and(|nodes| nodes.elements().is_empty()));
        assert_eq!(value.get("marks").map(Json::elements), Some(&[Json::Null][..]));
    }

    #[test]
    fn what_is_not_json_is_not_read() {
        for text in ["", "{", "[1,]", "{\"a\" 1}", "\"open", "tru", "[1] x", "{\"a\":\"\\q\"}"] {
            assert_eq!(Json::parse(text), None, "{text}");
        }
        let deep = format!("{}{}", "[".repeat(DEEPEST + 2), "]".repeat(DEEPEST + 2));
        assert_eq!(Json::parse(&deep), None, "too deep");
    }
}

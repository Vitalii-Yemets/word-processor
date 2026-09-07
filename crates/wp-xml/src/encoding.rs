//! Working out how a document's bytes encode its characters.
//!
//! The parser itself works on `&str`, so this is where raw bytes become text.
//! Every part of a `.docx` is UTF-8 in practice, but a file can arrive from
//! anywhere and the declaration has to be believed, so the encodings XML
//! requires every parser to accept are handled too.

use std::borrow::Cow;

use crate::{Error, ErrorKind, Position};

/// A character encoding this parser can decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// What office documents use, and the XML default when nothing says otherwise.
    Utf8,
    Utf16Le,
    Utf16Be,
    /// A strict subset of UTF-8, decoded the same way.
    Ascii,
    /// Latin-1. Every byte maps to the code point of the same value.
    Latin1,
}

/// Start of the input, used when an error is about the file as a whole.
const START: Position = Position { offset: 0, line: 1, column: 1 };

fn error(kind: ErrorKind) -> Error {
    Error { kind, position: START }
}

/// Turns a document's bytes into text, honouring the byte order mark and the
/// XML declaration.
///
/// Returns borrowed text for the common case of UTF-8 without a mark, so reading
/// a document does not copy every part twice.
pub fn decode_to_utf8(bytes: &[u8]) -> Result<Cow<'_, str>, Error> {
    let (encoding, body) = detect(bytes)?;

    match encoding {
        Encoding::Utf8 | Encoding::Ascii => match core::str::from_utf8(body) {
            Ok(text) => Ok(Cow::Borrowed(text)),
            Err(_) => Err(error(ErrorKind::MalformedEncoding(encoding))),
        },
        Encoding::Utf16Le => decode_utf16(body, u16::from_le_bytes).map(Cow::Owned),
        Encoding::Utf16Be => decode_utf16(body, u16::from_be_bytes).map(Cow::Owned),
        // Latin-1 has no invalid byte: every value is a code point below U+0100.
        Encoding::Latin1 => Ok(Cow::Owned(body.iter().map(|&byte| byte as char).collect())),
    }
}

/// Decides the encoding and returns the bytes that follow any byte order mark.
fn detect(bytes: &[u8]) -> Result<(Encoding, &[u8]), Error> {
    // A byte order mark outranks the declaration; the specification says so, and
    // it has to, since the declaration cannot be read without knowing the width
    // of a character first.
    if let Some(body) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Ok((Encoding::Utf8, body));
    }
    if let Some(body) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok((Encoding::Utf16Le, body));
    }
    if let Some(body) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Ok((Encoding::Utf16Be, body));
    }

    // With no mark, a UTF-16 file still gives itself away: the declaration
    // begins with ASCII, so every other byte is zero.
    if bytes.starts_with(&[b'<', 0x00, b'?', 0x00]) {
        return Ok((Encoding::Utf16Le, bytes));
    }
    if bytes.starts_with(&[0x00, b'<', 0x00, b'?']) {
        return Ok((Encoding::Utf16Be, bytes));
    }

    match declared_encoding(bytes) {
        Some(name) => Ok((named_encoding(&name)?, bytes)),
        None => Ok((Encoding::Utf8, bytes)),
    }
}

/// Reads the `encoding` attribute out of the XML declaration.
///
/// The declaration is ASCII by definition, so it can be scanned as bytes before
/// the encoding is known.
fn declared_encoding(bytes: &[u8]) -> Option<String> {
    const MAX_DECLARATION: usize = 256;

    let head = &bytes[..bytes.len().min(MAX_DECLARATION)];
    if !head.starts_with(b"<?xml") {
        return None;
    }
    let end = head.windows(2).position(|pair| pair == b"?>")?;
    let declaration = core::str::from_utf8(&head[..end]).ok()?;

    let after_keyword = declaration.split_once("encoding")?.1;
    let after_equals = after_keyword.trim_start().strip_prefix('=')?.trim_start();
    let quote = after_equals.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &after_equals[quote.len_utf8()..];
    let close = value.find(quote)?;
    Some(value[..close].to_owned())
}

/// Maps an encoding name to what we can decode. Names are case-insensitive.
fn named_encoding(name: &str) -> Result<Encoding, Error> {
    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "utf-8" | "utf8" => Ok(Encoding::Utf8),
        "utf-16" | "utf16" | "utf-16le" => Ok(Encoding::Utf16Le),
        "utf-16be" => Ok(Encoding::Utf16Be),
        "us-ascii" | "ascii" => Ok(Encoding::Ascii),
        "iso-8859-1" | "latin1" | "iso8859-1" => Ok(Encoding::Latin1),
        _ => Err(error(ErrorKind::UnsupportedEncoding(name.to_owned()))),
    }
}

/// Decodes UTF-16, joining surrogate pairs.
fn decode_utf16(bytes: &[u8], read_unit: fn([u8; 2]) -> u16) -> Result<String, Error> {
    if bytes.len() % 2 != 0 {
        return Err(error(ErrorKind::MalformedEncoding(Encoding::Utf16Le)));
    }

    let units: Vec<u16> =
        bytes.chunks_exact(2).map(|pair| read_unit([pair[0], pair[1]])).collect();

    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|_| error(ErrorKind::MalformedEncoding(Encoding::Utf16Le)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes text as UTF-16 for the round-trip tests.
    fn to_utf16(text: &str, little_endian: bool, with_mark: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if with_mark {
            out.extend_from_slice(if little_endian { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
        }
        for unit in text.encode_utf16() {
            out.extend_from_slice(&if little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        out
    }

    #[test]
    fn plain_utf8_is_borrowed_not_copied() {
        let bytes = b"<w:p>text</w:p>";
        assert!(matches!(decode_to_utf8(bytes).unwrap(), Cow::Borrowed(_)));
    }

    #[test]
    fn strips_the_utf8_byte_order_mark() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"<w:p/>");
        assert_eq!(decode_to_utf8(&bytes).unwrap(), "<w:p/>");
    }

    #[test]
    fn decodes_utf16_in_both_byte_orders() {
        let text = "<w:t>Многоязычный 文書 🖋</w:t>";
        for little_endian in [true, false] {
            for with_mark in [true, false] {
                // Without a mark the declaration is what reveals the width, so
                // the sample needs one in that case.
                let source = if with_mark {
                    text.to_owned()
                } else {
                    format!("<?xml version=\"1.0\"?>{text}")
                };
                let bytes = to_utf16(&source, little_endian, with_mark);
                assert_eq!(
                    decode_to_utf8(&bytes).unwrap(),
                    source,
                    "little_endian={little_endian} with_mark={with_mark}"
                );
            }
        }
    }

    #[test]
    fn honours_the_declared_encoding() {
        let bytes = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><t>caf\xE9</t>";
        assert_eq!(decode_to_utf8(bytes).unwrap(), "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><t>café</t>");
    }

    #[test]
    fn encoding_names_are_case_insensitive() {
        for name in ["UTF-8", "utf-8", "Utf-8", "UTF8"] {
            let source = format!("<?xml version=\"1.0\" encoding=\"{name}\"?><t/>");
            assert!(decode_to_utf8(source.as_bytes()).is_ok(), "{name} should be accepted");
        }
    }

    #[test]
    fn reports_an_encoding_it_cannot_handle() {
        let bytes = b"<?xml version=\"1.0\" encoding=\"Shift_JIS\"?><t/>";
        assert!(matches!(
            decode_to_utf8(bytes).unwrap_err().kind,
            ErrorKind::UnsupportedEncoding(name) if name == "Shift_JIS"
        ));
    }

    #[test]
    fn reports_bytes_that_are_not_valid_utf8() {
        // A lone continuation byte cannot start a UTF-8 sequence.
        assert!(matches!(
            decode_to_utf8(b"<t>\xFF\xFE\x00</t>").unwrap_err().kind,
            ErrorKind::MalformedEncoding(_)
        ));
    }
}

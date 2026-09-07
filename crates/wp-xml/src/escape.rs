//! Entity references, character references, and the normalizations XML requires.
//!
//! XML defines exactly five named entities. Anything else needs a document type
//! declaration, which office documents never carry and which this parser
//! deliberately does not process — a custom entity is the standard route for the
//! "billion laughs" expansion attack and for pulling external files into a
//! document.
//!
//! Two normalizations are not optional, and both change what the text says, so
//! they are done here rather than left to callers:
//!
//! * **Line endings** (XML 1.0 §2.11). `\r\n` and a lone `\r` both become `\n`
//!   in content. A carriage return that is genuinely meant survives only as a
//!   character reference, which is why [`escape_text`] writes one.
//! * **Attribute values** (§3.3.3). Literal tabs and line breaks become spaces.
//!   The same characters written as character references do not — that is the
//!   only way to keep them.

use std::borrow::Cow;

use crate::name::is_valid_character;
use crate::ErrorKind;

/// Which normalization applies to the text being expanded.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Character data: line endings collapse to `\n`.
    Content,
    /// An attribute value: every literal whitespace character becomes a space.
    AttributeValue,
}

/// Expands references in character data and normalizes line endings.
///
/// Returns borrowed text when there is nothing to change, which is the common
/// case for document content and avoids copying it.
pub fn unescape(text: &str) -> Result<Cow<'_, str>, ErrorKind> {
    expand(text, Mode::Content)
}

/// Expands references in an attribute value and normalizes whitespace.
pub(crate) fn unescape_attribute_value(text: &str) -> Result<Cow<'_, str>, ErrorKind> {
    expand(text, Mode::AttributeValue)
}

fn expand(text: &str, mode: Mode) -> Result<Cow<'_, str>, ErrorKind> {
    let has_whitespace_to_normalize = match mode {
        Mode::Content => text.contains('\r'),
        Mode::AttributeValue => text.contains(['\t', '\n', '\r']),
    };
    if !text.contains('&') && !has_whitespace_to_normalize {
        return Ok(Cow::Borrowed(text));
    }

    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0usize;

    while index < text.len() {
        let character = text[index..].chars().next().expect("index is on a character boundary");

        match character {
            '&' => {
                let after = &text[index + 1..];
                let Some(semicolon) = after.find(';') else {
                    return Err(ErrorKind::UnknownEntity(truncate_for_message(after)));
                };
                push_reference(&mut out, &after[..semicolon])?;
                index += 1 + semicolon + 1;
            }
            '\r' => {
                // A carriage return, alone or before a line feed, is one break.
                out.push(if mode == Mode::Content { '\n' } else { ' ' });
                index += 1;
                if bytes.get(index) == Some(&b'\n') {
                    index += 1;
                }
            }
            '\t' | '\n' if mode == Mode::AttributeValue => {
                out.push(' ');
                index += 1;
            }
            other => {
                out.push(other);
                index += other.len_utf8();
            }
        }
    }

    Ok(Cow::Owned(out))
}

/// Appends what a reference stands for.
fn push_reference(out: &mut String, reference: &str) -> Result<(), ErrorKind> {
    match reference {
        "amp" => out.push('&'),
        "lt" => out.push('<'),
        "gt" => out.push('>'),
        "quot" => out.push('"'),
        "apos" => out.push('\''),
        _ if reference.starts_with('#') => out.push(parse_character_reference(reference)?),
        _ => return Err(ErrorKind::UnknownEntity(reference.to_owned())),
    }
    Ok(())
}

/// Decodes `#123` or `#x1F600` into the character it names.
fn parse_character_reference(reference: &str) -> Result<char, ErrorKind> {
    let invalid = || ErrorKind::InvalidCharacterReference(format!("&{reference};"));

    let digits = &reference[1..];
    let code_point = if let Some(hex) = digits.strip_prefix(['x', 'X']) {
        u32::from_str_radix(hex, 16).map_err(|_| invalid())?
    } else {
        digits.parse::<u32>().map_err(|_| invalid())?
    };

    // Both checks matter: a code point can be a valid `char` and still be
    // illegal in XML, as every C0 control except tab and the two line endings is.
    if !is_valid_character(code_point) {
        return Err(invalid());
    }
    char::from_u32(code_point).ok_or_else(invalid)
}

/// Escapes text so it can appear as character data.
///
/// `>` does not strictly have to be escaped, but it is: leaving it alone means a
/// stray `]]>` could form and change what the document says.
///
/// A carriage return becomes a character reference because a parser is required
/// to turn a literal one into a line feed — writing it raw would silently change
/// the content on the next read.
#[must_use]
pub fn escape_text(text: &str) -> Cow<'_, str> {
    if !text.contains(['&', '<', '>', '\r']) {
        return Cow::Borrowed(text);
    }

    let mut out = String::with_capacity(text.len() + 16);
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            other => out.push(other),
        }
    }
    Cow::Owned(out)
}

/// Escapes a string so it can appear between double quotes as an attribute value.
///
/// Tabs and line breaks become character references, since literal ones would be
/// normalized to spaces when read back.
#[must_use]
pub fn escape_attribute_value(value: &str) -> Cow<'_, str> {
    if !value.contains(['&', '<', '>', '"', '\t', '\n', '\r']) {
        return Cow::Borrowed(value);
    }

    let mut out = String::with_capacity(value.len() + 16);
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            other => out.push(other),
        }
    }
    Cow::Owned(out)
}

/// Keeps an error message readable when the offending text is long.
fn truncate_for_message(text: &str) -> String {
    const LIMIT: usize = 32;
    match text.char_indices().nth(LIMIT) {
        Some((index, _)) => format!("{}...", &text[..index]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_without_references_is_not_copied() {
        assert!(matches!(unescape("plain text").unwrap(), Cow::Borrowed(_)));
        assert!(matches!(escape_text("plain text"), Cow::Borrowed(_)));
    }

    #[test]
    fn expands_the_five_built_in_entities() {
        assert_eq!(unescape("&amp;&lt;&gt;&quot;&apos;").unwrap(), "&<>\"'");
        assert_eq!(unescape("a &amp; b").unwrap(), "a & b");
    }

    #[test]
    fn expands_character_references() {
        assert_eq!(unescape("&#65;&#x42;&#x43;").unwrap(), "ABC");
        assert_eq!(unescape("&#1055;&#x440;").unwrap(), "Пр");
        assert_eq!(unescape("&#x1F600;").unwrap(), "\u{1F600}");
        // Tab, line feed and carriage return are the only legal C0 controls.
        assert_eq!(unescape("&#9;&#10;&#13;").unwrap(), "\t\n\r");
    }

    #[test]
    fn normalizes_line_endings_in_content() {
        assert_eq!(unescape("a\r\nb").unwrap(), "a\nb");
        assert_eq!(unescape("a\rb").unwrap(), "a\nb");
        assert_eq!(unescape("a\n\rb").unwrap(), "a\n\nb");
        // A reference is not a literal, so it survives untouched.
        assert_eq!(unescape("a&#13;b").unwrap(), "a\rb");
    }

    #[test]
    fn normalizes_whitespace_in_attribute_values() {
        assert_eq!(unescape_attribute_value("a\tb\nc\rd").unwrap(), "a b c d");
        assert_eq!(unescape_attribute_value("a\r\nb").unwrap(), "a b");
        // References keep the exact character, which is the only way to write one.
        assert_eq!(unescape_attribute_value("a&#9;b").unwrap(), "a\tb");
    }

    #[test]
    fn rejects_references_xml_does_not_define() {
        // Defined by HTML, not by XML: accepting it would change the text.
        assert!(matches!(unescape("&nbsp;"), Err(ErrorKind::UnknownEntity(_))));
        assert!(matches!(unescape("&custom;"), Err(ErrorKind::UnknownEntity(_))));
        assert!(matches!(unescape("&amp"), Err(ErrorKind::UnknownEntity(_))));
    }

    #[test]
    fn rejects_character_references_to_illegal_characters() {
        for reference in ["&#0;", "&#x0;", "&#8;", "&#xFFFE;", "&#x110000;", "&#xD800;"] {
            assert!(
                matches!(unescape(reference), Err(ErrorKind::InvalidCharacterReference(_))),
                "should have been rejected: {reference}"
            );
        }
    }

    #[test]
    fn escaping_then_unescaping_returns_the_original() {
        for original in [
            "plain",
            "a & b < c > d",
            "quotes \" and ' apostrophes",
            "tabs\tand newlines\n and returns\r",
            "многоязычный текст 文書 نص 🖋",
            "]]> looks like a CDATA end",
        ] {
            assert_eq!(unescape(&escape_text(original)).unwrap(), original, "text: {original:?}");
            assert_eq!(
                unescape_attribute_value(&escape_attribute_value(original)).unwrap(),
                original,
                "attribute: {original:?}"
            );
        }
    }

    #[test]
    fn escaped_text_cannot_form_markup() {
        let escaped = escape_text("<tag>text</tag> ]]> & more");
        assert!(!escaped.contains('<'));
        assert!(!escaped.contains('>'));
    }
}

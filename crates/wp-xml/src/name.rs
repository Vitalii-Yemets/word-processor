//! Which characters XML allows in a name.
//!
//! The ranges come straight from the XML 1.0 grammar (productions `NameStartChar`
//! and `NameChar`). They are wide — a name may be written in almost any script —
//! and getting them wrong in either direction causes real trouble: too strict and
//! valid documents are rejected, too lax and malformed ones slip through to be
//! written back out.

/// Whether a character may begin a name.
pub(crate) fn is_name_start(character: char) -> bool {
    matches!(character,
        ':' | '_'
        | 'A'..='Z'
        | 'a'..='z'
        | '\u{C0}'..='\u{D6}'
        | '\u{D8}'..='\u{F6}'
        | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}'
        | '\u{37F}'..='\u{1FFF}'
        | '\u{200C}'..='\u{200D}'
        | '\u{2070}'..='\u{218F}'
        | '\u{2C00}'..='\u{2FEF}'
        | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}'
        | '\u{FDF0}'..='\u{FFFD}'
        | '\u{10000}'..='\u{EFFFF}'
    )
}

/// Whether a character may appear in a name after the first position.
pub(crate) fn is_name_continuation(character: char) -> bool {
    is_name_start(character)
        || matches!(character,
            '-' | '.'
            | '0'..='9'
            | '\u{B7}'
            | '\u{300}'..='\u{36F}'
            | '\u{203F}'..='\u{2040}'
        )
}

/// Whether the whole string is a valid XML name.
pub(crate) fn is_valid_name(text: &str) -> bool {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) if is_name_start(first) => characters.all(is_name_continuation),
        _ => false,
    }
}

/// Whether a character is legal anywhere in an XML document.
///
/// Most control characters are not, which matters because a corrupted file can
/// otherwise carry them into the document model unnoticed.
pub(crate) fn is_valid_character(code_point: u32) -> bool {
    matches!(code_point,
        0x09 | 0x0A | 0x0D
        | 0x20..=0xD7FF
        | 0xE000..=0xFFFD
        | 0x10000..=0x10FFFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_names_used_by_office_documents() {
        for name in ["w:p", "pPr", "w:pStyle", "xmlns", "xml:space", "_x0020_", "a-b.c", "ns0:t"] {
            assert!(is_valid_name(name), "should be valid: {name:?}");
        }
    }

    #[test]
    fn accepts_names_in_other_scripts() {
        // A name may be written in any script; documents from localized tools do
        // this, and rejecting them would be a bug.
        for name in ["документ", "文書", "παράγραφος", "मूल"] {
            assert!(is_valid_name(name), "should be valid: {name:?}");
        }
    }

    #[test]
    fn rejects_malformed_names() {
        for name in ["", "1abc", "-abc", ".abc", "a b", "a<b", "a\"b", "a/b", " ", "\u{FFFF}"] {
            assert!(!is_valid_name(name), "should be invalid: {name:?}");
        }
    }

    #[test]
    fn rejects_characters_xml_forbids() {
        // NUL and most C0 controls are illegal even as character references.
        for code_point in [0x00, 0x01, 0x08, 0x0B, 0x0C, 0x1F, 0xFFFE, 0xFFFF, 0x11_0000] {
            assert!(!is_valid_character(code_point), "should be illegal: {code_point:#x}");
        }
        for code_point in [0x09, 0x0A, 0x0D, 0x20, 0x41, 0x400, 0xFFFD, 0x1_0000, 0x10_FFFF] {
            assert!(is_valid_character(code_point), "should be legal: {code_point:#x}");
        }
    }
}

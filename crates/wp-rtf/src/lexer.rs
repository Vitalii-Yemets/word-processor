//! Cutting an RTF file into its pieces.
//!
//! RTF is four things: braces that open and close groups, control words —
//! a backslash, letters, and perhaps a number — control symbols, which are a
//! backslash and one other character, and text, which is everything else.
//! That is the whole grammar; what the words mean is the reader's business.

/// One piece of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    /// `{`
    Open,
    /// `}`
    Close,
    /// A control word, with its number where it has one: `\fs24` is
    /// `Control("fs", Some(24))`.
    Control(String, Option<i32>),
    /// `\*`, which says the group it begins may be skipped by a reader that
    /// does not know it.
    Star,
    /// `\'hh`: one byte of text in the document's code page.
    Hex(u8),
    /// A control symbol that stands for a character: `\~` is a non-breaking
    /// space, `\{` a brace.
    Character(char),
    /// One byte of ordinary text.
    Byte(u8),
}

/// Reads the pieces one at a time.
pub struct Lexer<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Lexer<'a> {
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// The next piece, or nothing at the end.
    pub fn next_token(&mut self) -> Option<Token> {
        loop {
            let byte = *self.bytes.get(self.at)?;
            self.at += 1;
            match byte {
                b'{' => return Some(Token::Open),
                b'}' => return Some(Token::Close),
                b'\\' => return Some(self.control()),
                // Line ends mean nothing: the file is wrapped wherever the
                // writer felt like wrapping it.
                b'\r' | b'\n' => continue,
                other => return Some(Token::Byte(other)),
            }
        }
    }

    /// What follows a backslash.
    fn control(&mut self) -> Token {
        let Some(&first) = self.bytes.get(self.at) else { return Token::Byte(b'\\') };
        if first.is_ascii_alphabetic() {
            let start = self.at;
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_alphabetic) {
                self.at += 1;
            }
            let word = String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned();

            // The number: a sign, then digits, as far as they go.
            let number_start = self.at;
            if self.bytes.get(self.at) == Some(&b'-') {
                self.at += 1;
            }
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
            let number = if self.at > number_start {
                core::str::from_utf8(&self.bytes[number_start..self.at])
                    .ok()
                    .and_then(|text| text.parse::<i32>().ok())
            } else {
                None
            };
            // One space after a control word is part of it, not text.
            if self.bytes.get(self.at) == Some(&b' ') {
                self.at += 1;
            }
            return Token::Control(word, number);
        }

        self.at += 1;
        match first {
            b'\'' => {
                let high = self.bytes.get(self.at).copied().and_then(hex_digit);
                let low = self.bytes.get(self.at + 1).copied().and_then(hex_digit);
                match (high, low) {
                    (Some(high), Some(low)) => {
                        self.at += 2;
                        Token::Hex(high << 4 | low)
                    }
                    // A malformed escape is nothing, rather than a backslash
                    // and a quote in the text.
                    _ => Token::Character('\u{FFFD}'),
                }
            }
            b'*' => Token::Star,
            b'~' => Token::Character('\u{00A0}'),
            b'-' => Token::Character('\u{00AD}'),
            b'_' => Token::Character('\u{2011}'),
            b'{' | b'}' | b'\\' => Token::Character(first as char),
            // A backslash at the end of a line is an old way of writing a
            // paragraph end.
            b'\r' | b'\n' => {
                if first == b'\r' && self.bytes.get(self.at) == Some(&b'\n') {
                    self.at += 1;
                }
                Token::Control("par".to_owned(), None)
            }
            other => Token::Character(other as char),
        }
    }
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(text: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(text.as_bytes());
        let mut out = Vec::new();
        while let Some(token) = lexer.next_token() {
            out.push(token);
        }
        out
    }

    #[test]
    fn words_carry_their_numbers_and_swallow_one_space() {
        assert_eq!(
            all("{\\rtf1\\fs-24 Hi}"),
            vec![
                Token::Open,
                Token::Control("rtf".to_owned(), Some(1)),
                Token::Control("fs".to_owned(), Some(-24)),
                Token::Byte(b'H'),
                Token::Byte(b'i'),
                Token::Close,
            ]
        );
        // Two spaces: one belongs to the word, the other is text.
        assert_eq!(
            all("\\b  x"),
            vec![Token::Control("b".to_owned(), None), Token::Byte(b' '), Token::Byte(b'x')]
        );
    }

    #[test]
    fn symbols_and_hex_bytes_are_read() {
        assert_eq!(
            all("\\'e9\\~\\{\\*\\\\"),
            vec![
                Token::Hex(0xE9),
                Token::Character('\u{00A0}'),
                Token::Character('{'),
                Token::Star,
                Token::Character('\\'),
            ]
        );
        assert_eq!(all("a\r\nb"), vec![Token::Byte(b'a'), Token::Byte(b'b')]);
        assert_eq!(all("\\\r\n"), vec![Token::Control("par".to_owned(), None)]);
    }
}

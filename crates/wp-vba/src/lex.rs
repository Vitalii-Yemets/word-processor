//! Visual Basic, taken apart into words.
//!
//! # Every byte is kept
//!
//! A token carries the text it was written as and everything that came before
//! it on the way — the spaces, the tab somebody indented with, the underscore
//! that continues a line onto the next. Putting every token's `before` and
//! `text` back together in order gives back the file, byte for byte, and that
//! is what makes it possible to say that a module parsed *and* came back the
//! same. A lexer that threw away spaces would make the second half of that
//! question unaskable.
//!
//! # What is a word and what is not
//!
//! Visual Basic does not care about case: `Sub`, `sub` and `SUB` are the same
//! word, and a name is matched the same way. The text is kept exactly as it
//! was written, and the comparing is done by [`Token::is`].
//!
//! Three things need looking ahead at:
//!
//! - a line continuation is a space, an underscore and the end of the line,
//!   and it belongs to the *next* token's `before`, because the statement
//!   carries on;
//! - `#` begins a date when another `#` closes it on the same line with only
//!   date-ish characters between, and otherwise it is a symbol — which is
//!   what it is in `#If` and in `Print #1`;
//! - `&` begins a number when `H` or `O` follows it, and is otherwise the
//!   operator that joins two strings.

/// What a token is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A name or a keyword; which of the two is the parser's business.
    Word,
    /// A number, with whatever suffix it was written with.
    Number,
    /// Text in quotes.
    Text,
    /// A date between hashes.
    Date,
    /// An operator or a piece of punctuation.
    Symbol,
    /// A comment, from its apostrophe or its `Rem` to the end of the line.
    Comment,
    /// The end of a line, and the bytes that ended it.
    NewLine,
    /// The end of the module.
    End,
}

/// One word of a module, and everything written before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    /// Exactly as it was written.
    pub text: String,
    /// The spaces, tabs and line continuations in front of it.
    pub before: String,
    /// Which line it is on, counting from one, as an editor counts.
    pub line: usize,
}

impl Token {
    /// Whether this is the word given, whatever case it was written in.
    #[must_use]
    pub fn is(&self, word: &str) -> bool {
        self.kind == Kind::Word && self.text.eq_ignore_ascii_case(word)
    }

    /// Whether this is the symbol given.
    #[must_use]
    pub fn symbol(&self, symbol: &str) -> bool {
        self.kind == Kind::Symbol && self.text == symbol
    }

    /// The token as it appeared in the file, with what came before it.
    #[must_use]
    pub fn written(&self) -> String {
        format!("{}{}", self.before, self.text)
    }
}

/// Takes a module apart into words.
///
/// Never fails. Anything this does not understand comes back as a symbol of
/// one character, and the parser is where a complaint belongs: a lexer that
/// refused a file would refuse to show it as well.
#[must_use]
pub fn tokens(source: &str) -> Vec<Token> {
    let bytes: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut at = 0usize;
    let mut line = 1usize;

    loop {
        let before = trivia(&bytes, &mut at, &mut line);
        let Some(first) = bytes.get(at).copied() else {
            out.push(Token { kind: Kind::End, text: String::new(), before, line });
            return out;
        };

        let start = at;
        let kind = match first {
            '\r' | '\n' => {
                at += 1;
                if first == '\r' && bytes.get(at) == Some(&'\n') {
                    at += 1;
                }
                let token = Token {
                    kind: Kind::NewLine,
                    text: bytes[start..at].iter().collect(),
                    before,
                    line,
                };
                line += 1;
                out.push(token);
                continue;
            }
            '\'' => {
                while at < bytes.len() && bytes[at] != '\r' && bytes[at] != '\n' {
                    at += 1;
                }
                Kind::Comment
            }
            '"' => {
                at += 1;
                while at < bytes.len() {
                    if bytes[at] == '"' {
                        // Two quotes in a row are one quote in the text, and
                        // not the end of it.
                        if bytes.get(at + 1) == Some(&'"') {
                            at += 2;
                            continue;
                        }
                        at += 1;
                        break;
                    }
                    if bytes[at] == '\r' || bytes[at] == '\n' {
                        break;
                    }
                    at += 1;
                }
                Kind::Text
            }
            '#' if date_ends(&bytes, at) => {
                at += 1;
                while at < bytes.len() && bytes[at] != '#' {
                    at += 1;
                }
                at += usize::from(at < bytes.len());
                Kind::Date
            }
            '&' if matches!(bytes.get(at + 1), Some('h' | 'H' | 'o' | 'O')) => {
                at += 2;
                while at < bytes.len() && (bytes[at].is_ascii_alphanumeric()) {
                    at += 1;
                }
                // `&HFF&` is a long written in hexadecimal.
                if bytes.get(at) == Some(&'&') {
                    at += 1;
                }
                Kind::Number
            }
            letter if letter.is_alphabetic() || letter == '_' => {
                while at < bytes.len() && (bytes[at].is_alphanumeric() || bytes[at] == '_') {
                    at += 1;
                }
                // `Rem` is a comment that spells itself out.
                let word: String = bytes[start..at].iter().collect();
                if word.eq_ignore_ascii_case("rem") {
                    while at < bytes.len() && bytes[at] != '\r' && bytes[at] != '\n' {
                        at += 1;
                    }
                    Kind::Comment
                } else {
                    Kind::Word
                }
            }
            digit if digit.is_ascii_digit() => {
                number(&bytes, &mut at);
                Kind::Number
            }
            '.' if bytes.get(at + 1).is_some_and(char::is_ascii_digit) => {
                number(&bytes, &mut at);
                Kind::Number
            }
            _ => {
                // The three symbols made of two characters, and then the
                // ones made of one.
                let pair: String = bytes[at..(at + 2).min(bytes.len())].iter().collect();
                at += if matches!(pair.as_str(), "<=" | ">=" | "<>") { 2 } else { 1 };
                Kind::Symbol
            }
        };

        out.push(Token { kind, text: bytes[start..at].iter().collect(), before, line });
    }
}

/// The spaces, tabs and line continuations in front of the next token.
fn trivia(bytes: &[char], at: &mut usize, line: &mut usize) -> String {
    let start = *at;
    loop {
        match bytes.get(*at) {
            Some(' ' | '\t') => *at += 1,
            // An underscore at the end of a line carries the statement onto
            // the next one, so the end of that line is not the end of the
            // statement and belongs in front of whatever comes next.
            Some('_') if continues(bytes, *at) => {
                while bytes.get(*at) != Some(&'\n') {
                    *at += 1;
                }
                *at += 1;
                *line += 1;
            }
            _ => break,
        }
    }
    bytes[start..*at].iter().collect()
}

/// Whether an underscore here is a line continuation.
fn continues(bytes: &[char], at: usize) -> bool {
    let mut looking = at + 1;
    while matches!(bytes.get(looking), Some(' ' | '\t' | '\r')) {
        looking += 1;
    }
    bytes.get(looking) == Some(&'\n')
}

/// Whether a `#` here opens a date.
fn date_ends(bytes: &[char], at: usize) -> bool {
    let mut looking = at + 1;
    let mut anything = false;
    while let Some(letter) = bytes.get(looking) {
        match letter {
            '#' => return anything,
            '0'..='9' | 'a'..='z' | 'A'..='Z' | '/' | '-' | ':' | '.' | ' ' | ',' => {
                anything = true;
                looking += 1;
            }
            _ => return false,
        }
    }
    false
}

/// Walks past a number, with its exponent and its type suffix.
fn number(bytes: &[char], at: &mut usize) {
    let mut seen_point = false;
    while let Some(letter) = bytes.get(*at) {
        match letter {
            '0'..='9' => *at += 1,
            '.' if !seen_point => {
                seen_point = true;
                *at += 1;
            }
            // An exponent, which may carry a sign.
            'e' | 'E' | 'd' | 'D'
                if matches!(bytes.get(*at + 1), Some('0'..='9'))
                    || (matches!(bytes.get(*at + 1), Some('+' | '-'))
                        && matches!(bytes.get(*at + 2), Some('0'..='9'))) =>
            {
                *at += 2;
            }
            _ => break,
        }
    }
    // The suffix that says what kind of number it is.
    if matches!(bytes.get(*at), Some('&' | '%' | '#' | '!' | '@' | '^')) {
        *at += 1;
    }
}

/// The source a list of tokens was made from.
///
/// The other half of the promise: what went in comes out.
#[must_use]
pub fn written(tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        out.push_str(&token.before);
        out.push_str(&token.text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<(Kind, String)> {
        tokens(source)
            .into_iter()
            .filter(|token| token.kind != Kind::End)
            .map(|token| (token.kind, token.text))
            .collect()
    }

    #[test]
    fn what_goes_in_comes_out() {
        // The whole point of keeping the spaces: a module that parses and
        // does not come back the same has not been read, it has been guessed.
        for source in [
            "",
            "Sub Hello()\r\n    MsgBox \"Hello\"\r\nEnd Sub\r\n",
            "  Dim x   As  Long\t' with a comment\r\n",
            "a = b _\r\n    + c\r\n",
            "Rem an old-fashioned comment\n",
            "If a Then b = 1 Else b = 2\n",
            "x = #1/1/2000# 'a date\r\n",
        ] {
            assert_eq!(written(&tokens(source)), source, "{source:?}");
        }
    }

    #[test]
    fn a_line_continuation_belongs_to_the_statement_and_not_to_the_line() {
        // `a = b _` and `+ c` are one statement, so there is no end of line
        // between them: the newline is carried in front of the `+`.
        let found = tokens("a = b _\r\n    + c\r\n");
        let ends = found.iter().filter(|token| token.kind == Kind::NewLine).count();
        assert_eq!(ends, 1, "the continuation was read as the end of a statement");
        let plus = found.iter().find(|token| token.text == "+").expect("the plus");
        assert!(plus.before.contains('\n'), "the newline was thrown away");
        assert_eq!(plus.line, 2, "a continued line still counts as a line");
    }

    #[test]
    fn two_quotes_in_a_row_are_one_quote_and_not_the_end() {
        assert_eq!(
            kinds(r#"s = "He said ""no""" "#),
            vec![
                (Kind::Word, "s".to_owned()),
                (Kind::Symbol, "=".to_owned()),
                (Kind::Text, r#""He said ""no""""#.to_owned()),
            ]
        );
    }

    #[test]
    fn a_hash_is_a_date_only_when_one_closes_it() {
        assert_eq!(kinds("#1/1/2000#"), vec![(Kind::Date, "#1/1/2000#".to_owned())]);
        // `#If` and a file number are not dates, and reading them as one
        // would swallow the rest of the line.
        assert_eq!(kinds("#If")[0], (Kind::Symbol, "#".to_owned()));
        assert_eq!(kinds("Close #1")[1], (Kind::Symbol, "#".to_owned()));
    }

    #[test]
    fn numbers_are_read_with_their_exponents_and_their_suffixes() {
        assert_eq!(kinds("1.5e-3"), vec![(Kind::Number, "1.5e-3".to_owned())]);
        assert_eq!(kinds("&HFF&"), vec![(Kind::Number, "&HFF&".to_owned())]);
        assert_eq!(kinds("&O17"), vec![(Kind::Number, "&O17".to_owned())]);
        assert_eq!(kinds("100&"), vec![(Kind::Number, "100&".to_owned())]);
        // And an ampersand on its own is what joins two strings together.
        assert_eq!(kinds("a & b")[1], (Kind::Symbol, "&".to_owned()));
    }

    #[test]
    fn a_comment_runs_to_the_end_of_its_line_however_it_began() {
        assert_eq!(kinds("' one\n")[0], (Kind::Comment, "' one".to_owned()));
        assert_eq!(kinds("Rem two\n")[0], (Kind::Comment, "Rem two".to_owned()));
        // But a name that begins with those letters is a name.
        assert_eq!(kinds("Remaining = 1")[0], (Kind::Word, "Remaining".to_owned()));
    }

    #[test]
    fn the_case_a_word_was_written_in_is_kept_and_not_compared() {
        let found = tokens("SUB");
        assert_eq!(found[0].text, "SUB");
        assert!(found[0].is("sub"));
        assert!(!found[0].is("end"));
    }
}

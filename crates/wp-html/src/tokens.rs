//! Cutting a page into tags and text.
//!
//! Not a parser of HTML as the standard defines it — that is a document of
//! a thousand pages about recovering from every mistake anybody has ever
//! made — but a tokenizer of HTML as it is written: tags with attributes
//! quoted, half-quoted or not quoted at all, comments, the conditional
//! comments Word writes, `<script>` and `<style>` whose insides are not
//! tags, and text with its entities. What the tags mean is the reader's
//! business.

/// One piece of a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    /// An opening tag, with its name in lower case and its attributes.
    Open {
        name: String,
        attributes: Vec<(String, String)>,
        self_closing: bool,
    },
    /// A closing tag.
    Close(String),
    /// Text between tags, with its entities resolved and its whitespace as
    /// written.
    Text(String),
    /// The inside of a `<style>` element, as written.
    Style(String),
    /// A comment, or `<!DOCTYPE>`: nothing the page shows.
    Comment,
    /// `<!--[if gte vml 1]> ... <![endif]-->`: what only a reader that knows
    /// the thing named is shown — Word's drawings in VML, its table styles,
    /// its document settings — with the condition and what it holds.
    Hidden {
        condition: String,
        inner: String,
    },
    /// `<![if !supportLists]>`: what follows, up to `<![endif]>`, is shown
    /// only by a reader that does not know the thing named — which Word
    /// uses for the bullet of a list it has already described another way.
    /// `<!--[if !supportLists]-->` is the same, written so as to be a comment
    /// to a reader that knows no conditions at all.
    ConditionalOpen(String),
    ConditionalClose,
}

/// Reads the pieces one at a time.
pub struct Tokenizer<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Tokenizer<'a> {
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self { text, at: 0 }
    }

    /// The next piece, or nothing at the end.
    pub fn next_token(&mut self) -> Option<Token> {
        let rest = &self.text[self.at..];
        if rest.is_empty() {
            return None;
        }
        if let Some(after) = rest.strip_prefix('<') {
            if let Some(token) = self.markup(after) {
                return Some(token);
            }
            // A stray `<` that begins no tag is text.
            let end = rest[1..].find('<').map_or(rest.len(), |found| found + 1);
            self.at += end;
            return Some(Token::Text(decode_entities(&rest[..end])));
        }
        let end = rest.find('<').unwrap_or(rest.len());
        self.at += end;
        Some(Token::Text(decode_entities(&rest[..end])))
    }

    /// A comment that is a condition: `rest` is what follows `<!--`.
    fn conditional_comment(&mut self, rest: &str) -> Option<Token> {
        // `<!--[endif]-->`, the end of one written as a comment.
        if rest.starts_with("[endif]-->") {
            self.at += 1 + 3 + "[endif]-->".len();
            return Some(Token::ConditionalClose);
        }
        let inside = rest.strip_prefix("[if ")?;
        let close = inside.find(']')?;
        let condition = inside[..close].trim().to_owned();
        let after = &inside[close + 1..];
        // `<!--[if !supportLists]-->`: what follows is shown to everyone but
        // the reader that knows the thing.
        if after.starts_with("-->") {
            self.at += 1 + 3 + "[if ".len() + close + 1 + 3;
            return Some(Token::ConditionalOpen(condition));
        }
        // `<!--[if gte vml 1]> ... <![endif]-->`: shown only to that reader.
        let body = after.strip_prefix('>')?;
        let (inner, used) = match body.find("<![endif]-->") {
            Some(end) => (&body[..end], end + "<![endif]-->".len()),
            None => match body.find("-->") {
                Some(end) => (&body[..end], end + 3),
                None => (body, body.len()),
            },
        };
        let inner = inner.to_owned();
        self.at += 1 + 3 + "[if ".len() + close + 1 + 1 + used;
        Some(Token::Hidden { condition, inner })
    }

    /// Something beginning with `<`; `after` is what follows it.
    fn markup(&mut self, after: &str) -> Option<Token> {
        // Comments, the conditional ones among them, and the doctype.
        if let Some(rest) = after.strip_prefix("!--") {
            if let Some(token) = self.conditional_comment(rest) {
                return Some(token);
            }
            let end = rest.find("-->").map_or(rest.len(), |found| found + 3);
            self.at += 1 + 3 + end;
            return Some(Token::Comment);
        }
        if let Some(rest) = after.strip_prefix("![if ") {
            let end = rest.find("]>")?;
            self.at += 1 + 5 + end + 2;
            return Some(Token::ConditionalOpen(rest[..end].trim().to_owned()));
        }
        if let Some(rest) = after.strip_prefix("![endif]>") {
            let _ = rest;
            self.at += 1 + 9;
            return Some(Token::ConditionalClose);
        }
        if after.starts_with('!') || after.starts_with('?') {
            let end = after.find('>').map_or(after.len(), |found| found + 1);
            self.at += 1 + end;
            return Some(Token::Comment);
        }

        // A closing tag.
        if let Some(rest) = after.strip_prefix('/') {
            let end = rest.find('>')?;
            let name = rest[..end].trim().to_ascii_lowercase();
            self.at += 1 + 1 + end + 1;
            return Some(Token::Close(name));
        }

        // An opening tag: the name, then attributes to the `>`.
        let name_end =
            after.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(after.len());
        let name = after[..name_end].to_ascii_lowercase();
        if name.is_empty() || !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        let (attributes, consumed, self_closing) = attributes(&after[name_end..]);
        self.at += 1 + name_end + consumed;

        // The inside of a style or a script is not tags: it runs to the
        // closing tag, whatever it holds.
        if name == "style" || name == "script" {
            let rest = &self.text[self.at..];
            let lower = rest.to_ascii_lowercase();
            let close = lower.find(&format!("</{name}")).unwrap_or(rest.len());
            let inside = rest[..close].to_owned();
            let close_end = rest[close..].find('>').map_or(rest.len(), |found| close + found + 1);
            self.at += close_end;
            return Some(if name == "style" { Token::Style(inside) } else { Token::Comment });
        }
        Some(Token::Open { name, attributes, self_closing })
    }
}

/// The attributes of a tag, how many bytes they took up to and including
/// the `>`, and whether the tag closed itself.
fn attributes(text: &str) -> (Vec<(String, String)>, usize, bool) {
    let mut attributes = Vec::new();
    let mut at = 0;
    let bytes = text.as_bytes();
    let mut self_closing = false;
    loop {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at >= bytes.len() {
            return (attributes, at, self_closing);
        }
        if bytes[at] == b'>' {
            return (attributes, at + 1, self_closing);
        }
        if bytes[at] == b'/' {
            self_closing = true;
            at += 1;
            continue;
        }
        // The name, to whitespace, `=`, `>` or `/`.
        let name_start = at;
        while at < bytes.len()
            && !matches!(bytes[at], b'=' | b'>' | b'/')
            && !bytes[at].is_ascii_whitespace()
        {
            at += 1;
        }
        let name = text[name_start..at].to_ascii_lowercase();
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let mut value = String::new();
        if at < bytes.len() && bytes[at] == b'=' {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            if at < bytes.len() && (bytes[at] == b'"' || bytes[at] == b'\'') {
                let quote = bytes[at];
                at += 1;
                let start = at;
                while at < bytes.len() && bytes[at] != quote {
                    at += 1;
                }
                value = decode_entities(&text[start..at]);
                if at < bytes.len() {
                    at += 1;
                }
            } else {
                // Unquoted, as Word writes most of its attributes: to
                // whitespace or the end of the tag.
                let start = at;
                while at < bytes.len() && bytes[at] != b'>' && !bytes[at].is_ascii_whitespace() {
                    at += 1;
                }
                value = decode_entities(&text[start..at]);
            }
        }
        if !name.is_empty() {
            attributes.push((name, value));
        }
    }
}

/// Text with its entities turned into the characters they name.
#[must_use]
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest[1..].find(';').filter(|end| *end <= 10);
        let Some(end) = end else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..=end];
        match entity(name) {
            Some(character) => {
                out.push(character);
                rest = &rest[end + 2..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The character an entity names, numeric or by name.
fn entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let value = if let Some(hex) = number.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            number.parse::<u32>().ok()?
        };
        // The Windows-1252 range, which pages write by number and mean the
        // characters of that page.
        let value = match value {
            0x80..=0x9F => {
                let (byte, _) = ((value as u8), ());
                return wp_text::Encoding::CodePage(1252).decode(&[byte]).chars().next();
            }
            other => other,
        };
        return char::from_u32(value);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{00A0}',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "zwnj" => '\u{200C}',
        "zwj" => '\u{200D}',
        "lrm" => '\u{200E}',
        "rlm" => '\u{200F}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "sbquo" => '\u{201A}',
        "ldquo" => '\u{201C}',
        "rdquo" => '\u{201D}',
        "bdquo" => '\u{201E}',
        "hellip" => '\u{2026}',
        "bull" => '\u{2022}',
        "middot" => '\u{00B7}',
        "copy" => '\u{00A9}',
        "reg" => '\u{00AE}',
        "trade" => '\u{2122}',
        "deg" => '\u{00B0}',
        "plusmn" => '\u{00B1}',
        "times" => '\u{00D7}',
        "divide" => '\u{00F7}',
        "euro" => '\u{20AC}',
        "pound" => '\u{00A3}',
        "yen" => '\u{00A5}',
        "cent" => '\u{00A2}',
        "sect" => '\u{00A7}',
        "para" => '\u{00B6}',
        "laquo" => '\u{00AB}',
        "raquo" => '\u{00BB}',
        "shy" => '\u{00AD}',
        "iexcl" => '\u{00A1}',
        "iquest" => '\u{00BF}',
        "frac12" => '\u{00BD}',
        "frac14" => '\u{00BC}',
        "frac34" => '\u{00BE}',
        "sup2" => '\u{00B2}',
        "sup3" => '\u{00B3}',
        "micro" => '\u{00B5}',
        "dagger" => '\u{2020}',
        "Dagger" => '\u{2021}',
        "permil" => '\u{2030}',
        "prime" => '\u{2032}',
        "larr" => '\u{2190}',
        "rarr" => '\u{2192}',
        "uarr" => '\u{2191}',
        "darr" => '\u{2193}',
        "harr" => '\u{2194}',
        "ne" => '\u{2260}',
        "le" => '\u{2264}',
        "ge" => '\u{2265}',
        "infin" => '\u{221E}',
        "minus" => '\u{2212}',
        "alpha" => 'α',
        "beta" => 'β',
        "gamma" => 'γ',
        "delta" => 'δ',
        "pi" => 'π',
        "sigma" => 'σ',
        "omega" => 'ω',
        "Omega" => 'Ω',
        "Delta" => 'Δ',
        "Sigma" => 'Σ',
        _ => return latin1_entity(name),
    })
}

/// The accented Latin letters, whose names are their letter and their
/// accent: `eacute`, `Ntilde`, `uuml`.
fn latin1_entity(name: &str) -> Option<char> {
    const ACCENTS: &[(&str, &str, &str)] = &[
        ("grave", "AEIOUaeiou", "ÀÈÌÒÙàèìòù"),
        ("acute", "AEIOUYaeiouy", "ÁÉÍÓÚÝáéíóúý"),
        ("circ", "AEIOUaeiou", "ÂÊÎÔÛâêîôû"),
        ("tilde", "ANOano", "ÃÑÕãñõ"),
        ("uml", "AEIOUaeiouy", "ÄËÏÖÜäëïöüÿ"),
        ("ring", "Aa", "Åå"),
        ("cedil", "Cc", "Çç"),
        ("slash", "Oo", "Øø"),
    ];
    for (accent, letters, accented) in ACCENTS {
        if let Some(letter) = name.strip_suffix(accent) {
            let mut letter = letter.chars();
            let (Some(one), None) = (letter.next(), letter.next()) else { continue };
            if let Some(index) = letters.chars().position(|held| held == one) {
                return accented.chars().nth(index);
            }
        }
    }
    Some(match name {
        "szlig" => 'ß',
        "AElig" => 'Æ',
        "aelig" => 'æ',
        "OElig" => 'Œ',
        "oelig" => 'œ',
        "ETH" => 'Ð',
        "eth" => 'ð',
        "THORN" => 'Þ',
        "thorn" => 'þ',
        "Scaron" => 'Š',
        "scaron" => 'š',
        "Yuml" => 'Ÿ',
        "fnof" => 'ƒ',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(text: &str) -> Vec<Token> {
        let mut tokenizer = Tokenizer::new(text);
        let mut out = Vec::new();
        while let Some(token) = tokenizer.next_token() {
            out.push(token);
        }
        out
    }

    #[test]
    fn tags_come_with_their_attributes_however_they_are_quoted() {
        let tokens = all("<p class=MsoNormal style='text-align:center' align=\"center\">Hi</p>");
        assert_eq!(
            tokens[0],
            Token::Open {
                name: "p".to_owned(),
                attributes: vec![
                    ("class".to_owned(), "MsoNormal".to_owned()),
                    ("style".to_owned(), "text-align:center".to_owned()),
                    ("align".to_owned(), "center".to_owned()),
                ],
                self_closing: false,
            }
        );
        assert_eq!(tokens[1], Token::Text("Hi".to_owned()));
        assert_eq!(tokens[2], Token::Close("p".to_owned()));
        assert_eq!(
            all("<br/>")[0],
            Token::Open { name: "br".to_owned(), attributes: vec![], self_closing: true }
        );
        assert_eq!(
            all("<BR>")[0],
            Token::Open { name: "br".to_owned(), attributes: vec![], self_closing: false }
        );
    }

    #[test]
    fn comments_conditionals_and_styles_are_told_apart() {
        let tokens = all(
            "<!DOCTYPE html><!--[if gte mso 9]><xml><o:x/></xml><![endif]--><style>p {x}</style><![if !supportLists]>·<![endif]>a",
        );
        assert_eq!(
            tokens,
            vec![
                Token::Comment,
                Token::Hidden {
                    condition: "gte mso 9".to_owned(),
                    inner: "<xml><o:x/></xml>".to_owned()
                },
                Token::Style("p {x}".to_owned()),
                Token::ConditionalOpen("!supportLists".to_owned()),
                Token::Text("·".to_owned()),
                Token::ConditionalClose,
                Token::Text("a".to_owned()),
            ]
        );
    }

    #[test]
    fn a_condition_written_as_a_comment_is_a_condition() {
        assert_eq!(
            all("<!--[if !supportLists]-->\u{b7}<!--[endif]-->a<!-- plain -->"),
            vec![
                Token::ConditionalOpen("!supportLists".to_owned()),
                Token::Text("\u{b7}".to_owned()),
                Token::ConditionalClose,
                Token::Text("a".to_owned()),
                Token::Comment,
            ]
        );
    }

    #[test]
    fn entities_become_their_characters() {
        assert_eq!(
            decode_entities("a &amp; b &lt;c&gt; &nbsp;&eacute;&#233;&#x2014;&#150;&mdash;"),
            "a & b <c> \u{00A0}éé\u{2014}\u{2013}\u{2014}"
        );
        assert_eq!(decode_entities("AT&T &unknown; & x"), "AT&T &unknown; & x");
    }
}

//! Excel's number format codes, as a chart uses them.
//!
//! A chart says how its numbers are written in the language a spreadsheet
//! cell says it in: `#,##0.00` for money with the pennies, `0%` for a share,
//! `"$"#,##0` for dollars. Without reading it, money is drawn as a bare
//! number and a share of a whole as `0.25`. This is the part of that
//! language a chart meets: digits, thousands, decimals, percent, a scale by
//! a thousand, scientific notation, the words round a number, and the
//! sections that give a negative or a nought a format of its own.
//!
//! Dates and times, fractions and the conditions a section can carry
//! (`[>100]`) are not read: a code with any of them in it is written as
//! `General` says, which is the number as it is. Colours (`[Red]`) are read
//! and let go, because a chart draws its numbers in the colour its text is.

/// A number written the way a format code says.
#[must_use]
pub fn format(code: &str, value: f64) -> String {
    let sections = split_sections(code);
    if sections.is_empty() || sections.iter().all(|section| section.trim().is_empty()) {
        return general(value);
    }

    // Which section is the number's: the first is for positive numbers and
    // for everything when it is alone, the second for negative ones, the
    // third for nought. A negative number with a section of its own is
    // written as its size — the section says how the sign is shown.
    let (section, magnitude, signed) = match sections.len() {
        1 => (sections[0].as_str(), value.abs(), value < 0.0),
        2 if value < 0.0 => (sections[1].as_str(), value.abs(), false),
        2 => (sections[0].as_str(), value, false),
        _ if value < 0.0 => (sections[1].as_str(), value.abs(), false),
        _ if value == 0.0 => (sections[2].as_str(), 0.0, false),
        _ => (sections[0].as_str(), value, false),
    };

    let Some(pattern) = Pattern::parse(section) else { return general(value) };
    let written = pattern.write(magnitude);
    if signed && !written.is_empty() {
        format!("-{written}")
    } else {
        written
    }
}

/// What `General` means: the number as it is, to as many figures as it
/// needs and no more.
#[must_use]
pub fn general(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    if !value.is_finite() {
        return String::new();
    }
    // Ten significant figures is what a chart shows, and past them a number
    // is only its own rounding error.
    let scale = 10f64.powi(9 - value.abs().log10().floor() as i32);
    let rounded = (value * scale).round() / scale;
    if rounded == rounded.trunc() && rounded.abs() < 1e15 {
        return format!("{}", rounded as i64);
    }
    let text = format!("{rounded}");
    if text.contains('e') || text.contains('E') {
        return format!("{rounded:E}");
    }
    text
}

/// The sections of a code, split at the semicolons that are not inside
/// quotes.
fn split_sections(code: &str) -> Vec<String> {
    let mut sections = vec![String::new()];
    let mut quoted = false;
    let mut escaped = false;
    for character in code.chars() {
        if escaped {
            sections.last_mut().expect("one section").push(character);
            escaped = false;
            continue;
        }
        match character {
            '"' => quoted = !quoted,
            '\\' if !quoted => {
                escaped = true;
                sections.last_mut().expect("one section").push(character);
                continue;
            }
            ';' if !quoted => {
                sections.push(String::new());
                continue;
            }
            _ => {}
        }
        sections.last_mut().expect("one section").push(character);
    }
    sections
}

/// One piece of a section.
#[derive(Clone, Debug, PartialEq)]
enum Token {
    /// Words written as they are.
    Literal(String),
    /// A place for a digit: `0` always written, `#` written when there is
    /// one, `?` a space when there is not.
    Digit(char),
    Point,
    /// A comma among the digits: thousands are grouped, and each comma after
    /// the last digit divides by a thousand.
    Comma,
    Percent,
    /// `E+00` and its like: how many digits the exponent is padded to, and
    /// whether a positive one carries its sign.
    Exponent(usize, bool),
    /// Where the text of a text section goes, which for a number is the
    /// number as it is.
    Text,
}

/// A section, read into its pieces.
struct Pattern {
    tokens: Vec<Token>,
}

impl Pattern {
    /// Reads a section, or nothing for one written in a part of the
    /// language not read here.
    fn parse(section: &str) -> Option<Self> {
        if section.trim().eq_ignore_ascii_case("general") {
            return Some(Self { tokens: vec![Token::Text] });
        }
        let mut tokens = Vec::new();
        let mut literal = String::new();
        let mut characters = section.chars().peekable();
        let push_literal = |literal: &mut String, tokens: &mut Vec<Token>| {
            if !literal.is_empty() {
                tokens.push(Token::Literal(std::mem::take(literal)));
            }
        };

        while let Some(character) = characters.next() {
            match character {
                '"' => {
                    for inner in characters.by_ref() {
                        if inner == '"' {
                            break;
                        }
                        literal.push(inner);
                    }
                }
                '\\' => {
                    if let Some(next) = characters.next() {
                        literal.push(next);
                    }
                }
                // A space as wide as the character after, which on a chart is
                // a space.
                '_' => {
                    characters.next();
                    literal.push(' ');
                }
                // A character repeated to fill the cell, which a chart has no
                // cell to fill.
                '*' => {
                    characters.next();
                }
                '[' => {
                    let mut inside = String::new();
                    for inner in characters.by_ref() {
                        if inner == ']' {
                            break;
                        }
                        inside.push(inner);
                    }
                    // A colour is let go; a condition or a locale is more of
                    // the language than is read here.
                    if !is_a_colour(&inside) {
                        return None;
                    }
                }
                '0' | '#' | '?' => {
                    push_literal(&mut literal, &mut tokens);
                    tokens.push(Token::Digit(character));
                }
                '.' => {
                    push_literal(&mut literal, &mut tokens);
                    tokens.push(Token::Point);
                }
                ',' => {
                    push_literal(&mut literal, &mut tokens);
                    tokens.push(Token::Comma);
                }
                '%' => {
                    push_literal(&mut literal, &mut tokens);
                    tokens.push(Token::Percent);
                }
                'E' | 'e' => {
                    let sign = characters.peek().copied();
                    if sign == Some('+') || sign == Some('-') {
                        characters.next();
                        let mut digits = 0;
                        while characters.peek() == Some(&'0') {
                            characters.next();
                            digits += 1;
                        }
                        push_literal(&mut literal, &mut tokens);
                        tokens.push(Token::Exponent(digits, sign == Some('+')));
                    } else {
                        literal.push(character);
                    }
                }
                '@' => {
                    push_literal(&mut literal, &mut tokens);
                    tokens.push(Token::Text);
                }
                // A date, a time or a fraction, none of which is read here.
                'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' | '/' => return None,
                other => literal.push(other),
            }
        }
        push_literal(&mut literal, &mut tokens);
        Some(Self { tokens })
    }

    /// Writes a number the way the pattern says.
    fn write(&self, value: f64) -> String {
        let is_digit = |token: &Token| matches!(token, Token::Digit(_));
        let Some(first) = self.tokens.iter().position(is_digit) else {
            // No place for a digit: the words alone, with the number as it is
            // where the text placeholder stands — a section of words and
            // nothing else says those words for any number, which is what
            // `"nil"` for nought means.
            let mut out = String::new();
            for token in &self.tokens {
                match token {
                    Token::Literal(text) => out.push_str(text),
                    Token::Text => out.push_str(&general(value)),
                    _ => {}
                }
            }
            return out;
        };
        let last = self.tokens.iter().rposition(is_digit).unwrap_or(first);
        let point = self.tokens[first..=last]
            .iter()
            .position(|token| *token == Token::Point)
            .map(|at| at + first);
        let exponent = self.tokens.iter().position(|token| matches!(token, Token::Exponent(..)));
        let percent = self.tokens.contains(&Token::Percent);

        // The digit places before the point and after it; the commas among the
        // digits, which group the thousands; and the commas after the last
        // digit, which divide by a thousand each.
        let whole = &self.tokens[first..point.unwrap_or(last + 1)];
        let fraction: &[Token] = match point {
            Some(point) => &self.tokens[point + 1..=last],
            None => &[],
        };
        let count = |tokens: &[Token], wanted: &[char]| {
            tokens
                .iter()
                .filter(|token| matches!(token, Token::Digit(c) if wanted.contains(c)))
                .count()
        };
        let grouped = whole.contains(&Token::Comma);
        let scaled_down =
            self.tokens[last + 1..].iter().take_while(|token| **token == Token::Comma).count();

        let mut number = value;
        if percent {
            number *= 100.0;
        }
        for _ in 0..scaled_down {
            number /= 1000.0;
        }

        let (body, power) = match exponent {
            Some(at) => {
                let (mantissa, power) = split_exponent(number);
                let Token::Exponent(digits, plus) = &self.tokens[at] else { unreachable!() };
                let sign = if power < 0 {
                    "-"
                } else if *plus {
                    "+"
                } else {
                    ""
                };
                (mantissa, format!("E{sign}{:0width$}", power.abs(), width = *digits))
            }
            None => (number, String::new()),
        };
        let digits = fixed(
            body,
            count(fraction, &['0', '#', '?']),
            count(fraction, &['0']),
            count(whole, &['0']),
            grouped,
        );

        // The words before the first digit and after the last go round the
        // number; what is among the digits has been read already.
        let mut out = String::new();
        let words = |tokens: &[Token], out: &mut String| {
            for token in tokens {
                match token {
                    Token::Literal(text) => out.push_str(text),
                    Token::Percent => out.push('%'),
                    Token::Text => out.push_str(&general(value)),
                    _ => {}
                }
            }
        };
        words(&self.tokens[..first], &mut out);
        out.push_str(&digits);
        out.push_str(&power);
        let after = exponent.map_or(last + 1, |at| (at + 1).max(last + 1));
        words(&self.tokens[after..], &mut out);
        // A percent sign that stood among the digits is written after them.
        if percent
            && !self.tokens[..first]
                .iter()
                .chain(&self.tokens[after..])
                .any(|token| *token == Token::Percent)
        {
            out.push('%');
        }
        out
    }
}

/// A number split into a mantissa in `[1, 10)` and a power of ten.
fn split_exponent(value: f64) -> (f64, i32) {
    if value == 0.0 || !value.is_finite() {
        return (0.0, 0);
    }
    let power = value.abs().log10().floor() as i32;
    (value / 10f64.powi(power), power)
}

/// The digits of a number: rounded to so many places, with at least so many
/// of them written, at least so many before the point, and grouped in
/// thousands or not.
fn fixed(value: f64, places: usize, forced: usize, whole_forced: usize, grouped: bool) -> String {
    // Rounded half away from nought, as a spreadsheet rounds, rather than
    // half to even as the formatter would.
    let factor = 10f64.powi(places as i32);
    let value = (value * factor).round() / factor;
    let rounded = format!("{value:.places$}");
    let (whole, fraction) = rounded.split_once('.').unwrap_or((&rounded, ""));
    let (negative, whole) = match whole.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, whole),
    };

    // Trailing noughts past the forced places are not written: `0.##` writes
    // a half as `0.5` and one as `1`.
    let mut fraction = fraction.to_owned();
    while fraction.len() > forced && fraction.ends_with('0') {
        fraction.pop();
    }

    let mut whole = whole.trim_start_matches('0').to_owned();
    while whole.len() < whole_forced {
        whole.insert(0, '0');
    }
    if whole.is_empty() && fraction.is_empty() {
        whole.push('0');
    }
    if grouped {
        let mut with_commas = String::with_capacity(whole.len() + whole.len() / 3);
        for (index, character) in whole.chars().enumerate() {
            if index > 0 && (whole.len() - index) % 3 == 0 {
                with_commas.push(',');
            }
            with_commas.push(character);
        }
        whole = with_commas;
    }

    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&whole);
    if !fraction.is_empty() {
        out.push('.');
        out.push_str(&fraction);
    }
    out
}

/// Whether what a bracket holds is one of the colours a code may name.
fn is_a_colour(inside: &str) -> bool {
    let lower = inside.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "black" | "blue" | "cyan" | "green" | "magenta" | "red" | "white" | "yellow"
    ) || (lower.starts_with("color") && lower[5..].chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_writes_the_number_as_it_is() {
        assert_eq!(format("General", 1234.0), "1234");
        assert_eq!(format("General", 0.5), "0.5");
        assert_eq!(format("General", -3.25), "-3.25");
        assert_eq!(format("", 7.0), "7");
        assert_eq!(format("General", 1.0 / 3.0), "0.3333333333");
    }

    #[test]
    fn a_thousands_code_groups_the_digits() {
        assert_eq!(format("#,##0", 1234567.0), "1,234,567");
        assert_eq!(format("#,##0.00", 1234.5), "1,234.50");
        assert_eq!(format("#,##0", 12.0), "12");
    }

    #[test]
    fn forced_and_optional_places() {
        assert_eq!(format("0.00", 2.0), "2.00");
        assert_eq!(format("0.##", 2.5), "2.5");
        assert_eq!(format("0.##", 2.0), "2");
        assert_eq!(format("000", 7.0), "007");
        assert_eq!(format("#", 0.0), "0");
    }

    #[test]
    fn money_carries_its_sign_and_its_words() {
        assert_eq!(format("\"$\"#,##0.00", 1234.5), "$1,234.50");
        assert_eq!(format("$#,##0", 1234.5), "$1,235");
        assert_eq!(format("#,##0 \"€\"", 1000.0), "1,000 €");
        assert_eq!(format("\"$\"#,##0.00", -12.0), "-$12.00");
    }

    #[test]
    fn a_percent_is_a_hundred_times_the_share() {
        assert_eq!(format("0%", 0.25), "25%");
        assert_eq!(format("0.0%", 0.256), "25.6%");
    }

    #[test]
    fn a_negative_section_says_how_the_sign_is_shown() {
        assert_eq!(format("#,##0;(#,##0)", -1234.0), "(1,234)");
        assert_eq!(format("#,##0;(#,##0)", 1234.0), "1,234");
        assert_eq!(format("0;-0;\"nil\"", 0.0), "nil");
        assert_eq!(format("0;[Red]-0", -5.0), "-5");
    }

    #[test]
    fn a_comma_after_the_digits_divides_by_a_thousand() {
        assert_eq!(format("#,##0,", 1234567.0), "1,235");
        assert_eq!(format("0.0,,\"M\"", 2500000.0), "2.5M");
    }

    #[test]
    fn scientific_notation() {
        assert_eq!(format("0.00E+00", 12345.0), "1.23E+04");
        assert_eq!(format("0.0E-0", 0.00123), "1.2E-3");
    }

    #[test]
    fn the_padding_underscore_is_a_space_and_the_star_is_nothing() {
        assert_eq!(format("_(\"$\"* #,##0_)", 12.0), " $12 ");
    }

    #[test]
    fn a_date_is_more_than_is_read_and_comes_out_as_general() {
        assert_eq!(format("dd/mm/yyyy", 45000.0), "45000");
        assert_eq!(format("[>100]0;0.0", 5.0), "5");
    }

    #[test]
    fn text_in_a_number_section_stands_where_the_at_sign_is() {
        assert_eq!(format("\"Total: \"@", 12.5), "Total: 12.5");
    }
}

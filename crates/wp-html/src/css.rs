//! As much of CSS as a document needs: what a `style` attribute says, what
//! a `<style>` block says about a tag or a class, and what Word's `@list`
//! rules say about a list.
//!
//! Not a CSS engine. A page from Word puts the formatting in three places —
//! the class rules in the head, the `style` attribute on each element, and
//! the tag itself — and the reader has to fold them together in that order
//! of strength. That is what [`Sheet::declarations_for`] does; what the
//! declarations mean is the reader's business.

/// A property and its value, as written: `("font-size", "14.0pt")`.
pub type Declaration = (String, String);

/// One rule of a `<style>` block: what it selects, and what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// The tag it names, if it names one: `p` in `p.MsoNormal`.
    pub tag: Option<String>,
    /// The class it names, if it names one.
    pub class: Option<String>,
    pub declarations: Vec<Declaration>,
}

/// What Word says about one level of one list: `@list l0:level1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListLevel {
    pub list: String,
    pub level: u8,
    pub bullet: bool,
}

/// Everything the `<style>` blocks of a page said.
#[derive(Clone, Debug, Default)]
pub struct Sheet {
    rules: Vec<Rule>,
    pub lists: Vec<ListLevel>,
    /// The `@page` rules, by the name after `@page` — Word's section names,
    /// `WordSection1` — or none.
    pub pages: Vec<(String, Vec<Declaration>)>,
    /// The `@font-face` rules: what the page says about each of its fonts.
    pub fonts: Vec<Vec<Declaration>>,
}

impl Sheet {
    /// Reads a `<style>` block into the sheet, after what it already holds.
    pub fn read(&mut self, text: &str) {
        // Comments first, then rule after rule: selectors, a brace, the
        // declarations, a brace.
        let text = strip_comments(text);
        let mut rest = text.as_str();
        while let Some(open) = rest.find('{') {
            let selectors = rest[..open].trim().to_owned();
            let Some(close) = rest[open..].find('}') else { break };
            let body = &rest[open + 1..open + close];
            rest = &rest[open + close + 1..];

            if let Some(list) = selectors.strip_prefix("@list ") {
                self.read_list_rule(list.trim(), body);
                continue;
            }
            if let Some(name) = selectors.strip_prefix("@page") {
                // `@page:first` and the like are about pages of a kind, which
                // a section does not describe.
                let name = name.trim();
                if !name.contains(':') {
                    self.pages.push((name.to_owned(), parse_declarations(body)));
                }
                continue;
            }
            if selectors == "@font-face" {
                self.fonts.push(parse_declarations(body));
                continue;
            }
            if selectors.starts_with('@') {
                continue;
            }
            let declarations = parse_declarations(body);
            for selector in selectors.split(',') {
                let selector = selector.trim();
                // Only the last simple selector: `div.Section1 p` is about
                // paragraphs, near enough.
                let Some(last) = selector.split_whitespace().last() else { continue };
                let (tag, class) = match last.split_once('.') {
                    Some((tag, class)) => (
                        if tag.is_empty() { None } else { Some(tag.to_ascii_lowercase()) },
                        Some(class.to_owned()),
                    ),
                    None => (Some(last.to_ascii_lowercase()), None),
                };
                // Pseudo-classes and ids are not something a document has.
                if last.contains(':') || last.contains('#') || last.contains('[') {
                    continue;
                }
                self.rules.push(Rule { tag, class, declarations: declarations.clone() });
            }
        }
    }

    /// `@list l0:level1 {mso-level-number-format:bullet; ...}`.
    fn read_list_rule(&mut self, selector: &str, body: &str) {
        let Some((list, level)) = selector.split_once(':') else { return };
        let Some(level) = level.strip_prefix("level").and_then(|n| n.parse::<u8>().ok()) else {
            return;
        };
        let declarations = parse_declarations(body);
        let bullet = declarations
            .iter()
            .any(|(name, value)| name == "mso-level-number-format" && value == "bullet");
        self.lists.push(ListLevel { list: list.to_owned(), level, bullet });
    }

    /// What the sheet says about an element: the rules for its tag, then the
    /// rules for its classes, in the order written, so that a later rule
    /// outranks an earlier one and a class outranks a tag.
    #[must_use]
    pub fn declarations_for(&self, tag: &str, classes: &[String]) -> Vec<Declaration> {
        let mut out = Vec::new();
        for rule in &self.rules {
            if rule.class.is_none() && rule.tag.as_deref() == Some(tag) {
                out.extend(rule.declarations.iter().cloned());
            }
        }
        for rule in &self.rules {
            let Some(class) = &rule.class else { continue };
            if !classes.iter().any(|held| held == class) {
                continue;
            }
            if rule.tag.as_deref().is_none_or(|wanted| wanted == tag) {
                out.extend(rule.declarations.iter().cloned());
            }
        }
        out
    }

    /// Every rule for a tag or a class, in the order written.
    #[must_use]
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// What the `@page` rule of that name says, over what the one with no
    /// name says.
    #[must_use]
    pub fn page(&self, name: &str) -> Vec<Declaration> {
        let mut out: Vec<Declaration> = self
            .pages
            .iter()
            .filter(|(held, _)| held.is_empty())
            .flat_map(|(_, declarations)| declarations.iter().cloned())
            .collect();
        if !name.is_empty() {
            out.extend(
                self.pages
                    .iter()
                    .filter(|(held, _)| held.eq_ignore_ascii_case(name))
                    .flat_map(|(_, declarations)| declarations.iter().cloned()),
            );
        }
        out
    }

    /// Whether a list is bulleted at a level, as far as the sheet says.
    #[must_use]
    pub fn list_is_bulleted(&self, list: &str, level: u8) -> Option<bool> {
        self.lists
            .iter()
            .find(|held| held.list == list && held.level == level)
            .map(|held| held.bullet)
    }
}

/// `name: value; name: value` into pairs, names lower-cased and values as
/// written but trimmed.
#[must_use]
pub fn parse_declarations(text: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    for piece in split_declarations(text) {
        let Some((name, value)) = piece.split_once(':') else { continue };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().trim_end_matches("!important").trim().to_owned();
        if !name.is_empty() && !value.is_empty() {
            out.push((name, value));
        }
    }
    out
}

/// Declarations are cut at semicolons — but not the ones inside quotes,
/// which a font name may hold.
fn split_declarations(text: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for character in text.chars() {
        match (quote, character) {
            (None, '"' | '\'') => {
                quote = Some(character);
                current.push(character);
            }
            (Some(open), c) if c == open => {
                quote = None;
                current.push(c);
            }
            (None, ';') => pieces.push(core::mem::take(&mut current)),
            (_, c) => current.push(c),
        }
    }
    pieces.push(current);
    pieces
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    // Word wraps its style block in an HTML comment, which is not CSS.
    out.replace("<!--", " ").replace("-->", " ")
}

/// A length as CSS writes one, in twentieths of a point: `14.0pt`, `.5in`,
/// `2cm`, `12px`, or a bare number of pixels.
#[must_use]
pub fn twips(value: &str) -> Option<i32> {
    let value = value.trim();
    let (number, unit) = match value.find(|c: char| c.is_ascii_alphabetic() || c == '%') {
        Some(at) => (&value[..at], &value[at..]),
        None => (value, "px"),
    };
    let number: f32 = number.trim().parse().ok()?;
    let twips = match unit.trim() {
        "pt" => number * 20.0,
        "in" => number * 1440.0,
        "cm" => number * 566.93,
        "mm" => number * 56.693,
        "px" => number * 15.0,
        "pc" => number * 240.0,
        "em" => number * 220.0,
        _ => return None,
    };
    Some(twips.round() as i32)
}

/// A colour as CSS writes one, as six hex digits: `red`, `#c00`, `#cc0000`,
/// `rgb(204,0,0)`, `windowtext`.
#[must_use]
pub fn colour(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        return match hex.len() {
            6 if hex.chars().all(|c| c.is_ascii_hexdigit()) => Some(hex.to_uppercase()),
            3 if hex.chars().all(|c| c.is_ascii_hexdigit()) => {
                Some(hex.chars().flat_map(|c| [c, c]).collect::<String>().to_uppercase())
            }
            _ => None,
        };
    }
    if let Some(inside) = value.strip_prefix("rgb(").and_then(|rest| rest.strip_suffix(')')) {
        let parts: Vec<u8> =
            inside.split(',').filter_map(|p| p.trim().parse::<u8>().ok()).collect();
        if parts.len() == 3 {
            return Some(format!("{:02X}{:02X}{:02X}", parts[0], parts[1], parts[2]));
        }
        return None;
    }
    let named = match value.as_str() {
        "black" | "windowtext" => "000000",
        "white" | "window" => "FFFFFF",
        "red" => "FF0000",
        "green" => "008000",
        "lime" => "00FF00",
        "blue" => "0000FF",
        "yellow" => "FFFF00",
        "cyan" | "aqua" => "00FFFF",
        "magenta" | "fuchsia" => "FF00FF",
        "gray" | "grey" => "808080",
        "silver" => "C0C0C0",
        "maroon" => "800000",
        "navy" => "000080",
        "olive" => "808000",
        "purple" => "800080",
        "teal" => "008080",
        "orange" => "FFA500",
        _ => return None,
    };
    Some(named.to_owned())
}

/// A value with the quotes round it taken off: `"Heading 1 Char"`.
#[must_use]
pub fn unquote(value: &str) -> String {
    value.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_owned()
}

/// The first family of a `font-family` list, without its quotes: `"Times
/// New Roman",serif` is Times New Roman.
#[must_use]
pub fn font_family(value: &str) -> Option<String> {
    let first = split_declarations(&value.replace(',', ";")).into_iter().next()?;
    let name = first.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if name.is_empty() || matches!(name, "serif" | "sans-serif" | "monospace" | "inherit") {
        return None;
    }
    Some(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_block_is_rules_for_tags_and_classes() {
        let mut sheet = Sheet::default();
        sheet.read(
            "<!-- /* Font Definitions */ p.MsoNormal, li.MsoNormal {margin:0in; font-size:11.0pt;} h1 {font-size:16.0pt} .Big {color:red} @list l0:level1 {mso-level-number-format:bullet;} @list l1:level1 {mso-level-text:\"%1.\";} -->",
        );
        let normal = sheet.declarations_for("p", &["MsoNormal".to_owned()]);
        assert_eq!(
            normal,
            vec![
                ("margin".to_owned(), "0in".to_owned()),
                ("font-size".to_owned(), "11.0pt".to_owned())
            ]
        );
        assert!(
            sheet.declarations_for("div", &["MsoNormal".to_owned()]).is_empty(),
            "the rule was about paragraphs"
        );
        assert_eq!(
            sheet.declarations_for("h1", &[]),
            vec![("font-size".to_owned(), "16.0pt".to_owned())]
        );
        assert_eq!(
            sheet.declarations_for("span", &["Big".to_owned()]),
            vec![("color".to_owned(), "red".to_owned())]
        );
        assert_eq!(sheet.list_is_bulleted("l0", 1), Some(true));
        assert_eq!(sheet.list_is_bulleted("l1", 1), Some(false));
        assert_eq!(sheet.list_is_bulleted("l2", 1), None);
    }

    #[test]
    fn declarations_keep_their_quoted_semicolons() {
        let declarations =
            parse_declarations("font-family:\"Times New Roman\",serif;font-size:14.0pt");
        assert_eq!(declarations.len(), 2);
        assert_eq!(font_family(&declarations[0].1).as_deref(), Some("Times New Roman"));
        assert_eq!(font_family("serif"), None);
        assert_eq!(font_family("Arial, sans-serif").as_deref(), Some("Arial"));
    }

    #[test]
    fn lengths_and_colours_are_read() {
        assert_eq!(twips("14.0pt"), Some(280));
        assert_eq!(twips(".5in"), Some(720));
        assert_eq!(twips("1cm"), Some(567));
        assert_eq!(twips("16px"), Some(240));
        assert_eq!(twips("12"), Some(180));
        assert_eq!(twips("auto"), None);
        assert_eq!(colour("red").as_deref(), Some("FF0000"));
        assert_eq!(colour("#c00").as_deref(), Some("CC0000"));
        assert_eq!(colour("#1F4E79").as_deref(), Some("1F4E79"));
        assert_eq!(colour("rgb(0, 112, 192)").as_deref(), Some("0070C0"));
        assert_eq!(colour("windowtext").as_deref(), Some("000000"));
        assert_eq!(colour("transparent"), None);
    }
}

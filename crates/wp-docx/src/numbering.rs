//! Lists: what marks a paragraph and how it is counted.
//!
//! # How a list is put together in the format
//!
//! A paragraph does not say "bullet". It names a `w:numId` and a level, and
//! that identifier points at a numbering definition in `word/numbering.xml`,
//! which points in turn at an abstract definition holding one description per
//! level. Two indirections, because a document can have many lists sharing one
//! set of level definitions, and because a single list can be restarted without
//! redefining it.
//!
//! # Why bullets arrive as private-use characters
//!
//! Word writes a bullet as U+F0B7 in the Symbol font, not as U+2022. That
//! character means nothing outside Symbol, and Symbol is not on every machine.
//! So the well-known ones are mapped back to the real characters they stand
//! for, which any ordinary font can draw.

use std::collections::HashMap;

use wp_xml::tree::Element;

use crate::read::{value, W};

/// How the numbers of a level are written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NumberFormat {
    #[default]
    Decimal,
    /// 01, 02, 03 — decimal with a leading zero below ten.
    DecimalZero,
    LowerLetter,
    UpperLetter,
    LowerRoman,
    UpperRoman,
    /// A mark rather than a count.
    Bullet,
    /// Counted, but nothing is written.
    None,
    /// A format this program does not produce, kept as written.
    Other(String),
}

impl NumberFormat {
    #[must_use]
    pub fn from_attribute(text: &str) -> Self {
        match text {
            "decimal" => Self::Decimal,
            "decimalZero" => Self::DecimalZero,
            "lowerLetter" => Self::LowerLetter,
            "upperLetter" => Self::UpperLetter,
            "lowerRoman" => Self::LowerRoman,
            "upperRoman" => Self::UpperRoman,
            "bullet" => Self::Bullet,
            "none" => Self::None,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Writes a counter in this format.
    #[must_use]
    pub fn render(&self, value: i32) -> String {
        match self {
            Self::Decimal => value.to_string(),
            Self::DecimalZero => {
                if (0..10).contains(&value) {
                    format!("0{value}")
                } else {
                    value.to_string()
                }
            }
            Self::LowerLetter => letters(value, b'a'),
            Self::UpperLetter => letters(value, b'A'),
            Self::LowerRoman => roman(value).to_lowercase(),
            Self::UpperRoman => roman(value),
            // A bullet is not counted, and "none" is counted but not written.
            Self::Bullet | Self::None | Self::Other(_) => String::new(),
        }
    }
}

/// a, b, … z, aa, ab — the spreadsheet-column scheme the format uses.
fn letters(value: i32, first: u8) -> String {
    if value <= 0 {
        return String::new();
    }
    let mut remaining = value as u32;
    let mut out = Vec::new();
    while remaining > 0 {
        let index = (remaining - 1) % 26;
        out.push(first + index as u8);
        remaining = (remaining - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Roman numerals, in upper case.
fn roman(value: i32) -> String {
    const TABLE: &[(i32, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];

    if value <= 0 {
        return String::new();
    }
    let mut remaining = value;
    let mut out = String::new();
    for (amount, numeral) in TABLE {
        while remaining >= *amount {
            out.push_str(numeral);
            remaining -= amount;
        }
    }
    out
}

/// One level of a list definition.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Level {
    /// What the level counts from.
    pub start: i32,
    pub format: NumberFormat,
    /// The template, such as `%1.` or a single bullet character.
    pub text: String,
    /// Indents in twentieths of a point, when the level sets them.
    pub indent_start: Option<i32>,
    pub indent_hanging: Option<i32>,
    /// The font the mark is written in, which is how a bullet is identified.
    pub font: Option<String>,
}

impl Level {
    /// The mark a bullet level shows, as a character any font can draw.
    #[must_use]
    pub fn bullet(&self) -> char {
        let character = self.text.chars().next().unwrap_or('\u{2022}');
        map_symbol(character, self.font.as_deref())
    }
}

/// Turns a symbol-font character into the character it stands for.
///
/// The mappings are the marks Word offers in its bullet gallery. Anything else
/// in the private use area has no meaning without the font it was written for,
/// so it becomes an ordinary bullet rather than a missing-glyph box.
fn map_symbol(character: char, font: Option<&str>) -> char {
    let symbolic = matches!(
        font.map(str::to_ascii_lowercase).as_deref(),
        Some("symbol" | "wingdings" | "wingdings 2" | "wingdings 3" | "webdings")
    );

    match character {
        '\u{F0B7}' | '\u{F0A7}' if symbolic => {
            if character == '\u{F0B7}' {
                '\u{2022}'
            } else {
                '\u{25AA}'
            }
        }
        '\u{F0FC}' if symbolic => '\u{2713}',
        '\u{F0D8}' | '\u{F0E0}' if symbolic => '\u{27A2}',
        '\u{F075}' | '\u{F076}' if symbolic => '\u{25C6}',
        '\u{F06E}' if symbolic => '\u{25A0}',
        'o' => '\u{25E6}',
        // Anything left in the private use area cannot be drawn meaningfully.
        '\u{E000}'..='\u{F8FF}' => '\u{2022}',
        other => other,
    }
}

/// One list definition: its levels, in order.
#[derive(Clone, Debug, Default)]
struct AbstractList {
    levels: Vec<Level>,
}

/// Everything `word/numbering.xml` says.
#[derive(Clone, Debug, Default)]
pub struct Numbering {
    /// What each `w:numId` resolves to, already followed through to its levels.
    lists: HashMap<i32, AbstractList>,
}

impl Numbering {
    /// Reads a numbering part.
    #[must_use]
    pub fn parse(root: &Element) -> Self {
        let mut abstracts: HashMap<i32, AbstractList> = HashMap::new();

        for definition in root.children_named(Some(W), "abstractNum") {
            let Some(id) = numeric_attribute(definition, "abstractNumId") else {
                continue;
            };
            abstracts.insert(id, read_levels(definition));
        }

        let mut lists = HashMap::new();
        for entry in root.children_named(Some(W), "num") {
            let Some(id) = numeric_attribute(entry, "numId") else { continue };
            let Some(target) = entry
                .child(Some(W), "abstractNumId")
                .and_then(value)
                .and_then(|text| text.parse::<i32>().ok())
            else {
                continue;
            };
            let Some(mut list) = abstracts.get(&target).cloned() else { continue };

            // A definition may override individual levels without redefining
            // the whole list — that is how "restart numbering here" is written.
            for override_element in entry.children_named(Some(W), "lvlOverride") {
                let Some(index) = numeric_attribute(override_element, "ilvl") else {
                    continue;
                };
                let index = index.max(0) as usize;
                if let Some(level_element) = override_element.child(Some(W), "lvl") {
                    let level = read_level(level_element);
                    if index < list.levels.len() {
                        list.levels[index] = level;
                    }
                } else if let Some(start) = override_element
                    .child(Some(W), "startOverride")
                    .and_then(value)
                    .and_then(|text| text.parse::<i32>().ok())
                {
                    if let Some(level) = list.levels.get_mut(index) {
                        level.start = start;
                    }
                }
            }

            lists.insert(id, list);
        }

        Self { lists }
    }

    /// The definition of one level of one list.
    #[must_use]
    pub fn level(&self, num_id: i32, level: u8) -> Option<&Level> {
        self.lists.get(&num_id)?.levels.get(usize::from(level))
    }

    /// Whether anything was read at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lists.is_empty()
    }
}

fn numeric_attribute(element: &Element, name: &str) -> Option<i32> {
    element.attribute(Some(W), name)?.parse().ok()
}

fn read_levels(definition: &Element) -> AbstractList {
    let mut levels = Vec::new();
    for level in definition.children_named(Some(W), "lvl") {
        let index = numeric_attribute(level, "ilvl").unwrap_or(levels.len() as i32).max(0) as usize;
        // Levels are addressed by index, so a gap has to be filled rather than
        // silently shifting everything after it up one.
        while levels.len() <= index {
            levels.push(Level::default());
        }
        levels[index] = read_level(level);
    }
    AbstractList { levels }
}

fn read_level(level: &Element) -> Level {
    let start = level
        .child(Some(W), "start")
        .and_then(value)
        .and_then(|text| text.parse().ok())
        .unwrap_or(1);
    let format = level
        .child(Some(W), "numFmt")
        .and_then(value)
        .map(NumberFormat::from_attribute)
        .unwrap_or_default();
    let text = level.child(Some(W), "lvlText").and_then(value).unwrap_or_default().to_owned();

    let font = level
        .child(Some(W), "rPr")
        .and_then(|properties| properties.child(Some(W), "rFonts"))
        .and_then(|fonts| {
            fonts.attribute(Some(W), "ascii").or_else(|| fonts.attribute(Some(W), "hAnsi"))
        })
        .map(str::to_owned);

    let indent =
        level.child(Some(W), "pPr").and_then(|properties| properties.child(Some(W), "ind"));
    let indent_start = indent.and_then(|element| {
        element
            .attribute(Some(W), "start")
            .or_else(|| element.attribute(Some(W), "left"))
            .and_then(|text| text.parse().ok())
    });
    let indent_hanging = indent
        .and_then(|element| element.attribute(Some(W), "hanging"))
        .and_then(|text| text.parse().ok());

    Level { start, format, text, indent_start, indent_hanging, font }
}

/// Where each list has got to, as a document is laid out.
///
/// Counting has to be done in reading order and cannot be worked out from a
/// paragraph alone: the third item of a list is only the third because of the
/// two before it. Starting a level again resets everything under it, which is
/// what makes a sub-list begin at one under each new parent item.
#[derive(Clone, Debug, Default)]
pub struct ListCounters {
    /// Counts per list, one entry per level.
    counts: HashMap<i32, Vec<i32>>,
}

impl ListCounters {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances a list by one item and returns the mark to draw.
    ///
    /// `None` when the list is not defined, which is what happens to a document
    /// referring to a numbering part it does not carry.
    pub fn advance(
        &mut self,
        numbering: &Numbering,
        num_id: i32,
        level_index: u8,
    ) -> Option<String> {
        let level = numbering.level(num_id, level_index)?;
        let depth = usize::from(level_index);

        let counts = self.counts.entry(num_id).or_default();
        while counts.len() <= depth {
            let index = counts.len();
            let start = numbering.level(num_id, index as u8).map_or(1, |level| level.start);
            counts.push(start - 1);
        }

        counts[depth] += 1;
        // A new item at this level starts everything under it again.
        for (deeper, count) in counts.iter_mut().enumerate().skip(depth + 1) {
            let start = numbering.level(num_id, deeper as u8).map_or(1, |level| level.start);
            *count = start - 1;
        }

        if level.format == NumberFormat::Bullet {
            return Some(level.bullet().to_string());
        }
        if level.format == NumberFormat::None {
            return Some(String::new());
        }

        // The template names the levels it wants by number: "%1.%2." is the
        // parent's count, a dot, this level's count, a dot.
        let mut out = String::with_capacity(level.text.len() + 4);
        let mut characters = level.text.chars().peekable();
        while let Some(character) = characters.next() {
            if character != '%' {
                out.push(character);
                continue;
            }
            let Some(digit) = characters.peek().and_then(|next| next.to_digit(10)) else {
                out.push(character);
                continue;
            };
            characters.next();

            let wanted = digit as usize;
            if wanted == 0 || wanted > counts.len() {
                continue;
            }
            let format = numbering
                .level(num_id, (wanted - 1) as u8)
                .map_or(NumberFormat::Decimal, |level| level.format.clone());
            out.push_str(&format.render(counts[wanted - 1]));
        }

        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_xml::tree::XmlTree;

    fn numbering_from(inner: &str) -> Numbering {
        let source = format!("<w:numbering xmlns:w=\"{W}\">{inner}</w:numbering>");
        let tree = XmlTree::parse(&source).unwrap();
        Numbering::parse(&tree.root)
    }

    #[test]
    fn letters_run_past_the_alphabet() {
        assert_eq!(letters(1, b'a'), "a");
        assert_eq!(letters(26, b'a'), "z");
        assert_eq!(letters(27, b'a'), "aa");
        assert_eq!(letters(28, b'a'), "ab");
        assert_eq!(letters(52, b'A'), "AZ");
    }

    #[test]
    fn roman_numerals_are_written_the_usual_way() {
        assert_eq!(roman(1), "I");
        assert_eq!(roman(4), "IV");
        assert_eq!(roman(9), "IX");
        assert_eq!(roman(14), "XIV");
        assert_eq!(roman(1990), "MCMXC");
        assert_eq!(roman(2024), "MMXXIV");
    }

    #[test]
    fn a_count_of_nothing_writes_nothing() {
        assert_eq!(roman(0), "");
        assert_eq!(letters(0, b'a'), "");
    }

    #[test]
    fn a_symbol_font_bullet_becomes_a_real_bullet() {
        let level = Level {
            format: NumberFormat::Bullet,
            text: "\u{F0B7}".to_owned(),
            font: Some("Symbol".to_owned()),
            ..Level::default()
        };
        assert_eq!(level.bullet(), '\u{2022}');
    }

    #[test]
    fn an_unknown_private_use_mark_falls_back_to_a_bullet() {
        let level = Level {
            format: NumberFormat::Bullet,
            text: "\u{F0AB}".to_owned(),
            font: Some("Wingdings".to_owned()),
            ..Level::default()
        };
        assert_eq!(level.bullet(), '\u{2022}');
    }

    #[test]
    fn an_ordinary_character_is_left_alone() {
        let level = Level {
            format: NumberFormat::Bullet,
            text: "\u{2013}".to_owned(),
            font: None,
            ..Level::default()
        };
        assert_eq!(level.bullet(), '\u{2013}');
    }

    #[test]
    fn a_numbered_list_counts_up() {
        let numbering = numbering_from(
            r#"<w:abstractNum w:abstractNumId="0">
                 <w:lvl w:ilvl="0">
                   <w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/>
                 </w:lvl>
               </w:abstractNum>
               <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#,
        );

        let mut counters = ListCounters::new();
        assert_eq!(counters.advance(&numbering, 1, 0).as_deref(), Some("1."));
        assert_eq!(counters.advance(&numbering, 1, 0).as_deref(), Some("2."));
        assert_eq!(counters.advance(&numbering, 1, 0).as_deref(), Some("3."));
    }

    #[test]
    fn a_list_starts_where_it_says_it_does() {
        let numbering = numbering_from(
            r#"<w:abstractNum w:abstractNumId="0">
                 <w:lvl w:ilvl="0">
                   <w:start w:val="5"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1)"/>
                 </w:lvl>
               </w:abstractNum>
               <w:num w:numId="7"><w:abstractNumId w:val="0"/></w:num>"#,
        );

        let mut counters = ListCounters::new();
        assert_eq!(counters.advance(&numbering, 7, 0).as_deref(), Some("5)"));
        assert_eq!(counters.advance(&numbering, 7, 0).as_deref(), Some("6)"));
    }

    #[test]
    fn a_deeper_level_starts_again_under_each_parent() {
        let numbering = numbering_from(
            r#"<w:abstractNum w:abstractNumId="0">
                 <w:lvl w:ilvl="0">
                   <w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/>
                 </w:lvl>
                 <w:lvl w:ilvl="1">
                   <w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/>
                 </w:lvl>
               </w:abstractNum>
               <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#,
        );

        let mut counters = ListCounters::new();
        assert_eq!(counters.advance(&numbering, 1, 0).as_deref(), Some("1."));
        assert_eq!(counters.advance(&numbering, 1, 1).as_deref(), Some("a)"));
        assert_eq!(counters.advance(&numbering, 1, 1).as_deref(), Some("b)"));
        assert_eq!(counters.advance(&numbering, 1, 0).as_deref(), Some("2."));
        assert_eq!(
            counters.advance(&numbering, 1, 1).as_deref(),
            Some("a)"),
            "the sub-list starts again under the new item"
        );
    }

    #[test]
    fn a_template_can_name_more_than_one_level() {
        let numbering = numbering_from(
            r#"<w:abstractNum w:abstractNumId="0">
                 <w:lvl w:ilvl="0">
                   <w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1"/>
                 </w:lvl>
                 <w:lvl w:ilvl="1">
                   <w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1.%2"/>
                 </w:lvl>
               </w:abstractNum>
               <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#,
        );

        let mut counters = ListCounters::new();
        counters.advance(&numbering, 1, 0);
        counters.advance(&numbering, 1, 0);
        assert_eq!(counters.advance(&numbering, 1, 1).as_deref(), Some("2.1"));
        assert_eq!(counters.advance(&numbering, 1, 1).as_deref(), Some("2.2"));
    }

    #[test]
    fn two_lists_are_counted_separately() {
        let numbering = numbering_from(
            r#"<w:abstractNum w:abstractNumId="0">
                 <w:lvl w:ilvl="0">
                   <w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/>
                 </w:lvl>
               </w:abstractNum>
               <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
               <w:num w:numId="2"><w:abstractNumId w:val="0"/></w:num>"#,
        );

        let mut counters = ListCounters::new();
        assert_eq!(counters.advance(&numbering, 1, 0).as_deref(), Some("1."));
        assert_eq!(counters.advance(&numbering, 2, 0).as_deref(), Some("1."));
        assert_eq!(counters.advance(&numbering, 1, 0).as_deref(), Some("2."));
    }

    #[test]
    fn a_level_can_be_restarted_by_an_override() {
        let numbering = numbering_from(
            r#"<w:abstractNum w:abstractNumId="0">
                 <w:lvl w:ilvl="0">
                   <w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/>
                 </w:lvl>
               </w:abstractNum>
               <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
               <w:num w:numId="2">
                 <w:abstractNumId w:val="0"/>
                 <w:lvlOverride w:ilvl="0"><w:startOverride w:val="10"/></w:lvlOverride>
               </w:num>"#,
        );

        let mut counters = ListCounters::new();
        assert_eq!(counters.advance(&numbering, 1, 0).as_deref(), Some("1."));
        assert_eq!(counters.advance(&numbering, 2, 0).as_deref(), Some("10."));
    }

    #[test]
    fn a_list_nobody_defined_produces_no_mark() {
        let numbering = numbering_from("");
        let mut counters = ListCounters::new();
        assert_eq!(counters.advance(&numbering, 1, 0), None);
    }

    #[test]
    fn a_bullet_level_carries_its_indents() {
        let numbering = numbering_from(
            r#"<w:abstractNum w:abstractNumId="0">
                 <w:lvl w:ilvl="0">
                   <w:numFmt w:val="bullet"/><w:lvlText w:val="\u{F0B7}"/>
                   <w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr>
                 </w:lvl>
               </w:abstractNum>
               <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#,
        );

        let level = numbering.level(1, 0).expect("the level is defined");
        assert_eq!(level.indent_start, Some(720));
        assert_eq!(level.indent_hanging, Some(360));
    }
}

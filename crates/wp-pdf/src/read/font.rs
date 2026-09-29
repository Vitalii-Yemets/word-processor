//! A font as the content stream uses it: bytes in, characters and their
//! widths out, and what the font says about itself — its family, and
//! whether it is bold or italic — which is all a reflowed document keeps
//! of it.

use std::collections::HashMap;

use super::cmap::{glyph_char, private_use_bullet, BaseEncoding, CMap};
use super::file::File;
use super::object::{Dictionary, Lexer, Object};

/// One character as the font shows it.
#[derive(Clone, Debug)]
pub struct Shown {
    pub text: String,
    /// The advance in text space, for a font of size one.
    pub width: f64,
    /// Whether this is the single byte 32, to which word spacing applies.
    pub is_space_byte: bool,
}

/// What the font says of itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub fixed_pitch: bool,
    pub symbolic: bool,
}

/// A font, loaded once per resource.
#[derive(Clone, Debug)]
pub struct LoadedFont {
    composite: bool,
    /// For a composite font: bytes to character ids.
    encoding: CMap,
    to_unicode: Option<CMap>,
    /// For a simple font: the character of each code, by encoding.
    simple: Vec<Option<String>>,
    /// For a simple font: the width of each code.
    simple_widths: Vec<Option<f64>>,
    /// For a composite font: widths by character id, and the default.
    cid_widths: HashMap<u32, f64>,
    default_width: f64,
    missing_width: f64,
    /// A Type 3 font's own scale, applied to its widths.
    type3_scale: f64,
    /// How much bigger a Type 3 font's glyphs are than its size says:
    /// one for any font drawn in an em of its own.
    size_factor: f64,
    /// The embedded program's characters by glyph, for a composite font
    /// with no ToUnicode table.
    by_glyph: HashMap<u32, char>,
    pub style: Style,
}

impl LoadedFont {
    /// Loads a font dictionary.
    #[must_use]
    pub fn load(file: &File<'_>, dictionary: &Dictionary) -> Self {
        let subtype = file.get(dictionary, "Subtype").as_name().unwrap_or("").to_owned();
        let base_font = file.get(dictionary, "BaseFont").as_name().unwrap_or("").to_owned();
        let composite = subtype == "Type0";
        let descendant = if composite {
            file.get(dictionary, "DescendantFonts")
                .as_array()
                .and_then(|fonts| fonts.first())
                .and_then(|font| file.resolve(font).as_dictionary().cloned())
                .unwrap_or_default()
        } else {
            dictionary.clone()
        };
        let descriptor =
            file.get(&descendant, "FontDescriptor").as_dictionary().cloned().unwrap_or_default();
        let flags = file.get(&descriptor, "Flags").as_integer().unwrap_or(0);
        let mut style = style_of(&base_font, &descriptor, file, flags);
        let to_unicode = match file.get(dictionary, "ToUnicode") {
            Object::Stream(stream) => {
                let map = CMap::parse(&file.decode(&stream).0);
                map.has_text().then_some(map)
            }
            _ => None,
        };
        let missing_width =
            file.get(&descriptor, "MissingWidth").as_number().unwrap_or(0.0) / 1000.0;

        let mut font = Self {
            composite,
            encoding: CMap::default(),
            to_unicode,
            simple: Vec::new(),
            simple_widths: Vec::new(),
            cid_widths: HashMap::new(),
            default_width: 1.0,
            missing_width,
            type3_scale: 0.001,
            size_factor: 1.0,
            by_glyph: HashMap::new(),
            style: Style::default(),
        };
        if composite {
            font.encoding = match file.get(dictionary, "Encoding") {
                Object::Name(name) => CMap::predefined(&name),
                Object::Stream(stream) => {
                    let mut map = CMap::parse(&file.decode(&stream).0);
                    if map.is_empty() {
                        map = CMap::identity();
                    }
                    map
                }
                _ => CMap::identity(),
            };
            font.default_width = file.get(&descendant, "DW").as_number().unwrap_or(1000.0) / 1000.0;
            font.cid_widths = cid_widths(file, &file.get(&descendant, "W"));
            if font.to_unicode.is_none() {
                font.by_glyph = characters_by_glyph(file, &descriptor);
            }
        } else {
            let symbolic = flags & 4 != 0 && flags & 32 == 0;
            let is_type3 = subtype == "Type3";
            if is_type3 {
                if let Some(matrix) = file.get(dictionary, "FontMatrix").as_array() {
                    if let Some(scale) = matrix.first().and_then(|m| file.resolve(m).as_number()) {
                        font.type3_scale = scale;
                    }
                }
            }
            font.simple = simple_encoding(file, dictionary, &base_font, symbolic, is_type3);
            font.simple_widths =
                simple_widths(file, dictionary, &descriptor, &base_font, &font.simple);
            if is_type3 {
                for width in font.simple_widths.iter_mut().flatten() {
                    *width *= font.type3_scale * 1000.0;
                }
                font.size_factor = type3_size_factor(file, dictionary);
            }
            style.symbolic = symbolic;
        }
        font.style = style;
        font
    }

    /// The characters a string shows, in order.
    #[must_use]
    pub fn decode(&self, bytes: &[u8]) -> Vec<Shown> {
        let mut out = Vec::new();
        if self.composite {
            let mut at = 0;
            while at < bytes.len() {
                let (code, length) = self.encoding.next_code(&bytes[at..]);
                at += length;
                let cid = self.encoding.cid(code);
                let width = self.cid_widths.get(&cid).copied().unwrap_or(self.default_width);
                let text = self
                    .to_unicode
                    .as_ref()
                    .and_then(|map| map.text(code))
                    .or_else(|| self.encoding.text(code))
                    .or_else(|| self.by_glyph.get(&cid).map(|c| c.to_string()))
                    .unwrap_or_else(|| "\u{FFFD}".to_owned());
                out.push(Shown {
                    text: tidy(text, self.style.symbolic),
                    width,
                    is_space_byte: length == 1 && code == 32,
                });
            }
        } else {
            for &byte in bytes {
                let code = u32::from(byte);
                let width = self
                    .simple_widths
                    .get(usize::from(byte))
                    .copied()
                    .flatten()
                    .unwrap_or(self.missing_width);
                let text = self
                    .to_unicode
                    .as_ref()
                    .and_then(|map| map.text(code))
                    .or_else(|| self.simple.get(usize::from(byte)).cloned().flatten())
                    .unwrap_or_default();
                out.push(Shown {
                    text: tidy(text, self.style.symbolic),
                    width,
                    is_space_byte: byte == 32,
                });
            }
        }
        out
    }

    /// How much bigger the glyphs are than the size they are shown at.
    #[must_use]
    pub fn size_factor(&self) -> f64 {
        self.size_factor
    }

    #[must_use]
    pub fn is_vertical(&self) -> bool {
        self.composite && self.encoding.vertical
    }
}

/// Text as the document will hold it: a private-use bullet as the bullet
/// it stands for, a no-break space kept, nothing else changed.
fn tidy(text: String, symbolic: bool) -> String {
    if text.chars().any(|c| ('\u{E000}'..='\u{F8FF}').contains(&c)) {
        return text
            .chars()
            .map(|c| match private_use_bullet(c) {
                Some(bullet) => bullet,
                None if symbolic || c as u32 >= 0xF000 => {
                    // A symbolic font's private code is its Latin code
                    // moved up by 0xF000: take that as the character.
                    char::from_u32(c as u32 - 0xF000).filter(|c| c.is_ascii_graphic()).unwrap_or(c)
                }
                None => c,
            })
            .collect();
    }
    text
}

/// The `/W` array: `c [w1 w2 ...]` runs and `first last w` ranges.
fn cid_widths(file: &File<'_>, array: &Object) -> HashMap<u32, f64> {
    let mut widths = HashMap::new();
    let Some(items) = array.as_array() else { return widths };
    let items: Vec<Object> = items.iter().map(|item| file.resolve(item)).collect();
    let mut at = 0;
    while at < items.len() {
        let Some(first) = items[at].as_number() else {
            at += 1;
            continue;
        };
        match items.get(at + 1) {
            Some(Object::Array(run)) => {
                for (offset, width) in run.iter().enumerate() {
                    if let Some(width) = file.resolve(width).as_number() {
                        widths.insert(first as u32 + offset as u32, width / 1000.0);
                    }
                }
                at += 2;
            }
            Some(Object::Number(last)) => {
                let width = items.get(at + 2).and_then(Object::as_number).unwrap_or(1000.0);
                let (first, last) = (first as u32, *last as u32);
                if last >= first && last - first < 65536 {
                    for cid in first..=last {
                        widths.insert(cid, width / 1000.0);
                    }
                }
                at += 3;
            }
            _ => at += 1,
        }
    }
    widths
}

/// The character of each of a simple font's 256 codes.
fn simple_encoding(
    file: &File<'_>,
    dictionary: &Dictionary,
    base_font: &str,
    symbolic: bool,
    is_type3: bool,
) -> Vec<Option<String>> {
    let plain = plain_name(base_font);
    let mut base = if plain.starts_with("Symbol") {
        BaseEncoding::Symbol
    } else if symbolic && !is_type3 {
        BaseEncoding::Builtin
    } else {
        BaseEncoding::Standard
    };
    let mut differences: Vec<(u8, String)> = Vec::new();
    match file.get(dictionary, "Encoding") {
        Object::Name(name) => {
            if let Some(named) = BaseEncoding::named(&name) {
                base = named;
            }
        }
        Object::Dictionary(encoding) => {
            if let Some(named) =
                file.get(&encoding, "BaseEncoding").as_name().and_then(BaseEncoding::named)
            {
                base = named;
            } else if !symbolic {
                base = BaseEncoding::Standard;
            }
            if let Some(items) = file.get(&encoding, "Differences").as_array() {
                let mut code: i64 = 0;
                for item in items {
                    match file.resolve(item) {
                        Object::Number(number) => code = number as i64,
                        Object::Name(name) => {
                            if let Ok(byte) = u8::try_from(code) {
                                differences.push((byte, name));
                            }
                            code += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    let zapf = plain.contains("Dingbat");
    let mut table: Vec<Option<String>> = (0..=255u8)
        .map(|code| {
            if zapf {
                return dingbat(code).map(|c| c.to_string());
            }
            base.char_of(code).map(|c| c.to_string())
        })
        .collect();
    for (code, name) in differences {
        let named =
            glyph_char(&name).or_else(|| if is_type3 { type3_name_char(&name) } else { None });
        table[usize::from(code)] = named.map(|c| c.to_string()).or_else(|| {
            // A name that is not a character keeps the base's character.
            table[usize::from(code)].clone()
        });
    }
    table
}

/// The character a Type 3 font's glyph name means when it is not a name
/// of the glyph list: the code it stands for, which is how programs that
/// make fonts of their own name the glyphs — `a65` and `c65` in decimal,
/// `G41` and `g0041` in hex.
fn type3_name_char(name: &str) -> Option<char> {
    let digits = |text: &str, radix: u32| {
        (!text.is_empty() && text.chars().all(|c| c.is_digit(radix)))
            .then(|| u32::from_str_radix(text, radix).ok())
            .flatten()
    };
    let code = match (name.chars().next()?, name.len()) {
        ('a', 2..=4) | ('c' | 'C', 3..=4) => digits(&name[1..], 10),
        ('G', 3) | ('g', 5) => digits(&name[1..], 16),
        _ => None,
    }?;
    char::from_u32(code).filter(|c| !c.is_control())
}

/// How much bigger a Type 3 font's glyphs are than the size it is shown
/// at. A size scales the em; a Type 3 font's glyphs are drawn in a space
/// of their own that the font's matrix takes into the em — so a font drawn
/// in a thousand units to the em with a matrix of a thousandth is the size
/// it says, and one drawn in pixels with a matrix of one, as a bitmap font
/// is, is as many times bigger as its glyphs are pixels tall. How tall is
/// what the glyphs' procedures declare with `d1`, or else the font's box.
/// A factor near one is one: glyphs stand a little over or under an em.
fn type3_size_factor(file: &File<'_>, dictionary: &Dictionary) -> f64 {
    let matrix = file.get(dictionary, "FontMatrix");
    let scale = matrix
        .as_array()
        .and_then(|m| m.get(3))
        .and_then(|d| file.resolve(d).as_number())
        .map_or(0.001, f64::abs);
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    if let Some(procedures) = file.get(dictionary, "CharProcs").as_dictionary() {
        for value in procedures.values().take(256) {
            let Object::Stream(stream) = file.resolve(value) else { continue };
            let data = file.decode(&stream).0;
            let mut lexer = Lexer::new(&data);
            let mut numbers = Vec::new();
            while let Some(Object::Number(number)) = lexer.next_object() {
                numbers.push(number);
            }
            // `wx wy llx lly urx ury d1`: the operator ended the numbers.
            if numbers.len() == 6 {
                low = low.min(numbers[3]).min(numbers[5]);
                high = high.max(numbers[3]).max(numbers[5]);
            }
        }
    }
    if high <= low {
        if let Some(bounds) = file.get(dictionary, "FontBBox").as_array() {
            let values: Vec<f64> =
                bounds.iter().filter_map(|v| file.resolve(v).as_number()).collect();
            if values.len() == 4 {
                low = values[1].min(values[3]);
                high = values[1].max(values[3]);
            }
        }
    }
    if high <= low {
        return 1.0;
    }
    let factor = (high - low) * scale;
    if (0.5..=2.0).contains(&factor) {
        1.0
    } else {
        factor.clamp(0.01, 100.0)
    }
}

/// The few dingbats a document uses as bullets.
fn dingbat(code: u8) -> Option<char> {
    Some(match code {
        0x20 => ' ',
        0x33 => '\u{2713}',
        0x34 => '\u{2714}',
        0x48 => '\u{2605}',
        0x6C => '\u{25CF}',
        0x6D => '\u{274D}',
        0x6E => '\u{25A0}',
        0x6F => '\u{274F}',
        0x70 => '\u{2750}',
        0x71 => '\u{2751}',
        0x72 => '\u{2752}',
        0x73 => '\u{25B2}',
        0x74 => '\u{25BC}',
        0x75 => '\u{25C6}',
        0x76 => '\u{2756}',
        0x77 => '\u{25D7}',
        0xA8 => '\u{2767}',
        _ => return None,
    })
}

/// The width of each code, from the `/Widths` array, or the standard
/// fonts' known widths, or the embedded program.
fn simple_widths(
    file: &File<'_>,
    dictionary: &Dictionary,
    descriptor: &Dictionary,
    base_font: &str,
    characters: &[Option<String>],
) -> Vec<Option<f64>> {
    let mut widths: Vec<Option<f64>> = vec![None; 256];
    let first = file.get(dictionary, "FirstChar").as_integer().unwrap_or(0).clamp(0, 255) as usize;
    if let Some(items) = file.get(dictionary, "Widths").as_array() {
        if !items.is_empty() {
            for (offset, item) in items.iter().enumerate() {
                if let Some(slot) = widths.get_mut(first + offset) {
                    *slot = file.resolve(item).as_number().map(|w| w / 1000.0);
                }
            }
            return widths;
        }
    }
    // No widths: a standard font's, if it is one.
    let plain = plain_name(base_font);
    if let Some(table) = standard_widths(&plain) {
        for (code, slot) in widths.iter_mut().enumerate() {
            let c = characters.get(code).and_then(|c| c.as_ref()).and_then(|c| c.chars().next());
            *slot = Some(c.map_or(0.5, table));
        }
        return widths;
    }
    // The embedded program's own advances.
    if let Some(program) = program_of(file, descriptor) {
        if let Ok(font) = wp_font::Font::parse(&program) {
            let units = f64::from(font.units_per_em().max(1));
            for (code, slot) in widths.iter_mut().enumerate() {
                let c =
                    characters.get(code).and_then(|c| c.as_ref()).and_then(|c| c.chars().next());
                if let Some(glyph) = c.and_then(|c| font.glyph_for(c)) {
                    *slot = Some(f64::from(font.advance(glyph)) / units);
                }
            }
            return widths;
        }
    }
    widths.fill(Some(0.5));
    widths
}

/// The bytes of an embedded font program, whichever kind.
fn program_of(file: &File<'_>, descriptor: &Dictionary) -> Option<Vec<u8>> {
    for key in ["FontFile2", "FontFile3", "FontFile"] {
        if let Object::Stream(stream) = file.get(descriptor, key) {
            return Some(file.decode(&stream).0);
        }
    }
    None
}

/// A composite font with no ToUnicode table but an embedded TrueType
/// program: the program's own character map, turned round, says which
/// character each glyph — each character id, under the identity — draws.
fn characters_by_glyph(file: &File<'_>, descriptor: &Dictionary) -> HashMap<u32, char> {
    let mut by_glyph = HashMap::new();
    let Some(program) = program_of(file, descriptor) else { return by_glyph };
    if let Ok(font) = wp_font::Font::parse(&program) {
        for (c, glyph) in font.character_map().pairs() {
            by_glyph.entry(u32::from(glyph.0)).or_insert(c);
        }
    }
    by_glyph
}

/// The name without its subset tag.
fn plain_name(base_font: &str) -> String {
    let name = match base_font.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => base_font,
    };
    name.to_owned()
}

/// What the name and the descriptor say: the family, and bold and italic.
fn style_of(base_font: &str, descriptor: &Dictionary, file: &File<'_>, flags: i64) -> Style {
    let plain = plain_name(base_font);
    let (family, name_styles) = split_family(&plain);
    let lower = name_styles.to_ascii_lowercase();
    let weight = file.get(descriptor, "FontWeight").as_number().unwrap_or(0.0);
    let stem = file.get(descriptor, "StemV").as_number().unwrap_or(0.0);
    let italic_angle = file.get(descriptor, "ItalicAngle").as_number().unwrap_or(0.0);
    let bold = lower.contains("bold")
        || lower.contains("black")
        || lower.contains("heavy")
        || weight >= 600.0
        || flags & (1 << 18) != 0
        || (name_styles.is_empty() && weight == 0.0 && stem > 120.0);
    let italic = lower.contains("italic")
        || lower.contains("oblique")
        || flags & 64 != 0
        || italic_angle != 0.0;
    Style {
        family: word_family(&family),
        bold,
        italic,
        fixed_pitch: flags & 1 != 0 || family.to_ascii_lowercase().contains("courier"),
        symbolic: false,
    }
}

/// `Arial-BoldItalicMT` → (`Arial`, `BoldItalic`); `Arial,Bold` the same.
fn split_family(plain: &str) -> (String, String) {
    let plain = plain.trim_end_matches("-Identity-H");
    let (family, styles) = match plain.split_once(',') {
        Some((family, styles)) => (family.to_owned(), styles.to_owned()),
        None => match plain.split_once('-') {
            Some((family, styles)) => (family.to_owned(), styles.to_owned()),
            None => (plain.to_owned(), String::new()),
        },
    };
    let mut family = family;
    let mut styles = styles;
    // A style written onto the family without a dash: `ArialBold`.
    for word in ["BoldItalic", "BoldOblique", "Bold", "Italic", "Oblique"] {
        if family.len() > word.len() && family.ends_with(word) {
            let cut = family.len() - word.len();
            styles = format!("{word}{styles}");
            family.truncate(cut);
            break;
        }
    }
    for suffix in ["PSMT", "MT", "PS", "Regular", "Normal"] {
        if family.len() > suffix.len() + 2 && family.ends_with(suffix) {
            let cut = family.len() - suffix.len();
            family.truncate(cut);
            break;
        }
    }
    (family, styles)
}

/// The family as Word names it: the standard fonts, and the free fonts
/// cut to their measurements, by the fonts Word has; run-together names
/// with their spaces back.
fn word_family(family: &str) -> String {
    match family {
        "Helvetica" | "Arial" | "NimbusSans" | "NimbusSansL" | "LiberationSans"
        | "TeXGyreHeros" | "FreeSans" | "Arimo" => return "Arial".to_owned(),
        "Times" | "TimesNewRoman" | "Times New Roman" | "NimbusRoman" | "NimbusRomanNo9L"
        | "LiberationSerif" | "TeXGyreTermes" | "FreeSerif" | "Tinos" => {
            return "Times New Roman".to_owned()
        }
        "Courier" | "CourierNew" | "NimbusMono" | "NimbusMonoL" | "NimbusMonoPS"
        | "LiberationMono" | "TeXGyreCursor" | "FreeMono" | "Cousine" => {
            return "Courier New".to_owned()
        }
        "Symbol" | "StandardSymbolsPS" | "StandardSymL" => return "Symbol".to_owned(),
        "ZapfDingbats" | "Dingbats" => return "Wingdings".to_owned(),
        "Carlito" => return "Calibri".to_owned(),
        "Caladea" => return "Cambria".to_owned(),
        _ => {}
    }
    if family.contains(' ') {
        return family.to_owned();
    }
    // Names that are one word though their capitals say otherwise.
    let kept = ["DejaVu", "OpenType", "TrueType", "PT", "IBM", "STIX"];
    let mut out = String::new();
    let mut rest = family;
    let mut previous: Option<char> = None;
    while let Some(c) = rest.chars().next() {
        if let Some(word) = kept.iter().find(|word| rest.starts_with(*word)) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(word);
            rest = &rest[word.len()..];
            previous = word.chars().last();
            continue;
        }
        let breaks_word = previous.is_some_and(|p| {
            (c.is_uppercase() && (p.is_lowercase() || p.is_ascii_digit()))
                || (c.is_ascii_digit() && p.is_alphabetic())
        });
        if breaks_word {
            out.push(' ');
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
        previous = Some(c);
    }
    out
}

/// The widths of the standard fonts a file may use without carrying them.
fn standard_widths(plain: &str) -> Option<fn(char) -> f64> {
    let lower = plain.to_ascii_lowercase();
    if lower.starts_with("courier") {
        return Some(|_| 0.6);
    }
    if lower.starts_with("helvetica") || lower.starts_with("arial") {
        return Some(helvetica_width);
    }
    if lower.starts_with("times") {
        return Some(times_width);
    }
    if lower.starts_with("symbol") || lower.starts_with("zapf") {
        return Some(|_| 0.6);
    }
    None
}

fn helvetica_width(c: char) -> f64 {
    let thousandths: u32 = match c {
        ' ' | '!' | ',' | '.' | '/' | ':' | ';' | 'I' | '[' | '\\' | ']' | 'f' | 't' => 278,
        '"' => 355,
        '#'
        | '$'
        | '0'..='9'
        | '?'
        | '_'
        | 'L'
        | 'a'
        | 'b'
        | 'd'
        | 'e'
        | 'g'
        | 'h'
        | 'n'
        | 'o'
        | 'p'
        | 'q'
        | 'u' => 556,
        '%' => 889,
        '&' | 'A' | 'B' | 'E' | 'K' | 'P' | 'V' | 'X' | 'Y' => 667,
        '\'' | 'i' | 'j' | 'l' => 222,
        '(' | ')' | '-' | '`' | 'r' => 333,
        '*' => 389,
        '+' | '<' | '=' | '>' | '~' => 584,
        '@' => 1015,
        'C' | 'D' | 'H' | 'N' | 'R' | 'U' | 'w' => 722,
        'F' | 'T' | 'Z' => 611,
        'G' | 'O' | 'Q' => 778,
        'J' | 'c' | 'k' | 's' | 'v' | 'x' | 'y' | 'z' => 500,
        'M' | 'm' => 833,
        'S' => 667,
        'W' => 944,
        '^' => 469,
        '{' | '}' => 334,
        '|' => 260,
        _ => 556,
    };
    f64::from(thousandths) / 1000.0
}

fn times_width(c: char) -> f64 {
    let thousandths: u32 = match c {
        ' ' | ',' | '.' | '*' => 250,
        '!' | '(' | ')' | '-' | 'I' | '[' | ']' | '`' | 'f' | 'r' => 333,
        '"' => 408,
        '#'
        | '$'
        | '0'..='9'
        | '_'
        | 'b'
        | 'd'
        | 'g'
        | 'h'
        | 'n'
        | 'o'
        | 'p'
        | 'q'
        | 'u'
        | 'v'
        | 'x'
        | 'y'
        | 'k'
        | 'l' => 500,
        '%' => 833,
        '&' | 'B' | 'C' | 'R' => 667,
        '\'' => 180,
        '+' | '<' | '=' | '>' => 564,
        '/' | ':' | ';' | 'i' | 'j' | '\\' => 278,
        '?' | 'a' | 'c' | 'e' | 'z' => 444,
        '@' => 921,
        'A' | 'D' | 'G' | 'H' | 'K' | 'N' | 'O' | 'Q' | 'U' | 'V' | 'X' | 'Y' | 'w' => 722,
        'E' | 'T' | 'Z' => 611,
        'F' | 'P' | 'S' => 556,
        'J' | 's' => 389,
        'L' => 611,
        'M' => 889,
        'W' => 944,
        '^' => 469,
        'm' => 778,
        't' => 278,
        '{' | '}' => 480,
        '|' => 200,
        '~' => 541,
        _ => 500,
    };
    f64::from(thousandths) / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_say_what_a_font_is() {
        assert_eq!(split_family("Arial-BoldItalicMT"), ("Arial".into(), "BoldItalicMT".into()));
        assert_eq!(split_family("TimesNewRomanPSMT"), ("TimesNewRoman".into(), String::new()));
        assert_eq!(split_family("Calibri,Bold"), ("Calibri".into(), "Bold".into()));
        assert_eq!(split_family("ArialBold"), ("Arial".into(), "Bold".into()));
        assert_eq!(word_family("TimesNewRoman"), "Times New Roman");
        assert_eq!(word_family("LiberationSerif"), "Times New Roman");
        assert_eq!(word_family("NotoSans"), "Noto Sans");
        assert_eq!(word_family("DejaVuSans"), "DejaVu Sans");
        assert_eq!(word_family("Helvetica"), "Arial");
        assert_eq!(word_family("NimbusRoman"), "Times New Roman");
        assert_eq!(split_family("NimbusRomanRegular"), ("NimbusRoman".into(), String::new()));
        assert_eq!(word_family("Calibri"), "Calibri");
        assert_eq!(plain_name("ABCDEF+Calibri"), "Calibri");
    }

    #[test]
    fn a_type_3_glyph_named_by_its_code_is_that_character() {
        assert_eq!(type3_name_char("a65"), Some('A'));
        assert_eq!(type3_name_char("c97"), Some('a'));
        assert_eq!(type3_name_char("C101"), Some('e'));
        assert_eq!(type3_name_char("G41"), Some('A'));
        assert_eq!(type3_name_char("g0041"), Some('A'));
        assert_eq!(type3_name_char("a7"), None, "a control code");
        assert_eq!(type3_name_char("alpha"), None);
        assert_eq!(type3_name_char("g41"), None);
    }

    #[test]
    fn private_use_codes_become_their_bullets() {
        assert_eq!(tidy("\u{F0B7}".into(), true), "\u{2022}");
        assert_eq!(tidy("\u{F041}".into(), true), "A");
        assert_eq!(tidy("plain".into(), false), "plain");
    }
}

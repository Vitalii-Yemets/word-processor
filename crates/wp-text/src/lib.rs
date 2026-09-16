//! Plain text files: the code pages, telling them apart, reading and writing.
//!
//! # Why a text file is a question
//!
//! A `.txt` file is bytes with no note of what they mean. Before Unicode every
//! language had its own table of the hundred and twenty-eight bytes past
//! ASCII — Windows 1252 for Western Europe, 1251 for Russian, 1250 for Polish
//! and Czech, and the older ISO and DOS tables beside them — and a file
//! written under one and read under another is a page of the wrong letters.
//! Word's answer is to guess where it can be sure and to ask where it cannot:
//! a mark at the start of the file, or bytes that are only valid as UTF-8,
//! settle it; anything else opens the File Conversion dialog with a preview,
//! so that the person can see which table makes their text readable. That is
//! what [`detect`] decides and what the dialog is built from.
//!
//! # What is here
//!
//! The single-byte code pages Word lists, as tables generated from Unicode's
//! own mapping files (see `tools/generate-codepages.sh`), and UTF-8 and
//! UTF-16 both ways round. Decoding never fails: every byte of a single-byte
//! table is some character, and a byte that is not valid UTF-8 becomes the
//! replacement character, which is what Word shows too. Encoding can fail one
//! character at a time — a Cyrillic file cannot hold an ő — and says how many
//! it could not write, which is what Word's dialog marks in red.
//!
//! # What is not
//!
//! The East Asian encodings — Shift-JIS, GBK, Big5, EUC-KR — which are tables
//! of thousands rather than of a hundred and twenty-eight, and are named in
//! the roadmap rather than half done here.

#![forbid(unsafe_code)]

pub mod base64;

mod tables;

/// An encoding a text file can be in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Encoding {
    /// UTF-8, written with the mark at the start, which is how Word writes it.
    Utf8,
    /// UTF-16, least significant byte first, with the mark: Word's "Unicode".
    Utf16Le,
    /// UTF-16 the other way round: Word's "Unicode (Big-Endian)".
    Utf16Be,
    /// ASCII and nothing past it: what cannot be written becomes a question
    /// mark.
    Ascii,
    /// A single-byte code page, by its number.
    CodePage(u16),
}

/// One of the single-byte tables: its number, what Word calls it, and the
/// upper half of the table.
struct Page {
    number: u16,
    name: &'static str,
    upper: &'static [char; 128],
}

/// Every code page this program knows, in the order Word's list has them.
const PAGES: &[Page] = &[
    Page { number: 1252, name: "Western European (Windows)", upper: &tables::WINDOWS_1252 },
    Page { number: 1250, name: "Central European (Windows)", upper: &tables::WINDOWS_1250 },
    Page { number: 1251, name: "Cyrillic (Windows)", upper: &tables::WINDOWS_1251 },
    Page { number: 1253, name: "Greek (Windows)", upper: &tables::WINDOWS_1253 },
    Page { number: 1254, name: "Turkish (Windows)", upper: &tables::WINDOWS_1254 },
    Page { number: 1255, name: "Hebrew (Windows)", upper: &tables::WINDOWS_1255 },
    Page { number: 1256, name: "Arabic (Windows)", upper: &tables::WINDOWS_1256 },
    Page { number: 1257, name: "Baltic (Windows)", upper: &tables::WINDOWS_1257 },
    Page { number: 1258, name: "Vietnamese (Windows)", upper: &tables::WINDOWS_1258 },
    Page { number: 28591, name: "Western European (ISO)", upper: &tables::ISO_8859_1 },
    Page { number: 28592, name: "Central European (ISO)", upper: &tables::ISO_8859_2 },
    Page { number: 28595, name: "Cyrillic (ISO)", upper: &tables::ISO_8859_5 },
    Page { number: 28597, name: "Greek (ISO)", upper: &tables::ISO_8859_7 },
    Page { number: 28599, name: "Turkish (ISO)", upper: &tables::ISO_8859_9 },
    Page { number: 28605, name: "Latin 9 (ISO)", upper: &tables::ISO_8859_15 },
    Page { number: 20866, name: "Cyrillic (KOI8-R)", upper: &tables::KOI8_R },
    Page { number: 21866, name: "Cyrillic (KOI8-U)", upper: &tables::KOI8_U },
    Page { number: 437, name: "OEM United States", upper: &tables::DOS_437 },
    Page { number: 850, name: "Western European (DOS)", upper: &tables::DOS_850 },
    Page { number: 852, name: "Central European (DOS)", upper: &tables::DOS_852 },
    Page { number: 857, name: "Turkish (DOS)", upper: &tables::DOS_857 },
    Page { number: 866, name: "Cyrillic (DOS)", upper: &tables::DOS_866 },
];

impl Encoding {
    /// Every encoding there is to choose from, in the order Word lists them:
    /// Unicode first, then the code pages.
    #[must_use]
    pub fn all() -> Vec<Self> {
        let mut all = vec![Self::Utf8, Self::Utf16Le, Self::Utf16Be, Self::Ascii];
        all.extend(PAGES.iter().map(|page| Self::CodePage(page.number)));
        all
    }

    /// The code page of a number, if it is one this program knows.
    #[must_use]
    pub fn code_page(number: u32) -> Option<Self> {
        match number {
            65001 => Some(Self::Utf8),
            1200 => Some(Self::Utf16Le),
            1201 => Some(Self::Utf16Be),
            20127 => Some(Self::Ascii),
            other => {
                let number = u16::try_from(other).ok()?;
                PAGES.iter().any(|page| page.number == number).then_some(Self::CodePage(number))
            }
        }
    }

    /// The encoding a name from a file names: `windows-1251`, `utf-8`,
    /// `iso-8859-2`, `koi8-r`, `cp866`, as HTML and mail write them, in any
    /// case and with or without the hyphens.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        let name: String =
            name.trim().to_ascii_lowercase().chars().filter(|c| *c != '_' && *c != '-').collect();
        match name.as_str() {
            "utf8" => Some(Self::Utf8),
            "utf16" | "utf16le" | "unicode" => Some(Self::Utf16Le),
            "utf16be" => Some(Self::Utf16Be),
            "usascii" | "ascii" => Some(Self::Ascii),
            "koi8r" => Some(Self::CodePage(20866)),
            "koi8u" => Some(Self::CodePage(21866)),
            "latin1" | "l1" => Some(Self::CodePage(28591)),
            "latin2" | "l2" => Some(Self::CodePage(28592)),
            "latin9" => Some(Self::CodePage(28605)),
            other => {
                let iso = other.strip_prefix("iso8859");
                let digits = iso
                    .or_else(|| other.strip_prefix("windows"))
                    .or_else(|| other.strip_prefix("cp"))
                    .or_else(|| other.strip_prefix("ibm"))
                    .unwrap_or(other);
                let number: u32 = digits.parse().ok()?;
                let number = if iso.is_some() { 28590 + number } else { number };
                Self::code_page(number)
            }
        }
    }

    /// The name a file is given for it, as HTML and mail write them.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Utf8 => "utf-8".to_owned(),
            Self::Utf16Le => "utf-16".to_owned(),
            Self::Utf16Be => "utf-16be".to_owned(),
            Self::Ascii => "us-ascii".to_owned(),
            Self::CodePage(20866) => "koi8-r".to_owned(),
            Self::CodePage(21866) => "koi8-u".to_owned(),
            Self::CodePage(number @ 28591..=28605) => format!("iso-8859-{}", number - 28590),
            Self::CodePage(number @ 1250..=1258) => format!("windows-{number}"),
            Self::CodePage(number) => format!("cp{number}"),
        }
    }

    /// The number Windows knows it by.
    #[must_use]
    pub fn number(self) -> u32 {
        match self {
            Self::Utf8 => 65001,
            Self::Utf16Le => 1200,
            Self::Utf16Be => 1201,
            Self::Ascii => 20127,
            Self::CodePage(number) => u32::from(number),
        }
    }

    /// What Word calls it in the File Conversion dialog.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "Unicode (UTF-8)",
            Self::Utf16Le => "Unicode",
            Self::Utf16Be => "Unicode (Big-Endian)",
            Self::Ascii => "US-ASCII",
            Self::CodePage(number) => {
                PAGES.iter().find(|page| page.number == number).map_or("Unknown", |page| page.name)
            }
        }
    }

    fn page(self) -> Option<&'static Page> {
        match self {
            Self::CodePage(number) => PAGES.iter().find(|page| page.number == number),
            _ => None,
        }
    }

    /// The text the bytes hold under this encoding.
    ///
    /// Never fails: a byte that cannot be read is the replacement character,
    /// which is what Word shows for it. A mark at the start is not text and
    /// is dropped.
    #[must_use]
    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            Self::Utf8 => {
                let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
                String::from_utf8_lossy(body).into_owned()
            }
            Self::Utf16Le => {
                let body = bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes);
                decode_utf16(body, u16::from_le_bytes)
            }
            Self::Utf16Be => {
                let body = bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(bytes);
                decode_utf16(body, u16::from_be_bytes)
            }
            Self::Ascii => bytes
                .iter()
                .map(|&byte| if byte < 0x80 { byte as char } else { '\u{FFFD}' })
                .collect(),
            Self::CodePage(_) => {
                let Some(page) = self.page() else { return String::new() };
                let text: String = bytes
                    .iter()
                    .map(|&byte| {
                        if byte < 0x80 {
                            byte as char
                        } else {
                            page.upper[usize::from(byte - 0x80)]
                        }
                    })
                    .collect();
                // A page that keeps marks apart from their letters — Vietnamese
                // — gives a letter and then its marks; put together they are
                // the letter, which is what everything else expects.
                wp_normal::compose(&text)
            }
        }
    }

    /// The bytes of a text under this encoding, and how many characters
    /// could not be written in it.
    ///
    /// A character the encoding has no byte for becomes a question mark, as
    /// Word writes it — or, with substitution allowed, the nearest thing the
    /// encoding does have where there is one: a straight quote for a curly
    /// one, a hyphen for a dash. Unicode can write everything and loses
    /// nothing.
    #[must_use]
    pub fn encode(self, text: &str, substitute: bool) -> (Vec<u8>, usize) {
        match self {
            Self::Utf8 => {
                let mut out = vec![0xEF, 0xBB, 0xBF];
                out.extend_from_slice(text.as_bytes());
                (out, 0)
            }
            Self::Utf16Le => {
                let mut out = vec![0xFF, 0xFE];
                for unit in text.encode_utf16() {
                    out.extend_from_slice(&unit.to_le_bytes());
                }
                (out, 0)
            }
            Self::Utf16Be => {
                let mut out = vec![0xFE, 0xFF];
                for unit in text.encode_utf16() {
                    out.extend_from_slice(&unit.to_be_bytes());
                }
                (out, 0)
            }
            Self::Ascii | Self::CodePage(_) => {
                let page = self.page();
                let mut out = Vec::with_capacity(text.len());
                let mut lost = 0;
                for character in text.chars() {
                    if let Some(byte) = self.byte_for(page, character, substitute) {
                        out.push(byte);
                        continue;
                    }
                    // A letter the page writes in pieces: Vietnamese keeps its
                    // tone marks as separate bytes after the vowel, so ệ is ê
                    // and then the dot below, each of which the page has.
                    if let Some(pieces) = self.pieces_for(page, character) {
                        out.extend(pieces);
                        continue;
                    }
                    out.push(b'?');
                    lost += 1;
                }
                (out, lost)
            }
        }
    }

    /// Whether a text can be written under this encoding without loss.
    #[must_use]
    pub fn can_write(self, text: &str, substitute: bool) -> bool {
        self.encode(text, substitute).1 == 0
    }

    /// The bytes for a letter the page writes as a base and its marks, if
    /// it has every piece.
    fn pieces_for(self, page: Option<&Page>, character: char) -> Option<Vec<u8>> {
        // The letter taken apart all the way: a base and its marks in order.
        let mut base = character;
        let mut marks = Vec::new();
        while let Some((under, mark)) = wp_normal::decomposed(base) {
            if let Some(mark) = mark {
                marks.insert(0, mark);
            }
            base = under;
        }
        if marks.is_empty() {
            return None;
        }
        // Then put back together as far as the page has letters for: ê is
        // one byte of Vietnamese and the dot below it another, whichever
        // order the marks came apart in.
        let mut apart = Vec::new();
        for mark in marks {
            match wp_normal::composed(base, mark) {
                Some(joined) if self.byte_for(page, joined, false).is_some() => base = joined,
                _ => apart.push(mark),
            }
        }
        let mut pieces = vec![self.byte_for(page, base, false)?];
        for mark in apart {
            pieces.push(self.byte_for(page, mark, false)?);
        }
        Some(pieces)
    }

    /// The byte for a character, or for what may stand in for it.
    fn byte_for(self, page: Option<&Page>, character: char, substitute: bool) -> Option<u8> {
        if (character as u32) < 0x80 {
            return Some(character as u8);
        }
        if let Some(page) = page {
            if let Some(found) = page.upper.iter().position(|&held| held == character) {
                return Some(0x80 + found as u8);
            }
        }
        if !substitute {
            return None;
        }
        // The nearest thing ASCII has, for the characters a document is full
        // of and a code page is not: Word's list, near enough.
        let stand_in = match character {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2032}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{2033}' => '"',
            '\u{2013}' | '\u{2014}' | '\u{2212}' | '\u{2010}' | '\u{2011}' => '-',
            '\u{2026}' => return Some(b'.'),
            '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2009}' => ' ',
            '\u{2022}' | '\u{00B7}' => '*',
            '\u{00A9}' => 'c',
            '\u{2122}' | '\u{00AE}' => return None,
            '\u{00AB}' => '<',
            '\u{00BB}' => '>',
            '\u{00D7}' => 'x',
            '\u{00F7}' => '/',
            '\u{20AC}' => 'E',
            other => strip_accent(other)?,
        };
        Some(stand_in as u8)
    }
}

/// The base letter of an accented Latin one, for the code pages that have
/// not got the accented one: é to e, which is what Word's substitution does.
fn strip_accent(character: char) -> Option<char> {
    const TABLE: &[(&str, char)] = &[
        ("ÀÁÂÃÄÅĀĂĄ", 'A'),
        ("àáâãäåāăą", 'a'),
        ("ÇĆĈĊČ", 'C'),
        ("çćĉċč", 'c'),
        ("ĎĐ", 'D'),
        ("ďđ", 'd'),
        ("ÈÉÊËĒĔĖĘĚ", 'E'),
        ("èéêëēĕėęě", 'e'),
        ("ĜĞĠĢ", 'G'),
        ("ĝğġģ", 'g'),
        ("ÌÍÎÏĨĪĬĮİ", 'I'),
        ("ìíîïĩīĭįı", 'i'),
        ("ĹĻĽĿŁ", 'L'),
        ("ĺļľŀł", 'l'),
        ("ÑŃŅŇ", 'N'),
        ("ñńņň", 'n'),
        ("ÒÓÔÕÖØŌŎŐ", 'O'),
        ("òóôõöøōŏő", 'o'),
        ("ŔŖŘ", 'R'),
        ("ŕŗř", 'r'),
        ("ŚŜŞŠ", 'S'),
        ("śŝşš", 's'),
        ("ŢŤŦ", 'T'),
        ("ţťŧ", 't'),
        ("ÙÚÛÜŨŪŬŮŰŲ", 'U'),
        ("ùúûüũūŭůűų", 'u'),
        ("ÝŶŸ", 'Y'),
        ("ýÿŷ", 'y'),
        ("ŹŻŽ", 'Z'),
        ("źżž", 'z'),
        ("ß", 's'),
    ];
    TABLE.iter().find(|(accented, _)| accented.contains(character)).map(|(_, plain)| *plain)
}

/// UTF-16 to text, a pair of bytes at a time, the way the caller says the
/// bytes are ordered. An odd byte at the end, or a surrogate on its own, is
/// the replacement character.
fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| unit([pair[0], pair[1]])).collect();
    let mut text = String::from_utf16_lossy(&units);
    if bytes.len() % 2 == 1 {
        text.push('\u{FFFD}');
    }
    text
}

/// What was made of a file's bytes: the likeliest encoding, and whether it is
/// certain enough not to ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detected {
    pub encoding: Encoding,
    /// Whether the file settled it — a mark, or bytes that are only valid one
    /// way — or the encoding is a guess the person should see before it is
    /// believed.
    pub sure: bool,
}

/// Decides what a text file is in, as far as the bytes say.
///
/// `default` is the code page the machine writes by, which is what "Windows
/// (Default)" means in the dialog and what a file of plain bytes is taken to
/// be in when nothing says otherwise. A mark at the start settles it. So does
/// UTF-16 without one, which gives itself away by the zero bytes between the
/// letters, and UTF-8 without one, whose multi-byte sequences no other
/// encoding produces by accident. Bytes past ASCII that are not UTF-8 are
/// some code page, and which one is a guess: the default, marked as a guess.
#[must_use]
pub fn detect(bytes: &[u8], default: Encoding) -> Detected {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Detected { encoding: Encoding::Utf8, sure: true };
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Detected { encoding: Encoding::Utf16Le, sure: true };
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Detected { encoding: Encoding::Utf16Be, sure: true };
    }

    // UTF-16 of ordinary text has a zero in every other byte, and no other
    // encoding of text has zeros at all.
    if bytes.len() >= 4 {
        let zeros_at =
            |offset: usize| bytes.iter().skip(offset).step_by(2).filter(|byte| **byte == 0).count();
        let (even, odd) = (bytes.len().div_ceil(2), bytes.len() / 2);
        let (even_zeros, odd_zeros) = (zeros_at(0), zeros_at(1));
        if odd_zeros * 2 > odd && even_zeros * 8 < even {
            return Detected { encoding: Encoding::Utf16Le, sure: true };
        }
        if even_zeros * 2 > even && odd_zeros * 8 < odd {
            return Detected { encoding: Encoding::Utf16Be, sure: true };
        }
    }

    if bytes.iter().all(|byte| *byte < 0x80) {
        // Every encoding here agrees on these bytes, so whichever the machine
        // writes by is as right as any.
        return Detected { encoding: default, sure: true };
    }
    if core::str::from_utf8(bytes).is_ok() {
        return Detected { encoding: Encoding::Utf8, sure: true };
    }
    Detected { encoding: default, sure: false }
}

/// The lines of a text, whichever way its lines end.
///
/// Windows ends a line with CR LF, Unix with LF, the old Macintosh with CR,
/// and a file may have been through all three. A line ending at the very end
/// of the file ends the last line rather than beginning an empty one, which is
/// how Word reads it.
#[must_use]
pub fn lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                lines.push(core::mem::take(&mut current));
            }
            '\n' => lines.push(core::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// How the lines of a written file end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    /// Word's default, and Windows's.
    CrLf,
    Cr,
    Lf,
}

impl LineEnding {
    /// All three, in the order Word's dialog lists them.
    pub const ALL: [Self; 3] = [Self::CrLf, Self::Cr, Self::Lf];

    /// What Word's dialog calls it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::CrLf => "CR/LF",
            Self::Cr => "CR",
            Self::Lf => "LF",
        }
    }

    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
            Self::Lf => "\n",
        }
    }
}

/// The text of a file made of lines, ended the way asked.
///
/// Every line is ended, the last one too, which is how Word writes a text
/// file: a paragraph mark is a line ending.
#[must_use]
pub fn join(lines: &[String], ending: LineEnding) -> String {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push_str(ending.text());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_page_reads_its_own_letters() {
        let cyrillic = Encoding::CodePage(1251);
        assert_eq!(cyrillic.decode(&[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2]), "Привет");
        let western = Encoding::CodePage(1252);
        assert_eq!(western.decode(&[0x63, 0x61, 0x66, 0xE9, 0x20, 0x80]), "café €");
        let koi = Encoding::CodePage(20866);
        assert_eq!(koi.decode(&[0xF0, 0xD2, 0xC9, 0xD7, 0xC5, 0xD4]), "Привет");
        let dos = Encoding::CodePage(866);
        assert_eq!(dos.decode(&[0x8F, 0xE0, 0xA8, 0xA2, 0xA5, 0xE2]), "Привет");
    }

    #[test]
    fn what_is_written_is_what_is_read() {
        for encoding in Encoding::all() {
            let text = match encoding {
                Encoding::CodePage(1251 | 20866 | 21866 | 866 | 28595) => "Привет, мир!",
                Encoding::CodePage(1253 | 28597) => "Γειά σου",
                Encoding::CodePage(1255) => "שלום",
                Encoding::CodePage(1256) => "مرحبا",
                Encoding::CodePage(1250 | 28592 | 852) => "Zażółć gęślą jaźń",
                Encoding::CodePage(1254 | 28599 | 857) => "Şişli İstanbul",
                Encoding::CodePage(1257) => "Ā ā Ē ē",
                Encoding::CodePage(1258) => "Việt Nam",
                Encoding::Ascii => "plain words",
                Encoding::CodePage(1252)
                | Encoding::Utf8
                | Encoding::Utf16Le
                | Encoding::Utf16Be => "café — “quoted”",
                _ => "café",
            };
            let (bytes, lost) = encoding.encode(text, false);
            assert_eq!(lost, 0, "{encoding:?} could not write {text:?}");
            assert_eq!(encoding.decode(&bytes), text, "{encoding:?}");
        }
    }

    #[test]
    fn what_a_code_page_cannot_hold_is_counted_and_can_be_stood_in_for() {
        let cyrillic = Encoding::CodePage(1251);
        let (bytes, lost) = cyrillic.encode("Привет ő", false);
        assert_eq!(lost, 1);
        assert_eq!(bytes.last(), Some(&b'?'));
        assert!(!cyrillic.can_write("ő", false));

        let (bytes, lost) = Encoding::Ascii.encode("“café” — done…", true);
        assert_eq!(lost, 0);
        assert_eq!(bytes, b"\"cafe\" - done.");
        let (_, lost) = Encoding::Ascii.encode("“café”", false);
        assert_eq!(lost, 3);
        let (_, lost) = Encoding::Ascii.encode("ő 中", true);
        assert_eq!(lost, 1, "an accented letter has a plain one; a Chinese one has nothing");
    }

    #[test]
    fn unicode_writes_its_mark_and_reads_without_one() {
        let (bytes, _) = Encoding::Utf8.encode("é", false);
        assert_eq!(bytes, [0xEF, 0xBB, 0xBF, 0xC3, 0xA9]);
        assert_eq!(Encoding::Utf8.decode(&bytes), "é");
        assert_eq!(Encoding::Utf8.decode(&[0xC3, 0xA9]), "é");
        let (bytes, _) = Encoding::Utf16Le.encode("A", false);
        assert_eq!(bytes, [0xFF, 0xFE, 0x41, 0x00]);
        assert_eq!(Encoding::Utf16Be.decode(&[0xFE, 0xFF, 0x00, 0x41]), "A");
        assert_eq!(Encoding::Utf16Le.decode(&[0x41, 0x00, 0x42]), "A\u{FFFD}");
    }

    #[test]
    fn a_marked_file_is_sure_and_an_ascii_one_is_sure() {
        let default = Encoding::CodePage(1252);
        assert_eq!(
            detect(&[0xEF, 0xBB, 0xBF, b'a'], default),
            Detected { encoding: Encoding::Utf8, sure: true }
        );
        assert_eq!(
            detect(&[0xFF, 0xFE, b'a', 0], default),
            Detected { encoding: Encoding::Utf16Le, sure: true }
        );
        assert_eq!(detect(b"plain words\r\n", default), Detected { encoding: default, sure: true });
        assert_eq!(detect(b"", default), Detected { encoding: default, sure: true });
    }

    #[test]
    fn utf8_and_utf16_give_themselves_away_without_a_mark() {
        let default = Encoding::CodePage(1252);
        assert_eq!(
            detect("Привет, мир".as_bytes(), default),
            Detected { encoding: Encoding::Utf8, sure: true }
        );
        let (le, _) = Encoding::Utf16Le.encode("Hello there", false);
        assert_eq!(detect(&le[2..], default), Detected { encoding: Encoding::Utf16Le, sure: true });
        let (be, _) = Encoding::Utf16Be.encode("Hello there", false);
        assert_eq!(detect(&be[2..], default), Detected { encoding: Encoding::Utf16Be, sure: true });
    }

    #[test]
    fn a_code_page_file_is_a_guess_that_asks() {
        let default = Encoding::CodePage(1252);
        let (bytes, _) = Encoding::CodePage(1251).encode("Привет", false);
        assert_eq!(detect(&bytes, default), Detected { encoding: default, sure: false });
    }

    #[test]
    fn lines_end_however_the_file_ended_them() {
        assert_eq!(lines("one\r\ntwo\nthree\rfour"), vec!["one", "two", "three", "four"]);
        assert_eq!(lines("one\r\n"), vec!["one"], "a file ending in a line ending");
        assert_eq!(lines("one\n\ntwo"), vec!["one", "", "two"], "an empty line is a line");
        assert_eq!(lines(""), vec![""], "an empty file is one empty paragraph");
        assert_eq!(join(&["a".to_owned(), "b".to_owned()], LineEnding::CrLf), "a\r\nb\r\n");
        assert_eq!(join(&["a".to_owned()], LineEnding::Lf), "a\n");
    }

    #[test]
    fn a_name_from_a_file_names_its_encoding() {
        assert_eq!(Encoding::named("UTF-8"), Some(Encoding::Utf8));
        assert_eq!(Encoding::named("windows-1251"), Some(Encoding::CodePage(1251)));
        assert_eq!(Encoding::named("Windows-1252"), Some(Encoding::CodePage(1252)));
        assert_eq!(Encoding::named("cp866"), Some(Encoding::CodePage(866)));
        assert_eq!(Encoding::named("ISO-8859-2"), Some(Encoding::CodePage(28592)));
        assert_eq!(Encoding::named("koi8-r"), Some(Encoding::CodePage(20866)));
        assert_eq!(Encoding::named("latin1"), Some(Encoding::CodePage(28591)));
        assert_eq!(Encoding::named("shift_jis"), None);
        for encoding in Encoding::all() {
            assert_eq!(Encoding::named(&encoding.label()), Some(encoding), "{encoding:?}");
        }
    }

    #[test]
    fn every_encoding_has_a_name_and_a_number() {
        for encoding in Encoding::all() {
            assert_ne!(encoding.name(), "Unknown", "{encoding:?}");
            assert_eq!(Encoding::code_page(encoding.number()), Some(encoding));
        }
        assert_eq!(Encoding::CodePage(1251).name(), "Cyrillic (Windows)");
        assert_eq!(Encoding::code_page(932), None, "Shift-JIS is not here yet");
    }
}

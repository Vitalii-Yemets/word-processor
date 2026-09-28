//! Reading a bilingual dictionary: what a word is in another language.
//!
//! # What this is, and is not
//!
//! Word's Translate sends the text to Microsoft's servers and shows what comes
//! back. This program sends nothing anywhere, so what it can do is what a
//! bilingual dictionary on the machine can do: say what each word is in the
//! other language, sense by sense, with the part of speech and an example —
//! which is what Word's Translator pane shows for one word, and the part of
//! translation that needs no network.
//!
//! # The format
//!
//! dictd's, which is what the free bilingual dictionaries — FreeDict's,
//! generated from the Ding and the Wiktionary lists — are published in and
//! what `dict` reads. Two files: an `.index` of `headword TAB offset TAB
//! length` lines, the numbers written in a base 64 of dictd's own, and a
//! `.dict` holding every entry as text — usually as `.dict.dz`, which is one
//! gzip stream flushed every few kilobytes with a table of the pieces in its
//! header, so that an entry can be read without inflating what comes before
//! it. Nothing is shipped with this program: it reads whatever the machine
//! has, on the same terms as the spelling dictionaries and the thesaurus.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::Error;

/// One sense of a word: what it is in the other language in that sense.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meaning {
    /// The words it becomes: "Haus", or several that would each do.
    pub translations: Vec<String>,
    /// What kind of word it is in this sense, as the dictionary tags it:
    /// "adj", "neut", "fem".
    pub part_of_speech: Option<String>,
    /// A remark the dictionary makes about the sense: the field it belongs
    /// to, or which preposition goes with it.
    pub note: Option<String>,
    /// A phrase using the word in this sense, and what the phrase is in the
    /// other language.
    pub examples: Vec<(String, String)>,
}

/// A bilingual dictionary, open and ready to be asked.
#[derive(Debug)]
pub struct Bilingual {
    data: PathBuf,
    /// Where each headword's entries are in the uncompressed data — several
    /// per word, one per sense. Lower-cased, which is how the index keys
    /// them.
    entries: HashMap<String, Vec<(u64, usize)>>,
    /// How the data file is cut into pieces, when it is a dictzip.
    pieces: Option<Pieces>,
}

/// The pieces of a dictzip: how many uncompressed bytes each holds, and where
/// each one's compressed bytes are in the file.
#[derive(Debug)]
struct Pieces {
    each: usize,
    at: Vec<(u64, usize)>,
}

impl Bilingual {
    /// Opens a dictionary given its index; the data file is beside it under
    /// the same name, compressed or not.
    pub fn open(index: &Path) -> Result<Self, Error> {
        let stem = index.with_extension("");
        let compressed = stem.with_extension("dict.dz");
        let plain = stem.with_extension("dict");
        let (data, pieces) = if compressed.exists() {
            let pieces = read_pieces(&compressed)?;
            (compressed, Some(pieces))
        } else if plain.exists() {
            (plain, None)
        } else {
            return Err(Error::NotReadable("no data file beside the index".to_owned()));
        };

        let file = File::open(index).map_err(|error| Error::NotReadable(error.to_string()))?;
        let mut entries: HashMap<String, Vec<(u64, usize)>> = HashMap::new();
        for line in BufReader::new(file).split(b'\n').map_while(Result::ok) {
            let text = String::from_utf8_lossy(&line);
            let mut fields = text.trim_end().split('\t');
            let (Some(word), Some(offset), Some(length)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            // The lines about the dictionary itself are not words in it.
            if word.starts_with("00-database") || word.starts_with("00database") {
                continue;
            }
            let (Some(offset), Some(length)) = (number(offset), number(length)) else { continue };
            entries.entry(word.to_lowercase()).or_default().push((offset, length as usize));
        }
        if entries.is_empty() {
            return Err(Error::NotReadable("the index holds no words".to_owned()));
        }
        Ok(Self { data, entries, pieces })
    }

    /// How many words it knows.
    #[must_use]
    pub fn words(&self) -> usize {
        self.entries.len()
    }

    /// The senses of a word, and what it is in the other language in each.
    ///
    /// Empty for a word the dictionary has nothing to say about, which
    /// includes every word in a third language.
    #[must_use]
    pub fn meanings(&self, word: &str) -> Vec<Meaning> {
        let key = word.trim().to_lowercase();
        let Some(places) = self.entries.get(&key) else { return Vec::new() };
        let mut meanings = Vec::new();
        for (offset, length) in places {
            let Some(text) = self.read(*offset, *length) else { continue };
            if let Some(meaning) = parse_entry(&text) {
                meanings.push(meaning);
            }
        }
        meanings
    }

    /// Reads a stretch of the uncompressed data.
    fn read(&self, offset: u64, length: usize) -> Option<String> {
        let mut file = File::open(&self.data).ok()?;
        let bytes = match &self.pieces {
            None => {
                file.seek(SeekFrom::Start(offset)).ok()?;
                let mut buffer = vec![0u8; length];
                file.read_exact(&mut buffer).ok()?;
                buffer
            }
            Some(pieces) => {
                // The pieces the stretch runs through, inflated one after
                // another, and the stretch cut out of them.
                let each = pieces.each as u64;
                let first = (offset / each) as usize;
                let last = ((offset + length as u64).saturating_sub(1) / each) as usize;
                let mut held = Vec::with_capacity((last - first + 1) * pieces.each);
                for index in first..=last {
                    let (at, size) = *pieces.at.get(index)?;
                    file.seek(SeekFrom::Start(at)).ok()?;
                    let mut compressed = vec![0u8; size];
                    file.read_exact(&mut compressed).ok()?;
                    held.extend(wp_deflate::inflate_piece(&compressed, pieces.each).ok()?);
                }
                let start = (offset - first as u64 * each) as usize;
                held.get(start..start + length)?.to_vec()
            }
        };
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// dictd's base 64: the same alphabet as everyone else's, most significant
/// digit first, no padding.
fn number(text: &str) -> Option<u64> {
    let mut value: u64 = 0;
    for byte in text.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        value = value.checked_mul(64)?.checked_add(u64::from(digit))?;
    }
    Some(value)
}

/// Reads the table of pieces out of a dictzip's header.
///
/// A gzip header with an extra field, and in it a subfield `RA`: a version, how
/// many bytes each piece holds, how many pieces there are, and the compressed
/// length of each. The pieces follow the header one after another, so where
/// each begins is the sum of the lengths before it.
fn read_pieces(path: &Path) -> Result<Pieces, Error> {
    let mut file = File::open(path).map_err(|error| Error::NotReadable(error.to_string()))?;
    let mut header = vec![0u8; 64 * 1024];
    let read = file.read(&mut header).map_err(|error| Error::NotReadable(error.to_string()))?;
    header.truncate(read);
    parse_pieces(&header).ok_or_else(|| Error::NotReadable("not a dictzip".to_owned()))
}

/// The same, from the bytes at the start of the file.
fn parse_pieces(header: &[u8]) -> Option<Pieces> {
    if header.get(0..3)? != [0x1f, 0x8b, 8] {
        return None;
    }
    let flags = *header.get(3)?;
    let mut at = 10;
    if flags & 0x04 == 0 {
        return None;
    }
    let extra_length = usize::from(u16::from_le_bytes([*header.get(at)?, *header.get(at + 1)?]));
    at += 2;
    let extra = header.get(at..at + extra_length)?;
    at += extra_length;

    // The subfields of the extra field, looking for the one that is ours.
    let mut pieces = None;
    let mut cursor = 0;
    while cursor + 4 <= extra.len() {
        let id = &extra[cursor..cursor + 2];
        let length = usize::from(u16::from_le_bytes([extra[cursor + 2], extra[cursor + 3]]));
        let body = extra.get(cursor + 4..cursor + 4 + length)?;
        if id == b"RA" && body.len() >= 6 {
            let each = usize::from(u16::from_le_bytes([body[2], body[3]]));
            let count = usize::from(u16::from_le_bytes([body[4], body[5]]));
            let mut sizes = Vec::with_capacity(count);
            for index in 0..count {
                let pair = body.get(6 + index * 2..8 + index * 2)?;
                sizes.push(usize::from(u16::from_le_bytes([pair[0], pair[1]])));
            }
            pieces = Some((each, sizes));
        }
        cursor += 4 + length;
    }
    let (each, sizes) = pieces?;
    if each == 0 {
        return None;
    }

    // Past the rest of the header: a name, a comment, a checksum, whichever
    // the flags say are there.
    if flags & 0x08 != 0 {
        at = header.iter().skip(at).position(|byte| *byte == 0).map(|found| at + found + 1)?;
    }
    if flags & 0x10 != 0 {
        at = header.iter().skip(at).position(|byte| *byte == 0).map(|found| at + found + 1)?;
    }
    if flags & 0x02 != 0 {
        at += 2;
    }

    let mut begins = at as u64;
    let mut placed = Vec::with_capacity(sizes.len());
    for size in sizes {
        placed.push((begins, size));
        begins += size as u64;
    }
    Some(Pieces { each, at: placed })
}

/// Reads one entry as the Ding dictionaries write them.
///
/// The first line is the headword and how it is said. A line at the margin
/// after it is the translations, with the part of speech in angle brackets
/// and the field in square ones; an indented line is a note, a synonym, an
/// example in quotes with its translation after a dash, or a list of entries
/// to see. A dictionary written some other way gives its text as it stands,
/// which is still what the word is.
fn parse_entry(text: &str) -> Option<Meaning> {
    let mut lines = text.lines();
    let _headword = lines.next()?;
    let mut translations = Vec::new();
    let mut part_of_speech = None;
    let mut note = None;
    let mut examples = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // The German-English list sets a sense that begins with the field it
        // belongs to in by a space — " [zool.] dog <n>" — and it is the
        // translations all the same: the dog under "Hund" was lost for it.
        let labelled = translations.is_empty() && trimmed.starts_with('[');
        if !line.starts_with(char::is_whitespace) || labelled {
            // The translations, each with its own tags. "Geschlecht <neut>,
            // Familie <fem>" is two, and the first tag is the kind of word. A
            // dictionary written some other way has more lines at the margin,
            // and each is more of what the word is.
            for piece in split_translations(trimmed) {
                let (word, tags) = strip_tags(&piece);
                if part_of_speech.is_none() {
                    part_of_speech =
                        tags.iter().find(|(open, _)| *open == '<').map(|(_, t)| t.clone());
                }
                if note.is_none() {
                    note = tags.iter().find(|(open, _)| *open == '[').map(|(_, t)| t.clone());
                }
                if !word.is_empty() {
                    translations.push(word);
                }
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Note:") {
            if note.is_none() {
                note = Some(rest.trim().to_owned());
            }
        } else if trimmed.starts_with('"') {
            if let Some((phrase, meaning)) = trimmed.split_once("\"  - ") {
                examples
                    .push((phrase.trim_start_matches('"').to_owned(), meaning.trim().to_owned()));
            }
        } else if trimmed.starts_with("see:")
            || trimmed.starts_with("Synonym")
            || trimmed.starts_with("Antonym")
        {
            // Cross-references, which are the dictionary's business.
        }
    }

    if translations.is_empty() {
        return None;
    }
    Some(Meaning { translations, part_of_speech, note, examples })
}

/// Splits "glücklich, fröhlich <adj>" into its words, leaving the tags on
/// each — but not at a comma inside a bracket.
fn split_translations(line: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for character in line.chars() {
        match character {
            '<' | '[' | '(' | '{' => depth += 1,
            '>' | ']' | ')' | '}' => depth -= 1,
            ',' | ';' if depth <= 0 => {
                pieces.push(core::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    pieces.push(current);
    pieces
        .into_iter()
        .map(|piece| piece.trim().to_owned())
        .filter(|piece| !piece.is_empty())
        .collect()
}

/// Takes the `<adj>` and `[mus.]` tags off a word, giving them back with the
/// bracket each opened with.
fn strip_tags(piece: &str) -> (String, Vec<(char, String)>) {
    let mut word = String::new();
    let mut tags = Vec::new();
    let mut open: Option<(char, String)> = None;
    for character in piece.chars() {
        match (&mut open, character) {
            (None, '<') => open = Some(('<', String::new())),
            (None, '[') => open = Some(('[', String::new())),
            (Some((which, held)), '>' | ']') => {
                tags.push((*which, held.trim().to_owned()));
                open = None;
            }
            (Some((_, held)), other) => held.push(other),
            (None, other) => word.push(other),
        }
    }
    (word.split_whitespace().collect::<Vec<_>>().join(" "), tags)
}

/// A bilingual dictionary installed on this machine: which language it is
/// from, which it is to, and where its index is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// The language of the headwords, as a two-letter code where one exists.
    pub from: String,
    /// The language of the meanings, the same way.
    pub to: String,
    pub index: PathBuf,
}

/// Every bilingual dictionary installed on this machine.
///
/// Looked for where dictd keeps its dictionaries; the file is named
/// `freedict-eng-deu.index`, or `eng-deu.index`, and the languages are the
/// two three-letter codes.
#[must_use]
pub fn installed() -> Vec<Installed> {
    let mut found: Vec<Installed> = Vec::new();
    let mut places = crate::search_paths();
    places.push(PathBuf::from("/usr/share/dictd"));
    places.push(PathBuf::from("/usr/share/dict"));
    if let Ok(data) = std::env::var("APPDATA") {
        places.push(PathBuf::from(data).join("dictd"));
    }

    for directory in places {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("index") {
                continue;
            }
            let stem = path.with_extension("");
            if !stem.with_extension("dict.dz").exists() && !stem.with_extension("dict").exists() {
                continue;
            }
            let Some(name) = stem.file_name().and_then(|value| value.to_str()) else { continue };
            let Some((from, to)) = languages_of(name) else { continue };
            if found.iter().any(|held| held.from == from && held.to == to) {
                continue;
            }
            found.push(Installed { from, to, index: path });
        }
    }
    found.sort_by(|one, other| (&one.from, &one.to).cmp(&(&other.from, &other.to)));
    found
}

/// The two languages a dictionary's name gives: `freedict-eng-deu` is from
/// English to German.
fn languages_of(name: &str) -> Option<(String, String)> {
    let name = name.strip_prefix("freedict-").unwrap_or(name);
    let (from, to) = name.split_once('-')?;
    let to = to.split(['-', '.']).next()?;
    if from.len() != 3 || to.len() != 3 {
        return None;
    }
    Some((two_letter(from).to_owned(), two_letter(to).to_owned()))
}

/// The two-letter code for a three-letter one, for the languages that have
/// one; the three letters stand where there is none.
#[must_use]
pub fn two_letter(code: &str) -> &str {
    match code {
        "eng" => "en",
        "deu" | "ger" => "de",
        "fra" | "fre" => "fr",
        "spa" => "es",
        "ita" => "it",
        "nld" | "dut" => "nl",
        "por" => "pt",
        "rus" => "ru",
        "pol" => "pl",
        "ces" | "cze" => "cs",
        "slk" | "slo" => "sk",
        "dan" => "da",
        "swe" => "sv",
        "nor" | "nob" => "no",
        "fin" => "fi",
        "hun" => "hu",
        "tur" => "tr",
        "ara" => "ar",
        "heb" => "he",
        "ell" | "gre" => "el",
        "jpn" => "ja",
        "kor" => "ko",
        "zho" | "chi" => "zh",
        "ukr" => "uk",
        "bul" => "bg",
        "ron" | "rum" => "ro",
        "hrv" => "hr",
        "srp" => "sr",
        "slv" => "sl",
        "lit" => "lt",
        "lav" => "lv",
        "est" => "et",
        "afr" => "af",
        "cym" | "wel" => "cy",
        "gle" => "ga",
        "gla" => "gd",
        "isl" | "ice" => "is",
        "lat" => "la",
        "hin" => "hi",
        "tha" => "th",
        "vie" => "vi",
        "ind" => "id",
        "msa" | "may" => "ms",
        "swa" => "sw",
        "cat" => "ca",
        "eus" | "baq" => "eu",
        "epo" => "eo",
        "fas" | "per" => "fa",
        "urd" => "ur",
        "ben" => "bn",
        "tam" => "ta",
        "kur" => "ku",
        "bre" => "br",
        "nno" => "nn",
        "mkd" | "mac" => "mk",
        "sqi" | "alb" => "sq",
        "bel" => "be",
        "kat" | "geo" => "ka",
        "hye" | "arm" => "hy",
        "aze" => "az",
        "kaz" => "kk",
        "uzb" => "uz",
        "mon" => "mn",
        "khm" => "km",
        "mya" | "bur" => "my",
        "sin" => "si",
        "nep" => "ne",
        "mar" => "mr",
        "guj" => "gu",
        "pan" => "pa",
        "tel" => "te",
        "kan" => "kn",
        "mal" => "ml",
        "amh" => "am",
        "yid" => "yi",
        "ltz" => "lb",
        "fao" => "fo",
        "mlt" => "mt",
        "glg" => "gl",
        "oci" => "oc",
        "san" => "sa",
        "tgl" => "tl",
        "hau" => "ha",
        "yor" => "yo",
        "zul" => "zu",
        "xho" => "xh",
        other => other,
    }
}

/// What a language is called in English, from its two-letter code, for the
/// languages a dictionary is likely to be in. The code stands for the rest.
#[must_use]
pub fn language_name(code: &str) -> String {
    let base = code.split(['-', '_']).next().unwrap_or(code).to_lowercase();
    let name = match base.as_str() {
        "en" => "English",
        "de" => "German",
        "fr" => "French",
        "es" => "Spanish",
        "it" => "Italian",
        "nl" => "Dutch",
        "pt" => "Portuguese",
        "ru" => "Russian",
        "pl" => "Polish",
        "cs" => "Czech",
        "sk" => "Slovak",
        "da" => "Danish",
        "sv" => "Swedish",
        "no" | "nb" => "Norwegian",
        "nn" => "Norwegian Nynorsk",
        "fi" => "Finnish",
        "hu" => "Hungarian",
        "tr" => "Turkish",
        "ar" => "Arabic",
        "he" => "Hebrew",
        "el" => "Greek",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh" => "Chinese",
        "uk" => "Ukrainian",
        "bg" => "Bulgarian",
        "ro" => "Romanian",
        "hr" => "Croatian",
        "sr" => "Serbian",
        "sl" => "Slovenian",
        "lt" => "Lithuanian",
        "lv" => "Latvian",
        "et" => "Estonian",
        "af" => "Afrikaans",
        "cy" => "Welsh",
        "ga" => "Irish",
        "gd" => "Scottish Gaelic",
        "is" => "Icelandic",
        "la" => "Latin",
        "hi" => "Hindi",
        "th" => "Thai",
        "vi" => "Vietnamese",
        "id" => "Indonesian",
        "ms" => "Malay",
        "sw" => "Swahili",
        "ca" => "Catalan",
        "eu" => "Basque",
        "eo" => "Esperanto",
        "fa" => "Persian",
        "ur" => "Urdu",
        "bn" => "Bengali",
        "ta" => "Tamil",
        "ku" => "Kurdish",
        "br" => "Breton",
        "mk" => "Macedonian",
        "sq" => "Albanian",
        "be" => "Belarusian",
        "ka" => "Georgian",
        "hy" => "Armenian",
        "az" => "Azerbaijani",
        "kk" => "Kazakh",
        "uz" => "Uzbek",
        "mn" => "Mongolian",
        "km" => "Khmer",
        "my" => "Burmese",
        "si" => "Sinhala",
        "ne" => "Nepali",
        "mr" => "Marathi",
        "gu" => "Gujarati",
        "pa" => "Punjabi",
        "te" => "Telugu",
        "kn" => "Kannada",
        "ml" => "Malayalam",
        "am" => "Amharic",
        "yi" => "Yiddish",
        "lb" => "Luxembourgish",
        "fo" => "Faroese",
        "mt" => "Maltese",
        "gl" => "Galician",
        "oc" => "Occitan",
        "sa" => "Sanskrit",
        "tl" => "Tagalog",
        "ha" => "Hausa",
        "yo" => "Yoruba",
        "zu" => "Zulu",
        "xh" => "Xhosa",
        _ => return code.to_owned(),
    };
    name.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dictionary of three words, written to disk the way dictd writes one,
    /// uncompressed. Each test gets a directory of its own, for the reason
    /// the thesaurus tests give.
    fn small(name: &str, compressed: bool) -> (Bilingual, PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("wp-bilingual-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&directory);
        let index = directory.join("freedict-eng-deu.index");

        let entries = [
            ("house", "house /hˈaʊs/\nHaus <neut>\n      \"build a house\"  - ein Haus bauen\n see: {houses}\n\n"),
            ("house", "house /hˈaʊs/\nGeschlecht <neut>, Familie <fem>\n\n"),
            ("happy", "happy /hˈapi/\nglücklich, fröhlich <adj>\n         Note: über\n"),
            ("plain", "plain\njust some text\nand more of it\n"),
        ];
        let mut data = String::from("00-database-info\nA test dictionary\n");
        let mut lines = String::new();
        for (word, entry) in entries {
            let at = data.len();
            data.push_str(entry);
            lines.push_str(&format!(
                "{word}\t{}\t{}\n",
                encode(at as u64),
                encode(entry.len() as u64)
            ));
        }
        std::fs::write(&index, format!("00-database-info\tA\tB\n{lines}")).unwrap();

        if compressed {
            // One piece holding everything, which is a dictzip all the same.
            let deflated = wp_deflate::compress(data.as_bytes());
            let mut file = vec![0x1f, 0x8b, 8, 0x04, 0, 0, 0, 0, 0, 3];
            let mut extra = vec![b'R', b'A'];
            let body_length = 6 + 2;
            extra.extend((body_length as u16).to_le_bytes());
            extra.extend(1u16.to_le_bytes());
            extra.extend((data.len() as u16).to_le_bytes());
            extra.extend(1u16.to_le_bytes());
            extra.extend((deflated.len() as u16).to_le_bytes());
            file.extend((extra.len() as u16).to_le_bytes());
            file.extend(extra);
            file.extend(deflated);
            file.extend(wp_deflate::crc32(data.as_bytes()).to_le_bytes());
            file.extend((data.len() as u32).to_le_bytes());
            std::fs::write(directory.join("freedict-eng-deu.dict.dz"), file).unwrap();
        } else {
            std::fs::write(directory.join("freedict-eng-deu.dict"), &data).unwrap();
        }
        (Bilingual::open(&index).expect("a dictionary"), directory)
    }

    /// dictd's base 64, the other way.
    fn encode(mut value: u64) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut digits = Vec::new();
        loop {
            digits.push(ALPHABET[(value % 64) as usize] as char);
            value /= 64;
            if value == 0 {
                break;
            }
        }
        digits.iter().rev().collect()
    }

    #[test]
    fn the_numbers_are_read_in_dictd_base_64() {
        assert_eq!(number("A"), Some(0));
        assert_eq!(number("B"), Some(1));
        assert_eq!(number("a"), Some(26));
        assert_eq!(number("0"), Some(52));
        assert_eq!(number("/"), Some(63));
        assert_eq!(number("BA"), Some(64));
        assert_eq!(number("DLh6z"), Some(53_354_163));
        assert_eq!(number("x"), Some(49));
        assert_eq!(number("-"), None);
        for value in [0, 1, 63, 64, 4096, 53_354_163] {
            assert_eq!(number(&encode(value)), Some(value));
        }
    }

    #[test]
    fn a_word_has_its_senses_with_their_kinds_and_examples() {
        let (dictionary, directory) = small("senses", false);
        let meanings = dictionary.meanings("house");
        assert_eq!(meanings.len(), 2);
        assert_eq!(meanings[0].translations, vec!["Haus"]);
        assert_eq!(meanings[0].part_of_speech.as_deref(), Some("neut"));
        assert_eq!(
            meanings[0].examples,
            vec![("build a house".to_owned(), "ein Haus bauen".to_owned())]
        );
        assert_eq!(meanings[1].translations, vec!["Geschlecht", "Familie"]);

        let happy = dictionary.meanings("Happy");
        assert_eq!(happy.len(), 1);
        assert_eq!(happy[0].translations, vec!["glücklich", "fröhlich"]);
        assert_eq!(happy[0].part_of_speech.as_deref(), Some("adj"));
        assert_eq!(happy[0].note.as_deref(), Some("über"));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_sense_set_in_by_the_field_it_belongs_to_is_a_sense() {
        // How the German-English list writes "Hund" the animal, which a
        // reader that took translations only from the margin read as nothing.
        let entry = "Hund /h\u{2C8}\u{28A}nt/ <masc, n, sg>\n [zool.] dog <n>, dawg <n>\n         \
                     Note: used to represent American speech\n      \"einen Hund abrichten\"  - \
                     train a dog\n see: {Hunde}, {Haushund}\n";
        let meaning = parse_entry(entry).expect("the sense");
        assert_eq!(meaning.translations, vec!["dog", "dawg"]);
        assert_eq!(meaning.part_of_speech.as_deref(), Some("n"));
        assert_eq!(meaning.note.as_deref(), Some("zool."));
        assert_eq!(
            meaning.examples,
            vec![("einen Hund abrichten".to_owned(), "train a dog".to_owned())]
        );
        // And an example set in the same way after the translations is still
        // an example, not more translations.
        let entry = "Haus <n>\nhouse <n>\n [fig.] home\n";
        assert_eq!(parse_entry(entry).expect("the sense").translations, vec!["house"]);
    }

    #[test]
    fn an_entry_written_some_other_way_is_given_as_it_stands() {
        let (dictionary, directory) = small("plain", false);
        let meanings = dictionary.meanings("plain");
        assert_eq!(meanings.len(), 1);
        assert_eq!(meanings[0].translations, vec!["just some text", "and more of it"]);
        assert_eq!(meanings[0].part_of_speech, None);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_word_it_does_not_know_has_nothing_said_about_it() {
        let (dictionary, directory) = small("unknown", false);
        assert!(dictionary.meanings("xylophone").is_empty());
        assert!(dictionary.meanings("").is_empty());
        assert!(
            dictionary.meanings("00-database-info").is_empty(),
            "the notes about the file are not a word"
        );
        assert_eq!(dictionary.words(), 3);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_compressed_dictionary_reads_the_same() {
        let (dictionary, directory) = small("dictzip", true);
        assert!(dictionary.pieces.is_some(), "the dictzip header was not read");
        let meanings = dictionary.meanings("house");
        assert_eq!(meanings.len(), 2);
        assert_eq!(meanings[0].translations, vec!["Haus"]);
        assert_eq!(dictionary.meanings("happy")[0].translations, vec!["glücklich", "fröhlich"]);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn the_languages_are_read_off_the_name() {
        assert_eq!(languages_of("freedict-eng-deu"), Some(("en".to_owned(), "de".to_owned())));
        assert_eq!(languages_of("deu-eng"), Some(("de".to_owned(), "en".to_owned())));
        assert_eq!(languages_of("eng-ces"), Some(("en".to_owned(), "cs".to_owned())));
        assert_eq!(languages_of("gcide"), None, "a dictionary of one language is not a pair");
        assert_eq!(language_name("de-DE"), "German");
        assert_eq!(language_name("xx"), "xx");
    }

    #[test]
    fn a_translation_line_is_split_at_commas_outside_brackets() {
        assert_eq!(
            split_translations("glücklich, fröhlich <adj>"),
            vec!["glücklich", "fröhlich <adj>"]
        );
        assert_eq!(
            split_translations("House-Musik <fem>, House <fem> [mus.]"),
            vec!["House-Musik <fem>", "House <fem> [mus.]"]
        );
        assert_eq!(
            split_translations("etw. (mit jdm., etw.) teilen"),
            vec!["etw. (mit jdm., etw.) teilen"]
        );
        let (word, tags) = strip_tags("House <fem> [mus.]");
        assert_eq!(word, "House");
        assert_eq!(tags, vec![('<', "fem".to_owned()), ('[', "mus.".to_owned())]);
    }
}

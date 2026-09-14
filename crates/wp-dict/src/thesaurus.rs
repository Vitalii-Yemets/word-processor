//! Reading the open thesaurus format: what else a word could have been.
//!
//! # What a thesaurus is, and is not
//!
//! A list of the words that mean nearly what a word means, grouped by which
//! of its meanings they share — because "bright" is one thing said of a lamp
//! and another said of a child, and a synonym of the one is no use for the
//! other. Each group has a part of speech and a name, which is the first word
//! in it, and some of the words in it are marked as antonyms, or as broader or
//! narrower than the word itself.
//!
//! The format is MyThes's, which is what LibreOffice reads and what the free
//! thesauri are published in. Two files: a `.dat` holding every entry, and an
//! `.idx` saying where in it each word begins, so that a word can be found
//! without reading eighteen megabytes to look for it. On the same terms as the
//! dictionaries, none is shipped with this program.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::Error;

/// One meaning of a word, and the words that share it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sense {
    /// What kind of word it is in this meaning: "noun", "verb", "adj".
    pub part_of_speech: String,
    /// The first word of the group, which is what the group goes by.
    pub meaning: String,
    /// The words that mean this, the meaning itself first.
    pub synonyms: Vec<String>,
    /// The words that mean the opposite, where the file names any.
    pub antonyms: Vec<String>,
}

/// A thesaurus, open and ready to be asked.
#[derive(Debug)]
pub struct Thesaurus {
    data: PathBuf,
    /// Where each word's entry begins in the data file. Lower-cased, which is
    /// how the file keys them.
    offsets: HashMap<String, u64>,
    latin1: bool,
}

impl Thesaurus {
    /// Opens a thesaurus given its data file; the index is beside it under
    /// the same name.
    pub fn open(data: &Path) -> Result<Self, Error> {
        let index = data.with_extension("idx");
        let file = File::open(&index).map_err(|error| Error::NotReadable(error.to_string()))?;
        let mut lines = BufReader::new(file).split(b'\n');

        // The first line says the encoding; the second how many follow.
        let encoding = lines
            .next()
            .and_then(Result::ok)
            .map(|line| String::from_utf8_lossy(&line).trim().to_ascii_lowercase())
            .unwrap_or_default();
        let latin1 = match encoding.as_str() {
            "utf-8" | "utf8" => false,
            "iso8859-1" | "iso-8859-1" | "latin1" => true,
            other => return Err(Error::UnknownEncoding(other.to_owned())),
        };
        let _count = lines.next();

        let mut offsets = HashMap::new();
        for line in lines.map_while(Result::ok) {
            let text = decode(&line, latin1);
            let Some((word, offset)) = text.trim_end().rsplit_once('|') else { continue };
            let Ok(offset) = offset.parse::<u64>() else { continue };
            offsets.insert(word.to_lowercase(), offset);
        }
        if offsets.is_empty() {
            return Err(Error::NotReadable("the index holds no words".to_owned()));
        }
        Ok(Self { data: data.to_path_buf(), offsets, latin1 })
    }

    /// How many words it knows.
    #[must_use]
    pub fn words(&self) -> usize {
        self.offsets.len()
    }

    /// The meanings of a word, and the words that share each.
    ///
    /// Empty for a word the thesaurus has nothing to say about, which includes
    /// every word in another language.
    #[must_use]
    pub fn senses(&self, word: &str) -> Vec<Sense> {
        let key = word.trim().to_lowercase();
        let Some(offset) = self.offsets.get(&key) else { return Vec::new() };
        self.read_entry(*offset).unwrap_or_default()
    }

    /// Reads one entry out of the data file: a line naming the word and how
    /// many meanings follow, then that many lines of meanings.
    fn read_entry(&self, offset: u64) -> Option<Vec<Sense>> {
        let mut file = File::open(&self.data).ok()?;
        file.seek(SeekFrom::Start(offset)).ok()?;
        // An entry is a few hundred bytes; the largest are a few thousand.
        let mut buffer = vec![0u8; 16 * 1024];
        let read = file.read(&mut buffer).ok()?;
        buffer.truncate(read);
        let text = decode(&buffer, self.latin1);

        let mut lines = text.lines();
        let header = lines.next()?;
        let (_, count) = header.rsplit_once('|')?;
        let count: usize = count.trim().parse().ok()?;

        let mut senses = Vec::with_capacity(count);
        for line in lines.take(count) {
            let mut fields = line.split('|');
            let part = fields.next().unwrap_or_default().trim_matches(|c| c == '(' || c == ')');
            let mut synonyms = Vec::new();
            let mut antonyms = Vec::new();
            for field in fields {
                let field = field.trim();
                if field.is_empty() {
                    continue;
                }
                // A word may carry a note in brackets saying how it relates:
                // an antonym, a broader or narrower term, a similar one. The
                // antonyms are kept apart; the rest are synonyms of a kind.
                let (term, note) = match field.rsplit_once(" (") {
                    Some((term, note)) if note.ends_with(')') => (term, note.trim_end_matches(')')),
                    _ => (field, ""),
                };
                if note == "antonym" {
                    antonyms.push(term.to_owned());
                } else {
                    synonyms.push(term.to_owned());
                }
            }
            let Some(meaning) = synonyms.first().cloned() else { continue };
            senses.push(Sense { part_of_speech: part.to_owned(), meaning, synonyms, antonyms });
        }
        Some(senses)
    }
}

/// Bytes to text, in whichever of the two encodings the file said.
fn decode(bytes: &[u8], latin1: bool) -> String {
    if latin1 {
        bytes.iter().map(|byte| *byte as char).collect()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Every thesaurus installed on this machine, by the language it is for.
///
/// Looked for where the dictionaries are, and where LibreOffice keeps its own;
/// the file is named `th_<language>_v2.dat`, and the language is what is
/// between.
#[must_use]
pub fn installed() -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    let mut places = crate::search_paths();
    places.push(PathBuf::from("/usr/share/mythes"));

    for directory in places {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("dat") {
                continue;
            }
            if !path.with_extension("idx").exists() {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else { continue };
            let language =
                stem.strip_prefix("th_").unwrap_or(stem).trim_end_matches("_v2").replace('_', "-");
            if found.iter().any(|(held, _)| *held == language) {
                continue;
            }
            found.push((language, path));
        }
    }
    found.sort_by(|one, other| one.0.cmp(&other.0));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thesaurus of two words, written to disk the way the format is.
    /// Each test gets a directory of its own: the tests run side by side,
    /// and one tidying up while another reads is a test that fails for no
    /// reason.
    fn small(name: &str) -> (Thesaurus, std::path::PathBuf) {
        let directory = std::env::temp_dir().join(format!("wp-thes-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&directory);
        let data = directory.join("th_en_test_v2.dat");
        let index = directory.join("th_en_test_v2.idx");

        let mut text = String::from("UTF-8\n");
        let bright_at = text.len();
        text.push_str("bright|2\n");
        text.push_str("(adj)|bright|brilliant|vivid|dim (antonym)\n");
        text.push_str("(adj)|clever|smart|intelligent (similar term)|dull (antonym)\n");
        let happy_at = text.len();
        text.push_str("happy|1\n");
        text.push_str("(adj)|happy|glad|joyful|unhappy (antonym)\n");
        std::fs::write(&data, &text).unwrap();
        std::fs::write(&index, format!("UTF-8\n2\nbright|{bright_at}\nhappy|{happy_at}\n"))
            .unwrap();

        (Thesaurus::open(&data).expect("a thesaurus"), directory)
    }

    #[test]
    fn a_word_has_its_meanings_and_the_words_that_share_them() {
        let (thesaurus, directory) = small("meanings");
        let senses = thesaurus.senses("bright");
        assert_eq!(senses.len(), 2);
        assert_eq!(senses[0].part_of_speech, "adj");
        assert_eq!(senses[0].meaning, "bright");
        assert_eq!(senses[0].synonyms, vec!["bright", "brilliant", "vivid"]);
        assert_eq!(senses[0].antonyms, vec!["dim"]);
        assert_eq!(senses[1].meaning, "clever");
        assert_eq!(senses[1].synonyms, vec!["clever", "smart", "intelligent"]);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_word_is_found_however_it_is_capitalised() {
        let (thesaurus, directory) = small("case");
        assert_eq!(thesaurus.senses("Happy").len(), 1);
        assert_eq!(thesaurus.senses("HAPPY")[0].synonyms[1], "glad");
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_word_it_does_not_know_has_nothing_said_about_it() {
        let (thesaurus, directory) = small("unknown");
        assert!(thesaurus.senses("xylophone").is_empty());
        assert!(thesaurus.senses("").is_empty());
        let _ = std::fs::remove_dir_all(directory);
    }
}

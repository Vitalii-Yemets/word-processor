//! What the program remembers between one run and the next.
//!
//! # Why any of this is kept outside the document
//!
//! Because it is not about the document. Whether the window is dark, whether
//! the rulers are showing, how far the page is magnified — none of that belongs
//! in a `.docx`, and writing it there would change a colleague's screen when
//! they opened the file. Word keeps the same things in the registry and in
//! `Normal.dotm`; this keeps them in one small text file.
//!
//! # Why a text file of `key = value`
//!
//! It can be read by a person, edited by a person, and deleted by a person who
//! wants the program back the way it started. A binary format would need a
//! reader written before any of that were possible, and there are six settings.
//!
//! A line nobody here understands is left alone rather than thrown away, so a
//! file written by a later version survives being opened by an earlier one.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The name of the folder and the file inside it.
const FOLDER: &str = "WordProcessor";
const FILE: &str = "settings.txt";
/// What a macro's key is called, so it can be told from a setting.
const MACRO_PREFIX: &str = "macro.";
/// What a remembered document's key is called.
///
/// Numbered rather than run together on one line, because a path may contain a
/// comma and a list separated by commas would then be read back wrongly. The
/// number is the position, so the order survives as well as the paths.
const RECENT_PREFIX: &str = "recent.";

/// How many documents are remembered.
///
/// Word's own setting is fifty, and its list scrolls; so does the one on the
/// Open page. Fifty of them is four thousand characters of settings file, which
/// is nothing.
pub const RECENT_LIMIT: usize = 50;

/// What was remembered, and what is to be remembered next time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    /// Whether the window is dark. Nothing means the program has never been
    /// told, and picks for itself.
    pub dark: Option<bool>,
    pub rulers: Option<bool>,
    pub navigation: Option<bool>,
    /// How far the page is magnified, as a percentage.
    pub zoom: Option<f32>,
    /// The colour scheme and font pair new documents are made with, by name.
    pub theme_colors: Option<String>,
    pub theme_fonts: Option<String>,
    /// The parts of the strip along the bottom that have been switched off,
    /// by name. Empty means the strip is as it comes.
    pub status_off: Vec<String>,
    /// Whether the formatting marks are showing.
    pub marks: Option<bool>,
    /// Whether spelling is checked as you type.
    pub proofing: Option<bool>,
    /// Whether the grid behind the page is drawn.
    pub gridlines: Option<bool>,
    /// Whether the white space between one page and the next is shown.
    ///
    /// Word's wording, and the opposite of what the editor keeps: it remembers
    /// whether the pages are joined.
    pub white_space: Option<bool>,
    /// What unit measurements are shown in, by name. See [`crate::measure`].
    pub unit: Option<String>,
    /// The documents opened lately, the most recent first.
    ///
    /// Kept as written rather than as paths, because a path that no longer
    /// exists is still worth showing: Word shows it too, and says so when it is
    /// pressed. Throwing the line away the moment a memory stick is unplugged
    /// would lose the only record of where the document was.
    pub recent: Vec<String>,
    /// Everything the file said that this version does not know about, so that
    /// saving does not throw away a later version's settings.
    unknown: BTreeMap<String, String>,
}

impl Settings {
    /// Reads the file, or gives back nothing remembered if there is none.
    #[must_use]
    pub fn load() -> Self {
        let Some(path) = Self::path() else { return Self::default() };
        let Ok(text) = std::fs::read_to_string(path) else { return Self::default() };
        Self::parse(&text)
    }

    /// Writes the file, making the folder if it is not there.
    ///
    /// Failure is silent on purpose: a program that cannot write its
    /// preferences should still let a person write their document.
    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = std::fs::write(path, self.to_text());
    }

    /// Where the file lives on this machine.
    #[must_use]
    pub fn path() -> Option<PathBuf> {
        // Windows keeps a program's settings in the roaming profile, so they
        // follow the person from one machine to another.
        if let Ok(app_data) = std::env::var("APPDATA") {
            if !app_data.is_empty() {
                return Some(PathBuf::from(app_data).join(FOLDER).join(FILE));
            }
        }
        // Elsewhere, the directory the desktop specification names.
        if let Ok(config) = std::env::var("XDG_CONFIG_HOME") {
            if !config.is_empty() {
                return Some(PathBuf::from(config).join("word-processor").join(FILE));
            }
        }
        let home = std::env::var("HOME").ok().filter(|value| !value.is_empty())?;
        Some(PathBuf::from(home).join(".config").join("word-processor").join(FILE))
    }

    /// The names of every macro that has been recorded, in order.
    #[must_use]
    pub fn macro_names(&self) -> Vec<String> {
        self.unknown
            .keys()
            .filter_map(|key| key.strip_prefix(MACRO_PREFIX))
            .map(str::to_owned)
            .collect()
    }

    /// What one macro does, as the line it was written down as.
    #[must_use]
    pub fn macro_steps(&self, name: &str) -> Option<String> {
        self.unknown.get(&format!("{MACRO_PREFIX}{name}")).cloned()
    }

    /// Records a macro under a name, replacing one of the same name.
    pub fn set_macro(&mut self, name: &str, steps: &str) {
        self.unknown.insert(format!("{MACRO_PREFIX}{name}"), steps.to_owned());
    }

    /// Puts a document at the top of the list of the ones opened lately.
    ///
    /// A document that is already on the list moves to the top rather than
    /// appearing twice — opening the same file three times must not fill the
    /// list with it. Returns whether anything changed, so a caller need not
    /// write the file when nothing did.
    pub fn remember(&mut self, path: &std::path::Path) -> bool {
        let Some(written) = path.to_str() else { return false };
        if self.recent.first().is_some_and(|first| first == written) {
            return false;
        }
        self.recent.retain(|found| found != written);
        self.recent.insert(0, written.to_owned());
        self.recent.truncate(RECENT_LIMIT);
        true
    }
    /// Reads the settings out of the text of the file.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        // Gathered with their numbers and sorted afterwards, so a file whose
        // lines have been moved about by hand still gives the list back in the
        // order it was written in.
        let mut recent: Vec<(usize, String)> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "dark" => settings.dark = parse_flag(value),
                "rulers" => settings.rulers = parse_flag(value),
                "navigation" => settings.navigation = parse_flag(value),
                "zoom" => settings.zoom = value.parse().ok(),
                "marks" => settings.marks = parse_flag(value),
                "proofing" => settings.proofing = parse_flag(value),
                "gridlines" => settings.gridlines = parse_flag(value),
                "white-space" => settings.white_space = parse_flag(value),
                "unit" => settings.unit = Some(value.to_owned()),
                "theme-colors" => settings.theme_colors = Some(value.to_owned()),
                "theme-fonts" => settings.theme_fonts = Some(value.to_owned()),
                "status-off" => {
                    settings.status_off = value
                        .split(',')
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .map(str::to_owned)
                        .collect();
                }
                other => {
                    match other.strip_prefix(RECENT_PREFIX).and_then(|at| at.parse::<usize>().ok())
                    {
                        Some(at) if !value.is_empty() => recent.push((at, value.to_owned())),
                        // Anything else with that prefix is not a place in the
                        // list, and is kept as it is rather than guessed at.
                        _ => {
                            settings.unknown.insert(other.to_owned(), value.to_owned());
                        }
                    }
                }
            }
        }
        recent.sort_by_key(|(at, _)| *at);
        settings.recent = recent.into_iter().map(|(_, path)| path).collect();
        settings.recent.truncate(RECENT_LIMIT);
        settings
    }

    /// The text to write back.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::from("# Settings for the word processor.\n");
        let mut write = |key: &str, value: String| {
            out.push_str(key);
            out.push_str(" = ");
            out.push_str(&value);
            out.push('\n');
        };

        if let Some(dark) = self.dark {
            write("dark", flag(dark));
        }
        if let Some(rulers) = self.rulers {
            write("rulers", flag(rulers));
        }
        if let Some(navigation) = self.navigation {
            write("navigation", flag(navigation));
        }
        if let Some(zoom) = self.zoom {
            write("zoom", format!("{zoom:.0}"));
        }
        for (key, flagged) in [
            ("marks", self.marks),
            ("proofing", self.proofing),
            ("gridlines", self.gridlines),
            ("white-space", self.white_space),
        ] {
            if let Some(on) = flagged {
                write(key, flag(on));
            }
        }
        if let Some(unit) = &self.unit {
            write("unit", unit.clone());
        }
        if let Some(name) = &self.theme_colors {
            write("theme-colors", name.clone());
        }
        if let Some(name) = &self.theme_fonts {
            write("theme-fonts", name.clone());
        }
        if !self.status_off.is_empty() {
            write("status-off", self.status_off.join(", "));
        }
        for (at, path) in self.recent.iter().enumerate() {
            write(&format!("{RECENT_PREFIX}{at}"), path.clone());
        }
        for (key, value) in &self.unknown {
            write(key, value.clone());
        }
        out
    }
}

fn parse_flag(value: &str) -> Option<bool> {
    match value {
        "yes" | "true" | "on" | "1" => Some(true),
        "no" | "false" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn flag(state: bool) -> String {
    if state {
        "yes".to_owned()
    } else {
        "no".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_remembered_is_nothing_set() {
        assert_eq!(Settings::parse(""), Settings::default());
    }

    #[test]
    fn every_setting_survives_being_written_and_read_back() {
        let settings = Settings {
            dark: Some(true),
            rulers: Some(false),
            navigation: Some(true),
            zoom: Some(140.0),
            theme_colors: Some("Blue".to_owned()),
            theme_fonts: Some("Georgia".to_owned()),
            status_off: vec!["Language".to_owned(), "Zoom Slider".to_owned()],
            marks: Some(true),
            proofing: Some(false),
            gridlines: Some(true),
            white_space: Some(false),
            unit: Some("centimetres".to_owned()),
            recent: vec![
                "C:\\Documents\\Report, final.docx".to_owned(),
                "/home/somebody/notes.docx".to_owned(),
            ],
            unknown: BTreeMap::new(),
        };
        assert_eq!(Settings::parse(&settings.to_text()), settings);
    }

    #[test]
    fn a_document_opened_again_moves_up_rather_than_appearing_twice() {
        let mut settings = Settings::default();
        assert!(settings.remember(std::path::Path::new("/one.docx")));
        assert!(settings.remember(std::path::Path::new("/two.docx")));
        assert!(settings.remember(std::path::Path::new("/one.docx")));
        assert_eq!(settings.recent, vec!["/one.docx".to_owned(), "/two.docx".to_owned()]);

        // And the one already at the top changes nothing at all, so the
        // settings file is not written on every save.
        assert!(!settings.remember(std::path::Path::new("/one.docx")));
    }

    #[test]
    fn the_list_of_documents_stops_where_word_stops() {
        let mut settings = Settings::default();
        for number in 0..RECENT_LIMIT + 10 {
            settings.remember(std::path::Path::new(&format!("/{number}.docx")));
        }
        assert_eq!(settings.recent.len(), RECENT_LIMIT);
        // The most recent is the one at the top.
        assert_eq!(settings.recent[0], format!("/{}.docx", RECENT_LIMIT + 9));
    }

    #[test]
    fn a_flag_can_be_written_several_ways_and_still_be_read() {
        for text in ["dark = yes", "dark = true", "dark = on", "dark = 1"] {
            assert_eq!(Settings::parse(text).dark, Some(true), "{text}");
        }
        for text in ["dark = no", "dark = false", "dark = off", "dark = 0"] {
            assert_eq!(Settings::parse(text).dark, Some(false), "{text}");
        }
    }

    #[test]
    fn a_flag_saying_something_else_is_not_a_flag() {
        assert_eq!(Settings::parse("dark = perhaps").dark, None);
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let settings = Settings::parse("# a note\n\n   \nzoom = 90\n");
        assert_eq!(settings.zoom, Some(90.0));
    }

    #[test]
    fn a_setting_this_version_does_not_know_is_kept_rather_than_lost() {
        // A later version writes something; this one opens the file, changes
        // the theme and saves. The later version's setting must still be there.
        let settings = Settings::parse("dark = yes\nribbon-collapsed = yes\n");
        let written = settings.to_text();
        assert!(written.contains("ribbon-collapsed = yes"), "got {written}");
    }

    #[test]
    fn a_line_that_is_not_a_setting_at_all_is_ignored() {
        let settings = Settings::parse("this line has no equals sign\ndark = yes\n");
        assert_eq!(settings.dark, Some(true));
    }

    #[test]
    fn the_file_has_somewhere_to_live() {
        // On a machine with neither APPDATA nor HOME there is nowhere, and the
        // program carries on without remembering anything. Everywhere this is
        // ever run there is one or the other.
        if std::env::var("APPDATA").is_ok() || std::env::var("HOME").is_ok() {
            let path = Settings::path().expect("a path");
            assert!(path.ends_with(FILE), "got {}", path.display());
        }
    }
}

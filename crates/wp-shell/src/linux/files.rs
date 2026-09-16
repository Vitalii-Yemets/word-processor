//! The files a Linux desktop knows this program by.
//!
//! Both halves are files a program writes, and both are specified rather
//! than invented: the list of documents opened lately is
//! `recently-used.xbel` in the data directory, which is what a file
//! manager's Recent place reads; and which program opens a kind of file is
//! a desktop entry in `applications` naming the media types it takes,
//! together with `mimeapps.list` saying which entry is the one to use.
//! Writing them is the whole of it — there is no service to ask, and the
//! desktop notices because it watches those directories.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::files::Kind;

/// What this program's desktop entry is called. The name is the identity: a
/// `mimeapps.list` names the entry by this file name and nothing else.
const ENTRY: &str = "word-processor.desktop";

/// `$XDG_DATA_HOME`, or what the specification says it is when unset.
fn data_home() -> Option<PathBuf> {
    if let Ok(data) = std::env::var("XDG_DATA_HOME") {
        if !data.is_empty() {
            return Some(PathBuf::from(data));
        }
    }
    home().map(|home| home.join(".local").join("share"))
}

/// `$XDG_CONFIG_HOME`, or what the specification says it is when unset.
fn config_home() -> Option<PathBuf> {
    if let Ok(config) = std::env::var("XDG_CONFIG_HOME") {
        if !config.is_empty() {
            return Some(PathBuf::from(config));
        }
    }
    home().map(|home| home.join(".config"))
}

fn home() -> Option<PathBuf> {
    std::env::var("HOME").ok().filter(|home| !home.is_empty()).map(PathBuf::from)
}

/// A moment as a date and time in the form the bookmark file wants:
/// `2026-09-16T11:22:33Z`, which is the whole of the format it accepts.
fn stamp(at: SystemTime) -> String {
    let seconds = at.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The date a count of days since 1970 lands on. The arithmetic is the
/// standard one: shift the era to start in March so that the leap day is
/// the last day of the year and no month needs a special case.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 { shifted } else { shifted - 146_096 } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// A path as a `file:` address, with everything that is not allowed in one
/// written as a percent escape.
fn file_uri(path: &Path) -> String {
    let text = path.display().to_string();
    let mut out = String::from("file://");
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// Text as it can stand inside an XML attribute.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(character),
        }
    }
    out
}

/// How many bookmarks are kept. The specification names no limit; a file
/// that grows without one is a file that is read more slowly every day.
const RECENT_LIMIT: usize = 100;

pub(crate) fn remember_document(path: &Path, media_type: &str) {
    let Some(data) = data_home() else { return };
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let file = data.join("recently-used.xbel");
    let _ = std::fs::create_dir_all(&data);
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let uri = file_uri(&path);
    let now = stamp(SystemTime::now());
    let bookmark = one_bookmark(&uri, media_type, &now);
    let written = with_bookmark(&existing, &uri, &bookmark);
    let _ = std::fs::write(&file, written);
}

/// One bookmark, as this program writes them.
fn one_bookmark(uri: &str, media_type: &str, now: &str) -> String {
    let media_type = if media_type.is_empty() { "application/octet-stream" } else { media_type };
    format!(
        concat!(
            "  <bookmark href=\"{uri}\" added=\"{now}\" modified=\"{now}\" visited=\"{now}\">\n",
            "    <info>\n",
            "      <metadata owner=\"http://freedesktop.org\">\n",
            "        <mime:mime-type type=\"{media_type}\"/>\n",
            "        <bookmark:applications>\n",
            "          <bookmark:application name=\"Word Processor\" ",
            "exec=\"&apos;word-processor %u&apos;\" modified=\"{now}\" count=\"1\"/>\n",
            "        </bookmark:applications>\n",
            "      </metadata>\n",
            "    </info>\n",
            "  </bookmark>\n"
        ),
        uri = escape(uri),
        now = now,
        media_type = escape(media_type)
    )
}

/// The file with this bookmark at the top and any older one for the same
/// document taken out.
///
/// Kept as text rather than parsed into a tree: the file belongs to every
/// program on the desktop, and one that rewrites it wholesale throws away
/// whatever a newer specification has since put in it. Only the bookmark
/// for this document is touched.
fn with_bookmark(existing: &str, uri: &str, bookmark: &str) -> String {
    const HEAD: &str = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<xbel version=\"1.0\"\n",
        "      xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\"\n",
        "      xmlns:mime=\"http://www.freedesktop.org/standards/shared-mime-info\">\n"
    );
    let mut kept: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut inside = false;
    for line in existing.lines() {
        if line.trim_start().starts_with("<bookmark ") {
            inside = true;
            current.clear();
        }
        if inside {
            current.push_str(line);
            current.push('\n');
            if line.trim_start().starts_with("</bookmark>")
                || line.trim_end().ends_with("/>") && line.trim_start().starts_with("<bookmark ")
            {
                inside = false;
                let escaped = escape(uri);
                if !current.contains(&format!("href=\"{escaped}\"")) {
                    kept.push(core::mem::take(&mut current));
                }
                current.clear();
            }
        }
    }
    kept.truncate(RECENT_LIMIT - 1);
    let mut out = String::from(HEAD);
    out.push_str(bookmark);
    for entry in kept {
        out.push_str(&entry);
    }
    out.push_str("</xbel>\n");
    out
}

/// The desktop entry this program is known by, as its text.
fn desktop_entry(program: &Path, program_name: &str, kinds: &[Kind]) -> String {
    let types: Vec<&str> = kinds.iter().map(|kind| kind.media_type).collect();
    format!(
        concat!(
            "[Desktop Entry]\n",
            "Type=Application\n",
            "Name={name}\n",
            "Comment=Writes and reads Word documents\n",
            "Exec={program} %f\n",
            "Terminal=false\n",
            "Categories=Office;WordProcessor;\n",
            "MimeType={types};\n"
        ),
        name = program_name,
        program = program.display(),
        types = types.join(";")
    )
}

pub(crate) fn associate_kinds(kinds: &[Kind], program_name: &str) -> bool {
    let (Some(data), Some(config)) = (data_home(), config_home()) else { return false };
    let Ok(program) = std::env::current_exe() else { return false };
    let applications = data.join("applications");
    if std::fs::create_dir_all(&applications).is_err() {
        return false;
    }
    let entry = desktop_entry(&program, program_name, kinds);
    if std::fs::write(applications.join(ENTRY), entry).is_err() {
        return false;
    }

    // Which entry opens what. Both lists: one says this program can open the
    // kind, the other that it is the one to use.
    let file = config.join("mimeapps.list");
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let types: Vec<&str> = kinds.iter().map(|kind| kind.media_type).collect();
    let claimed: Vec<&str> =
        kinds.iter().filter(|kind| kind.becomes_default).map(|kind| kind.media_type).collect();
    let written = with_associations(&existing, &types, &claimed);
    let _ = std::fs::create_dir_all(&config);
    if std::fs::write(&file, written).is_err() {
        return false;
    }

    // The desktop keeps an index of which entry takes which kind. Where the
    // tool to rebuild it is installed the index is rebuilt now; where it is
    // not, the desktop rebuilds it itself when it next looks.
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&applications)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    true
}

/// `mimeapps.list` with this program named for these kinds, leaving every
/// other line of it as it was.
fn with_associations(existing: &str, types: &[&str], claimed: &[&str]) -> String {
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    let mut current = String::new();
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current = trimmed.to_owned();
            sections.push((current.clone(), Vec::new()));
            continue;
        }
        if current.is_empty() {
            // Anything before the first heading belongs to no section; keep
            // it at the top under a heading of its own name.
            sections.push((String::new(), vec![line.to_owned()]));
            continue;
        }
        if let Some((_, lines)) = sections.iter_mut().rev().find(|(name, _)| *name == current) {
            lines.push(line.to_owned());
        }
    }
    for (heading, wanted) in [("[Default Applications]", claimed), ("[Added Associations]", types)]
    {
        if wanted.is_empty() {
            continue;
        }
        if !sections.iter().any(|(name, _)| name == heading) {
            sections.push((heading.to_owned(), Vec::new()));
        }
        let Some((_, lines)) = sections.iter_mut().find(|(name, _)| name == heading) else {
            continue;
        };
        for media_type in wanted {
            lines.retain(|line| !line.trim_start().starts_with(&format!("{media_type}=")));
            lines.push(format!("{media_type}={ENTRY}"));
        }
    }
    let mut out = String::new();
    for (heading, lines) in sections {
        if !heading.is_empty() {
            out.push_str(&heading);
            out.push('\n');
        }
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            out.push_str(&line);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

pub(crate) fn opens_kind(kind: &Kind) -> bool {
    let Some(config) = config_home() else { return false };
    let Ok(text) = std::fs::read_to_string(config.join("mimeapps.list")) else { return false };
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == "[Default Applications]";
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((name, entries)) = trimmed.split_once('=') {
            if name.trim() == kind.media_type {
                // The first entry that exists wins, which is what a desktop
                // does with the list; this program is the default only if it
                // is that one.
                return entries.split(';').next().map(str::trim) == Some(ENTRY);
            }
        }
    }
    false
}

pub(crate) fn choose_default_programs() -> bool {
    // A Linux desktop has no one page for this, and the program has already
    // made itself the default by writing the list. Nothing to open.
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_is_written_as_a_date_and_a_time() {
        assert_eq!(stamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            stamp(UNIX_EPOCH + std::time::Duration::from_secs(1_757_939_045)),
            "2025-09-15T12:24:05Z"
        );
        // A leap day, which is where the arithmetic would show it was wrong.
        assert_eq!(
            stamp(UNIX_EPOCH + std::time::Duration::from_secs(1_709_164_800)),
            "2024-02-29T00:00:00Z"
        );
    }

    #[test]
    fn a_path_becomes_an_address_with_its_spaces_escaped() {
        assert_eq!(
            file_uri(Path::new("/home/a/My Letter.docx")),
            "file:///home/a/My%20Letter.docx"
        );
        assert_eq!(file_uri(Path::new("/tmp/a&b.docx")), "file:///tmp/a%26b.docx");
    }

    #[test]
    fn a_document_goes_to_the_top_and_is_not_listed_twice() {
        let first =
            with_bookmark("", "file:///a.docx", &one_bookmark("file:///a.docx", "text/x", "T"));
        assert_eq!(first.matches("<bookmark ").count(), 1);
        let second =
            with_bookmark(&first, "file:///b.docx", &one_bookmark("file:///b.docx", "text/x", "T"));
        assert_eq!(second.matches("<bookmark ").count(), 2);
        let again = with_bookmark(
            &second,
            "file:///a.docx",
            &one_bookmark("file:///a.docx", "text/x", "T"),
        );
        assert_eq!(again.matches("<bookmark ").count(), 2, "the same document is not listed twice");
        let first_at = again.find("file:///a.docx").expect("the document opened again");
        let other_at = again.find("file:///b.docx").expect("the other document");
        assert!(first_at < other_at, "the one just opened is at the top");
    }

    #[test]
    fn the_other_programs_entries_survive_being_made_the_default() {
        let existing = concat!(
            "[Default Applications]\n",
            "image/png=viewer.desktop\n",
            "text/plain=editor.desktop\n",
            "\n",
            "[Added Associations]\n",
            "image/png=viewer.desktop;\n"
        );
        let written = with_associations(
            existing,
            &["text/plain", "application/msword"],
            &["application/msword"],
        );
        assert!(
            written.contains("image/png=viewer.desktop"),
            "another program's kind is left alone"
        );
        assert!(
            written.contains("text/plain=editor.desktop"),
            "a kind this program does not ask for keeps the program that had it"
        );
        assert!(
            written.contains("application/msword=word-processor.desktop"),
            "and a kind it does ask for is taken"
        );
        let defaults = written.split("[Added Associations]").next().expect("the first section");
        assert!(!defaults.contains("text/plain=word-processor.desktop"));
        assert!(written.contains("text/plain=word-processor.desktop"), "but it can still open one");
        assert_eq!(written.matches("[Default Applications]").count(), 1);
        assert_eq!(written.matches("[Added Associations]").count(), 1);
    }

    #[test]
    fn a_desktop_entry_names_every_kind_the_program_opens() {
        let kinds = [
            Kind {
                extension: ".docx",
                description: "Word Document",
                media_type: "application/x-a",
                becomes_default: true,
            },
            Kind {
                extension: ".rtf",
                description: "Rich Text",
                media_type: "text/rtf",
                becomes_default: true,
            },
        ];
        let entry = desktop_entry(Path::new("/usr/bin/word-processor"), "Word Processor", &kinds);
        assert!(entry.contains("Exec=/usr/bin/word-processor %f"));
        assert!(entry.contains("MimeType=application/x-a;text/rtf;"));
    }
}

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
//!
//! Each is written beside itself and then put in its place (see
//! [`wp_files`]), which is how the desktop's own library writes them too: the
//! recent list and `mimeapps.list` belong to every program on the desktop,
//! and one cut short halfway would take every other program's entries with
//! it.

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

/// The files a `text/uri-list` names: its `file:` addresses, a line each,
/// with the escapes an address is written with taken out. Lines that begin
/// with `#` are comments.
pub(crate) fn paths_of_uri_list(list: &str) -> Vec<std::path::PathBuf> {
    list.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.strip_prefix("file://"))
        .map(|rest| {
            // A host may stand before the path: `file://host/path`.
            let path =
                if rest.starts_with('/') { rest } else { &rest[rest.find('/').unwrap_or(0)..] };
            std::path::PathBuf::from(unescape(path))
        })
        .collect()
}

/// An address's percent escapes, as the bytes they stand for.
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let digit = |byte: u8| char::from(byte).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (digit(bytes[at + 1]), digit(bytes[at + 2])) {
                out.push((high * 16 + low) as u8);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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
    let _ = wp_files::replace_with(&file, written.as_bytes());
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

/// A path as it can stand in a desktop entry's `Exec` line: as it is where
/// it needs nothing, and otherwise in double quotes with the four characters
/// the specification reserves inside them escaped.
fn exec_quoted(program: &Path) -> String {
    let text = program.display().to_string();
    let plain = text.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+,:@".contains(c));
    if plain {
        return text;
    }
    let mut quoted = String::from("\"");
    for c in text.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    // A percent sign is a field code anywhere on the line, so it is doubled.
    quoted.replace('%', "%%")
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
        program = exec_quoted(program),
        types = types.join(";")
    )
}

pub(crate) fn associate_kinds(kinds: &[Kind], program_name: &str, program: &Path) -> bool {
    let (Some(data), Some(config)) = (data_home(), config_home()) else { return false };
    let applications = data.join("applications");
    if std::fs::create_dir_all(&applications).is_err() {
        return false;
    }
    let entry = desktop_entry(program, program_name, kinds);
    if wp_files::replace_with(&applications.join(ENTRY), entry.as_bytes()).is_err() {
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
    if wp_files::replace_with(&file, written.as_bytes()).is_err() {
        return false;
    }
    rebuild_index(&applications);
    true
}

/// Takes back everything [`associate_kinds`] wrote: the desktop entry, and
/// this program's name wherever `mimeapps.list` gives it — the rest of the
/// list as it was, and a kind that had another program before this one
/// took it going back to that one.
pub(crate) fn dissociate_kinds() -> bool {
    let (Some(data), Some(config)) = (data_home(), config_home()) else { return false };
    let applications = data.join("applications");
    let entry = applications.join(ENTRY);
    let mut all = match std::fs::remove_file(&entry) {
        Ok(()) => true,
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    };
    let file = config.join("mimeapps.list");
    if let Ok(existing) = std::fs::read_to_string(&file) {
        all &= wp_files::replace_with(&file, without_associations(&existing).as_bytes()).is_ok();
    }
    rebuild_index(&applications);
    all
}

/// The desktop keeps an index of which entry takes which kind. Where the
/// tool to rebuild it is installed the index is rebuilt now; where it is
/// not, the desktop rebuilds it itself when it next looks.
fn rebuild_index(applications: &Path) {
    let _ = std::process::Command::new("update-desktop-database")
        .arg(applications)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// `mimeapps.list` as its sections, each a heading and its lines, in order.
/// Anything before the first heading belongs to no section and is kept at
/// the top under an empty heading.
fn sections_of(existing: &str) -> Vec<(String, Vec<String>)> {
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
            sections.push((String::new(), vec![line.to_owned()]));
            continue;
        }
        if let Some((_, lines)) = sections.iter_mut().rev().find(|(name, _)| *name == current) {
            lines.push(line.to_owned());
        }
    }
    sections
}

/// The sections written out again, a blank line after each.
fn joined(sections: Vec<(String, Vec<String>)>) -> String {
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

/// The entries a line of the list names, with this program's taken out.
fn others_in(entries: &str) -> Vec<&str> {
    entries.split(';').map(str::trim).filter(|entry| !entry.is_empty() && *entry != ENTRY).collect()
}

/// `mimeapps.list` with this program named for these kinds, leaving every
/// other line of it as it was.
///
/// A line is a list, and the other programs on it stay on it: this one goes
/// first where it is to be the one that opens the kind, so that the others
/// are what the desktop falls back to, and last where it is only one that
/// can, so that the order the person had is kept.
fn with_associations(existing: &str, types: &[&str], claimed: &[&str]) -> String {
    let mut sections = sections_of(existing);
    for (heading, wanted, first) in
        [("[Default Applications]", claimed, true), ("[Added Associations]", types, false)]
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
            let found = lines.iter().position(|line| {
                line.split_once('=').is_some_and(|(name, _)| name.trim() == *media_type)
            });
            let Some(at) = found else {
                lines.push(format!("{media_type}={ENTRY};"));
                continue;
            };
            let entries = lines[at].split_once('=').map_or("", |(_, entries)| entries).to_owned();
            let others = others_in(&entries);
            let listed = if first {
                std::iter::once(ENTRY).chain(others).collect::<Vec<_>>()
            } else {
                others.into_iter().chain(std::iter::once(ENTRY)).collect()
            };
            lines[at] = format!("{media_type}={};", listed.join(";"));
        }
    }
    joined(sections)
}

/// `mimeapps.list` with this program's name taken off every line, and a
/// line that named no one else taken out.
fn without_associations(existing: &str) -> String {
    let mut sections = sections_of(existing);
    for (heading, lines) in &mut sections {
        if heading.is_empty() {
            continue;
        }
        lines.retain_mut(|line| {
            let Some((name, entries)) = line.split_once('=') else { return true };
            if !entries.split(';').any(|entry| entry.trim() == ENTRY) {
                return true;
            }
            let others = others_in(entries);
            if others.is_empty() {
                return false;
            }
            *line = format!("{}={};", name.trim(), others.join(";"));
            true
        });
    }
    joined(sections)
}

/// Whether the list makes this program's entry the one for the kind. The
/// program is named by the entry rather than by its path, and an entry that
/// is not there any more opens nothing whatever the list says.
pub(crate) fn opens_kind(kind: &Kind, _program: &Path) -> bool {
    let (Some(data), Some(config)) = (data_home(), config_home()) else { return false };
    if !data.join("applications").join(ENTRY).is_file() {
        return false;
    }
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

/// `$XDG_DATA_HOME/word-processor`: a program installed for one person
/// keeps its files in that person's data directory, beside the desktop
/// entry that names it.
pub(crate) fn install_folder() -> Option<PathBuf> {
    data_home().map(|data| data.join("word-processor"))
}

/// The desktop entry is the whole of it on Linux: it is what the desktop's
/// menu lists the program by as well as what the kinds open with, and there
/// is no list of installed programs to be put on.
pub(crate) fn register_installed(installed: &crate::install::Installed) -> bool {
    associate_kinds(crate::files::KINDS, crate::install::PROGRAM_NAME, &installed.program)
}

pub(crate) fn unregister_installed(_installed: &crate::install::Installed) -> bool {
    dissociate_kinds()
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
                is_template: false,
                in_new_menu: false,
            },
            Kind {
                extension: ".rtf",
                description: "Rich Text",
                media_type: "text/rtf",
                becomes_default: true,
                is_template: false,
                in_new_menu: false,
            },
        ];
        let entry = desktop_entry(Path::new("/usr/bin/word-processor"), "Word Processor", &kinds);
        assert!(entry.contains("Exec=/usr/bin/word-processor %f"));
        assert!(entry.contains("MimeType=application/x-a;text/rtf;"));
    }

    #[test]
    fn a_program_in_a_folder_with_a_space_is_quoted_on_the_exec_line() {
        assert_eq!(
            exec_quoted(Path::new("/home/Ann Lee/.local/share/word-processor/word-processor")),
            "\"/home/Ann Lee/.local/share/word-processor/word-processor\""
        );
        assert_eq!(exec_quoted(Path::new("/opt/a$b\\c")), "\"/opt/a\\$b\\\\c\"");
        assert_eq!(exec_quoted(Path::new("/opt/100%")), "\"/opt/100%%\"");
    }

    #[test]
    fn the_other_programs_on_a_line_stay_on_it() {
        let existing = concat!(
            "[Default Applications]\n",
            "application/msword=other.desktop;\n",
            "\n",
            "[Added Associations]\n",
            "text/plain=editor.desktop;viewer.desktop;\n"
        );
        let written = with_associations(
            existing,
            &["text/plain", "application/msword"],
            &["application/msword"],
        );
        assert!(
            written.contains("application/msword=word-processor.desktop;other.desktop;"),
            "the kind it takes is this one's first, and the other one's after it: {written}"
        );
        assert!(
            written.contains("text/plain=editor.desktop;viewer.desktop;word-processor.desktop;"),
            "and a kind it only can open keeps the order it had, this one last: {written}"
        );
        // Asked again, nothing is listed twice.
        let again = with_associations(
            &written,
            &["text/plain", "application/msword"],
            &["application/msword"],
        );
        assert_eq!(again.matches("word-processor.desktop").count(), 3, "{again}");
    }

    #[test]
    fn taking_the_program_off_gives_each_kind_back() {
        let before = concat!(
            "[Default Applications]\n",
            "application/msword=other.desktop;\n",
            "image/png=viewer.desktop\n",
            "\n",
            "[Added Associations]\n",
            "text/plain=editor.desktop;\n"
        );
        let with = with_associations(
            before,
            &["text/plain", "application/msword", "application/rtf"],
            &["application/msword", "application/rtf"],
        );
        let without = without_associations(&with);
        assert!(!without.contains("word-processor.desktop"), "{without}");
        assert!(
            without.contains("application/msword=other.desktop;"),
            "the kind goes back to the program that had it: {without}"
        );
        assert!(without.contains("image/png=viewer.desktop"), "{without}");
        assert!(without.contains("text/plain=editor.desktop;"), "{without}");
        assert!(
            !without.contains("application/rtf"),
            "and a kind only this program was named for is not named at all: {without}"
        );
    }
}

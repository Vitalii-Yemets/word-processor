//! AutoRecover: the copy of the work that outlives the program.
//!
//! # Why a copy and not a save
//!
//! Because the document on disk is the person's, and a program that wrote
//! over it every ten minutes would be a program that could not be told
//! "don't save". The copy goes somewhere else, under a name of the
//! program's own making, and the document is left exactly as the person
//! last saved it. That is what Word does, and why Word can offer both the
//! recovered version and the one on disk after a crash.
//!
//! # Why the copies left behind mean a crash
//!
//! A run that ends properly takes its copy away — on the last save, and
//! again on the way out. So a copy still lying there on the next start is a
//! run that did not end properly, which is the whole of how this knows.
//! (Word writes a lock file and compares process identifiers; the effect is
//! the same, and this way there is nothing to go stale.)
//!
//! # What is kept beside the copy
//!
//! The document itself as a Word file, and one small text file saying what
//! it was called, where it came from and when the copy was taken. Without
//! the second, a recovered document has no name to show and nowhere to go
//! back to.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::Editor;
use crate::settings::Settings;

/// How often a copy is written, in minutes, where nobody has said otherwise.
/// Word's own setting, and Word's own number.
pub const DEFAULT_MINUTES: u32 = 10;

/// What the file holding the copy is called; the document itself is the
/// same name with the extension of a Word document.
const SIDECAR: &str = "recover";

/// Where the copies are kept: beside the settings, in a folder of the
/// program's own, which is made when the first copy is written and emptied
/// as the copies are taken away.
#[must_use]
pub fn folder() -> Option<PathBuf> {
    Some(Settings::path()?.parent()?.join("recovery"))
}

/// One copy left behind by a run that did not end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovered {
    /// What the document was called.
    pub name: String,
    /// Where it came from, for a document that had been saved before.
    pub original: Option<PathBuf>,
    /// When the copy was taken, as the clock said it.
    pub saved: String,
    /// The copy itself.
    pub copy: PathBuf,
}

impl Recovered {
    /// The file saying what the copy is.
    #[must_use]
    pub fn sidecar(&self) -> PathBuf {
        self.copy.with_extension(SIDECAR)
    }

    /// The copy and its sidecar, gone.
    pub fn remove(&self) {
        let _ = std::fs::remove_file(&self.copy);
        let _ = std::fs::remove_file(self.sidecar());
    }

    /// What the pane shows under the name: when the copy was taken, in the
    /// date and the clock this machine writes.
    ///
    /// The copy itself records the moment the one way everything agrees
    /// on; this is the same moment as the person reads one.
    #[must_use]
    pub fn when(&self) -> String {
        crate::locale::moment(&self.saved)
    }
}

/// Every copy lying in the folder, newest first.
///
/// Called once, on the way in, before this run has written a copy of its
/// own — so everything found belongs to a run that did not end.
#[must_use]
pub fn found() -> Vec<Recovered> {
    let mine = format!("{}-", std::process::id());
    copies(|stem| !stem.starts_with(&mine))
}

/// Every copy kept of work that was not saved, at any time: what a run that
/// did not end left, and what "don't save" left in this run — Word's
/// Recover Unsaved Documents. All but the copy of the document being
/// edited now, which is not unsaved work left behind but work going on.
#[must_use]
pub fn unsaved_copies(current: &str) -> Vec<Recovered> {
    copies(|stem| stem != current)
}

/// The copies in the folder whose names pass, newest first.
fn copies(wanted: impl Fn(&str) -> bool) -> Vec<Recovered> {
    let Some(folder) = folder() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&folder) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some(SIDECAR) {
            continue;
        }
        let stem =
            path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        if !wanted(&stem) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Some(recovered) = read_sidecar(&text, &path) else { continue };
        if !recovered.copy.exists() {
            // A sidecar with no document beside it is the remains of a copy
            // half written; there is nothing in it to recover.
            let _ = std::fs::remove_file(&path);
            continue;
        }
        out.push(recovered);
    }
    out.sort_by(|left, right| right.saved.cmp(&left.saved));
    out
}

/// The sidecar's three lines, as a copy.
fn read_sidecar(text: &str, sidecar: &Path) -> Option<Recovered> {
    let mut name = String::new();
    let mut original = None;
    let mut saved = String::new();
    let mut copy = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "name" => name = value.to_owned(),
            "path" if !value.is_empty() => original = Some(PathBuf::from(value)),
            "saved" => saved = value.to_owned(),
            "copy" if !value.is_empty() => copy = Some(PathBuf::from(value)),
            _ => {}
        }
    }
    if name.is_empty() {
        return None;
    }
    let copy = copy.unwrap_or_else(|| sidecar.with_extension("docx"));
    Some(Recovered { name, original, saved, copy })
}

/// The sidecar's text for a copy about to be written.
fn write_sidecar(name: &str, original: Option<&Path>, saved: &str, copy: &Path) -> String {
    format!(
        "name = {name}\npath = {}\nsaved = {saved}\ncopy = {}\n",
        original.map(|path| path.display().to_string()).unwrap_or_default(),
        copy.display()
    )
}

impl Editor {
    /// Writes a copy if one is due, on the tick that finds it due.
    ///
    /// Nothing is written for a document with no changes in it: the copy
    /// exists to hold what is not on disk, and for a saved document that is
    /// nothing at all.
    pub(super) fn autorecover_tick(&mut self) {
        if !self.autosave || !self.document.is_modified() {
            return;
        }
        let every = Duration::from_secs(u64::from(self.autosave_minutes.max(1)) * 60);
        if self.autosaved.elapsed() < every {
            return;
        }
        self.write_recovery_copy();
    }

    /// Takes the copy now, whatever the clock says.
    pub(super) fn write_recovery_copy(&mut self) {
        self.autosaved = Instant::now();
        let Some(folder) = folder() else { return };
        if std::fs::create_dir_all(&folder).is_err() {
            return;
        }
        let Ok(bytes) = self.document.clone().save() else { return };
        let copy = folder.join(format!("{}.docx", self.recovery_name));
        // Written over the last copy, and a crash in the middle of the write
        // is exactly what the copy is kept for: so the last one stays whole
        // until the new one is. The temporary file ends in `.tmp`, which
        // [`copies`] passes over. See [`wp_files`].
        if wp_files::replace_with(&copy, &bytes).is_err() {
            return;
        }
        let sidecar = write_sidecar(
            &self.document_name(),
            self.file.as_deref(),
            &super::files::timestamp(),
            &copy,
        );
        // And what it is, the same way: an empty sidecar says no name, and a
        // copy with no name is not offered.
        let _ = wp_files::replace_with(&copy.with_extension(SIDECAR), sidecar.as_bytes());
        self.recovery_written = true;
    }

    /// Leaves a copy of the work behind on "don't save", for it to be got
    /// back later: at the next start, or from Manage Document.
    ///
    /// The copy left is this document's, and whatever is edited next is
    /// copied under a name of its own — under the same one it would be
    /// written over the first, which is what happened until it had one.
    pub(super) fn keep_unsaved_copy(&mut self) {
        self.write_recovery_copy();
        self.recovery_written = false;
        self.recovery_name = Self::new_recovery_name();
    }

    /// Takes this run's copy away, which is what a document being saved and
    /// a program being closed properly both mean.
    pub(super) fn drop_recovery_copy(&mut self) {
        if !self.recovery_written {
            return;
        }
        self.recovery_written = false;
        let Some(folder) = folder() else { return };
        let copy = folder.join(format!("{}.docx", self.recovery_name));
        let _ = std::fs::remove_file(&copy);
        let _ = std::fs::remove_file(copy.with_extension(SIDECAR));
    }

    /// The name this run's copy is written under: this program among all
    /// those running, and this run of it.
    #[must_use]
    pub(super) fn new_recovery_name() -> String {
        // The moment it started as well as the process, because a process
        // identifier comes round again on a machine left running.
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        // And a count, because one run can leave more than one copy behind
        // — "don't save", then another document — within the same second.
        static MADE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let made = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("{}-{started}-{made}", std::process::id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The settings folder, and so the recovery folder with it, moved
    /// somewhere a test can look inside.
    ///
    /// One test at a time, because where the folder is is read from the
    /// environment and the environment belongs to the whole program.
    fn in_a_folder_of_its_own<R>(name: &str, work: impl FnOnce() -> R) -> R {
        static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _held = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let folder =
            std::env::temp_dir().join(format!("wp-autorecover-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("somewhere to keep the copies");
        let (was_app_data, was_config) =
            (std::env::var("APPDATA").ok(), std::env::var("XDG_CONFIG_HOME").ok());
        std::env::set_var("APPDATA", &folder);
        std::env::set_var("XDG_CONFIG_HOME", &folder);
        let out = work();
        match was_app_data {
            Some(value) => std::env::set_var("APPDATA", value),
            None => std::env::remove_var("APPDATA"),
        }
        match was_config {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        let _ = std::fs::remove_dir_all(&folder);
        out
    }

    fn library() -> &'static wp_layout::FontLibrary {
        Box::leak(Box::new(wp_layout::FontLibrary::scan_system()))
    }

    /// An editor holding a document with something typed into it that has
    /// not been saved — which is the only state a copy is taken in.
    fn editor_with_unsaved_work() -> Editor {
        use wp_docx::model::{Block, Body, Paragraph};
        use wp_docx::Document;

        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Work that was never saved")));
        let document = Document::create(&body).expect("a document");
        let mut editor = Editor::new(library(), document, None);
        editor.document.mark_modified();
        editor
    }

    /// The copy renamed as though another run had left it: [`found`] passes
    /// over anything this very run wrote, which is what keeps a program from
    /// offering to recover the document it is editing.
    fn as_another_runs_copy(editor: &Editor) -> PathBuf {
        let recovery = folder().expect("a folder for the copies");
        let mine = recovery.join(format!("{}.docx", editor.recovery_name));
        let theirs = recovery.join("7-1000.docx");
        std::fs::rename(&mine, &theirs).expect("the copy");
        let sidecar = std::fs::read_to_string(mine.with_extension(SIDECAR)).expect("the sidecar");
        let sidecar = sidecar.replace(&mine.display().to_string(), &theirs.display().to_string());
        std::fs::write(theirs.with_extension(SIDECAR), sidecar).expect("writing the sidecar");
        let _ = std::fs::remove_file(mine.with_extension(SIDECAR));
        theirs
    }

    /// "Don't save" leaves a copy, and the next document's copy does not
    /// write over it; and it can be got back at once, from Manage Document,
    /// without waiting for the next start.
    #[test]
    fn a_copy_left_by_dont_save_is_kept_apart_and_offered_at_any_time() {
        in_a_folder_of_its_own("unsaved", || {
            use wp_docx::model::{Block, Body, Paragraph};

            let mut editor = editor_with_unsaved_work();
            editor.keep_unsaved_copy();
            let kept = folder().expect("the folder").join(format!(
                "{}.docx",
                unsaved_copies(&editor.recovery_name)
                    .first()
                    .and_then(|copy| copy
                        .copy
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned()))
                    .expect("the copy left")
            ));
            let first = std::fs::read(&kept).expect("the kept copy");

            // The next document, with work of its own, copied in its turn.
            let mut body = Body::default();
            body.blocks.push(Block::Paragraph(Paragraph::text("The next document")));
            let document = wp_docx::Document::create(&body).expect("a document");
            editor.set_document(document, None);
            editor.document.mark_modified();
            editor.write_recovery_copy();
            assert_eq!(std::fs::read(&kept).expect("still there"), first, "not written over");

            // Offered now: the copy left, and not the one being written.
            let offered = unsaved_copies(&editor.recovery_name);
            assert_eq!(offered.len(), 1, "{offered:?}");
            assert!(found().is_empty(), "and nothing this run left is a crash's to recover");
            let info = editor.info_page();
            assert_eq!(info.rows[3].title, "Manage Document");
            assert_eq!(info.rows[3].note, "Recover unsaved documents: 1 copy kept");
            editor.recover_unsaved();
            assert_eq!(editor.recovery.as_ref().map(|pane| pane.entries.len()), Some(1));
        });
    }

    #[test]
    fn a_copy_is_taken_of_work_that_is_not_on_disk_and_recovered_after_a_crash() {
        in_a_folder_of_its_own("crash", || {
            let mut editor = editor_with_unsaved_work();
            editor.write_recovery_copy();

            let recovery = folder().expect("a folder for the copies");
            let copy = recovery.join(format!("{}.docx", editor.recovery_name));
            assert!(copy.exists(), "the copy is written where a crash cannot take it");
            assert!(copy.with_extension(SIDECAR).exists(), "and beside it, what it is");
            assert!(found().is_empty(), "this run's own copy is not something to recover from");

            // The program stops here, without another tick and without a
            // save. What the next start sees is the copy, and nothing else.
            as_another_runs_copy(&editor);
            let waiting = found();
            assert_eq!(waiting.len(), 1, "the copy a run that did not end left behind");
            assert_eq!(waiting[0].name, "Document");
            assert_eq!(waiting[0].original, None, "it had never been saved anywhere");

            let mut started = editor_with_unsaved_work();
            started.show_recovered(waiting.clone());
            started.open_recovered(0);
            assert!(
                started.document.paragraph_text(0).is_some_and(|text| text.contains("never saved")),
                "the work is back"
            );
        });
    }

    #[test]
    fn saving_takes_the_copy_away_because_the_work_is_now_on_disk() {
        in_a_folder_of_its_own("saved", || {
            let mut editor = editor_with_unsaved_work();
            editor.write_recovery_copy();
            let recovery = folder().expect("a folder for the copies");
            let copy = recovery.join(format!("{}.docx", editor.recovery_name));
            assert!(copy.exists());

            let document =
                std::env::temp_dir().join(format!("wp-saved-{}.docx", std::process::id()));
            assert!(editor.write_document(&document), "the document is saved");
            assert!(!copy.exists(), "and the copy it stood in for is gone");
            assert!(!copy.with_extension(SIDECAR).exists());
            assert!(found().is_empty());
        });
    }

    #[test]
    fn a_document_with_nothing_unsaved_in_it_has_no_copy_taken() {
        in_a_folder_of_its_own("unchanged", || {
            let mut editor = editor_with_unsaved_work();
            editor.document.mark_saved().expect("saving");
            assert!(!editor.document.is_modified(), "nothing to lose");
            editor.autosave = true;
            editor.autosave_minutes = 1;
            editor.autorecover_tick();
            let recovery = folder().expect("a folder for the copies");
            assert!(!recovery.join(format!("{}.docx", editor.recovery_name)).exists());
            let _ = std::fs::remove_dir_all(recovery);
        });
    }

    #[test]
    fn a_sidecar_says_what_the_copy_is_and_reads_back_as_it_was_written() {
        let text = write_sidecar(
            "Letter.docx",
            Some(Path::new("/home/a/Letter.docx")),
            "2026-09-16T11:22:33Z",
            Path::new("/tmp/r/1-2.docx"),
        );
        let read = read_sidecar(&text, Path::new("/tmp/r/1-2.recover")).expect("a copy");
        assert_eq!(read.name, "Letter.docx");
        assert_eq!(read.original.as_deref(), Some(Path::new("/home/a/Letter.docx")));
        assert_eq!(read.copy, PathBuf::from("/tmp/r/1-2.docx"));
        assert_eq!(
            read.when(),
            crate::locale::moment("2026-09-16T11:22:33Z"),
            "shown as this machine writes a date and a time, without the seconds"
        );
        assert!(read.when().contains("11:22"), "the time is there: {}", read.when());
        assert!(!read.when().contains(":33"), "and the seconds are not");
    }

    #[test]
    fn a_document_that_was_never_saved_has_nowhere_to_go_back_to() {
        let text =
            write_sidecar("Document", None, "2026-09-16T11:22:33Z", Path::new("/tmp/r/1-2.docx"));
        let read = read_sidecar(&text, Path::new("/tmp/r/1-2.recover")).expect("a copy");
        assert_eq!(read.original, None);
        assert_eq!(read.name, "Document");
    }

    #[test]
    fn a_sidecar_with_no_name_is_not_a_copy() {
        assert_eq!(read_sidecar("saved = now\n", Path::new("/tmp/r/1-2.recover")), None);
    }
}

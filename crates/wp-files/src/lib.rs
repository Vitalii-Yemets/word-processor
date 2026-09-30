//! Writing a file over one that is already there, so that what is left at
//! the path is always one whole file: the old one or the new one.
//!
//! # Why not `std::fs::write`
//!
//! Because it opens the path for writing, which empties the file first, and
//! only then writes. From that moment until the last byte is down the
//! person's document is not on disk anywhere: a disk that fills halfway, a
//! program that is killed, a machine whose power goes, and what is left where
//! the document was is nothing, or the first part of something.
//!
//! # What is done instead
//!
//! Everything is written to a new file beside the one it replaces, made to
//! reach the disk, and only then put in its place. A rename within one folder
//! is a single step for the file system: whoever looks sees the old file or
//! the new one and never a mixture, and a failure anywhere before it leaves
//! the old file exactly as it was. The new file is made in the same folder
//! and not in the system's temporary one, because a rename from one volume to
//! another is not a rename but a copy — the very thing being avoided.
//!
//! # What belonged to the old file
//!
//! A rename puts a new file where the old one was, and what belonged to the
//! old file rather than to its bytes would go with the old one. So it is
//! carried over. On Windows `ReplaceFileW` does it, which is the system's
//! own way of doing exactly this: the new file takes the old one's place,
//! its access list, its attributes, its creation time, its object
//! identifier and its alternate data streams — the mark of the web among
//! them, which says a document came from the internet and is to be opened
//! in Protected View. On Linux the new file is given the old one's mode and
//! its extended attributes, which is where an access list and a browser's
//! note of where a file came from are kept, and its owner and group as far
//! as the system allows: only the administrator may give a file away to
//! somebody else, and anybody else may give it only to a group they are in.
//! Word on Windows does no better; the file it saves is the saver's.
//!
//! # The temporary file
//!
//! It is called `.~Letter.docx.4120-7.tmp` beside `Letter.docx`: the name of
//! what it will become, so that a person who finds one knows where it came
//! from; the process and a count, so that two saves of the same name at the
//! same moment — two windows, or two copies of the program — each have a file
//! of their own; and `.tmp`, so that nothing takes it for a document. It is
//! made with `create_new`, so an old one that happens to have the same name is
//! never opened and written into; the next count is tried instead. The dot
//! hides it on Linux, and a name beginning `.~` is one Dropbox, for one,
//! does not copy anywhere, so a half-written one is not sent off.
//!
//! # One left behind
//!
//! A crash during a save leaves the temporary file beside the document, and
//! the document beside it is the one that was there before the save began.
//! The next save of that document takes such files away: those whose process
//! is no longer running, and not the others, since a file of that shape with
//! a running process's number in it may be a save that process is making at
//! that very moment, and taking it away would make that save fail. And only
//! those nothing has written to for ten minutes: a process's number means
//! something only on the machine that gave it, and in a folder two machines
//! share, a file whose number is nobody's here may be a save the other
//! machine is making now; a save in progress is being written, and one left
//! by a crash is not. And only while the document itself is in its place:
//! the one failure of Windows' replacing that could not put the old file
//! back leaves it under a name of this shape, and a file that may be the
//! only copy of the document is not swept up as rubbish.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

/// How much of the target's name is kept in the temporary file's, in bytes.
///
/// A folder allows 255 to a name, and the process, the count and the
/// extension need room of their own; a name that long is cut, and is still
/// enough to say whose file it was.
const NAME_KEPT: usize = 128;

/// How many names are tried before giving up, each one taken by a file of
/// the same shape left there before.
const TRIES: u32 = 64;

/// The bytes, written over the file at `target`, or where there is none, as
/// a new one.
///
/// # Errors
/// Whatever the system said, with whatever was at `target` as it was.
pub fn replace_with(target: &Path, bytes: &[u8]) -> io::Result<()> {
    write_replacing(target, |file| file.write_all(bytes))
}

/// Writes a file over the one at `target` by way of a temporary file beside
/// it, which `write` fills.
///
/// Returns once the new file is in place, and not before; on any error the
/// temporary file is taken away and whatever was at `target` is as it was.
/// A file marked read-only is refused, as writing into it would have been,
/// rather than quietly replaced by one that is not. On Linux what belonged
/// to the old file is on the new one before `write` is called, so that
/// `write` may change it: a program's installer makes its files runnable
/// whatever the files they replace were. On Windows it is merged in as the
/// new file is put in place.
///
/// # Errors
/// Whatever `write` or the system said, with whatever was at `target` as it
/// was.
pub fn write_replacing(
    target: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let target = followed(target);
    let Some(name) = target.file_name() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the path names no file"));
    };
    let folder = match target.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let existing = std::fs::metadata(&target).ok().filter(std::fs::Metadata::is_file);
    if existing.as_ref().is_some_and(|metadata| metadata.permissions().readonly()) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the file is read-only"));
    }
    let kept = shortened(&name.to_string_lossy(), NAME_KEPT).to_owned();
    if existing.is_some() {
        sweep(&folder, &kept);
    }

    let (mut file, temporary) = create_beside(&folder, &kept)?;
    // What the file allowed and to whom, which a new file would not
    // otherwise have: a document kept from the rest of the machine stays
    // kept from it. Given before the bytes are written, as they were given
    // to the old file before its bytes were: writing takes the set-user bit
    // off a file, and would have taken it off the old one too. Where the
    // system will not say or will not let it be set, the save goes on
    // regardless. On Windows this is `ReplaceFileW`'s to do, below.
    #[cfg(unix)]
    if let Some(metadata) = &existing {
        keep_what_belonged(&target, metadata, &file);
    }
    // A `File` holds nothing of its own to flush; what has to be made to
    // happen is the system's own writing out, which is `sync_all`.
    let written = write(&mut file).and_then(|()| file.sync_all());
    // Closed before it is put in place: the handle has done its work, and a
    // file still open is one Windows will not move.
    drop(file);

    let placed = written.and_then(|()| put_in_place(&temporary, &target, &folder, &kept));
    if let Err(error) = placed {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }

    // The rename is a change to the folder, which reaches the disk when the
    // folder does. The document is already in place whether this works or
    // not, so a failure here is not a failure of the save. Windows does not
    // open a folder as a file, and NTFS keeps a change to a folder in its
    // own journal.
    #[cfg(unix)]
    if let Ok(folder) = File::open(&folder) {
        let _ = folder.sync_all();
    }
    Ok(())
}

/// Puts the finished temporary file where the target is.
///
/// On Windows by `ReplaceFileW`, which carries over what belonged to the old
/// file, with a name beside it that the old file is moved to on the way and
/// that is gone again afterwards: see [`windows::replace`]. Elsewhere by
/// `rename`, which replaces in one step, what belonged to the old file having
/// been given to the new one already.
#[cfg(windows)]
fn put_in_place(temporary: &Path, target: &Path, folder: &Path, kept: &str) -> io::Result<()> {
    windows::replace(temporary, target, &unused_beside(folder, kept)?)
}

#[cfg(not(windows))]
fn put_in_place(temporary: &Path, target: &Path, _folder: &Path, _kept: &str) -> io::Result<()> {
    std::fs::rename(temporary, target)
}

/// Gives the new file what the old one had that was not its bytes: its
/// owner and group as far as they may be given, its mode, and on Linux its
/// extended attributes. The owner and group go first, because changing
/// either takes the set-user and set-group bits off a file, and the mode
/// after them puts back what may be put back; the extended attributes go
/// last, because an access list among them sets the group's bits of the
/// mode as it is set.
#[cfg(unix)]
fn keep_what_belonged(target: &Path, old: &std::fs::Metadata, file: &File) {
    use std::os::unix::fs::MetadataExt;

    // Only the administrator may give a file to somebody else; anybody may
    // keep it their own and give it to a group they are in, which is the
    // second try.
    if std::os::unix::fs::fchown(file, Some(old.uid()), Some(old.gid())).is_err() {
        let _ = std::os::unix::fs::fchown(file, None, Some(old.gid()));
    }
    let _ = file.set_permissions(old.permissions());
    #[cfg(target_os = "linux")]
    linux::copy_extended_attributes(target, file);
    #[cfg(not(target_os = "linux"))]
    let _ = target;
}

/// Where a path really leads.
///
/// A link is followed, so that the file it points at is the one replaced —
/// as writing through the link replaced it — and the link itself is left a
/// link, rather than turned into a file of its own beside the one it pointed
/// at. The temporary file is then made beside the real one, on its volume.
fn followed(target: &Path) -> PathBuf {
    let is_link =
        std::fs::symlink_metadata(target).is_ok_and(|metadata| metadata.file_type().is_symlink());
    if is_link {
        if let Ok(real) = std::fs::canonicalize(target) {
            return real;
        }
    }
    target.to_path_buf()
}

/// The next name of the temporary shape for this document in this folder.
///
/// The count is the process's own and only ever goes up, so no two calls in
/// one process give the same name, and the process's number keeps it apart
/// from every other process's.
fn next_name(folder: &Path, kept: &str) -> PathBuf {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let count = MADE.fetch_add(1, Ordering::Relaxed);
    folder.join(format!(".~{kept}.{}-{count}.tmp", std::process::id()))
}

/// Makes the temporary file in the folder, under a name nothing else has.
fn create_beside(folder: &Path, kept: &str) -> io::Result<(File, PathBuf)> {
    for _ in 0..TRIES {
        let path = next_name(folder, kept);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free name for a temporary file"))
}

/// A name of the temporary shape that nothing in the folder has, for the
/// old file to be moved to while the new one takes its place. Not made:
/// `ReplaceFileW` makes it by moving the old file there.
#[cfg(windows)]
fn unused_beside(folder: &Path, kept: &str) -> io::Result<PathBuf> {
    for _ in 0..TRIES {
        let path = next_name(folder, kept);
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(error),
            Ok(_) => {}
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free name for the file being replaced"))
}

/// How long nothing must have written to a leftover before it is taken for
/// one: far longer than a save spends between one write and the next, and
/// far shorter than the time until a document left by a crash is saved
/// again, most of the time.
const LEFT_FOR: Duration = Duration::from_secs(10 * 60);

/// Takes away the temporary files an earlier save of this document left
/// behind: those whose process is no longer running and that nothing has
/// written to for [`LEFT_FOR`]. Called only while the document is in its
/// place, for the reasons given at the top of this file.
///
/// A file that will not go is left for the next save to try; this save does
/// not depend on it.
fn sweep(folder: &Path, kept: &str) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let prefix = format!(".~{kept}.");
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(process) = name.to_str().and_then(|name| left_by(name, &prefix)) else {
            continue;
        };
        if running(process) {
            continue;
        }
        // Only a file: a folder of that name is not one this made. And only
        // one left alone long enough: a time the system will not give, or
        // one ahead of this machine's clock — another machine's, set
        // differently — is taken for a file still being written.
        let Ok(metadata) = entry.metadata() else { continue };
        let untouched = metadata
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok());
        if metadata.is_file() && untouched.is_some_and(|untouched| untouched >= LEFT_FOR) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The process that made a temporary file of this document's, read from its
/// name; nothing for a name of any other shape.
///
/// The number and the count are digits and nothing else, and a dot stands
/// before them, so the leftovers of `Letter` are told from those of
/// `Letter.5`, whose own would read `.~Letter.5.4120-7.tmp`.
fn left_by(name: &str, prefix: &str) -> Option<u32> {
    let middle = name.strip_prefix(prefix)?.strip_suffix(".tmp")?;
    let (process, count) = middle.split_once('-')?;
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(process) || !digits(count) {
        return None;
    }
    process.parse().ok()
}

/// Whether a process of this number is running on this machine.
#[cfg(windows)]
fn running(process: u32) -> bool {
    windows::running(process)
}

#[cfg(target_os = "linux")]
fn running(process: u32) -> bool {
    linux::running(process)
}

/// Elsewhere nothing is known, and every process is taken to be running:
/// a leftover stays, which is what it did before there was any sweeping.
#[cfg(not(any(windows, target_os = "linux")))]
fn running(_process: u32) -> bool {
    true
}

/// As much of a name as fits in so many bytes, cut between characters.
fn shortened(name: &str, bytes: usize) -> &str {
    if name.len() <= bytes {
        return name;
    }
    let mut end = bytes;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A process number no process has: Linux gives out none above 2^22,
    /// and Windows none anywhere near this high.
    const NO_PROCESS: u32 = 4_000_000_000;

    /// A folder of this test's own, empty, because the tests run side by side.
    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("wp-files-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a folder to work in");
        folder
    }

    /// What a folder holds, by name, in order.
    fn names_in(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .expect("the folder")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_replaced_file_holds_exactly_the_new_bytes_and_nothing_is_left_beside_it() {
        let folder = folder("replaced");
        let target = folder.join("Letter.docx");
        std::fs::write(&target, b"the old document, which was longer").expect("the old one");

        replace_with(&target, b"the new one").expect("replaced");
        assert_eq!(std::fs::read(&target).expect("read back"), b"the new one");
        assert_eq!(names_in(&folder), ["Letter.docx"], "a temporary file was left behind");

        // And where there was nothing, the file is made.
        let new = folder.join("New.docx");
        replace_with(&new, b"made").expect("made");
        assert_eq!(std::fs::read(&new).expect("read back"), b"made");
        assert_eq!(names_in(&folder), ["Letter.docx", "New.docx"]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_write_that_fails_halfway_leaves_the_old_file_as_it_was() {
        let folder = folder("halfway");
        let target = folder.join("Letter.docx");
        let old = b"every word of the document as it was last saved".to_vec();
        std::fs::write(&target, &old).expect("the old one");

        let failed = write_replacing(&target, |file| {
            file.write_all(b"the first half of the new")?;
            // Written beside the document, not over it and not elsewhere.
            let beside = names_in(&folder);
            assert_eq!(beside.len(), 2, "{beside:?}");
            assert!(
                beside
                    .iter()
                    .any(|name| name.starts_with(".~Letter.docx.") && name.ends_with(".tmp")),
                "{beside:?}"
            );
            assert_eq!(std::fs::read(&target).expect("read meanwhile"), old, "touched already");
            Err(io::Error::other("the disk is full"))
        });

        let error = failed.expect_err("the failure is passed on");
        assert_eq!(error.to_string(), "the disk is full");
        assert_eq!(std::fs::read(&target).expect("read back"), old, "the old file was harmed");
        assert_eq!(names_in(&folder), ["Letter.docx"], "the temporary file was left behind");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn two_saves_of_one_name_at_once_do_not_share_a_temporary_file() {
        let folder = folder("twice");
        let target = folder.join("Letter.docx");
        std::fs::write(&target, b"old").expect("the old one");

        // A second save of the same name, begun and finished while the first
        // is still writing: each has its own file, and the one that finishes
        // last is the one left. The second does not sweep the first's away,
        // because its process is running.
        write_replacing(&target, |file| {
            file.write_all(b"the first")?;
            replace_with(&target, b"the second")?;
            assert_eq!(std::fs::read(&target)?, b"the second");
            Ok(())
        })
        .expect("both saved");
        assert_eq!(std::fs::read(&target).expect("read back"), b"the first");
        assert_eq!(names_in(&folder), ["Letter.docx"]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_long_name_still_has_room_for_its_temporary_file() {
        let folder = folder("long");
        // 250 bytes, which a folder allows and which a temporary name made by
        // putting more around it whole would not fit in.
        let name = format!("{}.docx", "ж".repeat(122) + "x");
        assert_eq!(name.len(), 250);
        let target = folder.join(&name);
        std::fs::write(&target, b"old").expect("the old one");
        replace_with(&target, b"new").expect("replaced");
        assert_eq!(std::fs::read(&target).expect("read back"), b"new");
        assert_eq!(names_in(&folder), [name]);
        assert_eq!(shortened("жж", 3), "ж", "cut between characters, not inside one");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_path_longer_than_windows_once_allowed_is_still_replaced() {
        // Past the 260 characters an old Windows program was held to, which
        // the standard library reaches by writing the path out in full and
        // which the call that replaces the file has to reach the same way.
        let folder = folder("deep");
        let mut deep = folder.clone();
        for _ in 0..6 {
            deep = deep.join("a folder with a long name, as documents have");
        }
        std::fs::create_dir_all(&deep).expect("the deep folder");
        let target = deep.join("Letter.docx");
        assert!(target.as_os_str().len() > 300, "{}", target.display());
        std::fs::write(&target, b"old").expect("the old one");
        replace_with(&target, b"new").expect("replaced");
        assert_eq!(std::fs::read(&target).expect("read back"), b"new");
        assert_eq!(names_in(&deep), ["Letter.docx"]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_read_only_file_is_refused_and_left_alone() {
        let folder = folder("read-only");
        let target = folder.join("Letter.docx");
        std::fs::write(&target, b"old").expect("the old one");
        let mut permissions = std::fs::metadata(&target).expect("its permissions").permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&target, permissions).expect("made read-only");

        let error = replace_with(&target, b"new").expect_err("refused");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read(&target).expect("read back"), b"old");
        assert_eq!(names_in(&folder), ["Letter.docx"]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_leftover_is_known_by_its_name_and_by_nothing_else() {
        let prefix = ".~Letter.docx.";
        assert_eq!(left_by(".~Letter.docx.4120-7.tmp", prefix), Some(4120));
        assert_eq!(left_by(".~Letter.docx.4120.tmp", prefix), None, "no count");
        assert_eq!(left_by(".~Letter.docx.4120-.tmp", prefix), None, "an empty count");
        assert_eq!(left_by(".~Letter.docx.41x0-7.tmp", prefix), None, "not a number");
        assert_eq!(left_by(".~Letter.docx.4120-7.docx", prefix), None, "not a temporary file");
        assert_eq!(left_by(".~Other.docx.4120-7.tmp", prefix), None, "another document's");
        // Another document whose name begins with this one's.
        assert_eq!(left_by(".~Letter.docx.5.4120-7.tmp", prefix), None);
        assert_eq!(left_by(".~Letter.docx.99999999999-7.tmp", prefix), None, "no such number");
    }

    #[test]
    fn a_leftover_whose_process_is_gone_and_that_lay_untouched_is_swept_and_no_other() {
        let folder = folder("leftovers");
        let target = folder.join("Letter.docx");
        // A leftover last written so long ago, or just now.
        let left = |name: String, ago: Duration| {
            let path = folder.join(&name);
            std::fs::write(&path, b"half a document").expect("a leftover");
            let written = SystemTime::now() - ago;
            File::options()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_modified(written))
                .expect("written then");
            name
        };
        let (long_ago, just_now) = (Duration::from_secs(60 * 60), Duration::ZERO);
        // A crash's, whose process is gone and which nothing has written to
        // since; one whose process is gone here but which was written a
        // moment ago, which may be another machine's save in a folder both
        // share; one of this process's, which is running and so may be a
        // save in progress however long it has lain; one of another
        // document's, which this save has no business with; and one of the
        // same beginning but not the shape.
        let gone = left(format!(".~Letter.docx.{NO_PROCESS}-3.tmp"), long_ago);
        let fresh = left(format!(".~Letter.docx.{NO_PROCESS}-4.tmp"), just_now);
        let alive = left(format!(".~Letter.docx.{}-999999.tmp", std::process::id()), long_ago);
        let other = left(format!(".~Other.docx.{NO_PROCESS}-3.tmp"), long_ago);
        let unlike = left(format!(".~Letter.docx.{NO_PROCESS}.tmp"), long_ago);

        // No document there yet: nothing is swept, since what lies beside
        // it may be the only copy of it.
        replace_with(&target, b"first").expect("made");
        assert!(names_in(&folder).contains(&gone), "swept with no document in its place");

        replace_with(&target, b"second").expect("replaced");
        let mut expected = vec![fresh, alive, other, unlike, String::from("Letter.docx")];
        expected.sort();
        assert_eq!(names_in(&folder), expected);
        assert_eq!(std::fs::read(&target).expect("read back"), b"second");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[cfg(unix)]
    #[test]
    fn what_the_file_allowed_is_what_the_new_one_allows() {
        use std::os::unix::fs::PermissionsExt;

        let folder = folder("mode");
        let target = folder.join("Letter.docx");
        std::fs::write(&target, b"old").expect("the old one");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640))
            .expect("kept from others");
        replace_with(&target, b"new").expect("replaced");
        let mode = std::fs::metadata(&target).expect("its permissions").permissions().mode();
        assert_eq!(mode & 0o777, 0o640);

        // And a writer may change it, since it is given before the writing:
        // an installer's program is made runnable whatever it replaces.
        write_replacing(&target, |file| {
            file.write_all(b"a program")?;
            file.set_permissions(std::fs::Permissions::from_mode(0o755))
        })
        .expect("replaced");
        let mode = std::fs::metadata(&target).expect("its permissions").permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[cfg(unix)]
    #[test]
    fn the_owner_and_the_group_are_kept_where_the_system_allows() {
        use std::os::unix::fs::MetadataExt;

        let folder = folder("owner");
        let target = folder.join("Letter.docx");
        std::fs::write(&target, b"old").expect("the old one");
        // Somebody else's file, which only the administrator can make one;
        // where the tests do not run as the administrator there is nobody
        // else to make it, and this proves nothing, and says so.
        if std::os::unix::fs::chown(&target, Some(4321), Some(8765)).is_err() {
            eprintln!("not the administrator: a file cannot be given to somebody else here");
            let _ = std::fs::remove_dir_all(folder);
            return;
        }
        replace_with(&target, b"new").expect("replaced");
        let metadata = std::fs::metadata(&target).expect("its owner");
        assert_eq!((metadata.uid(), metadata.gid()), (4321, 8765));
        assert_eq!(std::fs::read(&target).expect("read back"), b"new");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_extended_attributes_come_with_the_new_file() {
        let folder = folder("attributes");
        let target = folder.join("Letter.docx");
        std::fs::write(&target, b"old").expect("the old one");
        // Where a browser writes down where a download came from: Linux's
        // mark of the web.
        let (name, value) = ("user.xdg.origin.url", b"https://example.com/Letter.docx");
        let file = File::open(&target).expect("the old one, open");
        if let Err(error) = linux::set_attribute(&file, name, value) {
            // A file system that keeps none has none to keep; the rest of the
            // test would prove nothing.
            eprintln!("this file system keeps no extended attributes: {error}");
            let _ = std::fs::remove_dir_all(folder);
            return;
        }
        drop(file);

        replace_with(&target, b"new").expect("replaced");
        assert_eq!(linux::attribute(&target, name).as_deref(), Some(&value[..]));
        assert_eq!(std::fs::read(&target).expect("read back"), b"new");
        assert_eq!(names_in(&folder), ["Letter.docx"]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_followed_and_left_a_link() {
        let folder = folder("link");
        let (real, links) = (folder.join("real"), folder.join("links"));
        std::fs::create_dir_all(&real).expect("a folder for the file");
        std::fs::create_dir_all(&links).expect("a folder for the link");
        let file = real.join("Letter.docx");
        std::fs::write(&file, b"old").expect("the old one");
        let link = links.join("Letter.docx");
        std::os::unix::fs::symlink(&file, &link).expect("a link to it");

        replace_with(&link, b"new").expect("replaced");
        assert!(
            std::fs::symlink_metadata(&link).expect("the link").file_type().is_symlink(),
            "the link was turned into a file"
        );
        assert_eq!(std::fs::read(&file).expect("read back"), b"new");
        assert_eq!(names_in(&real), ["Letter.docx"]);
        assert_eq!(names_in(&links), ["Letter.docx"]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[cfg(windows)]
    #[test]
    fn a_hidden_file_keeps_its_attributes_and_its_creation_time() {
        use std::os::windows::fs::{FileTimesExt, MetadataExt, OpenOptionsExt};

        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        let folder = folder("hidden");
        let target = folder.join("Letter.docx");
        // Made long ago, so that the new file's own creation time could not
        // be taken for it.
        let made = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .attributes(HIDDEN | SYSTEM)
                .open(&target)
                .expect("the old one, hidden");
            file.write_all(b"old").expect("its bytes");
            file.set_times(std::fs::FileTimes::new().set_created(made)).expect("made long ago");
        }
        let before = std::fs::metadata(&target).expect("the old one's attributes");
        assert_eq!(before.file_attributes() & (HIDDEN | SYSTEM), HIDDEN | SYSTEM);
        assert!(!before.permissions().readonly());

        replace_with(&target, b"new").expect("replaced");
        let after = std::fs::metadata(&target).expect("the new one's attributes");
        assert_eq!(std::fs::read(&target).expect("read back"), b"new");
        assert_eq!(after.file_attributes() & (HIDDEN | SYSTEM), HIDDEN | SYSTEM, "not hidden");
        assert_eq!(after.created().expect("its creation time"), made, "made just now");
        assert_eq!(names_in(&folder), ["Letter.docx"], "something was left beside it");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[cfg(windows)]
    #[test]
    fn the_mark_of_the_web_comes_with_the_new_file() {
        let folder = folder("stream");
        let target = folder.join("Letter.docx");
        std::fs::write(&target, b"old").expect("the old one");
        // What a browser writes beside a download, in a stream of the file's
        // own: the zone it came from, which is what opens it in Protected
        // View.
        let stream = folder.join("Letter.docx:Zone.Identifier");
        let zone = "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.com/Letter.docx\r\n";
        std::fs::write(&stream, zone).expect("the mark of the web");

        replace_with(&target, b"new").expect("replaced");
        assert_eq!(std::fs::read(&target).expect("read back"), b"new");
        assert_eq!(std::fs::read_to_string(&stream).expect("the stream, still there"), zone);
        assert_eq!(names_in(&folder), ["Letter.docx"]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[cfg(windows)]
    #[test]
    fn a_file_held_open_without_sharing_its_deletion_is_refused_and_kept() {
        use std::os::windows::fs::OpenOptionsExt;

        const SHARE_READ_AND_WRITE: u32 = 0x1 | 0x2;
        let folder = folder("held");
        let target = folder.join("Letter.docx");
        std::fs::write(&target, b"old").expect("the old one");
        // As another program holds a document it has open: others may read
        // it and write it, and not take it away.
        let held = OpenOptions::new()
            .read(true)
            .share_mode(SHARE_READ_AND_WRITE)
            .open(&target)
            .expect("held open");

        let error = replace_with(&target, b"new").expect_err("replaced under another program");
        eprintln!("refused: {error}");
        assert!(error.raw_os_error().is_some(), "{error}");
        drop(held);
        assert_eq!(std::fs::read(&target).expect("read back"), b"old");
        assert_eq!(names_in(&folder), ["Letter.docx"], "something was left beside it");
        let _ = std::fs::remove_dir_all(folder);
    }
}

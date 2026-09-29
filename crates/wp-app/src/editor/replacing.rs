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
//! reach the disk, and only then renamed over it. A rename within one folder
//! is a single step for the file system: whoever looks sees the old file or
//! the new one and never a mixture, and a failure anywhere before it leaves
//! the old file exactly as it was. The new file is made in the same folder
//! and not in the system's temporary one, because a rename from one volume to
//! another is not a rename but a copy — the very thing being avoided.
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
//! One left behind by a crash stays where it is. It is not the document and
//! nothing reads it; the document beside it is the one that was there before
//! the save began, and the work itself is in the AutoRecover copy. It is not
//! swept up by a later save either, because a file of that shape with another
//! process's number in it may be a save another copy of the program is making
//! at that very moment, and taking it away would make that save fail.

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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
pub(super) fn replace_with(target: &Path, bytes: &[u8]) -> io::Result<()> {
    write_replacing(target, |file| file.write_all(bytes))
}

/// Writes a file over the one at `target` by way of a temporary file beside
/// it, which `write` fills.
///
/// Returns once the new file is in place, and not before; on any error the
/// temporary file is taken away and whatever was at `target` is as it was.
/// A file marked read-only is refused, as writing into it would have been,
/// rather than quietly replaced by one that is not.
pub(super) fn write_replacing(
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

    let (mut file, temporary) = create_beside(&folder, name)?;
    let written = write(&mut file).and_then(|()| {
        // What the file allowed and to whom, which a new file would not
        // otherwise have: a document kept from the rest of the machine stays
        // kept from it. Where the system will not say or will not let it be
        // set, the save goes on regardless. On Windows what a file allows is
        // its access list, which the standard library does not read; its
        // permissions there are the read-only flag, dealt with above.
        #[cfg(unix)]
        if let Some(metadata) = &existing {
            let _ = file.set_permissions(metadata.permissions());
        }
        // A `File` holds nothing of its own to flush; what has to be made to
        // happen is the system's own writing out, which is this.
        file.sync_all()
    });
    // Closed before the rename: the handle has done its work, and a file
    // still open is one Windows may refuse to move.
    drop(file);

    // On Windows `std::fs::rename` is `MoveFileExW` with
    // `MOVEFILE_REPLACE_EXISTING`, and where that is refused as access denied,
    // `SetFileInformationByHandle` with `FileRenameInfoEx` and
    // `FILE_RENAME_FLAG_REPLACE_IF_EXISTS`: its documentation says it replaces
    // `to` where `to` exists, and the standard library's source says with
    // which flags. On Unix it is `rename`, which replaces in one step.
    let renamed = written.and_then(|()| std::fs::rename(&temporary, &target));
    if let Err(error) = renamed {
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

/// Makes the temporary file in the folder, under a name nothing else has.
fn create_beside(folder: &Path, name: &OsStr) -> io::Result<(File, PathBuf)> {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let name = name.to_string_lossy();
    let kept = shortened(&name, NAME_KEPT);
    for _ in 0..TRIES {
        let count = MADE.fetch_add(1, Ordering::Relaxed);
        let path = folder.join(format!(".~{kept}.{}-{count}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free name for a temporary file"))
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

    /// A folder of this test's own, empty, because the tests run side by side.
    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("wp-replacing-{}-{name}", std::process::id()));
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
        // last is the one left.
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
}

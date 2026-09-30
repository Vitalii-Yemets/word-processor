//! What Windows is asked: to put a new file in an old one's place, keeping
//! what belonged to the old one, whether a process is running, and a lock.
//!
//! # Why `ReplaceFileW` and not a rename
//!
//! A rename puts the new file where the old one was, and the new file is
//! its own: the access list its folder gives a new file, no hidden or
//! system attribute, a creation time of now, and none of the old file's
//! alternate data streams — the mark of the web among them, which is how a
//! document from the internet is known to be one and opened in Protected
//! View. `ReplaceFileW` is the system's call for exactly this save: it
//! merges all of that from the old file into the new one and then swaps
//! their names, the old file going to a name beside it given as the backup.
//!
//! # Why a backup, which is thrown away
//!
//! Because without one, the documentation says, one of its failures leaves
//! the old file gone and the new one not in its place — the very thing the
//! whole of this crate is there to prevent. With one, every failure leaves
//! the old file at the target's name or at the backup's, and the one that
//! leaves it at the backup's is undone here by moving it back. On success
//! the backup is the old file under another name, and is removed.
//!
//! # Why this is declared here and not in the shell
//!
//! Because the shell depends on this crate to write the desktop's files,
//! and the command line, the macros and the installer, which write files
//! for a person too, have no business depending on a window. The calls are
//! declared against the documented ABI as the shell declares its own; this
//! file and the Linux one are the only places in the crate that may use
//! `unsafe`, and only for those calls.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

type Handle = *mut c_void;

/// Where in a file a lock begins, for `LockFileEx` and `UnlockFileEx`: the
/// system's `OVERLAPPED`. For a file not opened for overlapped work only the
/// offset is read, and the call returns once it is done.
#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: Handle,
}

impl Overlapped {
    /// The start of the file.
    fn at_start() -> Self {
        Self {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event: core::ptr::null_mut(),
        }
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn ReplaceFileW(
        replaced: *const u16,
        replacement: *const u16,
        backup: *const u16,
        flags: u32,
        exclude: *mut c_void,
        reserved: *mut c_void,
    ) -> i32;
    fn OpenProcess(access: u32, inherit: i32, process: u32) -> Handle;
    fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn LockFileEx(
        file: Handle,
        flags: u32,
        reserved: u32,
        length_low: u32,
        length_high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn UnlockFileEx(
        file: Handle,
        reserved: u32,
        length_low: u32,
        length_high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
}

/// `LockFileEx` asked for a lock nobody else may share.
const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x2;

/// Whatever of the old file's attributes and access list cannot be merged
/// into the new one, the new one goes in regardless: without the right to
/// change a file's access list — a file in a folder somebody else keeps —
/// the save succeeds with the folder's, as it did before this was used.
const REPLACEFILE_IGNORE_MERGE_ERRORS: u32 = 0x2;
const REPLACEFILE_IGNORE_ACL_ERRORS: u32 = 0x4;

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_INVALID_PARAMETER: i32 = 87;

/// `OpenProcess` with the least there is to ask: enough to ask whether the
/// process has ended, which any process may ask of most others.
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
/// What `GetExitCodeProcess` says of a process that has not ended.
const STILL_ACTIVE: u32 = 259;

/// Below this many characters a path is passed as it is; at it and above,
/// written out in full with `\\?\` in front, which is the only way past the
/// 260 characters a path was once limited to. The number is the standard
/// library's own, so that every path it reaches this reaches too.
const LEGACY_MAX_PATH: usize = 248;

/// Puts `temporary` in `target`'s place with what belonged to the old file,
/// moving the old one to `backup` on the way and removing it after.
///
/// What each failure `ReplaceFileW` documents leaves, and what is done:
///
/// - `ERROR_UNABLE_TO_REMOVE_REPLACED` (1175): the old file could not be
///   moved aside — another program has it open and will not let it be
///   taken away. Both files keep their names; the error is passed on.
/// - `ERROR_UNABLE_TO_MOVE_REPLACEMENT` (1176): the new file could not be
///   moved into place. With a backup given, both keep their names; passed
///   on. (Without one the old file would be gone, which is why one is.)
/// - `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2` (1177): the old file is at the
///   backup's name and the new one at its own. The old file is moved back
///   to the target's name, and the error passed on.
/// - Any other — the target open elsewhere without its deletion shared, the
///   files on two volumes, which being in one folder they are not: both
///   keep their names and there is no backup; passed on.
///
/// Rather than trust the codes alone, the one thing that matters is looked
/// at after every failure: if the target's name is empty and the backup's
/// is not, the old file is moved back. The caller removes the temporary
/// file, whatever it has inherited on the way.
///
/// Where there is no file at the target yet there is nothing to keep, and
/// `ReplaceFileW` would find nothing to replace: the file is moved there.
pub(crate) fn replace(temporary: &Path, target: &Path, backup: &Path) -> io::Result<()> {
    if absent(target) {
        return std::fs::rename(temporary, target);
    }
    let (replaced, replacement, aside) = (wide(target)?, wide(temporary)?, wide(backup)?);
    // SAFETY: three null-terminated strings that outlive the call, and the
    // two reserved pointers null, as the documentation asks.
    let done = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            aside.as_ptr(),
            REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    };
    if done != 0 {
        // The old file, under the backup's name. One that will not go — a
        // program still running from it — is swept by a later save once
        // this process has ended.
        let _ = std::fs::remove_file(backup);
        return Ok(());
    }
    let error = io::Error::last_os_error();

    if absent(target) && !absent(backup) {
        // The old file went aside and the new one did not come: it goes
        // back. Should even that fail, it is still whole where it is, and
        // the error says where; no later save sweeps it, since none sweeps
        // while the document is not in its place.
        return match std::fs::rename(backup, target) {
            Ok(()) => Err(error),
            Err(_) => Err(io::Error::new(
                error.kind(),
                format!("{error}; the file that was there is now {}", backup.display()),
            )),
        };
    }
    if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) && absent(target) {
        // Taken away by somebody else between the look and the replacing:
        // nothing to keep, so the file is moved there.
        return std::fs::rename(temporary, target);
    }
    Err(error)
}

/// Whether nothing at all is at a path. Anything else the system says — a
/// folder it will not let this look into — is not taken for nothing.
fn absent(path: &Path) -> bool {
    matches!(std::fs::symlink_metadata(path), Err(error) if error.kind() == io::ErrorKind::NotFound)
}

/// A path as the system's wide string, null-terminated, and written out in
/// full with `\\?\` in front where it is too long to be passed as it is.
fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
    let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
    if units.len() >= LEGACY_MAX_PATH && !units.starts_with(&verbatim) {
        // In full, with `.` and `..` worked out and every `/` a `\`, since a
        // path written this way is taken exactly as it is written.
        let full: Vec<u16> = std::path::absolute(path)?.as_os_str().encode_wide().collect();
        let unc: Vec<u16> = r"\\".encode_utf16().collect();
        units = if full.starts_with(&unc) {
            // A share, `\\server\share\…`, which is `\\?\UNC\server\share\…`.
            r"\\?\UNC\".encode_utf16().chain(full[2..].iter().copied()).collect()
        } else {
            verbatim.into_iter().chain(full).collect()
        };
    }
    if units.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a path with a null in it"));
    }
    units.push(0);
    Ok(units)
}

/// Whether a process of this number is running.
///
/// No process of the number is the one answer that says it is not: one the
/// system will not let this process look at — another person's, or the
/// system's own — is running, and one that has ended but is still held by
/// somebody says so by its exit code.
pub(crate) fn running(process: u32) -> bool {
    // SAFETY: no pointers; a handle that comes back is closed below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process) };
    if handle.is_null() {
        return io::Error::last_os_error().raw_os_error() != Some(ERROR_INVALID_PARAMETER);
    }
    let mut code = 0u32;
    // SAFETY: the handle is open, and the code a place for one number.
    let asked = unsafe { GetExitCodeProcess(handle, &mut code) };
    // SAFETY: the handle is open, and closed once.
    unsafe { CloseHandle(handle) };
    asked == 0 || code == STILL_ACTIVE
}

/// A lock held, by way of its file, open; let go of when dropped.
#[derive(Debug)]
pub(crate) struct Held {
    file: File,
}

/// Takes the lock whose file is at `path`, waiting while anybody else has
/// it.
///
/// `LockFileEx` on the whole of the file, which is the lock the standard
/// library's own is made of, in a version newer than this workspace says it
/// needs. A lock belongs to the handle it was taken through, and the file is
/// opened anew for every lock, so each waits for every other, in this
/// process or in another. The file stays where it is when the lock is let go
/// of: nothing else on Windows asks for a lock of that name, and a file
/// another process has open is not simply taken away under it.
pub(crate) fn hold(path: &Path) -> io::Result<Held> {
    // Nothing is written in it, and nothing emptied.
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
    let mut start = Overlapped::at_start();
    // SAFETY: the handle is the open file's, not opened for overlapped work,
    // so the call returns once the lock is given and `start` outlives it.
    let given = unsafe {
        LockFileEx(file.as_raw_handle(), LOCKFILE_EXCLUSIVE_LOCK, 0, u32::MAX, u32::MAX, &mut start)
    };
    if given == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Held { file })
}

impl Drop for Held {
    fn drop(&mut self) {
        // Closing the handle would let go of it too, but only when the
        // system gets round to it, the documentation says; it asks for the
        // lock to be let go of first.
        let mut start = Overlapped::at_start();
        // SAFETY: as when the lock was taken, over the same bytes.
        unsafe { UnlockFileEx(self.file.as_raw_handle(), 0, u32::MAX, u32::MAX, &mut start) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_running_and_one_no_process_has_is_not() {
        assert!(running(std::process::id()));
        assert!(!running(4_000_000_000));
    }

    #[test]
    fn a_lock_is_kept_from_another_opening_in_the_same_thread() {
        // A lock not waited for, which says at once whether it was given.
        const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x1;
        const ERROR_LOCK_VIOLATION: i32 = 33;
        let path = std::env::temp_dir().join(format!("wp-files-{}-held.lock", std::process::id()));
        let held = hold(&path).expect("held");

        // The same file opened again, by the same thread and process.
        let again = OpenOptions::new().read(true).write(true).open(&path).expect("opened again");
        let mut start = Overlapped::at_start();
        // SAFETY: as in `hold`.
        let given = unsafe {
            LockFileEx(
                again.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                u32::MAX,
                u32::MAX,
                &mut start,
            )
        };
        let error = io::Error::last_os_error();
        assert_eq!((given, error.raw_os_error()), (0, Some(ERROR_LOCK_VIOLATION)));

        // Let go of, it is given to the other.
        drop(held);
        let mut start = Overlapped::at_start();
        // SAFETY: as in `hold`.
        let given = unsafe {
            LockFileEx(
                again.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                u32::MAX,
                u32::MAX,
                &mut start,
            )
        };
        assert_ne!(given, 0, "{}", io::Error::last_os_error());
        drop(again);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_long_path_is_written_out_in_full_and_a_short_one_left_as_it_is() {
        let short = wide(Path::new(r"C:\a\b.docx")).expect("a short path");
        assert_eq!(String::from_utf16_lossy(&short), "C:\\a\\b.docx\0");
        let long = format!(r"C:\{}\b.docx", "a".repeat(300));
        let written = String::from_utf16_lossy(&wide(Path::new(&long)).expect("a long path"));
        assert_eq!(written, format!(r"\\?\{long}{}", '\0'));
        let share = format!(r"\\server\share\{}\b.docx", "a".repeat(300));
        let written = String::from_utf16_lossy(&wide(Path::new(&share)).expect("a share"));
        assert_eq!(written, format!(r"\\?\UNC\server\share\{}\b.docx{}", "a".repeat(300), '\0'));
    }
}

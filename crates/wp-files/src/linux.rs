//! What Linux is asked: a file's extended attributes, whether a process is
//! running, and a lock.
//!
//! # Why the C library is called
//!
//! Because the extended attributes are the system's own and nothing in the
//! standard library reads them. They are where a file keeps what is not its
//! bytes and not its mode: its access list beyond the owner, the group and
//! the rest (`system.posix_acl_access`), what a browser noted of where a
//! download came from (`user.xdg.origin.url`), a security label. And the
//! standard library's own lock on a file is newer than the compiler this
//! workspace says it needs. The calls are declared here against the C
//! library's documented interface, as the shell declares its own; this file
//! and the Windows one are the only places in the crate that may use
//! `unsafe`, and only for those calls.

#![allow(unsafe_code)]

use std::ffi::{c_char, c_int, c_void, CString};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

extern "C" {
    fn listxattr(path: *const c_char, list: *mut c_char, size: usize) -> isize;
    fn getxattr(path: *const c_char, name: *const c_char, value: *mut c_void, size: usize)
        -> isize;
    fn fsetxattr(
        fd: c_int,
        name: *const c_char,
        value: *const c_void,
        size: usize,
        flags: c_int,
    ) -> c_int;
    fn flock(fd: c_int, operation: c_int) -> c_int;
}

/// `flock`'s exclusive lock, and letting go of it.
const LOCK_EX: c_int = 2;
const LOCK_UN: c_int = 8;

/// What the C library says when an answer grew between asking how long it
/// is and asking for it.
const ERANGE: i32 = 34;

/// Gives the file every extended attribute the one at `from` has that it
/// may be given.
///
/// Each is tried on its own and a refusal passes over it: an attribute only
/// the administrator may set (`trusted.`, most of `security.`) is refused to
/// anybody else, and one the file system cannot keep is refused by it, and
/// neither is a reason not to save the document.
pub(crate) fn copy_extended_attributes(from: &Path, to: &File) {
    let Ok(path) = CString::new(from.as_os_str().as_bytes()) else { return };
    // SAFETY: the path is null-terminated, and `asked` passes a buffer with
    // the length it has, or no buffer and nothing.
    let names = asked(|buffer, size| unsafe { listxattr(path.as_ptr(), buffer.cast(), size) });
    let Some(names) = names else { return };
    for name in names.split(|byte| *byte == 0).filter(|name| !name.is_empty()) {
        let Ok(name) = CString::new(name) else { continue };
        // SAFETY: as above, with the name null-terminated too.
        let value = asked(|buffer, size| unsafe {
            getxattr(path.as_ptr(), name.as_ptr(), buffer.cast(), size)
        });
        let Some(value) = value else { continue };
        // SAFETY: the descriptor is the open file's, the name null-terminated
        // and the value as long as the length passed with it.
        unsafe { fsetxattr(to.as_raw_fd(), name.as_ptr(), value.as_ptr().cast(), value.len(), 0) };
    }
}

/// What a call that first says how long its answer is, and then gives it,
/// gave; nothing where it would not.
///
/// The answer can grow between the two asks — somebody set an attribute —
/// and the call then says so rather than give half of it; it is asked again.
fn asked(call: impl Fn(*mut u8, usize) -> isize) -> Option<Vec<u8>> {
    for _ in 0..4 {
        let length = usize::try_from(call(core::ptr::null_mut(), 0)).ok()?;
        if length == 0 {
            return Some(Vec::new());
        }
        let mut buffer = vec![0u8; length];
        if let Ok(given) = usize::try_from(call(buffer.as_mut_ptr(), buffer.len())) {
            buffer.truncate(given);
            return Some(buffer);
        }
        if io::Error::last_os_error().raw_os_error() != Some(ERANGE) {
            return None;
        }
    }
    None
}

/// Whether a process of this number is running.
///
/// A running process has a folder of its number in `/proc`, and one that
/// has ended has none. Where `/proc` is not there at all nothing can be
/// said, and every process is taken to be running, so that no save that may
/// be in progress is swept away on a guess.
pub(crate) fn running(process: u32) -> bool {
    let proc = Path::new("/proc");
    !proc.join("self").exists() || proc.join(process.to_string()).exists()
}

/// A lock held, by way of its file, open; let go of when dropped.
#[derive(Debug)]
pub(crate) struct Held {
    file: File,
    path: PathBuf,
}

/// Takes the lock whose file is at `path`, waiting while anybody else has
/// it.
///
/// `flock`, and not the record locks of `fcntl`, because a record lock is
/// the process's: another thread of the same process asking for it is
/// given it at once, and two windows of one program, or two tests of one
/// run, would not keep each other out. A `flock` belongs to the file as
/// opened, and the file is opened anew for every lock, so each waits for
/// every other, in this process or in another.
///
/// The file is taken away as the lock is let go of. Beside the desktop's
/// list it has the name of KDE's own lock on that list, and KDE locks by
/// making the file: one it finds there with no process's number in it it
/// waits on for good, and one whose process has ended it takes away. So a
/// lock may be given on a file taken away while this waited for it, a lock
/// nobody else will ask for; it is let go of and the path opened again. And
/// this process's number is written in the file where KDE writes its own,
/// so that one left by a crash is known for what it is. KDE takes such a
/// file away only once it has the `flock` on it, which it cannot have while
/// the lock is held here.
pub(crate) fn hold(path: &Path) -> io::Result<Held> {
    loop {
        // Not emptied as it is opened: until the lock is given, what is in
        // it belongs to whoever holds it.
        let mut file =
            OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
        // SAFETY: the descriptor is the open file's.
        while unsafe { flock(file.as_raw_fd(), LOCK_EX) } != 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
        let held = file.metadata()?;
        match std::fs::metadata(path) {
            Ok(now) if (now.dev(), now.ino()) == (held.dev(), held.ino()) => {
                // The number and the program's name are for KDE's sake, on the
                // lines its own lock has them; the lock is held without them.
                let _ = file.set_len(0).and_then(|()| {
                    let program = std::env::current_exe().ok();
                    let name = program.as_deref().and_then(Path::file_name);
                    let name = name.map(|name| name.to_string_lossy()).unwrap_or_default();
                    write!(file, "{}\n{name}\n\n", std::process::id())
                });
                return Ok(Held { file, path: path.to_path_buf() });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // Taken away while still held, so that nobody opens it between the
        // letting go and the going; whoever was already waiting on it finds
        // it gone once it is given the lock, and opens the path again.
        let _ = std::fs::remove_file(&self.path);
        // Let go of now, rather than when the last copy of the descriptor
        // is closed: a program being started holds a copy until it starts.
        // SAFETY: the descriptor is the open file's.
        unsafe { flock(self.file.as_raw_fd(), LOCK_UN) };
    }
}

/// Sets one extended attribute on an open file, for the tests to give an
/// old file one to keep.
#[cfg(test)]
pub(crate) fn set_attribute(file: &File, name: &str, value: &[u8]) -> io::Result<()> {
    let name = CString::new(name).map_err(io::Error::other)?;
    // SAFETY: the descriptor is the open file's, the name null-terminated and
    // the value as long as the length passed with it.
    let set = unsafe {
        fsetxattr(file.as_raw_fd(), name.as_ptr(), value.as_ptr().cast(), value.len(), 0)
    };
    if set == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// One extended attribute of the file at a path, for the tests to read back.
#[cfg(test)]
pub(crate) fn attribute(path: &Path, name: &str) -> Option<Vec<u8>> {
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let name = CString::new(name).ok()?;
    // SAFETY: both null-terminated, and the buffer as long as the length.
    asked(|buffer, size| unsafe { getxattr(path.as_ptr(), name.as_ptr(), buffer.cast(), size) })
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
        const LOCK_NB: c_int = 4;
        const EWOULDBLOCK: i32 = 11;
        let path = std::env::temp_dir().join(format!("wp-files-{}-held.lock", std::process::id()));
        let held = hold(&path).expect("held");
        let text = std::fs::read_to_string(&path).expect("the lock's file");
        assert_eq!(text.lines().next(), Some(std::process::id().to_string().as_str()));

        // The same file opened again, by the same thread: a record lock
        // would be given it, the process having it already, and `flock`
        // is not.
        let again = File::open(&path).expect("opened again");
        // SAFETY: the descriptor is the open file's.
        let given = unsafe { flock(again.as_raw_fd(), LOCK_EX | LOCK_NB) };
        let error = io::Error::last_os_error();
        assert_eq!((given, error.raw_os_error()), (-1, Some(EWOULDBLOCK)));

        drop(held);
        assert!(!path.exists(), "the lock's file was left behind");
    }
}

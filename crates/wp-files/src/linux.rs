//! What Linux is asked: a file's extended attributes, and whether a process
//! is running.
//!
//! # Why the C library is called
//!
//! Because the extended attributes are the system's own and nothing in the
//! standard library reads them. They are where a file keeps what is not its
//! bytes and not its mode: its access list beyond the owner, the group and
//! the rest (`system.posix_acl_access`), what a browser noted of where a
//! download came from (`user.xdg.origin.url`), a security label. The calls
//! are declared here against the C library's documented interface, as the
//! shell declares its own; this file and the Windows one are the only places
//! in the crate that may use `unsafe`, and only for those calls.

#![allow(unsafe_code)]

use std::ffi::{c_char, c_int, c_void, CString};
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

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
}

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
}

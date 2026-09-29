//! Waiting on more than one connection at once.
//!
//! # Why the shells need it
//!
//! Because a window here listens to two parties. The display — the X server
//! or the compositor — sends the keys and the pointer; the accessibility
//! bus sends a screen reader's questions. Each shell's loop waited on the
//! display alone, for up to a tick, and looked at the bus between waits;
//! so every question a screen reader asked waited for the tick to run out
//! before it was answered, and a screen reader asks one question at a time.
//! Reading the ribbon was a hundred and fifty questions, and took thirty
//! seconds. Waiting on both at once answers each as it comes, which is what
//! `poll` is for, and why the C library has it.

use std::os::fd::RawFd;
use std::time::Duration;

/// `struct pollfd`, which is the same on every Linux there is.
#[repr(C)]
struct PollFd {
    fd: RawFd,
    events: i16,
    revents: i16,
}

extern "C" {
    fn poll(fds: *mut PollFd, count: core::ffi::c_ulong, timeout: core::ffi::c_int) -> i32;
}

/// Data to read.
const POLLIN: i16 = 1;

/// Waits until one of the connections has something to read, or until the
/// time runs out; says which of them have. A connection that has closed or
/// failed counts as having something, so that reading it says what went
/// wrong.
pub(crate) fn readable(fds: &[RawFd], timeout: Duration) -> Vec<bool> {
    let mut asked: Vec<PollFd> =
        fds.iter().map(|fd| PollFd { fd: *fd, events: POLLIN, revents: 0 }).collect();
    let millis = core::ffi::c_int::try_from(timeout.as_millis()).unwrap_or(core::ffi::c_int::MAX);
    // SAFETY: the array holds exactly as many entries as the count says, and
    // `poll` writes nothing but their `revents`.
    let answered = unsafe { poll(asked.as_mut_ptr(), asked.len() as core::ffi::c_ulong, millis) };
    if answered <= 0 {
        // Nothing, or a signal came first: either way the loop goes round.
        return vec![false; fds.len()];
    }
    asked.iter().map(|entry| entry.revents != 0).collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::time::Instant;

    use super::*;

    #[test]
    fn the_one_with_something_to_read_is_said_to_have_it() {
        let (mut near, far) = UnixStream::pair().expect("a pair");
        let (_quiet, other) = UnixStream::pair().expect("another pair");
        let fds = [far.as_raw_fd(), other.as_raw_fd()];
        let started = Instant::now();
        assert_eq!(readable(&fds, Duration::from_millis(30)), vec![false, false]);
        assert!(started.elapsed() >= Duration::from_millis(25), "it did not wait");
        near.write_all(b"x").expect("writing");
        let started = Instant::now();
        assert_eq!(readable(&fds, Duration::from_secs(5)), vec![true, false]);
        assert!(started.elapsed() < Duration::from_secs(1), "it waited for nothing");
    }
}

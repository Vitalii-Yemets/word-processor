//! The picture the compositor shows: a file both programs can see.
//!
//! # Why a file
//!
//! Wayland has no "put these pixels on that window". The client makes a
//! block of memory, hands the compositor a descriptor for it, and says
//! which rectangle of it is the window's contents; the compositor reads
//! the pixels straight out of the same memory. That is why drawing a
//! window costs one copy — from the canvas into this — rather than a round
//! trip through a server.
//!
//! # Why the memory has no name
//!
//! `memfd_create` makes a file that lives only as long as somebody holds
//! it: it is in no directory, so there is nothing for another program to
//! open and nothing left behind if this one stops.

use std::ffi::c_void;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use super::wire::Failure;

extern "C" {
    fn memfd_create(name: *const u8, flags: u32) -> i32;
    fn ftruncate(fd: i32, length: i64) -> i32;
    fn mmap(
        address: *mut c_void,
        length: usize,
        protection: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(address: *mut c_void, length: usize) -> i32;
}

const MFD_CLOEXEC: u32 = 1;
const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const MAP_SHARED: i32 = 1;
const MAP_FAILED: isize = -1;

/// A block of memory shared with the compositor.
#[derive(Debug)]
pub(crate) struct Shared {
    fd: OwnedFd,
    address: *mut u8,
    length: usize,
}

impl Shared {
    /// Makes one of the given size, mapped and ready to be written into.
    pub(crate) fn new(length: usize) -> Result<Self, Failure> {
        let name = b"wp-window\0";
        // SAFETY: the name is a null-terminated string that outlives the
        // call, and the descriptor is taken over below.
        let fd = unsafe { memfd_create(name.as_ptr(), MFD_CLOEXEC) };
        if fd < 0 {
            return Err(Failure::Socket(std::io::Error::last_os_error().to_string()));
        }
        // SAFETY: the descriptor came from the call above and is owned here.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        // SAFETY: the descriptor is open and the length is not negative.
        if unsafe { ftruncate(fd.as_raw_fd(), length as i64) } < 0 {
            return Err(Failure::Socket(std::io::Error::last_os_error().to_string()));
        }
        // SAFETY: a fresh mapping of a file this program just made, of the
        // length it was just given.
        let address = unsafe {
            mmap(
                core::ptr::null_mut(),
                length,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if address as isize == MAP_FAILED {
            return Err(Failure::Socket(std::io::Error::last_os_error().to_string()));
        }
        Ok(Self { fd, address: address.cast::<u8>(), length })
    }

    pub(crate) fn fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// The memory, to write the window's pixels into.
    pub(crate) fn bytes(&mut self) -> &mut [u8] {
        // SAFETY: the mapping is this long and lives as long as `self`.
        unsafe { core::slice::from_raw_parts_mut(self.address, self.length) }
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        // SAFETY: the mapping was made here and is not used again.
        unsafe { munmap(self.address.cast::<c_void>(), self.length) };
    }
}

/// Reads a file the compositor handed over — a keymap, a paste — into
/// memory.
pub(crate) fn read_fd(fd: OwnedFd) -> Vec<u8> {
    use std::io::Read;
    // SAFETY: the descriptor is owned here and is given to the file, which
    // closes it.
    let mut file = unsafe { <std::fs::File as FromRawFd>::from_raw_fd(fd.as_raw_fd()) };
    core::mem::forget(fd);
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    bytes
}

/// The canvas's red, green, blue, alpha as the format every compositor
/// takes: blue, green, red, and a byte it ignores.
pub(crate) fn to_compositor(pixels: &[u8], out: &mut [u8]) {
    for (source, destination) in pixels.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        destination[0] = source[2];
        destination[1] = source[1];
        destination[2] = source[0];
        destination[3] = 0xFF;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_of_shared_memory_can_be_written_and_read_back() {
        let mut shared = Shared::new(4096).expect("memory to share");
        shared.bytes()[0] = 0xAB;
        shared.bytes()[4095] = 0xCD;
        assert_eq!(shared.bytes()[0], 0xAB);
        assert_eq!(shared.bytes()[4095], 0xCD);
        assert!(shared.fd() >= 0, "and there is a descriptor to hand over");
    }

    #[test]
    fn the_canvas_is_turned_into_what_the_compositor_reads() {
        let pixels = [10, 20, 30, 40, 200, 100, 50, 255];
        let mut out = [0u8; 8];
        to_compositor(&pixels, &mut out);
        assert_eq!(out[..4], [30, 20, 10, 0xFF], "blue, green, red, opaque");
        assert_eq!(out[4..], [50, 100, 200, 0xFF]);
    }
}

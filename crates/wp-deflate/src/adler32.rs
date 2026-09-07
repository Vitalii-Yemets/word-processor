//! Adler-32 (RFC 1950) — the checksum carried by the zlib wrapper. Required for PNG.

const MOD_ADLER: u32 = 65521;

/// The largest number of bytes that can be accumulated before `u32` would overflow.
const NMAX: usize = 5552;

#[derive(Clone, Copy, Debug)]
pub struct Adler32 {
    a: u32,
    b: u32,
}

impl Adler32 {
    #[must_use]
    pub const fn new() -> Self {
        Self { a: 1, b: 0 }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(NMAX) {
            for &byte in chunk {
                self.a += u32::from(byte);
                self.b += self.a;
            }
            self.a %= MOD_ADLER;
            self.b %= MOD_ADLER;
        }
    }

    #[must_use]
    pub const fn finish(self) -> u32 {
        (self.b << 16) | self.a
    }
}

impl Default for Adler32 {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use]
pub fn adler32(bytes: &[u8]) -> u32 {
    let mut sum = Adler32::new();
    sum.update(bytes);
    sum.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(adler32(b""), 0x0000_0001);
        assert_eq!(adler32(b"a"), 0x0062_0062);
        assert_eq!(adler32(b"abc"), 0x024D_0127);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn survives_chunk_boundaries() {
        let data = vec![0xFFu8; NMAX * 2 + 13];
        let whole = adler32(&data);

        let mut chunked = Adler32::new();
        for chunk in data.chunks(999) {
            chunked.update(chunk);
        }
        assert_eq!(chunked.finish(), whole);
    }
}

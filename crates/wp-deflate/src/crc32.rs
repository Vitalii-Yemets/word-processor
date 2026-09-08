//! CRC-32 (ISO 3309 / ITU-T V.42) — the checksum used by ZIP entries and PNG chunks.

/// Reflected CRC-32 polynomial. Input is processed least-significant bit first,
/// so the mirrored form of 0x04C11DB7 is used.
const POLYNOMIAL: u32 = 0xEDB8_8320;

/// The 256-entry lookup table is computed at compile time and ships ready-made.
const TABLE: [u32; 256] = build_table();

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut byte = 0usize;
    while byte < 256 {
        let mut value = byte as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 { POLYNOMIAL ^ (value >> 1) } else { value >> 1 };
            bit += 1;
        }
        table[byte] = value;
        byte += 1;
    }
    table
}

/// Incremental CRC-32 accumulator, so a checksum can be computed chunk by chunk
/// without holding the whole file in memory.
#[derive(Clone, Copy, Debug)]
pub struct Crc32 {
    state: u32,
}

impl Crc32 {
    #[must_use]
    pub const fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        let mut state = self.state;
        for &byte in bytes {
            let index = ((state ^ u32::from(byte)) & 0xFF) as usize;
            state = TABLE[index] ^ (state >> 8);
        }
        self.state = state;
    }

    #[must_use]
    pub const fn finish(self) -> u32 {
        self.state ^ 0xFFFF_FFFF
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

/// CRC-32 of an entire buffer.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(bytes);
    crc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        // Reference values from the CRC-32 specification and the zlib documentation.
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"a"), 0xE8B7_BE43);
        assert_eq!(crc32(b"abc"), 0x3524_41C2);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
    }

    #[test]
    fn incremental_matches_whole() {
        let data: Vec<u8> = (0u16..1000).map(|value| (value % 251) as u8).collect();
        let whole = crc32(&data);

        let mut chunked = Crc32::new();
        for chunk in data.chunks(7) {
            chunked.update(chunk);
        }
        assert_eq!(chunked.finish(), whole);
    }
}

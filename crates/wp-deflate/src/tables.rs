//! Constant DEFLATE tables from RFC 1951, section 3.2.5.
//!
//! Kept in one place because both the encoder and the decoder need them: the
//! two sides must interpret length and distance codes identically.

/// Base match lengths for codes 257..=285.
pub(crate) const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

/// Number of extra bits that follow each length code.
pub(crate) const LENGTH_EXTRA: [u8; 29] =
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];

/// Base match distances for codes 0..=29.
pub(crate) const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

/// Number of extra bits that follow each distance code.
pub(crate) const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// The order in which code lengths of the auxiliary alphabet appear in a
/// dynamic block header.
pub(crate) const CODE_LENGTH_ORDER: [usize; 19] =
    [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// The shortest match worth encoding as a back-reference.
pub(crate) const MIN_MATCH: usize = 3;

/// The longest match expressible by a single length code.
pub(crate) const MAX_MATCH: usize = 258;

/// Sliding window size: a back-reference may not reach further than 32 KiB.
pub(crate) const WINDOW_SIZE: usize = 32_768;

/// Symbol number of the end-of-block marker.
pub(crate) const END_OF_BLOCK: u16 = 256;

/// Maps a match length to its code. The table is short, so scanning backwards
/// is cheaper than a binary search.
pub(crate) fn length_code(length: usize) -> usize {
    debug_assert!((MIN_MATCH..=MAX_MATCH).contains(&length));
    let mut index = LENGTH_BASE.len() - 1;
    while length < LENGTH_BASE[index] as usize {
        index -= 1;
    }
    index
}

/// Maps a match distance to its code.
pub(crate) fn distance_code(distance: usize) -> usize {
    debug_assert!((1..=WINDOW_SIZE).contains(&distance));
    let mut index = DISTANCE_BASE.len() - 1;
    while distance < DISTANCE_BASE[index] as usize {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_length_maps_to_a_code_that_can_encode_it() {
        for length in MIN_MATCH..=MAX_MATCH {
            let code = length_code(length);
            let base = LENGTH_BASE[code] as usize;
            let extra = LENGTH_EXTRA[code] as u32;
            assert!(length >= base, "length {length} is below the base of code {code}");
            assert!(
                length - base < (1usize << extra),
                "length {length} does not fit in the {extra} extra bits of code {code}"
            );
        }
    }

    #[test]
    fn every_distance_maps_to_a_code_that_can_encode_it() {
        for distance in 1..=WINDOW_SIZE {
            let code = distance_code(distance);
            let base = DISTANCE_BASE[code] as usize;
            let extra = DISTANCE_EXTRA[code] as u32;
            assert!(distance >= base);
            assert!(distance - base < (1usize << extra));
        }
    }
}

//! Writing a font file out of its tables.
//!
//! The one thing this crate writes rather than reads, and only because
//! cutting a font down ends with it: a PDF carries a font built out of the
//! original's tables, some copied and some cut, and those have to be put back
//! together as a file a reader will open. Both kinds are put together the
//! same way — the signature at the front is all that says which kind it is.

/// Writes tables out as a font file, in the order given, which should be
/// by tag: the format wants them so.
///
/// `signature` is `0x00010000` for a font whose outlines are in `glyf` and
/// `OTTO` for one whose outlines are in `CFF`.
#[must_use]
pub fn assemble(signature: [u8; 4], tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let count = tables.len() as u16;
    // The three numbers after the count are a binary search hint, and every
    // font writes them even though every reader could work them out.
    let entry_selector = (15 - count.max(1).leading_zeros()) as u16;
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = count * 16 - search_range.min(count * 16);

    let mut out = Vec::new();
    out.extend_from_slice(&signature);
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    // The directory comes first, so where each table will land has to be known
    // before any of them is written.
    let mut offset = 12 + tables.len() * 16;
    let mut directory = Vec::new();
    for (tag, data) in tables {
        directory.extend_from_slice(tag);
        directory.extend_from_slice(&checksum(data).to_be_bytes());
        directory.extend_from_slice(&(offset as u32).to_be_bytes());
        directory.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += padded(data.len());
    }
    out.extend_from_slice(&directory);

    for (_, data) in tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }

    // The header holds a checksum of the whole file, which can only be worked
    // out now that there is a whole file.
    if let Some(head_at) = table_offset(tables, b"head") {
        let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        out[head_at + 8..head_at + 12].copy_from_slice(&adjustment.to_be_bytes());
    }
    out
}

/// Where a table's bytes begin in the file being written.
fn table_offset(tables: &[([u8; 4], Vec<u8>)], wanted: &[u8; 4]) -> Option<usize> {
    let mut offset = 12 + tables.len() * 16;
    for (tag, data) in tables {
        if tag == wanted {
            return Some(offset);
        }
        offset += padded(data.len());
    }
    None
}

/// A length rounded up to the four-byte boundary the format aligns to.
fn padded(length: usize) -> usize {
    length.div_ceil(4) * 4
}

/// The sum of a table read as big-endian words, which is what a font file uses
/// for a checksum.
fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::{assemble, checksum, padded};

    #[test]
    fn a_table_is_summed_as_words_and_short_ones_are_padded_with_nothing() {
        assert_eq!(checksum(&[0, 0, 0, 1]), 1);
        assert_eq!(checksum(&[0, 0, 1, 0]), 256);
        // Three bytes are summed as though a fourth zero followed them.
        assert_eq!(checksum(&[0, 0, 1]), 256);
    }

    #[test]
    fn tables_are_aligned_to_four_bytes() {
        assert_eq!(padded(0), 0);
        assert_eq!(padded(1), 4);
        assert_eq!(padded(4), 4);
        assert_eq!(padded(5), 8);
    }

    #[test]
    fn the_file_says_its_kind_and_the_whole_of_it_sums_to_the_magic_number() {
        let head = vec![0u8; 54];
        let file = assemble(*b"OTTO", &[(*b"CFF ", vec![1, 2, 3]), (*b"head", head)]);
        assert_eq!(&file[..4], b"OTTO");
        assert_eq!(u16::from_be_bytes([file[4], file[5]]), 2);
        // With the adjustment written into the header, the file's own sum is
        // the number the format fixes.
        assert_eq!(checksum(&file), 0xB1B0_AFBA);
        let directory = crate::table_directory(&file).expect("a directory");
        assert_eq!(directory.len(), 2);
        assert_eq!(&directory[0].0, b"CFF ");
        assert_eq!(directory[0].1.length, 3);
    }
}

//! The two shapes nearly every table in the file comes in.
//!
//! A *PLC* is a run of positions followed by an entry for each stretch
//! between two of them: n + 1 four-byte positions, then n entries of a size
//! the kind of table fixes. The footnotes, the comments, the bookmarks, the
//! sections, the drawings and the formatting pages are all one of these, and
//! the only thing that tells n is the table's length.
//!
//! An *STTB* is a list of strings, each with bytes of its own after it: a
//! count, how many bytes follow each string, and the strings. Word 97's have
//! their strings in two bytes a character behind a marker that says so;
//! Word 6's are one byte a character, counted by the byte in front of each,
//! with the whole table's length where Word 97 put the count.

/// Positions, and an entry for each stretch between two of them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Plc<'a> {
    pub positions: Vec<u32>,
    entries: &'a [u8],
    size: usize,
}

impl<'a> Plc<'a> {
    /// The table in these bytes, whose entries are `size` bytes long.
    pub fn parse(bytes: &'a [u8], size: usize) -> Self {
        let count = bytes.len().saturating_sub(4) / (4 + size);
        let positions = (0..=count)
            .filter_map(|index| bytes.get(index * 4..index * 4 + 4))
            .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
            .collect::<Vec<_>>();
        if positions.len() < count + 1 {
            return Self::default();
        }
        let entries = bytes.get((count + 1) * 4..(count + 1) * 4 + count * size).unwrap_or(&[]);
        Self { positions, entries, size }
    }

    /// How many entries it has.
    pub fn len(&self) -> usize {
        self.positions.len().saturating_sub(1)
    }

    /// The bytes of one entry.
    pub fn entry(&self, index: usize) -> Option<&'a [u8]> {
        if self.size == 0 {
            return (index < self.len()).then_some(&[][..]);
        }
        self.entries.get(index * self.size..index * self.size + self.size)
    }

    /// Where a stretch begins.
    pub fn start(&self, index: usize) -> Option<u32> {
        self.positions.get(index).copied()
    }

    /// Where it ends: where the next begins.
    pub fn end(&self, index: usize) -> Option<u32> {
        self.positions.get(index + 1).copied()
    }
}

/// A list of strings, each with the extra bytes the table keeps beside it.
pub(crate) fn strings(bytes: &[u8], old: bool) -> Vec<(String, Vec<u8>)> {
    if old {
        return old_strings(bytes);
    }
    let u16_at = |at: usize| bytes.get(at..at + 2).map(|two| u16::from_le_bytes([two[0], two[1]]));
    let Some(first) = u16_at(0) else { return Vec::new() };
    let extended = first == 0xFFFF;
    let (count, extra, mut at) = if extended {
        (u16_at(2).unwrap_or(0), u16_at(4).unwrap_or(0), 6)
    } else {
        (first, u16_at(2).unwrap_or(0), 4)
    };
    let mut out = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let text = if extended {
            let Some(length) = u16_at(at) else { break };
            let length = usize::from(length);
            let Some(units) = bytes.get(at + 2..at + 2 + length * 2) else { break };
            at += 2 + length * 2;
            let units: Vec<u16> =
                units.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            String::from_utf16_lossy(&units)
        } else {
            let Some(&length) = bytes.get(at) else { break };
            let length = usize::from(length);
            let Some(narrow) = bytes.get(at + 1..at + 1 + length) else { break };
            at += 1 + length;
            narrow.iter().map(|byte| crate::text::old_character(*byte)).collect()
        };
        let extra_bytes = bytes.get(at..at + usize::from(extra)).unwrap_or(&[]).to_vec();
        at += usize::from(extra);
        out.push((text, extra_bytes));
    }
    out
}

/// Word 6's: the table's length, and each string counted by its first byte.
fn old_strings(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let total = bytes.get(0..2).map_or(0, |two| usize::from(u16::from_le_bytes([two[0], two[1]])));
    let end = total.min(bytes.len());
    let mut at = 2;
    let mut out = Vec::new();
    while at < end {
        let length = usize::from(bytes[at]);
        let Some(narrow) = bytes.get(at + 1..at + 1 + length) else { break };
        at += 1 + length;
        out.push((
            narrow.iter().map(|byte| crate::text::old_character(*byte)).collect(),
            Vec::new(),
        ));
    }
    out
}

/// Strings one after another with nothing else: the comments' authors.
/// Word 97's are each a count of characters and the characters, two bytes
/// each; Word 6's a count of bytes and the bytes.
pub(crate) fn run_of_strings(bytes: &[u8], old: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if old {
            let length = usize::from(bytes[at]);
            let Some(narrow) = bytes.get(at + 1..at + 1 + length) else { break };
            at += 1 + length;
            out.push(narrow.iter().map(|byte| crate::text::old_character(*byte)).collect());
        } else {
            let Some(two) = bytes.get(at..at + 2) else { break };
            let length = usize::from(u16::from_le_bytes([two[0], two[1]]));
            let Some(units) = bytes.get(at + 2..at + 2 + length * 2) else { break };
            at += 2 + length * 2;
            let units: Vec<u16> =
                units.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            out.push(String::from_utf16_lossy(&units));
        }
    }
    out
}

/// A number some bytes into an entry.
pub(crate) fn u16_at(bytes: &[u8], at: usize) -> u16 {
    bytes.get(at..at + 2).map_or(0, |two| u16::from_le_bytes([two[0], two[1]]))
}

pub(crate) fn u32_at(bytes: &[u8], at: usize) -> u32 {
    bytes.get(at..at + 4).map_or(0, |four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plc_is_positions_then_entries() {
        let mut bytes = Vec::new();
        for position in [0u32, 5, 9] {
            bytes.extend_from_slice(&position.to_le_bytes());
        }
        bytes.extend_from_slice(&[1, 2, 3, 4]);
        let plc = Plc::parse(&bytes, 2);
        assert_eq!(plc.len(), 2);
        assert_eq!((plc.start(1), plc.end(1)), (Some(5), Some(9)));
        assert_eq!(plc.entry(1), Some(&[3, 4][..]));
        assert_eq!(plc.entry(2), None);
    }

    #[test]
    fn both_kinds_of_string_table_are_read() {
        let mut wide = vec![0xFF, 0xFF, 2, 0, 1, 0];
        for (name, extra) in [("ab", 7u8), ("c", 9)] {
            wide.extend_from_slice(&(name.len() as u16).to_le_bytes());
            for unit in name.encode_utf16() {
                wide.extend_from_slice(&unit.to_le_bytes());
            }
            wide.push(extra);
        }
        assert_eq!(
            strings(&wide, false),
            vec![("ab".to_owned(), vec![7]), ("c".to_owned(), vec![9])]
        );
        let old = [9, 0, 2, b'a', b'b', 1, b'c', 0, 0];
        let read = strings(&old, true);
        assert_eq!(read[0].0, "ab");
        assert_eq!(read[1].0, "c");
    }
}

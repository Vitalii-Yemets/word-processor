//! Numbers coded with Huffman tables.
//!
//! [T.88] Annex B. A table is lines, each a prefix length, a range length
//! and the low end of its range: the prefix says which line, and that many
//! bits more where in its range. The prefixes themselves are made from
//! their lengths alone, the same way for every table. Fifteen tables are
//! standard; a file may send its own.

use std::collections::HashMap;

/// Bits read most significant first.
pub struct Bits<'a> {
    pub data: &'a [u8],
    /// In bits.
    pub at: usize,
}

impl Bits<'_> {
    pub fn bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.at / 8)?;
        let bit = (byte >> (7 - self.at % 8)) & 1;
        self.at += 1;
        Some(u32::from(bit))
    }

    pub fn read(&mut self, count: u32) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }
        Some(value)
    }

    pub fn align(&mut self) {
        self.at = self.at.div_ceil(8) * 8;
    }

    /// The byte the reading is at, once aligned.
    #[must_use]
    pub fn byte(&self) -> usize {
        self.at.div_ceil(8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Normal,
    Lower,
    Upper,
    OutOfBand,
}

#[derive(Clone, Copy, Debug)]
struct Line {
    prefix: u32,
    range: u32,
    low: i64,
    kind: Kind,
}

/// A table, its prefixes assigned.
#[derive(Clone, Debug, Default)]
pub struct Table {
    lines: Vec<Line>,
    codes: HashMap<(u32, u32), usize>,
    longest: u32,
}

/// What a table decodes to: a number, or the out-of-band value.
pub type Value = Option<i64>;

impl Table {
    fn build(lines: Vec<Line>) -> Self {
        let lengths: Vec<u32> = lines.iter().map(|line| line.prefix).collect();
        let codes = assign(&lengths);
        let mut map = HashMap::new();
        for (index, code) in codes.into_iter().enumerate() {
            if let Some(code) = code {
                map.insert((lines[index].prefix, code), index);
            }
        }
        let longest = lengths.iter().copied().max().unwrap_or(0);
        Self { lines, codes: map, longest }
    }

    /// A table whose every line is one value, numbered in order: for the
    /// codes of symbol numbers and of their code lengths.
    #[must_use]
    pub fn of_lengths(lengths: &[u32]) -> Self {
        Self::build(
            lengths
                .iter()
                .enumerate()
                .map(|(index, &prefix)| Line {
                    prefix,
                    range: 0,
                    low: index as i64,
                    kind: Kind::Normal,
                })
                .collect(),
        )
    }

    /// One value, or nothing when the bits run out or match no line.
    pub fn decode(&self, bits: &mut Bits<'_>) -> Option<Value> {
        let mut code = 0u32;
        for length in 1..=self.longest.max(1) {
            code = (code << 1) | bits.bit()?;
            if let Some(&index) = self.codes.get(&(length, code)) {
                let line = self.lines[index];
                return Some(match line.kind {
                    Kind::OutOfBand => None,
                    Kind::Normal => Some(line.low + i64::from(bits.read(line.range)?)),
                    Kind::Lower => Some(line.low - i64::from(bits.read(32)?)),
                    Kind::Upper => Some(line.low + i64::from(bits.read(32)?)),
                });
            }
        }
        None
    }
}

/// Prefix codes from their lengths, [T.88] B.3: shorter first, and in
/// order among the same length. A length of nought gets no code.
fn assign(lengths: &[u32]) -> Vec<Option<u32>> {
    let longest = lengths.iter().copied().max().unwrap_or(0) as usize;
    let mut counts = vec![0u32; longest + 1];
    for &length in lengths {
        counts[length as usize] += 1;
    }
    counts[0] = 0;
    let mut codes = vec![None; lengths.len()];
    let mut first = 0u32;
    for length in 1..=longest {
        first = (first + counts[length - 1]) << 1;
        let mut current = first;
        for (index, &line_length) in lengths.iter().enumerate() {
            if line_length as usize == length {
                codes[index] = Some(current);
                current += 1;
            }
        }
    }
    codes
}

/// Lines as the standard gives them: prefix length, range length, low end;
/// then the lower range, the upper range and the out-of-band line, a
/// prefix length of nought where the table has none.
struct Standard {
    lines: &'static [(u32, u32, i64)],
    lower: (u32, i64),
    upper: (u32, i64),
    out_of_band: u32,
}

const STANDARD: [Standard; 15] = [
    Standard {
        lines: &[(1, 4, 0), (2, 8, 16), (3, 16, 272)],
        lower: (0, 0),
        upper: (3, 65808),
        out_of_band: 0,
    },
    Standard {
        lines: &[(1, 0, 0), (2, 0, 1), (3, 0, 2), (4, 3, 3), (5, 6, 11)],
        lower: (0, 0),
        upper: (6, 75),
        out_of_band: 6,
    },
    Standard {
        lines: &[(8, 8, -256), (1, 0, 0), (2, 0, 1), (3, 0, 2), (4, 3, 3), (5, 6, 11)],
        lower: (8, -257),
        upper: (7, 75),
        out_of_band: 6,
    },
    Standard {
        lines: &[(1, 0, 1), (2, 0, 2), (3, 0, 3), (4, 3, 4), (5, 6, 12)],
        lower: (0, 0),
        upper: (5, 76),
        out_of_band: 0,
    },
    Standard {
        lines: &[(7, 8, -255), (1, 0, 1), (2, 0, 2), (3, 0, 3), (4, 3, 4), (5, 6, 12)],
        lower: (7, -256),
        upper: (6, 76),
        out_of_band: 0,
    },
    Standard {
        lines: &[
            (5, 10, -2048),
            (4, 9, -1024),
            (4, 8, -512),
            (4, 7, -256),
            (5, 6, -128),
            (5, 5, -64),
            (4, 5, -32),
            (2, 7, 0),
            (3, 7, 128),
            (3, 8, 256),
            (4, 9, 512),
            (4, 10, 1024),
        ],
        lower: (6, -2049),
        upper: (6, 2048),
        out_of_band: 0,
    },
    Standard {
        lines: &[
            (4, 9, -1024),
            (3, 8, -512),
            (4, 7, -256),
            (5, 6, -128),
            (5, 5, -64),
            (4, 5, -32),
            (4, 5, 0),
            (5, 5, 32),
            (5, 6, 64),
            (4, 7, 128),
            (3, 8, 256),
            (3, 9, 512),
            (3, 10, 1024),
        ],
        lower: (5, -1025),
        upper: (5, 2048),
        out_of_band: 0,
    },
    Standard {
        lines: &[
            (8, 3, -15),
            (9, 1, -7),
            (8, 1, -5),
            (9, 0, -3),
            (7, 0, -2),
            (4, 0, -1),
            (2, 1, 0),
            (5, 0, 2),
            (6, 0, 3),
            (3, 4, 4),
            (6, 1, 20),
            (4, 4, 22),
            (4, 5, 38),
            (5, 6, 70),
            (5, 7, 134),
            (6, 7, 262),
            (7, 8, 390),
            (6, 10, 646),
        ],
        lower: (9, -16),
        upper: (9, 1670),
        out_of_band: 2,
    },
    Standard {
        lines: &[
            (8, 4, -31),
            (9, 2, -15),
            (8, 2, -11),
            (9, 1, -7),
            (7, 1, -5),
            (4, 1, -3),
            (3, 1, -1),
            (3, 1, 1),
            (5, 1, 3),
            (6, 1, 5),
            (3, 5, 7),
            (6, 2, 39),
            (4, 5, 43),
            (4, 6, 75),
            (5, 7, 139),
            (5, 8, 267),
            (6, 8, 523),
            (7, 9, 779),
            (6, 11, 1291),
        ],
        lower: (9, -32),
        upper: (9, 3339),
        out_of_band: 2,
    },
    Standard {
        lines: &[
            (7, 4, -21),
            (8, 0, -5),
            (7, 0, -4),
            (5, 0, -3),
            (2, 2, -2),
            (5, 0, 2),
            (6, 0, 3),
            (7, 0, 4),
            (8, 0, 5),
            (2, 6, 6),
            (5, 5, 70),
            (6, 5, 102),
            (6, 6, 134),
            (6, 7, 198),
            (6, 8, 326),
            (6, 9, 582),
            (6, 10, 1094),
            (7, 11, 2118),
        ],
        lower: (8, -22),
        upper: (8, 4166),
        out_of_band: 2,
    },
    Standard {
        lines: &[
            (1, 0, 1),
            (2, 1, 2),
            (4, 0, 4),
            (4, 1, 5),
            (5, 1, 7),
            (5, 2, 9),
            (6, 2, 13),
            (7, 2, 17),
            (7, 3, 21),
            (7, 4, 29),
            (7, 5, 45),
            (7, 6, 77),
        ],
        lower: (0, 0),
        upper: (7, 141),
        out_of_band: 0,
    },
    Standard {
        lines: &[
            (1, 0, 1),
            (2, 0, 2),
            (3, 1, 3),
            (5, 0, 5),
            (5, 1, 6),
            (6, 1, 8),
            (7, 0, 10),
            (7, 1, 11),
            (7, 2, 13),
            (7, 3, 17),
            (7, 4, 25),
            (8, 5, 41),
        ],
        lower: (0, 0),
        upper: (8, 73),
        out_of_band: 0,
    },
    Standard {
        lines: &[
            (1, 0, 1),
            (3, 0, 2),
            (4, 0, 3),
            (5, 0, 4),
            (4, 1, 5),
            (3, 3, 7),
            (6, 1, 15),
            (6, 2, 17),
            (6, 3, 21),
            (6, 4, 29),
            (6, 5, 45),
            (7, 6, 77),
        ],
        lower: (0, 0),
        upper: (7, 141),
        out_of_band: 0,
    },
    Standard {
        lines: &[(3, 0, -2), (3, 0, -1), (1, 0, 0), (3, 0, 1), (3, 0, 2)],
        lower: (0, 0),
        upper: (0, 3),
        out_of_band: 0,
    },
    Standard {
        lines: &[
            (7, 4, -24),
            (6, 2, -8),
            (5, 1, -4),
            (4, 0, -2),
            (3, 0, -1),
            (1, 0, 0),
            (3, 0, 1),
            (4, 0, 2),
            (5, 1, 3),
            (6, 2, 5),
            (7, 4, 9),
        ],
        lower: (7, -25),
        upper: (7, 25),
        out_of_band: 0,
    },
];

/// Standard table B.`number`, one to fifteen.
#[must_use]
pub fn standard(number: usize) -> Table {
    let standard = &STANDARD[number.clamp(1, 15) - 1];
    let mut lines: Vec<Line> = standard
        .lines
        .iter()
        .map(|&(prefix, range, low)| Line { prefix, range, low, kind: Kind::Normal })
        .collect();
    let extra = [
        (standard.lower.0, standard.lower.1, Kind::Lower),
        (standard.upper.0, standard.upper.1, Kind::Upper),
        (standard.out_of_band, 0, Kind::OutOfBand),
    ];
    for (prefix, low, kind) in extra {
        if prefix > 0 {
            lines.push(Line {
                prefix,
                range: if kind == Kind::OutOfBand { 0 } else { 32 },
                low,
                kind,
            });
        }
    }
    Table::build(lines)
}

/// A table a file sends, [T.88] B.2.
#[must_use]
pub fn custom(data: &[u8]) -> Option<Table> {
    let flags = *data.first()?;
    let out_of_band = flags & 1 != 0;
    let prefix_bits = u32::from((flags >> 1) & 7) + 1;
    let range_bits = u32::from((flags >> 4) & 7) + 1;
    let low = i64::from(i32::from_be_bytes(data.get(1..5)?.try_into().ok()?));
    let high = i64::from(i32::from_be_bytes(data.get(5..9)?.try_into().ok()?));
    let mut bits = Bits { data: &data[9..], at: 0 };
    let mut lines = Vec::new();
    let mut current = low;
    while current < high {
        let prefix = bits.read(prefix_bits)?;
        let range = bits.read(range_bits)?;
        lines.push(Line { prefix, range, low: current, kind: Kind::Normal });
        current += 1i64 << range.min(40);
        if lines.len() > 1 << 16 {
            return None;
        }
    }
    let prefix = bits.read(prefix_bits)?;
    lines.push(Line { prefix, range: 32, low: low - 1, kind: Kind::Lower });
    let prefix = bits.read(prefix_bits)?;
    lines.push(Line { prefix, range: 32, low: high, kind: Kind::Upper });
    if out_of_band {
        let prefix = bits.read(prefix_bits)?;
        lines.push(Line { prefix, range: 0, low: 0, kind: Kind::OutOfBand });
    }
    Some(Table::build(lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_come_from_their_lengths() {
        // The standard's example in B.3's terms: lengths 1, 2, 3, 3 give
        // 0, 10, 110, 111.
        assert_eq!(assign(&[1, 2, 3, 3]), vec![Some(0), Some(0b10), Some(0b110), Some(0b111)]);
        assert_eq!(assign(&[2, 0, 1]), vec![Some(0b10), None, Some(0)]);
    }

    #[test]
    fn a_standard_table_decodes_its_ranges() {
        // B.1: "0" and four bits is 0 to 15; "10" and eight bits from 16.
        let table = standard(1);
        // "0" and 0101; "10" and 0000_0001.
        let data = [0x2C, 0x02];
        let mut bits = Bits { data: &data, at: 0 };
        assert_eq!(table.decode(&mut bits), Some(Some(5)));
        assert_eq!(table.decode(&mut bits), Some(Some(16 + 1)));
        // B.2's out-of-band line is six bits of ones.
        let table = standard(2);
        let data = [0b1111_1100];
        let mut bits = Bits { data: &data, at: 0 };
        assert_eq!(table.decode(&mut bits), Some(None));
    }
}

//! Word 6's and Word 95's modifiers, said again as Word 97's.
//!
//! A Word 6 sprm is one byte of number and an operand whose length that
//! number fixes — there is nothing in the number that says how long, so a
//! reader has to know every one or stop at the first it does not. Word 97
//! gave each the two-byte code [`crate::sprm`] reads, and most of them kept
//! their meaning and their operand. So a Word 6 group is turned into the Word
//! 97 group it would have been, and everything after this reads one kind.
//!
//! The ones whose operand changed are changed with them: a border was two
//! bytes and became four, a table cell's description ten bytes and became
//! twenty, a symbol's character one byte and became two, and a picture's
//! place lost the length Word 6 wrote in front of it.

use crate::sprm;

/// How long a Word 6 modifier's operand is: fixed, or given by the byte in
/// front of it, or — for the table definition alone — by the two bytes in
/// front of it.
#[derive(Clone, Copy)]
enum Length {
    Fixed(usize),
    Counted,
    Counted2,
}

fn length_of(number: u8) -> Option<Length> {
    use Length::{Counted, Counted2, Fixed};
    Some(match number {
        2 | 4..=11 | 13 | 14 | 24 | 25 | 29 | 37 | 44 | 50 | 51 => Fixed(1),
        16..=19 | 21 | 22 | 26..=28 | 30..=36 | 38..=43 | 45..=49 => Fixed(2),
        20 => Fixed(4),
        3 | 12 | 15 | 23 => Counted,
        52 => Fixed(0),
        65..=67 | 71 | 75 => Fixed(1),
        69 | 72 => Fixed(2),
        70 => Fixed(4),
        73 => Fixed(3),
        68 | 74 | 79 | 81 | 82 => Counted,
        83 => Fixed(0),
        85..=92 | 94 | 98 | 100 | 102 | 104 => Fixed(1),
        80 | 93 | 96 | 97 | 99 | 101 | 107 | 109..=112 => Fixed(2),
        95 => Fixed(3),
        103 | 105 | 106 | 108 | 113 | 115 | 116 => Counted,
        117..=119 => Fixed(1),
        120 => Counted,
        121..=124 => Fixed(2),
        131 | 132 | 138 | 139 | 142 | 143 | 146 | 147 | 150..=153 | 158 | 159 | 162 => Fixed(1),
        133 | 179 | 181 | 188 | 191 | 207 => Counted,
        136 | 137 => Fixed(3),
        140 | 141 | 144 | 145 | 148 | 149 | 154..=157 | 160 | 161 | 164..=171 => Fixed(2),
        163 => Fixed(0),
        182..=184 | 189 | 195 | 197 | 198 => Fixed(2),
        185 | 186 => Fixed(1),
        187 => Fixed(12),
        190 => Counted2,
        192 | 194 | 196 | 200 => Fixed(4),
        193 | 199 => Fixed(5),
        _ => return None,
    })
}

/// The Word 97 code a Word 6 number became, where its operand is the same.
fn same_operand(number: u8) -> Option<u16> {
    Some(match number {
        5 => sprm::P_JC_OLD,
        7 => sprm::P_KEEP,
        8 => sprm::P_KEEP_FOLLOW,
        9 => sprm::P_PAGE_BREAK_BEFORE,
        12 => sprm::P_ANLD,
        13 => sprm::P_NUMBERED_LEVEL,
        15 => sprm::P_CHANGE_TABS,
        16 => sprm::P_DXA_RIGHT_80,
        17 => sprm::P_DXA_LEFT_80,
        19 => sprm::P_DXA_LEFT1_80,
        20 => sprm::P_DYA_LINE,
        21 => sprm::P_DYA_BEFORE,
        22 => sprm::P_DYA_AFTER,
        24 => sprm::P_IN_TABLE,
        25 => sprm::P_TABLE_ROW_END,
        44 => sprm::P_NO_AUTO_HYPHEN,
        47 => sprm::P_SHADING_80,
        51 => sprm::P_WIDOW_CONTROL,
        65 => sprm::C_DELETED,
        66 => sprm::C_INSERTED,
        67 => sprm::C_FIELD_VANISH,
        69 => sprm::C_REVISION_AUTHOR,
        70 => sprm::C_REVISION_DATE,
        71 => sprm::C_DATA,
        75 => sprm::C_OLE2,
        80 => sprm::C_ISTD,
        85 => sprm::C_BOLD,
        86 => sprm::C_ITALIC,
        87 => sprm::C_STRIKE,
        88 => sprm::C_OUTLINE,
        90 => sprm::C_SMALL_CAPS,
        91 => sprm::C_CAPS,
        92 => sprm::C_HIDDEN,
        93 => sprm::C_FONT,
        94 => sprm::C_UNDERLINE,
        96 => sprm::C_SPACING,
        97 => sprm::C_LID,
        98 => sprm::C_COLOUR_INDEX,
        99 => sprm::C_SIZE,
        101 => sprm::C_POSITION,
        104 => sprm::C_SUPER_SUB,
        117 => sprm::C_SPECIAL,
        118 => sprm::C_OBJECT,
        142 => sprm::S_BREAK,
        143 => sprm::S_TITLE_PAGE,
        144 => sprm::S_COLUMNS,
        145 => sprm::S_COLUMN_GAP,
        147 => sprm::S_PAGE_NUMBER_FORMAT,
        150 => sprm::S_PAGE_NUMBER_RESTART,
        153 => sprm::S_HEADERS,
        156 => sprm::S_HEADER_DISTANCE,
        157 => sprm::S_FOOTER_DISTANCE,
        159 => sprm::S_VERTICAL_ALIGNMENT,
        161 => sprm::S_PAGE_NUMBER_START,
        162 => sprm::S_ORIENTATION,
        164 => sprm::S_PAGE_WIDTH,
        165 => sprm::S_PAGE_HEIGHT,
        166 => sprm::S_MARGIN_LEFT,
        167 => sprm::S_MARGIN_RIGHT,
        168 => sprm::S_MARGIN_TOP,
        169 => sprm::S_MARGIN_BOTTOM,
        170 => sprm::S_GUTTER,
        182 => sprm::T_JC_90,
        183 => sprm::T_DXA_LEFT,
        184 => sprm::T_DXA_GAP_HALF,
        185 => sprm::T_CANT_SPLIT_90,
        186 => sprm::T_HEADER,
        189 => sprm::T_ROW_HEIGHT,
        191 => sprm::T_SHADING_80,
        197 => sprm::T_MERGE,
        198 => sprm::T_SPLIT,
        200 => sprm::T_SET_SHADING_80,
        _ => return None,
    })
}

/// A Word 6 group, as the Word 97 group it would have been. What has no
/// Word 97 equal here is left out; a number this does not know ends the
/// group, since there is no telling how long it is.
#[must_use]
pub fn translate(old: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(old.len() * 2);
    let mut at = 0;
    while at < old.len() {
        let number = old[at];
        at += 1;
        let Some(length) = length_of(number) else { break };
        let (operand, next) = match length {
            Length::Fixed(size) => (old.get(at..at + size), at + size),
            Length::Counted => {
                let size = usize::from(old.get(at).copied().unwrap_or(0));
                (old.get(at + 1..at + 1 + size), at + 1 + size)
            }
            // The count includes one byte more than the operand has.
            Length::Counted2 => {
                let size = old
                    .get(at..at + 2)
                    .map_or(0, |two| usize::from(u16::from_le_bytes([two[0], two[1]])))
                    .saturating_sub(1);
                (old.get(at + 2..at + 2 + size), at + 2 + size)
            }
        };
        let Some(operand) = operand else { break };
        at = next;
        push_translated(&mut out, number, operand);
    }
    out
}

/// One modifier, said again.
fn push_translated(out: &mut Vec<u8>, number: u8, operand: &[u8]) {
    if let Some(code) = same_operand(number) {
        push(out, code, operand);
        return;
    }
    match number {
        // The style: a byte then, two bytes now.
        2 => push(out, sprm::P_ISTD, &u16::from(operand[0]).to_le_bytes()),
        // A paragraph's borders: the old two bytes made the new four.
        38..=42 => {
            let code = [
                sprm::P_BORDER_TOP_80,
                sprm::P_BORDER_LEFT_80,
                sprm::P_BORDER_BOTTOM_80,
                sprm::P_BORDER_RIGHT_80,
                sprm::P_BORDER_BETWEEN_80,
            ][usize::from(number - 38)];
            push(out, code, &border(operand));
        }
        // A picture's place, without the count in front of it.
        68 => {
            if let Some(four) = operand.get(0..4) {
                push(out, sprm::C_PICTURE, four);
            }
        }
        // A symbol: its font, and its character, which was one byte.
        74 => {
            if let (Some(font), Some(&character)) = (operand.get(0..2), operand.get(2)) {
                let mut both = font.to_vec();
                both.extend_from_slice(&u16::from(character).to_le_bytes());
                push(out, sprm::C_SYMBOL, &both);
            }
        }
        // The table's borders, six of them.
        187 => {
            let mut all = Vec::with_capacity(24);
            for pair in operand.chunks_exact(2) {
                all.extend_from_slice(&border(pair));
            }
            push(out, sprm::T_BORDERS_80, &all);
        }
        // The row: where the cells' edges are, and each cell's description,
        // whose four borders grew.
        190 => push(out, sprm::T_DEFINITION, &definition(operand)),
        // One border set over several cells.
        193 if operand.len() >= 5 => {
            let mut new = operand[0..3].to_vec();
            new.extend_from_slice(&border(&operand[3..5]));
            push(out, sprm::T_SET_BORDER_80, &new);
        }
        _ => {}
    }
}

/// Writes a Word 97 modifier: its code, a count where its code says the
/// operand is counted, and the operand.
fn push(out: &mut Vec<u8>, code: u16, operand: &[u8]) {
    out.extend_from_slice(&code.to_le_bytes());
    if code >> 13 == 6 {
        if code == sprm::T_DEFINITION {
            // A table definition's count is one more than its operand.
            out.extend_from_slice(&(operand.len() as u16 + 1).to_le_bytes());
        } else {
            out.push(operand.len() as u8);
        }
    }
    out.extend_from_slice(operand);
}

/// Word 6's two-byte border as Word 97's four: the width in three bits, the
/// kind in two, a shadow, the colour in five and the space in five, made
/// the width in eighths of a point, the kind, the colour and the space with
/// the shadow.
fn border(old: &[u8]) -> [u8; 4] {
    let value = old.get(0..2).map_or(0, |two| u16::from_le_bytes([two[0], two[1]]));
    if value == 0 {
        return [0; 4];
    }
    if value == 0xFFFF {
        return [0xFF; 4];
    }
    let width = (value & 0x0007) as u8;
    let kind = ((value >> 3) & 0x0003) as u8;
    let shadow = (value >> 5) & 1 != 0;
    let colour = ((value >> 6) & 0x001F) as u8;
    let space = ((value >> 11) & 0x001F) as u8;
    // Widths six and seven were a dotted and a dashed hairline; the others
    // are three-quarters of a point each.
    let (width, kind) = match width {
        6 => (2, 6),
        7 => (2, 7),
        _ => (width.saturating_mul(6), kind),
    };
    [width, kind, colour, space | if shadow { 0x20 } else { 0 }]
}

/// Word 6's row definition as Word 97's: the count of cells, their edges,
/// and each cell's ten-byte description made twenty.
fn definition(old: &[u8]) -> Vec<u8> {
    let Some(&count) = old.first() else { return Vec::new() };
    let count = usize::from(count);
    let edges_end = 1 + (count + 1) * 2;
    let mut new = old.get(..edges_end.min(old.len())).unwrap_or(&[]).to_vec();
    let mut at = edges_end;
    while at + 10 <= old.len() && (at - edges_end) / 10 < count {
        let cell = &old[at..at + 10];
        new.extend_from_slice(&cell[0..2]);
        new.extend_from_slice(&[0, 0]);
        for side in cell[2..10].chunks_exact(2) {
            new.extend_from_slice(&border(side));
        }
        at += 10;
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_six_group_reads_as_the_word_97_one() {
        // Bold, the size, a symbol, the left indent, and the style number.
        let old = [85, 1, 99, 24, 0, 74, 3, 2, 0, 0xB7, 17, 0xD0, 0x02, 2, 5];
        let translated = translate(&old);
        let sprms = sprm::parse(&translated);
        let codes: Vec<u16> = sprms.iter().map(|one| one.code).collect();
        assert_eq!(
            codes,
            vec![sprm::C_BOLD, sprm::C_SIZE, sprm::C_SYMBOL, sprm::P_DXA_LEFT_80, sprm::P_ISTD]
        );
        assert_eq!(sprms[1].u16(), 24);
        assert_eq!(sprms[2].operand, &[2, 0, 0xB7, 0]);
        assert_eq!(sprms[3].i16(), 720);
        assert_eq!(sprms[4].u16(), 5);
    }

    #[test]
    fn a_number_nobody_knows_ends_the_group() {
        let old = [85, 1, 1, 7, 86, 1];
        let sprms = translate(&old);
        assert_eq!(sprm::parse(&sprms).len(), 1);
    }

    #[test]
    fn a_row_definition_grows_its_cells() {
        // Two cells, three edges, and a cell description each, the first
        // with a single line along its top.
        let mut old = vec![2];
        for edge in [0i16, 1440, 2880] {
            old.extend_from_slice(&edge.to_le_bytes());
        }
        let single = 1u16 | (1 << 3);
        let mut first = vec![0, 0];
        first.extend_from_slice(&single.to_le_bytes());
        first.extend_from_slice(&[0; 6]);
        old.extend_from_slice(&first);
        old.extend_from_slice(&[0; 10]);
        let mut group = vec![190];
        group.extend_from_slice(&(old.len() as u16 + 1).to_le_bytes());
        group.extend_from_slice(&old);
        let translated = translate(&group);
        let sprms = sprm::parse(&translated);
        assert_eq!(sprms.len(), 1);
        assert_eq!(sprms[0].code, sprm::T_DEFINITION);
        assert_eq!(sprms[0].operand.len(), 1 + 6 + 40);
        assert_eq!(&sprms[0].operand[7 + 4..7 + 8], &[6, 1, 0, 0]);
    }
}

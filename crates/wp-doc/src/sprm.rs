//! Single property modifiers: how the binary file says "bold", "centred",
//! "half an inch in".
//!
//! [MS-DOC] 2.6. A sprm is a two-byte code and an operand. The code says
//! what it is about — a character, a paragraph, a table, a section — and
//! how long its operand is, so that a reader can step over the ones it
//! does not know, which is what lets a Word 97 reader open a Word 2003
//! file. A run of them, a `grpprl`, is the whole of a piece of formatting:
//! the paragraph's, the run's, the style's.

/// One modifier: its code, and its operand as bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sprm<'a> {
    pub code: u16,
    pub operand: &'a [u8],
}

impl Sprm<'_> {
    /// The operand as the one byte it usually is.
    #[must_use]
    pub fn byte(&self) -> u8 {
        self.operand.first().copied().unwrap_or(0)
    }

    /// The operand as a toggle: `1` on, `0` off, anything else "as the
    /// style has it", which this reads as on.
    #[must_use]
    pub fn on(&self) -> bool {
        self.byte() != 0
    }

    #[must_use]
    pub fn u16(&self) -> u16 {
        match self.operand {
            [low, high, ..] => u16::from_le_bytes([*low, *high]),
            [one] => u16::from(*one),
            [] => 0,
        }
    }

    #[must_use]
    pub fn i16(&self) -> i16 {
        self.u16() as i16
    }

    #[must_use]
    pub fn u32(&self) -> u32 {
        match self.operand {
            [a, b, c, d, ..] => u32::from_le_bytes([*a, *b, *c, *d]),
            _ => u32::from(self.u16()),
        }
    }
}

/// Every modifier in a group, in order, stepping over each by the length
/// its code declares.
#[must_use]
pub fn parse(grpprl: &[u8]) -> Vec<Sprm<'_>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 2 <= grpprl.len() {
        let code = u16::from_le_bytes([grpprl[at], grpprl[at + 1]]);
        at += 2;
        let spra = code >> 13;
        let length = match spra {
            0 | 1 => 1,
            2 | 4 | 5 => 2,
            3 => 4,
            7 => 3,
            // Variable: the first byte says how long — except for the table
            // definition, whose length is two bytes because a table is long,
            // and counts one byte more than follows it.
            _ => {
                if code == 0xD608 {
                    let Some(two) = grpprl.get(at..at + 2) else { break };
                    at += 2;
                    usize::from(u16::from_le_bytes([two[0], two[1]])).saturating_sub(1)
                } else {
                    let Some(one) = grpprl.get(at) else { break };
                    at += 1;
                    usize::from(*one)
                }
            }
        };
        let Some(operand) = grpprl.get(at..at + length) else { break };
        at += length;
        out.push(Sprm { code, operand });
    }
    out
}

// --- Paragraph ---------------------------------------------------------------
pub const P_ISTD: u16 = 0x4600;
pub const P_JC_OLD: u16 = 0x2403;
pub const P_KEEP: u16 = 0x2405;
pub const P_KEEP_FOLLOW: u16 = 0x2406;
pub const P_PAGE_BREAK_BEFORE: u16 = 0x2407;
pub const P_ILVL: u16 = 0x260A;
pub const P_ILFO: u16 = 0x460B;
/// Word 6's numbered paragraphs: which level of its outline numbering.
pub const P_NUMBERED_LEVEL: u16 = 0x260D;
/// Tab stops taken away and put in.
pub const P_CHANGE_TABS: u16 = 0xC60D;
pub const P_DXA_RIGHT_80: u16 = 0x840E;
pub const P_DXA_LEFT_80: u16 = 0x840F;
pub const P_DXA_LEFT1_80: u16 = 0x8411;
pub const P_DYA_LINE: u16 = 0x6412;
pub const P_DYA_BEFORE: u16 = 0xA413;
pub const P_DYA_AFTER: u16 = 0xA414;
pub const P_IN_TABLE: u16 = 0x2416;
pub const P_TABLE_ROW_END: u16 = 0x2417;
pub const P_BORDER_TOP_80: u16 = 0x6424;
pub const P_BORDER_LEFT_80: u16 = 0x6425;
pub const P_BORDER_BOTTOM_80: u16 = 0x6426;
pub const P_BORDER_RIGHT_80: u16 = 0x6427;
pub const P_BORDER_BETWEEN_80: u16 = 0x6428;
pub const P_NO_AUTO_HYPHEN: u16 = 0x242A;
pub const P_SHADING_80: u16 = 0x442D;
pub const P_WIDOW_CONTROL: u16 = 0x2431;
/// Word 6's numbering of a paragraph, described in full on the paragraph.
pub const P_ANLD: u16 = 0xC63E;
pub const P_OUTLINE_LEVEL: u16 = 0x2640;
/// The paragraph runs right to left.
pub const P_BIDI: u16 = 0x2441;
/// A cell of a table inside a table ends here, and a row of one.
pub const P_INNER_CELL: u16 = 0x244B;
pub const P_INNER_ROW_END: u16 = 0x244C;
pub const P_SHADING: u16 = 0xC64D;
pub const P_BORDER_TOP: u16 = 0xC64E;
pub const P_BORDER_LEFT: u16 = 0xC64F;
pub const P_BORDER_BOTTOM: u16 = 0xC650;
pub const P_BORDER_RIGHT: u16 = 0xC651;
pub const P_BORDER_BETWEEN: u16 = 0xC652;
pub const P_DXA_RIGHT: u16 = 0x845D;
pub const P_DXA_LEFT: u16 = 0x845E;
pub const P_DXA_LEFT1: u16 = 0x8460;
pub const P_JC: u16 = 0x2461;
/// How deep in tables the paragraph is.
pub const P_ITAP: u16 = 0x6649;
pub const P_CONTEXTUAL_SPACING: u16 = 0x246D;

// --- Character ---------------------------------------------------------------
/// Tracked changes: deleted, inserted, by whom and when.
pub const C_DELETED: u16 = 0x0800;
pub const C_INSERTED: u16 = 0x0801;
pub const C_FIELD_VANISH: u16 = 0x0802;
pub const C_PICTURE: u16 = 0x6A03;
pub const C_REVISION_AUTHOR: u16 = 0x4804;
pub const C_REVISION_DATE: u16 = 0x6805;
pub const C_DATA: u16 = 0x0806;
/// A character of a symbol font: which font, and which character.
pub const C_SYMBOL: u16 = 0x6A09;
pub const C_OLE2: u16 = 0x080A;
pub const C_HIGHLIGHT: u16 = 0x2A0C;
pub const C_ISTD: u16 = 0x4A30;
pub const C_BOLD: u16 = 0x0835;
pub const C_ITALIC: u16 = 0x0836;
pub const C_STRIKE: u16 = 0x0837;
pub const C_OUTLINE: u16 = 0x0838;
pub const C_SMALL_CAPS: u16 = 0x083A;
pub const C_CAPS: u16 = 0x083B;
pub const C_HIDDEN: u16 = 0x083C;
/// The font as Word 6 named it, which Word 97 still writes beside its own.
pub const C_FONT_DEFAULT: u16 = 0x4A3D;
pub const C_UNDERLINE: u16 = 0x2A3E;
pub const C_SPACING: u16 = 0x8840;
/// The language as Word 6 named it.
pub const C_LID: u16 = 0x4A41;
pub const C_COLOUR_INDEX: u16 = 0x2A42;
pub const C_SIZE: u16 = 0x4A43;
pub const C_POSITION: u16 = 0x4845;
pub const C_SUPER_SUB: u16 = 0x2A48;
pub const C_FONT: u16 = 0x4A4F;
pub const C_FONT_EAST: u16 = 0x4A50;
pub const C_FONT_OTHER: u16 = 0x4A51;
pub const C_DOUBLE_STRIKE: u16 = 0x2A53;
pub const C_SPECIAL: u16 = 0x0855;
pub const C_OBJECT: u16 = 0x0856;
/// The run is right to left.
pub const C_BIDI: u16 = 0x085A;
/// A tracked change of formatting: whether, by whom, and when.
pub const C_FORMAT_CHANGE_90: u16 = 0xCA57;
pub const C_FORMAT_CHANGE: u16 = 0xCA89;
/// Who deleted text, and when, where they are not who inserted it.
pub const C_DELETED_AUTHOR: u16 = 0x4863;
pub const C_DELETED_DATE: u16 = 0x6864;
pub const C_SHADING_OLD: u16 = 0x4866;
/// The language of the Latin text: Word 97's, and Word 2000's.
pub const C_LANGUAGE: u16 = 0x486D;
pub const C_LANGUAGE_NEW: u16 = 0x4873;
pub const C_COLOUR: u16 = 0x6870;
pub const C_SHADING: u16 = 0xCA71;

// --- Table -------------------------------------------------------------------
pub const T_JC_90: u16 = 0x5400;
pub const T_DXA_LEFT: u16 = 0x9601;
pub const T_DXA_GAP_HALF: u16 = 0x9602;
pub const T_CANT_SPLIT_90: u16 = 0x3403;
pub const T_HEADER: u16 = 0x3404;
pub const T_BORDERS_80: u16 = 0xD605;
pub const T_ROW_HEIGHT: u16 = 0x9407;
pub const T_DEFINITION: u16 = 0xD608;
pub const T_SHADING_80: u16 = 0xD609;
pub const T_SHADING_3RD: u16 = 0xD60C;
pub const T_SHADING: u16 = 0xD612;
pub const T_BORDERS: u16 = 0xD613;
pub const T_SHADING_2ND: u16 = 0xD616;
pub const T_SET_BORDER_80: u16 = 0xD620;
pub const T_MERGE: u16 = 0x5624;
pub const T_SPLIT: u16 = 0x5625;
pub const T_SET_SHADING_80: u16 = 0x7627;
pub const T_VERTICAL_MERGE: u16 = 0xD62B;
pub const T_VERTICAL_ALIGN: u16 = 0xD62C;
pub const T_SET_BORDER: u16 = 0xD62F;
pub const T_CANT_SPLIT: u16 = 0x3466;

// --- Section -----------------------------------------------------------------
/// How the section begins: on the same page, in the next column, on the next
/// page, the next even one or the next odd one.
pub const S_BREAK: u16 = 0x3009;
pub const S_TITLE_PAGE: u16 = 0x300A;
/// How many columns, less one, and the room between them.
pub const S_COLUMNS: u16 = 0x500B;
pub const S_COLUMN_GAP: u16 = 0x900C;
pub const S_PAGE_NUMBER_FORMAT: u16 = 0x300E;
pub const S_PAGE_NUMBER_RESTART: u16 = 0x3011;
/// Which headers and footers a Word 6 section has.
pub const S_HEADERS: u16 = 0x3014;
pub const S_HEADER_DISTANCE: u16 = 0xB017;
pub const S_FOOTER_DISTANCE: u16 = 0xB018;
pub const S_VERTICAL_ALIGNMENT: u16 = 0x301A;
pub const S_PAGE_NUMBER_START: u16 = 0x501C;
pub const S_ORIENTATION: u16 = 0x301D;
pub const S_PAGE_WIDTH: u16 = 0xB01F;
pub const S_PAGE_HEIGHT: u16 = 0xB020;
pub const S_MARGIN_LEFT: u16 = 0xB021;
pub const S_MARGIN_RIGHT: u16 = 0xB022;
pub const S_MARGIN_TOP: u16 = 0x9023;
pub const S_MARGIN_BOTTOM: u16 = 0x9024;
pub const S_GUTTER: u16 = 0xB025;

/// Word's sixteen colours, by the index the old files use for them.
#[must_use]
pub fn colour_by_index(index: u8) -> Option<&'static str> {
    Some(match index {
        0 => return None,
        1 => "000000",
        2 => "0000FF",
        3 => "00FFFF",
        4 => "00FF00",
        5 => "FF00FF",
        6 => "FF0000",
        7 => "FFFF00",
        8 => "FFFFFF",
        9 => "000080",
        10 => "008080",
        11 => "008000",
        12 => "800080",
        13 => "800000",
        14 => "808000",
        15 => "808080",
        16 => "C0C0C0",
        _ => return None,
    })
}

/// The highlight name for a colour, where the colour is one of the sixteen.
#[must_use]
pub fn highlight_by_colour(red: u8, green: u8, blue: u8) -> Option<&'static str> {
    let index = (1..=16u8).find(|index| {
        colour_by_index(*index).is_some_and(|hex| hex == format!("{red:02X}{green:02X}{blue:02X}"))
    })?;
    highlight_by_index(index)
}

/// The same sixteen as Word names them for a highlight.
#[must_use]
pub fn highlight_by_index(index: u8) -> Option<&'static str> {
    Some(match index {
        1 => "black",
        2 => "blue",
        3 => "cyan",
        4 => "green",
        5 => "magenta",
        6 => "red",
        7 => "yellow",
        8 => "white",
        9 => "darkBlue",
        10 => "darkCyan",
        11 => "darkGreen",
        12 => "darkMagenta",
        13 => "darkRed",
        14 => "darkYellow",
        15 => "darkGray",
        16 => "lightGray",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_are_stepped_over_by_the_length_their_code_says() {
        // Bold on, size 24 half-points, a left indent of 720 twips, then a
        // variable-length one of three bytes, then a colour of four.
        let bytes = [
            0x35, 0x08, 0x01, 0x43, 0x4A, 0x18, 0x00, 0x5E, 0x84, 0xD0, 0x02, 0x0A, 0xC6, 0x03,
            0xAA, 0xBB, 0xCC, 0x70, 0x68, 0x00, 0x00, 0xFF, 0x00,
        ];
        let sprms = parse(&bytes);
        assert_eq!(sprms.len(), 5, "{sprms:?}");
        assert_eq!(sprms[0].code, C_BOLD);
        assert!(sprms[0].on());
        assert_eq!(sprms[1].code, C_SIZE);
        assert_eq!(sprms[1].u16(), 24);
        assert_eq!(sprms[2].code, P_DXA_LEFT);
        assert_eq!(sprms[2].i16(), 720);
        assert_eq!(sprms[3].operand, &[0xAA, 0xBB, 0xCC]);
        assert_eq!(sprms[4].code, C_COLOUR);
        assert_eq!(sprms[4].u32(), 0x00FF_0000);
    }
}

//! A font cut down to the glyphs a document actually uses.
//!
//! # Why not embed the whole font
//!
//! Two reasons, and the second is the important one.
//!
//! A font is large — a few hundred kilobytes for a text face, several megabytes
//! for one that covers Chinese — and a document using thirty letters of it does
//! not need the rest.
//!
//! And a typeface is somebody's property. The ones Word uses belong to
//! Microsoft, and their licences allow a document to carry the shapes it needs
//! to be read, not the font itself. Embedding the whole file would be handing
//! out a copy of the typeface with every document.
//!
//! # How it is cut
//!
//! The glyphs keep their original numbers and the ones left out are given no
//! outline at all — which is exactly what the format already says about a
//! space. That means nothing has to be renumbered: the text in the PDF refers
//! to the same glyphs the layout chose, and a composite letter still finds the
//! pieces it is drawn from.

use std::collections::BTreeSet;

use wp_font::{Font, GlyphId};

/// The tables a PDF reader needs to draw with a cut-down font, in the order
/// the format wants them written: by tag.
const WANTED: &[&[u8; 4]] =
    &[b"cvt ", b"fpgm", b"glyf", b"head", b"hhea", b"hmtx", b"loca", b"maxp", b"prep"];

/// Builds a font holding only the glyphs given, and the ones they are drawn
/// from.
///
/// `None` when the font is not one this can cut — a PostScript-flavoured font,
/// whose outlines are in a different table altogether.
#[must_use]
pub(crate) fn build(font: &Font<'_>, used: &BTreeSet<u16>) -> Option<Vec<u8>> {
    let head = font.table(b"head")?;
    let glyph_count = font.glyph_count();
    if font.table(b"glyf").is_none() || head.len() < 54 {
        return None;
    }

    // Everything the wanted glyphs are built out of has to come too.
    let mut keep = used.clone();
    for glyph in used {
        for part in font.components(GlyphId(*glyph)) {
            keep.insert(part.0);
        }
    }
    // The first glyph is what a reader draws when it is asked for one that is
    // not there, and every font has it.
    keep.insert(0);

    let mut glyf = Vec::new();
    let mut loca = Vec::with_capacity(usize::from(glyph_count) * 4 + 4);
    for glyph in 0..glyph_count {
        loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
        if keep.contains(&glyph) {
            glyf.extend_from_slice(font.glyph_data(GlyphId(glyph)));
            while glyf.len() % 4 != 0 {
                glyf.push(0);
            }
        }
    }
    loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());

    // The header has to say that the offsets are longs, because the ones
    // written above are.
    let mut head = head.to_vec();
    head[50..52].copy_from_slice(&1u16.to_be_bytes());
    // The checksum of the whole file, which is worked out once the file exists.
    head[8..12].copy_from_slice(&0u32.to_be_bytes());

    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for tag in WANTED {
        let data = match *tag {
            b"glyf" => glyf.clone(),
            b"loca" => loca.clone(),
            b"head" => head.clone(),
            other => match font.table(other) {
                Some(data) => data.to_vec(),
                // Only the hinting tables are optional here; the rest were
                // checked for above.
                None => continue,
            },
        };
        tables.push((**tag, data));
    }

    Some(assemble(&tables))
}

/// Writes the tables out as a font file.
fn assemble(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let count = tables.len() as u16;
    // The three numbers after the count are a binary search hint, and every
    // font writes them even though every reader could work them out.
    let entry_selector = (15 - count.leading_zeros()) as u16;
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = count * 16 - search_range;

    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
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
    use super::{checksum, padded};

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
}

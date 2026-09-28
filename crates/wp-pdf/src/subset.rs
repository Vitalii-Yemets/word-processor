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
/// whose outlines are in a different table altogether and which is cut by
/// [`wp_font::Font::cut_postscript`] instead.
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

    Some(wp_font::assemble(0x0001_0000u32.to_be_bytes(), &tables))
}

//! Sections, and their headers and footers.
//!
//! A table of positions says where each section of the main text ends, and
//! for each a place in the `WordDocument` stream where its sprms are: its
//! paper, its margins, its columns, how it begins, whether its first page is
//! different. Whatever a section's sprms leave out is Word's default, which a
//! Word 97 section writes nothing for.
//!
//! The headers and footers are one story, cut by another table into pieces:
//! first the notes' separators, then six for each section — the even
//! header, the header, the even footer, the footer, the first page's header
//! and the first page's footer. A piece with nothing in it is the one before
//! it carried on. A Word 6 file keeps only the pieces there are: which each
//! section has is one of its sprms, and which separators the document has is
//! in its properties.

use wp_docx::furniture::{Furniture, Which};
use wp_docx::sections::{NumberFormat, PageNumbering, Start};

use crate::fib::{Fib, Table};
use crate::plc::Plc;
use crate::sprm;

/// How a section's pages are set up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageSetup {
    /// The paper, in twips, width first, turned as the section says.
    pub width: i32,
    pub height: i32,
    /// Top, right, bottom and left.
    pub margins: [i32; 4],
    pub landscape: bool,
    pub header_distance: i32,
    pub footer_distance: i32,
    pub columns: usize,
    pub column_gap: i32,
    /// How the section begins.
    pub start: Start,
    /// Whether its first page has a header and footer of its own.
    pub title_page: bool,
    /// How its pages are numbered, where it says anything of its own.
    pub numbering: Option<PageNumbering>,
    /// Which headers and footers a Word 6 section has.
    pub(crate) furniture_bits: u8,
}

impl Default for PageSetup {
    /// Word's: a Letter page, an inch at the top and bottom and an inch and
    /// a quarter at the sides, headers half an inch from the edge, one
    /// column, a new page.
    fn default() -> Self {
        Self {
            width: 12_240,
            height: 15_840,
            margins: [1440, 1800, 1440, 1800],
            landscape: false,
            header_distance: 720,
            footer_distance: 720,
            columns: 1,
            column_gap: 720,
            start: Start::NextPage,
            title_page: false,
            numbering: None,
            furniture_bits: 0,
        }
    }
}

/// Each section: the character position it ends at, and its setup.
pub(crate) fn sections_of(word: &[u8], table: &[u8], fib: &Fib) -> Vec<(u32, PageSetup)> {
    let Some(bytes) =
        fib.table(Table::Sections).and_then(|(offset, length)| table.get(offset..offset + length))
    else {
        return Vec::new();
    };
    let plc = Plc::parse(bytes, 12);
    (0..plc.len())
        .filter_map(|index| {
            let end = plc.end(index)?;
            let sed = plc.entry(index)?;
            let fc = crate::plc::u32_at(sed, 2);
            Some((end, setup_at(word, fc, fib.base.old)))
        })
        .collect()
}

/// A section's sprms, from where its entry says, over Word's defaults.
fn setup_at(word: &[u8], fc: u32, old: bool) -> PageSetup {
    let mut page = PageSetup::default();
    if fc == 0xFFFF_FFFF {
        return page;
    }
    let at = fc as usize;
    let Some(cb) =
        word.get(at..at + 2).map(|two| usize::from(u16::from_le_bytes([two[0], two[1]])))
    else {
        return page;
    };
    let Some(grpprl) = word.get(at + 2..at + 2 + cb) else { return page };
    let grpprl = if old { crate::old::translate(grpprl) } else { grpprl.to_vec() };
    let mut restart = false;
    let mut first_number = 1;
    let mut format = None;
    for sprm in sprm::parse(&grpprl) {
        match sprm.code {
            sprm::S_PAGE_WIDTH => page.width = i32::from(sprm.u16()),
            sprm::S_PAGE_HEIGHT => page.height = i32::from(sprm.u16()),
            sprm::S_MARGIN_TOP => page.margins[0] = i32::from(sprm.i16()),
            sprm::S_MARGIN_RIGHT => page.margins[1] = i32::from(sprm.u16()),
            sprm::S_MARGIN_BOTTOM => page.margins[2] = i32::from(sprm.i16()),
            sprm::S_MARGIN_LEFT => page.margins[3] = i32::from(sprm.u16()),
            sprm::S_ORIENTATION => page.landscape = sprm.byte() == 2,
            sprm::S_HEADER_DISTANCE => page.header_distance = i32::from(sprm.u16()),
            sprm::S_FOOTER_DISTANCE => page.footer_distance = i32::from(sprm.u16()),
            sprm::S_COLUMNS => page.columns = usize::from(sprm.u16()) + 1,
            sprm::S_COLUMN_GAP => page.column_gap = i32::from(sprm.u16()),
            sprm::S_TITLE_PAGE => page.title_page = sprm.on(),
            sprm::S_HEADERS => page.furniture_bits = sprm.byte(),
            sprm::S_BREAK => {
                page.start = match sprm.byte() {
                    0 => Start::Continuous,
                    1 => Start::NextColumn,
                    3 => Start::EvenPage,
                    4 => Start::OddPage,
                    _ => Start::NextPage,
                };
            }
            sprm::S_PAGE_NUMBER_RESTART => restart = sprm.on(),
            sprm::S_PAGE_NUMBER_START => first_number = i32::from(sprm.u16()),
            sprm::S_PAGE_NUMBER_FORMAT => {
                format = Some(match sprm.byte() {
                    1 => NumberFormat::UpperRoman,
                    2 => NumberFormat::LowerRoman,
                    3 => NumberFormat::UpperLetter,
                    4 => NumberFormat::LowerLetter,
                    _ => NumberFormat::Decimal,
                });
            }
            _ => {}
        }
    }
    if restart || format.is_some() {
        page.numbering = Some(PageNumbering {
            start: restart.then_some(first_number),
            format: format.unwrap_or_default(),
        });
    }
    page
}

/// Which header or footer each of a section's six pieces is.
const PIECES: [(Furniture, Which); 6] = [
    (Furniture::Header, Which::Even),
    (Furniture::Header, Which::Default),
    (Furniture::Footer, Which::Even),
    (Furniture::Footer, Which::Default),
    (Furniture::Header, Which::First),
    (Furniture::Footer, Which::First),
];

/// For each section, the headers and footers it has of its own, as stretches
/// of the header story counted from its start.
pub(crate) fn furniture_of(
    table: &[u8],
    fib: &Fib,
    sections: &[(u32, PageSetup)],
    separators: u8,
) -> Vec<Vec<(Furniture, Which, u32, u32)>> {
    let mut out = vec![Vec::new(); sections.len()];
    let Some(bytes) =
        fib.table(Table::Headers).and_then(|(offset, length)| table.get(offset..offset + length))
    else {
        return out;
    };
    let plc = Plc::parse(bytes, 0);
    let stretch = |index: usize| Some((plc.start(index)?, plc.end(index)?));
    if fib.base.old {
        let mut index = (separators & 0x3F).count_ones() as usize;
        for (section, (_, page)) in sections.iter().enumerate() {
            for (bit, (kind, which)) in PIECES.iter().enumerate() {
                if page.furniture_bits & (1 << bit) == 0 {
                    continue;
                }
                if let Some((start, end)) = stretch(index) {
                    if end > start {
                        out[section].push((*kind, *which, start, end));
                    }
                }
                index += 1;
            }
        }
        return out;
    }
    for (section, pieces) in out.iter_mut().enumerate() {
        for (piece, (kind, which)) in PIECES.iter().enumerate() {
            let Some((start, end)) = stretch(6 + section * 6 + piece) else { continue };
            if end > start {
                pieces.push((*kind, *which, start, end));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_says_only_what_differs_from_words_defaults() {
        let mut word = vec![0u8; 16];
        let grpprl = [
            0x1D, 0x30, 2, // landscape
            0x0B, 0x50, 1, 0, // two columns
            0x09, 0x30, 0, // continuous
        ];
        word.extend_from_slice(&(grpprl.len() as u16).to_le_bytes());
        word.extend_from_slice(&grpprl);
        let page = setup_at(&word, 16, false);
        assert!(page.landscape);
        assert_eq!(page.columns, 2);
        assert_eq!(page.start, Start::Continuous);
        assert_eq!(page.margins, [1440, 1800, 1440, 1800], "Word's own margins");
        assert_eq!(setup_at(&word, 0xFFFF_FFFF, false), PageSetup::default());
    }
}

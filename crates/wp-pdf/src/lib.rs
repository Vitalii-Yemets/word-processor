//! Writing a laid-out document out as a PDF, and reading one back in.
//!
//! Reading is the reverse and harder: a PDF says where every glyph goes
//! and nothing of why, so [`open`] works the paragraphs back out — see
//! the `read` module for how.
//!
//! # What a PDF is, for this purpose
//!
//! A heap of numbered objects with an index at the end. One of them says which
//! pages there are; each page says how big it is, what it may draw with, and
//! where its instructions are; the instructions are a small stack language —
//! move here, draw that, set this colour.
//!
//! # What matters about the result
//!
//! **It must be the same document.** The pages are laid out by the same engine
//! that draws them on screen, at the same resolution the paper is measured in,
//! so a line breaks in the PDF exactly where it breaks in the window. Nothing
//! is re-flowed by the reader: a PDF says where every glyph goes, and this says
//! where the layout put it.
//!
//! **The text must still be text.** A page could be written as a picture and
//! would look right, and then nobody could search it, copy a sentence out of
//! it, or read it aloud with a screen reader. So the glyphs go in as glyphs,
//! with the font they are drawn from carried along — cut down to the glyphs
//! actually used — and a table saying which character each glyph came from.
//! That table is what makes copying text out of the file give back what was
//! typed.
//!
//! # Example
//!
//! ```no_run
//! # use wp_layout::{Device, FontLibrary, LayoutEngine, Page};
//! # fn example(pages: &[Page], library: &FontLibrary) {
//! // The pages are laid out for paper — a point to the dot — and then written.
//! let bytes = wp_pdf::write(pages, library, "My document");
//! # }
//! // Where the pages come from:
//! # fn lay_out(library: &FontLibrary) {
//! let mut engine = LayoutEngine::for_device(library, Device::paper());
//! # }
//! ```

#![forbid(unsafe_code)]

mod content;
mod read;
mod subset;
mod writer;

pub use read::{looks_like_pdf, needs_password, open, open_with_password, Error as ReadError};

use std::collections::{BTreeMap, BTreeSet};

use wp_font::GlyphId;
use wp_layout::{FontLibrary, Page};

use writer::{number, text_string, Id, Writer};

/// Turns laid-out pages into the bytes of a PDF file.
///
/// The pages must have been laid out at 72 dots to the inch — a point to the
/// dot — because that is the unit a PDF measures in. [`wp_layout::Device`]'s
/// `paper` is that device.
#[must_use]
pub fn write(pages: &[Page], library: &FontLibrary, title: &str) -> Vec<u8> {
    let mut writer = Writer::new();
    let mut fonts = Fonts::gather(pages);
    fonts.embed(&mut writer, library);

    let pages_id = writer.reserve();
    let mut page_ids = Vec::with_capacity(pages.len());

    for page in pages {
        let drawing = content::of(page, &fonts.names, library);
        let contents = writer.add_stream("", drawing.stream.as_bytes());

        // Only the fonts and pictures this page uses, so that a reader opening
        // one page does not load the whole document's worth.
        let mut resources = String::from("<< /ProcSet [/PDF /Text /ImageC]");
        if !drawing.fonts.is_empty() {
            resources.push_str(" /Font <<");
            for name in &drawing.fonts {
                if let Some(id) = fonts.objects.get(name) {
                    resources.push_str(&format!(" /{name} {}", id.reference()));
                }
            }
            resources.push_str(" >>");
        }
        if !drawing.images.is_empty() {
            resources.push_str(" /XObject <<");
            for (name, picture) in &drawing.images {
                // A PDF keeps a picture's transparency as a grey picture of its
                // own, drawn over it as a mask, because a colour space says
                // nothing about how see-through a pixel is.
                let mask = picture.alpha.as_ref().map(|alpha| {
                    writer.add_stream(
                        &format!(
                            "/Type /XObject /Subtype /Image /Width {} /Height {} \
                             /ColorSpace /DeviceGray /BitsPerComponent 8",
                            picture.width, picture.height
                        ),
                        alpha,
                    )
                });
                let mut dictionary = format!(
                    "/Type /XObject /Subtype /Image /Width {} /Height {} \
                     /ColorSpace /DeviceRGB /BitsPerComponent 8",
                    picture.width, picture.height
                );
                if let Some(mask) = mask {
                    dictionary.push_str(&format!(" /SMask {}", mask.reference()));
                }
                let id = writer.add_stream(&dictionary, &picture.colours);
                resources.push_str(&format!(" /{name} {}", id.reference()));
            }
            resources.push_str(" >>");
        }
        if !drawing.fades.is_empty() {
            resources.push_str(" /ExtGState <<");
            for (name, alpha) in &drawing.fades {
                let value = number(f32::from(*alpha) / 255.0);
                resources
                    .push_str(&format!(" /{name} << /Type /ExtGState /ca {value} /CA {value} >>"));
            }
            resources.push_str(" >>");
        }
        resources.push_str(" >>");

        page_ids.push(writer.add(&format!(
            "<< /Type /Page /Parent {} /MediaBox [0 0 {} {}] /Resources {resources} /Contents {} >>",
            pages_id.reference(),
            number(page.width),
            number(page.height),
            contents.reference()
        )));
    }

    let kids: Vec<String> = page_ids.iter().map(|id| id.reference()).collect();
    writer.put(
        pages_id,
        &format!("<< /Type /Pages /Count {} /Kids [{}] >>", page_ids.len(), kids.join(" ")),
    );

    let catalogue = writer.add(&format!("<< /Type /Catalog /Pages {} >>", pages_id.reference()));
    let information = writer.add(&format!(
        "<< /Title {} /Producer (Word Processor) /Creator (Word Processor) >>",
        text_string(title)
    ));
    writer.finish(catalogue, information)
}

/// The fonts a document needs, and the glyphs of each that it uses.
#[derive(Debug, Default)]
struct Fonts {
    /// Which glyphs of each face are drawn anywhere in the document.
    used: BTreeMap<usize, BTreeSet<u16>>,
    /// The name each face goes by inside the file: `/F0`, `/F1`.
    names: BTreeMap<usize, String>,
    /// The object each of those names stands for.
    objects: BTreeMap<String, Id>,
}

impl Fonts {
    /// Walks the pages for every glyph that will be drawn.
    fn gather(pages: &[Page]) -> Self {
        let mut used: BTreeMap<usize, BTreeSet<u16>> = BTreeMap::new();
        for page in pages {
            let inside = page.shapes.iter().flat_map(|shape| shape.text.iter());
            for glyph in page.glyphs.iter().chain(inside) {
                if glyph.invisible {
                    continue;
                }
                used.entry(glyph.face).or_default().insert(glyph.glyph.0);
            }
        }

        let names =
            used.keys().enumerate().map(|(index, face)| (*face, format!("F{index}"))).collect();
        Self { used, names, objects: BTreeMap::new() }
    }

    /// Writes each face into the file, cut down to what is used.
    fn embed(&mut self, writer: &mut Writer, library: &FontLibrary) {
        for (face, used) in &self.used {
            let Some(name) = self.names.get(face) else { continue };
            let Some(entry) = library.face(*face) else { continue };
            let Some(font) = entry.font() else { continue };
            // A glyph drawn as layers is written as its layers, so the
            // glyphs of the layers have to be in the font the file carries.
            let mut glyphs = used.clone();
            if font.has_colour() {
                for glyph in used {
                    for layer in font.colour_layers(GlyphId(*glyph)).unwrap_or_default() {
                        glyphs.insert(layer.glyph.0);
                    }
                }
            }
            let glyphs = &glyphs;

            // A font whose outlines are PostScript ones is cut down by
            // following the programs its glyphs are drawn by, subroutines and
            // all — see [`wp_font::Font::cut_postscript`]. One that cannot be
            // cut that way goes in whole, which is larger and right: a reader
            // that gets half a CFF draws nothing.
            //
            // A CID-keyed one goes in as the bare table, which is what the
            // key says and what every reader finds its glyphs in by CID; an
            // OpenType file around it would have FreeType, and Poppler with
            // it, take the CID for the glyph's place and draw nothing. See
            // [`wp_font::CutDown::table`].
            let whole = font.has_postscript_outlines();
            let mut bare = false;
            let embedded = if whole {
                match font.cut_postscript(glyphs) {
                    Some(cut) if cut.cid_keyed => {
                        bare = true;
                        cut.table
                    }
                    Some(cut) => cut.font,
                    None => {
                        let Some(file) = entry.file() else { continue };
                        file.to_vec()
                    }
                }
            } else {
                let Some(subset) = subset::build(&font, glyphs) else { continue };
                subset
            };

            let units = f32::from(font.units_per_em().max(1));
            let scale = 1000.0 / units;
            let metrics = font.vertical_metrics();

            // A font stream says what it holds. The older kind says it by its
            // length; the newer one says it by name, and a reader that is not
            // told treats an OpenType file as bare PostScript and fails.
            let stream = if bare {
                "/Subtype /CIDFontType0C".to_owned()
            } else if whole {
                "/Subtype /OpenType".to_owned()
            } else {
                format!("/Length1 {}", embedded.len())
            };
            let file = writer.add_stream(&stream, &embedded);

            // A name a reader shows in its list of fonts. The six letters in
            // front of it are what the format asks for to say the font has been
            // cut down; they are made from the face rather than at random so
            // that writing the same document twice gives the same file.
            let family = font.family_name().unwrap_or_else(|| "Font".to_owned());
            let mut plain: String =
                family.chars().filter(|character| character.is_ascii_alphanumeric()).collect();
            // The style goes on the name the PostScript way, `-BoldItalic`,
            // which is how a reader that only has the name tells a bold
            // font from its regular one.
            let style = match (font.is_bold(), font.is_italic()) {
                (true, true) => "-BoldItalic",
                (true, false) => "-Bold",
                (false, true) => "-Italic",
                (false, false) => "",
            };
            plain.push_str(style);
            let tag = subset_tag(*face, &plain);
            let bounds = font_bounds(&font, scale);

            let descriptor = writer.add(&format!(
                "<< /Type /FontDescriptor /FontName /{tag}+{plain} /Flags {flags} \
                 /FontBBox [{bounds}] /ItalicAngle {angle} /Ascent {ascent} /Descent {descent} \
                 /CapHeight {cap} /StemV {stem} /FontWeight {weight} /{key} {file} >>",
                // Which key the font goes under says what kind it is, and a
                // reader believes the key rather than looking.
                key = if whole { "FontFile3" } else { "FontFile2" },
                // Symbolic (4), italic (64) and, for a bold face, the force-bold
                // bit (1 << 18): what the flags can say of the style.
                flags = 4
                    | if font.is_italic() { 64 } else { 0 }
                    | if font.is_bold() { 1 << 18 } else { 0 },
                angle = if font.is_italic() { -12 } else { 0 },
                stem = if font.is_bold() { 140 } else { 80 },
                weight = font.weight().clamp(100, 900),
                ascent = (f32::from(metrics.ascender) * scale).round() as i32,
                descent = (f32::from(metrics.descender) * scale).round() as i32,
                cap = (f32::from(metrics.ascender) * scale * 0.7).round() as i32,
                file = file.reference(),
            ));

            let widths = self.widths(&font, glyphs, scale);
            let descendant = writer.add(&format!(
                "<< /Type /Font /Subtype /{kind} /BaseFont /{tag}+{plain} \
                 /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                 /FontDescriptor {descriptor} {mapping}/DW 1000 /W [{widths}] >>",
                // Two kinds of outline are two kinds of descendant font, and
                // only the one built on a glyph table carries a map from the
                // number in the text to the glyph: in the other they are the
                // same number already.
                kind = if whole { "CIDFontType0" } else { "CIDFontType2" },
                mapping = if whole { "" } else { "/CIDToGIDMap /Identity " },
                descriptor = descriptor.reference(),
            ));

            let to_unicode = writer.add_stream("", unicode_map(&font, glyphs).as_bytes());
            let id = writer.add(&format!(
                "<< /Type /Font /Subtype /Type0 /BaseFont /{tag}+{plain} /Encoding /Identity-H \
                 /DescendantFonts [{descendant}] /ToUnicode {unicode} >>",
                descendant = descendant.reference(),
                unicode = to_unicode.reference(),
            ));
            self.objects.insert(name.clone(), id);
        }
    }

    /// How wide each glyph is, in the thousandths of an em a PDF measures in.
    fn widths(&self, font: &wp_font::Font<'_>, glyphs: &BTreeSet<u16>, scale: f32) -> String {
        let mut out = String::new();
        for glyph in glyphs {
            let width = (f32::from(font.advance(GlyphId(*glyph))) * scale).round() as i32;
            out.push_str(&format!("{glyph} [{width}] "));
        }
        out.trim_end().to_owned()
    }
}

/// The six letters that say a font has been cut down.
///
/// The format wants six capitals, and wants two subsets of the same font to
/// have different ones. Working them out from the face rather than at random
/// means the same document written twice comes out as the same bytes.
fn subset_tag(face: usize, family: &str) -> String {
    let mut hash = 0x811C_9DC5u32;
    for byte in family.bytes().chain(face.to_le_bytes()) {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    (0..6)
        .map(|step| {
            let letter = (hash >> (step * 5)) % 26;
            char::from(b'A' + letter as u8)
        })
        .collect()
}

/// The box every glyph of the font fits inside, which a reader uses to decide
/// how much of the page a line of text can touch.
fn font_bounds(font: &wp_font::Font<'_>, scale: f32) -> String {
    let Some(head) = font.table(b"head") else { return String::from("0 -200 1000 900") };
    if head.len() < 44 {
        return String::from("0 -200 1000 900");
    }
    let read = |at: usize| {
        let value = i16::from_be_bytes([head[at], head[at + 1]]);
        (f32::from(value) * scale).round() as i32
    };
    format!("{} {} {} {}", read(36), read(38), read(40), read(42))
}

/// The table that says which character each glyph stands for.
///
/// Without it a reader can draw the text and nothing else: copying a sentence
/// out gives gibberish, and searching finds nothing. It is written as a CMap,
/// which is a small program in a language of its own — this is the shape every
/// PDF writer emits.
fn unicode_map(font: &wp_font::Font<'_>, glyphs: &BTreeSet<u16>) -> String {
    // A glyph several characters are drawn with is read back as the one a
    // person writes. A font for Chinese draws the Kangxi radical for "text"
    // with the glyph of the ideograph for it, and the radical comes first in
    // the character map — so text copied out of the page came back as a
    // character nobody typed.
    let mut back: BTreeMap<u16, char> = BTreeMap::new();
    for (character, glyph) in font.character_map().pairs() {
        match back.entry(glyph.0) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(character);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if stands_for_another(*entry.get()) && !stands_for_another(character) {
                    entry.insert(character);
                }
            }
        }
    }

    let mut pairs = String::new();
    let mut count = 0usize;
    for glyph in glyphs {
        let Some(character) = back.get(glyph) else { continue };
        let mut encoded = String::new();
        let mut buffer = [0u16; 2];
        for unit in character.encode_utf16(&mut buffer) {
            encoded.push_str(&format!("{unit:04X}"));
        }
        pairs.push_str(&format!("<{glyph:04X}> <{encoded}>\n"));
        count += 1;
    }

    format!(
        "/CIDInit /ProcSet findresource begin\n\
         12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
         {count} beginbfchar\n{pairs}endbfchar\n\
         endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend"
    )
}

/// Whether a character is one of those that stand for another and are drawn
/// with its glyph: the radicals, which are the ideographs they are named
/// after, and the compatibility ideographs and forms, kept only so that text
/// in an older encoding could be carried over and come back unchanged.
fn stands_for_another(character: char) -> bool {
    matches!(
        character as u32,
        0x2E80..=0x2FDF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0x2F800..=0x2FA1F
    )
}

#[cfg(test)]
mod tests {
    use super::subset_tag;

    #[test]
    fn a_subset_tag_is_six_capitals() {
        let tag = subset_tag(3, "Calibri");
        assert_eq!(tag.len(), 6);
        assert!(tag.chars().all(|character| character.is_ascii_uppercase()));
    }

    #[test]
    fn the_same_font_gives_the_same_tag_and_a_different_one_a_different_tag() {
        assert_eq!(subset_tag(3, "Calibri"), subset_tag(3, "Calibri"));
        assert_ne!(subset_tag(3, "Calibri"), subset_tag(4, "Calibri"));
        assert_ne!(subset_tag(3, "Calibri"), subset_tag(3, "Cambria"));
    }
}

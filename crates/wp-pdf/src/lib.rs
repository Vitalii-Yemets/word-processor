//! Writing a laid-out document out as a PDF.
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
mod subset;
mod writer;

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
        let drawing = content::of(page, &fonts.names);
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
        for (face, glyphs) in &self.used {
            let Some(name) = self.names.get(face) else { continue };
            let Some(entry) = library.face(*face) else { continue };
            let Some(font) = entry.font() else { continue };
            let Some(subset) = subset::build(&font, glyphs) else { continue };

            let units = f32::from(font.units_per_em().max(1));
            let scale = 1000.0 / units;
            let metrics = font.vertical_metrics();

            let file = writer.add_stream(&format!("/Length1 {}", subset.len()), &subset);

            // A name a reader shows in its list of fonts. The six letters in
            // front of it are what the format asks for to say the font has been
            // cut down; they are made from the face rather than at random so
            // that writing the same document twice gives the same file.
            let family = font.family_name().unwrap_or_else(|| "Font".to_owned());
            let plain: String =
                family.chars().filter(|character| character.is_ascii_alphanumeric()).collect();
            let tag = subset_tag(*face, &plain);
            let bounds = font_bounds(&font, scale);

            let descriptor = writer.add(&format!(
                "<< /Type /FontDescriptor /FontName /{tag}+{plain} /Flags {flags} \
                 /FontBBox [{bounds}] /ItalicAngle {angle} /Ascent {ascent} /Descent {descent} \
                 /CapHeight {cap} /StemV 80 /FontFile2 {file} >>",
                flags = if font.is_italic() { 68 } else { 4 },
                angle = if font.is_italic() { -12 } else { 0 },
                ascent = (f32::from(metrics.ascender) * scale).round() as i32,
                descent = (f32::from(metrics.descender) * scale).round() as i32,
                cap = (f32::from(metrics.ascender) * scale * 0.7).round() as i32,
                file = file.reference(),
            ));

            let widths = self.widths(&font, glyphs, scale);
            let descendant = writer.add(&format!(
                "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{tag}+{plain} \
                 /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                 /FontDescriptor {descriptor} /CIDToGIDMap /Identity /DW 1000 /W [{widths}] >>",
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
    let mut back: BTreeMap<u16, char> = BTreeMap::new();
    for (character, glyph) in font.character_map().pairs() {
        back.entry(glyph.0).or_insert(character);
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

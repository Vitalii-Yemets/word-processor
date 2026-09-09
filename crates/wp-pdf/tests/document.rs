//! A document written out as a PDF, and read back in again.
//!
//! There is no PDF reader in this container to check the file against, so the
//! test is its own reader: it finds the streams, decompresses them, and reads
//! the text out of the instructions the way a reader would — through the table
//! that says which character each glyph stands for. If that comes back as what
//! was typed, then the file is a document rather than a picture of one, which
//! is the whole point of writing it this way.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::Document;
use wp_layout::{Device, FontLibrary, LayoutEngine, Page};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A document of the given paragraphs, laid out for paper.
fn laid_out(lines: &[&str]) -> Vec<Page> {
    let mut body = Body::default();
    for line in lines {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");
    let mut engine = LayoutEngine::for_device(library(), Device::paper());
    engine.layout_document(&document)
}

/// Where a run of bytes begins, from an offset.
fn find_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack[from..].windows(needle.len()).position(|window| window == needle).map(|at| at + from)
}

/// Every stream in the file, decompressed.
fn streams(pdf: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = find_from(pdf, b"stream\n", at) {
        let from = start + b"stream\n".len();
        let Some(end) = find_from(pdf, b"\nendstream", from) else { break };
        if let Ok(data) = wp_deflate::inflate_zlib(&pdf[from..end], 64 * 1024 * 1024) {
            out.push(data);
        }
        // Past the whole marker: "endstream" has "stream" inside it, and
        // stopping one byte on would find that instead of the next stream.
        at = end
            + b"
endstream"
                .len();
    }
    out
}

/// The table in a `ToUnicode` map, as glyph number to text.
fn character_table(pdf: &[u8]) -> std::collections::BTreeMap<u16, String> {
    let mut table = std::collections::BTreeMap::new();
    for stream in streams(pdf) {
        let text = String::from_utf8_lossy(&stream).into_owned();
        if !text.contains("beginbfchar") {
            continue;
        }
        for line in text.lines() {
            let line = line.trim();
            let Some((left, right)) = line.split_once("> <") else { continue };
            let Some(glyph) = left.strip_prefix('<') else { continue };
            let Some(characters) = right.strip_suffix('>') else { continue };
            let Ok(glyph) = u16::from_str_radix(glyph, 16) else { continue };

            let units: Vec<u16> = characters
                .as_bytes()
                .chunks(4)
                .filter_map(|chunk| u16::from_str_radix(&String::from_utf8_lossy(chunk), 16).ok())
                .collect();
            table.insert(glyph, String::from_utf16_lossy(&units));
        }
    }
    table
}

/// The text of the file, read the way a reader copying it out would: the glyph
/// numbers in the instructions, put through the table.
fn text_of(pdf: &[u8]) -> String {
    let table = character_table(pdf);
    let mut out = String::new();

    for stream in streams(pdf) {
        let text = String::from_utf8_lossy(&stream).into_owned();
        if !text.contains(" TJ") {
            continue;
        }
        // Every `<....>` in the instructions is a run of glyph numbers.
        let mut rest = text.as_str();
        while let Some(start) = rest.find('<') {
            let Some(end) = rest[start..].find('>') else { break };
            let hex = &rest[start + 1..start + end];
            for chunk in hex.as_bytes().chunks(4) {
                if let Ok(glyph) = u16::from_str_radix(&String::from_utf8_lossy(chunk), 16) {
                    out.push_str(table.get(&glyph).map_or("", String::as_str));
                }
            }
            rest = &rest[start + end + 1..];
        }
    }
    out
}

#[test]
fn a_pdf_begins_and_ends_the_way_the_format_says() {
    let pdf = wp_pdf::write(&laid_out(&["Hello"]), library(), "A document");
    assert!(pdf.starts_with(b"%PDF-1.7"), "it does not say what it is");
    assert!(pdf.ends_with(b"%%EOF\n"), "it does not say where it ends");
    assert!(pdf.len() > 1000, "a document with a font in it is not this small");
}

#[test]
fn there_is_a_page_for_every_page() {
    let long = "A paragraph that takes up room. ".repeat(200);
    let pages = laid_out(&[&long]);
    let pdf = wp_pdf::write(&pages, library(), "Several pages");

    let text = String::from_utf8_lossy(&pdf).into_owned();
    let count = text.matches("/Type /Page ").count();
    assert!(pages.len() > 1, "the document should run to several pages");
    assert_eq!(count, pages.len(), "the file has a different number of pages");
    assert!(text.contains(&format!("/Count {}", pages.len())));
}

#[test]
fn the_text_can_be_read_back_out_of_the_file() {
    // The one thing that separates a PDF from a picture of a page.
    let pdf = wp_pdf::write(&laid_out(&["The quick brown fox"]), library(), "Readable");
    let read = text_of(&pdf);
    assert!(read.contains("quick"), "the text did not survive: {read:?}");
    assert!(read.contains("brown fox"), "the words came back apart: {read:?}");
}

#[test]
fn the_text_survives_in_every_alphabet() {
    let lines = ["Съешь ещё этих мягких булок", "Ταχίστη αλώπηξ βαφής", "Voix ambiguë du cœur"];
    let pdf = wp_pdf::write(&laid_out(&lines), library(), "Multilingual");
    let read = text_of(&pdf);

    for line in lines {
        // Word by word: the words are what a reader searches for, and the
        // spaces between them are drawn as glyphs of their own.
        for word in line.split_whitespace() {
            assert!(read.contains(word), "{word:?} did not survive: {read:?}");
        }
    }
}

#[test]
fn the_font_that_is_carried_is_cut_down_to_what_is_used() {
    let short = wp_pdf::write(&laid_out(&["A"]), library(), "One letter");
    let long = wp_pdf::write(
        &laid_out(&["Every letter of the alphabet, and then some: 0123456789"]),
        library(),
        "Many letters",
    );

    // Both carry a font; the one that uses more of it carries more of it.
    assert!(short.windows(9).any(|window| window == b"/FontFile"));
    assert!(long.len() > short.len(), "the font was not cut down at all");
}

#[test]
fn a_pdf_of_nothing_is_still_a_pdf() {
    let pdf = wp_pdf::write(&[], library(), "Empty");
    assert!(pdf.starts_with(b"%PDF"));
    assert!(pdf.ends_with(b"%%EOF\n"));
    let text = String::from_utf8_lossy(&pdf).into_owned();
    assert!(text.contains("/Count 0"));
}

#[test]
fn the_same_document_is_written_the_same_way_twice() {
    // Nothing random and nothing from the clock: two runs give one file, which
    // is what makes a difference between two files mean something.
    let first = wp_pdf::write(&laid_out(&["Repeatable"]), library(), "Same");
    let second = wp_pdf::write(&laid_out(&["Repeatable"]), library(), "Same");
    assert_eq!(first, second);
}

#[test]
fn the_font_in_the_file_is_still_a_font() {
    // The riskiest thing here is the cutting down: a font a reader cannot parse
    // means a page of nothing, or a file it refuses altogether. So the font
    // that comes out is read back with the same parser that reads the ones on
    // the machine, and asked for a letter that was used.
    let pdf = wp_pdf::write(&laid_out(&["Hamburgefonstiv"]), library(), "A font");

    // The font file is the largest stream: the others are a page of
    // instructions and a table of characters.
    let embedded = streams(&pdf).into_iter().max_by_key(Vec::len).expect("a font in the file");
    let font = wp_font::Font::parse(&embedded).expect("a font that parses");

    assert!(font.glyph_count() > 0, "a font with no glyphs");
    assert!(font.units_per_em() > 0);

    // Every glyph the page draws has to be in it, with its outline.
    let table = character_table(&pdf);
    let drawn: Vec<u16> = table.keys().copied().collect();
    assert!(drawn.len() > 5, "the page draws more than five different letters");

    let mut with_outlines = 0;
    for glyph in drawn {
        if font.outline(wp_font::GlyphId(glyph)).ok().flatten().is_some() {
            with_outlines += 1;
        }
    }
    // Every one but the space, which has no outline in any font.
    assert!(with_outlines >= table.len() - 1, "{with_outlines} of {} were drawable", table.len());
}

#[test]
fn what_the_document_does_not_use_is_left_out_of_the_font() {
    let pdf = wp_pdf::write(&laid_out(&["A"]), library(), "One letter");
    let embedded = streams(&pdf).into_iter().max_by_key(Vec::len).expect("a font");
    let font = wp_font::Font::parse(&embedded).expect("a font that parses");

    // The glyphs are still numbered as they were — that is what lets the page
    // refer to them — but only the handful the document uses have any outline
    // left in them.
    let drawable = (0..font.glyph_count())
        .filter(|glyph| font.outline(wp_font::GlyphId(*glyph)).ok().flatten().is_some())
        .count();
    assert!(font.glyph_count() > 100, "a text font has hundreds of glyphs");
    assert!(drawable < 10, "{drawable} glyphs are still being carried for one letter");
}

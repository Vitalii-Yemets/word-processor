//! Fonts that name their characters in ways of their own: a Type 3 font
//! drawn in pixels with its glyphs numbered, and a CJK font through one of
//! the predefined Unicode CMaps with no table of its own.

use wp_docx::model::{Block, RunContent};
use wp_docx::Document;

/// A PDF of one page from its content and its fonts' objects, numbered
/// from 5.
fn page(content: &str, fonts: &str, objects: &[String]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.5\n".to_vec();
    pdf.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
    pdf.extend_from_slice(b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
    pdf.extend_from_slice(
        format!("3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << {fonts} >> >> /Contents 4 0 R >> endobj\n")
            .as_bytes(),
    );
    pdf.extend_from_slice(
        format!("4 0 obj << /Length {} >> stream\n{content}\nendstream endobj\n", content.len())
            .as_bytes(),
    );
    for (index, object) in objects.iter().enumerate() {
        pdf.extend_from_slice(format!("{} 0 obj {object} endobj\n", index + 5).as_bytes());
    }
    pdf.extend_from_slice(b"trailer << /Root 1 0 R >>\n%%EOF\n");
    pdf
}

fn paragraphs(document: &Document) -> Vec<String> {
    document.body().blocks.iter().map(Block::plain_text).collect()
}

/// The size of the run that holds some text, in half points.
fn size_of(document: &Document, text: &str) -> Option<u32> {
    for block in &document.body().blocks {
        let Block::Paragraph(paragraph) = block else { continue };
        for run in &paragraph.runs {
            let holds =
                run.content.iter().any(|c| matches!(c, RunContent::Text(t) if t.contains(text)));
            if holds {
                return run.properties.size_half_points;
            }
        }
    }
    None
}

#[test]
fn a_type_3_font_drawn_in_pixels_is_the_size_it_looks_and_says_its_characters() {
    // Glyphs forty pixels tall, a matrix of one, shown at a quarter: ten
    // points, like the Helvetica line above it. The glyphs are named by
    // their codes in decimal, as a program that makes bitmap fonts names
    // them.
    let glyph = |width: u32| {
        let body = format!("{width} 0 0 -32 {width} 8 d1 0 0 {width} 30 re f");
        format!("<< /Length {} >> stream\n{body}\nendstream", body.len())
    };
    let objects = vec![
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /Font /Subtype /Type3 /FontMatrix [1 0 0 -1 0 0] /FontBBox [0 -32 40 8] \
         /FirstChar 72 /LastChar 105 /Widths 7 0 R /Encoding << /Type /Encoding /Differences [72 /a72 105 /a105] >> \
         /CharProcs << /a72 8 0 R /a105 9 0 R >> /Resources << >> >>"
            .to_owned(),
        {
            let mut widths = vec!["0".to_owned(); 34];
            widths[0] = "30".to_owned();
            widths[33] = "12".to_owned();
            format!("[{}]", widths.join(" "))
        },
        glyph(30),
        glyph(12),
    ];
    let content = "BT /F1 10 Tf 72 700 Td (Body text here, and more of it.) Tj ET \
                   BT /F2 0.25 Tf 72 600 Td (Hi) Tj ET";
    let pdf = page(content, "/F1 5 0 R /F2 6 0 R", &objects);
    let document = wp_pdf::open(&pdf).expect("opened");
    let all = paragraphs(&document);
    assert!(all.iter().any(|p| p.trim() == "Hi"), "{all:?}");
    assert_eq!(size_of(&document, "Hi"), Some(20), "{all:?}");
    assert_eq!(size_of(&document, "Body"), Some(20));
}

#[test]
fn a_cjk_font_through_a_unicode_cmap_says_its_characters() {
    let objects = vec![
        "<< /Type /Font /Subtype /Type0 /BaseFont /STSong-Light /Encoding /UniGB-UCS2-H /DescendantFonts [6 0 R] >>"
            .to_owned(),
        "<< /Type /Font /Subtype /CIDFontType0 /BaseFont /STSong-Light \
         /CIDSystemInfo << /Registry (Adobe) /Ordering (GB1) /Supplement 4 >> /DW 1000 /W [34 [500]] /FontDescriptor 7 0 R >>"
            .to_owned(),
        "<< /Type /FontDescriptor /FontName /STSong-Light /Flags 6 /FontBBox [-25 -254 1000 880] /ItalicAngle 0 \
         /Ascent 880 /Descent -120 /CapHeight 880 /StemV 93 >>"
            .to_owned(),
    ];
    // "中文" and then "A", whose width the collection's Latin gives.
    let content = "BT /F1 12 Tf 72 700 Td <4E2D65870041> Tj ET";
    let pdf = page(content, "/F1 5 0 R", &objects);
    let document = wp_pdf::open(&pdf).expect("opened");
    assert_eq!(paragraphs(&document), vec!["\u{4E2D}\u{6587}A".to_owned()]);
}

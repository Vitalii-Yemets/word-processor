//! Web pages: reading the HTML Word and everything else writes, MHT with
//! it, and writing both back.
//!
//! # Why a word processor reads web pages
//!
//! Because Word does, and because half of what gets pasted into a document
//! came off a page. Word's own "Web Page" is a page and a folder of its
//! pictures; its "Single File Web Page" is the same in one MIME file; and
//! what Word writes is the mess this reads — see [`read`] for what a page
//! from Word looks like.
//!
//! # What is here
//!
//! [`read`] turns a page into the document model; [`open_html`] and
//! [`open_mht`] make documents of a page and of a single-file page, with
//! the pictures fetched from beside the page or from inside the file;
//! [`write`] goes the other way, and [`write_mht`] wraps the result as one
//! file. The page's bytes are read in whatever encoding its `<meta>` names,
//! through [`wp_text`].

#![forbid(unsafe_code)]

pub mod css;
pub mod mime;
mod read;
mod tokens;
mod write;

use std::path::Path;

pub use read::{read, LinkFound, PictureFound, Reading, PICTURE_MARK};
pub use write::{escape, write, Page, PictureFile};

use wp_docx::{Document, Error, TextPosition};
use wp_text::Encoding;

/// The text of a page's bytes, in the encoding the page names or the one
/// its bytes betray.
///
/// A mark at the start settles it; so does UTF-16. Otherwise the `<meta>`
/// that names a charset is believed — Word writes `charset=windows-1252`
/// on every page — and failing that, what the bytes look like.
#[must_use]
pub fn decode_page(bytes: &[u8]) -> String {
    let detected = wp_text::detect(bytes, Encoding::CodePage(1252));
    if detected.sure && !matches!(detected.encoding, Encoding::CodePage(_)) {
        return detected.encoding.decode(bytes);
    }
    if let Some(named) = declared_charset(bytes).and_then(|name| Encoding::named(&name)) {
        return named.decode(bytes);
    }
    detected.encoding.decode(bytes)
}

/// The charset the page's head names, if it names one.
fn declared_charset(bytes: &[u8]) -> Option<String> {
    let head = &bytes[..bytes.len().min(8192)];
    let text: String = head.iter().map(|byte| (*byte as char).to_ascii_lowercase()).collect();
    let at = text.find("charset=")?;
    let rest = &text[at + "charset=".len()..];
    let name: String = rest
        .trim_start_matches(['"', '\''])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Opens a page as a document, fetching its pictures from beside it: a
/// `src` that is a path is read relative to the page's own folder, and a
/// `data:` URI holds its bytes itself.
pub fn open_html(bytes: &[u8], page_path: Option<&Path>) -> Result<Document, Error> {
    let reading = read(&decode_page(bytes));
    let folder = page_path.and_then(Path::parent);
    assemble(reading, |source| {
        if let Some(inline) = data_uri(source) {
            return Some(inline);
        }
        let folder = folder?;
        let relative = source.strip_prefix("file:///").unwrap_or(source).replace("%20", " ");
        if relative.contains("://") {
            return None;
        }
        std::fs::read(folder.join(relative)).ok()
    })
}

/// Opens a single-file page as a document: the page is the part that is
/// HTML, and its pictures are the parts whose locations its `src`s name.
pub fn open_mht(bytes: &[u8]) -> Result<Document, Error> {
    let parts = mime::parts(bytes);
    let Some(page) = parts.iter().find(|part| part.content_type == "text/html") else {
        // No page among the parts: an empty document, which is what a
        // message with no page in it holds.
        return Document::create(&wp_docx::model::Body::default());
    };
    let text = match page.charset.as_deref().and_then(Encoding::named) {
        Some(encoding) => encoding.decode(&page.bytes),
        None => decode_page(&page.bytes),
    };
    let reading = read(&text);
    let base = page.location.clone().unwrap_or_default();
    assemble(reading, |source| {
        if let Some(inline) = data_uri(source) {
            return Some(inline);
        }
        // The part whose location is the source, resolved against the
        // page's own location the way a browser resolves it — or just
        // ending in the same file name, which is what survives being saved
        // somewhere else.
        let wanted = resolve(&base, source);
        parts
            .iter()
            .find(|part| part.location.as_deref() == Some(wanted.as_str()))
            .or_else(|| {
                let name = source.rsplit('/').next().unwrap_or(source);
                parts.iter().find(|part| {
                    part.location.as_deref().is_some_and(|location| location.ends_with(name))
                })
            })
            .map(|part| part.bytes.clone())
    })
}

/// Writes the document as a single-file page: the page and its pictures as
/// the parts of one MIME message, which is Word's "Single File Web Page".
#[must_use]
pub fn write_mht(document: &Document, name: &str, title: Option<&str>) -> Vec<u8> {
    let page = write(document, name, title);
    let boundary = "----=_NextPart_000_0000_01D00000.00000000";
    let base = format!("file:///C:/{name}");
    let mut out = String::new();
    out.push_str("MIME-Version: 1.0\r\n");
    out.push_str(&format!(
        "Content-Type: multipart/related;\r\n\tboundary=\"{boundary}\";\r\n\ttype=\"text/html\"\r\n"
    ));
    out.push_str("X-MimeOLE: Produced By Word Processor\r\n\r\n");
    out.push_str("This is a multi-part message in MIME format.\r\n\r\n");
    out.push_str(&format!("--{boundary}\r\n"));
    out.push_str(&format!("Content-Location: {base}\r\n"));
    out.push_str("Content-Transfer-Encoding: quoted-printable\r\n");
    out.push_str("Content-Type: text/html; charset=\"utf-8\"\r\n\r\n");
    out.push_str(&mime::encode_quoted_printable(page.html.as_bytes()));
    out.push_str("\r\n\r\n");
    for picture in &page.pictures {
        out.push_str(&format!("--{boundary}\r\n"));
        out.push_str(&format!("Content-Location: file:///C:/{}\r\n", picture.name));
        out.push_str("Content-Transfer-Encoding: base64\r\n");
        out.push_str(&format!("Content-Type: {}\r\n\r\n", picture.content_type));
        out.push_str(&mime::encode_base64(&picture.bytes));
        out.push_str("\r\n");
    }
    out.push_str(&format!("--{boundary}--\r\n"));
    out.into_bytes()
}

/// A document from what was read, with the pictures fetched and put in
/// where their marks were and the links laid over their text.
fn assemble(reading: Reading, fetch: impl Fn(&str) -> Option<Vec<u8>>) -> Result<Document, Error> {
    let mut document = Document::create(&reading.body)?;

    // The pictures, last first, so that putting one in does not move the
    // marks of the ones after it in the same paragraph.
    let mut pictures = reading.pictures;
    pictures.sort_by_key(|one| std::cmp::Reverse((one.paragraph, one.offset)));
    let mut links = reading.links;
    for picture in pictures {
        let start = TextPosition::new(picture.paragraph, picture.offset);
        let end = TextPosition::new(picture.paragraph, picture.offset + PICTURE_MARK.len_utf8());
        let before = document.paragraph_text(picture.paragraph).map_or(0, |text| text.len());
        document.set_caret(start);
        document.extend_selection_to(end);
        document.delete_selection();
        document.set_caret(start);
        if let Some(bytes) = fetch(&picture.source) {
            let extension = match wp_image::Format::detect(&bytes) {
                Some(wp_image::Format::Png) => "png",
                Some(wp_image::Format::Jpeg) => "jpeg",
                Some(wp_image::Format::Gif) => "gif",
                Some(wp_image::Format::Bmp) => "bmp",
                Some(wp_image::Format::Tiff) => "tiff",
                Some(wp_image::Format::Emf) => "emf",
                Some(wp_image::Format::Wmf) => "wmf",
                None => "bin",
            };
            // The size the page asked for, or the picture's own at
            // ninety-six to the inch.
            let (width, height) = match (picture.width_emu, picture.height_emu) {
                (Some(width), Some(height)) => (width, height),
                _ => wp_image::decode(&bytes)
                    .map(|image| (image.width as i64 * 9525, image.height as i64 * 9525))
                    .unwrap_or((96 * 9525, 96 * 9525)),
            };
            let _ = document.insert_picture(&bytes, extension, width, height);
        }
        // A picture is not the width of its mark in the text — and one that
        // could not be fetched is nothing — so the links after it move.
        let after = document.paragraph_text(picture.paragraph).map_or(0, |text| text.len());
        for link in &mut links {
            if link.paragraph == picture.paragraph && link.start >= picture.offset {
                link.start = (link.start + after).saturating_sub(before);
                link.end = (link.end + after).saturating_sub(before);
            }
        }
    }

    for link in links {
        if link.end <= link.start {
            continue;
        }
        document.set_caret(TextPosition::new(link.paragraph, link.start));
        document.extend_selection_to(TextPosition::new(link.paragraph, link.end));
        document.add_hyperlink(&link.address, "");
    }
    if let Some(title) = reading.title {
        let mut properties = document.properties();
        properties.title = title;
        let _ = document.set_properties(&properties);
    }

    document.set_caret(TextPosition::default());
    document.clear_selection();
    let _ = document.mark_saved();
    Ok(document)
}

/// The bytes a `data:` URI holds, if the source is one.
fn data_uri(source: &str) -> Option<Vec<u8>> {
    let rest = source.strip_prefix("data:")?;
    let (header, data) = rest.split_once(',')?;
    Some(if header.ends_with(";base64") {
        mime::decode_base64(data.as_bytes())
    } else {
        percent_decode(data)
    })
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&text[at + 1..at + 3], 16) {
                out.push(value);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    out
}

/// A source resolved against the page's location: `doc_files/image001.png`
/// beside `file:///C:/doc.htm` is `file:///C:/doc_files/image001.png`.
fn resolve(base: &str, source: &str) -> String {
    if source.contains("://") || base.is_empty() {
        return source.to_owned();
    }
    let folder = base.rsplit_once('/').map_or("", |(folder, _)| folder);
    format!("{folder}/{source}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Alignment, Block, Body, Paragraph, RunContent, Underline};

    #[test]
    fn a_plain_page_reads_to_its_paragraphs() {
        let reading = read("<html><head><title>Hello</title></head><body><h1>A heading</h1><p>One\n  two</p><p>Three &amp; four</p></body></html>");
        assert_eq!(reading.body.plain_text(), "A heading\nOne two\nThree & four");
        assert_eq!(reading.title.as_deref(), Some("Hello"));
        let Block::Paragraph(heading) = &reading.body.blocks[0] else { panic!() };
        assert_eq!(heading.properties.style.as_deref(), Some("Heading1"));
    }

    #[test]
    fn formatting_comes_from_tags_styles_and_the_sheet() {
        let reading = read(
            "<html><head><style>p.Big {font-size:14.0pt; text-align:center} .red {color:red}</style></head>\
             <body><p class=Big>Plain <b>bold <i>both</i></b> <span class=red style='font-family:\"Times New Roman\",serif;text-decoration:underline'>styled</span></p></body></html>",
        );
        let Block::Paragraph(paragraph) = &reading.body.blocks[0] else { panic!() };
        assert_eq!(paragraph.properties.alignment, Some(Alignment::Center));
        assert_eq!(paragraph.runs.len(), 5, "{:?}", paragraph.runs);
        assert_eq!(paragraph.runs[0].plain_text(), "Plain ");
        assert_eq!(paragraph.runs[0].properties.size_half_points, Some(28));
        assert_eq!(paragraph.runs[1].properties.bold, Some(true));
        assert_eq!(paragraph.runs[2].properties.italic, Some(true));
        assert_eq!(paragraph.runs[2].properties.bold, Some(true));
        assert_eq!(paragraph.runs[2].plain_text(), "both");
        let styled = &paragraph.runs[4];
        assert_eq!(styled.properties.color.as_deref(), Some("FF0000"));
        assert_eq!(styled.properties.font.as_deref(), Some("Times New Roman"));
        assert_eq!(styled.properties.underline, Some(Underline::Single));
    }

    #[test]
    fn lists_and_tables_are_read() {
        let reading = read(
            "<body><ul><li>Milk</li><li>Bread</li></ul><ol><li>First</li></ol>\
             <table><tr><td width=100>A</td><td style='width:150.0pt'>B</td></tr><tr><td>C</td><td>D</td></tr></table><p>After</p></body>",
        );
        let Block::Paragraph(milk) = &reading.body.blocks[0] else { panic!() };
        assert_eq!(milk.properties.numbering.map(|n| n.id), Some(wp_docx::BULLET_LIST));
        let Block::Paragraph(first) = &reading.body.blocks[2] else { panic!() };
        assert_eq!(first.properties.numbering.map(|n| n.id), Some(wp_docx::NUMBERED_LIST));
        let Block::Table(table) = &reading.body.blocks[3] else {
            panic!("{:?}", reading.body.blocks)
        };
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].cells[1].blocks[0].plain_text(), "B");
        assert_eq!(table.grid, vec![1500, 3000]);
        assert_eq!(reading.body.blocks[4].plain_text(), "After");
    }

    #[test]
    fn a_page_from_word_reads_past_its_mess() {
        let page = "<html xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:w=\"urn:schemas-microsoft-com:office:word\" xmlns=\"http://www.w3.org/TR/REC-html40\">\n\
<head>\n<meta http-equiv=Content-Type content=\"text/html; charset=windows-1252\">\n<meta name=Generator content=\"Microsoft Word 15 (filtered)\">\n\
<!--[if gte mso 9]><xml>\n <o:OfficeDocumentSettings>\n  <o:AllowPNG/>\n </o:OfficeDocumentSettings>\n</xml><![endif]-->\n\
<style>\n<!--\n /* Font Definitions */\n @font-face\n\t{font-family:\"Cambria Math\";\n\tpanose-1:2 4 5 3 5 4 6 3 2 4;}\n\
 /* Style Definitions */\n p.MsoNormal, li.MsoNormal, div.MsoNormal\n\t{margin-top:0in;\n\tmargin-right:0in;\n\tmargin-bottom:8.0pt;\n\tmargin-left:0in;\n\tline-height:107%;\n\tfont-size:11.0pt;\n\tfont-family:\"Calibri\",sans-serif;}\n\
h1\n\t{mso-style-link:\"Heading 1 Char\";\n\tmargin-top:12.0pt;\n\tmargin-right:0in;\n\tmargin-bottom:0in;\n\tmargin-left:0in;\n\tline-height:107%;\n\tfont-size:16.0pt;\n\tfont-family:\"Calibri Light\",sans-serif;\n\tcolor:#2F5496;\n\tfont-weight:normal;}\n\
p.MsoListParagraph, li.MsoListParagraph, div.MsoListParagraph\n\t{margin-top:0in;\n\tmargin-right:0in;\n\tmargin-bottom:8.0pt;\n\tmargin-left:.5in;\n\tline-height:107%;\n\tfont-size:11.0pt;\n\tfont-family:\"Calibri\",sans-serif;}\n\
 /* List Definitions */\n @list l0\n\t{mso-list-id:1234;\n\tmso-list-type:hybrid;}\n @list l0:level1\n\t{mso-level-number-format:bullet;\n\tmso-level-text:\\F0B7;\n\tmso-level-tab-stop:none;\n\tmso-level-number-position:left;\n\ttext-indent:-.25in;\n\tfont-family:Symbol;}\n\
-->\n</style>\n</head>\n\
<body lang=EN-US style='word-wrap:break-word'>\n<div class=WordSection1>\n\
<h1>A heading<o:p></o:p></h1>\n\
<p class=MsoNormal>Hello, <b>world</b> \u{2013} caf\u{e9} <span style='color:red'>red</span><o:p></o:p></p>\n\
<p class=MsoListParagraph style='text-indent:-.25in;mso-list:l0 level1 lfo1'><![if !supportLists]><span style='font-family:Symbol'><span style='mso-list:Ignore'>\u{b7}<span style='font:7.0pt \"Times New Roman\"'>&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;\n</span></span></span><![endif]>Milk<o:p></o:p></p>\n\
<p class=MsoNormal><a href=\"https://example.com/\">a link</a><o:p></o:p></p>\n\
</div>\n</body>\n</html>\n";
        let reading = read(page);
        assert_eq!(
            reading.body.plain_text(),
            "A heading\nHello, world \u{2013} caf\u{e9} red\nMilk\na link"
        );
        let Block::Paragraph(heading) = &reading.body.blocks[0] else { panic!() };
        assert_eq!(heading.properties.style.as_deref(), Some("Heading1"));
        let Block::Paragraph(hello) = &reading.body.blocks[1] else { panic!() };
        assert_eq!(hello.properties.space_after, Some(160));
        assert_eq!(hello.properties.line_spacing.map(|s| s.value), Some(257));
        assert_eq!(hello.runs[0].properties.size_half_points, Some(22));
        assert_eq!(hello.runs[0].properties.font.as_deref(), Some("Calibri"));
        assert_eq!(hello.runs[1].properties.bold, Some(true));
        let Block::Paragraph(milk) = &reading.body.blocks[2] else { panic!() };
        assert_eq!(
            milk.properties.numbering.map(|n| (n.id, n.level)),
            Some((wp_docx::BULLET_LIST, 0))
        );
        assert_eq!(milk.plain_text(), "Milk", "the bullet leaked into the text");
        assert_eq!(reading.links.len(), 1);
        assert_eq!(reading.links[0].address, "https://example.com/");
        assert_eq!((reading.links[0].start, reading.links[0].end), (0, 6));
    }

    #[test]
    fn the_page_is_read_in_the_charset_it_names() {
        let (bytes, _) = Encoding::CodePage(1251).encode(
            "<html><head><meta http-equiv=Content-Type content=\"text/html; charset=windows-1251\"></head><body><p>Привет</p></body></html>",
            false,
        );
        assert_eq!(read(&decode_page(&bytes)).body.plain_text(), "Привет");
        let utf8 = "<html><head><meta charset=\"utf-8\"></head><body><p>Привет</p></body></html>";
        assert_eq!(read(&decode_page(utf8.as_bytes())).body.plain_text(), "Привет");
    }

    #[test]
    fn a_picture_comes_from_beside_the_page_or_from_inside_it() {
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n\x2d\xb4\0\0\0\0IEND\xaeB`\x82";
        let folder = std::env::temp_dir().join(format!("wp-html-{}", std::process::id()));
        let _ = std::fs::create_dir_all(folder.join("page_files"));
        std::fs::write(folder.join("page_files/image001.png"), png).unwrap();
        let page = "<body><p>Before <img width=96 height=48 src=\"page_files/image001.png\"> and a <a href=\"https://example.com/\">link</a> after.</p></body>";
        let document =
            open_html(page.as_bytes(), Some(&folder.join("page.htm"))).expect("a document");
        let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!() };
        let picture = paragraph.runs.iter().find_map(|run| {
            run.content.iter().find_map(|c| match c {
                RunContent::Picture(picture) => Some(picture.clone()),
                _ => None,
            })
        });
        let picture = picture.expect("the picture was not put in");
        assert_eq!((picture.width_emu, picture.height_emu), (96 * 9525, 48 * 9525));
        let links = document.hyperlinks();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "link", "the link moved when the picture went in");
        let _ = std::fs::remove_dir_all(&folder);

        // The same page as one file, with the picture inside it.
        let message = format!(
            "MIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=\"B\"; type=\"text/html\"\r\n\r\n--B\r\nContent-Location: file:///C:/page.htm\r\nContent-Type: text/html; charset=\"utf-8\"\r\n\r\n{page}\r\n--B\r\nContent-Location: file:///C:/page_files/image001.png\r\nContent-Transfer-Encoding: base64\r\nContent-Type: image/png\r\n\r\n{}\r\n--B--\r\n",
            mime::encode_base64(png)
        );
        let document = open_mht(message.as_bytes()).expect("a document");
        let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!() };
        assert!(
            paragraph
                .runs
                .iter()
                .any(|run| run.content.iter().any(|c| matches!(c, RunContent::Picture(_)))),
            "the picture was not found among the parts"
        );
        assert_eq!(document.hyperlinks()[0].text, "link");
    }

    #[test]
    fn what_is_written_is_read_back() {
        let mut body = Body::default();
        let mut heading = Paragraph::text("A heading");
        heading.properties.style = Some("Heading1".to_owned());
        body.blocks.push(Block::Paragraph(heading));
        let mut runs = Vec::new();
        let mut bold = wp_docx::model::Run::text("Bold, ");
        bold.properties.bold = Some(true);
        runs.push(bold);
        let mut fancy = wp_docx::model::Run::text("café <b> & \u{201C}Привет\u{201D}");
        fancy.properties.italic = Some(true);
        fancy.properties.size_half_points = Some(28);
        fancy.properties.color = Some("0000FF".to_owned());
        fancy.properties.font = Some("Arial".to_owned());
        runs.push(fancy);
        let mut paragraph = Paragraph::from_runs(runs);
        paragraph.properties.alignment = Some(Alignment::Both);
        paragraph.properties.indent_first_line = Some(360);
        body.blocks.push(Block::Paragraph(paragraph));
        for item in ["Milk", "Bread"] {
            let mut item = Paragraph::text(item);
            item.properties.numbering =
                Some(wp_docx::model::NumberingReference { id: wp_docx::BULLET_LIST, level: 0 });
            body.blocks.push(Block::Paragraph(item));
        }
        let table = wp_docx::model::Table {
            rows: vec![wp_docx::model::TableRow {
                cells: vec![
                    wp_docx::model::TableCell::text("One"),
                    wp_docx::model::TableCell::text("Two"),
                ],
                ..Default::default()
            }],
            grid: vec![2000, 3000],
            ..Default::default()
        };
        body.blocks.push(Block::Table(Box::new(table)));
        let document = Document::create(&body).expect("a document");

        let page = write(&document, "letter.htm", Some("A letter"));
        assert!(page.html.contains("<title>A letter</title>"), "{}", page.html);
        assert!(page.html.contains("&lt;b&gt; &amp;"), "{}", page.html);
        let back = read(&page.html);
        assert_eq!(back.body.plain_text(), body.plain_text());
        let Block::Paragraph(heading) = &back.body.blocks[0] else { panic!() };
        assert_eq!(heading.properties.style.as_deref(), Some("Heading1"));
        let Block::Paragraph(paragraph) = &back.body.blocks[1] else { panic!() };
        assert_eq!(paragraph.properties.alignment, Some(Alignment::Both));
        assert_eq!(paragraph.properties.indent_first_line, Some(360));
        assert_eq!(paragraph.runs[0].properties.bold, Some(true));
        assert_eq!(paragraph.runs[1].properties.italic, Some(true));
        assert_eq!(paragraph.runs[1].properties.size_half_points, Some(28));
        assert_eq!(paragraph.runs[1].properties.color.as_deref(), Some("0000FF"));
        assert_eq!(paragraph.runs[1].properties.font.as_deref(), Some("Arial"));
        let Block::Paragraph(milk) = &back.body.blocks[2] else { panic!() };
        assert_eq!(milk.properties.numbering.map(|n| n.id), Some(wp_docx::BULLET_LIST));
        let Block::Table(table) = &back.body.blocks[4] else { panic!("the table was lost") };
        assert_eq!(table.rows[0].cells[1].blocks[0].plain_text(), "Two");
        assert_eq!(table.grid, vec![2000, 3000]);

        // And as one file.
        let mht = write_mht(&document, "letter.htm", None);
        let again = open_mht(&mht).expect("read back");
        assert_eq!(again.plain_text().trim_end(), body.plain_text());
    }
}

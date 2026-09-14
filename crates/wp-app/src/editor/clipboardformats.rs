//! What goes on the clipboard beside the words, and what is taken off it.
//!
//! Word puts a copy on the clipboard in every format another program might
//! take: the words as text, the same as HTML for a browser or a mail
//! program, as Rich Text for everything older, and a picture as a picture.
//! Pasting, it takes the richest format it finds. This does the same, so
//! that text copied here keeps its formatting in Word and text copied in
//! Word keeps its formatting here — which is the whole point of the
//! formats. See [`wp_shell::clipboard`].

use wp_docx::model::{Block, Paragraph, Run, RunContent};
use wp_docx::{Document, TextPosition};
use wp_shell::clipboard::Contents;

use super::Editor;

/// The character a paragraph holds where a picture was, while the
/// paragraphs are being made into a document of their own.
const PICTURE_MARK: char = '\u{FFFC}';

/// Where a picture sat in the copied paragraphs, and what it was.
struct Held {
    paragraph: usize,
    offset: usize,
    bytes: Vec<u8>,
    extension: String,
    width_emu: i64,
    height_emu: i64,
}

impl Editor {
    /// Everything the selection is, in every format the clipboard carries.
    pub(super) fn clipboard_contents_of_selection(&self, text: &str, blocks: &[Block]) -> Contents {
        let mut contents = Contents { text: Some(text.to_owned()), ..Contents::default() };
        let Some(fragment) = self.fragment_document(blocks) else { return contents };
        contents.rtf = Some(wp_rtf::write(&fragment));
        contents.html = Some(cf_html(&wp_html::write(&fragment, "clip.htm", None)));
        // A picture copied on its own goes as a picture too, which is how it
        // lands in a program that only takes pictures.
        if let Some(held) = self.only_picture(blocks) {
            if let Ok(image) = wp_image::decode(&held.bytes) {
                contents.dib = Some(dib_of(&image));
                contents.png = Some(if held.extension.eq_ignore_ascii_case("png") {
                    held.bytes.clone()
                } else {
                    png_of(&image)
                });
            }
        }
        contents
    }

    /// The copied paragraphs as a document of their own, with the pictures
    /// they refer to brought along, so that the writers can write it.
    fn fragment_document(&self, blocks: &[Block]) -> Option<Document> {
        let (body_blocks, held) = self.detach_pictures(blocks);
        if body_blocks.is_empty() {
            return None;
        }
        let body = wp_docx::model::Body { blocks: body_blocks };
        let mut document = Document::create(&body).ok()?;
        // Last first, so that putting one in does not move the marks after it
        // in the same paragraph.
        for picture in held.into_iter().rev() {
            let start = TextPosition::new(picture.paragraph, picture.offset);
            let end =
                TextPosition::new(picture.paragraph, picture.offset + PICTURE_MARK.len_utf8());
            document.set_caret(start);
            document.extend_selection_to(end);
            document.delete_selection();
            document.set_caret(start);
            let _ = document.insert_picture(
                &picture.bytes,
                &picture.extension,
                picture.width_emu.max(1),
                picture.height_emu.max(1),
            );
        }
        document.set_caret(TextPosition::default());
        document.clear_selection();
        Some(document)
    }

    /// The paragraphs with each picture replaced by a mark, and the pictures
    /// taken out with their bytes, in order.
    fn detach_pictures(&self, blocks: &[Block]) -> (Vec<Block>, Vec<Held>) {
        let mut held = Vec::new();
        let mut out = Vec::new();
        let mut paragraph_index = 0usize;
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => {
                    out.push(Block::Paragraph(self.detach_from(
                        paragraph,
                        paragraph_index,
                        &mut held,
                    )));
                    paragraph_index += 1;
                }
                Block::Table(table) => {
                    let mut table = (**table).clone();
                    for row in &mut table.rows {
                        for cell in &mut row.cells {
                            for cell_block in &mut cell.blocks {
                                if let Block::Paragraph(paragraph) = cell_block {
                                    *paragraph =
                                        self.detach_from(paragraph, paragraph_index, &mut held);
                                    paragraph_index += 1;
                                }
                            }
                        }
                    }
                    out.push(Block::Table(Box::new(table)));
                }
            }
        }
        (out, held)
    }

    fn detach_from(&self, paragraph: &Paragraph, index: usize, held: &mut Vec<Held>) -> Paragraph {
        let mut offset = 0usize;
        let mut runs = Vec::new();
        for run in &paragraph.runs {
            let mut kept = Run { content: Vec::new(), ..run.clone() };
            for content in &run.content {
                match content {
                    RunContent::Picture(picture) => {
                        let bytes = self.document.embedded_part(&picture.relationship);
                        let target = self.document.relationship_target(&picture.relationship);
                        if let (Some(bytes), Some(target)) = (bytes, target) {
                            {
                                let extension = target
                                    .rsplit('.')
                                    .next()
                                    .filter(|ext| !ext.contains('/'))
                                    .unwrap_or("png")
                                    .to_owned();
                                held.push(Held {
                                    paragraph: index,
                                    offset,
                                    bytes: bytes.to_vec(),
                                    extension,
                                    width_emu: picture.width_emu,
                                    height_emu: picture.height_emu,
                                });
                                kept.content.push(RunContent::Text(PICTURE_MARK.to_string()));
                                offset += PICTURE_MARK.len_utf8();
                            }
                        }
                    }
                    RunContent::Text(text) => {
                        offset += text.len();
                        kept.content.push(content.clone());
                    }
                    other => {
                        offset += 1;
                        kept.content.push(other.clone());
                    }
                }
            }
            runs.push(kept);
        }
        Paragraph { properties: paragraph.properties.clone(), runs }
    }

    /// The one picture the selection is, when it is one picture and nothing
    /// else.
    fn only_picture(&self, blocks: &[Block]) -> Option<Held> {
        let [Block::Paragraph(paragraph)] = blocks else { return None };
        if !paragraph.plain_text().trim().is_empty() {
            return None;
        }
        let (_, mut held) = self.detach_pictures(blocks);
        if held.len() == 1 {
            held.pop()
        } else {
            None
        }
    }

    /// What the clipboard holds, as text and as paragraphs: this program's
    /// own copy when the clipboard still holds it, else the richest format
    /// there is, read and made this document's.
    pub(super) fn take_clipboard(&mut self) -> (String, Vec<Block>) {
        let contents = wp_shell::clipboard::contents();
        self.take_contents(contents)
    }

    /// The same, from contents already read — which is how a test hands
    /// the clipboard in, since the build image has no clipboard.
    fn take_contents(&mut self, contents: Contents) -> (String, Vec<Block>) {
        let text = contents.text.clone().unwrap_or_default();
        if let Some((copied, blocks)) = &self.clipboard {
            if !text.is_empty() && *copied == text {
                return (text, blocks.clone());
            }
        }
        // Rich Text first, since it is what Word itself puts on the
        // clipboard and what says most; then HTML, which a browser or a
        // mail program puts there.
        let foreign =
            contents.rtf.as_deref().and_then(|rtf| wp_rtf::open(rtf).ok()).or_else(|| {
                contents.html.as_deref().and_then(|html| {
                    let page = html_of(html);
                    wp_html::open_html(&page, None).ok()
                })
            });
        if let Some(foreign) = foreign {
            let blocks = self.adopt_blocks(&foreign);
            let has_something = blocks.iter().any(|block| {
                !block.plain_text().trim().is_empty()
                    || matches!(block, Block::Paragraph(p) if p.runs.iter().any(|run| {
                        run.content.iter().any(|c| matches!(c, RunContent::Picture(_)))
                    }))
            });
            if has_something {
                let words = if text.is_empty() {
                    foreign.plain_text().trim_end_matches('\n').to_owned()
                } else {
                    text
                };
                return (words, blocks);
            }
        }
        // A picture alone: from a PNG, or from a bitmap given a file header.
        let picture = contents
            .png
            .as_deref()
            .and_then(|png| wp_image::decode(png).ok().map(|_| png.to_vec()))
            .or_else(|| {
                contents
                    .dib
                    .as_deref()
                    .and_then(|dib| wp_image::decode(&bmp_of(dib)).ok().map(|image| png_of(&image)))
            });
        if let Some(png) = picture {
            if let Ok(image) = wp_image::decode(&png) {
                if let Ok(id) = self.document.adopt_picture(&png, "png") {
                    let (width, height) = self.fit_picture(image.width, image.height);
                    let picture = wp_docx::model::Picture {
                        relationship: id,
                        width_emu: width,
                        height_emu: height,
                        ..wp_docx::model::Picture::default()
                    };
                    let run = Run {
                        content: vec![RunContent::Picture(Box::new(picture))],
                        ..Run::text("")
                    };
                    let paragraph = Paragraph { properties: Default::default(), runs: vec![run] };
                    return (text, vec![Block::Paragraph(paragraph)]);
                }
            }
        }
        (text, Vec::new())
    }

    /// Another document's paragraphs made this document's: its pictures
    /// taken into this package and the paragraphs pointed at the copies.
    fn adopt_blocks(&mut self, foreign: &Document) -> Vec<Block> {
        let mut blocks = foreign.body().blocks;
        let mut adopted: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for block in &mut blocks {
            let Block::Paragraph(paragraph) = block else { continue };
            for run in &mut paragraph.runs {
                for content in &mut run.content {
                    let RunContent::Picture(picture) = content else { continue };
                    let id = adopted.entry(picture.relationship.clone()).or_insert_with(|| {
                        let bytes = foreign.embedded_part(&picture.relationship);
                        let target = foreign.relationship_target(&picture.relationship);
                        match (bytes, target) {
                            (Some(bytes), Some(target)) => {
                                let extension =
                                    target.rsplit('.').next().unwrap_or("png").to_owned();
                                self.document.adopt_picture(bytes, &extension).unwrap_or_default()
                            }
                            _ => String::new(),
                        }
                    });
                    picture.relationship = id.clone();
                }
            }
            // A picture that could not be brought along is dropped rather than
            // left pointing at nothing.
            for run in &mut paragraph.runs {
                run.content.retain(|content| {
                    !matches!(content, RunContent::Picture(picture) if picture.relationship.is_empty())
                });
            }
        }
        blocks
    }

    /// How big a pasted picture is drawn: at its own size, at 96 dots to the
    /// inch, and no wider than the text.
    fn fit_picture(&self, width: usize, height: usize) -> (i64, i64) {
        let emu_per_pixel = 914_400 / 96;
        let mut width_emu = width as i64 * emu_per_pixel;
        let mut height_emu = height as i64 * emu_per_pixel;
        let (page_width, _) = self.document.page_size();
        let (_, right, _, left) = self.document.page_margins();
        let text_width = i64::from(page_width - right - left).max(1) * 635;
        if width_emu > text_width {
            height_emu = height_emu * text_width / width_emu.max(1);
            width_emu = text_width;
        }
        (width_emu.max(1), height_emu.max(1))
    }
}

/// The "HTML Format" clipboard format: a header of byte offsets, then the
/// page with the fragment marked inside its body.
fn cf_html(page: &wp_html::Page) -> Vec<u8> {
    // The pictures go inline, since a clipboard has no folder beside it.
    let mut html = page.html.clone();
    for picture in &page.pictures {
        let inline = format!(
            "data:{};base64,{}",
            picture.content_type,
            wp_html::mime::encode_base64(&picture.bytes)
        );
        html = html.replace(&format!("src=\"{}\"", picture.name), &format!("src=\"{inline}\""));
    }
    let body_open = html.find("<body").and_then(|at| html[at..].find('>').map(|end| at + end + 1));
    let body_close = html.rfind("</body>");
    let (start_fragment, end_fragment) = match (body_open, body_close) {
        (Some(open), Some(close)) if open <= close => (open, close),
        _ => (0, html.len()),
    };
    let fragment_open = "<!--StartFragment-->";
    let fragment_close = "<!--EndFragment-->";
    let mut marked = String::with_capacity(html.len() + 64);
    marked.push_str(&html[..start_fragment]);
    marked.push_str(fragment_open);
    marked.push_str(&html[start_fragment..end_fragment]);
    marked.push_str(fragment_close);
    marked.push_str(&html[end_fragment..]);

    // The header is written twice: once to learn its length, once with the
    // offsets that length gives.
    let header = |start_html: usize,
                  end_html: usize,
                  start_fragment: usize,
                  end_fragment: usize| {
        format!(
            "Version:0.9\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\nStartFragment:{start_fragment:010}\r\nEndFragment:{end_fragment:010}\r\n"
        )
    };
    let header_length = header(0, 0, 0, 0).len();
    let start_html = header_length;
    let end_html = header_length + marked.len();
    let fragment_start = header_length + start_fragment + fragment_open.len();
    let fragment_end = header_length + end_fragment + fragment_open.len();
    let mut out = header(start_html, end_html, fragment_start, fragment_end).into_bytes();
    out.extend_from_slice(marked.as_bytes());
    out
}

/// The page inside an "HTML Format" payload: from where its header says
/// the HTML starts to where it ends, or all of it after the header when
/// the header does not say.
fn html_of(payload: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(payload);
    let value = |key: &str| -> Option<usize> {
        text.lines().take(12).find_map(|line| line.strip_prefix(key)?.trim().parse::<usize>().ok())
    };
    // An offset is believed when a tag really begins there: a producer
    // that counted wrongly is not followed into the header.
    let starts_a_tag = |at: usize| {
        payload[at..].iter().find(|b| !b.is_ascii_whitespace()).is_some_and(|b| *b == b'<')
    };
    let start = value("StartHTML:").filter(|start| *start < payload.len() && starts_a_tag(*start));
    let end = value("EndHTML:").filter(|end| *end <= payload.len());
    match (start, end) {
        (Some(start), Some(end)) if start < end => payload[start..end].to_vec(),
        (Some(start), _) => payload[start..].to_vec(),
        _ => {
            // No header worth reading: the page begins at its first tag.
            let at = payload.iter().position(|b| *b == b'<').unwrap_or(0);
            payload[at..].to_vec()
        }
    }
}

/// A device-independent bitmap of a picture: the information header and
/// the pixels, bottom row first, blue-green-red-alpha, which is what
/// `CF_DIB` carries.
fn dib_of(image: &wp_image::Image) -> Vec<u8> {
    let (width, height) = (image.width as u32, image.height as u32);
    let mut out = Vec::with_capacity(40 + image.pixels.len());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(width * height * 4).to_le_bytes());
    out.extend_from_slice(&3780i32.to_le_bytes());
    out.extend_from_slice(&3780i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for row in image.pixels.chunks_exact(image.width * 4).rev() {
        for pixel in row.chunks_exact(4) {
            out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    out
}

/// A bitmap file made from a device-independent bitmap: the file header a
/// `.bmp` starts with, then the bitmap as it was.
fn bmp_of(dib: &[u8]) -> Vec<u8> {
    let header_size = dib.get(..4).map_or(40, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let bit_count = dib.get(14..16).map_or(32, |b| u16::from_le_bytes([b[0], b[1]]));
    let compression = dib.get(16..20).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let colours_used = dib.get(32..36).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    // The palette, or the colour masks, come between the header and the
    // pixels and are counted into where the pixels start.
    let palette = if bit_count <= 8 {
        let entries = if colours_used == 0 { 1u32 << bit_count } else { colours_used };
        entries * if header_size == 12 { 3 } else { 4 }
    } else if compression == 3 && header_size == 40 {
        12
    } else {
        0
    };
    let offset = 14 + header_size + palette;
    let mut out = Vec::with_capacity(14 + dib.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(14 + dib.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(dib);
    out
}

/// A picture as a PNG file.
fn png_of(image: &wp_image::Image) -> Vec<u8> {
    let mut canvas = wp_raster::Canvas::new(image.width, image.height);
    canvas.paste_rect(0, 0, image.width as i32, image.height as i32, &image.pixels);
    wp_raster::encode_png(&canvas)
}

#[cfg(test)]
mod tests {
    use wp_docx::model::{Body, Paragraph};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use super::*;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor(text: &str) -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::default()));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.type_text(text);
        editor
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n\x2d\xb4\0\0\0\0IEND\xaeB`\x82";

    #[test]
    fn a_copy_goes_out_as_text_rich_text_and_html() {
        let mut editor = editor("Dear reader");
        editor.document.set_caret(TextPosition::new(0, 0));
        editor.document.extend_selection_to(TextPosition::new(0, 4));
        editor.document.apply_run_formatting(&wp_docx::model::RunProperties {
            bold: Some(true),
            ..Default::default()
        });
        editor.document.set_caret(TextPosition::new(0, 0));
        editor.document.extend_selection_to(TextPosition::new(0, 11));
        let text = editor.document.selected_text();
        let blocks = editor.document.copy_selection();
        let contents = editor.clipboard_contents_of_selection(&text, &blocks);
        assert_eq!(contents.text.as_deref(), Some("Dear reader"));
        let rtf = String::from_utf8_lossy(contents.rtf.as_deref().unwrap()).into_owned();
        assert!(rtf.starts_with(r"{\rtf1"), "{rtf}");
        assert!(rtf.contains(r"\b"), "no bold in {rtf}");
        assert!(rtf.contains("Dear"), "{rtf}");
        let html = String::from_utf8_lossy(contents.html.as_deref().unwrap()).into_owned();
        assert!(html.starts_with("Version:0.9\r\nStartHTML:"), "{html}");
        assert!(html.contains("<!--StartFragment-->"), "{html}");
        assert!(html.contains("Dear"), "{html}");
        assert!(contents.png.is_none() && contents.dib.is_none());
    }

    #[test]
    fn a_picture_copied_alone_goes_out_as_a_picture_too() {
        let mut editor = editor("");
        let _ = editor.document.insert_picture(PNG, "png", 914_400, 914_400);
        editor.document.select_all();
        let text = editor.document.selected_text();
        let blocks = editor.document.copy_selection();
        let contents = editor.clipboard_contents_of_selection(&text, &blocks);
        assert!(contents.png.as_deref().is_some_and(|png| png.starts_with(b"\x89PNG")));
        assert!(contents.dib.as_deref().is_some_and(|dib| dib.len() == 40 + 4));
        let html = String::from_utf8_lossy(contents.html.as_deref().unwrap()).into_owned();
        assert!(html.contains("src=\"data:image/png;base64,"), "{html}");
    }

    #[test]
    fn rich_text_from_another_program_is_pasted_with_its_formatting() {
        let mut editor = editor("");
        let rtf = br"{\rtf1\ansi{\fonttbl{\f0 Arial;}}\pard Plain {\b bold} and {\i italic}\par}"
            .to_vec();
        let contents = Contents { rtf: Some(rtf), ..Contents::default() };
        let (text, blocks) = editor.take_contents(contents);
        assert_eq!(text, "Plain bold and italic");
        let Block::Paragraph(paragraph) = &blocks[0] else { panic!() };
        assert!(
            paragraph
                .runs
                .iter()
                .any(|run| run.properties.bold == Some(true) && run.plain_text() == "bold"),
            "{:?}",
            paragraph.runs
        );
        assert!(paragraph
            .runs
            .iter()
            .any(|run| run.properties.italic == Some(true) && run.plain_text() == "italic"));
    }

    #[test]
    fn html_from_another_program_is_pasted_with_its_formatting_and_pictures() {
        let mut editor = editor("");
        let page = wp_html::Page {
            html: format!(
                "<html><head><style>p {{ color: #FF0000 }}</style></head><body><p>Red <b>bold</b></p><p><img src=\"data:image/png;base64,{}\" width=\"48\" height=\"48\"></p></body></html>",
                wp_html::mime::encode_base64(PNG)
            ),
            pictures: Vec::new(),
        };
        let contents = Contents { html: Some(cf_html(&page)), ..Contents::default() };
        let (text, blocks) = editor.take_contents(contents);
        assert_eq!(text.trim_end(), "Red bold");
        let Block::Paragraph(paragraph) = &blocks[0] else { panic!() };
        assert!(
            paragraph
                .runs
                .iter()
                .any(|run| run.properties.bold == Some(true) && run.plain_text() == "bold"),
            "{:?}",
            paragraph.runs
        );
        assert!(
            paragraph.runs.iter().all(|run| run.properties.color.as_deref() == Some("FF0000")),
            "{:?}",
            paragraph.runs
        );
        let Block::Paragraph(picture) = &blocks[1] else { panic!() };
        let relationship = picture
            .runs
            .iter()
            .find_map(|run| {
                run.content.iter().find_map(|c| match c {
                    RunContent::Picture(p) => Some(p.relationship.clone()),
                    _ => None,
                })
            })
            .expect("a picture");
        assert!(
            editor.document.embedded_part(&relationship).is_some(),
            "the picture was not brought along"
        );
    }

    #[test]
    fn a_bitmap_alone_is_pasted_as_a_picture() {
        let mut editor = editor("");
        let image = wp_image::Image { width: 2, height: 2, pixels: [0, 0, 0, 255].repeat(4) };
        let contents = Contents { dib: Some(dib_of(&image)), ..Contents::default() };
        let (text, blocks) = editor.take_contents(contents);
        assert!(text.is_empty());
        let Block::Paragraph(paragraph) = &blocks[0] else { panic!() };
        assert!(paragraph
            .runs
            .iter()
            .any(|run| run.content.iter().any(|c| matches!(c, RunContent::Picture(_)))));
    }

    #[test]
    fn the_html_format_header_points_at_the_fragment() {
        let page = wp_html::Page {
            html: "<html><head></head><body><p>Hi</p></body></html>".to_owned(),
            pictures: Vec::new(),
        };
        let payload = cf_html(&page);
        let text = String::from_utf8(payload.clone()).unwrap();
        let value = |key: &str| -> usize {
            text.lines().find_map(|line| line.strip_prefix(key)?.parse().ok()).unwrap()
        };
        assert_eq!(
            &payload[value("StartHTML:")..value("EndHTML:")].to_vec(),
            text.split("\r\n").skip(5).collect::<Vec<_>>().join("\r\n").as_bytes()
        );
        assert_eq!(&payload[value("StartFragment:")..value("EndFragment:")], b"<p>Hi</p>");
        assert!(text.contains("<!--StartFragment--><p>Hi</p><!--EndFragment-->"));
        // And read back: the page between the offsets.
        let page = html_of(&payload);
        assert!(String::from_utf8_lossy(&page).starts_with("<html>"));
    }

    #[test]
    fn a_bitmap_gets_its_file_header_back() {
        let image =
            wp_image::Image { width: 2, height: 1, pixels: vec![255, 0, 0, 255, 0, 0, 255, 255] };
        let dib = dib_of(&image);
        assert_eq!(dib.len(), 40 + 8);
        assert_eq!(&dib[40..44], &[0, 0, 255, 255], "blue-green-red-alpha");
        let bmp = bmp_of(&dib);
        let back = wp_image::decode(&bmp).expect("a bitmap");
        assert_eq!((back.width, back.height), (2, 1));
        assert_eq!(&back.pixels[..4], &[255, 0, 0, 255]);
        let png = png_of(&back);
        assert!(png.starts_with(b"\x89PNG"));
    }
}

#[cfg(test)]
mod word_html_tests {
    use super::*;

    /// The shape Word puts on the clipboard: namespaces of its own, unquoted
    /// attributes, its classes, and an empty `o:p` at the end of a paragraph.
    #[test]
    fn words_own_clipboard_html_is_read() {
        let page = "<html xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:w=\"urn:schemas-microsoft-com:office:word\"><head><meta http-equiv=Content-Type content=\"text/html; charset=utf-8\"><style>p.MsoNormal {margin:0in;font-size:11.0pt;font-family:\"Calibri\",sans-serif;}</style></head><body lang=EN-US style='tab-interval:.5in'><!--StartFragment--><p class=MsoNormal><b>Bold</b> and <i>italic</i><o:p></o:p></p><!--EndFragment--></body></html>";
        let header = "Version:1.0\r\nStartHTML:0000000097\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
        let mut payload = header.as_bytes().to_vec();
        payload.extend_from_slice(page.as_bytes());
        let html = html_of(&payload);
        let document = wp_html::open_html(&html, None).expect("read");
        let body = document.body();
        let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!("{body:?}") };
        assert_eq!(paragraph.plain_text(), "Bold and italic");
        assert!(
            paragraph
                .runs
                .iter()
                .any(|run| run.properties.bold == Some(true) && run.plain_text() == "Bold"),
            "{:?}",
            paragraph.runs
        );
        assert!(paragraph.runs.iter().any(|run| run.properties.italic == Some(true)));
        assert!(
            paragraph
                .runs
                .iter()
                .all(|run| run.properties.size_half_points.is_none_or(|size| size == 22)),
            "{:?}",
            paragraph.runs
        );
    }
}

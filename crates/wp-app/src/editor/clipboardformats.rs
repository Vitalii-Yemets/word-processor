//! What goes on the clipboard beside the words, and what is taken off it.
//!
//! Word puts a copy on the clipboard in every format another program might
//! take: the words as text, the same as HTML for a browser or a mail
//! program, as Rich Text for everything older, and a picture as a picture.
//! Pasting, it takes the richest format it finds. This does the same, so
//! that text copied here keeps its formatting in Word and text copied in
//! Word keeps its formatting here — which is the whole point of the
//! formats. See [`wp_shell::clipboard`].
//!
//! # What is taken off it
//!
//! Whatever it is, it comes back as a copy the way this program makes one —
//! see [`wp_docx::clipboard`] — with every drawing carrying the parts it
//! points at. What another program put there is opened as a document of its
//! own and copied out of that whole, so a picture pasted from a browser, a
//! chart pasted from Word and a bitmap pasted on its own all go into the
//! document by the one road a copy made here goes by, and nothing is taken
//! into the document until something is really pasted.

use wp_docx::model::{Block, Body, Paragraph, RunContent};
use wp_docx::{Document, TextPosition};
use wp_shell::clipboard::Contents;

use super::Editor;

/// A picture that was copied on its own, and what kind of file it is.
struct Held {
    bytes: Vec<u8>,
    extension: String,
}

impl Editor {
    /// Everything the selection is, in every format the clipboard carries.
    pub(super) fn clipboard_contents_of_selection(&self, text: &str, blocks: &[Block]) -> Contents {
        let mut contents = Contents { text: Some(text.to_owned()), ..Contents::default() };
        let Some(fragment) = self.fragment_document(blocks) else { return contents };
        contents.rtf = Some(wp_rtf::write(&fragment));
        contents.html = Some(cf_html(&wp_html::write(&fragment, "clip.htm", None)));
        // And as a Word document of its own, which is Word's own format and
        // carries what the other two cannot.
        contents.document = fragment.save().ok();
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

    /// The copied paragraphs as a document of their own, so that the writers
    /// can write it.
    ///
    /// Made by pasting the copy into an empty document, which is what brings
    /// every drawing in it along with the parts it points at: the same road a
    /// paste takes, so what goes out is what a paste would have put down.
    fn fragment_document(&self, blocks: &[Block]) -> Option<Document> {
        if blocks.is_empty() {
            return None;
        }
        let empty = Body { blocks: vec![Block::Paragraph(Paragraph::default())] };
        let mut document = Document::create(&empty).ok()?;
        // With this document's styles, so that a heading copied is a heading
        // as this document has it wherever it lands, and not whatever a new
        // document's style of the name looks like.
        document.update_styles_from(&self.document);
        document.paste_blocks(blocks);
        document.set_caret(TextPosition::default());
        document.clear_selection();
        Some(document)
    }

    /// The one picture the selection is, when it is one picture and nothing
    /// else.
    fn only_picture(&self, blocks: &[Block]) -> Option<Held> {
        let [Block::Paragraph(paragraph)] = blocks else { return None };
        if !paragraph.plain_text().trim().is_empty() {
            return None;
        }
        let mut pictures = paragraph.runs.iter().flat_map(|run| &run.content).filter_map(|piece| {
            match piece.bare() {
                RunContent::Picture(picture) => Some(picture),
                _ => None,
            }
        });
        let picture = pictures.next()?;
        if pictures.next().is_some() {
            return None;
        }
        // Read from the document, which is the one the copy was just made in.
        let bytes = self.document.embedded_part(&picture.relationship)?;
        let target = self.document.relationship_target(&picture.relationship)?;
        let extension =
            target.rsplit('.').next().filter(|ext| !ext.contains('/')).unwrap_or("png").to_owned();
        Some(Held { bytes: bytes.to_vec(), extension })
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
    pub(super) fn take_contents(&mut self, contents: Contents) -> (String, Vec<Block>) {
        let text = contents.text.clone().unwrap_or_default();
        if let Some((copied, blocks)) = &self.clipboard {
            if !text.is_empty() && *copied == text {
                return (text, blocks.clone());
            }
        }
        // Word's own document first, where Word put one, since it is the
        // whole of what was copied; then Rich Text, which Word puts there
        // too and which says most of the rest; then HTML, which a browser
        // or a mail program puts there.
        let foreign = contents
            .document
            .as_deref()
            .and_then(|package| Document::open(package).ok())
            .or_else(|| contents.rtf.as_deref().and_then(|rtf| wp_rtf::open(rtf).ok()))
            .or_else(|| {
                contents.html.as_deref().and_then(|html| {
                    let page = html_of(html);
                    wp_html::open_html(&page, None).ok()
                })
            });
        if let Some(foreign) = foreign {
            let words = foreign.plain_text().trim_end_matches('\n').to_owned();
            let blocks = copied_whole(foreign);
            // A drawing is something even where there are no words.
            let has_something = blocks.iter().any(|block| {
                !block.plain_text().trim().is_empty()
                    || matches!(block, Block::Paragraph(p) if p.runs.iter().any(|run| {
                        run.content.iter().any(|c| matches!(c, RunContent::Copied(copied) if !copied.is_link()))
                    }))
            });
            if has_something {
                let words = if text.is_empty() { words } else { text };
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
                // Put in a document of its own and copied out of it, so that
                // it is pasted the way every other picture is.
                let (width, height) = self.fit_picture(image.width, image.height);
                let empty = Body { blocks: vec![Block::Paragraph(Paragraph::default())] };
                if let Ok(mut holder) = Document::create(&empty) {
                    if holder.insert_picture(&png, "png", width, height).unwrap_or(false) {
                        return (text, copied_whole(holder));
                    }
                }
            }
        }
        (text, Vec::new())
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

/// Everything a document holds, copied out of it the way a selection is:
/// every drawing with the parts it points at, which is what lets it be
/// pasted into a document it was never part of.
fn copied_whole(mut document: Document) -> Vec<Block> {
    document.select_all();
    document.copy_selection()
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

    /// How many pictures a document holds that reach their parts.
    fn pictures_in(document: &Document) -> usize {
        let body = document.body();
        body.paragraphs()
            .iter()
            .flat_map(|paragraph| &paragraph.runs)
            .flat_map(|run| &run.content)
            .filter(|piece| match piece {
                RunContent::Picture(picture) => {
                    document.embedded_part(&picture.relationship).is_some()
                }
                _ => false,
            })
            .count()
    }

    /// The document saved and opened again, which is what says whether
    /// everything in it points at something.
    fn reopened(document: &Document) -> Document {
        Document::open(&document.save().expect("saving")).expect("reopening")
    }

    /// "Before after " with a picture between the two words. The space at
    /// the end is so that a paste there needs no space of its own.
    fn with_picture() -> Editor {
        let mut editor = editor("Before after ");
        editor.document.set_caret(TextPosition::new(0, 7));
        assert!(editor.document.insert_picture(PNG, "png", 914_400, 914_400).expect("inserted"));
        editor
    }

    /// Selects the picture and copies it the way Copy does once the system
    /// has taken the copy: the words and the formatted copy are kept here,
    /// which is what a paste finds again. The build image has no clipboard
    /// for the rest of it.
    fn copy_the_picture(editor: &mut Editor) -> String {
        editor.document.set_caret(TextPosition::new(0, 7));
        editor.document.extend_selection_to(TextPosition::new(0, 8));
        let text = editor.document.selected_text();
        let blocks = editor.document.copy_selection();
        editor.clipboard = Some((text.clone(), blocks));
        text
    }

    fn pasted_here(editor: &mut Editor, text: String) {
        let contents = Contents { text: Some(text), ..Contents::default() };
        let (words, blocks) = editor.take_contents(contents);
        editor.put_down(&words, &blocks, super::super::paste::PasteAs::KeepSource);
    }

    #[test]
    fn our_own_copy_comes_back_whole_with_the_parts_it_points_at() {
        let mut editor = with_picture();
        let text = copy_the_picture(&mut editor);
        let kept = editor.clipboard.as_ref().expect("the copy").1.clone();

        let contents = Contents { text: Some(text.clone()), ..Contents::default() };
        let (words, blocks) = editor.take_contents(contents);
        assert_eq!(words, text);
        assert_eq!(blocks, kept, "what came back is not what was copied");
        let carried = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph),
                Block::Table(_) => None,
            })
            .flat_map(|paragraph| &paragraph.runs)
            .flat_map(|run| &run.content)
            .find_map(|piece| match piece {
                RunContent::Copied(copied) => Some(copied),
                _ => None,
            })
            .expect("the picture came back as a reference and nothing more");
        assert!(matches!(carried.content, RunContent::Picture(_)));
        assert!(
            carried.parts().any(|(_, bytes)| bytes == PNG),
            "the picture's bytes are not in it"
        );

        // And into another document opened in its place: the copy is still
        // what the clipboard holds, and it brings its picture.
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Elsewhere ")));
        let other = Document::open(&Document::create(&body).expect("made").save().expect("saved"))
            .expect("opened");
        editor.set_document(other, None);
        editor.document.set_caret(TextPosition::new(0, "Elsewhere ".len()));
        pasted_here(&mut editor, text);
        let other = reopened(&editor.document);
        assert_eq!(other.paragraph_text(0).as_deref(), Some("Elsewhere \u{1}"));
        assert_eq!(pictures_in(&other), 1, "the picture did not come into the other document");
    }

    /// "Sales " and a chart after it.
    fn with_chart(document: &mut Document) {
        let end = document.paragraph_text(0).expect("a paragraph").len();
        document.set_caret(TextPosition::new(0, end));
        let chart = wp_docx::chart::Chart::parse(
            wp_docx::chart::Kind::Column,
            "Sales",
            "North=10; South=20",
        );
        assert!(document.insert_chart(&chart, 914_400 * 4, 914_400 * 3).expect("the chart"));
    }

    /// How many charts a document holds that reach their parts.
    fn charts_in(document: &Document) -> usize {
        let body = document.body();
        body.paragraphs()
            .iter()
            .flat_map(|paragraph| &paragraph.runs)
            .flat_map(|run| &run.content)
            .filter(|piece| match piece {
                RunContent::Chart(chart) => document.chart(&chart.relationship).is_some(),
                _ => false,
            })
            .count()
    }

    #[test]
    fn a_chart_in_a_word_document_on_the_clipboard_is_pasted_with_its_part() {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Sales ")));
        let mut theirs = Document::create(&body).expect("a document");
        with_chart(&mut theirs);
        let contents = Contents {
            text: Some("Sales ".to_owned()),
            document: Some(theirs.save().expect("saved")),
            ..Contents::default()
        };

        let mut editor = editor("");
        let (text, blocks) = editor.take_contents(contents);
        assert_eq!(charts_in(&editor.document), 0, "taken in before anything was pasted");
        editor.put_down(&text, &blocks, super::super::paste::PasteAs::KeepSource);
        assert_eq!(charts_in(&reopened(&editor.document)), 1);
    }

    #[test]
    fn a_chart_copied_goes_out_in_the_word_document_on_the_clipboard() {
        let mut editor = editor("Sales ");
        with_chart(&mut editor.document);
        editor.document.select_all();
        let text = editor.document.selected_text();
        let blocks = editor.document.copy_selection();
        let contents = editor.clipboard_contents_of_selection(&text, &blocks);

        let copy = Document::open(&contents.document.expect("the copy as a document"))
            .expect("a .docx that opens");
        assert_eq!(copy.paragraph_text(0).as_deref(), Some("Sales \u{1}"));
        assert_eq!(charts_in(&copy), 1, "the chart did not go out with the copy");
    }

    #[test]
    fn a_link_comes_back_from_the_clipboard_as_a_link() {
        let mut editor = editor("See the manual.");
        editor.document.set_caret(TextPosition::new(0, 8));
        editor.document.extend_selection_to(TextPosition::new(0, 14));
        assert!(editor.document.add_hyperlink("https://example.com/manual", ""));
        editor.document.set_caret(TextPosition::new(0, 4));
        editor.document.extend_selection_to(TextPosition::new(0, 14));
        let text = editor.document.selected_text();
        let blocks = editor.document.copy_selection();
        editor.clipboard = Some((text.clone(), blocks));

        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Elsewhere ")));
        let other = Document::open(&Document::create(&body).expect("made").save().expect("saved"))
            .expect("opened");
        editor.set_document(other, None);
        editor.document.set_caret(TextPosition::new(0, "Elsewhere ".len()));
        pasted_here(&mut editor, text);

        let other = reopened(&editor.document);
        assert_eq!(other.paragraph_text(0).as_deref(), Some("Elsewhere the manual"));
        let links: Vec<(String, wp_docx::links::Destination)> =
            other.hyperlinks().into_iter().map(|link| (link.text, link.destination)).collect();
        let address = wp_docx::links::Destination::Address("https://example.com/manual".into());
        assert_eq!(links, vec![("manual".to_owned(), address)]);
    }

    #[test]
    fn a_picture_cut_and_pasted_is_still_there() {
        let mut editor = with_picture();
        let text = copy_the_picture(&mut editor);
        // What Cut does once the system has taken the copy.
        assert!(editor.document.delete_selection());
        assert_eq!(pictures_in(&editor.document), 0);

        editor.document.set_caret(TextPosition::new(0, "Before after ".len()));
        pasted_here(&mut editor, text);
        let document = reopened(&editor.document);
        assert_eq!(document.paragraph_text(0).as_deref(), Some("Before after \u{1}"));
        assert_eq!(pictures_in(&document), 1, "the cut picture was lost");
    }

    #[test]
    fn a_picture_copied_and_pasted_is_there_twice() {
        let mut editor = with_picture();
        let text = copy_the_picture(&mut editor);
        editor.document.set_caret(TextPosition::new(0, "Before \u{1}after ".len()));
        pasted_here(&mut editor, text);
        let document = reopened(&editor.document);
        assert_eq!(pictures_in(&document), 2);
        assert_eq!(editor.status, "Pasted 1 characters", "the paste said {:?}", editor.status);
    }

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

    /// The copy goes as a Word document of its own too, carrying the
    /// document's styles: a paragraph in a style this document made is in
    /// that style, as this document has it, in the copy.
    #[test]
    fn a_copy_goes_out_as_a_word_document_with_the_styles_it_uses() {
        let mut editor = editor("Kept as it was");
        let style = wp_docx::StyleDefinition {
            id: "Pullquote".to_owned(),
            name: "Pullquote".to_owned(),
            based_on: None,
            next: None,
            paragraph: wp_docx::model::ParagraphProperties::default(),
            run: wp_docx::model::RunProperties {
                italic: Some(true),
                size_half_points: Some(36),
                ..Default::default()
            },
        };
        assert!(editor.document.set_style(&style));
        assert!(editor.document.set_paragraph_style(0, Some("Pullquote")));
        editor.document.select_all();
        let text = editor.document.selected_text();
        let blocks = editor.document.copy_selection();
        let contents = editor.clipboard_contents_of_selection(&text, &blocks);

        let package = contents.document.expect("the copy as a document");
        let copy = Document::open(&package).expect("a .docx that opens");
        assert_eq!(copy.plain_text().trim_end(), "Kept as it was");
        assert_eq!(copy.paragraph_styles(), vec![Some("Pullquote".to_owned())]);
        let resolved = copy.styles().resolve_run(Some("Pullquote"), &Default::default());
        assert!(resolved.italic && resolved.size_half_points == 36, "{resolved:?}");
    }

    /// What Word put on the clipboard as a document of its own is what is
    /// pasted, before its Rich Text: the document says everything.
    #[test]
    fn a_word_document_on_the_clipboard_is_pasted_before_its_rich_text() {
        let mut editor = editor("");
        let mut body = Body::default();
        let mut paragraph = Paragraph::text("From the document");
        paragraph.runs[0].properties.underline = Some(wp_docx::model::Underline::Double);
        body.blocks.push(Block::Paragraph(paragraph));
        let package = Document::create(&body).expect("a document").save().expect("saved");
        let rtf = br"{\rtf1\ansi\pard From the Rich Text\par}".to_vec();
        let contents = Contents {
            text: Some("From the document".to_owned()),
            rtf: Some(rtf),
            document: Some(package),
            ..Contents::default()
        };
        let (text, blocks) = editor.take_contents(contents);
        assert_eq!(text, "From the document");
        let Block::Paragraph(paragraph) = &blocks[0] else { panic!("a paragraph") };
        assert_eq!(paragraph.plain_text(), "From the document");
        assert_eq!(
            paragraph.runs[0].properties.underline,
            Some(wp_docx::model::Underline::Double),
            "what Rich Text from here would not have said"
        );
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
        let carried = picture
            .runs
            .iter()
            .flat_map(|run| &run.content)
            .find_map(|c| match c {
                RunContent::Copied(copied) if matches!(copied.content, RunContent::Picture(_)) => {
                    Some(copied)
                }
                _ => None,
            })
            .expect("a picture, carried with its part");
        assert!(
            carried.parts().any(|(_, bytes)| !bytes.is_empty()),
            "the picture's bytes did not come with it"
        );
        // Nothing is taken into the document until it is pasted, and then
        // the picture is there with its part.
        assert_eq!(pictures_in(&editor.document), 0);
        editor.put_down(&text, &blocks, super::super::paste::PasteAs::KeepSource);
        assert_eq!(pictures_in(&editor.document), 1, "the picture was not brought along");
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
            .any(|run| run.content.iter().any(|c| matches!(c.bare(), RunContent::Picture(_)))));
        editor.put_down(&text, &blocks, super::super::paste::PasteAs::KeepSource);
        assert_eq!(pictures_in(&editor.document), 1, "the bitmap was not pasted");
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

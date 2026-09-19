//! The files a document carries inside it: a sound, a film, a spreadsheet
//! dropped in with Insert ▸ Object.
//!
//! # What an embedded object is
//!
//! `w:object`: a picture of the thing — the icon or the first frame, as VML —
//! and beside it `o:OLEObject`, which names the part the thing itself is kept
//! in, `word/embeddings/oleObject1.bin`. That part is a compound file, the
//! container OLE has used since 1993, which [`wp_ole`] reads. A file packaged
//! whole is in the stream `\1Ole10Native`, wrapped by the packager with its
//! name and the path it came from.
//!
//! # What this program does with one
//!
//! Shows the picture, which is what Word shows, and offers the file: not to
//! whatever program plays such things — a document does not get to say what
//! this program runs, see [`wp_shell::desktop::open`] — but to be saved where
//! the person chooses, from where they can open it themselves.
//!
//! A picture that carries `a:videoFile` is a video the same way: the frame,
//! and a link to the media, which is a part of the package when the video
//! was embedded and a path when it was linked.

use wp_xml::tree::Element;

use crate::{Document, TextPosition};

/// A file kept inside the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedFile {
    /// What the file is called: the name the packager kept, or the part's.
    pub name: String,
    /// The file itself.
    pub bytes: Vec<u8>,
}

/// Where the media behind a picture is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Media {
    /// Inside the package: the file, ready to be saved.
    Inside(EmbeddedFile),
    /// Outside it, at a path or an address the document names.
    Outside(String),
}

impl Document {
    /// The file the drawing at one place carries, if it carries one: the
    /// object embedded behind a `w:object`, or the media behind a picture
    /// that is a video.
    #[must_use]
    pub fn media_at(&self, at: TextPosition) -> Option<Media> {
        let drawing = self.drawing_element_at(at)?;
        if let Some(object) = find(drawing, "OLEObject") {
            let id = object
                .attribute(Some(crate::edit::RELATIONSHIPS), "id")
                .or_else(|| object.attribute_by_name("r:id"))?;
            let target = self.relationship_target(id)?;
            let bytes = self.package().part(&target)?;
            let named = object.attribute_by_name("ProgID").unwrap_or_default();
            return Some(Media::Inside(unwrap_object(bytes, &target, named)));
        }
        let video = find(drawing, "videoFile")?;
        let id = video
            .attribute(Some(crate::edit::RELATIONSHIPS), "link")
            .or_else(|| video.attribute_by_name("r:link"))?;
        let relationships = self.package().relationships(self.main_part()).ok()?;
        let relationship = relationships.by_id(id)?;
        if relationship.mode == wp_opc::TargetMode::External {
            return Some(Media::Outside(relationship.target.clone()));
        }
        let target = self.relationship_target(id)?;
        let bytes = self.package().part(&target)?.to_vec();
        Some(Media::Inside(EmbeddedFile { name: file_name(&target), bytes }))
    }
}

/// The file inside an embedded object's part: the packaged file when the
/// part is a compound file holding one, and otherwise the part as it is,
/// named after the program that made it.
fn unwrap_object(bytes: &[u8], part: &str, program: &str) -> EmbeddedFile {
    if let Ok(compound) = wp_ole::CompoundFile::open(bytes.to_vec()) {
        if let Some((name, data)) =
            compound.stream("\u{1}Ole10Native").and_then(|stream| packaged(&stream))
        {
            return EmbeddedFile { name, bytes: data };
        }
    }
    // An object made by some program rather than a file packaged whole —
    // an equation, a spreadsheet — is offered as the compound file it is,
    // which that program opens.
    let extension = match program.split('.').next().unwrap_or_default() {
        "Excel" => "xls",
        "Word" => "doc",
        "PowerPoint" => "ppt",
        "Visio" => "vsd",
        "Equation" => "bin",
        _ => "bin",
    };
    let stem = file_name(part);
    let stem = stem.rsplit_once('.').map_or(stem.as_str(), |(stem, _)| stem).to_owned();
    EmbeddedFile { name: format!("{stem}.{extension}"), bytes: bytes.to_vec() }
}

/// The name at the end of a part's path.
fn file_name(part: &str) -> String {
    part.rsplit('/').next().unwrap_or(part).to_owned()
}

/// The file the packager wrapped: its name, and its bytes.
///
/// The stream begins with its own length, then a word, then the file's name
/// and the path it came from — each ended by a nought — then a word of
/// flags, the length of a temporary path and that path, and last the length
/// of the file and the file.
fn packaged(stream: &[u8]) -> Option<(String, Vec<u8>)> {
    let mut at = 4 + 2;
    let text = |from: &mut usize| -> Option<String> {
        let end = stream[*from..].iter().position(|byte| *byte == 0)? + *from;
        let found = String::from_utf8_lossy(&stream[*from..end]).into_owned();
        *from = end + 1;
        Some(found)
    };
    let word = |from: &mut usize| -> Option<u32> {
        let bytes: [u8; 4] = stream.get(*from..*from + 4)?.try_into().ok()?;
        *from += 4;
        Some(u32::from_le_bytes(bytes))
    };
    let name = text(&mut at)?;
    let _path = text(&mut at)?;
    let _flags = word(&mut at)?;
    let temporary = word(&mut at)? as usize;
    at = at.checked_add(temporary)?;
    let length = word(&mut at)? as usize;
    let data = stream.get(at..at.checked_add(length)?)?;
    Some((if name.is_empty() { "file".to_owned() } else { name }, data.to_vec()))
}

/// The first descendant with a local name.
fn find<'a>(element: &'a Element, local: &str) -> Option<&'a Element> {
    for child in element.child_elements() {
        if child.local_name() == local {
            return Some(child);
        }
        if let Some(found) = find(child, local) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packager's wrapping round a file.
    pub(crate) fn wrapped(name: &str, data: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&2u16.to_le_bytes());
        body.extend_from_slice(name.as_bytes());
        body.push(0);
        body.extend_from_slice(b"C:\\Users\\Somebody\\Videos\\");
        body.extend_from_slice(name.as_bytes());
        body.push(0);
        body.extend_from_slice(&0x0003_0000u32.to_le_bytes());
        let temporary = b"C:\\Temp\\x.tmp\0";
        body.extend_from_slice(&(temporary.len() as u32).to_le_bytes());
        body.extend_from_slice(temporary);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend(body);
        out
    }

    #[test]
    fn the_packaged_file_comes_out_with_its_name() {
        let data = b"RIFF....WAVEfmt ".to_vec();
        let stream = wrapped("clip.wav", &data);
        assert_eq!(packaged(&stream), Some(("clip.wav".to_owned(), data)));
        assert_eq!(packaged(&stream[..20]), None, "a stream cut short is nothing");
    }

    #[test]
    fn an_object_that_is_not_a_packaged_file_is_offered_as_what_it_is() {
        let file = wp_ole::Builder::new().stream("Workbook", b"BIFF8".to_vec()).build();
        let offered = unwrap_object(&file, "word/embeddings/oleObject1.bin", "Excel.Sheet.8");
        assert_eq!(offered.name, "oleObject1.xls");
        assert_eq!(offered.bytes, file);
        let raw = unwrap_object(b"whatever", "word/embeddings/oleObject2.bin", "");
        assert_eq!(raw.name, "oleObject2.bin");
    }
}

#[cfg(test)]
mod document_tests {
    use super::*;
    use crate::model::{Block, Body, Paragraph};

    /// A one-pixel picture, for the preview an object shows.
    const PIXEL: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn document() -> Document {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before after")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let mut document = Document::open(&bytes).expect("reopening");
        document.set_caret(TextPosition::new(0, 7));
        document
    }

    /// Puts an element into the first paragraph at the caret, with the
    /// declarations the element needs.
    fn insert(document: &mut Document, xml: &str) {
        let element = wp_xml::tree::XmlTree::parse(xml).expect("parses").root;
        let caret = document.caret();
        let prefix = document.prefix();
        assert!(crate::position::insert_element_at(
            &mut document.tree_mut().root,
            caret,
            element,
            prefix.as_deref(),
        ));
    }

    #[test]
    fn an_object_embedded_with_a_preview_offers_the_file_packaged_inside_it() {
        let mut document = document();
        let clip = b"RIFF....WAVEfmt ".to_vec();
        let stream = super::tests::wrapped("clip.wav", &clip);
        let file = wp_ole::Builder::new().stream("\u{1}Ole10Native", stream).build();
        document.package_mut().add_part(
            "word/embeddings/oleObject1.bin",
            "application/vnd.openxmlformats-officedocument.oleObject",
            file,
        );
        let object = document
            .point_at_part(
                "word/embeddings/oleObject1.bin",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject",
            )
            .expect("pointed");
        let preview = document.adopt_picture(PIXEL, "png").expect("the preview");
        insert(
            &mut document,
            &format!(
                "<w:object xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
                 xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" \
                 xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
                 <v:shape id=\"_x0000_i1025\" type=\"#_x0000_t75\" style=\"width:64pt;height:48pt\" o:ole=\"\">\
                 <v:imagedata r:id=\"{preview}\" o:title=\"\"/></v:shape>\
                 <o:OLEObject Type=\"Embed\" ProgID=\"Package\" ShapeID=\"_x0000_i1025\" DrawAspect=\"Icon\" \
                 ObjectID=\"_1\" r:id=\"{object}\"/></w:object>"
            ),
        );

        // The object stands for one character, is a drawing where it is,
        // and shows its preview as a picture.
        assert_eq!(document.paragraph_text(0).unwrap_or_default().chars().count(), 13);
        let at = TextPosition::new(0, 7);
        assert!(document.drawing_at(at), "the object is not a drawing");
        let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!("a paragraph") };
        let pictures = paragraph
            .runs
            .iter()
            .flat_map(|run| run.content.iter())
            .filter(|piece| matches!(piece, crate::model::RunContent::Picture(_)))
            .count();
        assert_eq!(pictures, 1, "the preview is not shown");
        // And offers the file the packager wrapped.
        assert_eq!(
            document.media_at(at),
            Some(Media::Inside(EmbeddedFile { name: "clip.wav".to_owned(), bytes: clip }))
        );
        assert_eq!(document.media_at(TextPosition::new(0, 2)), None);
    }

    #[test]
    fn a_picture_that_is_a_video_offers_the_media_inside_or_names_it_outside() {
        let mut document = document();
        let film = b"\0\0\0\x1cftypisom".to_vec();
        document.package_mut().add_part("word/media/media1.mp4", "video/mp4", film.clone());
        let inside = document
            .point_at_part(
                "word/media/media1.mp4",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/video",
            )
            .expect("pointed");
        let frame = document.adopt_picture(PIXEL, "png").expect("the frame");
        let drawing = |link: &str| {
            format!(
                "<w:drawing xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
                 xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
                 xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
                 xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\" \
                 xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
                 <wp:inline><wp:extent cx=\"914400\" cy=\"914400\"/><wp:docPr id=\"7\" name=\"Video 7\"/>\
                 <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
                 <pic:pic><pic:nvPicPr><pic:cNvPr id=\"7\" name=\"Video 7\"><a:videoFile r:link=\"{link}\"/></pic:cNvPr>\
                 <pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{frame}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
                 <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm>\
                 <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
                 </a:graphicData></a:graphic></wp:inline></w:drawing>"
            )
        };
        insert(&mut document, &drawing(&inside));
        assert_eq!(
            document.media_at(TextPosition::new(0, 7)),
            Some(Media::Inside(EmbeddedFile { name: "media1.mp4".to_owned(), bytes: film }))
        );

        // Linked rather than embedded: the path, and nothing opened.
        let mut relationships =
            document.package().relationships(document.main_part()).expect("rels");
        let outside = relationships
            .add(
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/video",
                "file:///C:/Videos/holiday.mp4",
                wp_opc::TargetMode::External,
            )
            .id
            .clone();
        document.package_mut().set_relationships(&relationships).expect("written");
        document.set_caret(TextPosition::new(0, 9));
        insert(&mut document, &drawing(&outside));
        assert_eq!(
            document.media_at(TextPosition::new(0, 9)),
            Some(Media::Outside("file:///C:/Videos/holiday.mp4".to_owned()))
        );
    }
}

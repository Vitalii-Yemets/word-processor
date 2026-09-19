//! The files a document carries inside it: a sound, a film, a spreadsheet
//! dropped in with Insert ▸ Object.
//!
//! # What an embedded object is
//!
//! `w:object`: a picture of the thing — the icon or the first frame, as VML —
//! and beside it `o:OLEObject`, which names the part the thing itself is kept
//! in, `word/embeddings/oleObject1.bin`. That part is a compound file: the
//! container OLE has used since 1993, a little file system with sectors, a
//! file allocation table and a directory, holding streams by name. A file
//! packaged whole is in the stream `\1Ole10Native`, wrapped by the packager
//! with its name and the path it came from.
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
    if let Some(stream) = cfb::stream(bytes, "\u{1}Ole10Native") {
        if let Some((name, data)) = packaged(&stream) {
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

/// The compound file: enough of it to read one stream by name.
pub(crate) mod cfb {
    /// The eight bytes every compound file begins with.
    const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    /// The sector number that ends a chain.
    const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
    /// How many of the FAT's own sector numbers the header holds.
    const IN_HEADER: usize = 109;

    fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
    }

    fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    }

    /// The compound file's shape: how big a sector is, and the tables.
    struct Layout {
        sector: usize,
        mini_sector: usize,
        fat: Vec<u32>,
        mini_fat: Vec<u32>,
        directory: Vec<u8>,
        mini_stream: Vec<u8>,
        cutoff: usize,
    }

    /// Reads a stream out of a compound file by name, `\1Ole10Native` and
    /// the like. `None` for anything that is not a compound file, or one
    /// with no such stream.
    pub(crate) fn stream(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
        let layout = Layout::read(bytes)?;
        let wanted: Vec<u16> = name.encode_utf16().collect();
        for entry in layout.directory.chunks_exact(128) {
            let length = usize::from(u16_at(entry, 64)?).min(64) / 2;
            let found: Vec<u16> = (0..length.saturating_sub(1))
                .filter_map(|index| u16_at(entry, index * 2))
                .collect();
            if found != wanted || entry[66] != 2 {
                continue;
            }
            let start = u32_at(entry, 116)?;
            let size = u32_at(entry, 120)? as usize;
            return Some(layout.stream_at(bytes, start, size));
        }
        None
    }

    impl Layout {
        fn read(bytes: &[u8]) -> Option<Self> {
            if bytes.get(0..8)? != SIGNATURE {
                return None;
            }
            let sector = 1usize << u16_at(bytes, 30)?;
            let mini_sector = 1usize << u16_at(bytes, 32)?;
            if !(64..=65_536).contains(&sector) || mini_sector == 0 || mini_sector > sector {
                return None;
            }
            let fat_sectors = u32_at(bytes, 44)? as usize;
            let first_directory = u32_at(bytes, 48)?;
            let cutoff = u32_at(bytes, 56)? as usize;
            let first_mini_fat = u32_at(bytes, 60)?;
            let mini_fat_sectors = u32_at(bytes, 64)? as usize;
            let first_difat = u32_at(bytes, 68)?;
            let difat_sectors = u32_at(bytes, 72)? as usize;

            // Where the FAT's sectors are: the first hundred and nine in the
            // header, the rest in a chain of their own.
            let mut fat_places: Vec<u32> = (0..IN_HEADER)
                .filter_map(|index| u32_at(bytes, 76 + index * 4))
                .take(fat_sectors)
                .collect();
            let mut difat = first_difat;
            for _ in 0..difat_sectors {
                if difat >= END_OF_CHAIN - 1 {
                    break;
                }
                let at = (difat as usize + 1) * sector;
                let per = sector / 4 - 1;
                for index in 0..per {
                    if fat_places.len() >= fat_sectors {
                        break;
                    }
                    fat_places.push(u32_at(bytes, at + index * 4)?);
                }
                difat = u32_at(bytes, at + per * 4)?;
            }
            let mut fat = Vec::new();
            for place in fat_places {
                let at = (place as usize + 1) * sector;
                for index in 0..sector / 4 {
                    fat.push(u32_at(bytes, at + index * 4)?);
                }
            }

            let mut layout = Self {
                sector,
                mini_sector,
                fat,
                mini_fat: Vec::new(),
                directory: Vec::new(),
                mini_stream: Vec::new(),
                cutoff,
            };
            layout.directory = layout.chain(bytes, first_directory, usize::MAX);
            // The mini FAT is a chain like any other; the mini stream is the
            // root entry's own stream, in which the small streams live.
            if mini_fat_sectors > 0 {
                let raw = layout.chain(bytes, first_mini_fat, mini_fat_sectors * sector);
                layout.mini_fat = raw
                    .chunks_exact(4)
                    .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect();
            }
            if let Some(root) = layout.directory.get(0..128) {
                let start = u32_at(root, 116)?;
                let size = u32_at(root, 120)? as usize;
                layout.mini_stream = layout.chain(bytes, start, size);
            }
            Some(layout)
        }

        /// The sectors of a chain, one after another, cut to a size.
        fn chain(&self, bytes: &[u8], first: u32, size: usize) -> Vec<u8> {
            let mut out = Vec::new();
            let mut at = first;
            let mut steps = 0usize;
            while at < END_OF_CHAIN - 1 && out.len() < size && steps <= self.fat.len() {
                let from = (at as usize + 1) * self.sector;
                let Some(piece) = bytes.get(from..from + self.sector) else { break };
                out.extend_from_slice(piece);
                let Some(next) = self.fat.get(at as usize) else { break };
                at = *next;
                steps += 1;
            }
            out.truncate(size);
            out
        }

        /// A stream: from the mini stream when it is small, else from the
        /// file's own sectors.
        fn stream_at(&self, bytes: &[u8], first: u32, size: usize) -> Vec<u8> {
            if size >= self.cutoff {
                return self.chain(bytes, first, size);
            }
            let mut out = Vec::new();
            let mut at = first;
            let mut steps = 0usize;
            while at < END_OF_CHAIN - 1 && out.len() < size && steps <= self.mini_fat.len() {
                let from = at as usize * self.mini_sector;
                let Some(piece) = self.mini_stream.get(from..from + self.mini_sector) else {
                    break;
                };
                out.extend_from_slice(piece);
                let Some(next) = self.mini_fat.get(at as usize) else { break };
                at = *next;
                steps += 1;
            }
            out.truncate(size);
            out
        }
    }

    #[cfg(test)]
    pub(crate) mod tests {
        use super::*;

        /// A compound file of one stream, written the simplest way the
        /// format allows: one FAT sector, one directory sector, and the
        /// stream in the file's own sectors, however small it is.
        pub(crate) fn compound_file(name: &str, content: &[u8]) -> Vec<u8> {
            let sector = 512usize;
            let data_sectors = content.len().div_ceil(sector).max(1);
            // Sector 0 is the FAT, 1 the directory, 2.. the stream.
            let mut fat: Vec<u32> = vec![0xFFFF_FFFD, END_OF_CHAIN];
            for index in 0..data_sectors {
                fat.push(if index + 1 == data_sectors { END_OF_CHAIN } else { (index + 3) as u32 });
            }
            while fat.len() < sector / 4 {
                fat.push(0xFFFF_FFFF);
            }

            let mut header = vec![0u8; sector];
            header[0..8].copy_from_slice(&SIGNATURE);
            header[24..26].copy_from_slice(&0x3Eu16.to_le_bytes());
            header[26..28].copy_from_slice(&3u16.to_le_bytes());
            header[28..30].copy_from_slice(&0xFFFEu16.to_le_bytes());
            header[30..32].copy_from_slice(&9u16.to_le_bytes());
            header[32..34].copy_from_slice(&6u16.to_le_bytes());
            header[44..48].copy_from_slice(&1u32.to_le_bytes());
            header[48..52].copy_from_slice(&1u32.to_le_bytes());
            // Everything is in the file's own sectors: a cutoff of nought.
            header[56..60].copy_from_slice(&0u32.to_le_bytes());
            header[60..64].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
            header[64..68].copy_from_slice(&0u32.to_le_bytes());
            header[68..72].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
            header[72..76].copy_from_slice(&0u32.to_le_bytes());
            for index in 0..IN_HEADER {
                let value = if index == 0 { 0u32 } else { 0xFFFF_FFFF };
                header[76 + index * 4..80 + index * 4].copy_from_slice(&value.to_le_bytes());
            }

            let entry = |name: &str, kind: u8, start: u32, size: u32, child: u32| -> Vec<u8> {
                let mut out = vec![0u8; 128];
                let utf16: Vec<u16> = name.encode_utf16().collect();
                for (index, unit) in utf16.iter().enumerate().take(31) {
                    out[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
                }
                out[64..66].copy_from_slice(&(((utf16.len() + 1) * 2) as u16).to_le_bytes());
                out[66] = kind;
                out[67] = 1;
                out[68..72].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
                out[72..76].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
                out[76..80].copy_from_slice(&child.to_le_bytes());
                out[116..120].copy_from_slice(&start.to_le_bytes());
                out[120..124].copy_from_slice(&size.to_le_bytes());
                out
            };
            let mut directory = entry("Root Entry", 5, END_OF_CHAIN, 0, 1);
            directory.extend(entry(name, 2, 2, content.len() as u32, 0xFFFF_FFFF));
            directory.resize(sector, 0);

            let mut out = header;
            for value in fat {
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.extend_from_slice(&directory);
            out.extend_from_slice(content);
            out.resize((3 + data_sectors) * sector, 0);
            out
        }

        #[test]
        fn a_stream_is_read_back_out_of_a_compound_file_by_name() {
            let content: Vec<u8> = (0..1500u32).map(|n| (n % 251) as u8).collect();
            let file = compound_file("\u{1}Ole10Native", &content);
            assert_eq!(stream(&file, "\u{1}Ole10Native"), Some(content));
            assert_eq!(stream(&file, "Nothing"), None);
            assert_eq!(stream(b"not a compound file at all", "\u{1}Ole10Native"), None);
        }
    }
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
        let file = cfb::tests::compound_file("Workbook", b"BIFF8");
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
        let file = cfb::tests::compound_file("\u{1}Ole10Native", &stream);
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

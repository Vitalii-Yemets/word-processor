//! OpenDocument Text: reading and writing the open format.
//!
//! # Why
//!
//! It is what an open format is for: the format LibreOffice writes, that
//! governments ask for, that Word itself reads and writes. A package of
//! XML in a ZIP, like a `.docx` and unlike it in every name — see [`read`]
//! for how it is put together.
//!
//! # What is here
//!
//! [`open`] reads a package into a document: the text with its paragraph
//! and character formatting resolved through the styles, headings, lists,
//! tables, links, pictures, the page and the title. [`save`] writes one
//! that LibreOffice and Word read back. Not here: headers and footers,
//! footnotes, comments, tracked changes, sections, frames other than
//! pictures, and fields — named in the roadmap.

#![forbid(unsafe_code)]

mod read;
mod write;

pub use read::{read, LinkFound, PictureFound, Reading, Styles, PICTURE_MARK};
pub use write::{write, Parts, PictureFile};

use wp_docx::{Document, TextPosition};

pub const MIME_TYPE: &str = "application/vnd.oasis.opendocument.text";

/// Why a package could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a ZIP, or one with no `content.xml`.
    NotOpenDocument,
    /// A part is not valid XML.
    Xml(String),
    /// The document could not be built from what was read.
    Document(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotOpenDocument => write!(f, "not an OpenDocument text"),
            Self::Xml(what) => write!(f, "a part is not valid XML: {what}"),
            Self::Document(what) => write!(f, "{what}"),
        }
    }
}

impl std::error::Error for Error {}

/// Opens a package as a document.
pub fn open(bytes: &[u8]) -> Result<Document, Error> {
    let archive = wp_zip::ZipArchive::open(bytes).map_err(|_| Error::NotOpenDocument)?;
    let part = |name: &str| -> Option<String> {
        let bytes = archive.read_by_name(name)?.ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    };
    let content = part("content.xml").ok_or(Error::NotOpenDocument)?;
    let styles = part("styles.xml");
    let meta = part("meta.xml");
    let reading = read(&content, styles.as_deref(), meta.as_deref())
        .map_err(|error| Error::Xml(error.to_string()))?;

    let mut document =
        Document::create(&reading.body).map_err(|error| Error::Document(error.to_string()))?;
    let mut pictures = reading.pictures;
    pictures
        .sort_by(|one, other| (other.paragraph, other.offset).cmp(&(one.paragraph, one.offset)));
    let mut links = reading.links;
    for picture in pictures {
        let start = TextPosition::new(picture.paragraph, picture.offset);
        let end = TextPosition::new(picture.paragraph, picture.offset + PICTURE_MARK.len_utf8());
        let before = document.paragraph_text(picture.paragraph).map_or(0, |text| text.len());
        document.set_caret(start);
        document.extend_selection_to(end);
        document.delete_selection();
        document.set_caret(start);
        if let Some(Ok(bytes)) = archive.read_by_name(&picture.name) {
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
            let _ =
                document.insert_picture(&bytes, extension, picture.width_emu, picture.height_emu);
        }
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
    if let Some((width, height, [top, right, bottom, left])) = reading.page {
        if width > 0 && height > 0 {
            document.set_page_size(width, height);
            document.set_page_margins(top, right, bottom, left);
        }
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

/// The document as a package.
///
/// The `mimetype` first and stored, as the standard requires, so that the
/// kind of file is readable at a fixed offset; then the manifest and the
/// parts.
pub fn save(document: &Document) -> Result<Vec<u8>, wp_zip::Error> {
    let title = document.properties().title;
    let parts = write(document, (!title.is_empty()).then_some(title.as_str()));
    let mut zip = wp_zip::ZipWriter::new();
    zip.add_stored("mimetype", MIME_TYPE.as_bytes())?;

    let mut manifest = String::new();
    manifest.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\n");
    manifest.push_str(&format!("<manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{MIME_TYPE}\"/>\n"));
    for name in ["content.xml", "styles.xml", "meta.xml"] {
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path=\"{name}\" manifest:media-type=\"text/xml\"/>\n"));
    }
    for picture in &parts.pictures {
        manifest.push_str(&format!(
            "<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"{}\"/>\n",
            wp_xml::escape_attribute_value(&picture.name),
            picture.media_type
        ));
    }
    manifest.push_str("</manifest:manifest>\n");
    zip.add("META-INF/manifest.xml", manifest.as_bytes())?;
    zip.add("content.xml", parts.content.as_bytes())?;
    zip.add("styles.xml", parts.styles.as_bytes())?;
    zip.add("meta.xml", parts.meta.as_bytes())?;
    for picture in &parts.pictures {
        zip.add_stored(&picture.name, &picture.bytes)?;
    }
    zip.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Alignment, Block, Body, Paragraph, RunContent, Underline};

    const CONTENT: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.3">
<office:font-face-decls><style:font-face style:name="Liberation Serif" svg:font-family="&apos;Liberation Serif&apos;"/></office:font-face-decls>
<office:automatic-styles>
<style:style style:name="P1" style:family="paragraph" style:parent-style-name="Standard"><style:paragraph-properties fo:text-align="center" fo:margin-left="1in" fo:text-indent="0.5in"/></style:style>
<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
<style:style style:name="T2" style:family="text"><style:text-properties fo:font-style="italic" style:text-underline-style="solid" fo:color="#ff0000" fo:font-size="14pt" style:font-name="Liberation Serif"/></style:style>
<style:style style:name="Table1.A" style:family="table-column"><style:table-column-properties style:column-width="2in"/></style:style>
<style:style style:name="Table1.B" style:family="table-column"><style:table-column-properties style:column-width="3in"/></style:style>
<text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="•"/></text:list-style>
<text:list-style style:name="L2"><text:list-level-style-number text:level="1" style:num-format="1"/></text:list-style>
</office:automatic-styles>
<office:body><office:text>
<text:h text:style-name="Heading_20_1" text:outline-level="1">A heading</text:h>
<text:p text:style-name="P1">Hello, <text:span text:style-name="T1">world</text:span>   — <text:span text:style-name="T2">styled</text:span><text:tab/>after <text:s text:c="2"/>spaces<text:line-break/>next line.</text:p>
<text:list text:style-name="L1"><text:list-item><text:p>Milk</text:p></text:list-item><text:list-item><text:p>Bread</text:p></text:list-item></text:list>
<text:list text:style-name="L2"><text:list-item><text:p>First</text:p></text:list-item></text:list>
<table:table table:name="Table1"><table:table-column table:style-name="Table1.A"/><table:table-column table:style-name="Table1.B"/>
<table:table-row><table:table-cell><text:p>One</text:p></table:table-cell><table:table-cell><text:p>Two</text:p></table:table-cell></table:table-row>
</table:table>
<text:p>See <text:a xlink:type="simple" xlink:href="https://example.com/">the site</text:a> and <draw:frame text:anchor-type="as-char" svg:width="1in" svg:height="0.5in"><draw:image xlink:href="Pictures/one.png"/></draw:frame> a picture.</text:p>
</office:text></office:body></office:document-content>"##;

    const STYLES: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0">
<office:styles>
<style:default-style style:family="paragraph"><style:text-properties fo:font-size="12pt"/></style:default-style>
<style:style style:name="Standard" style:family="paragraph"/>
<style:style style:name="Heading_20_1" style:display-name="Heading 1" style:family="paragraph" style:parent-style-name="Standard" style:default-outline-level="1"><style:text-properties fo:font-size="18pt" fo:font-weight="bold"/></style:style>
</office:styles>
<office:automatic-styles><style:page-layout style:name="pm1"><style:page-layout-properties fo:page-width="21.001cm" fo:page-height="29.7cm" fo:margin-top="2cm" fo:margin-bottom="2cm" fo:margin-left="2cm" fo:margin-right="2cm"/></style:page-layout></office:automatic-styles>
<office:master-styles><style:master-page style:name="Standard" style:page-layout-name="pm1"/></office:master-styles>
</office:document-styles>"##;

    #[test]
    fn a_package_reads_to_its_text_and_formatting() {
        let reading = read(CONTENT, Some(STYLES), None).expect("read");
        let blocks = &reading.body.blocks;
        assert_eq!(
            reading.body.plain_text(),
            "A heading\nHello, world — styled\tafter   spaces\nnext line.\nMilk\nBread\nFirst\nOne\tTwo\nSee the site and \u{FFFC} a picture."
        );
        let Block::Paragraph(heading) = &blocks[0] else { panic!() };
        assert_eq!(heading.properties.style.as_deref(), Some("Heading1"));
        assert_eq!(
            heading.runs[0].properties.size_half_points,
            Some(36),
            "the heading style's size"
        );
        assert_eq!(heading.runs[0].properties.bold, Some(true));
        let Block::Paragraph(hello) = &blocks[1] else { panic!() };
        assert_eq!(hello.properties.alignment, Some(Alignment::Center));
        assert_eq!(hello.properties.indent_start, Some(1440));
        assert_eq!(hello.properties.indent_first_line, Some(720));
        assert_eq!(hello.runs[0].properties.size_half_points, Some(24), "the default style's size");
        assert_eq!(hello.runs[1].plain_text(), "world");
        assert_eq!(hello.runs[1].properties.bold, Some(true));
        let styled = hello.runs.iter().find(|run| run.plain_text() == "styled").expect("styled");
        assert_eq!(styled.properties.italic, Some(true));
        assert_eq!(styled.properties.underline, Some(Underline::Single));
        assert_eq!(styled.properties.color.as_deref(), Some("FF0000"));
        assert_eq!(styled.properties.size_half_points, Some(28));
        assert_eq!(styled.properties.font.as_deref(), Some("Liberation Serif"));
        let Block::Paragraph(milk) = &blocks[2] else { panic!() };
        assert_eq!(milk.properties.numbering.map(|n| n.id), Some(wp_docx::BULLET_LIST));
        let Block::Paragraph(first) = &blocks[4] else { panic!() };
        assert_eq!(first.properties.numbering.map(|n| n.id), Some(wp_docx::NUMBERED_LIST));
        let Block::Table(table) = &blocks[5] else { panic!() };
        assert_eq!(table.grid, vec![2880, 4320]);
        assert_eq!(table.rows[0].cells[1].width, Some(4320));
        assert_eq!(reading.links.len(), 1);
        assert_eq!(reading.links[0].address, "https://example.com/");
        assert_eq!((reading.links[0].start, reading.links[0].end), (4, 12));
        assert_eq!(reading.pictures.len(), 1);
        assert_eq!(reading.pictures[0].name, "Pictures/one.png");
        assert_eq!(
            (reading.pictures[0].width_emu, reading.pictures[0].height_emu),
            (914400, 457200)
        );
        assert_eq!(reading.page, Some((11906, 16838, [1134, 1134, 1134, 1134])));
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
        let mut fancy = wp_docx::model::Run::text("café <x> & “Привет”  two spaces");
        fancy.properties.italic = Some(true);
        fancy.properties.size_half_points = Some(28);
        fancy.properties.color = Some("0000FF".to_owned());
        fancy.properties.font = Some("Arial".to_owned());
        fancy.properties.highlight = Some("yellow".to_owned());
        runs.push(fancy);
        let mut paragraph = Paragraph::from_runs(runs);
        paragraph.properties.alignment = Some(Alignment::Both);
        paragraph.properties.indent_first_line = Some(360);
        paragraph.properties.space_after = Some(200);
        body.blocks.push(Block::Paragraph(paragraph));
        for item in ["Milk", "Bread"] {
            let mut item = Paragraph::text(item);
            item.properties.numbering =
                Some(wp_docx::model::NumberingReference { id: wp_docx::BULLET_LIST, level: 0 });
            body.blocks.push(Block::Paragraph(item));
        }
        let mut numbered = Paragraph::text("First");
        numbered.properties.numbering =
            Some(wp_docx::model::NumberingReference { id: wp_docx::NUMBERED_LIST, level: 0 });
        body.blocks.push(Block::Paragraph(numbered));
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
        body.blocks.push(Block::Paragraph(Paragraph::text("A tab\there and a line\nbreak.")));
        let mut document = Document::create(&body).expect("a document");
        document.set_caret(TextPosition::new(1, 0));
        document.extend_selection_to(TextPosition::new(1, 4));
        document.add_hyperlink("https://example.com/", "");
        document.set_page_size(11906, 16838);

        let package = save(&document).expect("saved");
        assert!(package.starts_with(b"PK"));
        // The kind of file at a fixed place: stored, first, right after
        // the entry's header.
        assert_eq!(&package[38..38 + MIME_TYPE.len()], MIME_TYPE.as_bytes());

        let again = open(&package).expect("read back");
        assert_eq!(again.plain_text().trim_end(), body.plain_text());
        let back = again.body();
        let Block::Paragraph(heading) = &back.blocks[0] else { panic!() };
        assert_eq!(heading.properties.style.as_deref(), Some("Heading1"));
        let Block::Paragraph(paragraph) = &back.blocks[1] else { panic!() };
        assert_eq!(paragraph.properties.alignment, Some(Alignment::Both));
        assert_eq!(paragraph.properties.indent_first_line, Some(360));
        assert_eq!(paragraph.properties.space_after, Some(200));
        let bold =
            paragraph.runs.iter().find(|run| run.plain_text().starts_with("Bold")).expect("bold");
        assert_eq!(bold.properties.bold, Some(true));
        let fancy =
            paragraph.runs.iter().find(|run| run.plain_text().starts_with("caf")).expect("fancy");
        assert_eq!(fancy.properties.italic, Some(true));
        assert_eq!(fancy.properties.size_half_points, Some(28));
        assert_eq!(fancy.properties.color.as_deref(), Some("0000FF"));
        assert_eq!(fancy.properties.font.as_deref(), Some("Arial"));
        assert_eq!(fancy.properties.highlight.as_deref(), Some("yellow"));
        let Block::Paragraph(milk) = &back.blocks[2] else { panic!() };
        assert_eq!(milk.properties.numbering.map(|n| n.id), Some(wp_docx::BULLET_LIST));
        let Block::Paragraph(first) = &back.blocks[4] else { panic!() };
        assert_eq!(first.properties.numbering.map(|n| n.id), Some(wp_docx::NUMBERED_LIST));
        let Block::Table(table) = &back.blocks[5] else { panic!("the table was lost") };
        assert_eq!(table.grid, vec![2000, 3000]);
        assert_eq!(table.rows[0].cells[1].blocks[0].plain_text(), "Two");
        let Block::Paragraph(last) = &back.blocks[6] else { panic!() };
        assert!(last
            .runs
            .iter()
            .any(|run| run.content.iter().any(|c| matches!(c, RunContent::Tab))));
        let links = again.hyperlinks();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "Bold");
        assert_eq!(again.page_size(), (11906, 16838));
    }
}

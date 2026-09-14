//! Rich Text Format: reading what Word and everything else writes, and
//! writing it back.
//!
//! # Why RTF matters
//!
//! It is the format everything else exports to. Every word processor since
//! 1987 reads it and writes it; the clipboard carries formatted text as it;
//! a program that cannot open a `.rtf` cannot open what half the world's
//! other programs hand it. Word still writes it, and what Word writes is the
//! test: a file from Word begins with a hundred lines of groups this program
//! has no use for, and the reading has to walk past them to the text.
//!
//! # What is here
//!
//! [`read`] turns a file into the document model — paragraphs with their
//! formatting, runs with theirs, tables, lists, pictures, links — and
//! [`open`] makes a document of it. [`write`] goes the other way. The text
//! is in whatever code page the file says, read through [`wp_text`], and
//! written back as the Western one with `\u` for the rest, which is how Word
//! writes it too.
//!
//! # What is not
//!
//! Headers and footers, footnotes, sections, nested tables, drawings other
//! than pictures, metafile pictures, fields other than links, and the
//! revision marks — each named in the roadmap rather than half read here.

#![forbid(unsafe_code)]

mod lexer;
mod read;
mod write;

pub use read::{read, LinkFound, PictureFound, Reading, PICTURE_MARK};
pub use write::write;

use wp_docx::{Document, Error, TextPosition};

/// Opens an RTF file as a document: everything [`read`] found, with the
/// pictures put in where their marks were and the links laid over their
/// text.
pub fn open(bytes: &[u8]) -> Result<Document, Error> {
    let reading = read(bytes);
    let mut document = Document::create(&reading.body)?;

    // The pictures, last first, so that putting one in does not move the
    // marks of the ones after it in the same paragraph.
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
        let _ = document.insert_picture(
            &picture.bytes,
            picture.extension,
            picture.width_emu,
            picture.height_emu,
        );
        // A picture is not the width of its mark in the text, so the links
        // after it in the paragraph move by the difference.
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

    document.set_caret(TextPosition::default());
    document.clear_selection();
    let _ = document.mark_saved();
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Alignment, Block, Body, Paragraph, RunContent, Underline};

    fn text_of(body: &Body) -> String {
        body.plain_text()
    }

    #[test]
    fn plain_paragraphs_are_read() {
        let reading =
            read(b"{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Calibri;}}\\pard One\\par\\pard Two\\par}");
        assert_eq!(text_of(&reading.body), "One\nTwo");
    }

    #[test]
    fn formatting_is_read_run_by_run() {
        let reading = read(
            b"{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Calibri;}{\\f1 Arial;}}{\\colortbl ;\\red255\\green0\\blue0;}\
              \\pard\\qc\\li720\\sb120 Plain {\\b bold} {\\i\\fs32\\f1\\cf1 red italic}\\par}",
        );
        let Block::Paragraph(paragraph) = &reading.body.blocks[0] else { panic!() };
        assert_eq!(paragraph.properties.alignment, Some(Alignment::Center));
        assert_eq!(paragraph.properties.indent_start, Some(720));
        assert_eq!(paragraph.properties.space_before, Some(120));
        assert_eq!(paragraph.runs.len(), 4, "{:?}", paragraph.runs);
        assert_eq!(paragraph.runs[1].properties.bold, Some(true));
        assert_eq!(paragraph.runs[1].plain_text(), "bold");
        let last = &paragraph.runs[3];
        assert_eq!(last.properties.italic, Some(true));
        assert_eq!(last.properties.size_half_points, Some(32));
        assert_eq!(last.properties.font.as_deref(), Some("Arial"));
        assert_eq!(last.properties.color.as_deref(), Some("FF0000"));
        assert_eq!(last.plain_text(), "red italic");
        // The bold ended with its group.
        assert_eq!(paragraph.runs[2].properties.bold, None);
    }

    #[test]
    fn the_text_is_in_the_code_page_the_file_says() {
        let reading = read(b"{\\rtf1\\ansi\\ansicpg1251\\deff0{\\fonttbl{\\f0\\fcharset204 Arial;}}\\pard \\'cf\\'f0\\'e8\\'e2\\'e5\\'f2\\par}");
        assert_eq!(text_of(&reading.body), "Привет");
        // A font's charset outranks the document's page.
        let reading = read(b"{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fcharset204 Arial;}}\\pard\\f0 \\'cf\\par}");
        assert_eq!(text_of(&reading.body), "П");
    }

    #[test]
    fn unicode_is_read_and_its_stand_in_skipped() {
        let reading = read(b"{\\rtf1\\ansi\\uc1 caf\\u233? and \\u1055?\\u1088?\\'e8\\par}");
        assert_eq!(text_of(&reading.body), "café and Прè");
        let reading = read(b"{\\rtf1\\ansi\\uc2 \\u8212??x\\par}");
        assert_eq!(text_of(&reading.body), "\u{2014}x");
        let reading = read(b"{\\rtf1\\ansi caf\\u233\\'e9 \\emdash\\rquote s\\par}");
        assert_eq!(text_of(&reading.body), "café \u{2014}\u{2019}s");
    }

    #[test]
    fn unknown_groups_are_walked_past() {
        let reading = read(
            b"{\\rtf1\\ansi\\deff0{\\*\\themedata 504b0304}{\\info{\\author Nobody}}{\\*\\generator Riched20;}\
              {\\fonttbl{\\f0 Calibri;}}{\\stylesheet{\\s0 Normal;}{\\*\\cs10 Default Paragraph Font;}}\
              \\pard Text{\\*\\bkmkstart here}{\\*\\bkmkend here} here\\par}",
        );
        assert_eq!(text_of(&reading.body), "Text here");
    }

    #[test]
    fn a_table_is_rows_of_cells() {
        let reading = read(
            b"{\\rtf1\\ansi\\deff0 \\trowd\\trgaph108\\cellx1440\\cellx4320\\pard\\intbl A\\cell B\\cell\\row\
              \\trowd\\cellx1440\\cellx4320\\pard\\intbl C\\cell D\\cell\\row\\pard After\\par}",
        );
        assert_eq!(reading.body.blocks.len(), 2, "{:?}", reading.body.blocks);
        let Block::Table(table) = &reading.body.blocks[0] else { panic!("no table") };
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].cells.len(), 2);
        assert_eq!(table.rows[0].cells[0].blocks[0].plain_text(), "A");
        assert_eq!(table.rows[1].cells[1].blocks[0].plain_text(), "D");
        assert_eq!(table.rows[0].cells[0].width, Some(1440));
        assert_eq!(table.rows[0].cells[1].width, Some(2880));
        assert_eq!(table.grid, vec![1440, 2880]);
        assert_eq!(reading.body.blocks[1].plain_text(), "After");
    }

    #[test]
    fn lists_are_told_apart_by_their_first_level() {
        let reading = read(
            b"{\\rtf1\\ansi\\deff0{\\*\\listtable{\\list\\listtemplateid1{\\listlevel\\levelnfc23\\leveljc0{\\leveltext\\'01\\u-3913 ?;}{\\levelnumbers;}\\fi-360\\li720}\\listid5}\
              {\\list\\listtemplateid2{\\listlevel\\levelnfc0\\leveljc0{\\leveltext\\'02\\'00.;}{\\levelnumbers\\'01;}}\\listid6}}\
              {\\*\\listoverridetable{\\listoverride\\listid5\\listoverridecount0\\ls1}{\\listoverride\\listid6\\listoverridecount0\\ls2}}\
              \\pard{\\listtext\\'b7\\tab}\\ls1\\ilvl0 Milk\\par\\pard{\\listtext 1.\\tab}\\ls2\\ilvl1 First\\par}",
        );
        let Block::Paragraph(milk) = &reading.body.blocks[0] else { panic!() };
        let numbering = milk.properties.numbering.expect("a list");
        assert_eq!(numbering.id, wp_docx::BULLET_LIST);
        assert_eq!(milk.plain_text(), "Milk", "the list text leaked into the paragraph");
        let Block::Paragraph(first) = &reading.body.blocks[1] else { panic!() };
        let numbering = first.properties.numbering.expect("a list");
        assert_eq!(numbering.id, wp_docx::NUMBERED_LIST);
        assert_eq!(numbering.level, 1);
    }

    #[test]
    fn a_picture_leaves_a_mark_and_its_bytes() {
        // The smallest PNG there is, as Word would write it.
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n\x2d\xb4\0\0\0\0IEND\xaeB`\x82";
        let hex: String = png.iter().map(|byte| format!("{byte:02x}")).collect();
        let file = format!(
            "{{\\rtf1\\ansi\\deff0 \\pard Before {{\\*\\shppict{{\\pict\\pngblip\\picw1\\pich1\\picwgoal1440\\pichgoal720 {hex}}}}}{{\\nonshppict{{\\pict\\wmetafile8 0100}}}} after\\par}}"
        );
        let reading = read(file.as_bytes());
        assert_eq!(reading.pictures.len(), 1, "{:?}", reading.pictures.len());
        let picture = &reading.pictures[0];
        assert_eq!(picture.bytes, png);
        assert_eq!(picture.extension, "png");
        assert_eq!((picture.width_emu, picture.height_emu), (1440 * 635, 720 * 635));
        assert_eq!(picture.paragraph, 0);
        assert_eq!(picture.offset, "Before ".len());
        assert_eq!(text_of(&reading.body), format!("Before {PICTURE_MARK} after"));

        let document = open(file.as_bytes()).expect("a document");
        assert_eq!(document.plain_text().trim_end(), "Before  after");
        let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!() };
        assert!(
            paragraph
                .runs
                .iter()
                .any(|run| run.content.iter().any(|c| matches!(c, RunContent::Picture(_)))),
            "the picture was not put in: {:?}",
            paragraph.runs
        );
    }

    #[test]
    fn a_link_field_becomes_a_link() {
        let file = b"{\\rtf1\\ansi\\deff0 \\pard See {\\field{\\*\\fldinst{HYPERLINK \"https://example.com/\"}}{\\fldrslt{\\ul\\cf1 the site}}} now\\par}";
        let reading = read(file);
        assert_eq!(text_of(&reading.body), "See the site now");
        assert_eq!(reading.links.len(), 1);
        assert_eq!(reading.links[0].address, "https://example.com/");
        assert_eq!((reading.links[0].start, reading.links[0].end), (4, 12));

        let document = open(file).expect("a document");
        let links = document.hyperlinks();
        assert_eq!(links.len(), 1, "{links:?}");
        assert_eq!(links[0].text, "the site");
        assert_eq!(links[0].range, (4, 12));
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
        let mut fancy = wp_docx::model::Run::text("café \u{2014} “Привет” 😀");
        fancy.properties.italic = Some(true);
        fancy.properties.underline = Some(Underline::Double);
        fancy.properties.size_half_points = Some(28);
        fancy.properties.color = Some("0000FF".to_owned());
        fancy.properties.font = Some("Arial".to_owned());
        runs.push(fancy);
        let mut paragraph = Paragraph::from_runs(runs);
        paragraph.properties.alignment = Some(Alignment::Both);
        paragraph.properties.indent_first_line = Some(360);
        body.blocks.push(Block::Paragraph(paragraph));
        let mut item = Paragraph::text("Milk");
        item.properties.numbering =
            Some(wp_docx::model::NumberingReference { id: wp_docx::BULLET_LIST, level: 0 });
        body.blocks.push(Block::Paragraph(item));
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
        body.blocks.push(Block::Paragraph(Paragraph::text("Braces { } and a \\ slash")));

        let document = Document::create(&body).expect("a document");
        let rtf = write(&document);
        let text = String::from_utf8_lossy(&rtf);
        assert!(text.starts_with("{\\rtf1\\ansi\\ansicpg1252"), "{text}");
        assert!(text.contains("\\'e9"), "é as a Western byte: {text}");
        assert!(text.contains("\\u1055?"), "П as unicode: {text}");
        assert!(text.contains("\\u-10179?\\u-8704?"), "the emoji as two units: {text}");
        assert!(text.contains("\\emdash "), "{text}");

        let back = read(&rtf);
        assert_eq!(back.body.plain_text(), body.plain_text());
        let Block::Paragraph(heading) = &back.body.blocks[0] else { panic!() };
        assert_eq!(heading.properties.style.as_deref(), Some("Heading1"));
        let Block::Paragraph(paragraph) = &back.body.blocks[1] else { panic!() };
        assert_eq!(paragraph.properties.alignment, Some(Alignment::Both));
        assert_eq!(paragraph.properties.indent_first_line, Some(360));
        assert_eq!(paragraph.runs[0].properties.bold, Some(true));
        assert_eq!(paragraph.runs[1].properties.underline, Some(Underline::Double));
        assert_eq!(paragraph.runs[1].properties.color.as_deref(), Some("0000FF"));
        assert_eq!(paragraph.runs[1].properties.font.as_deref(), Some("Arial"));
        let Block::Paragraph(item) = &back.body.blocks[2] else { panic!() };
        assert_eq!(item.properties.numbering.map(|n| n.id), Some(wp_docx::BULLET_LIST));
        assert_eq!(item.plain_text(), "Milk");
        let Block::Table(table) = &back.body.blocks[3] else { panic!("the table was lost") };
        assert_eq!(table.rows[0].cells[1].blocks[0].plain_text(), "Two");
        assert_eq!(table.grid, vec![2000, 3000]);
    }

    #[test]
    fn a_link_and_a_picture_survive_the_round_trip() {
        let file = b"{\\rtf1\\ansi\\deff0 \\pard See {\\field{\\*\\fldinst{HYPERLINK \"https://example.com/\"}}{\\fldrslt{the site}}} now\\par}";
        let document = open(file).expect("a document");
        let rtf = write(&document);
        let again = open(&rtf).expect("read back");
        let links = again.hyperlinks();
        assert_eq!(links.len(), 1, "{}", String::from_utf8_lossy(&rtf));
        assert_eq!(links[0].text, "the site");
        assert_eq!(again.plain_text().trim_end(), "See the site now");

        // With a picture before the link, which is not the width of its mark
        // in the text: the link has to move with it.
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n\x2d\xb4\0\0\0\0IEND\xaeB`\x82";
        let hex: String = png.iter().map(|byte| format!("{byte:02x}")).collect();
        let file = format!(
            r#"{{\rtf1\ansi\deff0 \pard A picture: {{\*\shppict{{\pict\pngblip\picw1\pich1\picwgoal1440\pichgoal1440 {hex}}}}} and a {{\field{{\*\fldinst{{HYPERLINK "https://example.com/"}}}}{{\fldrslt{{\ul link}}}}}} after it.\par}}"#
        );
        let document = open(file.as_bytes()).expect("a document");
        let links = document.hyperlinks();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "link", "the link moved when the picture went in");
        let rtf = write(&document);
        assert!(String::from_utf8_lossy(&rtf).contains("\\pngblip"), "the picture was not written");
        let again = open(&rtf).expect("read back");
        let links = again.hyperlinks();
        assert_eq!(links.len(), 1, "{}", String::from_utf8_lossy(&rtf));
        assert_eq!(links[0].text, "link");
        let Block::Paragraph(paragraph) = &again.body().blocks[0] else { panic!() };
        assert!(
            paragraph
                .runs
                .iter()
                .any(|run| run.content.iter().any(|c| matches!(c, RunContent::Picture(_)))),
            "the picture did not come back"
        );
    }

    /// A file as Word 2016 writes one, cut down to its shape: the header it
    /// writes and the groups it puts before the text.
    #[test]
    fn a_file_from_word_reads_to_its_text() {
        let file = b"{\\rtf1\\adeflang1025\\ansi\\ansicpg1252\\uc1\\adeff0\\deff0\\stshfdbch0\\stshfloch31506\\stshfhich31506\\stshfbi31506\\deflang1033\\deflangfe1033\\themelang1033\\themelangfe0\\themelangcs0\
{\\fonttbl{\\f0\\fbidi \\froman\\fcharset0\\fprq2{\\*\\panose 02020603050405020304}Times New Roman;}{\\f34\\fbidi \\fswiss\\fcharset0\\fprq2{\\*\\panose 020f0502020204030204}Calibri;}\
{\\flomajor\\f31500\\fbidi \\froman\\fcharset0\\fprq2{\\*\\panose 02020603050405020304}Times New Roman;}}\
{\\colortbl;\\red0\\green0\\blue0;\\red0\\green0\\blue255;\\red0\\green255\\blue255;}\
{\\*\\defchp \\fs22\\loch\\af31506\\hich\\af31506\\dbch\\af31505}{\\*\\defpap \\ql \\li0\\ri0\\sa160\\sl259\\slmult1\\widctlpar\\wrapdefault\\aspalpha\\aspnum\\faauto\\adjustright\\rin0\\lin0\\itap0 }\
{\\stylesheet{\\ql \\li0\\ri0\\sa160\\sl259\\slmult1\\widctlpar\\wrapdefault\\aspalpha\\aspnum\\faauto\\adjustright\\rin0\\lin0\\itap0 \\rtlch\\fcs1 \\af31507\\afs22\\alang1025 \\ltrch\\fcs0 \\fs22\\lang1033\\langfe1033\\loch\\f31506\\hich\\af31506\\dbch\\af31505\\cgrid\\langnp1033\\langfenp1033 \\snext0 \\sqformat \\spriority0 Normal;}\
{\\s1\\ql \\li0\\ri0\\sb240\\sa0\\keep\\keepn\\widctlpar\\wrapdefault\\aspalpha\\aspnum\\faauto\\outlinelevel0\\adjustright\\rin0\\lin0\\itap0 \\rtlch\\fcs1 \\af31503\\afs32\\alang1025 \\ltrch\\fcs0 \\fs32\\cf17\\lang1033\\langfe1033\\loch\\f31502\\hich\\af31502\\dbch\\af31501\\cgrid\\langnp1033\\langfenp1033 \\sbasedon0 \\snext0 \\slink15 \\sqformat \\spriority9 \\styrsid15676416 heading 1;}\
{\\*\\cs10 \\additive \\ssemihidden \\sunhideused \\spriority1 Default Paragraph Font;}}\
{\\*\\rsidtbl \\rsid15676416}{\\mmathPr\\mmathFont34\\mbrkBin0\\mbrkBinSub0\\msmallFrac0\\mdispDef1\\mlMargin0\\mrMargin0\\mdefJc1\\mwrapIndent1440\\mintLim0\\mnaryLim1}\
{\\info{\\author Somebody}{\\operator Somebody}{\\creatim\\yr2024\\mo1\\dy1\\hr9\\min0}{\\version1}{\\edmins0}{\\nofpages1}{\\nofwords4}{\\nofchars25}{\\nofcharsws28}{\\vern83}}\
{\\*\\xmlnstbl {\\xmlns1 http://schemas.microsoft.com/office/word/2003/wordml}}\
\\paperw12240\\paperh15840\\margl1440\\margr1440\\margt1440\\margb1440\\gutter0\\ltrsect \\widowctrl\\ftnbj\\aenddoc\\trackmoves0\\trackformatting1\\donotembedsysfont1\\relyonvml0\\donotembedlingdata0\\grfdocevents0\\validatexml1\\showplaceholdtext0\\ignoremixedcontent0\\saveinvalidxml0\\showxmlerrors1\
\\fet0{\\*\\wgrffmtfilter 2450}\\nofeaturethrottle1\\ilfomacatclnup0{\\*\\ftnsep \\ltrpar \\pard\\plain \\ltrpar\\ql \\li0\\ri0\\sa160\\sl259\\slmult1\\widctlpar\\wrapdefault\\aspalpha\\aspnum\\faauto\\adjustright\\rin0\\lin0\\itap0 \\rtlch\\fcs1 \\af31507\\afs22\\alang1025 \\ltrch\\fcs0 \\fs22\\lang1033\\langfe1033\\loch\\af31506\\hich\\af31506\\dbch\\af31505\\cgrid\\langnp1033\\langfenp1033 {\\rtlch\\fcs1 \\af31507 \\ltrch\\fcs0 \\insrsid15676416 \\chftnsep \\par }}\
\\ltrpar \\sectd \\ltrsect\\linex0\\endnhere\\sectlinegrid360\\sectdefaultcl\\sftnbj {\\*\\pnseclvl1\\pnucrm\\pnstart1\\pnindent720\\pnhang {\\pntxta .}}\
\\pard\\plain \\ltrpar\\s1\\ql \\li0\\ri0\\sb240\\sa0\\keep\\keepn\\widctlpar\\wrapdefault\\aspalpha\\aspnum\\faauto\\outlinelevel0\\adjustright\\rin0\\lin0\\itap0 \\rtlch\\fcs1 \\af31503\\afs32\\alang1025 \\ltrch\\fcs0 \\fs32\\cf17\\lang1033\\langfe1033\\loch\\af31502\\hich\\af31502\\dbch\\af31501\\cgrid\\langnp1033\\langfenp1033 \
{\\rtlch\\fcs1 \\af31503 \\ltrch\\fcs0 \\insrsid15676416 \\hich\\af31502\\dbch\\af31501\\loch\\f31502 A heading}{\\rtlch\\fcs1 \\af31503 \\ltrch\\fcs0 \\insrsid15676416 \\par }\
\\pard\\plain \\ltrpar\\ql \\li0\\ri0\\sa160\\sl259\\slmult1\\widctlpar\\wrapdefault\\aspalpha\\aspnum\\faauto\\adjustright\\rin0\\lin0\\itap0 \\rtlch\\fcs1 \\af31507\\afs22\\alang1025 \\ltrch\\fcs0 \\fs22\\lang1033\\langfe1033\\loch\\af31506\\hich\\af31506\\dbch\\af31505\\cgrid\\langnp1033\\langfenp1033 \
{\\rtlch\\fcs1 \\af31507 \\ltrch\\fcs0 \\insrsid15676416 \\hich\\af31506\\dbch\\af31505\\loch\\f31506 Hello, }{\\rtlch\\fcs1 \\ab\\af31507 \\ltrch\\fcs0 \\b\\insrsid15676416 \\hich\\af31506\\dbch\\af31505\\loch\\f31506 world}{\\rtlch\\fcs1 \\af31507 \\ltrch\\fcs0 \\insrsid15676416 \\hich\\af31506\\dbch\\af31505\\loch\\f31506  \\hich\\f31506 \\'96 caf\\'e9\\par }}";
        let reading = read(file);
        assert_eq!(text_of(&reading.body), "A heading\nHello, world \u{2013} café");
        let Block::Paragraph(heading) = &reading.body.blocks[0] else { panic!() };
        assert_eq!(heading.properties.style.as_deref(), Some("Heading1"));
        assert_eq!(heading.properties.keep_next, Some(true));
        let Block::Paragraph(second) = &reading.body.blocks[1] else { panic!() };
        let bold: Vec<&str> = second
            .runs
            .iter()
            .filter(|run| run.properties.bold == Some(true))
            .map(|run| {
                run.content
                    .iter()
                    .map(|c| match c {
                        RunContent::Text(t) => t.as_str(),
                        _ => "",
                    })
                    .collect::<Vec<_>>()
                    .concat()
            })
            .map(|_| "world")
            .collect();
        assert_eq!(bold, vec!["world"]);
        assert_eq!(second.properties.space_after, Some(160));
        assert_eq!(
            second.properties.line_spacing.map(|s| (s.value, s.rule)),
            Some((259, wp_docx::model::LineRule::Auto))
        );
    }
}

//! Web pages the way Word writes them, read here.
//!
//! What Word puts in a page that a browser does not need: the notes and the
//! comments at the end in divisions of their own, the drawings in VML inside
//! conditional comments with plainer copies after them, the table styles in
//! a style block only Office reads, the fonts as `@font-face` rules, and the
//! sections as `@page` rules naming the file their headers and footers are
//! in. Each page here is cut down to its shape from what Word writes, with
//! the copies for other readers left in where Word puts them.

use wp_docx::anchor::{Placement, Relative, Wrap};
use wp_docx::colour::Colour;
use wp_docx::fills::Fill;
use wp_docx::fonts::FontClass;
use wp_docx::furniture::{Furniture, Which};
use wp_docx::model::{Alignment, Block, Body, Paragraph, RunContent, Table};
use wp_docx::notes::Kind;
use wp_docx::table_properties::CellAlignment;
use wp_docx::{Document, StyleKind, TextPosition};

/// A picture as small as a PNG can be.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n\x2d\xb4\0\0\0\0IEND\xaeB`\x82";

/// The head Word writes, with the rules the pages below use.
const HEAD: &str = r##"<html xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:w="urn:schemas-microsoft-com:office:word" xmlns:m="http://schemas.microsoft.com/office/2004/12/omml" xmlns="http://www.w3.org/TR/REC-html40">
<head>
<meta http-equiv=Content-Type content="text/html; charset=utf-8">
<meta name=ProgId content=Word.Document>
<meta name=Generator content="Microsoft Word 15">
<link rel=File-List href="page_files/filelist.xml">
<!--[if gte mso 9]><xml>
 <o:DocumentProperties>
  <o:Author>Kim Smith</o:Author>
 </o:DocumentProperties>
</xml><![endif]-->
<style>
<!--
 /* Font Definitions */
 @font-face
	{font-family:"Cambria Math";
	panose-1:2 4 5 3 5 4 6 3 2 4;
	mso-font-charset:0;
	mso-generic-font-family:roman;
	mso-font-pitch:variable;}
@font-face
	{font-family:"Calibri Light";
	panose-1:2 15 3 2 2 2 4 3 2 4;
	mso-font-alt:"Calibri Light";
	mso-font-charset:0;
	mso-generic-font-family:swiss;
	mso-font-pitch:variable;}
@font-face
	{font-family:"Old Typewriter";
	mso-font-alt:Courier;
	mso-font-charset:0;
	mso-generic-font-family:modern;
	mso-font-pitch:fixed;}
 /* Style Definitions */
 p.MsoNormal, li.MsoNormal, div.MsoNormal
	{mso-style-unhide:no;
	mso-style-qformat:yes;
	mso-style-parent:"";
	margin-top:0in;
	margin-right:0in;
	margin-bottom:8.0pt;
	margin-left:0in;
	line-height:107%;
	font-size:11.0pt;
	font-family:"Calibri",sans-serif;}
h1
	{mso-style-priority:9;
	mso-style-qformat:yes;
	mso-style-link:"Heading 1 Char";
	mso-style-next:Normal;
	margin-top:12.0pt;
	margin-bottom:0in;
	page-break-after:avoid;
	font-size:16.0pt;
	font-family:"Calibri Light",sans-serif;
	color:#2F5496;
	font-weight:normal;}
p.MsoQuote, li.MsoQuote, div.MsoQuote
	{mso-style-priority:29;
	mso-style-qformat:yes;
	mso-style-link:"Quote Char";
	mso-style-next:Normal;
	margin-left:.6in;
	text-align:center;
	font-size:11.0pt;
	font-style:italic;
	color:#404040;}
p.Letterhead, li.Letterhead, div.Letterhead
	{mso-style-name:Letterhead;
	mso-style-parent:"";
	mso-style-next:Normal;
	text-align:right;
	font-size:9.0pt;
	color:#1F3864;}
span.QuoteChar
	{mso-style-name:"Quote Char";
	mso-style-priority:29;
	mso-style-unhide:no;
	mso-style-link:Quote;
	font-style:italic;
	color:#404040;}
span.Loud
	{mso-style-name:"Loud Red";
	mso-style-unhide:no;
	font-weight:bold;
	color:red;}
@page WordSection1
	{size:8.5in 11.0in;
	margin:1.0in 1.25in 1.0in 1.25in;
	mso-header-margin:.5in;
	mso-footer-margin:.4in;
	mso-header:url("page_files/header.htm") h1;
	mso-footer:url("page_files/header.htm") f1;
	mso-first-header:url("page_files/header.htm") fh1;
	mso-title-page:yes;
	mso-paper-source:0;}
div.WordSection1
	{page:WordSection1;}
@page WordSection2
	{size:11.0in 8.5in;
	mso-page-orientation:landscape;
	margin:.75in .75in .75in .75in;
	mso-header-margin:.5in;
	mso-footer-margin:.5in;
	mso-columns:2 even .5in;
	mso-paper-source:0;}
div.WordSection2
	{page:WordSection2;}
 /* List Definitions */
 @list l0
	{mso-list-id:1234;
	mso-list-type:hybrid;}
-->
</style>
<!--[if gte mso 10]>
<style>
 /* Style Definitions */
 table.MsoNormalTable
	{mso-style-name:"Table Normal";
	mso-tstyle-rowband-size:0;
	mso-tstyle-colband-size:0;
	mso-style-noshow:yes;
	mso-style-parent:"";
	mso-padding-alt:0in 5.4pt 0in 5.4pt;
	font-size:11.0pt;
	font-family:"Calibri",sans-serif;}
table.MsoTableGrid
	{mso-style-name:"Table Grid";
	mso-tstyle-rowband-size:0;
	mso-tstyle-colband-size:0;
	mso-style-priority:39;
	mso-style-unhide:no;
	border:solid windowtext 1.0pt;
	mso-border-alt:solid windowtext .5pt;
	mso-padding-alt:0in 5.4pt 0in 5.4pt;
	mso-border-insideh:.5pt solid windowtext;
	mso-border-insidev:.5pt solid windowtext;
	font-size:11.0pt;
	font-family:"Calibri",sans-serif;}
</style>
<![endif]--><!--[if gte mso 9]><xml>
 <o:shapedefaults v:ext="edit" spidmax="1029"/>
</xml><![endif]-->
</head>
"##;

fn page(body: &str) -> String {
    format!("{HEAD}<body bgcolor=\"#FDE9D9\" lang=EN-US style='tab-interval:.5in'>\n{body}\n</body>\n</html>\n")
}

/// Notes and a comment, as Word's Web Page writes them.
fn notes_and_comments() -> String {
    page(
        r##"<div class=WordSection1>
<h1><a name="chapter">Chapter one</a><o:p></o:p></h1>
<p class=MsoNormal>Words with a note<a style='mso-footnote-id:ftn1' href="#_ftn1" name="_ftnref1" title=""><span class=MsoFootnoteReference><span style='mso-special-character:footnote'><![if !supportFootnotes]><span class=MsoFootnoteReference><span style='font-size:11.0pt;line-height:107%;font-family:"Calibri",sans-serif'>[1]</span></span><![endif]></span></span></a> and an end<a style='mso-endnote-id:edn1' href="#_edn1" name="_ednref1" title=""><span class=MsoEndnoteReference><span style='mso-special-character:footnote'><![if !supportFootnotes]><span class=MsoEndnoteReference><span style='font-size:11.0pt'>[i]</span></span><![endif]></span></span></a>.<o:p></o:p></p>
<p class=MsoNormal>Some <a style='mso-comment-reference:KS_1;mso-comment-date:20240305T1030'>noted</a><span class=MsoCommentReference><span style='font-size:8.0pt;line-height:107%'><a style='mso-comment-reference:KS_1;mso-comment-date:20240305T1030'>&nbsp;</a><span style='mso-special-character:comment'>&nbsp;<![if !supportAnnotations]><a href="#_msocom_1" language=JavaScript id="_anchor_1" onmouseover="msoCommentShow('_anchor_1','_com_1')" onmouseout="msoCommentHide('_com_1')" class=msocomanchor name="_msoanchor_1">[KS1]</a><![endif]></span></span></span> text, and <a href="#chapter">back to the chapter</a>.<o:p></o:p></p>
<p class=MsoNormal><![if !supportEmptyParas]>&nbsp;<![endif]><o:p></o:p></p>
</div>
<div style='mso-element:comment-list'><![if !supportAnnotations]>
<hr class=msocomoff align=left size=1 width="33%">
<![endif]>
<div style='mso-element:comment'><![if !supportAnnotations]>
<div id="_com_1" class=msocomtxt language=JavaScript onmouseover="msoCommentShow('_anchor_1','_com_1')" onmouseout="msoCommentHide('_com_1')"><![endif]>
<div><![if !supportAnnotations]><a name="_msocom_1"></a><![endif]><span style='mso-comment-author:"Kim Smith"'></span>
<p class=MsoCommentText><span class=MsoCommentReference><span style='font-size:8.0pt'><span style='mso-special-character:comment'>&nbsp;</span></span></span>Comment <b>words</b>.<o:p></o:p></p>
</div>
<![if !supportAnnotations]></div>
<![endif]></div>
</div>
<div style='mso-element:footnote-list'><![if !supportFootnotes]><br clear=all>
<hr align=left size=1 width="33%">
<![endif]>
<div style='mso-element:footnote' id=ftn1>
<p class=MsoFootnoteText><a style='mso-footnote-id:ftn1' href="#_ftnref1" name="_ftn1" title=""><span class=MsoFootnoteReference><span style='mso-special-character:footnote'><![if !supportFootnotes]><span class=MsoFootnoteReference><span style='font-size:10.0pt'>[1]</span></span><![endif]></span></span></a> The note, <b>bold</b> in part.<o:p></o:p></p>
</div>
</div>
<div style='mso-element:endnote-list'><![if !supportEndnotes]><br clear=all>
<hr align=left size=1 width="33%">
<![endif]>
<div style='mso-element:endnote' id=edn1>
<p class=MsoEndnoteText><a style='mso-endnote-id:edn1' href="#_ednref1" name="_edn1" title=""><span class=MsoEndnoteReference><span style='mso-special-character:footnote'><![if !supportFootnotes]><span class=MsoEndnoteReference>[i]</span><![endif]></span></span></a> The endnote.<o:p></o:p></p>
</div>
</div>"##,
    )
}

fn paragraph_of(document: &Document, text: &str) -> usize {
    (0..document.paragraph_count())
        .find(|index| document.paragraph_text(*index).is_some_and(|found| found == text))
        .unwrap_or_else(|| {
            let all: Vec<String> = (0..document.paragraph_count())
                .filter_map(|index| document.paragraph_text(index))
                .collect();
            panic!("no paragraph {text:?} in {all:?}")
        })
}

fn paragraph<'a>(body: &'a Body, text: &str) -> &'a Paragraph {
    body.paragraphs()
        .into_iter()
        .find(|paragraph| paragraph.plain_text() == text)
        .unwrap_or_else(|| panic!("no paragraph {text:?} in {:?}", body.plain_text()))
}

fn first_table(blocks: &[Block]) -> &Table {
    blocks
        .iter()
        .find_map(|block| match block {
            Block::Table(table) => Some(&**table),
            Block::Paragraph(_) => None,
        })
        .unwrap_or_else(|| panic!("no table in {blocks:?}"))
}

#[test]
fn notes_comments_and_bookmarks_go_where_they_were() {
    let document = wp_html::open_html(notes_and_comments().as_bytes(), None).expect("opened");
    let words = paragraph_of(&document, "Words with a note\u{2} and an end\u{2}.");

    let footnotes = document.notes(Kind::Footnote);
    assert_eq!(footnotes.len(), 1, "{footnotes:?}");
    assert_eq!(footnotes[0].text, "The note, bold in part.");
    assert_eq!(footnotes[0].mark, Some(TextPosition::new(words, "Words with a note".len())));
    let body = document.note_body(Kind::Footnote, footnotes[0].id).expect("its words");
    let bold = body.paragraphs()[0].runs.iter().find(|run| run.plain_text() == "bold").cloned();
    assert_eq!(bold.and_then(|run| run.properties.bold), Some(true));
    let endnotes = document.notes(Kind::Endnote);
    assert_eq!(endnotes.len(), 1);
    assert_eq!(endnotes[0].text, "The endnote.");

    let comments = document.comments();
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert_eq!(comments[0].author, "Kim Smith");
    assert_eq!(comments[0].date, "2024-03-05T10:30:00Z");
    assert_eq!(comments[0].text, "Comment words.");
    let noted = paragraph_of(&document, "Some noted text, and back to the chapter.");
    assert_eq!(
        comments[0].range,
        Some((TextPosition::new(noted, 5), TextPosition::new(noted, 10)))
    );

    assert_eq!(document.bookmark_text("chapter").as_deref(), Some("Chapter one"));
    let links = document.hyperlinks();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0].destination, wp_docx::links::Destination::Place("chapter".to_owned()));

    // Nothing written only for other readers came through: no "[1]", no
    // "[KS1]", no rule over the notes, no empty paragraph's space.
    let text = document.plain_text();
    assert!(!text.contains('[') && !text.contains('\u{a0}'), "{text:?}");
    assert_eq!(document.page_color().as_deref(), Some("FDE9D9"));
}

#[test]
fn styles_of_every_kind_and_the_fonts_are_defined() {
    let rtl = format!(
        r##"<p class=MsoNormal dir=RTL style='text-align:right;direction:rtl;unicode-bidi:embed'><span lang=AR-SA dir=RTL>{}</span><span dir=LTR> and more</span><o:p></o:p></p>"##,
        "\u{645}\u{631}"
    );
    let html = page(&format!(
        r##"<div class=WordSection1>
<p class=MsoQuote>A quote, <span class=QuoteChar>quoted</span> and <span class=Loud>loud</span>.<o:p></o:p></p>
<p class=Letterhead>Kim Smith, Somewhere<o:p></o:p></p>
<p class=MsoNormal style='font-family:"Old Typewriter"'>Typed.<o:p></o:p></p>
{rtl}
<div style='mso-element:para-border-div;border:solid windowtext 1.0pt;mso-border-alt:solid windowtext .5pt;padding:1.0pt 4.0pt 1.0pt 4.0pt;background:#FFF2CC'>
<p class=MsoNormal style='border:none;mso-border-alt:solid windowtext .5pt;padding:0in;mso-padding-alt:1.0pt 4.0pt 1.0pt 4.0pt;background:#FFF2CC'>Boxed one.<o:p></o:p></p>
<p class=MsoNormal style='border:none;mso-border-alt:solid windowtext .5pt;padding:0in;mso-padding-alt:1.0pt 4.0pt 1.0pt 4.0pt;background:#FFF2CC'>Boxed two.<o:p></o:p></p>
</div>
</div>"##
    ));
    let document = wp_html::open_html(html.as_bytes(), None).expect("opened");
    let styles = document.styles();

    let quote = styles.get("Quote").cloned().expect("the quote style");
    assert_eq!(quote.kind, StyleKind::Paragraph);
    assert_eq!(quote.next.as_deref(), Some("Normal"));
    assert_eq!(quote.run.italic, Some(true));
    assert_eq!(quote.paragraph.alignment, Some(Alignment::Center));
    let letterhead = styles.get("Letterhead").cloned().expect("a style of the document's own");
    assert_eq!(letterhead.name.as_deref(), Some("Letterhead"));
    assert_eq!(letterhead.run.size_half_points, Some(18));
    let quote_char = styles.get("QuoteChar").cloned().expect("the character style");
    assert_eq!(quote_char.kind, StyleKind::Character);
    assert_eq!(quote_char.name.as_deref(), Some("Quote Char"));
    let loud = styles.get("Loud").cloned().expect("a character style of its own");
    assert_eq!(loud.run.bold, Some(true));
    assert_eq!(loud.run.color.as_deref(), Some("FF0000"));
    let grid =
        styles.get("TableGrid").cloned().expect("the table style from the block only Office reads");
    assert_eq!(grid.kind, StyleKind::Table);
    let heading = styles.get("Heading1").cloned().expect("the heading");
    assert_eq!(heading.run.size_half_points, Some(32));
    assert_eq!(heading.run.color.as_deref(), Some("2F5496"));

    let body = document.body();
    let quoted = paragraph(&body, "A quote, quoted and loud.");
    assert_eq!(quoted.properties.style.as_deref(), Some("Quote"));
    let run = |text: &str| {
        quoted.runs.iter().find(|run| run.plain_text() == text).cloned().expect("the run")
    };
    assert_eq!(run("quoted").properties.style.as_deref(), Some("QuoteChar"));
    assert_eq!(run("loud").properties.style.as_deref(), Some("Loud"));
    assert_eq!(
        paragraph(&body, "Kim Smith, Somewhere").properties.style.as_deref(),
        Some("Letterhead")
    );

    // The fonts, as the table the document keeps of them.
    let fonts = document.font_table();
    let font = |name: &str| fonts.iter().find(|font| font.name == name).cloned().expect("the font");
    assert_eq!(font("Cambria Math").class, FontClass::Roman);
    assert_eq!(font("Cambria Math").panose.as_deref(), Some("02040503050406030204"));
    assert_eq!(font("Old Typewriter").alt_name.as_deref(), Some("Courier"));
    assert_eq!(font("Old Typewriter").fixed_pitch, Some(true));

    // Right to left: the paragraph, its first run, and the page's right as
    // the side its lines start from.
    let arabic = paragraph(&body, "\u{645}\u{631} and more");
    assert_eq!(arabic.properties.right_to_left, Some(true));
    assert_eq!(arabic.properties.alignment, Some(Alignment::Start));
    assert_eq!(arabic.runs[0].properties.right_to_left, Some(true));
    assert_eq!(arabic.runs[0].properties.language.as_deref(), Some("ar-SA"));
    assert_eq!(arabic.runs[1].properties.right_to_left, None);

    // Word's box round several paragraphs is each of them boxed alike.
    for text in ["Boxed one.", "Boxed two."] {
        let boxed = paragraph(&body, text);
        let top = boxed.properties.borders.top.as_ref().expect("a line above");
        assert_eq!((top.style.as_str(), top.size), ("single", 4));
        assert!(boxed.properties.borders.start.is_some() && boxed.properties.borders.end.is_some());
        assert_eq!(boxed.properties.shading.as_deref(), Some("FFF2CC"));
    }
}

/// A table as Word writes one: a style, a cell across two columns, one down
/// two rows, shaded and aligned cells, and a table inside a cell.
fn word_table() -> String {
    page(
        r##"<div class=WordSection1>
<table class=MsoTableGrid border=1 cellspacing=0 cellpadding=0 style='border-collapse:collapse;border:none;mso-border-alt:solid windowtext .5pt;mso-yfti-tbllook:1184;mso-padding-alt:0in 5.4pt 0in 5.4pt'>
 <tr style='mso-yfti-irow:0;mso-yfti-firstrow:yes'>
  <td width=312 colspan=2 valign=top style='width:3.25in;border:solid windowtext 1.0pt;mso-border-alt:solid windowtext .5pt;padding:0in 5.4pt 0in 5.4pt'>
  <p class=MsoNormal>Across two<o:p></o:p></p>
  </td>
  <td width=156 valign=top style='width:1.625in;border:solid windowtext 1.0pt;border-left:none;mso-border-left-alt:solid windowtext .5pt;mso-border-alt:solid windowtext .5pt;background:#D9E2F3;padding:0in 5.4pt 0in 5.4pt'>
  <p class=MsoNormal>Shaded<o:p></o:p></p>
  </td>
 </tr>
 <tr style='mso-yfti-irow:1'>
  <td width=156 rowspan=2 valign=middle style='width:1.625in;border:solid windowtext 1.0pt;border-top:none;mso-border-top-alt:solid windowtext .5pt;mso-border-alt:solid windowtext .5pt;padding:0in 5.4pt 0in 5.4pt'>
  <p class=MsoNormal>Down two<o:p></o:p></p>
  </td>
  <td width=156 valign=top style='width:1.625in;border-top:none;border-left:none;border-bottom:solid windowtext 1.0pt;border-right:solid windowtext 1.0pt;mso-border-alt:solid windowtext .5pt;padding:0in 5.4pt 0in 5.4pt'>
  <p class=MsoNormal>B<o:p></o:p></p>
  </td>
  <td width=156 valign=top style='width:1.625in;border-top:none;border-left:none;border-bottom:solid windowtext 1.0pt;border-right:solid windowtext 1.0pt;mso-border-alt:solid windowtext .5pt;padding:0in 5.4pt 0in 5.4pt'>
  <table class=MsoTableGrid border=1 cellspacing=0 cellpadding=0 style='border-collapse:collapse;border:none'>
   <tr>
    <td width=60 style='width:45.0pt;border:solid windowtext 1.0pt;padding:0in 5.4pt 0in 5.4pt'><p class=MsoNormal>In A<o:p></o:p></p></td>
    <td width=60 style='width:45.0pt;border:solid windowtext 1.0pt;border-left:none;padding:0in 5.4pt 0in 5.4pt'><p class=MsoNormal>In B<o:p></o:p></p></td>
   </tr>
  </table>
  <p class=MsoNormal><o:p>&nbsp;</o:p></p>
  </td>
 </tr>
 <tr style='mso-yfti-irow:2;mso-yfti-lastrow:yes'>
  <td width=156 valign=top style='width:1.625in;border-top:none;border-left:none;border-bottom:solid windowtext 1.0pt;border-right:solid windowtext 1.0pt;padding:0in 5.4pt 0in 5.4pt'>
  <p class=MsoNormal>C<o:p></o:p></p>
  </td>
  <td width=156 valign=top style='width:1.625in;border-top:none;border-left:none;border-bottom:solid windowtext 1.0pt;border-right:solid windowtext 1.0pt;padding:0in 5.4pt 0in 5.4pt'>
  <p class=MsoNormal>D<o:p></o:p></p>
  </td>
 </tr>
</table>
<p class=MsoNormal>After.<o:p></o:p></p>
</div>"##,
    )
}

#[test]
fn a_word_table_keeps_its_merges_lines_shading_style_and_inner_table() {
    let document = wp_html::open_html(word_table().as_bytes(), None).expect("opened");
    let body = document.body();
    let table = first_table(&body.blocks);
    assert_eq!(table.style.as_deref(), Some("TableGrid"));
    assert_eq!(table.grid, vec![2340, 2340, 2340], "{:?}", table.grid);
    assert_eq!(table.rows.len(), 3);

    let across = &table.rows[0].cells[0];
    assert_eq!((across.blocks[0].plain_text().as_str(), across.span), ("Across two", 2));
    let shaded = &table.rows[0].cells[1];
    assert_eq!(shaded.shading.as_deref(), Some("D9E2F3"));
    // The precise line Word keeps for Office, not the one rounded for
    // browsers.
    assert_eq!(shaded.borders.top.as_ref().map(|line| line.size), Some(4));

    let down = &table.rows[1].cells[0];
    assert_eq!(down.blocks[0].plain_text(), "Down two");
    assert_eq!(down.vertical, CellAlignment::Middle);
    assert!(!down.merged_upwards);
    assert!(table.rows[2].cells[0].merged_upwards, "the cell under it does not carry on from it");
    assert_eq!(table.rows[2].cells[1].blocks[0].plain_text(), "C");
    assert_eq!(table.rows[2].cells.len(), 3);

    let holder = &table.rows[1].cells[2].blocks;
    let inner = first_table(holder);
    assert_eq!(inner.rows[0].cells[1].blocks[0].plain_text(), "In B");
    assert!(matches!(holder.last(), Some(Block::Paragraph(_))));
    assert_eq!(paragraph(&body, "After.").plain_text(), "After.");
}

/// The page with two sections, whose headers and footers are in Word's
/// file beside it, and drawings — all as one file, which is Word's Single
/// File Web Page.
fn single_file() -> Vec<u8> {
    let body = r##"<div class=WordSection1>
<p class=MsoNormal><!--[if gte vml 1]><v:shapetype id="_x0000_t202" coordsize="21600,21600" o:spt="202" path="m,l,21600r21600,l21600,xe"><v:stroke joinstyle="miter"/><v:path gradientshapeok="t" o:connecttype="rect"/></v:shapetype><v:shape id="Text_x0020_Box_x0020_2" o:spid="_x0000_s1026" type="#_x0000_t202" style='position:absolute;margin-left:216pt;margin-top:7.5pt;width:144pt;height:72pt;z-index:251659264;visibility:visible;mso-wrap-style:square;mso-position-horizontal:absolute;mso-position-horizontal-relative:text;mso-position-vertical:absolute;mso-position-vertical-relative:text;v-text-anchor:top' fillcolor="#fff2cc" strokeweight=".5pt"><v:textbox><![if !mso]><table cellpadding=0 cellspacing=0 width="100%"><tr><td><![endif]><div><p class=MsoNormal><b>Inside</b> the box.<o:p></o:p></p></div><![if !mso]></td></tr></table><![endif]></v:textbox><w:wrap type="square"/></v:shape><![endif]--><![if !vml]><span style='mso-ignore:vglayout;position:absolute;z-index:251659264;margin-left:288px;margin-top:10px;width:194px;height:98px'><table cellpadding=0 cellspacing=0><tr><td width=194 height=98 bgcolor=white style='border:.75pt solid black;vertical-align:top;background:white'><![endif]><![if !mso]><span style='position:absolute;mso-ignore:vglayout;left:0pt;z-index:251659264'><table cellpadding=0 cellspacing=0 width="100%"><tr><td><![endif]><div v:shape="Text_x0020_Box_x0020_2" style='padding:3.6pt 7.2pt 3.6pt 7.2pt' class=shape><p class=MsoNormal><b>Inside</b> the box.<o:p></o:p></p></div><![if !mso]></td></tr></table></span><![endif]><![if !vml]></td></tr></table></span><![endif]>Beside the box: <!--[if gte vml 1]><v:oval id="Oval_x0020_3" o:spid="_x0000_s1027" style='position:absolute;margin-left:0;margin-top:0;width:72pt;height:36pt;z-index:-251655168;mso-position-horizontal:center;mso-position-horizontal-relative:margin;mso-position-vertical-relative:text' fillcolor="red" strokecolor="#1f3763 [1604]" strokeweight="1pt"><v:stroke joinstyle="miter"/></v:oval><![endif]--><![if !vml]><span style='mso-ignore:vglayout;position:absolute;z-index:-1895825408'><img width=98 height=50 src="page_files/image003.png" v:shapes="Oval_x0020_3"></span><![endif]>and a picture: <!--[if gte vml 1]><v:shapetype id="_x0000_t75" coordsize="21600,21600" o:spt="75" o:preferrelative="t" path="m@4@5l@4@11@9@11@9@5xe" filled="f" stroked="f"><v:stroke joinstyle="miter"/></v:shapetype><v:shape id="Picture_x0020_1" o:spid="_x0000_i1025" type="#_x0000_t75" style='width:72pt;height:36pt;visibility:visible;mso-wrap-style:square'><v:imagedata src="page_files/image001.png" o:title=""/></v:shape><![endif]--><![if !vml]><img width=96 height=48 src="page_files/image002.png" v:shapes="Picture_x0020_1"><![endif]><o:p></o:p></p>
<p class=MsoNormal>Last of the first section.<o:p></o:p></p>
<span style='font-size:11.0pt;line-height:107%;font-family:"Calibri",sans-serif'><br clear=all style='page-break-before:always;mso-break-type:section-break'></span>
</div>
<div class=WordSection2>
<p class=MsoNormal>The second section.<o:p></o:p></p>
</div>"##;
    let header = r##"<html xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:w="urn:schemas-microsoft-com:office:word" xmlns="http://www.w3.org/TR/REC-html40">
<head>
<meta http-equiv=Content-Type content="text/html; charset=utf-8">
<link id=Main-File rel=Main-File href="../page.htm">
</head>
<body lang=EN-US>
<div style='mso-element:footnote-separator' id=fs>
<p class=MsoNormal><span style='mso-special-character:footnote-separator'><![if !supportFootnotes]>
<hr align=left size=1 width="33%">
<![endif]></span></p>
</div>
<div style='mso-element:header' id=h1>
<p class=MsoHeader>Running head <!--[if gte vml 1]><v:shape id="Logo" type="#_x0000_t75" style='width:18pt;height:12pt'><v:imagedata src="image001.png" o:title=""/></v:shape><![endif]--><![if !vml]><img width=24 height=16 src="image001.png" v:shapes="Logo"><![endif]><o:p></o:p></p>
</div>
<div style='mso-element:footer' id=f1>
<p class=MsoFooter align=center style='text-align:center'>Page footer<o:p></o:p></p>
</div>
<div style='mso-element:header' id=fh1>
<p class=MsoHeader>First page head<o:p></o:p></p>
</div>
</body>
</html>"##;
    let base64 = |bytes: &[u8]| wp_html::mime::encode_base64(bytes);
    let mut message = String::from(
        "MIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=\"----=_NextPart_01DA0000.00000000\"\r\n\r\n",
    );
    let part = |location: &str, kind: &str, body: &str| {
        format!("------=_NextPart_01DA0000.00000000\r\nContent-Location: file:///C:/{location}\r\nContent-Transfer-Encoding: base64\r\nContent-Type: {kind}\r\n\r\n{body}\r\n\r\n")
    };
    message.push_str(&part(
        "page.htm",
        "text/html; charset=\"utf-8\"",
        &base64(page(body).as_bytes()),
    ));
    message.push_str(&part(
        "page_files/header.htm",
        "text/html; charset=\"utf-8\"",
        &base64(header.as_bytes()),
    ));
    for name in ["image001.png", "image002.png", "image003.png"] {
        message.push_str(&part(&format!("page_files/{name}"), "image/png", &base64(PNG)));
    }
    message.push_str("------=_NextPart_01DA0000.00000000--\r\n");
    message.into_bytes()
}

#[test]
fn sections_have_their_pages_and_the_headers_from_the_file_beside_them() {
    let document = wp_html::open_mht(&single_file()).expect("opened");
    assert_eq!(document.section_count(), 2);
    let last = paragraph_of(&document, "Last of the first section.");
    assert_eq!(
        paragraph_of(&document, "The second section."),
        last + 1,
        "a paragraph came between"
    );

    let sections = document.sections();
    let first = &sections[0].setup;
    assert_eq!((first.width, first.height), (12_240, 15_840));
    assert_eq!((first.margin_top, first.margin_right), (1440, 1800));
    assert_eq!(document.furniture_distances_of(0), (720, 576));
    let second = &sections[1].setup;
    assert_eq!((second.width, second.height), (15_840, 12_240));
    assert_eq!((second.columns, second.column_gap), (2, 720));

    let text = |kind: Furniture, section: usize, which: Which| {
        document.furniture_of_page(kind, section, which).map(|body| body.plain_text())
    };
    assert_eq!(text(Furniture::Header, 0, Which::Default).as_deref(), Some("Running head "));
    assert_eq!(text(Furniture::Footer, 0, Which::Default).as_deref(), Some("Page footer"));
    assert_eq!(text(Furniture::Header, 0, Which::First).as_deref(), Some("First page head"));
    assert!(document.different_first_page(0));
    // The logo in the header is in the header's own part.
    let header =
        document.furniture_of_page(Furniture::Header, 0, Which::Default).expect("a header");
    let logo = header.paragraphs()[0]
        .runs
        .iter()
        .flat_map(|run| run.content.iter())
        .any(|content| matches!(content, RunContent::Picture(_)));
    assert!(logo, "{header:?}");
}

#[test]
fn drawings_come_from_their_vml_and_their_copies_do_not_come_twice() {
    if let Ok(folder) = std::env::var("WP_HTML_DUMP") {
        std::fs::write(format!("{folder}/page.mht"), single_file()).expect("dumped");
    }
    let document = wp_html::open_mht(&single_file()).expect("opened");
    let shapes = document.shapes();
    assert_eq!(shapes.len(), 2, "{shapes:?}");

    let text_box = shapes.iter().find(|shape| shape.has_text()).unwrap_or_else(|| {
        panic!(
            "no text box: {:?}",
            shapes.iter().map(|shape| (shape.preset.clone(), shape.text.len())).collect::<Vec<_>>()
        )
    });
    assert_eq!(text_box.body().plain_text(), "Inside the box.");
    assert_eq!(text_box.text[0].runs[0].properties.bold, Some(true));
    assert_eq!(text_box.fill, Fill::Solid(Colour::rgb("FFF2CC")));
    assert_eq!((text_box.width_emu, text_box.height_emu), (144 * 12_700, 72 * 12_700));
    assert_eq!(text_box.outline_emu, 6350);
    let anchor = text_box.anchor.as_ref().expect("it floats");
    assert_eq!(anchor.horizontal, Placement::Offset(216 * 12_700));
    assert_eq!(anchor.vertical, Placement::Offset(150 * 635));
    assert_eq!(anchor.wrap, Wrap::Square);

    let oval = shapes.iter().find(|shape| shape.preset == "ellipse").expect("the oval");
    assert_eq!(oval.fill, Fill::Solid(Colour::rgb("FF0000")));
    assert_eq!(oval.outline, Some(Colour::rgb("1F3763")));
    let anchor = oval.anchor.as_ref().expect("it floats");
    assert_eq!(anchor.horizontal_from, Relative::Margin);
    assert_eq!(anchor.horizontal, Placement::Aligned("center".to_owned()));
    assert!(anchor.behind_text, "a drawing below the text is behind it");

    // The picture, once, from its VML at the size it says; its copies for
    // other readers are not in the text.
    let first = paragraph_of(&document, "\u{1}Beside the box: \u{1}and a picture: \u{1}");
    let body = document.body();
    let pictures: Vec<_> = body.paragraphs()[first]
        .runs
        .iter()
        .flat_map(|run| run.content.iter())
        .filter_map(|content| match content {
            RunContent::Picture(picture) => Some(picture.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(pictures.len(), 1);
    assert_eq!((pictures[0].width_emu, pictures[0].height_emu), (72 * 12_700, 36 * 12_700));
    assert!(!document.plain_text().contains("Inside the box.\nInside"), "the text box came twice");
}

#[test]
fn a_page_written_filtered_has_nothing_only_office_reads() {
    let mut body = Body::default();
    let mut paragraph = Paragraph::text("Exact.");
    paragraph.properties.line_spacing =
        Some(wp_docx::model::LineSpacing { value: 300, rule: wp_docx::model::LineRule::Exact });
    body.blocks.push(Block::Paragraph(paragraph));
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![
        wp_docx::model::Run::text("A"),
        wp_docx::model::Run { content: vec![RunContent::Tab], ..wp_docx::model::Run::text("") },
        wp_docx::model::Run::text("B"),
    ])));
    let document = Document::create(&body).expect("a document");

    let full = wp_html::write(&document, "page.htm", None).html;
    assert!(full.contains("mso-") && full.contains("xmlns:w="), "{full}");
    let filtered = wp_html::write_filtered(&document, "page.htm", None).html;
    assert!(!filtered.contains("mso-"), "{filtered}");
    assert!(!filtered.contains("xmlns:o") && !filtered.contains("xmlns:w"), "{filtered}");
    assert!(filtered.contains("(filtered)"), "{filtered}");
    // And it still reads back as the document it was.
    let back = wp_html::read(&filtered);
    assert_eq!(back.body.plain_text().replace('\u{a0}', " ").replace("    ", "\t"), "Exact.\nA\tB");
}

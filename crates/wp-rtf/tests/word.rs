//! Rich Text the way Word writes it, read here.
//!
//! Word writes what LibreOffice does not: a group for each header and footer
//! of each section right after the section's description, footnotes without
//! a star, the description of a table's row a second time just before its
//! end, a table inside a table with its row described after the row, merged
//! cells, table styles, drawings described by their properties, pictures that
//! float, links to bookmarks, a table of contents whose result is several
//! paragraphs long. Each file here is cut down to its shape from what Word
//! writes, with the groups this reader walks past left in where Word puts
//! them.

use wp_docx::anchor::{Placement, Relative, Wrap};
use wp_docx::colour::Colour;
use wp_docx::fills::Fill;
use wp_docx::furniture::{Furniture, Which};
use wp_docx::links::Destination;
use wp_docx::model::{Alignment, Block, Body, Paragraph, RunContent, Table};
use wp_docx::notes::Kind;
use wp_docx::revisions::ChangeKind;
use wp_docx::sections::{NumberFormat, Start};
use wp_docx::table_properties::CellAlignment;
use wp_docx::{Document, StyleKind, TextPosition};

/// The fonts, colours and styles Word puts at the top of a file.
const HEADER: &str = r#"{\rtf1\adeflang1025\ansi\ansicpg1252\uc1\adeff0\deff0\stshfdbch0\stshfloch31506\stshfhich31506\stshfbi31506\deflang1033\deflangfe1033\themelang1033\themelangfe0\themelangcs0
{\fonttbl{\f0\fbidi \froman\fcharset0\fprq2{\*\panose 02020603050405020304}Times New Roman;}{\f1\fbidi \fswiss\fcharset0\fprq2{\*\panose 020f0502020204030204}Calibri;}}
{\colortbl;\red0\green0\blue0;\red0\green0\blue255;\red255\green0\blue0;\red255\green255\blue0;\red255\green255\blue255;}
{\*\defchp \fs22\loch\af1\hich\af1\dbch\af31505 }{\*\defpap \ql \li0\ri0\sa160\sl259\slmult1\widctlpar\wrapdefault\aspalpha\aspnum\faauto\adjustright\rin0\lin0\itap0 }
{\stylesheet{\ql \li0\ri0\sa160\sl259\slmult1\widctlpar\wrapdefault\aspalpha\aspnum\faauto\adjustright\rin0\lin0\itap0 \rtlch\fcs1 \af0\afs22\alang1025 \ltrch\fcs0 \fs22\lang1033\langfe1033\cgrid\langnp1033\langfenp1033 \snext0 \sqformat \spriority0 Normal;}
{\s1\ql \li0\ri0\sb240\keep\keepn\widctlpar\wrapdefault\aspalpha\aspnum\faauto\outlinelevel0\adjustright\rin0\lin0\itap0 \rtlch\fcs1 \af0\afs32\alang1025 \ltrch\fcs0 \fs32\cf2\lang1033\langfe1033\cgrid\langnp1033\langfenp1033 \sbasedon0 \snext0 \slink15 \sqformat \spriority9 heading 1;}
{\*\cs10 \additive \ssemihidden \sunhideused \spriority1 Default Paragraph Font;}
{\*\ts11\tsrowd\trftsWidthB3\trpaddl108\trpaddr108\trpaddfl3\trpaddft3\trpaddfb3\trpaddfr3\tblind0\tblindtype3\tsvertalt\tsbrdrt\tsbrdrl\tsbrdrb\tsbrdrr\tsbrdrdgl\tsbrdrdgr\tsbrdrh\tsbrdrv \ql \li0\ri0\sa160\sl259\slmult1\widctlpar\wrapdefault\aspalpha\aspnum\faauto\adjustright\rin0\lin0\itap0 \rtlch\fcs1 \af0\afs22\alang1025 \ltrch\fcs0 \fs22\lang1033\langfe1033\cgrid\langnp1033\langfenp1033 \snext11 \ssemihidden \sunhideused Normal Table;}
{\*\cs15 \additive \rtlch\fcs1 \af0\afs32 \ltrch\fcs0 \fs32\cf2 \sbasedon10 \slink1 \slocked \spriority9 Heading 1 Char;}
{\*\cs16 \additive \rtlch\fcs1 \ab\af0 \ltrch\fcs0 \b\cf3 \sbasedon10 \sqformat Emphasis Red;}
{\s17\ql \li720\ri720\sa160\widctlpar\wrapdefault\rin720\lin720\itap0 \rtlch\fcs1 \ai\af0 \ltrch\fcs0 \i\fs22 \sbasedon0 \snext17 \sqformat Block Quote;}
{\*\ts18\tsrowd\trbrdrt\brdrs\brdrw10 \trbrdrl\brdrs\brdrw10 \trbrdrb\brdrs\brdrw10 \trbrdrr\brdrs\brdrw10 \trbrdrh\brdrs\brdrw10 \trbrdrv\brdrs\brdrw10 \trftsWidthB3\trpaddl108\trpaddr108\trpaddfl3\trpaddft3\trpaddfb3\trpaddfr3\tblind0\tblindtype3\tsvertalt\tsbrdrt\tsbrdrl\tsbrdrb\tsbrdrr\tsbrdrdgl\tsbrdrdgr\tsbrdrh\tsbrdrv \ql \li0\ri0\widctlpar\wrapdefault\aspalpha\aspnum\faauto\adjustright\rin0\lin0\itap0 \rtlch\fcs1 \af0\afs22\alang1025 \ltrch\fcs0 \fs22\lang1033\langfe1033\cgrid\langnp1033\langfenp1033 \sbasedon11 \snext18 \spriority39 Table Grid;}}
{\*\revtbl {Unknown;}{Kim Smith;}}{\*\rsidtbl \rsid1\rsid2}{\*\generator Microsoft Word 11.0.0000;}{\info{\author Kim Smith}{\creatim\yr2024\mo3\dy5\hr10\min30}}
"#;

/// Word's packed date for the fifth of March 2024, half past ten, a Tuesday.
const DATE: &str = "1203972766";

fn page(body: &str) -> String {
    format!("{HEADER}{body}}}")
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
        .expect("a table")
}

/// Two sections, each with its own page and furniture, and notes, a comment
/// and tracked changes in the first.
fn sections_and_stories() -> String {
    let plain = r"\pard\plain \ltrpar\ql \li0\ri0\sa160\sl259\slmult1\widctlpar\wrapdefault\aspalpha\aspnum\faauto\adjustright\rin0\lin0\itap0 \rtlch\fcs1 \af0\afs22\alang1025 \ltrch\fcs0 \fs22\lang1033\langfe1033\cgrid\langnp1033\langfenp1033 ";
    let furniture = |word: &str, text: &str| {
        format!(
            r"{{\{word} \ltrpar {plain}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 {text}}}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 \par }}}}"
        )
    };
    let mut body = String::from(
        r"\paperw12240\paperh15840\margl1440\margr1440\margt1440\margb1440\gutter0\ltrsect \facingp\widowctrl\ftnbj\aenddoc\trackmoves0\trackformatting1
{\*\ftnsep \ltrpar \pard\plain \ltrpar\ql \li0\ri0\widctlpar\wrapdefault\aspalpha\aspnum\faauto\adjustright\rin0\lin0\itap0 {\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 \chftnsep \par }}
\ltrpar \sectd \ltrsect\linex0\headery708\footery708\colsx708\endnhere\titlepg\sectlinegrid360\sectdefaultcl\sftnbj ",
    );
    body.push_str(&furniture("headerl", "Even head"));
    body.push_str(&furniture("headerr", "Odd head"));
    body.push_str(&format!(
        r#"{{\footerr \ltrpar {plain}\qc {{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 Page }}{{\field{{\*\fldinst {{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1  PAGE   \\* MERGEFORMAT }}}}{{\fldrslt {{\rtlch\fcs1 \af0 \ltrch\fcs0 \lang1024\langfe1024\noproof\insrsid1 1}}}}}}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 \par }}}}"#
    ));
    body.push_str(&furniture("headerf", "First head"));
    // The heading, with a bookmark round its words.
    body.push_str(r"\pard\plain \ltrpar\s1\ql \li0\ri0\sb240\keep\keepn\widctlpar\wrapdefault\aspalpha\aspnum\faauto\outlinelevel0\adjustright\rin0\lin0\itap0 \rtlch\fcs1 \af0\afs32\alang1025 \ltrch\fcs0 \fs32\cf2\lang1033\langfe1033\cgrid\langnp1033\langfenp1033 {\*\bkmkstart chapter}{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 Chapter one}{\*\bkmkend chapter}{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 \par }");
    // A footnote with formatting in it, and an endnote.
    body.push_str(plain);
    body.push_str(r"{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 Words with a note}{\rtlch\fcs1 \af0 \ltrch\fcs0 \cs19\super\insrsid1 \chftn {\footnote \ltrpar \pard\plain \ltrpar\s20\ql \li0\ri0\widctlpar\wrapdefault\aspalpha\aspnum\faauto\adjustright\rin0\lin0\itap0 \rtlch\fcs1 \af0\afs20\alang1025 \ltrch\fcs0 \fs20\lang1033\langfe1033\cgrid\langnp1033\langfenp1033 {\rtlch\fcs1 \af0 \ltrch\fcs0 \cs19\super\insrsid1 \chftn }{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1  The note, }{\rtlch\fcs1 \ab\af0 \ltrch\fcs0 \b\insrsid1 bold}{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1  in part.}}}{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1  and an end}{\rtlch\fcs1 \af0 \ltrch\fcs0 \cs21\super\insrsid1 \chftn {\footnote\ftnalt \ltrpar \pard\plain \ltrpar\s22\ql \li0\ri0\widctlpar\itap0 \rtlch\fcs1 \af0\afs20 \ltrch\fcs0 \fs20 {\rtlch\fcs1 \af0 \ltrch\fcs0 \cs21\super\insrsid1 \chftn }{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1  The endnote.}}}{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 .\par }");
    // A comment over one word, and tracked changes.
    body.push_str(plain);
    body.push_str(&format!(r#"{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 Some }}{{\*\atrfstart 145236}}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 noted}}{{\*\atrfend 145236}}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 {{\*\atnid KS}}{{\*\atnauthor Kim Smith}}\chatn }}{{\*\annotation{{\*\atnref 145236}}{{\*\atndate {DATE}}}\ltrpar \pard\plain \ltrpar\s23\ql \li0\ri0\widctlpar\itap0 \rtlch\fcs1 \af0\afs20 \ltrch\fcs0 \fs20 {{\field{{\*\fldinst {{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 PAGE \\# "'Page: '#'\\n'"  }}}}{{\fldrslt }}}}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \cs24\insrsid1 \chatn }}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 Comment words.}}}}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1  text.\par }}"#));
    body.push_str(plain);
    body.push_str(&format!(r"{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 Kept }}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \revised\revauth1\revdttm{DATE}\insrsid2 added }}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \deleted\revauthdel1\revdttmdel{DATE}\insrsid1 removed }}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 end.}}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 \par }}"));
    // The section ends with its last paragraph, and the next is described.
    body.push_str(plain);
    body.push_str(r"{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 Last of one.}{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 \sect }\sectd \ltrsect\lndscpsxn\pgwsxn15840\pghsxn12240\marglsxn720\margrsxn720\margtsxn1080\margbsxn1080\cols2\colsx360\linex0\headery708\footery708\pgnrestart\pgnstarts5\pgnlcrm\sbkodd\sectlinegrid360\sectdefaultcl\sftnbj ");
    body.push_str(&furniture("headerr", "Landscape head"));
    body.push_str(&format!(r"{{\footerr \ltrpar {plain}\qc Page \chpgn \par }}"));
    body.push_str(plain);
    body.push_str(r"{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 Section two.}{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 \par }");
    page(&body)
}

#[test]
fn sections_have_their_pages_and_their_headers_and_footers() {
    let document = wp_rtf::open(sections_and_stories().as_bytes()).expect("opened");
    assert_eq!(document.section_count(), 2);
    let last = paragraph_of(&document, "Last of one.");
    assert_eq!(paragraph_of(&document, "Section two."), last + 1, "a paragraph came between");

    let sections = document.sections();
    let first = &sections[0].setup;
    assert_eq!((first.width, first.height), (12_240, 15_840));
    assert_eq!(first.margin_left, 1440);
    let second = &sections[1].setup;
    assert_eq!((second.width, second.height), (15_840, 12_240), "not on its side");
    assert_eq!((second.margin_top, second.margin_left), (1080, 720));
    assert_eq!((second.columns, second.column_gap), (2, 360));
    assert_eq!(second.start, Start::OddPage);
    let numbering = document.page_numbering(1);
    assert_eq!(numbering.start, Some(5));
    assert_eq!(numbering.format, NumberFormat::LowerRoman);

    // The first section's four, and a first page of its own.
    let text = |kind: Furniture, section: usize, which: Which| {
        document.furniture_of_page(kind, section, which).map(|body| body.plain_text())
    };
    assert!(document.different_odd_and_even());
    assert!(document.different_first_page(0));
    assert!(!document.different_first_page(1));
    assert_eq!(text(Furniture::Header, 0, Which::Default).as_deref(), Some("Odd head"));
    assert_eq!(text(Furniture::Header, 0, Which::Even).as_deref(), Some("Even head"));
    assert_eq!(text(Furniture::Header, 0, Which::First).as_deref(), Some("First head"));
    let footer =
        document.furniture_of_page(Furniture::Footer, 0, Which::Default).expect("a footer");
    assert_eq!(footer.plain_text(), "Page 1");
    let page = footer.paragraphs()[0].runs.iter().find_map(|run| run.field.clone());
    assert_eq!(page.as_deref(), Some("PAGE   \\* MERGEFORMAT"));
    // The second section's own header, and the even one it follows from the
    // first.
    assert_eq!(text(Furniture::Header, 1, Which::Default).as_deref(), Some("Landscape head"));
    assert!(document.has_own_furniture(Furniture::Header, 1, Which::Default));
    assert!(!document.has_own_furniture(Furniture::Header, 1, Which::Even));
    // The oldest writers' page number is a page field like any other.
    let footer =
        document.furniture_of_page(Furniture::Footer, 1, Which::Default).expect("a footer");
    let page = footer.paragraphs()[0].runs.iter().find_map(|run| run.field.clone());
    assert_eq!(page.as_deref(), Some("PAGE"));
}

#[test]
fn notes_comments_and_changes_go_where_they_were() {
    let document = wp_rtf::open(sections_and_stories().as_bytes()).expect("opened");

    let notes = paragraph_of(&document, "Words with a note\u{2} and an end\u{2}.");
    let footnotes = document.notes(Kind::Footnote);
    assert_eq!(footnotes.len(), 1);
    assert_eq!(footnotes[0].text, "The note, bold in part.");
    assert_eq!(footnotes[0].mark, Some(TextPosition::new(notes, "Words with a note".len())));
    let body = document.note_body(Kind::Footnote, footnotes[0].id).expect("its words");
    let bold = body.paragraphs()[0].runs.iter().find(|run| run.plain_text() == "bold").cloned();
    assert_eq!(bold.and_then(|run| run.properties.bold), Some(true), "{body:?}");
    // The note's own number is the note's, written as Word writes it.
    let part = document.notes_part(Kind::Footnote).expect("the part");
    let xml = document.package().xml_part(&part).expect("there").expect("text");
    assert!(xml.contains("<w:footnoteRef/>"), "{xml}");
    let endnotes = document.notes(Kind::Endnote);
    assert_eq!(endnotes.len(), 1);
    assert_eq!(endnotes[0].text, "The endnote.");

    let comments = document.comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].author, "Kim Smith");
    assert_eq!(comments[0].date, "2024-03-05T10:30:00Z");
    assert_eq!(comments[0].text, "Comment words.");
    let noted = paragraph_of(&document, "Some noted text.");
    assert_eq!(
        comments[0].range,
        Some((TextPosition::new(noted, 5), TextPosition::new(noted, 10)))
    );

    let changes = document.changes();
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert_eq!(changes[0].kind, ChangeKind::Insertion);
    assert_eq!(changes[0].text, "added ");
    assert_eq!(changes[0].author, "Kim Smith");
    assert_eq!(changes[0].date, "2024-03-05T10:30:00Z");
    assert_eq!(changes[1].kind, ChangeKind::Deletion);
    assert_eq!(changes[1].text, "removed ");
    // Deleted text is not where the caret goes: the text around it is
    // counted as though it were not there.
    assert_eq!(
        document.paragraph_text(paragraph_of(&document, "Kept added end.")).as_deref(),
        Some("Kept added end.")
    );

    // And nothing of it is something a person did.
    assert!(!document.can_undo());
    let chapter = document.bookmark_text("chapter");
    assert_eq!(chapter.as_deref(), Some("Chapter one"));
}

#[test]
fn styles_of_every_kind_are_defined() {
    let rtf = page(
        r"\pard\plain \ltrpar\s17\ql \li720\ri720\sa160\rin720\lin720\itap0 \rtlch\fcs1 \ai\af0 \ltrch\fcs0 \i\fs22 {\rtlch\fcs1 \ai\af0 \ltrch\fcs0 \i Quoted, }{\rtlch\fcs1 \ab\ai\af0 \ltrch\fcs0 \cs16\b\i\cf3 loudly}{\par }",
    );
    let document = wp_rtf::open(rtf.as_bytes()).expect("opened");
    let styles = document.styles();

    let quote = styles.get("BlockQuote").cloned().expect("the paragraph style");
    assert_eq!(quote.kind, StyleKind::Paragraph);
    assert_eq!(quote.name.as_deref(), Some("Block Quote"));
    assert_eq!(quote.based_on.as_deref(), Some("Normal"));
    assert_eq!(quote.paragraph.indent_start, Some(720));
    assert_eq!(quote.run.italic, Some(true));
    let red = styles.get("EmphasisRed").cloned().expect("the character style");
    assert_eq!(red.kind, StyleKind::Character);
    assert_eq!(red.run.bold, Some(true));
    assert_eq!(red.run.color.as_deref(), Some("FF0000"));
    let grid = styles.get("TableGrid").cloned().expect("the table style");
    assert_eq!(grid.kind, StyleKind::Table);
    assert_eq!(grid.based_on.as_deref(), Some("TableNormal"));
    let heading = styles.get("Heading1").cloned().expect("the heading");
    assert_eq!(heading.run.size_half_points, Some(32));
    assert_eq!(heading.run.color.as_deref(), Some("0000FF"));
    assert_eq!(heading.paragraph.keep_next, Some(true));
    assert_eq!(styles.get("Normal").map(|normal| normal.run.size_half_points), Some(Some(22)));

    let body = document.body();
    let quoted = paragraph(&body, "Quoted, loudly");
    assert_eq!(quoted.properties.style.as_deref(), Some("BlockQuote"));
    assert_eq!(quoted.runs[1].properties.style.as_deref(), Some("EmphasisRed"));
    // The table style's lines are in the styles part.
    let xml = document.package().xml_part("word/styles.xml").expect("there").expect("text");
    let grid_at = xml.find("w:styleId=\"TableGrid\"").expect("the table style");
    let grid_xml =
        &xml[grid_at..xml[grid_at..].find("</w:style>").map_or(xml.len(), |end| grid_at + end)];
    assert!(
        grid_xml.contains("<w:tblBorders>") && grid_xml.contains("<w:insideV w:val=\"single\""),
        "{grid_xml}"
    );
    assert!(xml.contains("w:type=\"character\" w:styleId=\"EmphasisRed\""), "{xml}");
}

/// A table as Word writes one: a style, cells merged across, a shaded cell,
/// a header row of a fixed height, and in the second row a table of its own,
/// described after its row, with the outer row described again at its end.
fn word_table() -> String {
    let lines = r"\clbrdrt\brdrs\brdrw10 \clbrdrl\brdrs\brdrw10 \clbrdrb\brdrs\brdrw10 \clbrdrr\brdrs\brdrw10 ";
    let row = |cells: &str, extra: &str| {
        format!(
            r"\trowd \irow0\irowband0\ltrrow\ts18\trgaph108\trleft-108\trbrdrt\brdrs\brdrw10 \trbrdrl\brdrs\brdrw10 \trbrdrb\brdrs\brdrw10 \trbrdrr\brdrs\brdrw10 \trbrdrh\brdrs\brdrw10 \trbrdrv\brdrs\brdrw10 {extra}\trftsWidth1\trftsWidthB3\trautofit1\trpaddl108\trpaddr108\trpaddfl3\trpaddft3\trpaddfb3\trpaddfr3\tblrsid1\tbllkhdrrows\tbllklastrow\tbllkhdrcols\tbllklastcol\tblind0\tblindtype3 {cells}"
        )
    };
    let first = row(
        &format!(
            r"\clvertalt{lines}\cltxlrtb\clftsWidth3\clwWidth3000\clmgf\clshdrawnil \cellx2892\clvertalt{lines}\cltxlrtb\clftsWidth3\clwWidth3000\clmrg\clshdrawnil \cellx5892\clvertalc{lines}\clcbpat4\cltxlrtb\clftsWidth3\clwWidth3000\clshdng0 \cellx8892"
        ),
        r"\trhdr\trrh-400",
    );
    let second = row(
        &format!(
            r"\clvertalt{lines}\cltxlrtb\clftsWidth3\clwWidth3000\clshdrawnil \cellx2892\clvertalt{lines}\cltxlrtb\clftsWidth3\clwWidth6000\clshdrawnil \cellx8892"
        ),
        "",
    );
    let inner = r"\trowd \irow0\irowband0\lastrow \ltrrow\ts18\trgaph108\trleft0\trbrdrt\brdrs\brdrw10 \trbrdrl\brdrs\brdrw10 \trbrdrb\brdrs\brdrw10 \trbrdrr\brdrs\brdrw10 \trbrdrh\brdrs\brdrw10 \trbrdrv\brdrs\brdrw10 \trftsWidth1\trftsWidthB3\trautofit1\trpaddl108\trpaddr108\tblind0\tblindtype3 \clvertalt\clbrdrt\brdrs\brdrw10 \cltxlrtb\clftsWidth3\clwWidth1500\clshdrawnil \cellx1500\clvertalt\clbrdrt\brdrs\brdrw10 \cltxlrtb\clftsWidth3\clwWidth1500\clshdrawnil \cellx3000";
    let cell = r"\pard\plain \ltrpar\ql \li0\ri0\widctlpar\intbl\wrapdefault\aspalpha\aspnum\faauto\adjustright\rin0\lin0 \rtlch\fcs1 \af0\afs22 \ltrch\fcs0 \fs22 ";
    let nested = r"\pard\plain \ltrpar\ql \li0\ri0\widctlpar\intbl\wrapdefault\aspalpha\aspnum\faauto\adjustright\rin0\lin0\itap2 \rtlch\fcs1 \af0\afs22 \ltrch\fcs0 \fs22 ";
    let back = r"\pard\plain \ltrpar\ql \li0\ri0\widctlpar\intbl\wrapdefault\aspalpha\aspnum\faauto\adjustright\rin0\lin0\itap1 \rtlch\fcs1 \af0\afs22 \ltrch\fcs0 \fs22 ";
    let mut body = String::from(r"\pard\plain \ltrpar\ql \sa160 {Before.\par }");
    body.push_str(&first);
    body.push_str(&format!(
        r"{cell}{{Merged across}}{{\cell }}{{\cell }}{{Yellow}}{{\cell }}{cell}{{{first}\row }}"
    ));
    body.push_str(&second);
    body.push_str(&format!(r"{cell}{{Outer}}{{\cell }}{nested}{{Inner A}}{{\nestcell{{\nonesttables\par }}}}{{Inner B}}{{\nestcell{{\nonesttables\par }}}}{nested}{{\*\nesttableprops{inner}\nestrow}}{{\nonesttables\par }}{back}{{After inner}}{{\cell }}{cell}{{{second}\row }}"));
    body.push_str(r"\pard\plain \ltrpar\ql \sa160 {After.\par }");
    page(&body)
}

#[test]
fn a_word_table_keeps_its_merges_shading_style_and_inner_table() {
    let reading = wp_rtf::read(word_table().as_bytes());
    let table = first_table(&reading.body.blocks);
    assert_eq!(table.style.as_deref(), Some("TableGrid"));
    assert!(table.borders.inside_vertical.as_ref().is_some_and(|line| line.style == "single"));
    assert_eq!(table.grid, vec![3000, 3000, 3000]);
    assert_eq!(table.rows.len(), 2);

    let header = &table.rows[0];
    assert!(header.is_header);
    assert_eq!((header.height, header.height_exact), (Some(400), true));
    assert_eq!(header.cells.len(), 2, "the merged cell is still two");
    assert_eq!(header.cells[0].blocks[0].plain_text(), "Merged across");
    assert_eq!((header.cells[0].span, header.cells[0].width), (2, Some(6000)));
    assert_eq!(header.cells[1].blocks[0].plain_text(), "Yellow");
    assert_eq!(header.cells[1].shading.as_deref(), Some("FFFF00"));
    assert_eq!(header.cells[1].vertical, CellAlignment::Middle);
    assert!(header.cells[0].borders.top.as_ref().is_some_and(|line| line.size == 4));

    let second = &table.rows[1];
    assert_eq!(second.cells.len(), 2);
    assert_eq!(second.cells[1].span, 2);
    let holder = &second.cells[1].blocks;
    assert_eq!(holder.len(), 2, "{holder:?}");
    let inner = first_table(holder);
    assert_eq!(inner.rows[0].cells.len(), 2);
    assert_eq!(inner.rows[0].cells[0].blocks[0].plain_text(), "Inner A");
    assert_eq!(inner.rows[0].cells[1].blocks[0].plain_text(), "Inner B");
    assert_eq!(inner.grid, vec![1500, 1500]);
    assert_eq!(holder[1].plain_text(), "After inner");

    // Nothing written for readers that do not know tables in tables leaks in.
    let text = reading.body.plain_text();
    assert!(text.starts_with("Before.") && text.ends_with("After."), "{text:?}");

    // And the document has it all as the model had it.
    let document = wp_rtf::open(word_table().as_bytes()).expect("opened");
    let body = document.body();
    let table = first_table(&body.blocks);
    assert_eq!(table.rows[0].cells[0].span, 2);
    assert_eq!(table.rows[0].cells[1].shading.as_deref(), Some("FFFF00"));
    assert_eq!(first_table(&table.rows[1].cells[1].blocks).rows[0].cells.len(), 2);
}

#[test]
fn cells_merged_down_stay_merged() {
    let row = |merge: &str| format!(r"\trowd\trgaph108\trleft0{merge}\cellx2000\cellx4000");
    let rtf = page(&format!(
        r"{}\pard\intbl Tall\cell A\cell\row {}\pard\intbl \cell B\cell\row \pard After\par ",
        row(r"\clvmgf"),
        row(r"\clvmrg")
    ));
    let document = wp_rtf::open(rtf.as_bytes()).expect("opened");
    let body = document.body();
    let table = first_table(&body.blocks);
    assert!(!table.rows[0].cells[0].merged_upwards);
    assert!(table.rows[1].cells[0].merged_upwards);
    let saved = Document::open(&document.save().expect("saved")).expect("reopened");
    let table = first_table(&saved.body().blocks).clone();
    assert!(table.rows[1].cells[0].merged_upwards, "the merge was not written");
}

/// A picture as small as a PNG can be.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n\x2d\xb4\0\0\0\0IEND\xaeB`\x82";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn drawings_become_shapes_and_framed_pictures_float() {
    let props = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(name, value)| format!(r"{{\sp{{\sn {name}}}{{\sv {value}}}}}"))
            .collect::<String>()
    };
    let oval = format!(
        r"{{\shp{{\*\shpinst\shpleft1440\shptop360\shpright2880\shpbottom1080\shpfhdr0\shpbxcolumn\shpbxignore\shpbypara\shpbyignore\shpwr3\shpwrk0\shpfblwtxt1\shpz2\shplid1026{}}}{{\shprslt{{\*\do\dobxcolumn\dobypara\dodhgt8193\dpellipse\dpx1440\dpy360\dpxsize1440\dpysize720}}}}}}",
        props(&[
            ("shapeType", "3"),
            ("fillColor", "255"),
            ("lineColor", "16711680"),
            ("lineWidth", "19050"),
            ("posh", "2"),
            ("posrelh", "0"),
            ("posrelv", "2"),
            ("wzName", "Oval 1"),
            ("rotation", "2949120")
        ])
    );
    let line = format!(
        r"{{\shp{{\*\shpinst\shpleft0\shptop0\shpright2880\shpbottom0\shpbxcolumn\shpbypara\shpwr3\shpz3\shplid1027{}}}}}",
        props(&[("shapeType", "20"), ("fFlipV", "1")])
    );
    let text_box = format!(
        r"{{\shp{{\*\shpinst\shpleft0\shptop0\shpright2880\shpbottom1440\shpbxcolumn\shpbypara\shpwr2\shpz4\shplid1028{}{{\shptxt \ltrpar\pard\plain \ltrpar\ql {{\b Inside}}{{ the box.}}\par }}}}}}",
        props(&[("shapeType", "202"), ("fFilled", "0"), ("wzDescription", "A note to the reader")])
    );
    let framed = format!(
        r"{{\shp{{\*\shpinst\shpleft720\shptop0\shpright2160\shpbottom720\shpbxpage\shpbxignore\shpbypage\shpbyignore\shpwr1\shpz5\shplid1029{}{{\sp{{\sn pib}}{{\sv {{\pict\picscalex100\picscaley100\picw1\pich1\picwgoal1440\pichgoal720\pngblip {}}}}}}}}}{{\shprslt {{\pict\wmetafile8\picw1\pich1 0100}}}}}}",
        props(&[
            ("shapeType", "75"),
            ("posrelh", "1"),
            ("posrelv", "1"),
            ("posh", "3"),
            ("posv", "1")
        ]),
        hex(PNG)
    );
    let inline = format!(
        r"{{\shp{{\*\shpinst\shpleft0\shptop0\shpright720\shpbottom720\shpwr3\shplid1030{}}}}}",
        props(&[("shapeType", "5"), ("fPseudoInline", "1")])
    );
    let grouped =
        r"{\shp{\*\shpinst\shpleft0\shptop0\shpright720\shpbottom720{\sp{\sn shapeType}{\sv 1}}}}";
    let rtf = page(&format!(
        r"\pard\plain Drawn: {oval}{line}{text_box}{framed}{inline}{{\shpgrp{{\*\shpinst {grouped}}}}}{{\field{{\*\fldinst SHAPE  \\* MERGEFORMAT }}{{\fldrslt {inline}}}}} end.\par "
    ));
    let document = wp_rtf::open(rtf.as_bytes()).expect("opened");
    let shapes = document.shapes();
    assert_eq!(shapes.len(), 5, "{shapes:?}");

    let oval = shapes.iter().find(|shape| shape.preset == "ellipse").expect("the oval");
    assert_eq!(oval.fill, Fill::Solid(Colour::rgb("FF0000")));
    assert_eq!(oval.outline, Some(Colour::rgb("0000FF")));
    assert_eq!(oval.outline_emu, 19050);
    assert_eq!((oval.width_emu, oval.height_emu), (1440 * 635, 720 * 635));
    assert_eq!(oval.name, "Oval 1");
    assert_eq!(oval.rotation, 45 * 60_000);
    let anchor = oval.anchor.as_ref().expect("it floats");
    assert_eq!(anchor.horizontal_from, Relative::Margin);
    assert_eq!(anchor.horizontal, Placement::Aligned("center".to_owned()));
    assert_eq!(anchor.vertical_from, Relative::Paragraph);
    assert_eq!(anchor.vertical, Placement::Offset(360 * 635));
    assert_eq!(anchor.wrap, Wrap::None);
    assert!(anchor.behind_text);

    let line = shapes.iter().find(|shape| shape.preset == "line").expect("the line");
    assert_eq!(line.fill, Fill::None);
    assert!(line.flipped_down);
    assert_eq!(line.anchor.as_ref().map(|anchor| anchor.horizontal_from), Some(Relative::Column));

    let text_box = shapes.iter().find(|shape| shape.has_text()).expect("the text box");
    assert_eq!(text_box.body().plain_text(), "Inside the box.");
    assert_eq!(text_box.text[0].runs[0].properties.bold, Some(true));
    assert_eq!(text_box.fill, Fill::None);
    assert_eq!(text_box.name, "Text Box");
    assert_eq!(text_box.description, "A note to the reader");
    assert_eq!(text_box.anchor.as_ref().map(|anchor| anchor.wrap), Some(Wrap::Square));

    let triangles: Vec<_> = shapes.iter().filter(|shape| shape.preset == "triangle").collect();
    assert_eq!(triangles.len(), 2, "one in the line and one in a drawing field");
    assert!(triangles.iter().all(|shape| shape.anchor.is_none()));

    // The framed picture is a picture, floating where the frame was.
    let drawn = paragraph_of(&document, "Drawn: \u{1}\u{1}\u{1}\u{1}\u{1}\u{1} end.");
    let at = TextPosition::new(drawn, "Drawn: ".len() + 3);
    let anchor = document.anchor_at(at).expect("the picture floats");
    assert_eq!(anchor.horizontal_from, Relative::Page);
    assert_eq!(anchor.horizontal, Placement::Aligned("right".to_owned()));
    assert_eq!(anchor.vertical, Placement::Aligned("top".to_owned()));
    assert_eq!(anchor.wrap, Wrap::TopAndBottom);
    let body = document.body();
    let picture = body.paragraphs()[drawn]
        .runs
        .iter()
        .flat_map(|run| run.content.iter())
        .find_map(|content| match content {
            RunContent::Picture(picture) => Some(picture.clone()),
            _ => None,
        })
        .expect("the picture");
    assert_eq!(document.embedded_part(&picture.relationship), Some(PNG));
    assert_eq!((picture.width_emu, picture.height_emu), (1440 * 635, 720 * 635));
}

#[test]
fn metafiles_bitmaps_and_binary_pictures_are_read() {
    // A metafile of nothing but its header and its end, as Word writes one
    // inside RTF: without the header that says how big it is.
    let mut wmf = vec![1, 0, 9, 0, 0, 3];
    wmf.extend_from_slice(&12u32.to_le_bytes());
    wmf.extend_from_slice(&[0, 0, 3, 0, 0, 0, 0, 0]);
    wmf.extend_from_slice(&[3, 0, 0, 0, 0, 0]);
    // One blue pixel, bottom up, as a bitmap with no file header.
    let mut dib = Vec::new();
    for value in [40u32, 1, 1] {
        dib.extend_from_slice(&value.to_le_bytes());
    }
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&24u16.to_le_bytes());
    for value in [0u32, 4, 0, 0, 0, 0] {
        dib.extend_from_slice(&value.to_le_bytes());
    }
    dib.extend_from_slice(&[0xFF, 0, 0, 0]);
    let emf = b" EMF".to_vec();

    let mut rtf = page(&format!(
        r"\pard A{{\pict\wmetafile8\picw2540\pich1270\picwgoal1440\pichgoal720 {}}}B{{\pict\emfblip\picw2540\pich2540 {}}}C{{\pict\dibitmap0\picw1\pich1\picscalex200\picscaley200 {}}}D{{\pict\pngblip\picwgoal720\pichgoal720\bin{} ",
        hex(&wmf),
        hex(&emf),
        hex(&dib),
        PNG.len()
    ))
    .into_bytes();
    // The picture given as its bytes, after `\bin`: braces and all.
    rtf.truncate(rtf.len() - 1);
    rtf.extend_from_slice(PNG);
    rtf.extend_from_slice(b"}E\\par }");

    let reading = wp_rtf::read(&rtf);
    let pictures = &reading.pictures;
    assert_eq!(pictures.len(), 4, "{:?}", reading.body.plain_text());
    assert_eq!(pictures[0].extension, "wmf");
    assert_eq!(pictures[0].bytes, wmf);
    assert_eq!((pictures[0].width_emu, pictures[0].height_emu), (1440 * 635, 720 * 635));
    assert_eq!(pictures[1].extension, "emf");
    // A metafile's own size is in hundredths of a millimetre.
    assert_eq!((pictures[1].width_emu, pictures[1].height_emu), (2540 * 360, 2540 * 360));
    assert_eq!(pictures[2].extension, "bmp");
    let bitmap = wp_image::decode(&pictures[2].bytes).expect("a bitmap");
    assert_eq!((bitmap.width, bitmap.height), (1, 1));
    assert_eq!((pictures[2].width_emu, pictures[2].height_emu), (2 * 9525, 2 * 9525), "scaled");
    assert_eq!(pictures[3].bytes, PNG);
    assert_eq!(reading.body.plain_text().replace(wp_rtf::PICTURE_MARK, "*"), "A*B*C*D*E");
    // Each where its mark was, counted as the one character it will be.
    assert_eq!(pictures.iter().map(|picture| picture.offset).collect::<Vec<_>>(), vec![1, 3, 5, 7]);

    let document = wp_rtf::open(&rtf).expect("opened");
    assert_eq!(document.paragraph_text(0).as_deref(), Some("A\u{1}B\u{1}C\u{1}D\u{1}E"));
}

#[test]
fn links_to_places_and_fields_that_run_over_paragraphs_are_read() {
    let rtf = page(
        r#"\pard\plain {\field{\*\fldinst { TOC \\o "1-3" \\h \\z \\u }}{\fldrslt {\field{\*\fldinst { HYPERLINK \\l "_Toc1" }}{\fldrslt {Chapter one\tab 1}}}\par \pard\plain {\field{\*\fldinst { HYPERLINK \\l "_Toc2" }}{\fldrslt {Chapter two\tab 2}}}\par }}\pard\plain {\*\bkmkstart _Toc1}Chapter one{\*\bkmkend _Toc1}\par \pard\plain {\*\bkmkstart _Toc2}Chapter two{\*\bkmkend _Toc2}\par \pard See {\field{\*\fldinst {HYPERLINK "https://example.com/a" \\l "part" \\o "A tip"}}{\fldrslt {\ul there}}} and {\field\fldlock{\*\fldinst { DATE \\@ "d MMMM yyyy" }}{\fldrslt {5 March 2024}}}.\par }"#,
    );
    let document = wp_rtf::open(rtf.as_bytes()).expect("opened");
    let links = document.hyperlinks();
    assert_eq!(links.len(), 3, "{links:?}");
    assert_eq!(links[0].destination, Destination::Place("_Toc1".to_owned()));
    // Over the whole entry, tab and page number too.
    assert_eq!((links[0].paragraph, links[0].range), (0, (0, "Chapter one\t1".len())));
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Chapter one\t1"));
    assert_eq!(links[1].destination, Destination::Place("_Toc2".to_owned()));
    assert!(
        matches!(&links[2].destination, Destination::Address(address) if address == "https://example.com/a#part"),
        "{links:?}"
    );
    assert_eq!(links[2].text, "there");
    assert!(document.bookmark("_Toc2").is_some());

    // The table of contents is two paragraphs, which one field in the model
    // cannot be: its result stays as text, and the links in it are links.
    let body = document.body();
    assert!(
        body.paragraphs()[0].runs.iter().all(|run| run.field.is_none()),
        "{:?}",
        body.paragraphs()[0]
    );
    let dated = paragraph(&body, "See there and 5 March 2024.");
    let date = dated.runs.iter().find_map(|run| run.field.clone());
    assert_eq!(date.as_deref(), Some("DATE \\@ \"d MMMM yyyy\""));
}

#[test]
fn borders_shading_and_direction_are_read() {
    let rtf = page(
        r"\pard\plain \box\brdrs\brdrw15\brdrcf3 \brsp20 \shading5000\cfpat1\cbpat5 {Half grey}\par \pard\plain \brdrb\brdrdb\brdrw10\brdrcf2 \cbpat4 {Ruled under}\par \pard\plain \rtlpar\qr {\ltrch\fcs0 \rtlch\fcs1 \u1605?\u1585?}{\rtlch\fcs1 \ltrch\fcs0  mixed}\par }",
    );
    let reading = wp_rtf::read(rtf.as_bytes());
    let body = &reading.body;

    let grey = paragraph(body, "Half grey");
    assert_eq!(grey.properties.shading.as_deref(), Some("808080"));
    for side in [
        &grey.properties.borders.top,
        &grey.properties.borders.start,
        &grey.properties.borders.bottom,
        &grey.properties.borders.end,
    ] {
        let side = side.as_ref().expect("a side of the box");
        assert_eq!(
            (side.style.as_str(), side.size, side.color.as_deref()),
            ("single", 6, Some("FF0000"))
        );
    }
    let ruled = paragraph(body, "Ruled under");
    assert_eq!(ruled.properties.shading.as_deref(), Some("FFFF00"));
    assert!(ruled.properties.borders.top.is_none());
    let under = ruled.properties.borders.bottom.as_ref().expect("the rule");
    assert_eq!(
        (under.style.as_str(), under.size, under.color.as_deref()),
        ("double", 4, Some("0000FF"))
    );

    let arabic = paragraph(body, "\u{645}\u{631} mixed");
    assert_eq!(arabic.properties.right_to_left, Some(true));
    // Aligned to the right of the page, which is where its lines start.
    assert_eq!(arabic.properties.alignment, Some(Alignment::Start));
    assert_eq!(arabic.runs[0].properties.right_to_left, Some(true));
    assert_eq!(arabic.runs[1].properties.right_to_left, None, "the last of the two holds");
}

/// Whether a body holds a picture anywhere in its runs.
fn has_picture(body: &Body) -> bool {
    body.paragraphs()
        .iter()
        .flat_map(|paragraph| paragraph.runs.iter())
        .any(|run| run.content.iter().any(|content| matches!(content, RunContent::Picture(_))))
}

/// Whether a part names a picture among its relationships.
fn part_has_image(document: &Document, part: &str) -> bool {
    document.package().relationships(part).is_ok_and(|relationships| {
        relationships.all().iter().any(|relationship| relationship.kind.ends_with("/image"))
    })
}

#[test]
fn pictures_in_headers_notes_and_comments_go_into_their_own_parts() {
    let pict = format!(r"{{\*\shppict{{\pict\pngblip\picwgoal720\pichgoal720 {}}}}}", hex(PNG));
    let logo = format!(
        r"{{\shp{{\*\shpinst\shpleft0\shptop0\shpright1440\shpbottom720\shpbxmargin\shpbymargin\shpwr3\shplid1025{{\sp{{\sn shapeType}}{{\sv 75}}}}{{\sp{{\sn pib}}{{\sv {{\pict\pngblip\picwgoal1440\pichgoal720 {}}}}}}}}}}}",
        hex(PNG)
    );
    let rtf = page(&format!(
        r"\sectd {{\header \pard\plain Logo {logo} and {pict} here.\par \pard Second line {pict}\par }}\pard\plain Text{{\super\chftn {{\footnote \pard\plain {{\super\chftn}} A note {pict} with a picture.}}}} and {{\*\atrfstart 1}}this{{\*\atrfend 1}}{{\*\atnauthor Kim}}\chatn {{\*\annotation{{\*\atnref 1}}\pard\plain First.\par \pard Then {pict} a picture.}}.\par "
    ));
    let reading = wp_rtf::read(rtf.as_bytes());
    let header = &reading.sections[0].furniture[0];
    assert_eq!(header.pictures.len(), 3, "{:?}", header.body.plain_text());
    assert_eq!((header.pictures[0].paragraph, header.pictures[0].offset), (0, "Logo ".len()));
    assert!(header.pictures[0].anchor.is_some());
    assert_eq!(
        (header.pictures[2].paragraph, header.pictures[2].offset),
        (1, "Second line ".len())
    );
    assert_eq!(reading.notes[0].pictures.len(), 1);
    assert_eq!(reading.comments[0].pictures.len(), 1);
    assert_eq!(reading.comments[0].pictures[0].paragraph, 1);
    assert!(reading.pictures.is_empty(), "a story's picture landed in the text");

    let document = wp_rtf::open(rtf.as_bytes()).expect("opened");
    let part = document.furniture_part_for(Furniture::Header, 0, Which::Default).expect("a header");
    assert!(part_has_image(&document, &part), "the header names no picture");
    let header =
        document.furniture_of_page(Furniture::Header, 0, Which::Default).expect("a header");
    assert_eq!(header.plain_text(), "Logo  and  here.\nSecond line ");
    assert!(has_picture(&header));

    let footnotes = document.notes(Kind::Footnote);
    let note = document.note_body(Kind::Footnote, footnotes[0].id).expect("the note");
    assert!(has_picture(&note), "{note:?}");
    assert_eq!(footnotes[0].text, "A note  with a picture.");
    let part = document.notes_part(Kind::Footnote).expect("the notes");
    assert!(part_has_image(&document, &part));

    let comments = document.comments();
    let comment = document.comment_body(comments[0].id).expect("the comment");
    assert!(has_picture(&comment), "{comment:?}");
    assert_eq!(comment.plain_text(), "First.\nThen  a picture.");
    assert_eq!(document.plain_text().trim_end(), "Text and this.");
}

#[test]
fn a_tracked_change_of_formatting_keeps_what_it_changed() {
    let rtf = page(&format!(
        r"\pard\plain {{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1 Made }}{{\rtlch\fcs1 \ab\af0 \ltrch\fcs0 \b\i\crauth1\crdate{DATE}{{\*\oldcprops \ltrch\fcs0 \i }}\insrsid1 bold}}{{\rtlch\fcs1 \af0 \ltrch\fcs0 \insrsid1  later.}}\par "
    ));
    let document = wp_rtf::open(rtf.as_bytes()).expect("opened");
    let body = document.body();
    let made = paragraph(&body, "Made bold later.");
    let run = made.runs.iter().find(|run| run.plain_text() == "bold").expect("the run");
    assert_eq!(run.properties.bold, Some(true));
    let change = run.format_change.as_ref().expect("the change");
    assert_eq!(change.author, "Kim Smith");
    assert_eq!(change.date, "2024-03-05T10:30:00Z");
    assert_eq!(change.before.italic, Some(true), "what it was: {:?}", change.before);
    assert_eq!(change.before.bold, None);
    assert!(made
        .runs
        .iter()
        .filter(|run| run.plain_text() != "bold")
        .all(|run| run.format_change.is_none()));
    assert!(document.changes().iter().any(|found| found.kind == ChangeKind::Formatting));

    // Written as the change it is, and read back as one.
    let saved = Document::open(&document.save().expect("saved")).expect("reopened");
    let body = saved.body();
    let run = paragraph(&body, "Made bold later.").runs[1].clone();
    assert_eq!(run.format_change.map(|change| change.before.italic), Some(Some(true)));
}

#[test]
fn left_and_right_are_the_pages_and_start_and_end_the_texts() {
    let rtf = page(
        r"\pard\plain \rtlpar\ql\li720\ri360\brdrl\brdrs\brdrw10 {Backwards}\par \pard\plain \rtlpar\qr\li720\ri360\lin100\rin200 {Both given}\par \pard\plain \qr\li720 {Forwards}\par ",
    );
    let reading = wp_rtf::read(rtf.as_bytes());
    let backwards = paragraph(&reading.body, "Backwards");
    assert_eq!(backwards.properties.alignment, Some(Alignment::End));
    assert_eq!(
        (backwards.properties.indent_start, backwards.properties.indent_end),
        (Some(360), Some(720))
    );
    assert!(
        backwards.properties.borders.end.is_some() && backwards.properties.borders.start.is_none()
    );
    let both = paragraph(&reading.body, "Both given");
    assert_eq!((both.properties.indent_start, both.properties.indent_end), (Some(100), Some(200)));
    let forwards = paragraph(&reading.body, "Forwards");
    assert_eq!(forwards.properties.alignment, Some(Alignment::End));
    assert_eq!(forwards.properties.indent_start, Some(720));
}

#[test]
fn a_link_that_begins_with_a_picture_begins_at_the_picture() {
    let rtf = page(&format!(
        r#"\pard\plain See {{\field{{\*\fldinst {{HYPERLINK "https://example.com/"}}}}{{\fldrslt {{\*\shppict{{\pict\pngblip\picwgoal240\pichgoal240 {}}}}}{{ the logo}}}}}} here.\par "#,
        hex(PNG)
    ));
    let document = wp_rtf::open(rtf.as_bytes()).expect("opened");
    assert_eq!(document.paragraph_text(0).as_deref(), Some("See \u{1} the logo here."));
    let links = document.hyperlinks();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].range, (4, 4 + 1 + " the logo".len()), "the link moved off its picture");
}

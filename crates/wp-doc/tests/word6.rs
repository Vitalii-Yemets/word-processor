//! Word 6 and Word 95 documents.
//!
//! No program in the build image writes one — LibreOffice reads them and
//! has not written them for years — so the file is built here, byte by
//! byte, as the format lays one out: the block at its fixed places, the text
//! in one stretch of single bytes, the formatting pages with Word 6's
//! narrower entries and one-byte sprms, the stylesheet, the fonts, a section,
//! a header, a note, a comment, a bookmark, a field and a table. What holds
//! it to the format is LibreOffice reading it: it is handed the file,
//! converts it to a `.docx`, and what it read is what this reader must read.

use std::process::{Command, Output};
use std::sync::Mutex;

use wp_docx::model::{Alignment, Block};
use wp_docx::Document;

/// LibreOffice takes turns with itself.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn run(command: &mut Command) -> std::io::Result<Output> {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    command.output()
}

/// Where the text begins in the stream, past the block.
const TEXT_AT: u32 = 0x400;

/// A paragraph of the file: its text, its style, and its paragraph sprms.
struct Para {
    text: Vec<u8>,
    istd: u16,
    sprms: Vec<u8>,
}

/// A stretch of characters with sprms of their own: where, and which.
struct Chars {
    from: u32,
    to: u32,
    sprms: Vec<u8>,
}

/// The file being built: the stream, and the pairs the block will name.
struct Word6 {
    stream: Vec<u8>,
    pairs: Vec<(usize, u32, u32)>,
}

impl Word6 {
    fn put(&mut self, index: usize, bytes: &[u8]) {
        let at = self.stream.len() as u32;
        self.stream.extend_from_slice(bytes);
        if self.stream.len() % 2 == 1 {
            self.stream.push(0);
        }
        self.pairs.push((index, at, bytes.len() as u32));
    }

    fn pad_to_page(&mut self) {
        while self.stream.len() % 512 != 0 {
            self.stream.push(0);
        }
    }
}

fn plc(positions: &[u32], entries: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for position in positions {
        out.extend_from_slice(&position.to_le_bytes());
    }
    for entry in entries {
        out.extend_from_slice(entry);
    }
    out
}

fn pascal(text: &[u8]) -> Vec<u8> {
    let mut out = vec![text.len() as u8];
    out.extend_from_slice(text);
    out
}

/// A style: its number of Word's own, its kind, its base, its name, and its
/// paragraph and character sprms.
fn style(
    sti: u16,
    kind: u16,
    base: u16,
    name: &str,
    papx: Option<(u16, &[u8])>,
    chpx: &[u8],
) -> Vec<u8> {
    let cupx = if papx.is_some() { 2u16 } else { 1 };
    let mut std = Vec::new();
    std.extend_from_slice(&sti.to_le_bytes());
    std.extend_from_slice(&(kind | (base << 4)).to_le_bytes());
    // How many property groups, and the next style, which is Normal.
    std.extend_from_slice(&cupx.to_le_bytes());
    std.extend_from_slice(&0u16.to_le_bytes());
    std.extend_from_slice(&pascal(name.as_bytes()));
    std.push(0);
    if let Some((istd, grpprl)) = papx {
        if std.len() % 2 == 1 {
            std.push(0);
        }
        std.extend_from_slice(&((grpprl.len() + 2) as u16).to_le_bytes());
        std.extend_from_slice(&istd.to_le_bytes());
        std.extend_from_slice(grpprl);
    }
    if std.len() % 2 == 1 {
        std.push(0);
    }
    std.extend_from_slice(&(chpx.len() as u16).to_le_bytes());
    std.extend_from_slice(chpx);
    if std.len() % 2 == 1 {
        std.push(0);
    }
    let mut out = (std.len() as u16).to_le_bytes().to_vec();
    out.extend_from_slice(&std);
    out
}

/// A font: its kind of letter, its character set, its name.
fn font(bits: u8, charset: u8, name: &str) -> Vec<u8> {
    let mut ffn = vec![0, bits, 0x90, 0x01, charset, 0];
    ffn.extend_from_slice(name.as_bytes());
    ffn.push(0);
    ffn[0] = (ffn.len() - 1) as u8;
    ffn
}

/// Word 6's sprms, by number.
mod sprm {
    pub const PJC: u8 = 5;
    pub const PKEEP_FOLLOW: u8 = 8;
    pub const PANLD: u8 = 12;
    pub const PNLVL_ANM: u8 = 13;
    pub const PDXA_LEFT: u8 = 17;
    pub const PDYA_BEFORE: u8 = 21;
    pub const PIN_TABLE: u8 = 24;
    pub const PTTP: u8 = 25;
    pub const CFSPEC: u8 = 117;
    pub const CFBOLD: u8 = 85;
    pub const CFITALIC: u8 = 86;
    pub const CFTC: u8 = 93;
    pub const CHPS: u8 = 99;
    pub const SGPRF_IHDT: u8 = 153;
    pub const SXA_PAGE: u8 = 164;
    pub const SYA_PAGE: u8 = 165;
    pub const SDXA_LEFT: u8 = 166;
    pub const SDXA_RIGHT: u8 = 167;
    pub const SDYA_TOP: u8 = 168;
    pub const SDYA_BOTTOM: u8 = 169;
    pub const TDEF_TABLE: u8 = 190;
}

fn word(number: u8, value: u16) -> Vec<u8> {
    let mut out = vec![number];
    out.extend_from_slice(&value.to_le_bytes());
    out
}

/// A bullet as Word 6 describes one on the paragraph: the kind of number,
/// no text before and one character after, and the bullet in the font given.
fn bullet(font: u16) -> Vec<u8> {
    let mut anld = vec![0u8; 52];
    anld[0] = 23;
    anld[2] = 1;
    anld[6..8].copy_from_slice(&font.to_le_bytes());
    anld[8..10].copy_from_slice(&20u16.to_le_bytes());
    anld[12..14].copy_from_slice(&360u16.to_le_bytes());
    anld[20] = 0xB7;
    let mut out = vec![sprm::PANLD, anld.len() as u8];
    out.extend_from_slice(&anld);
    out.extend_from_slice(&[sprm::PNLVL_ANM, 11]);
    out
}

/// A row's end: in a table, ending a row, and the row's two cells an inch
/// and a half each.
fn row_end() -> Vec<u8> {
    let mut definition = vec![2u8];
    for edge in [0i16, 2160, 4320] {
        definition.extend_from_slice(&edge.to_le_bytes());
    }
    definition.extend_from_slice(&[0; 20]);
    let mut out = vec![sprm::PIN_TABLE, 1, sprm::PTTP, 1, sprm::TDEF_TABLE];
    out.extend_from_slice(&(definition.len() as u16 + 1).to_le_bytes());
    out.extend_from_slice(&definition);
    out
}

/// The whole file.
fn word6() -> Vec<u8> {
    let cyrillic: Vec<u8> = vec![0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2, 0x2C, 0x20, 0xEC, 0xE8, 0xF0];
    let mut hello = b"Plain, bold and italic text".to_vec();
    let note_at = hello.len();
    hello.extend_from_slice(b"\x02.\r");
    let mut marked = b"Marked text with a comment".to_vec();
    let comment_at = marked.len();
    marked.extend_from_slice(b"\x05.\r");
    let mut cyrillic_paragraph = cyrillic.clone();
    cyrillic_paragraph.push(b'\r');
    let main = vec![
        Para { text: b"Heading one\r".to_vec(), istd: 1, sprms: Vec::new() },
        Para { text: hello, istd: 0, sprms: Vec::new() },
        Para {
            text: b"Centred and indented.\r".to_vec(),
            istd: 0,
            sprms: [vec![sprm::PJC, 1], word(sprm::PDXA_LEFT, 720)].concat(),
        },
        Para { text: cyrillic_paragraph, istd: 0, sprms: Vec::new() },
        Para { text: b"A\x07".to_vec(), istd: 0, sprms: vec![sprm::PIN_TABLE, 1] },
        Para { text: b"B\x07".to_vec(), istd: 0, sprms: vec![sprm::PIN_TABLE, 1] },
        Para { text: b"\x07".to_vec(), istd: 0, sprms: row_end() },
        Para { text: b"C\x07".to_vec(), istd: 0, sprms: vec![sprm::PIN_TABLE, 1] },
        Para { text: b"D\x07".to_vec(), istd: 0, sprms: vec![sprm::PIN_TABLE, 1] },
        Para { text: b"\x07".to_vec(), istd: 0, sprms: row_end() },
        Para { text: b"A bulleted item\r".to_vec(), istd: 0, sprms: bullet(2) },
        Para { text: marked, istd: 0, sprms: Vec::new() },
        Para { text: b"The end.\r".to_vec(), istd: 0, sprms: Vec::new() },
    ];
    let footnotes = vec![
        Para { text: b"\x02The footnote.\r".to_vec(), istd: 0, sprms: Vec::new() },
        Para { text: b"\r".to_vec(), istd: 0, sprms: Vec::new() },
    ];
    let header = vec![
        Para { text: b"Page \x13 PAGE \x141\x15\r".to_vec(), istd: 0, sprms: Vec::new() },
        Para { text: b"\r".to_vec(), istd: 0, sprms: Vec::new() },
    ];
    let comments = vec![
        Para { text: b"\x05A note on it.\r".to_vec(), istd: 0, sprms: Vec::new() },
        Para { text: b"\r".to_vec(), istd: 0, sprms: Vec::new() },
    ];
    let last = vec![Para { text: b"\r".to_vec(), istd: 0, sprms: Vec::new() }];
    let length = |paras: &[Para]| paras.iter().map(|para| para.text.len() as u32).sum::<u32>();
    let (main_length, footnote_length, header_length, comment_length) =
        (length(&main), length(&footnotes), length(&header), length(&comments));
    let stories = [&main, &footnotes, &header, &comments, &last];

    let mut file = Word6 { stream: vec![0; TEXT_AT as usize], pairs: Vec::new() };
    let mut text = Vec::new();
    for story in stories {
        for para in story.iter() {
            text.extend_from_slice(&para.text);
        }
    }
    file.stream.extend_from_slice(&text);
    let text_end = TEXT_AT + text.len() as u32;

    // Where each paragraph ends, as a file position.
    let mut paragraph_ends = Vec::new();
    let mut at = TEXT_AT;
    for story in stories {
        for para in story.iter() {
            at += para.text.len() as u32;
            paragraph_ends.push((at, para.istd, para.sprms.clone()));
        }
    }

    // The characters with sprms of their own.
    let fc = |cp: usize| TEXT_AT + cp as u32;
    let offset_of =
        |paragraph: usize| main[..paragraph].iter().map(|para| para.text.len()).sum::<usize>();
    let hello_at = offset_of(1);
    let cyrillic_at = offset_of(3);
    let marked_at = offset_of(11);
    let footnote_story = main_length as usize;
    let header_story = footnote_story + footnote_length as usize;
    let comment_story = header_story + header_length as usize;
    let field = |from: usize, length: usize| Chars {
        from: fc(from),
        to: fc(from + length),
        sprms: vec![sprm::CFSPEC, 1],
    };
    let mut chars = vec![
        Chars { from: fc(hello_at + 7), to: fc(hello_at + 11), sprms: vec![sprm::CFBOLD, 1] },
        Chars { from: fc(hello_at + 16), to: fc(hello_at + 22), sprms: vec![sprm::CFITALIC, 1] },
        field(hello_at + note_at, 1),
        Chars {
            from: fc(cyrillic_at),
            to: fc(cyrillic_at + cyrillic.len()),
            sprms: word(sprm::CFTC, 1),
        },
        field(marked_at + comment_at, 1),
        field(footnote_story, 1),
        field(comment_story, 1),
    ];
    chars.sort_by_key(|stretch| stretch.from);

    // The character page: every stretch between one change and the next.
    file.pad_to_page();
    let character_page = file.stream.len() / 512;
    let mut edges = vec![TEXT_AT];
    for stretch in &chars {
        edges.push(stretch.from);
        edges.push(stretch.to);
    }
    edges.push(text_end);
    edges.sort_unstable();
    edges.dedup();
    let mut page = vec![0u8; 512];
    let count = edges.len() - 1;
    for (index, edge) in edges.iter().enumerate() {
        page[index * 4..index * 4 + 4].copy_from_slice(&edge.to_le_bytes());
    }
    let mut free = 511usize;
    for index in 0..count {
        let found = chars.iter().find(|stretch| stretch.from == edges[index]);
        let Some(stretch) = found else { continue };
        free -= stretch.sprms.len() + 1;
        free -= free % 2;
        page[free] = stretch.sprms.len() as u8;
        page[free + 1..free + 1 + stretch.sprms.len()].copy_from_slice(&stretch.sprms);
        page[(count + 1) * 4 + index] = (free / 2) as u8;
    }
    page[511] = count as u8;
    file.stream.extend_from_slice(&page);

    // The paragraph page: an entry of seven bytes for each paragraph.
    let paragraph_page = file.stream.len() / 512;
    let mut page = vec![0u8; 512];
    let count = paragraph_ends.len();
    page[0..4].copy_from_slice(&TEXT_AT.to_le_bytes());
    for (index, (end, ..)) in paragraph_ends.iter().enumerate() {
        page[(index + 1) * 4..(index + 2) * 4].copy_from_slice(&end.to_le_bytes());
    }
    let mut free = 511usize;
    for (index, (_, istd, sprms)) in paragraph_ends.iter().enumerate() {
        let mut data = istd.to_le_bytes().to_vec();
        data.extend_from_slice(sprms);
        if data.len() % 2 == 1 {
            data.push(0);
        }
        free -= data.len() + 1;
        free -= free % 2;
        page[free] = (data.len() / 2) as u8;
        page[free + 1..free + 1 + data.len()].copy_from_slice(&data);
        page[(count + 1) * 4 + index * 7] = (free / 2) as u8;
    }
    page[511] = count as u8;
    file.stream.extend_from_slice(&page);

    // The tables the block names, one after another.
    let normal = style(
        0,
        1,
        0x0FFF,
        "Normal",
        Some((0, &[])),
        &[&word(sprm::CFTC, 0)[..], &word(sprm::CHPS, 24)[..]].concat(),
    );
    let heading_papx = [vec![sprm::PKEEP_FOLLOW, 1], word(sprm::PDYA_BEFORE, 240)].concat();
    let heading = style(
        1,
        1,
        0,
        "heading 1",
        Some((1, &heading_papx)),
        &[&[sprm::CFBOLD, 1][..], &word(sprm::CHPS, 32)[..]].concat(),
    );
    let mut stylesheet = 14u16.to_le_bytes().to_vec();
    for value in [11u16, 8, 1, 11, 11, 0, 0] {
        stylesheet.extend_from_slice(&value.to_le_bytes());
    }
    stylesheet.extend_from_slice(&normal);
    stylesheet.extend_from_slice(&heading);
    for _ in 2..10 {
        stylesheet.extend_from_slice(&0u16.to_le_bytes());
    }
    stylesheet.extend_from_slice(&style(65, 2, 0x0FFF, "Default Paragraph Font", None, &[]));
    file.put(0, &stylesheet);
    file.put(1, &stylesheet);

    let note_cp = (hello_at + note_at) as u32;
    file.put(2, &plc(&[note_cp, main_length], &[1u16.to_le_bytes().to_vec()]));
    file.put(3, &plc(&[0, 15, footnote_length], &[]));
    let comment_cp = (marked_at + comment_at) as u32;
    let mut atrd = pascal(b"KS");
    atrd.resize(10, 0);
    atrd.extend_from_slice(&0u16.to_le_bytes());
    atrd.extend_from_slice(&0u16.to_le_bytes());
    atrd.extend_from_slice(&0u16.to_le_bytes());
    atrd.extend_from_slice(&(-1i32).to_le_bytes());
    file.put(4, &plc(&[comment_cp, main_length], &[atrd]));
    file.put(5, &plc(&[0, 14, comment_length], &[]));

    // The section, and its sprms somewhere in the stream.
    let sepx_at = file.stream.len() as u32;
    let mut sepx = Vec::new();
    for (number, value) in [
        (sprm::SXA_PAGE, 12_240u16),
        (sprm::SYA_PAGE, 15_840),
        (sprm::SDXA_LEFT, 1000),
        (sprm::SDXA_RIGHT, 1100),
        (sprm::SDYA_TOP, 1200),
        (sprm::SDYA_BOTTOM, 1300),
    ] {
        sepx.extend_from_slice(&word(number, value));
    }
    sepx.extend_from_slice(&[sprm::SGPRF_IHDT, 0x02]);
    file.stream.extend_from_slice(&(sepx.len() as u16).to_le_bytes());
    file.stream.extend_from_slice(&sepx);
    if file.stream.len() % 2 == 1 {
        file.stream.push(0);
    }
    let mut sed = 0u16.to_le_bytes().to_vec();
    sed.extend_from_slice(&sepx_at.to_le_bytes());
    sed.extend_from_slice(&0u16.to_le_bytes());
    sed.extend_from_slice(&u32::MAX.to_le_bytes());
    file.put(6, &plc(&[0, main_length], &[sed]));
    // The header: one story, the section's header, and the end.
    file.put(
        11,
        &plc(&[0, header.first().map_or(0, |para| para.text.len() as u32), header_length], &[]),
    );
    file.put(12, &plc(&[TEXT_AT, text_end], &[(character_page as u16).to_le_bytes().to_vec()]));
    file.put(13, &plc(&[TEXT_AT, text_end], &[(paragraph_page as u16).to_le_bytes().to_vec()]));
    let mut fonts = Vec::new();
    for entry in [
        font(0x16, 0, "Times New Roman"),
        font(0x16, 204, "Times New Roman Cyr"),
        font(0x12, 2, "Symbol"),
    ] {
        fonts.extend_from_slice(&entry);
    }
    let mut table = ((fonts.len() + 2) as u16).to_le_bytes().to_vec();
    table.extend_from_slice(&fonts);
    file.put(15, &table);
    // The header's field: where it begins, is divided and ends.
    let page_field = [5, 12, 14, header_length];
    file.put(17, &plc(&page_field, &[vec![0x13, 33], vec![0x14, 0], vec![0x15, 0x80]]));
    // The bookmark over "Marked".
    let mut names = Vec::new();
    names.extend_from_slice(&pascal(b"marked_place"));
    let mut sttbf = ((names.len() + 2) as u16).to_le_bytes().to_vec();
    sttbf.extend_from_slice(&names);
    file.put(21, &sttbf);
    let bookmark_at = marked_at as u32;
    file.put(22, &plc(&[bookmark_at, main_length], &[vec![0, 0, 0, 0]]));
    file.put(23, &plc(&[bookmark_at + 6, main_length], &[]));
    // The document's properties: a default tab stop of half an inch.
    let mut dop = vec![0u8; 84];
    dop[10..12].copy_from_slice(&720u16.to_le_bytes());
    file.put(31, &dop);
    // The comments' author.
    file.put(36, &pascal(b"Kim Smith"));

    // And the block.
    let stream = &mut file.stream;
    let mut set16 =
        |at: usize, value: u16| stream[at..at + 2].copy_from_slice(&value.to_le_bytes());
    set16(0, 0xA5DC);
    set16(2, 104);
    set16(4, 0x6027);
    set16(6, 0x0409);
    set16(0x0C, 101);
    set16(0x18A, character_page as u16);
    set16(0x18C, paragraph_page as u16);
    set16(0x18E, 1);
    set16(0x190, 1);
    let mut set32 =
        |at: usize, value: u32| stream[at..at + 4].copy_from_slice(&value.to_le_bytes());
    set32(0x18, TEXT_AT);
    set32(0x1C, text_end);
    set32(0x34, main_length);
    set32(0x38, footnote_length);
    set32(0x3C, header_length);
    set32(0x44, comment_length);
    for (index, at, length) in file.pairs.clone() {
        let place = if index <= 37 { 0x58 + index * 8 } else { 0x192 + (index - 38) * 8 };
        set32(place, at);
        set32(place + 4, length);
    }
    let length = file.stream.len() as u32;
    file.stream[0x20..0x24].copy_from_slice(&length.to_le_bytes());

    let mut builder = wp_ole::Builder::new();
    builder.stream("WordDocument", file.stream);
    builder.build()
}

/// The file through LibreOffice, as the `.docx` it makes of it.
fn through_libreoffice(doc: &[u8]) -> Document {
    let folder = std::env::temp_dir().join(format!("wp-doc-word6-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let source = folder.join("old.doc");
    std::fs::write(&source, doc).expect("the file written");
    let output = run(Command::new("soffice")
        .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
        .args(["--headless", "--convert-to", "docx:MS Word 2007 XML", "--outdir"])
        .arg(&folder)
        .arg(&source))
    .expect("soffice runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let converted = std::fs::read(folder.join("old.docx")).unwrap_or_else(|error| {
        panic!(
            "LibreOffice made nothing of the file ({error}): {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let _ = std::fs::remove_dir_all(&folder);
    if let Ok(dump) = std::env::var("WP_DOC_DUMP") {
        std::fs::write(format!("{dump}/word6.doc"), doc).expect("dumped");
        std::fs::write(format!("{dump}/word6.docx"), &converted).expect("dumped");
    }
    Document::open(&converted).expect("LibreOffice's docx opens")
}

/// What is looked for, in a document either reader made.
fn check(document: &Document, who: &str) {
    let body = document.body();
    let texts: Vec<String> = body.paragraphs().iter().map(|p| p.plain_text()).collect();
    let paragraph = |text: &str| {
        body.paragraphs()
            .into_iter()
            .find(|paragraph| paragraph.plain_text().replace('\u{2}', "") == text)
            .unwrap_or_else(|| panic!("{who}: no paragraph {text:?} in {texts:?}"))
    };
    let heading = paragraph("Heading one");
    assert_eq!(heading.properties.style.as_deref(), Some("Heading1"), "{who}");

    let hello = paragraph("Plain, bold and italic text.");
    let run = |text: &str| {
        hello
            .runs
            .iter()
            .find(|run| run.plain_text() == text)
            .unwrap_or_else(|| panic!("{who}: no run {text:?}: {:?}", hello.runs))
    };
    assert_eq!(run("bold").properties.bold, Some(true), "{who}");
    assert_eq!(run("italic").properties.italic, Some(true), "{who}");

    let centred = paragraph("Centred and indented.");
    assert_eq!(centred.properties.alignment, Some(Alignment::Center), "{who}");
    assert_eq!(centred.properties.indent_start, Some(720), "{who}");

    paragraph("Привет, мир");

    let table = body
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Table(table) => Some(table),
            Block::Paragraph(_) => None,
        })
        .unwrap_or_else(|| panic!("{who}: no table in {texts:?}"));
    assert_eq!(table.rows.len(), 2, "{who}");
    assert_eq!(table.rows[1].cells[1].blocks[0].plain_text(), "D", "{who}");
    assert!(
        table.rows[0].cells[0].width.is_some_and(|width| (width - 2160).abs() < 30),
        "{who}: {:?}",
        table.rows[0].cells[0].width
    );

    let item = paragraph("A bulleted item");
    let reference = item.properties.numbering.unwrap_or_else(|| panic!("{who}: not in a list"));
    let format =
        document.numbering().level(reference.id, reference.level).map(|level| level.format.clone());
    assert_eq!(format, Some(wp_docx::numbering::NumberFormat::Bullet), "{who}");

    let footnotes = document.notes(wp_docx::notes::Kind::Footnote);
    assert_eq!(footnotes.len(), 1, "{who}: {footnotes:?}");
    assert_eq!(footnotes[0].text, "The footnote.", "{who}");

    let comments = document.comments();
    assert_eq!(comments.len(), 1, "{who}: {comments:?}");
    assert_eq!(comments[0].author, "Kim Smith", "{who}");
    assert_eq!(comments[0].text, "A note on it.", "{who}");

    assert_eq!(document.bookmark_text("marked_place").as_deref(), Some("Marked"), "{who}");
    assert_eq!(document.page_margins(), (1200, 1100, 1300, 1000), "{who}");
    let header = document
        .furniture_of_page(
            wp_docx::furniture::Furniture::Header,
            0,
            wp_docx::furniture::Which::Default,
        )
        .unwrap_or_else(|| panic!("{who}: no header"));
    let field = header.paragraphs()[0].runs.iter().find_map(|run| run.field.clone());
    assert_eq!(field.as_deref().map(str::trim), Some("PAGE"), "{who}: {header:?}");
}

#[test]
fn a_word_95_document_reads_as_libreoffice_reads_it() {
    let file = word6();
    let theirs = through_libreoffice(&file);
    check(&theirs, "LibreOffice");
    let ours = wp_doc::open(&file).expect("opened");
    check(&ours, "this reader");
}

//! Every property the reader keeps, written from the model and read back.
//!
//! # Why a struct literal for each
//!
//! The reader and the writer are two lists of the same properties, kept in
//! two places, and nothing but a test holds them to each other. A run's theme
//! colour, its theme font and its text effect were read into the model and
//! never written from it, and a table's indent and look the same; each was a
//! property somebody added to one list and not the other. So every properties
//! struct is built here with every field named — no `..Default::default()` —
//! and set to something other than what saying nothing gives: a field added
//! to the model later does not compile here until this file names it, and
//! once named, a writer that drops it fails the round trip.
//!
//! # The round trip
//!
//! Through the whole stack, as a document written from a model goes: made
//! with `Document::create`, saved, opened again, and its body read. A prefix
//! the writer forgot to declare is caught on the way, because the file would
//! not open.
//!
//! What the file cannot say is said at the end, each with what comes back
//! instead.

use wp_docx::eastasian::{CombineBrackets, EastAsianLayout};
use wp_docx::effects::{Effect, TextEffect};
use wp_docx::floating::Turned;
use wp_docx::model::{
    Alignment, Block, Body, Border, CellMargins, ChartReference, DiagramReference, FormatChange,
    InkReference, LineRule, LineSpacing, NumberingReference, Paragraph, ParagraphBorders,
    ParagraphProperties, Picture, Revision, RevisionKind, Run, RunContent, RunProperties,
    TabAlignment, TabLeader, TabStop, Table, TableBorders, TableCell, TableFit, TableLook,
    TableRow, TextDirection, Underline, VerticalAlignment,
};
use wp_docx::table_properties::CellAlignment;
use wp_docx::theme::{FontSlot, Slot, ThemeColor};
use wp_docx::typography::{Ligatures, NumberForms, NumberSpacing, OpenType};
use wp_docx::Document;

/// A body written as a new document, saved, opened and read again.
fn through_the_file(body: &Body) -> Body {
    let bytes = Document::create(body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening").body()
}

/// A body of one paragraph holding one run.
fn one_run(run: Run) -> Body {
    Body { blocks: vec![Block::Paragraph(Paragraph::from_runs(vec![run]))] }
}

/// The first run of the first paragraph.
fn first_run(body: &Body) -> &Run {
    let Some(Block::Paragraph(paragraph)) = body.blocks.first() else {
        panic!("no paragraph first: {body:?}")
    };
    paragraph.runs.first().expect("a run")
}

/// The first table of a body.
fn first_table(body: &Body) -> &Table {
    body.blocks
        .iter()
        .find_map(|block| match block {
            Block::Table(table) => Some(&**table),
            Block::Paragraph(_) => None,
        })
        .expect("a table")
}

/// A run of text with these properties and nothing else about it.
fn run_with(properties: RunProperties) -> Run {
    Run {
        properties,
        content: vec![RunContent::Text("words".to_owned())],
        revision: None,
        format_change: None,
        field: None,
    }
}

/// A line with everything a line can say.
fn every_border(color: &str) -> Border {
    Border {
        style: "double".to_owned(),
        size: 12,
        color: Some(color.to_owned()),
        shadow: true,
        frame: true,
    }
}

/// Word 2010's text effect, in a colour of its own.
fn every_effect() -> TextEffect {
    TextEffect { effect: Effect::Glow, color: Some("FF6600".to_owned()) }
}

/// A colour named after the theme, lightened and darkened both.
fn every_theme_color() -> ThemeColor {
    ThemeColor { slot: Slot::Accent1, tint: Some(0x99), shade: Some(0xBF) }
}

fn every_open_type() -> OpenType {
    OpenType {
        ligatures: Ligatures::StandardContextual,
        number_spacing: NumberSpacing::Tabular,
        number_forms: NumberForms::OldStyle,
        stylistic_sets: vec![3, 7],
        contextual_alternates: true,
    }
}

fn every_east_asian_layout() -> EastAsianLayout {
    EastAsianLayout {
        horizontal_in_vertical: true,
        fit_in_line: true,
        two_lines_in_one: true,
        brackets: CombineBrackets::Square,
    }
}

fn every_run_property() -> RunProperties {
    RunProperties {
        style: Some("Emphasis".to_owned()),
        bold: Some(true),
        italic: Some(true),
        strike: Some(true),
        underline: Some(Underline::Double),
        size_half_points: Some(28),
        color: Some("4472C4".to_owned()),
        highlight: Some("yellow".to_owned()),
        vertical_align: Some(VerticalAlignment::Superscript),
        font: Some("Cambria".to_owned()),
        right_to_left: Some(true),
        language: Some("en-GB".to_owned()),
        color_theme: Some(every_theme_color()),
        effect: Some(every_effect()),
        font_theme: Some(FontSlot::Major),
        double_strike: Some(false),
        caps: Some(true),
        small_caps: Some(false),
        hidden: Some(false),
        no_proof: Some(true),
        underline_color: Some("0070C0".to_owned()),
        scale: Some(150),
        spacing_twentieths: Some(-20),
        position_half_points: Some(6),
        kerning_half_points: Some(16),
        open_type: Some(every_open_type()),
        east_asian_layout: Some(every_east_asian_layout()),
    }
}

fn every_paragraph_property() -> ParagraphProperties {
    ParagraphProperties {
        style: Some("Heading1".to_owned()),
        alignment: Some(Alignment::Center),
        right_to_left: Some(true),
        indent_start: Some(720),
        indent_end: Some(360),
        indent_first_line: Some(-360),
        space_before: Some(120),
        space_after: Some(60),
        line_spacing: Some(LineSpacing { value: 300, rule: LineRule::AtLeast }),
        keep_next: Some(true),
        keep_lines: Some(true),
        page_break_before: Some(true),
        widow_control: Some(false),
        outline_level: Some(1),
        numbering: Some(NumberingReference { id: wp_docx::NUMBERED_LIST, level: 2 }),
        borders: ParagraphBorders {
            top: Some(every_border("FF0000")),
            start: Some(every_border("00FF00")),
            bottom: Some(every_border("0000FF")),
            end: Some(every_border("FFFF00")),
            between: Some(every_border("00FFFF")),
        },
        shading: Some("D9E2F3".to_owned()),
        tab_stops: vec![
            TabStop { position: 1440, alignment: TabAlignment::Center, leader: TabLeader::Dot },
            TabStop {
                position: 4320,
                alignment: TabAlignment::Decimal,
                leader: TabLeader::MiddleDot,
            },
        ],
        contextual_spacing: Some(true),
        mirror_indents: Some(true),
        suppress_line_numbers: Some(true),
        no_hyphenation: Some(true),
    }
}

fn every_table_border() -> TableBorders {
    TableBorders {
        top: Some(every_border("FF0000")),
        start: Some(every_border("00FF00")),
        bottom: Some(every_border("0000FF")),
        end: Some(every_border("FFFF00")),
        inside_horizontal: Some(every_border("00FFFF")),
        inside_vertical: Some(every_border("FF00FF")),
    }
}

/// Every one of the six the other way from what a new table has.
fn every_table_look() -> TableLook {
    TableLook {
        first_row: false,
        last_row: true,
        first_column: false,
        last_column: true,
        banded_rows: false,
        banded_columns: true,
    }
}

fn every_table_property() -> Table {
    Table {
        rows: vec![TableRow::text(&["one", "two"]), TableRow::text(&["three", "four"])],
        style: Some("TableGrid".to_owned()),
        look: every_table_look(),
        grid: vec![2400, 3600],
        borders: every_table_border(),
        indent: 360,
        cell_margins: CellMargins {
            top: Some(40),
            start: Some(100),
            bottom: Some(60),
            end: Some(120),
        },
        cell_spacing: Some(20),
        fit: TableFit::Window(80),
    }
}

// --- The run ------------------------------------------------------------------

#[test]
fn every_run_property_comes_back_from_the_file() {
    let wanted = every_run_property();
    let read = through_the_file(&one_run(run_with(wanted.clone())));
    assert_eq!(first_run(&read).properties, wanted);
}

#[test]
fn everything_a_run_is_besides_its_properties_comes_back_from_the_file() {
    // The two a run can be at once: inside a change somebody made, and
    // formatted differently from how it was before. A field is the third,
    // and apart, because a change cannot hold one — see the end of the file.
    let changed = Run {
        properties: RunProperties { bold: Some(true), ..RunProperties::default() },
        content: vec![
            RunContent::Text("before".to_owned()),
            RunContent::Tab,
            RunContent::Break(wp_docx::model::BreakKind::Line),
            RunContent::PositionTab(TabAlignment::End),
            RunContent::Text("after".to_owned()),
        ],
        revision: Some(Revision {
            kind: RevisionKind::Inserted,
            author: "Somebody".to_owned(),
            date: "2026-09-30T12:00:00Z".to_owned(),
            id: 7,
        }),
        format_change: Some(FormatChange {
            author: "Somebody else".to_owned(),
            date: "2026-09-30T13:00:00Z".to_owned(),
            id: 8,
            before: Box::new(every_run_property()),
        }),
        field: None,
    };
    let field = Run {
        properties: RunProperties::default(),
        content: vec![RunContent::Text("3".to_owned())],
        revision: None,
        format_change: None,
        field: Some("PAGE".to_owned()),
    };
    let body = Body {
        blocks: vec![Block::Paragraph(Paragraph::from_runs(vec![changed.clone(), field.clone()]))],
    };

    let read = through_the_file(&body);
    let Some(Block::Paragraph(paragraph)) = read.blocks.first() else { panic!("{read:?}") };
    assert_eq!(paragraph.runs, [changed, field]);
}

// --- The paragraph ------------------------------------------------------------

#[test]
fn every_paragraph_property_comes_back_from_the_file() {
    let wanted = every_paragraph_property();
    let paragraph = Paragraph { properties: wanted.clone(), runs: vec![Run::text("words")] };
    let read = through_the_file(&Body { blocks: vec![Block::Paragraph(paragraph)] });
    let Some(Block::Paragraph(paragraph)) = read.blocks.first() else { panic!("{read:?}") };
    assert_eq!(paragraph.properties, wanted);
}

// --- The table, its rows and its cells ------------------------------------------

#[test]
fn every_table_property_comes_back_from_the_file() {
    let wanted = every_table_property();
    let read = through_the_file(&Body { blocks: vec![Block::Table(Box::new(wanted.clone()))] });
    assert_eq!(*first_table(&read), wanted);
}

#[test]
fn every_row_property_comes_back_from_the_file() {
    let wanted = TableRow {
        cells: vec![TableCell::text("name"), TableCell::text("value")],
        height: Some(567),
        height_exact: true,
        is_header: true,
    };
    let table = Table::from_rows(vec![wanted.clone(), TableRow::text(&["a", "b"])])
        .with_grid(vec![2400, 2400]);
    let read = through_the_file(&Body { blocks: vec![Block::Table(Box::new(table))] });
    assert_eq!(first_table(&read).rows[0], wanted);
}

#[test]
fn every_cell_property_comes_back_from_the_file() {
    // Merged with the one above it, which is only a continuation where there
    // is a cell above to continue: the first row's, across the same two
    // columns.
    let wanted = TableCell {
        blocks: vec![Block::Paragraph(Paragraph::text("continued"))],
        width: Some(4800),
        span: 2,
        merged_upwards: true,
        borders: every_table_border(),
        shading: Some("FFC000".to_owned()),
        direction: TextDirection::Up,
        margins: CellMargins { top: Some(20), start: Some(80), bottom: Some(30), end: Some(90) },
        vertical: CellAlignment::Bottom,
    };
    let table = Table::from_rows(vec![
        TableRow::from_cells(vec![TableCell::text("above").spanning(2)]),
        TableRow::from_cells(vec![wanted.clone()]),
    ])
    .with_grid(vec![2400, 2400]);
    let read = through_the_file(&Body { blocks: vec![Block::Table(Box::new(table))] });
    assert_eq!(first_table(&read).rows[1].cells[0], wanted);
}

// --- What a run points at -------------------------------------------------------

#[test]
fn every_drawing_a_run_points_at_comes_back_from_the_file() {
    // In the line of text, each of them: where one floats is the anchor's,
    // which has tests of its own.
    let picture = Picture {
        relationship: "rId20".to_owned(),
        width_emu: 914_400,
        height_emu: 457_200,
        description: Some("A tree in a field".to_owned()),
        anchor: None,
        turned: Turned { rotation: 5_400_000, flipped_across: true, flipped_down: true },
        link: Some("rId21".to_owned()),
        // Not said by the file: see the end.
        video: false,
    };
    let chart = ChartReference {
        relationship: "rId22".to_owned(),
        width_emu: 5_486_400,
        height_emu: 3_200_400,
    };
    let diagram = DiagramReference {
        relationship: "rId23".to_owned(),
        layout: "rId24".to_owned(),
        style: "rId25".to_owned(),
        colours: "rId26".to_owned(),
        name: "Who reports to whom".to_owned(),
        description: "The team, as a tree".to_owned(),
        width_emu: 5_486_400,
        height_emu: 3_200_400,
    };
    let ink = InkReference {
        relationship: "rId27".to_owned(),
        name: "Signature".to_owned(),
        description: "Signed by hand".to_owned(),
        width_emu: 1_828_800,
        height_emu: 457_200,
        anchor: None,
    };
    let content = vec![
        RunContent::Picture(Box::new(picture)),
        RunContent::Chart(chart),
        RunContent::Diagram(diagram),
        RunContent::Ink(ink),
    ];
    let read = through_the_file(&one_run(Run {
        content: content.clone(),
        ..run_with(RunProperties::default())
    }));
    assert_eq!(first_run(&read).content, content);
}

// --- What the file says in more than one way, or cannot say ---------------------

#[test]
fn a_colour_and_a_font_named_only_after_the_theme_come_back_named_only_after_it() {
    // What Word writes for a heading's font, and for a colour picked from the
    // theme's row of the palette once the colour itself has been taken out:
    // the name alone. `w:color` has to say a value, and says `auto`, which is
    // the reader's nothing.
    let wanted = RunProperties {
        color_theme: Some(ThemeColor { slot: Slot::Accent2, tint: Some(0x66), shade: None }),
        font_theme: Some(FontSlot::Minor),
        ..RunProperties::default()
    };
    let read = through_the_file(&one_run(run_with(wanted.clone())));
    assert_eq!(first_run(&read).properties, wanted);
}

#[test]
fn every_slot_of_the_theme_comes_back_as_itself() {
    // The file has two names for four of them — `text1` and `dark1` are one
    // slot — and the writer writes Word's: the one it comes back as is the
    // same slot either way.
    let runs = Slot::ALL
        .iter()
        .map(|slot| {
            run_with(RunProperties {
                color_theme: Some(ThemeColor { slot: *slot, tint: None, shade: Some(0x80) }),
                ..RunProperties::default()
            })
        })
        .collect::<Vec<_>>();
    let body = Body { blocks: vec![Block::Paragraph(Paragraph::from_runs(runs.clone()))] };
    let read = through_the_file(&body);
    let Some(Block::Paragraph(paragraph)) = read.blocks.first() else { panic!("{read:?}") };
    let slots = |runs: &[Run]| {
        runs.iter().map(|run| run.properties.color_theme.clone()).collect::<Vec<_>>()
    };
    assert_eq!(slots(&paragraph.runs), slots(&runs));
}

#[test]
fn every_effect_the_gallery_offers_comes_back_as_it_went() {
    for effect in Effect::DRAWN {
        let wanted =
            RunProperties { effect: Some(TextEffect::plain(*effect)), ..RunProperties::default() };
        let read = through_the_file(&one_run(run_with(wanted.clone())));
        assert_eq!(first_run(&read).properties, wanted, "{}", effect.label());
    }
}

#[test]
fn an_effect_read_without_a_colour_is_written_in_its_usual_one() {
    // A glow Word drew in one of the theme's colours is read without one:
    // the reader keeps a colour written out and nothing else. Written back
    // with none, it would be a glow of no colour, which Word's schema does
    // not allow; so it takes the colour the gallery gives it.
    let read = through_the_file(&one_run(run_with(RunProperties {
        effect: Some(TextEffect { effect: Effect::Glow, color: None }),
        ..RunProperties::default()
    })));
    assert_eq!(first_run(&read).properties.effect, Some(TextEffect::plain(Effect::Glow)));
}

#[test]
fn every_look_a_table_can_have_comes_back_as_it_went() {
    // All sixty-four, a table each, with an indent each — Word's own tables
    // are set a little into the margin, which is a negative indent.
    let looks: Vec<TableLook> = (0u32..64)
        .map(|bits| TableLook {
            first_row: bits & 1 != 0,
            last_row: bits & 2 != 0,
            first_column: bits & 4 != 0,
            last_column: bits & 8 != 0,
            banded_rows: bits & 16 != 0,
            banded_columns: bits & 32 != 0,
        })
        .collect();
    let mut body = Body::default();
    for (index, look) in looks.iter().enumerate() {
        let mut table = Table::from_rows(vec![TableRow::text(&["a"])]).with_grid(vec![2400]);
        table.look = *look;
        table.indent = index as i32 * 10 - 108;
        body.blocks.push(Block::Table(Box::new(table)));
        body.blocks.push(Block::Paragraph(Paragraph::text("between")));
    }

    let read = through_the_file(&body);
    let tables: Vec<(TableLook, i32)> = read
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Table(table) => Some((table.look, table.indent)),
            Block::Paragraph(_) => None,
        })
        .collect();
    let wanted: Vec<(TableLook, i32)> =
        looks.iter().enumerate().map(|(index, look)| (*look, index as i32 * 10 - 108)).collect();
    assert_eq!(tables, wanted);
}

/// The one `w:tblLook` of a saved document, as its text.
fn written_look(table: &Table) -> String {
    let body = Body { blocks: vec![Block::Table(Box::new(table.clone()))] };
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let xml = package.xml_part("word/document.xml").expect("the body").expect("readable");
    let start = xml.find("<w:tblLook").expect("a look is written");
    let end = start + xml[start..].find("/>").expect("an empty element");
    xml[start..end + 2].to_owned()
}

#[test]
fn a_look_is_written_both_ways_word_writes_it() {
    // What Word writes for a table it has just made: the number Word 2007
    // read, and the six attributes every Word since reads.
    let table = Table::from_rows(vec![TableRow::text(&["a"])]).with_grid(vec![2400]);
    assert_eq!(
        written_look(&table),
        "<w:tblLook w:val=\"04A0\" w:firstRow=\"1\" w:lastRow=\"0\" w:firstColumn=\"1\" \
         w:lastColumn=\"0\" w:noHBand=\"0\" w:noVBand=\"1\"/>"
    );
}

#[test]
fn a_reader_that_knows_only_the_number_sees_the_same_look() {
    // The attributes taken off, as a file Word 2007 wrote would have them.
    let table = every_table_property();
    let body = Body { blocks: vec![Block::Table(Box::new(table.clone()))] };
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let main = "word/document.xml";
    let mut xml = package.xml_part(main).expect("the body").expect("readable");
    for name in ["firstRow", "lastRow", "firstColumn", "lastColumn", "noHBand", "noVBand"] {
        for value in ["0", "1"] {
            xml = xml.replace(&format!(" w:{name}=\"{value}\""), "");
        }
    }
    assert!(xml.contains("<w:tblLook w:val="), "the number went with the attributes: {xml}");
    package.set_part(main, xml.into_bytes());
    let bytes = package.save().expect("saving the package");
    let read = Document::open(&bytes).expect("reopening").body();
    assert_eq!(first_table(&read).look, table.look);
}

#[test]
fn an_empty_look_is_the_look_of_a_table_that_has_none() {
    // `<w:tblLook/>` says nothing, and nothing said is what no element says:
    // Word's new table. It had every switch on, the last row and the last
    // column with the rest.
    let table = Table::from_rows(vec![TableRow::text(&["a"])]).with_grid(vec![2400]);
    let body = Body {
        blocks: vec![
            Block::Table(Box::new(table.clone())),
            Block::Paragraph(Paragraph::text("between")),
            Block::Table(Box::new(table.clone())),
        ],
    };
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let main = "word/document.xml";
    let xml = package.xml_part(main).expect("the body").expect("readable");
    let look = written_look(&table);
    assert_eq!(xml.matches(&look).count(), 2, "{xml}");
    let xml = xml.replacen(&look, "<w:tblLook/>", 1).replacen(&look, "", 1);
    package.set_part(main, xml.into_bytes());
    let bytes = package.save().expect("saving the package");

    let read = Document::open(&bytes).expect("reopening").body();
    let looks: Vec<TableLook> = read
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Table(table) => Some(table.look),
            Block::Paragraph(_) => None,
        })
        .collect();
    assert_eq!(looks, [TableLook::default(), TableLook::default()]);
}

#[test]
fn a_change_cannot_hold_a_field_so_its_runs_keep_the_answer_as_text() {
    // `w:fldSimple` is not something `w:ins` may hold, so a run that is both
    // comes back as the change, with the field's answer in it as words.
    let wanted = Run {
        properties: RunProperties::default(),
        content: vec![RunContent::Text("3".to_owned())],
        revision: Some(Revision {
            kind: RevisionKind::Inserted,
            author: "Somebody".to_owned(),
            date: "2026-09-30T12:00:00Z".to_owned(),
            id: 3,
        }),
        format_change: None,
        field: Some("PAGE".to_owned()),
    };
    let read = through_the_file(&one_run(wanted.clone()));
    assert_eq!(*first_run(&read), Run { field: None, ..wanted });
}

// --- Written into a document that was not made from the model -----------------

#[test]
fn a_run_pasted_with_its_formatting_keeps_its_theme_and_its_effect() {
    // Keep Source Formatting writes the copied runs from the model into the
    // document pasted into. That one says nothing of Word 2010's namespace —
    // it has no effects of its own — so the glow has to say what its prefix
    // means, or the file saved after the paste is not XML and does not open.
    let wanted = RunProperties {
        color_theme: Some(ThemeColor { slot: Slot::Accent1, tint: Some(0x99), shade: None }),
        font_theme: Some(FontSlot::Major),
        effect: Some(TextEffect::plain(Effect::Glow)),
        open_type: Some(every_open_type()),
        ..RunProperties::default()
    };
    let bytes = Document::create(&one_run(run_with(wanted.clone())))
        .expect("the source")
        .save()
        .expect("saving the source");
    let mut source = Document::open(&bytes).expect("reopening the source");
    source.select_all();
    let copied = source.copy_selection();

    let plain = Body { blocks: vec![Block::Paragraph(Paragraph::text("Plain "))] };
    let bytes = Document::create(&plain).expect("the target").save().expect("saving the target");
    let mut target = Document::open(&bytes).expect("reopening the target");
    target.set_caret(wp_docx::TextPosition::new(0, 6));
    assert!(target.paste_blocks(&copied));

    let read = Document::open(&target.save().expect("saving after the paste"))
        .expect("the pasted document opens")
        .body();
    let Some(Block::Paragraph(paragraph)) = read.blocks.first() else { panic!("{read:?}") };
    let pasted = paragraph
        .runs
        .iter()
        .find(|run| run.plain_text() == "words")
        .unwrap_or_else(|| panic!("the pasted run: {paragraph:?}"));
    assert_eq!(pasted.properties, wanted);
}

/// A plain document, as a person would have it open.
fn plain_document() -> Document {
    let body = Body { blocks: vec![Block::Paragraph(Paragraph::text("Plain"))] };
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// A document saved, its styles part parsed on its own — a part that does
/// not parse is not something opening the document would say, since a
/// document with no styles opens with the defaults — and opened again.
fn saved_with_styles_that_parse(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let styles = package.xml_part("word/styles.xml").expect("a styles part").expect("readable");
    if let Err(error) = wp_xml::tree::XmlTree::parse(&styles) {
        panic!("the styles part does not parse: {error:?}\n{styles}");
    }
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_style_asking_for_ligatures_comes_back_from_the_file() {
    // Set from its definition, into a styles part whose root says nothing of
    // Word 2010's namespace. The ligatures and the glow were written with a
    // prefix nobody declared, the part did not parse, and the document came
    // back with no styles of its own at all.
    let wanted = RunProperties {
        open_type: Some(OpenType { ligatures: Ligatures::Standard, ..OpenType::default() }),
        effect: Some(TextEffect::plain(Effect::Glow)),
        ..RunProperties::default()
    };
    let mut document = plain_document();
    assert!(document.set_style(&wp_docx::StyleDefinition {
        id: "Ligated".to_owned(),
        name: "Ligated".to_owned(),
        based_on: Some("Normal".to_owned()),
        next: None,
        paragraph: ParagraphProperties::default(),
        run: wanted.clone(),
    }));

    let reopened = saved_with_styles_that_parse(&document);
    let style = reopened.styles().get("Ligated").expect("the style came back");
    assert_eq!(style.run.open_type, wanted.open_type);
    assert_eq!(style.run.effect, wanted.effect);
}

#[test]
fn a_style_with_ligatures_copied_from_another_document_comes_back_from_the_file() {
    // As Word writes one: the prefix declared on the styles part's root and
    // nowhere else, which the Organizer's copy does not bring with it.
    let bytes = plain_document().save().expect("saving the source");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let part = "word/styles.xml";
    let styles = package.xml_part(part).expect("the styles").expect("readable");
    let declared = styles.replacen(
        "<w:styles ",
        "<w:styles xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" ",
        1,
    );
    let with_style = declared.replacen(
        "</w:styles>",
        "<w:style w:type=\"character\" w:styleId=\"Ligated\"><w:name w:val=\"Ligated\"/>\
         <w:rPr><w14:ligatures w14:val=\"standard\"/></w:rPr></w:style></w:styles>",
        1,
    );
    assert_ne!(with_style, styles, "the source's styles were not changed");
    package.set_part(part, with_style.into_bytes());
    let source = Document::open(&package.save().expect("saving the package")).expect("the source");
    assert!(source.styles().get("Ligated").is_some(), "the source has no such style");

    let mut document = plain_document();
    assert!(document.copy_style_from(&source, "Ligated"));

    let reopened = saved_with_styles_that_parse(&document);
    let style = reopened.styles().get("Ligated").expect("the style came back");
    assert_eq!(
        style.run.open_type,
        Some(OpenType { ligatures: Ligatures::Standard, ..OpenType::default() })
    );
}

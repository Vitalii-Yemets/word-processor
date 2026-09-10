//! Word's Table Style Options: which parts of a table its style may dress up.
//!
//! # Why these were the only two buttons that said they did nothing
//!
//! Because on their own they cannot do anything. Ticking "Header Row" does not
//! shade the first row: it says the *style* may treat the first row specially,
//! and if the style says nothing about first rows then nothing happens — which
//! is true in Word too. What was missing was the other half: table styles that
//! carry conditional formatting, and a layout that applies it.
//!
//! Both halves are here now. The switches write `w:tblLook`
//! ([`wp_docx::model::TableLook`]), the styles carry `w:tblStylePr` for each
//! part ([`wp_docx::styles::Conditional`]), and the layout works out which
//! parts each cell is in and shades it accordingly.
//!
//! # The gallery
//!
//! Word's Design tab opens on a gallery of table styles. The ones offered here
//! are written into the document when they are chosen, because a style has to
//! exist in `styles.xml` before a table can name it — a document from elsewhere
//! brings its own, and those are offered too.

use wp_docx::model::TableLook;
use wp_docx::styles::Conditional;
use wp_docx::tablestyles::{Line, Part, TableStyle};
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

impl Editor {
    /// Turns one of Word's six Table Style Options on or off.
    pub(super) fn toggle_table_look(&mut self, command: Command) -> Response {
        let Some(look) = self.document.table_look() else {
            return self.report("Put the caret in a table first");
        };

        let wanted = match command {
            Command::TableHeaderRow => TableLook { first_row: !look.first_row, ..look },
            Command::TableTotalRow => TableLook { last_row: !look.last_row, ..look },
            Command::TableFirstColumn => TableLook { first_column: !look.first_column, ..look },
            Command::TableLastColumn => TableLook { last_column: !look.last_column, ..look },
            Command::TableBandedRows => TableLook { banded_rows: !look.banded_rows, ..look },
            Command::TableBandedColumns => {
                TableLook { banded_columns: !look.banded_columns, ..look }
            }
            _ => return Response::Ignored,
        };

        let changed = self.document.set_table_look(wanted);
        self.relayout();
        self.edited(changed, &format!("{} {}", name_of(command), on_or_off(wanted, command)))
    }
}

/// What one of them is called, for the strip along the bottom.
fn name_of(command: Command) -> &'static str {
    match command {
        Command::TableHeaderRow => "Header row",
        Command::TableTotalRow => "Total row",
        Command::TableFirstColumn => "First column",
        Command::TableLastColumn => "Last column",
        Command::TableBandedRows => "Banded rows",
        Command::TableBandedColumns => "Banded columns",
        _ => "",
    }
}

fn on_or_off(look: TableLook, command: Command) -> &'static str {
    let on = match command {
        Command::TableHeaderRow => look.first_row,
        Command::TableTotalRow => look.last_row,
        Command::TableFirstColumn => look.first_column,
        Command::TableLastColumn => look.last_column,
        Command::TableBandedRows => look.banded_rows,
        Command::TableBandedColumns => look.banded_columns,
        _ => false,
    };
    if on {
        "on"
    } else {
        "off"
    }
}

/// The table styles the gallery offers.
///
/// Word's own, by Word's identifiers and names, so that a table given one here
/// arrives in Word as the style it says it is. Their colours are Word's Blue
/// accent, which is the accent this program's default theme uses.
const GALLERY: &[TableStyle] = &[
    TableStyle {
        id: "TableGrid",
        name: "Table Grid",
        lines: Some(Line { style: "single", size: 4, color: "auto" }),
        parts: &[],
    },
    TableStyle {
        id: "PlainTable1",
        name: "Plain Table 1",
        lines: Some(Line { style: "single", size: 4, color: "BFBFBF" }),
        parts: &[
            (Conditional::FirstRow, Part { bold: true, ..PLAIN }),
            (Conditional::FirstColumn, Part { bold: true, ..PLAIN }),
            (Conditional::Band1Horizontal, Part { shading: Some("F2F2F2"), ..PLAIN }),
        ],
    },
    TableStyle {
        id: "GridTable1Light",
        name: "Grid Table 1 Light",
        lines: Some(Line { style: "single", size: 4, color: "9CC3E5" }),
        parts: &[
            (Conditional::FirstRow, Part { bold: true, ..PLAIN }),
            (Conditional::FirstColumn, Part { bold: true, ..PLAIN }),
        ],
    },
    TableStyle {
        id: "GridTable4Accent1",
        name: "Grid Table 4 – Accent 1",
        lines: Some(Line { style: "single", size: 4, color: "8EAADB" }),
        parts: &[
            // A header row in the accent colour with white text on it, which
            // is what makes this the one everybody picks.
            (
                Conditional::FirstRow,
                Part { shading: Some("4472C4"), bold: true, color: Some("FFFFFF") },
            ),
            (Conditional::FirstColumn, Part { bold: true, ..PLAIN }),
            (Conditional::Band1Horizontal, Part { shading: Some("D9E2F3"), ..PLAIN }),
        ],
    },
    TableStyle {
        id: "ListTable3Accent1",
        name: "List Table 3 – Accent 1",
        lines: None,
        parts: &[
            (
                Conditional::FirstRow,
                Part { shading: Some("4472C4"), bold: true, color: Some("FFFFFF") },
            ),
            (Conditional::FirstColumn, Part { bold: true, ..PLAIN }),
            (Conditional::Band1Horizontal, Part { shading: Some("D9E2F3"), ..PLAIN }),
        ],
    },
];

/// A part that says nothing, so the ones above can name only what they change.
const PLAIN: Part = Part { shading: None, bold: false, color: None };

impl Editor {
    /// Drops open the gallery of table styles.
    pub(super) fn open_table_styles(&mut self) -> Response {
        if self.close_popup_if(Choice::TableStyle) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::TableStyles) else {
            return Response::Ignored;
        };

        let here = self.document.table_style();
        let mut items: Vec<String> = vec!["None".to_owned()];
        items.extend(GALLERY.iter().map(|style| style.name.to_owned()));
        let current = match here.as_deref() {
            None => Some(0),
            Some(id) => GALLERY.iter().position(|style| style.id == id).map(|at| at + 1),
        };

        let rows = items.iter().map(|_| crate::chrome::popup::Row::default()).collect();
        self.popup =
            Some(Popup::new(Choice::TableStyle, items, current, left, top, 240.0).with_rows(rows));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Gives the table at the caret whichever style was picked.
    pub(super) fn choose_table_style(&mut self, index: usize) -> Response {
        self.popup = None;
        if self.document.table_here().is_none() {
            return self.report("Put the caret in a table first");
        }

        let Some(index) = index.checked_sub(1) else {
            let changed = self.document.set_table_style(None);
            self.relayout();
            return self.edited(changed, "Table style removed");
        };
        let Some(style) = GALLERY.get(index) else { return Response::Ignored };

        // The definition has to be in the document before a table can name it.
        self.document.add_table_style(style);
        let changed = self.document.set_table_style(Some(style.id));
        self.relayout();
        self.edited(changed, style.name)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// A document with a table in it, the caret inside the table.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.insert_table(3, 3);
        editor.relayout();
        editor
    }

    #[test]
    fn a_new_table_has_what_word_gives_one() {
        let editor = editor();
        let look = editor.document.table_look().expect("the caret is in a table");
        assert!(look.first_row, "a new table has a header row");
        assert!(look.banded_rows, "a new table has banded rows");
        assert!(!look.last_row, "a new table has no total row");
    }

    #[test]
    fn each_switch_turns_its_own_thing_on_and_off() {
        let mut editor = editor();
        for command in [
            Command::TableHeaderRow,
            Command::TableTotalRow,
            Command::TableFirstColumn,
            Command::TableLastColumn,
            Command::TableBandedRows,
            Command::TableBandedColumns,
        ] {
            let before = on_or_off(editor.document.table_look().expect("a table"), command);
            editor.toggle_table_look(command);
            let now = on_or_off(editor.document.table_look().expect("a table"), command);
            assert_ne!(now, before, "{command:?} did not turn over");
            editor.toggle_table_look(command);
            let back = on_or_off(editor.document.table_look().expect("a table"), command);
            assert_eq!(back, before, "{command:?} did not come back");
        }
    }

    #[test]
    fn the_switches_survive_being_saved_and_opened_again() {
        let mut editor = editor();
        editor.toggle_table_look(Command::TableBandedRows);
        editor.toggle_table_look(Command::TableLastColumn);

        let saved = editor.document.save().expect("saving");
        let mut reopened = Document::open(&saved).expect("reopening");
        reopened.set_caret(editor.document.caret());
        let look = reopened.table_look().expect("a table");
        assert!(!look.banded_rows, "the bands came back on");
        assert!(look.last_column, "the last column did not survive");
    }

    #[test]
    fn a_switch_outside_a_table_says_so_rather_than_doing_nothing_quietly() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.toggle_table_look(Command::TableHeaderRow);
        assert!(editor.status.contains("table"), "got {:?}", editor.status);
    }

    /// How many cells of the first page are shaded.
    fn shaded(editor: &Editor) -> usize {
        editor.pages.first().map_or(0, |page| page.decorations.len())
    }

    #[test]
    fn a_style_from_the_gallery_goes_into_the_document_and_onto_the_table() {
        let mut editor = editor();
        // "Grid Table 4 – Accent 1", which is the one with a coloured header.
        editor.choose_table_style(4);

        assert_eq!(editor.document.table_style().as_deref(), Some("GridTable4Accent1"));
        assert!(editor.document.has_style("GridTable4Accent1"), "the definition was not written");
    }

    /// What colour the first cell of the table is drawn on.
    ///
    /// The decorations are pushed row by row and cell by cell, so the first of
    /// them belongs to the first cell of the first row.
    fn first_cell_colour(editor: &Editor) -> Option<wp_raster::Color> {
        editor.pages.first()?.decorations.first().map(|shading| shading.color)
    }

    #[test]
    fn a_header_row_is_shaded_only_while_the_switch_says_it_may_be() {
        // The whole point of the item: the switch was one of the only two
        // buttons in the program that said they did nothing, because nothing
        // carried the other half of the answer.
        let mut editor = editor();
        editor.choose_table_style(4);
        let header = first_cell_colour(&editor).expect("the header row is shaded");
        assert_eq!(header, wp_raster::Color::from_hex("4472C4").expect("a colour"));

        // With the switch off the first row is an ordinary row, and in this
        // style an ordinary first row is the first band.
        editor.toggle_table_look(Command::TableHeaderRow);
        let plain = first_cell_colour(&editor);
        assert_ne!(plain, Some(header), "the header stayed a header");

        editor.toggle_table_look(Command::TableHeaderRow);
        assert_eq!(first_cell_colour(&editor), Some(header), "it did not come back");
    }

    #[test]
    fn banded_rows_shade_every_other_row_and_stop_when_told_to() {
        let mut editor = editor();
        editor.choose_table_style(4);
        let banded = shaded(&editor);

        editor.toggle_table_look(Command::TableBandedRows);
        let plain = shaded(&editor);
        assert!(plain < banded, "the bands did not go away");
    }

    #[test]
    fn a_style_the_document_already_has_is_left_as_it_is() {
        // A document from Word carries Word's own definition, and overwriting
        // it would change how that document looks in the program it was made
        // in.
        let mut editor = editor();
        editor.choose_table_style(1);
        let first = editor.document.save().expect("saving");

        editor.choose_table_style(1);
        let again = editor.document.save().expect("saving");
        let styles = |bytes: &[u8]| {
            Document::open(bytes)
                .expect("reopening")
                .styles()
                .all()
                .iter()
                .filter(|style| style.id == "TableGrid")
                .count()
        };
        assert_eq!(styles(&first), 1);
        assert_eq!(styles(&again), 1, "the style was written twice");
    }

    #[test]
    fn none_takes_the_style_off_again() {
        let mut editor = editor();
        editor.choose_table_style(4);
        assert!(editor.document.table_style().is_some());

        editor.choose_table_style(0);
        assert!(editor.document.table_style().is_none(), "the style stayed on");
    }

    #[test]
    fn shading_inside_a_table_colours_the_cell_and_not_the_paragraph() {
        // A colour on the paragraph would stop at the ends of the text; a cell
        // is what a table is made of, and Word colours the cell.
        let mut editor = editor();
        editor.apply_color(crate::chrome::palette::Kind::Shading, Some("FFFF00"), "Yellow");

        assert_eq!(editor.document.cell_shading().as_deref(), Some("FFFF00"));

        let body = editor.document.body();
        let table = body.blocks.iter().find_map(|block| match block {
            Block::Table(table) => Some(table),
            Block::Paragraph(_) => None,
        });
        let cell = &table.expect("a table").rows[0].cells[0];
        assert_eq!(cell.shading.as_deref(), Some("FFFF00"), "the cell did not take the colour");
        let Block::Paragraph(inside) = &cell.blocks[0] else { panic!("a paragraph") };
        assert_eq!(inside.properties.shading, None, "it coloured the paragraph");
    }

    #[test]
    fn a_cell_colour_is_drawn_and_can_be_taken_off_again() {
        let mut editor = editor();
        editor.apply_color(crate::chrome::palette::Kind::Shading, Some("FFFF00"), "Yellow");
        assert!(shaded(&editor) > 0, "the colour was not drawn");

        editor.apply_color(crate::chrome::palette::Kind::Shading, None, "No Color");
        assert_eq!(editor.document.cell_shading(), None, "the colour stayed");
    }
}

//! The Print page: opening it, driving it, and printing what it says.
//!
//! Word's Ctrl+P does not open a dialog over the document; it turns the whole
//! window into a page about printing, with the settings down the left and the
//! document as it will come out filling the rest. This is that page's
//! behaviour — the drawing is [`crate::chrome::printpane`].
//!
//! The preview is not a photograph of the window. It is the document laid out
//! for the printer that is going to print it, drawn small: the same pages, the
//! same line breaks, the same number of them. That is what makes it a preview
//! rather than an illustration.

use wp_layout::{Device, LayoutEngine, Page, Renderer};
use wp_raster::{Canvas, Color, Transform};
use wp_shell::Response;

use crate::chrome::popup::{Choice, Popup};
use crate::chrome::printpane::{self, Hit, PaneState, Preview, PrintPane, Sides, Which, PER_SHEET};

use super::Editor;
/// The colour a sheet is drawn in, both in the preview and on paper.
const PAPER: Color = Color::rgb(255, 255, 255);

/// The printer that is not a printer: choosing it writes a PDF.
///
/// Word has one of these — "Microsoft Print to PDF" — and a person wanting a
/// PDF looks for it where they look for paper.
pub(super) const TO_PDF: &str = "Save as PDF";

/// What the save dialog offers when it is a PDF being saved.
const PDF_FILTERS: &[wp_shell::dialog::FileFilter] = &[
    wp_shell::dialog::FileFilter { label: "PDF documents (*.pdf)", pattern: "*.pdf" },
    wp_shell::dialog::FileFilter { label: "All files (*.*)", pattern: "*.*" },
];

impl Editor {
    /// Opens the Print page, or closes it if it is already open.
    pub(super) fn open_print(&mut self) -> Response {
        if self.print_pane.is_some() {
            return self.close_print();
        }

        self.printer_name = wp_shell::printing::default_name().unwrap_or_else(|| TO_PDF.to_owned());
        self.print_device = self.ask_the_printer();
        let mut pane = PrintPane::new();
        pane.page = self.caret_page();
        self.print_pane = Some(pane);
        self.print_preview = self.layout_for_print(Device::screen());
        self.needs_redraw = true;
        Response::Redraw
    }

    /// What the chosen printer says about itself.
    ///
    /// Opening it and letting it go again is the only way to ask; the answer is
    /// kept so that the page can say whether the margins fall where the printer
    /// cannot reach. Nothing to ask means a screen, which reaches everywhere.
    fn ask_the_printer(&self) -> Device {
        if self.printer_name.is_empty() {
            return Device::screen();
        }
        let Some(printer) = wp_shell::printing::open(&self.printer_name, None) else {
            return Device::screen();
        };
        let paper = printer.page();
        let (left, top, right, bottom) = paper.unprintable();
        Device::from_dots(paper.dpi_x, left, top, right, bottom)
    }
    /// Goes back to the document.
    pub(super) fn close_print(&mut self) -> Response {
        if self.print_pane.take().is_none() {
            return Response::Ignored;
        }
        self.print_preview.clear();
        self.popup = None;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Whether the Print page is what the window is showing.
    #[must_use]
    pub(super) fn printing(&self) -> bool {
        self.print_pane.is_some()
    }

    /// The document laid out for the printer it is going to.
    ///
    /// At the screen's resolution rather than the printer's: the two break
    /// their lines and their pages in the same places, and a preview does not
    /// need six hundred dots to the inch to be looked at.
    pub(super) fn layout_for_print(&mut self, device: Device) -> Vec<Page> {
        let markup = self.print_markup();
        let mut engine = LayoutEngine::for_device(self.library, device).with_markup(markup);

        // Printing a selection lays the selection out on its own, as Word does:
        // one sheet with the chosen paragraphs on it, rather than the pages
        // they happen to fall on.
        if self.printing_selection() {
            let body = wp_docx::model::Body { blocks: self.document.copy_selection() };
            let metrics = wp_layout::PageMetrics::from_document(&self.document);
            return engine.layout_body(&body, &self.document, metrics);
        }
        engine.layout_document(&self.document)
    }

    /// Whether the job is the selection rather than the document.
    fn printing_selection(&self) -> bool {
        self.print_pane.as_ref().is_some_and(|pane| pane.settings.which == Which::Selection)
            && self.document.selection().is_some()
    }

    /// Whether the preview and the printer show the tracked changes.
    fn print_markup(&self) -> bool {
        self.print_pane.as_ref().is_some_and(|pane| pane.settings.markup)
    }

    /// What the page says about the document.
    fn pane_state(&self) -> PaneState {
        let showing = self.print_pane.as_ref().map_or(1, |pane| pane.page);
        let paper = self.document.page_size_name().unwrap_or("Custom size").to_owned();
        let margins = self.document.margin_preset_name().unwrap_or("Custom margins").to_owned();

        PaneState {
            printer: if self.printer_name.is_empty() {
                "No printer".to_owned()
            } else {
                self.printer_name.clone()
            },
            pages: self.print_preview.len(),
            showing: showing.clamp(1, self.print_preview.len().max(1)),
            orientation: if self.document.is_landscape() {
                "Landscape Orientation".to_owned()
            } else {
                "Portrait Orientation".to_owned()
            },
            paper: format!("{paper}  {}", self.document.page_size_note()),
            margins: format!("{margins} Margins"),
            margin_warning: !self
                .print_device
                .holds(&wp_layout::PageMetrics::from_document(&self.document)),
        }
    }

    /// Draws the whole page, preview and all.
    pub(super) fn draw_print_pane(&mut self) {
        let Some(mut pane) = self.print_pane.take() else { return };
        let state = self.pane_state();
        let theme = self.theme;
        let top = crate::chrome::TITLE_HEIGHT;

        let preview = pane.draw(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            &state,
            top,
            &theme,
        );
        self.print_pane = Some(pane);
        self.draw_preview(preview);
    }

    /// Draws the sheet as it will come out of the printer.
    fn draw_preview(&mut self, preview: Preview) {
        let Some(pane) = &self.print_pane else { return };
        let per_sheet = pane.settings.per_sheet;
        let zoom = pane.zoom;
        let showing = pane.page;

        let chosen = self.chosen_pages();
        let Some(first) = chosen.iter().position(|page| *page >= showing).or(Some(0)) else {
            return;
        };
        let sheet = first / per_sheet.max(1);
        let on_this_sheet: Vec<usize> =
            chosen.iter().skip(sheet * per_sheet).take(per_sheet).copied().collect();
        let Some(sample) = self.print_preview.first() else { return };

        // The sheet, drawn as large as it fits with room to breathe.
        let scale =
            (preview.width / sample.width).min(preview.height / sample.height).min(2.0) * zoom;
        let width = sample.width * scale;
        let height = sample.height * scale;
        let left = preview.left + (preview.width - width) / 2.0;
        let top = preview.top + (preview.height - height) / 2.0;

        self.canvas.fill_rect(left as i32, top as i32, width as i32, height as i32, PAPER);
        let edge = self.theme.page_edge;
        outline(&mut self.canvas, left, top, width, height, edge);

        for (place, number) in
            printpane::arrangement(per_sheet, width, height).iter().zip(&on_this_sheet)
        {
            let Some(page) = self.print_preview.get(number.saturating_sub(1)) else { continue };
            let (cell_x, cell_y, cell_width, cell_height) = *place;
            // Inside its cell, and no larger than the cell.
            let inside = (cell_width / page.width).min(cell_height / page.height);
            let transform = Transform::scale(inside, inside)
                .then(&Transform::translate(left + cell_x, top + cell_y));
            self.renderer.draw_transformed(&mut self.canvas, page, &transform);
        }
    }

    /// The pages the settings say to print, counted from one.
    fn chosen_pages(&self) -> Vec<usize> {
        let Some(pane) = &self.print_pane else { return Vec::new() };
        pane.settings.chosen(self.print_preview.len(), pane.page)
    }

    /// Follows the pointer over the page.
    pub(super) fn print_pane_hover(&mut self, x: i32, y: i32) -> bool {
        self.print_pane.as_mut().is_some_and(|pane| pane.hover(x, y))
    }

    /// Reacts to a press on the page.
    pub(super) fn print_pane_press(&mut self, x: i32, y: i32) -> Response {
        let Some(pane) = &mut self.print_pane else { return Response::Ignored };
        let Some(hit) = pane.hit(x, y) else {
            pane.typing_pages = false;
            self.needs_redraw = true;
            return Response::Redraw;
        };
        pane.typing_pages = hit == Hit::Pages;

        match hit {
            Hit::Back => self.close_print(),
            Hit::Print => self.print_now(),
            Hit::MoreCopies => {
                pane.settings.copies = pane.settings.copies.saturating_add(1).min(999);
                self.redrawn()
            }
            Hit::FewerCopies => {
                pane.settings.copies = pane.settings.copies.saturating_sub(1).max(1);
                self.redrawn()
            }
            Hit::Previous => {
                pane.page = pane.page.saturating_sub(1).max(1);
                self.redrawn()
            }
            Hit::Next => {
                let pages = self.print_preview.len().max(1);
                if let Some(pane) = &mut self.print_pane {
                    pane.page = (pane.page + 1).min(pages);
                }
                self.redrawn()
            }
            Hit::ZoomIn => {
                pane.zoom = (pane.zoom * 1.25).min(4.0);
                self.redrawn()
            }
            Hit::ZoomOut => {
                pane.zoom = (pane.zoom / 1.25).max(0.25);
                self.redrawn()
            }
            Hit::Pages => {
                pane.settings.which = Which::Custom;
                self.redrawn()
            }
            Hit::Markup => {
                pane.settings.markup = !pane.settings.markup;
                self.print_preview = self.layout_for_print(Device::screen());
                self.redrawn()
            }
            Hit::Collation => {
                pane.settings.collated = !pane.settings.collated;
                self.redrawn()
            }
            other => self.open_print_choice(other),
        }
    }

    /// Marks the window for redrawing and says so.
    fn redrawn(&mut self) -> Response {
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Drops open the list belonging to one of the settings.
    fn open_print_choice(&mut self, hit: Hit) -> Response {
        let Some(pane) = &self.print_pane else { return Response::Ignored };
        let Some((left, top, width)) = pane.rect_of(hit) else { return Response::Ignored };

        let (choice, items, current) = match hit {
            Hit::Printer => {
                // The PDF goes at the end of the list, where Word keeps its own
                // one, and is there even when the machine has no printer at all.
                let mut names = wp_shell::printing::names();
                names.push(TO_PDF.to_owned());
                let current = names.iter().position(|name| *name == self.printer_name);
                (Choice::Printer, names, current)
            }
            Hit::Which => {
                // Printing a selection is offered only when there is one, as
                // Word offers it: the setting is greyed out otherwise.
                let has_selection = self.document.selection().is_some();
                let offered: Vec<Which> = Which::ALL
                    .iter()
                    .copied()
                    .filter(|which| has_selection || *which != Which::Selection)
                    .collect();
                let items = offered.iter().map(|which| which.label().to_owned()).collect();
                let current = offered.iter().position(|which| *which == pane.settings.which);
                (Choice::PrintWhich, items, current)
            }
            Hit::Sides => {
                // Only a printer that can turn the paper over is asked to: a
                // list of one is not a choice, and Word greys it out too.
                if !wp_shell::printing::prints_both_sides(&self.printer_name) {
                    return Response::Ignored;
                }
                let items = Sides::ALL.iter().map(|sides| sides.label().to_owned()).collect();
                let current = Sides::ALL.iter().position(|sides| *sides == pane.settings.sides);
                (Choice::PrintSides, items, current)
            }
            Hit::PerSheet => {
                let items = PER_SHEET
                    .iter()
                    .map(|many| match many {
                        1 => "1 Page Per Sheet".to_owned(),
                        many => format!("{many} Pages Per Sheet"),
                    })
                    .collect();
                let current = PER_SHEET.iter().position(|many| *many == pane.settings.per_sheet);
                (Choice::PrintPerSheet, items, current)
            }
            Hit::Orientation => {
                let items =
                    vec!["Portrait Orientation".to_owned(), "Landscape Orientation".to_owned()];
                (Choice::Orientation, items, Some(usize::from(self.document.is_landscape())))
            }
            Hit::Paper => {
                let items =
                    wp_docx::page::PAGE_SIZES.iter().map(|(name, ..)| (*name).to_owned()).collect();
                (Choice::Paper, items, None)
            }
            Hit::Margins => {
                let items = wp_docx::page::MARGIN_PRESETS
                    .iter()
                    .map(|(name, ..)| format!("{name} Margins"))
                    .collect();
                (Choice::Margin, items, None)
            }
            _ => return Response::Ignored,
        };

        if items.is_empty() {
            return Response::Ignored;
        }
        self.popup_anchor = Some((left, top, width));
        self.popup = Some(Popup::new(choice, items, current, left, top, width));
        self.redrawn()
    }

    /// Takes one of those choices.
    pub(super) fn choose_print_setting(&mut self, choice: Choice, index: usize) -> Response {
        self.popup = None;
        self.popup_anchor = None;
        let Some(pane) = &mut self.print_pane else { return Response::Ignored };

        match choice {
            Choice::Printer => {
                let mut names = wp_shell::printing::names();
                names.push(TO_PDF.to_owned());
                if let Some(name) = names.get(index) {
                    self.printer_name = name.clone();
                }
            }
            Choice::PrintWhich => {
                let has_selection = self.document.selection().is_some();
                let offered: Vec<Which> = Which::ALL
                    .iter()
                    .copied()
                    .filter(|which| has_selection || *which != Which::Selection)
                    .collect();
                if let Some(which) = offered.get(index) {
                    if let Some(pane) = &mut self.print_pane {
                        pane.settings.which = *which;
                    }
                    self.print_preview = self.layout_for_print(Device::screen());
                }
            }
            Choice::PrintSides => {
                if let Some(sides) = Sides::ALL.get(index) {
                    pane.settings.sides = *sides;
                }
            }
            Choice::PrintPerSheet => {
                if let Some(many) = PER_SHEET.get(index) {
                    pane.settings.per_sheet = *many;
                }
            }
            _ => return Response::Ignored,
        }
        self.redrawn()
    }

    /// Types into the box the pages go in.
    pub(super) fn print_pane_character(&mut self, character: char) -> bool {
        let Some(pane) = &mut self.print_pane else { return false };
        if !pane.typing_pages {
            return false;
        }
        match character {
            '\u{8}' => {
                pane.settings.pages.pop();
            }
            '\r' | '\n' => pane.typing_pages = false,
            character if !character.is_control() => pane.settings.pages.push(character),
            _ => return false,
        }
        pane.settings.which = Which::Custom;
        self.needs_redraw = true;
        true
    }

    /// Turns to the next sheet of the preview, or the one before.
    pub(super) fn turn_print_page(&mut self, forwards: bool) -> Response {
        let pages = self.print_preview.len().max(1);
        let Some(pane) = &mut self.print_pane else { return Response::Ignored };
        pane.page =
            if forwards { (pane.page + 1).min(pages) } else { pane.page.saturating_sub(1).max(1) };
        self.redrawn()
    }

    /// Sends the job, which is what the Print button and Enter both do.
    pub(super) fn start_printing(&mut self) -> Response {
        self.print_now()
    }

    /// Writes the document out as a PDF instead of sending it to a printer.
    ///
    /// Word has a printer of its own called "Microsoft Print to PDF", and a
    /// person looking for a PDF looks in the same place they look for paper.
    /// This is that: the same settings, the same pages, written to a file.
    fn write_pdf(&mut self, chosen: &[usize]) -> Response {
        let name = self.document_name();
        let suggested = std::path::PathBuf::from(format!("{name}.pdf"));
        let Some(path) = wp_shell::dialog::save_file("Save as PDF", PDF_FILTERS, Some(&suggested))
        else {
            self.status = String::from("Not saved");
            return self.redrawn();
        };

        let all = self.layout_for_print(Device::paper());
        // Only the pages that were asked for, in the order they were asked
        // for — the same list the preview was showing.
        let pages: Vec<wp_layout::Page> =
            chosen.iter().filter_map(|number| all.get(number.saturating_sub(1)).cloned()).collect();

        let bytes = wp_pdf::write(&pages, self.library, &name);
        match std::fs::write(&path, &bytes) {
            Ok(()) => {
                self.status = format!("Saved {} pages to {}", pages.len(), path.display());
                self.close_print()
            }
            Err(error) => {
                wp_shell::dialog::show_error(&format!("Cannot write {}: {error}", path.display()));
                self.status = String::from("Not saved");
                self.redrawn()
            }
        }
    }

    /// Sends the job to the printer the page names.
    fn print_now(&mut self) -> Response {
        let settings = match &self.print_pane {
            Some(pane) => pane.settings.clone(),
            None => return Response::Ignored,
        };
        let chosen = self.chosen_pages();
        if chosen.is_empty() {
            self.status = String::from("There are no pages to print");
            return self.redrawn();
        }
        if self.printer_name == TO_PDF {
            return self.write_pdf(&chosen);
        }

        let opened = if self.printer_name.is_empty() {
            wp_shell::printing::choose()
        } else {
            wp_shell::printing::open(&self.printer_name, settings.sides.both_sides())
        };
        let Some(mut printer) = opened else {
            wp_shell::dialog::show_error("There is no printer to print to.");
            self.status = String::from("No printer");
            return self.redrawn();
        };

        let paper = printer.page();
        let (left, top, right, bottom) = paper.unprintable();
        let device = Device::from_dots(paper.dpi_x, left, top, right, bottom);
        let pages = self.layout_for_print(device);

        if !printer.start(&self.document_name()) {
            wp_shell::dialog::show_error("The printer would not accept the document.");
            self.status = String::from("The printer refused the document");
            return self.redrawn();
        }

        let mut renderer = Renderer::new(self.library);
        let sheets = sheets_of(&chosen, settings.per_sheet, settings.copies, settings.collated);
        let mut printed = 0usize;

        for sheet in &sheets {
            let Some(sample) = pages.first() else { break };
            let (width, height) = Renderer::printable_dots(sample, device);
            let width = paper.width.min(width);
            let height = paper.height.min(height);
            let places = printpane::arrangement(settings.per_sheet, width as f32, height as f32);

            let sent = printer.print_page(width, height, |band_top, rows| {
                let mut canvas = Canvas::filled(width, rows, PAPER);
                for (place, number) in places.iter().zip(sheet) {
                    let Some(page) = pages.get(number.saturating_sub(1)) else { continue };
                    let (cell_x, cell_y, cell_width, cell_height) = *place;
                    let inside = (cell_width / page.width).min(cell_height / page.height);
                    let transform = Transform::scale(inside, inside).then(&Transform::translate(
                        cell_x - device.dots(device.unprintable.left),
                        cell_y - device.dots(device.unprintable.top) - band_top as f32,
                    ));
                    renderer.draw_transformed(&mut canvas, page, &transform);
                }
                canvas
            });
            if !sent {
                break;
            }
            printed += 1;
        }

        if printed == sheets.len() {
            printer.finish();
            self.status = match printed {
                1 => "Printed one sheet".to_owned(),
                many => format!("Printed {many} sheets"),
            };
        } else {
            printer.cancel();
            self.status = format!("Printing stopped after {printed} sheets");
        }
        self.close_print()
    }
}

/// The sheets a job comes out as: which pages go on each, in the order they are
/// printed.
///
/// Collated means the whole document, then the whole document again. Uncollated
/// means every copy of the first sheet, then every copy of the second — which
/// is what a printer does when it is left to itself, and what a person wants
/// when they are going to staple the piles rather than the sets.
#[must_use]
pub(super) fn sheets_of(
    pages: &[usize],
    per_sheet: usize,
    copies: u16,
    collated: bool,
) -> Vec<Vec<usize>> {
    let per_sheet = per_sheet.max(1);
    let one_copy: Vec<Vec<usize>> = pages.chunks(per_sheet).map(<[usize]>::to_vec).collect();
    let copies = copies.max(1) as usize;

    if collated {
        (0..copies).flat_map(|_| one_copy.clone()).collect()
    } else {
        one_copy.into_iter().flat_map(|sheet| vec![sheet; copies]).collect()
    }
}

fn outline(canvas: &mut Canvas, x: f32, y: f32, width: f32, height: f32, colour: Color) {
    let (x, y, width, height) = (x as i32, y as i32, width as i32, height as i32);
    canvas.fill_rect(x, y, width, 1, colour);
    canvas.fill_rect(x, y + height - 1, width, 1, colour);
    canvas.fill_rect(x, y, 1, height, colour);
    canvas.fill_rect(x + width - 1, y, 1, height, colour);
}

#[cfg(test)]
mod tests {
    use super::sheets_of;

    #[test]
    fn one_page_to_a_sheet_is_a_sheet_a_page() {
        assert_eq!(sheets_of(&[1, 2, 3], 1, 1, true), vec![vec![1], vec![2], vec![3]]);
    }

    #[test]
    fn two_pages_to_a_sheet_are_paired_in_order() {
        assert_eq!(sheets_of(&[1, 2, 3], 2, 1, true), vec![vec![1, 2], vec![3]]);
    }

    #[test]
    fn collated_copies_come_out_as_whole_documents() {
        assert_eq!(
            sheets_of(&[1, 2], 1, 2, true),
            vec![vec![1], vec![2], vec![1], vec![2]],
            "a collated job is the document, then the document again"
        );
    }

    #[test]
    fn uncollated_copies_come_out_as_piles_of_each_sheet() {
        assert_eq!(
            sheets_of(&[1, 2], 1, 2, false),
            vec![vec![1], vec![1], vec![2], vec![2]],
            "an uncollated job is every copy of a sheet together"
        );
    }

    #[test]
    fn nothing_to_print_is_no_sheets_rather_than_one_empty_one() {
        assert!(sheets_of(&[], 1, 3, true).is_empty());
    }
}

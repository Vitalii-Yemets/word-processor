//! The Print page: what will come out, and what it will come out on.
//!
//! Word gives printing a whole page of its own rather than a dialog over the
//! document: the settings down the left, and the page as it will be printed
//! filling the rest. The arrangement is not decoration. A person printing is
//! asking one question — *what will come out of the printer?* — and a preview
//! beside the settings answers it as each one is changed.
//!
//! What is here is the page itself: the settings, where everything is drawn,
//! and what was pressed. The preview is drawn by the editor, because only the
//! editor can lay a document out; this hands back the rectangle to draw it in.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

use super::icons::{self, Icon};
use super::theme::Theme;

/// How wide the column of settings is.
pub const COLUMN_WIDTH: f32 = 300.0;

/// The height of one setting's box.
const BOX_HEIGHT: f32 = 26.0;

/// The gap under one setting before the next one's label.
const ROW_GAP: f32 = 14.0;

/// Which pages go to the printer.
///
/// Word also prints just what is selected. That is not here yet: it means
/// laying out the selection on its own rather than choosing among the pages
/// already laid out, which is a piece of work of its own — see item A7.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Which {
    #[default]
    All,
    /// The page the caret is on.
    CurrentPage,
    /// The pages typed into the box under it.
    Custom,
}

impl Which {
    pub const ALL: &'static [Self] = &[Self::All, Self::CurrentPage, Self::Custom];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Print All Pages",
            Self::CurrentPage => "Print Current Page",
            Self::Custom => "Custom Print",
        }
    }

    /// What Word writes under the name, to say what it means.
    #[must_use]
    pub fn note(self) -> &'static str {
        match self {
            Self::All => "The whole thing",
            Self::CurrentPage => "Just this page",
            Self::Custom => "Type specific pages",
        }
    }
}

/// Whether the sheets are printed on both sides, and which way they turn.
///
/// Not offered yet: telling a printer to turn the paper over means handing the
/// driver a `DEVMODE` with the duplex field set, and a setting drawn on the
/// page but not carried to the printer would be a lie. See item A6.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sides {
    #[default]
    One,
}

impl Sides {
    pub const ALL: &'static [Self] = &[Self::One];

    #[must_use]
    pub fn label(self) -> &'static str {
        "Print One Sided"
    }

    #[must_use]
    pub fn note(self) -> &'static str {
        "Only print on one side of the sheet"
    }
}

/// How many pages of the document go on one sheet of paper.
pub const PER_SHEET: &[usize] = &[1, 2, 4, 6, 8, 16];

/// The settings of one print job.
///
/// The paper, the orientation and the margins are deliberately not here: in
/// Word they are the document's own page setup, changed from this page as from
/// the Layout tab, and a print-only copy of them would be a second answer to a
/// question that already has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub copies: u16,
    pub which: Which,
    /// The pages as they were typed: `1-3, 8, 12-`.
    pub pages: String,
    pub sides: Sides,
    pub collated: bool,
    pub per_sheet: usize,
    /// Whether tracked changes and comments are printed as well as the text.
    pub markup: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            copies: 1,
            which: Which::All,
            pages: String::new(),
            sides: Sides::One,
            collated: true,
            per_sheet: 1,
            markup: false,
        }
    }
}

impl Settings {
    /// Which pages of the document go to the printer, counted from one.
    ///
    /// An empty answer means nothing would come out, which the page says rather
    /// than printing a blank sheet.
    #[must_use]
    pub fn chosen(&self, total: usize, current: usize) -> Vec<usize> {
        match self.which {
            Which::All => (1..=total).collect(),
            Which::CurrentPage => {
                if current >= 1 && current <= total {
                    vec![current]
                } else {
                    Vec::new()
                }
            }
            Which::Custom => pages_in(&self.pages, total),
        }
    }
}

/// Reads a list of pages the way Word does: `1-3, 8, 12-` — a page, a range, or
/// a range with an open end.
///
/// Anything that is not a number is a separator, which is what makes `1;2` and
/// `1 2` work as well as `1,2`. Pages outside the document are left out rather
/// than refused: asking for pages 1 to 100 of a ten-page document means the ten
/// that are there, which is what a person means by it.
#[must_use]
pub fn pages_in(text: &str, total: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();

    for piece in text.split([',', ';']) {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }

        let (first, last) = match piece.split_once('-') {
            // "12-" is from there to the end, "-4" is from the start to there.
            Some((from, to)) => {
                let from = from.trim().parse::<usize>().unwrap_or(1).max(1);
                let to = to.trim().parse::<usize>().unwrap_or(total).max(1);
                (from.min(to), from.max(to))
            }
            None => match piece.parse::<usize>() {
                Ok(page) if page >= 1 => (page, page),
                _ => continue,
            },
        };

        for page in first..=last.min(total) {
            if page <= total && !out.contains(&page) {
                out.push(page);
            }
        }
    }

    out
}

/// Where each page goes on a sheet holding several of them.
///
/// Word arranges them in reading order, in the grid that fits the paper best:
/// two side by side on a landscape sheet, four in a square, and so on. What
/// comes back is a rectangle per page, in the sheet's own units.
#[must_use]
pub fn arrangement(per_sheet: usize, width: f32, height: f32) -> Vec<(f32, f32, f32, f32)> {
    let (columns, rows) = grid(per_sheet, width, height);
    let cell_width = width / columns as f32;
    let cell_height = height / rows as f32;

    (0..columns * rows)
        .map(|index| {
            let column = index % columns;
            let row = index / columns;
            (column as f32 * cell_width, row as f32 * cell_height, cell_width, cell_height)
        })
        .collect()
}

/// How many across and how many down, for a number of pages on a sheet.
///
/// The grid that wastes least: a sheet twice as tall as it is wide takes two
/// pages one above the other, not side by side.
#[must_use]
fn grid(per_sheet: usize, width: f32, height: f32) -> (usize, usize) {
    match per_sheet.max(1) {
        1 => (1, 1),
        2 => {
            if width >= height {
                (2, 1)
            } else {
                (1, 2)
            }
        }
        4 => (2, 2),
        6 => {
            if width >= height {
                (3, 2)
            } else {
                (2, 3)
            }
        }
        8 => {
            if width >= height {
                (4, 2)
            } else {
                (2, 4)
            }
        }
        16 => (4, 4),
        // Anything else is squared off as best it can be.
        many => {
            let across = (many as f32).sqrt().ceil() as usize;
            (across, many.div_ceil(across))
        }
    }
}

/// What was pressed on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The arrow back to the document.
    Back,
    /// The big Print button.
    Print,
    MoreCopies,
    FewerCopies,
    /// Each of the settings, which drop open a list.
    Printer,
    Which,
    Pages,
    Sides,
    Collation,
    Orientation,
    Paper,
    Margins,
    PerSheet,
    Markup,
    /// The preview's own controls.
    Previous,
    Next,
    ZoomIn,
    ZoomOut,
}

/// What the page needs to know about the document to describe it.
#[derive(Clone, Debug)]
pub struct PaneState {
    pub printer: String,
    pub pages: usize,
    /// Which page the preview is showing, counted from one.
    pub showing: usize,
    pub orientation: String,
    pub paper: String,
    pub margins: String,
    /// Set when the document's margins fall inside the band the printer cannot
    /// reach, which is Word's "One or more margins are set outside the
    /// printable area".
    pub margin_warning: bool,
}

/// Where the preview is to be drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preview {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// The Print page.
#[derive(Clone, Debug, Default)]
pub struct PrintPane {
    pub settings: Settings,
    /// Which page the preview shows, counted from one.
    pub page: usize,
    /// How much of the sheet the preview fills, one being the whole of it.
    pub zoom: f32,
    /// Whether the pages box has the keyboard.
    pub typing_pages: bool,
    placed: Vec<(Hit, f32, f32, f32, f32)>,
    hovered: Option<Hit>,
}

impl PrintPane {
    #[must_use]
    pub fn new() -> Self {
        Self { page: 1, zoom: 1.0, ..Self::default() }
    }

    /// What is under a point, if anything.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        let (x, y) = (x as f32, y as f32);
        self.placed
            .iter()
            .find(|(_, left, top, width, height)| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(hit, ..)| *hit)
    }

    /// Follows the pointer. True when something has to be drawn again.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let over = self.hit(x, y);
        let changed = over != self.hovered;
        self.hovered = over;
        changed
    }

    /// Where one of the settings ended up, so a list can drop from it.
    #[must_use]
    pub fn rect_of(&self, hit: Hit) -> Option<(f32, f32, f32)> {
        self.placed
            .iter()
            .find(|(found, ..)| *found == hit)
            .map(|(_, left, top, width, _)| (*left, *top, *width))
    }

    /// Draws the page, and says where the preview goes.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        state: &PaneState,
        top: f32,
        theme: &Theme,
    ) -> Preview {
        self.placed.clear();
        let width = canvas.width() as f32;
        let height = canvas.height() as f32;
        canvas.fill_rect(0, top as i32, width as i32, (height - top) as i32, theme.ribbon);
        canvas.fill_rect(0, top as i32, COLUMN_WIDTH as i32, (height - top) as i32, theme.pane);

        let mut y = top + 18.0;
        self.draw_back(canvas, engine, renderer, 16.0, &mut y, theme);
        self.draw_print_button(canvas, engine, renderer, &mut y, theme);
        self.draw_copies(canvas, engine, renderer, &mut y, theme);

        self.heading(canvas, engine, renderer, "Printer", &mut y, theme);
        self.setting(canvas, engine, renderer, Hit::Printer, &state.printer, "", &mut y, theme);

        self.heading(canvas, engine, renderer, "Settings", &mut y, theme);
        let which = self.settings.which;
        self.setting(
            canvas,
            engine,
            renderer,
            Hit::Which,
            which.label(),
            which.note(),
            &mut y,
            theme,
        );
        self.draw_pages_box(canvas, engine, renderer, &mut y, theme);

        let sides = self.settings.sides;
        self.setting(
            canvas,
            engine,
            renderer,
            Hit::Sides,
            sides.label(),
            sides.note(),
            &mut y,
            theme,
        );

        let collation = if self.settings.collated { "Collated" } else { "Uncollated" };
        let example = if self.settings.collated { "1,2,3   1,2,3" } else { "1,1   2,2   3,3" };
        self.setting(canvas, engine, renderer, Hit::Collation, collation, example, &mut y, theme);

        self.setting(
            canvas,
            engine,
            renderer,
            Hit::Orientation,
            &state.orientation,
            "",
            &mut y,
            theme,
        );
        self.setting(canvas, engine, renderer, Hit::Paper, &state.paper, "", &mut y, theme);
        self.setting(canvas, engine, renderer, Hit::Margins, &state.margins, "", &mut y, theme);

        let per_sheet = match self.settings.per_sheet {
            1 => "1 Page Per Sheet".to_owned(),
            many => format!("{many} Pages Per Sheet"),
        };
        self.setting(canvas, engine, renderer, Hit::PerSheet, &per_sheet, "", &mut y, theme);

        let markup = if self.settings.markup { "Print Markup" } else { "Print the Document" };
        let note = if self.settings.markup {
            "Tracked changes and comments as well"
        } else {
            "The text as it would be accepted"
        };
        self.setting(canvas, engine, renderer, Hit::Markup, markup, note, &mut y, theme);

        if state.margin_warning {
            self.warn(canvas, engine, renderer, &mut y, theme);
        }

        self.draw_preview_bar(canvas, engine, renderer, state, height, theme);

        // What is left over is the preview, with room round it so the sheet
        // does not touch the edges of the window.
        Preview {
            left: COLUMN_WIDTH + 24.0,
            top: top + 24.0,
            width: (width - COLUMN_WIDTH - 48.0).max(1.0),
            height: (height - top - 80.0).max(1.0),
        }
    }

    fn draw_back(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        y: &mut f32,
        theme: &Theme,
    ) {
        let hovered = self.hovered == Some(Hit::Back);
        if hovered {
            canvas.fill_rect(left as i32 - 4, *y as i32 - 4, 28, 28, theme.hover);
        }
        icons::draw_sized(canvas, Icon::Previous, left, *y, 20.0, theme.text);
        self.placed.push((Hit::Back, left - 4.0, *y - 4.0, 28.0, 28.0));

        let line = engine.simple_line("Print", left + 34.0, *y + 16.0, 14.0, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        *y += 44.0;
    }

    fn draw_print_button(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        y: &mut f32,
        theme: &Theme,
    ) {
        let (left, width, height) = (16.0f32, 120.0f32, 40.0f32);
        let colour = if self.hovered == Some(Hit::Print) { theme.emphasis } else { theme.accent };
        canvas.fill_rect(left as i32, *y as i32, width as i32, height as i32, colour);
        icons::draw_sized(canvas, Icon::Print, left + 10.0, *y + 10.0, 20.0, theme.on_accent());
        let line = engine.simple_line("Print", left + 40.0, *y + 25.0, 11.0, theme.on_accent());
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        self.placed.push((Hit::Print, left, *y, width, height));
        *y += height + 14.0;
    }

    fn draw_copies(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        y: &mut f32,
        theme: &Theme,
    ) {
        let line = engine.simple_line("Copies", 16.0, *y + 17.0, 9.0, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        let (left, width) = (100.0f32, 70.0f32);
        canvas.fill_rect(left as i32, *y as i32, width as i32, BOX_HEIGHT as i32, theme.field);
        outline(canvas, left, *y, width, BOX_HEIGHT, theme.field_edge);
        let count = self.settings.copies.to_string();
        let line = engine.simple_line(&count, left + 8.0, *y + 17.0, 9.0, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        // The two little arrows, one above the other, as a spinner is drawn
        // everywhere.
        let arrows = left + width - 18.0;
        spinner(canvas, engine, renderer, arrows, *y, true, theme.text);
        spinner(canvas, engine, renderer, arrows, *y + 13.0, false, theme.text);
        self.placed.push((Hit::MoreCopies, arrows, *y, 18.0, 13.0));
        self.placed.push((Hit::FewerCopies, arrows, *y + 13.0, 18.0, 13.0));

        *y += BOX_HEIGHT + 20.0;
    }

    fn heading(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        text: &str,
        y: &mut f32,
        theme: &Theme,
    ) {
        let line = engine.simple_line(text, 16.0, *y + 10.0, 9.0, theme.accent);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        canvas.fill_rect(16, (*y + 16.0) as i32, (COLUMN_WIDTH - 32.0) as i32, 1, theme.pane_edge);
        *y += 24.0;
    }

    /// One setting: the choice in a box, with what it means under it.
    #[allow(clippy::too_many_arguments)]
    fn setting(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        hit: Hit,
        label: &str,
        note: &str,
        y: &mut f32,
        theme: &Theme,
    ) {
        let (left, width) = (16.0f32, COLUMN_WIDTH - 32.0);
        let background = if self.hovered == Some(hit) { theme.hover } else { theme.field };
        canvas.fill_rect(left as i32, *y as i32, width as i32, BOX_HEIGHT as i32, background);
        outline(canvas, left, *y, width, BOX_HEIGHT, theme.field_edge);

        let line = engine.simple_line(label, left + 8.0, *y + 17.0, 9.0, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        chevron(canvas, left + width - 18.0, *y + BOX_HEIGHT / 2.0, theme.text);
        self.placed.push((hit, left, *y, width, BOX_HEIGHT));
        *y += BOX_HEIGHT;

        if !note.is_empty() {
            let line = engine.simple_line(note, left + 8.0, *y + 12.0, 8.0, theme.dim_text);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
            *y += 16.0;
        }
        *y += ROW_GAP;
    }

    /// The box the pages are typed into, which Word keeps under the first
    /// setting whether or not it is in use.
    fn draw_pages_box(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        y: &mut f32,
        theme: &Theme,
    ) {
        let line = engine.simple_line("Pages:", 16.0, *y + 17.0, 9.0, theme.dim_text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        let (left, width) = (66.0f32, COLUMN_WIDTH - 82.0);
        canvas.fill_rect(left as i32, *y as i32, width as i32, BOX_HEIGHT as i32, theme.field);
        let edge = if self.typing_pages { theme.accent } else { theme.field_edge };
        outline(canvas, left, *y, width, BOX_HEIGHT, edge);

        let typed =
            engine.simple_line(&self.settings.pages, left + 8.0, *y + 17.0, 9.0, theme.text);
        let measured = typed.width - (left + 8.0);
        renderer.draw_onto(canvas, &typed, 0.0, 0.0);
        if self.typing_pages {
            canvas.fill_rect((left + 8.0 + measured) as i32, (*y + 6.0) as i32, 1, 14, theme.text);
        }

        self.placed.push((Hit::Pages, left, *y, width, BOX_HEIGHT));
        *y += BOX_HEIGHT + ROW_GAP;
    }

    /// Word's warning that the margins fall where the printer cannot reach.
    fn warn(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        y: &mut f32,
        theme: &Theme,
    ) {
        let text = "One or more margins are outside the printable area.";
        let line = engine.simple_line(text, 16.0, *y + 12.0, 8.0, theme.danger);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        *y += 20.0;
    }

    /// The strip under the preview: which page is showing, and the zoom.
    fn draw_preview_bar(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        state: &PaneState,
        height: f32,
        theme: &Theme,
    ) {
        let y = height - 40.0;
        let left = COLUMN_WIDTH + 24.0;

        icons::draw_sized(canvas, Icon::Previous, left, y, 16.0, theme.text);
        self.placed.push((Hit::Previous, left - 4.0, y - 4.0, 24.0, 24.0));

        let counted = format!("{} of {}", state.showing, state.pages.max(1));
        let line = engine.simple_line(&counted, left + 28.0, y + 12.0, 9.0, theme.text);
        let measured = line.width - (left + 28.0);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        let next = left + 36.0 + measured;
        icons::draw_sized(canvas, Icon::Next, next, y, 16.0, theme.text);
        self.placed.push((Hit::Next, next - 4.0, y - 4.0, 24.0, 24.0));

        // The zoom sits at the right-hand end, where the status bar keeps it,
        // and is drawn the same way: a minus and a plus, which need no font.
        let right = canvas.width() as f32 - 40.0;
        let ink = theme.text;
        canvas.fill_rect((right - 30.0) as i32, (y + 7.0) as i32, 9, 2, ink);
        self.placed.push((Hit::ZoomOut, right - 34.0, y - 4.0, 24.0, 24.0));
        canvas.fill_rect(right as i32, (y + 7.0) as i32, 9, 2, ink);
        canvas.fill_rect((right + 3.5) as i32, (y + 3.5) as i32, 2, 9, ink);
        self.placed.push((Hit::ZoomIn, right - 4.0, y - 4.0, 24.0, 24.0));
        self.placed.push((Hit::ZoomIn, right - 4.0, y - 4.0, 24.0, 24.0));
    }
}

/// The small triangle that says a list drops from here.
fn chevron(canvas: &mut Canvas, x: f32, centre_y: f32, colour: Color) {
    for step in 0..4 {
        canvas.fill_rect(
            (x + step as f32) as i32,
            (centre_y - 2.0 + step as f32) as i32,
            (7 - step * 2).max(1),
            1,
            colour,
        );
    }
}

/// One half of a spinner: the little arrow that puts the number up or down.
fn spinner(
    canvas: &mut Canvas,
    engine: &mut LayoutEngine<'_>,
    renderer: &mut Renderer<'_>,
    x: f32,
    y: f32,
    up: bool,
    colour: Color,
) {
    let _ = (engine, renderer);
    for step in 0..4 {
        let row = if up { 3 - step } else { step };
        canvas.fill_rect(
            (x + step as f32) as i32,
            (y + 4.0 + row as f32) as i32,
            (7 - step * 2).max(1),
            1,
            colour,
        );
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
    use super::{arrangement, pages_in, Settings, Which};

    #[test]
    fn a_page_on_its_own_is_that_page() {
        assert_eq!(pages_in("3", 10), vec![3]);
    }

    #[test]
    fn a_range_is_every_page_in_it() {
        assert_eq!(pages_in("2-5", 10), vec![2, 3, 4, 5]);
    }

    #[test]
    fn a_list_is_read_in_the_order_it_was_typed() {
        assert_eq!(pages_in("5, 1, 3", 10), vec![5, 1, 3]);
    }

    #[test]
    fn a_range_with_an_open_end_runs_to_the_end_of_the_document() {
        assert_eq!(pages_in("8-", 10), vec![8, 9, 10]);
        assert_eq!(pages_in("-3", 10), vec![1, 2, 3]);
    }

    #[test]
    fn pages_that_are_not_there_are_left_out_rather_than_refused() {
        // Asking for 1 to 100 of a ten-page document means the ten there are.
        assert_eq!(pages_in("1-100", 10).len(), 10);
        assert!(pages_in("40", 10).is_empty());
    }

    #[test]
    fn a_page_asked_for_twice_is_printed_once() {
        assert_eq!(pages_in("2, 2-3, 3", 10), vec![2, 3]);
    }

    #[test]
    fn nonsense_is_ignored_rather_than_printing_the_wrong_thing() {
        assert!(pages_in("", 10).is_empty());
        assert!(pages_in("x", 10).is_empty());
        assert!(pages_in("0", 10).is_empty());
    }

    #[test]
    fn the_settings_say_which_pages_go_to_the_printer() {
        let all = Settings::default();
        assert_eq!(all.chosen(3, 2), vec![1, 2, 3]);

        let here = Settings { which: Which::CurrentPage, ..Settings::default() };
        assert_eq!(here.chosen(3, 2), vec![2]);

        let some =
            Settings { which: Which::Custom, pages: "1,3".to_owned(), ..Settings::default() };
        assert_eq!(some.chosen(3, 1), vec![1, 3]);
    }

    #[test]
    fn one_page_to_a_sheet_fills_the_sheet() {
        let places = arrangement(1, 100.0, 200.0);
        assert_eq!(places, vec![(0.0, 0.0, 100.0, 200.0)]);
    }

    #[test]
    fn two_pages_to_a_sheet_are_laid_the_way_the_sheet_is_longest() {
        // A tall sheet takes them one above the other.
        let tall = arrangement(2, 100.0, 200.0);
        assert_eq!(tall.len(), 2);
        assert_eq!(tall[0], (0.0, 0.0, 100.0, 100.0));
        assert_eq!(tall[1], (0.0, 100.0, 100.0, 100.0));

        // A wide one takes them side by side.
        let wide = arrangement(2, 200.0, 100.0);
        assert_eq!(wide[1], (100.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn four_pages_to_a_sheet_are_a_square_in_reading_order() {
        let places = arrangement(4, 200.0, 200.0);
        assert_eq!(places.len(), 4);
        assert_eq!(places[0].0, 0.0);
        assert_eq!(places[1].0, 100.0, "the second is to the right of the first");
        assert_eq!(places[2].1, 100.0, "the third is under the first");
    }
}

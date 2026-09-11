//! The editor: what the window shows, and what every command does to it.

mod appearance;
mod arrange;
mod autocorrectdialog;
mod autoscroll;
mod backstage;
mod boxes;
mod chart;
mod citations;
mod commands;
mod context;
mod correcting;
mod diagram;
mod dialogs;
mod dispatch;
mod dragtext;
mod draw;
mod effects;
mod equation;
mod events;
pub(crate) mod files;
mod fontdialog;
mod furnitureedit;
mod groups;
mod handles;
mod help;
mod insert;
mod keytips;
mod links;
mod macros;
mod mailings;
mod matching;
mod menus;
mod minibar;
mod notes;
mod numbering;
mod optionsdialog;
mod outline;
mod pagebordersdialog;
mod pagesetup;
mod paragraphdialog;
mod parts;
mod paste;
mod preferences;
mod printpane;
mod proofing;
mod properties;
mod references;
mod review;
mod ribbondialog;
mod ruler;
mod rules;
mod screenshot;
mod scrolling;
mod search;
mod selecting;
mod shapes;
mod signature;
mod split;
mod stationery;
mod statusmenu;
mod styledialog;
mod styles;
mod symboldialog;
mod tabledialog;
mod tablelayout;
mod tablestyle;
mod tabsdialog;
mod theme_effects;
mod themes;
mod translate;
mod video;
mod views;
mod watermark;
mod windows;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use wp_docx::model::{Alignment, NumberingReference};
use wp_docx::page::CaseChange;
use wp_docx::{CharacterFormat, Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, Page, Renderer};
use wp_raster::{Canvas, Color};
use wp_shell::Response;

use crate::chrome::navigation::{Contents, Found, Heading};
use crate::chrome::palette::Palette;
use crate::chrome::status::{MAX_ZOOM, MIN_ZOOM};
use crate::chrome::{
    self, status, Command, MiniBar, Navigation, Popup, Ribbon, TableGrid, Theme, Tip,
    HORIZONTAL_HEIGHT, RIBBON_TOTAL, STATUS_HEIGHT, VERTICAL_WIDTH,
};

/// Space between one page and the next, in pixels.
const PAGE_GAP: f32 = 24.0;
/// The same once the white space between pages is hidden: a line, not a gap.
const JOINED_PAGE_GAP: f32 = 6.0;
/// How far the wheel moves the view per notch.
const SCROLL_PER_NOTCH: f32 = 90.0;
/// How much of the window to keep clear around the caret when scrolling to it.
const CARET_MARGIN: f32 = 40.0;
/// Dots per inch a document is laid out at before zoom. 96 is what Windows
/// calls 100%.
pub const DPI: f32 = 96.0;
/// Points per inch, for turning page metrics into pixels.
/// How wide the caret is drawn, in pixels.
const CARET_WIDTH: i32 = 2;

const POINTS_PER_INCH: f32 = 72.0;
/// Twentieths of a point, the unit the format measures indents in.
const TWIPS_PER_POINT: f32 = 20.0;
/// How far one press of the indent button moves a paragraph: half an inch.
const INDENT_STEP: i32 = 720;

/// Shows a document, and edits it.
pub struct Editor {
    pub document: Document,
    engine: LayoutEngine<'static>,
    /// A second engine for everything that is not the document: the ribbon, the
    /// bars, the menus.
    ///
    /// Kept at the screen's own resolution while the other one follows the
    /// zoom, because the zoom belongs to the document and not to the program —
    /// Word at two hundred per cent has a bigger page and a ribbon exactly the
    /// size it was.
    chrome_engine: LayoutEngine<'static>,
    renderer: Renderer<'static>,
    pages: Vec<Page>,
    /// The window-sized image, kept so a repaint does not allocate one.
    canvas: Canvas,
    scroll: f32,
    /// How far the view has been moved sideways.
    ///
    /// Only ever anything when the page is wider than the window — at a big
    /// zoom, or on wide paper — and then it is the only way to reach the right
    /// edge of the page at all.
    scroll_across: f32,
    view_width: usize,
    view_height: usize,
    /// Whether the left button is down and dragging out a selection.
    dragging: bool,
    /// Where the bar between two views of the document sits, as a share of the
    /// window, and how the second view is scrolled. See [`split`].
    split: Option<f32>,
    other_scroll: f32,
    active_pane: usize,
    split_dragging: bool,
    /// What has been done since recording started, if a macro is being
    /// recorded. See [`macros`].
    recording: Option<Vec<macros::Step>>,
    /// The macros the list last offered.
    macro_names: Vec<String>,
    /// Which piece of furniture is being edited, and the document as it was
    /// last laid out, to draw behind it. See [`furnitureedit`].
    editing_furniture: Option<wp_docx::furniture::Furniture>,
    /// Which tab the ribbon was showing before a header was opened, so that
    /// coming back out puts it back.
    tab_before_furniture: Option<crate::chrome::ribbon::Tab>,
    dimmed: Vec<Page>,
    /// A press inside the selection that has not yet moved, and the text it
    /// turned into a drag. See [`dragtext`].
    pending_text_drag: Option<(i32, i32)>,
    text_drag: Option<dragtext::TextDrag>,
    /// When and where the last double click was, so a third click can be told
    /// from a first. See [`selecting`].
    last_double_click: Option<(std::time::Instant, i32, i32)>,
    /// How much of the text the drag now running takes at a time.
    drag_by: selecting::Granularity,
    /// What was last copied: the words that went to the system clipboard, and
    /// the same content with its formatting. See [`wp_docx::clipboard`].
    clipboard: Option<(String, Vec<wp_docx::model::Block>)>,
    /// The command behind each row of whichever menu is open — the list a
    /// collapsed group of the ribbon becomes, or the menu the right button
    /// opens. Nothing where the row is a line between two groups.
    group_commands: Vec<Option<crate::chrome::Command>>,
    /// Where the scroll bar was last drawn, and how far down the thumb it is
    /// being held. See [`scrolling`].
    scrollbar: Option<crate::chrome::ScrollBar>,
    scroll_grab: Option<f32>,
    /// The same for the bar across the bottom, when the page is too wide to
    /// fit and there is one.
    across_bar: Option<crate::chrome::ScrollBar>,
    across_grab: Option<f32>,
    /// The terms a translation replaces, read from a file the person chose.
    glossary: wp_docx::translate::Glossary,
    /// Where each of the other windows is looking. See [`windows`].
    window_states: Vec<Option<windows::WindowState>>,
    active_window: usize,
    /// Which kind of chart is being drawn, while its numbers are typed.
    chart_kind: wp_docx::chart::Kind,
    /// Which merge rule is being put in, while its condition is typed.
    merge_rule: wp_docx::rules::Rule,
    /// How a diagram is arranged, while its boxes are being typed.
    diagram_arrangement: wp_docx::diagram::Arrangement,
    /// The windows a screenshot could be taken of, while the list is open.
    screen_windows: Vec<wp_shell::screen::Window>,
    /// How deep the outline goes when the document is shown as one.
    outline_depth: u8,
    /// Which way the pages run, and therefore which way the view scrolls.
    movement: views::Movement,
    /// Whether the zoom slider is being dragged.
    sliding: bool,
    /// Where the pointer last was, so the wheel and the cursor know what it is
    /// over.
    pointer_x: f32,
    pointer_y: f32,
    /// Whether the navigation pane's edge is being dragged.
    resizing_pane: bool,
    /// Which marker or margin on the rulers is being dragged, if any.
    ruler_drag: Option<ruler::Grab>,
    /// Where the stop being dragged along the ruler is now, in twips from the
    /// left margin, so the next move knows which one to take hold of.
    ruler_stop_at: Option<i32>,
    /// What kind of tab stop a click on the ruler puts down. See [`ruler`].
    tab_kind: wp_docx::model::TabAlignment,
    /// What would stop somebody reading the document, when it was last asked.
    accessibility: Vec<wp_docx::accessibility::Finding>,
    /// Whether merge fields are shaded so they can be told from ordinary text.
    highlight_fields: bool,
    /// Whether the writing is being checked, and what was found.
    show_proofing: bool,
    issues: Vec<wp_docx::proofing::Issue>,
    dictionary: wp_docx::proofing::Dictionary,
    /// The mistake a correction is being chosen for.
    pending_issue: Option<wp_docx::proofing::Issue>,
    /// The people a mail merge is for, read from the file beside the letter.
    recipients: wp_docx::merge::Recipients,
    recipient_file: Option<PathBuf>,
    /// Which of them the letter is being shown for, if it is being previewed.
    preview_record: Option<usize>,
    /// Which WordArt look was chosen, while its words are being typed.
    word_art_style: usize,
    /// Which part of an address is matched to which column of the list, where
    /// the guess has been corrected by hand.
    field_matches: Vec<(String, String)>,
    /// The part a column is being chosen for, while the list is open.
    matching_part: usize,
    /// Which envelope or sheet of labels was chosen, while what goes on it is
    /// being typed.
    stationery_choice: usize,
    /// The drawing being dragged, and what it was when the drag began.
    shape_drag: Option<handles::ShapeDrag>,
    /// Which document property the strip is taking a new value for.
    editing_property: Option<properties::Field>,
    /// Which way round the desktop was last told the window's colours go.
    frame_told: Option<crate::chrome::theme::Mode>,
    titlebar: crate::chrome::TitleBar,
    ribbon: Ribbon,
    navigation: Navigation,
    /// The button the pointer is over, so it can be shown as reachable.
    hovered: Option<Command>,
    /// The list of fonts, sizes, styles or zooms, while one is dropped open.
    popup: Option<Popup>,
    /// Where the next list should hang, when it is not a ribbon button that is
    /// dropping it open.
    ///
    /// The mini toolbar has boxes of its own, and a list dropped from one of
    /// them has to hang under that box rather than under the ribbon button
    /// with the same name.
    popup_anchor: Option<(f32, f32, f32)>,
    /// What is corrected as it is typed. See [`crate::autocorrect`].
    pub(super) autocorrect: crate::autocorrect::AutoCorrect,
    /// What is being changed about the ribbon and the toolbar while the
    /// Options dialog is up. See [`ribbondialog`].
    pub(super) editing_chrome: crate::chrome::Customisation,
    /// The corrections being changed while the AutoCorrect dialog is up.
    ///
    /// A working copy, because that dialog is built again every time one of its
    /// buttons adds to a list, and because the Exceptions dialog takes it away
    /// and puts it back. Nothing reaches `autocorrect` until OK is pressed.
    pub(super) editing_rules: crate::autocorrect::AutoCorrect,
    /// What the two exception lists held when the Exceptions dialog opened, so
    /// that its Cancel can put them back.
    pub(super) exceptions_stash:
        Option<(std::collections::BTreeSet<String>, std::collections::BTreeSet<String>)>,
    /// The measurement box on the ribbon that has the keyboard, and whether
    /// what is in it is still the value it opened with. See [`boxes`].
    ribbon_box: Option<(crate::chrome::Command, bool)>,
    /// What has been typed into it.
    box_text: String,
    /// What the last paste put down, while the little button that offers the
    /// other ways of pasting it is still showing. See [`paste`].
    pasted: Option<paste::Pasted>,
    /// The File tab, while it is what the window is showing.
    ///
    /// Word's File tab is not a ribbon page: it is a window of its own about
    /// the document rather than about the text in it.
    backstage: Option<crate::chrome::backstage::Backstage>,
    /// The Print page, while it is what the window is showing.
    ///
    /// Word gives printing a page rather than a dialog: the settings down one
    /// side and the document as it will come out beside them.
    print_pane: Option<crate::chrome::printpane::PrintPane>,
    /// The document laid out for the printer, which is what the preview shows.
    print_preview: Vec<wp_layout::Page>,
    /// Which printer the job would go to, and what it says about itself.
    printer_name: String,
    print_device: wp_layout::Device,
    /// The pixels the caret was drawn over, so a blink can put them back
    /// rather than drawing the whole window again.
    under_caret: Option<(i32, i32, Vec<u8>)>,
    /// The dialog that is up, if any. While one is, it has the window.
    dialog: Option<crate::chrome::dialog::Dialog>,
    /// What that dialog is asking, so its answer can be acted on.
    asking: Option<dialogs::Asking>,
    /// Whether Word Count counts what is written round the edges of the body:
    /// notes and text boxes. Word remembers the tick between openings, so this
    /// lives here rather than in the dialog.
    count_the_edges: bool,
    /// Whether the only thing that changed is the caret's half of a blink.
    caret_only: bool,
    /// The little bar of formatting buttons floating over a selection.
    mini_bar: Option<MiniBar>,
    /// How long the caret rests on each side of a blink.
    ///
    /// Read from the system once, because it is a setting the person owns.
    /// Nothing means they have asked for a caret that does not blink at all,
    /// and then it never does.
    caret_blink: Option<Duration>,
    /// Whether the caret is showing this instant, and when that last changed.
    ///
    /// Anything that moves the caret puts it back on and starts the count
    /// again, so it is never invisible at the moment somebody looks for it —
    /// which is what every editor does and what makes typing feel steady.
    caret_on: bool,
    caret_flipped: Instant,
    /// Where the mark sits while the middle button is scrolling the document.
    /// See [`autoscroll`].
    autoscroll: Option<(i32, i32)>,
    /// Where the strip's own menu was opened, so it can be opened again in
    /// the same place when something on it is ticked.
    status_menu_at: Option<(i32, i32)>,
    /// Which parts of the strip along the bottom are showing. See
    /// [`crate::chrome::status::Shows`].
    status_shows: crate::chrome::status::Shows,
    /// How far the letters over the ribbon have been followed, while Alt has
    /// put them there. See [`keytips`].
    key_tips: Option<keytips::Level>,
    /// The tip showing under a ribbon button, once the pointer has rested on it
    /// long enough, and when the pointer arrived.
    tip: Option<Tip>,
    hovered_since: Instant,
    /// The grid of squares that asks how big a table should be.
    table_grid: Option<TableGrid>,
    /// The colours dropped open by one of the three coloured buttons.
    palette: Option<Palette>,
    /// The find-and-replace strip, while it is open.
    find_bar: Option<crate::chrome::findbar::FindBar>,
    /// Which of the two the open furniture menu is about.
    choosing_furniture: wp_docx::furniture::Furniture,
    /// Whether tracked changes are drawn as changes.
    show_markup: bool,
    /// Whether the boundaries of a table with no borders are drawn.
    ///
    /// Word's View Gridlines: on the screen only, never printed.
    show_table_gridlines: bool,
    /// Which kind of note the open strip is about.
    note_kind: wp_docx::notes::Kind,
    /// Which way the open list of references would point.
    reference_kind: wp_docx::captions::Reference,
    /// The formatting the format painter is carrying, if it is armed.
    painter: Option<wp_docx::model::RunProperties>,
    /// Which case the change-case button will apply next.
    case_change: CaseChange,
    /// The colours those two buttons would apply without opening anything.
    chosen_text_color: Color,
    chosen_highlight_color: Color,
    show_gridlines: bool,
    /// Whether the space between pages is collapsed.
    joined_pages: bool,
    /// Whether the document is shown as one endless page rather than as paper.
    /// How the document is being looked at.
    view: views::View,
    /// What the rulers and the pane were doing before reading mode hid them.
    remembered_rulers: bool,
    remembered_navigation: bool,
    /// Which way round the colours go, and every colour that follows from it.
    theme: Theme,
    /// The fonts on this machine, gathered once.
    families: Vec<String>,
    /// Kept for laying the document out again at a printer resolution.
    library: &'static FontLibrary,
    /// How far the document is magnified, as a percentage.
    zoom: f32,
    show_rulers: bool,
    show_navigation: bool,
    /// Whether the styles pane is showing, down the right-hand side.
    show_styles: bool,
    styles_pane: crate::chrome::stylespane::StylesPane,
    /// The style being made or changed, while its dialog is up.
    editing_style: Option<wp_docx::StyleDefinition>,
    /// Whether the Font or Paragraph dialog was opened from inside the style
    /// dialog, so that answering it comes back there rather than to the page.
    formatting_a_style: bool,
    /// Which block of characters the Symbol dialog is showing.
    symbol_subset: usize,
    /// The characters most recently put in, newest first. Word remembers these
    /// between openings and so does this.
    recent_symbols: Vec<char>,
    /// What unit measurements are shown in. Word's Options sets it, and every
    /// box in the program is in it. See [`crate::measure`].
    unit: crate::measure::Unit,
    show_marks: bool,
    /// Where the zoom slider was last drawn.
    slider: Option<status::SliderRect>,
    status_buttons: Vec<(Command, f32, f32)>,
    pub file: Option<PathBuf>,
    /// What the strip along the bottom says about the last command.
    status: String,
    /// What the program remembers between one run and the next.
    settings: crate::settings::Settings,
    /// What the caption bar currently says, so it is only set when it changes.
    title: String,
    needs_redraw: bool,
}

impl Editor {
    pub fn new(library: &'static FontLibrary, document: Document, file: Option<PathBuf>) -> Self {
        let theme = Theme::default();
        let mut engine = LayoutEngine::new(library)
            .with_dpi(DPI)
            .with_automatic_colors(theme.page_text, theme.table_line);
        let pages = engine.layout_document(&document);

        Self {
            document,
            engine,
            chrome_engine: LayoutEngine::new(library).with_dpi(DPI),
            renderer: Renderer::new(library),
            pages,
            canvas: Canvas::new(1, 1),
            scroll: 0.0,
            scroll_across: 0.0,
            view_width: 0,
            view_height: 0,
            dragging: false,
            editing_furniture: None,
            tab_before_furniture: None,
            dimmed: Vec::new(),
            pending_text_drag: None,
            text_drag: None,
            last_double_click: None,
            drag_by: selecting::Granularity::default(),
            clipboard: None,
            group_commands: Vec::new(),
            scrollbar: None,
            scroll_grab: None,
            across_bar: None,
            across_grab: None,
            glossary: wp_docx::translate::Glossary::default(),
            window_states: Vec::new(),
            active_window: 0,
            chart_kind: wp_docx::chart::Kind::default(),
            merge_rule: wp_docx::rules::Rule::If,
            recording: None,
            macro_names: Vec::new(),
            diagram_arrangement: wp_docx::diagram::Arrangement::default(),
            screen_windows: Vec::new(),
            outline_depth: outline::ALL_LEVELS,
            movement: views::Movement::default(),
            split: None,
            other_scroll: 0.0,
            active_pane: 0,
            split_dragging: false,
            sliding: false,
            pointer_x: 0.0,
            pointer_y: 0.0,
            resizing_pane: false,
            ruler_drag: None,
            ruler_stop_at: None,
            tab_kind: wp_docx::model::TabAlignment::Start,
            accessibility: Vec::new(),
            highlight_fields: false,
            show_proofing: true,
            issues: Vec::new(),
            dictionary: wp_docx::proofing::Dictionary::default(),
            pending_issue: None,
            recipients: wp_docx::merge::Recipients::default(),
            recipient_file: None,
            preview_record: None,
            word_art_style: 0,
            field_matches: Vec::new(),
            matching_part: 0,
            stationery_choice: 0,
            settings: crate::settings::Settings::default(),
            shape_drag: None,
            editing_property: None,
            frame_told: None,
            titlebar: crate::chrome::TitleBar::new(),
            ribbon: Ribbon::new(),
            navigation: Navigation::new(),
            hovered: None,
            popup: None,
            popup_anchor: None,
            autocorrect: crate::autocorrect::AutoCorrect::default(),
            editing_chrome: crate::chrome::Customisation::default(),
            editing_rules: crate::autocorrect::AutoCorrect::default(),
            exceptions_stash: None,
            ribbon_box: None,
            box_text: String::new(),
            pasted: None,
            backstage: None,
            print_pane: None,
            print_preview: Vec::new(),
            printer_name: String::new(),
            print_device: wp_layout::Device::screen(),
            dialog: None,
            asking: None,
            count_the_edges: false,
            under_caret: None,
            caret_only: false,
            mini_bar: None,
            caret_blink: wp_shell::caret_blink_millis()
                .map(|millis| Duration::from_millis(u64::from(millis))),
            caret_on: true,
            caret_flipped: Instant::now(),
            autoscroll: None,
            status_menu_at: None,
            status_shows: crate::chrome::status::Shows::default(),
            key_tips: None,
            tip: None,
            hovered_since: Instant::now(),
            table_grid: None,
            palette: None,
            find_bar: None,
            choosing_furniture: wp_docx::furniture::Furniture::Header,
            show_markup: true,
            show_table_gridlines: true,
            note_kind: wp_docx::notes::Kind::Footnote,
            reference_kind: wp_docx::captions::Reference::Text,
            painter: None,
            case_change: CaseChange::default(),
            chosen_text_color: Color::rgb(0xC0, 0x00, 0x00),
            chosen_highlight_color: Color::rgb(0xFF, 0xFF, 0x00),
            show_gridlines: false,
            joined_pages: false,
            view: views::View::default(),
            remembered_rulers: true,
            remembered_navigation: false,
            theme,
            families: library.families().into_iter().map(str::to_owned).collect(),
            library,
            zoom: 100.0,
            show_rulers: true,
            show_navigation: true,
            show_styles: false,
            styles_pane: crate::chrome::stylespane::StylesPane::new(),
            editing_style: None,
            formatting_a_style: false,
            symbol_subset: 0,
            recent_symbols: Vec::new(),
            unit: crate::measure::Unit::default(),
            show_marks: false,
            slider: None,
            status_buttons: Vec::new(),
            file,
            status: String::new(),
            title: String::new(),
            needs_redraw: true,
        }
    }

    /// Where the caret is. The document owns it, so undo can put it back.
    fn caret(&self) -> TextPosition {
        self.document.caret()
    }

    // --- Geometry ------------------------------------------------------------

    /// Pixels to the inch at the current magnification.
    fn pixels_per_inch(&self) -> f32 {
        DPI * self.zoom / 100.0
    }

    /// The left edge of the page area, past the navigation pane and the ruler.
    fn content_left(&self) -> f32 {
        self.pane_width() + if self.show_rulers { VERTICAL_WIDTH } else { 0.0 }
    }

    /// Where the ribbon begins, under the title bar.
    fn ribbon_bottom(&self) -> f32 {
        // Reading mode has no ribbon, so the page starts straight under the
        // title bar — which is the whole point of it.
        if !self.view.shows_furniture() {
            return crate::chrome::TITLE_HEIGHT;
        }
        crate::chrome::TITLE_HEIGHT + RIBBON_TOTAL
    }

    /// The whole band between the ribbon and the status strip.
    ///
    /// The window's band, not a pane's: a split divides this, and the bar
    /// between the panes is placed inside it.
    fn whole_content_band(&self) -> (f32, f32) {
        let top = self.ribbon_bottom()
            + self.find_bar_height()
            + if self.show_rulers { HORIZONTAL_HEIGHT } else { 0.0 };
        let bottom = (self.view_height as f32 - STATUS_HEIGHT).max(top + 1.0);
        (top, bottom)
    }

    /// The bottom of the whole content band, above the status strip.
    ///
    /// The furniture — the status strip, the navigation pane, the rulers —
    /// belongs to the window rather than to a pane, so it measures from here.
    pub(super) fn whole_content_top(&self) -> f32 {
        self.whole_content_band().0
    }

    pub(super) fn window_bottom(&self) -> f32 {
        self.whole_content_band().1
    }

    /// Where the pages begin: the top of the pane being edited.
    fn content_top(&self) -> f32 {
        self.pane_band(self.active_pane).0
    }

    /// How much room the find strip takes, which is none when it is shut.
    pub(super) fn find_bar_height(&self) -> f32 {
        if self.find_bar.is_some() {
            crate::chrome::findbar::HEIGHT
        } else {
            0.0
        }
    }

    /// Where they end, above the status strip.
    fn content_bottom(&self) -> f32 {
        self.pane_band(self.active_pane).1
    }

    fn viewport_height(&self) -> f32 {
        (self.content_bottom() - self.content_top()).max(1.0)
    }

    fn viewport_width(&self) -> f32 {
        // The bar down the right takes its width out of the page area, so the
        // page is centred in what is left rather than under the bar. So does
        // the styles pane, when it is open.
        (self.view_width as f32
            - self.content_left()
            - self.styles_pane_width()
            - crate::chrome::SCROLLBAR_THICKNESS)
            .max(1.0)
    }

    /// Where a page's top-left corner sits, before scrolling is applied.
    ///
    /// Except sideways, where the scroll has already been taken off the across
    /// axis: there is nothing left for the caller to subtract, which is what
    /// [`Editor::scroll_down`] says.
    fn page_origin(&self, index: usize) -> (f32, f32) {
        let (trim_top, _) = self.page_trim();

        if self.is_side_to_side() {
            let mut x = self.content_left() + self.page_gap();
            for page in self.pages.iter().take(index) {
                x += page.width + self.page_gap();
            }
            // Down the middle of the view, which is where a page being turned
            // sits.
            let height = self.pages.get(index).map_or(0.0, |page| page.height);
            let y = ((self.viewport_height() - height) / 2.0).max(self.page_gap());
            return (x - self.scroll, y);
        }

        let mut y = self.page_gap();
        for page in self.pages.iter().take(index) {
            y += self.visible_height(page) + self.page_gap();
        }
        // The origin is where the page's own coordinates start, which is above
        // the visible band when the top margin has been trimmed away.
        let width = self.pages.get(index).map_or(0.0, |page| page.width);
        let x = self.content_left() + ((self.viewport_width() - width) / 2.0).max(PAGE_GAP)
            - self.scroll_across;
        (x, y - trim_top)
    }

    /// How much of a page is hidden at the top and at the bottom.
    ///
    /// Nothing, normally. With the white space hidden it is the two margins,
    /// which is what makes one page run into the next.
    fn page_trim(&self) -> (f32, f32) {
        if !self.joined_pages {
            return (0.0, 0.0);
        }
        let metrics = wp_layout::PageMetrics::from_document(&self.document);
        let scale = self.pixels_per_inch() / POINTS_PER_INCH;
        // A sliver of margin is left so the text does not touch the edge.
        let keep = 6.0;
        (
            (metrics.margin_top * scale - keep).max(0.0),
            (metrics.margin_bottom * scale - keep).max(0.0),
        )
    }

    /// How tall one page is on screen, once anything hidden is taken off.
    fn visible_height(&self, page: &Page) -> f32 {
        let (top, bottom) = self.page_trim();
        (page.height - top - bottom).max(1.0)
    }

    /// The space between one page and the next.
    fn page_gap(&self) -> f32 {
        // A view that draws no paper has no sheets to leave room between: the
        // text runs on, with a line where one page ends and the next begins.
        if self.joined_pages || !self.view.shows_paper() {
            JOINED_PAGE_GAP
        } else {
            PAGE_GAP
        }
    }

    fn total_height(&self) -> f32 {
        self.pages.iter().map(|page| self.visible_height(page) + self.page_gap()).sum::<f32>()
            + self.page_gap()
    }

    /// How far the scroll may run, and how much of that fits on screen.
    ///
    /// Down the pages, or across them once they are turned sideways: the same
    /// question about a different axis.
    fn scroll_extent(&self) -> (f32, f32) {
        if self.is_side_to_side() {
            let across = self.pages.iter().map(|page| page.width + self.page_gap()).sum::<f32>()
                + self.page_gap();
            return (across, self.viewport_width());
        }
        (self.total_height(), self.viewport_height())
    }

    fn clamp_scroll(&mut self) {
        let (extent, visible) = self.scroll_extent();
        let limit = (extent - visible).max(0.0);
        self.scroll = self.scroll.clamp(0.0, limit);
    }

    /// How far the view may be moved sideways.
    ///
    /// Nothing at all while the page fits across the window, which is the
    /// common case; the overhang plus a margin of desk once it does not.
    fn across_limit(&self) -> f32 {
        if self.is_side_to_side() {
            return 0.0;
        }
        let widest = self.pages.iter().fold(0.0f32, |widest, page| widest.max(page.width));
        (widest + PAGE_GAP * 2.0 - self.viewport_width()).max(0.0)
    }

    fn clamp_across(&mut self) {
        let limit = self.across_limit();
        self.scroll_across = self.scroll_across.clamp(0.0, limit);
    }

    /// Moves the view sideways. Returns whether it moved.
    fn scroll_across_by(&mut self, amount: f32) -> Response {
        let before = self.scroll_across;
        self.scroll_across += amount;
        self.clamp_across();
        if (self.scroll_across - before).abs() < 0.5 {
            return Response::Ignored;
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    fn scroll_by(&mut self, amount: f32) -> Response {
        let before = self.scroll;
        self.scroll += amount;
        self.clamp_scroll();
        if (self.scroll - before).abs() < 0.5 {
            Response::Ignored
        } else {
            self.needs_redraw = true;
            Response::Redraw
        }
    }

    // --- The caret -----------------------------------------------------------

    /// Every line in the document, as page and line indices in reading order.
    fn lines(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (page_index, page) in self.pages.iter().enumerate() {
            for line_index in 0..page.lines.len() {
                out.push((page_index, line_index));
            }
        }
        out
    }

    /// Which line the caret is on.
    fn caret_line(&self) -> Option<(usize, usize)> {
        let caret = self.caret();
        self.lines().into_iter().find(|(page, line)| {
            let line = &self.pages[*page].lines[*line];
            line.paragraph == caret.paragraph
                && caret.offset >= line.start_offset
                && caret.offset <= line.end_offset
        })
    }

    /// Which page the rulers and the status strip are about.
    ///
    /// The topmost one still on screen. Not the page the caret is on: a person
    /// who scrolls two pages down is looking at what is in front of them, and a
    /// ruler describing a page they cannot see is a ruler describing nothing.
    #[must_use]
    pub(super) fn visible_page(&self) -> usize {
        if self.is_side_to_side() {
            let left = self.content_left();
            for index in 0..self.pages.len() {
                let (origin_x, _) = self.page_origin(index);
                // A pixel of slack: a page whose right edge lands exactly on
                // the left of the view is not on screen, and floating point
                // does not always agree about "exactly".
                if origin_x + self.pages[index].width > left + 1.0 {
                    return index;
                }
            }
            return self.pages.len().saturating_sub(1);
        }

        let top = self.content_top();
        for index in 0..self.pages.len() {
            let (_, origin_y) = self.page_origin(index);
            let y = top + origin_y - self.scroll_down();
            let height = self.visible_height(&self.pages[index]);
            // The first page whose foot has not yet gone past the top of the
            // view is the one being looked at.
            if y + height > top {
                return index;
            }
        }
        self.pages.len().saturating_sub(1)
    }

    /// Which page the caret is on, counted from one.
    fn caret_page(&self) -> usize {
        self.caret_line().map_or(0, |(page, _)| page) + 1
    }

    /// Where in the document a point in the window falls, if anywhere.
    fn position_at(&self, x: i32, y: i32) -> Option<TextPosition> {
        // Nearest page first, so a drag that runs off the top or the bottom of
        // a page keeps selecting rather than stopping dead.
        let mut best: Option<(f32, TextPosition)> = None;

        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let page_x = x as f32 - origin_x;
            let page_y = y as f32 - self.content_top() - origin_y + self.scroll_down();
            let page = &self.pages[index];

            let distance = if page_y < 0.0 {
                -page_y
            } else if page_y > page.height {
                page_y - page.height
            } else {
                0.0
            };

            if best.as_ref().is_some_and(|(closest, _)| *closest <= distance) {
                continue;
            }
            if let Some(position) = page.position_at(page_x, page_y) {
                best = Some((distance, position));
            }
        }

        best.map(|(_, position)| self.snapped(position))
    }

    /// The same position, moved to the nearest place a caret may actually be.
    ///
    /// A click is aimed at a pixel, and a pixel can fall between a letter and
    /// the accent drawn on it, or inside an emoji built out of several code
    /// points. The caret never goes there: it goes to the edge of the whole
    /// character, as it does in Word.
    fn snapped(&self, position: TextPosition) -> TextPosition {
        let Some(text) = self.document.paragraph_text(position.paragraph) else {
            return position;
        };
        let nearest = wp_segment::character_boundaries(&text)
            .into_iter()
            .min_by_key(|at| at.abs_diff(position.offset))
            .unwrap_or(position.offset);
        TextPosition::new(position.paragraph, nearest)
    }

    /// Where the caret should be drawn, in window coordinates.
    fn caret_rect(&self) -> Option<(f32, f32, f32)> {
        self.caret_rect_at(self.caret())
    }

    /// The same for any position, which is what the mark showing where carried
    /// text would land needs.
    pub(super) fn caret_rect_at(&self, at: TextPosition) -> Option<(f32, f32, f32)> {
        let page_index = self.pages.iter().position(|page| {
            page.lines.iter().any(|line| {
                line.paragraph == at.paragraph
                    && at.offset >= line.start_offset
                    && at.offset <= line.end_offset
            })
        })?;
        let page = self.pages.get(page_index)?;
        let (origin_x, origin_y) = self.page_origin(page_index);
        let (x, y, height) = page.caret_at(at)?;
        Some((origin_x + x, self.content_top() + origin_y + y - self.scroll_down(), height))
    }

    /// Scrolls so the caret is on screen, if it is not already.
    fn reveal_caret(&mut self) {
        // Shown at once, and the blink counted from here.
        self.caret_on = true;
        self.caret_flipped = Instant::now();
        let Some((x, y, height)) = self.caret_rect() else { return };

        if self.is_side_to_side() {
            // Sideways, the caret goes off the side rather than off the foot.
            let left = self.content_left() + CARET_MARGIN;
            let right = (self.view_width as f32 - CARET_MARGIN).max(left + 1.0);
            if x < left {
                self.scroll -= left - x;
            } else if x > right {
                self.scroll += x - right;
            }
            self.clamp_scroll();
            return;
        }

        let top = self.content_top() + CARET_MARGIN;
        let bottom = self.content_bottom() - CARET_MARGIN;

        if y < top {
            self.scroll -= top - y;
        } else if y + height > bottom {
            self.scroll += y + height - bottom;
        }
        self.clamp_scroll();

        // And sideways, where the page is too wide to fit: typing along a line
        // that runs off the edge has to bring the edge with it.
        if self.across_limit() > 0.0 {
            let left = self.content_left() + CARET_MARGIN;
            let right = (left + self.viewport_width() - CARET_MARGIN * 2.0).max(left + 1.0);
            if x < left {
                self.scroll_across -= left - x;
            } else if x > right {
                self.scroll_across += x - right;
            }
            self.clamp_across();
        }
    }

    /// Moves the caret a line up or down, keeping roughly the same column.
    fn move_vertically(&mut self, downwards: bool, extend: bool) {
        let lines = self.lines();
        let Some(current) = self.caret_line() else { return };
        let Some(index) = lines.iter().position(|entry| *entry == current) else { return };

        let target = if downwards { index + 1 } else { index.wrapping_sub(1) };
        let Some(&(page_index, line_index)) = lines.get(target) else { return };

        let wanted_x = self
            .pages
            .get(current.0)
            .and_then(|page| page.caret_at(self.caret()))
            .map_or(0.0, |(x, _, _)| x);

        let page = &self.pages[page_index];
        let line = &page.lines[line_index];
        if let Some(position) = page.position_at(wanted_x, line.baseline) {
            self.document.move_caret(position, extend);
        }
    }

    fn move_to_line_edge(&mut self, end: bool, extend: bool) {
        let Some((page_index, line_index)) = self.caret_line() else { return };
        let line = &self.pages[page_index].lines[line_index];
        let target = TextPosition::new(
            line.paragraph,
            if end { line.end_offset } else { line.start_offset },
        );
        self.document.move_caret(target, extend);
    }

    fn move_to_document_edge(&mut self, end: bool, extend: bool) {
        let target = if end {
            let last = self.document.paragraph_count().saturating_sub(1);
            TextPosition::new(last, self.document.paragraph_text(last).unwrap_or_default().len())
        } else {
            TextPosition::new(0, 0)
        };
        self.document.move_caret(target, extend);
    }

    /// Reacts to a caret move: shows it, and repaints.
    fn moved(&mut self) -> Response {
        self.reveal_caret();
        self.needs_redraw = true;
        Response::Redraw
    }

    // --- Editing -------------------------------------------------------------

    /// Lays the document out again after a change.
    fn relayout(&mut self) {
        // The engine is kept rather than built again, because it remembers what
        // every paragraph measured to and a keystroke changes one of them. Each
        // setting is handed to it, and anything that really changed throws away
        // what depended on it.
        //
        // Text the document calls "automatic" is whatever reads against the
        // paper, and the paper depends on the theme — so the engine is told
        // both every time.
        self.engine.set_dpi(self.pixels_per_inch());
        self.engine.set_automatic_colors(self.theme.page_text, self.theme.table_line);
        self.engine.set_markup(self.show_markup);
        self.engine.set_table_gridlines(self.show_table_gridlines);
        self.engine.set_marks(self.show_marks);
        self.engine.set_outline(self.outline_for_layout());
        // A letter being previewed shows one recipient's values in place of the
        // names of its merge fields.
        let record = self.preview_record.map(|at| self.recipients.record(at)).unwrap_or_default();
        self.engine.set_merge_record(record);

        let metrics = self.view_metrics();
        self.pages = self.engine.layout_document_with(&self.document, metrics);
        self.recheck_proofing();
        // Every other window is now showing pages worked out before this.
        self.other_windows_are_stale();
        self.clamp_scroll();
        self.needs_redraw = true;
    }

    /// Finishes an edit: lays out again, shows the caret, and says so.
    fn edited(&mut self, changed: bool, note: &str) -> Response {
        if !changed {
            return Response::Ignored;
        }
        self.relayout();
        self.reveal_caret();
        self.status = note.to_owned();
        self.update_title();
        Response::Redraw
    }

    /// Says something in the strip without changing the document.
    fn report(&mut self, note: &str) -> Response {
        self.status = note.to_owned();
        self.needs_redraw = true;
        Response::Redraw
    }

    fn undo(&mut self) -> Response {
        let changed = self.document.undo();
        self.edited(changed, "Undone");
        self.needs_redraw = true;
        Response::Redraw
    }

    fn redo(&mut self) -> Response {
        let changed = self.document.redo();
        self.edited(changed, "Redone");
        self.needs_redraw = true;
        Response::Redraw
    }

    fn copy(&mut self) -> Response {
        let text = self.document.selected_text();
        if text.is_empty() {
            return self.report("Nothing is selected");
        }
        let characters = text.chars().count();
        let note = if wp_shell::clipboard::set_text(&text) {
            // The formatted content is kept here as well as the words on the
            // system clipboard, so a paste back into this program can put it
            // down the way it was. See [`wp_docx::clipboard`].
            self.clipboard = Some((text, self.document.copy_selection()));
            format!("Copied {characters} characters")
        } else {
            String::from("The clipboard is busy; nothing was copied")
        };
        self.report(&note)
    }

    fn cut(&mut self) -> Response {
        let text = self.document.selected_text();
        if text.is_empty() {
            return self.report("Nothing is selected");
        }
        // Nothing is removed unless the clipboard really took it, because text
        // that is cut and then not on the clipboard is text a person has lost.
        if !wp_shell::clipboard::set_text(&text) {
            return self.report("The clipboard is busy; nothing was cut");
        }
        let characters = text.chars().count();
        self.clipboard = Some((text, self.document.copy_selection()));
        let changed = self.document.delete_selection();
        self.edited(changed, &format!("Cut {characters} characters"))
    }

    fn select_all(&mut self) -> Response {
        self.document.select_all();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Turns a character format on or off.
    fn toggle(&mut self, format: CharacterFormat, name: &str) -> Response {
        let changed = self.document.toggle_format(format);
        let on = self.document.format_is_on(format);
        self.status = format!("{name} {}", if on { "on" } else { "off" });
        if changed {
            self.relayout();
            self.reveal_caret();
            self.update_title();
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    pub(super) fn apply_style(&mut self, style: Option<&str>) -> Response {
        let changed = self.document.set_paragraph_style_here(style);
        self.edited(changed, style.unwrap_or("Normal"))
    }

    fn apply_alignment(&mut self, alignment: Alignment, name: &str) -> Response {
        let changed = self.document.set_alignment_here(alignment);
        self.edited(changed, name)
    }

    /// Steps the size of the selection up or down the list of sizes.
    fn step_size(&mut self, up: bool) -> Response {
        let current = self.document.size_here();
        let wanted = if up {
            chrome::SIZES.iter().find(|size| **size > current + 0.01).copied()
        } else {
            chrome::SIZES.iter().rev().find(|size| **size < current - 0.01).copied()
        };
        let Some(wanted) = wanted else {
            return self.report("Already at the end of the sizes");
        };
        let changed = self.document.set_size(wanted);
        self.status = format!("{} point", chrome::format_size(wanted));
        if changed {
            self.relayout();
            self.reveal_caret();
            self.update_title();
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts the selection into a list, or takes it out of one.
    fn toggle_list(&mut self, id: i32, name: &str) -> Response {
        let current = self.document.list_here();
        let wanted = match current {
            Some(list) if list.id == id => None,
            _ => Some(NumberingReference { id, level: 0 }),
        };
        let changed = self.document.set_list_here(wanted);
        let note = if wanted.is_some() { name.to_owned() } else { format!("{name} off") };
        self.edited(changed, &note)
    }

    /// Magnifies the document, within the limits the slider allows.
    fn set_zoom(&mut self, zoom: f32) -> Response {
        let wanted = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if (wanted - self.zoom).abs() < 0.5 {
            return Response::Ignored;
        }
        self.zoom = wanted;
        self.relayout();
        self.reveal_caret();
        self.remember_window();
        self.status = format!("{}%", self.zoom.round() as i32);
        Response::Redraw
    }

    /// Steps the zoom up or down the list of ordinary magnifications.
    fn step_zoom(&mut self, up: bool) -> Response {
        let wanted = if up {
            chrome::ZOOMS.iter().find(|value| **value > self.zoom + 0.5).copied()
        } else {
            chrome::ZOOMS.iter().rev().find(|value| **value < self.zoom - 0.5).copied()
        };
        self.set_zoom(wanted.unwrap_or(self.zoom))
    }

    /// The headings of the document, for the navigation pane.
    ///
    /// Read out of the document every time rather than kept: an outline that is
    /// maintained separately is an outline that goes stale.
    fn headings(&self) -> Vec<Heading> {
        let mut out = Vec::new();
        for index in 0..self.document.paragraph_count() {
            // The outline level, not the style name: a document is free to call
            // its heading styles whatever it likes, and a translated Word does.
            let Some(level) = self.document.outline_level(index) else { continue };
            let text = self.document.paragraph_text(index).unwrap_or_default();
            if text.trim().is_empty() {
                continue;
            }
            out.push(Heading { text, level, paragraph: index });
        }
        out
    }

    /// Scrolls so a page is at the top of the view.
    pub(super) fn scroll_to_page(&mut self, index: usize) -> Response {
        let index = index.min(self.pages.len().saturating_sub(1));
        if self.is_side_to_side() {
            // The distance across to that page, which is what the scroll means
            // now. Measured from the pages rather than from the origin, because
            // the origin already has the scroll taken off it.
            let mut across = 0.0;
            for page in self.pages.iter().take(index) {
                across += page.width + self.page_gap();
            }
            self.scroll = across;
        } else {
            let (_, origin_y) = self.page_origin(index);
            self.scroll = origin_y;
        }
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// How much room the navigation pane takes, which is none when it is shut.
    pub(super) fn pane_width(&self) -> f32 {
        if self.show_navigation {
            self.navigation.width()
        } else {
            0.0
        }
    }

    /// Everything the navigation pane shows, gathered fresh.
    ///
    /// Read out of the document every time rather than kept alongside it: an
    /// outline that is rebuilt cannot go stale, and the document is the only
    /// thing that knows what is in it.
    pub(super) fn pane_contents(&self) -> Contents {
        Contents {
            headings: self.headings(),
            pages: self.pages.len().max(1),
            current_page: self.caret_page(),
            found: self.search_results(),
            notes: self
                .document
                .comments()
                .into_iter()
                .map(|comment| crate::chrome::navigation::Note {
                    author: comment.author,
                    text: comment.text,
                    paragraph: comment.range.map_or(0, |(start, _)| start.paragraph),
                    offset: comment.range.map_or(0, |(start, _)| start.offset),
                })
                .collect(),
        }
    }

    /// Everywhere the pane search box's text appears in the document.
    fn search_results(&self) -> Vec<Found> {
        let needle = self.navigation.search.trim();
        if needle.is_empty() {
            return Vec::new();
        }
        let lowered = needle.to_lowercase();

        let mut out = Vec::new();
        for paragraph in 0..self.document.paragraph_count() {
            let Some(text) = self.document.paragraph_text(paragraph) else { continue };
            let haystack = text.to_lowercase();
            let mut from = 0usize;
            while let Some(found) = haystack[from..].find(&lowered) {
                let offset = from + found;
                out.push(Found {
                    context: context_around(&text, offset, needle.len()),
                    paragraph,
                    offset,
                });
                from = offset + lowered.len().max(1);
                if out.len() >= 200 {
                    return out;
                }
            }
        }
        out
    }

    /// Which heading the caret is under.
    fn current_heading(&self, headings: &[Heading]) -> Option<usize> {
        let caret = self.caret().paragraph;
        headings.iter().rposition(|heading| heading.paragraph <= caret)
    }
}

/// A snippet of a line with the match somewhere in it, for the results list.
///
/// Cut at character boundaries and around whole words where it can be, so a
/// result reads as a phrase rather than as a fragment ending mid-letter.
fn context_around(text: &str, offset: usize, length: usize) -> String {
    const REACH: usize = 40;

    let start = text[..offset.min(text.len())]
        .char_indices()
        .rev()
        .take(REACH)
        .last()
        .map_or(0, |(index, _)| index);
    let after = (offset + length).min(text.len());
    let end = text[after..]
        .char_indices()
        .take(REACH)
        .last()
        .map_or(after, |(index, character)| after + index + character.len_utf8());

    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(text[start..end].trim());
    if end < text.len() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::Editor;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;

    /// The fonts, read once per test.
    ///
    /// Leaked rather than kept: a `FontLibrary` caches lazily and so is not
    /// shareable between threads, and the tests run on several.
    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor holding a document long enough to run over several pages.
    fn editor(paragraphs: usize) -> Editor {
        let mut body = Body::default();
        for index in 0..paragraphs {
            body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");

        let mut editor = Editor::new(library(), document, None);
        editor.view_width = 1200;
        editor.view_height = 800;
        editor.relayout();
        editor
    }

    /// The window as it stands, as bytes.
    fn painted(editor: &mut Editor) -> Vec<u8> {
        editor.paint(editor.view_width, editor.view_height);
        editor.canvas().pixels().to_vec()
    }

    #[test]
    fn a_blink_leaves_the_window_exactly_as_a_full_repaint_would() {
        // The caret blinks twice a second for as long as the window is open,
        // and drawing only the caret is only worth doing if the result cannot
        // be told from drawing everything. So: blink it off, blink it on, and
        // compare with a window drawn from nothing.
        let mut editor = editor(40);
        editor.caret_on = true;
        let with_caret = painted(&mut editor);

        editor.caret_on = false;
        editor.caret_only = true;
        let without = painted(&mut editor);

        editor.needs_redraw = true;
        let expected_without = painted(&mut editor);
        assert_eq!(without, expected_without, "the caret was not taken away cleanly");

        editor.caret_on = true;
        editor.caret_only = true;
        let back = painted(&mut editor);
        assert_eq!(back, with_caret, "the caret did not come back the way it was");
    }

    #[test]
    fn a_blink_over_an_open_list_draws_the_window_rather_than_the_caret() {
        // A list dropped over the page may have been drawn on top of the
        // caret, and putting back what was under it would put it back over the
        // list.
        let mut editor = editor(40);
        editor.caret_on = true;
        painted(&mut editor);

        editor.popup = Some(crate::chrome::Popup::new(
            crate::chrome::Choice::Zoom,
            vec!["100%".to_owned()],
            None,
            100.0,
            300.0,
            120.0,
        ));
        let with_list = painted(&mut editor);

        editor.caret_on = false;
        editor.caret_only = true;
        let blinked = painted(&mut editor);
        assert_ne!(blinked, with_list, "nothing was drawn at all");

        editor.needs_redraw = true;
        editor.caret_on = false;
        assert_eq!(blinked, painted(&mut editor), "the list was drawn over");
    }
    #[test]
    fn the_page_being_looked_at_is_the_first_one_before_anything_is_scrolled() {
        assert_eq!(editor(200).visible_page(), 0);
    }

    #[test]
    fn scrolling_past_a_page_moves_on_to_the_next() {
        let mut editor = editor(200);
        assert!(editor.pages.len() > 1, "the document should run over more than one page");

        // Far enough down that the first page's foot has gone past the top.
        let (_, second_origin) = editor.page_origin(1);
        editor.scroll = second_origin + 10.0;
        assert_eq!(editor.visible_page(), 1);
    }

    #[test]
    fn the_ruler_describes_a_page_that_is_actually_on_screen() {
        let mut editor = editor(200);

        // Scrolled two pages down, where the first page is long gone.
        let (_, third_origin) = editor.page_origin(2);
        editor.scroll = third_origin + 10.0;

        let (_, side) = editor.ruler_measurements();
        // The page the ruler measures has to overlap the band it is drawn
        // beside. Measuring page one from page three puts the whole of it above
        // the view, and the ruler then shows margin and no paper at all.
        assert!(
            side.page_top < side.bottom && side.page_top + side.page_height > side.top,
            "the ruler measures a page from {} to {}, and the view runs {} to {}",
            side.page_top,
            side.page_top + side.page_height,
            side.top,
            side.bottom
        );
    }

    #[test]
    fn web_layout_puts_the_whole_document_on_one_sheet() {
        let mut editor = editor(200);
        assert!(editor.pages.len() > 1, "print layout should need several pages");

        editor.set_view(super::views::View::Web);
        assert_eq!(editor.pages.len(), 1, "web layout runs on without page breaks");
    }

    #[test]
    fn web_layout_makes_the_sheet_as_wide_as_the_window() {
        let mut editor = editor(20);
        let printed = editor.pages[0].width;
        editor.set_view(super::views::View::Web);
        assert!(
            editor.pages[0].width > printed,
            "a window 1200 wide should give a sheet wider than A4"
        );
    }

    #[test]
    fn draft_keeps_the_pages_and_takes_the_margins() {
        let mut editor = editor(200);
        let printed = editor.pages.len();
        editor.set_view(super::views::View::Draft);
        assert!(editor.pages.len() <= printed, "narrower margins hold more, not less");
        assert!(editor.pages.len() > 1, "draft still breaks pages");
    }

    #[test]
    fn reading_mode_takes_the_ribbon_away_and_gives_it_back() {
        let mut editor = editor(20);
        let with_ribbon = editor.ribbon_bottom();

        editor.set_view(super::views::View::Reading);
        assert!(editor.ribbon_bottom() < with_ribbon, "the ribbon is still taking room");
        assert!(!editor.show_rulers);

        editor.set_view(super::views::View::Print);
        assert_eq!(editor.ribbon_bottom(), with_ribbon);
        assert!(editor.show_rulers, "the rulers should come back");
    }

    #[test]
    fn a_document_of_one_page_never_looks_at_another() {
        let mut editor = editor(2);
        assert_eq!(editor.pages.len(), 1);
        editor.scroll = 10_000.0;
        assert_eq!(editor.visible_page(), 0);
    }

    #[test]
    fn a_window_that_is_not_split_has_one_pane_filling_it() {
        let editor = editor(10);
        assert!(!editor.is_split());
        assert_eq!(editor.pane_band(0), editor.whole_content_band());
    }

    #[test]
    fn splitting_gives_two_panes_that_between_them_fill_the_window() {
        let mut editor = editor(10);
        editor.toggle_split();

        let (whole_top, whole_bottom) = editor.whole_content_band();
        let (upper_top, upper_bottom) = editor.pane_band(0);
        let (lower_top, lower_bottom) = editor.pane_band(1);

        assert_eq!(upper_top, whole_top);
        assert_eq!(lower_bottom, whole_bottom);
        // The bar sits between them, so the lower pane starts below the upper.
        assert!(lower_top > upper_bottom, "{lower_top} is not below {upper_bottom}");
        assert!(upper_bottom > upper_top && lower_bottom > lower_top, "a pane has no height");
    }

    #[test]
    fn the_viewport_is_the_pane_being_edited() {
        let mut editor = editor(10);
        let whole = editor.viewport_height();
        editor.toggle_split();
        assert!(editor.viewport_height() < whole, "the pane is not smaller than the window");
    }

    #[test]
    fn the_two_panes_scroll_apart_from_one_another() {
        let mut editor = editor(60);
        editor.toggle_split();
        editor.scroll_by(200.0);
        let upper = editor.scroll;

        // Clicking in the lower pane makes it the one that scrolls.
        let (lower_top, _) = editor.pane_band(1);
        editor.activate_pane(editor.pane_at(lower_top + 5.0));
        assert_eq!(editor.active_pane, 1);
        assert!(
            (editor.scroll - upper).abs() > 1.0,
            "the second view came up scrolled where the first one was"
        );

        // And the first pane's place is waiting when it is gone back to.
        editor.activate_pane(0);
        assert!((editor.scroll - upper).abs() < 1.0, "the first view lost its place");
    }

    #[test]
    fn the_bar_can_be_dragged_and_stays_inside_the_window() {
        let mut editor = editor(10);
        editor.toggle_split();
        let (top, bottom) = editor.whole_content_band();

        editor.start_split_drag();
        editor.drag_split_to(top + (bottom - top) * 0.75);
        let bar = editor.split_bar_top().expect("a bar");
        assert!(bar > top && bar < bottom, "the bar at {bar} left the band {top}..{bottom}");

        // Dragged off the top, it stops short of the edge rather than leaving
        // a pane with no height at all.
        editor.drag_split_to(-500.0);
        let (_, upper_bottom) = editor.pane_band(0);
        assert!(upper_bottom > top, "the upper pane was squeezed away");
    }

    #[test]
    fn splitting_and_unsplitting_leaves_the_window_as_it_was() {
        let mut editor = editor(10);
        let before = editor.pane_band(0);
        editor.toggle_split();
        editor.toggle_split();
        assert!(!editor.is_split());
        assert_eq!(editor.pane_band(0), before);
    }

    #[test]
    fn pages_run_down_the_window_until_they_are_turned_sideways() {
        let mut editor = editor(40);
        assert!(!editor.is_side_to_side());
        let (first_x, first_y) = editor.page_origin(0);
        let (second_x, second_y) = editor.page_origin(1);
        assert_eq!(first_x, second_x, "one page is not above the other");
        assert!(second_y > first_y);

        editor.toggle_movement();
        assert!(editor.is_side_to_side());
        let (first_x, first_y) = editor.page_origin(0);
        let (second_x, second_y) = editor.page_origin(1);
        assert_eq!(first_y, second_y, "one page is not beside the other");
        assert!(second_x > first_x);
    }

    #[test]
    fn turning_the_pages_sideways_scrolls_sideways() {
        let mut editor = editor(40);
        editor.toggle_movement();
        // Nothing of the scroll goes downwards any more, so the page stays put
        // up and down however far it is scrolled across.
        let (_, before) = editor.page_origin(0);
        editor.scroll_by(300.0);
        let (x, after) = editor.page_origin(0);
        assert_eq!(before, after, "the page moved up or down");
        assert!(x < editor.content_left(), "the page did not move across");
        assert_eq!(editor.scroll_down(), 0.0);
    }

    #[test]
    fn a_page_turned_sideways_sits_down_the_middle_of_the_view() {
        let mut editor = editor(5);
        editor.toggle_movement();
        let (_, y) = editor.page_origin(0);
        let height = editor.pages[0].height;
        let room = editor.viewport_height();
        // Within a pixel of centred, or hard against the top when the page is
        // taller than the window.
        let expected = ((room - height) / 2.0).max(editor.page_gap());
        assert!((y - expected).abs() < 1.0, "{y} is not {expected}");
    }

    #[test]
    fn the_scroll_stops_at_the_last_page_either_way_round() {
        for sideways in [false, true] {
            let mut editor = editor(30);
            if sideways {
                editor.toggle_movement();
            }
            editor.scroll_by(100_000.0);
            let (extent, visible) = editor.scroll_extent();
            assert!(
                (editor.scroll - (extent - visible).max(0.0)).abs() < 1.0,
                "sideways {sideways}: stopped at {}",
                editor.scroll
            );
        }
    }

    #[test]
    fn turning_the_pages_sideways_keeps_the_page_being_looked_at() {
        let mut editor = editor(300);
        editor.scroll_to_page(3);
        assert_eq!(editor.visible_page(), 3);
        editor.toggle_movement();
        assert_eq!(editor.visible_page(), 3, "the view jumped to another page");
    }

    #[test]
    fn turning_them_back_puts_them_one_above_the_next_again() {
        let mut editor = editor(120);
        editor.toggle_movement();
        editor.toggle_movement();
        assert!(!editor.is_side_to_side());
        let (first_x, _) = editor.page_origin(0);
        let (second_x, _) = editor.page_origin(1);
        assert_eq!(first_x, second_x);
    }

    /// An editor holding headings of three levels with a paragraph under each.
    fn outlined() -> Editor {
        use wp_docx::model::ParagraphProperties;

        let heading = |level: u8, text: &str| {
            Block::Paragraph(Paragraph {
                properties: ParagraphProperties {
                    style: Some(format!("Heading{level}")),
                    ..ParagraphProperties::default()
                },
                runs: vec![wp_docx::model::Run::text(text)],
            })
        };

        let mut body = Body::default();
        for chapter in 0..3 {
            body.blocks.push(heading(1, &format!("Chapter {chapter}")));
            body.blocks.push(Block::Paragraph(Paragraph::text("Body under the chapter")));
            body.blocks.push(heading(2, &format!("Section {chapter}")));
            body.blocks.push(Block::Paragraph(Paragraph::text("Body under the section")));
            body.blocks.push(heading(3, &format!("Point {chapter}")));
        }

        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.view_width = 1200;
        editor.view_height = 800;
        editor.relayout();
        editor
    }

    /// The text of every paragraph actually drawn.
    fn shown(editor: &Editor) -> Vec<String> {
        let mut out = Vec::new();
        for page in &editor.pages {
            for line in &page.lines {
                let text = editor.document.paragraph_text(line.paragraph).unwrap_or_default();
                if !out.contains(&text) {
                    out.push(text);
                }
            }
        }
        out
    }

    #[test]
    fn a_document_shows_everything_until_it_is_shown_as_an_outline() {
        let editor = outlined();
        assert!(shown(&editor).iter().any(|text| text.starts_with("Body under")));
    }

    #[test]
    fn the_top_level_of_an_outline_shows_the_chapters_alone() {
        let mut editor = outlined();
        editor.choose_outline_level(0);

        let shown = shown(&editor);
        assert!(shown.iter().any(|text| text.starts_with("Chapter")), "{shown:?}");
        assert!(!shown.iter().any(|text| text.starts_with("Section")), "{shown:?}");
        assert!(!shown.iter().any(|text| text.starts_with("Body")), "{shown:?}");
    }

    #[test]
    fn a_deeper_level_shows_more_of_the_document() {
        let mut editor = outlined();
        editor.choose_outline_level(1);
        let shown = shown(&editor);
        assert!(shown.iter().any(|text| text.starts_with("Section")), "{shown:?}");
        assert!(!shown.iter().any(|text| text.starts_with("Point")), "{shown:?}");
        assert!(!shown.iter().any(|text| text.starts_with("Body")), "{shown:?}");
    }

    #[test]
    fn all_levels_shows_the_body_text_too() {
        let mut editor = outlined();
        editor.choose_outline_level(usize::from(super::outline::ALL_LEVELS) - 1);
        assert!(shown(&editor).iter().any(|text| text.starts_with("Body")));
    }

    #[test]
    fn each_level_of_an_outline_is_further_in_than_the_one_above_it() {
        let mut editor = outlined();
        editor.choose_outline_level(usize::from(super::outline::ALL_LEVELS) - 1);

        // The left edge of the first line of each of the first three
        // paragraphs: a chapter, its body, and a section.
        let left = |text: &str| {
            editor
                .pages
                .iter()
                .flat_map(|page| page.lines.iter())
                .find(|line| {
                    editor
                        .document
                        .paragraph_text(line.paragraph)
                        .is_some_and(|found| found.starts_with(text))
                })
                .map(|line| line.left)
                .unwrap_or_else(|| panic!("no line for {text}"))
        };

        assert!(left("Section 0") > left("Chapter 0"), "a section is not indented past a chapter");
        assert!(left("Point 0") > left("Section 0"), "a point is not indented past a section");
    }

    #[test]
    fn a_caret_in_a_hidden_paragraph_moves_to_one_that_is_shown() {
        let mut editor = outlined();
        // Into the body text under the first chapter, which level one hides.
        editor.document.set_caret(wp_docx::TextPosition::new(1, 3));
        editor.choose_outline_level(0);

        let caret = editor.document.caret();
        let visible = editor
            .pages
            .iter()
            .any(|page| page.lines.iter().any(|line| line.paragraph == caret.paragraph));
        assert!(visible, "the caret was left in a paragraph nobody can see");
    }

    #[test]
    fn leaving_the_outline_puts_the_whole_document_back() {
        let mut editor = outlined();
        editor.choose_outline_level(0);
        editor.choose_outline_level(usize::from(super::outline::ALL_LEVELS));
        assert!(shown(&editor).iter().any(|text| text.starts_with("Body")));
    }

    #[test]
    fn a_second_window_starts_where_the_first_one_is() {
        let mut editor = editor(60);
        editor.scroll_by(200.0);
        let where_it_was = editor.scroll;

        // Nothing is saved for a window that has just opened, so it takes up
        // the place the one it was opened from is looking at.
        editor.use_window(1);
        assert_eq!(editor.active_window, 1);
        assert!((editor.scroll - where_it_was).abs() < 1.0, "got {}", editor.scroll);
    }

    #[test]
    fn each_window_keeps_its_own_place_in_the_document() {
        let mut editor = editor(120);
        editor.scroll_by(150.0);
        let first = editor.scroll;

        editor.use_window(1);
        editor.scroll_by(400.0);
        let second = editor.scroll;
        assert!((second - first).abs() > 1.0, "the second window did not move");

        editor.use_window(0);
        assert!((editor.scroll - first).abs() < 1.0, "the first window lost its place");
        editor.use_window(1);
        assert!((editor.scroll - second).abs() < 1.0, "the second window lost its place");
    }

    #[test]
    fn each_window_keeps_its_own_magnification() {
        let mut editor = editor(20);
        editor.use_window(1);
        editor.set_zoom(150.0);
        assert!((editor.zoom - 150.0).abs() < 0.5);

        editor.use_window(0);
        assert!((editor.zoom - 100.0).abs() < 0.5, "the first window was magnified too");
    }

    #[test]
    fn switching_to_the_window_already_being_answered_changes_nothing() {
        let mut editor = editor(20);
        editor.scroll_by(100.0);
        let before = editor.scroll;
        editor.use_window(0);
        assert_eq!(editor.scroll, before);
    }

    #[test]
    fn typing_in_one_window_leaves_the_other_needing_to_be_laid_out_again() {
        let mut editor = editor(20);
        // Give the second window something saved, then go back to the first.
        editor.use_window(1);
        editor.use_window(0);

        editor.type_character('x');
        // The second window's pages were worked out before the letter existed.
        editor.use_window(1);
        assert_eq!(editor.document.plain_text().matches('x').count(), 1);
        assert!(!editor.pages.is_empty(), "the second window was left with no pages");
    }

    #[test]
    fn the_document_is_the_same_document_in_every_window() {
        let mut editor = editor(10);
        editor.type_character('q');
        editor.use_window(1);
        assert!(editor.document.plain_text().contains('q'));
    }

    /// Where on screen a position in the text is, for a test to press there.
    fn point_at(editor: &Editor, paragraph: usize, offset: usize) -> (i32, i32) {
        let at = wp_docx::TextPosition::new(paragraph, offset);
        let (x, y, height) = editor.caret_rect_at(at).expect("a place on screen");
        (x as i32 + 1, (y + height / 2.0) as i32)
    }

    #[test]
    fn a_press_inside_the_selection_decides_nothing_yet() {
        let mut editor = editor(10);
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 9));

        let (x, y) = point_at(&editor, 0, 4);
        assert!(editor.press_may_drag_text(x, y, false, false));
        editor.wait_for_text_drag(x, y);
        // The selection is still there: nothing has been decided.
        assert!(editor.document.selection().is_some());
    }

    #[test]
    fn a_press_outside_the_selection_is_not_a_drag() {
        let mut editor = editor(10);
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 5));

        let (x, y) = point_at(&editor, 2, 3);
        assert!(!editor.press_may_drag_text(x, y, false, false));
    }

    #[test]
    fn shift_and_control_do_not_start_a_drag() {
        let mut editor = editor(10);
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 9));

        let (x, y) = point_at(&editor, 0, 4);
        assert!(!editor.press_may_drag_text(x, y, true, false), "shift reaches instead");
        assert!(!editor.press_may_drag_text(x, y, false, true), "control jumps instead");
    }

    #[test]
    fn a_press_that_never_moves_gives_up_the_selection() {
        let mut editor = editor(10);
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 9));

        let (x, y) = point_at(&editor, 0, 4);
        editor.wait_for_text_drag(x, y);
        editor.drop_text(x, y, false);
        assert!(editor.document.selection().is_none(), "the selection was not given up");
    }

    #[test]
    fn carrying_text_somewhere_else_moves_it() {
        let mut editor = editor(10);
        let before = editor.document.plain_text();
        // "Paragraph" of the first paragraph.
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 9));

        let (from_x, from_y) = point_at(&editor, 0, 4);
        editor.wait_for_text_drag(from_x, from_y);
        let (to_x, to_y) = point_at(&editor, 3, 0);
        editor.drag_text(to_x, to_y);
        assert!(editor.dragging_text(), "the drag did not start");
        editor.drop_text(to_x, to_y, false);

        let after = editor.document.plain_text();
        assert_ne!(after, before, "nothing moved");
        assert!(after.starts_with(" 0"), "the text was not taken away: {after:?}");
        assert!(
            editor.document.paragraph_text(3).is_some_and(|text| text.starts_with("Paragraph")),
            "the text did not arrive"
        );
    }

    #[test]
    fn carrying_it_with_control_leaves_the_original_where_it_was() {
        let mut editor = editor(10);
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 9));

        let (from_x, from_y) = point_at(&editor, 0, 4);
        editor.wait_for_text_drag(from_x, from_y);
        let (to_x, to_y) = point_at(&editor, 3, 0);
        editor.drag_text(to_x, to_y);
        editor.drop_text(to_x, to_y, true);

        assert!(
            editor.document.paragraph_text(0).is_some_and(|text| text.starts_with("Paragraph")),
            "the original was taken away"
        );
    }

    #[test]
    fn letting_go_inside_the_text_it_came_from_changes_nothing() {
        let mut editor = editor(10);
        let before = editor.document.plain_text();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 9));

        let (from_x, from_y) = point_at(&editor, 0, 2);
        editor.wait_for_text_drag(from_x, from_y);
        let (to_x, to_y) = point_at(&editor, 0, 6);
        editor.drag_text(to_x, to_y);
        editor.drop_text(to_x, to_y, false);

        assert_eq!(editor.document.plain_text(), before);
    }

    #[test]
    fn a_move_is_one_thing_to_undo() {
        let mut editor = editor(10);
        let before = editor.document.plain_text();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 9));

        let (from_x, from_y) = point_at(&editor, 0, 4);
        editor.wait_for_text_drag(from_x, from_y);
        let (to_x, to_y) = point_at(&editor, 3, 0);
        editor.drag_text(to_x, to_y);
        editor.drop_text(to_x, to_y, false);

        assert!(editor.document.undo());
        assert_eq!(editor.document.plain_text(), before);
    }

    #[test]
    fn a_page_that_fits_across_the_window_does_not_scroll_sideways() {
        let editor = editor(3);
        assert_eq!(editor.across_limit(), 0.0);
        assert!(editor.across_scroll_bar().is_none(), "a bar was shown with nothing to scroll");
    }

    #[test]
    fn a_page_too_wide_for_the_window_scrolls_sideways() {
        let mut editor = editor(3);
        editor.view_width = 400;
        editor.relayout();

        assert!(editor.across_limit() > 0.0, "the page fits, so nothing is proved");
        assert!(editor.across_scroll_bar().is_some(), "no bar to reach the right of the page with");
    }

    #[test]
    fn scrolling_sideways_moves_the_page_and_stops_at_the_edge() {
        let mut editor = editor(3);
        editor.view_width = 400;
        editor.relayout();
        let (before, _) = editor.page_origin(0);

        editor.scroll_across_by(60.0);
        let (after, _) = editor.page_origin(0);
        assert!((before - after - 60.0).abs() < 0.5, "the page did not move with the scroll");

        // And it stops rather than running on into nothing.
        editor.scroll_across_by(100_000.0);
        assert!((editor.scroll_across - editor.across_limit()).abs() < 0.5);
        editor.scroll_across_by(-100_000.0);
        assert_eq!(editor.scroll_across, 0.0);
    }

    #[test]
    fn a_click_lands_where_it_looks_after_scrolling_sideways() {
        // The scroll moves the page, so where a point falls in the text has to
        // move with it — otherwise every click after a scroll is in the wrong
        // place.
        let mut editor = editor(3);
        editor.view_width = 400;
        editor.relayout();

        let (x, y) = point_at(&editor, 1, 3);
        let before = editor.position_at(x, y);
        editor.scroll_across_by(40.0);
        let after = editor.position_at(x - 40, y);
        assert_eq!(before, after, "the same place in the text is no longer under the same point");
    }

    #[test]
    fn alt_shows_the_letters_and_alt_again_puts_them_away() {
        let mut editor = editor(3);
        assert!(!editor.showing_key_tips());
        editor.toggle_key_tips();
        assert!(editor.showing_key_tips());
        editor.toggle_key_tips();
        assert!(!editor.showing_key_tips());
    }

    #[test]
    fn a_letter_opens_the_tab_it_belongs_to() {
        // Alt then N is Insert, in Word and here.
        let mut editor = editor(3);
        editor.paint(1200, 800);
        editor.toggle_key_tips();
        editor.press_key_tip('n');
        assert_eq!(editor.ribbon.tab, crate::chrome::ribbon::Tab::Insert);
        assert!(editor.showing_key_tips(), "the letters went away before the command was picked");
    }

    #[test]
    fn escape_steps_back_out_a_level_at_a_time() {
        let mut editor = editor(3);
        editor.paint(1200, 800);
        editor.toggle_key_tips();
        editor.press_key_tip('h');
        editor.leave_key_tips();
        assert!(editor.showing_key_tips(), "one Escape put them all away");
        editor.leave_key_tips();
        assert!(!editor.showing_key_tips());
    }

    #[test]
    fn a_letter_that_belongs_to_nothing_leaves_them_showing() {
        let mut editor = editor(3);
        editor.paint(1200, 800);
        editor.toggle_key_tips();
        assert_eq!(editor.press_key_tip('ф'), wp_shell::Response::Ignored);
        assert!(editor.showing_key_tips());
    }

    #[test]
    fn the_middle_button_starts_the_scroll_and_stops_it() {
        let mut editor = editor(60);
        editor.paint(1200, 800);
        editor.toggle_autoscroll(500, 400);
        assert!(editor.autoscrolling());
        editor.toggle_autoscroll(500, 400);
        assert!(!editor.autoscrolling());
    }

    #[test]
    fn the_wheel_pressed_on_the_ribbon_starts_nothing() {
        let mut editor = editor(60);
        editor.paint(1200, 800);
        editor.toggle_autoscroll(500, 10);
        assert!(!editor.autoscrolling(), "the ribbon is not the document");
    }

    #[test]
    fn the_document_moves_away_from_the_mark_and_not_towards_it() {
        let mut editor = editor(60);
        editor.paint(1200, 800);
        editor.toggle_autoscroll(500, 400);

        // The pointer resting on the mark moves nothing.
        editor.pointer_y = 400.0;
        assert_eq!(editor.autoscroll_tick(), wp_shell::Response::Ignored);

        // Below it, the document goes down.
        editor.pointer_y = 700.0;
        let before = editor.scroll;
        editor.autoscroll_tick();
        assert!(editor.scroll > before, "the document did not follow the pointer down");

        // And above it, back up.
        editor.pointer_y = 100.0;
        let before = editor.scroll;
        editor.autoscroll_tick();
        assert!(editor.scroll < before, "the document did not follow the pointer up");
    }

    #[test]
    fn the_strip_along_the_bottom_can_be_told_what_to_show() {
        let mut editor = editor(3);
        assert!(editor.status_shows.at(0), "the page number should show to begin with");
        editor.open_status_menu(300, 780);
        // The first row is the heading, so the first part is the second row.
        editor.choose_status_part(1);
        assert!(!editor.status_shows.at(0), "the page number was not switched off");
        assert!(editor.popup.is_some(), "the list closed when something was ticked");
    }

    /// An editor whose paragraph has one right-hand stop with dots.
    fn with_a_tab_stop() -> Editor {
        use wp_docx::model::{TabAlignment, TabLeader, TabStop};
        let mut editor = editor(3);
        editor.paint(1200, 800);
        editor.document.set_tab_stops_here(&[TabStop {
            position: 2880,
            alignment: TabAlignment::End,
            leader: TabLeader::Dot,
        }]);
        editor
    }

    #[test]
    fn a_double_click_on_a_stop_opens_the_tabs_dialog_on_it() {
        // Word opens its Tabs dialog here. There was a menu instead until the
        // dialog existed; a menu Word does not have is one nobody looks for.
        let mut editor = with_a_tab_stop();
        editor.open_tab_stop_menu(0, 400, 100);

        let dialog = editor.dialog.as_ref().expect("the dialog is open");
        assert_eq!(dialog.title, "Tabs");
        // Opened on the stop that was double-clicked, not on a blank one.
        assert_eq!(dialog.said(2), "2.00", "the stop's own position is not shown");
    }

    #[test]
    fn the_tabs_dialog_changes_what_a_stop_does_to_the_text() {
        use wp_docx::model::TabAlignment;
        let mut editor = with_a_tab_stop();
        editor.open_tab_stop_menu(0, 400, 100);

        // Right is the third alignment Word offers.
        if let Some(dialog) = &mut editor.dialog {
            if let Some(crate::chrome::dialog::Field::Choice { current, .. }) =
                dialog.fields.get_mut(5)
            {
                *current = 2;
            }
        }
        let dialog = editor.dialog.clone().expect("a dialog");
        editor.tabs_dialog_button(&dialog, "Set");

        let stops = editor.document.tab_stops_here();
        assert_eq!(stops.len(), 1, "Set on an existing stop replaces it");
        assert_eq!(stops[0].alignment, TabAlignment::End);
    }

    #[test]
    fn a_stop_can_be_cleared_from_the_tabs_dialog() {
        let mut editor = with_a_tab_stop();
        editor.open_tab_stop_menu(0, 400, 100);
        let dialog = editor.dialog.clone().expect("a dialog");
        editor.tabs_dialog_button(&dialog, "Clear");
        assert!(editor.document.tab_stops_here().is_empty());
    }

    #[test]
    fn clear_all_takes_every_stop_away() {
        use wp_docx::model::{TabAlignment, TabLeader, TabStop};
        let mut editor = with_a_tab_stop();
        editor.document.add_tab_stop_here(TabStop {
            position: 5760,
            alignment: TabAlignment::Center,
            leader: TabLeader::None,
        });
        assert_eq!(editor.document.tab_stops_here().len(), 2);

        editor.open_tab_stop_menu(0, 400, 100);
        let dialog = editor.dialog.clone().expect("a dialog");
        editor.tabs_dialog_button(&dialog, "Clear All");
        assert!(editor.document.tab_stops_here().is_empty());
    }

    #[test]
    fn editing_a_header_puts_the_header_and_footer_tab_on_the_ribbon() {
        use crate::chrome::ribbon::Tab;
        let mut editor = editor(3);
        editor.paint(1200, 800);
        editor
            .document
            .set_furniture(
                wp_docx::furniture::Furniture::Header,
                wp_docx::furniture::Preset::Text,
                wp_docx::model::Alignment::Start,
                "Title",
            )
            .expect("a header");
        editor.relayout();

        editor.edit_furniture(wp_docx::furniture::Furniture::Header);
        assert!(editor.in_furniture(), "the header was not opened");
        assert_eq!(editor.ribbon.tab, Tab::HeaderFooter);
        assert!(editor.toolbar_state().in_furniture, "the ribbon was not told");
        assert!(Tab::HeaderFooter.applies(&editor.toolbar_state()));

        editor.leave_furniture();
        assert!(!editor.in_furniture());
        assert_ne!(editor.ribbon.tab, Tab::HeaderFooter, "the tab stayed after coming out");
    }
}

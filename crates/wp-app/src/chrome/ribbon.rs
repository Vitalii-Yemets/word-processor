//! The ribbon: tabs, groups of commands, and the labels that name them.
//!
//! # Why a ribbon rather than a row of buttons
//!
//! A word processor has too many commands for one row and too few for a menu
//! nobody can find anything in. The ribbon's answer is to name the groups: a
//! person looking for "make this a bullet list" looks under Paragraph, and the
//! label under the group is what tells them they are in the right place. That
//! naming is the whole point, so it is what this reproduces first.
//!
//! # Why every button is here, including the ones that do nothing yet
//!
//! The tabs and groups below are Word's, in Word's order, down to the buttons
//! whose work is not written. That is deliberate. Somebody who knows Word knows
//! that line numbering is under Layout and a citation is under References, and
//! that knowledge should carry across. A button that is not built says so when
//! it is pressed, by name, which is a great deal better than the command not
//! being anywhere and the person concluding it cannot be done.
//!
//! Everything is drawn onto the same canvas as the document, by the same
//! rasterizer, out of the same fonts. There are no system controls in the
//! window at all.

use wp_docx::model::Alignment;
use wp_docx::CharacterFormat;
use wp_layout::{LayoutEngine, Renderer, TextStyle};
use wp_raster::{Canvas, Color};

use super::icons::{self, Icon};
use super::theme::Theme;
use super::{Choice, Command, TableBorderChoice, ToolbarState};

/// Height of the strip of tab names.
pub const TAB_HEIGHT: f32 = 32.0;
/// Height of the ribbon under the tabs.
pub const RIBBON_HEIGHT: f32 = 100.0;
/// Everything the ribbon occupies together.
pub const TOTAL_HEIGHT: f32 = TAB_HEIGHT + RIBBON_HEIGHT;

/// Height of the label naming a group, along the bottom of the ribbon.
const GROUP_LABEL_HEIGHT: f32 = 15.0;
/// A row of small buttons.
const ROW_HEIGHT: f32 = 26.0;
/// Space between one group and the next, either side of the separator.
const GROUP_PADDING: f32 = 8.0;
/// And between two columns of small buttons inside one group.
const COLUMN_GAP: f32 = 6.0;
/// How wide a group given up to one button is.
const COLLAPSED_WIDTH: f32 = 62.0;
const EDGE: f32 = 4.0;
/// How wide one tile of the style gallery is.
const STYLE_TILE_WIDTH: f32 = 76.0;
/// How many of them are shown when there is room for them all, and the fewest
/// worth showing when there is not.
///
/// The gallery is the widest thing on the Home tab, so it is what makes the
/// ribbon too long for a narrow window. Word shrinks it and keeps the group;
/// giving up the whole group instead would take the styles off the tab a person
/// uses them from.
const STYLE_TILES_MOST: usize = 8;
const STYLE_TILES_LEAST: usize = 3;
/// The letters a style tile is shown with, which are Word's own.
const SPECIMEN: &str = "AaBbCcDdEe";

/// Which tab of the ribbon is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    File,
    Home,
    Insert,
    Design,
    Layout,
    References,
    Mailings,
    Review,
    View,
    Help,
    /// The two that appear only when the caret is in a table, as Word's do.
    TableDesign,
    TableLayout,
    /// And the one that appears while a header or a footer is being edited.
    HeaderFooter,
}

impl Tab {
    /// Every tab, in the order Word shows them.
    pub const ALL: &'static [Tab] = &[
        Tab::File,
        Tab::Home,
        Tab::Insert,
        Tab::Design,
        Tab::Layout,
        Tab::References,
        Tab::Mailings,
        Tab::Review,
        Tab::View,
        Tab::Help,
    ];

    /// The tabs that are only shown while what they are about is being worked
    /// on: the two for a table, and the one for a header or a footer.
    pub const CONTEXTUAL: &'static [Tab] = &[Tab::TableDesign, Tab::TableLayout, Tab::HeaderFooter];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Home => "Home",
            Self::Insert => "Insert",
            Self::Design => "Design",
            Self::Layout => "Layout",
            Self::References => "References",
            Self::Mailings => "Mailings",
            Self::Review => "Review",
            Self::View => "View",
            Self::Help => "Help",
            Self::TableDesign => "Table Design",
            Self::TableLayout => "Table Layout",
            Self::HeaderFooter => "Header & Footer",
        }
    }

    /// The letter Alt puts over the tab.
    ///
    /// Word's own, rather than anything worked out from the name: Alt+H for
    /// Home and Alt+N for Insert are in the fingers of everybody who uses the
    /// program, and a copy that put different letters there would be a copy
    /// they could not use.
    #[must_use]
    pub fn key_tip(self) -> &'static str {
        match self {
            Self::File => "F",
            Self::Home => "H",
            Self::Insert => "N",
            Self::Design => "G",
            Self::Layout => "P",
            Self::References => "S",
            Self::Mailings => "M",
            Self::Review => "R",
            Self::View => "W",
            Self::Help => "Y",
            // Word reaches the two table tabs through J; with no other tab on
            // that letter here, they take one of their own.
            // Word reaches the header and footer tab through J as well; here
            // it takes a letter nothing else has.
            Self::HeaderFooter => "E",
            Self::TableDesign => "T",
            Self::TableLayout => "L",
        }
    }

    /// Whether the tab applies just now: a contextual tab is only there while
    /// what it is about is being worked on.
    #[must_use]
    pub fn applies(self, state: &ToolbarState) -> bool {
        match self {
            Self::TableDesign | Self::TableLayout => state.in_table,
            Self::HeaderFooter => state.in_furniture,
            _ => true,
        }
    }
}

/// One thing on the ribbon.
#[derive(Clone, Copy, Debug)]
pub enum Item {
    /// An icon above a label, for the command a group is really about.
    Large(Command, Icon, &'static str),
    /// An icon with its label beside it.
    Small(Command, Icon, &'static str),
    /// An icon on its own, for the commands whose shape says everything.
    Button(Command, Icon),
    /// A letter set in the formatting it applies.
    Letter(Command, &'static str, TextStyle),
    /// A box showing what is in effect, which opens a list when pressed.
    Field(Command, Choice, f32),
    /// A box showing a measurement, with the label that names it.
    Measure(Command, &'static str, f32),
    /// The gallery of paragraph styles, each shown in its own formatting.
    StyleGallery,
    /// Starts a new row within the group.
    Break,
    /// Starts a new column of rows within the group.
    ///
    /// A row break wraps into a new column once the group is as tall as the
    /// ribbon; this asks for one outright. Word's Paragraph group on the Layout
    /// tab is two columns of two — the indents beside the spacing — and left to
    /// wrap it would come out three and one.
    NewColumn,
}

/// A button that drops a menu, and how much of it drops one.
///
/// Word has two kinds and they do not behave the same. A **split button** runs
/// a command from its face and drops a list from its arrow: pressing Bullets
/// puts bullets on, pressing the arrow beside it offers the shapes. A **plain
/// dropdown** has no command of its own, and pressing anywhere on it drops the
/// list — Change Case is one, because there is no such thing as "the case".
///
/// Drawing one as the other is not a detail: a person who presses the face of
/// what they take for a split button and gets a list has lost a keystroke, and
/// one who presses what they take for a dropdown and silently changes the
/// document has lost more than that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Menu {
    pub command: Command,
    pub choice: Choice,
    /// Whether the face does something of its own.
    pub split: bool,
}

/// How wide the arrow half of a button is.
const ARROW_WIDTH: f32 = 10.0;

/// How wide the two little arrows at the end of a measurement box are.
const SPINNER_WIDTH: f32 = 13.0;

/// How deep the arrow half of a large button is.
///
/// Word splits a large button across rather than down: the icon on top runs the
/// command, the label and the arrow underneath drop the list.
const ARROW_DEPTH: f32 = 22.0;

/// Every button that drops a menu, and which menu it drops.
///
/// One table rather than a variant per button, because what makes these
/// different from an ordinary button is one fact about each of them, and a
/// table of facts is a thing that can be read through and checked against Word.
static MENUS: &[Menu] = &[
    Menu { command: Command::Bullets, choice: Choice::BulletLibrary, split: true },
    Menu { command: Command::Numbering, choice: Choice::NumberLibrary, split: true },
    Menu { command: Command::MultilevelList, choice: Choice::MultilevelLibrary, split: false },
    Menu { command: Command::LineSpacing, choice: Choice::LineSpacing, split: false },
    Menu { command: Command::DocumentSpacing, choice: Choice::DocumentSpacing, split: false },
    Menu { command: Command::ChangeCase, choice: Choice::LetterCase, split: false },
    Menu { command: Command::PageNumber, choice: Choice::PageNumberPlace, split: false },
    Menu { command: Command::SelectAll, choice: Choice::Selecting, split: false },
    Menu { command: Command::NextNote, choice: Choice::NoteJump, split: true },
    Menu { command: Command::AcceptChange, choice: Choice::Accepting, split: true },
    Menu { command: Command::RejectChange, choice: Choice::Rejecting, split: true },
    Menu { command: Command::TrackChanges, choice: Choice::Tracking, split: true },
];

/// The menu a command drops, if it drops one.
#[must_use]
pub fn menu_of(command: Command) -> Option<Menu> {
    MENUS.iter().copied().find(|menu| menu.command == command)
}

/// And the other way: the button a menu drops from, which is where it hangs.
#[must_use]
pub fn command_of(choice: Choice) -> Option<Command> {
    MENUS.iter().find(|menu| menu.choice == choice).map(|menu| menu.command)
}

/// What a press on the ribbon means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// Run it.
    Run(Command),
    /// Drop the menu it carries.
    Drop(Command, Choice),
    /// Put the keyboard in a measurement box.
    Type(Command),
    /// Nudge a measurement up or down by one step.
    Step(Command, bool),
}

/// A named set of commands.
#[derive(Debug)]
pub struct Group {
    pub label: &'static str,
    pub items: &'static [Item],
    /// The small arrow in the bottom right-hand corner, which opens the dialog
    /// behind the group. Word puts one on every group that has more to offer
    /// than the buttons show; a group with nothing more has none.
    pub launcher: Option<Command>,
}

/// Where one item was placed, so a click can find it again.
#[derive(Debug)]
struct Placed {
    command: Command,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    /// Where the box of a measurement item starts, when the item has one.
    ///
    /// A measurement is a label and a box beside it, and only the box takes
    /// the keyboard: pressing the word "Left:" should do nothing, as it does
    /// nothing in Word.
    field: Option<(f32, f32)>,
}

/// The ribbon, and which tab of it is open.
#[derive(Debug)]
pub struct Ribbon {
    pub tab: Tab,
    /// Where the ribbon was last drawn, so a hit test knows where to look.
    top: f32,
    /// Where everything ended up when it was last drawn.
    placed: Vec<Placed>,
    tabs: Vec<(Tab, f32, f32)>,
    /// Where the search box on the tab strip ended up.
    search: Option<(f32, f32, f32)>,
    /// How many tiles of the style gallery the window has room for.
    style_tiles: usize,
}

impl Default for Ribbon {
    fn default() -> Self {
        Self::new()
    }
}

impl Ribbon {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tab: Tab::Home,
            top: 0.0,
            placed: Vec::new(),
            tabs: Vec::new(),
            search: None,
            style_tiles: STYLE_TILES_MOST,
        }
    }

    /// The groups of the tab currently open.
    #[must_use]
    pub fn groups(&self) -> &'static [Group] {
        match self.tab {
            Tab::File => FILE_GROUPS,
            Tab::Home => HOME_GROUPS,
            Tab::Insert => INSERT_GROUPS,
            Tab::Design => DESIGN_GROUPS,
            Tab::Layout => LAYOUT_GROUPS,
            Tab::References => REFERENCES_GROUPS,
            Tab::Mailings => MAILINGS_GROUPS,
            Tab::Review => REVIEW_GROUPS,
            Tab::View => VIEW_GROUPS,
            Tab::Help => HELP_GROUPS,
            Tab::TableDesign => TABLE_DESIGN_GROUPS,
            Tab::TableLayout => TABLE_LAYOUT_GROUPS,
            Tab::HeaderFooter => HEADER_FOOTER_GROUPS,
        }
    }

    /// The tab at a point in the tab strip, if there is one.
    #[must_use]
    pub fn tab_at(&self, x: i32, y: i32) -> Option<Tab> {
        let y = y as f32 - self.top;
        if !(0.0..TAB_HEIGHT).contains(&y) {
            return None;
        }
        self.tabs
            .iter()
            .find(|(_, left, width)| x as f32 >= *left && (x as f32) < left + width)
            .map(|(tab, _, _)| *tab)
    }

    /// Whether a point is on the "Tell me what you want to do" box.
    #[must_use]
    pub fn search_at(&self, x: i32, y: i32) -> bool {
        let Some((left, top, width)) = self.search else { return false };
        (x as f32) >= left
            && (x as f32) < left + width
            && (y as f32) >= top
            && (y as f32) < top + TAB_HEIGHT - 8.0
    }

    /// The command at a point in the ribbon, if there is one.
    #[must_use]
    pub fn command_at(&self, x: i32, y: i32) -> Option<Command> {
        let (x, y) = (x as f32, y as f32);
        self.placed
            .iter()
            .find(|item| {
                x >= item.left
                    && x < item.left + item.width
                    && y >= item.top
                    && y < item.top + item.height
            })
            .map(|item| item.command)
    }

    /// What a press at a point means: running a command, or dropping its menu.
    ///
    /// The two halves of a split button are told apart here and nowhere else,
    /// so the drawing and the pressing cannot disagree about where the line
    /// between them is.
    #[must_use]
    pub fn press_at(&self, x: i32, y: i32) -> Option<Press> {
        let command = self.command_at(x, y)?;

        // A measurement is a label and a box, and only the box answers: its
        // two little arrows nudge the number, and the rest of it takes the
        // keyboard. Pressing the word "Left:" does nothing, as in Word.
        if let Some(placed) = self.placed.iter().find(|item| item.command == command) {
            if let Some((box_left, box_width)) = placed.field {
                let (x, y) = (x as f32, y as f32);
                if x < box_left {
                    return None;
                }
                if x >= box_left + box_width - SPINNER_WIDTH {
                    let up = y < placed.top + placed.height / 2.0;
                    return Some(Press::Step(command, up));
                }
                return Some(Press::Type(command));
            }
        }

        let Some(menu) = menu_of(command) else { return Some(Press::Run(command)) };
        if !menu.split {
            return Some(Press::Drop(command, menu.choice));
        }

        let item = self
            .placed
            .iter()
            .find(|item| item.command == command)
            .map(|item| (item.left, item.top, item.width, item.height))?;
        let (left, top, width, height) = item;
        let dropping = if item_is_large(height) {
            (y as f32) >= top + height - ARROW_DEPTH
        } else {
            (x as f32) >= left + width - ARROW_WIDTH
        };
        if dropping {
            Some(Press::Drop(command, menu.choice))
        } else {
            Some(Press::Run(command))
        }
    }

    /// Every tab that is showing, and where it sits: its left edge and width.
    ///
    /// For the letters that appear over them when Alt is pressed, which have to
    /// land on the tabs wherever the tabs ended up.
    #[must_use]
    pub fn tab_places(&self) -> Vec<(Tab, f32, f32)> {
        self.tabs.clone()
    }

    /// Every command drawn in the open tab, and where it sits: its left edge,
    /// its top, and how wide and tall it is.
    #[must_use]
    pub fn command_places(&self) -> Vec<(Command, f32, f32, f32, f32)> {
        self.placed
            .iter()
            .map(|item| (item.command, item.left, item.top, item.width, item.height))
            .collect()
    }

    /// The top of the tab strip, which is where a badge over a tab goes.
    #[must_use]
    pub fn strip_top(&self) -> f32 {
        self.top
    }

    /// Where a command's button sits, for hanging a list under it.
    #[must_use]
    pub fn command_rect(&self, command: Command) -> Option<(f32, f32, f32)> {
        self.placed
            .iter()
            .find(|item| item.command == command)
            .map(|item| (item.left, item.top + item.height, item.width))
    }

    /// Draws the tab strip and the open tab's groups.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        top: f32,
        state: &ToolbarState,
        hovered: Option<Command>,
        theme: &Theme,
    ) {
        self.placed.clear();
        self.tabs.clear();
        self.search = None;
        self.top = top;

        let width = canvas.width() as i32;
        canvas.fill_rect(0, top as i32, width, TAB_HEIGHT as i32, theme.tab_strip);
        canvas.fill_rect(0, (top + TAB_HEIGHT) as i32, width, RIBBON_HEIGHT as i32, theme.ribbon);
        canvas.fill_rect(0, (top + TOTAL_HEIGHT - 1.0) as i32, width, 1, theme.ribbon_edge);

        // A contextual tab that no longer applies cannot stay open.
        if !self.tab.applies(state) {
            self.tab = Tab::Home;
        }

        self.draw_tabs(canvas, engine, renderer, state, theme);
        self.draw_groups(canvas, engine, renderer, state, hovered, theme);
    }

    fn draw_tabs(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        state: &ToolbarState,
        theme: &Theme,
    ) {
        let top = self.top;
        let mut x = 8.0f32;

        // The contextual tabs come after the fixed ones and only while they
        // apply, which is what Word does with Table Design and Table Layout.
        let showing: Vec<Tab> = Tab::ALL
            .iter()
            .copied()
            .chain(Tab::CONTEXTUAL.iter().copied().filter(|tab| tab.applies(state)))
            .collect();

        for tab in &showing {
            let chosen = *tab == self.tab;
            // File is not a tab like the others and Word does not draw it like
            // one: it is a filled button in the accent colour, because it does
            // not open a page under the strip but a window over everything.
            let file = *tab == Tab::File;
            // An open tab is a notch cut out of the strip into the ribbon
            // below, so its name is written in the ribbon's text colour. The
            // rest sit on the strip, which is dark in both themes.
            let color = if file {
                theme.bar_text()
            } else if chosen {
                theme.text
            } else {
                theme.bar_dim_text()
            };
            let measured = engine.simple_line(tab.label(), 0.0, 0.0, 9.0, color);
            let width = measured.width + 24.0;

            if file {
                canvas.fill_rect(
                    x as i32,
                    top as i32,
                    width as i32,
                    TAB_HEIGHT as i32,
                    theme.backstage(),
                );
            } else if chosen {
                canvas.fill_rect(
                    x as i32,
                    top as i32,
                    width as i32,
                    TAB_HEIGHT as i32,
                    theme.ribbon,
                );
            }

            let line = engine.simple_line(tab.label(), x + 12.0, top + 21.0, 9.0, color);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);

            self.tabs.push((*tab, x, width));
            x += width;
        }

        // "Tell me what you want to do", which is where a person goes when they
        // cannot find a command — so it is the one thing on the strip that is
        // not a tab.
        let hint = "Tell me what you want to do";
        let dim = theme.bar_dim_text();
        icons::draw_sized(canvas, Icon::Help, x + 12.0, top + 7.0, 18.0, dim);
        let line = engine.simple_line(hint, x + 38.0, top + 21.0, 8.5, dim);
        let measured = line.width - (x + 38.0);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        self.search = Some((x + 8.0, top + 4.0, measured + 38.0));
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_groups(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        state: &ToolbarState,
        hovered: Option<Command>,
        theme: &Theme,
    ) {
        let top = self.top + TAB_HEIGHT + 3.0;
        let body = RIBBON_HEIGHT - GROUP_LABEL_HEIGHT - 6.0;
        let mut x = EDGE;
        let right_edge = canvas.width() as f32;
        // How many rows of small buttons the ribbon is tall enough for.
        let rows = ((body / ROW_HEIGHT).floor() as usize).max(1);

        // The style gallery gives up tiles before any group is given up
        // altogether, because it is the widest thing on the tab and the only
        // one that can be made smaller without losing a command.
        self.style_tiles = self.tiles_that_fit(engine, rows, right_edge - EDGE);

        // A window too narrow for every group shows the last of them as one
        // button each, which opens what is inside it. Word does the same, and
        // the alternative is a group drawn half off the edge of the window.
        let collapsed = self.collapsed_groups(engine, rows, right_edge - EDGE);

        for (index, group) in self.groups().iter().enumerate() {
            if index >= collapsed {
                x = self.draw_collapsed_group(
                    canvas, engine, renderer, group, index, x, top, hovered, theme,
                );
                continue;
            }
            let start = x + GROUP_PADDING;
            let mut cursor = start;
            // A tall button occupies every row, so the rows after it begin
            // where it ends rather than back at the edge of the group.
            let mut row_start = start;
            let mut row = 0usize;
            let mut widest = 0.0f32;

            for item in group.items {
                if matches!(item, Item::Break | Item::NewColumn) {
                    widest = widest.max(cursor - start);
                    row += 1;
                    // A group with more rows than the ribbon is tall carries on
                    // in a second column, which is what Word does — the
                    // alternative is a button drawn over the group's name.
                    if row >= rows || matches!(item, Item::NewColumn) {
                        row = 0;
                        row_start = start + widest + COLUMN_GAP;
                    }
                    cursor = row_start;
                    continue;
                }

                let tall = matches!(item, Item::Large(..) | Item::StyleGallery);
                let height = if tall { body } else { ROW_HEIGHT };
                let item_top = if tall { top } else { top + row as f32 * ROW_HEIGHT };
                // A group that would run off the edge is simply not drawn: a
                // button half over the edge of the window is worse than none.
                if cursor > right_edge {
                    break;
                }
                let width = self.draw_item(
                    canvas, engine, renderer, item, cursor, item_top, height, state, hovered, theme,
                );
                cursor += width;
                if tall {
                    row_start = cursor;
                }
            }
            widest = widest.max(cursor - start);

            // The small arrow in the corner, where a group has more behind it
            // than its buttons show. Word's is a corner mark with an arrow
            // through it, and pressing it opens the group's dialog.
            if let Some(command) = group.launcher {
                let size = 9.0;
                let arrow_x = start + widest - size - 2.0;
                let arrow_y = self.top + TAB_HEIGHT + RIBBON_HEIGHT - GROUP_LABEL_HEIGHT + 1.0;
                launcher_mark(canvas, arrow_x, arrow_y, size, theme.dim_text);
                self.placed.push(Placed {
                    command,
                    left: arrow_x - 3.0,
                    top: arrow_y - 3.0,
                    width: size + 6.0,
                    height: size + 6.0,
                    field: None,
                });
            }

            // The name of the group, centred under it: the thing that makes a
            // ribbon findable rather than a wall of icons.
            let measured = engine.simple_line(group.label, 0.0, 0.0, 7.5, theme.dim_text);
            let label_x = start + (widest - measured.width) / 2.0;
            let line = engine.simple_line(
                group.label,
                label_x.max(start),
                self.top + TAB_HEIGHT + RIBBON_HEIGHT - 5.0,
                7.5,
                theme.dim_text,
            );
            renderer.draw_onto(canvas, &line, 0.0, 0.0);

            x = start + widest + GROUP_PADDING;
            if x < right_edge {
                canvas.fill_rect(
                    x as i32,
                    (top + 2.0) as i32,
                    1,
                    (RIBBON_HEIGHT - 12.0) as i32,
                    theme.group_separator,
                );
            }
            x += 1.0;
        }
    }

    /// How wide an item is drawn, without drawing it.
    ///
    /// Measured rather than guessed because a button is as wide as its label,
    /// and the label is set in a font that only the layout engine can measure.
    fn item_width(&self, engine: &mut LayoutEngine<'_>, item: &Item) -> f32 {
        let colour = Color::BLACK;
        // A button that drops a menu needs room for the arrow beside it —
        // except a large one, whose arrow goes under the label where there is
        // room already.
        let arrow = match item {
            Item::Small(command, ..) | Item::Button(command, _) if menu_of(*command).is_some() => {
                ARROW_WIDTH
            }
            _ => 0.0,
        };
        arrow
            + match item {
                Item::Large(_, _, label) => {
                    engine.simple_line(label, 0.0, 0.0, 8.0, colour).width.max(icons::LARGE_SIZE)
                        + 14.0
                }
                Item::Small(_, _, label) => {
                    engine.simple_line(label, 0.0, 0.0, 8.0, colour).width + icons::SIZE + 14.0
                }
                Item::Button(..) | Item::Letter(..) => ROW_HEIGHT,
                Item::Measure(_, label, width) => {
                    engine.simple_line(label, 0.0, 0.0, 8.0, colour).width + width + 14.0
                }
                Item::Field(_, _, width) => *width,
                Item::StyleGallery => STYLE_TILE_WIDTH * self.style_tiles as f32,
                Item::Break | Item::NewColumn => 0.0,
            }
    }

    /// How wide a whole group is, laid out the way it will be drawn.
    fn group_width(&self, engine: &mut LayoutEngine<'_>, group: &Group, rows: usize) -> f32 {
        let mut cursor = 0.0f32;
        let mut row_start = 0.0f32;
        let mut row = 0usize;
        let mut widest = 0.0f32;

        for item in group.items {
            if matches!(item, Item::Break | Item::NewColumn) {
                widest = widest.max(cursor);
                row += 1;
                if row >= rows || matches!(item, Item::NewColumn) {
                    row = 0;
                    row_start = widest + COLUMN_GAP;
                }
                cursor = row_start;
                continue;
            }
            cursor += self.item_width(engine, item);
            if matches!(item, Item::Large(..) | Item::StyleGallery) {
                row_start = cursor;
            }
        }
        widest.max(cursor)
    }

    /// How many tiles of the style gallery there is room for.
    ///
    /// The most that leaves every group of the tab drawn in full, and the
    /// fewest worth showing when even that is not enough — at which point the
    /// groups on the right are given up as they always were.
    fn tiles_that_fit(&mut self, engine: &mut LayoutEngine<'_>, rows: usize, room: f32) -> usize {
        let showing = self.groups().len();
        for tiles in (STYLE_TILES_LEAST..=STYLE_TILES_MOST).rev() {
            self.style_tiles = tiles;
            if self.collapsed_groups(engine, rows, room) >= showing {
                return tiles;
            }
        }
        STYLE_TILES_LEAST
    }

    /// How many groups are drawn in full, the rest being collapsed to one
    /// button each.
    ///
    /// Groups are given up from the right, because the ones on the left are the
    /// ones a person reaches for — and because that is the order Word gives
    /// them up in.
    fn collapsed_groups(&self, engine: &mut LayoutEngine<'_>, rows: usize, room: f32) -> usize {
        let groups = self.groups();
        let widths: Vec<f32> = groups
            .iter()
            .map(|group| self.group_width(engine, group, rows) + GROUP_PADDING * 2.0)
            .collect();
        let collapsed_width = COLLAPSED_WIDTH + GROUP_PADDING;

        for open in (0..=groups.len()).rev() {
            let full: f32 = widths[..open].iter().sum();
            let shut = (groups.len() - open) as f32 * collapsed_width;
            if full + shut <= room {
                return open;
            }
        }
        0
    }

    /// Draws a group given up to one button, and says where the next begins.
    #[allow(clippy::too_many_arguments)]
    fn draw_collapsed_group(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        group: &Group,
        index: usize,
        left: f32,
        top: f32,
        hovered: Option<Command>,
        theme: &Theme,
    ) -> f32 {
        let command = Command::ExpandGroup(index as u8);
        let x = left + GROUP_PADDING;
        let height = RIBBON_HEIGHT - GROUP_LABEL_HEIGHT - 6.0;

        if hovered == Some(command) {
            canvas.fill_rect(
                x as i32,
                top as i32,
                COLLAPSED_WIDTH as i32,
                height as i32,
                theme.hover,
            );
        }

        // The icon of whatever the group is mostly about, which is its first
        // button — the same picture Word puts on a collapsed group. A group
        // with no icon of its own has its name down the middle instead.
        let icon = group.items.iter().find_map(|item| match item {
            Item::Large(_, icon, _) | Item::Small(_, icon, _) | Item::Button(_, icon) => {
                Some(*icon)
            }
            _ => None,
        });
        if let Some(icon) = icon {
            icons::draw_sized(
                canvas,
                icon,
                x + (COLLAPSED_WIDTH - icons::LARGE_SIZE) / 2.0,
                top + 6.0,
                icons::LARGE_SIZE,
                theme.text,
            );
        }

        let measured = engine.simple_line(group.label, 0.0, 0.0, 7.5, theme.text);
        let baseline = if icon.is_some() { top + height - 14.0 } else { top + height / 2.0 };
        let line = engine.simple_line(
            group.label,
            x + (COLLAPSED_WIDTH - measured.width.min(COLLAPSED_WIDTH - 4.0)) / 2.0,
            baseline,
            7.5,
            theme.text,
        );
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        chevron(canvas, x + COLLAPSED_WIDTH / 2.0, top + height - 6.0, theme.dim_text);

        self.placed.push(Placed {
            command,
            left: x,
            top,
            width: COLLAPSED_WIDTH,
            height,
            field: None,
        });
        x + COLLAPSED_WIDTH + GROUP_PADDING
    }

    /// Draws one item and returns how wide it turned out.
    #[allow(clippy::too_many_arguments)]
    fn draw_item(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        item: &Item,
        left: f32,
        top: f32,
        height: f32,
        state: &ToolbarState,
        hovered: Option<Command>,
        theme: &Theme,
    ) -> f32 {
        if let Item::StyleGallery = item {
            return self.draw_style_gallery(
                canvas, engine, renderer, left, top, height, state, hovered, theme,
            );
        }

        let command = match item {
            Item::Large(command, ..)
            | Item::Small(command, ..)
            | Item::Button(command, _)
            | Item::Letter(command, ..)
            | Item::Measure(command, ..)
            | Item::Field(command, ..) => *command,
            Item::Break | Item::NewColumn | Item::StyleGallery => return 0.0,
        };

        let mut field: Option<(f32, f32)> = None;
        let enabled = super::is_enabled(command, state);
        let active = enabled && super::is_active(command, state);
        let color = if enabled { theme.text } else { theme.disabled_text };

        let width = self.item_width(engine, item);

        let background = if active {
            Some(theme.accent)
        } else if enabled && hovered == Some(command) {
            Some(theme.hover)
        } else {
            None
        };
        if let Some(fill) = background {
            canvas.fill_rect(left as i32, top as i32, width as i32, height as i32, fill);
        }
        let color = if active { theme.on_accent() } else { color };

        // The arrow that says a menu drops from here, and the line that says
        // which part of the button drops it.
        if let Some(menu) = menu_of(command) {
            let large = matches!(item, Item::Large(..));
            let (arrow_x, arrow_y) = if large {
                (left + width / 2.0, top + height - 4.0)
            } else {
                (left + width - ARROW_WIDTH / 2.0, top + height / 2.0)
            };
            // Only a split button is divided: a plain dropdown is one button
            // that happens to have an arrow on it, and a line down the middle
            // of it would promise a face command it does not have.
            if menu.split && enabled {
                let edge = if active { theme.on_accent() } else { theme.group_separator };
                if large {
                    canvas.fill_rect(
                        (left + 4.0) as i32,
                        (top + height - ARROW_DEPTH) as i32,
                        (width - 8.0) as i32,
                        1,
                        edge,
                    );
                } else {
                    canvas.fill_rect(
                        (left + width - ARROW_WIDTH) as i32,
                        (top + 4.0) as i32,
                        1,
                        (height - 8.0) as i32,
                        edge,
                    );
                }
            }
            chevron(canvas, arrow_x - 3.5, arrow_y, color);
        }

        match item {
            Item::Large(_, icon, label) => {
                // The icon over its name, which is how a ribbon marks the one
                // command a group is really about.
                icons::draw_sized(
                    canvas,
                    *icon,
                    left + (width - icons::LARGE_SIZE) / 2.0,
                    top + 6.0,
                    icons::LARGE_SIZE,
                    color,
                );
                // A button with a menu keeps its label clear of the arrow
                // underneath it, so a descender and the arrow are not drawn on
                // top of one another.
                let baseline = top + height - if menu_of(command).is_some() { 12.0 } else { 7.0 };
                let measured = engine.simple_line(label, 0.0, 0.0, 8.0, color);
                let line = engine.simple_line(
                    label,
                    left + (width - measured.width) / 2.0,
                    baseline,
                    8.0,
                    color,
                );
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
            }
            Item::Small(_, icon, label) => {
                let icon_top = top + (ROW_HEIGHT - icons::SIZE) / 2.0;
                icons::draw(canvas, *icon, left + 4.0, icon_top, color);
                let line =
                    engine.simple_line(label, left + icons::SIZE + 9.0, top + 17.0, 8.0, color);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
            }
            Item::Button(_, icon) => {
                let inset = (ROW_HEIGHT - icons::SIZE) / 2.0;
                icons::draw(canvas, *icon, left + inset, top + inset, color);
                match *icon {
                    Icon::TextColor => {
                        icons::draw_color_band(
                            canvas,
                            left + inset,
                            top + inset,
                            icons::SIZE,
                            state.text_color,
                        );
                    }
                    Icon::Highlight => {
                        icons::draw_color_band(
                            canvas,
                            left + inset,
                            top + inset,
                            icons::SIZE,
                            state.highlight_color,
                        );
                    }
                    _ => {}
                }
            }
            Item::Letter(_, text, style) => {
                let measured = engine.styled_line(text, 0.0, 0.0, 10.0, color, *style);
                let line = engine.styled_line(
                    text,
                    left + (width - measured.width) / 2.0,
                    top + 18.0,
                    10.0,
                    color,
                    *style,
                );
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
            }
            Item::Measure(_, label, box_width) => {
                let line = engine.simple_line(label, left, top + 17.0, 8.0, theme.dim_text);
                let measured = line.width - left;
                renderer.draw_onto(canvas, &line, 0.0, 0.0);

                let box_left = left + measured + 8.0;
                field = Some((box_left, *box_width));
                field_box(canvas, box_left, top, *box_width, theme);

                // A box with the keyboard is outlined in the accent colour, as
                // every box that has it is.
                let typing = state.typing.as_ref().is_some_and(|(found, _)| *found == command);
                if typing {
                    outline(canvas, box_left, top, *box_width, ROW_HEIGHT, theme.accent);
                }

                let value = state.measure(command);
                let line = engine.simple_line(&value, box_left + 5.0, top + 17.0, 8.0, color);
                let text_width = line.width - (box_left + 5.0);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);

                // The caret sits after what has been typed, so that a box being
                // typed into looks like one.
                if typing {
                    canvas.fill_rect(
                        (box_left + 6.0 + text_width) as i32,
                        (top + 5.0) as i32,
                        1,
                        (ROW_HEIGHT - 10.0) as i32,
                        theme.text,
                    );
                }
                spinner(canvas, box_left + box_width - SPINNER_WIDTH, top, color);
            }
            Item::Field(_, choice, _) => {
                field_box(canvas, left, top, width, theme);
                let text = match choice {
                    Choice::Font => state.font.clone().unwrap_or_else(|| "(default)".to_owned()),
                    Choice::Size => super::format_size(state.size),
                    Choice::Style => state.style.clone().unwrap_or_else(|| "Normal".to_owned()),
                    Choice::Zoom => format!("{}%", state.zoom.round() as i32),
                    Choice::Border
                    | Choice::Furniture
                    | Choice::Reference
                    | Choice::Citation
                    | Choice::Source
                    | Choice::LineNumbers
                    | Choice::Hyphenation
                    | Choice::Protection
                    | Choice::StatusBar
                    | Choice::PageNumbering
                    | Choice::Printer
                    | Choice::PrintWhich
                    | Choice::PrintSides
                    | Choice::PrintPerSheet
                    | Choice::Margin
                    | Choice::Orientation
                    | Choice::Paper
                    | Choice::Column
                    | Choice::Break
                    | Choice::Watermark
                    | Choice::PasteOption
                    | Choice::TableStyle
                    | Choice::DocumentSpacing
                    | Choice::BulletLibrary
                    | Choice::NumberLibrary
                    | Choice::MultilevelLibrary
                    | Choice::LineSpacing
                    | Choice::LetterCase
                    | Choice::PageNumberPlace
                    | Choice::Selecting
                    | Choice::NoteJump
                    | Choice::Accepting
                    | Choice::Rejecting
                    | Choice::Tracking
                    | Choice::Cover
                    | Choice::Authority
                    | Choice::Theme
                    | Choice::ThemeColors
                    | Choice::ThemeFonts
                    | Choice::Language
                    | Choice::Shape
                    | Choice::Wrap
                    | Choice::Position
                    | Choice::QuickPart
                    | Choice::WordArt
                    | Choice::Drawing
                    | Choice::MergeKind
                    | Choice::Recipient
                    | Choice::MergeField
                    | Choice::Correction
                    | Choice::Accessibility
                    | Choice::Context
                    | Choice::Group
                    | Choice::ThemeEffects
                    | Choice::Chart
                    | Choice::Rule
                    | Choice::Macro
                    | Choice::Diagram
                    | Choice::Screenshot
                    | Choice::OutlineLevel
                    | Choice::MatchField
                    | Choice::MatchColumn
                    | Choice::TextEffect
                    | Choice::Envelope
                    | Choice::Label => String::new(),
                };
                let line = engine.simple_line(&text, left + 6.0, top + 17.0, 8.5, color);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
                chevron(canvas, left + width - 12.0, top + ROW_HEIGHT / 2.0, color);
            }
            Item::Break | Item::NewColumn | Item::StyleGallery => {}
        }

        self.placed.push(Placed { command, left, top, width, height, field });
        width
    }

    /// The gallery of paragraph styles, each tile set in its own formatting.
    ///
    /// This is what makes a style gallery useful rather than a list of names:
    /// the tile *is* the answer to "what will this look like".
    #[allow(clippy::too_many_arguments)]
    fn draw_style_gallery(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        top: f32,
        height: f32,
        state: &ToolbarState,
        hovered: Option<Command>,
        theme: &Theme,
    ) -> f32 {
        let mut x = left;
        for (index, sample) in state.styles.iter().take(self.style_tiles).enumerate() {
            let command = Command::Style(index);
            let chosen = sample.id == state.style;

            let background = if chosen {
                theme.accent
            } else if hovered == Some(command) {
                theme.hover
            } else {
                theme.field
            };
            canvas.fill_rect(
                x as i32,
                top as i32,
                STYLE_TILE_WIDTH as i32 - 3,
                height as i32,
                background,
            );
            outline(canvas, x, top, STYLE_TILE_WIDTH - 3.0, height, theme.field_edge);

            // The specimen, in the style's own formatting, shrunk to fit.
            let ink = if chosen { theme.on_accent() } else { theme.text };
            let style = TextStyle {
                bold: sample.bold,
                italic: sample.italic,
                underline: false,
                strike: false,
            };

            // The specimen is set at the style's own size, brought down until it
            // fits the tile — which is what makes a heading tile look like a
            // heading and a body tile look like body text.
            let room = STYLE_TILE_WIDTH - 11.0;
            let mut size = (sample.size * 0.62).clamp(6.5, 14.0);
            let mut measured = engine.styled_line(SPECIMEN, 0.0, 0.0, size, ink, style).width;
            while measured > room && size > 5.5 {
                size -= 0.5;
                measured = engine.styled_line(SPECIMEN, 0.0, 0.0, size, ink, style).width;
            }

            let specimen_x = x + (STYLE_TILE_WIDTH - 3.0 - measured) / 2.0;
            let line = engine.styled_line(
                SPECIMEN,
                specimen_x.max(x + 2.0),
                top + height * 0.5,
                size,
                ink,
                style,
            );
            renderer.draw_onto(canvas, &line, 0.0, 0.0);

            // The name under it, cut down to the tile.
            let name = trim_to_width(engine, &sample.name, STYLE_TILE_WIDTH - 10.0, ink);
            let measured = engine.simple_line(&name, 0.0, 0.0, 7.5, ink);
            let name_x = x + (STYLE_TILE_WIDTH - 3.0 - measured.width) / 2.0;
            let line = engine.simple_line(&name, name_x.max(x + 2.0), top + height - 6.0, 7.5, ink);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);

            self.placed.push(Placed {
                command,
                left: x,
                top,
                width: STYLE_TILE_WIDTH - 3.0,
                field: None,
                height,
            });
            x += STYLE_TILE_WIDTH;
        }
        x - left
    }
}

/// The sunken box a field or a measurement sits in.
fn field_box(canvas: &mut Canvas, left: f32, top: f32, width: f32, theme: &Theme) {
    canvas.fill_rect(
        left as i32,
        (top + 3.0) as i32,
        width as i32,
        (ROW_HEIGHT - 6.0) as i32,
        theme.field,
    );
    outline(canvas, left, top + 3.0, width, ROW_HEIGHT - 6.0, theme.field_edge);
}

fn outline(canvas: &mut Canvas, x: f32, y: f32, width: f32, height: f32, colour: Color) {
    let (x, y, width, height) = (x as i32, y as i32, width as i32, height as i32);
    canvas.fill_rect(x, y, width, 1, colour);
    canvas.fill_rect(x, y + height - 1, width, 1, colour);
    canvas.fill_rect(x, y, 1, height, colour);
    canvas.fill_rect(x + width - 1, y, 1, height, colour);
}

/// Whether an item's height means it is a large button.
///
/// A large button fills the group; every other kind is one row of it. Told from
/// the height rather than carried about, because the height is what the person
/// is pointing at, and it is what decides whether the button is split across or
/// down.
fn item_is_large(height: f32) -> bool {
    height > ROW_HEIGHT + 1.0
}

/// The two little arrows at the right of a measurement box.
///
/// Word puts them on every box that holds a number, and they are how a
/// measurement is nudged rather than typed — which is most of the time, because
/// the answer is usually "a bit more than that".
fn spinner(canvas: &mut Canvas, left: f32, top: f32, colour: Color) {
    let middle = top + ROW_HEIGHT / 2.0;
    let centre = left + SPINNER_WIDTH / 2.0;
    for step in 0..3 {
        let step = step as f32;
        // Up in the top half, down in the bottom half.
        canvas.fill_rect((centre - step) as i32, (middle - 4.0 + step) as i32, 1, 1, colour);
        canvas.fill_rect((centre + step) as i32, (middle - 4.0 + step) as i32, 1, 1, colour);
        canvas.fill_rect((centre - step) as i32, (middle + 4.0 - step) as i32, 1, 1, colour);
        canvas.fill_rect((centre + step) as i32, (middle + 4.0 - step) as i32, 1, 1, colour);
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

/// Cuts a label down to what fits, ending it with an ellipsis.
fn trim_to_width(engine: &mut LayoutEngine<'_>, text: &str, width: f32, colour: Color) -> String {
    if engine.simple_line(text, 0.0, 0.0, 7.5, colour).width <= width {
        return text.to_owned();
    }
    let mut kept = String::new();
    for character in text.chars() {
        let mut candidate = kept.clone();
        candidate.push(character);
        candidate.push('…');
        if engine.simple_line(&candidate, 0.0, 0.0, 7.5, colour).width > width {
            break;
        }
        kept.push(character);
    }
    kept.push('…');
    kept
}

// --- What each tab holds ------------------------------------------------------

const BOLD: TextStyle = TextStyle { bold: true, italic: false, underline: false, strike: false };
const ITALIC: TextStyle = TextStyle { bold: false, italic: true, underline: false, strike: false };
const UNDERLINE: TextStyle =
    TextStyle { bold: false, italic: false, underline: true, strike: false };
const STRIKE: TextStyle = TextStyle { bold: false, italic: false, underline: false, strike: true };

/// The File tab has no groups, because it has no ribbon page.
///
/// Word's File tab opens the backstage over the whole window rather than
/// dropping a page of buttons under the strip, and so does this one — see
/// [`super::backstage`]. An empty page rather than no arm at all, because the
/// tab is still on the strip and the ribbon may still be asked what is on it.
static FILE_GROUPS: &[Group] = &[];

static HOME_GROUPS: &[Group] = &[
    Group {
        label: "Clipboard",
        items: &[
            Item::Large(Command::Paste, Icon::Clipboard, "Paste"),
            Item::Small(Command::Cut, Icon::Scissors, "Cut"),
            Item::Break,
            Item::Small(Command::Copy, Icon::Copy, "Copy"),
            Item::Break,
            Item::Small(Command::FormatPainter, Icon::Brush, "Format Painter"),
        ],
        launcher: None,
    },
    Group {
        label: "Font",
        items: &[
            Item::Field(Command::ChooseFont, Choice::Font, 128.0),
            Item::Field(Command::ChooseSize, Choice::Size, 52.0),
            Item::Button(Command::GrowFont, Icon::LetterUp),
            Item::Button(Command::ShrinkFont, Icon::LetterDown),
            Item::Button(Command::ChangeCase, Icon::ChangeCase),
            Item::Button(Command::ClearFormatting, Icon::LetterClear),
            Item::Break,
            Item::Letter(Command::Format(CharacterFormat::Bold), "B", BOLD),
            Item::Letter(Command::Format(CharacterFormat::Italic), "I", ITALIC),
            Item::Letter(Command::Format(CharacterFormat::Underline), "U", UNDERLINE),
            Item::Letter(Command::Format(CharacterFormat::Strikethrough), "S", STRIKE),
            Item::Button(Command::Subscript, Icon::Subscript),
            Item::Button(Command::Superscript, Icon::Superscript),
            Item::Button(Command::TextEffects, Icon::TextEffects),
            Item::Button(Command::Highlight, Icon::Highlight),
            Item::Button(Command::TextColor, Icon::TextColor),
        ],
        launcher: Some(Command::FontDialog),
    },
    Group {
        label: "Paragraph",
        items: &[
            Item::Button(Command::Bullets, Icon::Bullets),
            Item::Button(Command::Numbering, Icon::Numbering),
            Item::Button(Command::MultilevelList, Icon::MultilevelList),
            Item::Button(Command::IndentLess, Icon::IndentLess),
            Item::Button(Command::IndentMore, Icon::IndentMore),
            Item::Button(Command::Sort, Icon::Sort),
            Item::Button(Command::ShowMarks, Icon::Pilcrow),
            Item::Break,
            Item::Button(Command::Align(Alignment::Start), Icon::AlignStart),
            Item::Button(Command::Align(Alignment::Center), Icon::AlignCenter),
            Item::Button(Command::Align(Alignment::End), Icon::AlignEnd),
            Item::Button(Command::Align(Alignment::Both), Icon::AlignBoth),
            Item::Button(Command::LineSpacing, Icon::LineSpacing),
            Item::Button(Command::Shading, Icon::Shading),
            Item::Button(Command::Borders, Icon::Borders),
        ],
        launcher: Some(Command::ParagraphDialog),
    },
    Group { label: "Styles", items: &[Item::StyleGallery], launcher: Some(Command::StylesPane) },
    Group {
        label: "Editing",
        items: &[
            Item::Small(Command::Find, Icon::Find, "Find"),
            Item::Break,
            Item::Small(Command::Replace, Icon::Replace, "Replace"),
            Item::Break,
            Item::Small(Command::SelectAll, Icon::Select, "Select"),
        ],
        launcher: None,
    },
];

static INSERT_GROUPS: &[Group] = &[
    Group {
        label: "Pages",
        items: &[
            Item::Small(Command::CoverPage, Icon::CoverPage, "Cover Page"),
            Item::Break,
            Item::Small(Command::BlankPage, Icon::BlankPage, "Blank Page"),
            Item::Break,
            Item::Small(Command::PageBreak, Icon::PageBreak, "Page Break"),
        ],
        launcher: None,
    },
    Group {
        label: "Tables",
        items: &[Item::Large(Command::InsertTable, Icon::Table, "Table")],
        launcher: None,
    },
    Group {
        label: "Illustrations",
        items: &[
            Item::Large(Command::InsertPicture, Icon::Picture, "Pictures"),
            Item::Large(Command::InsertShape, Icon::Shapes, "Shapes"),
            Item::Large(Command::SmartArt, Icon::SmartArt, "SmartArt"),
            Item::Large(Command::Chart, Icon::Chart, "Chart"),
            Item::Large(Command::Screenshot, Icon::Screenshot, "Screenshot"),
        ],
        launcher: None,
    },
    Group {
        label: "Media",
        items: &[Item::Large(Command::OnlineVideo, Icon::Video, "Video")],
        launcher: None,
    },
    Group {
        label: "Links",
        items: &[
            Item::Small(Command::InsertLink, Icon::Link, "Link"),
            Item::Break,
            Item::Small(Command::RemoveLink, Icon::Close, "Remove Link"),
            Item::Break,
            Item::Small(Command::AddBookmark, Icon::Bookmark, "Bookmark"),
            Item::Break,
            Item::Small(Command::CrossReference, Icon::CrossReference, "Cross-reference"),
        ],
        launcher: None,
    },
    Group {
        label: "Comments",
        items: &[Item::Large(Command::NewComment, Icon::Comment, "Comment")],
        launcher: None,
    },
    Group {
        label: "Header & Footer",
        items: &[
            Item::Large(Command::Header, Icon::Header, "Header"),
            Item::Large(Command::Footer, Icon::Footer, "Footer"),
            Item::Large(Command::PageNumber, Icon::PageNumber, "Page Number"),
            Item::Small(Command::FormatPageNumbers, Icon::Numbering, "Format Page Numbers"),
        ],
        launcher: None,
    },
    Group {
        label: "Text",
        items: &[
            Item::Small(Command::InsertTextBox, Icon::TextBox, "Text Box"),
            Item::Small(Command::SignatureLine, Icon::Signature, "Signature Line"),
            Item::Break,
            Item::Small(Command::QuickParts, Icon::QuickParts, "Quick Parts"),
            Item::Small(Command::InsertDate, Icon::DateTime, "Date & Time"),
            Item::Break,
            Item::Small(Command::WordArt, Icon::WordArt, "WordArt"),
            Item::Small(Command::TextFromFile, Icon::Object, "Text from File"),
        ],
        launcher: None,
    },
    Group {
        label: "Symbols",
        items: &[
            Item::Large(Command::Equation, Icon::Equation, "Equation"),
            Item::Large(Command::InsertSymbol, Icon::Symbol, "Symbol"),
        ],
        launcher: None,
    },
];

static DESIGN_GROUPS: &[Group] = &[
    Group {
        label: "Document Formatting",
        items: &[
            Item::Large(Command::Themes, Icon::Themes, "Themes"),
            Item::Large(Command::ThemeColors, Icon::Colors, "Colors"),
            Item::Large(Command::ThemeFonts, Icon::Fonts, "Fonts"),
            Item::Small(Command::DocumentSpacing, Icon::ParagraphSpacing, "Paragraph Spacing"),
            Item::Break,
            Item::Small(Command::Effects, Icon::Effects, "Effects"),
            Item::Break,
            Item::Small(Command::SetAsDefault, Icon::SetAsDefault, "Set as Default"),
        ],
        launcher: None,
    },
    Group {
        label: "Page Background",
        items: &[
            Item::Large(Command::Watermark, Icon::Watermark, "Watermark"),
            Item::Large(Command::PageColor, Icon::PageColor, "Page Color"),
            Item::Large(Command::PageBorders, Icon::PageBorders, "Page Borders"),
        ],
        launcher: None,
    },
];

static LAYOUT_GROUPS: &[Group] = &[
    Group {
        label: "Page Setup",
        items: &[
            Item::Large(Command::Margins, Icon::Margins, "Margins"),
            Item::Large(Command::Orientation, Icon::Orientation, "Orientation"),
            Item::Large(Command::PageSize, Icon::PageSize, "Size"),
            Item::Large(Command::Columns, Icon::Columns, "Columns"),
            Item::Small(Command::Breaks, Icon::Breaks, "Breaks"),
            Item::Break,
            Item::Small(Command::LineNumbers, Icon::LineNumbers, "Line Numbers"),
            Item::Break,
            Item::Small(Command::Hyphenation, Icon::Hyphenation, "Hyphenation"),
        ],
        launcher: None,
    },
    Group {
        label: "Paragraph",
        items: &[
            Item::Measure(Command::IndentLeftBox, "Left:", 62.0),
            Item::Break,
            Item::Measure(Command::IndentRightBox, "Right:", 62.0),
            Item::NewColumn,
            // Word's Paragraph group is two columns: the indents and, beside
            // them, the room above and below.
            Item::Measure(Command::SpaceBeforeBox, "Before:", 62.0),
            Item::Break,
            Item::Measure(Command::SpaceAfterBox, "After:", 62.0),
        ],
        launcher: Some(Command::ParagraphDialog),
    },
    Group {
        label: "Arrange",
        items: &[
            Item::Large(Command::Position, Icon::Position, "Position"),
            Item::Large(Command::WrapText, Icon::WrapText, "Wrap Text"),
            Item::Small(Command::BringForward, Icon::BringForward, "Bring Forward"),
            Item::Break,
            Item::Small(Command::SendBackward, Icon::SendBackward, "Send Backward"),
            Item::Break,
            Item::Small(Command::SelectionPane, Icon::SelectionPane, "Selection Pane"),
        ],
        launcher: None,
    },
];

static REFERENCES_GROUPS: &[Group] = &[
    Group {
        label: "Table of Contents",
        items: &[
            Item::Large(Command::InsertContents, Icon::TableOfContents, "Contents"),
            Item::Small(Command::UpdateContents, Icon::UpdateTable, "Update Table"),
            Item::Break,
            Item::Small(Command::RemoveContents, Icon::Close, "Remove Table"),
        ],
        launcher: None,
    },
    Group {
        label: "Footnotes",
        items: &[
            Item::Large(Command::InsertFootnote, Icon::Footnote, "Footnote"),
            Item::Small(Command::InsertEndnote, Icon::Endnote, "Insert Endnote"),
            Item::Break,
            Item::Small(Command::NextNote, Icon::NextFootnote, "Next Footnote"),
            Item::Break,
            Item::Small(Command::DeleteNote, Icon::Close, "Delete Note"),
        ],
        launcher: None,
    },
    Group {
        label: "Citations & Bibliography",
        items: &[
            Item::Large(Command::InsertCitation, Icon::Citation, "Citation"),
            Item::Small(Command::AddSource, Icon::NewComment, "Add Source"),
            Item::Break,
            Item::Small(Command::ManageSources, Icon::ManageSources, "Manage Sources"),
            Item::Break,
            Item::Small(Command::InsertBibliography, Icon::Bibliography, "Bibliography"),
        ],
        launcher: None,
    },
    Group {
        label: "Captions",
        items: &[
            Item::Large(Command::InsertCaption, Icon::Caption, "Caption"),
            Item::Small(Command::CrossReference, Icon::CrossReference, "Cross-reference"),
            Item::Break,
            Item::Small(Command::PageReference, Icon::PageNumber, "Page Reference"),
            Item::Break,
            Item::Small(Command::TableOfFigures, Icon::TableOfFigures, "Table of Figures"),
        ],
        launcher: None,
    },
    Group {
        label: "Index",
        items: &[
            Item::Large(Command::MarkIndexEntry, Icon::MarkEntry, "Mark Entry"),
            Item::Small(Command::InsertIndex, Icon::Index, "Insert Index"),
            Item::Break,
            Item::Small(Command::InsertIndex, Icon::UpdateTable, "Update Index"),
        ],
        launcher: None,
    },
    Group {
        label: "Table of Authorities",
        items: &[
            Item::Large(Command::MarkCitation, Icon::MarkCitation, "Mark Citation"),
            Item::Small(Command::TableOfAuthorities, Icon::TableOfAuthorities, "Insert Table"),
        ],
        launcher: None,
    },
];

static MAILINGS_GROUPS: &[Group] = &[
    Group {
        label: "Create",
        items: &[
            Item::Large(Command::Envelopes, Icon::Envelope, "Envelopes"),
            Item::Large(Command::Labels, Icon::Labels, "Labels"),
        ],
        launcher: None,
    },
    Group {
        label: "Start Mail Merge",
        items: &[
            Item::Large(Command::StartMailMerge, Icon::MailMerge, "Start Merge"),
            Item::Large(Command::SelectRecipients, Icon::Recipients, "Recipients"),
            Item::Large(Command::EditRecipientList, Icon::EditRecipients, "Edit List"),
        ],
        launcher: None,
    },
    Group {
        label: "Write & Insert Fields",
        items: &[
            Item::Large(Command::HighlightMergeFields, Icon::MergeFields, "Highlight"),
            Item::Large(Command::AddressBlock, Icon::AddressBlock, "Address"),
            Item::Large(Command::GreetingLine, Icon::GreetingLine, "Greeting"),
            Item::Small(Command::InsertMergeField, Icon::InsertMergeField, "Merge Field"),
            Item::Break,
            Item::Small(Command::Rules, Icon::Rules, "Rules"),
            Item::Break,
            Item::Small(Command::MatchFields, Icon::MatchFields, "Match Fields"),
        ],
        launcher: None,
    },
    Group {
        label: "Preview Results",
        items: &[
            Item::Large(Command::PreviewResults, Icon::PreviewResults, "Preview"),
            Item::Small(Command::PreviousRecipient, Icon::Previous, "Previous"),
            Item::Break,
            Item::Small(Command::NextRecipient, Icon::Next, "Next"),
            Item::Break,
            Item::Small(Command::CheckMergeErrors, Icon::CheckErrors, "Check for Errors"),
        ],
        launcher: None,
    },
    Group {
        label: "Finish",
        items: &[Item::Large(Command::FinishMerge, Icon::FinishMerge, "Finish")],
        launcher: None,
    },
];

static REVIEW_GROUPS: &[Group] = &[
    Group {
        label: "Proofing",
        items: &[
            Item::Large(Command::Spelling, Icon::Spelling, "Spelling"),
            Item::Large(Command::ShowProofing, Icon::ShowMarkup, "Show Marks"),
            Item::Large(Command::LoadDictionary, Icon::Thesaurus, "Word List"),
            Item::Large(Command::WordCount, Icon::WordCount, "Word Count"),
        ],
        launcher: None,
    },
    Group {
        label: "Accessibility",
        items: &[Item::Large(Command::CheckAccessibility, Icon::Accessibility, "Check")],
        launcher: None,
    },
    Group {
        label: "Language",
        items: &[
            Item::Large(Command::Translate, Icon::Translate, "Translate"),
            Item::Large(Command::Language, Icon::Language, "Language"),
        ],
        launcher: None,
    },
    Group {
        label: "Comments",
        items: &[
            Item::Large(Command::NewComment, Icon::NewComment, "New"),
            Item::Large(Command::DeleteComment, Icon::DeleteComment, "Delete"),
            Item::Large(Command::PreviousComment, Icon::Previous, "Previous"),
            Item::Large(Command::NextComment, Icon::Next, "Next"),
            Item::Large(Command::ShowComments, Icon::ShowComments, "Show"),
        ],
        launcher: None,
    },
    Group {
        label: "Tracking",
        items: &[
            Item::Large(Command::TrackChanges, Icon::TrackChanges, "Track Changes"),
            Item::Small(Command::ShowMarkup, Icon::ShowMarkup, "Show Markup"),
            Item::Break,
            Item::Small(Command::ReviewingPane, Icon::ReviewingPane, "Reviewing Pane"),
        ],
        launcher: None,
    },
    Group {
        label: "Changes",
        items: &[
            Item::Large(Command::AcceptChange, Icon::Accept, "Accept"),
            Item::Large(Command::RejectChange, Icon::Reject, "Reject"),
            Item::Small(Command::AcceptAll, Icon::SetAsDefault, "Accept All"),
            Item::Break,
            Item::Small(Command::RejectAll, Icon::Close, "Reject All"),
        ],
        launcher: None,
    },
    Group {
        label: "Compare",
        items: &[Item::Large(Command::Compare, Icon::Compare, "Compare")],
        launcher: None,
    },
    Group {
        label: "Protect",
        items: &[
            Item::Small(Command::BlockAuthors, Icon::BlockAuthors, "Block Authors"),
            Item::Break,
            Item::Small(Command::RestrictEditing, Icon::RestrictEditing, "Restrict Editing"),
        ],
        launcher: None,
    },
];

static VIEW_GROUPS: &[Group] = &[
    Group {
        label: "Views",
        items: &[
            Item::Large(Command::ReadMode, Icon::ReadMode, "Read Mode"),
            Item::Large(Command::PrintLayout, Icon::PrintLayout, "Print Layout"),
            Item::Large(Command::WebLayout, Icon::WebLayout, "Web Layout"),
            Item::Large(Command::OutlineView, Icon::Outline, "Outline"),
            Item::Large(Command::DraftView, Icon::Draft, "Draft"),
        ],
        launcher: None,
    },
    Group {
        label: "Dark Mode",
        items: &[Item::Large(Command::ToggleTheme, Icon::Theme, "Switch Modes")],
        launcher: None,
    },
    Group {
        label: "Page Movement",
        items: &[
            Item::Small(Command::JoinPages, Icon::JoinPages, "Hide White Space"),
            Item::Break,
            Item::Small(Command::SideToSide, Icon::SideToSide, "Side to Side"),
        ],
        launcher: None,
    },
    Group {
        label: "Show",
        items: &[
            Item::Small(Command::ToggleRulers, Icon::Ruler, "Ruler"),
            Item::Break,
            Item::Small(Command::Gridlines, Icon::Gridlines, "Gridlines"),
            Item::Break,
            Item::Small(Command::ToggleNavigation, Icon::SelectionPane, "Navigation Pane"),
        ],
        launcher: None,
    },
    Group {
        label: "Zoom",
        items: &[
            Item::Large(Command::ChooseZoom, Icon::Zoom, "Zoom"),
            Item::Large(Command::ZoomHundred, Icon::ZoomHundred, "100%"),
            Item::Small(Command::OnePage, Icon::OnePage, "One Page"),
            Item::Break,
            Item::Small(Command::PageWidth, Icon::PageWidth, "Page Width"),
            Item::Break,
            Item::Small(Command::ShowMarks, Icon::Pilcrow, "Formatting Marks"),
        ],
        launcher: None,
    },
    Group {
        label: "Window",
        items: &[
            Item::Small(Command::NewWindow, Icon::NewWindow, "New Window"),
            Item::Break,
            Item::Small(Command::ArrangeAll, Icon::ArrangeAll, "Arrange All"),
            Item::Break,
            Item::Small(Command::Split, Icon::Split, "Split"),
        ],
        launcher: None,
    },
    Group {
        label: "Macros",
        items: &[Item::Large(Command::Macros, Icon::Macros, "Macros")],
        launcher: None,
    },
];

static HELP_GROUPS: &[Group] = &[Group {
    label: "Help",
    items: &[
        Item::Large(Command::About, Icon::Help, "Help"),
        Item::Large(Command::Feedback, Icon::Feedback, "Feedback"),
        Item::Large(Command::ShowTraining, Icon::Training, "Training"),
        Item::Large(Command::WhatsNew, Icon::WhatsNew, "What's New"),
    ],
    launcher: None,
}];

static TABLE_DESIGN_GROUPS: &[Group] = &[
    Group {
        label: "Table Style Options",
        items: &[
            // Word's six, in Word's arrangement: the rows down one column and
            // the columns down the other.
            Item::Small(Command::TableHeaderRow, Icon::HeaderRow, "Header Row"),
            Item::Break,
            Item::Small(Command::TableTotalRow, Icon::HeaderRow, "Total Row"),
            Item::Break,
            Item::Small(Command::TableBandedRows, Icon::BandedRows, "Banded Rows"),
            Item::NewColumn,
            Item::Small(Command::TableFirstColumn, Icon::HeaderRow, "First Column"),
            Item::Break,
            Item::Small(Command::TableLastColumn, Icon::HeaderRow, "Last Column"),
            Item::Break,
            Item::Small(Command::TableBandedColumns, Icon::BandedRows, "Banded Columns"),
        ],
        launcher: None,
    },
    Group {
        label: "Table Styles",
        items: &[Item::Large(Command::TableStyles, Icon::Themes, "Table Styles")],
        launcher: None,
    },
    Group {
        label: "Borders",
        items: &[
            Item::Large(Command::TableBorders(TableBorderChoice::All), Icon::Borders, "All"),
            Item::Large(
                Command::TableBorders(TableBorderChoice::Outside),
                Icon::BorderOutside,
                "Outside",
            ),
            Item::Large(Command::TableBorders(TableBorderChoice::None), Icon::BorderNone, "None"),
        ],
        launcher: None,
    },
];

/// The tab that appears while a header or a footer is being edited.
///
/// Word's, and in Word's order: what to put in, how to move about, what the
/// section asks for, and the way out.
static HEADER_FOOTER_GROUPS: &[Group] = &[
    Group {
        label: "Header & Footer",
        items: &[
            Item::Large(Command::Header, Icon::Header, "Header"),
            Item::Large(Command::Footer, Icon::Footer, "Footer"),
            Item::Large(Command::PageNumber, Icon::PageNumber, "Page Number"),
            Item::Small(Command::FormatPageNumbers, Icon::Numbering, "Format Page Numbers"),
        ],
        launcher: None,
    },
    Group {
        label: "Insert",
        items: &[
            Item::Small(Command::InsertDate, Icon::DateTime, "Date & Time"),
            Item::Break,
            Item::Small(Command::DocumentProperties, Icon::Properties, "Document Info"),
            Item::Break,
            Item::Small(Command::InsertPicture, Icon::Picture, "Pictures"),
        ],
        launcher: None,
    },
    Group {
        label: "Navigation",
        items: &[
            Item::Small(Command::GoToHeader, Icon::Header, "Go to Header"),
            Item::Break,
            Item::Small(Command::GoToFooter, Icon::Footer, "Go to Footer"),
            Item::Break,
            Item::Small(Command::LinkToPrevious, Icon::Link, "Link to Previous"),
        ],
        launcher: None,
    },
    Group {
        label: "Options",
        items: &[
            Item::Small(Command::DifferentFirstPage, Icon::OnePage, "Different First Page"),
            Item::Break,
            Item::Small(
                Command::DifferentOddEven,
                Icon::MultiplePages,
                "Different Odd & Even Pages",
            ),
        ],
        launcher: None,
    },
    Group {
        label: "Close",
        items: &[Item::Large(Command::CloseFurniture, Icon::Close, "Close Header and Footer")],
        launcher: None,
    },
];

static TABLE_LAYOUT_GROUPS: &[Group] = &[
    Group {
        label: "Rows & Columns",
        items: &[
            Item::Large(Command::InsertRowAbove, Icon::InsertRowAbove, "Above"),
            Item::Large(Command::InsertRowBelow, Icon::InsertRowBelow, "Below"),
            Item::Large(Command::InsertColumnLeft, Icon::InsertColumnLeft, "Left"),
            Item::Large(Command::InsertColumnRight, Icon::InsertColumnRight, "Right"),
        ],
        launcher: None,
    },
    Group {
        label: "Delete",
        items: &[
            Item::Small(Command::DeleteRow, Icon::DeleteRow, "Delete Row"),
            Item::Break,
            Item::Small(Command::DeleteColumn, Icon::DeleteColumn, "Delete Column"),
            Item::Break,
            Item::Small(Command::DeleteTable, Icon::DeleteTable, "Delete Table"),
        ],
        launcher: None,
    },
    Group {
        label: "Merge",
        items: &[
            Item::Small(Command::MergeCells, Icon::MergeCells, "Merge Cells"),
            Item::Break,
            Item::Small(Command::SplitCells, Icon::SplitCells, "Split Cells"),
        ],
        launcher: None,
    },
    Group {
        label: "Cell Size",
        items: &[
            Item::Small(Command::DistributeColumns, Icon::AutoFit, "Distribute"),
            Item::Break,
            Item::Small(Command::TableProperties, Icon::TableProperties, "Properties"),
        ],
        launcher: None,
    },
    Group {
        label: "Alignment",
        items: &[
            Item::Button(Command::Align(Alignment::Start), Icon::AlignStart),
            Item::Button(Command::Align(Alignment::Center), Icon::AlignCenter),
            Item::Button(Command::Align(Alignment::End), Icon::AlignEnd),
        ],
        launcher: None,
    },
];

/// The groups of one tab, without needing a ribbon to ask.
#[must_use]
pub fn groups_of(tab: Tab) -> &'static [Group] {
    match tab {
        Tab::File => FILE_GROUPS,
        Tab::Home => HOME_GROUPS,
        Tab::Insert => INSERT_GROUPS,
        Tab::Design => DESIGN_GROUPS,
        Tab::Layout => LAYOUT_GROUPS,
        Tab::References => REFERENCES_GROUPS,
        Tab::Mailings => MAILINGS_GROUPS,
        Tab::Review => REVIEW_GROUPS,
        Tab::View => VIEW_GROUPS,
        Tab::Help => HELP_GROUPS,
        Tab::TableDesign => TABLE_DESIGN_GROUPS,
        Tab::TableLayout => TABLE_LAYOUT_GROUPS,
        Tab::HeaderFooter => HEADER_FOOTER_GROUPS,
    }
}

/// The commands that have a name but no button on the ribbon.
///
/// The File tab's places are here because that tab opens the backstage rather
/// than a page of buttons, and the paste options because they live on the
/// little button at the end of a paste. Both still need names: a macro that
/// saves the document has to be able to write down that it saved the document.
static OFF_RIBBON_COMMANDS: &[(Command, &str)] = &[
    (Command::New, "New"),
    (Command::Open, "Open"),
    (Command::Save, "Save"),
    (Command::SaveAs, "Save As"),
    (Command::Print, "Print"),
    (Command::DocumentProperties, "Info"),
    (Command::Options, "Options"),
    (Command::CloseDocument, "Close"),
    (Command::PasteKeepSource, "Keep Source Formatting"),
    (Command::PasteMerge, "Merge Formatting"),
    (Command::PasteAsPicture, "Paste as Picture"),
    (Command::PasteTextOnly, "Keep Text Only"),
];

/// What a command is called, taken from the button that runs it.
///
/// The ribbon is where every command already has a name in English, so it is
/// also where a macro gets the name to write down. A command with no button —
/// one only a keystroke reaches — has no name here and is not recorded.
#[must_use]
pub fn name_of(command: Command) -> Option<&'static str> {
    if let Some((_, name)) = OFF_RIBBON_COMMANDS.iter().find(|(found, _)| *found == command) {
        return Some(name);
    }
    for tab in Tab::ALL {
        for group in groups_of(*tab) {
            for item in group.items {
                let (found, label) = match item {
                    Item::Large(found, _, label) | Item::Small(found, _, label) => (*found, *label),
                    Item::Letter(found, label, _) | Item::Measure(found, label, _) => {
                        (*found, *label)
                    }
                    Item::Button(..)
                    | Item::Field(..)
                    | Item::StyleGallery
                    | Item::Break
                    | Item::NewColumn => {
                        continue;
                    }
                };
                if found == command {
                    return Some(label);
                }
            }
        }
    }
    None
}

/// And back again: the command a name stands for.
#[must_use]
pub fn command_named(name: &str) -> Option<Command> {
    if let Some((command, _)) = OFF_RIBBON_COMMANDS.iter().find(|(_, found)| *found == name) {
        return Some(*command);
    }
    for tab in Tab::ALL {
        for group in groups_of(*tab) {
            for item in group.items {
                let (found, label) = match item {
                    Item::Large(found, _, label) | Item::Small(found, _, label) => (*found, *label),
                    Item::Letter(found, label, _) | Item::Measure(found, label, _) => {
                        (*found, *label)
                    }
                    Item::Button(..)
                    | Item::Field(..)
                    | Item::StyleGallery
                    | Item::Break
                    | Item::NewColumn => {
                        continue;
                    }
                };
                if label == name {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// What is inside one group of a tab: every command it holds, and its name.
#[must_use]
pub fn group_commands(tab: Tab, index: usize) -> Vec<(Command, &'static str)> {
    let Some(group) = groups_of(tab).get(index) else { return Vec::new() };
    group
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Large(command, _, label) | Item::Small(command, _, label) => {
                Some((*command, *label))
            }
            Item::Letter(command, label, _) | Item::Measure(command, label, _) => {
                Some((*command, *label))
            }
            // A button with no label and a box showing a value are named by
            // what they are about, which the catalogue of icons knows.
            Item::Button(command, _) | Item::Field(command, ..) => {
                name_of(*command).map(|label| (*command, label))
            }
            // The gallery of styles is a group in itself; the list of them is
            // what opening it means.
            Item::StyleGallery => Some((Command::ChooseStyle, "Styles")),
            Item::Break | Item::NewColumn => None,
        })
        .collect()
}

/// Word's dialog launcher: a corner and an arrow leaving it.
///
/// Small enough that it is a shape rather than a picture — a right angle open
/// at the top right, with a short diagonal going out through the opening.
fn launcher_mark(canvas: &mut Canvas, x: f32, y: f32, size: f32, colour: Color) {
    let (x, y, size) = (x as i32, y as i32, size as i32);
    // The corner: down the left-hand side and along the bottom.
    canvas.fill_rect(x, y + size / 3, 1, size - size / 3, colour);
    canvas.fill_rect(x, y + size - 1, size, 1, colour);
    // The arrow leaving it, going up and to the right.
    for step in 0..size - size / 3 {
        canvas.fill_rect(x + 2 + step, y + size - 3 - step, 1, 1, colour);
    }
    // And its head.
    canvas.fill_rect(x + size - 3, y + 1, 3, 1, colour);
    canvas.fill_rect(x + size - 1, y + 1, 1, 3, colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Places one item by hand, the way a drawing pass would.
    fn placed(command: Command, left: f32, top: f32, width: f32, height: f32) -> Ribbon {
        let mut ribbon = Ribbon::new();
        ribbon.placed.push(Placed { command, left, top, width, height, field: None });
        ribbon
    }

    #[test]
    fn a_button_with_no_menu_is_run_wherever_it_is_pressed() {
        let ribbon = placed(Command::Copy, 10.0, 40.0, 26.0, 26.0);
        assert_eq!(ribbon.press_at(12, 42), Some(Press::Run(Command::Copy)));
        assert_eq!(ribbon.press_at(34, 42), Some(Press::Run(Command::Copy)));
    }

    #[test]
    fn the_face_of_a_split_button_runs_and_its_arrow_drops() {
        // Bullets is Word's split button: pressing it puts bullets on, and
        // pressing the arrow beside it asks which bullet.
        let ribbon = placed(Command::Bullets, 10.0, 40.0, 39.0, 26.0);
        assert_eq!(ribbon.press_at(12, 45), Some(Press::Run(Command::Bullets)));
        assert_eq!(
            ribbon.press_at(46, 45),
            Some(Press::Drop(Command::Bullets, Choice::BulletLibrary))
        );
    }

    #[test]
    fn a_plain_dropdown_drops_wherever_it_is_pressed() {
        // Change Case has no command of its own: there is no such thing as
        // "the case", so every part of the button asks which one.
        let ribbon = placed(Command::ChangeCase, 10.0, 40.0, 39.0, 26.0);
        assert_eq!(
            ribbon.press_at(12, 45),
            Some(Press::Drop(Command::ChangeCase, Choice::LetterCase))
        );
        assert_eq!(
            ribbon.press_at(46, 45),
            Some(Press::Drop(Command::ChangeCase, Choice::LetterCase))
        );
    }

    #[test]
    fn a_large_split_button_is_divided_across_rather_than_down() {
        // Word splits a large button into an icon that runs the command and a
        // label under it that drops the list.
        let ribbon = placed(Command::AcceptChange, 10.0, 40.0, 60.0, 79.0);
        assert_eq!(ribbon.press_at(40, 50), Some(Press::Run(Command::AcceptChange)));
        assert_eq!(
            ribbon.press_at(40, 110),
            Some(Press::Drop(Command::AcceptChange, Choice::Accepting))
        );
    }

    #[test]
    fn every_menu_hangs_under_a_button_that_is_really_on_the_ribbon() {
        // A menu whose button is on no tab could never be opened, and the
        // arrow drawn on it would be an arrow to nowhere.
        for menu in MENUS {
            let on_a_tab = Tab::ALL
                .iter()
                .chain(Tab::CONTEXTUAL.iter())
                .flat_map(|tab| groups_of(*tab))
                .flat_map(|group| group.items.iter())
                .any(|item| match item {
                    Item::Large(command, ..)
                    | Item::Small(command, ..)
                    | Item::Button(command, _)
                    | Item::Letter(command, ..)
                    | Item::Measure(command, ..)
                    | Item::Field(command, ..) => *command == menu.command,
                    Item::Break | Item::NewColumn | Item::StyleGallery => false,
                });
            assert!(on_a_tab, "{:?} drops a menu and is on no tab", menu.command);
        }
    }

    #[test]
    fn no_command_carries_two_menus() {
        for menu in MENUS {
            let count = MENUS.iter().filter(|other| other.command == menu.command).count();
            assert_eq!(count, 1, "{:?} carries more than one menu", menu.command);
        }
    }
}

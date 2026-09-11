//! Dialogs: the box that asks a question a click cannot answer.
//!
//! # Why there was none
//!
//! Everything this program draws is drawn by this program, on one canvas, and
//! there was no machinery for a box that takes the keyboard, holds fields, and
//! comes back with an answer. So the questions Word asks in a dialog have been
//! asked in a strip along the top of the window — which works, and is not what
//! Word does, and cannot hold half of what a dialog holds.
//!
//! # What a dialog is here
//!
//! A panel drawn over the document, with the document dimmed behind it. Not a
//! window of the operating system: everything else in this program is drawn on
//! the one canvas, and a dialog drawn the same way works on any machine the
//! program is ever ported to and can be photographed for a test. What it gives
//! up is being dragged outside the window, which Word's can be.
//!
//! What it keeps is everything else a person expects: a title, fields that take
//! the keyboard in order, Tab and Shift+Tab between them, Space to tick a box,
//! the arrows inside a list, Enter for the button that is in bold, Escape to
//! cancel, and a click anywhere outside that does nothing at all — because a
//! modal dialog is modal.

use wp_docx::model::{Alignment, ResolvedParagraphProperties, ResolvedRunProperties};
use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};
use wp_shell::Key;

use super::icons::{self, Icon};
use super::theme::Theme;

/// How wide a dialog is drawn, unless it says otherwise.
const WIDTH: f32 = 420.0;

/// The height of one row of the body.
const ROW: f32 = 30.0;

/// The height of a box that takes typing.
const BOX_HEIGHT: f32 = 24.0;

/// The room round everything inside the panel.
const PADDING: f32 = 16.0;

/// The bar along the top of the panel.
const TITLE_HEIGHT: f32 = 34.0;

/// The bar along the bottom, where the buttons are.
const FOOTER_HEIGHT: f32 = 48.0;

/// The strip of tabs under the caption, when a dialog has them.
const TAB_HEIGHT: f32 = 30.0;

/// The box a preview is drawn inside.
const PREVIEW_HEIGHT: f32 = 64.0;

/// The box a paragraph's shape is drawn inside, which needs room for three
/// paragraphs rather than one line.
const SHAPE_HEIGHT: f32 = 108.0;

/// The room a label takes when it stands above its field rather than beside it.
const LABEL_HEIGHT: f32 = 18.0;

/// Between two fields that share a row.
const COLUMN_GAP: f32 = 10.0;

/// How far in from the panel a group's contents are set.
const GROUP_INSET: f32 = 10.0;

/// The room a group's caption takes above its first field.
const GROUP_CAPTION: f32 = 20.0;

/// How many characters a grid puts across a row.
///
/// Word's number, and the reason its Symbol dialog is the width it is.
const GRID_COLUMNS: usize = 16;

/// How many rows of it are shown at once; the rest is scrolled to.
const GRID_ROWS: usize = 8;

/// How big one cell of the grid is.
const GRID_CELL: f32 = 26.0;

/// How many rows of a list of pairs are shown at once.
///
/// Word's AutoCorrect dialog shows seven of its nine hundred replacements and
/// scrolls the rest, which is as many as fit without the dialog growing taller
/// than the screen it has to stand on.
pub const PAIR_ROWS: usize = 7;

/// How tall one of those rows is.
const PAIR_ROW: f32 = 20.0;

/// How many rows of a tree are shown at once.
///
/// More than a list of pairs shows, because the two pages a tree is on hold
/// nothing else: the room is there, and every row it saves is a row somebody
/// does not have to scroll to.
pub const TREE_ROWS: usize = 12;

/// How far one depth of a tree sets a row in from the one above it.
const TREE_INDENT: f32 = 16.0;

/// How big a tick box on a tree row is.
const TICK_SIZE: f32 = 12.0;

/// And how much room the triangle that folds a row open takes.
const FOLD_SIZE: f32 = 12.0;

/// Which part of a row of a tree was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TreePart {
    /// The triangle, which folds what is under it open or shut.
    Fold,
    /// The tick box, which switches the thing on or off.
    Tick,
    /// Anywhere else, which only chooses the row.
    Words,
}

/// One thing a dialog asks about.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    /// A heading inside the body, which nothing can land on.
    Heading(String),
    /// A line of text the dialog is telling rather than asking.
    Said { label: String, value: String },
    /// A box with words in it.
    Text { label: String, value: String },
    /// A box with a number in it, and what the number is measured in.
    Number { label: String, value: String, unit: &'static str },
    /// A box that is ticked or not.
    Check { label: String, on: bool },
    /// One of several, shown as a list that drops open.
    Choice { label: String, items: Vec<String>, current: usize },
    /// The start of a tab. Everything after it belongs to that tab until the
    /// next one, and only the fields of the tab that is showing are drawn or
    /// reachable.
    ///
    /// A marker in the one list rather than a list of lists, so that a field
    /// keeps the same number whichever tab it is on: what a dialog is asked for
    /// afterwards — "what does field six say" — must not change because
    /// somebody clicked a tab.
    Tab(String),
    /// A sample of what is being asked about, drawn as the document would draw
    /// it. See [`crate::editor`] for what fills it in.
    Preview(Box<Sample>),
    /// A sample of a paragraph rather than of a letter: bars standing for lines
    /// of text, at the indents, spacing and alignment being asked about.
    ///
    /// Word's Paragraph dialog shows this rather than real text, and it is
    /// right to: what is being set is where the lines start and end and how far
    /// apart they are, and grey bars show that at a glance where a wall of
    /// lorem ipsum would not.
    Shape(Box<ParagraphSample>),
    /// The next `n` fields share one row, each with its label above it rather
    /// than beside it.
    ///
    /// Word's dialogs are built of these: Font, Font style and Size across the
    /// top of the Font dialog; Left, Right and Special across the Paragraph
    /// dialog. Fields that belong together are put together, and the eye finds
    /// them as a group instead of walking a column of twenty.
    Columns(u8),
    /// A grid of characters, one of them chosen.
    ///
    /// Word's Symbol dialog is built round one, and nothing else in a dialog
    /// looks anything like it: a hundred and twenty-eight cells that are picked
    /// from with the arrows or the mouse, and that scroll when the subset is
    /// longer than the grid.
    Grid { label: String, items: Vec<char>, current: usize, scroll: usize },
    /// Two columns of words, one row of them chosen, scrolling.
    ///
    /// Word's AutoCorrect dialog is built round one: what is typed on the left,
    /// what goes in its place on the right. A `Choice` will not do — that drops
    /// open, holds one column, and is for picking one of a few. This stands
    /// open, holds a pair on every row, and is for looking through a list that
    /// is added to and deleted from.
    ///
    /// `label` heads the left column and `second` the right. A list with no
    /// second heading is a list of single words — Word's AutoCorrect
    /// exceptions — and is drawn as one column across the whole width.
    Pairs {
        label: String,
        second: String,
        rows: Vec<(String, String)>,
        current: usize,
        scroll: usize,
    },
    /// A column of rows at different depths, some of which are ticked.
    ///
    /// Word's Customize Ribbon page is built round one: the tabs against the
    /// left-hand edge, their groups set in from it, the commands set in again,
    /// and a tick box against each tab and group saying whether it is shown. A
    /// list with every row at the same depth and no tick boxes is the plain
    /// list beside it, which is the same thing with nothing turned on.
    Tree { label: String, rows: Vec<TreeRow>, current: usize, scroll: usize },
    /// A rectangle round everything that follows, with a caption on its top
    /// edge, until the next group or the next tab.
    ///
    /// The other half of what makes Word's dialogs readable: "Indentation"
    /// drawn round the two indent boxes says what those two boxes have to do
    /// with each other, which no amount of putting them near each other does.
    Group(String),
}

/// One row of a [`Field::Tree`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    /// How far in it is set: nothing for a tab, one for a group, two for a
    /// command inside one.
    pub depth: u8,
    pub text: String,
    /// Whether it carries a tick box, and whether the box is ticked.
    pub tick: Option<bool>,
    /// Whether what is under it is showing.
    ///
    /// Nothing at all means nothing is under it. A tree of every tab, every
    /// group and everything added to one is some seventy rows, and seven of
    /// them are in sight at once: without this, reaching the View tab would be
    /// nine screens of scrolling. Word's tree collapses for the same reason.
    pub open: Option<bool>,
}

impl TreeRow {
    /// A row with no tick box and nothing under it, which is what a plain list
    /// is made of.
    #[must_use]
    pub fn plain(text: &str) -> Self {
        Self { depth: 0, text: text.to_owned(), tick: None, open: None }
    }

    /// One set in from the edge, with a tick box.
    #[must_use]
    pub fn ticked(depth: u8, text: &str, on: bool) -> Self {
        Self { depth, text: text.to_owned(), tick: Some(on), open: None }
    }

    /// One set in from the edge with no tick box: what is inside a group.
    #[must_use]
    pub fn under(depth: u8, text: &str) -> Self {
        Self { depth, text: text.to_owned(), tick: None, open: None }
    }

    /// The same row, with something under it that is showing or is not.
    #[must_use]
    pub fn opening(mut self, open: bool) -> Self {
        self.open = Some(open);
        self
    }
}

/// Which rows of a tree are in sight: the ones no closed row stands above.
fn shown_rows(rows: &[TreeRow]) -> Vec<usize> {
    let mut out = Vec::with_capacity(rows.len());
    // The depth below which everything is folded away, while anything is.
    let mut folded: Option<u8> = None;
    for (at, row) in rows.iter().enumerate() {
        match folded {
            Some(depth) if row.depth >= depth => continue,
            _ => folded = None,
        }
        out.push(at);
        if row.open == Some(false) {
            folded = Some(row.depth + 1);
        }
    }
    out
}

/// What a preview shows: some text, and the formatting to draw it with.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub text: String,
    pub properties: Box<ResolvedRunProperties>,
}

/// What a paragraph preview shows: the shape a paragraph so formatted takes.
#[derive(Clone, Debug, PartialEq)]
pub struct ParagraphSample {
    pub properties: Box<ResolvedParagraphProperties>,
}

impl Field {
    /// Whether the keyboard can land on it.
    #[must_use]
    pub fn takes_focus(&self) -> bool {
        !matches!(
            self,
            Self::Heading(_)
                | Self::Said { .. }
                | Self::Tab(_)
                | Self::Preview(_)
                | Self::Shape(_)
                | Self::Columns(_)
                | Self::Group(_)
        )
    }

    /// The word down the left-hand column, for the fields that have one.
    ///
    /// A heading spans the panel and a tick box labels itself, so neither takes
    /// room in that column.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Heading(_)
            | Self::Check { .. }
            | Self::Tab(_)
            | Self::Preview(_)
            | Self::Shape(_)
            | Self::Columns(_)
            | Self::Group(_) => None,
            // A grid's label stands above it rather than beside it, whatever
            // row it is on: sixteen cells across leave no room for a column.
            // So does a list of pairs, for the same reason.
            Self::Grid { .. } | Self::Pairs { .. } | Self::Tree { .. } => None,
            Self::Said { label, .. }
            | Self::Text { label, .. }
            | Self::Number { label, .. }
            | Self::Choice { label, .. } => Some(label),
        }
    }

    /// What kind of thing this is, in words.
    ///
    /// For the check a dialog makes on itself when it is built: the rows it
    /// reads back are named by constant, and a row inserted in the middle moves
    /// every row after it. Comparing what is at each named row against what
    /// should be there catches that, and catches it loudly — a dialog reading
    /// the wrong rows applies the wrong thing and never looks wrong doing it.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Heading(_) => "a heading",
            Self::Said { .. } => "a line",
            Self::Text { .. } => "a box",
            Self::Number { .. } => "a number",
            Self::Check { .. } => "a tick box",
            Self::Choice { .. } => "a list",
            Self::Tab(_) => "a tab",
            Self::Preview(_) => "a preview",
            Self::Shape(_) => "a shape",
            Self::Columns(_) => "a row",
            Self::Group(_) => "a group",
            Self::Grid { .. } => "a grid",
            Self::Pairs { .. } => "a list of pairs",
            Self::Tree { .. } => "a list of rows",
        }
    }

    /// How tall this field is drawn, on a row of its own.
    fn height(&self) -> f32 {
        match self {
            // Neither takes a row: the tabs are drawn along the top, and a
            // shared row's height comes from the fields on it.
            Self::Tab(_) | Self::Columns(_) => 0.0,
            Self::Heading(_) => ROW,
            // Room for the caption, and then whatever follows it inside.
            Self::Group(_) => GROUP_CAPTION,
            Self::Preview(_) => PREVIEW_HEIGHT + PADDING,
            Self::Shape(_) => SHAPE_HEIGHT + PADDING,
            Self::Grid { .. } => LABEL_HEIGHT + GRID_ROWS as f32 * GRID_CELL + PADDING,
            Self::Pairs { .. } => LABEL_HEIGHT + PAIR_ROWS as f32 * PAIR_ROW + PADDING,
            Self::Tree { .. } => LABEL_HEIGHT + TREE_ROWS as f32 * PAIR_ROW + PADDING,
            _ => ROW + 4.0,
        }
    }
}

/// What a button does when it is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Take what the fields say and act on it.
    Accept,
    /// Leave everything as it was.
    Cancel,
    /// Take what the fields say, and do the further thing the named button
    /// offered.
    ///
    /// Word's dialogs have buttons past OK and Cancel often enough to be worth
    /// a third answer, and more than one of them at a time: the Paragraph
    /// dialog has both Tabs and Set As Default. Named rather than numbered so
    /// that the place that acts on the answer says which button it means.
    Named(&'static str),
}

/// A button along the bottom of the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub answer: Answer,
    /// Whether Enter presses it. Word draws that one with a ring round it.
    pub default: bool,
}

/// What the keyboard or a click did to the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    /// Nothing this dialog knows about.
    Ignored,
    /// Something changed and the window has to be drawn again.
    Changed,
    /// The dialog is finished with, one way or the other.
    Closed(Answer),
}

/// One row of a dialog's body: which fields are drawn across it, and how tall.
///
/// Worked out once and used by both the drawing and the measuring, because a
/// panel whose height was decided one way and whose contents were laid out
/// another is a panel with its buttons over its last field.
#[derive(Clone, Debug)]
struct Row {
    /// One field, or several sharing the row.
    fields: Vec<usize>,
    height: f32,
    /// Whether the labels stand above their fields rather than beside them.
    ///
    /// True for fields that share a row and carry a label — Word puts the
    /// label over the box there. Not for a row of tick boxes, which label
    /// themselves and would leave an empty line above each one.
    labels_above: bool,
    /// Whether the row is inside a group's rectangle, and so set in from the
    /// edge of the panel.
    inside_group: bool,
}

/// Where one field is drawn.
#[derive(Clone, Copy, Debug)]
struct Place {
    label_x: f32,
    label_y: f32,
    box_x: f32,
    box_y: f32,
    box_width: f32,
}

/// Where the mouse can land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    Field(usize),
    Button(usize),
    /// One of the tabs along the top, counted from the first.
    Tab(usize),
    Close,
}

/// A dialog, and everything it is asking.
#[derive(Clone, Debug)]
pub struct Dialog {
    pub title: String,
    pub fields: Vec<Field>,
    pub buttons: Vec<Button>,
    /// Which field the keyboard is on, or, past the end of them, which button.
    focus: usize,
    /// Which list is dropped open, if any.
    open_list: Option<usize>,
    placed: Vec<(Hit, f32, f32, f32, f32)>,
    hovered: Option<Hit>,
    width: f32,
    /// Which tab is showing, for a dialog that has them.
    tab: usize,
    /// The buttons that belong to one tab, and which.
    ///
    /// Word puts Add and Delete beside the list they work on, which is on one
    /// tab of the AutoCorrect dialog and not the other. Here every button is
    /// along the bottom, so a button that works on something another tab does
    /// not show says which tab it is for and is drawn on that one alone: a
    /// button that changes what nobody can see is worse than no button.
    button_tabs: Vec<(Answer, usize)>,
}

impl Dialog {
    /// A dialog with the two buttons every dialog has.
    #[must_use]
    pub fn new(title: &str, fields: Vec<Field>) -> Self {
        Self::with_buttons(
            title,
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
    }

    /// A dialog that is telling rather than asking, and so has one button.
    #[must_use]
    pub fn message(title: &str, fields: Vec<Field>) -> Self {
        Self::with_buttons(
            title,
            fields,
            vec![Button { label: "Close".to_owned(), answer: Answer::Accept, default: true }],
        )
    }

    #[must_use]
    pub fn with_buttons(title: &str, fields: Vec<Field>, buttons: Vec<Button>) -> Self {
        let mut dialog = Self {
            title: title.to_owned(),
            fields,
            buttons,
            focus: 0,
            open_list: None,
            placed: Vec::new(),
            hovered: None,
            width: WIDTH,
            tab: 0,
            button_tabs: Vec::new(),
        };
        // The keyboard starts on the first thing that can take it, which is
        // where a person expects to start typing.
        dialog.focus = dialog.first_focus();
        dialog
    }

    /// Carries over everything about a dialog that is not what it is asking:
    /// which tab was showing, where the keyboard was, and what has been typed.
    ///
    /// A dialog whose preview follows its fields is built again whenever one
    /// changes. Without this the rebuilt one would open on its first tab with
    /// the keyboard at the top, and a person half-way through filling it in
    /// would be thrown back to the beginning.
    ///
    /// The typing matters more than it looks. A box is built from a value, and
    /// a value is what the box says once it has been read and made sense of —
    /// so a box being typed into would be rewritten at every keystroke with
    /// whatever the half-finished number came to. Typing 150 into a box that
    /// allows at most 600 goes 100, 1001, 600: the second keystroke is read,
    /// clamped, and written back, and the number can never be finished. What
    /// has been typed belongs to the person typing it until they are done.
    pub fn carry_typing_from(&mut self, previous: &Self) {
        self.tab = previous.tab.min(self.tabs().len().saturating_sub(1));
        // Only where the keyboard can actually land: a rebuilt dialog may have
        // fewer fields than the one before it, and a button that belongs to
        // another tab is not drawn on this one.
        if self.stops().contains(&previous.focus) {
            self.focus = previous.focus;
        }
        self.open_list = previous.open_list;

        for (field, before) in self.fields.iter_mut().zip(previous.fields.iter()) {
            if let (
                Field::Text { value, .. } | Field::Number { value, .. },
                Field::Text { value: typed, .. } | Field::Number { value: typed, .. },
            ) = (field, before)
            {
                *value = typed.clone();
            }
        }
    }

    /// Carries over what a dialog has been answered as well as what has been
    /// typed into it.
    ///
    /// For a dialog that is built again for a reason that has nothing to do
    /// with its fields — Options, whose Add and Remove buttons change the
    /// ribbon and leave the dialog standing. The fields are rebuilt from how
    /// the window is now, which is not how the dialog says it should be, so
    /// without this a tick made on one tab would be undone by pressing a button
    /// on another.
    ///
    /// Not what [`Self::carry_typing_from`] does, and deliberately separate
    /// from it: a dialog rebuilt *because* a field changed — the Tabs dialog
    /// after Set, the Font dialog's preview — is rebuilt to show something the
    /// fields do not say yet, and carrying the old answers over would put back
    /// what it was built to change.
    pub fn carry_answers_from(&mut self, previous: &Self) {
        self.carry_typing_from(previous);
        for (field, before) in self.fields.iter_mut().zip(previous.fields.iter()) {
            match (field, before) {
                (Field::Check { on, .. }, Field::Check { on: was, .. }) => *on = *was,
                (Field::Choice { items, current, .. }, Field::Choice { current: was, .. }) => {
                    if *was < items.len() {
                        *current = *was;
                    }
                }
                // A tree carries over which rows are folded open, matched by
                // what they say rather than by where they are: the rows are
                // built again and a command added or taken away moves every row
                // after it.
                (Field::Tree { rows, current, scroll, .. }, Field::Tree { rows: before, .. }) => {
                    for row in rows.iter_mut() {
                        let was = before
                            .iter()
                            .find(|old| old.depth == row.depth && old.text == row.text);
                        if let (Some(open), Some(was)) = (row.open.as_mut(), was) {
                            if let Some(before) = was.open {
                                *open = before;
                            }
                        }
                    }
                    *current = (*current).min(rows.len().saturating_sub(1));
                    *scroll = (*scroll).min(shown_rows(rows).len().saturating_sub(1));
                }
                _ => {}
            }
        }
    }

    /// The same dialog, with one of its buttons kept to one tab.
    #[must_use]
    pub fn button_on_tab(mut self, answer: Answer, tab: usize) -> Self {
        self.button_tabs.push((answer, tab));
        self
    }

    /// The same dialog, drawn wider.
    ///
    /// Word's dialogs are not all one width: the Font dialog holds three
    /// fields across and needs the room for them; the Bookmark dialog holds
    /// one name.
    #[must_use]
    pub fn wide(mut self, width: f32) -> Self {
        self.width = width.max(240.0);
        self
    }

    /// Whether a box is ticked.
    #[must_use]
    pub fn ticked(&self, index: usize) -> bool {
        matches!(self.fields.get(index), Some(Field::Check { on: true, .. }))
    }

    /// What was typed into a box, or chosen from a list.
    #[must_use]
    pub fn said(&self, index: usize) -> String {
        match self.fields.get(index) {
            Some(Field::Text { value, .. } | Field::Number { value, .. }) => value.clone(),
            Some(Field::Choice { items, current, .. }) => {
                items.get(*current).cloned().unwrap_or_default()
            }
            _ => String::new(),
        }
    }

    /// Which tab is showing, counted from the first.
    #[must_use]
    pub fn showing_tab(&self) -> usize {
        self.tab
    }

    /// Which cell of a grid is picked, as the character it stands for.
    #[must_use]
    pub fn picked(&self, index: usize) -> Option<char> {
        match self.fields.get(index) {
            Some(Field::Grid { items, current, .. }) => items.get(*current).copied(),
            _ => None,
        }
    }

    /// Which row of a list of pairs is chosen, as the pair itself.
    #[must_use]
    pub fn pair(&self, index: usize) -> Option<(&str, &str)> {
        match self.fields.get(index) {
            Some(Field::Pairs { rows, current, .. }) => {
                rows.get(*current).map(|(what, with)| (what.as_str(), with.as_str()))
            }
            _ => None,
        }
    }

    /// Which row of a list of rows is chosen, and the row itself.
    #[must_use]
    pub fn chose_row(&self, index: usize) -> usize {
        match self.fields.get(index) {
            Some(Field::Tree { current, .. }) => *current,
            _ => 0,
        }
    }

    /// Every row of one, for a caller that has to read the ticks back.
    #[must_use]
    pub fn tree_rows(&self, index: usize) -> &[TreeRow] {
        match self.fields.get(index) {
            Some(Field::Tree { rows, .. }) => rows,
            _ => &[],
        }
    }

    /// Which row of a list of pairs is chosen, as its place on the list.
    #[must_use]
    pub fn chose_pair(&self, index: usize) -> usize {
        match self.fields.get(index) {
            Some(Field::Pairs { current, .. }) => *current,
            _ => 0,
        }
    }

    /// Which of a list was chosen.
    #[must_use]
    pub fn chose(&self, index: usize) -> usize {
        match self.fields.get(index) {
            Some(Field::Choice { current, .. }) => *current,
            _ => 0,
        }
    }

    /// The first thing the keyboard can land on.
    fn first_focus(&self) -> usize {
        self.stops().into_iter().next().unwrap_or(self.fields.len())
    }

    /// How many places the keyboard can be: the fields it can land on, and then
    /// the buttons.
    fn stops(&self) -> Vec<usize> {
        // Only the tab that is showing: Tab must not walk the keyboard into
        // fields nobody can see.
        let mut out: Vec<usize> = (0..self.fields.len())
            .filter(|index| self.fields[*index].takes_focus() && self.on_this_tab(*index))
            .collect();
        out.extend(
            (0..self.buttons.len())
                .filter(|index| self.button_showing(*index))
                .map(|index| self.fields.len() + index),
        );
        out
    }

    /// Whether a button is drawn on the tab that is showing.
    ///
    /// A button named for no tab is drawn on all of them; one named for any is
    /// drawn on those and nowhere else.
    fn button_showing(&self, index: usize) -> bool {
        let Some(button) = self.buttons.get(index) else { return false };
        let mut named = self.button_tabs.iter().filter(|(answer, _)| *answer == button.answer);
        let Some(first) = named.next() else { return true };
        first.1 == self.tab || named.any(|(_, tab)| *tab == self.tab)
    }

    /// Moves the keyboard on, or back.
    fn step_focus(&mut self, forwards: bool) {
        let stops = self.stops();
        if stops.is_empty() {
            return;
        }
        let at = stops.iter().position(|stop| *stop == self.focus).unwrap_or(0);
        let next =
            if forwards { (at + 1) % stops.len() } else { (at + stops.len() - 1) % stops.len() };
        self.focus = stops[next];
    }

    /// A key pressed while the dialog is up.
    pub fn key(&mut self, key: Key, shift: bool, control: bool) -> Reaction {
        // A list that is dropped open takes the keyboard until it is done with.
        if let Some(index) = self.open_list {
            return match key {
                Key::Up | Key::Down => {
                    let step = if key == Key::Down { 1i32 } else { -1 };
                    self.move_choice(index, step);
                    Reaction::Changed
                }
                Key::Enter | Key::Escape => {
                    self.open_list = None;
                    Reaction::Changed
                }
                _ => Reaction::Ignored,
            };
        }

        match key {
            // Ctrl and Tab walk the tabs, as they do in every dialog Word has;
            // Tab on its own walks the fields of the one showing.
            Key::Tab if control && !self.tabs().is_empty() => {
                let count = self.tabs().len();
                let step = if shift { count - 1 } else { 1 };
                self.show_tab((self.tab + step) % count);
                Reaction::Changed
            }
            Key::Tab => {
                self.step_focus(!shift);
                Reaction::Changed
            }
            Key::Escape => Reaction::Closed(Answer::Cancel),
            Key::Enter => {
                // Enter on a button presses it; anywhere else it presses the
                // one in bold, which is what a dialog's Enter means.
                if let Some(button) = self.focused_button() {
                    return Reaction::Closed(self.buttons[button].answer);
                }
                let default = self.buttons.iter().find(|button| button.default);
                default.map_or(Reaction::Ignored, |button| Reaction::Closed(button.answer))
            }
            // A grid is walked in two directions, as Word's is: the arrows move
            // from cell to cell and the grid scrolls to follow.
            Key::Up | Key::Down | Key::Left | Key::Right
                if matches!(self.fields.get(self.focus), Some(Field::Grid { .. })) =>
            {
                let step = match key {
                    Key::Left => -1,
                    Key::Right => 1,
                    Key::Up => -(GRID_COLUMNS as i32),
                    _ => GRID_COLUMNS as i32,
                };
                self.move_in_grid(self.focus, step);
                Reaction::Changed
            }
            // A list of pairs is walked a row at a time, like any list.
            Key::Up | Key::Down
                if matches!(self.fields.get(self.focus), Some(Field::Pairs { .. })) =>
            {
                self.move_in_pairs(self.focus, if key == Key::Up { -1 } else { 1 });
                Reaction::Changed
            }
            // And so is a list of rows, whose ticks Space turns on and off —
            // which is what Space does to a tick box everywhere else.
            Key::Up | Key::Down
                if matches!(self.fields.get(self.focus), Some(Field::Tree { .. })) =>
            {
                self.move_in_tree(self.focus, if key == Key::Up { -1 } else { 1 });
                Reaction::Changed
            }
            Key::Space if matches!(self.fields.get(self.focus), Some(Field::Tree { .. })) => {
                self.tick_in_tree(self.focus);
                Reaction::Changed
            }
            Key::Up | Key::Down => {
                // The arrows walk a list without dropping it open, as they do
                // in Word.
                let step = if key == Key::Down { 1i32 } else { -1 };
                if matches!(self.fields.get(self.focus), Some(Field::Choice { .. })) {
                    self.move_choice(self.focus, step);
                    return Reaction::Changed;
                }
                self.step_focus(key == Key::Down);
                Reaction::Changed
            }
            Key::Backspace => {
                if let Some(Field::Text { value, .. } | Field::Number { value, .. }) =
                    self.fields.get_mut(self.focus)
                {
                    value.pop();
                    return Reaction::Changed;
                }
                Reaction::Ignored
            }
            _ => Reaction::Ignored,
        }
    }

    /// A character typed while the dialog is up.
    pub fn character(&mut self, character: char) -> Reaction {
        // Space ticks the box the keyboard is on, as it does everywhere.
        if character == ' ' {
            if let Some(Field::Check { on, .. }) = self.fields.get_mut(self.focus) {
                *on = !*on;
                return Reaction::Changed;
            }
            if let Some(button) = self.focused_button() {
                return Reaction::Closed(self.buttons[button].answer);
            }
        }
        if character.is_control() {
            return Reaction::Ignored;
        }

        match self.fields.get_mut(self.focus) {
            Some(Field::Text { value, .. }) => {
                value.push(character);
                Reaction::Changed
            }
            // A number box takes numbers, a point, and a minus at the front.
            Some(Field::Number { value, .. }) => {
                let allowed = character.is_ascii_digit()
                    || (character == '.' && !value.contains('.'))
                    || (character == '-' && value.is_empty());
                if !allowed {
                    return Reaction::Ignored;
                }
                value.push(character);
                Reaction::Changed
            }
            _ => Reaction::Ignored,
        }
    }

    /// A press somewhere in the window.
    pub fn press(&mut self, x: i32, y: i32) -> Reaction {
        // A list dropped open takes the press, wherever it lands.
        if let Some(index) = self.open_list {
            let chosen = self.list_row_at(index, x, y);
            self.open_list = None;
            if let Some(row) = chosen {
                if let Some(Field::Choice { current, .. }) = self.fields.get_mut(index) {
                    *current = row;
                }
            }
            return Reaction::Changed;
        }

        match self.at(x, y) {
            Some(Hit::Close) => Reaction::Closed(Answer::Cancel),
            Some(Hit::Button(index)) => Reaction::Closed(self.buttons[index].answer),
            Some(Hit::Tab(index)) => {
                self.show_tab(index);
                Reaction::Changed
            }
            Some(Hit::Field(index)) => {
                self.focus = index;
                // A grid is picked from cell by cell, so where in it the press
                // landed is the whole of what the press said.
                if matches!(self.fields.get(index), Some(Field::Grid { .. })) {
                    if let Some(cell) = self.grid_cell_at(index, x, y) {
                        if let Some(Field::Grid { current, .. }) = self.fields.get_mut(index) {
                            *current = cell;
                        }
                    }
                    return Reaction::Changed;
                }
                // And a list of rows row by row, with the tick box on each one
                // answering for itself: pressing the box turns it on or off,
                // pressing the words beside it only chooses the row. Word's
                // tree behaves the same, and a box that toggled whenever its
                // row was chosen would switch a group off by being read.
                if matches!(self.fields.get(index), Some(Field::Tree { .. })) {
                    if let Some((row, part)) = self.tree_row_at(index, x, y) {
                        if let Some(Field::Tree { current, .. }) = self.fields.get_mut(index) {
                            *current = row;
                        }
                        match part {
                            TreePart::Fold => self.fold_in_tree(index),
                            TreePart::Tick => self.tick_in_tree(index),
                            TreePart::Words => {}
                        }
                    }
                    return Reaction::Changed;
                }
                // And a list of pairs row by row.
                if matches!(self.fields.get(index), Some(Field::Pairs { .. })) {
                    if let Some(row) = self.pair_row_at(index, y) {
                        if let Some(Field::Pairs { current, .. }) = self.fields.get_mut(index) {
                            *current = row;
                        }
                    }
                    return Reaction::Changed;
                }
                match self.fields.get_mut(index) {
                    Some(Field::Check { on, .. }) => *on = !*on,
                    Some(Field::Choice { .. }) => self.open_list = Some(index),
                    _ => {}
                }
                Reaction::Changed
            }
            // Inside the panel but on nothing, or outside it altogether: a
            // modal dialog swallows the press rather than letting it reach the
            // document behind.
            _ => Reaction::Changed,
        }
    }

    /// Follows the pointer. True when something has to be drawn again.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let over = self.at(x, y);
        let changed = over != self.hovered;
        self.hovered = over;
        changed
    }

    fn at(&self, x: i32, y: i32) -> Option<Hit> {
        let (x, y) = (x as f32, y as f32);
        self.placed
            .iter()
            .find(|(_, left, top, width, height)| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(hit, ..)| *hit)
    }

    fn focused_button(&self) -> Option<usize> {
        self.focus.checked_sub(self.fields.len()).filter(|index| *index < self.buttons.len())
    }

    /// Moves the cell a grid has picked, and scrolls the grid to keep it in
    /// sight.
    ///
    /// A grid that let the picked cell go off the top is a grid where the
    /// arrows appear to do nothing.
    fn move_in_grid(&mut self, index: usize, step: i32) {
        let Some(Field::Grid { items, current, scroll, .. }) = self.fields.get_mut(index) else {
            return;
        };
        if items.is_empty() {
            return;
        }
        let last = items.len() as i32 - 1;
        *current = (*current as i32 + step).clamp(0, last) as usize;

        let row = *current / GRID_COLUMNS;
        if row < *scroll {
            *scroll = row;
        } else if row >= *scroll + GRID_ROWS {
            *scroll = row + 1 - GRID_ROWS;
        }
    }

    /// Which cell of a grid a point is on.
    fn grid_cell_at(&self, index: usize, x: i32, y: i32) -> Option<usize> {
        let (left, top, width, _) = self.rect_of(Hit::Field(index))?;
        let Some(Field::Grid { items, scroll, .. }) = self.fields.get(index) else { return None };
        let cell = width / GRID_COLUMNS as f32;
        if cell <= 0.0 {
            return None;
        }
        let column = ((x as f32 - left) / cell).floor();
        let row = ((y as f32 - top) / cell).floor();
        if column < 0.0 || row < 0.0 || column >= GRID_COLUMNS as f32 || row >= GRID_ROWS as f32 {
            return None;
        }
        let at = (scroll + row as usize) * GRID_COLUMNS + column as usize;
        (at < items.len()).then_some(at)
    }

    /// Moves the row a list of rows has chosen, scrolling to keep it in sight.
    ///
    /// A row folded away is not a row the arrows stop on: the keyboard walks
    /// what the eye can see.
    fn move_in_tree(&mut self, index: usize, step: i32) {
        let Some(Field::Tree { rows, current, scroll, .. }) = self.fields.get_mut(index) else {
            return;
        };
        let shown = shown_rows(rows);
        if shown.is_empty() {
            return;
        }
        let at = shown.iter().position(|row| *row == *current).unwrap_or(0) as i32;
        let wanted = (at + step).clamp(0, shown.len() as i32 - 1) as usize;
        *current = shown[wanted];
        if wanted < *scroll {
            *scroll = wanted;
        } else if wanted >= *scroll + TREE_ROWS {
            *scroll = wanted + 1 - TREE_ROWS;
        }
    }

    /// Turns the tick of the chosen row on or off, if it has one; and folds it
    /// open or shut if it has anything under it instead.
    fn tick_in_tree(&mut self, index: usize) {
        let Some(Field::Tree { rows, current, .. }) = self.fields.get_mut(index) else { return };
        let Some(row) = rows.get_mut(*current) else { return };
        if let Some(on) = &mut row.tick {
            *on = !*on;
        } else if let Some(open) = &mut row.open {
            *open = !*open;
        }
    }

    /// Folds the chosen row open or shut.
    fn fold_in_tree(&mut self, index: usize) {
        let Some(Field::Tree { rows, current, .. }) = self.fields.get_mut(index) else { return };
        if let Some(open) = rows.get_mut(*current).and_then(|row| row.open.as_mut()) {
            *open = !*open;
        }
    }

    /// What a point in a list of rows is on: the row, and which part of it.
    fn tree_row_at(&self, index: usize, x: i32, y: i32) -> Option<(usize, TreePart)> {
        let (left, top, _, _) = self.rect_of(Hit::Field(index))?;
        let Some(Field::Tree { rows, scroll, .. }) = self.fields.get(index) else { return None };
        let row = ((y as f32 - top) / PAIR_ROW).floor();
        if row < 0.0 || row >= TREE_ROWS as f32 {
            return None;
        }
        let at = *shown_rows(rows).get(scroll + row as usize)?;
        let held = rows.get(at)?;

        let mut edge = left + 5.0 + f32::from(held.depth) * TREE_INDENT;
        if held.open.is_some() {
            if (x as f32) < edge + FOLD_SIZE {
                return Some((at, TreePart::Fold));
            }
            edge += FOLD_SIZE;
        }
        if held.tick.is_some() && (x as f32) < edge + TICK_SIZE + 4.0 {
            return Some((at, TreePart::Tick));
        }
        Some((at, TreePart::Words))
    }

    /// Moves the row a list of pairs has chosen, scrolling to keep it in sight.
    fn move_in_pairs(&mut self, index: usize, step: i32) {
        let Some(Field::Pairs { rows, current, scroll, .. }) = self.fields.get_mut(index) else {
            return;
        };
        if rows.is_empty() {
            return;
        }
        let last = rows.len() as i32 - 1;
        *current = (*current as i32 + step).clamp(0, last) as usize;
        if *current < *scroll {
            *scroll = *current;
        } else if *current >= *scroll + PAIR_ROWS {
            *scroll = *current + 1 - PAIR_ROWS;
        }
    }

    /// Which row of a list of pairs a point is on.
    fn pair_row_at(&self, index: usize, y: i32) -> Option<usize> {
        let (_, top, _, _) = self.rect_of(Hit::Field(index))?;
        let Some(Field::Pairs { rows, scroll, .. }) = self.fields.get(index) else { return None };
        let row = ((y as f32 - top) / PAIR_ROW).floor();
        if row < 0.0 || row >= PAIR_ROWS as f32 {
            return None;
        }
        let at = scroll + row as usize;
        (at < rows.len()).then_some(at)
    }

    fn move_choice(&mut self, index: usize, step: i32) {
        if let Some(Field::Choice { items, current, .. }) = self.fields.get_mut(index) {
            if items.is_empty() {
                return;
            }
            let last = items.len() as i32 - 1;
            let wanted = (*current as i32 + step).clamp(0, last);
            *current = wanted as usize;
        }
    }

    /// Which row of a dropped-open list a point is on.
    fn list_row_at(&self, index: usize, x: i32, y: i32) -> Option<usize> {
        let (left, top, width, _) = self.rect_of(Hit::Field(index))?;
        let Some(Field::Choice { items, .. }) = self.fields.get(index) else { return None };
        let (x, y) = (x as f32, y as f32);
        if x < left || x > left + width {
            return None;
        }
        let row = ((y - (top + BOX_HEIGHT)) / ROW).floor();
        if row < 0.0 {
            return None;
        }
        let row = row as usize;
        (row < items.len()).then_some(row)
    }

    fn rect_of(&self, hit: Hit) -> Option<(f32, f32, f32, f32)> {
        self.placed
            .iter()
            .find(|(found, ..)| *found == hit)
            .map(|(_, left, top, width, height)| (*left, *top, *width, *height))
    }

    /// How tall the panel is, from what is in it.
    ///
    /// Only the tab that is showing counts, and the panel is at least as tall
    /// as the tallest tab — a dialog whose height jumped as tabs were clicked
    /// would move its own buttons out from under the pointer.
    fn height(&self) -> f32 {
        let strip = if self.tabs().is_empty() { 0.0 } else { TAB_HEIGHT };
        TITLE_HEIGHT + strip + PADDING + self.tallest_page() + PADDING + FOOTER_HEIGHT
    }

    /// The body height of the tallest tab, or of the whole dialog when it has
    /// no tabs.
    fn tallest_page(&self) -> f32 {
        let count = self.tabs().len();
        if count == 0 {
            return self.rows_of(0).iter().map(|row| row.height).sum();
        }
        (0..count)
            .map(|tab| self.rows_of(tab).iter().map(|row| row.height).sum::<f32>())
            .fold(0.0f32, f32::max)
    }

    /// The rows of the body, in the order they are drawn.
    fn rows(&self) -> Vec<Row> {
        self.rows_of(self.tab)
    }

    /// The same, for whichever tab is asked about.
    ///
    /// The one place that decides what goes on a row and how tall it is. Both
    /// the drawing and the measuring go through it, because a panel measured
    /// one way and laid out another is a panel with its buttons on top of its
    /// last field.
    fn rows_of(&self, tab: usize) -> Vec<Row> {
        let mut out: Vec<Row> = Vec::new();
        let mut inside_group = false;
        let mut index = 0usize;

        // A group's rectangle needs room under its last field for its own
        // bottom edge, and only the row before knows where that is.
        let finish_group = |out: &mut Vec<Row>, inside: &mut bool| {
            if *inside {
                if let Some(last) = out.last_mut() {
                    last.height += GROUP_INSET;
                }
            }
            *inside = false;
        };

        while index < self.fields.len() {
            // A tab marker ends whatever group was open on the tab before it.
            if matches!(self.fields[index], Field::Tab(_)) {
                finish_group(&mut out, &mut inside_group);
                index += 1;
                continue;
            }
            if !self.on_tab(index, tab) {
                index += 1;
                continue;
            }

            match &self.fields[index] {
                Field::Tab(_) => unreachable!("handled above"),
                Field::Group(_) => {
                    finish_group(&mut out, &mut inside_group);
                    inside_group = true;
                    out.push(Row {
                        fields: vec![index],
                        height: GROUP_CAPTION,
                        labels_above: false,
                        inside_group: false,
                    });
                    index += 1;
                }
                Field::Columns(count) => {
                    // The fields that follow, up to the number asked for or up
                    // to whatever ends the row first.
                    let wanted = usize::from(*count).max(1);
                    let mut together = Vec::new();
                    let mut at = index + 1;
                    while at < self.fields.len() && together.len() < wanted {
                        if matches!(
                            self.fields[at],
                            Field::Tab(_) | Field::Columns(_) | Field::Group(_)
                        ) {
                            break;
                        }
                        together.push(at);
                        at += 1;
                    }
                    if together.is_empty() {
                        index = at.max(index + 1);
                        continue;
                    }
                    // A row of tick boxes needs no label above it: each one
                    // carries its own, and stacking would leave an empty line
                    // over every box.
                    let labels_above =
                        together.iter().any(|at| !matches!(self.fields[*at], Field::Check { .. }));
                    // A row is as tall as a box and its label, unless something
                    // on it is taller than that — a list of rows, a grid — in
                    // which case that is what decides it. Without this a row
                    // holding two lists would be drawn over the buttons under
                    // it, because the panel was measured for boxes.
                    let tallest =
                        together.iter().map(|at| self.fields[*at].height()).fold(0.0f32, f32::max);
                    let height = if labels_above {
                        (LABEL_HEIGHT + BOX_HEIGHT + 10.0).max(tallest)
                    } else {
                        (ROW + 4.0).max(tallest)
                    };
                    out.push(Row { fields: together, height, labels_above, inside_group });
                    index = at;
                }
                field => {
                    out.push(Row {
                        fields: vec![index],
                        height: field.height(),
                        labels_above: false,
                        inside_group,
                    });
                    index += 1;
                }
            }
        }

        finish_group(&mut out, &mut inside_group);
        out
    }

    /// Whether a field belongs to a given tab, or to a dialog with none.
    fn on_tab(&self, index: usize, tab: usize) -> bool {
        self.tabs().is_empty() || self.tab_of(index).is_none_or(|found| found == tab)
    }

    /// The tabs this dialog has, in order.
    fn tabs(&self) -> Vec<&str> {
        self.fields
            .iter()
            .filter_map(|field| match field {
                Field::Tab(label) => Some(label.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Which tab a field is on, counting from the first.
    ///
    /// Everything before the first marker is on every tab: a dialog with a
    /// heading above its tabs is not what Word draws, but a dialog with nothing
    /// before them is the common case and this costs nothing.
    fn tab_of(&self, index: usize) -> Option<usize> {
        let mut tab = None;
        for (at, field) in self.fields.iter().enumerate() {
            if let Field::Tab(_) = field {
                tab = Some(tab.map_or(0, |seen: usize| seen + 1));
            }
            if at == index {
                return tab;
            }
        }
        None
    }

    /// Whether a field is on the tab that is showing.
    fn on_this_tab(&self, index: usize) -> bool {
        self.on_tab(index, self.tab)
    }

    /// Puts the keyboard on one field, if it is one the keyboard can land on.
    pub fn focus_field(&mut self, index: usize) {
        if self.stops().contains(&index) {
            self.focus = index;
        }
    }

    /// Shows one of the tabs, putting the keyboard on its first field.
    pub fn show_tab(&mut self, tab: usize) {
        if tab >= self.tabs().len() || tab == self.tab {
            return;
        }
        self.tab = tab;
        self.focus = self.first_focus();
    }

    /// Draws the dialog over the window, with everything behind it dimmed.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
    ) {
        self.placed.clear();
        let (window_width, window_height) = (canvas.width() as f32, canvas.height() as f32);

        // The document behind is dimmed, which is what says that it cannot be
        // reached until this is answered.
        canvas.fill_rect(0, 0, window_width as i32, window_height as i32, Color::rgba(0, 0, 0, 90));

        let width = self.width;
        let height = self.height();
        let left = ((window_width - width) / 2.0).max(0.0);
        let top = ((window_height - height) / 2.0).max(0.0);

        // A shadow under the panel, so it reads as being in front rather than
        // painted on.
        canvas.fill_rect(
            (left + 4.0) as i32,
            (top + 4.0) as i32,
            width as i32,
            height as i32,
            Color::rgba(0, 0, 0, 60),
        );
        canvas.fill_rect(left as i32, top as i32, width as i32, height as i32, theme.pane);
        outline(canvas, left, top, width, height, theme.pane_edge);

        self.draw_title(canvas, engine, renderer, left, top, width, theme);
        self.draw_tabs(canvas, engine, renderer, left, top, width, theme);
        let after_body = self.draw_body(canvas, engine, renderer, left, top, width, theme);
        self.draw_buttons(canvas, engine, renderer, left, top + height, width, theme);
        let _ = after_body;

        // The open list goes over everything, including the buttons.
        if let Some(index) = self.open_list {
            self.draw_open_list(canvas, engine, renderer, index, theme);
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// The strip of tabs under the caption, for a dialog that has them.
    ///
    /// Word's own: each as wide as its name needs, the one showing drawn as
    /// part of the page below it, the rest set back.
    #[allow(clippy::too_many_arguments)]
    fn draw_tabs(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        top: f32,
        width: f32,
        theme: &Theme,
    ) {
        let labels: Vec<String> = self.tabs().into_iter().map(str::to_owned).collect();
        if labels.is_empty() {
            return;
        }

        let strip_top = top + TITLE_HEIGHT;
        canvas.fill_rect(
            left as i32,
            strip_top as i32,
            width as i32,
            TAB_HEIGHT as i32,
            theme.pane,
        );
        // The line the tabs sit on, broken by whichever of them is showing.
        canvas.fill_rect(
            left as i32,
            (strip_top + TAB_HEIGHT) as i32 - 1,
            width as i32,
            1,
            theme.pane_edge,
        );

        let mut x = left + PADDING;
        for (index, label) in labels.iter().enumerate() {
            let measured = engine.simple_line(label, 0.0, 0.0, 9.0, theme.text).width;
            let tab_width = measured + PADDING * 2.0;
            let showing = index == self.tab;

            if showing {
                canvas.fill_rect(
                    x as i32,
                    strip_top as i32,
                    tab_width as i32,
                    TAB_HEIGHT as i32,
                    theme.pane,
                );
                outline(canvas, x, strip_top, tab_width, TAB_HEIGHT, theme.pane_edge);
                // The bottom edge is rubbed out, so the tab and the page under
                // it read as one surface — which is what a tab is.
                canvas.fill_rect(
                    x as i32 + 1,
                    (strip_top + TAB_HEIGHT) as i32 - 1,
                    tab_width as i32 - 2,
                    1,
                    theme.pane,
                );
            } else if self.hovered == Some(Hit::Tab(index)) {
                canvas.fill_rect(
                    x as i32,
                    strip_top as i32 + 2,
                    tab_width as i32,
                    TAB_HEIGHT as i32 - 3,
                    theme.hover,
                );
            }

            let colour = if showing { theme.text } else { theme.dim_text };
            let line = engine.simple_line(label, x + PADDING, strip_top + 19.0, 9.0, colour);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);

            self.placed.push((Hit::Tab(index), x, strip_top, tab_width, TAB_HEIGHT));
            x += tab_width;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_title(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        top: f32,
        width: f32,
        theme: &Theme,
    ) {
        // A dialog's caption is the same surface as the dialog itself, with a
        // hairline under it — which is how Word's dialogs come up on Windows,
        // where the caption belongs to the system rather than to the ribbon.
        canvas.fill_rect(left as i32, top as i32, width as i32, TITLE_HEIGHT as i32, theme.pane);
        canvas.fill_rect(
            left as i32,
            (top + TITLE_HEIGHT) as i32 - 1,
            width as i32,
            1,
            theme.pane_edge,
        );
        let line = engine.simple_line(&self.title, left + PADDING, top + 22.0, 10.0, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        // The cross at the right-hand end, which every dialog has.
        let close_left = left + width - 28.0;
        if self.hovered == Some(Hit::Close) {
            canvas.fill_rect(close_left as i32 - 4, top as i32 + 6, 24, 22, theme.hover);
        }
        icons::draw_sized(canvas, Icon::Close, close_left, top + 10.0, 14.0, theme.text);
        self.placed.push((Hit::Close, close_left - 4.0, top + 6.0, 24.0, 22.0));
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_body(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        top: f32,
        width: f32,
        theme: &Theme,
    ) -> f32 {
        // The labels that stand beside their field line up in one column, as
        // wide as the widest of them. A fixed column would either waste room
        // or — with a label as long as "Characters (no spaces)" — run into what
        // stands beside it.
        let widest = (0..self.fields.len())
            .filter(|index| self.on_this_tab(*index))
            .filter_map(|index| self.fields[index].label())
            .map(|label| engine.simple_line(label, 0.0, 0.0, 9.0, theme.text).width)
            .fold(0.0f32, f32::max);
        let room = width - PADDING * 2.0;
        let label_width = (widest + PADDING).clamp(130.0, (room - 120.0).max(130.0));

        let strip = if self.tabs().is_empty() { 0.0 } else { TAB_HEIGHT };
        let body_top = top + TITLE_HEIGHT + strip + PADDING;
        let rows = self.rows();

        // The boxes round the groups go on first, so that everything inside
        // them is drawn over the lines rather than under.
        self.draw_group_boxes(canvas, engine, renderer, left, body_top, width, &rows, theme);

        let mut y = body_top;
        for row in &rows {
            if matches!(self.fields.get(row.fields[0]), Some(Field::Group(_))) {
                // The caption was drawn with the box; nothing else goes here.
                y += row.height;
                continue;
            }

            let inset = if row.inside_group { GROUP_INSET } else { 0.0 };
            let row_left = left + PADDING + inset;
            let row_width = width - PADDING * 2.0 - inset * 2.0;

            if row.fields.len() > 1 {
                // Fields side by side: Word's Font, Font style and Size across
                // the top of its Font dialog, and its two columns of tick boxes
                // under Effects.
                let count = row.fields.len() as f32;
                let each = (row_width - COLUMN_GAP * (count - 1.0)) / count;
                for (at, index) in row.fields.iter().enumerate() {
                    let column = row_left + (each + COLUMN_GAP) * at as f32;
                    let place = if row.labels_above {
                        Place {
                            label_x: column,
                            label_y: y + LABEL_HEIGHT - 5.0,
                            box_x: column,
                            box_y: y + LABEL_HEIGHT,
                            box_width: each,
                        }
                    } else {
                        Place {
                            label_x: column,
                            label_y: y + 17.0,
                            box_x: column,
                            box_y: y,
                            box_width: each,
                        }
                    };
                    self.draw_field(canvas, engine, renderer, *index, place, theme);
                }
            } else {
                let index = row.fields[0];
                let place = Place {
                    label_x: row_left,
                    label_y: y + 17.0,
                    box_x: row_left + label_width,
                    box_y: y,
                    box_width: (row_width - label_width).max(60.0),
                };
                self.draw_field(canvas, engine, renderer, index, place, theme);
            }
            y += row.height;
        }
        y
    }

    /// Draws the rectangle round each group, with its caption on the top edge.
    ///
    /// Word's dialogs are made of these, and they are what turns a column of
    /// fields into a dialog somebody can read: "Indentation" round the two
    /// indent boxes says what those two boxes have to do with each other.
    #[allow(clippy::too_many_arguments)]
    fn draw_group_boxes(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        body_top: f32,
        width: f32,
        rows: &[Row],
        theme: &Theme,
    ) {
        let mut y = body_top;
        let mut open: Option<(String, f32)> = None;

        for row in rows {
            let starts_group = match self.fields.get(row.fields[0]) {
                Some(Field::Group(caption)) => Some(caption.clone()),
                _ => None,
            };
            if let Some(caption) = starts_group {
                if let Some((was, from)) = open.take() {
                    self.close_group(canvas, engine, renderer, left, from, y, width, &was, theme);
                }
                open = Some((caption, y));
            }
            y += row.height;
        }
        if let Some((was, from)) = open {
            self.close_group(canvas, engine, renderer, left, from, y, width, &was, theme);
        }
    }

    /// One group's rectangle, once its bottom edge is known.
    #[allow(clippy::too_many_arguments)]
    fn close_group(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        from: f32,
        to: f32,
        width: f32,
        caption: &str,
        theme: &Theme,
    ) {
        let box_left = left + PADDING;
        let box_width = width - PADDING * 2.0;
        // The line sits half-way up the caption, which is what makes the
        // caption read as sitting on the edge rather than inside the box.
        let box_top = from + GROUP_CAPTION / 2.0;
        outline(
            canvas,
            box_left,
            box_top,
            box_width,
            (to - box_top - 4.0).max(1.0),
            theme.pane_edge,
        );

        // The caption, with the line rubbed out behind it.
        let measured = engine.simple_line(caption, 0.0, 0.0, 8.5, theme.text).width;
        let text_left = box_left + PADDING;
        canvas.fill_rect(
            (text_left - 4.0) as i32,
            box_top as i32,
            (measured + 8.0) as i32,
            1,
            theme.pane,
        );
        let line =
            engine.simple_line(caption, text_left, from + GROUP_CAPTION - 3.0, 8.5, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
    }

    /// Draws one field into the room it was given.
    #[allow(clippy::too_many_arguments)]
    fn draw_field(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        index: usize,
        place: Place,
        theme: &Theme,
    ) {
        let Some(field) = self.fields.get(index).cloned() else { return };
        let focused = self.focus == index;
        let Place { label_x, label_y, box_x, box_y, box_width } = place;

        match field {
            // Neither is drawn here: a tab goes along the top, and a group's
            // caption goes on its own rectangle.
            Field::Tab(_) | Field::Group(_) | Field::Columns(_) => {}

            Field::Preview(sample) => {
                // Word's Preview: a box with the text drawn in it as it will be
                // drawn on the page. Not an approximation — the same engine,
                // the same font choice, the same shaping.
                canvas.fill_rect(
                    label_x as i32,
                    box_y as i32,
                    (box_x + box_width - label_x) as i32,
                    PREVIEW_HEIGHT as i32,
                    theme.field,
                );
                outline(
                    canvas,
                    label_x,
                    box_y,
                    box_x + box_width - label_x,
                    PREVIEW_HEIGHT,
                    theme.field_edge,
                );

                // Measured first so it can be centred: a sample pushed against
                // the left edge reads as a mistake.
                let room = box_x + box_width - label_x;
                let measured = engine.sample_line(&sample.text, &sample.properties, 0.0, 0.0).width;
                let start = label_x + ((room - measured) / 2.0).max(6.0);
                let line = engine.sample_line(
                    &sample.text,
                    &sample.properties,
                    start,
                    box_y + PREVIEW_HEIGHT * 0.62,
                );
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
            }

            Field::Grid { label, items, current, scroll } => {
                let line = engine.simple_line(&label, label_x, label_y, 9.0, theme.text);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);

                let room = box_x + box_width - label_x;
                let grid_top = box_y + LABEL_HEIGHT;
                let cell = (room / GRID_COLUMNS as f32).min(GRID_CELL);
                let grid_width = cell * GRID_COLUMNS as f32;
                let grid_height = cell * GRID_ROWS as f32;

                canvas.fill_rect(
                    label_x as i32,
                    grid_top as i32,
                    grid_width as i32,
                    grid_height as i32,
                    theme.field,
                );
                outline(canvas, label_x, grid_top, grid_width, grid_height, theme.field_edge);

                let first = scroll * GRID_COLUMNS;
                for row in 0..GRID_ROWS {
                    for column in 0..GRID_COLUMNS {
                        let at = first + row * GRID_COLUMNS + column;
                        let Some(character) = items.get(at) else { break };
                        let cell_x = label_x + cell * column as f32;
                        let cell_y = grid_top + cell * row as f32;

                        if at == current {
                            canvas.fill_rect(
                                cell_x as i32,
                                cell_y as i32,
                                cell as i32,
                                cell as i32,
                                if focused { theme.accent } else { theme.hover },
                            );
                        }
                        // Centred in its cell, which is the only way a grid of
                        // characters of every width reads as a grid.
                        let mut text = String::new();
                        text.push(*character);
                        let colour =
                            if at == current && focused { theme.on_accent() } else { theme.text };
                        let measured = engine.simple_line(&text, 0.0, 0.0, 11.0, colour).width;
                        let line = engine.simple_line(
                            &text,
                            cell_x + (cell - measured) / 2.0,
                            cell_y + cell * 0.72,
                            11.0,
                            colour,
                        );
                        renderer.draw_within(canvas, &line, cell_x, cell_y, cell, cell);
                    }
                }
                // The lines between the cells, drawn over them so a cell that
                // is picked keeps its own edges.
                for column in 1..GRID_COLUMNS {
                    let at = label_x + cell * column as f32;
                    canvas.fill_rect(
                        at as i32,
                        grid_top as i32,
                        1,
                        grid_height as i32,
                        theme.field_edge,
                    );
                }
                for row in 1..GRID_ROWS {
                    let at = grid_top + cell * row as f32;
                    canvas.fill_rect(
                        label_x as i32,
                        at as i32,
                        grid_width as i32,
                        1,
                        theme.field_edge,
                    );
                }
                self.placed.push((Hit::Field(index), label_x, grid_top, grid_width, grid_height));
            }

            Field::Tree { label, rows, current, scroll } => {
                let line = engine.simple_line(&label, label_x, label_y, 9.0, theme.text);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);

                let room = box_x + box_width - label_x;
                let list_top = box_y + LABEL_HEIGHT;
                let list_height = TREE_ROWS as f32 * PAIR_ROW;
                canvas.fill_rect(
                    label_x as i32,
                    list_top as i32,
                    room as i32,
                    list_height as i32,
                    theme.field,
                );
                outline(canvas, label_x, list_top, room, list_height, theme.field_edge);

                let shown = shown_rows(&rows);
                for showing in 0..TREE_ROWS {
                    let Some(at) = shown.get(scroll + showing).copied() else { break };
                    let row = &rows[at];
                    let row_y = list_top + PAIR_ROW * showing as f32;
                    let picked = at == current;
                    if picked {
                        canvas.fill_rect(
                            (label_x + 1.0) as i32,
                            row_y as i32,
                            (room - 2.0) as i32,
                            PAIR_ROW as i32,
                            if focused { theme.accent } else { theme.hover },
                        );
                    }
                    let colour = if picked && focused { theme.on_accent() } else { theme.text };
                    let mut at_x = label_x + 5.0 + f32::from(row.depth) * TREE_INDENT;
                    if let Some(open) = row.open {
                        draw_fold_mark(canvas, at_x, row_y, open, colour);
                        at_x += FOLD_SIZE;
                    }
                    if let Some(on) = row.tick {
                        let box_top = row_y + (PAIR_ROW - TICK_SIZE) / 2.0;
                        draw_tick_box(canvas, at_x, box_top, TICK_SIZE, on, theme);
                        at_x += TICK_SIZE + 4.0;
                    }
                    let line =
                        engine.simple_line(&row.text, at_x, row_y + PAIR_ROW * 0.72, 9.0, colour);
                    renderer.draw_within(
                        canvas,
                        &line,
                        at_x,
                        row_y,
                        room - (at_x - label_x),
                        PAIR_ROW,
                    );
                }
                self.placed.push((Hit::Field(index), label_x, list_top, room, list_height));
            }

            Field::Pairs { label, second, rows, current, scroll } => {
                let room = box_x + box_width - label_x;
                // The two columns are equal, which is how Word divides them:
                // what is typed is short and what replaces it is not much
                // longer, and a line between them says where one ends.
                let two = !second.is_empty();
                let column = if two { room / 2.0 } else { room };

                let line = engine.simple_line(&label, label_x, label_y, 9.0, theme.text);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
                if two {
                    let line = engine.simple_line(
                        &second,
                        label_x + column + 5.0,
                        label_y,
                        9.0,
                        theme.text,
                    );
                    renderer.draw_onto(canvas, &line, 0.0, 0.0);
                }

                let list_top = box_y + LABEL_HEIGHT;
                let list_height = PAIR_ROWS as f32 * PAIR_ROW;
                canvas.fill_rect(
                    label_x as i32,
                    list_top as i32,
                    room as i32,
                    list_height as i32,
                    theme.field,
                );
                outline(canvas, label_x, list_top, room, list_height, theme.field_edge);

                if two {
                    canvas.fill_rect(
                        (label_x + column) as i32,
                        list_top as i32,
                        1,
                        list_height as i32,
                        theme.field_edge,
                    );
                }

                for showing in 0..PAIR_ROWS {
                    let at = scroll + showing;
                    let Some((what, with)) = rows.get(at) else { break };
                    let row_y = list_top + PAIR_ROW * showing as f32;
                    let picked = at == current;
                    if picked {
                        canvas.fill_rect(
                            (label_x + 1.0) as i32,
                            row_y as i32,
                            (room - 2.0) as i32,
                            PAIR_ROW as i32,
                            if focused { theme.accent } else { theme.hover },
                        );
                    }
                    let colour = if picked && focused { theme.on_accent() } else { theme.text };
                    // Each half is clipped to its own column, so a long
                    // replacement stops at the divider instead of running
                    // across the one beside it.
                    let halves: &[(&String, f32)] =
                        if two { &[(what, 0.0), (with, column)] } else { &[(what, 0.0)] };
                    for (text, from) in halves.iter().copied() {
                        let line = engine.simple_line(
                            text,
                            label_x + from + 5.0,
                            row_y + PAIR_ROW * 0.72,
                            9.0,
                            colour,
                        );
                        renderer.draw_within(
                            canvas,
                            &line,
                            label_x + from,
                            row_y,
                            column - 2.0,
                            PAIR_ROW,
                        );
                    }
                }
                self.placed.push((Hit::Field(index), label_x, list_top, room, list_height));
            }

            Field::Shape(sample) => {
                let room = box_x + box_width - label_x;
                canvas.fill_rect(
                    label_x as i32,
                    box_y as i32,
                    room as i32,
                    SHAPE_HEIGHT as i32,
                    theme.field,
                );
                outline(canvas, label_x, box_y, room, SHAPE_HEIGHT, theme.field_edge);
                draw_paragraph_shape(canvas, &sample.properties, label_x, box_y, room, theme);
            }

            Field::Heading(text) => {
                let line = engine.simple_line(&text, label_x, box_y + 16.0, 9.0, theme.text);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
                canvas.fill_rect(
                    label_x as i32,
                    (box_y + 22.0) as i32,
                    (box_x + box_width - label_x) as i32,
                    1,
                    theme.pane_edge,
                );
            }

            Field::Said { label, value } => {
                let line = engine.simple_line(&label, label_x, box_y + 16.0, 9.0, theme.text);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
                let line = engine.simple_line(&value, box_x, box_y + 16.0, 9.0, theme.text);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
            }

            Field::Check { label, on } => {
                // The box, then the label beside it: a tick box labels itself,
                // which is why it has no label down the left.
                let size = 16.0;
                canvas.fill_rect(
                    label_x as i32,
                    (box_y + 4.0) as i32,
                    size as i32,
                    size as i32,
                    theme.field,
                );
                outline(
                    canvas,
                    label_x,
                    box_y + 4.0,
                    size,
                    size,
                    if focused { theme.accent } else { theme.field_edge },
                );
                if on {
                    canvas.fill_rect(
                        (label_x + 4.0) as i32,
                        (box_y + 8.0) as i32,
                        (size - 8.0) as i32,
                        (size - 8.0) as i32,
                        theme.accent,
                    );
                }
                let line =
                    engine.simple_line(&label, label_x + size + 8.0, box_y + 16.0, 9.0, theme.text);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
                self.placed.push((
                    Hit::Field(index),
                    label_x,
                    box_y,
                    box_x + box_width - label_x,
                    ROW,
                ));
            }

            Field::Text { label, value } | Field::Number { label, value, .. } => {
                let line = engine.simple_line(&label, label_x, label_y, 9.0, theme.text);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
                self.draw_box(canvas, box_x, box_y, box_width, focused, theme);

                let unit = match &self.fields[index] {
                    Field::Number { unit, .. } => *unit,
                    _ => "",
                };
                let shown = with_unit(&value, unit);
                let typed = engine.simple_line(&shown, box_x + 6.0, box_y + 17.0, 9.0, theme.text);
                let measured = typed.width - (box_x + 6.0);
                renderer.draw_onto(canvas, &typed, 0.0, 0.0);
                if focused {
                    let caret = box_x + 6.0 + measured - unit_width(unit, measured, &shown);
                    canvas.fill_rect(caret as i32, (box_y + 5.0) as i32, 1, 14, theme.text);
                }
                self.placed.push((Hit::Field(index), box_x, box_y, box_width, BOX_HEIGHT));
            }

            Field::Choice { label, items, current } => {
                let line = engine.simple_line(&label, label_x, label_y, 9.0, theme.text);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
                self.draw_box(canvas, box_x, box_y, box_width, focused, theme);

                let chosen = items.get(current).cloned().unwrap_or_default();
                // Clipped to the box: a font with a long name must not run out
                // over the field beside it.
                let line = engine.simple_line(&chosen, box_x + 6.0, box_y + 17.0, 9.0, theme.text);
                renderer.draw_within(canvas, &line, box_x, box_y, box_width - 18.0, BOX_HEIGHT);
                chevron(canvas, box_x + box_width - 16.0, box_y + BOX_HEIGHT / 2.0, theme.text);
                self.placed.push((Hit::Field(index), box_x, box_y, box_width, BOX_HEIGHT));
            }
        }
    }

    /// The sunken box a value sits in, which every field that holds one shares.
    fn draw_box(
        &self,
        canvas: &mut Canvas,
        x: f32,
        y: f32,
        width: f32,
        focused: bool,
        theme: &Theme,
    ) {
        canvas.fill_rect(x as i32, y as i32, width as i32, BOX_HEIGHT as i32, theme.field);
        outline(
            canvas,
            x,
            y,
            width,
            BOX_HEIGHT,
            if focused { theme.accent } else { theme.field_edge },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_buttons(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        bottom: f32,
        width: f32,
        theme: &Theme,
    ) {
        let height = 26.0f32;
        let y = bottom - FOOTER_HEIGHT + (FOOTER_HEIGHT - height) / 2.0;
        let mut right = left + width - PADDING;

        // Right to left, so that the first button in the list ends up nearest
        // the right-hand edge — which is where OK goes.
        for index in (0..self.buttons.len()).rev() {
            if !self.button_showing(index) {
                continue;
            }
            let button = self.buttons[index].clone();
            let measured =
                engine.simple_line(&button.label, 0.0, 0.0, 9.0, theme.text).width + 32.0;
            let button_width = measured.max(80.0);
            let button_left = right - button_width;

            let focused = self.focus == self.fields.len() + index;
            let background = if button.default || focused { theme.accent } else { theme.field };
            canvas.fill_rect(
                button_left as i32,
                y as i32,
                button_width as i32,
                height as i32,
                background,
            );
            if focused {
                outline(
                    canvas,
                    button_left - 2.0,
                    y - 2.0,
                    button_width + 4.0,
                    height + 4.0,
                    theme.accent,
                );
            } else if !button.default {
                outline(canvas, button_left, y, button_width, height, theme.field_edge);
            }

            let ink = if button.default || focused { theme.on_accent() } else { theme.text };
            let line = engine.simple_line(&button.label, 0.0, 0.0, 9.0, ink);
            let text_width = line.width;
            let line = engine.simple_line(
                &button.label,
                button_left + (button_width - text_width) / 2.0,
                y + 17.0,
                9.0,
                ink,
            );
            renderer.draw_onto(canvas, &line, 0.0, 0.0);

            self.placed.push((Hit::Button(index), button_left, y, button_width, height));
            right = button_left - 8.0;
        }
    }

    fn draw_open_list(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        index: usize,
        theme: &Theme,
    ) {
        let Some((left, top, width, _)) = self.rect_of(Hit::Field(index)) else { return };
        let Some(Field::Choice { items, current, .. }) = self.fields.get(index).cloned() else {
            return;
        };

        let height = items.len() as f32 * ROW;
        let list_top = top + BOX_HEIGHT;
        canvas.fill_rect(left as i32, list_top as i32, width as i32, height as i32, theme.field);
        outline(canvas, left, list_top, width, height, theme.pane_edge);

        for (row, item) in items.iter().enumerate() {
            let y = list_top + row as f32 * ROW;
            if row == current {
                canvas.fill_rect(left as i32, y as i32, width as i32, ROW as i32, theme.hover);
            }
            let line = engine.simple_line(item, left + 8.0, y + 20.0, 9.0, theme.text);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
        }
    }
}

/// A number with what it is measured in written after it.
///
/// Word writes an inch as `1"`, with the mark against the digit, and a
/// centimetre as `2.54 cm`, with a space. So the space belongs to the word
/// rather than to the number, and a unit that is a mark gets none.
fn with_unit(value: &str, unit: &str) -> String {
    if unit.is_empty() {
        return value.to_owned();
    }
    if unit.chars().all(|character| !character.is_alphabetic()) {
        return format!("{value}{unit}");
    }
    format!("{value} {unit}")
}

/// How much of a measured width is the unit rather than the number.
///
/// The caret goes after the number, not after the unit: a person typing inches
/// is typing the number.
fn unit_width(unit: &str, measured: f32, shown: &str) -> f32 {
    let all = shown.chars().count();
    // Whatever `with_unit` put on the end: the unit, and the space before it
    // when it took one. Asking the same function is what keeps the two from
    // disagreeing.
    let tail = with_unit("", unit).chars().count();
    if tail == 0 || tail >= all {
        return 0.0;
    }
    // Proportional to the characters, which is near enough for a caret.
    measured * tail as f32 / all as f32
}

/// A tick box on a row of a tree.
///
/// Smaller than the one a [`Field::Check`] draws, because it stands inside a
/// row of a list rather than on a row of its own. The box keeps the colours of a
/// tick box wherever it stands, rather than the colours of the row it is on: a
/// tick drawn in the chosen row.s white would vanish into the white box.
/// The triangle before a row that has anything under it: pointing right while
/// what is under it is folded away, and down while it is showing.
///
/// Drawn as a shape rather than an icon, because at seven pixels a triangle is
/// seven rows of rectangles and a drawing read from a file would be a smudge.
fn draw_fold_mark(canvas: &mut Canvas, x: f32, y: f32, open: bool, colour: Color) {
    let size = 7.0f32;
    let left = (x + (FOLD_SIZE - size) / 2.0).round() as i32;
    let top = (y + (PAIR_ROW - size) / 2.0).round() as i32;
    for step in 0..size as i32 {
        if open {
            // Pointing down: a row that narrows as it goes.
            let width = size as i32 - step * 2;
            if width <= 0 {
                break;
            }
            canvas.fill_rect(left + step, top + step, width, 1, colour);
        } else {
            let height = size as i32 - step * 2;
            if height <= 0 {
                break;
            }
            canvas.fill_rect(left + step, top + step, 1, height, colour);
        }
    }
}

fn draw_tick_box(canvas: &mut Canvas, x: f32, y: f32, size: f32, on: bool, theme: &Theme) {
    canvas.fill_rect(x as i32, y as i32, size as i32, size as i32, theme.field);
    outline(canvas, x, y, size, size, theme.field_edge);
    if on {
        canvas.fill_rect(
            (x + 3.0) as i32,
            (y + 3.0) as i32,
            (size - 6.0) as i32,
            (size - 6.0) as i32,
            theme.accent,
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

#[cfg(test)]
mod tests {
    use super::{Answer, Dialog, Field, Reaction};
    use wp_shell::Key;

    fn dialog() -> Dialog {
        Dialog::new(
            "Test",
            vec![
                Field::Heading("A heading".to_owned()),
                Field::Text { label: "Name".to_owned(), value: String::new() },
                Field::Check { label: "Ticked".to_owned(), on: false },
                Field::Choice {
                    label: "Which".to_owned(),
                    items: vec!["First".to_owned(), "Second".to_owned()],
                    current: 0,
                },
            ],
        )
    }

    #[test]
    fn the_keyboard_starts_on_the_first_field_it_can_land_on() {
        // Not the heading, which is not something to fill in.
        let dialog = dialog();
        assert_eq!(dialog.focus, 1);
    }

    #[test]
    fn tab_walks_the_fields_and_then_the_buttons_and_comes_round() {
        let mut dialog = dialog();
        let mut seen = vec![dialog.focus];
        for _ in 0..5 {
            dialog.key(Key::Tab, false, false);
            seen.push(dialog.focus);
        }
        // Three fields, two buttons, and then back to the first field.
        assert_eq!(seen, vec![1, 2, 3, 4, 5, 1]);
    }

    #[test]
    fn shift_and_tab_walk_the_other_way() {
        let mut dialog = dialog();
        dialog.key(Key::Tab, true, false);
        assert_eq!(dialog.focus, 5, "the last button");
        dialog.key(Key::Tab, true, false);
        assert_eq!(dialog.focus, 4);
    }

    #[test]
    fn typing_goes_into_the_box_the_keyboard_is_on() {
        let mut dialog = dialog();
        for character in "Hello".chars() {
            dialog.character(character);
        }
        assert_eq!(dialog.said(1), "Hello");
        dialog.key(Key::Backspace, false, false);
        assert_eq!(dialog.said(1), "Hell");
    }

    #[test]
    fn a_number_box_takes_a_number_and_nothing_else() {
        let mut dialog = Dialog::new(
            "Test",
            vec![Field::Number { label: "Width".to_owned(), value: String::new(), unit: "\"" }],
        );
        for character in "1a2.5.x-".chars() {
            dialog.character(character);
        }
        assert_eq!(dialog.said(0), "12.5");
    }

    #[test]
    fn space_ticks_the_box_the_keyboard_is_on() {
        let mut dialog = dialog();
        dialog.key(Key::Tab, false, false);
        assert_eq!(dialog.focus, 2);
        assert!(!dialog.ticked(2));
        dialog.character(' ');
        assert!(dialog.ticked(2));
        dialog.character(' ');
        assert!(!dialog.ticked(2));
    }

    #[test]
    fn the_arrows_walk_a_list_without_dropping_it_open() {
        let mut dialog = dialog();
        dialog.key(Key::Tab, false, false);
        dialog.key(Key::Tab, false, false);
        assert_eq!(dialog.focus, 3);
        assert_eq!(dialog.chose(3), 0);
        dialog.key(Key::Down, false, false);
        assert_eq!(dialog.chose(3), 1);
        // And stops at the end rather than coming round, as a list does.
        dialog.key(Key::Down, false, false);
        assert_eq!(dialog.chose(3), 1);
        dialog.key(Key::Up, false, false);
        assert_eq!(dialog.chose(3), 0);
    }

    #[test]
    fn enter_presses_the_button_in_bold_and_escape_cancels() {
        let mut dialog = dialog();
        assert_eq!(dialog.key(Key::Enter, false, false), Reaction::Closed(Answer::Accept));
        assert_eq!(dialog.key(Key::Escape, false, false), Reaction::Closed(Answer::Cancel));
    }

    #[test]
    fn enter_on_a_button_presses_that_one_rather_than_the_default() {
        let mut dialog = dialog();
        // Walk to Cancel, which is the second button.
        for _ in 0..4 {
            dialog.key(Key::Tab, false, false);
        }
        assert_eq!(dialog.focus, 5);
        assert_eq!(dialog.key(Key::Enter, false, false), Reaction::Closed(Answer::Cancel));
    }

    #[test]
    fn a_dialog_that_is_telling_rather_than_asking_has_one_button() {
        let dialog = Dialog::message(
            "Word Count",
            vec![Field::Said { label: "Words".to_owned(), value: "417".to_owned() }],
        );
        assert_eq!(dialog.buttons.len(), 1);
        assert!(dialog.buttons[0].default);
    }

    #[test]
    fn a_mark_goes_against_the_number_and_a_word_after_a_space() {
        // Word writes an inch as 1" and a centimetre as 2.54 cm.
        assert_eq!(super::with_unit("1", "\""), "1\"");
        assert_eq!(super::with_unit("2.54", "cm"), "2.54 cm");
        assert_eq!(super::with_unit("3", ""), "3");
    }

    #[test]
    fn the_caret_sits_after_the_number_rather_than_after_the_unit() {
        // Half the width for half the characters: `1"` is two characters, one
        // of which is the mark.
        let width = super::unit_width("\"", 20.0, "1\"");
        assert!((width - 10.0).abs() < 0.001, "{width}");
        // Nothing to skip when the box carries no unit.
        assert_eq!(super::unit_width("", 20.0, "12"), 0.0);
        // Nor when the unit is all there is.
        assert_eq!(super::unit_width("cm", 20.0, " cm"), 0.0);
    }

    #[test]
    fn the_label_column_is_the_one_the_widest_label_needs() {
        // The column is measured from the labels, so a long one cannot run into
        // what stands beside it. Only the fields with a label down the left are
        // counted: a heading spans the panel and a tick box labels itself.
        let dialog = Dialog::message(
            "Word Count",
            vec![
                Field::Heading("Counts".to_owned()),
                Field::Said {
                    label: "Characters (no spaces)".to_owned(),
                    value: "1908".to_owned(),
                },
                Field::Check { label: "Include footnotes".to_owned(), on: false },
            ],
        );
        let labels: Vec<&str> = dialog.fields.iter().filter_map(Field::label).collect();
        assert_eq!(labels, vec!["Characters (no spaces)"]);
    }
}

#[cfg(test)]
mod layout_tests {
    use super::{Answer, Button, Dialog, Field};

    fn number(label: &str) -> Field {
        Field::Number { label: label.to_owned(), value: "0".to_owned(), unit: "pt" }
    }

    fn check(label: &str) -> Field {
        Field::Check { label: label.to_owned(), on: false }
    }

    fn dialog(fields: Vec<Field>) -> Dialog {
        Dialog::with_buttons(
            "Test",
            fields,
            vec![Button { label: "OK".to_owned(), answer: Answer::Accept, default: true }],
        )
    }

    #[test]
    fn fields_told_to_share_a_row_share_one() {
        let one = dialog(vec![number("a"), number("b"), number("c")]);
        let across = dialog(vec![Field::Columns(3), number("a"), number("b"), number("c")]);

        assert_eq!(one.rows().len(), 3, "three fields on three rows");
        assert_eq!(across.rows().len(), 1, "three fields on one row");
        assert_eq!(across.rows()[0].fields.len(), 3);
        // And the panel is shorter for it, which is the point.
        assert!(across.height() < one.height());
    }

    #[test]
    fn a_row_takes_only_as_many_fields_as_it_was_promised() {
        let dialog =
            dialog(vec![Field::Columns(2), number("a"), number("b"), number("c"), number("d")]);
        let rows = dialog.rows();
        assert_eq!(rows.len(), 3, "two together and two alone");
        assert_eq!(rows[0].fields.len(), 2);
        assert_eq!(rows[1].fields.len(), 1);
    }

    #[test]
    fn a_row_of_tick_boxes_needs_no_label_above_it() {
        // Each tick box carries its own label, so stacking would leave an empty
        // line over every one of them.
        let boxes = dialog(vec![Field::Columns(2), check("one"), check("two")]);
        let fields = dialog(vec![Field::Columns(2), number("one"), number("two")]);

        assert!(!boxes.rows()[0].labels_above);
        assert!(fields.rows()[0].labels_above);
        assert!(boxes.rows()[0].height < fields.rows()[0].height);
    }

    #[test]
    fn a_group_holds_what_follows_it_until_the_next_one() {
        let dialog = dialog(vec![
            Field::Group("First".to_owned()),
            number("a"),
            number("b"),
            Field::Group("Second".to_owned()),
            number("c"),
        ]);
        let rows = dialog.rows();

        // A row for each caption, and each field inside the group before it.
        assert_eq!(rows.len(), 5);
        assert!(!rows[0].inside_group, "the caption is not inside its own box");
        assert!(rows[1].inside_group && rows[2].inside_group);
        assert!(!rows[3].inside_group);
        assert!(rows[4].inside_group);
    }

    #[test]
    fn a_new_tab_closes_the_group_the_old_one_left_open() {
        let dialog = dialog(vec![
            Field::Tab("One".to_owned()),
            Field::Group("Boxed".to_owned()),
            number("a"),
            Field::Tab("Two".to_owned()),
            number("b"),
        ]);
        // The second tab's field is not inside the first tab's group.
        let second = dialog.rows_of(1);
        assert_eq!(second.len(), 1);
        assert!(!second[0].inside_group);
    }

    #[test]
    fn the_panel_is_as_tall_as_its_tallest_tab() {
        // A dialog whose height jumped as tabs were clicked would move its own
        // buttons out from under the pointer.
        let dialog = dialog(vec![
            Field::Tab("Short".to_owned()),
            number("a"),
            Field::Tab("Long".to_owned()),
            number("b"),
            number("c"),
            number("d"),
        ]);
        let tall = dialog.height();

        let mut showing_the_long_one = dialog.clone();
        showing_the_long_one.show_tab(1);
        assert_eq!(showing_the_long_one.height(), tall, "the panel changed height with the tab");
    }

    #[test]
    fn the_keyboard_does_not_walk_into_a_marker() {
        // A row marker and a group's caption take places in the list of fields
        // but are not fields anybody can type into.
        let dialog = dialog(vec![
            Field::Group("Boxed".to_owned()),
            Field::Columns(2),
            number("a"),
            number("b"),
        ]);
        assert!(!Field::Columns(2).takes_focus());
        assert!(!Field::Group("Boxed".to_owned()).takes_focus());
        // The keyboard starts on the first thing it can land on, which is the
        // first number rather than either marker.
        assert_eq!(dialog.said(2), "0");
    }
}

/// Draws the shape a paragraph takes, as Word's Paragraph dialog shows it.
///
/// Three paragraphs: the one before, the one being set, and the one after. The
/// middle one is drawn dark and the others faint, so that what is being changed
/// stands out from what it will sit between — which is the whole reason Word
/// shows its neighbours at all. Spacing before and after is only visible as a
/// gap against something, and an indent is only visible against a margin.
fn draw_paragraph_shape(
    canvas: &mut Canvas,
    wanted: &ResolvedParagraphProperties,
    left: f32,
    top: f32,
    width: f32,
    theme: &Theme,
) {
    // A page of the preview is the box less a margin, and everything the
    // paragraph asks for is measured against that.
    let margin = 12.0;
    let page_left = left + margin;
    let page_width = width - margin * 2.0;

    // Twentieths of a point against a page six inches wide, which is what the
    // preview is standing in for.
    let scale = page_width / (6.0 * 1440.0);
    let indent_start = (wanted.indent_start as f32 * scale).clamp(0.0, page_width / 2.0);
    let indent_end = (wanted.indent_end as f32 * scale).clamp(0.0, page_width / 2.0);
    let indent_first = (wanted.indent_first_line as f32 * scale)
        .clamp(-indent_start, page_width / 2.0 - indent_start);

    let bar = 3.0f32;
    // Line spacing, as a proportion of single. Only the multiple rule is worth
    // showing: an exact height in points means nothing at this size.
    let leading = match wanted.line_spacing {
        Some(spacing) if spacing.rule == wp_docx::model::LineRule::Auto => {
            (spacing.value as f32 / 240.0).clamp(0.7, 3.0)
        }
        _ => 1.0,
    };
    let step = bar + 3.0 * leading;
    let before = (wanted.space_before as f32 * 0.02).clamp(0.0, 14.0);
    let after = (wanted.space_after as f32 * 0.02).clamp(0.0, 14.0);

    // A line of the middle paragraph, honouring the indents and the alignment.
    let faint = fade(theme.text, theme.field);
    let mut y = top + 8.0;

    let paragraph =
        |canvas: &mut Canvas, y: &mut f32, lines: usize, colour: Color, of_its_own: bool| {
            for line in 0..lines {
                let first = line == 0;
                let last = line + 1 == lines;
                let (start, end) = if of_its_own {
                    (indent_start + if first { indent_first.max(0.0) } else { 0.0 }, indent_end)
                } else {
                    (0.0, 0.0)
                };
                // A hanging indent pulls the first line out rather than pushing it
                // in, which is the whole of what "hanging" means.
                let start = if of_its_own && first && indent_first < 0.0 {
                    (indent_start + indent_first).max(0.0)
                } else {
                    start
                };

                let room = (page_width - start - end).max(6.0);
                // The last line of a paragraph is short, as the last line of a
                // paragraph is — except in justified text, where it is still short:
                // justification does not stretch it.
                let drawn = if last { room * 0.62 } else { room };
                let x = match wanted.alignment {
                    Alignment::Center => page_left + start + (room - drawn) / 2.0,
                    Alignment::End => page_left + start + (room - drawn),
                    _ => page_left + start,
                };
                canvas.fill_rect(x as i32, *y as i32, drawn as i32, bar as i32, colour);
                *y += step;
            }
        };

    paragraph(canvas, &mut y, 2, faint, false);
    y += before;
    paragraph(canvas, &mut y, 4, theme.text, true);
    y += after;
    paragraph(canvas, &mut y, 2, faint, false);
}

/// A colour mixed most of the way towards the paper.
///
/// For the parts of a preview that are only there to be measured against: the
/// paragraphs either side, which say where this one begins and ends without
/// competing with it for attention.
fn fade(colour: Color, paper: Color) -> Color {
    let mix = |ink: u8, paper: u8| {
        // A quarter of the ink and three quarters of the paper, which is faint
        // enough to read as background on either theme.
        ((u16::from(ink) + u16::from(paper) * 3) / 4) as u8
    };
    Color::rgb(
        mix(colour.red, paper.red),
        mix(colour.green, paper.green),
        mix(colour.blue, paper.blue),
    )
}

/// Checks that a dialog's rows are where the constants naming them say.
///
/// A dialog is built as one list and read back by row number. A row inserted in
/// the middle moves every row after it, and a dialog reading the wrong ones
/// applies the wrong thing without ever looking wrong. Checked when the dialog
/// is built — a handful of comparisons against a click — rather than left to a
/// test, because the cost is nothing and the failure is silent.
///
/// The last row named must be the last row there is, so a row added at the end
/// is caught as well as one added in the middle.
pub fn check_rows(dialog: &str, fields: &[Field], wanted: &[(usize, &str)]) {
    for (row, expected) in wanted {
        let found = fields.get(*row).map_or("nothing", Field::kind_name);
        assert_eq!(found, *expected, "row {row} of the {dialog} dialog is {found}, not {expected}");
    }
    let last = wanted.iter().map(|(row, _)| *row).max().map_or(0, |row| row + 1);
    assert_eq!(fields.len(), last, "the {dialog} dialog has a row nobody named");
}

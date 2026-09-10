//! The lists that drop open from a ribbon field.
//!
//! Drawn by this program like everything else, rather than by a system list
//! control: the window has no controls in it at all, and adding one would mean
//! two ways of drawing and two ways of handling a click.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::Canvas;

use super::icons::{self, Icon};
use super::theme::Theme;

/// Which list is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Font,
    Size,
    Style,
    Zoom,
    Border,
    /// The ready-made headers, footers and page numbers.
    Furniture,
    /// The places a cross-reference could point at.
    Reference,
    /// The sources a citation could name.
    Citation,
    /// The sources, for taking one away.
    Source,
    /// The ready-made watermarks.
    Watermark,
    /// The cover page arrangements.
    Cover,
    /// The categories a table of authorities can gather.
    Authority,
    /// The shapes that can be drawn.
    Shape,
    /// What the text does about a drawing.
    Wrap,
    /// Where a drawing sits across the page.
    Position,
    /// The fields that can be dropped into the text.
    QuickPart,
    /// The looks WordArt comes in.
    WordArt,
    /// The drawings in the document.
    Drawing,
    /// What a mail merge produces.
    MergeKind,
    /// The people a letter is going to.
    Recipient,
    /// The columns their list has.
    MergeField,
    /// What to do about a mistake in the writing.
    Correction,
    /// What would stop somebody reading the document.
    Accessibility,
    /// What the right button offers where it was pressed.
    Context,
    /// What is inside a group of the ribbon that has been given up to one
    /// button.
    Group,
    /// The shadow a theme puts under its shapes.
    ThemeEffects,
    /// The kinds of chart.
    Chart,
    /// The rules a mail merge can carry.
    Rule,
    /// The macros that have been recorded.
    Macro,
    /// The arrangements a diagram can take.
    Diagram,
    /// The printers this machine can reach.
    Printer,
    /// Which pages of the document go to one.
    PrintWhich,
    /// Whether the sheets are printed on both sides.
    PrintSides,
    /// How many pages go on one sheet.
    PrintPerSheet,
    /// The windows a picture can be taken of.
    Screenshot,
    /// How deep the outline goes.
    OutlineLevel,
    /// The parts of an address, and the columns they can come from.
    MatchField,
    MatchColumn,
    /// The looks the letters themselves can be drawn with.
    TextEffect,
    /// The sizes an envelope comes in.
    Envelope,
    /// The sheets of labels that can be printed.
    Label,
    /// What can be changed about a table.
    /// The languages a stretch of text can be marked as.
    Language,
    /// The document's themes, and the two halves of one.
    Theme,
    ThemeColors,
    ThemeFonts,
    /// How the lines down the margin are numbered.
    LineNumbers,
    /// Whether words are broken across lines.
    Hyphenation,
    /// What a reader is allowed to do.
    Protection,
    /// What the strip along the bottom shows.
    StatusBar,
    /// How a section numbers its pages.
    PageNumbering,
    /// The breaks that can be put in: of a page, of a column, and of a section.
    Break,
    /// The margins Word offers by name.
    Margin,
    /// The paper sizes.
    Paper,
    /// Which way round the page is.
    Orientation,
    /// How many columns the text runs down.
    Column,
}

/// The sizes offered, in points.
///
/// Word's own list: close together where text is read and further apart where
/// it is only ever a heading.
pub const SIZES: &[f32] =
    &[8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 28.0, 36.0, 48.0, 72.0];

/// The zooms offered, as percentages.
pub const ZOOMS: &[f32] = &[50.0, 75.0, 100.0, 125.0, 150.0, 200.0, 300.0, 400.0];

const ROW_HEIGHT: f32 = 22.0;
/// How tall the line between two groups of a menu is.
const SEPARATOR_HEIGHT: f32 = 7.0;
/// How much room the icons take at the left of a menu that has any.
const ICON_COLUMN: f32 = 26.0;
/// How many rows a list shows before it has to be scrolled.
const VISIBLE_ROWS: usize = 14;

/// What one row of a list is, beyond the words on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// Something that can be picked.
    #[default]
    Choice,
    /// Names the group under it, the way the two halves of Word's Breaks menu
    /// are named. Cannot be picked.
    Heading,
    /// A line between two groups. Carries no words and cannot be picked.
    Separator,
    /// Something that does not apply just now — Cut with nothing selected.
    /// Shown, so the menu keeps its shape, but grey and unpickable.
    Disabled,
}

/// One row, as far as anything but its text is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    /// The drawing at the left of it, which is what makes a context menu look
    /// like a context menu. [`Icon::None`] leaves the column empty.
    pub icon: Icon,
}

impl Default for Row {
    /// A plain choice with no drawing, which is what every list that says
    /// nothing about its rows is made of.
    fn default() -> Self {
        Self { kind: Kind::Choice, icon: Icon::None }
    }
}

impl Row {
    #[must_use]
    pub fn new(kind: Kind, icon: Icon) -> Self {
        Self { kind, icon }
    }

    /// A line between two groups.
    #[must_use]
    pub fn separator() -> Self {
        Self { kind: Kind::Separator, icon: Icon::None }
    }

    /// A row that names the ones under it.
    #[must_use]
    pub fn heading() -> Self {
        Self { kind: Kind::Heading, icon: Icon::None }
    }

    #[must_use]
    fn pickable(self) -> bool {
        self.kind == Kind::Choice
    }
}

/// A list of choices dropped open under a field.
#[derive(Debug)]
pub struct Popup {
    pub choice: Choice,
    items: Vec<String>,
    /// Which item is in effect now, so it can be marked.
    current: Option<usize>,
    left: f32,
    top: f32,
    width: f32,
    scroll: usize,
    hovered: Option<usize>,
    /// What each row is, where the list is more than a list of words.
    ///
    /// Empty means every row is a plain choice with no drawing on it, which is
    /// what all but the menus are.
    rows: Vec<Row>,
}

impl Popup {
    #[must_use]
    pub fn new(
        choice: Choice,
        items: Vec<String>,
        current: Option<usize>,
        left: f32,
        top: f32,
        width: f32,
    ) -> Self {
        // Opened showing what is in effect, rather than at the top of a list of
        // hundreds where the current font may be nowhere in sight.
        let scroll = current
            .unwrap_or(0)
            .saturating_sub(VISIBLE_ROWS / 2)
            .min(items.len().saturating_sub(VISIBLE_ROWS));
        Self {
            choice,
            items,
            current,
            left,
            top,
            width: width.max(90.0),
            scroll,
            hovered: None,
            rows: Vec::new(),
        }
    }

    /// Says what each row is: a choice, a heading, a line, or something that
    /// does not apply — and what drawing goes on it.
    ///
    /// One entry per item. Anything the list is not told about is a plain
    /// choice with no drawing.
    #[must_use]
    pub fn with_rows(mut self, rows: Vec<Row>) -> Self {
        self.rows = rows;
        self
    }

    /// What a row is.
    #[must_use]
    pub fn row(&self, index: usize) -> Row {
        self.rows.get(index).copied().unwrap_or_default()
    }

    /// Whether the list has any drawings on it, which is what decides whether
    /// the words are indented to leave room for them.
    #[must_use]
    fn has_icons(&self) -> bool {
        self.rows.iter().any(|row| row.icon != Icon::None)
    }

    /// How many rows are showing.
    fn showing(&self) -> usize {
        self.items.len().saturating_sub(self.scroll).min(VISIBLE_ROWS)
    }

    /// How tall one row is: a line between groups is thinner than a choice.
    fn row_height(&self, index: usize) -> f32 {
        if self.row(index).kind == Kind::Separator {
            SEPARATOR_HEIGHT
        } else {
            ROW_HEIGHT
        }
    }

    fn height(&self) -> f32 {
        let showing = self.showing();
        (0..showing).map(|row| self.row_height(self.scroll + row)).sum::<f32>() + 4.0
    }

    /// Whether a point is inside the list at all.
    #[must_use]
    pub fn covers(&self, x: i32, y: i32) -> bool {
        let (x, y) = (x as f32, y as f32);
        x >= self.left
            && x < self.left + self.width
            && y >= self.top
            && y < self.top + self.height()
    }

    /// Which item a point is on, if any.
    ///
    /// Walked rather than divided, because the rows are not all the same
    /// height once a menu has lines between its groups.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        if !self.covers(x, y) {
            return None;
        }
        let wanted = y as f32;
        let mut top = self.top + 2.0;
        for row in 0..self.showing() {
            let index = self.scroll + row;
            let bottom = top + self.row_height(index);
            if wanted < bottom {
                return self.row(index).pickable().then_some(index);
            }
            top = bottom;
        }
        None
    }

    /// Lights up whatever the pointer is over. Returns whether that changed.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let found = self.hit(x, y);
        let changed = found != self.hovered;
        self.hovered = found;
        changed
    }

    /// Moves the highlight up or down the list, skipping whatever cannot be
    /// picked. Returns whether it moved.
    ///
    /// It wraps: past the last entry is the first one again, which is what
    /// every menu does and what makes the down key enough on its own to reach
    /// anything.
    pub fn move_by(&mut self, step: i32) -> bool {
        let count = self.items.len();
        if count == 0 {
            return false;
        }
        // From wherever the highlight is; failing that from whatever is in
        // effect, so the first press lands next to it rather than at the top.
        let from = self.hovered.or(self.current);
        let mut index = match from {
            Some(index) => index as i32,
            None if step > 0 => -1,
            None => count as i32,
        };

        for _ in 0..count {
            index += step;
            if index < 0 {
                index = count as i32 - 1;
            } else if index >= count as i32 {
                index = 0;
            }
            let found = index as usize;
            if self.row(found).pickable() {
                let changed = self.hovered != Some(found);
                self.hovered = Some(found);
                self.show_row(found);
                return changed;
            }
        }
        false
    }

    /// Puts the highlight on the first thing that can be picked, or the last.
    pub fn move_to_end(&mut self, last: bool) -> bool {
        self.hovered = None;
        self.move_by(if last { -1 } else { 1 })
    }

    /// Which row the keyboard is on, if any.
    #[must_use]
    pub fn highlighted(&self) -> Option<usize> {
        self.hovered
    }

    /// Scrolls so that a row is on screen.
    fn show_row(&mut self, index: usize) {
        if index < self.scroll {
            self.scroll = index;
        } else if index >= self.scroll + VISIBLE_ROWS {
            self.scroll = index + 1 - VISIBLE_ROWS;
        }
    }

    /// Scrolls the list. Returns whether it moved.
    pub fn scroll_by(&mut self, rows: i32) -> bool {
        let limit = self.items.len().saturating_sub(VISIBLE_ROWS);
        let wanted = (self.scroll as i32 + rows).clamp(0, limit as i32) as usize;
        let changed = wanted != self.scroll;
        self.scroll = wanted;
        changed
    }

    #[must_use]
    pub fn item(&self, index: usize) -> Option<&str> {
        self.items.get(index).map(String::as_str)
    }

    /// Moves the list back inside the window.
    ///
    /// A menu opened near the foot of the window would hang off the bottom,
    /// where half of it cannot be read and none of it can be pressed. Every
    /// menu on every desktop answers that the same way: it goes above the point
    /// it hangs from instead. Done when the list is drawn, because that is when
    /// how big the window is and how tall the list came out are both known.
    pub fn keep_inside(&mut self, width: f32, top_limit: f32, bottom: f32) {
        let height = self.height();
        if self.top + height > bottom {
            let above = self.top - height;
            self.top = if above >= top_limit { above } else { (bottom - height).max(top_limit) };
        }
        if self.left + self.width > width {
            self.left = (width - self.width).max(0.0);
        }
    }

    pub fn draw(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
    ) {
        let height = self.height();
        canvas.fill_rect(
            self.left as i32 - 1,
            self.top as i32 - 1,
            self.width as i32 + 2,
            height as i32 + 2,
            theme.field_edge,
        );
        canvas.fill_rect(
            self.left as i32,
            self.top as i32,
            self.width as i32,
            height as i32,
            theme.pane,
        );

        // The words are indented past the drawings, so a menu with icons on
        // some of its rows lines up down one edge whether or not every row has
        // one.
        let text_left = self.left + if self.has_icons() { ICON_COLUMN } else { 8.0 };
        let mut y = self.top + 2.0;

        for row in 0..self.showing() {
            let index = self.scroll + row;
            let Some(text) = self.items.get(index) else { break };
            let Row { kind, icon } = self.row(index);
            let row_height = self.row_height(index);

            if kind == Kind::Separator {
                // A line across the middle of the space it is given, kept clear
                // of both edges the way every menu draws one.
                canvas.fill_rect(
                    (self.left + 6.0) as i32,
                    (y + row_height / 2.0) as i32,
                    (self.width - 12.0) as i32,
                    1,
                    theme.field_edge,
                );
                y += row_height;
                continue;
            }

            // Only a choice lights up: a heading and something that does not
            // apply are there to be read, not pressed.
            let background = if kind == Kind::Choice && self.hovered == Some(index) {
                Some(theme.emphasis)
            } else if kind == Kind::Choice && self.current == Some(index) {
                Some(theme.hover)
            } else {
                None
            };
            if let Some(color) = background {
                canvas.fill_rect(
                    self.left as i32,
                    y as i32,
                    self.width as i32,
                    row_height as i32,
                    color,
                );
            }

            let color = match kind {
                Kind::Heading => theme.dim_text,
                Kind::Disabled => theme.disabled_text,
                _ => theme.text,
            };
            if icon != Icon::None {
                icons::draw(
                    canvas,
                    icon,
                    self.left + 3.0,
                    y + (row_height - icons::SIZE) / 2.0,
                    color,
                );
            }
            let line = engine.simple_line(text, text_left, y + 15.0, 9.0, color);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
            y += row_height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A menu of three things with a line across the middle of it.
    fn menu() -> Popup {
        let items =
            ["Cut", "Copy", "", "Paste"].iter().map(|text| (*text).to_owned()).collect::<Vec<_>>();
        Popup::new(Choice::Context, items, None, 100.0, 200.0, 200.0).with_rows(vec![
            Row::new(Kind::Choice, Icon::Scissors),
            Row::new(Kind::Disabled, Icon::Copy),
            Row::new(Kind::Separator, Icon::None),
            Row::new(Kind::Choice, Icon::Clipboard),
        ])
    }

    #[test]
    fn a_row_nothing_was_said_about_is_a_plain_choice() {
        let list = Popup::new(Choice::Font, vec!["Calibri".to_owned()], None, 0.0, 0.0, 100.0);
        assert_eq!(list.row(0), Row::default());
        assert_eq!(list.hit(10, 10), Some(0));
    }

    #[test]
    fn something_that_does_not_apply_cannot_be_picked() {
        let menu = menu();
        assert_eq!(menu.hit(110, 202 + 11), Some(0), "the first entry is not where it should be");
        assert_eq!(menu.hit(110, 202 + 22 + 11), None, "a greyed entry was pickable");
    }

    #[test]
    fn a_line_between_groups_cannot_be_picked() {
        let menu = menu();
        // Two rows of 22, then the thin one.
        assert_eq!(menu.hit(110, 202 + 44 + 3), None);
    }

    #[test]
    fn what_is_under_a_line_is_still_reachable() {
        // The rows are not all the same height, so the one after a line is
        // only found by walking — which is what this proves.
        let menu = menu();
        assert_eq!(menu.hit(110, 202 + 44 + SEPARATOR_HEIGHT as i32 + 11), Some(3));
    }

    #[test]
    fn a_menu_with_no_room_below_goes_above_the_point_instead() {
        let mut menu = menu();
        let height = menu.height();
        menu.keep_inside(1000.0, 100.0, 260.0);
        assert!(menu.top + height <= 260.0, "the menu still hangs off the bottom");
        assert!(menu.top < 200.0, "the menu did not go above the point it was opened at");
    }

    #[test]
    fn a_menu_that_fits_is_left_where_it_was_opened() {
        let mut menu = menu();
        menu.keep_inside(1000.0, 0.0, 900.0);
        assert_eq!((menu.left, menu.top), (100.0, 200.0));
    }

    #[test]
    fn a_menu_off_the_right_edge_is_brought_back() {
        let mut menu = menu();
        menu.keep_inside(250.0, 0.0, 900.0);
        assert_eq!(menu.left, 50.0);
    }

    #[test]
    fn the_down_key_lands_on_the_first_thing_that_can_be_picked() {
        let mut menu = menu();
        assert!(menu.move_by(1));
        assert_eq!(menu.highlighted(), Some(0));
    }

    #[test]
    fn walking_down_steps_over_what_cannot_be_picked() {
        // Cut, then the greyed Copy and the line are both stepped over.
        let mut menu = menu();
        menu.move_by(1);
        menu.move_by(1);
        assert_eq!(menu.highlighted(), Some(3));
    }

    #[test]
    fn walking_past_the_end_comes_back_to_the_beginning() {
        let mut menu = menu();
        menu.move_by(1);
        menu.move_by(1);
        menu.move_by(1);
        assert_eq!(menu.highlighted(), Some(0));
    }

    #[test]
    fn the_up_key_from_nothing_lands_on_the_last_one() {
        let mut menu = menu();
        menu.move_by(-1);
        assert_eq!(menu.highlighted(), Some(3));
    }

    #[test]
    fn home_and_end_go_to_the_two_ends() {
        let mut menu = menu();
        menu.move_to_end(true);
        assert_eq!(menu.highlighted(), Some(3));
        menu.move_to_end(false);
        assert_eq!(menu.highlighted(), Some(0));
    }

    #[test]
    fn the_list_scrolls_to_keep_the_highlight_in_sight() {
        let items: Vec<String> = (0..40).map(|index| format!("Item {index}")).collect();
        let mut list = Popup::new(Choice::Font, items, None, 0.0, 0.0, 100.0);
        for _ in 0..20 {
            list.move_by(1);
        }
        assert_eq!(list.highlighted(), Some(19));
        assert!(list.scroll > 0, "the list did not follow the highlight down");
        assert!(list.scroll <= 19, "{}", list.scroll);
    }

    #[test]
    fn a_list_of_nothing_cannot_be_walked() {
        let mut list = Popup::new(Choice::Font, Vec::new(), None, 0.0, 0.0, 100.0);
        assert!(!list.move_by(1));
        assert_eq!(list.highlighted(), None);
    }
}

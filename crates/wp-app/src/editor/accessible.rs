//! What the window tells a screen reader about itself.
//!
//! Every control drawn on the window is given to the shell as an element with
//! a kind, a name, a place and what pressing it does: the ribbon's tabs, its
//! buttons and its boxes; the panes, with the navigation pane's parts inside
//! it; the strip across the top of the page; the rulers and the scroll bars;
//! the document; the status strip and the message on it; a menu that has
//! dropped open, with its items inside it; a dialog, with its fields, lists
//! and buttons inside it. The document's text is given as text with a
//! selection in it, broken into lines where the layout breaks it, with how
//! each stretch of it is set. The shell puts all of that through the
//! platform's own accessibility tree; see [`wp_shell::accessibility`].
//!
//! Nothing is described here that is not drawn, and each part is described
//! from the same account of it the drawing used — the words on a box, the
//! rows a list is showing, where a dialog put its buttons — since two
//! accounts of one window drift apart, and the one a screen reader's user is
//! given is the one nobody looks at.

use wp_docx::model::Underline;
use wp_docx::TextPosition;
use wp_shell::accessibility::{Element, Role, TextAttributes, TextState};
use wp_shell::Response;

use crate::chrome::dialog::{Field, Part, Reaction};
use crate::chrome::findbar::{self, Hit as StripHit};
use crate::chrome::navigation::{Hit as PaneHit, Navigation, Section};
use crate::chrome::ribbon::{self, Boxed, Tab, TAB_HEIGHT};
use crate::chrome::{
    Choice, Command, HORIZONTAL_HEIGHT, SCROLLBAR_THICKNESS, STATUS_HEIGHT, VERTICAL_WIDTH,
};
use crate::messages::{t, translated};

use super::Editor;

/// The document's element: where the keyboard goes when nothing else has it.
const DOCUMENT: u64 = 1;
/// The ribbon's tabs, by their place in the ribbon's own list.
const TAB_BASE: u64 = 0x1000;
/// The ribbon's commands, by their place in the list of every command.
const COMMAND_BASE: u64 = 0x2000;
/// The status strip's buttons, by the same numbering.
const STATUS_BASE: u64 = 0x3000;
/// The gallery of styles and its tiles, and the buttons a squeezed group
/// becomes, by their place on the tab.
const GALLERY: u64 = 0x3700;
const STYLE_TILE: u64 = 0x3800;
const EXPAND_GROUP: u64 = 0x3C00;
/// The furniture round the page, of which there is one of each.
const ACROSS_RULER: u64 = 0x4001;
const DOWN_RULER: u64 = 0x4002;
const SCROLL_BAR: u64 = 0x4003;
const ACROSS_SCROLL_BAR: u64 = 0x4004;
const STATUS_BAR: u64 = 0x4005;
const STATUS_SUMMARY: u64 = 0x4006;
/// The panes, by which pane: the navigation pane first.
const PANE_BASE: u64 = 0x5000;
const NAVIGATION: u64 = PANE_BASE;
/// The navigation pane's parts.
const NAVIGATION_CLOSE: u64 = 0x5100;
const NAVIGATION_SEARCH: u64 = 0x5101;
const NAVIGATION_LIST: u64 = 0x5102;
const NAVIGATION_TAB: u64 = 0x5110;
/// The strip across the top of the page, and its parts by which part.
const STRIP: u64 = 0x6000;
const STRIP_PART: u64 = 0x6010;
/// A menu that has dropped open.
const MENU: u64 = 0x7000;
/// A dialog, its close button, its tabs, its buttons and its fields.
const DIALOG: u64 = 0x8000;
const DIALOG_CLOSE: u64 = 0x8001;
const DIALOG_TAB: u64 = 0x8100;
const DIALOG_BUTTON: u64 = 0x8200;
const DIALOG_FIELD: u64 = 0x1_0000;
/// The rows of the lists, which can run to thousands: the navigation pane's,
/// a menu's, and a dialog's, numbered by field and by row.
const NAVIGATION_ROW: u64 = 0x100_0000;
const MENU_ITEM: u64 = 0x200_0000;
const DIALOG_ROW: u64 = 0x1_0000_0000;

/// What pressing an element does, and what putting a value into it does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    /// Nothing: it only says something.
    Nothing,
    Tab(Tab),
    Run(Command),
    /// One of the ribbon's measurement boxes.
    Measure(Command),
    /// One of the ribbon's boxes with a list under it.
    List(Command, Choice),
    /// A part of the navigation pane, pressed where it was drawn.
    Pane(i32, i32),
    NavigationSearch,
    /// A part of the strip across the top of the page.
    Strip(i32, i32, StripHit),
    MenuItem(usize),
    /// A part of the dialog, pressed where it was drawn.
    Dialog(i32, i32),
    /// A field of the dialog that holds a value, and where it was drawn.
    DialogField(usize, i32, i32),
    /// A row of one of the dialog's lists.
    DialogRow(usize, usize),
    /// A scroll bar: the one across the bottom, or the other.
    Scroll(bool),
}

impl Editor {
    /// Every control on the window, in reading order: the ribbon, the
    /// panes, the strip, the rulers, the document, the scroll bars, the
    /// status strip, and then whatever is in front of all of them.
    pub(super) fn accessible_elements(&self) -> Vec<Element> {
        self.described().into_iter().map(|(element, _)| element).collect()
    }

    /// Presses a control: a tab opens, a button runs its command, a box
    /// takes the keyboard, a row is chosen.
    pub(super) fn accessible_invoke(&mut self, id: u64) -> Response {
        let Some(act) = self.act_of(id) else { return Response::Ignored };
        match act {
            Act::Nothing | Act::Scroll(_) => Response::Ignored,
            Act::Tab(tab) => self.choose_tab(tab),
            Act::Run(command) | Act::List(command, _) => self.run(command),
            Act::Measure(command) => self.type_in_box(command),
            Act::Pane(x, y) => {
                let (top, bottom) = (self.ribbon_bottom(), self.window_bottom());
                self.pressed_in_pane(x, y, top, bottom)
            }
            Act::NavigationSearch => {
                self.navigation.searching = true;
                self.needs_redraw = true;
                Response::Redraw
            }
            Act::Strip(x, y, _) => self.pressed_in_find(x, y),
            Act::MenuItem(index) => self.choose(index),
            Act::Dialog(x, y) | Act::DialogField(_, x, y) => self.dialog_press(x, y),
            Act::DialogRow(field, row) => {
                if self.dialog.as_mut().is_some_and(|dialog| dialog.press_row(field, row)) {
                    self.reacted(Reaction::Changed)
                } else {
                    Response::Ignored
                }
            }
        }
    }

    /// Puts a value into a control that holds one, as though it had been
    /// typed there: a box's measurement, a font's name, a search, a field
    /// of a dialog, how far a scroll bar has run.
    pub(super) fn accessible_set_value(&mut self, id: u64, value: &str) -> Response {
        let Some(act) = self.act_of(id) else { return Response::Ignored };
        match act {
            Act::Measure(command) => {
                if self.type_in_box(command) == Response::Ignored {
                    return Response::Ignored;
                }
                value.clone_into(&mut self.box_text);
                self.ribbon_box = Some((command, false));
                self.finish_box()
            }
            Act::List(_, choice) => self.write_list(choice, value),
            Act::NavigationSearch => {
                value.clone_into(&mut self.navigation.search);
                self.navigation.section = Section::Results;
                self.navigation.searching = true;
                self.needs_redraw = true;
                Response::Redraw
            }
            Act::Strip(_, _, hit @ (StripHit::FindField | StripHit::ReplaceField)) => {
                self.write_find(hit == StripHit::ReplaceField, value)
            }
            Act::DialogField(index, ..) => {
                let Some(dialog) = &mut self.dialog else { return Response::Ignored };
                let chosen = match dialog.fields.get(index) {
                    Some(Field::Choice { items, .. }) => {
                        items.iter().position(|item| translated(item) == value || item == value)
                    }
                    _ => None,
                };
                let changed = match chosen {
                    Some(row) => dialog.set_row(index, row),
                    None => dialog.set_text(index, value),
                };
                if changed {
                    self.reacted(Reaction::Changed)
                } else {
                    Response::Ignored
                }
            }
            Act::Scroll(across) => {
                let Ok(wanted) = value.trim().parse::<f32>() else { return Response::Ignored };
                let bar = if across { self.across_bar } else { self.scrollbar };
                let Some(bar) = bar else { return Response::Ignored };
                let amount = wanted.clamp(0.0, bar.limit) - bar.position;
                if across {
                    self.scroll_across_by(amount)
                } else {
                    self.scroll_by(amount)
                }
            }
            _ => Response::Ignored,
        }
    }

    /// Chooses from one of the ribbon's boxes with a list by name, as
    /// choosing that name from the list would.
    fn write_list(&mut self, choice: Choice, value: &str) -> Response {
        let value = value.trim();
        match choice {
            Choice::Font if !value.is_empty() => {
                let changed = self.document.set_font(value);
                self.finish_character_change(changed, &format!("Font {value}"))
            }
            Choice::Size => match value.trim_end_matches("pt").trim().parse::<f32>() {
                Ok(points) if points > 0.0 => {
                    let changed = self.document.set_size(points);
                    self.finish_character_change(changed, &format!("{value} point"))
                }
                _ => self.report("Not a size"),
            },
            Choice::Style => {
                let found = self
                    .style_gallery()
                    .into_iter()
                    .find(|sample| sample.name.eq_ignore_ascii_case(value));
                match found {
                    Some(sample) => self.apply_style(sample.id.as_deref()),
                    None => Response::Ignored,
                }
            }
            Choice::Zoom => match value.trim_end_matches('%').trim().parse::<f32>() {
                Ok(zoom) if zoom > 0.0 => self.set_zoom(zoom),
                _ => Response::Ignored,
            },
            _ => Response::Ignored,
        }
    }

    /// What an element does, found by describing the window again: an
    /// element is only there to be pressed while it is there to be seen.
    fn act_of(&self, id: u64) -> Option<Act> {
        self.described().into_iter().find(|(element, _)| element.id == id).map(|(_, act)| act)
    }

    /// Every element, with what it does.
    fn described(&self) -> Vec<(Element, Act)> {
        let mut out = Vec::new();
        let backstage = self.in_backstage();
        let furniture = self.view.shows_furniture() && !backstage;
        let state = self.toolbar_state();
        if furniture {
            self.describe_ribbon(&state, &mut out);
        }
        if !backstage {
            self.describe_panes(&mut out);
            self.describe_strip(&mut out);
            if self.show_rulers {
                self.describe_rulers(&mut out);
            }
        }
        // The document: the page area, which is where the keyboard goes
        // unless a box or a dialog has taken it.
        let top = self.content_top() as i32;
        let bottom = self.content_bottom() as i32;
        out.push((
            Element {
                id: DOCUMENT,
                role: Role::Document,
                name: self.document_name(),
                rect: (0, top, self.view_width as i32, (bottom - top).max(0)),
                enabled: !self.is_locked(),
                ..Element::default()
            },
            Act::Nothing,
        ));
        if !backstage {
            self.describe_scroll_bars(&mut out);
        }
        if furniture {
            self.describe_status(&mut out);
        }
        self.describe_menu(&mut out);
        self.describe_dialog(&mut out);

        // One element has the keyboard, and it is told so; the rest are
        // told they have not.
        let focus = self.focus_id();
        for (element, _) in &mut out {
            element.focused = element.id == focus;
        }
        out
    }

    /// Which element has the keyboard: whatever is in front, else a box
    /// being typed into, else the document.
    fn focus_id(&self) -> u64 {
        if let Some(dialog) = &self.dialog {
            return match dialog.focused_part() {
                Part::Field(index) => dialog
                    .list_rows(index)
                    .iter()
                    .find(|row| row.chosen)
                    .map_or(DIALOG_FIELD + index as u64, |row| dialog_row(index, row.index)),
                Part::Button(index) => DIALOG_BUTTON + index as u64,
                Part::Tab(_) | Part::Close => DIALOG,
            };
        }
        if let Some(popup) = &self.popup {
            return popup.highlighted().map_or(MENU, |index| MENU_ITEM + index as u64);
        }
        if let Some((command, _)) = self.ribbon_box {
            return command_id(command).unwrap_or(DOCUMENT);
        }
        if let Some(bar) = &self.find_bar {
            let hit = match bar.focus {
                findbar::Focus::Find => StripHit::FindField,
                findbar::Focus::Replace => StripHit::ReplaceField,
            };
            return STRIP_PART + strip_index(hit);
        }
        if self.show_navigation && self.recovery.is_none() && self.navigation.searching {
            return NAVIGATION_SEARCH;
        }
        DOCUMENT
    }

    /// The tabs, and the open tab's buttons and boxes.
    fn describe_ribbon(&self, state: &crate::chrome::ToolbarState, out: &mut Vec<(Element, Act)>) {
        let strip_top = self.ribbon.strip_top();
        for (tab, left, width) in self.ribbon.tab_places() {
            let Some(index) = tab_index(tab) else { continue };
            out.push((
                Element {
                    id: TAB_BASE + index,
                    role: Role::TabItem,
                    name: t(tab.label()).to_owned(),
                    access_key: super::keytips::tab_key_tip(tab),
                    rect: on_window((left, strip_top, width, TAB_HEIGHT)),
                    selected: tab == self.ribbon.tab,
                    enabled: true,
                    ..Element::default()
                },
                Act::Tab(tab),
            ));
        }
        let places = self.ribbon.command_places();
        // The gallery of styles is a list of them, the one in effect chosen,
        // and a group squeezed down to one button is a button called what
        // the group is called.
        let tiles: Vec<(f32, f32, f32, f32)> = places
            .iter()
            .filter(|(command, ..)| matches!(command, Command::Style(_)))
            .map(|(_, left, top, width, height)| (*left, *top, *width, *height))
            .collect();
        let gallery = if tiles.is_empty() { Vec::new() } else { self.style_gallery() };
        if let Some(first) = tiles.first() {
            let right =
                tiles.iter().map(|(left, _, width, _)| left + width).fold(first.0, f32::max);
            let bottom =
                tiles.iter().map(|(_, top, _, height)| top + height).fold(first.1, f32::max);
            out.push((
                Element {
                    id: GALLERY,
                    role: Role::List,
                    name: t("Styles").to_owned(),
                    rect: on_window((first.0, first.1, right - first.0, bottom - first.1)),
                    enabled: true,
                    ..Element::default()
                },
                Act::Nothing,
            ));
        }
        let groups = self.ribbon.groups();
        for (command, left, top, width, height) in places {
            let rect = on_window((left, top, width, height));
            match command {
                Command::Style(index) => {
                    let sample = gallery.get(index);
                    out.push((
                        Element {
                            id: STYLE_TILE + index as u64,
                            role: Role::ListItem,
                            // The paragraph's style is said by identifier, as
                            // the tile's is; the tile's name is only shown.
                            selected: sample.is_some_and(|sample| sample.id == state.style),
                            name: sample.map(|sample| sample.name.clone()).unwrap_or_default(),
                            rect,
                            enabled: true,
                            parent: Some(GALLERY),
                            ..Element::default()
                        },
                        Act::Run(command),
                    ));
                    continue;
                }
                Command::ExpandGroup(index) => {
                    out.push((
                        Element {
                            id: EXPAND_GROUP + u64::from(index),
                            role: Role::Button,
                            name: groups
                                .get(usize::from(index))
                                .map(|group| t(group.label).to_owned())
                                .unwrap_or_default(),
                            rect,
                            enabled: true,
                            ..Element::default()
                        },
                        Act::Run(command),
                    ));
                    continue;
                }
                _ => {}
            }
            let Some(id) = command_id(command) else { continue };
            let element = Element {
                id,
                name: command_name(command),
                access_key: command_keys(command),
                rect,
                // The same answer the eye is given: a button drawn grey is a
                // button a screen reader must call unavailable, or the two
                // accounts of the window disagree.
                enabled: crate::chrome::is_enabled(command, state),
                ..Element::default()
            };
            out.push(match ribbon::box_of(command) {
                // A box is a box: what is in it is its value, and it can be
                // typed into or chosen from without pressing it first.
                Some(Boxed::Measure) => (
                    Element { role: Role::Edit, value: state.measure(command), ..element },
                    Act::Measure(command),
                ),
                Some(Boxed::Field(choice)) => (
                    Element {
                        role: Role::ComboBox,
                        value: ribbon::field_text(choice, state),
                        ..element
                    },
                    Act::List(command, choice),
                ),
                None => {
                    let on = toggled(command, state);
                    (
                        Element {
                            role: if on.is_some() { Role::Toggle } else { Role::Button },
                            selected: on.unwrap_or(false),
                            ..element
                        },
                        Act::Run(command),
                    )
                }
            });
        }
    }

    /// The panes that are open: the one on the left with its parts, the
    /// ones on the right by name.
    fn describe_panes(&self, out: &mut Vec<(Element, Act)>) {
        let (top, bottom) = (self.ribbon_bottom(), self.window_bottom());
        let height = bottom - top;
        let pane = |id: u64, name: &str, rect: (f32, f32, f32, f32)| {
            (
                Element {
                    id,
                    role: Role::Pane,
                    name: name.to_owned(),
                    rect: on_window(rect),
                    enabled: true,
                    ..Element::default()
                },
                Act::Nothing,
            )
        };
        if let Some(recovery) = &self.recovery {
            let rect = (0.0, top, recovery.width(), height);
            out.push(pane(PANE_BASE + 1, t("Document Recovery"), rect));
        } else if self.show_navigation {
            out.push(pane(
                NAVIGATION,
                t("Navigation"),
                (0.0, top, self.navigation.width(), height),
            ));
            self.describe_navigation(top, bottom, out);
        }
        let width = self.view_width as f32;
        let right = [
            (self.show_styles, t("Styles"), self.styles_pane_left()),
            (self.show_restrict, t("Restrict Editing"), self.restrict_pane_left()),
            (self.show_signatures, t("Signatures"), self.signature_pane_left()),
            (self.show_mapping, t("XML Mapping"), self.mapping_pane_left()),
            (self.show_text_pane, t("Text Pane"), self.text_pane_left()),
            (self.show_translator, t("Translator"), self.translator_left()),
        ];
        for (which, (shown, name, left)) in right.into_iter().enumerate() {
            if shown {
                out.push(pane(
                    PANE_BASE + 2 + which as u64,
                    name,
                    (left, top, width - left, height),
                ));
            }
        }
        if self.comparing.is_some() {
            let left = width - super::comparing::WIDTH - SCROLLBAR_THICKNESS;
            let top = self.whole_content_top();
            let rect = (left, top, super::comparing::WIDTH, bottom - top);
            out.push(pane(PANE_BASE + 2 + right.len() as u64, t("Compare"), rect));
        }
    }

    /// The navigation pane's close button, its tabs, its search box, and
    /// the list its open tab shows, row by row.
    fn describe_navigation(&self, top: f32, bottom: f32, out: &mut Vec<(Element, Act)>) {
        let contents = self.pane_contents();
        let section = self.navigation.section;
        let (count, current) = match section {
            Section::Headings => {
                (contents.headings.len(), self.current_heading(&contents.headings))
            }
            Section::Pages => (contents.pages, contents.current_page.checked_sub(1)),
            Section::Results => (contents.found.len(), None),
            Section::Comments => (contents.notes.len(), None),
        };
        let row_name = |index: usize| -> String {
            match section {
                Section::Headings => contents.headings[index].text.clone(),
                Section::Pages => crate::messages::with("Page {0}", &[&(index + 1).to_string()]),
                Section::Results => contents.found[index].context.clone(),
                Section::Comments => {
                    let note = &contents.notes[index];
                    format!("{}: {}", note.author, note.text)
                }
            }
        };
        let list_top = Navigation::list_top(top);
        out.push((
            Element {
                id: NAVIGATION_LIST,
                role: Role::List,
                name: t(section.label()).to_owned(),
                rect: on_window((0.0, list_top, self.navigation.width(), bottom - list_top)),
                enabled: true,
                parent: Some(NAVIGATION),
                ..Element::default()
            },
            Act::Nothing,
        ));
        for (hit, (x, y, width, height)) in self.navigation.places(top, count) {
            // Already turned about with the window: the pane says where it
            // drew its parts in the window's own terms.
            let rect = (x as i32, y as i32, width as i32, height as i32);
            let press = Act::Pane((x + width / 2.0) as i32, (y + height / 2.0) as i32);
            let element =
                Element { rect, enabled: true, parent: Some(NAVIGATION), ..Element::default() };
            out.push(match hit {
                PaneHit::Close => (
                    Element {
                        id: NAVIGATION_CLOSE,
                        role: Role::Button,
                        name: t("Close").to_owned(),
                        ..element
                    },
                    press,
                ),
                PaneHit::Tab(shown) => (
                    Element {
                        id: NAVIGATION_TAB + section_index(shown),
                        role: Role::TabItem,
                        name: t(shown.label()).to_owned(),
                        selected: shown == section,
                        ..element
                    },
                    press,
                ),
                PaneHit::SearchBox => (
                    Element {
                        id: NAVIGATION_SEARCH,
                        role: Role::Edit,
                        name: t("Search document").to_owned(),
                        value: self.navigation.search.clone(),
                        ..element
                    },
                    Act::NavigationSearch,
                ),
                PaneHit::Row(index) => (
                    Element {
                        id: NAVIGATION_ROW + index as u64,
                        role: Role::ListItem,
                        name: row_name(index),
                        selected: current == Some(index),
                        parent: Some(NAVIGATION_LIST),
                        ..element
                    },
                    press,
                ),
            });
        }
    }

    /// The strip across the top of the page — find, or whatever else it
    /// was opened to be typed into — with its fields and its buttons.
    fn describe_strip(&self, out: &mut Vec<(Element, Act)>) {
        let Some(bar) = &self.find_bar else { return };
        let top = self.ribbon_bottom() + self.info_bar_height();
        let left = self.pane_width();
        out.push((
            Element {
                id: STRIP,
                role: Role::Pane,
                name: t(bar.field_label()).to_owned(),
                rect: on_window((left, top, self.view_width as f32 - left, findbar::HEIGHT)),
                enabled: true,
                ..Element::default()
            },
            Act::Nothing,
        ));
        for (hit, name, place) in bar.parts() {
            let rect = on_window(place);
            let press = Act::Strip(rect.0 + rect.2 / 2, rect.1 + rect.3 / 2, hit);
            let element = Element {
                id: STRIP_PART + strip_index(hit),
                name: t(name).to_owned(),
                rect,
                enabled: true,
                parent: Some(STRIP),
                ..Element::default()
            };
            let element = match hit {
                StripHit::FindField => {
                    Element { role: Role::Edit, value: bar.needle.clone(), ..element }
                }
                StripHit::ReplaceField => {
                    Element { role: Role::Edit, value: bar.replacement.clone(), ..element }
                }
                StripHit::MatchCase => {
                    Element { role: Role::Toggle, selected: bar.match_case, ..element }
                }
                StripHit::WholeWord => {
                    Element { role: Role::Toggle, selected: bar.whole_word, ..element }
                }
                StripHit::FindNext
                | StripHit::FindPrevious
                | StripHit::Replace
                | StripHit::ReplaceAll
                | StripHit::Close
                | StripHit::Add => Element { role: Role::Button, ..element },
            };
            out.push((element, press));
        }
    }

    /// The ruler across the top of the page and the one down its side.
    fn describe_rulers(&self, out: &mut Vec<(Element, Act)>) {
        let left = self.pane_width();
        let ruler = |id: u64, name: &str, rect: (f32, f32, f32, f32)| {
            (
                Element {
                    id,
                    role: Role::Ruler,
                    name: name.to_owned(),
                    rect: on_window(rect),
                    enabled: true,
                    ..Element::default()
                },
                Act::Nothing,
            )
        };
        let across = (left, self.ruler_top(), self.view_width as f32 - left, HORIZONTAL_HEIGHT);
        out.push(ruler(ACROSS_RULER, t("Horizontal Ruler"), across));
        // Word shows no side ruler in web layout, where a page has no height.
        if self.view != super::views::View::Web {
            let top = self.whole_content_top();
            let down = (left, top, VERTICAL_WIDTH, self.window_bottom() - top);
            out.push(ruler(DOWN_RULER, t("Vertical Ruler"), down));
        }
    }

    /// The scroll bars, each with how far it has run and how far it may.
    fn describe_scroll_bars(&self, out: &mut Vec<(Element, Act)>) {
        for (bar, id, across) in
            [(self.scrollbar, SCROLL_BAR, false), (self.across_bar, ACROSS_SCROLL_BAR, true)]
        {
            let Some(bar) = bar else { continue };
            let (rect, name) = if bar.vertical {
                ((bar.left, bar.top, SCROLLBAR_THICKNESS, bar.length), t("Vertical Scroll Bar"))
            } else {
                ((bar.left, bar.top, bar.length, SCROLLBAR_THICKNESS), t("Horizontal Scroll Bar"))
            };
            out.push((
                Element {
                    id,
                    role: Role::ScrollBar,
                    name: name.to_owned(),
                    rect: on_window(rect),
                    enabled: true,
                    range: Some((0.0, bar.limit, bar.position)),
                    ..Element::default()
                },
                Act::Scroll(across),
            ));
        }
    }

    /// The status strip: the message on it as its value, what it says of
    /// the document, and its buttons.
    fn describe_status(&self, out: &mut Vec<(Element, Act)>) {
        let top = self.view_height as f32 - STATUS_HEIGHT;
        let rect = on_window((0.0, top, self.view_width as f32, STATUS_HEIGHT));
        // What the last command said, or where the link at the caret goes:
        // the same note the strip shows.
        let note = if self.status.is_empty() {
            self.link_note().unwrap_or_default()
        } else {
            self.status.clone()
        };
        out.push((
            Element {
                id: STATUS_BAR,
                role: Role::StatusBar,
                name: t("Status Bar").to_owned(),
                rect,
                enabled: true,
                value: translated(&note),
                ..Element::default()
            },
            Act::Nothing,
        ));
        if !self.reader_status.is_empty() {
            out.push((
                Element {
                    id: STATUS_SUMMARY,
                    role: Role::Text,
                    name: self.reader_status.join(", "),
                    rect,
                    enabled: true,
                    parent: Some(STATUS_BAR),
                    ..Element::default()
                },
                Act::Nothing,
            ));
        }
        for (command, left, top) in &self.status_buttons {
            // A button switched off is put where nothing can reach it, and
            // is not there to be told of either.
            let Some(index) = command_index(*command).filter(|_| *left >= 0.0) else { continue };
            out.push((
                Element {
                    id: STATUS_BASE + index,
                    role: Role::Button,
                    name: command_name(*command),
                    rect: on_window((*left, *top, 16.0, 16.0)),
                    enabled: true,
                    parent: Some(STATUS_BAR),
                    ..Element::default()
                },
                Act::Run(*command),
            ));
        }
    }

    /// A list that has dropped open, with the rows it is showing.
    fn describe_menu(&self, out: &mut Vec<(Element, Act)>) {
        let Some(popup) = &self.popup else { return };
        // Called what the box or button it dropped from is called; the menu
        // a right click opens is Word's context menu.
        let name = if popup.choice == Choice::Context {
            t("Context Menu").to_owned()
        } else {
            ribbon::command_of(popup.choice)
                .or_else(|| ribbon::field_of(popup.choice))
                .map(command_name)
                .unwrap_or_default()
        };
        out.push((
            Element {
                id: MENU,
                role: Role::Menu,
                name,
                rect: on_window(popup.frame()),
                enabled: true,
                ..Element::default()
            },
            Act::Nothing,
        ));
        for (index, place) in popup.row_places() {
            out.push((
                Element {
                    id: MENU_ITEM + index as u64,
                    role: Role::MenuItem,
                    name: popup.item(index).map(translated).unwrap_or_default(),
                    rect: on_window(place),
                    selected: popup.current() == Some(index),
                    enabled: true,
                    parent: Some(MENU),
                    ..Element::default()
                },
                Act::MenuItem(index),
            ));
        }
    }

    /// A dialog, with everything it shows inside it.
    fn describe_dialog(&self, out: &mut Vec<(Element, Act)>) {
        let Some(dialog) = &self.dialog else { return };
        out.push((
            Element {
                id: DIALOG,
                role: Role::Dialog,
                name: translated(&dialog.title),
                rect: on_window(dialog.frame()),
                enabled: true,
                ..Element::default()
            },
            Act::Nothing,
        ));
        let (tabs, showing) = dialog.tab_names();
        for (part, place) in dialog.parts() {
            let rect = on_window(place);
            let (x, y) = (rect.0 + rect.2 / 2, rect.1 + rect.3 / 2);
            let element =
                Element { rect, enabled: true, parent: Some(DIALOG), ..Element::default() };
            match part {
                Part::Close => out.push((
                    Element {
                        id: DIALOG_CLOSE,
                        role: Role::Button,
                        name: t("Close").to_owned(),
                        ..element
                    },
                    Act::Dialog(x, y),
                )),
                Part::Tab(index) => out.push((
                    Element {
                        id: DIALOG_TAB + index as u64,
                        role: Role::TabItem,
                        name: tabs.get(index).map(|name| translated(name)).unwrap_or_default(),
                        selected: index == showing,
                        ..element
                    },
                    Act::Dialog(x, y),
                )),
                Part::Button(index) => out.push((
                    Element {
                        id: DIALOG_BUTTON + index as u64,
                        role: Role::Button,
                        name: dialog
                            .buttons
                            .get(index)
                            .map(|button| translated(&button.label))
                            .unwrap_or_default(),
                        ..element
                    },
                    Act::Dialog(x, y),
                )),
                Part::Field(index) => self.describe_field(dialog, index, element, out),
            }
        }
    }

    /// One field of a dialog, and the rows of it where it is a list.
    fn describe_field(
        &self,
        dialog: &crate::chrome::dialog::Dialog,
        index: usize,
        element: Element,
        out: &mut Vec<(Element, Act)>,
    ) {
        let rect = element.rect;
        let press = Act::DialogField(index, rect.0 + rect.2 / 2, rect.1 + rect.3 / 2);
        let element = Element { id: DIALOG_FIELD + index as u64, ..element };
        let Some(field) = dialog.fields.get(index) else { return };
        let described = match field {
            Field::Text { label, value } | Field::Number { label, value, .. } => Element {
                role: Role::Edit,
                name: translated(label),
                value: value.clone(),
                ..element
            },
            // What is typed into a password box is not read out: the box
            // says how much is in it, as it shows the eye.
            Field::Secret { label, value } => Element {
                role: Role::Edit,
                name: translated(label),
                value: "•".repeat(value.chars().count()),
                ..element
            },
            Field::Check { label, on } => {
                Element { role: Role::CheckBox, name: translated(label), selected: *on, ..element }
            }
            Field::Choice { label, items, current } => Element {
                role: Role::ComboBox,
                name: translated(label),
                value: items.get(*current).map(|item| translated(item)).unwrap_or_default(),
                ..element
            },
            Field::Tree { label, .. } | Field::Pairs { label, .. } | Field::Grid { label, .. } => {
                let list = Element { role: Role::List, name: translated(label), ..element };
                let parent = list.id;
                out.push((list, Act::Nothing));
                for row in dialog.list_rows(index) {
                    out.push((
                        Element {
                            id: dialog_row(index, row.index),
                            // A row with a tick box is the tick box: pressing
                            // it turns it on or off.
                            role: if row.tick.is_some() { Role::CheckBox } else { Role::ListItem },
                            name: translated(&row.text),
                            rect: on_window(row.rect),
                            selected: row.tick.unwrap_or(row.chosen),
                            enabled: true,
                            parent: Some(parent),
                            ..Element::default()
                        },
                        Act::DialogRow(index, row.index),
                    ));
                }
                return;
            }
            // A box of lines to read, each line a row of it.
            Field::Lines { label, lines, scroll, .. } => {
                let list = Element { role: Role::List, name: translated(label), ..element };
                let parent = list.id;
                let (left, top, width, height) = list.rect;
                let each = height / crate::chrome::dialog::LINES_SHOWN as i32;
                out.push((list, Act::Nothing));
                let shown = lines.iter().enumerate().skip(*scroll);
                for (place, (at, line)) in
                    shown.take(crate::chrome::dialog::LINES_SHOWN).enumerate()
                {
                    out.push((
                        Element {
                            id: dialog_row(index, at),
                            role: Role::ListItem,
                            name: line.clone(),
                            rect: (left, top + each * place as i32, width, each),
                            enabled: true,
                            parent: Some(parent),
                            ..Element::default()
                        },
                        Act::Nothing,
                    ));
                }
                return;
            }
            Field::Said { label, value } => {
                let name = format!("{} {}", translated(label), value).trim().to_owned();
                out.push((Element { role: Role::Text, name, ..element }, Act::Nothing));
                return;
            }
            Field::Heading(text) | Field::Group(text) => {
                let name = translated(text);
                out.push((Element { role: Role::Text, name, ..element }, Act::Nothing));
                return;
            }
            // The pictures — a preview, the shape of a paragraph, the
            // columns — say nothing a screen reader could read.
            Field::Tab(_) | Field::Preview(_) | Field::Shape(_) | Field::Columns(_) => return,
        };
        out.push((described, press));
    }

    /// The document's text, one line break between paragraphs, with the
    /// selection as character offsets into it.
    pub(super) fn accessible_text(&self) -> TextState {
        let mut text = String::new();
        for index in 0..self.document.paragraph_count() {
            if index > 0 {
                text.push('\n');
            }
            text.push_str(&self.document.paragraph_text(index).unwrap_or_default());
        }
        let caret = self.document.caret();
        let (start, end) = self.document.selection().unwrap_or((caret, caret));
        TextState { text, selection: (self.offset_of(start), self.offset_of(end)) }
    }

    /// The document's lines as the layout broke them, each as the offsets
    /// it starts and ends at; a line runs up to the next, so the line break
    /// at a paragraph's end belongs to its last line.
    pub(super) fn accessible_lines(&self) -> Vec<(usize, usize)> {
        let count = self.document.paragraph_count();
        let texts: Vec<String> = (0..count)
            .map(|index| self.document.paragraph_text(index).unwrap_or_default())
            .collect();
        let mut starts = Vec::with_capacity(count);
        let mut total = 0;
        for text in &texts {
            starts.push(total);
            total += text.chars().count() + 1;
        }
        let length = total.saturating_sub(1);
        let at = |paragraph: usize, byte: usize| {
            let text = &texts[paragraph];
            let mut byte = byte.min(text.len());
            while !text.is_char_boundary(byte) {
                byte -= 1;
            }
            starts[paragraph] + text[..byte].chars().count()
        };
        let mut breaks: Vec<usize> = self
            .pages
            .iter()
            .flat_map(|page| page.lines.iter())
            .filter(|line| line.paragraph < count)
            .map(|line| at(line.paragraph, line.start_offset))
            .collect();
        breaks.push(0);
        breaks.sort_unstable();
        breaks.dedup();
        breaks
            .iter()
            .enumerate()
            .map(|(index, start)| (*start, breaks.get(index + 1).copied().unwrap_or(length)))
            .collect()
    }

    /// How the text at an offset is set, and the stretch round it that is
    /// set the same — the run it is in, with the line break after the run
    /// that ends a paragraph.
    pub(super) fn accessible_attributes(
        &self,
        offset: usize,
    ) -> Option<(TextAttributes, usize, usize)> {
        let at = self.position_of(offset);
        let (properties, from, to) = self.document.formatting_at(at);
        let start = self.offset_of(TextPosition::new(at.paragraph, from));
        let mut end = self.offset_of(TextPosition::new(at.paragraph, to));
        let length = self.document.paragraph_text(at.paragraph).map_or(0, |text| text.len());
        if to >= length && at.paragraph + 1 < self.document.paragraph_count() {
            end += 1;
        }
        let rgb = |colour: wp_raster::Color| (colour.red, colour.green, colour.blue);
        let attributes = TextAttributes {
            font: properties.font.clone().unwrap_or_default(),
            size: properties.size_half_points as f32 / 2.0,
            bold: properties.bold,
            italic: properties.italic,
            underline: properties.underline != Underline::None,
            strike: properties.strike || properties.double_strike,
            color: properties.color.as_deref().and_then(wp_raster::Color::from_hex).map(rgb),
            background: properties
                .highlight
                .as_deref()
                .and_then(crate::chrome::palette::highlight_color)
                .map(rgb),
        };
        Some((attributes, start, end.max(start)))
    }

    /// Selects a stretch given as character offsets into that text.
    pub(super) fn accessible_select(&mut self, start: usize, end: usize) -> Response {
        let (from, to) = (self.position_of(start), self.position_of(end));
        self.document.set_caret(from);
        if to != from {
            self.document.extend_selection_to(to);
        } else {
            self.document.clear_selection();
        }
        self.reveal_caret();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Where a stretch of the text is on the window, a rectangle per line.
    pub(super) fn accessible_rects(&self, start: usize, end: usize) -> Vec<(i32, i32, i32, i32)> {
        let (from, to) = (self.position_of(start), self.position_of(end.max(start)));
        let mut rects = Vec::new();
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            if top > self.view_height as f32 || top + self.pages[index].height < 0.0 {
                continue;
            }
            for (x, y, width, height) in self.pages[index].selection_rects(from, to) {
                rects.push((
                    (origin_x + x) as i32,
                    (top + y) as i32,
                    width.ceil() as i32,
                    height.ceil() as i32,
                ));
            }
        }
        rects
    }

    /// A place in the document as a character offset into its text.
    fn offset_of(&self, at: TextPosition) -> usize {
        let mut offset = 0;
        for index in 0..at.paragraph.min(self.document.paragraph_count()) {
            offset +=
                self.document.paragraph_text(index).map_or(0, |text| text.chars().count()) + 1;
        }
        let text = self.document.paragraph_text(at.paragraph).unwrap_or_default();
        offset + text[..at.offset.min(text.len())].chars().count()
    }

    /// A character offset into the text as a place in the document.
    fn position_of(&self, offset: usize) -> TextPosition {
        let mut remaining = offset;
        let count = self.document.paragraph_count();
        for index in 0..count {
            let text = self.document.paragraph_text(index).unwrap_or_default();
            let length = text.chars().count();
            if remaining <= length {
                let byte = text.char_indices().nth(remaining).map_or(text.len(), |(byte, _)| byte);
                return TextPosition::new(index, byte);
            }
            remaining -= length + 1;
        }
        let last = count.saturating_sub(1);
        let end = self.document.paragraph_text(last).map_or(0, |text| text.len());
        TextPosition::new(last, end)
    }

    /// Tells the shell when the selection has moved since it last drew, so
    /// a screen reader can read what the caret is on now.
    pub(super) fn note_selection_for_reader(&mut self) {
        let caret = self.document.caret();
        let now = (caret, self.document.selection());
        if self.reader_selection != Some(now) {
            self.reader_selection = Some(now);
            wp_shell::selection_changed();
        }
    }
}

/// A place the chrome was drawn, in the window's own pixels: turned about
/// with the window where the interface reads right to left, since that is
/// where the eye finds it and where a press on it lands.
fn on_window((left, top, width, height): (f32, f32, f32, f32)) -> (i32, i32, i32, i32) {
    let flipped =
        crate::chrome::mirror::flip_f(left + width).min(crate::chrome::mirror::flip_f(left));
    (flipped as i32, top as i32, width as i32, height as i32)
}

/// A row of one of a dialog's lists.
fn dialog_row(field: usize, row: usize) -> u64 {
    DIALOG_ROW + ((field as u64) << 24) + row as u64
}

/// What a command is called: what its tip says, which is what Word's
/// screen reader says — "Bold", not the B on the button — or, failing
/// that, what the ribbon calls it; in the language the window is in. The
/// two boxes with a list and the arrows in the groups' corners, which
/// carry no label of their own, are called what Word calls the boxes and
/// what the group the arrow is in is called.
fn command_name(command: Command) -> String {
    known_name(command).unwrap_or_else(|| format!("{command:?}"))
}

/// The name a command is given somewhere a person reads it, if it is.
fn known_name(command: Command) -> Option<String> {
    match command {
        Command::ChooseFont => return Some(t("Font").to_owned()),
        Command::ChooseSize => return Some(t("Font Size").to_owned()),
        _ => {}
    }
    crate::chrome::tip::label_of(command)
        .or_else(|| ribbon::name_of(command))
        .or_else(|| ribbon::launcher_of(command))
        .map(translated)
}

/// A command's element on the ribbon: its place in the list of every
/// command there is a button for.
fn command_id(command: Command) -> Option<u64> {
    command_index(command).map(|index| COMMAND_BASE + index)
}

/// The keys that do what the button does, for the screen reader to say.
fn command_keys(command: Command) -> String {
    crate::chrome::tip::shortcut_of(command).unwrap_or_default().to_owned()
}

/// Where a command is in the list of every command, which is its number.
fn command_index(command: Command) -> Option<u64> {
    ribbon::every_command().iter().position(|found| *found == command).map(|index| index as u64)
}

/// Whether a command is one that is on or off, and which it is: the ones
/// whose buttons show as pressed while they are in effect.
fn toggled(command: Command, state: &crate::chrome::ToolbarState) -> Option<bool> {
    let toggles = matches!(
        command,
        Command::Format(_)
            | Command::Subscript
            | Command::Superscript
            | Command::Align(_)
            | Command::FormatPainter
            | Command::ShowMarks
            | Command::ToggleRulers
            | Command::ToggleNavigation
            | Command::ToggleTheme
            | Command::Gridlines
            | Command::JoinPages
            | Command::TrackChanges
            | Command::TableHeaderRow
            | Command::TableTotalRow
            | Command::TableFirstColumn
            | Command::TableLastColumn
            | Command::TableBandedRows
            | Command::TableBandedColumns
            | Command::ViewGridlines
            | Command::RepeatHeaderRow
            | Command::AlignCell(_)
            | Command::ShowMarkup
            | Command::BorderPainter
            | Command::DrawTable
            | Command::Eraser
            | Command::ShowProofing
            | Command::ReviewingPane
            | Command::ShowComments
            | Command::DesignMode
            | Command::OutlineShowFormatting
            | Command::OutlineFirstLineOnly
    );
    toggles.then(|| crate::chrome::is_active(command, state))
}

fn tab_index(tab: Tab) -> Option<u64> {
    Tab::ALL.iter().chain(Tab::CONTEXTUAL.iter()).position(|found| *found == tab).map(|i| i as u64)
}

fn section_index(section: Section) -> u64 {
    Section::ALL.iter().position(|found| *found == section).unwrap_or(0) as u64
}

/// Which part of the strip, as a number that stays the same while the
/// strip grows a field or loses one.
fn strip_index(hit: StripHit) -> u64 {
    match hit {
        StripHit::FindField => 0,
        StripHit::ReplaceField => 1,
        StripHit::FindNext => 2,
        StripHit::FindPrevious => 3,
        StripHit::Replace => 4,
        StripHit::ReplaceAll => 5,
        StripHit::MatchCase => 6,
        StripHit::WholeWord => 7,
        StripHit::Close => 8,
        StripHit::Add => 9,
    }
}

#[cfg(test)]
mod tests {
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use super::*;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor(paragraphs: &[&str]) -> Editor {
        let mut body = Body::default();
        for text in paragraphs {
            body.blocks.push(Block::Paragraph(Paragraph::text(text)));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.draw(1400, 900);
        editor
    }

    fn find<'a>(elements: &'a [Element], role: Role, name: &str) -> &'a Element {
        elements
            .iter()
            .find(|element| element.role == role && element.name == name)
            .unwrap_or_else(|| panic!("no {role:?} called {name}: {elements:#?}"))
    }

    #[test]
    fn the_window_describes_its_tabs_buttons_and_document() {
        let editor = editor(&["Hello"]);
        let elements = editor.accessible_elements();
        let home = find(&elements, Role::TabItem, "Home");
        assert!(home.selected);
        assert_eq!(home.access_key, "H");
        let bold = find(&elements, Role::Toggle, "Bold");
        assert!(!bold.selected);
        assert!(bold.rect.2 > 0 && bold.rect.3 > 0);
        let document = elements.iter().find(|e| e.role == Role::Document).expect("the document");
        assert!(document.focused);
        assert_eq!(document.id, DOCUMENT);
        // Every element has a name and a place, and no two share an id.
        assert!(elements.iter().all(|e| !e.name.is_empty()), "{elements:#?}");
        let mut ids: Vec<u64> = elements.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), elements.len());
        // Exactly one thing has the keyboard.
        assert_eq!(elements.iter().filter(|e| e.focused).count(), 1);
    }

    /// Every button and box on every tab is told of, and by a name a person
    /// would use: nothing on the ribbon is left out for want of a number,
    /// and nothing is read out as the program's own word for it.
    #[test]
    fn every_control_on_every_tab_is_told_of_by_its_name() {
        let mut editor = editor(&["Hello"]);
        let (mut unnamed, mut unnumbered) = (Vec::new(), Vec::new());
        for tab in Tab::ALL.iter().copied().filter(|tab| *tab != Tab::File) {
            editor.choose_tab(tab);
            editor.draw(1400, 900);
            let elements = editor.accessible_elements();
            for (command, ..) in editor.ribbon.command_places() {
                // The gallery's tiles and a squeezed group's button are named
                // by the style and the group.
                if let Command::Style(_) | Command::ExpandGroup(_) = command {
                    let named = elements.iter().filter(|e| e.role != Role::TabItem).any(|e| {
                        !e.name.is_empty()
                            && matches!(e.role, Role::ListItem | Role::Button)
                            && (STYLE_TILE..EXPAND_GROUP + 0x100).contains(&e.id)
                    });
                    assert!(named, "{command:?} on {tab:?} is not told of by name");
                    continue;
                }
                let id = command_id(command);
                if id.is_none() {
                    unnumbered.push(command);
                    continue;
                }
                assert!(elements.iter().any(|e| Some(e.id) == id), "{command:?} is not told of");
                if known_name(command).is_none() {
                    unnamed.push(command);
                }
            }
        }
        assert!(unnumbered.is_empty(), "no element for {unnumbered:?}");
        assert!(unnamed.is_empty(), "no name for {unnamed:?}");
    }

    #[test]
    fn pressing_an_element_does_what_the_button_does() {
        let mut editor = editor(&["Hello"]);
        let elements = editor.accessible_elements();
        let insert = find(&elements, Role::TabItem, "Insert");
        editor.accessible_invoke(insert.id);
        assert_eq!(editor.ribbon.tab, Tab::Insert);
        editor.accessible_invoke(TAB_BASE + tab_index(Tab::Home).unwrap());
        assert_eq!(editor.ribbon.tab, Tab::Home);
        // The buttons are where they were last drawn.
        editor.draw(1400, 900);
        let elements = editor.accessible_elements();
        let bold = find(&elements, Role::Toggle, "Bold");
        editor.document.select_all();
        editor.accessible_invoke(bold.id);
        editor.document.set_caret(TextPosition::new(0, 1));
        assert!(editor.document.character_format_here().bold, "Bold was not applied");
        let elements = editor.accessible_elements();
        let bold = find(&elements, Role::Toggle, "Bold");
        assert!(bold.selected, "the toggle does not show it is on");
    }

    #[test]
    fn the_text_is_read_with_its_selection_and_places_map_both_ways() {
        let mut editor = editor(&["One two", "Three"]);
        editor.document.set_caret(TextPosition::new(1, 0));
        editor.document.extend_selection_to(TextPosition::new(1, 3));
        let state = editor.accessible_text();
        assert_eq!(state.text, "One two\nThree");
        assert_eq!(state.selection, (8, 11));
        assert_eq!(editor.position_of(8), TextPosition::new(1, 0));
        assert_eq!(editor.position_of(4), TextPosition::new(0, 4));
        assert_eq!(editor.position_of(99), TextPosition::new(1, 5));
        editor.accessible_select(4, 7);
        assert_eq!(editor.document.selected_text(), "two");
        assert!(!editor.accessible_rects(0, 3).is_empty(), "no rectangle for the first word");
    }

    #[test]
    fn the_ribbon_boxes_are_boxes_with_what_is_in_them() {
        let mut editor = editor(&["Hello"]);
        let elements = editor.accessible_elements();
        let font = find(&elements, Role::ComboBox, "Font");
        assert!(!font.value.is_empty(), "the font box says nothing");
        let size = find(&elements, Role::ComboBox, "Font Size");
        assert_eq!(size.value, ribbon::field_text(Choice::Size, &editor.toolbar_state()));

        // A size written into the box is the size chosen from its list.
        editor.document.select_all();
        editor.accessible_set_value(size.id, "18");
        editor.document.set_caret(TextPosition::new(0, 1));
        assert!((editor.document.size_here() - 18.0).abs() < 0.01);

        // And a measurement typed into one of the Layout tab's boxes.
        editor.choose_tab(Tab::Layout);
        editor.draw(1400, 900);
        let elements = editor.accessible_elements();
        let indent = elements
            .iter()
            .find(|e| e.role == Role::Edit && Some(e.id) == command_id(Command::IndentLeftBox))
            .expect("the left indent box");
        // In the unit the box shows, as a number typed into it would be.
        editor.accessible_set_value(indent.id, "2");
        let indent_twips = editor.document.indents_here().0;
        assert!(indent_twips > 0, "the indent was not set");
        let elements = editor.accessible_elements();
        let again = elements.iter().find(|e| e.id == indent.id).expect("the box again");
        assert!(again.value.starts_with('2'), "the box says {}", again.value);
        assert_ne!(again.value, indent.value);
    }

    #[test]
    fn a_dialog_is_described_with_its_fields_and_buttons_inside_it() {
        let mut editor = editor(&["Hello"]);
        editor.run(Command::FontDialog);
        editor.draw(1400, 900);
        let elements = editor.accessible_elements();
        let dialog = elements.iter().find(|e| e.role == Role::Dialog).expect("the dialog");
        assert_eq!(dialog.name, "Font");
        let inside: Vec<&Element> = elements.iter().filter(|e| e.parent == Some(DIALOG)).collect();
        assert!(inside.iter().any(|e| e.role == Role::Button && e.name == "OK"), "{inside:#?}");
        assert!(inside.iter().any(|e| e.role == Role::TabItem && e.selected), "{inside:#?}");
        // The keyboard is in the dialog, not in the document behind it.
        let document = elements.iter().find(|e| e.role == Role::Document).unwrap();
        assert!(!document.focused);
        let focused: Vec<&Element> = elements.iter().filter(|e| e.focused).collect();
        assert_eq!(focused.len(), 1, "{focused:#?}");
        assert!(
            focused[0].parent == Some(DIALOG) || focused[0].id >= DIALOG_ROW,
            "the keyboard is on {focused:#?}"
        );
        // Pressing OK shuts it.
        let ok = inside.iter().find(|e| e.name == "OK").unwrap().id;
        editor.accessible_invoke(ok);
        assert!(editor.dialog.is_none(), "OK did not shut the dialog");
    }

    #[test]
    fn a_dialog_list_is_a_list_of_rows_and_a_tick_box_can_be_ticked() {
        let mut editor = editor(&["Hello"]);
        editor.run(Command::WordCount);
        editor.draw(1400, 900);
        let elements = editor.accessible_elements();
        let tick = elements.iter().find(|e| e.role == Role::CheckBox).expect("a tick box");
        let was = tick.selected;
        editor.accessible_invoke(tick.id);
        editor.draw(1400, 900);
        let elements = editor.accessible_elements();
        let again = elements.iter().find(|e| e.id == tick.id).expect("the tick box again");
        assert_ne!(again.selected, was, "pressing the tick box did not turn it");
    }

    #[test]
    fn a_menu_is_described_with_its_items_and_choosing_one_applies_it() {
        let mut editor = editor(&["Hello"]);
        let elements = editor.accessible_elements();
        let size = find(&elements, Role::ComboBox, "Font Size");
        editor.accessible_invoke(size.id);
        editor.draw(1400, 900);
        let elements = editor.accessible_elements();
        let menu = elements.iter().find(|e| e.role == Role::Menu).expect("the list dropped");
        assert_eq!(menu.name, "Font Size");
        let items: Vec<&Element> = elements.iter().filter(|e| e.parent == Some(MENU)).collect();
        assert!(items.iter().all(|e| e.role == Role::MenuItem));
        assert!(items.iter().any(|e| e.selected), "the size in effect is not marked");
        let twenty = items.iter().find(|e| e.name == "20").expect("a size of 20");
        editor.document.select_all();
        editor.accessible_invoke(twenty.id);
        assert!(editor.popup.is_none());
        editor.document.set_caret(TextPosition::new(0, 1));
        assert!((editor.document.size_here() - 20.0).abs() < 0.01);
    }

    #[test]
    fn the_furniture_is_described_rulers_scroll_bar_status_and_panes() {
        let paragraphs: Vec<String> = (0..80).map(|index| format!("Line {index}")).collect();
        let paragraphs: Vec<&str> = paragraphs.iter().map(String::as_str).collect();
        let mut editor = editor(&paragraphs);
        editor.show_rulers = true;
        editor.show_navigation = true;
        editor.status = "Saved".to_owned();
        editor.draw(1400, 900);
        let elements = editor.accessible_elements();
        assert!(elements.iter().any(|e| e.role == Role::Ruler && e.id == ACROSS_RULER));
        let bar = elements.iter().find(|e| e.role == Role::ScrollBar).expect("a scroll bar");
        let (least, most, now) = bar.range.expect("the bar's range");
        assert!(least == 0.0 && most > 0.0 && now == 0.0, "{:?}", bar.range);
        let status = elements.iter().find(|e| e.role == Role::StatusBar).expect("the strip");
        assert_eq!(status.value, "Saved");
        let summary = elements.iter().find(|e| e.id == STATUS_SUMMARY).expect("what it says");
        assert!(summary.name.contains("Page 1 of"), "{}", summary.name);
        let pane = find(&elements, Role::Pane, "Navigation");
        assert!(elements.iter().any(|e| e.parent == Some(pane.id) && e.role == Role::Edit));

        // Writing into the bar scrolls.
        editor.accessible_set_value(bar.id, &format!("{}", most / 2.0));
        assert!((editor.scroll - most / 2.0).abs() < 1.0, "scrolled to {}", editor.scroll);

        // And a search written into the pane's box finds.
        let search = elements.iter().find(|e| e.id == NAVIGATION_SEARCH).unwrap();
        editor.accessible_set_value(search.id, "Line 7");
        editor.draw(1400, 900);
        let elements = editor.accessible_elements();
        let rows: Vec<&Element> =
            elements.iter().filter(|e| e.parent == Some(NAVIGATION_LIST)).collect();
        assert!(rows.iter().any(|e| e.name.contains("Line 7")), "{rows:#?}");
    }

    #[test]
    fn lines_are_where_the_layout_breaks_them() {
        let long = "word ".repeat(60);
        let mut editor = editor(&[long.trim_end(), "Short"]);
        editor.draw(1400, 900);
        let lines = editor.accessible_lines();
        let text = editor.accessible_text().text;
        assert!(lines.len() >= 3, "the long paragraph was not broken: {lines:?}");
        assert_eq!(lines[0].0, 0);
        // The lines run on from one another and cover the whole text.
        for pair in lines.windows(2) {
            assert_eq!(pair[0].1, pair[1].0, "{lines:?}");
        }
        assert_eq!(lines.last().unwrap().1, text.chars().count());
        let short = text.chars().count() - "Short".len();
        assert!(lines.contains(&(short, text.chars().count())), "{lines:?}");
    }

    #[test]
    fn the_attributes_of_a_run_are_its_formatting_and_its_extent() {
        let mut editor = editor(&["Plain bold plain"]);
        editor.document.set_caret(TextPosition::new(0, 6));
        editor.document.extend_selection_to(TextPosition::new(0, 10));
        editor.run(Command::Format(wp_docx::CharacterFormat::Bold));
        let (attributes, start, end) = editor.accessible_attributes(7).expect("attributes");
        assert!(attributes.bold);
        assert_eq!((start, end), (6, 10));
        assert!(attributes.size > 0.0);
        let (attributes, start, _) = editor.accessible_attributes(2).expect("attributes");
        assert!(!attributes.bold);
        assert_eq!(start, 0);
    }
}

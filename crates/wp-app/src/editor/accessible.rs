//! What the window tells a screen reader about itself.
//!
//! Every control drawn on the window — the ribbon's tabs and buttons, the
//! document, the buttons on the status strip — is given to the shell as an
//! element with a kind, a name, a place and what pressing it does, and the
//! document's text is given as text with a selection in it. The shell puts
//! that through the platform's own accessibility tree; see
//! [`wp_shell::accessibility`]. Nothing is described here that is not
//! drawn, and nothing that is drawn is left out, since a control a screen
//! reader cannot reach is a control its user does not have.

use wp_docx::TextPosition;
use wp_shell::accessibility::{Element, Role, TextState};
use wp_shell::Response;

use crate::chrome::ribbon::{self, Tab, TAB_HEIGHT};
use crate::chrome::Command;

use super::Editor;

/// The document's element: the one thing the keyboard goes to.
const DOCUMENT: u64 = 1;
/// The ribbon's tabs, by their place in the ribbon's own list.
const TAB_BASE: u64 = 0x1000;
/// The ribbon's commands, by their place in the list of every command.
const COMMAND_BASE: u64 = 0x2000;
/// The status strip's buttons, by the same numbering.
const STATUS_BASE: u64 = 0x3000;

impl Editor {
    /// Every control on the window, in reading order: the tabs, the open
    /// tab's buttons, the document, the status strip.
    pub(super) fn accessible_elements(&self) -> Vec<Element> {
        let mut elements = Vec::new();
        let furniture = self.view.shows_furniture() && !self.in_backstage();
        let commands = ribbon::all_commands();
        let command_index = |command: Command| -> Option<u64> {
            commands.iter().position(|(found, _)| *found == command).map(|index| index as u64)
        };
        let state = self.toolbar_state();
        if furniture {
            let strip_top = self.ribbon.strip_top();
            for (tab, left, width) in self.ribbon.tab_places() {
                let Some(index) = tab_index(tab) else { continue };
                elements.push(Element {
                    id: TAB_BASE + index,
                    role: Role::TabItem,
                    name: tab.label().to_owned(),
                    access_key: tab.key_tip().to_owned(),
                    rect: (left as i32, strip_top as i32, width as i32, TAB_HEIGHT as i32),
                    selected: tab == self.ribbon.tab,
                    enabled: true,
                    focused: false,
                });
            }
            for (command, left, top, width, height) in self.ribbon.command_places() {
                let Some(index) = command_index(command) else { continue };
                let on = toggled(command, &state);
                elements.push(Element {
                    id: COMMAND_BASE + index,
                    role: if on.is_some() { Role::Toggle } else { Role::Button },
                    name: command_name(command),
                    access_key: command_keys(command),
                    rect: (left as i32, top as i32, width as i32, height as i32),
                    selected: on.unwrap_or(false),
                    enabled: true,
                    focused: false,
                });
            }
        }
        // The document: the page area, which is where the keyboard goes
        // unless a box or a dialog has taken it.
        let top = self.content_top() as i32;
        let bottom = self.content_bottom() as i32;
        elements.push(Element {
            id: DOCUMENT,
            role: Role::Document,
            name: self.document_name(),
            access_key: String::new(),
            rect: (0, top, self.view_width as i32, (bottom - top).max(0)),
            selected: false,
            enabled: !self.is_locked(),
            focused: !self.in_dialog() && !self.typing_in_box() && !self.find_has_keyboard(),
        });
        if furniture {
            for (command, left, top) in &self.status_buttons {
                let Some(index) = command_index(*command) else { continue };
                elements.push(Element {
                    id: STATUS_BASE + index,
                    role: Role::Button,
                    name: command_name(*command),
                    access_key: String::new(),
                    rect: (*left as i32, *top as i32, 16, 16),
                    selected: false,
                    enabled: true,
                    focused: false,
                });
            }
        }
        elements
    }

    /// Presses a control: a tab opens, a button runs its command.
    pub(super) fn accessible_invoke(&mut self, id: u64) -> Response {
        if let Some(index) = id.checked_sub(TAB_BASE).filter(|_| id < COMMAND_BASE) {
            let Some(tab) = tab_at(index) else { return Response::Ignored };
            return self.choose_tab(tab);
        }
        let commands = ribbon::all_commands();
        let index = if id >= STATUS_BASE {
            id - STATUS_BASE
        } else if id >= COMMAND_BASE {
            id - COMMAND_BASE
        } else {
            return Response::Ignored;
        };
        let Some((command, _)) = commands.get(index as usize) else { return Response::Ignored };
        self.run(*command)
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

/// What a command is called: what its tip says, which is what Word's
/// screen reader says — "Bold", not the B on the button — or, failing
/// that, what the ribbon calls it.
fn command_name(command: Command) -> String {
    crate::chrome::tip::label_of(command)
        .or_else(|| ribbon::name_of(command))
        .map_or_else(|| format!("{command:?}"), str::to_owned)
}

/// The keys that do what the button does, for the screen reader to say.
fn command_keys(command: Command) -> String {
    crate::chrome::tip::shortcut_of(command).unwrap_or_default().to_owned()
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
    );
    toggles.then(|| crate::chrome::is_active(command, state))
}

fn tab_index(tab: Tab) -> Option<u64> {
    Tab::ALL.iter().chain(Tab::CONTEXTUAL.iter()).position(|found| *found == tab).map(|i| i as u64)
}

fn tab_at(index: u64) -> Option<Tab> {
    Tab::ALL.iter().chain(Tab::CONTEXTUAL.iter()).nth(index as usize).copied()
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

    #[test]
    fn the_window_describes_its_tabs_buttons_and_document() {
        let editor = editor(&["Hello"]);
        let elements = editor.accessible_elements();
        let home = elements.iter().find(|e| e.name == "Home").expect("the Home tab");
        assert_eq!(home.role, Role::TabItem);
        assert!(home.selected);
        assert_eq!(home.access_key, "H");
        let bold = elements.iter().find(|e| e.name == "Bold").expect("the Bold button");
        assert_eq!(bold.role, Role::Toggle);
        assert!(!bold.selected);
        assert!(bold.rect.2 > 0 && bold.rect.3 > 0);
        let document = elements.iter().find(|e| e.role == Role::Document).expect("the document");
        assert!(document.focused);
        assert_eq!(document.id, DOCUMENT);
        // Every element has a name and a place, and no two share an id.
        assert!(elements.iter().all(|e| !e.name.is_empty()));
        let mut ids: Vec<u64> = elements.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), elements.len());
    }

    #[test]
    fn pressing_an_element_does_what_the_button_does() {
        let mut editor = editor(&["Hello"]);
        let elements = editor.accessible_elements();
        let insert = elements.iter().find(|e| e.name == "Insert").expect("the Insert tab");
        editor.accessible_invoke(insert.id);
        assert_eq!(editor.ribbon.tab, Tab::Insert);
        editor.accessible_invoke(TAB_BASE + tab_index(Tab::Home).unwrap());
        assert_eq!(editor.ribbon.tab, Tab::Home);
        // The buttons are where they were last drawn.
        editor.draw(1400, 900);
        let elements = editor.accessible_elements();
        let bold = elements.iter().find(|e| e.name == "Bold").expect("the Bold button");
        editor.document.select_all();
        editor.accessible_invoke(bold.id);
        editor.document.set_caret(TextPosition::new(0, 1));
        assert!(editor.document.character_format_here().bold, "Bold was not applied");
        let elements = editor.accessible_elements();
        let bold = elements.iter().find(|e| e.name == "Bold").expect("the Bold button");
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
}

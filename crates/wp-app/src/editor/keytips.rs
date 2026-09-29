//! Driving the ribbon from the keyboard: Alt, then a letter, then a letter.
//!
//! # The shape of it
//!
//! Alt on its own puts a letter over every tab. Pressing one of those letters
//! opens that tab and puts a letter over everything in it. Pressing one of
//! those runs the command and the letters go. Escape steps back out, Alt puts
//! them away, and so does a click on anything.
//!
//! That is Word's, and it is the reason somebody who knows the program can work
//! it without reaching for the mouse at all.
//!
//! # What is different
//!
//! Word also letters the buttons above the ribbon — save, undo, redo — and
//! their letters are digits. Those three have shortcuts of their own here
//! (Ctrl+S, Ctrl+Z, Ctrl+Y), so nothing is out of reach without them.

use wp_shell::Response;

use crate::chrome::ribbon::Tab;
use crate::chrome::{keytips, tip, Command};

use super::Editor;

/// The letter over a tab.
///
/// In English, Word's own letters, which a person who knows Word has in
/// their fingers — H for Home, N for Insert: see [`Tab::key_tip`]. In any
/// other language they are worked out from the tabs' names as that language
/// has them, as the commands' letters are, so that the letter over a tab is
/// in the word under it: English letters over German words are letters
/// that match nothing a German is looking at. Worked out over every tab in
/// one fixed order, so that a tab's letter does not move when a tab of
/// tables comes and goes.
#[must_use]
pub(super) fn tab_key_tip(tab: Tab) -> String {
    if crate::messages::language() == crate::messages::ENGLISH {
        return tab.key_tip().to_owned();
    }
    let every: Vec<Tab> = Tab::ALL.iter().chain(Tab::CONTEXTUAL.iter()).copied().collect();
    let labels: Vec<&str> = every.iter().map(|each| crate::messages::t(each.label())).collect();
    let letters = keytips::assign(&labels);
    every.iter().position(|each| *each == tab).map(|at| letters[at].clone()).unwrap_or_default()
}

impl Editor {
    /// What a command's button says, in the interface's language, for its
    /// letter to be taken from: the words on the button where it has any,
    /// else what its tip calls it, else the group its corner arrow is in; a
    /// squeezed group's button says the group's name, and a tile of the
    /// gallery its style's.
    fn shown_label(&self, command: Command) -> String {
        match command {
            Command::ExpandGroup(index) => {
                let groups = self.ribbon.groups();
                return groups.get(usize::from(index)).map_or_else(
                    || "?".to_owned(),
                    |group| crate::messages::t(group.label).to_owned(),
                );
            }
            Command::Style(index) => {
                return self
                    .style_gallery()
                    .get(index)
                    .map_or_else(|| "?".to_owned(), |sample| sample.name.clone());
            }
            _ => {}
        }
        crate::chrome::ribbon::name_of(command)
            .or_else(|| tip::label_of(command))
            .or_else(|| crate::chrome::ribbon::launcher_of(command))
            .map_or_else(|| "?".to_owned(), crate::messages::translated)
    }
}

/// How far the letters have been followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Level {
    /// A letter over each tab.
    Tabs,
    /// A letter over everything in the tab that is open.
    Commands,
}

impl Editor {
    /// Shows the letters, or puts them away if they are already showing.
    pub(super) fn toggle_key_tips(&mut self) -> Response {
        self.key_tips = match self.key_tips {
            Some(_) => None,
            None => Some(Level::Tabs),
        };
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Whether the letters are showing.
    #[must_use]
    pub(super) fn showing_key_tips(&self) -> bool {
        self.key_tips.is_some()
    }

    /// Takes them away. Returns whether any were showing.
    pub(super) fn hide_key_tips(&mut self) -> bool {
        if self.key_tips.take().is_some() {
            self.needs_redraw = true;
            return true;
        }
        false
    }

    /// Steps back out one level, the way Escape does.
    pub(super) fn leave_key_tips(&mut self) -> Response {
        self.key_tips = match self.key_tips {
            Some(Level::Commands) => Some(Level::Tabs),
            _ => None,
        };
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Follows one letter.
    pub(super) fn press_key_tip(&mut self, letter: char) -> Response {
        let wanted = letter.to_ascii_uppercase().to_string();
        match self.key_tips {
            Some(Level::Tabs) => {
                let Some((tab, ..)) =
                    self.tab_tips().into_iter().find(|(_, letters, ..)| *letters == wanted)
                else {
                    return Response::Ignored;
                };
                // File opens the backstage rather than a ribbon page, and there
                // are no key tips over a window that is not the ribbon.
                if tab == crate::chrome::ribbon::Tab::File {
                    self.key_tips = None;
                    return self.choose_tab(tab);
                }
                self.ribbon.tab = tab;
                self.key_tips = Some(Level::Commands);
                self.needs_redraw = true;
                Response::Redraw
            }
            Some(Level::Commands) => {
                let Some((command, ..)) =
                    self.command_tips().into_iter().find(|(_, letters, ..)| *letters == wanted)
                else {
                    return Response::Ignored;
                };
                self.key_tips = None;
                self.needs_redraw = true;
                self.run(command)
            }
            None => Response::Ignored,
        }
    }

    /// The tabs, their letters, and where each sits.
    #[must_use]
    fn tab_tips(&self) -> Vec<(Tab, String, f32, f32)> {
        self.ribbon
            .tab_places()
            .into_iter()
            .map(|(tab, left, width)| (tab, tab_key_tip(tab), left, width))
            .collect()
    }

    /// The commands of the open tab, their letters, and where each sits.
    #[must_use]
    fn command_tips(&self) -> Vec<(Command, String, f32, f32, f32, f32)> {
        let places = self.ribbon.command_places();
        // Worked out from the labels as they are shown, in the interface's
        // language: a letter is only any use if it is in the word under it.
        let labels: Vec<String> =
            places.iter().map(|(command, ..)| self.shown_label(*command)).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let letters = keytips::assign(&labels);
        places
            .into_iter()
            .zip(letters)
            .map(|((command, left, top, width, height), letter)| {
                (command, letter, left, top, width, height)
            })
            .collect()
    }

    /// Draws whichever letters are showing.
    pub(super) fn draw_key_tips(&mut self) {
        let Some(level) = self.key_tips else { return };
        let theme = self.theme;

        // Gathered first, because drawing borrows the same engine the places
        // were worked out with.
        let badges: Vec<(String, f32, f32)> = match level {
            Level::Tabs => {
                let strip = self.ribbon.strip_top();
                self.tab_tips()
                    .into_iter()
                    .map(|(_, letter, left, width)| (letter, left + width / 2.0 - 8.0, strip + 8.0))
                    .collect()
            }
            Level::Commands => self
                .command_tips()
                .into_iter()
                .map(|(_, letter, left, top, width, height)| {
                    (letter, left + width / 2.0 - 8.0, top + height - 14.0)
                })
                .collect(),
        };

        for (letter, x, y) in badges {
            keytips::draw(
                &mut self.canvas,
                &mut self.chrome_engine,
                &mut self.renderer,
                &letter,
                x,
                y,
                &theme,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;

    use super::*;
    use crate::messages::t;

    fn editor() -> Editor {
        let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Text")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let mut editor = Editor::new(library, Document::open(&bytes).expect("reopening"), None);
        editor.paint(1400, 900);
        editor
    }

    /// In English, Word's own letters over the tabs.
    #[test]
    fn in_english_the_tabs_have_word_s_letters() {
        let editor = editor();
        let tips = editor.tab_tips();
        let letter =
            |wanted: Tab| tips.iter().find(|(tab, ..)| *tab == wanted).map(|t| t.1.clone());
        assert_eq!(letter(Tab::Home).as_deref(), Some("H"));
        assert_eq!(letter(Tab::Insert).as_deref(), Some("N"));
    }

    /// In German, each tab's letter and each command's is in the German
    /// word under it, all different; and a tab's letter opens it.
    #[test]
    fn in_german_each_letter_is_in_the_german_word_under_it() {
        crate::messages::tests::in_language("de", || {
            let mut editor = editor();
            let in_word = |letter: &str, word: &str| {
                letter.chars().all(|c| c.is_ascii_digit()) || word.to_uppercase().contains(letter)
            };
            let tips = editor.tab_tips();
            let mut seen = Vec::new();
            for (tab, letter, ..) in &tips {
                assert!(in_word(letter, t(tab.label())), "{letter} over {}", t(tab.label()));
                assert!(!seen.contains(letter), "{letter} twice");
                seen.push(letter.clone());
            }
            let (_, insert, ..) =
                tips.iter().find(|(tab, ..)| *tab == Tab::Insert).expect("Einfügen");
            assert_ne!(insert, "N", "not English's letter over a German word");

            editor.toggle_key_tips();
            editor.press_key_tip(insert.chars().next().expect("a letter").to_ascii_lowercase());
            assert_eq!(editor.ribbon.tab, Tab::Insert, "the letter opens the tab it is over");
            editor.paint(1400, 900);
            if let Ok(directory) = std::env::var("WP_PROOFS") {
                let picture = wp_raster::encode_png(editor.canvas());
                let _ = std::fs::create_dir_all(&directory);
                let path = std::path::Path::new(&directory).join("key-tips-german.png");
                let _ = std::fs::write(path, picture);
            }
            for (command, letter, ..) in editor.command_tips() {
                let label = editor.shown_label(command);
                assert!(letter.is_empty() || in_word(&letter, &label), "{letter} over {label}");
            }
        });
    }
}

//! Word's Symbol dialog: a grid of characters, and the named ones beside it.
//!
//! # Two tabs, because two questions
//!
//! "Which of the six hundred characters in this font do I want" and "give me an
//! em dash" are not the same question, and Word answers them on two tabs. The
//! first is a grid to hunt through; the second is a short list of the ones
//! people ask for by name, each with the keys that put it in without opening
//! anything at all.
//!
//! # Subsets
//!
//! A font holds thousands of characters and a grid shows a hundred and
//! twenty-eight. Unicode's own blocks are what Word divides them by, and the
//! table below is those blocks by their own names. Choosing one moves the grid;
//! nothing is searched and nothing is guessed.
//!
//! # The shortcuts are real
//!
//! Word shows a shortcut beside each special character and those shortcuts
//! work. The ones listed here work too — see [`super::events`] — which is the
//! difference between a dialog that documents the program and one that
//! describes some other program.

use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};

use super::dialogs::Asking;
use super::Editor;

// The Symbols tab.
const TAB_SYMBOLS: usize = 0;
const ROW_FONT: usize = 1;
const FONT: usize = 2;
const SUBSET: usize = 3;
const GRID: usize = 4;
const ROW_CODE: usize = 5;
const CODE: usize = 6;
const RECENT: usize = 7;

// The Special Characters tab.
const TAB_SPECIAL: usize = 8;
const SPECIAL: usize = 9;

/// The button that puts the chosen character in without shutting the dialog.
///
/// Word's Insert: a person putting in three symbols should not have to open the
/// dialog three times.
pub(super) const INSERT: &str = "Insert";

/// How many characters are remembered as recently used.
///
/// Word shows sixteen, which is one row of its grid.
const RECENT_COUNT: usize = 16;

/// The blocks of Unicode Word divides the grid by, with the ranges they cover.
///
/// Their own names, not invented ones: a person who knows that an arrow is in
/// "Arrows" should find it under "Arrows".
const SUBSETS: &[(&str, u32, u32)] = &[
    ("Basic Latin", 0x0020, 0x007E),
    ("Latin-1 Supplement", 0x00A0, 0x00FF),
    ("Latin Extended-A", 0x0100, 0x017F),
    ("Latin Extended-B", 0x0180, 0x024F),
    ("IPA Extensions", 0x0250, 0x02AF),
    ("Greek and Coptic", 0x0370, 0x03FF),
    ("Cyrillic", 0x0400, 0x04FF),
    ("Hebrew", 0x0590, 0x05FF),
    ("Arabic", 0x0600, 0x06FF),
    ("General Punctuation", 0x2000, 0x206F),
    ("Superscripts and Subscripts", 0x2070, 0x209F),
    ("Currency Symbols", 0x20A0, 0x20BF),
    ("Letterlike Symbols", 0x2100, 0x214F),
    ("Number Forms", 0x2150, 0x218F),
    ("Arrows", 0x2190, 0x21FF),
    ("Mathematical Operators", 0x2200, 0x22FF),
    ("Box Drawing", 0x2500, 0x257F),
    ("Block Elements", 0x2580, 0x259F),
    ("Geometric Shapes", 0x25A0, 0x25FF),
    ("Miscellaneous Symbols", 0x2600, 0x26FF),
    ("Dingbats", 0x2700, 0x27BF),
];

/// Word's list of special characters, with the keys that put each one in.
///
/// The order is Word's. A shortcut of `None` is a character Word offers no keys
/// for, and saying so is better than inventing some.
pub(super) const SPECIAL_CHARACTERS: &[(&str, char, Option<&str>)] = &[
    ("Em Dash", '\u{2014}', Some("Ctrl+Alt+Num -")),
    ("En Dash", '\u{2013}', Some("Ctrl+Num -")),
    ("Nonbreaking Hyphen", '\u{2011}', Some("Ctrl+Shift+-")),
    ("Optional Hyphen", '\u{00AD}', Some("Ctrl+-")),
    ("Em Space", '\u{2003}', None),
    ("En Space", '\u{2002}', None),
    ("Quarter Em Space", '\u{2005}', None),
    ("Nonbreaking Space", '\u{00A0}', Some("Ctrl+Shift+Space")),
    ("Copyright", '\u{00A9}', Some("Ctrl+Alt+C")),
    ("Registered", '\u{00AE}', Some("Ctrl+Alt+R")),
    ("Trademark", '\u{2122}', Some("Ctrl+Alt+T")),
    ("Section", '\u{00A7}', None),
    ("Paragraph", '\u{00B6}', None),
    ("Ellipsis", '\u{2026}', Some("Ctrl+Alt+.")),
    ("Single Opening Quote", '\u{2018}', Some("Ctrl+`, `")),
    ("Single Closing Quote", '\u{2019}', Some("Ctrl+', '")),
    ("Double Opening Quote", '\u{201C}', Some("Ctrl+`, \"")),
    ("Double Closing Quote", '\u{201D}', Some("Ctrl+', \"")),
];

/// Every character of a block that a font can actually draw.
///
/// A block is a range of numbers, not a list of characters: most blocks have
/// holes, and every font has more. A grid full of empty boxes is worse than a
/// short grid, so the ones nothing can draw are left out.
fn characters_of(subset: usize, has_glyph: impl Fn(char) -> bool) -> Vec<char> {
    let Some((_, first, last)) = SUBSETS.get(subset).copied() else { return Vec::new() };
    (first..=last)
        .filter_map(char::from_u32)
        // A character with no width of its own would be an empty cell that is
        // not empty, which reads as a mistake.
        .filter(|character| !character.is_control() && has_glyph(*character))
        .collect()
}

impl Editor {
    /// Opens Word's Symbol dialog.
    pub(super) fn open_symbol_dialog(&mut self) -> Response {
        let dialog = self.symbol_dialog(self.symbol_subset, None);
        self.ask(Asking::Symbol, dialog)
    }

    /// The dialog itself.
    ///
    /// Built again whenever the subset changes, because the grid is the subset:
    /// there is nothing else in it to keep.
    pub(super) fn symbol_dialog(&self, subset: usize, picked: Option<char>) -> Dialog {
        let library = self.engine.library();
        let characters = characters_of(subset, |character| library.can_draw(character));
        let current = picked
            .and_then(|wanted| characters.iter().position(|found| *found == wanted))
            .unwrap_or(0);

        let fonts: Vec<String> = core::iter::once("(normal text)".to_owned())
            .chain(self.families.iter().cloned())
            .collect();
        let code = characters
            .get(current)
            .map_or_else(String::new, |character| format!("{:04X}", u32::from(*character)));

        let recent: String = self.recent_symbols.iter().collect();
        let named: Vec<String> = SPECIAL_CHARACTERS
            .iter()
            .map(|(name, character, keys)| match keys {
                Some(keys) => format!("{character}   {name}   {keys}"),
                None => format!("{character}   {name}"),
            })
            .collect();

        let fields = vec![
            // --- Symbols ---------------------------------------------------
            Field::Tab("Symbols".to_owned()),
            Field::Columns(2),
            Field::Choice { label: "Font".to_owned(), items: fonts, current: 0 },
            Field::Choice {
                label: "Subset".to_owned(),
                items: SUBSETS.iter().map(|(name, ..)| (*name).to_owned()).collect(),
                current: subset,
            },
            Field::Grid {
                label: "Symbols".to_owned(),
                items: characters,
                current,
                scroll: current / 16,
            },
            Field::Columns(2),
            Field::Number { label: "Character code".to_owned(), value: code, unit: "" },
            Field::Text { label: "Recently used".to_owned(), value: recent },
            // --- Special Characters ----------------------------------------
            Field::Tab("Special Characters".to_owned()),
            Field::Choice { label: "Character".to_owned(), items: named, current: 0 },
        ];

        crate::chrome::dialog::check_rows(
            "Symbol",
            &fields,
            &[
                (TAB_SYMBOLS, "a tab"),
                (ROW_FONT, "a row"),
                (FONT, "a list"),
                (SUBSET, "a list"),
                (GRID, "a grid"),
                (ROW_CODE, "a row"),
                (CODE, "a number"),
                (RECENT, "a box"),
                (TAB_SPECIAL, "a tab"),
                (SPECIAL, "a list"),
            ],
        );

        Dialog::with_buttons(
            "Symbol",
            fields,
            vec![
                Button { label: INSERT.to_owned(), answer: Answer::Named(INSERT), default: true },
                Button { label: "Close".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(500.0)
    }

    /// Which character the dialog is offering, on whichever tab is showing.
    pub(super) fn symbol_dialog_says(&self, dialog: &Dialog) -> Option<char> {
        if dialog.showing_tab() == 1 {
            return SPECIAL_CHARACTERS
                .get(dialog.chose(SPECIAL))
                .map(|(_, character, _)| *character);
        }
        dialog.picked(GRID)
    }

    /// Word's Insert, which puts the character in and leaves the dialog up.
    pub(super) fn insert_symbol_from_dialog(&mut self, dialog: &Dialog) -> Response {
        let Some(character) = self.symbol_dialog_says(dialog) else { return Response::Ignored };

        // Remembered before it is put in, so the list is right even if the
        // document refuses the change.
        self.remember_symbol(character);
        let subset = dialog.chose(SUBSET);
        let changed = self.document.type_text(&character.to_string());

        // Built again so that the row of recent ones shows what just went in.
        let mut built = self.symbol_dialog(subset, Some(character));
        built.carry_typing_from(dialog);
        self.dialog = Some(built);
        self.symbol_subset = subset;
        self.edited(changed, &format!("Symbol: {character}"))
    }

    /// The subset changing is the grid changing, so the dialog is rebuilt.
    pub(super) fn symbol_dialog_changed(&mut self) {
        let Some(dialog) = self.dialog.take() else { return };
        let subset = dialog.chose(SUBSET);
        let picked = dialog.picked(GRID);

        // Only when it is a different subset: rebuilding on every keystroke
        // would throw away the cell that was picked.
        let mut built = if subset == self.symbol_subset {
            self.dialog = Some(dialog);
            return;
        } else {
            self.symbol_subset = subset;
            self.symbol_dialog(subset, picked)
        };
        built.carry_typing_from(&dialog);
        self.dialog = Some(built);
    }

    /// Puts a character at the front of the recently used, as Word does.
    pub(super) fn remember_symbol(&mut self, character: char) {
        self.recent_symbols.retain(|found| *found != character);
        self.recent_symbols.insert(0, character);
        self.recent_symbols.truncate(RECENT_COUNT);
    }

    /// Types one of Word's named characters, from its keyboard shortcut.
    pub(super) fn insert_special_character(&mut self, name: &str) -> Response {
        let Some((_, character, _)) =
            SPECIAL_CHARACTERS.iter().find(|(found, ..)| *found == name).copied()
        else {
            return Response::Ignored;
        };
        self.remember_symbol(character);
        let changed = self.document.type_text(&character.to_string());
        self.edited(changed, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subset_holds_only_what_can_be_drawn() {
        // A grid full of empty boxes is worse than a short grid.
        let all = characters_of(0, |_| true);
        let none = characters_of(0, |_| false);
        assert!(!all.is_empty());
        assert!(none.is_empty(), "characters nothing can draw were kept");
    }

    #[test]
    fn basic_latin_starts_at_the_space_rather_than_at_nothing() {
        // The block begins at U+0000; the printable part begins at the space,
        // and the control characters would be cells that draw nothing.
        let latin = characters_of(0, |_| true);
        assert_eq!(latin.first().copied(), Some(' '));
        assert!(!latin.iter().any(|character| character.is_control()));
    }

    #[test]
    fn every_subset_word_lists_covers_a_real_range() {
        for (name, first, last) in SUBSETS {
            assert!(first < last, "{name} runs backwards");
            assert!(char::from_u32(*first).is_some(), "{name} starts at nothing");
        }
    }

    #[test]
    fn the_named_characters_are_the_ones_they_are_named_after() {
        // A list that said "Em Dash" and gave an en dash would be worse than no
        // list at all.
        let find = |name: &str| {
            SPECIAL_CHARACTERS.iter().find(|(found, ..)| *found == name).map(|(_, c, _)| *c)
        };
        assert_eq!(find("Em Dash"), Some('\u{2014}'));
        assert_eq!(find("En Dash"), Some('\u{2013}'));
        assert_eq!(find("Nonbreaking Space"), Some('\u{00A0}'));
        assert_eq!(find("Ellipsis"), Some('\u{2026}'));
    }

    #[test]
    fn no_two_named_characters_are_the_same_character() {
        for (index, (name, character, _)) in SPECIAL_CHARACTERS.iter().enumerate() {
            for (other, other_character, _) in SPECIAL_CHARACTERS.iter().skip(index + 1) {
                assert_ne!(character, other_character, "{name} and {other} are the same");
            }
        }
    }
}

#[cfg(test)]
mod editor_tests {
    use super::*;
    use crate::chrome::dialog::Field;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Text")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.set_caret(wp_docx::TextPosition::new(0, 4));
        editor
    }

    /// Presses the dialog's Insert, the way a click on it would.
    fn insert(editor: &mut Editor) {
        let dialog = editor.dialog.clone().expect("a dialog");
        editor.insert_symbol_from_dialog(&dialog);
    }

    #[test]
    fn the_grid_holds_the_subset_that_was_chosen() {
        let mut editor = editor();
        editor.open_symbol_dialog();
        let dialog = editor.dialog.as_ref().expect("a dialog");

        match dialog.fields.get(GRID) {
            Some(Field::Grid { items, .. }) => {
                assert!(items.contains(&'A'), "Basic Latin has no A: {}", items.len());
                assert!(!items.contains(&'\u{2014}'), "an em dash is not Basic Latin");
            }
            other => panic!("row {GRID} is {other:?}, not a grid"),
        }
    }

    #[test]
    fn a_character_picked_from_the_grid_goes_into_the_document() {
        let mut editor = editor();
        editor.open_symbol_dialog();
        // Walk to the letter A rather than assuming where it is.
        let at = match editor.dialog.as_ref().and_then(|dialog| dialog.fields.get(GRID)) {
            Some(Field::Grid { items, .. }) => {
                items.iter().position(|found| *found == 'A').expect("Basic Latin has an A")
            }
            other => panic!("row {GRID} is {other:?}"),
        };
        if let Some(Field::Grid { current, .. }) =
            editor.dialog.as_mut().and_then(|dialog| dialog.fields.get_mut(GRID))
        {
            *current = at;
        }
        insert(&mut editor);

        assert!(editor.document.plain_text().contains("TextA"));
        // And the dialog is still up, as Word's is.
        assert!(editor.in_dialog(), "Insert shut the dialog");
    }

    #[test]
    fn what_went_in_is_remembered_as_recently_used() {
        let mut editor = editor();
        editor.remember_symbol('\u{2014}');
        editor.remember_symbol('\u{00A9}');
        editor.remember_symbol('\u{2014}');

        // Newest first, and never twice.
        assert_eq!(editor.recent_symbols, vec!['\u{2014}', '\u{00A9}']);
    }

    #[test]
    fn the_list_of_recent_ones_is_bounded() {
        let mut editor = editor();
        for code in 0x0041..0x0080 {
            editor.remember_symbol(char::from_u32(code).expect("a letter"));
        }
        assert_eq!(editor.recent_symbols.len(), RECENT_COUNT);
    }

    #[test]
    fn the_special_characters_tab_offers_its_own_list() {
        let mut editor = editor();
        editor.open_symbol_dialog();
        editor.dialog_key(Key::Tab, false, true);

        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_eq!(dialog.showing_tab(), 1);
        // The first is Word's em dash, and what the dialog offers is that.
        assert_eq!(editor.symbol_dialog_says(dialog), Some('\u{2014}'));
    }

    #[test]
    fn word_s_shortcuts_put_the_characters_in() {
        // The dialog says Ctrl+Alt+C gives a copyright sign. It has to.
        let mut editor = editor();
        let control_alt = Modifiers { control: true, alt: true, ..Modifiers::default() };
        editor.handle(Event::KeyDown { key: Key::Letter('c'), modifiers: control_alt });
        assert!(editor.document.plain_text().contains('\u{00A9}'), "no copyright sign");

        editor.handle(Event::KeyDown { key: Key::Letter('t'), modifiers: control_alt });
        assert!(editor.document.plain_text().contains('\u{2122}'), "no trademark sign");
    }

    #[test]
    fn a_dash_shortcut_gives_the_dash_it_names() {
        let mut editor = editor();
        let control = Modifiers { control: true, ..Modifiers::default() };
        let control_alt = Modifiers { control: true, alt: true, ..Modifiers::default() };
        let all_three = Modifiers { control: true, alt: true, shift: true };

        editor.handle(Event::KeyDown { key: Key::Digit('-'), modifiers: all_three });
        assert!(editor.document.plain_text().contains('\u{2014}'), "no em dash");
        editor.handle(Event::KeyDown { key: Key::Digit('-'), modifiers: control_alt });
        assert!(editor.document.plain_text().contains('\u{2013}'), "no en dash");
        editor.handle(Event::KeyDown { key: Key::Digit('-'), modifiers: control });
        assert!(editor.document.plain_text().contains('\u{00AD}'), "no optional hyphen");
    }

    #[test]
    fn a_space_with_shift_and_control_is_the_one_that_does_not_break() {
        let mut editor = editor();
        editor.handle(Event::KeyDown {
            key: Key::Space,
            modifiers: Modifiers { control: true, shift: true, ..Modifiers::default() },
        });
        assert!(editor.document.plain_text().contains('\u{00A0}'), "no non-breaking space");
    }
}

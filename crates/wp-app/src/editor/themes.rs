//! The document's theme, from the Design tab.
//!
//! Not the window's theme, which is a setting of this program and has nothing
//! to do with the document. This is the set of colours and fonts the document's
//! own styles are named after — change it and every heading that says
//! `accent1` rather than a colour changes with it.

use wp_docx::gallery::{COLOR_SCHEMES, FONT_PAIRS};
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

impl Editor {
    /// Drops open the whole themes: colours and fonts together.
    pub(super) fn open_themes(&mut self) -> Response {
        let here = self.document.theme();
        let current = COLOR_SCHEMES.iter().position(|scheme| scheme.name == here.name);
        let items = COLOR_SCHEMES.iter().map(|scheme| scheme.name.to_owned()).collect();
        self.open_theme_list(Choice::Theme, Command::Themes, items, current)
    }

    /// Drops open the colour schemes on their own.
    pub(super) fn open_theme_colors(&mut self) -> Response {
        let here = self.document.theme();
        let current = COLOR_SCHEMES.iter().position(|scheme| scheme.theme().colors == here.colors);
        let items = COLOR_SCHEMES.iter().map(|scheme| scheme.name.to_owned()).collect();
        self.open_theme_list(Choice::ThemeColors, Command::ThemeColors, items, current)
    }

    /// Drops open the font pairings on their own.
    pub(super) fn open_theme_fonts(&mut self) -> Response {
        let here = self.document.theme();
        let current = FONT_PAIRS
            .iter()
            .position(|pair| pair.major == here.major_font && pair.minor == here.minor_font);
        let items = FONT_PAIRS.iter().map(wp_docx::gallery::FontPair::label).collect();
        self.open_theme_list(Choice::ThemeFonts, Command::ThemeFonts, items, current)
    }

    /// Puts on a whole theme.
    pub(super) fn choose_theme(&mut self, index: usize) -> Response {
        let Some(scheme) = COLOR_SCHEMES.get(index) else { return Response::Ignored };
        let wanted = scheme.theme();
        self.apply_theme(wanted, &format!("Theme: {}", scheme.name))
    }

    /// Changes only the colours.
    pub(super) fn choose_theme_colors(&mut self, index: usize) -> Response {
        let Some(scheme) = COLOR_SCHEMES.get(index) else { return Response::Ignored };
        let wanted = self.document.theme().with_colors(scheme);
        self.apply_theme(wanted, &format!("Theme colours: {}", scheme.name))
    }

    /// Changes only the fonts.
    pub(super) fn choose_theme_fonts(&mut self, index: usize) -> Response {
        let Some(pair) = FONT_PAIRS.get(index) else { return Response::Ignored };
        let wanted = self.document.theme().with_fonts(pair);
        self.apply_theme(wanted, &format!("Theme fonts: {}", pair.label()))
    }

    /// Writes a theme and lays the document out again with it.
    fn apply_theme(&mut self, wanted: wp_docx::theme::Theme, note: &str) -> Response {
        self.popup = None;
        match self.document.set_theme(&wanted) {
            Ok(changed) => {
                // Every colour and font named after the theme resolves to
                // something else now, so the whole document is measured again.
                self.relayout();
                self.edited(changed, note)
            }
            Err(error) => self.report(&format!("The theme could not be saved: {error}")),
        }
    }

    /// The three lists differ only in what is in them.
    fn open_theme_list(
        &mut self,
        choice: Choice,
        button: Command,
        items: Vec<String>,
        current: Option<usize>,
    ) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == choice) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(button) else {
            return Response::Ignored;
        };
        self.popup = Some(Popup::new(choice, items, current, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }
}

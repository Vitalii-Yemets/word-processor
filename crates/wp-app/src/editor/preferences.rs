//! Putting the remembered settings on and taking them off again.
//!
//! The window remembers how it was left: dark or light, rulers up or down, the
//! pane open or shut, the magnification. It also remembers which theme new
//! documents are made with, which is what the Design tab's Set as Default does.
//!
//! Nothing here is written into the document. See [`crate::settings`] for why.

use wp_docx::gallery::{COLOR_SCHEMES, FONT_PAIRS};
use wp_shell::Response;

use crate::chrome::theme::Mode;
use crate::chrome::Theme;
use crate::settings::Settings;

use super::Editor;

impl Editor {
    /// Puts on whatever was remembered from last time.
    ///
    /// Called after the window is built and before it is first drawn, so that
    /// it comes up the way it was left rather than flashing the default and
    /// then changing.
    pub fn apply_settings(&mut self, settings: Settings) {
        if let Some(dark) = settings.dark {
            self.theme = Theme::of(if dark { Mode::Dark } else { Mode::Light });
        }
        if let Some(rulers) = settings.rulers {
            self.show_rulers = rulers;
            self.remembered_rulers = rulers;
        }
        if let Some(navigation) = settings.navigation {
            self.show_navigation = navigation;
            self.remembered_navigation = navigation;
        }
        self.status_shows = crate::chrome::status::Shows::with_switched_off(&settings.status_off);
        if let Some(marks) = settings.marks {
            self.show_marks = marks;
        }
        if let Some(proofing) = settings.proofing {
            self.show_proofing = proofing;
        }
        if let Some(gridlines) = settings.gridlines {
            self.show_gridlines = gridlines;
        }
        if let Some(white_space) = settings.white_space {
            self.joined_pages = !white_space;
        }
        if let Some(unit) = &settings.unit {
            self.unit = crate::measure::Unit::from_name(unit);
        }
        if let Some(zoom) = settings.zoom {
            self.zoom =
                zoom.clamp(crate::chrome::status::MIN_ZOOM, crate::chrome::status::MAX_ZOOM);
        }
        self.settings = settings;
        self.relayout();
    }

    /// Writes down how the window is now.
    pub(super) fn remember_window(&mut self) {
        self.settings.dark = Some(self.theme.mode == Mode::Dark);
        self.settings.rulers = Some(self.show_rulers);
        self.settings.navigation = Some(self.show_navigation);
        self.settings.zoom = Some(self.zoom);
        self.settings.save();
    }

    /// The theme new documents are made with.
    #[must_use]
    pub(super) fn default_theme(&self) -> wp_docx::theme::Theme {
        let mut theme = wp_docx::theme::Theme::default();
        if let Some(name) = &self.settings.theme_colors {
            if let Some(scheme) = COLOR_SCHEMES.iter().find(|scheme| scheme.name == name) {
                theme = theme.with_colors(scheme);
            }
        }
        if let Some(name) = &self.settings.theme_fonts {
            if let Some(pair) = FONT_PAIRS.iter().find(|pair| pair.name == name) {
                theme = theme.with_fonts(pair);
            }
        }
        theme
    }

    /// Makes this document's theme the one new documents are made with.
    ///
    /// Word's Set as Default, which is about new documents rather than about
    /// this one — nothing in the open document changes.
    pub(super) fn set_theme_as_default(&mut self) -> Response {
        let theme = self.document.theme();

        // The theme is written out colour by colour, so what is remembered is
        // the name of the scheme it matches. A theme somebody built by hand
        // matches none of them, and then there is no name to remember.
        let colors = COLOR_SCHEMES
            .iter()
            .find(|scheme| theme_matches_colors(&theme, scheme))
            .map(|scheme| scheme.name.to_owned());
        let fonts = FONT_PAIRS
            .iter()
            .find(|pair| theme_matches_fonts(&theme, pair))
            .map(|pair| pair.name.to_owned());

        if colors.is_none() && fonts.is_none() {
            return self.report("This document's theme is not one of the ones on offer");
        }

        let named = colors.clone().unwrap_or_else(|| "the document's".to_owned());
        self.settings.theme_colors = colors;
        self.settings.theme_fonts = fonts;
        self.settings.save();
        self.report(&format!("New documents will use {named} colours"))
    }
}

/// Whether a theme's colours are the ones a scheme names.
fn theme_matches_colors(
    theme: &wp_docx::theme::Theme,
    scheme: &wp_docx::gallery::ColorScheme,
) -> bool {
    wp_docx::theme::Slot::ALL.iter().all(|slot| theme.color(*slot) == scheme.color(*slot))
}

/// And whether its fonts are the ones a pair names.
fn theme_matches_fonts(theme: &wp_docx::theme::Theme, pair: &wp_docx::gallery::FontPair) -> bool {
    theme.font(wp_docx::theme::FontSlot::Major) == pair.major
        && theme.font(wp_docx::theme::FontSlot::Minor) == pair.minor
}

//! The shadow a document's theme puts under its shapes, from the Design tab.
//!
//! Word's Effects gallery is fifteen tiles of gradients, bevels and shadows. A
//! shape with a flat fill shows only the shadow, so that is what is offered: no
//! effects, or one of three shadows. What is written is the theme's own effect
//! styles, so Word draws the same shadow this program draws.

use wp_docx::theme::Effect;
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

impl Editor {
    /// Drops open the effects the theme can carry.
    pub(super) fn open_theme_effects(&mut self) -> Response {
        if self.close_popup_if(Choice::ThemeEffects) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Effects) else {
            return Response::Ignored;
        };

        let here = self.document.theme().effect;
        let current = Effect::ALL.iter().position(|effect| *effect == here);
        let items = Effect::ALL.iter().map(|effect| effect.label().to_owned()).collect();
        self.popup = Some(Popup::new(Choice::ThemeEffects, items, current, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts the chosen effect on the theme.
    pub(super) fn choose_theme_effects(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(effect) = Effect::ALL.get(index).copied() else { return Response::Ignored };

        let wanted = wp_docx::theme::Theme { effect, ..self.document.theme() };
        match self.document.set_theme(&wanted) {
            Ok(changed) => {
                // Every shape is drawn with the theme's shadow, so they are all
                // measured and drawn again.
                self.relayout();
                self.edited(changed, &format!("Effects: {}", effect.label()))
            }
            Err(error) => self.report(&format!("The theme could not be changed: {error}")),
        }
    }
}

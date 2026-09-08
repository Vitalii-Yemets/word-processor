//! Text effects, from the Font group of the Home tab.

use wp_docx::effects::Effect;
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

impl Editor {
    /// Drops open the looks the letters can be drawn with.
    pub(super) fn open_text_effects(&mut self) -> Response {
        if self.close_popup_if(Choice::TextEffect) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::TextEffects) else {
            return Response::Ignored;
        };

        let here = self.document.text_effect_here();
        let current = Effect::CHOICES.iter().position(|effect| *effect == here);
        let items = Effect::CHOICES.iter().map(|effect| effect.label().to_owned()).collect();
        self.popup = Some(Popup::new(Choice::TextEffect, items, current, left, top, 200.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Applies the effect that was chosen.
    pub(super) fn choose_text_effect(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(effect) = Effect::CHOICES.get(index).copied() else {
            return Response::Ignored;
        };

        let changed = self.document.set_text_effect(effect);
        self.finish_character_change(changed, effect.label())
    }
}

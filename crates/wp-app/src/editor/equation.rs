//! Equations, from the Insert tab.
//!
//! One strip and one line: the linear format Word's own editor accepts, typed
//! in and turned into a built-up equation. `a/b` is a fraction, `x^2` a power,
//! `x_1` an index, `sqrt(x)` a root, and `\alpha` a Greek letter.

use wp_docx::math;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};

use super::Editor;

impl Editor {
    /// Asks for the equation.
    pub(super) fn start_equation(&mut self) -> Response {
        self.find_bar = Some(FindBar::for_purpose(Purpose::Equation));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the equation: a/b, x^2, x_1, sqrt(x), \\alpha")
    }

    /// Puts it in.
    pub(super) fn finish_equation(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.clone();
        self.find_bar = None;
        self.needs_redraw = true;

        let parsed = math::parse(&typed);
        if parsed.is_empty() {
            return self.report("Nothing was typed, so no equation was put in");
        }

        let changed = self.document.insert_equation(&parsed);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("Equation: {}", parsed.plain_text()))
    }
}

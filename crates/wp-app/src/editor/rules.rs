//! Merge rules, from the Mailings tab.
//!
//! Two steps for the rules that need a condition — pick the rule, then type
//! what it compares — and one for the two that only number the letters.

use wp_docx::rules::Rule;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

impl Editor {
    /// Drops open the rules a merge can carry.
    pub(super) fn open_rules(&mut self) -> Response {
        if self.close_popup_if(Choice::Rule) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Rules) else {
            return Response::Ignored;
        };

        let items = Rule::ALL.iter().map(|rule| rule.label().to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Rule, items, None, left, top, 260.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts the rule in, asking for its condition first when it has one.
    pub(super) fn choose_rule(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(rule) = Rule::ALL.get(index).copied() else { return Response::Ignored };
        self.merge_rule = rule;

        let Some(prompt) = rule.prompt() else {
            // Nothing to ask: the rule is the whole of itself.
            return self.write_rule("");
        };
        self.find_bar = Some(FindBar::for_purpose(Purpose::Rule));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report(&format!("Type the {prompt}"))
    }

    /// Writes the rule with what was typed for it.
    pub(super) fn finish_rule(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.clone();
        self.find_bar = None;
        self.needs_redraw = true;
        self.write_rule(&typed)
    }

    fn write_rule(&mut self, typed: &str) -> Response {
        let rule = self.merge_rule;
        let Some(instruction) = rule.instruction(typed) else {
            return self.report("A rule needs the name of a column to look at");
        };

        let changed = self.document.insert_rule(&instruction);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, rule.label())
    }
}

//! Line numbers, hyphenation and who may edit the document.

use wp_docx::appearance::{EditMode, LineNumbers, Restart};
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// The ways of numbering the lines, in the order Word lists them.
const NUMBERING: &[(&str, Option<Restart>)] = &[
    ("None", None),
    ("Continuous", Some(Restart::Continuous)),
    ("Restart Each Page", Some(Restart::NewPage)),
    ("Restart Each Section", Some(Restart::NewSection)),
];

impl Editor {
    /// Drops open the ways of numbering the lines.
    pub(super) fn open_line_numbers(&mut self) -> Response {
        if self.close_popup_if(Choice::LineNumbers) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::LineNumbers) else {
            return Response::Ignored;
        };

        let here = self.document.line_numbers().map(|numbers| numbers.restart);
        let current = NUMBERING.iter().position(|(_, restart)| *restart == here);
        let items = NUMBERING.iter().map(|(label, _)| (*label).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::LineNumbers, items, current, left, top, 220.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Numbers the lines whichever way was chosen.
    pub(super) fn choose_line_numbers(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((label, restart)) = NUMBERING.get(index).copied() else {
            return Response::Ignored;
        };

        // Whatever else was set — how often to print a number, where to start —
        // is kept, so picking a different restart does not undo it.
        let wanted = restart.map(|restart| LineNumbers {
            restart,
            ..self.document.line_numbers().unwrap_or_default()
        });
        let changed = self.document.set_line_numbers(wanted);
        self.relayout();
        self.edited(changed, &format!("Line numbers: {label}"))
    }

    /// Drops open the two ways of hyphenating.
    pub(super) fn open_hyphenation(&mut self) -> Response {
        if self.close_popup_if(Choice::Hyphenation) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Hyphenation) else {
            return Response::Ignored;
        };

        let current = usize::from(self.document.automatic_hyphenation());
        let items = vec!["None".to_owned(), "Automatic".to_owned()];
        self.popup = Some(Popup::new(Choice::Hyphenation, items, Some(current), left, top, 200.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Turns hyphenation on or off.
    pub(super) fn choose_hyphenation(&mut self, index: usize) -> Response {
        self.popup = None;
        let on = index == 1;
        let changed = self.document.set_automatic_hyphenation(on);
        // Nothing about the layout changes yet — this program does not break
        // words itself — but the document says so, and Word will.
        self.edited(changed, if on { "Hyphenation: automatic" } else { "Hyphenation: none" })
    }

    /// Drops open what a reader may be allowed to do.
    pub(super) fn open_protection(&mut self) -> Response {
        if self.close_popup_if(Choice::Protection) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::RestrictEditing) else {
            return Response::Ignored;
        };

        let here = self.document.protection();
        let current = match here {
            None => Some(0),
            Some(mode) => EditMode::ALL.iter().position(|entry| *entry == mode).map(|at| at + 1),
        };
        let mut items = vec!["Stop Protection".to_owned()];
        items.extend(EditMode::ALL.iter().map(|mode| mode.label().to_owned()));
        self.popup = Some(Popup::new(Choice::Protection, items, current, left, top, 260.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Restricts editing, or lifts the restriction.
    pub(super) fn choose_protection(&mut self, index: usize) -> Response {
        self.popup = None;
        // The first line lifts it; the rest are the kinds, in order.
        let wanted = index.checked_sub(1).and_then(|at| EditMode::ALL.get(at).copied());
        if index > 0 && wanted.is_none() {
            return Response::Ignored;
        }

        let changed = self.document.set_protection(wanted);
        let note = match wanted {
            None => "Protection lifted".to_owned(),
            Some(mode) => format!("Restricted to: {}", mode.label()),
        };
        self.needs_redraw = true;
        self.edited(changed, &note)
    }

    /// Closes an open list if it is the one asked about.
    ///
    /// Pressing a button whose list is already open closes it, which is what
    /// every list in every ribbon does.
    pub(super) fn close_popup_if(&mut self, choice: Choice) -> bool {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == choice) {
            self.popup = None;
            self.needs_redraw = true;
            return true;
        }
        false
    }

    /// The colour the paper is painted.
    ///
    /// A document that names its own page colour gets it, whatever the theme
    /// says: that colour is part of the document, and a dark window is not a
    /// reason to show a different one.
    pub(super) fn page_paint(&self) -> wp_raster::Color {
        self.document
            .page_color()
            .as_deref()
            .and_then(wp_raster::Color::from_hex)
            .unwrap_or(self.theme.page)
    }
}

impl Editor {
    /// Whether the document says it may not be edited.
    #[must_use]
    pub(super) fn is_locked(&self) -> bool {
        self.document.protection() == Some(EditMode::ReadOnly)
    }

    /// Says why nothing happened.
    pub(super) fn refuse_locked(&mut self) -> Response {
        self.report("This document is protected — Review ▸ Restrict Editing lifts it")
    }

    /// Locks the selection so that only this author may change it, or unlocks
    /// the stretch the caret is in.
    ///
    /// Word's Block Authors, which is one button that does both: in a document
    /// several people have open, you lock what you are working on and let it go
    /// again when you are done.
    pub(super) fn toggle_block_authors(&mut self) -> Response {
        if self.document.locked_here().is_some() {
            let changed = self.document.unblock_authors();
            self.relayout();
            return self.edited(changed, "Unblocked");
        }

        // Who is doing the locking: whoever the document says is writing it,
        // which is the same name a tracked change is signed with.
        let author = super::files::user_name();
        let changed = self.document.block_authors(&author);
        if !changed {
            return self.report("Select the text to block other authors from first");
        }
        self.relayout();
        self.edited(changed, &format!("Blocked for everybody but {author}"))
    }
}

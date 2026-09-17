//! The Restrict Editing pane: what it shows, and what pressing things in it
//! does.
//!
//! The pane itself is [`crate::chrome::restrictpane`]; what is here is where
//! its answers come from and where they go. The rule that decides whether a
//! person may type is not here either — it is [`super::protection`], and it is
//! one rule read by everything.
//!
//! # Where the list of people comes from
//!
//! This was the thing the item waited on, and the answer turned out to be
//! that the document already knows. Every stretch with its own rule about who
//! may edit it names somebody — `w:permStart w:ed="…"` — so the names in a
//! document are the people it has been shared with, and reading them back is
//! how a document somebody else restricted shows its editors here. To that go
//! the person at the keyboard, who is always on the list because they are the
//! one selecting things, and whoever has been typed into Word's More users…
//! since the program started.
//!
//! What there is not, and cannot honestly be, is a directory. Word offers the
//! address book; there is no address book on a machine running this, so the
//! names are the ones somebody has typed or the document already carries.
//!
//! # Why a person's colour is not chosen here
//!
//! Because it is chosen in [`wp_layout::author_color`], where a reviewer's
//! tracked changes get theirs. The same name has to come out the same colour
//! in the margin, in the pane, and round the stretch they may edit, and two
//! places choosing would sooner or later choose differently.
//!
//! # What a tick against a name does
//!
//! Writes the markers round the selection then and there, which is Word's
//! behaviour and the only one that makes sense: the exceptions are about
//! particular words, so they cannot wait for a button at the bottom of the
//! pane the way the two restrictions above them do.

use wp_docx::permissions::EVERYONE;
use wp_docx::protection::{EditMode, Protection};
use wp_shell::Response;

use crate::chrome::dialog::{Dialog, Field};
use crate::chrome::restrictpane::{Editor as Person, Hit, Shown, WIDTH};
use crate::chrome::{Choice, Popup};

use super::dialogs::Asking;
use super::Editor;

/// Where the answer sits in the dialog that takes a password.
const WORD: usize = 2;
const AGAIN: usize = 3;

/// And in the one that takes names.
const NAMES: usize = 1;

/// How many bytes of salt, asked of the format rather than decided here.
const SALT: usize = wp_docx::protection::SALT_BYTES;

impl Editor {
    /// How much room the pane takes, which is none when it is shut.
    pub(super) fn restrict_pane_width(&self) -> f32 {
        if self.show_restrict {
            WIDTH
        } else {
            0.0
        }
    }

    /// Where its left edge is.
    pub(super) fn restrict_pane_left(&self) -> f32 {
        self.view_width as f32 - WIDTH
    }

    /// Whether a point is inside it at all.
    pub(super) fn over_restrict_pane(&self, x: i32) -> bool {
        let x = crate::chrome::mirror::flip(x);
        self.show_restrict && (x as f32) >= self.restrict_pane_left()
    }

    /// Word's Restrict Editing button: opens the pane, or shuts it again.
    ///
    /// Opening it reads the document into the two boxes, so that a pane opened
    /// on a document that already limits its formatting says so rather than
    /// offering to limit it again.
    pub(super) fn open_protection(&mut self) -> Response {
        if self.show_restrict {
            self.show_restrict = false;
            self.clamp_scroll();
            self.needs_redraw = true;
            return Response::Redraw;
        }

        self.restrict_limiting = self.document.formatting_is_limited();
        match self.document.protection_rules().and_then(|rules| rules.mode) {
            Some(mode) => {
                self.restrict_restricting = true;
                self.restrict_mode = EditMode::ALL.iter().position(|one| *one == mode).unwrap_or(0);
            }
            None => self.restrict_restricting = false,
        }
        // The styles pane and this one are both down the right-hand side, and
        // two panes in one place is one pane with the other underneath it.
        self.show_styles = false;
        self.show_restrict = true;
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Everything the pane draws, worked out afresh from the document.
    ///
    /// Rebuilt on every drawing rather than kept beside the document, for the
    /// reason the Styles pane is: a list that is rebuilt cannot go stale, and
    /// the document is the only thing that knows what is in it.
    pub(super) fn restrict_pane_shown(&self) -> Shown {
        let rules = self.document.protection_rules();
        let enforced = rules.is_some();
        let me = super::files::user_name();
        let mine = self.regions_i_may_edit().len();

        Shown {
            limiting: self.restrict_limiting,
            allowed: self.allowed_style_count(),
            restricting: self.restrict_restricting,
            modes: EditMode::ALL.iter().map(|mode| mode.label().to_owned()).collect(),
            mode: self.restrict_mode,
            editors: self.editors_shown(&me),
            enforced,
            locked_with_a_password: rules.as_ref().is_some_and(|rules| rules.password.is_some()),
            permission: self.permission_said(rules.as_ref()),
            highlight: self.highlight_regions,
            mine,
        }
    }

    /// How many styles the document allows, for the line under the first box.
    fn allowed_style_count(&self) -> usize {
        self.document.styles().all().iter().filter(|style| !style.locked).count()
    }

    /// The people the pane lists, the group first and the rest by name.
    pub(super) fn editors_shown(&self, me: &str) -> Vec<Person> {
        let marked = self.document.locked_regions();
        let caret = self.document.caret();
        let (start, end) = self.document.selection().unwrap_or((caret, caret));

        let mut names: Vec<String> = vec![EVERYONE.to_owned()];
        let push = |name: &str, into: &mut Vec<String>| {
            let name = name.trim();
            if name.is_empty() || into.iter().any(|had| had.eq_ignore_ascii_case(name)) {
                return;
            }
            into.push(name.to_owned());
        };
        push(me, &mut names);
        for region in &marked {
            push(&region.editor, &mut names);
        }
        for extra in &self.extra_editors {
            push(extra, &mut names);
        }
        // The group stays at the top and the people are sorted under it, which
        // is Word's order and keeps a name from moving about the list as the
        // document changes.
        names[1..].sort_by_key(|name| name.to_lowercase());

        names
            .into_iter()
            .map(|name| {
                let group = name == EVERYONE;
                let stretches = marked
                    .iter()
                    .filter(|region| {
                        if group {
                            region.for_everyone()
                        } else {
                            region.editor.eq_ignore_ascii_case(&name)
                        }
                    })
                    .count();
                // Ticked when the selection sits inside one of theirs, which
                // is what makes the tick a statement about these words rather
                // than about the document.
                let on = start != end
                    && marked.iter().any(|region| {
                        let theirs = if group {
                            region.for_everyone()
                        } else {
                            region.editor.eq_ignore_ascii_case(&name)
                        };
                        theirs && region.covers(start) && region.covers(end)
                    });
                let shown =
                    if group { crate::messages::t("Everyone").to_owned() } else { name.clone() };
                Person { name, shown, on, stretches }
            })
            .collect()
    }

    /// What this person may do, said in a sentence.
    fn permission_said(&self, rules: Option<&Protection>) -> String {
        let Some(rules) = rules else { return String::new() };
        use crate::messages::t;
        match rules.mode {
            Some(EditMode::ReadOnly) => t("This document is protected from editing."),
            Some(EditMode::Comments) => t("You may leave comments and change nothing else."),
            Some(EditMode::TrackedChanges) => t("You may edit, and every change is recorded."),
            Some(EditMode::Forms) => t("You may fill in the form and change nothing else."),
            // A restriction on the formatting alone leaves the words open, and
            // a person told they may not type would be told something untrue.
            None => t("The words are open; the formatting is limited to this document's styles."),
        }
        .to_owned()
    }

    /// The stretches this person is allowed to edit, in the order they appear.
    pub(super) fn regions_i_may_edit(&self) -> Vec<wp_docx::permissions::Locked> {
        let me = super::files::user_name();
        self.document.locked_regions().into_iter().filter(|region| region.admits(&me)).collect()
    }

    /// A press inside the pane.
    pub(super) fn restrict_pane_press(&mut self, x: i32, y: i32) -> Response {
        let Some(hit) = self.restrict_pane.at(x, y) else { return Response::Ignored };
        self.restrict_pane_do(hit)
    }

    /// And what landing on one of its parts means.
    ///
    /// Apart from the press itself so that what a part of the pane does can be
    /// asked without the pane having been drawn: where things land is decided
    /// by the drawing, and a test about what a button does is not a test about
    /// where it sits.
    pub(super) fn restrict_pane_do(&mut self, hit: Hit) -> Response {
        match hit {
            Hit::Close => self.open_protection(),
            Hit::Limit => {
                self.restrict_limiting = !self.restrict_limiting;
                self.needs_redraw = true;
                // Ticking the box is Word's way into the list of styles, so
                // the list opens with it rather than waiting to be asked for
                // a second time.
                if self.restrict_limiting {
                    return self.open_formatting_limits();
                }
                self.document.allow_every_style();
                self.relayout();
                self.edited(true, crate::messages::t("Every style may be used again"))
            }
            Hit::Settings => self.open_formatting_limits(),
            Hit::Restrict => {
                self.restrict_restricting = !self.restrict_restricting;
                self.needs_redraw = true;
                Response::Redraw
            }
            Hit::Mode => self.open_restrict_modes(),
            Hit::Person(index) => self.toggle_person(index),
            Hit::MoreUsers => self.ask_for_users(),
            Hit::Start => self.start_enforcing(),
            Hit::Stop => self.stop_enforcing(),
            Hit::FindNext => self.find_next_region(),
            Hit::ShowAll => self.show_all_regions(),
            Hit::Highlight => {
                self.highlight_regions = !self.highlight_regions;
                self.needs_redraw = true;
                Response::Redraw
            }
        }
    }

    /// The pointer moving over it.
    pub(super) fn restrict_pane_hover(&mut self, x: i32, y: i32) -> bool {
        self.restrict_pane.hover(x, y)
    }

    /// The list of kinds of editing, dropped under the box that says which.
    pub(super) fn open_restrict_modes(&mut self) -> Response {
        if self.close_popup_if(Choice::RestrictMode) {
            return Response::Redraw;
        }
        let items = EditMode::ALL.iter().map(|mode| mode.label().to_owned()).collect();
        let left = self.restrict_pane_left() + 30.0;
        let top = self.ribbon_bottom() + 150.0;
        self.popup = Some(Popup::new(
            Choice::RestrictMode,
            items,
            Some(self.restrict_mode),
            left,
            top,
            WIDTH - 40.0,
        ));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Whichever kind was picked.
    pub(super) fn choose_restrict_mode(&mut self, index: usize) -> Response {
        self.popup = None;
        if index >= EditMode::ALL.len() {
            return Response::Ignored;
        }
        self.restrict_mode = index;
        self.restrict_restricting = true;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Ticking or unticking one of the people against the selection.
    pub(super) fn toggle_person(&mut self, index: usize) -> Response {
        let me = super::files::user_name();
        let Some(person) = self.editors_shown(&me).into_iter().nth(index) else {
            return Response::Ignored;
        };

        let caret = self.document.caret();
        let (start, end) = self.document.selection().unwrap_or((caret, caret));
        if start == end {
            return self
                .report(crate::messages::t("Select the part of the document they may edit first"));
        }

        if person.on {
            // Off again: their pair of markers, and not whichever pair round
            // these words happens to be found first.
            let group = person.name == EVERYONE;
            let theirs = self.document.locked_regions().into_iter().find(|region| {
                let theirs = if group {
                    region.for_everyone()
                } else {
                    region.editor.eq_ignore_ascii_case(&person.name)
                };
                theirs && region.covers(start) && region.covers(end)
            });
            let Some(region) = theirs else { return Response::Ignored };
            let changed = self.document.remove_locked(region.id);
            self.relayout();
            return self.edited(
                changed,
                &crate::messages::with("{0} may no longer edit this", &[&person.shown]),
            );
        }

        let changed = if person.name == EVERYONE {
            self.document.allow_everyone()
        } else {
            self.document.block_authors(&person.name)
        };
        self.relayout();
        self.edited(changed, &crate::messages::with("{0} may edit this", &[&person.shown]))
    }

    /// Word's More users…, which takes names nobody has typed yet.
    pub(super) fn ask_for_users(&mut self) -> Response {
        let dialog = Dialog::new(
            "Add Users",
            vec![
                Field::note("Names separated by semicolons."),
                Field::Text { label: "Names".to_owned(), value: String::new() },
            ],
        );
        self.ask(Asking::MoreUsers, dialog)
    }

    /// Keeps whatever was typed, for as long as the program is running.
    ///
    /// Not written into the document, because a name nobody has been given a
    /// stretch of the document is not a fact about the document. It becomes
    /// one the moment they are ticked against something.
    pub(super) fn apply_more_users(&mut self, dialog: &Dialog) -> Response {
        let said = dialog.said(NAMES);
        let mut added = 0usize;
        for name in said.split(';') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            if self.extra_editors.iter().any(|had| had.eq_ignore_ascii_case(name)) {
                continue;
            }
            self.extra_editors.push(name.to_owned());
            added += 1;
        }
        self.needs_redraw = true;
        if added == 0 {
            return self.report(crate::messages::t("No names were typed"));
        }
        self.report(&crate::messages::with("{0} added to the list", &[&added.to_string()]))
    }

    /// Word's Yes, Start Enforcing Protection: the password, twice.
    pub(super) fn start_enforcing(&mut self) -> Response {
        if !self.restrict_limiting && !self.restrict_restricting {
            return self
                .report(crate::messages::t("Nothing is restricted: tick one of the two boxes"));
        }
        let dialog = Dialog::new(
            "Start Enforcing Protection",
            vec![
                // Word's dialog says this, and it is the truest sentence in
                // it: a password on a document that is not encrypted stops a
                // person, not a program.
                Field::note("The document is not encrypted."),
                Field::note("Anybody who can open the file can take this off."),
                Field::Secret {
                    label: "Enter new password (optional)".to_owned(),
                    value: String::new(),
                },
                Field::Secret {
                    label: "Reenter password to confirm".to_owned(),
                    value: String::new(),
                },
            ],
        );
        self.ask(Asking::Enforce, dialog)
    }

    /// And what that dialog's answer does.
    pub(super) fn apply_enforcement(&mut self, dialog: &Dialog) -> Response {
        let word = dialog.said(WORD);
        if word != dialog.said(AGAIN) {
            let mut again = dialog.clone();
            for row in [WORD, AGAIN] {
                if let Some(Field::Secret { value, .. }) = again.fields.get_mut(row) {
                    value.clear();
                }
            }
            self.status = crate::messages::t("The two passwords are not the same").to_owned();
            return self.ask(Asking::Enforce, again);
        }

        let mode = self
            .restrict_restricting
            .then(|| EditMode::ALL.get(self.restrict_mode).copied())
            .flatten();
        let mut wanted = Protection {
            mode,
            formatting: self.restrict_limiting,
            theme_locked: self.restrict_limiting && self.restrict_theme_locked,
            style_set_locked: self.restrict_limiting && self.restrict_style_set_locked,
            auto_format_override: self.restrict_auto_format,
            password: None,
        };
        if !word.is_empty() {
            let Some(salt) = wp_shell::random::bytes::<SALT>() else {
                return self.report(crate::messages::t(
                    "This machine would not give the random bytes a password needs",
                ));
            };
            wanted = wanted.behind(&word, &salt);
        }

        let changed = self.document.set_protection(Some(&wanted));
        self.needs_redraw = true;
        let said = match mode {
            Some(mode) if self.restrict_limiting => crate::messages::with(
                "Restricted to: {0}, formatting limited",
                &[crate::messages::t(mode.label())],
            ),
            Some(mode) => {
                crate::messages::with("Restricted to: {0}", &[crate::messages::t(mode.label())])
            }
            None => crate::messages::t("Formatting limited to this document's styles").to_owned(),
        };
        self.edited(changed, &said)
    }

    /// Word's Stop Protection, which asks for the password when there is one.
    pub(super) fn stop_enforcing(&mut self) -> Response {
        match self.document.protection_rules() {
            None => Response::Ignored,
            Some(rules) if rules.password.is_none() => self.stop_protecting(),
            Some(rules) => {
                let dialog = Self::unprotect_dialog(&rules);
                self.ask(Asking::Unprotect, dialog)
            }
        }
    }

    /// Word's Find Next Region I Can Edit.
    pub(super) fn find_next_region(&mut self) -> Response {
        let regions = self.regions_i_may_edit();
        if regions.is_empty() {
            return self
                .report(crate::messages::t("There is no part of this document you may edit"));
        }
        let caret = self.document.caret();
        // The next one that begins at or after the caret, and round to the
        // first when there is none. At or after rather than after, because
        // the caret at the very start of the document sits on the first
        // stretch's own beginning, and a rule that wanted a later one would
        // skip the stretch a person is looking straight at. Selecting one
        // leaves the caret at its end, so pressing again always moves on.
        let wanted = regions
            .iter()
            .find(|region| region.start >= caret)
            .or_else(|| regions.first())
            .map(|region| (region.start, region.end));

        let Some((start, end)) = wanted else { return Response::Ignored };
        self.document.move_caret(start, false);
        self.document.move_caret(end, true);
        self.highlight_regions = true;
        self.reveal_caret();
        self.needs_redraw = true;
        self.report(crate::messages::t("The next stretch you may edit"))
    }

    /// Word's Show All Regions I Can Edit.
    pub(super) fn show_all_regions(&mut self) -> Response {
        let regions = self.regions_i_may_edit();
        if regions.is_empty() {
            return self
                .report(crate::messages::t("There is no part of this document you may edit"));
        }
        self.highlight_regions = true;
        // The view goes to the first of them, so that "show all" shows
        // something even when the caret was pages away from any of them.
        if let Some(first) = regions.first() {
            self.document.move_caret(first.start, false);
            self.reveal_caret();
        }
        self.needs_redraw = true;
        if regions.len() == 1 {
            return self.report(crate::messages::t("One stretch you may edit, shaded"));
        }
        self.report(&crate::messages::with(
            "{0} stretches you may edit, shaded",
            &[&regions.len().to_string()],
        ))
    }

    /// Whether the marked stretches are shaded at all.
    ///
    /// Word's tick box, and it is a tick box because the shading is help while
    /// a person is looking for where they may type and clutter once they have
    /// found it.
    pub(super) fn regions_are_highlighted(&self) -> bool {
        self.highlight_regions
    }

    /// Which position to start drawing a bracket at, for each marked stretch.
    ///
    /// The colour is the editor's own, so that a document three people may
    /// edit different parts of reads as three people rather than as one
    /// shaded mess.
    pub(super) fn region_colour(region: &wp_docx::permissions::Locked) -> wp_raster::Color {
        wp_layout::author_color(region.named())
    }

    /// Draws the pane down the right-hand side of the window.
    pub(super) fn draw_restrict_pane(&mut self) {
        if !self.show_restrict {
            return;
        }
        let shown = self.restrict_pane_shown();
        let left = self.restrict_pane_left();
        let top = self.ribbon_bottom();
        let bottom = self.window_bottom();
        let theme = self.theme;

        let mut pane = core::mem::take(&mut self.restrict_pane);
        pane.draw(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            &shown,
            left,
            top,
            bottom,
            &theme,
        );
        self.restrict_pane = pane;
    }

    /// Where the caret would go for a stretch, used by the tests.
    #[cfg(test)]
    pub(super) fn first_region_start(&self) -> Option<wp_docx::TextPosition> {
        self.regions_i_may_edit().first().map(|region| region.start)
    }
}

impl Editor {
    /// Ticks the first three styles in the Formatting Restrictions dialog.
    ///
    /// Two of the pictures want a document whose formatting is limited, and
    /// the way a person gets one is by ticking boxes in that dialog rather
    /// than by a call nobody can make from the keyboard.
    pub(super) fn limit_to_three_styles(&mut self) {
        let Some(dialog) = self.dialog.as_mut() else { return };
        if let Some(crate::chrome::dialog::Field::Check { on, .. }) = dialog.fields.get_mut(0) {
            *on = true;
        }
        if let Some(crate::chrome::dialog::Field::Tree { rows, .. }) = dialog.fields.get_mut(1) {
            for (at, row) in rows.iter_mut().enumerate() {
                row.tick = Some(at < 3);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_shell::{App, Event};

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("One two three four five")));
        body.blocks.push(Block::Paragraph(Paragraph::text("Six seven eight nine ten")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let library: &'static wp_layout::FontLibrary =
            Box::leak(Box::new(wp_layout::FontLibrary::scan_system()));
        let mut editor = Editor::new(library, document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// Selects some words of a paragraph.
    fn select(editor: &mut Editor, paragraph: usize, from: usize, to: usize) {
        editor.document.set_caret(TextPosition::new(paragraph, from));
        editor.document.move_caret(TextPosition::new(paragraph, to), true);
    }

    /// Who is at the keyboard, which the list always holds.
    fn me() -> String {
        super::super::files::user_name()
    }

    /// Which row of the pane's list a name is on.
    fn row_of(editor: &Editor, name: &str) -> usize {
        editor
            .editors_shown(&me())
            .iter()
            .position(|person| person.name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("{name} is not on the list"))
    }

    #[test]
    fn the_button_opens_a_pane_and_not_a_dialog() {
        // The thing J16 named and this closes: Word's Restrict Editing stands
        // beside the document, because the exceptions are about text a person
        // has to be able to select while it is open.
        let mut editor = editor();
        editor.run(crate::chrome::Command::RestrictEditing);
        assert!(editor.show_restrict, "no pane");
        assert!(editor.dialog.is_none(), "it opened a dialog");
        assert!(editor.restrict_pane_width() > 0.0, "the pane takes no room");

        // And pressing it again shuts it.
        editor.run(crate::chrome::Command::RestrictEditing);
        assert!(!editor.show_restrict);
        assert_eq!(editor.restrict_pane_width(), 0.0);
    }

    #[test]
    fn the_list_of_people_is_everyone_then_whoever_the_document_names() {
        let mut editor = editor();
        let shown = editor.editors_shown(&me());
        assert_eq!(shown[0].name, EVERYONE, "the group is not at the top");
        assert!(
            shown.iter().any(|person| person.name == me()),
            "the person at the keyboard is not on it"
        );

        // A stretch somebody else was given puts them on the list, which is
        // how a document restricted elsewhere shows who it was shared with.
        select(&mut editor, 0, 0, 3);
        editor.document.block_authors("Ada Lovelace");
        let shown = editor.editors_shown(&me());
        assert!(
            shown.iter().any(|person| person.name == "Ada Lovelace"),
            "a name out of the document is not listed: {shown:?}"
        );
    }

    #[test]
    fn ticking_somebody_marks_the_selection_and_unticking_takes_it_off_again() {
        let mut editor = editor();
        select(&mut editor, 0, 0, 7);
        let everyone = row_of(&editor, EVERYONE);
        editor.toggle_person(everyone);

        let marked = editor.document.locked_regions();
        assert_eq!(marked.len(), 1, "{marked:?}");
        assert!(marked[0].for_everyone());
        // And the pane says so, which is what makes the tick a statement
        // about these words rather than about the document.
        assert!(editor.editors_shown(&me())[everyone].on);

        editor.toggle_person(everyone);
        assert!(editor.document.locked_regions().is_empty(), "it would not come off");
    }

    #[test]
    fn two_people_on_the_same_words_are_two_pairs_and_both_may_edit() {
        // The format gives a marker one editor, so a stretch two people share
        // is two pairs round the same words. A rule that read the first pair
        // only would let one of them in and shut the other out.
        let mut editor = editor();
        select(&mut editor, 0, 0, 7);
        editor.document.block_authors("Ada Lovelace");
        select(&mut editor, 0, 0, 7);
        let mine = row_of(&editor, &me());
        editor.toggle_person(mine);

        assert_eq!(editor.document.locked_regions().len(), 2, "one pair for two people");
        let both = editor.document.locked_all_at(TextPosition::new(0, 3));
        assert_eq!(both.len(), 2);
        assert!(both.iter().any(|region| region.admits("Ada Lovelace")));
        assert!(both.iter().any(|region| region.admits(&me())));
    }

    #[test]
    fn unticking_takes_off_their_pair_and_leaves_somebody_elses() {
        let mut editor = editor();
        select(&mut editor, 0, 0, 7);
        editor.document.block_authors("Ada Lovelace");
        select(&mut editor, 0, 0, 7);
        let mine = row_of(&editor, &me());
        editor.toggle_person(mine);
        assert_eq!(editor.document.locked_regions().len(), 2);

        select(&mut editor, 0, 0, 7);
        let mine = row_of(&editor, &me());
        editor.toggle_person(mine);

        let left = editor.document.locked_regions();
        assert_eq!(left.len(), 1, "it took off the wrong pair: {left:?}");
        assert_eq!(left[0].editor, "Ada Lovelace");
    }

    #[test]
    fn nothing_selected_says_what_it_needs_rather_than_marking_nothing() {
        let mut editor = editor();
        editor.document.set_caret(TextPosition::new(0, 0));
        let everyone = row_of(&editor, EVERYONE);
        editor.toggle_person(everyone);
        assert!(editor.document.locked_regions().is_empty());
        assert!(editor.status.contains("Select"), "{}", editor.status);
    }

    #[test]
    fn find_next_walks_the_stretches_this_person_may_edit() {
        let mut editor = editor();
        select(&mut editor, 0, 0, 3);
        editor.document.allow_everyone();
        select(&mut editor, 1, 0, 3);
        editor.document.allow_everyone();

        editor.document.set_caret(TextPosition::new(0, 0));
        editor.find_next_region();
        assert_eq!(editor.document.caret().paragraph, 0, "it skipped the first");

        editor.find_next_region();
        assert_eq!(editor.document.caret().paragraph, 1, "it did not go on to the second");

        // And round again, so that a document with one stretch still goes
        // somewhere when the button is pressed twice.
        editor.find_next_region();
        assert_eq!(editor.document.caret().paragraph, 0);
    }

    #[test]
    fn find_next_says_so_when_there_is_nowhere_to_go() {
        let mut editor = editor();
        editor.find_next_region();
        assert!(editor.status.contains("no part of this document"), "{}", editor.status);
    }

    #[test]
    fn a_stretch_that_names_somebody_else_is_not_one_of_mine() {
        let mut editor = editor();
        select(&mut editor, 0, 0, 3);
        editor.document.block_authors("Ada Lovelace");
        assert!(editor.regions_i_may_edit().is_empty(), "somebody else's stretch was counted");
        assert_eq!(editor.first_region_start(), None);
    }

    #[test]
    fn show_all_turns_the_shading_on_and_says_how_many() {
        let mut editor = editor();
        select(&mut editor, 0, 0, 3);
        editor.document.allow_everyone();
        editor.highlight_regions = false;

        editor.show_all_regions();
        assert!(editor.regions_are_highlighted(), "it did not turn the shading on");
        // One rather than "1 stretches", which is what the strip along the
        // bottom says everywhere else in the program.
        assert!(editor.status.contains("One stretch"), "{}", editor.status);
    }

    #[test]
    fn the_tick_box_turns_the_shading_off_again() {
        // Shading is help while a person is looking for where they may type
        // and clutter once they have found it, which is why Word makes it a
        // box rather than always drawing it.
        let mut editor = editor();
        assert!(editor.regions_are_highlighted(), "it starts off");
        editor.restrict_pane_do(Hit::Highlight);
        assert!(!editor.regions_are_highlighted());
    }

    #[test]
    fn more_users_keeps_the_names_that_were_typed() {
        let mut editor = editor();
        editor.ask_for_users();
        let dialog = editor.dialog.as_mut().expect("the dialog");
        if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(NAMES) {
            *value = " Ada Lovelace ; Grace Hopper ;; ".to_owned();
        }
        let dialog = editor.dialog.clone().expect("the dialog");
        editor.apply_more_users(&dialog);

        assert_eq!(
            editor.extra_editors,
            vec!["Ada Lovelace".to_owned(), "Grace Hopper".to_owned()]
        );
        let shown = editor.editors_shown(&me());
        assert!(shown.iter().any(|person| person.name == "Grace Hopper"), "{shown:?}");
    }

    #[test]
    fn the_same_name_twice_is_one_name() {
        let mut editor = editor();
        editor.extra_editors = vec!["Ada Lovelace".to_owned()];
        editor.ask_for_users();
        let dialog = editor.dialog.as_mut().expect("the dialog");
        if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(NAMES) {
            *value = "ada lovelace".to_owned();
        }
        let dialog = editor.dialog.clone().expect("the dialog");
        editor.apply_more_users(&dialog);
        assert_eq!(editor.extra_editors.len(), 1, "{:?}", editor.extra_editors);
    }

    #[test]
    fn the_pane_changes_once_something_is_being_enforced() {
        let mut editor = editor();
        let shown = editor.restrict_pane_shown();
        assert!(!shown.enforced);
        assert!(shown.permission.is_empty(), "it said what a person may do before anything was on");

        editor.run(crate::chrome::Command::RestrictEditing);
        editor.choose_restrict_mode(0);
        editor.start_enforcing();
        editor.finish_dialog(crate::chrome::dialog::Answer::Accept);

        let shown = editor.restrict_pane_shown();
        assert!(shown.enforced, "the pane did not notice");
        assert!(!shown.permission.is_empty(), "it does not say what this person may do");
        assert!(!shown.locked_with_a_password, "an empty box became a password");
    }

    #[test]
    fn each_person_is_drawn_in_the_colour_their_changes_are_drawn_in() {
        // The same name in the margin, in the pane and round the stretch they
        // may edit: two places choosing a colour would choose two colours.
        let mut editor = editor();
        select(&mut editor, 0, 0, 3);
        editor.document.block_authors("Ada Lovelace");
        let region = editor.document.locked_regions().remove(0);
        assert_eq!(Editor::region_colour(&region), wp_layout::author_color("Ada Lovelace"));
    }
}

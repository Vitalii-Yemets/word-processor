//! The Developer tab: the controls a form is made of, and what they do when
//! somebody uses them.
//!
//! # Two ways of writing the same thing
//!
//! A form is boxes a person fills in, and the format has two ways of writing
//! one. The older is a field — see [`wp_docx::forms`] — and the newer is a
//! content control — [`wp_docx::controls`]. Word offers both on this tab, the
//! newer ones as buttons and the older ones behind a menu called Legacy
//! Tools, and so does this: a form somebody was sent is written whichever way
//! the program that made it wrote them.
//!
//! # What a control does when it is used
//!
//! A tick box ticks when it is clicked. A drop-down drops open. That sounds
//! too small to say, and it is the whole of the item this belongs to: a
//! program that could write a tick box into a document and not tick it would
//! be offering a picture of a form rather than a form.

use wp_docx::controls::ControlKind;
use wp_docx::forms::FormKind;
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::dialog::{Dialog, Field};
use crate::chrome::Choice;

use super::dialogs::Asking;

use super::Editor;

/// The three older fields, in the order Word's Legacy Tools lists them.
const LEGACY: &[(FormKind, &str)] = &[
    (FormKind::Text, "Text Form Field"),
    (FormKind::CheckBox, "Check Box Form Field"),
    (FormKind::DropDown, "Drop-Down Form Field"),
];

/// What a drop-down offers when it is made, since a control with an empty
/// list is one nobody can use.
///
/// Word writes an empty list and makes a person fill it in through the
/// Properties dialog. This puts three in, because a drop-down that drops open
/// onto nothing teaches nobody anything about what it is for.
const EXAMPLE_ITEMS: &[&str] = &["First", "Second", "Third"];

/// Where each answer sits in the Properties dialog.
const TITLE: usize = 1;
const TAG: usize = 2;
const NO_DELETE: usize = 3;
const NO_EDIT: usize = 4;
const ITEMS: usize = 6;

impl Editor {
    /// Word's Properties, on the control the caret is in.
    ///
    /// A control made and never changed is a control whose name is wrong for
    /// ever and whose list is whatever it was born with. This is where both
    /// are put right.
    pub(super) fn open_control_properties(&mut self) -> Response {
        let Some(control) = self.document.control_at(self.document.caret()) else {
            return self.report(crate::messages::t("Put the caret in a content control first"));
        };

        let mut fields = vec![
            Field::note(control.kind.label()),
            Field::Text { label: "Title".to_owned(), value: control.alias.clone() },
            Field::Text { label: "Tag".to_owned(), value: control.tag.clone() },
            Field::Check {
                label: "Content control cannot be deleted".to_owned(),
                on: control.locked_delete,
            },
            Field::Check { label: "Contents cannot be edited".to_owned(), on: control.locked_edit },
        ];
        if control.kind.has_items() {
            fields.push(Field::Heading("Drop-down list properties".to_owned()));
            fields.push(Field::Text {
                label: "Items, separated by semicolons".to_owned(),
                value: control
                    .items
                    .iter()
                    .map(|(shown, _)| shown.as_str())
                    .collect::<Vec<&str>>()
                    .join("; "),
            });
        }
        self.ask(
            Asking::ControlProperties,
            Dialog::new("Content Control Properties", fields).wide(520.0),
        )
    }

    /// Writes what the dialog said onto the control.
    pub(super) fn apply_control_properties(&mut self, dialog: &Dialog) -> Response {
        let at = self.document.caret();
        let Some(control) = self.document.control_at(at) else { return Response::Ignored };

        let mut changed = self.document.set_control_properties(
            at,
            &dialog.said(TITLE),
            &dialog.said(TAG),
            dialog.ticked(NO_DELETE),
            dialog.ticked(NO_EDIT),
        );
        if control.kind.has_items() {
            let items: Vec<String> =
                dialog.said(ITEMS).split(';').map(|item| item.trim().to_owned()).collect();
            changed |= self.document.set_control_items(at, &items);
        }
        self.relayout();
        self.edited(changed, "Properties")
    }

    /// Whether the caret is inside a control whose contents are locked.
    ///
    /// The lock is the document's, not a restriction's, so it holds whether
    /// or not anything else does — which is the point of it: a form's labels
    /// stay labels while its boxes are filled in.
    pub(super) fn inside_a_locked_control(&self) -> bool {
        let caret = self.document.caret();
        let (start, end) = self.document.selection().unwrap_or((caret, caret));
        self.document
            .control_at(start)
            .is_some_and(|control| control.locked_edit && control.covers(end))
    }

    /// Whether what is about to be deleted would take a control with it.
    ///
    /// Word refuses, and says so: a control marked as one that cannot be
    /// deleted is one somebody meant to keep.
    pub(super) fn would_delete_a_locked_control(&self) -> Option<String> {
        let caret = self.document.caret();
        let (start, end) = self.document.selection()?;
        let _ = caret;
        self.document
            .controls()
            .into_iter()
            .find(|control| control.locked_delete && control.start >= start && control.end <= end)
            .map(|control| {
                if control.alias.is_empty() {
                    control.kind.label().to_owned()
                } else {
                    control.alias.clone()
                }
            })
    }

    /// Says why a deletion did nothing.
    pub(super) fn refuse_deleting_a_control(&mut self, named: &str) -> Response {
        self.report(&crate::messages::with("{0} is a control that cannot be deleted", &[named]))
    }
}

impl Editor {
    /// Puts one of the content controls in at the caret.
    pub(super) fn insert_content_control(&mut self, which: usize) -> Response {
        let Some(kind) = ControlKind::ALL.get(which).copied() else { return Response::Ignored };
        let items: Vec<String> = if kind.has_items() {
            EXAMPLE_ITEMS.iter().map(|item| (*item).to_owned()).collect()
        } else {
            Vec::new()
        };
        let changed = self.document.insert_control(kind, "", &items);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, kind.label())
    }

    /// The names of the three older fields, for the menu behind Legacy Tools.
    pub(super) fn legacy_field_names() -> Vec<String> {
        use crate::messages::t;
        LEGACY.iter().map(|(_, label)| t(label).to_owned()).collect()
    }

    /// Drops that menu open.
    pub(super) fn open_legacy_fields(&mut self) -> Response {
        self.open_ribbon_menu(Choice::LegacyField)
    }

    /// Puts whichever was chosen in.
    pub(super) fn choose_legacy_field(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((kind, label)) = LEGACY.get(index).copied() else { return Response::Ignored };
        let items: Vec<String> = if kind == FormKind::DropDown {
            EXAMPLE_ITEMS.iter().map(|item| (*item).to_owned()).collect()
        } else {
            Vec::new()
        };
        let changed = self.document.insert_form_field(kind, label, &items);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, label)
    }

    /// What a click on a control does, if it does anything.
    ///
    /// Ticking and dropping open are the two things a control does by being
    /// clicked; everything else a control holds is text, and a click in text
    /// puts the caret there like any other.
    ///
    /// Says whether the click was the control's rather than the caret's.
    pub(super) fn used_a_control(&mut self, at: TextPosition) -> Option<Response> {
        // The newer kind first: a document can hold both, and a content
        // control is what a document made this decade uses.
        if let Some(control) = self.document.control_at(at) {
            match control.kind {
                ControlKind::CheckBox => {
                    let on = !control.checked;
                    let changed = self.document.set_control_checked(control.start, on);
                    self.relayout();
                    return Some(self.edited(changed, if on { "Ticked" } else { "Unticked" }));
                }
                kind if kind.has_items() => {
                    return Some(self.open_control_items(&control));
                }
                _ => {}
            }
        }

        let field = self.document.form_field_at(at)?;
        match field.kind {
            FormKind::CheckBox => {
                let on = !field.checked;
                let changed = self.document.set_check_box(field.start, on);
                self.relayout();
                Some(self.edited(changed, if on { "Ticked" } else { "Unticked" }))
            }
            FormKind::DropDown => Some(self.open_field_items(&field)),
            FormKind::Text => None,
        }
    }

    /// Drops a content control's list open under the caret.
    fn open_control_items(&mut self, control: &wp_docx::controls::Control) -> Response {
        let items: Vec<String> = control.items.iter().map(|(shown, _)| shown.clone()).collect();
        if items.is_empty() {
            return self.report("This control offers nothing to choose");
        }
        self.filling_in = Some(control.start);
        self.open_list_at_caret(Choice::FillIn, items)
    }

    /// And a form field's.
    fn open_field_items(&mut self, field: &wp_docx::forms::FormField) -> Response {
        if field.items.is_empty() {
            return self.report("This field offers nothing to choose");
        }
        self.filling_in = Some(field.start);
        self.open_list_at_caret(Choice::FillIn, field.items.clone())
    }

    /// A list that hangs where the caret is rather than under a button.
    fn open_list_at_caret(&mut self, choice: Choice, items: Vec<String>) -> Response {
        // Under the control rather than under a button: the list belongs to
        // what was clicked, and where that is is where the caret went.
        let (left, top) = match self.caret_rect() {
            Some((x, y, _, height)) => (x, y + height),
            None => (100.0, 200.0),
        };
        self.popup = Some(crate::chrome::Popup::new(choice, items, None, left, top, 220.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Takes what was chosen and puts it in the control it came from.
    pub(super) fn choose_fill_in(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(at) = self.filling_in.take() else { return Response::Ignored };

        let changed = self.document.choose_control_item(at, index)
            || self.document.choose_form_item(at, index);
        self.relayout();
        self.edited(changed, "Chosen")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::Command;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Name: ")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        let end = editor.document.paragraph_text(0).unwrap_or_default().len();
        editor.document.set_caret(TextPosition::new(0, end));
        editor
    }

    #[test]
    fn every_content_control_the_tab_offers_goes_in() {
        for which in 0..ControlKind::ALL.len() {
            let mut editor = editor();
            editor.insert_content_control(which);
            let controls = editor.document.controls();
            assert_eq!(controls.len(), 1, "{which}: {controls:?}");
            assert_eq!(controls[0].kind, ControlKind::ALL[which]);
        }
        // And a number past the end puts nothing in.
        let mut editor = editor();
        editor.insert_content_control(99);
        assert!(editor.document.controls().is_empty());
    }

    #[test]
    fn every_older_field_the_menu_offers_goes_in() {
        assert_eq!(Editor::legacy_field_names().len(), 3);
        for (which, (kind, label)) in LEGACY.iter().enumerate() {
            let mut editor = editor();
            editor.choose_legacy_field(which);
            let fields = editor.document.form_fields();
            assert_eq!(fields.len(), 1, "{label}: {fields:?}");
            assert_eq!(fields[0].kind, *kind, "{label}");
            assert_eq!(fields[0].name, *label, "{label}");
        }
    }

    #[test]
    fn clicking_a_tick_box_ticks_it_and_clicking_again_unticks_it() {
        let mut editor = editor();
        editor.insert_content_control(2);
        let at = editor.document.controls()[0].start;

        assert!(editor.used_a_control(at).is_some(), "the click was not the control's");
        assert!(editor.document.controls()[0].checked, "it did not tick");
        assert!(editor.used_a_control(at).is_some());
        assert!(!editor.document.controls()[0].checked, "it did not untick");
    }

    #[test]
    fn clicking_the_older_tick_box_ticks_it_too() {
        let mut editor = editor();
        editor.choose_legacy_field(1);
        let at = editor.document.form_fields()[0].start;

        assert!(editor.used_a_control(at).is_some());
        assert!(editor.document.form_fields()[0].checked, "the older box did not tick");
    }

    #[test]
    fn clicking_a_drop_down_drops_it_open_and_choosing_puts_the_word_in() {
        let mut editor = editor();
        editor.insert_content_control(4);
        let at = editor.document.controls()[0].start;

        assert!(editor.used_a_control(at).is_some());
        let popup = editor.popup.as_ref().expect("a list");
        assert_eq!(popup.choice, Choice::FillIn);
        assert!(popup.item(0).is_some_and(|item| item == "First"), "{:?}", popup.item(0));

        editor.choose_fill_in(2);
        assert!(editor.document.plain_text().contains("Third"), "{}", editor.document.plain_text());
        assert!(editor.popup.is_none());
    }

    #[test]
    fn clicking_in_ordinary_text_is_not_the_controls_click() {
        let mut editor = editor();
        editor.insert_content_control(1);
        assert!(
            editor.used_a_control(TextPosition::new(0, 0)).is_none(),
            "a click at the start of the paragraph was taken by a control"
        );
    }

    #[test]
    fn clicking_a_text_control_leaves_the_click_to_the_caret() {
        let mut editor = editor();
        editor.insert_content_control(1);
        let at = editor.document.controls()[0].start;
        assert!(editor.used_a_control(at).is_none(), "a text control swallowed the click");
    }
    /// Puts a control in, opens its Properties, and hands the dialog back.
    fn properties_of(editor: &mut Editor, which: usize) {
        editor.insert_content_control(which);
        let at = editor.document.controls()[0].start;
        editor.document.set_caret(at);
        editor.run(Command::ControlProperties);
    }

    #[test]
    fn the_properties_dialog_shows_what_the_control_says_and_writes_back_what_it_is_told() {
        let mut editor = editor();
        properties_of(&mut editor, 1);
        let dialog = editor.dialog.as_mut().expect("the dialog");
        assert_eq!(dialog.title, "Content Control Properties");

        if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(TITLE) {
            *value = "Family name".to_owned();
        }
        if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(TAG) {
            *value = "family".to_owned();
        }
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(NO_DELETE) {
            *on = true;
        }
        editor.finish_dialog(crate::chrome::dialog::Answer::Accept);

        let control = &editor.document.controls()[0];
        assert_eq!(control.alias, "Family name");
        assert_eq!(control.tag, "family");
        assert!(control.locked_delete);
        assert!(!control.locked_edit);
    }

    #[test]
    fn a_list_control_is_offered_its_list_and_a_text_one_is_not() {
        let mut listed = editor();
        properties_of(&mut listed, 4);
        let dialog = listed.dialog.as_ref().expect("the dialog");
        assert!(dialog.fields.len() > ITEMS, "a drop-down was not offered its list");
        assert!(dialog.said(ITEMS).contains("First"), "{}", dialog.said(ITEMS));

        let mut plain = editor();
        properties_of(&mut plain, 1);
        let dialog = plain.dialog.as_ref().expect("the dialog");
        assert!(dialog.fields.len() <= ITEMS, "a text control was offered a list");
    }

    #[test]
    fn the_list_typed_into_the_dialog_is_the_list_the_control_offers() {
        let mut editor = editor();
        properties_of(&mut editor, 4);
        if let Some(Field::Text { value, .. }) =
            editor.dialog.as_mut().expect("the dialog").fields.get_mut(ITEMS)
        {
            *value = "Dr; Professor".to_owned();
        }
        editor.finish_dialog(crate::chrome::dialog::Answer::Accept);

        let items: Vec<String> =
            editor.document.controls()[0].items.iter().map(|(shown, _)| shown.clone()).collect();
        assert_eq!(items, vec!["Dr".to_owned(), "Professor".to_owned()]);
    }

    #[test]
    fn nothing_to_be_had_properties_of_says_so() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.run(Command::ControlProperties);
        assert!(editor.dialog.is_none(), "a dialog opened on no control at all");
        assert!(editor.status.contains("content control"), "{}", editor.status);
    }

    #[test]
    fn a_control_whose_contents_are_locked_takes_no_typing() {
        let mut editor = editor();
        editor.insert_content_control(1);
        let at = editor.document.controls()[0].start;
        editor.document.set_control_properties(at, "Label", "label", false, true);
        editor.document.set_caret(editor.document.controls()[0].start);

        assert!(editor.is_locked(), "a locked control is open");
        let before = editor.document.plain_text();
        editor.handle(Event::Char('x'));
        assert_eq!(editor.document.plain_text(), before, "a locked control took typing");
        editor.run(Command::Format(wp_docx::CharacterFormat::Bold));
        assert!(editor.status.contains("cannot be edited"), "{}", editor.status);

        // And outside it the document is open as it was.
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        assert!(!editor.is_locked(), "the whole document was shut by one control");
    }

    #[test]
    fn a_control_that_cannot_be_deleted_is_not_deleted() {
        let mut editor = editor();
        editor.insert_content_control(1);
        let at = editor.document.controls()[0].start;
        editor.document.set_control_properties(at, "Keep me", "keep", true, false);

        // Everything selected, which is what would take the control with it.
        let end = editor.document.paragraph_text(0).unwrap_or_default().len();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, end));
        assert!(editor.would_delete_a_locked_control().is_some());

        let before = editor.document.plain_text();
        editor.handle(Event::KeyDown { key: wp_shell::Key::Delete, modifiers: Default::default() });
        assert_eq!(editor.document.plain_text(), before, "a control that cannot be deleted went");
        assert!(editor.status.contains("cannot be deleted"), "{}", editor.status);
        assert_eq!(editor.document.controls().len(), 1);
    }
}

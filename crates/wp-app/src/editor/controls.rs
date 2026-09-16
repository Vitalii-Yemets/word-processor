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

use crate::chrome::Choice;

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
}

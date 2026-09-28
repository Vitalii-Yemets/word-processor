//! Word's XML Mapping pane, and the bindings it makes.
//!
//! # What the pane is for
//!
//! A document can carry data as well as words — see [`wp_docx::customxml`]
//! — and a content control can be bound to one node of it, so that the
//! control shows what the node says and writing in the control writes the
//! node. The pane is where that is done: the parts the document carries,
//! the chosen part's tree, and a button that binds a control to the chosen
//! node — the control the caret is in, or a new one put in at the caret.
//!
//! # When the two halves are brought together
//!
//! The node into the control when the document is opened, which is when
//! Word does it: a document whose data was filled in by a program shows the
//! data. The control into the node as it is typed in: after every event
//! that could have changed the document, the control the caret is in is
//! written to its node if it is bound and its words have changed. That is
//! cheap — one control found by the caret — and it keeps the pane, which
//! shows the node's words, true while a person types.

use wp_docx::controls::ControlKind;
use wp_docx::customxml::Binding;
use wp_shell::Response;

use crate::chrome::mappingpane::{Hit, PartRow, Shown, WIDTH};
use crate::chrome::{Choice, Popup};
use crate::messages::t;

use super::Editor;

/// The kinds of control the pane can put in, in Word's order on its menu.
const KINDS: &[(ControlKind, &str)] = &[
    (ControlKind::RichText, "Rich Text"),
    (ControlKind::PlainText, "Plain Text"),
    (ControlKind::CheckBox, "Check Box"),
    (ControlKind::ComboBox, "Combo Box"),
    (ControlKind::DropDown, "Drop-Down List"),
    (ControlKind::Date, "Date Picker"),
];

/// What the pane offers to put in.
#[must_use]
pub fn mapped_control_names() -> Vec<String> {
    KINDS.iter().map(|(_, label)| t(label).to_owned()).collect()
}

impl Editor {
    /// How much of the window the pane takes, when it is open.
    #[must_use]
    pub(super) fn mapping_pane_width(&self) -> f32 {
        if self.show_mapping {
            WIDTH
        } else {
            0.0
        }
    }

    /// Where its left edge is.
    pub(super) fn mapping_pane_left(&self) -> f32 {
        self.view_width as f32 - WIDTH
    }

    /// Whether a point is inside it at all.
    pub(super) fn over_mapping_pane(&self, x: i32) -> bool {
        let x = crate::chrome::mirror::flip(x);
        self.show_mapping && (x as f32) >= self.mapping_pane_left()
    }

    /// Word's XML Mapping Pane button: opens the pane, or shuts it again.
    pub(super) fn open_mapping(&mut self) -> Response {
        if self.show_mapping {
            self.show_mapping = false;
            self.clamp_scroll();
            self.needs_redraw = true;
            return Response::Redraw;
        }
        // The panes down the right-hand side share one strip of window, and
        // two at once is one hidden behind the other.
        self.show_styles = false;
        self.show_restrict = false;
        self.show_signatures = false;
        self.show_text_pane = false;
        self.show_translator = false;
        self.show_mapping = true;
        self.mapping_row = None;
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Everything the pane draws, worked out afresh from the document.
    pub(super) fn mapping_pane_shown(&self) -> Shown {
        let parts = self.document.custom_xml_parts();
        let part = self.mapping_part.min(parts.len().saturating_sub(1));
        let rows = parts.get(part).map(|held| held.rows()).unwrap_or_default();
        Shown {
            parts: parts
                .iter()
                .map(|held| PartRow { label: held.label(), id: held.id.clone() })
                .collect(),
            part,
            row: self.mapping_row.filter(|row| *row < rows.len()),
            rows,
            caret_in_control: self.document.control_at(self.document.caret()).is_some(),
        }
    }

    /// A press inside the pane.
    pub(super) fn mapping_pane_press(&mut self, x: i32, y: i32) -> Response {
        match self.mapping_pane.at(x, y) {
            Some(Hit::Close) => self.open_mapping(),
            Some(Hit::Part(index)) => {
                self.mapping_part = index;
                self.mapping_row = None;
                self.needs_redraw = true;
                Response::Redraw
            }
            Some(Hit::Row(index)) => {
                self.mapping_row = Some(index);
                self.needs_redraw = true;
                Response::Redraw
            }
            Some(Hit::Insert) => self.bind_or_offer(),
            Some(Hit::AddPart) => self.add_custom_xml_from_file(),
            Some(Hit::DeletePart) => self.delete_custom_xml(),
            None => Response::Ignored,
        }
    }

    /// The pointer moving over it.
    pub(super) fn mapping_pane_hover(&mut self, x: i32, y: i32) -> bool {
        self.mapping_pane.hover(x, y)
    }

    /// Draws it, over the document's right-hand edge.
    pub(super) fn draw_mapping_pane(&mut self) {
        if !self.show_mapping {
            return;
        }
        let shown = self.mapping_pane_shown();
        let left = self.mapping_pane_left();
        let top = self.ribbon_bottom();
        let bottom = self.window_bottom();
        let theme = self.theme;

        let mut pane = core::mem::take(&mut self.mapping_pane);
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
        self.mapping_pane = pane;
    }

    /// The binding the pane's chosen node makes, if one is chosen.
    fn chosen_binding(&self) -> Option<Binding> {
        let parts = self.document.custom_xml_parts();
        let part = parts.get(self.mapping_part.min(parts.len().saturating_sub(1)))?;
        if part.id.is_empty() {
            return None;
        }
        let rows = part.rows();
        let row = rows.get(self.mapping_row?)?;
        Some(Binding::to_node(part, row))
    }

    /// The Insert button: binds the control the caret is in, or offers the
    /// kinds of control to put in and bind.
    fn bind_or_offer(&mut self) -> Response {
        let Some(binding) = self.chosen_binding() else {
            return self.report(t("Choose a node of the part to bind a control to"));
        };
        let caret = self.document.caret();
        if self.document.control_at(caret).is_some() {
            let changed = self.document.bind_control(caret, &binding);
            self.has_bindings = self.has_bindings || changed;
            return self.edited(changed, t("Bound"));
        }
        if self.close_popup_if(Choice::MappedControl) {
            return Response::Redraw;
        }
        let (left, top, _, height) = self.mapping_pane.insert_rect;
        self.popup = Some(Popup::new(
            Choice::MappedControl,
            mapped_control_names(),
            None,
            left,
            top + height,
            WIDTH - 40.0,
        ));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts in a control of the kind chosen, bound to the chosen node and
    /// showing what it says.
    pub(super) fn choose_mapped_control(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((kind, label)) = KINDS.get(index).copied() else { return Response::Ignored };
        let Some(binding) = self.chosen_binding() else { return Response::Ignored };
        // The node's name is the control's title, which is what Word gives
        // a control made this way.
        let name = self
            .document
            .custom_xml_parts()
            .get(self.mapping_part)
            .and_then(|part| part.rows().get(self.mapping_row?).map(|row| row.name.clone()))
            .unwrap_or_default();
        if !self.document.insert_control(kind, &name, &[]) {
            return Response::Ignored;
        }
        // The caret went past the control; the control begins where the
        // caret was, which is what it is found by.
        let after = self.document.caret();
        let controls = self.document.controls();
        let Some(control) = controls.iter().rev().find(|control| control.end == after) else {
            return self.edited(true, label);
        };
        let start = control.start;
        let changed = self.document.bind_control(start, &binding);
        self.has_bindings = true;
        self.document.set_caret(start);
        self.edited(true, if changed { t("Bound") } else { label })
    }

    /// Word's Add new part: an XML file, read and put into the document.
    fn add_custom_xml_from_file(&mut self) -> Response {
        const FILTERS: &[wp_shell::dialog::FileFilter] = &[
            wp_shell::dialog::FileFilter { label: "XML files (*.xml)", pattern: "*.xml" },
            wp_shell::dialog::FileFilter { label: "All files (*.*)", pattern: "*.*" },
        ];
        let Some(path) =
            wp_shell::dialog::open_file(t("Add new part"), &super::files::readable(FILTERS))
        else {
            return Response::Ignored;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return self
                .report(&crate::messages::with("Cannot read {0}", &[&path.display().to_string()]));
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        self.add_custom_xml_text(&text)
    }

    /// The same from text, which is also what a test hands over.
    pub(super) fn add_custom_xml_text(&mut self, text: &str) -> Response {
        match self.document.add_custom_xml(text) {
            Some(_) => {
                let count = self.document.custom_xml_parts().len();
                self.mapping_part = count.saturating_sub(1);
                self.mapping_row = None;
                self.update_title();
                self.report(t("The part was added"))
            }
            None => self.report(t("That is not well-formed XML, so it cannot be a part")),
        }
    }

    /// Word's Delete part: the chosen part goes, and the controls bound to
    /// it stay as they are.
    fn delete_custom_xml(&mut self) -> Response {
        let parts = self.document.custom_xml_parts();
        let Some(part) = parts.get(self.mapping_part.min(parts.len().saturating_sub(1))) else {
            return Response::Ignored;
        };
        let id = part.id.clone();
        let removed = if id.is_empty() { false } else { self.document.remove_custom_xml(&id) };
        if !removed {
            return self.report(t("That part has no identifier, so it cannot be taken out"));
        }
        self.mapping_part = 0;
        self.mapping_row = None;
        self.update_title();
        self.report(t("The part was deleted"))
    }

    /// Brings every bound control to what its node says, as Word does when
    /// a document is opened, and notes whether there is anything bound.
    pub(super) fn refresh_bindings(&mut self) {
        self.document.refresh_bound_controls();
        self.has_bindings =
            self.document.controls().iter().any(|control| control.binding.is_some());
    }

    /// After an event that may have changed the document: the control the
    /// caret is in goes into its node, if it is bound.
    pub(super) fn keep_bindings(&mut self) {
        if !self.has_bindings {
            return;
        }
        let caret = self.document.caret();
        if self.document.store_bound_control(caret) {
            self.needs_redraw = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use crate::chrome::Command;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Dear ")));
        let document = Document::create(&body).expect("a document");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    const ORDER: &str = "<o:order xmlns:o=\"http://example.com/order\">\
        <o:customer>Habgood</o:customer><o:item>Nails</o:item></o:order>";

    #[test]
    fn the_pane_opens_from_the_developer_tab_and_lists_the_parts_as_a_tree() {
        let mut editor = editor();
        editor.set_view_option("tab=developer").expect("the Developer tab");
        editor.run(Command::XmlMapping);
        assert!(editor.show_mapping);
        let shown = editor.mapping_pane_shown();
        assert!(shown.parts.is_empty());

        editor.add_custom_xml_text(ORDER);
        let shown = editor.mapping_pane_shown();
        assert_eq!(shown.parts.len(), 1);
        assert_eq!(shown.parts[0].label, "http://example.com/order");
        let names: Vec<(usize, &str)> =
            shown.rows.iter().map(|row| (row.depth, row.name.as_str())).collect();
        assert_eq!(names, [(0, "order"), (1, "customer"), (1, "item")]);
        assert_eq!(shown.rows[1].value, "Habgood");

        editor.run(Command::XmlMapping);
        assert!(!editor.show_mapping);
    }

    #[test]
    fn a_control_put_in_from_the_pane_shows_the_node_and_typing_in_it_writes_the_node() {
        let mut editor = editor();
        editor.set_view_option("tab=developer").expect("the Developer tab");
        editor.run(Command::XmlMapping);
        editor.add_custom_xml_text(ORDER);
        editor.mapping_row = Some(1);
        let end = editor.document.paragraph_text(0).unwrap_or_default().chars().count();
        editor.document.set_caret(TextPosition::new(0, end));

        // Plain Text is the second on the menu.
        editor.choose_mapped_control(1);
        assert_eq!(editor.document.plain_text().trim(), "Dear Habgood");
        let control = editor.document.controls().pop().expect("the control");
        assert_eq!(control.alias, "customer");
        let binding = control.binding.clone().expect("it is bound");
        assert_eq!(binding.xpath, "/ns0:order[1]/ns0:customer[1]");

        // Typing into the control writes the node, event by event.
        let inside = TextPosition::new(control.start.paragraph, control.end.offset);
        editor.document.set_caret(inside);
        for character in "-Smith".chars() {
            editor.handle(Event::Char(character));
        }
        assert_eq!(editor.document.custom_xml_text(&binding).as_deref(), Some("Habgood-Smith"));
        let shown = editor.mapping_pane_shown();
        assert_eq!(shown.rows[1].value, "Habgood-Smith");
    }

    #[test]
    fn the_control_the_caret_is_in_is_bound_and_a_reopened_document_shows_the_node() {
        let mut editor = editor();
        editor.run(Command::XmlMapping);
        editor.add_custom_xml_text(ORDER);
        editor.document.set_caret(TextPosition::new(0, 0));
        assert!(editor.document.insert_control(ControlKind::PlainText, "Who", &[]));
        let control = editor.document.controls().pop().expect("the control");
        editor.document.set_caret(control.start);
        editor.mapping_row = Some(1);
        editor.bind_or_offer();
        assert!(
            editor.document.plain_text().starts_with("Habgood"),
            "{}",
            editor.document.plain_text()
        );

        // The node changed under the document — by a program, say — and
        // the document opened again shows the change.
        let binding = editor.document.controls()[0].binding.clone().expect("bound");
        assert!(editor.document.set_custom_xml_text(&binding, "Okafor"));
        let bytes = editor.document.save().expect("saving");
        let mut again = self::tests::editor();
        again.set_document(Document::open(&bytes).expect("reopening"), None);
        assert!(
            again.document.plain_text().starts_with("Okafor"),
            "{}",
            again.document.plain_text()
        );
        assert!(!again.document.is_modified(), "opening is not an edit");
    }

    #[test]
    fn xml_that_is_not_well_formed_is_refused_with_a_reason() {
        let mut editor = editor();
        editor.add_custom_xml_text("<order><customer></order>");
        assert!(editor.status.contains("well-formed"), "{}", editor.status);
        assert!(editor.document.custom_xml_parts().is_empty());
    }
}

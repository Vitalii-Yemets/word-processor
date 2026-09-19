//! Content controls: the form field written again, twenty years later.
//!
//! # Why there are two of everything
//!
//! [`crate::forms`] is the older way: a field written as five runs, with what
//! it is written inside the first of them. It is what Word 97 wrote and what
//! a form protection still works on. Content controls are the newer way,
//! written as one element wrapping its own content, and they are what a
//! document made this decade uses. They do more — a control can hold whole
//! paragraphs, a picture, a date with a format — and they are not tied to
//! protecting the document.
//!
//! Both are here because both are in the documents people have.
//!
//! # How one is written
//!
//! ```text
//! <w:sdt>
//!   <w:sdtPr>
//!     <w:alias w:val="Surname"/><w:tag w:val="surname"/><w:id w:val="1"/>
//!     <w:text/>                       ← what kind it is
//!   </w:sdtPr>
//!   <w:sdtContent><w:r><w:t>Habgood</w:t></w:r></w:sdtContent>
//! </w:sdt>
//! ```
//!
//! The kind is said by which element is in the properties, and a control with
//! none of them is a rich text control — the one that holds anything. A tick
//! box says so in a namespace Word added in 2010, which is why that one
//! element is written with its own declaration on it.
//!
//! # The picture control
//!
//! One more of the same shape, holding a drawing rather than words: `w:picture`
//! in the properties, and a run with a `w:drawing` in the content. It is put
//! in holding a picture — the placeholder the program draws — because a
//! control holding nothing has no width and nothing to click, and it is
//! clicked to be given the picture it is for. The controls that hold whole
//! paragraphs — the repeating section and the building-block gallery — are
//! [`crate::blockcontrols`].

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::{edit, read, Document, TextPosition};

/// The namespace Word added in 2010, which is where a tick box lives.
const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

/// What sort of control it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControlKind {
    /// Anything at all, formatting included. A control with no kind named is
    /// one of these, which is what the format says.
    #[default]
    RichText,
    /// Words, and no formatting of their own.
    PlainText,
    /// One of a list, and nothing else.
    DropDown,
    /// One of a list, or something typed instead.
    ComboBox,
    /// A date, with a format it is written in.
    Date,
    /// Ticked or not.
    CheckBox,
    /// A picture, chosen by clicking the control.
    Picture,
}

impl ControlKind {
    /// The element in the properties that says so, if there is one.
    #[must_use]
    fn element(self) -> Option<&'static str> {
        match self {
            Self::RichText => None,
            Self::PlainText => Some("text"),
            Self::DropDown => Some("dropDownList"),
            Self::ComboBox => Some("comboBox"),
            Self::Date => Some("date"),
            Self::CheckBox => Some("checkbox"),
            Self::Picture => Some("picture"),
        }
    }

    /// What a person is shown when picking one.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::RichText => "Rich Text Content Control",
            Self::PlainText => "Plain Text Content Control",
            Self::DropDown => "Drop-Down List Content Control",
            Self::ComboBox => "Combo Box Content Control",
            Self::Date => "Date Picker Content Control",
            Self::CheckBox => "Check Box Content Control",
            Self::Picture => "Picture Content Control",
        }
    }

    /// Every one that can be put into a document, in Word's order — with
    /// the picture last, because it is put in by its own way and the ones
    /// before it are numbered on the ribbon.
    pub const ALL: &'static [Self] = &[
        Self::RichText,
        Self::PlainText,
        Self::CheckBox,
        Self::ComboBox,
        Self::DropDown,
        Self::Date,
        Self::Picture,
    ];

    /// Whether it offers a list to pick from.
    #[must_use]
    pub fn has_items(self) -> bool {
        matches!(self, Self::DropDown | Self::ComboBox)
    }
}

/// One content control, and where its content is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub kind: ControlKind,
    /// The name Word shows on the control's tab. Empty where it has none.
    pub alias: String,
    /// The name a program uses, which a person never sees.
    pub tag: String,
    /// What a list offers: what is shown, and what is meant by it.
    pub items: Vec<(String, String)>,
    /// Whether a tick box is ticked.
    pub checked: bool,
    /// Whether the control may be taken out of the document, and whether what
    /// is inside it may be changed.
    ///
    /// Word's `w:lock`, and its Properties dialog's two tick boxes. A form
    /// somebody is meant to fill in and not dismantle is a form whose boxes
    /// cannot be deleted and whose labels cannot be retyped, and those are
    /// these two.
    pub locked_delete: bool,
    pub locked_edit: bool,
    /// Where the content starts and ends.
    pub start: TextPosition,
    pub end: TextPosition,
    /// The node of a custom XML part it is bound to, if it is bound. See
    /// [`crate::customxml`].
    pub binding: Option<crate::customxml::Binding>,
}

impl Control {
    /// Whether a position is inside the content.
    #[must_use]
    pub fn covers(&self, position: TextPosition) -> bool {
        position >= self.start && position <= self.end
    }
}

/// What a tick box shows, ticked and not.
///
/// Word's own two characters, and the same ones [`crate::forms`] uses: a
/// program that drew a different box for the two kinds of tick box would be
/// showing a difference that is not there.
pub const TICKED: char = crate::forms::TICKED;
pub const UNTICKED: char = crate::forms::UNTICKED;

impl Document {
    /// Every content control in the body, in the order they appear.
    #[must_use]
    pub fn controls(&self) -> Vec<Control> {
        let mut found = Vec::new();
        for (index, paragraph) in self.paragraph_elements().into_iter().enumerate() {
            let mut offset = 0usize;
            walk(paragraph, index, &mut offset, &mut found);
        }
        found
    }

    /// The control a position is inside.
    ///
    /// The same rule as a form field's: a position shared by two controls
    /// belongs to the one beginning there.
    #[must_use]
    pub fn control_at(&self, position: TextPosition) -> Option<Control> {
        let controls = self.controls();
        controls
            .iter()
            .find(|control| control.start == position)
            .or_else(|| controls.iter().find(|control| control.covers(position)))
            .cloned()
    }

    /// Puts a content control in at the caret.
    pub fn insert_control(&mut self, kind: ControlKind, alias: &str, items: &[String]) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(path) = crate::position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };
        crate::format::split_runs_at_offset(paragraph, caret.offset);
        let at = edit::child_position_at_offset(paragraph, caret.offset);
        let shown = shown_for(kind, alias, items);
        paragraph.insert_element(at, written(kind, alias, items, prefix.as_deref()));

        // The caret goes past what was put in, which is where somebody who
        // put a control in wants to carry on typing. Without this, two
        // controls put in one after another come out in the other order.
        self.set_caret(TextPosition::new(caret.paragraph, caret.offset + shown.len()));
        self.mark_modified();
        true
    }

    /// Puts a picture control in at the caret, holding this picture: the
    /// placeholder the program draws, until a person chooses one.
    pub fn insert_picture_control(
        &mut self,
        bytes: &[u8],
        extension: &str,
        width_emu: i64,
        height_emu: i64,
    ) -> Result<bool, crate::Error> {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let id = self.adopt_picture(bytes, extension)?;
        let prefix = self.prefix();

        let Some(path) = crate::position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return Ok(false);
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return Ok(false);
        };
        crate::format::split_runs_at_offset(paragraph, caret.offset);
        let at = edit::child_position_at_offset(paragraph, caret.offset);
        let mut control = written(ControlKind::Picture, "", &[], prefix.as_deref());
        if let Some(content) = control.child_mut(Some(read::W), "sdtContent") {
            content.children.clear();
            let mut run = Element::new(&edit::name_with(prefix.as_deref(), "r"), Some(read::W));
            run.push_element(edit::drawing_element(&id, width_emu, height_emu, prefix.as_deref()));
            content.push_element(run);
        }
        paragraph.insert_element(at, control);
        // Past the picture, which is one character of the text.
        self.set_caret(TextPosition::new(caret.paragraph, caret.offset + 1));
        self.mark_modified();
        Ok(true)
    }

    /// Puts a picture into the picture control at a position, in place of
    /// the one it holds.
    pub fn set_control_picture(
        &mut self,
        at: TextPosition,
        bytes: &[u8],
        extension: &str,
        width_emu: i64,
        height_emu: i64,
    ) -> Result<bool, crate::Error> {
        let Some(control) = self.control_at(at) else { return Ok(false) };
        if control.kind != ControlKind::Picture {
            return Ok(false);
        }
        let id = self.adopt_picture(bytes, extension)?;
        Ok(self.change_control(&control, move |_, content, prefix| {
            content.children.clear();
            let mut run = Element::new(&edit::name_with(prefix, "r"), Some(read::W));
            run.push_element(edit::drawing_element(&id, width_emu, height_emu, prefix));
            content.push_element(run);
        }))
    }

    /// Ticks or unticks the tick box at a position.
    pub fn set_control_checked(&mut self, at: TextPosition, on: bool) -> bool {
        let Some(control) = self.control_at(at) else { return false };
        if control.kind != ControlKind::CheckBox {
            return false;
        }
        self.change_control(&control, |properties, content, prefix| {
            if let Some(box_of) = properties.child_mut(Some(W14), "checkbox") {
                set_w14(box_of, "checked", if on { "1" } else { "0" });
            }
            write_content(content, &if on { TICKED } else { UNTICKED }.to_string(), prefix);
        })
    }

    /// Picks one of a list's entries.
    pub fn choose_control_item(&mut self, at: TextPosition, index: usize) -> bool {
        let Some(control) = self.control_at(at) else { return false };
        if !control.kind.has_items() {
            return false;
        }
        let Some((shown, _)) = control.items.get(index).cloned() else { return false };
        self.change_control(&control, |_, content, prefix| write_content(content, &shown, prefix))
    }

    /// Word's Properties: what a control is called, and what may be done to
    /// it.
    ///
    /// The identifier a program uses — Word calls it the tag — is kept beside
    /// the name a person reads, because the two are for different readers and
    /// a control that had only one of them would be missing whichever the
    /// other reader wanted.
    pub fn set_control_properties(
        &mut self,
        at: TextPosition,
        alias: &str,
        tag: &str,
        locked_delete: bool,
        locked_edit: bool,
    ) -> bool {
        let Some(control) = self.control_at(at) else { return false };
        let (alias, tag) = (alias.trim().to_owned(), tag.trim().to_owned());
        self.change_control(&control, move |properties, _, prefix| {
            for (local, value) in [("alias", alias.as_str()), ("tag", tag.as_str())] {
                properties.remove_children_named(Some(read::W), local);
                if value.is_empty() {
                    continue;
                }
                let mut element = Element::new(&edit::name_with(prefix, local), Some(read::W));
                element.set_namespaced_attribute(&edit::name_with(prefix, "val"), read::W, value);
                properties.insert_element(0, element);
            }

            properties.remove_children_named(Some(read::W), "lock");
            if locked_delete || locked_edit {
                let mut element = Element::new(&edit::name_with(prefix, "lock"), Some(read::W));
                element.set_namespaced_attribute(
                    &edit::name_with(prefix, "val"),
                    read::W,
                    lock_word(locked_delete, locked_edit),
                );
                properties.push_element(element);
            }
        })
    }

    /// What a list offers, set after the control was made.
    ///
    /// Word's Properties dialog has Add, Modify and Remove under the list;
    /// this takes the whole list, because a list is a small thing and
    /// replacing it is one edit to undo rather than three.
    pub fn set_control_items(&mut self, at: TextPosition, items: &[String]) -> bool {
        let Some(control) = self.control_at(at) else { return false };
        if !control.kind.has_items() {
            return false;
        }
        let Some(local) = control.kind.element().map(str::to_owned) else { return false };
        let items: Vec<String> = items
            .iter()
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect();
        if items.is_empty() {
            return false;
        }
        let shown = items[0].clone();

        self.change_control(&control, move |properties, content, prefix| {
            properties.remove_children_named(Some(read::W), &local);
            let mut list = Element::new(&edit::name_with(prefix, &local), Some(read::W));
            for item in &items {
                let mut entry = Element::new(&edit::name_with(prefix, "listItem"), Some(read::W));
                entry.set_namespaced_attribute(
                    &edit::name_with(prefix, "displayText"),
                    read::W,
                    item,
                );
                entry.set_namespaced_attribute(&edit::name_with(prefix, "value"), read::W, item);
                list.push_element(entry);
            }
            properties.push_element(list);
            // What it shows has to be one of what it offers, or the control
            // would be showing an answer that is no longer on its list.
            write_content(content, &shown, prefix);
        })
    }

    /// Writes words into a control's content.
    pub fn set_control_text(&mut self, at: TextPosition, text: &str) -> bool {
        let Some(control) = self.control_at(at) else { return false };
        if control.kind == ControlKind::CheckBox {
            return false;
        }
        self.change_control(&control, |properties, content, prefix| {
            // A control showing its placeholder stops showing it the moment
            // somebody writes in it, which is what the flag is for.
            properties.remove_children_named(Some(read::W), "showingPlcHdr");
            write_content(content, text, prefix);
        })
    }

    /// Finds the control's two halves and hands them to whoever is changing
    /// it.
    pub(crate) fn change_control(
        &mut self,
        control: &Control,
        change: impl FnOnce(&mut Element, &mut Element, Option<&str>),
    ) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(path) =
            crate::position::paragraph_path(&self.tree().root, control.start.paragraph)
        else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };
        let mut path = Vec::new();
        if !control_path(paragraph, control.start.offset, &mut 0, &mut path) {
            return false;
        }
        let Some(element) = edit::element_at_path_mut(paragraph, &path) else { return false };

        let (mut properties, mut content) = (None, None);
        for child in &mut element.children {
            let Some(child) = child.as_element_mut() else { continue };
            match child.local_name() {
                "sdtPr" => properties = Some(child),
                "sdtContent" => content = Some(child),
                _ => {}
            }
        }
        let (Some(properties), Some(content)) = (properties, content) else { return false };
        change(properties, content, prefix.as_deref());

        self.mark_modified();
        true
    }
}

/// Walks a paragraph, counting characters and finding the controls.
fn walk(element: &Element, paragraph: usize, offset: &mut usize, found: &mut Vec<Control>) {
    for node in &element.children {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(read::W) {
            continue;
        }
        match child.local_name() {
            "sdt" => {
                let properties = child.child(Some(read::W), "sdtPr");
                let start = TextPosition::new(paragraph, *offset);
                if let Some(content) = child.child(Some(read::W), "sdtContent") {
                    walk(content, paragraph, offset, found);
                }
                found.push(Control {
                    kind: kind_of(properties),
                    alias: said(properties, "alias"),
                    tag: said(properties, "tag"),
                    items: items_of(properties),
                    checked: properties
                        .and_then(|properties| properties.child(Some(W14), "checkbox"))
                        .and_then(|box_of| box_of.child(Some(W14), "checked"))
                        .and_then(|checked| checked.attribute(Some(W14), "val"))
                        .is_some_and(|value| matches!(value, "1" | "true" | "on")),
                    locked_delete: locked(properties).0,
                    locked_edit: locked(properties).1,
                    start,
                    end: TextPosition::new(paragraph, *offset),
                    binding: binding_of(properties),
                });
            }
            "t" => *offset += child.text_content().len(),
            "instrText" => {}
            // A picture, a hyphen written as an element, a note's mark: each
            // is one thing in the text, counted as the text is counted.
            _ if crate::edit::atomic_text(child).is_some() => {
                *offset += crate::edit::atomic_text(child).map_or(0, str::len);
            }
            "cr" => *offset += 1,
            _ => walk(child, paragraph, offset, found),
        }
    }
}

/// What `w:dataBinding` says, where there is one.
fn binding_of(properties: Option<&Element>) -> Option<crate::customxml::Binding> {
    let binding = properties?.child(Some(read::W), "dataBinding")?;
    let attribute = |local: &str| {
        binding
            .attribute(Some(read::W), local)
            .or_else(|| binding.attribute_by_name(&format!("w:{local}")))
            .unwrap_or_default()
            .to_owned()
    };
    Some(crate::customxml::Binding {
        prefixes: attribute("prefixMappings"),
        xpath: attribute("xpath"),
        store_item: attribute("storeItemID"),
    })
}

/// What a property says, by its `w:val`.
fn said(properties: Option<&Element>, local: &str) -> String {
    properties
        .and_then(|properties| properties.child(Some(read::W), local))
        .and_then(|child| child.attribute(Some(read::W), "val"))
        .unwrap_or_default()
        .to_owned()
}

/// What `w:lock` says: whether the control may be deleted, and whether its
/// contents may be edited.
///
/// One attribute with four words in it, which is how the format writes two
/// answers: `sdtLocked` is the control, `contentLocked` is what is in it,
/// `sdtContentLocked` is both, and anything else is neither.
fn locked(properties: Option<&Element>) -> (bool, bool) {
    match said(properties, "lock").as_str() {
        "sdtLocked" => (true, false),
        "contentLocked" => (false, true),
        "sdtContentLocked" => (true, true),
        _ => (false, false),
    }
}

/// And the word for a pair of answers.
#[must_use]
fn lock_word(delete: bool, edit: bool) -> &'static str {
    match (delete, edit) {
        (true, true) => "sdtContentLocked",
        (true, false) => "sdtLocked",
        (false, true) => "contentLocked",
        (false, false) => "unlocked",
    }
}

/// Which kind the properties name.
fn kind_of(properties: Option<&Element>) -> ControlKind {
    let Some(properties) = properties else { return ControlKind::RichText };
    if properties.child(Some(W14), "checkbox").is_some() {
        return ControlKind::CheckBox;
    }
    for kind in ControlKind::ALL {
        let Some(local) = kind.element() else { continue };
        if properties.child(Some(read::W), local).is_some() {
            return *kind;
        }
    }
    ControlKind::RichText
}

/// What a list offers.
fn items_of(properties: Option<&Element>) -> Vec<(String, String)> {
    let Some(properties) = properties else { return Vec::new() };
    for local in ["dropDownList", "comboBox"] {
        let Some(list) = properties.child(Some(read::W), local) else { continue };
        return list
            .child_elements()
            .filter(|child| child.is(Some(read::W), "listItem"))
            .map(|child| {
                let shown =
                    child.attribute(Some(read::W), "displayText").unwrap_or_default().to_owned();
                let meant = child.attribute(Some(read::W), "value").unwrap_or_default().to_owned();
                (if shown.is_empty() { meant.clone() } else { shown }, meant)
            })
            .collect();
    }
    Vec::new()
}

/// What a control shows before anybody has written in it.
///
/// Word calls this the placeholder and shows it in grey. A control with
/// nothing in it at all would be a control nobody could find: it has no width
/// and nothing to click.
fn shown_for(kind: ControlKind, alias: &str, items: &[String]) -> String {
    match kind {
        ControlKind::CheckBox => UNTICKED.to_string(),
        ControlKind::DropDown | ControlKind::ComboBox => {
            items.first().cloned().unwrap_or_else(|| "Choose an item".to_owned())
        }
        ControlKind::Date => "Enter a date".to_owned(),
        // A picture control is given its picture by whoever puts it in.
        ControlKind::Picture => String::new(),
        _ if alias.is_empty() => "Enter text".to_owned(),
        _ => format!("Enter {}", alias.to_lowercase()),
    }
}

/// Builds one.
fn written(kind: ControlKind, alias: &str, items: &[String], prefix: Option<&str>) -> Element {
    let named = |local: &str| edit::name_with(prefix, local);
    let valued = |local: &str, value: &str| {
        let mut element = Element::new(&named(local), Some(read::W));
        element.set_namespaced_attribute(&named("val"), read::W, value);
        element
    };

    let mut properties = Element::new(&named("sdtPr"), Some(read::W));
    if !alias.is_empty() {
        properties.push_element(valued("alias", alias));
        properties.push_element(valued("tag", &alias.to_lowercase().replace(' ', "")));
    }
    match kind {
        ControlKind::RichText => {}
        ControlKind::CheckBox => {
            // The one element from the namespace Word added later, written
            // with its own declaration so that a document that never heard of
            // that namespace still reads.
            let mut box_of = Element::new("w14:checkbox", Some(W14));
            box_of.declarations.push((Some("w14".to_owned()), W14.to_owned()));
            set_w14(&mut box_of, "checked", "0");
            set_w14(&mut box_of, "checkedState", &format!("{:04X}", TICKED as u32));
            set_w14(&mut box_of, "uncheckedState", &format!("{:04X}", UNTICKED as u32));
            properties.push_element(box_of);
        }
        ControlKind::DropDown | ControlKind::ComboBox => {
            let local = if kind == ControlKind::DropDown { "dropDownList" } else { "comboBox" };
            let mut list = Element::new(&named(local), Some(read::W));
            for item in items {
                let mut entry = Element::new(&named("listItem"), Some(read::W));
                entry.set_namespaced_attribute(&named("displayText"), read::W, item);
                entry.set_namespaced_attribute(&named("value"), read::W, item);
                list.push_element(entry);
            }
            properties.push_element(list);
        }
        ControlKind::Date => {
            let mut date = Element::new(&named("date"), Some(read::W));
            date.push_element(valued("dateFormat", "d MMMM yyyy"));
            properties.push_element(date);
        }
        ControlKind::PlainText => {
            properties.push_element(Element::new(&named("text"), Some(read::W)));
        }
        ControlKind::Picture => {
            properties.push_element(Element::new(&named("picture"), Some(read::W)));
        }
    }

    let shown = shown_for(kind, alias, items);

    let mut content = Element::new(&named("sdtContent"), Some(read::W));
    write_content(&mut content, &shown, prefix);

    let mut control = Element::new(&named("sdt"), Some(read::W));
    control.push_element(properties);
    control.push_element(content);
    control
}

/// Puts words inside a control, taking whatever was there out.
fn write_content(content: &mut Element, text: &str, prefix: Option<&str>) {
    let named = |local: &str| edit::name_with(prefix, local);
    // The formatting of what was there is kept where there is any: a control
    // somebody made bold should not come back plain because they ticked it.
    let properties = content
        .child_elements()
        .find(|child| child.is(Some(read::W), "r"))
        .and_then(|run| run.child(Some(read::W), "rPr"))
        .cloned();

    content.children.clear();
    let mut run = Element::new(&named("r"), Some(read::W));
    if let Some(properties) = properties {
        run.push_element(properties);
    }
    let mut written = Element::new(&named("t"), Some(read::W));
    edit::preserve_space_if_needed(&mut written, text);
    written.set_text(text);
    run.push_element(written);
    content.push_element(run);
}

/// Sets one of the tick box's own attributes.
fn set_w14(element: &mut Element, local: &str, value: &str) {
    let name = format!("w14:{local}");
    match element.child_mut(Some(W14), local) {
        Some(child) => child.set_namespaced_attribute("w14:val", W14, value),
        None => {
            let mut child = Element::new(&name, Some(W14));
            child.set_namespaced_attribute("w14:val", W14, value);
            element.push_element(child);
        }
    }
}

/// Where the control whose content begins at an offset is, as the indices to
/// walk down through.
///
/// Found on the tree as it is read and then walked again to change it,
/// because a reference handed back out of a search would borrow the whole
/// paragraph for as long as it lived.
fn control_path(
    element: &Element,
    wanted: usize,
    offset: &mut usize,
    path: &mut Vec<usize>,
) -> bool {
    for (at, node) in element.children.iter().enumerate() {
        let Some(child) = node.as_element() else { continue };
        if child.namespace.as_deref() != Some(read::W) {
            continue;
        }
        match child.local_name() {
            "sdt" => {
                if *offset == wanted {
                    path.push(at);
                    return true;
                }
                path.push(at);
                if control_path(child, wanted, offset, path) {
                    return true;
                }
                path.pop();
            }
            "t" => *offset += child.text_content().len(),
            "instrText" => {}
            _ if crate::edit::atomic_text(child).is_some() => {
                *offset += crate::edit::atomic_text(child).map_or(0, str::len);
            }
            "cr" => *offset += 1,
            _ => {
                path.push(at);
                if control_path(child, wanted, offset, path) {
                    return true;
                }
                path.pop();
            }
        }
    }
    false
}

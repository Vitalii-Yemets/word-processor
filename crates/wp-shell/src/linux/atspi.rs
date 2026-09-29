//! AT-SPI: what a screen reader on Linux is told.
//!
//! # How a screen reader reads a program on Linux
//!
//! Over a bus of its own — the accessibility bus, whose address the session
//! bus gives — on which every program that can be read puts a tree of
//! objects: the application, its windows, and what is in them, each with
//! its role, its name, its states and its place on the screen, and with the
//! interfaces of what it is — something that can be pressed, text that can
//! be read and selected, a value between two ends. The program registers
//! its tree with the registry on that bus, answers the screen reader's calls
//! about it, and tells it with signals what changes. Orca reads GTK and Qt
//! programs this way, and now this one.
//!
//! # What this program says
//!
//! What UI Automation is told on Windows — the application sees to that,
//! through the shell's accessibility methods on [`crate::App`]. The objects
//! are the application, the window, and one for each control the
//! application describes, inside whichever control it says it is inside: a
//! dialog's fields in the dialog, a menu's items in the menu, a pane's list
//! in the pane. The document's text is read by character, word, sentence and
//! line — lines as the layout breaks them — with how each stretch is set,
//! the caret and the selection, which can be set, and each stretch's place.
//! A box's text, a list's choice and the status strip's message are read as
//! text too, and a box can be written; a scroll bar says where it is.
//!
//! The bus is D-Bus, spoken by [`super::dbus`]; this is the AT-SPI protocol
//! on top of it, written out against at-spi2-core's interface descriptions.

use std::cell::RefCell;
use std::time::Duration;

use super::dbus::{Connection, Kind, Message, Value};
use crate::accessibility::{Element, Role, TextAttributes, TextState};

/// What the adaptor asks of the shell and the application, for the window
/// being read.
pub(crate) trait Window {
    fn title(&mut self) -> String;
    /// Where the drawing area's top left is on the screen, in the screen's
    /// pixels; the origin where the window system does not say.
    fn origin(&mut self) -> (i32, i32);
    /// The drawing area's size, in its own pixels.
    fn size(&mut self) -> (i32, i32);
    /// How many of the screen's pixels one of the application's is.
    fn scale(&mut self) -> f32;
    fn elements(&mut self) -> Vec<Element>;
    fn text(&mut self) -> Option<TextState>;
    fn rects(&mut self, start: usize, end: usize) -> Vec<(i32, i32, i32, i32)>;
    fn lines(&mut self) -> Vec<(usize, usize)>;
    fn attributes(&mut self, offset: usize) -> Option<(TextAttributes, usize, usize)>;
    fn invoke(&mut self, id: u64);
    fn select(&mut self, start: usize, end: usize);
    fn set_value(&mut self, id: u64, value: &str);
}

/// The application's object, which the registry is given.
const ROOT: &str = "/org/a11y/atspi/accessible/root";
/// The window's.
const FRAME: &str = "/org/a11y/atspi/accessible/frame";
/// A control's, followed by its number.
const ELEMENT: &str = "/org/a11y/atspi/accessible/e";
/// Where the cache of the whole tree is asked for.
const CACHE: &str = "/org/a11y/atspi/cache";
/// The object that is no object.
const NULL: &str = "/org/a11y/atspi/null";

/// Roles, as AT-SPI numbers them.
mod role {
    pub(super) const ALERT: u32 = 2;
    pub(super) const CHECK_BOX: u32 = 7;
    pub(super) const COMBO_BOX: u32 = 11;
    pub(super) const DIALOG: u32 = 16;
    pub(super) const FRAME: u32 = 23;
    pub(super) const LABEL: u32 = 29;
    pub(super) const LIST: u32 = 31;
    pub(super) const LIST_ITEM: u32 = 32;
    pub(super) const MENU: u32 = 33;
    pub(super) const MENU_ITEM: u32 = 35;
    pub(super) const PAGE_TAB: u32 = 37;
    pub(super) const PANEL: u32 = 39;
    pub(super) const PUSH_BUTTON: u32 = 43;
    pub(super) const SCROLL_BAR: u32 = 48;
    pub(super) const STATUS_BAR: u32 = 54;
    pub(super) const TOGGLE_BUTTON: u32 = 62;
    pub(super) const RULER: u32 = 74;
    pub(super) const APPLICATION: u32 = 75;
    pub(super) const ENTRY: u32 = 79;
    pub(super) const DOCUMENT_TEXT: u32 = 94;
}

/// States, as AT-SPI numbers the bits.
mod state {
    pub(super) const ACTIVE: u32 = 1;
    pub(super) const CHECKED: u32 = 4;
    pub(super) const EDITABLE: u32 = 7;
    pub(super) const ENABLED: u32 = 8;
    pub(super) const EXPANDABLE: u32 = 9;
    pub(super) const FOCUSABLE: u32 = 11;
    pub(super) const FOCUSED: u32 = 12;
    pub(super) const HORIZONTAL: u32 = 14;
    pub(super) const MODAL: u32 = 16;
    pub(super) const MULTI_LINE: u32 = 17;
    pub(super) const PRESSED: u32 = 20;
    pub(super) const SELECTABLE: u32 = 22;
    pub(super) const SELECTED: u32 = 23;
    pub(super) const SENSITIVE: u32 = 24;
    pub(super) const SHOWING: u32 = 25;
    pub(super) const SINGLE_LINE: u32 = 26;
    pub(super) const VERTICAL: u32 = 29;
    pub(super) const VISIBLE: u32 = 30;
    pub(super) const SELECTABLE_TEXT: u32 = 38;
    pub(super) const CHECKABLE: u32 = 41;
}

/// The accessibility bus, while this program is on it.
struct Bus {
    connection: Connection,
    /// The registry's own root, which is the application's parent.
    desktop: (String, String),
    /// The number the registry gave the application.
    id: i32,
    /// The selection as the screen reader was last told it.
    told: Option<(usize, usize)>,
    /// The controls as the screen reader was last told of them, once it
    /// has been told of any.
    known: Option<Vec<Element>>,
    /// When they were last looked over for changes.
    looked: std::time::Instant,
}

thread_local! {
    static BUS: RefCell<Option<Bus>> = const { RefCell::new(None) };
    /// Set when the application says the selection moved, which it says
    /// in the middle of handling an event — when it cannot be asked where
    /// the selection is now. The screen reader is told on the next pump.
    static MOVED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Joins the accessibility bus and registers the application's tree, where
/// the desktop has the bus. Nothing where it does not, which is a desktop
/// no screen reader could read anything on.
pub(crate) fn start() {
    let Some(session) = super::dbus::session_address() else { return };
    let Ok(mut session) = Connection::open(&session) else { return };
    let address = session
        .call(
            &Message::call(
                "org.a11y.Bus",
                "/org/a11y/bus",
                "org.a11y.Bus",
                "GetAddress",
                Vec::new(),
            ),
            Duration::from_secs(3),
        )
        .ok()
        .and_then(|reply| reply.body.first().and_then(Value::as_str).map(str::to_owned));
    let Some(address) = address else { return };
    let Ok(mut connection) = Connection::open(&address) else { return };
    let own = connection.name.clone();
    let embedded = connection.call(
        &Message::call(
            "org.a11y.atspi.Registry",
            ROOT,
            "org.a11y.atspi.Socket",
            "Embed",
            vec![Value::Struct(vec![Value::str(&own), Value::path(ROOT)])],
        ),
        Duration::from_secs(5),
    );
    let desktop = embedded
        .ok()
        .and_then(|reply| {
            let parent = reply.body.first()?;
            let [name, path] = parent.items() else { return None };
            Some((name.as_str()?.to_owned(), path.as_str()?.to_owned()))
        })
        .unwrap_or_else(|| ("org.a11y.atspi.Registry".to_owned(), ROOT.to_owned()));
    BUS.with(|slot| {
        *slot.borrow_mut() = Some(Bus {
            connection,
            desktop,
            id: 0,
            told: None,
            known: None,
            looked: std::time::Instant::now(),
        });
    });
}

/// The application says the caret or the selection moved.
pub(crate) fn note_selection_changed() {
    MOVED.with(|moved| moved.set(true));
}

/// The accessibility bus's socket, once the application is on the bus: the
/// shell's loop waits on it beside the display's, so that a question is
/// answered when it is asked rather than at the next tick.
pub(crate) fn bus_fd() -> Option<std::os::fd::RawFd> {
    BUS.with(|slot| slot.borrow().as_ref().and_then(|bus| bus.connection.raw_fd()))
}

/// Answers whatever the screen reader has asked since the last time, and
/// tells it what has changed.
pub(crate) fn pump(window: &mut dyn Window) {
    if BUS.with(|slot| slot.borrow().is_none()) {
        return;
    }
    if MOVED.with(|moved| moved.replace(false)) {
        selection_changed(window);
    }
    let messages = BUS.with(|slot| {
        slot.borrow_mut().as_mut().map(|bus| bus.connection.poll()).unwrap_or_default()
    });
    let mut acted = false;
    for message in messages {
        if message.kind != Kind::Call {
            continue;
        }
        // A press, a selection or a value written changes what the
        // controls are, and the screen reader is told at once rather than
        // at the next look.
        acted |= matches!(
            message.member.as_deref(),
            Some(
                "DoAction"
                    | "SetSelection"
                    | "AddSelection"
                    | "RemoveSelection"
                    | "SetCaretOffset"
                    | "SetTextContents"
                    | "Set"
            )
        );
        let answer = answer(&message, window);
        BUS.with(|slot| {
            if let Some(bus) = slot.borrow_mut().as_mut() {
                if let Some(answer) = answer {
                    let _ = bus.connection.send(&answer);
                }
            }
        });
    }
    let due = BUS
        .with(|slot| slot.borrow().as_ref().is_some_and(|bus| bus.looked.elapsed() > LOOK_EVERY));
    if acted || due {
        look_over(window);
    }
    if acted && MOVED.with(|moved| moved.replace(false)) {
        selection_changed(window);
    }
}

/// How often the controls are looked over for what changed, when nothing
/// said so: often enough that a screen reader is not told late, seldom
/// enough that describing them costs nothing.
const LOOK_EVERY: Duration = Duration::from_millis(300);

/// An event, as AT-SPI sends every one: what, two numbers, a value, and
/// properties — none, here.
fn event(
    path: &str,
    interface: &str,
    member: &str,
    detail: &str,
    one: i32,
    two: i32,
    any: Value,
) -> Message {
    Message::signal(
        path,
        interface,
        member,
        vec![
            Value::str(detail),
            Value::Int32(one),
            Value::Int32(two),
            Value::Variant(Box::new(any)),
            Value::Array("{sv}".to_owned(), Vec::new()),
        ],
    )
}

fn object_event(path: &str, member: &str, detail: &str, one: i32, two: i32, any: Value) -> Message {
    event(path, "org.a11y.atspi.Event.Object", member, detail, one, two, any)
}

/// Tells the screen reader the caret or the selection moved, so that it
/// reads what the caret is on.
fn selection_changed(window: &mut dyn Window) {
    let Some(text) = window.text() else { return };
    let Some(document) =
        window.elements().into_iter().find(|element| element.role == Role::Document)
    else {
        return;
    };
    let path = element_path(document.id);
    BUS.with(|slot| {
        let mut held = slot.borrow_mut();
        let Some(bus) = held.as_mut() else { return };
        let previous = bus.told.replace(text.selection);
        if previous == Some(text.selection) {
            return;
        }
        // The selection changed where there was one before or is one now;
        // otherwise only the caret moved.
        let moved_selection = previous.is_some_and(|(start, end)| start != end)
            || text.selection.0 != text.selection.1;
        let caret = text.selection.1 as i32;
        let _ = bus.connection.send(&object_event(
            &path,
            "TextCaretMoved",
            "",
            caret,
            0,
            Value::Int32(0),
        ));
        if moved_selection {
            let _ = bus.connection.send(&object_event(
                &path,
                "TextSelectionChanged",
                "",
                0,
                0,
                Value::Int32(0),
            ));
        }
    });
}

/// Tells the screen reader what changed among the controls since it was
/// last told: which came and went — a dialog opened, a tab chosen that
/// brings its own buttons — which were turned on or off, chosen, given the
/// keyboard or renamed, and what a box, a list or the status strip holds
/// now. A screen reader keeps its own copy of the tree and changes it by
/// these; a dialog that came is said to have been opened, and the message
/// the status strip shows is its text changed, which is how it is heard.
fn look_over(window: &mut dyn Window) {
    let now = window.elements();
    let before = BUS.with(|slot| {
        let mut held = slot.borrow_mut();
        let bus = held.as_mut()?;
        bus.looked = std::time::Instant::now();
        bus.known.replace(now.clone())
    });
    let Some(before) = before else { return };
    let mut signals = Vec::new();
    for old in &before {
        if !now.iter().any(|new| new.id == old.id) {
            let path = element_path(old.id);
            signals.push(Message::signal(
                CACHE,
                "org.a11y.atspi.Cache",
                "RemoveAccessible",
                vec![reference(&path)],
            ));
            let parent = parent_path(old.parent, &before);
            let index = index_among(old, &before);
            signals.push(object_event(
                &parent,
                "ChildrenChanged",
                "remove",
                index,
                0,
                reference(&path),
            ));
        }
    }
    for new in &now {
        let path = element_path(new.id);
        let Some(old) = before.iter().find(|old| old.id == new.id) else {
            let object = Object::Control(new.clone());
            signals.push(Message::signal(
                CACHE,
                "org.a11y.atspi.Cache",
                "AddAccessible",
                vec![cache_item(&object, window, &now)],
            ));
            let parent = parent_path(new.parent, &now);
            let index = index_among(new, &now);
            signals.push(object_event(
                &parent,
                "ChildrenChanged",
                "add",
                index,
                0,
                reference(&path),
            ));
            if new.role == Role::Dialog {
                for member in ["Create", "Activate"] {
                    signals.push(event(
                        &path,
                        "org.a11y.atspi.Event.Window",
                        member,
                        "",
                        0,
                        0,
                        Value::str(""),
                    ));
                }
            }
            if new.focused {
                signals.push(object_event(&path, "StateChanged", "focused", 1, 0, Value::Int32(0)));
            }
            continue;
        };
        let chosen = if matches!(new.role, Role::TabItem | Role::ListItem | Role::MenuItem) {
            "selected"
        } else {
            "checked"
        };
        let changes = [
            (chosen, old.selected, new.selected),
            ("focused", old.focused, new.focused),
            ("enabled", old.enabled, new.enabled),
            ("sensitive", old.enabled, new.enabled),
        ];
        for (name, was, is) in changes {
            if was != is {
                signals.push(object_event(
                    &path,
                    "StateChanged",
                    name,
                    i32::from(is),
                    0,
                    Value::Int32(0),
                ));
            }
        }
        if old.name != new.name {
            signals.push(object_event(
                &path,
                "PropertyChange",
                "accessible-name",
                0,
                0,
                Value::Str(new.name.clone()),
            ));
        }
        if old.value != new.value && holds_text(new.role) {
            let (was, is) = (old.value.chars().count() as i32, new.value.chars().count() as i32);
            signals.push(object_event(
                &path,
                "TextChanged",
                "delete",
                0,
                was,
                Value::Str(old.value.clone()),
            ));
            signals.push(object_event(
                &path,
                "TextChanged",
                "insert",
                0,
                is,
                Value::Str(new.value.clone()),
            ));
        }
        if old.range != new.range && new.range.is_some() {
            let now = new.range.map_or(0.0, |range| range.2);
            signals.push(object_event(
                &path,
                "PropertyChange",
                "accessible-value",
                0,
                0,
                Value::Double(f64::from(now)),
            ));
        }
    }
    BUS.with(|slot| {
        if let Some(bus) = slot.borrow_mut().as_mut() {
            for signal in &signals {
                let _ = bus.connection.send(signal);
            }
        }
    });
}

fn element_path(id: u64) -> String {
    format!("{ELEMENT}{id}")
}

/// The path of the object an element is inside.
fn parent_path(parent: Option<u64>, elements: &[Element]) -> String {
    match parent.filter(|id| elements.iter().any(|element| element.id == *id)) {
        Some(id) => element_path(id),
        None => FRAME.to_owned(),
    }
}

/// An element's place among the others inside the same thing.
fn index_among(element: &Element, elements: &[Element]) -> i32 {
    let parent = effective_parent(element, elements);
    elements
        .iter()
        .filter(|other| effective_parent(other, elements) == parent)
        .position(|other| other.id == element.id)
        .map_or(-1, |index| index as i32)
}

/// What an element is inside, of what is there: an element that names a
/// parent that is not described is on the window.
fn effective_parent(element: &Element, elements: &[Element]) -> Option<u64> {
    element.parent.filter(|id| elements.iter().any(|other| other.id == *id))
}

/// A reference to an object of this program's: its bus name and its path.
fn reference(path: &str) -> Value {
    let name = BUS.with(|slot| {
        slot.borrow().as_ref().map(|bus| bus.connection.name.clone()).unwrap_or_default()
    });
    Value::Struct(vec![Value::Str(name), Value::path(path)])
}

fn desktop_reference() -> Value {
    BUS.with(|slot| {
        let held = slot.borrow();
        let (name, path) = held.as_ref().map(|bus| bus.desktop.clone()).unwrap_or_default();
        Value::Struct(vec![Value::Str(name), Value::Path(path)])
    })
}

fn null_reference() -> Value {
    Value::Struct(vec![Value::str(""), Value::path(NULL)])
}

/// Which object a path is.
#[derive(Clone, Debug)]
enum Object {
    Application,
    Frame,
    Control(Element),
}

fn object_at(path: &str, elements: &[Element]) -> Option<Object> {
    match path {
        ROOT => Some(Object::Application),
        FRAME => Some(Object::Frame),
        _ => {
            let id: u64 = path.strip_prefix(ELEMENT)?.parse().ok()?;
            elements.iter().find(|element| element.id == id).cloned().map(Object::Control)
        }
    }
}

fn role_of(object: &Object) -> u32 {
    match object {
        Object::Application => role::APPLICATION,
        Object::Frame => role::FRAME,
        Object::Control(element) => match element.role {
            Role::Button => role::PUSH_BUTTON,
            Role::Toggle => role::TOGGLE_BUTTON,
            Role::TabItem => role::PAGE_TAB,
            Role::Document => role::DOCUMENT_TEXT,
            Role::Text => role::LABEL,
            Role::Dialog => role::DIALOG,
            Role::Pane => role::PANEL,
            Role::Menu => role::MENU,
            Role::MenuItem => role::MENU_ITEM,
            Role::List => role::LIST,
            Role::ListItem => role::LIST_ITEM,
            Role::Edit => role::ENTRY,
            Role::CheckBox => role::CHECK_BOX,
            Role::ComboBox => role::COMBO_BOX,
            Role::ScrollBar => role::SCROLL_BAR,
            Role::Ruler => role::RULER,
            Role::StatusBar => role::STATUS_BAR,
        },
    }
}

fn role_name(role: u32) -> &'static str {
    match role {
        role::APPLICATION => "application",
        role::FRAME => "frame",
        role::PUSH_BUTTON => "push button",
        role::TOGGLE_BUTTON => "toggle button",
        role::PAGE_TAB => "page tab",
        role::DOCUMENT_TEXT => "document text",
        role::DIALOG => "dialog",
        role::PANEL => "panel",
        role::MENU => "menu",
        role::MENU_ITEM => "menu item",
        role::LIST => "list",
        role::LIST_ITEM => "list item",
        role::ENTRY => "entry",
        role::CHECK_BOX => "check box",
        role::COMBO_BOX => "combo box",
        role::SCROLL_BAR => "scroll bar",
        role::RULER => "ruler",
        role::STATUS_BAR => "status bar",
        role::ALERT => "alert",
        _ => "label",
    }
}

/// Whether an element's value is read as text.
fn holds_text(role: Role) -> bool {
    matches!(role, Role::Edit | Role::ComboBox | Role::StatusBar)
}

/// Whether pressing an element does something.
fn presses(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::Toggle
            | Role::TabItem
            | Role::CheckBox
            | Role::MenuItem
            | Role::ListItem
            | Role::ComboBox
    )
}

fn interfaces_of(object: &Object) -> Vec<&'static str> {
    let mut interfaces = vec!["org.a11y.atspi.Accessible"];
    match object {
        Object::Application => interfaces.push("org.a11y.atspi.Application"),
        Object::Frame => interfaces.push("org.a11y.atspi.Component"),
        Object::Control(element) => {
            interfaces.push("org.a11y.atspi.Component");
            if presses(element.role) {
                interfaces.push("org.a11y.atspi.Action");
            }
            if element.role == Role::Document || holds_text(element.role) {
                interfaces.push("org.a11y.atspi.Text");
            }
            if element.role == Role::Edit {
                interfaces.push("org.a11y.atspi.EditableText");
            }
            if element.range.is_some() {
                interfaces.push("org.a11y.atspi.Value");
            }
        }
    }
    interfaces
}

/// The states, as the two words of bits AT-SPI sends them in.
fn states_of(object: &Object) -> Vec<Value> {
    let mut bits: Vec<u32> = vec![state::VISIBLE, state::SHOWING];
    match object {
        Object::Application => {}
        Object::Frame => bits.extend([state::ACTIVE, state::ENABLED, state::SENSITIVE]),
        Object::Control(element) => {
            if element.enabled {
                bits.extend([state::ENABLED, state::SENSITIVE]);
            }
            if element.focused {
                bits.push(state::FOCUSED);
            }
            match element.role {
                Role::Button => bits.push(state::FOCUSABLE),
                Role::Toggle | Role::CheckBox => {
                    bits.extend([state::FOCUSABLE, state::CHECKABLE]);
                    if element.selected {
                        bits.push(state::CHECKED);
                        if element.role == Role::Toggle {
                            bits.push(state::PRESSED);
                        }
                    }
                }
                Role::TabItem | Role::ListItem | Role::MenuItem => {
                    bits.extend([state::FOCUSABLE, state::SELECTABLE]);
                    if element.selected {
                        bits.push(state::SELECTED);
                    }
                }
                Role::Document => bits.extend([
                    state::FOCUSABLE,
                    state::EDITABLE,
                    state::MULTI_LINE,
                    state::SELECTABLE_TEXT,
                ]),
                Role::Edit => bits.extend([
                    state::FOCUSABLE,
                    state::EDITABLE,
                    state::SINGLE_LINE,
                    state::SELECTABLE_TEXT,
                ]),
                Role::ComboBox => bits.extend([state::FOCUSABLE, state::EXPANDABLE]),
                Role::Dialog => bits.extend([state::ACTIVE, state::MODAL]),
                Role::ScrollBar => bits.push(state::VERTICAL),
                Role::Ruler => bits.push(state::HORIZONTAL),
                Role::Text | Role::Pane | Role::Menu | Role::List | Role::StatusBar => {}
            }
        }
    }
    let mut words = [0u32; 2];
    for bit in bits {
        words[(bit / 32) as usize] |= 1 << (bit % 32);
    }
    words.iter().map(|word| Value::Uint32(*word)).collect()
}

fn name_of(object: &Object, window: &mut dyn Window) -> String {
    match object {
        Object::Application => crate::install::PROGRAM_NAME.to_owned(),
        Object::Frame => window.title(),
        Object::Control(element) => element.name.clone(),
    }
}

fn parent_of(object: &Object, elements: &[Element]) -> Value {
    match object {
        Object::Application => desktop_reference(),
        Object::Frame => reference(ROOT),
        Object::Control(element) => reference(&parent_path(element.parent, elements)),
    }
}

fn children_of(object: &Object, elements: &[Element]) -> Vec<Value> {
    let inside = |parent: Option<u64>| {
        elements
            .iter()
            .filter(|element| effective_parent(element, elements) == parent)
            .map(|element| reference(&element_path(element.id)))
            .collect()
    };
    match object {
        Object::Application => vec![reference(FRAME)],
        Object::Frame => inside(None),
        Object::Control(element) => inside(Some(element.id)),
    }
}

fn index_of(object: &Object, elements: &[Element]) -> i32 {
    match object {
        Object::Application => -1,
        Object::Frame => 0,
        Object::Control(element) => index_among(element, elements),
    }
}

/// A rectangle of the drawing area as a screen reader asks for it: on the
/// screen, or in the window.
fn placed(
    window: &mut dyn Window,
    (x, y, width, height): (i32, i32, i32, i32),
    coords: u32,
) -> Value {
    let scale = window.scale();
    let device = |value: i32| (value as f32 * scale).round() as i32;
    let (left, top) = if coords == 0 { window.origin() } else { (0, 0) };
    Value::Struct(vec![
        Value::Int32(left + device(x)),
        Value::Int32(top + device(y)),
        Value::Int32(device(width)),
        Value::Int32(device(height)),
    ])
}

fn extents_of(object: &Object, window: &mut dyn Window) -> (i32, i32, i32, i32) {
    match object {
        Object::Application | Object::Frame => {
            let (width, height) = window.size();
            let scale = window.scale().max(0.01);
            (0, 0, (width as f32 / scale) as i32, (height as f32 / scale) as i32)
        }
        Object::Control(element) => element.rect,
    }
}

/// The answer to a call, or nothing where none is wanted.
fn answer(message: &Message, window: &mut dyn Window) -> Option<Message> {
    let path = message.path.as_deref().unwrap_or("");
    let interface = message.interface.as_deref().unwrap_or("");
    let member = message.member.as_deref().unwrap_or("");
    let reply = |body: Vec<Value>| Some(message.reply(body));
    let unknown = || {
        Some(
            message.error("org.freedesktop.DBus.Error.UnknownMethod", "not something this answers"),
        )
    };
    let argument = |index: usize| message.body.get(index).map(Value::inner);

    if path == CACHE {
        return match member {
            "GetItems" => reply(vec![cache_items(window)]),
            _ => unknown(),
        };
    }
    let elements = window.elements();
    let Some(object) = object_at(path, &elements) else {
        return Some(message.error("org.freedesktop.DBus.Error.UnknownObject", "no such object"));
    };

    match (interface, member) {
        ("org.freedesktop.DBus.Properties", "Get") => {
            let property = argument(1).and_then(Value::as_str).unwrap_or("");
            let value = property_of(&object, property, window, &elements)?;
            reply(vec![Value::Variant(Box::new(value))])
        }
        ("org.freedesktop.DBus.Properties", "GetAll") => {
            let wanted = argument(0).and_then(Value::as_str).unwrap_or("").to_owned();
            let names: &[&str] = match wanted.as_str() {
                "org.a11y.atspi.Accessible" => {
                    &["Name", "Description", "Parent", "ChildCount", "Locale", "AccessibleId"]
                }
                "org.a11y.atspi.Application" => &["ToolkitName", "Version", "AtspiVersion", "Id"],
                "org.a11y.atspi.Action" => &["NActions"],
                "org.a11y.atspi.Text" => &["CharacterCount", "CaretOffset"],
                "org.a11y.atspi.Value" => {
                    &["MinimumValue", "MaximumValue", "MinimumIncrement", "CurrentValue", "Text"]
                }
                _ => &[],
            };
            let entries = names
                .iter()
                .filter_map(|name| {
                    let value = property_of(&object, name, window, &elements)?;
                    Some(Value::DictEntry(
                        Box::new(Value::str(name)),
                        Box::new(Value::Variant(Box::new(value))),
                    ))
                })
                .collect();
            reply(vec![Value::Array("{sv}".to_owned(), entries)])
        }
        ("org.freedesktop.DBus.Properties", "Set") => {
            match argument(1).and_then(Value::as_str) {
                // The registry numbers each application it is given.
                Some("Id") => {
                    let id = argument(2).and_then(Value::as_i64).unwrap_or(0) as i32;
                    BUS.with(|slot| {
                        if let Some(bus) = slot.borrow_mut().as_mut() {
                            bus.id = id;
                        }
                    });
                }
                // A scroll bar moved by a screen reader.
                Some("CurrentValue") => {
                    if let (Object::Control(element), Some(Value::Double(value))) =
                        (&object, argument(2))
                    {
                        window.set_value(element.id, &value.to_string());
                    }
                }
                _ => {}
            }
            reply(Vec::new())
        }
        ("org.freedesktop.DBus.Introspectable", "Introspect") => reply(vec![Value::str("<node/>")]),
        ("org.freedesktop.DBus.Peer", "Ping") => reply(Vec::new()),
        ("org.a11y.atspi.Accessible", _) => accessible(message, &object, window, &elements),
        ("org.a11y.atspi.Application", "GetLocale") => reply(vec![Value::Str(locale())]),
        ("org.a11y.atspi.Component", _) => component(message, &object, window, &elements),
        ("org.a11y.atspi.Action", _) => {
            let Object::Control(element) = &object else { return unknown() };
            let key = element.access_key.clone();
            let action = if element.role == Role::ComboBox { "open" } else { "press" };
            match member {
                "GetName" | "GetLocalizedName" => reply(vec![Value::str(action)]),
                "GetDescription" => reply(vec![Value::Str(element.name.clone())]),
                "GetKeyBinding" => reply(vec![Value::Str(key)]),
                "GetActions" => reply(vec![Value::Array(
                    "(sss)".to_owned(),
                    vec![Value::Struct(vec![
                        Value::str(action),
                        Value::Str(element.name.clone()),
                        Value::Str(key),
                    ])],
                )]),
                "DoAction" => {
                    window.invoke(element.id);
                    reply(vec![Value::Bool(true)])
                }
                _ => unknown(),
            }
        }
        ("org.a11y.atspi.Text", _) => text(message, &object, window),
        ("org.a11y.atspi.EditableText", _) => {
            let Object::Control(element) = &object else { return unknown() };
            match member {
                "SetTextContents" => {
                    let value = argument(0).and_then(Value::as_str).unwrap_or("").to_owned();
                    window.set_value(element.id, &value);
                    reply(vec![Value::Bool(true)])
                }
                // A box is written whole; its parts are not.
                "InsertText" | "CopyText" | "CutText" | "DeleteText" | "PasteText" => {
                    reply(vec![Value::Bool(false)])
                }
                _ => unknown(),
            }
        }
        _ => unknown(),
    }
}

fn locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .map_or_else(|| "C".to_owned(), |value| value.split('.').next().unwrap_or("C").to_owned())
}

/// The text an object holds and the selection in it: the document's, or a
/// box's value, which is read whole with the caret at its end.
fn text_of(object: &Object, window: &mut dyn Window) -> TextState {
    match object {
        Object::Control(element) if element.role == Role::Document => {
            window.text().unwrap_or_default()
        }
        Object::Control(element) => {
            let length = element.value.chars().count();
            TextState { text: element.value.clone(), selection: (length, length) }
        }
        _ => TextState::default(),
    }
}

/// One of an object's properties.
fn property_of(
    object: &Object,
    name: &str,
    window: &mut dyn Window,
    elements: &[Element],
) -> Option<Value> {
    let range = match object {
        Object::Control(element) => element.range,
        _ => None,
    };
    Some(match name {
        "Name" => Value::Str(name_of(object, window)),
        "Description" | "HelpText" => Value::str(""),
        "Parent" => parent_of(object, elements),
        "ChildCount" => Value::Int32(children_of(object, elements).len() as i32),
        "Locale" => Value::Str(locale()),
        "AccessibleId" => Value::Str(match object {
            Object::Application => "application".to_owned(),
            Object::Frame => "window".to_owned(),
            Object::Control(element) => element.id.to_string(),
        }),
        "ToolkitName" => Value::str("word-processor"),
        "Version" => Value::str(env!("CARGO_PKG_VERSION")),
        "AtspiVersion" => Value::str("2.1"),
        "Id" => Value::Int32(BUS.with(|slot| slot.borrow().as_ref().map_or(0, |bus| bus.id))),
        "NActions" => Value::Int32(1),
        "CharacterCount" => Value::Int32(text_of(object, window).text.chars().count() as i32),
        "CaretOffset" => Value::Int32(text_of(object, window).selection.1 as i32),
        "MinimumValue" => Value::Double(f64::from(range?.0)),
        "MaximumValue" => Value::Double(f64::from(range?.1)),
        "CurrentValue" => Value::Double(f64::from(range?.2)),
        "MinimumIncrement" => Value::Double(1.0),
        "Text" => Value::Str(match object {
            Object::Control(element) => element.value.clone(),
            _ => String::new(),
        }),
        _ => return None,
    })
}

fn accessible(
    message: &Message,
    object: &Object,
    window: &mut dyn Window,
    elements: &[Element],
) -> Option<Message> {
    let reply = |body: Vec<Value>| Some(message.reply(body));
    let index = message.body.first().and_then(|value| value.inner().as_i64()).unwrap_or(0);
    match message.member.as_deref().unwrap_or("") {
        "GetChildAtIndex" => {
            let children = children_of(object, elements);
            let child = usize::try_from(index)
                .ok()
                .and_then(|index| children.get(index).cloned())
                .unwrap_or_else(null_reference);
            reply(vec![child])
        }
        "GetChildren" => {
            reply(vec![Value::Array("(so)".to_owned(), children_of(object, elements))])
        }
        "GetIndexInParent" => reply(vec![Value::Int32(index_of(object, elements))]),
        "GetRelationSet" => reply(vec![Value::Array("(ua(so))".to_owned(), Vec::new())]),
        "GetRole" => reply(vec![Value::Uint32(role_of(object))]),
        "GetRoleName" | "GetLocalizedRoleName" => {
            reply(vec![Value::str(role_name(role_of(object)))])
        }
        "GetState" => reply(vec![Value::Array("u".to_owned(), states_of(object))]),
        "GetAttributes" => {
            let mut attributes = Vec::new();
            if let Object::Control(element) = object {
                if !element.access_key.is_empty() {
                    attributes.push(Value::DictEntry(
                        Box::new(Value::str("keyshortcuts")),
                        Box::new(Value::Str(element.access_key.clone())),
                    ));
                }
                // The status strip's message is one to be heard when it
                // changes, which is what a live region is.
                if element.role == Role::StatusBar {
                    attributes.push(Value::DictEntry(
                        Box::new(Value::str("container-live")),
                        Box::new(Value::str("polite")),
                    ));
                }
            }
            reply(vec![Value::Array("{ss}".to_owned(), attributes)])
        }
        "GetApplication" => reply(vec![reference(ROOT)]),
        "GetInterfaces" => reply(vec![Value::Array(
            "s".to_owned(),
            interfaces_of(object).into_iter().map(Value::str).collect(),
        )]),
        _ => {
            let _ = window;
            Some(
                message.error(
                    "org.freedesktop.DBus.Error.UnknownMethod",
                    "not something this answers",
                ),
            )
        }
    }
}

fn component(
    message: &Message,
    object: &Object,
    window: &mut dyn Window,
    elements: &[Element],
) -> Option<Message> {
    let reply = |body: Vec<Value>| Some(message.reply(body));
    let int = |index: usize| {
        message.body.get(index).and_then(|value| value.inner().as_i64()).unwrap_or(0)
    };
    let extents = extents_of(object, window);
    match message.member.as_deref().unwrap_or("") {
        "GetExtents" => {
            let coords = int(0) as u32;
            reply(vec![placed(window, extents, coords)])
        }
        "GetPosition" => {
            let Value::Struct(fields) = placed(window, extents, int(0) as u32) else { return None };
            reply(vec![Value::Struct(fields[..2].to_vec())])
        }
        "GetSize" => {
            let Value::Struct(fields) = placed(window, extents, 1) else { return None };
            reply(vec![Value::Struct(fields[2..].to_vec())])
        }
        "Contains" | "GetAccessibleAtPoint" => {
            let (x, y, coords) = (int(0) as i32, int(1) as i32, int(2) as u32);
            let inside = |rect: (i32, i32, i32, i32), window: &mut dyn Window| {
                let Value::Struct(fields) = placed(window, rect, coords) else { return false };
                let number = |index: usize| fields[index].as_i64().unwrap_or(0) as i32;
                let (left, top, width, height) = (number(0), number(1), number(2), number(3));
                x >= left && y >= top && x < left + width && y < top + height
            };
            if message.member.as_deref() == Some("Contains") {
                let inside = inside(extents, window);
                return reply(vec![Value::Bool(inside)]);
            }
            let parent = match object {
                Object::Frame => Some(None),
                Object::Control(element) => Some(Some(element.id)),
                Object::Application => None,
            };
            let found = parent.and_then(|parent| {
                elements
                    .iter()
                    .rev()
                    .filter(|element| effective_parent(element, elements) == parent)
                    .find(|element| inside(element.rect, window))
                    .map(|element| reference(&element_path(element.id)))
            });
            reply(vec![found.unwrap_or_else(null_reference)])
        }
        "GetLayer" => reply(vec![Value::Uint32(3)]),
        "GetMDIZOrder" => reply(vec![Value::Int16(0)]),
        "GetAlpha" => reply(vec![Value::Double(1.0)]),
        "GrabFocus" | "SetExtents" | "SetPosition" | "SetSize" | "ScrollTo" | "ScrollToPoint" => {
            reply(vec![Value::Bool(false)])
        }
        _ => Some(
            message.error("org.freedesktop.DBus.Error.UnknownMethod", "not something this answers"),
        ),
    }
}

/// Where the unit a granularity names begins and ends around an offset:
/// the character, the word, the sentence, or the line — a line from the
/// layout where it gave them, a paragraph otherwise.
fn unit_around(
    characters: &[char],
    offset: usize,
    granularity: u32,
    lines: &[(usize, usize)],
) -> (usize, usize) {
    let length = characters.len();
    let offset = offset.min(length);
    match granularity {
        // A word: the run of letters and digits the offset is in or before,
        // and the spaces after it.
        1 => {
            let word = |c: &char| c.is_alphanumeric() || *c == '\'';
            let mut start = offset;
            while start > 0 && word(&characters[start - 1]) {
                start -= 1;
            }
            let mut end = offset;
            while end < length && word(&characters[end]) {
                end += 1;
            }
            while end < length && characters[end] == ' ' {
                end += 1;
            }
            (start, end)
        }
        // A sentence: to the stop, and the spaces after it.
        2 => {
            let stop = |c: &char| matches!(c, '.' | '!' | '?' | '\n');
            let mut start = offset;
            while start > 0 && !stop(&characters[start - 1]) {
                start -= 1;
            }
            while start < offset && characters[start] == ' ' {
                start += 1;
            }
            let mut end = offset;
            while end < length && !stop(&characters[end]) {
                end += 1;
            }
            if end < length {
                end += 1;
            }
            while end < length && characters[end] == ' ' {
                end += 1;
            }
            (start, end)
        }
        // A line as the layout broke it.
        3 if !lines.is_empty() => lines
            .iter()
            .copied()
            .find(|(start, end)| offset >= *start && offset < *end)
            .or_else(|| lines.iter().copied().rev().find(|(start, _)| offset >= *start))
            .unwrap_or((offset, offset)),
        // A paragraph, with its break.
        3 | 4 => {
            let mut start = offset;
            while start > 0 && characters[start - 1] != '\n' {
                start -= 1;
            }
            let mut end = offset;
            while end < length && characters[end] != '\n' {
                end += 1;
            }
            if end < length {
                end += 1;
            }
            (start, end)
        }
        _ => (offset, (offset + 1).min(length)),
    }
}

/// The granularity an old-style boundary asks for.
fn granularity_of_boundary(boundary: u32) -> u32 {
    match boundary {
        1 | 2 => 1,
        3 | 4 => 2,
        5 | 6 => 3,
        _ => 0,
    }
}

/// How a stretch is set, as AT-SPI names the attributes.
fn attribute_list(attributes: &TextAttributes) -> Vec<Value> {
    let mut list = Vec::new();
    let mut add = |name: &str, value: String| {
        list.push(Value::DictEntry(Box::new(Value::str(name)), Box::new(Value::Str(value))));
    };
    if !attributes.font.is_empty() {
        add("family-name", attributes.font.clone());
    }
    if attributes.size > 0.0 {
        add("size", format!("{}", attributes.size));
    }
    add("weight", if attributes.bold { "700" } else { "400" }.to_owned());
    add("style", if attributes.italic { "italic" } else { "normal" }.to_owned());
    add("underline", if attributes.underline { "single" } else { "none" }.to_owned());
    add("strikethrough", attributes.strike.to_string());
    if let Some((red, green, blue)) = attributes.color {
        add("fg-color", format!("{red},{green},{blue}"));
    }
    if let Some((red, green, blue)) = attributes.background {
        add("bg-color", format!("{red},{green},{blue}"));
    }
    list
}

fn text(message: &Message, object: &Object, window: &mut dyn Window) -> Option<Message> {
    let reply = |body: Vec<Value>| Some(message.reply(body));
    let int = |index: usize| {
        message.body.get(index).and_then(|value| value.inner().as_i64()).unwrap_or(0)
    };
    let document = matches!(object, Object::Control(element) if element.role == Role::Document);
    let state = text_of(object, window);
    let characters: Vec<char> = state.text.chars().collect();
    let length = characters.len();
    let lines = if document { window.lines() } else { vec![(0, length)] };
    let clamp = |value: i64| -> usize {
        if value < 0 {
            length
        } else {
            usize::try_from(value).unwrap_or(length).min(length)
        }
    };
    let slice = |start: usize, end: usize| -> String {
        characters[start.min(end)..end.max(start)].iter().collect()
    };
    let unit = |start: usize, end: usize| {
        vec![Value::Str(slice(start, end)), Value::Int32(start as i32), Value::Int32(end as i32)]
    };
    let (selection_start, selection_end) = (
        state.selection.0.min(state.selection.1).min(length),
        state.selection.0.max(state.selection.1).min(length),
    );
    let element_id = match object {
        Object::Control(element) => element.id,
        _ => 0,
    };
    match message.member.as_deref().unwrap_or("") {
        "GetText" => {
            let (start, end) = (clamp(int(0).max(0)), clamp(int(1)));
            reply(vec![Value::Str(slice(start, end))])
        }
        "GetCharacterAtOffset" => {
            let at = clamp(int(0));
            reply(vec![Value::Int32(characters.get(at).map_or(0, |c| *c as i32))])
        }
        "GetStringAtOffset" => {
            let (start, end) = unit_around(&characters, clamp(int(0)), int(1) as u32, &lines);
            reply(unit(start, end))
        }
        "GetTextAtOffset" => {
            let granularity = granularity_of_boundary(int(1) as u32);
            let (start, end) = unit_around(&characters, clamp(int(0)), granularity, &lines);
            reply(unit(start, end))
        }
        "GetTextBeforeOffset" => {
            let granularity = granularity_of_boundary(int(1) as u32);
            let (start, _) = unit_around(&characters, clamp(int(0)), granularity, &lines);
            let (before, _) =
                unit_around(&characters, start.saturating_sub(1), granularity, &lines);
            reply(unit(before.min(start), start))
        }
        "GetTextAfterOffset" => {
            let granularity = granularity_of_boundary(int(1) as u32);
            let (_, end) = unit_around(&characters, clamp(int(0)), granularity, &lines);
            let (_, after) = unit_around(&characters, end, granularity, &lines);
            reply(unit(end, after.max(end)))
        }
        "SetCaretOffset" => {
            if document {
                let at = clamp(int(0));
                window.select(at, at);
            }
            reply(vec![Value::Bool(document)])
        }
        "GetNSelections" => reply(vec![Value::Int32(i32::from(selection_start != selection_end))]),
        "GetSelection" => {
            reply(vec![Value::Int32(selection_start as i32), Value::Int32(selection_end as i32)])
        }
        "AddSelection" | "SetSelection" => {
            let offset = usize::from(message.member.as_deref() == Some("SetSelection"));
            if document {
                let (start, end) = (clamp(int(offset)), clamp(int(offset + 1)));
                window.select(start, end);
            }
            reply(vec![Value::Bool(document)])
        }
        "RemoveSelection" => {
            if document {
                window.select(selection_end, selection_end);
            }
            reply(vec![Value::Bool(document)])
        }
        "GetCharacterExtents" | "GetRangeExtents" => {
            let range = message.member.as_deref() == Some("GetRangeExtents");
            let start = clamp(int(0));
            let end = if range { clamp(int(1)) } else { (start + 1).min(length.max(start + 1)) };
            let coords = int(if range { 2 } else { 1 }) as u32;
            let rects = if document {
                window.rects(start.min(end), end.max(start))
            } else {
                window
                    .elements()
                    .into_iter()
                    .find(|element| element.id == element_id)
                    .map(|element| vec![element.rect])
                    .unwrap_or_default()
            };
            let union = rects.iter().copied().reduce(|a, b| {
                let left = a.0.min(b.0);
                let top = a.1.min(b.1);
                let right = (a.0 + a.2).max(b.0 + b.2);
                let bottom = (a.1 + a.3).max(b.1 + b.3);
                (left, top, right - left, bottom - top)
            });
            let rect = union.unwrap_or((0, 0, 0, 0));
            let Value::Struct(fields) = placed(window, rect, coords) else { return None };
            if range {
                reply(vec![Value::Struct(fields)])
            } else {
                reply(fields)
            }
        }
        "GetOffsetAtPoint" => reply(vec![Value::Int32(-1)]),
        "GetAttributes" | "GetAttributeRun" => {
            let at = clamp(int(0));
            let found = if document { window.attributes(at) } else { None };
            let (list, start, end) = match found {
                Some((attributes, start, end)) => (attribute_list(&attributes), start, end),
                None => (Vec::new(), 0, length),
            };
            reply(vec![
                Value::Array("{ss}".to_owned(), list),
                Value::Int32(start as i32),
                Value::Int32(end as i32),
            ])
        }
        "GetAttributeValue" => {
            let wanted = message.body.get(1).and_then(Value::as_str).unwrap_or("").to_owned();
            let at = clamp(int(0));
            let list = if document {
                window.attributes(at).map(|(attributes, ..)| attribute_list(&attributes))
            } else {
                None
            }
            .unwrap_or_default();
            let value = list
                .iter()
                .find_map(|entry| match entry {
                    Value::DictEntry(name, value) if name.as_str() == Some(&wanted) => {
                        value.as_str().map(str::to_owned)
                    }
                    _ => None,
                })
                .unwrap_or_default();
            reply(vec![Value::Str(value)])
        }
        "GetDefaultAttributes" | "GetDefaultAttributeSet" => {
            reply(vec![Value::Array("{ss}".to_owned(), Vec::new())])
        }
        "GetBoundedRanges" => reply(vec![Value::Array("(iisv)".to_owned(), Vec::new())]),
        "ScrollSubstringTo" | "ScrollSubstringToPoint" => reply(vec![Value::Bool(false)]),
        _ => Some(
            message.error("org.freedesktop.DBus.Error.UnknownMethod", "not something this answers"),
        ),
    }
}

/// The whole tree at once, which a screen reader asks for so as not to ask
/// object by object: each object's reference, the application's, its
/// parent's, its place among its siblings, how many children it has, its
/// interfaces, name, role, description and states.
fn cache_items(window: &mut dyn Window) -> Value {
    let elements = window.elements();
    let mut objects = vec![Object::Application, Object::Frame];
    objects.extend(elements.iter().cloned().map(Object::Control));
    let items = objects.iter().map(|object| cache_item(object, window, &elements)).collect();
    Value::Array("((so)(so)(so)iiassusau)".to_owned(), items)
}

/// One object as the cache holds it.
fn cache_item(object: &Object, window: &mut dyn Window, elements: &[Element]) -> Value {
    let path = match object {
        Object::Application => ROOT.to_owned(),
        Object::Frame => FRAME.to_owned(),
        Object::Control(element) => element_path(element.id),
    };
    Value::Struct(vec![
        reference(&path),
        reference(ROOT),
        parent_of(object, elements),
        Value::Int32(index_of(object, elements)),
        Value::Int32(children_of(object, elements).len() as i32),
        Value::Array("s".to_owned(), interfaces_of(object).into_iter().map(Value::str).collect()),
        Value::Str(name_of(object, window)),
        Value::Uint32(role_of(object)),
        Value::str(""),
        Value::Array("u".to_owned(), states_of(object)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_sentences_and_lines_are_found_around_an_offset() {
        let text: Vec<char> = "One two. Three\nFour".chars().collect();
        assert_eq!(unit_around(&text, 5, 1, &[]), (4, 7), "the word and not the stop");
        assert_eq!(unit_around(&text, 1, 1, &[]), (0, 4), "a word takes the space after it");
        assert_eq!(unit_around(&text, 2, 2, &[]), (0, 9), "the sentence to its stop");
        assert_eq!(unit_around(&text, 10, 3, &[]), (0, 15), "a paragraph with its break");
        assert_eq!(unit_around(&text, 16, 3, &[]), (15, 19), "and the last without one");
        assert_eq!(unit_around(&text, 3, 0, &[]), (3, 4), "a character");
        // Lines as the layout broke them: the first paragraph over two.
        let lines = [(0, 9), (9, 15), (15, 19)];
        assert_eq!(unit_around(&text, 10, 3, &lines), (9, 15), "the line the layout made");
        assert_eq!(unit_around(&text, 19, 3, &lines), (15, 19), "and the end is the last line's");
    }

    #[test]
    fn states_go_in_two_words_of_bits() {
        let element = Element {
            id: 1,
            role: Role::Toggle,
            name: "Bold".to_owned(),
            access_key: "1".to_owned(),
            rect: (0, 0, 10, 10),
            selected: true,
            enabled: true,
            ..Element::default()
        };
        let words = states_of(&Object::Control(element));
        let low = words[0].as_i64().unwrap() as u32;
        let high = words[1].as_i64().unwrap() as u32;
        assert!(low & (1 << state::CHECKED) != 0 && low & (1 << state::ENABLED) != 0);
        assert!(high & (1 << (state::CHECKABLE - 32)) != 0, "checkable is in the second word");
    }

    #[test]
    fn an_element_is_inside_what_it_names_when_that_is_there() {
        let element = |id: u64, parent: Option<u64>| Element { id, parent, ..Element::default() };
        let elements =
            vec![element(1, None), element(2, Some(1)), element(3, Some(1)), element(4, Some(99))];
        assert_eq!(index_among(&elements[2], &elements), 1, "second in the dialog");
        assert_eq!(index_among(&elements[3], &elements), 1, "a parent not there is the window");
        assert_eq!(children_of(&Object::Control(elements[0].clone()), &elements).len(), 2);
        assert_eq!(children_of(&Object::Frame, &elements).len(), 2);
    }

    #[test]
    fn a_stretch_is_described_as_screen_readers_name_it() {
        let attributes = TextAttributes {
            font: "Calibri".to_owned(),
            size: 11.0,
            bold: true,
            color: Some((192, 0, 0)),
            ..TextAttributes::default()
        };
        let list = attribute_list(&attributes);
        let has = |name: &str, value: &str| {
            list.iter().any(|entry| {
                matches!(entry, Value::DictEntry(key, item)
                    if key.as_str() == Some(name) && item.as_str() == Some(value))
            })
        };
        assert!(has("family-name", "Calibri"));
        assert!(has("size", "11"));
        assert!(has("weight", "700"));
        assert!(has("style", "normal"));
        assert!(has("fg-color", "192,0,0"));
    }
}

//! AT-SPI: what a screen reader on Linux is told.
//!
//! # How a screen reader reads a program on Linux
//!
//! Over a bus of its own — the accessibility bus, whose address the session
//! bus gives — on which every program that can be read puts a tree of
//! objects: the application, its windows, and what is in them, each with
//! its role, its name, its states and its place on the screen, and with the
//! interfaces of what it is — something that can be pressed, text that can
//! be read and selected. The program registers its tree with the registry
//! on that bus, answers the screen reader's calls about it, and tells it
//! with signals when the caret moves. Orca reads GTK and Qt programs this
//! way, and now this one.
//!
//! # What this program says
//!
//! What UI Automation is told on Windows — the application sees to that,
//! through the shell's accessibility methods on [`crate::App`]: the ribbon's
//! tabs, its buttons and its toggles, which a screen reader presses; and the
//! document, whose text is read, by character, word, sentence and line, with
//! the caret and the selection, which can be set, and each stretch's place on
//! the screen. The objects are the application, the window, and one for each
//! control the application describes.
//!
//! The bus is D-Bus, spoken by [`super::dbus`]; this is the AT-SPI protocol
//! on top of it, written out against at-spi2-core's interface descriptions.

use std::cell::RefCell;
use std::time::Duration;

use super::dbus::{Connection, Kind, Message, Value};
use crate::accessibility::{Element, Role, TextState};

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
    fn invoke(&mut self, id: u64);
    fn select(&mut self, start: usize, end: usize);
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
    pub(super) const APPLICATION: u32 = 75;
    pub(super) const FRAME: u32 = 23;
    pub(super) const PUSH_BUTTON: u32 = 43;
    pub(super) const TOGGLE_BUTTON: u32 = 62;
    pub(super) const PAGE_TAB: u32 = 37;
    pub(super) const DOCUMENT_TEXT: u32 = 94;
    pub(super) const LABEL: u32 = 29;
}

/// States, as AT-SPI numbers the bits.
mod state {
    pub(super) const ACTIVE: u32 = 1;
    pub(super) const CHECKED: u32 = 4;
    pub(super) const EDITABLE: u32 = 7;
    pub(super) const ENABLED: u32 = 8;
    pub(super) const FOCUSABLE: u32 = 11;
    pub(super) const FOCUSED: u32 = 12;
    pub(super) const MULTI_LINE: u32 = 17;
    pub(super) const PRESSED: u32 = 20;
    pub(super) const SELECTABLE: u32 = 22;
    pub(super) const SELECTED: u32 = 23;
    pub(super) const SENSITIVE: u32 = 24;
    pub(super) const SHOWING: u32 = 25;
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

/// Answers whatever the screen reader has asked since the last time, and
/// tells it what has moved.
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
        // A press or a selection changes what the controls are, and the
        // screen reader is told at once rather than at the next look.
        acted |= matches!(
            message.member.as_deref(),
            Some(
                "DoAction" | "SetSelection" | "AddSelection" | "RemoveSelection" | "SetCaretOffset"
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

/// Tells the screen reader what changed among the controls since it was
/// last told: which came and went — a tab chosen brings its own buttons —
/// and which were turned on or off, chosen, given the keyboard, or renamed.
/// A screen reader keeps its own copy of the tree and changes it by these.
fn look_over(window: &mut dyn Window) {
    let now = window.elements();
    let signals = BUS.with(|slot| {
        let mut held = slot.borrow_mut();
        let Some(bus) = held.as_mut() else { return Vec::new() };
        bus.looked = std::time::Instant::now();
        let Some(before) = bus.known.replace(now.clone()) else { return Vec::new() };
        let mut signals = Vec::new();
        let event = |path: &str, member: &str, detail: &str, one: i32, any: Value| {
            Message::signal(
                path,
                "org.a11y.atspi.Event.Object",
                member,
                vec![
                    Value::str(detail),
                    Value::Int32(one),
                    Value::Int32(0),
                    Value::Variant(Box::new(any)),
                    Value::Array("{sv}".to_owned(), Vec::new()),
                ],
            )
        };
        for (index, old) in before.iter().enumerate() {
            if !now.iter().any(|new| new.id == old.id) {
                let path = element_path(old.id);
                signals.push(Message::signal(
                    CACHE,
                    "org.a11y.atspi.Cache",
                    "RemoveAccessible",
                    vec![reference(&path)],
                ));
                signals.push(event(
                    FRAME,
                    "ChildrenChanged",
                    "remove",
                    index as i32,
                    reference(&path),
                ));
            }
        }
        for (index, new) in now.iter().enumerate() {
            let path = element_path(new.id);
            let Some(old) = before.iter().find(|old| old.id == new.id) else {
                signals.push(event(
                    FRAME,
                    "ChildrenChanged",
                    "add",
                    index as i32,
                    reference(&path),
                ));
                continue;
            };
            let changes = [
                (
                    if new.role == Role::TabItem { "selected" } else { "checked" },
                    old.selected,
                    new.selected,
                ),
                ("focused", old.focused, new.focused),
                ("enabled", old.enabled, new.enabled),
                ("sensitive", old.enabled, new.enabled),
            ];
            for (name, was, is) in changes {
                if was != is {
                    signals.push(event(
                        &path,
                        "StateChanged",
                        name,
                        i32::from(is),
                        Value::Int32(0),
                    ));
                }
            }
            if old.name != new.name {
                signals.push(event(
                    &path,
                    "PropertyChange",
                    "accessible-name",
                    0,
                    Value::Str(new.name.clone()),
                ));
            }
        }
        signals
    });
    // The objects that came, as the cache takes them, before they are
    // said to have come.
    let added: Vec<Message> = now
        .iter()
        .enumerate()
        .filter(|(_, new)| {
            signals.iter().any(|signal| {
                signal.member.as_deref() == Some("ChildrenChanged")
                    && signal.body.first().and_then(Value::as_str) == Some("add")
                    && signal.body.get(3).map(Value::inner)
                        == Some(&reference(&element_path(new.id)))
            })
        })
        .map(|(index, new)| {
            Message::signal(
                CACHE,
                "org.a11y.atspi.Cache",
                "AddAccessible",
                vec![cache_item(&Object::Control(new.clone(), index), window, &now)],
            )
        })
        .collect();
    BUS.with(|slot| {
        if let Some(bus) = slot.borrow_mut().as_mut() {
            for signal in added.iter().chain(&signals) {
                let _ = bus.connection.send(signal);
            }
        }
    });
}

thread_local! {
    /// Set when the application says the selection moved, which it says
    /// in the middle of handling an event — when it cannot be asked where
    /// the selection is now. The screen reader is told on the next pump.
    static MOVED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The application says the caret or the selection moved.
pub(crate) fn note_selection_changed() {
    MOVED.with(|moved| moved.set(true));
}

/// Tells the screen reader the caret or the selection moved, so that it
/// reads what the caret is on.
fn selection_changed(window: &mut dyn Window) {
    if BUS.with(|slot| slot.borrow().is_none()) {
        return;
    }
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
        let event = |member: &str, detail1: i32| {
            Message::signal(
                &path,
                "org.a11y.atspi.Event.Object",
                member,
                vec![
                    Value::str(""),
                    Value::Int32(detail1),
                    Value::Int32(0),
                    Value::Variant(Box::new(Value::Int32(0))),
                    Value::Array("{sv}".to_owned(), Vec::new()),
                ],
            )
        };
        let _ = bus.connection.send(&event("TextCaretMoved", caret));
        if moved_selection {
            let _ = bus.connection.send(&event("TextSelectionChanged", 0));
        }
    });
}

fn element_path(id: u64) -> String {
    format!("{ELEMENT}{id}")
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
    Control(Element, usize),
}

fn object_at(path: &str, elements: &[Element]) -> Option<Object> {
    match path {
        ROOT => Some(Object::Application),
        FRAME => Some(Object::Frame),
        _ => {
            let id: u64 = path.strip_prefix(ELEMENT)?.parse().ok()?;
            let index = elements.iter().position(|element| element.id == id)?;
            Some(Object::Control(elements[index].clone(), index))
        }
    }
}

fn role_of(object: &Object) -> u32 {
    match object {
        Object::Application => role::APPLICATION,
        Object::Frame => role::FRAME,
        Object::Control(element, _) => match element.role {
            Role::Button => role::PUSH_BUTTON,
            Role::Toggle => role::TOGGLE_BUTTON,
            Role::TabItem => role::PAGE_TAB,
            Role::Document => role::DOCUMENT_TEXT,
            Role::Text => role::LABEL,
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
        _ => "label",
    }
}

fn interfaces_of(object: &Object) -> Vec<&'static str> {
    let mut interfaces = vec!["org.a11y.atspi.Accessible"];
    match object {
        Object::Application => interfaces.push("org.a11y.atspi.Application"),
        Object::Frame => interfaces.push("org.a11y.atspi.Component"),
        Object::Control(element, _) => {
            interfaces.push("org.a11y.atspi.Component");
            match element.role {
                Role::Button | Role::Toggle | Role::TabItem => {
                    interfaces.push("org.a11y.atspi.Action");
                }
                Role::Document => interfaces.push("org.a11y.atspi.Text"),
                Role::Text => {}
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
        Object::Control(element, _) => {
            if element.enabled {
                bits.extend([state::ENABLED, state::SENSITIVE]);
            }
            if element.focused {
                bits.push(state::FOCUSED);
            }
            match element.role {
                Role::Button => bits.push(state::FOCUSABLE),
                Role::Toggle => {
                    bits.extend([state::FOCUSABLE, state::CHECKABLE]);
                    if element.selected {
                        bits.extend([state::CHECKED, state::PRESSED]);
                    }
                }
                Role::TabItem => {
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
                Role::Text => {}
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
        Object::Control(element, _) => element.name.clone(),
    }
}

fn parent_of(object: &Object) -> Value {
    match object {
        Object::Application => desktop_reference(),
        Object::Frame => reference(ROOT),
        Object::Control(..) => reference(FRAME),
    }
}

fn children_of(object: &Object, elements: &[Element]) -> Vec<Value> {
    match object {
        Object::Application => vec![reference(FRAME)],
        Object::Frame => {
            elements.iter().map(|element| reference(&element_path(element.id))).collect()
        }
        Object::Control(..) => Vec::new(),
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
        Object::Control(element, _) => element.rect,
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
    let int = |index: usize| argument(index).and_then(Value::as_i64).unwrap_or(0);

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
            // The registry numbers each application it is given.
            if argument(1).and_then(Value::as_str) == Some("Id") {
                let id = argument(2).and_then(Value::as_i64).unwrap_or(0) as i32;
                BUS.with(|slot| {
                    if let Some(bus) = slot.borrow_mut().as_mut() {
                        bus.id = id;
                    }
                });
            }
            reply(Vec::new())
        }
        ("org.freedesktop.DBus.Introspectable", "Introspect") => reply(vec![Value::str("<node/>")]),
        ("org.freedesktop.DBus.Peer", "Ping") => reply(Vec::new()),
        ("org.a11y.atspi.Accessible", _) => accessible(message, &object, window, &elements),
        ("org.a11y.atspi.Application", "GetLocale") => reply(vec![Value::Str(locale())]),
        ("org.a11y.atspi.Component", _) => component(message, &object, window, &elements),
        ("org.a11y.atspi.Action", _) => {
            let Object::Control(element, _) = &object else { return unknown() };
            let key = element.access_key.clone();
            match member {
                "GetName" | "GetLocalizedName" => reply(vec![Value::str("press")]),
                "GetDescription" => reply(vec![Value::Str(element.name.clone())]),
                "GetKeyBinding" => reply(vec![Value::Str(key)]),
                "GetActions" => reply(vec![Value::Array(
                    "(sss)".to_owned(),
                    vec![Value::Struct(vec![
                        Value::str("press"),
                        Value::Str(element.name.clone()),
                        Value::Str(key),
                    ])],
                )]),
                "DoAction" => {
                    let _ = int(0);
                    window.invoke(element.id);
                    reply(vec![Value::Bool(true)])
                }
                _ => unknown(),
            }
        }
        ("org.a11y.atspi.Text", _) => text(message, window),
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

/// One of an object's properties.
fn property_of(
    object: &Object,
    name: &str,
    window: &mut dyn Window,
    elements: &[Element],
) -> Option<Value> {
    Some(match name {
        "Name" => Value::Str(name_of(object, window)),
        "Description" | "HelpText" => Value::str(""),
        "Parent" => parent_of(object),
        "ChildCount" => Value::Int32(children_of(object, elements).len() as i32),
        "Locale" => Value::Str(locale()),
        "AccessibleId" => Value::Str(match object {
            Object::Application => "application".to_owned(),
            Object::Frame => "window".to_owned(),
            Object::Control(element, _) => element.id.to_string(),
        }),
        "ToolkitName" => Value::str("word-processor"),
        "Version" => Value::str(env!("CARGO_PKG_VERSION")),
        "AtspiVersion" => Value::str("2.1"),
        "Id" => Value::Int32(BUS.with(|slot| slot.borrow().as_ref().map_or(0, |bus| bus.id))),
        "NActions" => Value::Int32(1),
        "CharacterCount" => {
            Value::Int32(window.text().map_or(0, |text| text.text.chars().count() as i32))
        }
        "CaretOffset" => Value::Int32(window.text().map_or(0, |text| text.selection.1 as i32)),
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
        "GetIndexInParent" => reply(vec![Value::Int32(match object {
            Object::Application => -1,
            Object::Frame => 0,
            Object::Control(_, index) => *index as i32,
        })]),
        "GetRelationSet" => reply(vec![Value::Array("(ua(so))".to_owned(), Vec::new())]),
        "GetRole" => reply(vec![Value::Uint32(role_of(object))]),
        "GetRoleName" | "GetLocalizedRoleName" => {
            reply(vec![Value::str(role_name(role_of(object)))])
        }
        "GetState" => reply(vec![Value::Array("u".to_owned(), states_of(object))]),
        "GetAttributes" => {
            let mut attributes = Vec::new();
            if let Object::Control(element, _) = object {
                if !element.access_key.is_empty() {
                    attributes.push(Value::DictEntry(
                        Box::new(Value::str("keyshortcuts")),
                        Box::new(Value::Str(element.access_key.clone())),
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
            let found = match object {
                Object::Frame => elements
                    .iter()
                    .rev()
                    .find(|element| inside(element.rect, window))
                    .map(|element| reference(&element_path(element.id))),
                _ => None,
            };
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

/// Where the unit a boundary names begins and ends around an offset: the
/// character, the word, the sentence, or the line — a line being a
/// paragraph, as the text has them.
fn unit_around(characters: &[char], offset: usize, granularity: u32) -> (usize, usize) {
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
        // A line, which is the paragraph, with its break.
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

fn text(message: &Message, window: &mut dyn Window) -> Option<Message> {
    let reply = |body: Vec<Value>| Some(message.reply(body));
    let int = |index: usize| {
        message.body.get(index).and_then(|value| value.inner().as_i64()).unwrap_or(0)
    };
    let state = window.text().unwrap_or_default();
    let characters: Vec<char> = state.text.chars().collect();
    let length = characters.len();
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
            let (start, end) = unit_around(&characters, clamp(int(0)), int(1) as u32);
            reply(unit(start, end))
        }
        "GetTextAtOffset" => {
            let granularity = granularity_of_boundary(int(1) as u32);
            let (start, end) = unit_around(&characters, clamp(int(0)), granularity);
            reply(unit(start, end))
        }
        "GetTextBeforeOffset" => {
            let granularity = granularity_of_boundary(int(1) as u32);
            let (start, _) = unit_around(&characters, clamp(int(0)), granularity);
            let (before, _) = unit_around(&characters, start.saturating_sub(1), granularity);
            reply(unit(before.min(start), start))
        }
        "GetTextAfterOffset" => {
            let granularity = granularity_of_boundary(int(1) as u32);
            let (_, end) = unit_around(&characters, clamp(int(0)), granularity);
            let (_, after) = unit_around(&characters, end, granularity);
            reply(unit(end, after.max(end)))
        }
        "SetCaretOffset" => {
            let at = clamp(int(0));
            window.select(at, at);
            reply(vec![Value::Bool(true)])
        }
        "GetNSelections" => reply(vec![Value::Int32(i32::from(selection_start != selection_end))]),
        "GetSelection" => {
            reply(vec![Value::Int32(selection_start as i32), Value::Int32(selection_end as i32)])
        }
        "AddSelection" | "SetSelection" => {
            let offset = usize::from(message.member.as_deref() == Some("SetSelection"));
            let (start, end) = (clamp(int(offset)), clamp(int(offset + 1)));
            window.select(start, end);
            reply(vec![Value::Bool(true)])
        }
        "RemoveSelection" => {
            window.select(selection_end, selection_end);
            reply(vec![Value::Bool(true)])
        }
        "GetCharacterExtents" | "GetRangeExtents" => {
            let range = message.member.as_deref() == Some("GetRangeExtents");
            let start = clamp(int(0));
            let end = if range { clamp(int(1)) } else { (start + 1).min(length.max(start + 1)) };
            let coords = int(if range { 2 } else { 1 }) as u32;
            let rects = window.rects(start.min(end), end.max(start));
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
        "GetAttributes" | "GetAttributeRun" => reply(vec![
            Value::Array("{ss}".to_owned(), Vec::new()),
            Value::Int32(0),
            Value::Int32(length as i32),
        ]),
        "GetAttributeValue" => reply(vec![Value::str("")]),
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
    objects.extend(
        elements.iter().enumerate().map(|(index, element)| Object::Control(element.clone(), index)),
    );
    let items = objects.iter().map(|object| cache_item(object, window, &elements)).collect();
    Value::Array("((so)(so)(so)iiassusau)".to_owned(), items)
}

/// One object as the cache holds it.
fn cache_item(object: &Object, window: &mut dyn Window, elements: &[Element]) -> Value {
    let path = match object {
        Object::Application => ROOT.to_owned(),
        Object::Frame => FRAME.to_owned(),
        Object::Control(element, _) => element_path(element.id),
    };
    let index = match object {
        Object::Application => -1,
        Object::Frame => 0,
        Object::Control(_, index) => *index as i32,
    };
    Value::Struct(vec![
        reference(&path),
        reference(ROOT),
        parent_of(object),
        Value::Int32(index),
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
        assert_eq!(unit_around(&text, 5, 1), (4, 7), "the word and not the stop");
        assert_eq!(unit_around(&text, 1, 1), (0, 4), "a word takes the space after it");
        assert_eq!(unit_around(&text, 2, 2), (0, 9), "the sentence to its stop");
        assert_eq!(unit_around(&text, 10, 3), (0, 15), "the line with its break");
        assert_eq!(unit_around(&text, 16, 3), (15, 19), "and the last without one");
        assert_eq!(unit_around(&text, 3, 0), (3, 4), "a character");
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
            focused: false,
        };
        let words = states_of(&Object::Control(element, 0));
        let low = words[0].as_i64().unwrap() as u32;
        let high = words[1].as_i64().unwrap() as u32;
        assert!(low & (1 << state::CHECKED) != 0 && low & (1 << state::ENABLED) != 0);
        assert!(high & (1 << (state::CHECKABLE - 32)) != 0, "checkable is in the second word");
    }
}

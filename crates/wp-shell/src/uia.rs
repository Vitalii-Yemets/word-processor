//! What a screen reader is told, through UI Automation.
//!
//! A window that draws its own ribbon, its own buttons and its own page is
//! one blank rectangle to a screen reader unless it says otherwise. UI
//! Automation is how a program on Windows says otherwise: when the system
//! asks the window for its root provider, the window gives an object that
//! describes itself — its children, each with a control type, a name, a
//! rectangle and the patterns it supports — and the screen reader walks
//! that. Buttons support Invoke, tabs SelectionItem, and the document Text,
//! which is a range of characters that can be moved by character, word,
//! paragraph or document, read, and selected; that last is what lets a
//! screen reader read a document the way it reads Word's.
//!
//! Everything here is a COM object written out against the documented
//! tables — see [`crate::com`] — and every answer comes from the
//! application through [`crate::App`]'s accessibility methods.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;

use crate::accessibility::{Element, Role, TextState};
use crate::com::{
    add_ref, query_interface, release, Counted, Guid, HResult, E_FAIL, E_INVALIDARG, E_POINTER,
    S_FALSE, S_OK,
};
use crate::windows::{
    owner_window, wide, with_application, ClientToScreen, Handle, InvalidateRect, Point,
    ScreenToClient,
};
use crate::Response;

const IID_IRAWELEMENTPROVIDERSIMPLE: Guid = Guid {
    data1: 0xD6DD_68D1,
    data2: 0x86FD,
    data3: 0x4332,
    data4: [0x86, 0x66, 0x9A, 0xBE, 0xDE, 0xA2, 0xD2, 0x4C],
};
const IID_IRAWELEMENTPROVIDERFRAGMENT: Guid = Guid {
    data1: 0xF706_3DA8,
    data2: 0x8359,
    data3: 0x439C,
    data4: [0x92, 0x97, 0xBB, 0xC5, 0x29, 0x9A, 0x7D, 0x87],
};
const IID_IRAWELEMENTPROVIDERFRAGMENTROOT: Guid = Guid {
    data1: 0x620C_E2A5,
    data2: 0xAB8F,
    data3: 0x40A9,
    data4: [0x86, 0xCB, 0xDE, 0x3C, 0x75, 0x59, 0x9B, 0x58],
};
const IID_IINVOKEPROVIDER: Guid = Guid {
    data1: 0x54FC_B24B,
    data2: 0xE18E,
    data3: 0x47A2,
    data4: [0xB4, 0xD3, 0xEC, 0xCB, 0xE7, 0x75, 0x99, 0xA2],
};
const IID_ISELECTIONITEMPROVIDER: Guid = Guid {
    data1: 0x2ACA_D808,
    data2: 0xB2D4,
    data3: 0x452D,
    data4: [0xA4, 0x07, 0x91, 0xFF, 0x1A, 0xD1, 0x67, 0xB2],
};
const IID_ITOGGLEPROVIDER: Guid = Guid {
    data1: 0x56D0_0BD0,
    data2: 0xC4F4,
    data3: 0x433C,
    data4: [0xA8, 0x36, 0x1A, 0x52, 0xA5, 0x7E, 0x08, 0x92],
};
const IID_ITEXTPROVIDER: Guid = Guid {
    data1: 0x3589_C92C,
    data2: 0x63F3,
    data3: 0x4367,
    data4: [0x99, 0xBB, 0xAD, 0xA6, 0x53, 0xB7, 0x7C, 0xF2],
};
const IID_ITEXTRANGEPROVIDER: Guid = Guid {
    data1: 0x5347_AD7B,
    data2: 0xC355,
    data3: 0x46F8,
    data4: [0xAF, 0xF5, 0x90, 0x90, 0x33, 0x58, 0x2F, 0x63],
};

/// The system's request for the root provider, in `WM_GETOBJECT`.
pub(crate) const ROOT_OBJECT_ID: isize = -25;

const PROVIDER_OPTIONS_SERVER_SIDE: i32 = 0x1;

// Property ids.
const UIA_LOCALIZED_CONTROL_TYPE: i32 = 30004;
const UIA_CONTROL_TYPE: i32 = 30003;
const UIA_NAME: i32 = 30005;
const UIA_ACCESS_KEY: i32 = 30007;
const UIA_HAS_KEYBOARD_FOCUS: i32 = 30008;
const UIA_IS_KEYBOARD_FOCUSABLE: i32 = 30009;
const UIA_IS_ENABLED: i32 = 30010;
const UIA_AUTOMATION_ID: i32 = 30011;
const UIA_CLASS_NAME: i32 = 30012;
const UIA_IS_CONTROL_ELEMENT: i32 = 30016;
const UIA_IS_CONTENT_ELEMENT: i32 = 30017;
const UIA_IS_OFFSCREEN: i32 = 30022;
const UIA_FRAMEWORK_ID: i32 = 30024;
const UIA_PROVIDER_DESCRIPTION: i32 = 30107;

// Control type ids.
const CONTROL_BUTTON: i32 = 50000;
const CONTROL_TAB_ITEM: i32 = 50019;
const CONTROL_TEXT: i32 = 50020;
const CONTROL_DOCUMENT: i32 = 50030;
const CONTROL_PANE: i32 = 50033;

// Pattern ids.
const PATTERN_INVOKE: i32 = 10000;
const PATTERN_SELECTION_ITEM: i32 = 10010;
const PATTERN_TEXT: i32 = 10014;
const PATTERN_TOGGLE: i32 = 10015;

// Events.
const EVENT_TEXT_SELECTION_CHANGED: i32 = 20014;
const EVENT_INVOKED: i32 = 20009;

// Navigation.
const NAVIGATE_PARENT: i32 = 0;
const NAVIGATE_NEXT_SIBLING: i32 = 1;
const NAVIGATE_PREVIOUS_SIBLING: i32 = 2;
const NAVIGATE_FIRST_CHILD: i32 = 3;
const NAVIGATE_LAST_CHILD: i32 = 4;

// Text units and endpoints.
const UNIT_CHARACTER: i32 = 0;
const UNIT_FORMAT: i32 = 1;
const UNIT_WORD: i32 = 2;
const UNIT_LINE: i32 = 3;
const UNIT_PARAGRAPH: i32 = 4;
const UNIT_PAGE: i32 = 5;
const UNIT_DOCUMENT: i32 = 6;
const ENDPOINT_START: i32 = 0;
const SUPPORTED_TEXT_SELECTION_SINGLE: i32 = 1;

/// The runtime id of a fragment starts with this, which says the rest is
/// the provider's own numbering.
const RUNTIME_ID_APPEND: i32 = 3;

// Variant types.
const VT_EMPTY: u16 = 0;
const VT_I4: u16 = 3;
const VT_R8: u16 = 5;
const VT_BSTR: u16 = 8;
const VT_BOOL: u16 = 11;
const VT_UNKNOWN: u16 = 13;

/// A value of any of the kinds the automation properties come in.
#[repr(C)]
struct Variant {
    kind: u16,
    reserved: [u16; 3],
    value: u64,
    extra: u64,
}

impl Variant {
    const EMPTY: Self = Self { kind: VT_EMPTY, reserved: [0; 3], value: 0, extra: 0 };

    fn integer(value: i32) -> Self {
        Self { kind: VT_I4, value: value as u32 as u64, ..Self::EMPTY }
    }

    fn boolean(value: bool) -> Self {
        Self { kind: VT_BOOL, value: if value { 0xFFFF } else { 0 }, ..Self::EMPTY }
    }

    /// A string, as the BSTR the system frees.
    unsafe fn text(value: &str) -> Self {
        let units = wide(value);
        let length = units.len().saturating_sub(1) as u32;
        let bstr = SysAllocStringLen(units.as_ptr(), length);
        Self { kind: VT_BSTR, value: bstr as u64, ..Self::EMPTY }
    }

    unsafe fn unknown(object: *mut c_void) -> Self {
        Self { kind: VT_UNKNOWN, value: object as u64, ..Self::EMPTY }
    }
}

/// A rectangle on the screen, in the automation's own doubles.
#[repr(C)]
struct UiaRect {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UiaPoint {
    x: f64,
    y: f64,
}

#[link(name = "uiautomationcore")]
extern "system" {
    fn UiaReturnRawElementProvider(
        window: Handle,
        word: usize,
        long: isize,
        provider: *mut c_void,
    ) -> isize;
    fn UiaHostProviderFromHwnd(window: Handle, provider: *mut *mut c_void) -> HResult;
    fn UiaRaiseAutomationEvent(provider: *mut c_void, event: i32) -> HResult;
    fn UiaClientsAreListening() -> i32;
    fn UiaGetReservedNotSupportedValue(value: *mut *mut c_void) -> HResult;
}

#[link(name = "oleaut32")]
extern "system" {
    fn SysAllocStringLen(text: *const u16, length: u32) -> *mut u16;
    fn SafeArrayCreateVector(kind: u16, lower: i32, count: u32) -> *mut c_void;
    fn SafeArrayPutElement(array: *mut c_void, indices: *const i32, value: *mut c_void) -> HResult;
}

thread_local! {
    /// The root provider of each window, made when the system first asks.
    static ROOTS: RefCell<HashMap<usize, *mut Root>> = RefCell::new(HashMap::new());
}

/// Answers the system's request for the window's root provider.
pub(crate) fn root_provider(window: Handle, word: usize, long: isize) -> isize {
    let root = ROOTS.with(|roots| {
        *roots.borrow_mut().entry(window as usize).or_insert_with(|| {
            Box::into_raw(Box::new(Root { vtbl: &ROOT_VTBL, refs: Cell::new(1), window }))
        })
    });
    // SAFETY: the root outlives the window, which is unregistered before it
    // goes; the system takes references of its own.
    unsafe { UiaReturnRawElementProvider(window, word, long, root.cast()) }
}

/// Lets the system go of a window's providers as the window closes.
pub(crate) fn forget_window(window: Handle) {
    let root = ROOTS.with(|roots| roots.borrow_mut().remove(&(window as usize)));
    if let Some(root) = root {
        // SAFETY: the window is still valid, being in the middle of its
        // destroy message; the root's own reference is given up last.
        unsafe {
            UiaReturnRawElementProvider(window, 0, 0, core::ptr::null_mut());
            release(root);
        }
    }
}

/// Tells a screen reader the document's selection moved.
pub(crate) fn selection_changed() {
    // SAFETY: the calls take an object made here and released after.
    unsafe {
        if UiaClientsAreListening() == 0 {
            return;
        }
        let window = owner_window();
        if window.is_null() {
            return;
        }
        let Some(document) = with_application(|app| {
            app.accessible_elements().into_iter().find(|element| element.role == Role::Document)
        })
        .flatten() else {
            return;
        };
        let item = Item::new(window, document.id);
        UiaRaiseAutomationEvent(item.cast(), EVENT_TEXT_SELECTION_CHANGED);
        release(item);
    }
}

/// The elements as the application gives them now.
fn elements() -> Vec<Element> {
    with_application(|app| app.accessible_elements()).unwrap_or_default()
}

fn element(id: u64) -> Option<Element> {
    elements().into_iter().find(|element| element.id == id)
}

/// A client rectangle as a screen one, in the automation's doubles.
unsafe fn screen_rect(window: Handle, rect: (i32, i32, i32, i32)) -> UiaRect {
    let scale = crate::windows::scale_of(window);
    let (x, y) = crate::windows::to_device(window, (rect.0, rect.1));
    let mut corner = Point { x, y };
    ClientToScreen(window, &mut corner);
    UiaRect {
        left: f64::from(corner.x),
        top: f64::from(corner.y),
        width: f64::from(rect.2) * f64::from(scale),
        height: f64::from(rect.3) * f64::from(scale),
    }
}

/// An array of interface pointers, as the system wants lists of them.
unsafe fn array_of_unknowns(objects: &[*mut c_void]) -> *mut c_void {
    let array = SafeArrayCreateVector(VT_UNKNOWN, 0, objects.len() as u32);
    if array.is_null() {
        return array;
    }
    for (index, object) in objects.iter().enumerate() {
        let index = index as i32;
        SafeArrayPutElement(array, &index, *object);
    }
    array
}

unsafe fn array_of_integers(values: &[i32]) -> *mut c_void {
    let array = SafeArrayCreateVector(VT_I4, 0, values.len() as u32);
    if array.is_null() {
        return array;
    }
    for (index, value) in values.iter().enumerate() {
        let index = index as i32;
        let mut value = *value;
        SafeArrayPutElement(array, &index, (&mut value as *mut i32).cast());
    }
    array
}

unsafe fn array_of_doubles(values: &[f64]) -> *mut c_void {
    let array = SafeArrayCreateVector(VT_R8, 0, values.len() as u32);
    if array.is_null() {
        return array;
    }
    for (index, value) in values.iter().enumerate() {
        let index = index as i32;
        let mut value = *value;
        SafeArrayPutElement(array, &index, (&mut value as *mut f64).cast());
    }
    array
}

fn redraw(window: Handle, response: Response) {
    if response == Response::Redraw {
        // SAFETY: the window is one of this thread's.
        unsafe {
            InvalidateRect(window, core::ptr::null(), 0);
        }
    }
}

// --- The root: the window itself -----------------------------------------------

/// `IRawElementProviderFragmentRoot`, which is a fragment, which is a
/// simple provider: one table serves all three.
#[repr(C)]
struct RootVtbl {
    query_interface: unsafe extern "system" fn(*mut Root, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut Root) -> u32,
    release: unsafe extern "system" fn(*mut Root) -> u32,
    get_provider_options: unsafe extern "system" fn(*mut Root, *mut i32) -> HResult,
    get_pattern_provider: unsafe extern "system" fn(*mut Root, i32, *mut *mut c_void) -> HResult,
    get_property_value: unsafe extern "system" fn(*mut Root, i32, *mut Variant) -> HResult,
    get_host_raw_element_provider:
        unsafe extern "system" fn(*mut Root, *mut *mut c_void) -> HResult,
    navigate: unsafe extern "system" fn(*mut Root, i32, *mut *mut c_void) -> HResult,
    get_runtime_id: unsafe extern "system" fn(*mut Root, *mut *mut c_void) -> HResult,
    get_bounding_rectangle: unsafe extern "system" fn(*mut Root, *mut UiaRect) -> HResult,
    get_embedded_fragment_roots: unsafe extern "system" fn(*mut Root, *mut *mut c_void) -> HResult,
    set_focus: unsafe extern "system" fn(*mut Root) -> HResult,
    get_fragment_root: unsafe extern "system" fn(*mut Root, *mut *mut c_void) -> HResult,
    element_provider_from_point:
        unsafe extern "system" fn(*mut Root, f64, f64, *mut *mut c_void) -> HResult,
    get_focus: unsafe extern "system" fn(*mut Root, *mut *mut c_void) -> HResult,
}

#[repr(C)]
struct Root {
    vtbl: *const RootVtbl,
    refs: Cell<u32>,
    window: Handle,
}

impl Counted for Root {
    fn refs(&self) -> &Cell<u32> {
        &self.refs
    }
}

static ROOT_VTBL: RootVtbl = RootVtbl {
    query_interface: root_query_interface,
    add_ref: add_ref::<Root>,
    release: release::<Root>,
    get_provider_options: root_get_provider_options,
    get_pattern_provider: root_get_pattern_provider,
    get_property_value: root_get_property_value,
    get_host_raw_element_provider: root_get_host_raw_element_provider,
    navigate: root_navigate,
    get_runtime_id: root_get_runtime_id,
    get_bounding_rectangle: root_get_bounding_rectangle,
    get_embedded_fragment_roots: root_get_embedded_fragment_roots,
    set_focus: root_set_focus,
    get_fragment_root: root_get_fragment_root,
    element_provider_from_point: root_element_provider_from_point,
    get_focus: root_get_focus,
};

unsafe extern "system" fn root_query_interface(
    this: *mut Root,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HResult {
    query_interface(
        this,
        iid,
        out,
        &[
            IID_IRAWELEMENTPROVIDERSIMPLE,
            IID_IRAWELEMENTPROVIDERFRAGMENT,
            IID_IRAWELEMENTPROVIDERFRAGMENTROOT,
        ],
    )
}

unsafe extern "system" fn root_get_provider_options(_this: *mut Root, out: *mut i32) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = PROVIDER_OPTIONS_SERVER_SIDE;
    S_OK
}

unsafe extern "system" fn root_get_pattern_provider(
    _this: *mut Root,
    _pattern: i32,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    S_OK
}

unsafe extern "system" fn root_get_property_value(
    _this: *mut Root,
    property: i32,
    out: *mut Variant,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = match property {
        UIA_CONTROL_TYPE => Variant::integer(CONTROL_PANE),
        UIA_IS_CONTROL_ELEMENT | UIA_IS_CONTENT_ELEMENT => Variant::boolean(true),
        UIA_FRAMEWORK_ID => Variant::text("Word Processor"),
        UIA_PROVIDER_DESCRIPTION => Variant::text("Word Processor: the window"),
        _ => Variant::EMPTY,
    };
    S_OK
}

unsafe extern "system" fn root_get_host_raw_element_provider(
    this: *mut Root,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    UiaHostProviderFromHwnd((*this).window, out)
}

unsafe extern "system" fn root_navigate(
    this: *mut Root,
    direction: i32,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    let all = elements();
    let chosen = match direction {
        NAVIGATE_FIRST_CHILD => all.first(),
        NAVIGATE_LAST_CHILD => all.last(),
        _ => None,
    };
    if let Some(chosen) = chosen {
        *out = Item::new((*this).window, chosen.id).cast();
    }
    S_OK
}

unsafe extern "system" fn root_get_runtime_id(_this: *mut Root, out: *mut *mut c_void) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    // The host's runtime id stands for the root.
    *out = core::ptr::null_mut();
    S_OK
}

unsafe extern "system" fn root_get_bounding_rectangle(
    _this: *mut Root,
    out: *mut UiaRect,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    // The host says where the window is.
    *out = UiaRect { left: 0.0, top: 0.0, width: 0.0, height: 0.0 };
    S_OK
}

unsafe extern "system" fn root_get_embedded_fragment_roots(
    _this: *mut Root,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    S_OK
}

unsafe extern "system" fn root_set_focus(_this: *mut Root) -> HResult {
    S_OK
}

unsafe extern "system" fn root_get_fragment_root(
    this: *mut Root,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    add_ref(this);
    *out = this.cast();
    S_OK
}

unsafe extern "system" fn root_element_provider_from_point(
    this: *mut Root,
    x: f64,
    y: f64,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    let mut point = Point { x: x as i32, y: y as i32 };
    ScreenToClient((*this).window, &mut point);
    let (px, py) = crate::windows::to_logical((*this).window, (point.x, point.y));
    let hit = elements().into_iter().find(|element| {
        let (left, top, width, height) = element.rect;
        px >= left && px < left + width && py >= top && py < top + height
    });
    if let Some(hit) = hit {
        *out = Item::new((*this).window, hit.id).cast();
    }
    S_OK
}

unsafe extern "system" fn root_get_focus(this: *mut Root, out: *mut *mut c_void) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if let Some(focused) = elements().into_iter().find(|element| element.focused) {
        *out = Item::new((*this).window, focused.id).cast();
    }
    S_OK
}

// --- An item: one control on the window ------------------------------------------

/// `IRawElementProviderFragment`, which is a simple provider too.
#[repr(C)]
struct ItemVtbl {
    query_interface: unsafe extern "system" fn(*mut Item, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut Item) -> u32,
    release: unsafe extern "system" fn(*mut Item) -> u32,
    get_provider_options: unsafe extern "system" fn(*mut Item, *mut i32) -> HResult,
    get_pattern_provider: unsafe extern "system" fn(*mut Item, i32, *mut *mut c_void) -> HResult,
    get_property_value: unsafe extern "system" fn(*mut Item, i32, *mut Variant) -> HResult,
    get_host_raw_element_provider:
        unsafe extern "system" fn(*mut Item, *mut *mut c_void) -> HResult,
    navigate: unsafe extern "system" fn(*mut Item, i32, *mut *mut c_void) -> HResult,
    get_runtime_id: unsafe extern "system" fn(*mut Item, *mut *mut c_void) -> HResult,
    get_bounding_rectangle: unsafe extern "system" fn(*mut Item, *mut UiaRect) -> HResult,
    get_embedded_fragment_roots: unsafe extern "system" fn(*mut Item, *mut *mut c_void) -> HResult,
    set_focus: unsafe extern "system" fn(*mut Item) -> HResult,
    get_fragment_root: unsafe extern "system" fn(*mut Item, *mut *mut c_void) -> HResult,
}

#[repr(C)]
struct Item {
    vtbl: *const ItemVtbl,
    refs: Cell<u32>,
    window: Handle,
    id: u64,
}

impl Counted for Item {
    fn refs(&self) -> &Cell<u32> {
        &self.refs
    }
}

impl Item {
    fn new(window: Handle, id: u64) -> *mut Self {
        Box::into_raw(Box::new(Self { vtbl: &ITEM_VTBL, refs: Cell::new(1), window, id }))
    }
}

static ITEM_VTBL: ItemVtbl = ItemVtbl {
    query_interface: item_query_interface,
    add_ref: add_ref::<Item>,
    release: release::<Item>,
    get_provider_options: item_get_provider_options,
    get_pattern_provider: item_get_pattern_provider,
    get_property_value: item_get_property_value,
    get_host_raw_element_provider: item_get_host_raw_element_provider,
    navigate: item_navigate,
    get_runtime_id: item_get_runtime_id,
    get_bounding_rectangle: item_get_bounding_rectangle,
    get_embedded_fragment_roots: item_get_embedded_fragment_roots,
    set_focus: item_set_focus,
    get_fragment_root: item_get_fragment_root,
};

unsafe extern "system" fn item_query_interface(
    this: *mut Item,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HResult {
    query_interface(
        this,
        iid,
        out,
        &[IID_IRAWELEMENTPROVIDERSIMPLE, IID_IRAWELEMENTPROVIDERFRAGMENT],
    )
}

unsafe extern "system" fn item_get_provider_options(_this: *mut Item, out: *mut i32) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = PROVIDER_OPTIONS_SERVER_SIDE;
    S_OK
}

unsafe extern "system" fn item_get_pattern_provider(
    this: *mut Item,
    pattern: i32,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    let Some(element) = element((*this).id) else { return S_OK };
    let window = (*this).window;
    *out = match (element.role, pattern) {
        (Role::Button | Role::Toggle | Role::TabItem, PATTERN_INVOKE) => {
            Pattern::new(&INVOKE_VTBL, window, element.id).cast()
        }
        (Role::Toggle, PATTERN_TOGGLE) => Pattern::new(&TOGGLE_VTBL, window, element.id).cast(),
        (Role::TabItem, PATTERN_SELECTION_ITEM) => {
            Pattern::new(&SELECTION_ITEM_VTBL, window, element.id).cast()
        }
        (Role::Document, PATTERN_TEXT) => Pattern::new(&TEXT_VTBL, window, element.id).cast(),
        _ => core::ptr::null_mut(),
    };
    S_OK
}

fn control_type(role: Role) -> i32 {
    match role {
        Role::Button | Role::Toggle => CONTROL_BUTTON,
        Role::TabItem => CONTROL_TAB_ITEM,
        Role::Document => CONTROL_DOCUMENT,
        Role::Text => CONTROL_TEXT,
    }
}

fn control_name(role: Role) -> &'static str {
    match role {
        Role::Button | Role::Toggle => "button",
        Role::TabItem => "tab item",
        Role::Document => "document",
        Role::Text => "text",
    }
}

unsafe extern "system" fn item_get_property_value(
    this: *mut Item,
    property: i32,
    out: *mut Variant,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = Variant::EMPTY;
    let Some(element) = element((*this).id) else { return S_OK };
    *out = match property {
        UIA_CONTROL_TYPE => Variant::integer(control_type(element.role)),
        UIA_LOCALIZED_CONTROL_TYPE => Variant::text(control_name(element.role)),
        UIA_NAME => Variant::text(&element.name),
        UIA_ACCESS_KEY if !element.access_key.is_empty() => Variant::text(&element.access_key),
        UIA_HAS_KEYBOARD_FOCUS => Variant::boolean(element.focused),
        UIA_IS_KEYBOARD_FOCUSABLE => Variant::boolean(element.role == Role::Document),
        UIA_IS_ENABLED => Variant::boolean(element.enabled),
        UIA_AUTOMATION_ID => Variant::text(&format!("wp-{}", element.id)),
        UIA_CLASS_NAME => Variant::text(control_name(element.role)),
        UIA_IS_CONTROL_ELEMENT | UIA_IS_CONTENT_ELEMENT => Variant::boolean(true),
        UIA_IS_OFFSCREEN => Variant::boolean(false),
        UIA_FRAMEWORK_ID => Variant::text("Word Processor"),
        UIA_PROVIDER_DESCRIPTION => Variant::text("Word Processor"),
        _ => Variant::EMPTY,
    };
    S_OK
}

unsafe extern "system" fn item_get_host_raw_element_provider(
    _this: *mut Item,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    S_OK
}

unsafe extern "system" fn item_navigate(
    this: *mut Item,
    direction: i32,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    let window = (*this).window;
    match direction {
        NAVIGATE_PARENT => {
            let root = ROOTS.with(|roots| roots.borrow().get(&(window as usize)).copied());
            if let Some(root) = root {
                add_ref(root);
                *out = root.cast();
            }
        }
        NAVIGATE_NEXT_SIBLING | NAVIGATE_PREVIOUS_SIBLING => {
            let all = elements();
            let Some(index) = all.iter().position(|element| element.id == (*this).id) else {
                return S_OK;
            };
            let neighbour = if direction == NAVIGATE_NEXT_SIBLING {
                all.get(index + 1)
            } else {
                index.checked_sub(1).and_then(|before| all.get(before))
            };
            if let Some(neighbour) = neighbour {
                *out = Item::new(window, neighbour.id).cast();
            }
        }
        _ => {}
    }
    S_OK
}

unsafe extern "system" fn item_get_runtime_id(this: *mut Item, out: *mut *mut c_void) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = array_of_integers(&[RUNTIME_ID_APPEND, (*this).id as i32]);
    S_OK
}

unsafe extern "system" fn item_get_bounding_rectangle(
    this: *mut Item,
    out: *mut UiaRect,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = match element((*this).id) {
        Some(element) => screen_rect((*this).window, element.rect),
        None => UiaRect { left: 0.0, top: 0.0, width: 0.0, height: 0.0 },
    };
    S_OK
}

unsafe extern "system" fn item_get_embedded_fragment_roots(
    _this: *mut Item,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    S_OK
}

unsafe extern "system" fn item_set_focus(_this: *mut Item) -> HResult {
    S_OK
}

unsafe extern "system" fn item_get_fragment_root(
    this: *mut Item,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    let root = ROOTS.with(|roots| roots.borrow().get(&((*this).window as usize)).copied());
    if let Some(root) = root {
        add_ref(root);
        *out = root.cast();
    }
    S_OK
}

// --- The patterns a control supports -----------------------------------------------

/// One object shape serves every pattern: which pattern it is, is which
/// table it points at.
#[repr(C)]
struct Pattern {
    vtbl: *const c_void,
    refs: Cell<u32>,
    window: Handle,
    id: u64,
}

impl Counted for Pattern {
    fn refs(&self) -> &Cell<u32> {
        &self.refs
    }
}

impl Pattern {
    fn new<V>(vtbl: &'static V, window: Handle, id: u64) -> *mut Self {
        Box::into_raw(Box::new(Self {
            vtbl: (vtbl as *const V).cast(),
            refs: Cell::new(1),
            window,
            id,
        }))
    }
}

#[repr(C)]
struct InvokeVtbl {
    query_interface:
        unsafe extern "system" fn(*mut Pattern, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut Pattern) -> u32,
    release: unsafe extern "system" fn(*mut Pattern) -> u32,
    invoke: unsafe extern "system" fn(*mut Pattern) -> HResult,
}

static INVOKE_VTBL: InvokeVtbl = InvokeVtbl {
    query_interface: invoke_query_interface,
    add_ref: add_ref::<Pattern>,
    release: release::<Pattern>,
    invoke: pattern_invoke,
};

unsafe extern "system" fn invoke_query_interface(
    this: *mut Pattern,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HResult {
    query_interface(this, iid, out, &[IID_IINVOKEPROVIDER])
}

/// Presses the control, and tells the screen reader it was pressed.
unsafe extern "system" fn pattern_invoke(this: *mut Pattern) -> HResult {
    let (window, id) = ((*this).window, (*this).id);
    let Some(response) = with_application(|app| app.accessible_invoke(id)) else { return E_FAIL };
    redraw(window, response);
    let item = Item::new(window, id);
    UiaRaiseAutomationEvent(item.cast(), EVENT_INVOKED);
    release(item);
    S_OK
}

#[repr(C)]
struct ToggleVtbl {
    query_interface:
        unsafe extern "system" fn(*mut Pattern, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut Pattern) -> u32,
    release: unsafe extern "system" fn(*mut Pattern) -> u32,
    toggle: unsafe extern "system" fn(*mut Pattern) -> HResult,
    get_toggle_state: unsafe extern "system" fn(*mut Pattern, *mut i32) -> HResult,
}

static TOGGLE_VTBL: ToggleVtbl = ToggleVtbl {
    query_interface: toggle_query_interface,
    add_ref: add_ref::<Pattern>,
    release: release::<Pattern>,
    toggle: pattern_invoke,
    get_toggle_state: toggle_get_state,
};

unsafe extern "system" fn toggle_query_interface(
    this: *mut Pattern,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HResult {
    query_interface(this, iid, out, &[IID_ITOGGLEPROVIDER])
}

unsafe extern "system" fn toggle_get_state(this: *mut Pattern, out: *mut i32) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    // Off is nought, on is one.
    *out = i32::from(element((*this).id).is_some_and(|element| element.selected));
    S_OK
}

#[repr(C)]
struct SelectionItemVtbl {
    query_interface:
        unsafe extern "system" fn(*mut Pattern, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut Pattern) -> u32,
    release: unsafe extern "system" fn(*mut Pattern) -> u32,
    select: unsafe extern "system" fn(*mut Pattern) -> HResult,
    add_to_selection: unsafe extern "system" fn(*mut Pattern) -> HResult,
    remove_from_selection: unsafe extern "system" fn(*mut Pattern) -> HResult,
    get_is_selected: unsafe extern "system" fn(*mut Pattern, *mut i32) -> HResult,
    get_selection_container: unsafe extern "system" fn(*mut Pattern, *mut *mut c_void) -> HResult,
}

static SELECTION_ITEM_VTBL: SelectionItemVtbl = SelectionItemVtbl {
    query_interface: selection_item_query_interface,
    add_ref: add_ref::<Pattern>,
    release: release::<Pattern>,
    select: pattern_invoke,
    add_to_selection: pattern_invoke,
    remove_from_selection: selection_item_remove,
    get_is_selected: selection_item_is_selected,
    get_selection_container: selection_item_container,
};

unsafe extern "system" fn selection_item_query_interface(
    this: *mut Pattern,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HResult {
    query_interface(this, iid, out, &[IID_ISELECTIONITEMPROVIDER])
}

/// A tab cannot be unchosen: one is always open.
unsafe extern "system" fn selection_item_remove(_this: *mut Pattern) -> HResult {
    E_INVALIDARG
}

unsafe extern "system" fn selection_item_is_selected(this: *mut Pattern, out: *mut i32) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = i32::from(element((*this).id).is_some_and(|element| element.selected));
    S_OK
}

unsafe extern "system" fn selection_item_container(
    _this: *mut Pattern,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    S_OK
}

// --- The document as text -------------------------------------------------------------

#[repr(C)]
struct TextVtbl {
    query_interface:
        unsafe extern "system" fn(*mut Pattern, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut Pattern) -> u32,
    release: unsafe extern "system" fn(*mut Pattern) -> u32,
    get_selection: unsafe extern "system" fn(*mut Pattern, *mut *mut c_void) -> HResult,
    get_visible_ranges: unsafe extern "system" fn(*mut Pattern, *mut *mut c_void) -> HResult,
    range_from_child:
        unsafe extern "system" fn(*mut Pattern, *mut c_void, *mut *mut c_void) -> HResult,
    range_from_point:
        unsafe extern "system" fn(*mut Pattern, UiaPoint, *mut *mut c_void) -> HResult,
    get_document_range: unsafe extern "system" fn(*mut Pattern, *mut *mut c_void) -> HResult,
    get_supported_text_selection: unsafe extern "system" fn(*mut Pattern, *mut i32) -> HResult,
}

static TEXT_VTBL: TextVtbl = TextVtbl {
    query_interface: text_query_interface,
    add_ref: add_ref::<Pattern>,
    release: release::<Pattern>,
    get_selection: text_get_selection,
    get_visible_ranges: text_get_visible_ranges,
    range_from_child: text_range_from_child,
    range_from_point: text_range_from_point,
    get_document_range: text_get_document_range,
    get_supported_text_selection: text_get_supported_text_selection,
};

unsafe extern "system" fn text_query_interface(
    this: *mut Pattern,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HResult {
    query_interface(this, iid, out, &[IID_ITEXTPROVIDER])
}

fn text_state() -> Option<TextState> {
    with_application(|app| app.accessible_text()).flatten()
}

unsafe extern "system" fn text_get_selection(this: *mut Pattern, out: *mut *mut c_void) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    let Some(state) = text_state() else { return E_FAIL };
    let range = Range::new((*this).window, (*this).id, state.selection.0, state.selection.1);
    *out = array_of_unknowns(&[range.cast()]);
    release(range);
    S_OK
}

unsafe extern "system" fn text_get_visible_ranges(
    this: *mut Pattern,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    let Some(state) = text_state() else { return E_FAIL };
    let range = Range::new((*this).window, (*this).id, 0, state.text.chars().count());
    *out = array_of_unknowns(&[range.cast()]);
    release(range);
    S_OK
}

unsafe extern "system" fn text_range_from_child(
    _this: *mut Pattern,
    _child: *mut c_void,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    S_OK
}

/// The range at a point: where the caret is, since the page's own
/// geometry is not asked here.
unsafe extern "system" fn text_range_from_point(
    this: *mut Pattern,
    _point: UiaPoint,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    let Some(state) = text_state() else { return E_FAIL };
    *out = Range::new((*this).window, (*this).id, state.selection.0, state.selection.0).cast();
    S_OK
}

unsafe extern "system" fn text_get_document_range(
    this: *mut Pattern,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    let Some(state) = text_state() else { return E_FAIL };
    *out = Range::new((*this).window, (*this).id, 0, state.text.chars().count()).cast();
    S_OK
}

unsafe extern "system" fn text_get_supported_text_selection(
    _this: *mut Pattern,
    out: *mut i32,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = SUPPORTED_TEXT_SELECTION_SINGLE;
    S_OK
}

// --- A range of the document's text ---------------------------------------------------

#[repr(C)]
struct RangeVtbl {
    query_interface:
        unsafe extern "system" fn(*mut Range, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut Range) -> u32,
    release: unsafe extern "system" fn(*mut Range) -> u32,
    clone: unsafe extern "system" fn(*mut Range, *mut *mut c_void) -> HResult,
    compare: unsafe extern "system" fn(*mut Range, *mut Range, *mut i32) -> HResult,
    compare_endpoints:
        unsafe extern "system" fn(*mut Range, i32, *mut Range, i32, *mut i32) -> HResult,
    expand_to_enclosing_unit: unsafe extern "system" fn(*mut Range, i32) -> HResult,
    find_attribute:
        unsafe extern "system" fn(*mut Range, i32, Variant, i32, *mut *mut c_void) -> HResult,
    find_text:
        unsafe extern "system" fn(*mut Range, *mut u16, i32, i32, *mut *mut c_void) -> HResult,
    get_attribute_value: unsafe extern "system" fn(*mut Range, i32, *mut Variant) -> HResult,
    get_bounding_rectangles: unsafe extern "system" fn(*mut Range, *mut *mut c_void) -> HResult,
    get_enclosing_element: unsafe extern "system" fn(*mut Range, *mut *mut c_void) -> HResult,
    get_text: unsafe extern "system" fn(*mut Range, i32, *mut *mut u16) -> HResult,
    move_by: unsafe extern "system" fn(*mut Range, i32, i32, *mut i32) -> HResult,
    move_endpoint_by_unit:
        unsafe extern "system" fn(*mut Range, i32, i32, i32, *mut i32) -> HResult,
    move_endpoint_by_range: unsafe extern "system" fn(*mut Range, i32, *mut Range, i32) -> HResult,
    select: unsafe extern "system" fn(*mut Range) -> HResult,
    add_to_selection: unsafe extern "system" fn(*mut Range) -> HResult,
    remove_from_selection: unsafe extern "system" fn(*mut Range) -> HResult,
    scroll_into_view: unsafe extern "system" fn(*mut Range, i32) -> HResult,
    get_children: unsafe extern "system" fn(*mut Range, *mut *mut c_void) -> HResult,
}

/// A stretch of the document, as character offsets into its text.
#[repr(C)]
struct Range {
    vtbl: *const RangeVtbl,
    refs: Cell<u32>,
    window: Handle,
    /// The document element, for the enclosing element.
    document: u64,
    start: Cell<usize>,
    end: Cell<usize>,
}

impl Counted for Range {
    fn refs(&self) -> &Cell<u32> {
        &self.refs
    }
}

impl Range {
    fn new(window: Handle, document: u64, start: usize, end: usize) -> *mut Self {
        Box::into_raw(Box::new(Self {
            vtbl: &RANGE_VTBL,
            refs: Cell::new(1),
            window,
            document,
            start: Cell::new(start.min(end)),
            end: Cell::new(end.max(start)),
        }))
    }
}

static RANGE_VTBL: RangeVtbl = RangeVtbl {
    query_interface: range_query_interface,
    add_ref: add_ref::<Range>,
    release: release::<Range>,
    clone: range_clone,
    compare: range_compare,
    compare_endpoints: range_compare_endpoints,
    expand_to_enclosing_unit: range_expand_to_enclosing_unit,
    find_attribute: range_find_attribute,
    find_text: range_find_text,
    get_attribute_value: range_get_attribute_value,
    get_bounding_rectangles: range_get_bounding_rectangles,
    get_enclosing_element: range_get_enclosing_element,
    get_text: range_get_text,
    move_by: range_move,
    move_endpoint_by_unit: range_move_endpoint_by_unit,
    move_endpoint_by_range: range_move_endpoint_by_range,
    select: range_select,
    add_to_selection: range_select,
    remove_from_selection: range_remove_from_selection,
    scroll_into_view: range_scroll_into_view,
    get_children: range_get_children,
};

unsafe extern "system" fn range_query_interface(
    this: *mut Range,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HResult {
    query_interface(this, iid, out, &[IID_ITEXTRANGEPROVIDER])
}

/// Whether another range is one of ours, and so can be looked into.
unsafe fn ours(other: *mut Range) -> bool {
    !other.is_null() && core::ptr::eq((*other).vtbl, &RANGE_VTBL)
}

unsafe extern "system" fn range_clone(this: *mut Range, out: *mut *mut c_void) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    let range = &*this;
    *out = Range::new(range.window, range.document, range.start.get(), range.end.get()).cast();
    S_OK
}

unsafe extern "system" fn range_compare(
    this: *mut Range,
    other: *mut Range,
    out: *mut i32,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    if !ours(other) {
        return E_INVALIDARG;
    }
    let (range, other) = (&*this, &*other);
    *out = i32::from(range.start.get() == other.start.get() && range.end.get() == other.end.get());
    S_OK
}

unsafe extern "system" fn range_compare_endpoints(
    this: *mut Range,
    endpoint: i32,
    other: *mut Range,
    other_endpoint: i32,
    out: *mut i32,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    if !ours(other) {
        return E_INVALIDARG;
    }
    let mine = if endpoint == ENDPOINT_START { (*this).start.get() } else { (*this).end.get() };
    let theirs =
        if other_endpoint == ENDPOINT_START { (*other).start.get() } else { (*other).end.get() };
    *out = match mine.cmp(&theirs) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    };
    S_OK
}

/// Where the units of a kind begin in the text: every offset a unit starts
/// at, and the offset past the end.
fn unit_starts(text: &str, unit: i32) -> Vec<usize> {
    let chars: Vec<char> = text.chars().collect();
    let count = chars.len();
    let mut starts = vec![0];
    match unit {
        UNIT_CHARACTER => starts.extend(1..count),
        UNIT_WORD => {
            // A word runs from its first letter to the start of the next word,
            // spaces included, which is how the pattern counts words.
            for index in 1..count {
                let before = chars[index - 1];
                let here = chars[index];
                let starts_word = (before.is_whitespace() && !here.is_whitespace())
                    || (before != '\n' && here == '\n')
                    || before == '\n';
                if starts_word {
                    starts.push(index);
                }
            }
        }
        UNIT_LINE | UNIT_PARAGRAPH | UNIT_FORMAT => {
            for index in 1..count {
                if chars[index - 1] == '\n' {
                    starts.push(index);
                }
            }
        }
        _ => {}
    }
    starts.dedup();
    if starts.last().copied() != Some(count) {
        starts.push(count);
    }
    starts
}

/// The start of the unit an offset is in, and the start of the next.
fn unit_round(starts: &[usize], offset: usize) -> (usize, usize) {
    let index = starts.iter().rposition(|start| *start <= offset).unwrap_or(0);
    let start = starts[index];
    let end = starts.get(index + 1).copied().unwrap_or(start);
    (start, end)
}

unsafe extern "system" fn range_expand_to_enclosing_unit(this: *mut Range, unit: i32) -> HResult {
    let Some(state) = text_state() else { return E_FAIL };
    let range = &*this;
    let count = state.text.chars().count();
    if matches!(unit, UNIT_DOCUMENT | UNIT_PAGE) {
        range.start.set(0);
        range.end.set(count);
        return S_OK;
    }
    let starts = unit_starts(&state.text, unit);
    let (start, end) = unit_round(&starts, range.start.get().min(count));
    range.start.set(start);
    range.end.set(end.max(start));
    S_OK
}

unsafe extern "system" fn range_find_attribute(
    _this: *mut Range,
    _attribute: i32,
    _value: Variant,
    _backward: i32,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    S_OK
}

unsafe extern "system" fn range_find_text(
    this: *mut Range,
    text: *mut u16,
    backward: i32,
    ignore_case: i32,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if text.is_null() {
        return E_INVALIDARG;
    }
    let Some(state) = text_state() else { return E_FAIL };
    // A BSTR carries its length in the four bytes before it.
    let length = *text.cast::<u32>().sub(1) as usize / 2;
    let wanted = String::from_utf16_lossy(core::slice::from_raw_parts(text, length));
    let range = &*this;
    let chars: Vec<char> = state.text.chars().collect();
    let (start, end) = (range.start.get().min(chars.len()), range.end.get().min(chars.len()));
    let within: String = chars[start..end].iter().collect();
    let (haystack, needle) = if ignore_case != 0 {
        (within.to_lowercase(), wanted.to_lowercase())
    } else {
        (within.clone(), wanted.clone())
    };
    let found = if backward != 0 { haystack.rfind(&needle) } else { haystack.find(&needle) };
    if let Some(byte) = found {
        let at = start + haystack[..byte].chars().count();
        *out = Range::new(range.window, range.document, at, at + wanted.chars().count()).cast();
    }
    S_OK
}

unsafe extern "system" fn range_get_attribute_value(
    _this: *mut Range,
    _attribute: i32,
    out: *mut Variant,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    let mut not_supported: *mut c_void = core::ptr::null_mut();
    UiaGetReservedNotSupportedValue(&mut not_supported);
    *out = Variant::unknown(not_supported);
    S_OK
}

unsafe extern "system" fn range_get_bounding_rectangles(
    this: *mut Range,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    let range = &*this;
    let (start, end) = (range.start.get(), range.end.get());
    let rects = with_application(|app| app.accessible_rects(start, end)).unwrap_or_default();
    let mut values = Vec::with_capacity(rects.len() * 4);
    for rect in rects {
        let on_screen = screen_rect(range.window, rect);
        values.extend_from_slice(&[
            on_screen.left,
            on_screen.top,
            on_screen.width,
            on_screen.height,
        ]);
    }
    *out = array_of_doubles(&values);
    S_OK
}

unsafe extern "system" fn range_get_enclosing_element(
    this: *mut Range,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = Item::new((*this).window, (*this).document).cast();
    S_OK
}

unsafe extern "system" fn range_get_text(
    this: *mut Range,
    most: i32,
    out: *mut *mut u16,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    let Some(state) = text_state() else { return E_FAIL };
    let range = &*this;
    let taken: String = state
        .text
        .chars()
        .skip(range.start.get())
        .take(range.end.get().saturating_sub(range.start.get()))
        .take(if most < 0 { usize::MAX } else { most as usize })
        .collect();
    let units: Vec<u16> = taken.encode_utf16().collect();
    *out = SysAllocStringLen(units.as_ptr(), units.len() as u32);
    S_OK
}

/// Moves an offset by whole units, and says how many it moved.
fn step(starts: &[usize], offset: usize, count: i32) -> (usize, i32) {
    let mut index = starts.iter().rposition(|start| *start <= offset).unwrap_or(0);
    let last = starts.len().saturating_sub(1);
    let mut moved = 0;
    if count > 0 {
        while moved < count && index < last {
            index += 1;
            moved += 1;
        }
    } else {
        // Starting inside a unit, the first step back goes to its start.
        if starts[index] != offset && index < last {
            index += 1;
        }
        while moved > count && index > 0 {
            index -= 1;
            moved -= 1;
        }
    }
    (starts[index], moved)
}

unsafe extern "system" fn range_move(
    this: *mut Range,
    unit: i32,
    count: i32,
    out: *mut i32,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = 0;
    let Some(state) = text_state() else { return E_FAIL };
    let range = &*this;
    let unit = if matches!(unit, UNIT_DOCUMENT | UNIT_PAGE) { UNIT_DOCUMENT } else { unit };
    if unit == UNIT_DOCUMENT {
        range.start.set(0);
        range.end.set(state.text.chars().count());
        return S_OK;
    }
    let starts = unit_starts(&state.text, unit);
    let (start, _) = unit_round(&starts, range.start.get());
    let (landed, moved) = step(&starts, start, count);
    let (unit_start, unit_end) = unit_round(&starts, landed);
    range.start.set(unit_start);
    range.end.set(unit_end);
    *out = moved;
    S_OK
}

unsafe extern "system" fn range_move_endpoint_by_unit(
    this: *mut Range,
    endpoint: i32,
    unit: i32,
    count: i32,
    out: *mut i32,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = 0;
    let Some(state) = text_state() else { return E_FAIL };
    let range = &*this;
    let total = state.text.chars().count();
    let (landed, moved) = if matches!(unit, UNIT_DOCUMENT | UNIT_PAGE) {
        if count > 0 {
            (total, 1)
        } else {
            (0, -1)
        }
    } else {
        let starts = unit_starts(&state.text, unit);
        let from = if endpoint == ENDPOINT_START { range.start.get() } else { range.end.get() };
        step(&starts, from, count)
    };
    if endpoint == ENDPOINT_START {
        range.start.set(landed);
        if range.end.get() < landed {
            range.end.set(landed);
        }
    } else {
        range.end.set(landed);
        if range.start.get() > landed {
            range.start.set(landed);
        }
    }
    *out = moved;
    S_OK
}

unsafe extern "system" fn range_move_endpoint_by_range(
    this: *mut Range,
    endpoint: i32,
    other: *mut Range,
    other_endpoint: i32,
) -> HResult {
    if !ours(other) {
        return E_INVALIDARG;
    }
    let theirs =
        if other_endpoint == ENDPOINT_START { (*other).start.get() } else { (*other).end.get() };
    let range = &*this;
    if endpoint == ENDPOINT_START {
        range.start.set(theirs);
        if range.end.get() < theirs {
            range.end.set(theirs);
        }
    } else {
        range.end.set(theirs);
        if range.start.get() > theirs {
            range.start.set(theirs);
        }
    }
    S_OK
}

unsafe extern "system" fn range_select(this: *mut Range) -> HResult {
    let range = &*this;
    let (start, end) = (range.start.get(), range.end.get());
    let Some(response) = with_application(|app| app.accessible_select(start, end)) else {
        return E_FAIL;
    };
    redraw(range.window, response);
    S_OK
}

unsafe extern "system" fn range_remove_from_selection(_this: *mut Range) -> HResult {
    S_FALSE
}

unsafe extern "system" fn range_scroll_into_view(_this: *mut Range, _align_top: i32) -> HResult {
    S_OK
}

unsafe extern "system" fn range_get_children(_this: *mut Range, out: *mut *mut c_void) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = array_of_unknowns(&[]);
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_begin_where_words_and_paragraphs_do() {
        let text = "One two\nThree";
        assert_eq!(unit_starts(text, UNIT_WORD), vec![0, 4, 7, 8, 13]);
        assert_eq!(unit_starts(text, UNIT_PARAGRAPH), vec![0, 8, 13]);
        assert_eq!(unit_starts(text, UNIT_CHARACTER).len(), 14);
        assert_eq!(unit_round(&unit_starts(text, UNIT_WORD), 5), (4, 7));
    }

    #[test]
    fn a_range_steps_by_units_and_says_how_far_it_got() {
        let starts = unit_starts("One two three", UNIT_WORD);
        assert_eq!(step(&starts, 0, 2), (8, 2));
        assert_eq!(step(&starts, 8, 5), (13, 2));
        assert_eq!(step(&starts, 8, -1), (4, -1));
        assert_eq!(step(&starts, 5, -1), (4, -1));
        assert_eq!(step(&starts, 0, -1), (0, 0));
    }
}

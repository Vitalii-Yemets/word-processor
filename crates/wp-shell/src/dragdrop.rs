//! Dragging and dropping between this program and the others.
//!
//! Windows does this through OLE: a window that takes drops registers a
//! drop target, and a program that gives drags hands the system a data
//! object and a drop source and waits while the drag runs. All three are
//! COM objects — a table of functions with the object's own state behind
//! it — and are written out here as such, against the documented layout,
//! since no binding library is used anywhere in the project.
//!
//! What crosses is what the clipboard carries: text, Rich Text, HTML, a
//! picture, and files from the desktop. See [`crate::clipboard`].

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::path::PathBuf;

use crate::clipboard::Contents;
use crate::com::{
    Guid, HResult, E_NOINTERFACE, E_NOTIMPL, E_OUTOFMEMORY, E_POINTER, S_FALSE, S_OK,
};
use crate::windows::{
    deliver, wide, GlobalAlloc, GlobalFree, GlobalLock, GlobalSize, GlobalUnlock, Handle, Point,
    RegisterClipboardFormatW, ScreenToClient, MEMORY_MOVEABLE,
};
use crate::{DragEffect, Event};

const DV_E_FORMATETC: HResult = 0x8004_0064_u32 as i32;
const OLE_E_ADVISENOTSUPPORTED: HResult = 0x8004_0003_u32 as i32;
const DATA_S_SAMEFORMATETC: HResult = 0x0004_0130;
const DRAGDROP_S_DROP: HResult = 0x0004_0100;
const DRAGDROP_S_CANCEL: HResult = 0x0004_0101;
const DRAGDROP_S_USEDEFAULTCURSORS: HResult = 0x0004_0102;

const DROPEFFECT_NONE: u32 = 0;
const DROPEFFECT_COPY: u32 = 1;
const DROPEFFECT_MOVE: u32 = 2;

const MK_LBUTTON: u32 = 0x0001;
const MK_CONTROL: u32 = 0x0008;

const CF_DIB: u16 = 8;
const CF_UNICODETEXT: u16 = 13;
const CF_HDROP: u16 = 15;
const TYMED_HGLOBAL: u32 = 1;
const DVASPECT_CONTENT: u32 = 1;
const DATADIR_GET: u32 = 1;

use crate::com::IID_IUNKNOWN;

const IID_IENUMFORMATETC: Guid = Guid::standard(0x0000_0103);
const IID_IDATAOBJECT: Guid = Guid::standard(0x0000_010E);
const IID_IDROPSOURCE: Guid = Guid::standard(0x0000_0121);
const IID_IDROPTARGET: Guid = Guid::standard(0x0000_0122);
/// An identifier of this program's own, which only its own data object
/// answers to: how a drop is known to have come from this window.
const IID_OWN_DATA: Guid = Guid {
    data1: 0x7B2A_5C41,
    data2: 0x3D0E,
    data3: 0x4F8A,
    data4: [0x9C, 0x11, 0x57, 0x0D, 0x2E, 0x6A, 0x91, 0x3F],
};

/// Which format, in what medium.
#[repr(C)]
#[derive(Clone, Copy)]
struct FormatEtc {
    format: u16,
    target_device: *mut c_void,
    aspect: u32,
    index: i32,
    tymed: u32,
}

impl FormatEtc {
    const fn global(format: u16) -> Self {
        Self {
            format,
            target_device: core::ptr::null_mut(),
            aspect: DVASPECT_CONTENT,
            index: -1,
            tymed: TYMED_HGLOBAL,
        }
    }
}

/// The data, in its medium: here always a global memory handle.
#[repr(C)]
struct StgMedium {
    tymed: u32,
    handle: Handle,
    release: *mut c_void,
}

/// A point on the screen, as the drag messages give it.
#[repr(C)]
#[derive(Clone, Copy)]
struct Pointl {
    x: i32,
    y: i32,
}

#[link(name = "ole32")]
extern "system" {
    fn OleInitialize(reserved: *mut c_void) -> HResult;
    fn RegisterDragDrop(window: Handle, target: *mut c_void) -> HResult;
    fn RevokeDragDrop(window: Handle) -> HResult;
    fn DoDragDrop(
        data: *mut c_void,
        source: *mut c_void,
        allowed: u32,
        effect: *mut u32,
    ) -> HResult;
    fn ReleaseStgMedium(medium: *mut StgMedium);
}

#[link(name = "shell32")]
extern "system" {
    fn DragQueryFileW(drop: Handle, index: u32, file: *mut u16, length: u32) -> u32;
}

thread_local! {
    /// Whether OLE has been set up on this thread.
    static INITIALISED: Cell<bool> = const { Cell::new(false) };
    /// The data object of the drag this window is giving, while it is.
    static SOURCE: Cell<*mut DataObject> = const { Cell::new(core::ptr::null_mut()) };
    /// Where the drag this window gave landed back in this window, if it did.
    static DROPPED_ON_SELF: Cell<Option<(i32, i32, bool)>> = const { Cell::new(None) };
    /// The registered formats, looked up once.
    static FORMATS: RefCell<Option<Registered>> = const { RefCell::new(None) };
}

/// The clipboard formats that are registered by name rather than numbered.
#[derive(Clone, Copy)]
struct Registered {
    html: u16,
    rtf: u16,
    png: u16,
}

fn registered() -> Registered {
    FORMATS.with(|slot| {
        *slot.borrow_mut().get_or_insert_with(|| {
            // SAFETY: the names outlive the calls, which copy them.
            let register = |name: &str| unsafe { RegisterClipboardFormatW(wide(name).as_ptr()) };
            Registered {
                html: register("HTML Format") as u16,
                rtf: register("Rich Text Format") as u16,
                png: register("PNG") as u16,
            }
        })
    })
}

/// Makes a window take drops. Called once per window, after it is made.
pub(crate) fn register_window(window: Handle) {
    // SAFETY: OLE is set up once on the thread that owns the windows, and
    // the target is leaked so that it outlives the window it is registered
    // for; the system keeps its own reference beside ours.
    unsafe {
        if !INITIALISED.with(Cell::get) {
            if OleInitialize(core::ptr::null_mut()) < 0 {
                return;
            }
            INITIALISED.with(|slot| slot.set(true));
        }
        let target = Box::into_raw(Box::new(DropTarget {
            vtbl: &DROP_TARGET_VTBL,
            refs: Cell::new(1),
            window,
            effect: Cell::new(DROPEFFECT_NONE),
        }));
        RegisterDragDrop(window, target.cast());
    }
}

/// Stops a window taking drops, as it closes.
pub(crate) fn unregister_window(window: Handle) {
    // SAFETY: the window is one this thread registered.
    unsafe {
        RevokeDragDrop(window);
    }
}

/// Gives a drag of the contents, and waits for it to end.
pub(crate) fn start_drag(contents: &Contents) -> DragEffect {
    let formats = registered();
    let mut held: Vec<(u16, Vec<u8>)> = Vec::new();
    if let Some(text) = &contents.text {
        let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
        for unit in text.encode_utf16().chain(std::iter::once(0)) {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        held.push((CF_UNICODETEXT, bytes));
    }
    for (format, bytes) in [(formats.rtf, &contents.rtf), (formats.html, &contents.html)] {
        if let (true, Some(bytes)) = (format != 0, bytes) {
            let mut bytes = bytes.clone();
            bytes.push(0);
            held.push((format, bytes));
        }
    }
    if let (true, Some(png)) = (formats.png != 0, &contents.png) {
        held.push((formats.png, png.clone()));
    }
    if let Some(dib) = &contents.dib {
        held.push((CF_DIB, dib.clone()));
    }
    if held.is_empty() {
        return DragEffect::None;
    }
    // SAFETY: both objects are made here with one reference each, which is
    // given up once the drag is over; the system takes references of its own
    // while it runs and gives them back before DoDragDrop returns.
    unsafe {
        if !INITIALISED.with(Cell::get) && OleInitialize(core::ptr::null_mut()) >= 0 {
            INITIALISED.with(|slot| slot.set(true));
        }
        let data = Box::into_raw(Box::new(DataObject {
            vtbl: &DATA_OBJECT_VTBL,
            refs: Cell::new(1),
            formats: held,
        }));
        let source =
            Box::into_raw(Box::new(DropSource { vtbl: &DROP_SOURCE_VTBL, refs: Cell::new(1) }));
        SOURCE.with(|slot| slot.set(data));
        DROPPED_ON_SELF.with(|slot| slot.set(None));
        let mut effect = DROPEFFECT_NONE;
        let result =
            DoDragDrop(data.cast(), source.cast(), DROPEFFECT_COPY | DROPEFFECT_MOVE, &mut effect);
        SOURCE.with(|slot| slot.set(core::ptr::null_mut()));
        release_data(data);
        release_source(source);
        if let Some((x, y, copying)) = DROPPED_ON_SELF.with(Cell::take) {
            return DragEffect::DroppedOnSelf { x, y, copying };
        }
        if result != DRAGDROP_S_DROP {
            return DragEffect::None;
        }
        match effect {
            DROPEFFECT_MOVE => DragEffect::Move,
            DROPEFFECT_COPY => DragEffect::Copy,
            _ => DragEffect::None,
        }
    }
}

// --- The drop target ----------------------------------------------------------

/// The functions of `IDropTarget`, in the order the interface lays them out.
#[repr(C)]
struct DropTargetVtbl {
    query_interface:
        unsafe extern "system" fn(*mut DropTarget, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut DropTarget) -> u32,
    release: unsafe extern "system" fn(*mut DropTarget) -> u32,
    drag_enter: unsafe extern "system" fn(
        *mut DropTarget,
        *mut ComObject,
        u32,
        Pointl,
        *mut u32,
    ) -> HResult,
    drag_over: unsafe extern "system" fn(*mut DropTarget, u32, Pointl, *mut u32) -> HResult,
    drag_leave: unsafe extern "system" fn(*mut DropTarget) -> HResult,
    drop: unsafe extern "system" fn(
        *mut DropTarget,
        *mut ComObject,
        u32,
        Pointl,
        *mut u32,
    ) -> HResult,
}

#[repr(C)]
struct DropTarget {
    vtbl: *const DropTargetVtbl,
    refs: Cell<u32>,
    window: Handle,
    /// What the drag over the window would do if let go: worked out on
    /// entry, kept for the moves.
    effect: Cell<u32>,
}

static DROP_TARGET_VTBL: DropTargetVtbl = DropTargetVtbl {
    query_interface: target_query_interface,
    add_ref: target_add_ref,
    release: target_release,
    drag_enter: target_drag_enter,
    drag_over: target_drag_over,
    drag_leave: target_drag_leave,
    drop: target_drop,
};

unsafe extern "system" fn target_query_interface(
    this: *mut DropTarget,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    if *iid == IID_IUNKNOWN || *iid == IID_IDROPTARGET {
        target_add_ref(this);
        *out = this.cast();
        S_OK
    } else {
        *out = core::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn target_add_ref(this: *mut DropTarget) -> u32 {
    let refs = (*this).refs.get() + 1;
    (*this).refs.set(refs);
    refs
}

unsafe extern "system" fn target_release(this: *mut DropTarget) -> u32 {
    let refs = (*this).refs.get().saturating_sub(1);
    (*this).refs.set(refs);
    if refs == 0 {
        drop(Box::from_raw(this));
    }
    refs
}

/// What a drag would do here: a copy with Control held, else a move when
/// the source allows one, else a copy; nothing when the data is nothing
/// this program takes.
fn effect_for(keys: u32, allowed: u32, takes: bool) -> u32 {
    if !takes {
        return DROPEFFECT_NONE;
    }
    if keys & MK_CONTROL != 0 && allowed & DROPEFFECT_COPY != 0 {
        DROPEFFECT_COPY
    } else if allowed & DROPEFFECT_MOVE != 0 {
        DROPEFFECT_MOVE
    } else if allowed & DROPEFFECT_COPY != 0 {
        DROPEFFECT_COPY
    } else {
        DROPEFFECT_NONE
    }
}

unsafe fn client_point(window: Handle, point: Pointl) -> (i32, i32) {
    let mut point = Point { x: point.x, y: point.y };
    ScreenToClient(window, &mut point);
    crate::windows::to_logical(window, (point.x, point.y))
}

unsafe extern "system" fn target_drag_enter(
    this: *mut DropTarget,
    data: *mut ComObject,
    keys: u32,
    point: Pointl,
    effect: *mut u32,
) -> HResult {
    if effect.is_null() {
        return E_POINTER;
    }
    let takes = !data.is_null() && (is_own(data) || has_something(data));
    let chosen = effect_for(keys, *effect, takes);
    (*this).effect.set(chosen);
    *effect = chosen;
    if chosen != DROPEFFECT_NONE && !is_own(data) {
        let (x, y) = client_point((*this).window, point);
        deliver((*this).window, Event::DataDragOver { x, y });
    }
    S_OK
}

unsafe extern "system" fn target_drag_over(
    this: *mut DropTarget,
    keys: u32,
    point: Pointl,
    effect: *mut u32,
) -> HResult {
    if effect.is_null() {
        return E_POINTER;
    }
    let takes = (*this).effect.get() != DROPEFFECT_NONE;
    let chosen = effect_for(keys, *effect, takes);
    *effect = chosen;
    if chosen != DROPEFFECT_NONE && SOURCE.with(Cell::get).is_null() {
        let (x, y) = client_point((*this).window, point);
        deliver((*this).window, Event::DataDragOver { x, y });
    }
    S_OK
}

unsafe extern "system" fn target_drag_leave(this: *mut DropTarget) -> HResult {
    if (*this).effect.replace(DROPEFFECT_NONE) != DROPEFFECT_NONE
        && SOURCE.with(Cell::get).is_null()
    {
        deliver((*this).window, Event::DataDragLeft);
    }
    S_OK
}

unsafe extern "system" fn target_drop(
    this: *mut DropTarget,
    data: *mut ComObject,
    keys: u32,
    point: Pointl,
    effect: *mut u32,
) -> HResult {
    if effect.is_null() {
        return E_POINTER;
    }
    let takes = !data.is_null() && (is_own(data) || has_something(data));
    let chosen = effect_for(keys, *effect, takes);
    (*this).effect.set(DROPEFFECT_NONE);
    *effect = chosen;
    if chosen == DROPEFFECT_NONE {
        return S_OK;
    }
    let (x, y) = client_point((*this).window, point);
    let copying = chosen == DROPEFFECT_COPY;
    // A drag this window gave, landing back in it: the application is busy
    // giving the drag, so it is told afterwards rather than now.
    if is_own(data) {
        DROPPED_ON_SELF.with(|slot| slot.set(Some((x, y, copying))));
        return S_OK;
    }
    let (contents, files) = read_data(data);
    if !files.is_empty() {
        deliver((*this).window, Event::FilesDropped { paths: files, x, y });
    } else if !contents.is_empty() {
        deliver((*this).window, Event::DataDropped { contents, x, y, copying });
    } else {
        *effect = DROPEFFECT_NONE;
    }
    S_OK
}

// --- Reading another program's data object -----------------------------------

/// The functions of `IDataObject`, in order, as far as they are called.
#[repr(C)]
struct DataObjectVtbl {
    query_interface:
        unsafe extern "system" fn(*mut ComObject, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut ComObject) -> u32,
    release: unsafe extern "system" fn(*mut ComObject) -> u32,
    get_data:
        unsafe extern "system" fn(*mut ComObject, *const FormatEtc, *mut StgMedium) -> HResult,
    get_data_here:
        unsafe extern "system" fn(*mut ComObject, *const FormatEtc, *mut StgMedium) -> HResult,
    query_get_data: unsafe extern "system" fn(*mut ComObject, *const FormatEtc) -> HResult,
    get_canonical_format_etc:
        unsafe extern "system" fn(*mut ComObject, *const FormatEtc, *mut FormatEtc) -> HResult,
    set_data:
        unsafe extern "system" fn(*mut ComObject, *const FormatEtc, *mut StgMedium, i32) -> HResult,
    enum_format_etc: unsafe extern "system" fn(*mut ComObject, u32, *mut *mut ComObject) -> HResult,
    d_advise: unsafe extern "system" fn(
        *mut ComObject,
        *const FormatEtc,
        u32,
        *mut c_void,
        *mut u32,
    ) -> HResult,
    d_unadvise: unsafe extern "system" fn(*mut ComObject, u32) -> HResult,
    enum_d_advise: unsafe extern "system" fn(*mut ComObject, *mut *mut c_void) -> HResult,
}

/// Any COM object, seen through the first field every one begins with.
#[repr(C)]
struct ComObject {
    vtbl: *const DataObjectVtbl,
}

/// Whether the data object is this window's own drag.
unsafe fn is_own(data: *mut ComObject) -> bool {
    if data.is_null() {
        return false;
    }
    if data.cast::<DataObject>() == SOURCE.with(Cell::get) {
        return true;
    }
    let mut found: *mut c_void = core::ptr::null_mut();
    if ((*(*data).vtbl).query_interface)(data, &IID_OWN_DATA, &mut found) == S_OK
        && !found.is_null()
    {
        ((*(*data).vtbl).release)(found.cast());
        return true;
    }
    false
}

unsafe fn has_format(data: *mut ComObject, format: u16) -> bool {
    format != 0 && ((*(*data).vtbl).query_get_data)(data, &FormatEtc::global(format)) == S_OK
}

/// Whether the data object holds anything this program takes.
unsafe fn has_something(data: *mut ComObject) -> bool {
    let formats = registered();
    [CF_HDROP, CF_UNICODETEXT, formats.rtf, formats.html, formats.png, CF_DIB]
        .into_iter()
        .any(|format| has_format(data, format))
}

/// One format's bytes, copied out of the object's global memory.
unsafe fn bytes_of(data: *mut ComObject, format: u16) -> Option<Vec<u8>> {
    if !has_format(data, format) {
        return None;
    }
    let mut medium =
        StgMedium { tymed: 0, handle: core::ptr::null_mut(), release: core::ptr::null_mut() };
    if ((*(*data).vtbl).get_data)(data, &FormatEtc::global(format), &mut medium) != S_OK {
        return None;
    }
    let mut out = None;
    if medium.tymed == TYMED_HGLOBAL && !medium.handle.is_null() {
        let source = GlobalLock(medium.handle).cast::<u8>();
        if !source.is_null() {
            let size = GlobalSize(medium.handle);
            out = Some(core::slice::from_raw_parts(source, size).to_vec());
            GlobalUnlock(medium.handle);
        }
    }
    ReleaseStgMedium(&mut medium);
    out
}

/// The files a drop names, from the drop handle the desktop gives.
unsafe fn files_of(data: *mut ComObject) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !has_format(data, CF_HDROP) {
        return files;
    }
    let mut medium =
        StgMedium { tymed: 0, handle: core::ptr::null_mut(), release: core::ptr::null_mut() };
    if ((*(*data).vtbl).get_data)(data, &FormatEtc::global(CF_HDROP), &mut medium) != S_OK {
        return files;
    }
    if medium.tymed == TYMED_HGLOBAL && !medium.handle.is_null() {
        files = files_in_drop(medium.handle);
    }
    ReleaseStgMedium(&mut medium);
    files
}

/// The files named by a drop handle, whether it came through OLE or
/// through the older message.
pub(crate) unsafe fn files_in_drop(drop: Handle) -> Vec<PathBuf> {
    let count = DragQueryFileW(drop, u32::MAX, core::ptr::null_mut(), 0);
    let mut files = Vec::with_capacity(count as usize);
    for index in 0..count {
        let length = DragQueryFileW(drop, index, core::ptr::null_mut(), 0);
        if length == 0 {
            continue;
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let got = DragQueryFileW(drop, index, buffer.as_mut_ptr(), buffer.len() as u32);
        buffer.truncate(got as usize);
        files.push(PathBuf::from(String::from_utf16_lossy(&buffer)));
    }
    files
}

/// Everything a data object holds that this program takes.
unsafe fn read_data(data: *mut ComObject) -> (Contents, Vec<PathBuf>) {
    let files = files_of(data);
    let formats = registered();
    let cut = |bytes: Vec<u8>| {
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        let mut bytes = bytes;
        bytes.truncate(end);
        bytes
    };
    let text = bytes_of(data, CF_UNICODETEXT).map(|bytes| {
        let units: Vec<u16> =
            bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
        String::from_utf16_lossy(&units[..end])
    });
    let contents = Contents {
        text,
        html: bytes_of(data, formats.html).map(cut),
        rtf: bytes_of(data, formats.rtf).map(cut),
        png: bytes_of(data, formats.png),
        dib: bytes_of(data, CF_DIB),
        // Word's own object comes in a drag as a storage, which this does
        // not ask for: what is dropped is taken from the other formats.
        document: None,
    };
    (contents, files)
}

// --- This program's data object ------------------------------------------------

#[repr(C)]
struct DataObject {
    vtbl: *const DataObjectVtbl,
    refs: Cell<u32>,
    /// Each format with its bytes, as they go into global memory.
    formats: Vec<(u16, Vec<u8>)>,
}

static DATA_OBJECT_VTBL: DataObjectVtbl = DataObjectVtbl {
    query_interface: data_query_interface,
    add_ref: data_add_ref,
    release: data_release,
    get_data: data_get_data,
    get_data_here: data_get_data_here,
    query_get_data: data_query_get_data,
    get_canonical_format_etc: data_get_canonical_format_etc,
    set_data: data_set_data,
    enum_format_etc: data_enum_format_etc,
    d_advise: data_d_advise,
    d_unadvise: data_d_unadvise,
    enum_d_advise: data_enum_d_advise,
};

unsafe fn release_data(data: *mut DataObject) {
    data_release(data.cast());
}

unsafe extern "system" fn data_query_interface(
    this: *mut ComObject,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    if *iid == IID_IUNKNOWN || *iid == IID_IDATAOBJECT || *iid == IID_OWN_DATA {
        data_add_ref(this);
        *out = this.cast();
        S_OK
    } else {
        *out = core::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn data_add_ref(this: *mut ComObject) -> u32 {
    let this = this.cast::<DataObject>();
    let refs = (*this).refs.get() + 1;
    (*this).refs.set(refs);
    refs
}

unsafe extern "system" fn data_release(this: *mut ComObject) -> u32 {
    let this = this.cast::<DataObject>();
    let refs = (*this).refs.get().saturating_sub(1);
    (*this).refs.set(refs);
    if refs == 0 {
        drop(Box::from_raw(this));
    }
    refs
}

/// Whether a format asked for is one held, in a medium given.
unsafe fn holds(this: *mut DataObject, wanted: *const FormatEtc) -> Option<usize> {
    if wanted.is_null() {
        return None;
    }
    let wanted = *wanted;
    if wanted.tymed & TYMED_HGLOBAL == 0 || wanted.aspect & DVASPECT_CONTENT == 0 {
        return None;
    }
    (*this).formats.iter().position(|(format, _)| *format == wanted.format)
}

unsafe extern "system" fn data_get_data(
    this: *mut ComObject,
    wanted: *const FormatEtc,
    medium: *mut StgMedium,
) -> HResult {
    let this = this.cast::<DataObject>();
    if medium.is_null() {
        return E_POINTER;
    }
    let Some(index) = holds(this, wanted) else { return DV_E_FORMATETC };
    let object = &*this;
    let bytes = &object.formats[index].1;
    let memory = GlobalAlloc(MEMORY_MOVEABLE, bytes.len().max(1));
    if memory.is_null() {
        return E_OUTOFMEMORY;
    }
    let destination = GlobalLock(memory).cast::<u8>();
    if destination.is_null() {
        GlobalFree(memory);
        return E_OUTOFMEMORY;
    }
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
    GlobalUnlock(memory);
    *medium = StgMedium { tymed: TYMED_HGLOBAL, handle: memory, release: core::ptr::null_mut() };
    S_OK
}

unsafe extern "system" fn data_get_data_here(
    _this: *mut ComObject,
    _wanted: *const FormatEtc,
    _medium: *mut StgMedium,
) -> HResult {
    E_NOTIMPL
}

unsafe extern "system" fn data_query_get_data(
    this: *mut ComObject,
    wanted: *const FormatEtc,
) -> HResult {
    if holds(this.cast(), wanted).is_some() {
        S_OK
    } else {
        DV_E_FORMATETC
    }
}

unsafe extern "system" fn data_get_canonical_format_etc(
    _this: *mut ComObject,
    given: *const FormatEtc,
    out: *mut FormatEtc,
) -> HResult {
    if given.is_null() || out.is_null() {
        return E_POINTER;
    }
    *out = *given;
    (*out).target_device = core::ptr::null_mut();
    DATA_S_SAMEFORMATETC
}

unsafe extern "system" fn data_set_data(
    _this: *mut ComObject,
    _format: *const FormatEtc,
    _medium: *mut StgMedium,
    _release: i32,
) -> HResult {
    E_NOTIMPL
}

unsafe extern "system" fn data_enum_format_etc(
    this: *mut ComObject,
    direction: u32,
    out: *mut *mut ComObject,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if direction != DATADIR_GET {
        return E_NOTIMPL;
    }
    let this = this.cast::<DataObject>();
    let formats = (*this).formats.iter().map(|(format, _)| FormatEtc::global(*format)).collect();
    let enumerator = Box::into_raw(Box::new(EnumFormatEtc {
        vtbl: &ENUM_VTBL,
        refs: Cell::new(1),
        formats,
        index: Cell::new(0),
    }));
    *out = enumerator.cast();
    S_OK
}

unsafe extern "system" fn data_d_advise(
    _this: *mut ComObject,
    _format: *const FormatEtc,
    _flags: u32,
    _sink: *mut c_void,
    _connection: *mut u32,
) -> HResult {
    OLE_E_ADVISENOTSUPPORTED
}

unsafe extern "system" fn data_d_unadvise(_this: *mut ComObject, _connection: u32) -> HResult {
    OLE_E_ADVISENOTSUPPORTED
}

unsafe extern "system" fn data_enum_d_advise(
    _this: *mut ComObject,
    out: *mut *mut c_void,
) -> HResult {
    if !out.is_null() {
        *out = core::ptr::null_mut();
    }
    OLE_E_ADVISENOTSUPPORTED
}

// --- The list of formats, as `IEnumFORMATETC` ----------------------------------

#[repr(C)]
struct EnumVtbl {
    query_interface:
        unsafe extern "system" fn(*mut EnumFormatEtc, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut EnumFormatEtc) -> u32,
    release: unsafe extern "system" fn(*mut EnumFormatEtc) -> u32,
    next: unsafe extern "system" fn(*mut EnumFormatEtc, u32, *mut FormatEtc, *mut u32) -> HResult,
    skip: unsafe extern "system" fn(*mut EnumFormatEtc, u32) -> HResult,
    reset: unsafe extern "system" fn(*mut EnumFormatEtc) -> HResult,
    clone: unsafe extern "system" fn(*mut EnumFormatEtc, *mut *mut EnumFormatEtc) -> HResult,
}

#[repr(C)]
struct EnumFormatEtc {
    vtbl: *const EnumVtbl,
    refs: Cell<u32>,
    formats: Vec<FormatEtc>,
    index: Cell<usize>,
}

static ENUM_VTBL: EnumVtbl = EnumVtbl {
    query_interface: enum_query_interface,
    add_ref: enum_add_ref,
    release: enum_release,
    next: enum_next,
    skip: enum_skip,
    reset: enum_reset,
    clone: enum_clone,
};

unsafe extern "system" fn enum_query_interface(
    this: *mut EnumFormatEtc,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    if *iid == IID_IUNKNOWN || *iid == IID_IENUMFORMATETC {
        enum_add_ref(this);
        *out = this.cast();
        S_OK
    } else {
        *out = core::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn enum_add_ref(this: *mut EnumFormatEtc) -> u32 {
    let refs = (*this).refs.get() + 1;
    (*this).refs.set(refs);
    refs
}

unsafe extern "system" fn enum_release(this: *mut EnumFormatEtc) -> u32 {
    let refs = (*this).refs.get().saturating_sub(1);
    (*this).refs.set(refs);
    if refs == 0 {
        drop(Box::from_raw(this));
    }
    refs
}

unsafe extern "system" fn enum_next(
    this: *mut EnumFormatEtc,
    wanted: u32,
    out: *mut FormatEtc,
    fetched: *mut u32,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    let enumerator = &*this;
    let mut given = 0u32;
    while given < wanted {
        let index = enumerator.index.get();
        let Some(format) = enumerator.formats.get(index) else { break };
        *out.add(given as usize) = *format;
        enumerator.index.set(index + 1);
        given += 1;
    }
    if !fetched.is_null() {
        *fetched = given;
    }
    if given == wanted {
        S_OK
    } else {
        S_FALSE
    }
}

unsafe extern "system" fn enum_skip(this: *mut EnumFormatEtc, count: u32) -> HResult {
    let index = (*this).index.get() + count as usize;
    if index <= (*this).formats.len() {
        (*this).index.set(index);
        S_OK
    } else {
        (*this).index.set((*this).formats.len());
        S_FALSE
    }
}

unsafe extern "system" fn enum_reset(this: *mut EnumFormatEtc) -> HResult {
    (*this).index.set(0);
    S_OK
}

unsafe extern "system" fn enum_clone(
    this: *mut EnumFormatEtc,
    out: *mut *mut EnumFormatEtc,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = Box::into_raw(Box::new(EnumFormatEtc {
        vtbl: &ENUM_VTBL,
        refs: Cell::new(1),
        formats: (*this).formats.clone(),
        index: Cell::new((*this).index.get()),
    }));
    S_OK
}

// --- The drop source -------------------------------------------------------------

#[repr(C)]
struct DropSourceVtbl {
    query_interface:
        unsafe extern "system" fn(*mut DropSource, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut DropSource) -> u32,
    release: unsafe extern "system" fn(*mut DropSource) -> u32,
    query_continue_drag: unsafe extern "system" fn(*mut DropSource, i32, u32) -> HResult,
    give_feedback: unsafe extern "system" fn(*mut DropSource, u32) -> HResult,
}

#[repr(C)]
struct DropSource {
    vtbl: *const DropSourceVtbl,
    refs: Cell<u32>,
}

static DROP_SOURCE_VTBL: DropSourceVtbl = DropSourceVtbl {
    query_interface: source_query_interface,
    add_ref: source_add_ref,
    release: source_release,
    query_continue_drag: source_query_continue_drag,
    give_feedback: source_give_feedback,
};

unsafe fn release_source(source: *mut DropSource) {
    source_release(source);
}

unsafe extern "system" fn source_query_interface(
    this: *mut DropSource,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    if *iid == IID_IUNKNOWN || *iid == IID_IDROPSOURCE {
        source_add_ref(this);
        *out = this.cast();
        S_OK
    } else {
        *out = core::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn source_add_ref(this: *mut DropSource) -> u32 {
    let refs = (*this).refs.get() + 1;
    (*this).refs.set(refs);
    refs
}

unsafe extern "system" fn source_release(this: *mut DropSource) -> u32 {
    let refs = (*this).refs.get().saturating_sub(1);
    (*this).refs.set(refs);
    if refs == 0 {
        drop(Box::from_raw(this));
    }
    refs
}

/// Escape gives the drag up; letting the button go drops; anything else
/// carries on.
unsafe extern "system" fn source_query_continue_drag(
    _this: *mut DropSource,
    escape: i32,
    keys: u32,
) -> HResult {
    if escape != 0 {
        DRAGDROP_S_CANCEL
    } else if keys & MK_LBUTTON == 0 {
        DRAGDROP_S_DROP
    } else {
        S_OK
    }
}

unsafe extern "system" fn source_give_feedback(_this: *mut DropSource, _effect: u32) -> HResult {
    DRAGDROP_S_USEDEFAULTCURSORS
}

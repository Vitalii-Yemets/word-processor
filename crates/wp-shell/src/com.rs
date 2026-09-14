//! The little of COM that the Windows shell speaks.
//!
//! A COM object is a pointer to a table of functions, with the object's
//! own state after it; an interface is one such table's layout, named by a
//! GUID; every table begins with the three functions of `IUnknown`. That
//! is all that is needed to be given objects by the system and to give it
//! objects of this program's own — for dragging and dropping, and for
//! telling a screen reader what is on the window — and it is written out
//! here rather than taken from a binding library, as everything in this
//! project is.

use std::cell::Cell;
use std::ffi::c_void;

pub(crate) type HResult = i32;

pub(crate) const S_OK: HResult = 0;
pub(crate) const S_FALSE: HResult = 1;
pub(crate) const E_NOTIMPL: HResult = 0x8000_4001_u32 as i32;
pub(crate) const E_NOINTERFACE: HResult = 0x8000_4002_u32 as i32;
pub(crate) const E_POINTER: HResult = 0x8000_4003_u32 as i32;
pub(crate) const E_FAIL: HResult = 0x8000_4005_u32 as i32;
pub(crate) const E_INVALIDARG: HResult = 0x8007_0057_u32 as i32;
pub(crate) const E_OUTOFMEMORY: HResult = 0x8007_000E_u32 as i32;

/// An interface identifier.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Guid {
    pub(crate) data1: u32,
    pub(crate) data2: u16,
    pub(crate) data3: u16,
    pub(crate) data4: [u8; 8],
}

impl Guid {
    /// One of the system's own, which all end the same way.
    pub(crate) const fn standard(data1: u32) -> Self {
        Self { data1, data2: 0, data3: 0, data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] }
    }
}

pub(crate) const IID_IUNKNOWN: Guid = Guid::standard(0x0000_0000);

/// An object with a reference count, which every one of this program's
/// COM objects keeps just after its table.
pub(crate) trait Counted {
    fn refs(&self) -> &Cell<u32>;
}

/// `IUnknown::AddRef`.
///
/// # Safety
/// `this` must point at a live object made by `Box::into_raw`.
pub(crate) unsafe extern "system" fn add_ref<T: Counted>(this: *mut T) -> u32 {
    let refs = (*this).refs().get() + 1;
    (*this).refs().set(refs);
    refs
}

/// `IUnknown::Release`: the object is freed when the last reference goes.
///
/// # Safety
/// `this` must point at a live object made by `Box::into_raw`.
pub(crate) unsafe extern "system" fn release<T: Counted>(this: *mut T) -> u32 {
    let refs = (*this).refs().get().saturating_sub(1);
    (*this).refs().set(refs);
    if refs == 0 {
        drop(Box::from_raw(this));
    }
    refs
}

/// `IUnknown::QueryInterface` for an object with one table that answers
/// to each of the given interfaces.
///
/// # Safety
/// `this` must point at a live object; `iid` and `out` are the system's.
pub(crate) unsafe fn query_interface<T: Counted>(
    this: *mut T,
    iid: *const Guid,
    out: *mut *mut c_void,
    answers: &[Guid],
) -> HResult {
    if out.is_null() {
        return E_POINTER;
    }
    if iid.is_null() {
        *out = core::ptr::null_mut();
        return E_INVALIDARG;
    }
    if *iid == IID_IUNKNOWN || answers.contains(&*iid) {
        add_ref(this);
        *out = this.cast();
        S_OK
    } else {
        *out = core::ptr::null_mut();
        E_NOINTERFACE
    }
}

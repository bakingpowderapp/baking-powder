#![allow(non_camel_case_types, non_upper_case_globals)]
use std::ffi::{CStr, c_char, c_void};

#[cfg(not(target_arch = "aarch64"))]
compile_error!("objc_msgSend returns NSRect and NSPoint in registers only on arm64");

pub type id = *mut c_void;
pub type Sel = *const c_void;
pub type Class = *mut c_void;
pub type Bool = i8;
pub const YES: Bool = 1;
pub const NO: Bool = 0;
pub const nil: id = std::ptr::null_mut();

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NSPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NSSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NSRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[link(name = "objc")]
unsafe extern "C" {
    pub fn objc_getClass(name: *const c_char) -> Class;
    pub fn objc_allocateClassPair(superclass: Class, name: *const c_char, extra: usize) -> Class;
    pub fn objc_registerClassPair(class: Class);
    pub fn class_addMethod(class: Class, sel: Sel, imp: *const c_void, types: *const c_char) -> Bool;
    pub fn sel_registerName(name: *const c_char) -> Sel;
    pub fn objc_msgSend();
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

pub fn class(name: &CStr) -> Class {
    unsafe { objc_getClass(name.as_ptr()) }
}

pub fn sel(name: &CStr) -> Sel {
    unsafe { sel_registerName(name.as_ptr()) }
}

macro_rules! msg {
    ($recv:expr, $sel:expr $(, $arg:expr)*) => {{
        let f: unsafe extern "C" fn($crate::objc::id, $crate::objc::Sel $(, msg!(@ty $arg))*) -> _ =
            unsafe { std::mem::transmute($crate::objc::objc_msgSend as *const ()) };
        unsafe { f($recv, $crate::objc::sel($sel) $(, $arg)*) }
    }};
    (@ty $arg:expr) => { _ };
}
pub(crate) use msg;

pub fn alloc(name: &CStr) -> id {
    let object: id = msg!(class(name), c"alloc");
    object
}

pub fn nsstring(text: &CStr) -> id {
    let string: id = msg!(class(c"NSString"), c"stringWithUTF8String:", text.as_ptr());
    string
}

pub fn subclass(superclass: &CStr, name: &CStr, methods: &[(&CStr, *const c_void, &CStr)]) -> Class {
    let cls = unsafe { objc_allocateClassPair(class(superclass), name.as_ptr(), 0) };
    for (selector, imp, types) in methods {
        unsafe { class_addMethod(cls, sel(selector), *imp, types.as_ptr()) };
    }
    unsafe { objc_registerClassPair(cls) };
    cls
}

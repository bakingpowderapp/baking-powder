use crate::files::{Told, write_later};
use crate::objc::{NO, YES, class, id, msg, nsstring};
use std::ffi::{CStr, CString, c_char};

#[link(name = "UniformTypeIdentifiers", kind = "framework")]
unsafe extern "C" {}

pub(crate) fn string_of(string: id) -> Option<String> {
    if string.is_null() {
        return None;
    }
    let utf8: *const c_char = msg!(string, c"UTF8String");
    if utf8.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(utf8) }.to_str().ok().map(String::from)
}

const MODAL_OK: i64 = 1;

pub(crate) fn save_file(name: String, extension: &CStr, bytes: Vec<u8>, told: Told) {
    let Ok(name) = CString::new(name) else {
        return;
    };
    if let Some(path) = panel(c"NSSavePanel", c"savePanel", extension, &name) {
        write_later(path, bytes, told);
    }
}

pub(crate) fn panel(cls: &CStr, constructor: &CStr, extension: &CStr, name: &CStr) -> Option<String> {
    let panel: id = msg!(class(cls), constructor);
    allow(panel, extension, name);
    chosen(panel)
}

pub(crate) fn export_panel(name: &str, message: &str) -> Option<String> {
    let (Ok(name), Ok(message)) = (CString::new(name), CString::new(message)) else {
        return None;
    };
    let panel: id = msg!(class(c"NSSavePanel"), c"savePanel");
    allow(panel, c"wav", &name);
    let _: () = msg!(panel, c"setMessage:", nsstring(&message));
    let _: () = msg!(panel, c"setPrompt:", nsstring(c"Export"));
    chosen(panel)
}

pub(crate) fn folder_panel(at: Option<&str>) -> Option<String> {
    let panel: id = msg!(class(c"NSOpenPanel"), c"openPanel");
    let _: () = msg!(panel, c"setCanChooseDirectories:", YES);
    let _: () = msg!(panel, c"setCanChooseFiles:", NO);
    let _: () = msg!(panel, c"setAllowsMultipleSelection:", NO);
    if let Some(at) = at.and_then(|at| CString::new(at).ok()) {
        let url: id = msg!(class(c"NSURL"), c"fileURLWithPath:", nsstring(&at));
        let _: () = msg!(panel, c"setDirectoryURL:", url);
        let _: () = msg!(panel, c"setMessage:", nsstring(c"Open lets Baking Powder save and export here."));
    }
    chosen(panel)
}

fn allow(panel: id, extension: &CStr, name: &CStr) {
    let file_type: id = msg!(class(c"UTType"), c"typeWithFilenameExtension:", nsstring(extension));
    let types: id = msg!(class(c"NSArray"), c"arrayWithObject:", file_type);
    let _: () = msg!(panel, c"setAllowedContentTypes:", types);
    let _: () = msg!(panel, c"setNameFieldStringValue:", nsstring(name));
}

fn chosen(panel: id) -> Option<String> {
    let response: i64 = msg!(panel, c"runModal");
    if response != MODAL_OK {
        return None;
    }
    let url: id = msg!(panel, c"URL");
    let path: id = msg!(url, c"path");
    string_of(path)
}

use bp::app::Dropped;
use bp::browser::{self, Ghost, Kind};
use bp::draw::{Lens, Theme};
use crate::files::{keep_access, open};
use crate::objc::{Bool, NO, NSRect, NSSize, Sel, YES, alloc, class, id, msg, nil, nsstring};
use crate::panels::string_of;
use crate::{CGContextRef, VIEW, lens, location, paint, settle, units_at, with_app};
use std::cell::Cell;
use std::ffi::CString;
use std::path::Path;

const DRAG_NONE: u64 = 0;
const DRAG_COPY: u64 = 1;

fn dragged(sender: id) -> Option<(String, Kind)> {
    let pasteboard: id = msg!(sender, c"draggingPasteboard");
    let classes: id = msg!(class(c"NSArray"), c"arrayWithObject:", class(c"NSURL"));
    let urls: id = msg!(pasteboard, c"readObjectsForClasses:options:", classes, nil);
    let url: id = msg!(urls, c"firstObject");
    if url.is_null() {
        return None;
    }
    let path: id = msg!(url, c"path");
    let path = string_of(path)?;
    let kind = browser::kind(&path, Path::new(&path).is_dir())?;
    Some((path, kind))
}

fn drop_point(view: id, sender: id) -> (i32, i32) {
    units_at(view, msg!(sender, c"draggingLocation"))
}

pub(crate) extern "C" fn dragging_entered(_: id, _: Sel, sender: id) -> u64 {
    if dragged(sender).is_some() { DRAG_COPY } else { DRAG_NONE }
}

pub(crate) extern "C" fn perform_drag_operation(this: id, _: Sel, sender: id) -> Bool {
    let Some((path, kind)) = dragged(sender) else {
        return NO;
    };
    let (x, y) = drop_point(this, sender);
    let taken = match with_app(|app| app.dropped(path.clone(), kind, x, y)) {
        Dropped::Kept => {
            keep_access(&path);
            true
        }
        Dropped::Read => true,
        Dropped::Document => open(path, Some((x, y))),
    };
    settle(this, true);
    if taken { YES } else { NO }
}

pub(crate) extern "C" fn source_operation_mask(_: id, _: Sel, _: id, _: i64) -> u64 {
    DRAG_COPY
}

fn ghost_image(ghost: &Ghost, theme: Theme, lens: Lens) -> id {
    let image: id = msg!(alloc(c"NSImage"), c"initWithSize:", NSSize { width: lens.point(ghost.w), height: lens.point(ghost.h) });
    let _: () = msg!(image, c"lockFocusFlipped:", YES);
    let graphics: id = msg!(class(c"NSGraphicsContext"), c"currentContext");
    let context: CGContextRef = msg!(graphics, c"CGContext");
    for fill in &ghost.fills {
        paint(context, fill, theme, lens);
    }
    let _: () = msg!(image, c"unlockFocus");
    let image: id = msg!(image, c"autorelease");
    image
}

pub(crate) fn drag_out(path: &str, ghost: &Ghost) {
    let view = VIEW.with(Cell::get);
    let app: id = msg!(class(c"NSApplication"), c"sharedApplication");
    let event: id = msg!(app, c"currentEvent");
    let Ok(cpath) = CString::new(path) else {
        return;
    };
    if view.is_null() || event.is_null() {
        return;
    }
    let (x, y) = location(view, event);
    let lens = lens(view);
    let image = ghost_image(ghost, with_app(|app| app.theme()), lens);
    let url: id = msg!(class(c"NSURL"), c"fileURLWithPath:", nsstring(&cpath));
    let item: id = msg!(alloc(c"NSDraggingItem"), c"initWithPasteboardWriter:", url);
    let item: id = msg!(item, c"autorelease");
    let frame = NSRect { x: lens.point(x - ghost.grab.0), y: lens.point(y - ghost.grab.1), width: lens.point(ghost.w), height: lens.point(ghost.h) };
    let _: () = msg!(item, c"setDraggingFrame:contents:", frame, image);
    let items: id = msg!(class(c"NSArray"), c"arrayWithObject:", item);
    let _: id = msg!(view, c"beginDraggingSessionWithItems:event:source:", items, event, view);
}

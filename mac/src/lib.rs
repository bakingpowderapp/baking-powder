mod audio;
mod drag;
mod files;
mod objc;
mod panels;

use bp::app::{App, Event, Request};
use bp::keyboard::Key;
use bp::draw::{Fill, Lens, Theme};
use objc::{Bool, NO, NSPoint, NSRect, Sel, YES, alloc, class, id, msg, nil, nsstring, sel, subclass};
use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_void};
use drag::{drag_out, dragging_entered, perform_drag_operation, source_operation_mask};
use files::{Told, default_path, finish_writes, hold_folders, keep_access, list, listing, open, own_path, read_file, rename, reveal, scan, session_path, settle_lists, settle_scans, settle_writes, stow, sweep, trash, write_later, write_own};
use panels::{export_panel, folder_panel, panel, save_file, string_of};

pub(crate) type CGContextRef = *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGContextSetRGBFillColor(context: CGContextRef, red: f64, green: f64, blue: f64, alpha: f64);
    fn CGContextFillRect(context: CGContextRef, rect: NSRect);
}

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new());
    static OUTPUT: RefCell<Option<audio::Output>> = const { RefCell::new(None) };
    pub(crate) static VIEW: Cell<id> = const { Cell::new(nil) };
    static MODIFIERS: Cell<u64> = const { Cell::new(0) };
    static SIZED: Cell<bool> = const { Cell::new(false) };
    pub(crate) static SCOPES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    pub(crate) static WRITING: RefCell<Vec<(String, id)>> = const { RefCell::new(Vec::new()) };
}

const NS_SHIFT: u64 = 1 << 17;
const NS_CONTROL: u64 = 1 << 18;
const NS_COMMAND: u64 = 1 << 20;
const KEY_RETURN: u16 = 36;
const KEY_ENTER: u16 = 76;
const KEY_ESCAPE: u16 = 53;
const KEY_DELETE: u16 = 51;
const KEY_FORWARD_DELETE: u16 = 117;
const KEY_LEFT: u16 = 123;
const KEY_RIGHT: u16 = 124;
const KEY_DOWN: u16 = 125;
const KEY_UP: u16 = 126;

fn sync_modifiers(view: id, event: id) -> u64 {
    let flags: u64 = msg!(event, c"modifierFlags");
    let flags = flags & (NS_SHIFT | NS_COMMAND);
    if MODIFIERS.with(|cell| cell.replace(flags)) != flags {
        deliver(view, Event::Modifiers { shift: flags & NS_SHIFT != 0, command: flags & NS_COMMAND != 0 });
    }
    flags
}

extern "C" fn flags_changed(this: id, _: Sel, event: id) {
    sync_modifiers(this, event);
}

fn key_of(event: id) -> Option<Key> {
    let code: u16 = msg!(event, c"keyCode");
    match code {
        KEY_RETURN | KEY_ENTER => return Some(Key::Return),
        KEY_ESCAPE => return Some(Key::Escape),
        KEY_DELETE | KEY_FORWARD_DELETE => return Some(Key::Delete),
        KEY_LEFT => return Some(Key::Left),
        KEY_RIGHT => return Some(Key::Right),
        KEY_UP => return Some(Key::Up),
        KEY_DOWN => return Some(Key::Down),
        _ => {}
    }
    match string_of(msg!(event, c"charactersIgnoringModifiers"))?.chars().next()? {
        ' ' => Some(Key::Space),
        c if c.is_ascii_graphic() => Some(Key::Char(c.to_ascii_lowercase())),
        _ => None,
    }
}

fn deliver_key(view: id, event: id) {
    let flags = sync_modifiers(view, event);
    if let Some(key) = key_of(event) {
        let repeat: Bool = msg!(event, c"isARepeat");
        deliver(view, Event::Key { key, shift: flags & NS_SHIFT != 0, command: flags & NS_COMMAND != 0, repeat: repeat != NO });
    }
}

extern "C" fn key_down(this: id, _: Sel, event: id) {
    deliver_key(this, event);
}

extern "C" fn key_up(this: id, _: Sel, event: id) {
    if let Some(key) = key_of(event) {
        deliver(this, Event::KeyUp { key });
    }
}

extern "C" fn perform_key_equivalent(this: id, _: Sel, event: id) -> Bool {
    let flags: u64 = msg!(event, c"modifierFlags");
    if flags & NS_COMMAND == 0 || !key_of(event).is_some_and(App::takes_command) {
        return NO;
    }
    deliver_key(this, event);
    YES
}

const TICK_SECONDS: f64 = 1.0 / 60.0;

extern "C" fn tick(_: id, _: Sel, _: id) {
    let written = settle_writes() | settle_scans() | settle_lists();
    let repaint = with_app(|app| app.tick(TICK_SECONDS as f32)) || written;
    let view = VIEW.with(Cell::get);
    if !view.is_null() {
        settle(view, repaint);
    }
}

fn start_ticking() {
    let cls = subclass(c"NSObject", c"BakingPowderTicker", &[(c"tick:", tick as *const c_void, c"v@:@")]);
    let ticker: id = msg!(cls, c"alloc");
    let ticker: id = msg!(ticker, c"init");
    let timer: id = msg!(class(c"NSTimer"), c"timerWithTimeInterval:target:selector:userInfo:repeats:", TICK_SECONDS, ticker, sel(c"tick:"), nil, YES);
    let run_loop: id = msg!(class(c"NSRunLoop"), c"currentRunLoop");
    let _: () = msg!(run_loop, c"addTimer:forMode:", timer, nsstring(c"kCFRunLoopCommonModes"));
}

pub(crate) fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|app| f(&mut app.borrow_mut()))
}

fn start_audio() {
    open_audio(with_app(|app| app.buffer()));
}

fn open_audio(frames: u32) {
    OUTPUT.with(|slot| slot.borrow_mut().take());
    with_app(|app| app.audio_stopped());
    let Ok(mut output) = audio::Output::open(frames) else {
        return;
    };
    let mut renderer = with_app(|app| app.audio_started(output.sample_rate, output.max_frames, output.frames));
    let workers = renderer.workers();
    if output.start(Box::new(move |left, right| renderer.render(left, right))).is_ok() {
        let (helpers, promote) = output.pool();
        workers.start(helpers, promote);
        OUTPUT.with(|slot| *slot.borrow_mut() = Some(output));
    } else {
        with_app(|app| app.audio_stopped());
    }
}

pub(crate) fn lens(view: id) -> Lens {
    let bounds: NSRect = msg!(view, c"bounds");
    let window: id = msg!(view, c"window");
    let pixels: f64 = msg!(window, c"backingScaleFactor");
    with_app(|app| app.lens(bounds.width as i32, bounds.height as i32, pixels as i32))
}

pub(crate) fn paint(context: CGContextRef, fill: &Fill, theme: Theme, lens: Lens) {
    let [red, green, blue, alpha] = theme.rgba(*fill);
    let (x, y) = (lens.point(fill.rect.x), lens.point(fill.rect.y));
    let rect = NSRect { x, y, width: lens.point(fill.rect.x + fill.rect.w) - x, height: lens.point(fill.rect.y + fill.rect.h) - y };
    unsafe {
        CGContextSetRGBFillColor(context, red, green, blue, alpha);
        CGContextFillRect(context, rect);
    }
}

extern "C" fn is_flipped(_: id, _: Sel) -> Bool {
    YES
}

extern "C" fn accepts_first_responder(_: id, _: Sel) -> Bool {
    YES
}

extern "C" fn accepts_first_mouse(_: id, _: Sel, _: id) -> Bool {
    YES
}

extern "C" fn draw_rect(this: id, _: Sel, _: NSRect) {
    let bounds: NSRect = msg!(this, c"bounds");
    let graphics: id = msg!(class(c"NSGraphicsContext"), c"currentContext");
    let context: CGContextRef = msg!(graphics, c"CGContext");
    let lens = lens(this);
    with_app(|app| {
        let theme = app.theme();
        for fill in app.draw(lens.units(bounds.width).ceil() as i32, lens.units(bounds.height).ceil() as i32) {
            paint(context, fill, theme, lens);
        }
    });
}

pub(crate) fn units_at(view: id, window_point: NSPoint) -> (i32, i32) {
    let point: NSPoint = msg!(view, c"convertPoint:fromView:", window_point, nil);
    let lens = lens(view);
    (lens.units(point.x).floor() as i32, lens.units(point.y).floor() as i32)
}

pub(crate) fn location(view: id, event: id) -> (i32, i32) {
    units_at(view, msg!(event, c"locationInWindow"))
}

fn deliver(view: id, event: Event) {
    let repaint = with_app(|app| app.event(event));
    settle(view, repaint);
}

fn serve() -> bool {
    let mut served = false;
    loop {
        let requests = with_app(|app| app.take_requests());
        if requests.is_empty() {
            break;
        }
        served = true;
        for request in requests {
            handle(request);
        }
    }
    served
}

pub(crate) fn settle(view: id, repaint: bool) {
    if serve() || repaint {
        let _: () = msg!(view, c"setNeedsDisplay:", YES);
    }
}

fn deliver_to_view(event: Event) {
    let view = VIEW.with(Cell::get);
    if !view.is_null() {
        deliver(view, event);
    }
}

fn fresh() {
    let path = default_path();
    let text = std::fs::read_to_string(&path).ok();
    with_app(|app| app.begin(path.to_string_lossy().into_owned(), text.as_deref()));
}

fn begin() {
    let resumed = std::fs::read_to_string(session_path()).is_ok_and(|text| with_app(|app| app.resume(&text)));
    if !resumed {
        fresh();
    } else if !SIZED.with(Cell::get) {
        sweep();
    }
    hold_folders();
    factory();
}

fn files_under(directory: &std::path::Path) -> usize {
    std::fs::read_dir(directory).map_or(0, |entries| entries.flatten().map(|entry| if entry.file_type().is_ok_and(|kind| kind.is_dir()) { files_under(&entry.path()) } else { 1 }).sum())
}

fn factory() {
    let root = own_path("Factory");
    let fresh = own_path("Factory.fresh");
    let whole = files_under(&root) == bp::factory::PRESETS.len() && bp::factory::PRESETS.iter().all(|(name, json)| std::fs::read(root.join(name)).is_ok_and(|held| held == json.as_bytes()));
    let written = whole || {
        let _ = std::fs::remove_dir_all(&fresh);
        bp::factory::PRESETS.iter().all(|(name, json)| write_own(&fresh.join(name), json))
    };
    let path = if whole {
        root
    } else if written {
        let _ = std::fs::remove_dir_all(&root);
        if std::fs::rename(&fresh, &root).is_ok() { root } else { fresh }
    } else {
        let _ = std::fs::remove_dir_all(&fresh);
        root
    };
    with_app(|app| {
        if !written {
            app.factory_not_written();
        }
        if path.is_dir() {
            app.factory(path.to_string_lossy().into_owned());
        }
    });
}

fn handle(request: Request) {
    match request {
        Request::Open { path } => {
            if let Some(path) = path.or_else(|| panel(c"NSOpenPanel", c"openPanel", c"json", c"Untitled.json")) {
                open(path, None);
            }
        }
        Request::SaveAs { json, save } => match panel(c"NSSavePanel", c"savePanel", c"json", c"Untitled.json") {
            Some(path) => {
                with_app(|app| app.assured(save));
                write_later(path, json.into_bytes(), Told::Save(save));
            }
            None => with_app(|app| app.unsaved(save)),
        },
        Request::Write { path, json, save } => write_later(path, json.into_bytes(), Told::Save(save)),
        Request::Remember { json } => {
            if !SIZED.with(Cell::get) {
                let path = session_path();
                if let Some(directory) = path.parent() {
                    let _ = std::fs::create_dir_all(directory);
                }
                write_later(path.to_string_lossy().into_owned(), json.into_bytes(), Told::Session);
            }
        }
        Request::Default { json } => match json {
            Some(json) => {
                if !write_own(&default_path(), &json) {
                    with_app(|app| app.default_not_saved(false));
                }
            }
            None => {
                if std::fs::remove_file(default_path()).is_err_and(|error| error.kind() != std::io::ErrorKind::NotFound) {
                    with_app(|app| app.default_not_saved(true));
                }
            }
        },
        Request::Fresh => fresh(),
        Request::Export { name, message } => {
            if let Some(path) = export_panel(&name, &message) {
                with_app(|app| app.export_to(path));
            }
        }
        Request::Exported { path, bytes } => write_later(path, bytes, Told::Export),
        Request::Midi { name, bytes } => save_file(name, c"mid", bytes, Told::Failure),
        Request::Wave { name, bytes } => save_file(name, c"wav", bytes, Told::Export),
        Request::Preset { name, json } => save_file(name, c"json", json.into_bytes(), Told::Failure),
        Request::OpenLink { url } => {
            let Ok(url) = CString::new(url) else {
                return;
            };
            let url: id = msg!(class(c"NSURL"), c"URLWithString:", nsstring(&url));
            if !url.is_null() {
                let workspace: id = msg!(class(c"NSWorkspace"), c"sharedWorkspace");
                let _: Bool = msg!(workspace, c"openURL:", url);
            }
        }
        Request::Buffer { frames } => open_audio(frames),
        Request::Copy { text } => {
            let Ok(text) = CString::new(text) else {
                return;
            };
            let pasteboard: id = msg!(class(c"NSPasteboard"), c"generalPasteboard");
            let _: i64 = msg!(pasteboard, c"clearContents");
            let _: Bool = msg!(pasteboard, c"setString:forType:", nsstring(&text), nsstring(c"public.utf8-plain-text"));
        }
        Request::Folder { at } => {
            if let Some(path) = folder_panel(at.as_deref()) {
                keep_access(&path);
                with_app(|app| app.add_folder(path));
            }
        }
        Request::List { path } => list(path),
        Request::Scan { path } => scan(path),
        Request::Rename { path, name } => rename(&path, &name),
        Request::Delete { path } => trash(&path),
        Request::Reveal { path } => reveal(&path),
        Request::Keep { path, text } => write_later(path, text.into_bytes(), Told::Nobody),
        Request::Read { path } => read_file(path),
        Request::Stow { file, bytes } => stow(&file, bytes),
        Request::Drag { path, ghost } => drag_out(&path, &ghost),
    }
}

extern "C" fn mouse_moved(this: id, _: Sel, event: id) {
    sync_modifiers(this, event);
    let (x, y) = location(this, event);
    deliver(this, Event::Move { x, y });
}

extern "C" fn mouse_down(this: id, _: Sel, event: id) {
    sync_modifiers(this, event);
    let (x, y) = location(this, event);
    let flags: u64 = msg!(event, c"modifierFlags");
    if flags & NS_CONTROL != 0 {
        deliver(this, Event::RightDown { x, y });
        return;
    }
    let clicks: i64 = msg!(event, c"clickCount");
    deliver(this, Event::Down { x, y, clicks: clicks as i32 });
}

extern "C" fn right_mouse_down(this: id, _: Sel, event: id) {
    sync_modifiers(this, event);
    let (x, y) = location(this, event);
    deliver(this, Event::RightDown { x, y });
}

extern "C" fn other_mouse_down(this: id, _: Sel, event: id) {
    sync_modifiers(this, event);
    let button: i64 = msg!(event, c"buttonNumber");
    if button != MIDDLE_BUTTON {
        return;
    }
    let (x, y) = location(this, event);
    deliver(this, Event::MiddleDown { x, y });
}

extern "C" fn mouse_up(this: id, _: Sel, event: id) {
    let (x, y) = location(this, event);
    deliver(this, Event::Up { x, y });
}

extern "C" fn mouse_exited(this: id, _: Sel, _: id) {
    deliver(this, Event::Leave);
}

const MIDDLE_BUTTON: i64 = 2;
const WHEEL_LINE_PT: f64 = 32.0;

extern "C" fn scroll_wheel(this: id, _: Sel, event: id) {
    let (x, y) = location(this, event);
    let delta_x: f64 = msg!(event, c"scrollingDeltaX");
    let delta_y: f64 = msg!(event, c"scrollingDeltaY");
    let precise: Bool = msg!(event, c"hasPreciseScrollingDeltas");
    let lens = lens(this);
    let units = |delta: f64| lens.units(if precise != NO { delta } else { delta * WHEEL_LINE_PT }).round() as i32;
    deliver(this, Event::Scroll { x, y, dx: units(delta_x), dy: units(delta_y) });
}

const MOUSE_ENTERED_AND_EXITED: u64 = 0x01;
const MOUSE_MOVED: u64 = 0x02;
const ACTIVE_IN_KEY_WINDOW: u64 = 0x20;
const IN_VISIBLE_RECT: u64 = 0x200;

fn view(frame: NSRect) -> id {
    let cls = subclass(c"NSView", c"BakingPowderView", &[
        (c"isFlipped", is_flipped as *const c_void, c"B@:"),
        (c"acceptsFirstResponder", accepts_first_responder as *const c_void, c"B@:"),
        (c"acceptsFirstMouse:", accepts_first_mouse as *const c_void, c"B@:@"),
        (c"drawRect:", draw_rect as *const c_void, c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}"),
        (c"mouseMoved:", mouse_moved as *const c_void, c"v@:@"),
        (c"mouseDragged:", mouse_moved as *const c_void, c"v@:@"),
        (c"mouseDown:", mouse_down as *const c_void, c"v@:@"),
        (c"rightMouseDown:", right_mouse_down as *const c_void, c"v@:@"),
        (c"rightMouseDragged:", mouse_moved as *const c_void, c"v@:@"),
        (c"otherMouseDown:", other_mouse_down as *const c_void, c"v@:@"),
        (c"mouseUp:", mouse_up as *const c_void, c"v@:@"),
        (c"rightMouseUp:", mouse_up as *const c_void, c"v@:@"),
        (c"mouseExited:", mouse_exited as *const c_void, c"v@:@"),
        (c"scrollWheel:", scroll_wheel as *const c_void, c"v@:@"),
        (c"keyDown:", key_down as *const c_void, c"v@:@"),
        (c"keyUp:", key_up as *const c_void, c"v@:@"),
        (c"flagsChanged:", flags_changed as *const c_void, c"v@:@"),
        (c"performKeyEquivalent:", perform_key_equivalent as *const c_void, c"B@:@"),
        (c"draggingEntered:", dragging_entered as *const c_void, c"Q@:@"),
        (c"performDragOperation:", perform_drag_operation as *const c_void, c"B@:@"),
        (c"draggingSession:sourceOperationMaskForDraggingContext:", source_operation_mask as *const c_void, c"Q@:@q"),
    ]);
    let object: id = msg!(cls, c"alloc");
    let object: id = msg!(object, c"initWithFrame:", frame);
    let options = MOUSE_ENTERED_AND_EXITED | MOUSE_MOVED | ACTIVE_IN_KEY_WINDOW | IN_VISIBLE_RECT;
    let area: id = msg!(alloc(c"NSTrackingArea"), c"initWithRect:options:owner:userInfo:", frame, options, object, nil);
    let _: () = msg!(object, c"addTrackingArea:", area);
    let types: id = msg!(class(c"NSArray"), c"arrayWithObject:", nsstring(c"public.file-url"));
    let _: () = msg!(object, c"registerForDraggedTypes:", types);
    object
}

extern "C" fn quit_after_last_window(_: id, _: Sel, _: id) -> Bool {
    YES
}

extern "C" fn resigned_active(_: id, _: Sel, _: id) {
    deliver_to_view(Event::Blur);
}

extern "C" fn became_active(_: id, _: Sel, _: id) {
    deliver_to_view(Event::Focus);
}

extern "C" fn will_terminate(_: id, _: Sel, _: id) {
    deliver_to_view(Event::Quit);
    loop {
        finish_writes();
        if !settle_writes() || !serve() {
            break;
        }
    }
}

fn delegate() -> id {
    let cls = subclass(c"NSObject", c"BakingPowderDelegate", &[
        (c"applicationShouldTerminateAfterLastWindowClosed:", quit_after_last_window as *const c_void, c"B@:@"),
        (c"applicationDidResignActive:", resigned_active as *const c_void, c"v@:@"),
        (c"applicationDidBecomeActive:", became_active as *const c_void, c"v@:@"),
        (c"applicationWillTerminate:", will_terminate as *const c_void, c"v@:@"),
    ]);
    let object: id = msg!(cls, c"alloc");
    let object: id = msg!(object, c"init");
    object
}

fn main_menu() -> id {
    let bar: id = msg!(alloc(c"NSMenu"), c"init");
    let app_item: id = msg!(alloc(c"NSMenuItem"), c"init");
    let app_menu: id = msg!(alloc(c"NSMenu"), c"init");
    let quit: id = msg!(alloc(c"NSMenuItem"), c"initWithTitle:action:keyEquivalent:", nsstring(c"Quit Baking Powder"), sel(c"terminate:"), nsstring(c"q"));
    let _: () = msg!(app_menu, c"addItem:", quit);
    let _: () = msg!(app_item, c"setSubmenu:", app_menu);
    let _: () = msg!(bar, c"addItem:", app_item);
    bar
}

fn screenshot(arguments: &[String]) -> Result<(), String> {
    let usage = || String::from("usage: baking-powder --screenshot WIDTHxHEIGHT@SCALE FILE.png");
    let [size, path] = arguments else {
        return Err(usage());
    };
    let (size, scale) = size.split_once('@').ok_or_else(usage)?;
    let (width, height) = size.split_once('x').ok_or_else(usage)?;
    let parse = |text: &str| text.parse::<i32>().ok().filter(|&value| value > 0).ok_or_else(usage);
    let (width, height, scale) = (parse(width)?, parse(height)?, parse(scale)?);
    SIZED.with(|cell| cell.set(true));
    begin();
    loop {
        serve();
        let pending = listing();
        if !settle_lists() && !pending {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let png = with_app(|app| app.screenshot(width, height, scale));
    if let Some(directory) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(directory);
    }
    std::fs::write(path, png).map_err(|error| format!("{path}: {error}"))
}

fn sized(arguments: &[String]) -> Result<Option<(f64, f64)>, String> {
    let Some(at) = arguments.iter().position(|argument| argument == "--size") else {
        return Ok(None);
    };
    let usage = || String::from("usage: baking-powder --size WIDTHxHEIGHT");
    let (width, height) = arguments.get(at + 1).and_then(|size| size.split_once('x')).ok_or_else(usage)?;
    let parse = |text: &str| text.parse::<i32>().ok().filter(|&value| value > 0).map(f64::from).ok_or_else(usage);
    Ok(Some((parse(width)?, parse(height)?)))
}

pub fn run() {
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(at) = arguments.iter().position(|argument| argument == "--screenshot") {
        if let Err(problem) = screenshot(&arguments[at + 1..]) {
            eprintln!("{problem}");
            std::process::exit(1);
        }
        return;
    }
    let sized = match sized(&arguments) {
        Ok(sized) => sized,
        Err(problem) => {
            eprintln!("{problem}");
            std::process::exit(1);
        }
    };
    SIZED.with(|cell| cell.set(sized.is_some()));
    let app: id = msg!(class(c"NSApplication"), c"sharedApplication");
    let _: Bool = msg!(app, c"setActivationPolicy:", 0i64);
    let _: () = msg!(app, c"setDelegate:", delegate());
    let _: () = msg!(app, c"setMainMenu:", main_menu());
    let titled_closable_miniaturizable_resizable: u64 = 1 | 2 | 4 | 8;
    let buffered: u64 = 2;
    let (width, height) = sized.unwrap_or((1280.0, 800.0));
    let frame = NSRect { x: 0.0, y: 0.0, width, height };
    let window: id = msg!(alloc(c"NSWindow"), c"initWithContentRect:styleMask:backing:defer:", frame, titled_closable_miniaturizable_resizable, buffered, NO);
    let _: () = msg!(window, c"setTitle:", nsstring(c"Baking Powder"));
    let view = view(frame);
    VIEW.with(|cell| cell.set(view));
    let _: () = msg!(window, c"setContentView:", view);
    let _: () = msg!(window, c"makeFirstResponder:", view);
    let _: () = msg!(window, c"setAcceptsMouseMovedEvents:", YES);
    let _: () = msg!(window, c"center");
    if sized.is_none() {
        let _: Bool = msg!(window, c"setFrameAutosaveName:", nsstring(c"Baking Powder"));
    }
    let _: () = msg!(window, c"makeKeyAndOrderFront:", nil);
    let _: () = msg!(app, c"activateIgnoringOtherApps:", YES);
    begin();
    settle(view, false);
    start_audio();
    audio::watch_default_device(start_audio);
    start_ticking();
    let _: () = msg!(app, c"run");
}

#[unsafe(no_mangle)]
pub extern "C" fn baking_powder_main() {
    run();
}

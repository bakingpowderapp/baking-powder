use bp::app::Opened;
use bp::browser::{self, Found, Item, Kind, Seen};
use crate::objc::{Bool, NO, YES, alloc, class, id, msg, nil, nsstring};
use crate::panels::string_of;
use crate::{SCOPES, WRITING, with_app};
use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;

#[derive(Clone, Copy)]
pub(crate) enum Told {
    Nobody,
    Failure,
    Session,
    Save(u64),
    Export,
    Stow,
}

type Write = (String, Vec<u8>, Told);

static WRITER: Mutex<Option<(Sender<Write>, JoinHandle<()>)>> = Mutex::new(None);
static WRITTEN: Mutex<Vec<(String, bool, Told)>> = Mutex::new(Vec::new());
static WRITES: AtomicU64 = AtomicU64::new(0);
static SCANNED: Mutex<Vec<(String, Vec<Found>)>> = Mutex::new(Vec::new());
static LISTER: Mutex<Option<Sender<String>>> = Mutex::new(None);
static LISTED: Mutex<Vec<(String, Vec<Found>)>> = Mutex::new(Vec::new());
static LISTING: AtomicUsize = AtomicUsize::new(0);
static SEEN: Mutex<Seen> = Mutex::new(Seen::NONE);

fn locked<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn own_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Application Support/Baking Powder").join(name)
}

pub(crate) fn session_path() -> PathBuf {
    own_path("session.json")
}

pub(crate) fn default_path() -> PathBuf {
    own_path("default.json")
}

pub(crate) fn write_own(path: &Path, json: &str) -> bool {
    if let Some(directory) = path.parent() {
        let _ = std::fs::create_dir_all(directory);
    }
    write_replacing(path, json.as_bytes(), false)
}

const BOOKMARK_WITH_SECURITY_SCOPE: u64 = 1 << 11;
const RESOLVE_WITH_SECURITY_SCOPE: u64 = 1 << 10;
const ITEM_REPLACEMENT_DIRECTORY: u64 = 99;
const USER_DOMAIN: u64 = 1;
const ORIGINAL_ITEM_LOCATION: &CStr = c"NSFileOriginalItemLocationKey";

fn bookmark_key(path: &str) -> Option<CString> {
    CString::new(format!("bookmark:{path}")).ok()
}

pub(crate) fn keep_access(path: &str) {
    let (Ok(cpath), Some(key)) = (CString::new(path), bookmark_key(path)) else {
        return;
    };
    let url: id = msg!(class(c"NSURL"), c"fileURLWithPath:", nsstring(&cpath));
    let data: id = msg!(url, c"bookmarkDataWithOptions:includingResourceValuesForKeys:relativeToURL:error:", BOOKMARK_WITH_SECURITY_SCOPE, nil, nil, nil);
    if !data.is_null() {
        let defaults: id = msg!(class(c"NSUserDefaults"), c"standardUserDefaults");
        let _: () = msg!(defaults, c"setObject:forKey:", data, nsstring(&key));
    }
}

fn regain_access(path: &str) -> Option<id> {
    let key = bookmark_key(path)?;
    let defaults: id = msg!(class(c"NSUserDefaults"), c"standardUserDefaults");
    let data: id = msg!(defaults, c"dataForKey:", nsstring(&key));
    if data.is_null() {
        return None;
    }
    let mut stale: Bool = NO;
    let url: id = msg!(class(c"NSURL"), c"URLByResolvingBookmarkData:options:relativeToURL:bookmarkDataIsStale:error:", data, RESOLVE_WITH_SECURITY_SCOPE, nil, &raw mut stale, nil);
    if url.is_null() {
        return None;
    }
    let started: Bool = msg!(url, c"startAccessingSecurityScopedResource");
    (started != NO).then_some(url)
}

fn put(mut file: std::fs::File, contents: &[u8], sync: bool) -> std::io::Result<()> {
    std::io::Write::write_all(&mut file, contents)?;
    if sync { file.sync_all() } else { Ok(()) }
}

fn write_replacing(path: &Path, contents: &[u8], sync: bool) -> bool {
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => {
            let written = put(file, contents, sync).is_ok();
            if !written {
                let _ = std::fs::remove_file(path);
            }
            return written;
        }
        Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => return false,
        Err(_) => {}
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".{}.tmp", WRITES.fetch_add(1, Ordering::Relaxed)));
    let temp = PathBuf::from(temp);
    match std::fs::File::create(&temp).and_then(|file| put(file, contents, sync)) {
        Ok(()) if std::fs::rename(&temp, path).is_ok() => true,
        Ok(()) => {
            let _ = std::fs::remove_file(&temp);
            false
        }
        Err(_) => {
            let _ = std::fs::remove_file(&temp);
            replace_item(path, contents, sync)
        }
    }
}

fn replace_item(path: &Path, contents: &[u8], sync: bool) -> bool {
    let (Some(name), Ok(cpath)) = (path.file_name(), CString::new(path.to_string_lossy().into_owned())) else {
        return false;
    };
    let pool: id = msg!(alloc(c"NSAutoreleasePool"), c"init");
    let manager: id = msg!(class(c"NSFileManager"), c"defaultManager");
    let url: id = msg!(class(c"NSURL"), c"fileURLWithPath:", nsstring(&cpath));
    let room: id = msg!(manager, c"URLForDirectory:inDomain:appropriateForURL:create:error:", ITEM_REPLACEMENT_DIRECTORY, USER_DOMAIN, url, YES, nil);
    let room_path: id = msg!(room, c"path");
    let replaced = string_of(room_path).is_some_and(|room| {
        let staged = Path::new(&room).join(name);
        let mut error: id = nil;
        let replaced = std::fs::File::create(&staged).and_then(|file| put(file, contents, sync)).is_ok()
            && CString::new(staged.to_string_lossy().into_owned()).is_ok_and(|cstaged| {
                let staged_url: id = msg!(class(c"NSURL"), c"fileURLWithPath:", nsstring(&cstaged));
                let replaced: Bool = msg!(manager, c"replaceItemAtURL:withItemAtURL:backupItemName:options:resultingItemURL:error:", url, staged_url, nil, 0u64, nil, &raw mut error);
                replaced != NO
            });
        let settled = replaced || {
            let info: id = msg!(error, c"userInfo");
            let moved: id = msg!(info, c"objectForKey:", nsstring(ORIGINAL_ITEM_LOCATION));
            let moved_path: id = msg!(moved, c"path");
            match string_of(moved_path) {
                None => true,
                Some(moved) => !path.exists() && std::fs::rename(&moved, path).is_ok(),
            }
        };
        if settled {
            let _ = std::fs::remove_dir_all(&room);
        }
        replaced
    });
    let _: () = msg!(pool, c"drain");
    replaced
}

fn hold_access(path: &str) {
    let held = SCOPES.with(|scopes| scopes.borrow().iter().any(|root| Path::new(path).starts_with(root)));
    if !held && regain_access(path).is_some() {
        SCOPES.with(|scopes| scopes.borrow_mut().push(String::from(path)));
    }
}

pub(crate) fn hold_folders() {
    for folder in with_app(|app| app.folders()) {
        hold_access(&folder);
    }
}

fn document(path: &Path) -> Option<Item> {
    let (key, metadata) = (path.to_str()?, std::fs::metadata(path).ok()?);
    let changed = metadata.modified().ok().and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |since| since.as_nanos());
    let stamp = (metadata.len(), changed);
    if let Some(item) = locked(&SEEN).known(key, stamp) {
        return Some(item);
    }
    let item = Item::read(&std::fs::read_to_string(path).ok()?);
    locked(&SEEN).keep(key, stamp, &item);
    Some(item)
}

fn bytes_of(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(u64::MAX, |metadata| metadata.len())
}

fn item_of(kind: Kind, path: &Path) -> Option<Item> {
    match kind {
        Kind::Folder => Some(Item::Folder),
        Kind::Wave => Some(Item::Wave(bytes_of(path))),
        Kind::Midi => Some(Item::Midi(bytes_of(path))),
        Kind::Document => document(path),
    }
}

fn found_in(path: &str) -> Vec<Found> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let child = entry.path();
            let (Some(name), Ok(metadata), Some(child_path)) = (entry.file_name().to_str().map(String::from), std::fs::metadata(&child), child.to_str().map(String::from)) else {
                continue;
            };
            let Some(item) = browser::kind(&child_path, metadata.is_dir()).and_then(|kind| item_of(kind, &child)) else {
                continue;
            };
            found.push(Found { path: child_path, name, item });
        }
    }
    found
}

fn listed(path: String, found: Vec<Found>) {
    locked(&LISTED).push((path, found));
}

pub(crate) fn list(path: String) {
    hold_access(&path);
    LISTING.fetch_add(1, Ordering::Relaxed);
    let mut lister = locked(&LISTER);
    if lister.is_none() {
        let (sender, paths) = channel::<String>();
        let spawned = std::thread::Builder::new().name(String::from("bp-list")).spawn(move || {
            for path in paths {
                let found = found_in(&path);
                listed(path, found);
            }
        });
        *lister = spawned.ok().map(|_| sender);
    }
    let unsent = match lister.as_ref() {
        Some(sender) => sender.send(path).err().map(|refused| refused.0),
        None => Some(path),
    };
    drop(lister);
    if let Some(path) = unsent {
        let found = found_in(&path);
        listed(path, found);
    }
}

pub(crate) fn listing() -> bool {
    LISTING.load(Ordering::Relaxed) > 0
}

pub(crate) fn settle_lists() -> bool {
    let done = std::mem::take(&mut *locked(&LISTED));
    let any = !done.is_empty();
    for (path, found) in done {
        LISTING.fetch_sub(1, Ordering::Relaxed);
        with_app(|app| app.listed(&path, found));
    }
    any
}

fn walk(directory: &Path, walked: &mut HashSet<(u64, u64)>, found: &mut Vec<Found>) {
    use std::os::unix::fs::MetadataExt;
    let Ok(metadata) = std::fs::metadata(directory) else {
        return;
    };
    if !walked.insert((metadata.dev(), metadata.ino())) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut entries: Vec<(bool, String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str().map(String::from)?;
            let kind = entry.file_type().ok()?;
            let kind = if kind.is_symlink() { std::fs::metadata(entry.path()).ok()?.file_type() } else { kind };
            (kind.is_dir() || kind.is_file()).then(|| (kind.is_dir(), name, entry.path()))
        })
        .collect();
    entries.sort_by_cached_key(|(folder, name, _)| (*folder, name.to_lowercase()));
    for (folder, name, child) in entries {
        let Some(path) = child.to_str().map(String::from) else {
            continue;
        };
        match browser::kind(&path, folder) {
            Some(Kind::Folder) => walk(&child, walked, found),
            Some(kind) => {
                if let Some(item) = item_of(kind, &child) {
                    found.push(Found { path, name, item });
                }
            }
            None => {}
        }
    }
}

pub(crate) fn scan(path: String) {
    hold_access(&path);
    let root = path.clone();
    let spawned = std::thread::Builder::new().name(String::from("bp-scan")).spawn(move || {
        let mut found = Vec::new();
        walk(Path::new(&path), &mut HashSet::new(), &mut found);
        locked(&SCANNED).push((path, found));
    });
    if spawned.is_err() {
        with_app(|app| app.scanned(root, Vec::new()));
    }
}

pub(crate) fn settle_scans() -> bool {
    let done = std::mem::take(&mut *locked(&SCANNED));
    let any = !done.is_empty();
    for (path, found) in done {
        with_app(|app| app.scanned(path, found));
    }
    any
}

pub(crate) fn read_file(path: String) {
    hold_access(&path);
    let bytes = std::fs::read(&path).ok();
    with_app(|app| app.read(path, bytes));
}

fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

pub(crate) fn rename(path: &str, name: &str) {
    hold_access(path);
    let from = Path::new(path);
    let to = from.with_file_name(name);
    if to.exists() && !same_file(from, &to) {
        return;
    }
    let _ = std::fs::rename(from, to);
}

pub(crate) fn trash(path: &str) {
    hold_access(path);
    let Ok(cpath) = CString::new(path) else {
        return;
    };
    let url: id = msg!(class(c"NSURL"), c"fileURLWithPath:", nsstring(&cpath));
    let manager: id = msg!(class(c"NSFileManager"), c"defaultManager");
    let _: Bool = msg!(manager, c"trashItemAtURL:resultingItemURL:error:", url, nil, nil);
}

pub(crate) fn reveal(path: &str) {
    let Ok(cpath) = CString::new(path) else {
        return;
    };
    let url: id = msg!(class(c"NSURL"), c"fileURLWithPath:", nsstring(&cpath));
    let urls: id = msg!(class(c"NSArray"), c"arrayWithObject:", url);
    let workspace: id = msg!(class(c"NSWorkspace"), c"sharedWorkspace");
    let _: () = msg!(workspace, c"activateFileViewerSelectingURLs:", urls);
}

pub(crate) fn stow(file: &str, bytes: Vec<u8>) {
    let path = own_path("Audio").join(file);
    if let Some(directory) = path.parent() {
        let _ = std::fs::create_dir_all(directory);
    }
    write_later(path.to_string_lossy().into_owned(), bytes, Told::Stow);
}

pub(crate) fn sweep() {
    for entry in std::fs::read_dir(own_path("Audio")).into_iter().flatten().flatten() {
        let path = entry.path();
        if !with_app(|app| app.holds(&path.to_string_lossy())) {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub(crate) fn write_later(path: String, bytes: Vec<u8>, told: Told) {
    if let Some(url) = regain_access(&path) {
        let url: id = msg!(url, c"retain");
        WRITING.with(|writing| writing.borrow_mut().push((path.clone(), url)));
    }
    let mut writer = locked(&WRITER);
    if writer.is_none() {
        let (sender, jobs) = channel::<Write>();
        let spawned = std::thread::Builder::new().name(String::from("bp-write")).spawn(move || {
            for (path, bytes, told) in jobs {
                let written = write_replacing(Path::new(&path), &bytes, matches!(told, Told::Save(_)));
                locked(&WRITTEN).push((path, written, told));
            }
        });
        *writer = spawned.ok().map(|handle| (sender, handle));
    }
    if let Some((sender, _)) = writer.as_ref() {
        let _ = sender.send((path, bytes, told));
    }
}

pub(crate) fn finish_writes() {
    let writer = locked(&WRITER).take();
    if let Some((sender, handle)) = writer {
        drop(sender);
        let _ = handle.join();
    }
}

pub(crate) fn settle_writes() -> bool {
    let done = std::mem::take(&mut *locked(&WRITTEN));
    let any = !done.is_empty();
    for (path, written, told) in done {
        let held = WRITING.with(|writing| {
            let mut writing = writing.borrow_mut();
            let at = writing.iter().position(|(held, _)| *held == path)?;
            Some(writing.remove(at).1)
        });
        if let Some(url) = held {
            let _: () = msg!(url, c"stopAccessingSecurityScopedResource");
            let _: () = msg!(url, c"release");
        }
        match told {
            Told::Failure if !written => with_app(|app| app.not_saved()),
            Told::Session if !written => with_app(|app| app.not_remembered()),
            Told::Nobody | Told::Failure | Told::Session => {}
            Told::Save(save) if written => {
                keep_access(&path);
                with_app(|app| app.saved(path, save));
            }
            Told::Save(save) => with_app(|app| app.unwritten(save)),
            Told::Export => with_app(|app| app.exported(path, written)),
            Told::Stow => with_app(|app| app.stowed(path, written)),
        }
    }
    any
}

pub(crate) fn open(path: String, at: Option<(i32, i32)>) -> bool {
    hold_access(&path);
    let text = std::fs::read_to_string(&path).ok();
    match with_app(|app| app.opened(path.clone(), text.as_deref(), at)) {
        Opened::Project => {
            keep_access(&path);
            hold_folders();
            true
        }
        Opened::Preset => true,
        Opened::Refused => false,
    }
}

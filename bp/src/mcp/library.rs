use super::tools::{flag_arg, text_arg};
use super::{harmony, object, path, text};
use crate::browser::{self, Kind};
use crate::midi::Part;
use sdk::json::{self, Json};
use crate::project::codec::clip_to_json;
use crate::project::{Clip, DEFAULT_BPM, Document};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const MOST_LISTED: usize = 200;
const MOST_WALKED: usize = 100_000;
const MOST_CHUNKS: usize = 64;
const MOST_DOCUMENT_BYTES: u64 = 64 << 20;
const MOST_PEEK_BYTES: u64 = 256 << 10;
const SNIFF_BYTES: usize = 512;
const KINDS: [&str; 5] = ["folder", "wave", "midi", "preset", "project"];
const MISSING: &str = "missing; the user must re-add it in the browser";

pub struct Shelf {
    pub name: String,
    pub path: String,
    pub fixed: bool,
}

#[derive(Default)]
struct Walked {
    entries: Vec<Json>,
    matched: usize,
    visited: usize,
    stopped: Option<String>,
    unsearched: Vec<String>,
    folders: HashSet<PathBuf>,
}

struct Sift<'a> {
    words: Vec<String>,
    kind: Option<String>,
    hearts: &'a [String],
    stopped: &'a AtomicBool,
}

impl Sift<'_> {
    fn narrows(&self) -> bool {
        !self.words.is_empty() || self.kind.is_some()
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

fn sift<'a>(hearts: &'a [String], find: Option<&str>, kind: Option<&str>, stopped: &'a AtomicBool) -> Result<Sift<'a>, String> {
    let words = find.unwrap_or_default().split_whitespace().map(str::to_lowercase).collect();
    match kind.map(str::to_ascii_lowercase) {
        Some(asked) if !KINDS.contains(&asked.as_str()) => Err(format!("kind is one of {}; leave it out for every kind", KINDS.join(", "))),
        kind => Ok(Sift { words, kind, hearts, stopped }),
    }
}

impl Walked {
    fn keep(&mut self, name: &str, path: &str, kind: Kind, sift: &Sift, shown: impl FnOnce() -> String) {
        let mut fields = match kind {
            Kind::Folder => vec![("kind", text("folder"))],
            Kind::Wave => vec![("kind", text("wave"))],
            Kind::Midi => midi_described(path),
            Kind::Document => described(path),
        };
        let mut sought = name.to_lowercase();
        if let Some(description) = fields.iter().find(|(key, _)| *key == "description").and_then(|(_, value)| value.as_str()) {
            sought.push(' ');
            sought.push_str(&description.to_lowercase());
        }
        let kind_name = fields.first().and_then(|(_, kind)| kind.as_str()).unwrap_or_default();
        if !matched(&sought, &sift.words) || sift.kind.as_deref().is_some_and(|wanted| wanted != kind_name) {
            return;
        }
        self.matched += 1;
        if self.entries.len() >= MOST_LISTED {
            return;
        }
        fields.insert(1, ("path", Json::String(shown())));
        if kind == Kind::Folder {
            fields.push(("files", Json::Number(files_under(Path::new(path), sift) as f64)));
        } else if sift.hearts.iter().any(|heart| heart == path) {
            fields.push(("hearted", Json::Bool(true)));
        }
        self.entries.push(object(fields));
    }
}

pub(super) fn browsed(shelves: &[Shelf], hearts: &[String], args: &Json, stopped: &AtomicBool) -> Result<Json, String> {
    let sift = sift(hearts, text_arg(args, "find"), text_arg(args, "kind"), stopped)?;
    let path = text_arg(args, "path");
    if flag_arg(args, "hearted") { hearted(shelves, path, &sift) } else { listed(shelves, path, flag_arg(args, "deep"), flag_arg(args, "notes"), &sift) }
}

fn hearted(shelves: &[Shelf], path: Option<&str>, sift: &Sift) -> Result<Json, String> {
    let under = path.map(|asked| whole(shelves, asked)).transpose()?;
    let mut kept: Vec<&String> = sift.hearts.iter().filter(|heart| under.as_deref().is_none_or(|under| Path::new(heart.as_str()).starts_with(under))).collect();
    kept.sort();
    let mut walked = Walked::default();
    for heart in kept {
        if sift.stopped() {
            break;
        }
        let Some(kind) = std::fs::metadata(heart).ok().and_then(|metadata| browser::kind(heart, metadata.is_dir())).filter(|kind| *kind != Kind::Folder) else {
            continue;
        };
        let name = Path::new(heart.as_str()).file_name().and_then(|name| name.to_str()).unwrap_or(heart);
        walked.keep(name, heart, kind, sift, || labelled(shelves, heart));
    }
    Ok(listing(vec![("hearted", Json::Bool(true))], sift, walked))
}

fn labelled(shelves: &[Shelf], path: &str) -> String {
    let alone = |shelf: &&Shelf| shelves.iter().filter(|other| other.name.eq_ignore_ascii_case(&shelf.name)).count() == 1;
    let under = shelves.iter().filter(alone).find_map(|shelf| Path::new(path).strip_prefix(&shelf.path).ok().and_then(Path::to_str).map(|rest| (shelf, rest)));
    match under {
        Some((shelf, "")) => shelf.name.clone(),
        Some((shelf, rest)) => format!("{}/{rest}", shelf.name),
        None => String::from(path),
    }
}

pub(super) fn heartable(shelves: &[Shelf], path: &str) -> Result<(), String> {
    shelf_of(shelves, path, true)?;
    let metadata = std::fs::metadata(path).map_err(|_| format!("{path} is not there"))?;
    match browser::kind(path, metadata.is_dir()) {
        Some(Kind::Folder) => Err(format!("{path} is a folder; only files take a heart")),
        Some(_) => Ok(()),
        None => Err(format!("{path} is not a file the browser lists")),
    }
}

fn listed(shelves: &[Shelf], path: Option<&str>, deep: bool, notes: bool, sift: &Sift) -> Result<Json, String> {
    let Some(asked) = path else {
        if sift.narrows() {
            return Ok(found(shelves, sift));
        }
        let folders = shelves.iter().map(|shelf| {
            let mut fields = vec![("name", text(&shelf.name)), ("path", text(&shelf.path))];
            if Path::new(&shelf.path).is_dir() {
                fields.push(("files", Json::Number(files_under(Path::new(&shelf.path), sift) as f64)));
            } else {
                fields.push(("missing", Json::Bool(true)));
            }
            if shelf.fixed {
                fields.push(("factory", Json::Bool(true)));
            }
            object(fields)
        });
        return Ok(object(vec![("folders", Json::Array(folders.collect()))]));
    };
    let (path, metadata) = standing(shelves, asked)?;
    if metadata.is_dir() {
        return Ok(folder(asked, &path, deep || sift.narrows(), sift));
    }
    file(asked, &path, metadata.len(), notes)
}

fn standing(shelves: &[Shelf], asked: &str) -> Result<(String, std::fs::Metadata), String> {
    let path = whole(shelves, asked)?;
    shelf_of(shelves, &path, true)?;
    let metadata = std::fs::metadata(&path).map_err(|_| format!("{asked} is not there, or cannot be read"))?;
    Ok((path, metadata))
}

pub(super) fn fetched(shelves: &[Shelf], asked: &str) -> Result<Json, String> {
    let asked = asked.trim();
    if asked.is_empty() {
        return Ok(object(Vec::new()));
    }
    let (path, metadata) = standing(shelves, asked)?;
    if metadata.is_dir() {
        return Ok(object(vec![("path", text(asked))]));
    }
    file(asked, &path, metadata.len(), true)
}

pub fn whole(shelves: &[Shelf], path: &str) -> Result<String, String> {
    let path = path.trim().trim_end_matches('/');
    if Path::new(path).is_absolute() {
        return Ok(String::from(path));
    }
    let (first, rest) = path.split_once('/').unwrap_or((path, ""));
    let named: Vec<&Shelf> = shelves.iter().filter(|shelf| shelf.name.eq_ignore_ascii_case(first)).collect();
    match named.as_slice() {
        [shelf] if rest.is_empty() => Ok(shelf.path.clone()),
        [shelf] => Ok(format!("{}/{rest}", shelf.path.trim_end_matches('/'))),
        [] => Err(format!("{path} is neither absolute nor under a browser folder; library with no path lists them")),
        many => Err(format!("{first} names {} browser folders, {}; give the whole path", many.len(), many.iter().map(|shelf| shelf.path.as_str()).collect::<Vec<_>>().join(" and "))),
    }
}

fn folder(asked: &str, path: &str, deep: bool, sift: &Sift) -> Json {
    let mut walked = Walked::default();
    let root = Path::new(path);
    walk(root, root, "", deep, sift, &mut walked);
    let fields = vec![("path", text(asked))];
    listing(fields, sift, walked)
}

fn found(shelves: &[Shelf], sift: &Sift) -> Json {
    let mut walked = Walked::default();
    for (index, shelf) in shelves.iter().enumerate() {
        if walked.visited >= MOST_WALKED {
            walked.unsearched = shelves[index..].iter().map(|shelf| shelf.name.clone()).collect();
            break;
        }
        let root = Path::new(&shelf.path);
        walked.folders.clear();
        if shelves.iter().filter(|other| other.name.eq_ignore_ascii_case(&shelf.name)).count() == 1 {
            walk(root, root, &format!("{}/", shelf.name), true, sift, &mut walked);
        } else {
            walk(root, Path::new("/"), "/", true, sift, &mut walked);
        }
    }
    let mut fields = Vec::new();
    let missing: Vec<Json> = shelves.iter().filter(|shelf| !Path::new(&shelf.path).is_dir()).map(|shelf| text(&shelf.path)).collect();
    if !missing.is_empty() {
        fields.push(("missing", Json::Array(missing)));
    }
    listing(fields, sift, walked)
}

fn listing(mut fields: Vec<(&str, Json)>, sift: &Sift, mut walked: Walked) -> Json {
    if !sift.words.is_empty() {
        fields.push(("find", text(&sift.words.join(" "))));
    }
    if let Some(kind) = &sift.kind {
        fields.push(("kind", text(kind)));
    }
    let cut = fitted(&mut walked.entries);
    let shown = walked.entries.len();
    fields.push(("count", Json::Number(walked.matched as f64)));
    if cut {
        fields.push(("cut", text("Descriptions shortened; narrow path or find.")));
    }
    let unwalked = walked.stopped.take().map(|stopped| {
        let rest = if walked.unsearched.is_empty() { String::from("folders sorted after it went unsearched") } else { format!("unsearched: {}", walked.unsearched.join(", ")) };
        format!("Stopped at {MOST_WALKED} files, in {stopped}; {rest}. Give path to search one.")
    });
    fields.push(("entries", Json::Array(walked.entries)));
    if walked.matched > shown {
        fields.push(("truncated", Json::String(format!("{shown}/{} entries; narrow path or find.", walked.matched))));
    }
    if let Some(unwalked) = unwalked {
        fields.push(("unwalked", Json::String(unwalked)));
    }
    object(fields)
}

fn fitted(entries: &mut Vec<Json>) -> bool {
    let size = |entry: &Json| entry.write_at(2).len() + 6;
    if entries.iter().map(size).sum::<usize>() <= super::MOST_REPLY_BYTES {
        return false;
    }
    for entry in entries.iter_mut() {
        if let Json::Object(fields) = entry
            && let Some((_, Json::String(description))) = fields.iter_mut().find(|(key, _)| key == "description")
        {
            *description = opening(description);
        }
    }
    let mut total = 0;
    let kept = entries
        .iter()
        .take_while(|entry| {
            total += size(entry);
            total <= super::MOST_REPLY_BYTES
        })
        .count();
    entries.truncate(kept);
    true
}

fn opening(description: &str) -> String {
    let end = [": ", ". ", "; "].iter().filter_map(|mark| description.find(mark)).min().unwrap_or(description.len());
    String::from(description[..end].trim_end_matches('.'))
}

fn matched(sought: &str, words: &[String]) -> bool {
    words.iter().all(|word| sought.contains(word.as_str()))
}

fn walk(directory: &Path, root: &Path, prefix: &str, deep: bool, sift: &Sift, out: &mut Walked) {
    if !std::fs::canonicalize(directory).is_ok_and(|real| out.folders.insert(real)) {
        return;
    }
    let Ok(read) = std::fs::read_dir(directory) else {
        return;
    };
    let mut found: Vec<(String, String, Kind, bool)> = read
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str().map(String::from)?;
            let path = entry.path().to_str().map(String::from)?;
            let kind = entry.file_type().ok()?;
            let kind = if kind.is_symlink() { std::fs::metadata(entry.path()).ok()?.file_type() } else { kind };
            let folder = kind.is_dir();
            if !folder && !kind.is_file() {
                return None;
            }
            let kind = browser::kind(&path, folder)?;
            Some((name, path, kind, folder))
        })
        .collect();
    found.sort_by_cached_key(|(name, _, _, folder)| (!folder, name.to_lowercase()));
    for (name, path, kind, folder) in found {
        if sift.stopped() {
            return;
        }
        if out.visited >= MOST_WALKED {
            out.stopped.get_or_insert_with(|| format!("{prefix}{}", directory.strip_prefix(root).ok().and_then(Path::to_str).unwrap_or_default()).trim_end_matches('/').to_string());
            return;
        }
        out.visited += 1;
        out.keep(&name, &path, kind, sift, || {
            let relative = Path::new(&path).strip_prefix(root).ok().and_then(Path::to_str).unwrap_or(&path);
            format!("{prefix}{relative}")
        });
        if folder && deep {
            walk(Path::new(&path), root, prefix, deep, sift, out);
        }
    }
}

fn files_under(directory: &Path, sift: &Sift) -> usize {
    let mut count = 0;
    let mut walked = HashSet::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(directory) = pending.pop().filter(|_| !sift.stopped()) {
        if !std::fs::canonicalize(&directory).is_ok_and(|real| walked.insert(real)) {
            continue;
        }
        let Ok(read) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in read.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() || (kind.is_symlink() && entry.path().is_dir()) {
                pending.push(entry.path());
            } else if entry.path().to_str().is_some_and(|path| browser::kind(path, false).is_some()) {
                count += 1;
            }
            if count >= MOST_WALKED {
                return count;
            }
        }
    }
    count
}

fn midi_described(path: &str) -> Vec<(&'static str, Json)> {
    let mut fields = vec![("kind", text("midi"))];
    let small = std::fs::metadata(path).is_ok_and(|metadata| metadata.len() <= MOST_PEEK_BYTES);
    if let Some(description) = small.then(|| parts_of(path)).flatten().as_deref().and_then(|parts| harmony::described(parts, None)) {
        fields.push(("description", Json::String(description)));
    }
    fields
}

fn parts_of(path: &str) -> Option<Vec<Part>> {
    std::fs::read(path).ok().and_then(|bytes| crate::midi::parse(&bytes, DEFAULT_BPM))
}

fn described(path: &str) -> Vec<(&'static str, Json)> {
    let plain = |kind: &str| vec![("kind", text(kind))];
    let Ok(bytes) = std::fs::metadata(path).map(|metadata| metadata.len()) else {
        return plain("json");
    };
    let mut head = vec![0u8; SNIFF_BYTES.min(bytes as usize)];
    if std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut head)).is_err() {
        return plain("json");
    }
    let sniffed = String::from_utf8_lossy(&head).split_once("\"kind\"").and_then(|(_, rest)| rest.split('"').nth(1).map(String::from));
    if sniffed.as_deref() == Some("project") {
        return plain("project");
    }
    if bytes > MOST_PEEK_BYTES {
        return plain("json");
    }
    let Some(json) = std::fs::read_to_string(path).ok().and_then(|text| json::parse(&text)) else {
        return plain("json");
    };
    let Some(kind) = json.get("kind").and_then(Json::as_str).map(String::from) else {
        return plain("json");
    };
    if kind == "project" {
        return plain("project");
    }
    let mut fields = plain("preset");
    if !matches!(kind.as_str(), "rack" | "chords") {
        fields.push(("row", Json::String(kind)));
    }
    if let Some(description) = json.get("description").filter(|description| description.as_str().is_some()) {
        fields.push(("description", description.clone()));
    }
    fields.extend(contents(&json));
    fields
}

fn contents(preset: &Json) -> Vec<(&'static str, Json)> {
    let list = |key: &str| preset.get(key).and_then(Json::as_array).unwrap_or(&[]);
    let names = |key: &str, name: &str| list(key).iter().filter_map(|item| item.get(name)).filter_map(Json::as_str).collect::<Vec<_>>().join(", ");
    let mut fields = Vec::new();
    if !list("rack").is_empty() {
        fields.push(("rows", Json::String(names("rack", "kind"))));
    }
    if !list("lfos").is_empty() {
        fields.push(("lfos", Json::Number(list("lfos").len() as f64)));
    }
    if !list("palette").is_empty() {
        fields.push(("columns", Json::Number(list("palette").len() as f64)));
    }
    fields
}

fn file(asked: &str, path: &str, bytes: u64, notes: bool) -> Result<Json, String> {
    let mut fields = vec![("path", text(asked)), ("bytes", Json::Number(bytes as f64))];
    match browser::kind(path, false) {
        Some(Kind::Wave) => {
            fields.insert(1, ("kind", text("wave")));
            if let Some((rate, channels, seconds)) = wave_header(path) {
                fields.extend([("sampleRateHz", Json::Number(rate)), ("channels", Json::Number(channels)), ("seconds", Json::Number(seconds))]);
            }
        }
        Some(Kind::Midi) => {
            fields.insert(1, ("kind", text("midi")));
            if let Some(parts) = parts_of(path) {
                if let Some(description) = harmony::described(&parts, None) {
                    fields.push(("description", Json::String(description)));
                }
                let stem = Path::new(path).file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
                let mut parts = Json::Array(parts.iter().map(|part| clip_to_json(&Clip::midi(0, part.label(stem), part.notes.clone(), part.length))).collect());
                if !notes {
                    path::fold(&mut parts, "parts", Some(asked), true, false);
                }
                fields.push(("parts", parts));
            }
        }
        Some(Kind::Document) => {
            if bytes > MOST_DOCUMENT_BYTES {
                return Err(format!("{asked} is over 64 MB; for the open project, use read"));
            }
            let text_read = std::fs::read_to_string(path).map_err(|_| format!("{asked} would not read"))?;
            let Some(mut json) = json::parse(&text_read) else {
                return Err(format!("{asked} is not JSON"));
            };
            let kind = match Document::from_json(&json) {
                Some(Document::Project(_)) => "project",
                Some(Document::Preset(_)) => "preset",
                None => "json",
            };
            if let Json::Object(entries) = &mut json {
                entries.retain(|(key, _)| key != "waves" && key != "versions");
            }
            if !notes {
                path::fold(&mut json, "", Some(asked), true, false);
            }
            fields.insert(1, ("kind", text(kind)));
            fields.push(("document", json));
        }
        Some(Kind::Folder) | None => return Err(format!("{asked} is not a wave, MIDI file or JSON document")),
    }
    Ok(object(fields))
}

fn wave_header(path: &str) -> Option<(f64, f64, f64)> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff).ok()?;
    if riff[..4] != *b"RIFF" || riff[8..] != *b"WAVE" {
        return None;
    }
    let (mut format, mut data) = (None, None);
    for _ in 0..MOST_CHUNKS {
        if format.is_some() && data.is_some() {
            break;
        }
        let mut head = [0u8; 8];
        file.read_exact(&mut head).ok()?;
        let size = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
        let mut skip = i64::from(size) + i64::from(size & 1);
        if head[..4] == *b"fmt " && size >= 16 {
            let mut body = [0u8; 16];
            file.read_exact(&mut body).ok()?;
            format = Some((u16::from_le_bytes([body[2], body[3]]), u32::from_le_bytes([body[4], body[5], body[6], body[7]]), u16::from_le_bytes([body[14], body[15]])));
            skip -= 16;
        } else if head[..4] == *b"data" {
            data = Some(size);
        }
        file.seek(SeekFrom::Current(skip)).ok()?;
    }
    let ((channels, rate, bits), data) = (format?, data?);
    let frame = usize::from(channels) * usize::from(bits.max(8)) / 8;
    (frame > 0 && rate > 0).then(|| (f64::from(rate), f64::from(channels), f64::from(data) / frame as f64 / f64::from(rate)))
}

fn shelf_of<'a>(shelves: &'a [Shelf], path: &str, follow: bool) -> Result<(&'a Shelf, bool), String> {
    let given = Path::new(path);
    if !given.is_absolute() || given.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir)) {
        return Err(format!("{path} is not an absolute browser path; join entries to their listing's path"));
    }
    absent(shelves, path)?;
    let outside = || format!("{path} is outside the browser's folders; library with add true and path {} asks the user to allow it", if given.is_dir() { String::from(path) } else { parent(path) });
    let real = if follow { resolved(given) } else { given.parent().and_then(resolved).zip(given.file_name()).map(|(parent, name)| parent.join(name)) };
    let real = real.ok_or_else(outside)?;
    shelves.iter().find_map(|shelf| std::fs::canonicalize(&shelf.path).ok().filter(|root| real.starts_with(root)).map(|root| (shelf, root == real))).ok_or_else(outside)
}

fn absent(shelves: &[Shelf], path: &str) -> Result<(), String> {
    let under = |root: &str| path == root || path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'));
    match shelves.iter().find(|shelf| under(shelf.path.trim_end_matches('/')) && !Path::new(&shelf.path).is_dir()) {
        Some(shelf) => Err(format!("{} ({}) is {MISSING}", shelf.name, shelf.path)),
        None => Ok(()),
    }
}

fn resolved(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok().or_else(|| Some(resolved(path.parent()?)?.join(path.file_name()?)))
}

fn writable(shelves: &[Shelf], path: &str) -> Result<(), String> {
    let (shelf, root) = shelf_of(shelves, path, false)?;
    if shelf.fixed {
        return Err(format!("{} is a Factory folder, which stays as it is", shelf.name));
    }
    if root {
        return Err(format!("{path} is a browser folder and stays; its contents can move"));
    }
    Ok(())
}

pub(super) fn renamed(shelves: &[Shelf], path: &str, name: &str) -> Result<String, String> {
    writable(shelves, path)?;
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.starts_with('.') {
        return Err(String::from("name is a file name alone, not hidden, no slash; move changes folder"));
    }
    let to = Path::new(path).with_file_name(name);
    moved(path, &to)
}

pub(super) fn relocated(shelves: &[Shelf], path: &str, folder: &str) -> Result<String, String> {
    writable(shelves, path)?;
    let (shelf, _) = shelf_of(shelves, folder, true)?;
    if shelf.fixed {
        return Err(format!("{} is a Factory folder, which takes nothing", shelf.name));
    }
    let name = Path::new(path).file_name().ok_or_else(|| format!("{path} has no file name"))?;
    let to = Path::new(folder).join(name);
    clear(path, &to)?;
    std::fs::create_dir_all(folder).map_err(|error| format!("{folder} could not be made: {error}"))?;
    moved(path, &to)
}

fn clear(from: &str, to: &Path) -> Result<String, String> {
    let landing = to.to_str().map(String::from).ok_or_else(|| String::from("The new path is not text"))?;
    if !Path::new(from).exists() {
        return Err(format!("{from} is not there"));
    }
    if to.exists() && landing != from {
        return Err(format!("{landing} is already there"));
    }
    Ok(landing)
}

fn moved(from: &str, to: &Path) -> Result<String, String> {
    let landing = clear(from, to)?;
    std::fs::rename(from, to).map_err(|error| format!("{from} would not move: {error}"))?;
    Ok(landing)
}

pub(super) fn free(shelves: &[Shelf], path: &str) -> Result<(), String> {
    writable(shelves, path)?;
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(format!("{path} is already there; trash it first, or pick another name"));
    }
    Ok(())
}

pub(super) fn replaceable(shelves: &[Shelf], path: &str) -> Result<bool, String> {
    writable(shelves, path)?;
    match std::fs::symlink_metadata(path) {
        Err(_) => Ok(false),
        Ok(found) if found.is_file() && described(path).first().and_then(|(_, kind)| kind.as_str()) == Some("preset") => Ok(true),
        Ok(_) => Err(format!("{path} is there and is not a preset; overwrite replaces a preset alone")),
    }
}

pub(super) fn housed(path: &str) -> Result<Option<String>, String> {
    let folder = parent(path);
    if Path::new(&folder).is_dir() {
        return Ok(None);
    }
    let standing = std::iter::successors(Path::new(&folder).parent(), |above| above.parent()).find(|above| above.is_dir()).and_then(Path::to_str).map(String::from);
    std::fs::create_dir_all(&folder).map_err(|error| format!("{folder} could not be made: {error}"))?;
    Ok(standing)
}

pub(super) fn wave_in(shelves: &[Shelf], path: &str) -> Result<(), String> {
    if browser::kind(path, false) != Some(Kind::Wave) {
        return Err(format!("{path} is not a .wav file"));
    }
    shelf_of(shelves, path, true)?;
    if !Path::new(path).is_file() {
        return Err(format!("{path} is not there"));
    }
    Ok(())
}

pub(super) fn parent(path: &str) -> String {
    Path::new(path).parent().and_then(Path::to_str).map_or_else(|| String::from(path), String::from)
}

pub(super) fn trashable(shelves: &[Shelf], path: &str) -> Result<(), String> {
    writable(shelves, path)?;
    if !Path::new(path).exists() {
        return Err(format!("{path} is not there"));
    }
    Ok(())
}

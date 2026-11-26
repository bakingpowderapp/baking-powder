use super::{EXPORT, LIBRARY, LISTEN, MOST_REPLY_BYTES, Reply, Shelf, catalogue, counted, harmony, library, notes, object, path, write};
use crate::browser::{self, Kind};
use crate::flatten::Sent;
use sdk::json::{self, Json};
use sdk::schema::{EMPTY, TEXT};
use crate::project::{Clip, DEFAULT_BPM, Host as Owner, Keeping, Project, Row, SEND_COUNT, warp};
use std::sync::atomic::AtomicBool;

pub trait Host {
    fn document(&self) -> Json;
    fn manual(&self) -> Json;
    fn adopt(&mut self, document: &Json) -> Result<(), String>;
    fn follow(&mut self, _shifts: &[write::Shift]) {}
    fn connected(&mut self, client: String);
    fn import(&mut self, path: &str) -> Result<Json, String>;
    fn play_from(&mut self, beat: Option<f64>, until: Option<f64>) -> Result<String, String>;
    fn stop_playing(&mut self) -> String;
    fn step_back(&mut self) -> Option<String>;
    fn step_forward(&mut self) -> Option<String>;
    fn shelves(&self) -> Vec<Shelf>;
    fn relist(&mut self, folder: String);
    fn trash(&mut self, path: String);
    fn hearts(&self) -> Vec<String>;
    fn heart(&mut self, path: String, on: bool) -> bool;
    fn revisions(&mut self) -> &mut write::Revisions;
    fn project_path(&self) -> Option<String>;
    fn save(&mut self, path: String) -> String;
    fn save_copy(&mut self, path: String) -> String;
    fn render(&mut self, path: String, span: Option<(f64, f64)>) -> Result<String, String>;
    fn export_midi(&mut self, parts: &[(String, Vec<Json>)], bpm: Option<f64>, span: Option<(f64, f64)>, path: String) -> Result<String, String>;
    fn ask_folder(&mut self, at: Option<String>);
    fn sampled(&self, steps: &[String], beats: &[f64]) -> Result<Vec<f64>, String>;
    fn sent(&self, project: &Project) -> Sent;
    fn alike(&self, _clips: &[Clip], _like: usize) -> Vec<usize> {
        Vec::new()
    }
    fn heard_until(&self, _clip: &Clip) -> Option<f64> {
        None
    }
    fn keep(&mut self, _path: String, _text: String) {}
    fn export_clip(&mut self, track: usize, clip: usize, _span: Option<(f64, f64)>, _path: String) -> Result<String, String> {
        Err(format!("tracks/{track}/clips/{clip} cannot be written as a wave here"))
    }
}

const MOST_SPANS: usize = 64;
const MOST_CHANGES: usize = 64;
const FINE_PER_BEAT: f64 = 256.0;
const MOST_FINE: f64 = 65536.0;
const SAMPLES: usize = 32;

pub(super) fn text_arg<'a>(args: &'a Json, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Json::as_str).map(str::trim).filter(|text| !text.is_empty())
}

pub(super) fn flag_arg(args: &Json, name: &str) -> bool {
    args.get(name).and_then(Json::as_bool).unwrap_or(false)
}

pub fn serve(host: &mut dyn Host, tool: &str, args: &Json) -> Reply {
    let mut aside = None;
    let answered = match tool {
        "initialize" => {
            host.connected(client_name(args));
            Ok(String::new())
        }
        "read" => read(host, args).map(|(text, revision)| {
            aside = revision.map(|revision| format!("revision {revision}"));
            text
        }),
        "write" => write::written(host, args),
        "manual" => manual(host, args),
        "import" => imported(host, args),
        "play" => host.play_from(args.get("beat").and_then(Json::as_f64), args.get("untilBeat").and_then(Json::as_f64)),
        "stop" => Ok(host.stop_playing()),
        "undo" => match args.get("revision").and_then(Json::as_f64) {
            Some(revision) => write::reverted(host, revision.max(0.0) as u64),
            None => host.step_back().ok_or_else(|| String::from("Nothing to undo")),
        },
        "redo" => host.step_forward().ok_or_else(|| String::from("Nothing to redo")),
        LIBRARY => {
            return match browsing(host, args) {
                Ok(walk) => walk(&AtomicBool::new(false)),
                Err(reply) => reply,
            };
        }
        "heart" => hearted(host, args),
        "rename" => renamed(host, args),
        "move" => relocated(host, args),
        "trash" => trashed(host, args),
        "save" => saved(host, args),
        EXPORT => exported(host, args),
        LISTEN => Err(String::from("listen is answered by the app once its render ends")),
        _ => Err(format!("No tool named {tool}")),
    };
    match answered {
        Ok(text) => Reply { aside, ..Reply::ok(text) },
        Err(text) => Reply::refused(text),
    }
}

pub type Walk = Box<dyn FnOnce(&AtomicBool) -> Reply + Send>;

pub fn browsing(host: &mut dyn Host, args: &Json) -> Result<Walk, Reply> {
    if flag_arg(args, "add") {
        return Err(added(host, args).map_or_else(Reply::refused, Reply::ok));
    }
    let (shelves, hearts, args) = (host.shelves(), host.hearts(), args.clone());
    Ok(Box::new(move |stopped: &AtomicBool| library::browsed(&shelves, &hearts, &args, stopped).map_or_else(Reply::refused, |json| Reply::ok(json.write()))))
}

fn client_name(info: &Json) -> String {
    let name = text_arg(info, "name").unwrap_or("a client");
    match text_arg(info, "version") {
        Some(version) => format!("{name} {version}"),
        None => String::from(name),
    }
}

fn read(host: &mut dyn Host, args: &Json) -> Result<(String, Option<u64>), String> {
    let steps = path::steps(text_arg(args, "path").unwrap_or(""));
    if let Some(revision) = args.get("since").and_then(Json::as_f64) {
        return since(host, revision.max(0.0) as u64, &steps).map(|text| (text, None));
    }
    if let Some(span) = beats_arg(args, "fromBeat and untilBeat go together, untilBeat after fromBeat; or leave both out")? {
        return sampled(host, &steps, span).map(|text| (text, None));
    }
    let (mut document, revision) = match args.get("revision").and_then(Json::as_f64) {
        Some(asked) => {
            let revision = asked.max(0.0) as u64;
            (host.revisions().document_of(revision)?.clone(), revision)
        }
        None => {
            let document = host.document();
            let revision = host.revisions().mark(&document);
            (document, revision)
        }
    };
    if steps.first().is_none_or(|step| step == "tracks") {
        harmony::chorded(host, &mut document);
    }
    if steps.is_empty()
        && let Json::Object(fields) = &mut document
    {
        fields.insert(0, (String::from("revision"), Json::Number(revision as f64)));
    }
    let outline = flag_arg(args, "outline");
    let notes = flag_arg(args, "notes") || steps.iter().any(|step| step == "clips");
    if outline || !notes {
        path::fold(&mut document, "", None, !notes, outline);
    }
    let beside = (!steps.is_empty()).then_some(revision);
    if let Some(written) = path::locate(&document, &steps).map(Json::write) {
        if outline || notes || written.len() <= MOST_REPLY_BYTES {
            return Ok((written, beside));
        }
        path::fold(&mut document, "", None, true, true);
        let outlined = match path::locate(&document, &steps).cloned() {
            Some(Json::Object(mut fields)) => {
                fields.insert(0, (String::from("outlined"), Json::String(format!("{} bytes whole, past the {MOST_REPLY_BYTES} a reply holds; read a folded line's path for it whole", written.len()))));
                Json::Object(fields).write()
            }
            Some(other) => other.write(),
            None => written,
        };
        return Ok((outlined, beside));
    }
    let at = path::joined(&steps);
    let manual = host.manual();
    let line = path::manual_for(&manual, &document, &steps).ok_or_else(|| path::missing(&document, &steps))?;
    let mut fields = vec![("path", Json::String(at.clone())), ("stored", Json::Bool(false)), ("revision", Json::Number(revision as f64))];
    match line {
        Json::String(line) => {
            if let Some(default) = default_in(line) {
                fields.push(("default", default));
            }
            fields.push(("about", Json::String(line.clone())));
        }
        _ => fields.push(("about", Json::String(format!("not written; manual at {at} gives its shape")))),
    }
    Ok((object(fields).write(), None))
}

fn default_in(line: &str) -> Option<Json> {
    let (_, rest) = line.split_once("default ")?;
    let said = rest.split([':', ';']).next()?.trim();
    Some(match said {
        "" => return None,
        several if several.contains([',', '(']) => return None,
        "true" => Json::Bool(true),
        "false" => Json::Bool(false),
        EMPTY => Json::String(String::new()),
        word if line.starts_with(TEXT) => Json::String(String::from(word)),
        token => token.parse::<f64>().map_or_else(|_| Json::String(String::from(token)), Json::Number),
    })
}

fn sampled(host: &dyn Host, steps: &[String], (from, until): (f64, f64)) -> Result<String, String> {
    let fine = ((until - from) * FINE_PER_BEAT).ceil().clamp(1.0, MOST_FINE) as usize;
    let beats: Vec<f64> = (0..=fine).map(|index| from + (until - from) * index as f64 / fine as f64).collect();
    let values = host.sampled(steps, &beats)?;
    let (min, max) = values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), &value| (min.min(value), max.max(value)));
    let shown = SAMPLES.min(fine);
    let samples = (0..=shown).map(|index| index * fine / shown).map(|index| Json::Array(vec![Json::Number(json::tidy(beats[index])), Json::Number(json::tidy(values[index]))]));
    let mut fields = vec![("path", Json::String(path::joined(steps))), ("fromBeat", Json::Number(from)), ("untilBeat", Json::Number(until)), ("min", Json::Number(json::tidy(min))), ("max", Json::Number(json::tidy(max)))];
    fields.extend(super::aim::sources(&host.document(), steps, (from, until))?);
    fields.push(("samples", Json::Array(samples.collect())));
    Ok(object(fields).write())
}

fn since(host: &mut dyn Host, revision: u64, steps: &[String]) -> Result<String, String> {
    let now = host.document();
    let then = host.revisions().document_of(revision)?.clone();
    let changed = path::song_changes(&then, &now);
    let marked = host.revisions().mark(&now);
    let changes: Vec<&path::Change> = changed.iter().filter(|change| change.at.starts_with(steps) || steps.starts_with(&change.at)).collect();
    let entries: Vec<(String, Json, Json)> = changes
        .iter()
        .take(MOST_CHANGES)
        .map(|change| {
            let at = path::joined(&change.at);
            let was = match &change.before {
                None => Json::String(String::from("nothing")),
                Some(scalar @ (Json::Number(_) | Json::Bool(_) | Json::String(_) | Json::Null)) => scalar.clone(),
                Some(other) => Json::String(path::short(other)),
            };
            let value = match path::locate(&now, &change.at).cloned() {
                Some(mut value) => {
                    path::fold(&mut value, &at, None, true, true);
                    value
                }
                None => Json::String(String::from("removed")),
            };
            (at, was, value)
        })
        .collect();
    let reply = |shortened: bool| {
        let listed = entries.iter().map(|(at, was, value)| object(vec![("path", Json::String(at.clone())), ("was", was.clone()), ("now", if shortened { Json::String(path::short(value)) } else { value.clone() })])).collect();
        let mut fields = vec![("since", Json::Number(revision as f64)), ("now", Json::Number(marked as f64)), ("count", Json::Number(changes.len() as f64))];
        if changes.len() > MOST_CHANGES {
            fields.push(("truncated", Json::String(format!("{MOST_CHANGES} of {} changes; pass path to narrow", changes.len()))));
        }
        fields.push(("changes", Json::Array(listed)));
        object(fields).write()
    };
    let whole = reply(false);
    Ok(if whole.len() <= MOST_REPLY_BYTES { whole } else { reply(true) })
}

pub(super) fn import(host: &mut dyn Host, path: &str) -> Result<Json, String> {
    let path = library::whole(&host.shelves(), path)?;
    library::wave_in(&host.shelves(), &path)?;
    host.import(&path)
}

fn shelved(host: &dyn Host, args: &Json, name: &str) -> Result<Option<String>, String> {
    text_arg(args, name).map(|path| library::whole(&host.shelves(), path)).transpose()
}

fn imported(host: &mut dyn Host, args: &Json) -> Result<String, String> {
    let path = text_arg(args, "path").ok_or("path is needed: a .wav in a folder library lists")?;
    let wave = import(host, path)?;
    let bpm = host.document().get("bpm").and_then(Json::as_f64).unwrap_or(DEFAULT_BPM);
    let seconds = wave.get("seconds").and_then(Json::as_f64).unwrap_or(0.0);
    let stem = std::path::Path::new(path).file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
    let tempo = warp::tempo_of(stem, seconds, bpm);
    let length = json::tidy(seconds * tempo.unwrap_or(bpm) / 60.0);
    let name = wave.get("name").cloned().unwrap_or(Json::Null);
    let mut played = vec![("name", name)];
    if let Some(tempo) = tempo {
        played.extend([("warp", Json::Bool(true)), ("warpBpm", Json::Number(tempo))]);
    }
    let clip = object(vec![("startBeat", Json::Number(0.0)), ("lengthBeats", Json::Number(length)), ("wave", object(played))]);
    let Json::Object(mut fields) = wave else {
        return Err(format!("{path} would not describe itself"));
    };
    fields.push((String::from("lengthBeats"), Json::Number(length)));
    fields.push((String::from("clip"), clip));
    Ok(Json::Object(fields).write())
}

fn manual(host: &dyn Host, args: &Json) -> Result<String, String> {
    let manual = host.manual();
    let words: Vec<String> = text_arg(args, "find").unwrap_or_default().split_whitespace().map(str::to_lowercase).collect();
    if text_arg(args, "path") == Some("guide") {
        let lines: Vec<&str> = catalogue::GUIDE.lines().filter(|line| words.iter().all(|word| line.to_lowercase().contains(word.as_str()))).collect();
        return if lines.is_empty() { Err(format!("No guide line holds {}; leave find out for the whole guide", words.join(" "))) } else { Ok(lines.join("\n")) };
    }
    let steps = path::steps(text_arg(args, "path").unwrap_or(""));
    let row = text_arg(args, "row").map(str::to_ascii_lowercase);
    let found = match &row {
        Some(row) => {
            let kind = kind_named(&manual, row).ok_or_else(|| format!("No row kind named {row}; manual at tracks/1/rack lists them"))?;
            path::locate(kind, &steps).cloned().ok_or_else(|| format!("In the {row} row's manual, {}", path::missing(kind, &steps)))?
        }
        None => {
            let mut found = path::manual_for(&manual, &host.document(), &steps).ok_or_else(|| format!("No manual at {}: no such path; for a rack row the song lacks, pass row", path::joined(&steps)))?.clone();
            path::fold_shapes(&mut found);
            found
        }
    };
    if words.is_empty() {
        return Ok(sections(found, row.as_deref(), &steps).write());
    }
    let mut sifted = Vec::new();
    sift(&found, &words, &mut Vec::new(), &mut sifted);
    if sifted.is_empty() {
        let rows = if row.is_none() { "; a rack row's keys are searched with row or a row's path" } else { "" };
        return Err(format!("No key or line here holds {}{rows}; leave find out for the whole", words.join(" ")));
    }
    let (matched, mut total) = (sifted.len(), 0);
    let kept = sifted
        .iter()
        .take_while(|(key, value)| {
            total += key.len() + value.write_at(1).len() + 6;
            total <= MOST_REPLY_BYTES
        })
        .count();
    sifted.truncate(kept.max(1));
    if sifted.len() < matched {
        sifted.push((String::from("truncated"), Json::String(format!("{} matching; add a word to find, or narrow path or row", counted(matched - sifted.len(), "more key")))));
    }
    Ok(Json::Object(sifted).write())
}

fn sections(found: Json, row: Option<&str>, steps: &[String]) -> Json {
    let Json::Object(mut fields) = found else {
        return found;
    };
    let size = |value: &Json| value.write_at(1).len();
    let mut total: usize = fields.iter().map(|(key, value)| key.len() + size(value) + 6).sum();
    let mut largest: Vec<usize> = (0..fields.len()).filter(|&index| matches!(fields[index].1, Json::Object(_) | Json::Array(_))).collect();
    largest.sort_by_cached_key(|&index| std::cmp::Reverse(size(&fields[index].1)));
    for index in largest {
        if total <= MOST_REPLY_BYTES {
            break;
        }
        let (key, value) = &fields[index];
        let (before, line) = (size(value), section(key, value, row, steps));
        total = total + line.len() + 2 - before;
        fields[index].1 = Json::String(line);
    }
    Json::Object(fields)
}

fn section(key: &str, value: &Json, row: Option<&str>, steps: &[String]) -> String {
    let mut at = steps.to_vec();
    at.push(String::from(key));
    let whole = if matches!(value, Json::Object(inner) if inner.len() == 1) { "it" } else { "them" };
    let call = match row {
        Some(row) => format!("manual with row {row} and path {} gives {whole} whole", path::joined(&at)),
        None => format!("manual with path {} gives {whole} whole", path::joined(&at)),
    };
    match value {
        Json::Object(inner) => format!("{}, {}: {call}", counted(inner.len(), "key"), inner.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ")),
        Json::Array(items) => format!("{}: {call}", items.first().and_then(Json::as_str).unwrap_or("a list")),
        _ => call,
    }
}

fn sift(json: &Json, words: &[String], at: &mut Vec<String>, out: &mut Vec<(String, Json)>) {
    let holds = |text: &str| {
        let lower = text.to_lowercase();
        words.iter().all(|word| lower.contains(word.as_str()))
    };
    match json {
        Json::Object(fields) => {
            for (key, value) in fields {
                at.push(key.clone());
                if holds(key) {
                    out.push((path::joined(at), value.clone()));
                } else {
                    sift(value, words, at, out);
                }
                at.pop();
            }
        }
        Json::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                at.push(index.to_string());
                sift(item, words, at, out);
                at.pop();
            }
        }
        Json::String(line) if holds(line) => out.push((path::joined(at), json.clone())),
        _ => {}
    }
}

fn kind_named<'a>(json: &'a Json, wanted: &str) -> Option<&'a Json> {
    match json {
        Json::Object(fields) => {
            if json.get("kind").and_then(Json::as_str) == Some(wanted) {
                return Some(json);
            }
            fields.iter().find_map(|(_, value)| kind_named(value, wanted))
        }
        Json::Array(items) => items.iter().find_map(|item| kind_named(item, wanted)),
        _ => None,
    }
}

fn added(host: &mut dyn Host, args: &Json) -> Result<String, String> {
    let folder = shelved(host, args, "path")?;
    host.ask_folder(folder.clone());
    Ok(match folder {
        Some(folder) => format!("Asked the user to allow {folder} in the app; library lists it once picked"),
        None => String::from("Asked the user to pick a folder in the app; library lists it once picked"),
    })
}

fn renamed(host: &mut dyn Host, args: &Json) -> Result<String, String> {
    let (path, name) = (shelved(host, args, "path")?.ok_or("path is needed")?, text_arg(args, "name").ok_or("name is needed")?);
    let landed = library::renamed(&host.shelves(), &path, name)?;
    host.relist(library::parent(&path));
    Ok(format!("Renamed to {landed}"))
}

fn relocated(host: &mut dyn Host, args: &Json) -> Result<String, String> {
    let (path, to) = (shelved(host, args, "path")?.ok_or("path is needed")?, shelved(host, args, "to")?.ok_or("to is needed")?);
    let landed = library::relocated(&host.shelves(), &path, &to)?;
    host.relist(library::parent(&path));
    host.relist(to);
    Ok(format!("Moved to {landed}"))
}

fn hearted(host: &mut dyn Host, args: &Json) -> Result<String, String> {
    let path = shelved(host, args, "path")?.ok_or("path is needed")?;
    let on = args.get("hearted").and_then(Json::as_bool).unwrap_or(true);
    if on {
        library::heartable(&host.shelves(), &path)?;
    }
    let changed = host.heart(path.clone(), on);
    host.relist(library::parent(&path));
    Ok(match (changed, on) {
        (true, true) => format!("Hearted {path}"),
        (true, false) => format!("Took the heart off {path}"),
        (false, true) => format!("{path} was hearted already"),
        (false, false) => format!("{path} had no heart"),
    })
}

fn trashed(host: &mut dyn Host, args: &Json) -> Result<String, String> {
    let path = shelved(host, args, "path")?.ok_or("path is needed")?;
    library::trashable(&host.shelves(), &path)?;
    host.trash(path.clone());
    host.relist(library::parent(&path));
    Ok(format!("Moved {path} to the trash"))
}

const MOST_WORDS: usize = 12;

fn preset_saved(host: &mut dyn Host, args: &Json, at: &str) -> Result<String, String> {
    let path = shelved(host, args, "path")?.ok_or("preset needs path, a new .json under a browser folder")?;
    if browser::kind(&path, false) != Some(Kind::Document) {
        return Err(format!("{path} is not a .json file"));
    }
    if flag_arg(args, "copy") {
        return Err(String::from("preset saves a row or a rack and copy the whole project; pass one of them"));
    }
    let description = text_arg(args, "description").ok_or("description is needed: twelve words at most saying what the sound is, never naming gear, a brand or how it was made")?;
    let words = description.split_whitespace().count();
    if words > MOST_WORDS {
        return Err(format!("description is twelve words at most, and this one has {words}"));
    }
    let project = Project::from_json(&host.document()).ok_or("The document would not read as a project")?;
    let (steps, effects) = (path::steps(at), flag_arg(args, "effects"));
    let numbered = |step: &String, most: usize| step.parse::<usize>().ok().filter(|index| *index < most);
    let (owner, row) = match steps.as_slice() {
        [master, rest @ ..] if master == "master" => (Some(Owner::Master), rest),
        [tracks, track, rest @ ..] if tracks == "tracks" => (numbered(track, project.tracks.len()).map(Owner::Track), rest),
        [returns, send, rest @ ..] if returns == "returns" => (numbered(send, SEND_COUNT).map(Owner::Send), rest),
        _ => (None, &[][..]),
    };
    let wrong = || format!("preset is a path in the song: a rack row like tracks/0/rack/2 for that row, or tracks/0, returns/0 or master for the whole rack; nothing to save is at {at}");
    let owner = owner.ok_or_else(wrong)?;
    let rack = project.rack(owner);
    let (keeping, what) = match row {
        [] if rack.is_empty() => return Err(format!("{at} has no rows to save")),
        [] if effects && !rack.iter().any(Row::is_effect) => return Err(format!("{at} has no effects to save")),
        [] if effects => (Keeping::Effects, format!("the effects of {at}")),
        [] => (Keeping::Rack, format!("the rack of {at}")),
        [list, _] if list == "rack" && effects => return Err(format!("effects goes with a track's own path; {at} is one row, saved alone")),
        [list, index] if list == "rack" => {
            let index = numbered(index, rack.len()).ok_or_else(|| format!("No row at {at}: the rack has {}", counted(rack.len(), "row")))?;
            (Keeping::Row(index), format!("the {} row at {at}", rack[index].label().to_lowercase()))
        }
        _ => return Err(wrong()),
    };
    let preset = project.preset_of(owner, keeping).ok_or_else(wrong)?;
    let Json::Object(mut fields) = preset.to_json() else {
        return Err(wrong());
    };
    fields.insert(1.min(fields.len()), (String::from("description"), Json::String(String::from(description))));
    let replaced = if flag_arg(args, "overwrite") { library::replaceable(&host.shelves(), &path)? } else { library::free(&host.shelves(), &path).map(|()| false)? };
    housed(host, &path)?;
    host.keep(path.clone(), Json::Object(fields).write());
    host.relist(library::parent(&path));
    Ok(format!("Writing {what} as a preset to {path}{}; the song stays as it is", if replaced { ", in place of the preset there" } else { "" }))
}

fn saved(host: &mut dyn Host, args: &Json) -> Result<String, String> {
    if let Some(at) = text_arg(args, "preset") {
        return preset_saved(host, args, at);
    }
    let (own, copy) = (host.project_path(), flag_arg(args, "copy"));
    let path = match (shelved(host, args, "path")?, copy) {
        (Some(path), _) => path,
        (None, true) => return Err(String::from("copy needs path, a new .json in a browser folder")),
        (None, false) => own.clone().ok_or("No project file yet; pass path, a new .json in a browser folder")?,
    };
    if !copy && own.as_deref() == Some(path.as_str()) {
        return Ok(host.save(path));
    }
    if browser::kind(&path, false) != Some(Kind::Document) {
        return Err(format!("{path} is not a .json file"));
    }
    library::free(&host.shelves(), &path)?;
    housed(host, &path)?;
    Ok(if copy { host.save_copy(path) } else { host.save(path) })
}

pub fn housed(host: &mut dyn Host, path: &str) -> Result<(), String> {
    if let Some(folder) = library::housed(path)? {
        host.relist(folder);
    }
    Ok(())
}

fn exported(host: &mut dyn Host, args: &Json) -> Result<String, String> {
    let path = shelved(host, args, "path")?.ok_or("path is needed: a new .wav or .mid")?;
    let span = beats_arg(args, "fromBeat and untilBeat go together, untilBeat after fromBeat; or leave both out")?;
    let clip = args.get("clip").filter(|clip| !matches!(clip, Json::Null));
    match (browser::kind(&path, false), clip) {
        (Some(Kind::Wave), None) => {
            library::free(&host.shelves(), &path)?;
            host.render(path, span)
        }
        (Some(Kind::Wave), Some(clip)) => {
            let (track, index) = clip.as_str().and_then(clip_at).ok_or("clip with a .wav path is one audio clip's path like tracks/1/clips/0; a .wav without clip renders the song, and a .mid takes the rest")?;
            library::free(&host.shelves(), &path)?;
            host.export_clip(track, index, span, path)
        }
        (Some(Kind::Midi), clip) => {
            let parts = parts_given(host, clip)?;
            library::free(&host.shelves(), &path)?;
            host.export_midi(&parts, args.get("bpm").and_then(Json::as_f64), span, path)
        }
        _ => Err(String::from("path is a new .wav or .mid")),
    }
}

fn clip_at(at: &str) -> Option<(usize, usize)> {
    match path::steps(at.trim()).as_slice() {
        [tracks, track, clips, index] if tracks == "tracks" && clips == "clips" => track.parse().ok().zip(index.parse().ok()),
        _ => None,
    }
}

fn parts_given(host: &dyn Host, clip: Option<&Json>) -> Result<Vec<(String, Vec<Json>)>, String> {
    let document = host.document();
    match clip {
        None => {
            let tracks = document.get("tracks").and_then(Json::as_array).ok_or("The song has no tracks")?;
            Ok(tracks.iter().enumerate().filter_map(|(index, track)| {
                let clips = midi_clips(track);
                (!clips.is_empty()).then(|| (track_name(track, index), clips))
            }).collect())
        }
        Some(Json::Array(items)) => items.iter().map(|item| part_given(host, &document, item)).collect(),
        Some(clip) => Ok(vec![part_given(host, &document, clip)?]),
    }
}

fn part_given(host: &dyn Host, document: &Json, clip: &Json) -> Result<(String, Vec<Json>), String> {
    match clip {
        Json::String(at) => {
            let at = at.trim();
            let steps = path::steps(at);
            let numbered = |step: &String| step.parse::<usize>().ok();
            match steps.as_slice() {
                [tracks, track, clips, index] if tracks == "tracks" && clips == "clips" && numbered(track).is_some() && numbered(index).is_some() => {
                    let clip = path::locate(document, &steps).cloned().ok_or_else(|| format!("No clip at {at}"))?;
                    Ok((clip_name(&clip), vec![clip]))
                }
                [tracks, track] if tracks == "tracks" && numbered(track).is_some() => {
                    let found = path::locate(document, &steps).ok_or_else(|| format!("No track at {at}"))?;
                    let clips = midi_clips(found);
                    if clips.is_empty() {
                        return Err(format!("{at} has no MIDI clips"));
                    }
                    Ok((track_name(found, numbered(track).unwrap_or(0)), clips))
                }
                _ => Err(format!("{at} is not a clip path like tracks/0/clips/2 or a track path like tracks/0")),
            }
        }
        Json::Object(_) => {
            let shelves = host.shelves();
            let fetch = |file: &str| library::fetched(&shelves, file).ok();
            let mut clip = clip.clone();
            path::unfold(&mut clip, None, document, &fetch)?;
            notes::patterned(&mut clip, &[String::from("clip")], document, &mut Vec::new())?;
            Ok((clip_name(&clip), vec![clip]))
        }
        _ => Err(String::from("clip is a clip path like tracks/0/clips/2, a track path like tracks/0, a clip object with notes, or a list of these")),
    }
}

fn midi_clips(track: &Json) -> Vec<Json> {
    track.get("clips").and_then(Json::as_array).unwrap_or(&[]).iter().filter(|clip| clip.get("notes").is_some()).cloned().collect()
}

fn track_name(track: &Json, index: usize) -> String {
    track.get("name").and_then(Json::as_str).filter(|name| !name.trim().is_empty()).map_or_else(|| format!("tracks/{index}"), String::from)
}

fn clip_name(clip: &Json) -> String {
    String::from(clip.get("name").and_then(Json::as_str).unwrap_or(""))
}

pub struct Listening {
    pub span: Option<(f64, f64)>,
    pub track: Option<usize>,
    pub alone: Option<usize>,
    pub spans: Vec<(f64, f64)>,
    pub path: Option<String>,
    pub difference: Option<String>,
    pub against: Option<(u64, Json)>,
    pub revision: u64,
}

pub fn listening(host: &mut dyn Host, args: &Json) -> Result<Listening, String> {
    let span = beats_arg(args, "fromBeat and untilBeat go together, untilBeat after fromBeat; or leave both out")?;
    let track = index_arg(args, "track", usize::MAX, "track is a whole number from 0")?;
    let alone = index_arg(args, "return", SEND_COUNT, &format!("return is a whole number from 0 to {}", SEND_COUNT - 1))?;
    if track.is_some() && alone.is_some() {
        return Err(String::from("track and return each solo one thing; pass one of them"));
    }
    let spans = spans_arg(args)?;
    let against = match args.get("against").filter(|revision| !matches!(revision, Json::Null)) {
        None => None,
        Some(revision) => {
            let revision = revision.as_f64().filter(|revision| *revision >= 0.0 && revision.fract() == 0.0).ok_or("against is a revision number")? as u64;
            Some((revision, host.revisions().document_of(revision)?.clone()))
        }
    };
    let (path, difference) = (shelved(host, args, "path")?, shelved(host, args, "difference")?);
    if difference.is_some() && against.is_none() {
        return Err(String::from("difference keeps this render less a revision's; pass against, the revision to take away"));
    }
    if path.is_some() && difference.is_some() {
        return Err(String::from("path keeps the render and difference what changed since a revision; a listen keeps one file, so pass one"));
    }
    for kept in path.iter().chain(&difference) {
        if browser::kind(kept, false) != Some(Kind::Wave) {
            return Err(format!("{kept} is not a .wav file"));
        }
        library::free(&host.shelves(), kept)?;
    }
    let document = host.document();
    let revision = host.revisions().mark(&document);
    Ok(Listening { span, track, alone, spans, path, difference, against, revision })
}

fn index_arg(args: &Json, name: &str, most: usize, shape: &str) -> Result<Option<usize>, String> {
    match args.get(name).filter(|value| !matches!(value, Json::Null)) {
        None => Ok(None),
        Some(value) => value.as_f64().filter(|index| *index >= 0.0 && index.fract() == 0.0 && *index < most as f64).map(|index| Some(index as usize)).ok_or_else(|| String::from(shape)),
    }
}

fn spans_arg(args: &Json) -> Result<Vec<(f64, f64)>, String> {
    let shape = "spans is a list of [fromBeat, untilBeat] pairs, untilBeat after fromBeat";
    let pairs = match args.get("spans") {
        None | Some(Json::Null) => return Ok(Vec::new()),
        Some(spans) => spans.as_array().ok_or(shape)?,
    };
    if pairs.len() > MOST_SPANS {
        return Err(format!("spans holds {} pairs; {MOST_SPANS} at most", pairs.len()));
    }
    pairs
        .iter()
        .map(|pair| match pair.as_array() {
            Some([from, until]) => match (from.as_f64(), until.as_f64()) {
                (Some(from), Some(until)) if from >= 0.0 && until > from => Ok((from, until)),
                _ => Err(String::from(shape)),
            },
            _ => Err(String::from(shape)),
        })
        .collect()
}

fn beats_arg(args: &Json, unpaired: &str) -> Result<Option<(f64, f64)>, String> {
    match (args.get("fromBeat").and_then(Json::as_f64), args.get("untilBeat").and_then(Json::as_f64)) {
        (Some(from), Some(until)) if until > from => Ok(Some((from, until))),
        (None, None) => Ok(None),
        _ => Err(String::from(unpaired)),
    }
}

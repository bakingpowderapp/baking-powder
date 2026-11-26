use super::tools::{Host, flag_arg, import, text_arg};
use super::{arrange, counted, harmony, library, notes, path, presets};
use crate::arp::Arp;
use sdk::json::{Json, tidy};
use crate::project::codec::{clip_from_json, fade_to_json, fades_from_json};
use crate::project::{Clip, Note, Project, Slope, SongNote, beats, ticks};
use std::borrow::Cow;
use std::collections::HashMap;

const MOST_REVISIONS: usize = 32;
const MOST_PREVIEWED: usize = 40;
const MOST_PLAYED: usize = 8;
const VALUE: &str = "value";
const IMPORTING: &str = "importing ";
const OPERATIONS: [&str; 18] = [VALUE, "scale", "transpose", "fit", "insert", "remove", "copy", "cut", "join", "duplicate", "move", "match", "flatten", "quantize", "double", "halve", "reverse", "chords"];

#[derive(Default)]
pub struct Revisions {
    last: u64,
    kept: Vec<Revision>,
}

struct Revision {
    id: u64,
    after: Json,
    changes: Vec<path::Change>,
}

type Sounded = (i32, i32, Note);

pub(super) struct Edit {
    pub(super) steps: Vec<String>,
    pub(super) sent: Json,
    pub(super) did: Option<String>,
    pub(super) arranged: bool,
    pub(super) said: Option<Json>,
    pub(super) shifts: Vec<Shift>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shift {
    Insert(usize),
    Remove(usize),
    Move(usize, usize),
}

impl Revisions {
    pub fn clear(&mut self) {
        self.kept.clear();
    }

    fn record(&mut self, after: &Json, changes: Vec<path::Change>) -> Option<u64> {
        (!changes.is_empty()).then(|| self.keep(after, changes))
    }

    pub(super) fn mark(&mut self, now: &Json) -> u64 {
        match self.kept.last() {
            Some(last) if path::song_changes(&last.after, now).is_empty() => last.id,
            _ => self.keep(now, Vec::new()),
        }
    }

    fn keep(&mut self, after: &Json, changes: Vec<path::Change>) -> u64 {
        self.last += 1;
        self.kept.push(Revision { id: self.last, after: after.clone(), changes });
        self.kept.drain(..self.kept.len().saturating_sub(MOST_REVISIONS));
        self.last
    }

    fn kept(&self, id: u64) -> Result<&Revision, String> {
        self.kept.iter().find(|revision| revision.id == id).ok_or_else(|| format!("No revision {id} is kept, only the last {MOST_REVISIONS}; read for a new one"))
    }

    fn reverted(&self, id: u64, current: &Json) -> Result<Json, String> {
        let revision = self.kept(id)?;
        if revision.changes.is_empty() {
            return Err(format!("Revision {id} marks a read, not a write; nothing to undo"));
        }
        path::reverted(current, &revision.after, &revision.changes).map_err(|why| format!("Revision {id} stays: {why}"))
    }

    pub(super) fn document_of(&self, id: u64) -> Result<&Json, String> {
        self.kept(id).map(|revision| &revision.after)
    }
}

pub(super) fn written(host: &mut dyn Host, args: &Json) -> Result<String, String> {
    let current = host.document();
    let preview = flag_arg(args, "preview");
    let stale = match args.get("unchangedSince").and_then(Json::as_f64) {
        Some(revision) => {
            let revision = revision.max(0.0) as u64;
            let then = host.revisions().document_of(revision)?.clone();
            let moved = path::song_changes(&then, &current);
            Some((revision, then, moved))
        }
        None => None,
    };
    let mut next = current.clone();
    let (mut edits, mut typed, mut waves, mut patterns) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut sounds = presets::Report::default();
    let shelves = host.shelves();
    let manual = host.manual();
    let preset = |file: &str| library::fetched(&shelves, file);
    let fetch = |file: &str| preset(file).ok();
    let requests = asked(args)?;
    let count = requests.len();
    let mut again = Vec::new();
    let mut shifts = Some(Vec::new());
    for (asked, missing) in requests {
        for mut edit in edit(&*host, asked, &next, missing)? {
            match (edit.did.is_some(), edit.steps.as_slice()) {
                (true, _) => shifts.iter_mut().for_each(|known| known.extend(&edit.shifts)),
                (false, []) => shifts = None,
                (false, [tracks]) if tracks == "tracks" => shifts = None,
                (false, [tracks, index]) if tracks == "tracks" && edit.sent != Json::Null && path::locate(&next, &edit.steps).is_none() => {
                    let at = index.parse().ok();
                    shifts = shifts.zip(at).map(|(mut known, at)| {
                        known.push(Shift::Insert(at));
                        known
                    });
                }
                _ => {}
            }
            path::unfold(&mut edit.sent, edit.steps.last().map(String::as_str), &next, &fetch)?;
            harmony::unchorded(&mut edit.sent);
            typed.push(edit.sent.clone());
            let (steps, rewritten) = (edit.steps.clone(), sounds.rewritten.len());
            presets::poured(&mut edit.sent, &mut edit.steps, &mut next, &manual, &preset, &mut sounds)?;
            again.push((sounds.rewritten.len() > rewritten).then_some(steps));
            notes::patterned(&mut edit.sent, &edit.steps, &next, &mut patterns)?;
            pending(&shelves, &mut edit.sent, edit.steps.last().is_some_and(|step| step == "wave"), preview, &mut waves)?;
            if !edit.steps.is_empty() {
                path::opened(&mut next, &edit.steps, &manual);
                path::place(&mut next, &edit.steps, edit.sent.clone())?;
            } else if edit.sent.get("kind").and_then(Json::as_str) == Some("project") {
                next = edit.sent.clone();
            } else {
                return Err(String::from("A whole document says kind project; for one part, pass its path"));
            }
            edits.push(edit);
        }
    }
    let named = arrange::numbered(&mut next);
    if let Some(track) = arrange::orphan(&next).filter(|_| arrange::orphan(&current).is_none()) {
        return Err(format!("Nothing written: tracks/{track} would be marked inGroup with no group or member right above it; a member leaves its group by inGroup false at its own path, which moves it below the group"));
    }
    if let Some(crowded) = arrange::crowded(&next).filter(|_| arrange::crowded(&current).is_none()) {
        return Err(format!("Nothing written: {crowded}"));
    }
    let elsewhere = match &stale {
        Some((revision, then, moved)) => {
            let mut reached: Vec<Vec<String>> = edits.iter().map(|edit| edit.steps.clone()).collect();
            let plain = |(edit, again): &(&Edit, &Option<Vec<String>>)| edit.did.is_none() && again.is_none();
            let hosts: Vec<&[String]> = edits.iter().zip(&again).filter(plain).filter_map(|(edit, _)| presets::reshaped(&edit.steps)).collect();
            if !moved.is_empty() && (!sounds.rewritten.is_empty() || !hosts.is_empty()) {
                let was = replayed(then, &edits, &typed, &again, &manual, &preset).ok();
                for owner in &sounds.rewritten {
                    footprint(owner, then, &current, Some(&next), was.as_ref(), &mut reached);
                }
                if !hosts.is_empty() {
                    let settled = |document: &Json| Project::from_json(document).map(|project| project.to_json());
                    let (landed, was) = (settled(&next), was.as_ref().and_then(settled));
                    for owner in hosts {
                        footprint(owner, then, &current, landed.as_ref(), was.as_ref(), &mut reached);
                    }
                }
            }
            let touched: Vec<String> = moved.iter().filter(|change| reached.iter().any(|at| change.at.starts_with(at) || at.starts_with(&change.at))).map(|change| path::joined(&change.at)).collect();
            if !touched.is_empty() {
                return Err(format!("Nothing written: the song changed since revision {revision} at {}; read with since {revision}", touched.join(", ")));
            }
            (!moved.is_empty()).then(|| format!(" · {} elsewhere since revision {revision}; read with since {revision}", counted(moved.len(), "change")))
        }
        None => None,
    };
    if preview {
        let revision = host.revisions().mark(&current);
        let parted = parted(&next, &edits)?;
        return Ok(previewed(revision, elsewhere, &edits, &current, &settled(&current, &parted)));
    }
    let imports = imported(host, &mut next, &waves)?;
    if let Some(shifts) = shifts.filter(|shifts| !shifts.is_empty()) {
        host.follow(&shifts);
    }
    let parted = parted(&next, &edits)?;
    host.adopt(&parted)?;
    let kept = host.document();
    let details = flag_arg(args, "details");
    let mut out = if details || (count == 1 && edits[0].did.is_none()) { told(&edits, &next, &kept) } else { format!("Applied {}", counted(count, "edit")) };
    let changes = path::changes(&current, &kept);
    let (mut lines, mut arps, mut seen) = (Vec::new(), Vec::new(), Vec::new());
    let own = |at: &str| {
        let steps = path::steps(at);
        if sounds.as_sent.iter().any(|kept| steps.starts_with(kept)) { path::locate(&next, &steps) } else { said(&edits, &typed, at) }
    };
    for (edit, typed) in edits.iter().zip(&typed) {
        if seen.contains(&edit.steps) {
            continue;
        }
        seen.push(edit.steps.clone());
        let (meant, landed) = (path::locate(&next, &edit.steps), path::locate(&kept, &edit.steps));
        match (path::manual_at(&manual, &kept, &edit.steps), meant) {
            (_, None) => {
                if let Some(still) = landed {
                    lines.push(format!("{}: removal kept {}", path::joined(&edit.steps), path::short(still)));
                }
            }
            (Some(None), Some(_)) if landed.is_none() => lines.push(format!("{}: not a key here, so it does nothing", path::joined(&edit.steps))),
            (guide, Some(meant)) => {
                path::differences(meant, landed, guide.flatten(), &path::joined(&edit.steps), &own, &mut lines);
                let mut found = Vec::new();
                path::arps(typed, &mut edit.steps.clone(), &mut found);
                for at in found {
                    let said = chose(typed, &edit.steps, &at);
                    match arps.iter_mut().find(|(_, known)| *known == at) {
                        Some(known) => known.0 |= said,
                        None => arps.push((said, at)),
                    }
                }
            }
        }
    }
    for at in &sounds.as_sent {
        if seen.iter().any(|steps| at.starts_with(steps) || steps.starts_with(at)) {
            continue;
        }
        seen.push(at.clone());
        if let Some(meant) = path::locate(&next, at) {
            path::differences(meant, path::locate(&kept, at), path::manual_at(&manual, &kept, at).flatten(), &path::joined(at), &own, &mut lines);
        }
    }
    let lines: Vec<String> = lines.iter().enumerate().filter(|(index, line)| !lines[..*index].contains(line)).map(|(_, line)| line.clone()).collect();
    let mut unheard: Vec<String> = arps.iter().filter(|(_, at)| path::reached(&changes, at)).filter_map(|(chose, at)| unheard(&kept, at, *chose)).collect();
    unheard.extend(silent(&kept, &changes));
    let overlaps = overlapping(&kept, &changes);
    let played = if edits.iter().any(|edit| edit.arranged) { Vec::new() } else { played(host, &current, &kept, &changes) };
    if let Some(revision) = host.revisions().record(&kept, changes) {
        out.push_str(&format!(" as revision {revision}"));
    } else {
        out.push_str(" (unchanged)");
    }
    if let Some(elsewhere) = elsewhere {
        out.push_str(&elsewhere);
    }
    if details {
        out.push_str(" · ");
        out.push_str(&summary(&kept));
        listed(&mut out, "Imports:", &imports);
        listed(&mut out, "Presets:", &sounds.loaded);
        listed(&mut out, "Patterns:", &patterns);
    } else {
        for (label, count) in [("imports", imports.len()), ("presets", sounds.loaded.len()), ("patterns", patterns.len())] {
            if count > 0 {
                out.push_str(&format!(" · {label}: {count}"));
            }
        }
        let mut operations: Vec<String> = edits.iter().filter_map(|edit| edit.did.clone()).collect();
        operations.dedup();
        listed(&mut out, "Operations:", &operations);
    }
    listed(&mut out, "Named as the app names a new track:", &named);
    listed(&mut out, "Notes:", &played);
    listed(&mut out, "Preset pitch:", &sounds.pitch);
    listed(&mut out, "Kept differently, so read it back:", &lines);
    listed(&mut out, "Kept, but not heard:", &unheard);
    listed(&mut out, "Kept, overlapping on one track:", &overlaps);
    Ok(out)
}

fn parted<'a>(next: &'a Json, edits: &[Edit]) -> Result<Cow<'a, Json>, String> {
    let mut parted = Cow::Borrowed(next);
    for steps in edits.iter().rev().filter(|edit| edit.did.is_none()).map(|edit| &edit.steps) {
        let fade = steps.iter().position(|step| step == "wave").map_or(steps.len(), |wave| wave + 1);
        let written = steps.get(fade).and_then(|step| Slope::BOTH.into_iter().find(|slope| slope.key() == step));
        if let Some(slope) = written
            && let Some(wave) = path::locate(&parted, &steps[..fade])
        {
            let stopped = fades_from_json(wave, slope).of(slope).map_or(Json::Null, fade_to_json);
            path::place(parted.to_mut(), &steps[..=fade], stopped)?;
        }
    }
    Ok(parted)
}

fn settled(current: &Json, document: &Json) -> Json {
    let song = |document: &Json| Project::from_json(document).map(|project| project.to_json());
    let (Some(Json::Object(was)), Some(Json::Object(now)), Json::Object(held), Json::Object(asked)) = (song(current), song(document), current, document) else {
        return document.clone();
    };
    let has = |fields: &[(String, Json)], key: &str| fields.iter().any(|(known, _)| known == key);
    let mut fields: Vec<(String, Json)> = asked
        .iter()
        .filter(|(key, _)| !has(&now, key) && !has(&was, key))
        .filter_map(|(key, value)| if path::MOVING.contains(&key.as_str()) { Some((key.clone(), value.clone())) } else { held.iter().find(|(known, _)| known == key).cloned() })
        .collect();
    fields.extend(now);
    Json::Object(fields)
}

fn said<'a>(edits: &'a [Edit], typed: &'a [Json], at: &str) -> Option<&'a Json> {
    let at = path::steps(at);
    for (edit, typed) in edits.iter().zip(typed).rev().filter(|(edit, _)| at.starts_with(&edit.steps)) {
        let rest = &at[edit.steps.len()..];
        match (&edit.did, &edit.said) {
            (None, _) => return path::locate(typed, rest),
            (Some(_), Some(said)) => {
                if let Some(own) = path::locate(said, rest).filter(|own| **own != Json::Null) {
                    return Some(own);
                }
            }
            (Some(_), None) => {}
        }
    }
    None
}

fn found<'a>(document: &'a Json, at: &[String]) -> &'a Json {
    path::locate(document, at).unwrap_or(&Json::Null)
}

fn footprint(owner: &[String], then: &Json, current: &Json, landed: Option<&Json>, was: Option<&Json>, reached: &mut Vec<Vec<String>>) {
    let (before, now) = (found(then, owner), found(current, owner));
    match (landed, was) {
        (Some(landed), Some(was)) => {
            path::lost(now, found(landed, owner), before, &mut owner.to_vec(), reached);
            path::lost(before, found(was, owner), now, &mut owner.to_vec(), reached);
        }
        _ => reached.push(owner.to_vec()),
    }
}

fn replayed(then: &Json, edits: &[Edit], typed: &[Json], again: &[Option<Vec<String>>], manual: &Json, preset: &dyn Fn(&str) -> Result<Json, String>) -> Result<Json, String> {
    let mut was = then.clone();
    for ((edit, typed), again) in edits.iter().zip(typed).zip(again) {
        let (mut sent, mut steps) = (edit.sent.clone(), edit.steps.clone());
        if let Some(asked) = again {
            (sent, steps) = (typed.clone(), asked.clone());
            presets::poured(&mut sent, &mut steps, &mut was, manual, preset, &mut presets::Report::default())?;
        }
        if steps.is_empty() {
            was = sent;
        } else {
            path::opened(&mut was, &steps, manual);
            path::place(&mut was, &steps, sent)?;
        }
    }
    Ok(was)
}

fn told(edits: &[Edit], next: &Json, kept: &Json) -> String {
    let mut said: Vec<String> = edits.iter().filter_map(|edit| edit.did.clone()).collect();
    said.dedup();
    let plain: Vec<&Edit> = edits.iter().filter(|edit| edit.did.is_none()).collect();
    match plain.as_slice() {
        [] => {}
        [one] => {
            let at = path::joined(&one.steps);
            said.push(match (&one.sent, path::locate(kept, &one.steps)) {
                (Json::Null, Some(_)) if shortened(next, kept, &one.steps) => format!("Removed {at}; the items after it moved up one"),
                (Json::Null, Some(still)) => format!("{at} is still there as {}", path::short(still)),
                (Json::Null, None) => format!("Removed {at}"),
                _ if one.steps.is_empty() => String::from("Written, the whole document"),
                _ => format!("Written at {at}"),
            });
        }
        many => said.push(format!("Written at {}", many.iter().map(|edit| if edit.steps.is_empty() { String::from("the whole document") } else { path::joined(&edit.steps) }).collect::<Vec<_>>().join(", "))),
    }
    said.join("; ")
}

fn shortened(next: &Json, kept: &Json, steps: &[String]) -> bool {
    let Some((index, list)) = steps.split_last() else {
        return false;
    };
    let count = |document: &Json| path::locate(document, list).and_then(Json::as_array).map(<[Json]>::len);
    index.parse::<usize>().is_ok() && matches!((count(next), count(kept)), (Some(meant), Some(landed)) if landed <= meant)
}

fn previewed(revision: u64, elsewhere: Option<String>, edits: &[Edit], current: &Json, next: &Json) -> String {
    let mut lines = vec![format!("Nothing written at revision {revision}; write with unchangedSince {revision}{}", elsewhere.unwrap_or_default())];
    let mut operations: Vec<String> = edits.iter().filter_map(|edit| edit.did.clone()).collect();
    operations.dedup();
    lines.extend(operations);
    let changes = path::changes(current, next);
    lines.extend(changes.iter().take(MOST_PREVIEWED).map(|change| format!("{}: was {}, now {}", path::joined(&change.at), shown(change.before.as_ref()), shown(path::locate(next, &change.at)))));
    if changes.len() > MOST_PREVIEWED {
        lines.push(format!("and {} more", changes.len() - MOST_PREVIEWED));
    }
    if changes.is_empty() {
        lines.push(String::from("Nothing would change"));
    }
    lines.join("\n")
}

fn shown(value: Option<&Json>) -> String {
    match value {
        None => String::from("nothing"),
        Some(scalar @ (Json::Number(_) | Json::Bool(_) | Json::String(_) | Json::Null)) => scalar.write_at(0),
        Some(other) => path::short(other),
    }
}

fn pending(shelves: &[library::Shelf], json: &mut Json, wave: bool, preview: bool, out: &mut Vec<(String, String)>) -> Result<(), String> {
    match json {
        Json::Object(fields) if wave => {
            let Some(file) = fields.iter().find(|(key, _)| key == "path").and_then(|(_, value)| value.as_str()).map(String::from) else {
                return Ok(());
            };
            if preview {
                return Err(String::from("preview does not import waves; import first or write without preview"));
            }
            let path = library::whole(shelves, &file)?;
            library::wave_in(shelves, &path)?;
            fields.retain(|(key, _)| key != "path" && key != "name");
            fields.insert(0, (String::from("name"), Json::String(format!("{IMPORTING}{path}"))));
            if !out.iter().any(|(known, _)| *known == file) {
                out.push((file, path));
            }
        }
        Json::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                pending(shelves, value, key == "wave", preview, out)?;
            }
        }
        Json::Array(items) => {
            for item in items {
                pending(shelves, item, false, preview, out)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn imported(host: &mut dyn Host, next: &mut Json, waves: &[(String, String)]) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for (file, path) in waves {
        let found = import(host, file)?;
        let name = found.get("name").cloned().unwrap_or(Json::Null);
        lines.push(format!("{file} as wave {}, {:.3} s", name.as_str().unwrap_or(""), found.get("seconds").and_then(Json::as_f64).unwrap_or(0.0)));
        pooled(next, &format!("{IMPORTING}{path}"), &name, false);
    }
    Ok(lines)
}

fn pooled(json: &mut Json, held: &str, name: &Json, wave: bool) {
    match json {
        Json::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                if wave && key == "name" && value.as_str() == Some(held) {
                    *value = name.clone();
                } else {
                    pooled(value, held, name, key == "wave");
                }
            }
        }
        Json::Array(items) => items.iter_mut().for_each(|item| pooled(item, held, name, false)),
        _ => {}
    }
}

fn played(host: &dyn Host, current: &Json, kept: &Json, changes: &[path::Change]) -> Vec<String> {
    let mut out = Vec::new();
    let home = harmony::home_of(kept);
    let project = Project::from_json(kept);
    let mut sent = None;
    for track in 0..kept.get("tracks").and_then(Json::as_array).map_or(0, <[Json]>::len) {
        let lane = [String::from("tracks"), track.to_string(), String::from("clips")];
        if !path::reached(changes, &lane) {
            continue;
        }
        let clips = |document: &Json| -> Vec<Clip> { path::locate(document, &lane).and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(clip_from_json).collect() };
        let (before, after) = (clips(current), clips(kept));
        let olds: Vec<(i32, Sounded)> = before.iter().flat_map(|clip| sounding(clip).into_iter().map(|note| (clip.start, note))).collect();
        let mut slots: HashMap<(i32, u8), Vec<usize>> = HashMap::new();
        for (slot, (_, old)) in olds.iter().enumerate() {
            slots.entry((old.0, old.2.pitch)).or_default().push(slot);
        }
        let mut was: Vec<Vec<Sounded>> = vec![Vec::new(); after.len()];
        for (clip, was) in after.iter().zip(&mut was) {
            for note in sounding(clip) {
                let alike = |slot: &usize| (olds[*slot].1.1, olds[*slot].1.2.velocity, olds[*slot].1.2.articulation) == (note.1, note.2.velocity, note.2.articulation);
                let found = slots.get_mut(&(note.0, note.2.pitch)).and_then(|slots| slots.iter().position(alike).map(|at| slots.swap_remove(at)));
                if found.is_some() {
                    was.push(note);
                }
            }
        }
        for slot in slots.into_values().flatten() {
            let (start, note) = olds[slot];
            let under = after.iter().position(|clip| (clip.start..clip.end()).contains(&note.0));
            let own = || after.iter().position(|clip| clip.start == start && !clip.notes.contains(&note.2));
            if let Some(index) = under.or_else(own) {
                was[index].push(note);
            }
        }
        let mut heard: Option<Option<Vec<SongNote>>> = None;
        for (index, (clip, was)) in after.iter().zip(&was).enumerate() {
            let at: Vec<String> = lane.iter().cloned().chain(std::iter::once(index.to_string())).collect();
            if !path::reached(changes, &at) || (was.is_empty() && !before.iter().any(|old| old.start == clip.start)) {
                continue;
            }
            let across = |notes: &[Sounded]| Clip { start: clip.start, length: clip.length, label: String::new(), notes: notes.iter().map(|&(start, length, note)| Note { start: beats(start - clip.start) as f32, length: beats(length) as f32, ..note }).collect(), file: None };
            let heard = heard.get_or_insert_with(|| project.as_ref().and_then(|project| harmony::arp_output(project, track, sent.get_or_insert_with(|| host.sent(project)))));
            if let Some(line) = notes::compared(&across(was), &across(&sounding(clip)), home, heard.as_deref()) {
                out.push(format!("{}: {line}", path::joined(&at)));
            }
        }
    }
    if out.len() > MOST_PLAYED {
        let more = out.len() - MOST_PLAYED;
        out.truncate(MOST_PLAYED);
        out.push(format!("and {}", counted(more, "more clip")));
    }
    out
}

fn sounding(clip: &Clip) -> Vec<Sounded> {
    let placed = |note: &Note| {
        let start = ticks(f64::from(note.start));
        let length = ticks(f64::from(note.length)).max(1).min(clip.length - start);
        (start < clip.length).then_some((clip.start + start, length, *note))
    };
    clip.notes.iter().filter_map(placed).collect()
}

fn silent(kept: &Json, changes: &[path::Change]) -> Vec<String> {
    let pool: Vec<&str> = kept.get("waves").and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(|wave| wave.get("name")).filter_map(Json::as_str).collect();
    let mut out = Vec::new();
    for (track, owner) in kept.get("tracks").and_then(Json::as_array).unwrap_or(&[]).iter().enumerate() {
        for (index, clip) in owner.get("clips").and_then(Json::as_array).unwrap_or(&[]).iter().enumerate() {
            let at = [String::from("tracks"), track.to_string(), String::from("clips"), index.to_string()];
            if !path::reached(changes, &at) {
                continue;
            }
            if let Some(name) = clip.get("wave").and_then(|wave| wave.get("name")).and_then(Json::as_str)
                && !pool.contains(&name)
            {
                out.push(format!("{}: wave {name} is missing; set wave.path or import it", path::joined(&at)));
            }
            let length = clip.get("lengthBeats").and_then(Json::as_f64).unwrap_or(notes::CLIP_BEATS);
            let start = clip.get("startBeat").and_then(Json::as_f64).unwrap_or(0.0);
            let notes = clip.get("notes").and_then(Json::as_array).unwrap_or(&[]);
            let begins = |note: &Json| note.get("startBeat").and_then(Json::as_f64).unwrap_or(0.0);
            let late = notes.iter().filter(|note| begins(note) >= length).count();
            if late > 0 {
                let in_song_beats = start > 0.0 && notes.iter().all(|note| (0.0..length).contains(&(begins(note) - start)));
                let hint = if in_song_beats { format!("subtract clip startBeat {start} from note starts") } else { format!("lengthen the clip or move {}", if late == 1 { "it" } else { "them" }) };
                let starting = match (late, notes.len()) {
                    (_, 1) => String::from("its note starts"),
                    (1, all) => format!("1 of {all} notes starts"),
                    (late, all) => format!("{late} of {all} notes start"),
                };
                out.push(format!("{}: {starting} outside lengthBeats {length}; {hint}", path::joined(&at)));
            }
            let grown: Vec<String> = at.iter().cloned().chain(std::iter::once(String::from("lengthBeats"))).collect();
            let ends = notes.iter().map(|note| begins(note) + note.get("lengthBeats").and_then(Json::as_f64).unwrap_or(0.0)).fold(0.0, f64::max);
            if let Some(before) = path::before(changes, &grown).and_then(Json::as_f64)
                && before < length
                && !notes.is_empty()
                && ends <= before
            {
                out.push(format!("{}: lengthBeats {before} → {length}, notes end at {}; write notes to fill it", path::joined(&at), tidy(ends)));
            }
        }
    }
    out
}

fn overlapping(kept: &Json, changes: &[path::Change]) -> Vec<String> {
    let mut out = Vec::new();
    for (track, owner) in kept.get("tracks").and_then(Json::as_array).unwrap_or(&[]).iter().enumerate() {
        if !path::reached(changes, &[String::from("tracks"), track.to_string(), String::from("clips")]) {
            continue;
        }
        let mut spans: Vec<(f64, f64, usize)> = owner.get("clips").and_then(Json::as_array).unwrap_or(&[]).iter().enumerate().map(|(index, clip)| {
            let start = clip.get("startBeat").and_then(Json::as_f64).unwrap_or(0.0);
            (start, start + clip.get("lengthBeats").and_then(Json::as_f64).unwrap_or(notes::CLIP_BEATS), index)
        }).collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut reach: Option<(f64, usize)> = None;
        for (start, end, index) in spans {
            if let Some((until, first)) = reach.filter(|(until, _)| start < *until) {
                out.push(format!("tracks/{track}/clips/{first} and {index}: both play at beats {start}–{}", until.min(end)));
            }
            if reach.is_none_or(|(until, _)| end > until) {
                reach = Some((end, index));
            }
        }
    }
    out
}

fn listed(out: &mut String, title: &str, lines: &[String]) {
    if !lines.is_empty() {
        out.push('\n');
        out.push_str(title);
        for line in lines {
            out.push_str("\n  ");
            out.push_str(line);
        }
    }
}

fn chose(typed: &Json, steps: &[String], at: &[String]) -> bool {
    if steps.len() > at.len() {
        return steps[at.len()] == "on";
    }
    path::locate(typed, &at[steps.len()..]).is_none_or(|arp| arp.get("on").is_some())
}

fn unheard(document: &Json, at: &[String], chose: bool) -> Option<String> {
    let arp = Arp::from_json(path::locate(document, at));
    let at = path::joined(at);
    if !arp.on {
        return (!chose).then(|| format!("{at} is off; set on true"));
    }
    let keys = arp.unheard();
    (!keys.is_empty()).then(|| format!("{at}: stepMode {} ignores its {}; stepMode {} plays them", arp.stepped, keys.join(", "), !arp.stepped))
}

fn asked(args: &Json) -> Result<Vec<(&Json, &'static str)>, String> {
    let Some(edits) = args.get("edits").filter(|edits| !matches!(edits, Json::Null)) else {
        return Ok(vec![(args, "value is needed, or scale, transpose, fit, insert, remove, copy, cut, join, duplicate, move, match, flatten, quantize, double, halve, reverse, chords or edits")]);
    };
    let edits = edits.as_array().filter(|edits| !edits.is_empty()).ok_or("edits is a list of objects, each a write of its own")?;
    Ok(edits.iter().map(|each| (each, "each edit needs value, scale, transpose, fit, insert, remove, copy, cut, join, duplicate, move, match, flatten, quantize, double, halve, reverse or chords")).collect())
}

fn edit(host: &dyn Host, args: &Json, document: &Json, missing: &str) -> Result<Vec<Edit>, String> {
    let meant = |key: &&str| args.get(key).is_some_and(|given| *key == VALUE || !matches!(given, Json::Null | Json::Bool(false)));
    let mut named: Vec<&str> = OPERATIONS.into_iter().filter(meant).collect();
    if named.len() > 1 && args.get(VALUE) == Some(&Json::Null) {
        named.retain(|key| *key != VALUE);
    }
    let operation = match named.as_slice() {
        [] => return Err(String::from(missing)),
        [one] => *one,
        many => return Err(format!("One operation per edit, and this one carries {}; split them across edits", many.join(" and "))),
    };
    let mut steps = path::steps(text_arg(args, "path").unwrap_or(""));
    path::ended(&mut steps, document);
    if operation == "insert"
        && let Some(given @ Json::Object(_)) = args.get("insert")
    {
        let row = steps.iter().rev().nth(1).is_some_and(|step| step == "rack");
        return if row { arrange::racked(given, document, &steps) } else { arrange::tracked(given, document, &steps) };
    }
    if operation == "move" {
        return arrange::relocated(args.get("move").unwrap_or(&Json::Null), document, &steps);
    }
    if operation == "duplicate" {
        return arrange::duplicated(document, &steps);
    }
    if operation == "cut" {
        return arrange::cut(args.get("cut"), document, &steps);
    }
    if operation == "match" {
        return arrange::matched(host, document, &steps);
    }
    if operation == "flatten" {
        return arrange::flattened(host, document, &steps);
    }
    if operation == "join"
        && let Some(Json::Array(clips)) = args.get("join")
    {
        return arrange::stacked_clips(clips, document, &steps);
    }
    if let Some(verb) = arrange::verb(operation) {
        return arrange::arranged(verb, args, document, &steps);
    }
    let given = args.get(operation).unwrap_or(&Json::Null);
    let at = if steps.is_empty() { String::from("the document") } else { path::joined(&steps) };
    let found = |what: &str| path::locate(document, &steps).ok_or_else(|| format!("{what}; nothing is at {at}"));
    let filter = notes::filter(args, document, &at)?;
    if let "quantize" | "double" | "halve" | "reverse" | "chords" = operation {
        return arrange::rolled(operation, given, document, &steps, &filter);
    }
    let (sent, did) = match operation {
        "transpose" => notes::transposed(found("transpose needs a clip or track")?, given, &at, &filter).map(|(sent, did)| (sent, Some(did)))?,
        "scale" => notes::scaled(found("scale needs a curve, clip or track")?, given, &at, &filter).map(|(sent, did)| (sent, Some(did)))?,
        "fit" => notes::fitted(found("fit needs a clip or track")?, given, &at, &filter).map(|(sent, did)| (sent, Some(did)))?,
        _ if filter.any() => return Err(String::from("fromBeat, untilBeat, above, below and picked choose notes for transpose, scale, fit or quantize; value takes none")),
        _ => match arrange::regrouped(given, document, &steps)? {
            Some(edits) => return Ok(edits),
            None => (given.clone(), None),
        },
    };
    Ok(vec![Edit { steps, sent, did, arranged: false, said: None, shifts: Vec::new() }])
}

pub(super) fn reverted(host: &mut dyn Host, revision: u64) -> Result<String, String> {
    let current = host.document();
    let next = host.revisions().reverted(revision, &current)?;
    host.adopt(&next)?;
    let kept = host.document();
    let changes = path::changes(&current, &kept);
    let undone = match host.revisions().record(&kept, changes) {
        Some(id) => format!("Undid revision {revision} as revision {id}"),
        None => format!("Revision {revision}: no remaining changes"),
    };
    Ok(undone)
}

fn summary(document: &Json) -> String {
    let tracks = document.get("tracks").and_then(Json::as_array).unwrap_or(&[]);
    let clips: Vec<&Json> = tracks.iter().filter_map(|track| track.get("clips")).filter_map(Json::as_array).flatten().collect();
    let end = clips.iter().map(|clip| clip.get("startBeat").and_then(Json::as_f64).unwrap_or(0.0) + clip.get("lengthBeats").and_then(Json::as_f64).unwrap_or(notes::CLIP_BEATS)).fold(0.0, f64::max);
    let bpm = document.get("bpm").and_then(Json::as_f64).unwrap_or(0.0);
    let key = format!("{} {}", document.get("root").and_then(Json::as_str).unwrap_or("C"), document.get("scale").and_then(Json::as_str).unwrap_or("major"));
    format!("{}, {}, {}, {bpm} bpm, {key}", counted(tracks.len(), "track"), counted(clips.len(), "clip"), counted((end / 4.0).ceil() as usize, "bar"))
}

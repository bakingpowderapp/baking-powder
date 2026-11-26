use super::counted;
use crate::theory::pitch_class_name;
use sdk::json::{self, Json, tidy};
use sdk::schema::{EMPTY, TEXT};

const FOLDED: &str = "folded ";
const AT: &str = " at ";
const IN: &str = " in ";
const MOST_DIFFERENCES: usize = 12;
const KEPT_WITHIN: f64 = 1e-4;
const SHORT: usize = 48;
const PASSING: [&str; 15] = ["waves", "folders", "playing", "endBeat", "latency", "selectedTrack", "selectedClips", "selectionStartBeat", "selectionEndBeat", "selectedTracks", "openRow", "openClip", "pickedNotes", "shownPitches", "shownBeats"];
pub(super) const MOVING: [&str; 3] = ["playheadBeat", "loopStartBeat", "loopEndBeat"];
const READ_ONLY: &str = "read only";
const STANDS_IN: &str = "in place of";
const END: &str = "end";
const FOLDS: [&str; 6] = ["notes", "clips", "rack", "curves", "lfos", "palette"];

pub(super) fn steps(path: &str) -> Vec<String> {
    path.split(['/', '.', '[', ']']).filter(|step| !step.is_empty()).map(String::from).collect()
}

pub(super) fn joined(steps: &[String]) -> String {
    steps.join("/")
}

pub(super) fn ended(steps: &mut [String], document: &Json) {
    let mut here = Some(document);
    for step in steps.iter_mut() {
        if step == END && !matches!(here, Some(Json::Object(_))) {
            *step = here.and_then(Json::as_array).map_or(0, <[Json]>::len).to_string();
        }
        here = here.and_then(|json| locate(json, std::slice::from_ref(step)));
    }
}

pub(super) fn locate<'a>(json: &'a Json, steps: &[String]) -> Option<&'a Json> {
    steps.iter().try_fold(json, |here, step| match here {
        Json::Object(_) => here.get(step),
        Json::Array(items) => items.get(step.parse::<usize>().ok()?),
        _ => None,
    })
}

pub(super) fn place(json: &mut Json, steps: &[String], value: Json) -> Result<(), String> {
    let mut walked: Vec<String> = Vec::new();
    place_from(json, steps, value, &mut walked)
}

fn place_from(json: &mut Json, steps: &[String], value: Json, walked: &mut Vec<String>) -> Result<(), String> {
    let Some((head, rest)) = steps.split_first() else {
        *json = value;
        return Ok(());
    };
    let here = || if walked.is_empty() { String::from("the document") } else { joined(walked) };
    match json {
        Json::Object(fields) => {
            let at = fields.iter().position(|(key, _)| key == head);
            if rest.is_empty() {
                match (at, value) {
                    (Some(at), Json::Null) => {
                        fields.remove(at);
                    }
                    (Some(at), value) => fields[at].1 = value,
                    (None, Json::Null) => {}
                    (None, value) => fields.push((head.clone(), value)),
                }
                return Ok(());
            }
            let at = at.ok_or_else(|| format!("{} has no {head}; its keys are {}", here(), keys(fields)))?;
            walked.push(head.clone());
            place_from(&mut fields[at].1, rest, value, walked)
        }
        Json::Array(items) => {
            let index = head.parse::<usize>().map_err(|_| format!("{} is a list of {}; use an index, not {head}", here(), items.len()))?;
            if rest.is_empty() {
                match value {
                    Json::Null if index < items.len() => {
                        items.remove(index);
                    }
                    Json::Null => {}
                    value if index < items.len() => items[index] = value,
                    value if index == items.len() => items.push(value),
                    _ => return Err(format!("{} holds {}; {} appends", here(), counted(items.len(), "item"), items.len())),
                }
                return Ok(());
            }
            let count = items.len();
            let item = items.get_mut(index).ok_or_else(|| format!("{} holds {}, none at {index}", here(), counted(count, "item")))?;
            walked.push(head.clone());
            place_from(item, rest, value, walked)
        }
        _ => Err(format!("{} is a value, not an object or a list", here())),
    }
}

pub(super) fn opened(document: &mut Json, steps: &[String], manual: &Json) {
    for index in 0..steps.len().saturating_sub(1) {
        let (parent, key) = (&steps[..index], &steps[index]);
        let absent = matches!(locate(document, parent), Some(Json::Object(fields)) if !fields.iter().any(|(known, _)| known == key));
        if !absent || !matches!(manual_at(manual, document, &steps[..=index]), Some(Some(_))) {
            continue;
        }
        let empty = if steps[index + 1].parse::<usize>().is_ok() { Json::Array(Vec::new()) } else { Json::Object(Vec::new()) };
        if place(document, &steps[..=index], empty).is_err() {
            return;
        }
    }
}

fn keys(fields: &[(String, Json)]) -> String {
    fields.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>().join(", ")
}

pub(super) fn missing(json: &Json, steps: &[String]) -> String {
    let mut here = json;
    let mut walked = 0;
    while let Some(next) = steps.get(walked).and_then(|step| locate(here, std::slice::from_ref(step))) {
        here = next;
        walked += 1;
    }
    let at = if walked == 0 { String::from("the document") } else { joined(&steps[..walked]) };
    let what = match here {
        Json::Object(fields) => format!("its keys are {}", keys(fields)),
        Json::Array(items) => format!("it is a list of {}", items.len()),
        _ => String::from("it is a value"),
    };
    match steps.get(walked) {
        Some(step) => format!("{at} has nothing at {step}; {what}"),
        None => format!("{at} is there; {what}"),
    }
}

pub(super) fn fold(json: &mut Json, at: &str, source: Option<&str>, notes: bool, outline: bool) {
    fold_in(json, &mut steps(at), source, notes, outline);
}

fn fold_in(json: &mut Json, at: &mut Vec<String>, source: Option<&str>, notes: bool, outline: bool) {
    match json {
        Json::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                at.push(key.clone());
                match folded(key, value, at, source, notes, outline) {
                    Some(line) => *value = Json::String(line),
                    None => fold_in(value, at, source, notes, outline),
                }
                at.pop();
            }
        }
        Json::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                at.push(index.to_string());
                fold_in(item, at, source, notes, outline);
                at.pop();
            }
        }
        _ => {}
    }
}

fn folded(key: &str, value: &Json, at: &[String], source: Option<&str>, notes: bool, outline: bool) -> Option<String> {
    let items = value.as_array().filter(|items| !items.is_empty())?;
    let (noun, detail) = match key {
        "notes" if notes => ("note", range_of(items)),
        "clips" if outline && notes => ("clip", span_of(items)),
        "rack" if outline => ("row", named(items, "kind")),
        "curves" if outline => ("curve", named(items, "param")),
        "lfos" if outline => ("lfo", named(items, "param")),
        "palette" if outline => ("column", String::new()),
        _ => return None,
    };
    let from = source.map_or_else(String::new, |file| format!("{IN}{file}"));
    Some(format!("{FOLDED}{}{detail}{AT}{}{from}", counted(items.len(), noun), joined(at)))
}

fn range_of(notes: &[Json]) -> String {
    let pitches = notes.iter().filter_map(|note| note.get("pitch")).filter_map(Json::as_i64);
    let (low, high) = pitches.fold((i64::MAX, i64::MIN), |(low, high), pitch| (low.min(pitch), high.max(pitch)));
    if low <= high { format!(", {} to {}", pitch_name(low), pitch_name(high)) } else { String::new() }
}

fn span_of(clips: &[Json]) -> String {
    let at = |clip: &Json, key: &str| clip.get(key).and_then(Json::as_f64);
    let (from, until) = clips.iter().filter_map(|clip| Some((at(clip, "startBeat")?, at(clip, "lengthBeats")?))).fold((f64::INFINITY, f64::NEG_INFINITY), |(from, until), (start, length)| (from.min(start), until.max(start + length)));
    if from <= until { format!(", beats {} to {}", tidy(from), tidy(until)) } else { String::new() }
}

fn named(items: &[Json], key: &str) -> String {
    let names: Vec<&str> = items.iter().filter_map(|item| item.get(key)).filter_map(Json::as_str).collect();
    if names.is_empty() { String::new() } else { format!(", {}", names.join(", ")) }
}

pub(super) fn fold_shapes(json: &mut Json) {
    match json {
        Json::Array(shapes) if shapes.len() > 2 && shapes[1..].iter().all(|shape| shape.get("kind").is_some()) => {
            let kinds: Vec<&str> = shapes[1..].iter().filter_map(|shape| shape.get("kind")).filter_map(Json::as_str).collect();
            let line = format!("{FOLDED}{} row shapes, {}; manual with row or a row's path gives one whole", kinds.len(), kinds.join(", "));
            shapes.truncate(1);
            shapes.push(Json::String(line));
        }
        Json::Array(items) => items.iter_mut().for_each(fold_shapes),
        Json::Object(fields) => fields.iter_mut().for_each(|(_, value)| fold_shapes(value)),
        _ => {}
    }
}

pub(super) fn unfold(json: &mut Json, key: Option<&str>, current: &Json, library: &dyn Fn(&str) -> Option<Json>) -> Result<(), String> {
    let whole = match (&*json, key) {
        (Json::String(line), Some(key)) if line.starts_with(FOLDED) && FOLDS.contains(&key) => Some(unfolded(key, line, current, library)?),
        _ => None,
    };
    match (whole, json) {
        (Some(whole), json) => *json = whole,
        (None, Json::Object(fields)) => fields.iter_mut().try_for_each(|(key, value)| unfold(value, Some(key), current, library))?,
        (None, Json::Array(items)) => items.iter_mut().try_for_each(|item| unfold(item, None, current, library))?,
        (None, _) => {}
    }
    Ok(())
}

fn folded_path(line: &str) -> (&str, Option<&str>) {
    let path = line.split_once(AT).map_or("", |(_, path)| path);
    match path.split_once(IN) {
        Some((at, file)) => (at, Some(file)),
        None => (path, None),
    }
}

fn unfolded(key: &str, line: &str, current: &Json, library: &dyn Fn(&str) -> Option<Json>) -> Result<Json, String> {
    let (at, file) = folded_path(line);
    if steps(at).last().is_none_or(|own| own != key) {
        return Err(format!("Folded line at {at} is not {key}; write it at its own key, or send the value itself"));
    }
    let fetched;
    let source = match file {
        Some(file) => {
            fetched = library(file).ok_or_else(|| format!("Folded line at {at}: {file} would not read; check it with library"))?;
            &fetched
        }
        None => current,
    };
    let root = source.get("document").unwrap_or(source);
    let found = locate(root, &steps(at)).filter(|found| found.as_array().is_some()).ok_or_else(|| format!("Folded line at {at}: nothing is there now; read again"))?;
    let said = |line: &str| line.trim_start_matches(FOLDED).split(AT).next().unwrap_or_default().to_string();
    match folded(key, found, &steps(at), file, true, true) {
        Some(now) if now == line => Ok(found.clone()),
        Some(now) => Err(format!("Folded line at {at} was {}, now {}; read again or send the value", said(line), said(&now))),
        None => Err(format!("Folded line at {at} was {}, now nothing; send the value itself", said(line))),
    }
}

pub(super) fn manual_for<'a>(manual: &'a Json, document: &Json, steps: &[String]) -> Option<&'a Json> {
    let mut entry = manual;
    for (index, step) in steps.iter().enumerate() {
        entry = match entry {
            Json::Object(_) => entry.get(step)?,
            Json::Array(shapes) => {
                step.parse::<usize>().ok()?;
                locate(document, &steps[..=index]).and_then(|item| shape(entry, item)).or_else(|| shapes.get(1).filter(|_| shapes.len() == 2))?
            }
            _ => return None,
        };
    }
    Some(entry)
}

pub(super) fn pitch_name(pitch: i64) -> String {
    format!("{}{}", pitch_class_name(pitch.rem_euclid(12) as i32, false), pitch.div_euclid(12) - 1)
}

pub(super) fn differences<'a>(sent: &Json, kept: Option<&Json>, manual: Option<&Json>, at: &str, own: &dyn Fn(&str) -> Option<&'a Json>, out: &mut Vec<String>) {
    if out.len() >= MOST_DIFFERENCES {
        return;
    }
    let here = if at.is_empty() { "the document" } else { at };
    let said = || match own(at) {
        Some(Json::Object(_) | Json::Array(_)) => format!("sent {}", short(sent)),
        Some(own) => format!("sent {}", short(own)),
        None => format!("written {}", short(sent)),
    };
    let listed = |items: &[Json]| format!("{} {}", counted(items.len(), "item"), if own(at).is_some() { "sent" } else { "written" });
    let worded = || match (sent, manual.and_then(Json::as_str)) {
        (Json::Number(number), Some(line)) if line.starts_with(TEXT) => format!("; it is text, so send \"{}\"", json::number(*number)),
        _ => String::new(),
    };
    match (sent, kept) {
        (Json::Null, _) => {}
        (Json::Object(fields), Some(Json::Object(_)) | None) if kept.is_some() || matches!(manual, Some(Json::Object(_))) => {
            for (key, value) in fields {
                let inner = if at.is_empty() { key.clone() } else { format!("{at}/{key}") };
                let line = manual.filter(|manual| matches!(manual, Json::Object(_))).map(|manual| manual.get(key));
                let about = line.flatten().and_then(Json::as_str);
                if about.is_some_and(|line| line.contains(READ_ONLY)) || (about.is_some_and(|line| line.contains(STANDS_IN)) && kept.is_some_and(|kept| kept.get(key).is_none())) {
                    continue;
                }
                match (kept.and_then(|kept| kept.get(key)), line) {
                    (None, Some(None)) => out.push(format!("{inner}: not a key here, so it does nothing")),
                    (landed, line) => differences(value, landed, line.flatten(), &inner, own, out),
                }
            }
        }
        (Json::Array(items), None) => {
            let mut inner = Vec::new();
            for (index, item) in items.iter().enumerate() {
                differences(item, None, item_manual(manual, item), &format!("{at}/{index}"), own, &mut inner);
            }
            if !inner.is_empty() {
                out.push(format!("{here}: {}, none kept", listed(items)));
            }
        }
        (Json::Array(items), Some(Json::Array(others))) => {
            if items.len() != others.len() {
                out.push(format!("{here}: {}, {} kept", listed(items), others.len()));
            }
            for (index, (item, other)) in items.iter().zip(others).enumerate() {
                differences(item, Some(other), item_manual(manual, other), &format!("{at}/{index}"), own, out);
            }
        }
        (_, None) => match manual.and_then(Json::as_str).map(default_of) {
            Some(Some(default)) if at_default(sent, default) => {}
            Some(Some(default)) => out.push(format!("{here}: {}, kept the default {default}{}", said(), worded())),
            Some(None) => out.push(format!("{here}: {}, not kept, so it takes its default", said())),
            None => out.push(format!("{here} not kept: unknown here, or its default")),
        },
        (Json::Number(a), Some(Json::Number(b))) if (a - b).abs() <= KEPT_WITHIN * a.abs().max(b.abs()).max(1.0) => {}
        (a, Some(b)) if a == b => {}
        (_, Some(b)) => out.push(format!("{here}: {}, kept {}{}", said(), short(b), worded())),
    }
}

pub(super) fn manual_at<'a>(manual: &'a Json, document: &Json, steps: &[String]) -> Option<Option<&'a Json>> {
    let mut entry = manual;
    for (index, step) in steps.iter().enumerate() {
        entry = match entry {
            Json::Object(_) => match entry.get(step) {
                Some(next) => next,
                None => return (index + 1 == steps.len()).then_some(None),
            },
            Json::Array(_) => shape(entry, locate(document, &steps[..=index])?)?,
            _ => return None,
        };
    }
    Some(Some(entry))
}

fn item_manual<'a>(manual: Option<&'a Json>, item: &Json) -> Option<&'a Json> {
    match manual? {
        list @ Json::Array(_) => shape(list, item),
        line @ Json::String(_) => Some(line),
        _ => None,
    }
}

fn shape<'a>(list: &'a Json, item: &Json) -> Option<&'a Json> {
    let shapes = list.as_array()?.get(1..)?;
    if let [only] = shapes {
        return Some(only);
    }
    let kind = item.get("kind").and_then(Json::as_str)?;
    shapes.iter().find(|shape| shape.get("kind").and_then(Json::as_str) == Some(kind))
}

fn default_of(line: &str) -> Option<&str> {
    line.split_once("default ").and_then(|(_, rest)| rest.split([':', ';', ',']).next()).map(str::trim)
}

fn at_default(sent: &Json, default: &str) -> bool {
    match sent {
        Json::Number(number) => default.parse::<f64>().is_ok_and(|default| (number - default).abs() <= 1e-9 * number.abs().max(default.abs()).max(1.0)),
        Json::Bool(flag) => flag.to_string() == default,
        Json::String(name) => name.eq_ignore_ascii_case(default) || (name.is_empty() && default == EMPTY),
        _ => false,
    }
}

pub(super) fn reached(changes: &[Change], at: &[String]) -> bool {
    changes.iter().any(|change| change.at.starts_with(at) || at.starts_with(&change.at))
}

pub(super) fn before<'a>(changes: &'a [Change], at: &[String]) -> Option<&'a Json> {
    changes.iter().find(|change| change.at == at).and_then(|change| change.before.as_ref())
}

pub(super) fn arps(json: &Json, at: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    if let Some(index) = at.iter().position(|step| step == "arp").filter(|index| index.checked_sub(2).is_some_and(|rack| at[rack] == "rack")) {
        if !out.iter().any(|known| known[..] == at[..=index]) {
            out.push(at[..=index].to_vec());
        }
        return;
    }
    let children: Vec<(String, &Json)> = match json {
        Json::Object(fields) => fields.iter().map(|(key, value)| (key.clone(), value)).collect(),
        Json::Array(items) => items.iter().enumerate().map(|(index, item)| (index.to_string(), item)).collect(),
        _ => Vec::new(),
    };
    for (step, child) in children {
        at.push(step);
        arps(child, at, out);
        at.pop();
    }
}

pub(super) fn short(json: &Json) -> String {
    match json {
        Json::Object(fields) => format!("an object of {}", counted(fields.len(), "key")),
        Json::Array(items) => format!("a list of {}", items.len()),
        scalar => {
            let text = scalar.write_at(0);
            if text.chars().count() > SHORT { format!("{}…", text.chars().take(SHORT).collect::<String>()) } else { text }
        }
    }
}

pub(super) struct Change {
    pub(super) at: Vec<String>,
    pub(super) before: Option<Json>,
}

pub(super) fn changes(before: &Json, after: &Json) -> Vec<Change> {
    let mut out = Vec::new();
    changed(Some(before), Some(after), &mut Vec::new(), &mut out);
    out
}

pub(super) fn song_changes(before: &Json, after: &Json) -> Vec<Change> {
    changes(before, after).into_iter().filter(|change| change.at.first().is_none_or(|step| !MOVING.contains(&step.as_str()))).collect()
}

fn changed(before: Option<&Json>, after: Option<&Json>, at: &mut Vec<String>, out: &mut Vec<Change>) {
    match (before, after) {
        (Some(Json::Object(old)), Some(Json::Object(new))) => {
            let root = at.is_empty();
            let added = new.iter().filter(|(key, _)| !old.iter().any(|(known, _)| known == key));
            for (key, _) in old.iter().chain(added).filter(|(key, _)| !(root && PASSING.contains(&key.as_str()))) {
                at.push(key.clone());
                changed(before.and_then(|json| json.get(key)), after.and_then(|json| json.get(key)), at, out);
                at.pop();
            }
        }
        (Some(Json::Array(old)), Some(Json::Array(new))) if old.len() == new.len() => {
            for (index, (old, new)) in old.iter().zip(new).enumerate() {
                at.push(index.to_string());
                changed(Some(old), Some(new), at, out);
                at.pop();
            }
        }
        (Some(Json::Number(old)), Some(Json::Number(new))) if tidy(*old) == tidy(*new) => {}
        (old, new) if old == new => {}
        (old, _) => out.push(Change { at: at.clone(), before: old.cloned() }),
    }
}

pub(super) fn lost(from: &Json, to: &Json, other: &Json, at: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    let held = |list: &[Json], item: &Json| list.iter().any(|known| alike(known, item));
    match (from, to) {
        _ if alike(from, to) => {}
        (Json::Object(_) | Json::Null, Json::Object(_) | Json::Null) => {
            let keys = |json: &Json| -> Vec<String> {
                match json {
                    Json::Object(fields) => fields.iter().map(|(key, _)| key.clone()).collect(),
                    _ => Vec::new(),
                }
            };
            let mut all = keys(from);
            all.extend(keys(to).into_iter().filter(|key| from.get(key).is_none()));
            for key in all {
                at.push(key.clone());
                lost(part(from, &key), part(to, &key), part(other, &key), at, out);
                at.pop();
            }
        }
        (Json::Array(_) | Json::Null, Json::Array(_) | Json::Null) => {
            let (old, new, kept) = (from.as_array().unwrap_or(&[]), to.as_array().unwrap_or(&[]), other.as_array().unwrap_or(&[]));
            let mut left: Vec<&Json> = new.iter().collect();
            let gone: Vec<usize> = (0..old.len()).filter(|&index| left.iter().position(|item| alike(&old[index], item)).map(|found| left.remove(found)).is_none()).collect();
            for index in gone {
                at.push(index.to_string());
                match left.iter().position(|item| kin(&old[index], item)) {
                    Some(found) => lost(&old[index], left.remove(found), kept.iter().find(|item| kin(item, &old[index])).or(kept.get(index)).unwrap_or(&Json::Null), at, out),
                    None if !held(kept, &old[index]) => out.push(at.clone()),
                    None => {}
                }
                at.pop();
            }
            if left.iter().any(|added| held(kept, added) && !held(old, added)) {
                out.push(at.clone());
            }
        }
        _ if !alike(from, other) => out.push(at.clone()),
        _ => {}
    }
}

fn part<'a>(json: &'a Json, key: &str) -> &'a Json {
    json.get(key).unwrap_or(&Json::Null)
}

fn kin(a: &Json, b: &Json) -> bool {
    let plain = |fields: &[(String, Json)]| Json::Object(fields.iter().filter(|(_, value)| !matches!(value, Json::Object(_) | Json::Array(_))).cloned().collect());
    matches!((a, b), (Json::Object(old), Json::Object(new)) if alike(&plain(old), &plain(new)))
}

fn alike(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Object(old), Json::Object(new)) => old.len() == new.len() && old.iter().all(|(key, value)| b.get(key).is_some_and(|other| alike(value, other))),
        (Json::Array(old), Json::Array(new)) => old.len() == new.len() && old.iter().zip(new).all(|(was, now)| alike(was, now)),
        (Json::Number(old), Json::Number(new)) => tidy(*old) == tidy(*new),
        _ => a == b,
    }
}

fn holder(at: &[String]) -> &[String] {
    at[..at.len().saturating_sub(1)].iter().rposition(|step| step.parse::<usize>().is_ok()).map_or(at, |index| &at[..=index])
}

pub(super) fn reverted(current: &Json, after: &Json, changes: &[Change]) -> Result<Json, String> {
    let mut moved: Vec<String> = changes.iter().map(|change| holder(&change.at)).filter(|held| locate(current, held) != locate(after, held)).map(joined).collect();
    moved.dedup();
    if !moved.is_empty() {
        return Err(format!("{} changed since; read and write instead of undoing", moved.join(", ")));
    }
    let mut next = current.clone();
    for change in changes {
        place(&mut next, &change.at, change.before.clone().unwrap_or(Json::Null))?;
    }
    Ok(next)
}

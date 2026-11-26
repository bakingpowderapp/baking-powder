use super::{arrange, counted, path};
use crate::project::{Denial, Host, LaneLfo, Landed, Plan, Preset, Project, RACK_ROWS, Row, SEND_COUNT, settled_index};
use sdk::json::Json;

type Fields = Vec<(String, Json)>;
type Landing = (Vec<String>, String, Vec<Option<usize>>);

const PRESET: &str = "preset";
const RACK: &str = "rack";
const MASTER: &str = "master";
const LEFT_OUT: [&str; 4] = ["description", "author", "website", "lfos"];
const SOUND_KEYS: [&str; 2] = ["rack", "lfos"];
const AIMED: [&str; 2] = ["lfos", "curves"];
const LANDED: [&str; 5] = ["rack", "lfos", "curves", "sound", "soundDescription"];
const AIM: [&str; 4] = ["row", "param", "send", "lfoParam"];

#[derive(Default)]
pub(super) struct Report {
    pub loaded: Vec<String>,
    pub pitch: Vec<String>,
    pub rewritten: Vec<Vec<String>>,
    pub as_sent: Vec<Vec<String>>,
}

const OSCS: [&str; 4] = ["osc1", "osc2", "osc3", "osc4"];

fn noted(landed: &Json, site: &str, file: &str, out: &mut Report) {
    let mut found = Vec::new();
    pitched(landed, &mut Vec::new(), &mut found);
    for (at, offsets) in found {
        let site = if at.is_empty() { String::from(site) } else { format!("{site}/{at}") };
        out.pitch.push(format!("{site}: {file}: {offsets}"));
    }
}

fn pitched(json: &Json, at: &mut Vec<String>, out: &mut Vec<(String, String)>) {
    let nonzero = |value: &Json| value.as_f64().filter(|value| *value != 0.0);
    let signed = |value: f64| if value > 0.0 { format!("+{value}") } else { value.to_string() };
    match json {
        Json::Object(fields) => {
            let mut found = Vec::new();
            for (key, value) in fields {
                if OSCS.contains(&key.as_str()) {
                    if let Some(semitones) = value.get("tuneSemitones").and_then(nonzero) {
                        found.push(format!("{key} {} st", signed(semitones)));
                    }
                    if let Some(cents) = value.get("fineCents").and_then(nonzero) {
                        found.push(format!("{key} {} c", signed(cents)));
                    }
                    if let (Some(unison), Some(cents)) = (value.get("unison").and_then(Json::as_f64).filter(|unison| *unison > 1.0), value.get("detuneCents").and_then(nonzero)) {
                        found.push(format!("{key} unison {unison} detune {cents} c"));
                    }
                } else if let Some(osc) = key.strip_suffix("Semitones").filter(|osc| OSCS.contains(osc))
                    && let Some(semitones) = nonzero(value)
                {
                    found.push(format!("mod {osc} {} st", signed(semitones)));
                }
            }
            if !found.is_empty() {
                out.push((path::joined(at), found.join(", ")));
            }
            for (key, value) in fields {
                at.push(key.clone());
                pitched(value, at, out);
                at.pop();
            }
        }
        Json::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                at.push(index.to_string());
                pitched(item, at, out);
                at.pop();
            }
        }
        _ => {}
    }
}

struct Site {
    at: Vec<String>,
    file: String,
    preset: Json,
    beside: Fields,
    replacing: bool,
}

impl Site {
    fn kind(&self) -> &str {
        self.preset.get("kind").and_then(Json::as_str).unwrap_or_default()
    }

    fn written(&self, key: &str) -> bool {
        self.beside.iter().any(|(known, _)| known == key)
    }

    fn merged(&self, left_out: &[&str]) -> Json {
        let Json::Object(own) = &self.preset else {
            return Json::Null;
        };
        let mut landed: Fields = own.iter().filter(|(key, _)| !left_out.contains(&key.as_str())).cloned().collect();
        for (key, value) in &self.beside {
            match landed.iter_mut().find(|(known, _)| known == key) {
                Some((_, slot)) => *slot = value.clone(),
                None => landed.push((key.clone(), value.clone())),
            }
        }
        Json::Object(landed)
    }

    fn scratch(&self) -> Json {
        if !self.written(RACK) {
            return Json::Object(self.beside.iter().filter(|(key, _)| !AIMED.contains(&key.as_str())).cloned().collect());
        }
        let (name, description) = self.named();
        let own = [("sound", Some(Json::String(String::from(name)))), ("soundDescription", Some(Json::String(String::from(description)))), ("lfos", self.preset.get("lfos").cloned())];
        let added = own.into_iter().filter(|(key, _)| !self.written(key)).filter_map(|(key, value)| value.map(|value| (String::from(key), value)));
        Json::Object(self.beside.iter().cloned().chain(added).collect())
    }

    fn entries(&self, key: &str) -> impl Iterator<Item = &Json> {
        self.beside.iter().filter(move |(known, _)| known == key).filter_map(|(_, list)| list.as_array()).flatten()
    }

    fn lanes(&self, rowed: bool) -> Vec<Json> {
        self.entries("lfos").filter(|lane| lane.get("row").is_some() == rowed).cloned().collect()
    }

    fn faded(&self) -> Vec<Json> {
        if self.kind() == RACK && !self.written(RACK) { self.lanes(false) } else { Vec::new() }
    }

    fn curves(&self, rows: &[Option<usize>]) -> (Json, usize) {
        let kept: Vec<Json> = self.entries("curves").filter_map(|curve| rowed(curve, rows)).collect();
        let lost = self.entries("curves").count() - kept.len();
        (Json::Array(kept), lost)
    }

    fn typed(&self, host: &[String], rows: &[Option<usize>], landed: &Json) -> Vec<(Vec<String>, Json)> {
        let mut found = Vec::new();
        for key in ["lfos", "curves"] {
            let list = [host, &[String::from(key)]].concat();
            let held = path::locate(landed, &list).and_then(Json::as_array).unwrap_or(&[]);
            let mut taken = Vec::new();
            for lane in self.entries(key).filter_map(|lane| rowed(lane, rows)) {
                if let Some(index) = (0..held.len()).find(|index| !taken.contains(index) && AIM.iter().all(|key| held[*index].get(key) == lane.get(key))) {
                    taken.push(index);
                    found.push(([&list[..], &[index.to_string()]].concat(), lane));
                }
            }
        }
        let plain = self.entries(RACK).enumerate().filter(|(_, row)| row.get(PRESET).is_none());
        found.extend(plain.filter_map(|(index, row)| Some(([host, &[String::from(RACK), rows.get(index).copied().flatten()?.to_string()]].concat(), row.clone()))));
        found
    }

    fn document(&self) -> Json {
        let mut document = self.preset.clone();
        if let Json::Object(fields) = &mut document
            && self.written("lfos")
        {
            fields.retain(|(known, _)| known != "lfos");
            fields.push((String::from("lfos"), Json::Array(self.lanes(true))));
        }
        document
    }

    fn overlaid(&self, rows: &[Option<usize>]) -> Vec<(String, Json)> {
        if self.kind() != RACK {
            return self.beside.iter().filter(|(key, _)| key != "lfos").cloned().collect();
        }
        if self.written(RACK) {
            return Vec::new();
        }
        let aimed = |(key, value): &(String, Json)| (key.clone(), if key == "curves" { self.curves(rows).0 } else { value.clone() });
        self.beside.iter().filter(|(key, _)| !SOUND_KEYS.contains(&key.as_str())).map(aimed).collect()
    }

    fn brought(&self, at: &[String], rows: &[Option<usize>]) -> Vec<(String, Vec<String>)> {
        if self.kind() != RACK {
            return vec![(path::joined(&self.at), at.to_vec())];
        }
        let landed = rows.iter().flatten().filter(|_| !self.written(RACK));
        landed.map(|row| [at, &[String::from(RACK), row.to_string()]].concat()).map(|place| (path::joined(&place), place)).collect()
    }

    fn named(&self) -> (&str, &str) {
        let name = std::path::Path::new(&self.file).file_stem().and_then(|stem| stem.to_str()).unwrap_or(&self.file);
        (name, self.preset.get("description").and_then(Json::as_str).unwrap_or_default())
    }

    fn refused(&self, why: Denial) -> String {
        let place = path::joined(&self.at);
        match why {
            Denial::NoRoom(short) => format!("{place}: {} does not fit, it would be {}", self.file, arrange::crowding(short)),
            Denial::EffectsOnly => format!("{place} takes effects only, so {} does not land there", self.file),
        }
    }
}

pub(super) fn poured(sent: &mut Json, steps: &mut Vec<String>, next: &mut Json, manual: &Json, fetch: &dyn Fn(&str) -> Result<Json, String>, out: &mut Report) -> Result<(), String> {
    let mut sites = Vec::new();
    if steps.last().is_some_and(|step| step == PRESET) {
        let file = sent.as_str().map(String::from).ok_or_else(|| format!("{} takes a preset path from library", path::joined(steps)))?;
        steps.pop();
        *sent = path::locate(next, steps).cloned().unwrap_or(Json::Object(Vec::new()));
        sites.push(site(&file, steps.clone(), Vec::new(), true, fetch)?);
    } else {
        within(sent, &mut steps.clone(), fetch, &mut sites)?;
    }
    if sites.is_empty() {
        return Ok(());
    }
    let alone = sites[0].at == *steps && sites[0].kind() != RACK;
    let appended = alone && path::locate(next, steps).is_none();
    let mut scratch = next.clone();
    if !alone {
        path::opened(&mut scratch, steps, manual);
        path::place(&mut scratch, steps, sent.clone())?;
    }
    let mut project = Project::from_json(&scratch).ok_or("The document would not read as a project")?;
    let mut landings = Vec::new();
    for site in sites.iter().rev() {
        landings.push(if site.kind() == RACK { sound(&mut project, &scratch, site)? } else { row(&mut project, &scratch, site, appended, !alone)? });
    }
    landings.reverse();
    let mut landed = project.to_json();
    for (site, (at, line, rows)) in sites.iter().zip(&landings) {
        for (key, value) in site.overlaid(rows) {
            path::place(&mut landed, &[at.clone(), vec![key]].concat(), value)?;
        }
        let faded = site.faded();
        if !faded.is_empty() {
            let lanes = [at.clone(), vec![String::from("lfos")]].concat();
            let kept = path::locate(&landed, &lanes).and_then(Json::as_array).map(<[Json]>::to_vec).unwrap_or_default();
            path::place(&mut landed, &lanes, Json::Array(kept.into_iter().chain(faded).collect()))?;
        }
        out.loaded.push(line.clone());
        for (said, place) in site.brought(at, rows) {
            noted(path::locate(&landed, &place).unwrap_or(&Json::Null), &said, &site.file, out);
        }
        for (place, typed) in site.typed(&at[..if at[0] == MASTER { 1 } else { 2 }], rows, &landed) {
            if path::locate(&landed, &place) != Some(&typed) {
                let mut lines = Vec::new();
                path::differences(&typed, path::locate(&landed, &place), path::manual_at(manual, &landed, &place).flatten(), "", &|_| None, &mut lines);
                if lines.is_empty() {
                    continue;
                }
                let mut as_sent = landed.clone();
                path::place(&mut as_sent, &place, typed)?;
                if Project::from_json(&as_sent) != Project::from_json(&landed) {
                    continue;
                }
                landed = as_sent;
            }
            out.as_sent.push(place);
        }
    }
    let own = if sites[0].at == *steps { landings[0].0.clone() } else { steps.clone() };
    let mut rewrote = false;
    for (at, ..) in &landings {
        let host = &at[..if at[0] == MASTER { 1 } else { 2 }];
        let value = path::locate(&landed, host).cloned().unwrap_or(Json::Null);
        if host.starts_with(steps) {
            relaid(sent, &host[steps.len()..], value)?;
        } else {
            relaid(next, host, value)?;
            *sent = path::locate(&landed, &own).cloned().unwrap_or(Json::Null);
            out.rewritten.push(host.to_vec());
            rewrote = true;
        }
    }
    if rewrote {
        *steps = own;
    }
    Ok(())
}

fn rowed(lane: &Json, rows: &[Option<usize>]) -> Option<Json> {
    match (lane, lane.get("row").map(|row| row.as_index(RACK_ROWS).and_then(|row| rows.get(row).copied().flatten()))) {
        (Json::Object(fields), Some(Some(row))) => Some(Json::Object(fields.iter().map(|(key, value)| (key.clone(), if key == "row" { Json::Number(row as f64) } else { value.clone() })).collect())),
        (_, Some(_)) => None,
        (_, None) => Some(lane.clone()),
    }
}

pub(super) fn settled(project: &Project, host: Host, entries: &[Json], index: usize) -> Option<usize> {
    let rows: Vec<Row> = entries.iter().filter_map(Row::from_json).collect();
    let at = entries.iter().take(index).filter(|entry| Row::from_json(entry).is_some()).count();
    settled_index(project.cores(host), &rows, at)
}

fn relaid(json: &mut Json, host: &[String], landed: Json) -> Result<(), String> {
    let beside: Fields = match path::locate(json, host) {
        Some(Json::Object(fields)) => fields.iter().filter(|(key, _)| !LANDED.contains(&key.as_str())).cloned().collect(),
        _ => Fields::new(),
    };
    path::place(json, host, landed)?;
    beside.into_iter().try_for_each(|(key, value)| path::place(json, &[host, &[key]].concat(), value))
}

fn within(json: &mut Json, at: &mut Vec<String>, fetch: &dyn Fn(&str) -> Result<Json, String>, sites: &mut Vec<Site>) -> Result<(), String> {
    let asked = match json {
        Json::Object(fields) => fields.iter().position(|(key, value)| key == PRESET && matches!(value, Json::String(_))).map(|index| {
            let (_, file) = fields.remove(index);
            (file, std::mem::take(fields))
        }),
        _ => None,
    };
    if let Some((file, beside)) = asked {
        let site = site(file.as_str().unwrap_or_default(), at.clone(), beside, false, fetch)?;
        *json = if site.kind() == RACK { site.scratch() } else { site.merged(&LEFT_OUT) };
        sites.push(site);
    }
    match json {
        Json::Object(fields) => fields.iter_mut().try_for_each(|(key, value)| {
            at.push(key.clone());
            let done = within(value, at, fetch, sites);
            at.pop();
            done
        }),
        Json::Array(items) => items.iter_mut().enumerate().try_for_each(|(index, item)| {
            at.push(index.to_string());
            let done = within(item, at, fetch, sites);
            at.pop();
            done
        }),
        _ => Ok(()),
    }
}

fn site(file: &str, at: Vec<String>, beside: Fields, replacing: bool, fetch: &dyn Fn(&str) -> Result<Json, String>) -> Result<Site, String> {
    let fetched = fetch(file)?;
    let preset = fetched.get("document").filter(|_| fetched.get("kind").and_then(Json::as_str) == Some(PRESET)).cloned().ok_or_else(|| format!("{file} is not a preset; library lists each preset with kind preset"))?;
    let site = Site { at, file: String::from(file), preset, beside, replacing };
    let place = path::joined(&site.at);
    match site.kind() {
        "chords" => Err(format!("{file} is a chords preset; library gives its columns for palette")),
        RACK if !matches!(&site.at[..], [tracks, index] if tracks == "tracks" && index.parse::<usize>().is_ok()) => Err(format!("{file} is a whole sound and lands on a track; {place} is not one")),
        kind if kind != RACK && host_of(&site.at).is_none() => Err(format!("{file} is a {kind} row and lands at .../rack/N; {place} is not one")),
        _ => Ok(site),
    }
}

pub(super) fn reshaped(at: &[String]) -> Option<&[String]> {
    let hosted = match at {
        [master, ..] if master == MASTER => 1,
        [list, ..] if list == "tracks" || list == "returns" => 2,
        _ => return None,
    };
    let reshapes = match at.get(hosted..)? {
        [key] => key == RACK || key == "kind",
        [rack, _] => rack == RACK,
        [rack, _, kind] => rack == RACK && kind == "kind",
        _ => false,
    };
    reshapes.then(|| &at[..hosted])
}

fn host_of(at: &[String]) -> Option<(Host, usize)> {
    let index = |step: &String| step.parse::<usize>().ok();
    match at {
        [master, rack, row] if master == MASTER && rack == RACK => Some((Host::Master, index(row)?)),
        [tracks, track, rack, row] if tracks == "tracks" && rack == RACK => Some((Host::Track(index(track)?), index(row)?)),
        [returns, send, rack, row] if returns == "returns" && rack == RACK => Some((Host::Send(index(send).filter(|&send| send < SEND_COUNT)?), index(row)?)),
        _ => None,
    }
}

fn sound(project: &mut Project, scratch: &Json, site: &Site) -> Result<Landing, String> {
    let track = site.at[1].parse::<usize>().ok().filter(|&track| track < project.tracks.len()).ok_or_else(|| format!("{} is not a track", path::joined(&site.at)))?;
    let Some(preset @ Preset::Rack(_)) = Preset::from_json(&site.document()) else {
        return Err(format!("{} would not read as a sound", site.file));
    };
    let (kept, moved) = if site.written(RACK) { (false, (0..project.tracks[track].rack.len()).map(Some).collect()) } else { landed_sound(project, site, track, preset)? };
    let landed: Vec<usize> = moved.iter().copied().flatten().collect();
    let owner = &project.tracks[track];
    let rows: Vec<String> = kept.then_some(0).iter().chain(&landed).filter_map(|&row| kind_of(&owner.rack[row])).collect();
    let lanes = owner.automation.lfos.iter().filter(|lfo| lfo.aim.row().is_some_and(|row| landed.contains(&row))).count();
    let list = |key: &str| site.preset.get(key).and_then(Json::as_array).map_or(0, <[Json]>::len);
    let mut line = format!("{}: {}, {}, {}", path::joined(&site.at), site.file, counted(rows.len(), "row"), rows.join(", "));
    if lanes > 0 {
        line.push_str(&format!(", {}", counted(lanes, "LFO lane")));
    }
    let beside: Vec<&str> = SOUND_KEYS.into_iter().filter(|key| site.written(key) && (*key == RACK || list(key) > 0)).collect();
    if !beside.is_empty() {
        line.push_str(&format!("; its {} as written beside preset", beside.join(" and ")));
    }
    let curves = if site.written(RACK) { 0 } else { site.curves(&moved).1 };
    for (lost, noun) in [(site.lanes(true).len().saturating_sub(lanes), "written LFO lane"), (curves, "written curve")] {
        if lost > 0 {
            line.push_str(&format!("; {} dropped, no such row or knob", counted(lost, noun)));
        }
    }
    let written = path::locate(scratch, &[&site.at[..], &[String::from(RACK)]].concat()).and_then(Json::as_array).filter(|_| site.written(RACK));
    let rows = written.map_or(moved, |entries| (0..entries.len()).map(|index| Row::from_json(&entries[index]).and_then(|_| settled(project, Host::Track(track), entries, index))).collect());
    Ok((vec![String::from("tracks"), track.to_string()], line, rows))
}

pub(super) fn kind_of(row: &Row) -> Option<String> {
    row.to_json().get("kind").and_then(Json::as_str).map(String::from)
}

fn on_row(lanes: Option<&Json>, row: usize) -> usize {
    lanes.and_then(Json::as_array).unwrap_or(&[]).iter().filter(|lane| lane.get("row").and_then(|at| at.as_index(RACK_ROWS)) == Some(row)).count()
}

fn landed_sound(project: &mut Project, site: &Site, track: usize, preset: Preset) -> Result<(bool, Vec<Option<usize>>), String> {
    let host = Host::Track(track);
    let plan = project.plan(host, &preset, None, false).map_err(|why| site.refused(why))?;
    let kept = site.replacing && plan == Plan::Replaces && project.tracks[track].rack.first().is_some_and(Row::is_midi) && !preset.rows().iter().any(|row| row.is_midi());
    let (Landed::Sound(rows) | Landed::Joined(rows)) = project.land(host, preset, plan, None, site.named()) else {
        return Err(format!("{} would not read as a sound", site.file));
    };
    Ok((kept, rows))
}

fn row(project: &mut Project, scratch: &Json, site: &Site, appended: bool, nested: bool) -> Result<Landing, String> {
    let kind = site.kind();
    let Some((host, index)) = host_of(&site.at).filter(|(host, _)| host.track().is_none_or(|track| track < project.tracks.len())) else {
        return Err(format!("{} is a {kind} row and lands at .../rack/N; {} is not one", site.file, path::joined(&site.at)));
    };
    let Some(preset @ Preset::Row(..)) = Preset::from_json(&site.merged(&LEFT_OUT[..3])) else {
        return Err(format!("{} would not read as a row", site.file));
    };
    let rack_at = &site.at[..site.at.len() - 1];
    let entries = path::locate(scratch, rack_at).and_then(Json::as_array).unwrap_or(&[]);
    let swapped = if appended {
        if index > entries.len() {
            return Err(format!("{} holds {}; {} appends", path::joined(rack_at), counted(entries.len(), "item"), entries.len()));
        }
        None
    } else {
        Some(settled(project, host, entries, index).ok_or_else(|| site.refused(Denial::EffectsOnly))?)
    };
    let plan = project.plan(host, &preset, swapped, false).map_err(|why| site.refused(why))?;
    let aimed: Vec<LaneLfo> = swapped.filter(|_| nested).map(|at| project.automation(host).lfos.iter().filter(|lfo| lfo.aim.row() == Some(at)).cloned().collect()).unwrap_or_default();
    let other_effect = |at: &usize| !nested && preset.rows().iter().any(|new| new.is_effect() && project.rack(host)[*at].is_effect() && !new.same_kind(&project.rack(host)[*at]));
    let by_key = swapped.filter(other_effect).and_then(|_| {
        let mut turned = scratch.clone();
        path::place(&mut turned, &site.at, site.merged(&LEFT_OUT)).ok()?;
        Project::from_json(&turned)
    });
    let landed = match project.land(host, preset, plan, swapped, site.named()) {
        Landed::Swapped { row, .. } => Some(row),
        Landed::Joined(rows) => rows.into_iter().flatten().next(),
        Landed::Sound(_) | Landed::Cores { .. } => None,
    };
    let row = landed.ok_or_else(|| site.refused(Denial::NoRoom(1)))?;
    if let Some(turned) = by_key {
        project.automation_mut(host).curves = turned.automation(host).curves.clone();
    }
    let stayed = aimed.len();
    if !aimed.is_empty() {
        let lanes = &mut project.automation_mut(host).lfos;
        lanes.retain(|lfo| !aimed.iter().any(|own| own.aim == lfo.aim));
        lanes.extend(aimed);
    }
    let mut line = format!("{}: {}, a {kind} row", path::joined(&site.at), site.file);
    if !site.beside.is_empty() {
        line.push_str(&format!(", its {} as written beside preset", site.beside.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>().join(", ")));
    }
    let mut at: Vec<String> = match host {
        Host::Track(track) => vec![String::from("tracks"), track.to_string()],
        Host::Send(send) => vec![String::from("returns"), send.to_string()],
        Host::Master => vec![String::from(MASTER)],
    };
    if swapped.is_some() {
        let (lanes, held) = (project.automation(host), |key: &str| path::locate(scratch, &[&at[..], &[String::from(key)]].concat()));
        let lost = [
            (if nested { on_row(held("lfos"), index).saturating_sub(stayed) } else { 0 }, "LFO lane"),
            (on_row(held("curves"), index).saturating_sub(lanes.curves.iter().filter(|curve| curve.aim.row() == Some(row)).count()), "curve"),
        ];
        for (lost, noun) in lost.into_iter().filter(|(lost, _)| *lost > 0) {
            line.push_str(&format!("; {} dropped, no such knob", counted(lost, noun)));
        }
    }
    at.extend([String::from(RACK), row.to_string()]);
    Ok((at, line, vec![Some(row)]))
}

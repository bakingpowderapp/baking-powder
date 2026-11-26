use super::write::{Edit, Shift};
use super::notes::Filter;
use super::{path, presets, tools};
use crate::flatten::{self, Flattened};
use crate::roll;
use sdk::json::{Json, tidy};
use crate::project::codec::clip_from_json;
use crate::project::{Automation, Clip, Cores, Host, LONGEST_TICKS, Note, Project, RACK_ROWS, Row, SEND_COUNT, TICKS_PER_BAR, TRACKS, Track, beats, continues, joined, stacked, taken, ticks};

const MOST_COPIES: i32 = 1024;
const MOST_LAID: usize = 4096;
const MOST_BARS: usize = 6;
const MOST_MATCHED: usize = 12;
const STACKS: &str = "join takes beats, or a list of two or more MIDI clip paths like tracks/0/clips/2, one a track";

#[derive(Clone, Copy)]
pub(super) enum Verb {
    Insert,
    Remove,
    Copy,
    Join,
}

impl Verb {
    fn name(self) -> &'static str {
        match self {
            Verb::Insert => "insert",
            Verb::Remove => "remove",
            Verb::Copy => "copy",
            Verb::Join => "join",
        }
    }
}

#[derive(Clone, Copy)]
enum Scope {
    Whole,
    Track(usize),
}

impl Scope {
    fn holds(self, track: usize) -> bool {
        match self {
            Scope::Whole => true,
            Scope::Track(at) => at == track,
        }
    }

    fn whole(self) -> bool {
        matches!(self, Scope::Whole)
    }
}

#[derive(Clone, Copy)]
struct Marks {
    playhead: i32,
    looped: Option<(i32, i32)>,
}

#[derive(Clone, Copy)]
struct Beat {
    said: f64,
    tick: i32,
}

pub(super) fn verb(operation: &str) -> Option<Verb> {
    match operation {
        "insert" => Some(Verb::Insert),
        "remove" => Some(Verb::Remove),
        "copy" => Some(Verb::Copy),
        "join" => Some(Verb::Join),
        _ => None,
    }
}

pub(super) fn arranged(verb: Verb, args: &Json, document: &Json, steps: &[String]) -> Result<Vec<Edit>, String> {
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let scope = scoped(verb.name(), steps, project.tracks.len())?;
    let length = length_arg(args, verb.name())?;
    let from = beat_arg(args, "fromBeat", &format!("fromBeat is needed: the beat {} starts at, from 0", verb.name()), "fromBeat is a beat from 0")?;
    let before = marks_of(document);
    let (did, after) = match verb {
        Verb::Insert => inserted(&mut project, scope, from, length, before)?,
        Verb::Remove => removed(&mut project, scope, from, length, before),
        Verb::Copy => {
            let to = beat_arg(args, "toBeat", "toBeat is needed: the beat the first copy lands on", "toBeat is a beat from 0")?;
            (copied(&mut project, scope, from, length, to, copies_arg(args)?)?, before)
        }
        Verb::Join => (consolidated(&mut project, scope, from, length)?, before),
    };
    edits(&project, document, scope, before, after, &did)
}

pub(super) fn cut(given: Option<&Json>, document: &Json, steps: &[String]) -> Result<Vec<Edit>, String> {
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let scope = scoped("cut", steps, project.tracks.len())?;
    let shape = "cut is a song beat above 0, or a list of them";
    let said: Vec<f64> = match given {
        Some(Json::Array(beats)) if !beats.is_empty() => beats.iter().map(|beat| beat.as_f64().ok_or(shape)).collect::<Result<_, _>>()?,
        Some(beat) => vec![beat.as_f64().ok_or(shape)?],
        None => return Err(String::from(shape)),
    };
    if said.len() > MOST_LAID {
        return Err(format!("cut takes {MOST_LAID} beats at most, and this list holds {}", said.len()));
    }
    let mut at = Vec::with_capacity(said.len());
    for beat in said {
        if !(beat.is_finite() && beat > 0.0) {
            return Err(String::from(shape));
        }
        at.push(within("cut", beat, ticks(beat))?);
    }
    let bpm = project.bpm;
    let (mut split, mut whole) = (0, Vec::new());
    for beat in &at {
        let before = split;
        for track in tracks_in(&mut project, scope) {
            split += usize::from(track.cut(beat.tick, bpm));
        }
        if split == before {
            whole.push(beat.said.to_string());
        }
    }
    let landed = at.iter().map(|beat| beat.tick);
    let (first, last) = (landed.clone().min().unwrap_or(0), landed.max().unwrap_or(0));
    let wide = if let [one] = at.as_slice() { format!("beat {}", one.said) } else { format!("{} beats", at.len()) };
    let more = if whole.len() > MOST_BARS { format!(" and {} more", whole.len() - MOST_BARS) } else { String::new() };
    let spared = if whole.is_empty() { String::new() } else { format!("; no clip to split at {}{more}", whole[..whole.len().min(MOST_BARS)].join(", ")) };
    let did = format!("Cut at {wide}, {}: {}{spared}", bars(&[(first, last + 1)]), tally(split, "clip", " split in two").unwrap_or_else(|| String::from("nothing split")));
    let marks = marks_of(document);
    edits(&project, document, scope, marks, marks, &did)
}

fn scoped(verb: &str, steps: &[String], tracks: usize) -> Result<Scope, String> {
    let index = match steps {
        [] => return Ok(Scope::Whole),
        [list, index] if list == "tracks" => index.parse::<usize>().ok(),
        _ => None,
    };
    let Some(index) = index else {
        return Err(format!("{verb} takes tracks/N, or no path for the whole song; {} is neither", path::joined(steps)));
    };
    if index >= tracks {
        return Err(format!("{} is past the end: the project has {}, the last tracks/{}", path::joined(steps), counted(tracks, "track"), tracks.saturating_sub(1)));
    }
    Ok(Scope::Track(index))
}

fn length_arg(args: &Json, name: &str) -> Result<Beat, String> {
    let said = args.get(name).and_then(Json::as_f64).filter(|length| length.is_finite() && *length > 0.0).ok_or_else(|| format!("{name} is a number of beats above 0"))?;
    let tick = ticks(said);
    if tick < 1 {
        return Err(format!("{name} {said} rounds to no tick, 960 to a beat"));
    }
    within(name, said, tick)
}

fn beat_arg(args: &Json, key: &str, needed: &str, wrong: &str) -> Result<Beat, String> {
    let said = args.get(key).ok_or(needed)?.as_f64().filter(|beat| beat.is_finite() && *beat >= 0.0).ok_or(wrong)?;
    within(key, said, ticks(said))
}

fn within(key: &str, said: f64, tick: i32) -> Result<Beat, String> {
    if tick > LONGEST_TICKS {
        return Err(format!("{key} {} is past beat {}, the song's limit", tidy(said), beats(LONGEST_TICKS)));
    }
    Ok(Beat { said: tidy(said), tick })
}

fn copies_arg(args: &Json) -> Result<i32, String> {
    match args.get("copies") {
        None | Some(Json::Null) => Ok(1),
        Some(value) => {
            let copies = value.as_f64().filter(|copies| copies.fract() == 0.0 && *copies >= 1.0).ok_or("copies is a whole number from 1")?;
            if copies > f64::from(MOST_COPIES) {
                return Err(format!("copies is at most {MOST_COPIES}; copy the copies for more"));
            }
            Ok(copies as i32)
        }
    }
}

fn marks_of(document: &Json) -> Marks {
    let tick = |key: &str| document.get(key).and_then(Json::as_f64).map(|beat| ticks(beat).min(LONGEST_TICKS));
    let looped = match (tick("loopStartBeat"), tick("loopEndBeat")) {
        (Some(start), Some(end)) if start >= 0 && end > start => Some((start, end)),
        _ => None,
    };
    Marks { playhead: tick("playheadBeat").unwrap_or(0).max(0), looped }
}

fn tracks_in(project: &mut Project, scope: Scope) -> impl Iterator<Item = &mut Track> {
    project.tracks.iter_mut().enumerate().filter(move |(index, _)| scope.holds(*index)).map(|(_, track)| track)
}

fn clips_in(project: &Project, scope: Scope) -> impl Iterator<Item = &Clip> {
    project.tracks.iter().enumerate().filter(move |(index, _)| scope.holds(*index)).flat_map(|(_, track)| &track.clips)
}

fn lanes_in(project: &Project, scope: Scope) -> impl Iterator<Item = &Automation> {
    let tracks = project.tracks.iter().enumerate().filter(move |(index, _)| scope.holds(*index)).map(|(_, track)| &track.automation);
    tracks.chain(project.returns().filter(move |_| scope.whole()).map(|ret| &ret.automation))
}

fn each_lane(project: &mut Project, scope: Scope, mut work: impl FnMut(&mut Automation)) {
    for track in tracks_in(project, scope) {
        work(&mut track.automation);
    }
    if scope.whole() {
        for ret in project.returns_mut() {
            work(&mut ret.automation);
        }
    }
}

fn node_ticks(lane: &Automation) -> impl Iterator<Item = i32> {
    lane.curves.iter().flat_map(|curve| &curve.nodes).map(|node| node.tick)
}

fn inserted(project: &mut Project, scope: Scope, from: Beat, length: Beat, before: Marks) -> Result<(String, Marks), String> {
    let (at, by) = (from.tick, length.tick);
    let moved = clips_in(project, scope).filter(|clip| clip.start >= at).count();
    let split = clips_in(project, scope).filter(|clip| clip.start < at && at < clip.end()).count();
    let nodes = lanes_in(project, scope).flat_map(node_ticks).filter(|tick| *tick >= at).count();
    let reach = clips_in(project, scope).map(Clip::end).filter(|end| *end > at).chain(lanes_in(project, scope).flat_map(node_ticks).filter(|tick| *tick >= at)).max();
    if reach.is_some_and(|reach| i64::from(reach) + i64::from(by) > i64::from(LONGEST_TICKS)) {
        return Err(format!("insert {} at beat {} would run past beat {}, the song's limit", length.said, from.said, beats(LONGEST_TICKS)));
    }
    match scope {
        Scope::Whole => project.insert_time(at, by),
        Scope::Track(index) => {
            let bpm = project.bpm;
            project.tracks[index].insert(at, by, bpm);
        }
    }
    let later = |mark: i32| if mark >= at { (mark + by).min(LONGEST_TICKS) } else { mark };
    let marks = if scope.whole() { Marks { playhead: later(before.playhead), looped: before.looped.map(|(start, end)| (later(start), later(end))).filter(|(start, end)| end > start) } } else { before };
    let said = listed([tally(moved, "clip", " moved"), (split > 0).then(|| format!("{split} split")), tally(nodes, "node", " moved")].into_iter().chain(marked(before, marks)), "nothing lay past it");
    Ok((format!("Inserted {} at beat {}, {}: {said}", beats_said(length.said), from.said, bars(&[(at, at + by)])), marks))
}

fn removed(project: &mut Project, scope: Scope, from: Beat, length: Beat, before: Marks) -> (String, Marks) {
    let (start, by) = (from.tick, length.tick);
    let end = start + by;
    let bpm = project.bpm;
    let (mut cut, mut trimmed, mut moved, mut nodes_cut, mut nodes_moved) = (0, 0, 0, 0, 0);
    for track in tracks_in(project, scope) {
        cut += track.clips.iter().filter(|clip| start <= clip.start && clip.end() <= end).count();
        trimmed += track.clips.iter().filter(|clip| (clip.start < start && start < clip.end()) || (clip.start < end && end < clip.end())).count();
        track.take_between(start, end, bpm);
        for clip in track.clips.iter_mut().filter(|clip| clip.start >= end) {
            clip.start -= by;
            moved += 1;
        }
    }
    each_lane(project, scope, |lane| {
        nodes_cut += node_ticks(lane).filter(|tick| (start..end).contains(tick)).count();
        nodes_moved += node_ticks(lane).filter(|tick| *tick >= end).count();
        lane.close(start, end);
    });
    let earlier = |mark: i32| if mark >= end { mark - by } else { mark.min(start) };
    let marks = if scope.whole() { Marks { playhead: earlier(before.playhead), looped: before.looped.map(|(from, until)| (earlier(from), earlier(until))).filter(|(from, until)| until > from) } } else { before };
    let said = listed([tally(cut, "clip", " cut"), (trimmed > 0).then(|| format!("{trimmed} trimmed")), tally(moved, "clip", " moved"), tally(nodes_cut, "node", " cut"), tally(nodes_moved, "node", " moved")].into_iter().chain(marked(before, marks)), "nothing lay there or after it");
    (format!("Removed beats {} to {}, {}: {said}", from.said, tidy(from.said + length.said), bars(&[(start, end)])), marks)
}

fn copied(project: &mut Project, scope: Scope, from: Beat, length: Beat, to: Beat, copies: i32) -> Result<String, String> {
    let (start, by) = (from.tick, length.tick);
    let end = start + by;
    if i64::from(to.tick) + i64::from(copies) * i64::from(by) > i64::from(LONGEST_TICKS) {
        return Err(format!("copy {} to beat {} with {} would run past beat {}, the song's limit", length.said, to.said, copies_said(copies), beats(LONGEST_TICKS)));
    }
    let (bpm, carry) = (project.bpm, !project.lock);
    let parts_of: Vec<(usize, Vec<Clip>)> = project.tracks.iter().enumerate().filter(|(index, _)| scope.holds(*index)).map(|(index, track)| (index, track.clone().take_between(start, end, bpm))).collect();
    let laying = parts_of.iter().map(|(_, parts)| parts.len()).sum::<usize>() * copies as usize;
    if laying > MOST_LAID {
        return Err(format!("copy would lay {laying} clips, over {MOST_LAID}; copy less, then copy the copies"));
    }
    let shifts: Vec<i32> = (0..copies).map(|copy| to.tick - start + copy * by).collect();
    let (mut laid, mut replaced, mut carried, mut cleared) = (0, 0, 0, 0);
    for (index, parts) in parts_of {
        let track = &mut project.tracks[index];
        let landed = |clip: &Clip| shifts.iter().any(|shift| clip.start < end + shift && start + shift < clip.end());
        replaced += track.clips.iter().filter(|clip| landed(clip)).count();
        for &shift in &shifts {
            track.take_between(start + shift, end + shift, bpm);
            for part in &parts {
                track.place(Clip { start: part.start + shift, ..part.clone() }, bpm);
                laid += 1;
            }
        }
    }
    let locked = !carry && lanes_in(project, scope).any(|lane| !lane.curves.is_empty());
    if carry {
        each_lane(project, scope, |lane| {
            let (curves, edges) = lane.between(start, end);
            let nodes: usize = curves.iter().map(|curve| curve.nodes.len()).sum();
            cleared += node_ticks(lane).filter(|tick| (to.tick..to.tick + copies * by).contains(tick)).count();
            for copy in 0..copies {
                lane.replace(to.tick + copy * by, to.tick + (copy + 1) * by, &edges, &curves);
                carried += nodes;
            }
        });
    }
    let spans: Vec<(i32, i32)> = std::iter::once((start, end)).chain(shifts.iter().map(|shift| (start + shift, end + shift))).collect();
    let clips = match (tally(laid, "clip", " laid"), replaced) {
        (None, 0) => String::from("no clips there"),
        (None, replaced) => format!("no clips to lay, {} there cleared", counted(replaced, "clip")),
        (Some(said), 0) => said,
        (Some(said), replaced) => format!("{said} over {replaced}"),
    };
    let nodes = match (carried, cleared) {
        (0, 0) => None,
        (0, cleared) => Some(format!("{} there cleared", counted(cleared, "node"))),
        (carried, 0) => Some(format!("{} carried", counted(carried, "node"))),
        (carried, cleared) => Some(format!("{} carried, {cleared} there cleared", counted(carried, "node"))),
    };
    let automation = if locked { Some(String::from("automation locked")) } else { nodes.map(|nodes| format!("automation repeats with each copy: {nodes}")) };
    let total = format!("1 original + {} = {} total, {}", copies_said(copies), copies + 1, bars(&spans));
    Ok(format!("Copied beats {} to {} to beat {}: {total}; {clips}{}", from.said, tidy(from.said + length.said), to.said, automation.map(|said| format!("; {said}")).unwrap_or_default()))
}

fn consolidated(project: &mut Project, scope: Scope, from: Beat, length: Beat) -> Result<String, String> {
    let (start, end) = (from.tick, from.tick + length.tick);
    if end > LONGEST_TICKS {
        return Err(format!("join {} at beat {} would run past beat {}, the song's limit", length.said, from.said, beats(LONGEST_TICKS)));
    }
    let bpm = project.bpm;
    let (mut tracks, mut clips, mut notes) = (0, 0, 0);
    for (index, track) in project.tracks.iter_mut().enumerate().filter(|(index, _)| scope.holds(*index)) {
        let parts = track.clone().take_between(start, end, bpm);
        let whole = parts.first().is_some_and(|part| part.start == start) && parts.last().is_some_and(|part| part.end() == end);
        if parts.is_empty() || (parts.len() == 1 && whole) {
            continue;
        }
        let file = match parts.iter().find_map(|part| part.file.clone()) {
            None => None,
            Some(first) if whole && parts.windows(2).all(|pair| continues(&pair[0], &pair[1], bpm)) => Some(first),
            Some(_) => return Err(format!("tracks/{index}: joining its audio clips there needs a bounce; the user joins them in the app with ⌘J")),
        };
        let merged = joined(&parts, start, end, file);
        (tracks, clips, notes) = (tracks + 1, clips + parts.len(), notes + merged.notes.len());
        track.take_between(start, end, bpm);
        track.place(merged, bpm);
    }
    let said = listed([tally(tracks, "track", " joined"), tally(clips, "clip", " merged"), tally(notes, "note", " kept")], "no track held clips to join there");
    Ok(format!("Joined beats {} to {}, {}: {said}", from.said, tidy(from.said + length.said), bars(&[(start, end)])))
}

pub(super) fn stacked_clips(list: &[Json], document: &Json, steps: &[String]) -> Result<Vec<Edit>, String> {
    if !steps.is_empty() {
        return Err(format!("join with a list of clips takes no path; {} was given", path::joined(steps)));
    }
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let mut picked: Vec<(usize, usize, Clip)> = Vec::new();
    for item in list {
        let at = item.as_str().map(str::trim).ok_or(STACKS)?;
        let found = match path::steps(at).as_slice() {
            [tracks, track, clips, index] if tracks == "tracks" && clips == "clips" => track.parse::<usize>().ok().zip(index.parse::<usize>().ok()),
            _ => None,
        };
        let (track, index) = found.ok_or_else(|| format!("{at} is not a clip path like tracks/0/clips/2"))?;
        let owner = project.tracks.get(track).ok_or_else(|| format!("No track at tracks/{track}: the project has {}", counted(project.tracks.len(), "track")))?;
        let clip = sorted_slot(document, track, index).and_then(|slot| owner.clips.get(slot)).ok_or_else(|| format!("No clip at {at}: the track has {}", counted(owner.clips.len(), "clip")))?;
        if owner.audio || clip.file.is_some() {
            return Err(format!("{at} plays a wave; only MIDI clips stack"));
        }
        if picked.iter().any(|(known, ..)| *known == track) {
            return Err(format!("tracks/{track} holds two of the clips; along one track, join takes beats with fromBeat at tracks/{track}"));
        }
        picked.push((track, index, clip.clone()));
    }
    if picked.len() < 2 {
        return Err(String::from(STACKS));
    }
    picked.sort_by_key(|(track, ..)| *track);
    let (top, first) = (picked[0].0, picked[0].2.start);
    let start = picked.iter().map(|(.., clip)| clip.start).min().unwrap_or(first);
    let end = picked.iter().map(|(.., clip)| clip.end()).max().unwrap_or(start);
    let bpm = project.bpm;
    let mut parts = project.tracks[top].take_between(start, end, bpm);
    parts.sort_by_key(|clip| clip.start != first);
    let beside = parts.len().saturating_sub(1);
    parts.extend(picked.iter().skip(1).map(|(.., clip)| clip.clone()));
    let sounding: usize = parts.iter().map(|clip| clip.notes.iter().filter(|note| note.start < clip.beats()).count()).sum();
    let merged = stacked(&parts, start, end);
    let kept = merged.notes.len();
    project.tracks[top].place(merged, bpm);
    let landed = project.tracks[top].clips.iter().position(|clip| clip.start == start).unwrap_or_default();
    let from: Vec<String> = picked.iter().map(|(track, index, _)| format!("tracks/{track}/clips/{index}")).collect();
    let said = listed([tally(kept, "note", " kept"), tally(sounding.saturating_sub(kept), "double", " dropped"), tally(beside, "other clip", &format!(" of tracks/{top} there taken in"))], "no notes");
    let did = format!("Stacked {} into tracks/{top}/clips/{landed}, beats {} to {}, {}: {said}; the lower clips stay", from.join(", "), tidy(beats(start)), tidy(beats(end)), bars(&[(start, end)]));
    let marks = marks_of(document);
    edits(&project, document, Scope::Track(top), marks, marks, &did)
}

pub(super) fn regrouped(given: &Json, document: &Json, steps: &[String]) -> Result<Option<Vec<Edit>>, String> {
    match (steps, given) {
        ([tracks, index], Json::Null) if tracks == "tracks" => dropped(document, index).map(Some),
        ([tracks, index, key], Json::Bool(false) | Json::Null) if tracks == "tracks" && key == "inGroup" => Ok(left_group(document, index)),
        ([.., rack, _], Json::Null) if rack == "rack" => unracked(document, steps),
        _ => Ok(None),
    }
}

fn rack_of<'a>(project: &Project, steps: &'a [String]) -> Option<(Host, &'a [String])> {
    let (host, owner) = match steps {
        [tracks, track, rack, _] if tracks == "tracks" && rack == "rack" => (Host::Track(track.parse().ok()?), &steps[..2]),
        [returns, send, rack, _] if returns == "returns" && rack == "rack" => (Host::Send(send.parse().ok().filter(|send| *send < SEND_COUNT)?), &steps[..2]),
        [master, rack, _] if master == "master" && rack == "rack" => (Host::Master, &steps[..1]),
        _ => return None,
    };
    host.track().is_none_or(|track| track < project.tracks.len()).then_some((host, owner))
}

fn owned(project: &Project, owner: &[String], did: String, said: Option<Json>) -> Result<Vec<Edit>, String> {
    let sent = path::locate(&project.to_json(), owner).cloned().ok_or("The project came back without the rack's owner")?;
    Ok(vec![Edit { steps: owner.to_vec(), sent, did: Some(did), arranged: true, said, shifts: Vec::new() }])
}

fn unracked(document: &Json, steps: &[String]) -> Result<Option<Vec<Edit>>, String> {
    let Some(mut project) = Project::from_json(document) else {
        return Ok(None);
    };
    let Some((host, owner)) = rack_of(&project, steps) else {
        return Ok(None);
    };
    let at = path::joined(steps);
    let entries = path::locate(document, &steps[..steps.len() - 1]).and_then(Json::as_array).unwrap_or(&[]);
    let Some(written) = steps.last().and_then(|row| row.parse::<usize>().ok()) else {
        return Ok(None);
    };
    if written >= entries.len() {
        return Err(format!("No row at {at}: the rack has {}", counted(entries.len(), "row")));
    }
    let Some(index) = presets::settled(&project, host, entries, written) else {
        return Ok(None);
    };
    let rack = project.rack(host);
    if index == 0 && rack[index].is_midi() {
        return Err(format!("{at} is the midi row, which a MIDI track keeps at row 0"));
    }
    let core = rack[index].is_core();
    if core && project.cores(host) == Cores::Required && rack.iter().filter(|row| row.is_core()).count() <= 1 {
        return Err(format!("{at} is the track's only Core, and a MIDI track keeps one"));
    }
    let kind = presets::kind_of(&rack[index]).unwrap_or_default();
    let below = rack.len() - 1 - index;
    let (gone, later) = (aimed(project.automation(host), |row| row == index), aimed(project.automation(host), |row| row > index));
    project.remove_row(host, index);
    crate::project::tidy(project.rack_mut(host));
    if let (true, Host::Track(track)) = (core, host) {
        project.tracks[track].forget_sound();
    }
    let said = listed([tally(below, "row", " below moved up one"), gone.map(|gone| format!("{gone} on it dropped")), later.map(|later| format!("{later} below renumbered"))], "nothing else moved");
    owned(&project, owner, format!("Removed the {kind} row at {at}: {said}"), None).map(Some)
}

pub(super) fn relocated(given: &Json, document: &Json, steps: &[String]) -> Result<Vec<Edit>, String> {
    let to = given.as_f64().filter(|to| *to >= 0.0 && to.fract() == 0.0).map(|to| to as usize).ok_or("move is the index to move to, a whole number from 0")?;
    match steps {
        [tracks, index] if tracks == "tracks" => moved_track(document, index, to),
        [.., rack, _] if rack == "rack" => moved_row(document, steps, to),
        _ => Err(format!("move takes tracks/N or a rack row like tracks/N/rack/R; {} is neither", path::joined(steps))),
    }
}

fn moved_track(document: &Json, index: &str, to: usize) -> Result<Vec<Edit>, String> {
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let count = project.tracks.len();
    let at = index.parse::<usize>().ok().filter(|at| *at < count).ok_or_else(|| format!("No track at tracks/{index}: the project has {}", counted(count, "track")))?;
    let moving: Vec<usize> = std::iter::once(at).chain(project.members_of(at)).collect();
    let others: Vec<usize> = (0..count).filter(|track| !moving.contains(track)).collect();
    if to > others.len() {
        return Err(format!("tracks/{at} can move to tracks/0 through tracks/{}; {to} is past that", others.len()));
    }
    let name = format!(" {}", project.tracks[at].name);
    let marks = marks_of(document);
    if to == at {
        return edits(&project, document, Scope::Whole, marks, marks, &format!("{} already sits at tracks/{at}: nothing moved", name.trim_start()));
    }
    let before = others.get(to).copied().unwrap_or(count);
    let (group, was_inside) = (project.tracks[at].group, project.tracks[at].inside);
    let under = to.checked_sub(1).map(|above| &project.tracks[others[above]]).is_some_and(|above| above.group || above.inside);
    if group && under && project.tracks.get(before).is_some_and(|below| below.inside) {
        return Err(among(to));
    }
    let inside = under && !group;
    let mut order = others.clone();
    order.splice(to..to, moving.iter().copied());
    let renumbered = sources(&project).into_iter().filter(|source| order.get(*source) != Some(source)).count();
    let members: Vec<bool> = moving.iter().map(|&track| project.group_of(track).is_some_and(|group| moving.contains(&group))).collect();
    let above = moving.iter().filter(|&&track| track < before).count();
    let sunk = moving[..above].iter().enumerate().map(|(sunk, &track)| Shift::Move(track - sunk, before - 1));
    let shifts: Vec<Shift> = sunk.chain(moving[above..].iter().enumerate().map(|(risen, &track)| Shift::Move(track, before + risen))).collect();
    for shift in &shifts {
        if let Shift::Move(from, to) = *shift {
            project.move_track(from, to);
        }
    }
    for (offset, member) in members.into_iter().enumerate() {
        let track = &mut project.tracks[before - above + offset];
        if !track.group {
            track.inside = member || inside;
        }
    }
    let grouping = match (was_inside, inside) {
        _ if group => None,
        (false, true) => Some(String::from("joined the group it landed in")),
        (true, false) => Some(String::from("left its group")),
        _ => None,
    };
    let said = listed([tally(moving.len() - 1, "member", " with it"), grouping, tally(renumbered, "track reference", " renumbered, sourceTrack and follows")], "nothing else changed");
    edits(&project, document, Scope::Whole, marks, marks, &format!("Moved{name} from tracks/{at} to tracks/{to}: {said}")).map(|edits| shifting(edits, shifts))
}

fn among(to: usize) -> String {
    format!("tracks/{to} lies among the members of another group; a group lands outside other groups")
}

fn moved_row(document: &Json, steps: &[String], to: usize) -> Result<Vec<Edit>, String> {
    let at = path::joined(steps);
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let (host, owner) = rack_of(&project, steps).ok_or_else(|| format!("{at} is not a rack row"))?;
    let entries = path::locate(document, &steps[..steps.len() - 1]).and_then(Json::as_array).unwrap_or(&[]);
    let settled = |written: usize| (written < entries.len()).then(|| presets::settled(&project, host, entries, written)).flatten();
    let from = steps.last().and_then(|row| row.parse::<usize>().ok()).and_then(settled).ok_or_else(|| format!("No row at {at}: the rack has {}", counted(entries.len(), "row")))?;
    let to = settled(to).ok_or_else(|| format!("{at} can move to row 0 through {}; {to} is past that", entries.len().saturating_sub(1)))?;
    let rack = project.rack(host);
    if from.min(to) == 0 && rack[0].is_midi() {
        return Err(format!("row 0 of {} is the midi row, which stays there", path::joined(owner)));
    }
    let kind = presets::kind_of(&rack[from]).unwrap_or_default();
    if from == to {
        return owned(&project, owner, format!("The {kind} row already sits at {at}: nothing moved"), None);
    }
    let (low, high) = (from.min(to), from.max(to));
    let lanes = aimed(project.automation(host), |row| (low..=high).contains(&row));
    project.move_row(host, from, to);
    let said = listed([tally(high - low, "row", if to < from { " moved down one" } else { " moved up one" }), lanes.map(|lanes| format!("{lanes} renumbered"))], "nothing else moved");
    owned(&project, owner, format!("Moved the {kind} row from {at} to {}/rack/{to}: {said}", path::joined(owner)), None)
}

pub(super) fn racked(row: &Json, document: &Json, steps: &[String]) -> Result<Vec<Edit>, String> {
    let at = path::joined(steps);
    let project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let (host, owner) = rack_of(&project, steps).ok_or_else(|| format!("insert with a row takes a rack row like tracks/N/rack/R; {at} is not one"))?;
    let list = path::joined(&steps[..steps.len() - 1]);
    let entries = path::locate(document, &steps[..steps.len() - 1]).and_then(Json::as_array).unwrap_or(&[]);
    let index = steps.last().and_then(|row| row.parse::<usize>().ok()).filter(|index| *index <= entries.len()).ok_or_else(|| format!("{list} holds {}; a row goes in at 0 through {}", counted(entries.len(), "row"), entries.len()))?;
    if index == entries.len() {
        return Ok(vec![Edit { steps: steps.to_vec(), sent: row.clone(), did: None, arranged: false, said: None, shifts: Vec::new() }]);
    }
    let short = (taken(project.rack(host)) + Row::from_json(row).map_or(1, |row| row.times())).saturating_sub(RACK_ROWS);
    if short > 0 {
        return Err(format!("{list} has no room for the row: it would be {}", crowding(short)));
    }
    if index == 0 && project.cores(host) == Cores::Required {
        return Err(format!("{at} is the midi row's place; a MIDI track's other rows go in from row 1"));
    }
    let Some(Json::Object(fields)) = path::locate(document, owner) else {
        return Err(format!("{} is not there", path::joined(owner)));
    };
    let later = |row: usize| if row >= index { row + 1 } else { row };
    let mut landed = Vec::with_capacity(fields.len());
    for (key, value) in fields {
        let moved = match (key.as_str(), value) {
            ("rack", Json::Array(rows)) => {
                let mut rows: Vec<Json> = rows.iter().enumerate().map(|(at, entry)| seeded(entry, presets::settled(&project, host, entries, at).and_then(|at| project.rack(host).get(at)))).collect();
                rows.insert(index, row.clone());
                Json::Array(rows)
            }
            ("curves" | "lfos", Json::Array(lanes)) => Json::Array(lanes.iter().map(|lane| rerowed(lane, &later)).collect()),
            _ => value.clone(),
        };
        landed.push((key.clone(), moved));
    }
    let landed = Json::Object(landed);
    let kind = row.get("kind").and_then(Json::as_str);
    if row.get("preset").is_none() {
        let mut probe = document.clone();
        path::place(&mut probe, owner, landed.clone())?;
        let fits = Row::from_json(row).is_some() && Project::from_json(&probe).is_some_and(|probe| probe.rack(host).len() == project.rack(host).len() + 1);
        if !fits {
            return Err(format!("{at} takes no {} row there", kind.unwrap_or("such")));
        }
    }
    let what = kind.map_or_else(|| String::from("a row"), |kind| format!("the {kind} row"));
    let lanes = aimed(project.automation(host), |row| presets::settled(&project, host, entries, index).is_some_and(|index| row >= index));
    let said = listed([tally(entries.len() - index, "row", " below moved down one"), lanes.map(|lanes| format!("{lanes} renumbered"))], "nothing else moved");
    let typed = Json::Object(vec![(String::from("rack"), Json::Array((0..=index).map(|slot| if slot == index { row.clone() } else { Json::Null }).collect()))]);
    Ok(vec![Edit { steps: owner.to_vec(), sent: landed, did: Some(format!("Inserted {what} at {at}: {said}")), arranged: true, said: Some(typed), shifts: Vec::new() }])
}

fn seeded(entry: &Json, row: Option<&Row>) -> Json {
    match (entry, row) {
        (Json::Object(fields), Some(Row::Core(layer))) if entry.get("seed").is_none() => Json::Object(fields.iter().cloned().chain(std::iter::once((String::from("seed"), Json::Number(layer.seed as f64)))).collect()),
        _ => entry.clone(),
    }
}

fn rerowed(lane: &Json, later: &impl Fn(usize) -> usize) -> Json {
    let Json::Object(fields) = lane else {
        return lane.clone();
    };
    let moved = fields.iter().map(|(key, value)| match value.as_index(RACK_ROWS) {
        Some(row) if key == "row" => (key.clone(), Json::Number(later(row) as f64)),
        _ => (key.clone(), value.clone()),
    });
    Json::Object(moved.collect())
}

fn aimed(lanes: &Automation, on: impl Fn(usize) -> bool) -> Option<String> {
    let held = |row: Option<usize>| row.is_some_and(&on);
    let found = [
        (lanes.curves.iter().filter(|curve| held(curve.aim.row())).count(), "curve"),
        (lanes.lfos.iter().filter(|lfo| held(lfo.aim.row())).count(), "LFO lane"),
    ];
    let said: Vec<String> = found.into_iter().filter(|(count, _)| *count > 0).map(|(count, noun)| counted(count, noun)).collect();
    (!said.is_empty()).then(|| said.join(", "))
}

pub(super) fn crowding(short: usize) -> String {
    format!("{} more than the {RACK_ROWS} a rack holds, a row counting once for each of its times", counted(short, "row"))
}

pub(super) fn crowded(document: &Json) -> Option<String> {
    let (host, short) = Project::crowded(document)?;
    let rack = match host {
        Host::Track(track) => format!("tracks/{track}/rack"),
        Host::Send(send) => format!("returns/{send}/rack"),
        Host::Master => String::from("master/rack"),
    };
    Some(format!("{rack} would be {}", crowding(short)))
}

pub(super) fn orphan(document: &Json) -> Option<usize> {
    let mut grouped = false;
    for (index, track) in document.get("tracks").and_then(Json::as_array).unwrap_or(&[]).iter().enumerate() {
        let group = track.get("kind").and_then(Json::as_str) == Some("group");
        let inside = !group && track.get("inGroup").and_then(Json::as_bool).unwrap_or(false);
        if inside && !grouped {
            return Some(index);
        }
        if !inside {
            grouped = group;
        }
    }
    None
}

fn dropped(document: &Json, index: &str) -> Result<Vec<Edit>, String> {
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let count = project.tracks.len();
    let at = index.parse::<usize>().ok().filter(|at| *at < count).ok_or_else(|| format!("No track at tracks/{index}: the project has {}", counted(count, "track")))?;
    let name = format!(" {}", project.tracks[at].name);
    let members = project.members_of(at).len();
    let others: Vec<usize> = project.tracks.iter().enumerate().filter(|(index, _)| *index != at).flat_map(|(_, track)| sources_of(track)).chain(project.returns().flat_map(|ret| wired(&ret.rack))).collect();
    let (cut, renumbered) = (others.iter().filter(|source| **source == at).count(), others.iter().filter(|source| **source > at).count());
    if !project.remove_track(at) {
        return Err(String::from("The project keeps one track at least"));
    }
    let said = listed([tally(members, "member", " left ungrouped"), tally(count - 1 - at, "track", " below moved up one"), tally(renumbered, "track reference", " renumbered, sourceTrack and follows"), tally(cut, "reference", " to it dropped")], "nothing else moved");
    let marks = marks_of(document);
    edits(&project, document, Scope::Whole, marks, marks, &format!("Removed{name} at tracks/{at}: {said}")).map(|edits| shifting(edits, vec![Shift::Remove(at)]))
}

fn left_group(document: &Json, index: &str) -> Option<Vec<Edit>> {
    let mut project = Project::from_json(document)?;
    let at = index.parse::<usize>().ok()?;
    let last = project.members_of(project.group_of(at)?).last().copied().filter(|last| *last != at)?;
    let name = format!(" {}", project.tracks[at].name);
    let renumbered = sources(&project).into_iter().filter(|source| (at..=last).contains(source)).count();
    project.move_track(at, last);
    project.tracks[last].inside = false;
    let said = listed([tally(last - at, "member", " moved up one"), tally(renumbered, "track reference", " renumbered, sourceTrack and follows")], "nothing else moved");
    let marks = marks_of(document);
    edits(&project, document, Scope::Whole, marks, marks, &format!("Ungrouped{name}, moved from tracks/{at} to tracks/{last}, below its group: {said}")).ok().map(|edits| shifting(edits, vec![Shift::Move(at, last)]))
}

pub(super) fn tracked(track: &Json, document: &Json, steps: &[String]) -> Result<Vec<Edit>, String> {
    let tracks = document.get("tracks").and_then(Json::as_array).unwrap_or(&[]);
    let at = match steps {
        [list, index] if list == "tracks" => index.parse::<usize>().ok().filter(|at| *at <= tracks.len()),
        _ => None,
    };
    let Some(at) = at else {
        return Err(format!("insert with a track takes tracks/0 to tracks/{} or tracks/end; {} is not one", tracks.len(), path::joined(steps)));
    };
    if tracks.len() >= TRACKS {
        return Err(format!("The project holds {TRACKS} tracks, its most"));
    }
    let mut appended = document.clone();
    path::place(&mut appended, &[String::from("tracks"), tracks.len().to_string()], track.clone())?;
    let mut project = Project::from_json(&appended).ok_or("The document would not read as a project")?;
    let placed = project.tracks.pop().ok_or("The document would not read as a project")?;
    let under = at.checked_sub(1).is_some_and(|above| project.tracks[above].group || project.tracks[above].inside);
    if placed.group && under && project.tracks.get(at).is_some_and(|below| below.inside) {
        return Err(among(at));
    }
    let rows = track.get("rack").and_then(Json::as_array).map_or(&[][..], |rows| rows);
    let renumbered = sources(&project).into_iter().chain(placed.follows).chain(rows.iter().filter_map(raw_source)).filter(|source| *source >= at).count();
    project.add_track(at, placed);
    if let Some(inside) = track.get("inGroup").and_then(Json::as_bool) {
        project.tracks[at].inside = inside && !project.tracks[at].group;
    }
    let name = track.get("name").and_then(Json::as_str).map(str::trim).filter(|name| !name.is_empty()).map_or(String::new(), |name| format!(" {name}"));
    let said = listed([tally(tracks.len() - at, "track", " below it moved down one"), tally(renumbered, "track reference", " renumbered, sourceTrack and follows")], "nothing else moved");
    let marks = marks_of(document);
    let mut out = edits(&project, document, Scope::Whole, marks, marks, &format!("Inserted{name} at tracks/{at}: {said}"))?;
    let later = |source: usize| if source >= at { source + 1 } else { source };
    if let (Json::Object(fields), Some(edit)) = (track, out.first_mut())
        && let Json::Array(list) = &mut edit.sent
        && let Some(Json::Object(landed)) = list.get(at)
    {
        let carried = |(key, value): &(String, Json)| match value {
            Json::Array(rows) if key == "rack" => (key.clone(), Json::Array(rows.iter().map(|row| rewired(row, &later)).collect())),
            _ => moved(key, value, &later),
        };
        let grouped = |key: &str| key == "inGroup";
        list[at] = Json::Object(fields.iter().filter(|(key, _)| !grouped(key)).map(carried).chain(landed.iter().filter(|(key, _)| grouped(key)).cloned()).collect());
        edit.said = Some(Json::Array((0..list.len()).map(|index| if index == at { track.clone() } else { Json::Null }).collect()));
    }
    Ok(shifting(out, vec![Shift::Insert(at)]))
}

pub(super) fn numbered(document: &mut Json) -> Vec<String> {
    let nameless = |track: &Json| matches!(track, Json::Object(_)) && track.get("name").and_then(Json::as_str).is_none_or(|name| name.trim().is_empty());
    let found: Vec<usize> = document.get("tracks").and_then(Json::as_array).unwrap_or(&[]).iter().enumerate().filter(|(_, track)| nameless(track)).map(|(index, _)| index).collect();
    let Some(mut project) = Project::from_json(document).filter(|_| !found.is_empty()) else {
        return Vec::new();
    };
    let (count, mut lines) = (project.tracks.len(), Vec::new());
    for index in found.into_iter().filter(|index| *index < count) {
        let mut track = project.tracks.remove(index);
        track.name.clear();
        project.add_track_outside(index, track);
        let name = project.tracks[index].name.clone();
        if path::place(document, &[String::from("tracks"), index.to_string(), String::from("name")], Json::String(name.clone())).is_ok() {
            lines.push(format!("tracks/{index}: {name}"));
        }
    }
    lines
}

fn moved(key: &str, value: &Json, later: &impl Fn(usize) -> usize) -> (String, Json) {
    match value.as_index(TRACKS) {
        Some(source) if key == "sourceTrack" || key == "follows" => (String::from(key), Json::Number(later(source) as f64)),
        _ => (String::from(key), value.clone()),
    }
}

fn raw_source(row: &Json) -> Option<usize> {
    let wires = match Row::from_json(row) {
        Some(Row::Effect(slot)) => slot.wiring().is_some(),
        Some(Row::Midi(_) | Row::Core(_) | Row::Bus(_)) => false,
        None => true,
    };
    row.get("sourceTrack").and_then(|source| source.as_index(TRACKS)).filter(|_| wires)
}

fn rewired(row: &Json, later: &impl Fn(usize) -> usize) -> Json {
    let Json::Object(fields) = row else {
        return row.clone();
    };
    Json::Object(fields.iter().map(|(key, value)| moved(key, value, later)).collect())
}

pub(super) fn duplicated(document: &Json, steps: &[String]) -> Result<Vec<Edit>, String> {
    match steps {
        [tracks, index] if tracks == "tracks" => duplicated_track(document, index),
        [tracks, index, clips, at] if tracks == "tracks" && clips == "clips" => duplicated_clip(document, index, at),
        _ => Err(format!("duplicate takes tracks/N, a track with its members, or tracks/N/clips/K, a clip after itself; {} is neither", path::joined(steps))),
    }
}

fn duplicated_track(document: &Json, index: &str) -> Result<Vec<Edit>, String> {
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let count = project.tracks.len();
    let at = index.parse::<usize>().ok().filter(|at| *at < count).ok_or_else(|| format!("No track at tracks/{index}: the project has {}", counted(count, "track")))?;
    let chosen: Vec<usize> = std::iter::once(at).chain(project.members_of(at)).collect();
    let last = chosen[chosen.len() - 1];
    if count + chosen.len() > TRACKS {
        return Err(format!("The project holds {}; {} more would pass {TRACKS}", counted(count, "track"), chosen.len()));
    }
    let renumbered = sources(&project).into_iter().chain(chosen.iter().flat_map(|&track| sources_of(&project.tracks[track]))).filter(|source| *source > last).count();
    let held: usize = chosen.iter().map(|&track| wired(&project.tracks[track].rack).count()).sum();
    let added = project.duplicate_tracks(&chosen);
    let kept: usize = (last + 1..last + 1 + added).map(|copy| wired(&project.tracks[copy].rack).count()).sum();
    let name = format!(" {}", project.tracks[at].name);
    let landed = if added > 1 { format!("tracks/{} to {}", last + 1, last + added) } else { format!("tracks/{}", last + 1) };
    let said = listed([tally(added - 1, "member", " with it"), tally(count - 1 - last, "track", " below moved down"), tally(renumbered, "track reference", " renumbered, sourceTrack and follows"), tally(held.saturating_sub(kept), "sourceTrack", " left off the copy, where it would loop back")], "nothing else moved");
    let marks = marks_of(document);
    edits(&project, document, Scope::Whole, marks, marks, &format!("Duplicated{name} as {landed}: {said}")).map(|edits| shifting(edits, (0..added).map(|copy| Shift::Insert(last + 1 + copy)).collect()))
}

fn duplicated_clip(document: &Json, track: &str, at: &str) -> Result<Vec<Edit>, String> {
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let index = track.parse::<usize>().ok().filter(|index| *index < project.tracks.len()).ok_or_else(|| format!("No track at tracks/{track}: the project has {}", counted(project.tracks.len(), "track")))?;
    let clip = at.parse::<usize>().ok().and_then(|at| sorted_slot(document, index, at)).and_then(|at| project.tracks[index].clips.get(at)).cloned().ok_or_else(|| format!("No clip at tracks/{track}/clips/{at}: the track has {}", counted(project.tracks[index].clips.len(), "clip")))?;
    let length = clip.length;
    let (start, end) = (clip.end(), clip.end() + length);
    if end > LONGEST_TICKS {
        return Err(format!("A copy would run past beat {}, the song's limit", beats(LONGEST_TICKS)));
    }
    let (bpm, carry) = (project.bpm, !project.lock);
    let piece = project.tracks[index].piece(clip, carry).shifted(length);
    project.tracks[index].lay(piece, carry, bpm);
    let marks = marks_of(document);
    let did = format!("Duplicated tracks/{index}/clips/{at} at beat {}, {}", tidy(beats(start)), bars(&[(start, end)]));
    edits(&project, document, Scope::Track(index), marks, marks, &did)
}

pub(super) fn matched(app: &dyn tools::Host, document: &Json, steps: &[String]) -> Result<Vec<Edit>, String> {
    match steps {
        [tracks, track, clips, at] if tracks == "tracks" && clips == "clips" => matched_clips(app, document, track, at),
        [.., rack, _] if rack == "rack" => matched_rows(document, steps),
        _ => Err(format!("match takes an audio clip like tracks/N/clips/K or a sidechain row like tracks/N/rack/R; {} is neither", path::joined(steps))),
    }
}

fn matched_clips(app: &dyn tools::Host, document: &Json, track: &str, at: &str) -> Result<Vec<Edit>, String> {
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let index = track.parse::<usize>().ok().filter(|index| *index < project.tracks.len()).ok_or_else(|| format!("No track at tracks/{track}: the project has {}", counted(project.tracks.len(), "track")))?;
    let owner = &mut project.tracks[index];
    let like = at.parse::<usize>().ok().and_then(|at| sorted_slot(document, index, at)).filter(|like| *like < owner.clips.len()).ok_or_else(|| format!("No clip at tracks/{track}/clips/{at}: the track has {}", counted(owner.clips.len(), "clip")))?;
    if owner.clips[like].file.is_none() {
        return Err(format!("tracks/{index}/clips/{at} holds notes; match takes an audio clip or a sidechain row"));
    }
    let alike = app.alike(&owner.clips, like);
    for &other in &alike {
        owner.clips[other].file = owner.clips[other].matched(&owner.clips[like], |clip| app.heard_until(clip));
    }
    let places: Vec<String> = alike.iter().map(|other| format!("clips/{other}")).collect();
    let did = if alike.is_empty() { format!("Matched nothing to tracks/{index}/clips/{at}: no other clip on its track sounds like it") } else { format!("Matched {} of tracks/{index} to clips/{at}, its fadeIn, fadeOut, level spans, semitones and cents: {}", counted(alike.len(), "clip"), named(&places)) };
    let marks = marks_of(document);
    edits(&project, document, Scope::Track(index), marks, marks, &did)
}

fn matched_rows(document: &Json, steps: &[String]) -> Result<Vec<Edit>, String> {
    let at = path::joined(steps);
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let (host, _) = rack_of(&project, steps).ok_or_else(|| format!("{at} is not a rack row"))?;
    let entries = path::locate(document, &steps[..steps.len() - 1]).and_then(Json::as_array).unwrap_or(&[]);
    let settled = |written: usize| (written < entries.len()).then(|| presets::settled(&project, host, entries, written)).flatten();
    let row = steps.last().and_then(|row| row.parse::<usize>().ok()).and_then(settled).ok_or_else(|| format!("No row at {at}: the rack has {}", counted(entries.len(), "row")))?;
    let source = match project.rack(host)[row] {
        Row::Effect(slot) => slot.source(),
        Row::Midi(_) | Row::Core(_) | Row::Bus(_) => None,
    };
    let Some(others) = project.match_sidechains(host, row) else {
        return Err(format!("{at} is a {} row; match takes a sidechain row or an audio clip", presets::kind_of(&project.rack(host)[row]).unwrap_or_default()));
    };
    let places: Vec<String> = others.iter().map(|(owner, index)| match owner {
        Host::Track(track) => format!("tracks/{track}/rack/{index}"),
        Host::Send(send) => format!("returns/{send}/rack/{index}"),
        Host::Master => format!("master/rack/{index}"),
    }).collect();
    let signal = source.map_or_else(|| String::from("without a sourceTrack"), |(track, _)| format!("taking the signal from tracks/{track}"));
    let did = if places.is_empty() { format!("Matched nothing to {at}: no other sidechain row is {signal}") } else { format!("Matched {} {signal} to {at}: {}", counted(places.len(), "sidechain row"), named(&places)) };
    let marks = marks_of(document);
    edits(&project, document, Scope::Whole, marks, marks, &did)
}

pub(super) fn rolled(operation: &str, given: &Json, document: &Json, steps: &[String], filter: &Filter) -> Result<Vec<Edit>, String> {
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    if operation == "quantize" {
        return quantized(given, document, &mut project, steps, filter);
    }
    let at = path::joined(steps);
    if filter.any() {
        return Err(format!("fromBeat, untilBeat, above, below and picked choose notes for transpose, scale, fit or quantize; {operation} takes the whole clip"));
    }
    let found = match steps {
        [tracks, track, clips, slot] if tracks == "tracks" && clips == "clips" => track.parse::<usize>().ok().filter(|track| *track < project.tracks.len()).zip(slot.parse::<usize>().ok()),
        _ => None,
    };
    let (index, slot) = found.and_then(|(index, slot)| Some((index, sorted_slot(document, index, slot)?))).filter(|(index, slot)| *slot < project.tracks[*index].clips.len()).ok_or_else(|| format!("{operation} takes a MIDI clip like tracks/N/clips/K; nothing is at {at}"))?;
    let owner = &mut project.tracks[index];
    if owner.audio || owner.clips[slot].file.is_some() {
        return Err(format!("{at} plays a wave; {operation} takes a MIDI clip"));
    }
    let next = owner.clips.get(slot + 1).map(|next| next.start);
    let clip = &mut owner.clips[slot];
    let (room, count) = (next.unwrap_or(LONGEST_TICKS) - clip.end(), counted(clip.notes.len(), "note"));
    let spanned = |clip: &Clip| format!("{}, {}", beats_said(tidy(beats(clip.length))), bars(&[(clip.start, clip.end())]));
    let did = match operation {
        "double" if roll::double(clip, room) => format!("Doubled {at} to {}: {count} stretched with it", spanned(clip)),
        "double" => return Err(match next {
            Some(next) => format!("{at} has no room to double: the next clip starts at beat {}, {} after its end", tidy(beats(next)), beats_said(tidy(beats(room)))),
            None => format!("{at} would run past beat {}, the song's limit", beats(LONGEST_TICKS)),
        }),
        "halve" if roll::halve(clip) => format!("Halved {at} to {}: {count} shrunk with it", spanned(clip)),
        "halve" => return Err(format!("{at} is {} long; a clip under two beats is not halved", beats_said(tidy(beats(clip.length))))),
        "reverse" if roll::reverse(clip) => format!("Reversed {at}: {} turned back to front in time", counted(clip.notes.len(), "note")),
        "reverse" => return Err(format!("{at} has no notes inside its length to reverse")),
        _ => match roll::chordify(clip) {
            Some(chords) => {
                let mut starts: Vec<i32> = chords.iter().map(|note| ticks(f64::from(note.start))).collect();
                starts.dedup();
                format!("Turned {at} into chords: {count} gathered into {} of {}", counted(starts.len(), "chord"), counted(chords.len(), "note"))
            }
            None => return Err(format!("{at} has no notes to gather into chords, or holds those chords already")),
        },
    };
    noted(&project, index, did)
}

fn quantized(given: &Json, document: &Json, project: &mut Project, steps: &[String], filter: &Filter) -> Result<Vec<Edit>, String> {
    let grid = given.as_f64().filter(|grid| grid.is_finite() && ticks(*grid) >= 1 && ticks(*grid) <= TICKS_PER_BAR * 4).ok_or("quantize is a grid in beats, as 0.25 for sixteenths, from one tick to sixteen beats")?;
    let at = if steps.is_empty() { String::from("the song") } else { path::joined(steps) };
    let count = project.tracks.len();
    let track_at = |track: &String| track.parse::<usize>().ok().filter(|track| *track < count);
    let scope: Vec<(usize, Option<usize>)> = match steps {
        [] => (0..count).map(|track| (track, None)).collect(),
        [tracks, track] if tracks == "tracks" => track_at(track).map(|track| (track, None)).into_iter().collect(),
        [tracks, track, clips, slot] if tracks == "tracks" && clips == "clips" => track_at(track).zip(slot.parse::<usize>().ok()).and_then(|(track, slot)| Some((track, Some(sorted_slot(document, track, slot)?)))).into_iter().collect(),
        _ => Vec::new(),
    };
    if scope.is_empty() {
        return Err(format!("quantize takes a MIDI clip, a track or, with no path, the whole song; nothing to quantize is at {at}"));
    }
    let (mut chosen, mut moved, mut replaced, mut changed) = (0, 0, 0, Vec::new());
    for (index, only) in scope {
        let owner = &mut project.tracks[index];
        if owner.audio {
            continue;
        }
        let mut touched = false;
        for (_, clip) in owner.clips.iter_mut().enumerate().filter(|(slot, clip)| clip.file.is_none() && only.is_none_or(|only| only == *slot)) {
            let (start, length, held) = (beats(clip.start), clip.beats(), clip.notes.clone());
            let takes = |place: usize, note: &Note| roll::shown(note, length) && filter.holds(place, f64::from(note.pitch), start + f64::from(note.start));
            let picked: Vec<Note> = held.iter().enumerate().filter(|(place, note)| takes(*place, note)).map(|(_, note)| *note).collect();
            chosen += picked.len();
            if let Some(snapped) = roll::quantize(clip, grid as f32, takes) {
                touched = true;
                moved += picked.iter().zip(&snapped).filter(|(was, now)| was != now).count();
                replaced += held.len().saturating_sub(clip.notes.len());
            }
        }
        if touched {
            changed.push(index);
        }
    }
    if chosen == 0 {
        return Err(format!("{at} has no notes{} to quantize", filter.said()));
    }
    let grid = beats_said(tidy(grid));
    if changed.is_empty() {
        let index = match steps {
            [_, track, ..] => track.parse::<usize>().unwrap_or(0),
            _ => 0,
        };
        return noted(project, index, format!("Quantized {at} to {grid}: every note{} sits on the grid already", filter.said()));
    }
    let gave = if replaced > 0 { format!(", and {} gave way where a moved note landed on its pitch", counted(replaced, "note")) } else { String::new() };
    let did = format!("Quantized {at} to {grid}: {moved} of {}{} moved onto the grid{gave}", counted(chosen, "note"), filter.said());
    let mut out = Vec::new();
    for index in changed {
        out.extend(noted(project, index, did.clone())?);
    }
    Ok(out)
}

fn noted(project: &Project, index: usize, did: String) -> Result<Vec<Edit>, String> {
    let sent = project.to_json().get("tracks").and_then(Json::as_array).and_then(|tracks| tracks.get(index)).cloned().ok_or_else(|| format!("The project came back without tracks/{index}"))?;
    Ok(vec![Edit { steps: vec![String::from("tracks"), index.to_string()], sent, did: Some(did), arranged: false, said: None, shifts: Vec::new() }])
}

pub(super) fn flattened(app: &dyn tools::Host, document: &Json, steps: &[String]) -> Result<Vec<Edit>, String> {
    let at = path::joined(steps);
    let mut project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let (host, _) = rack_of(&project, steps).ok_or_else(|| format!("flatten takes a midi row like tracks/N/rack/R; {at} is not a rack row"))?;
    let entries = path::locate(document, &steps[..steps.len() - 1]).and_then(Json::as_array).unwrap_or(&[]);
    let settled = |written: usize| (written < entries.len()).then(|| presets::settled(&project, host, entries, written)).flatten();
    let row = steps.last().and_then(|row| row.parse::<usize>().ok()).and_then(settled).ok_or_else(|| format!("No row at {at}: the rack has {}", counted(entries.len(), "row")))?;
    let kind = presets::kind_of(&project.rack(host)[row]).unwrap_or_default();
    let Host::Track(index) = host else {
        return Err(format!("{at} is a {kind} row; flatten takes a track's midi row"));
    };
    if !project.rack(host)[row].is_midi() {
        let rendered = if kind == "sidechain" { ", and a sidechain row's FLATTEN renders its signal, so the user presses it in the app" } else { "" };
        return Err(format!("{at} is a {kind} row; flatten takes a midi row{rendered}"));
    }
    let marks = marks_of(document);
    let printed = |project: &Project, track: usize| {
        let clips = &project.tracks[track].clips;
        format!("{} printed over {}", counted(clips.iter().map(|clip| clip.notes.len()).sum(), "note"), counted(clips.len(), "clip"))
    };
    if let Some(reason) = flatten::refusal(&project, index, row) {
        return Err(format!("{at} can't be flattened: {reason}"));
    }
    let sent = app.sent(&project);
    match flatten::row(&mut project, index, row, &sent) {
        Flattened::Nothing => Err(format!("{at} has nothing to flatten")),
        Flattened::Printed => {
            let above = if row > 0 { ", and so are the midi rows feeding it, as no Core stays behind to play the notes as written" } else { "" };
            edits(&project, document, Scope::Track(index), marks, marks, &format!("Flattened {at}: {}, its arp off, velocity 127 and transposeSemitones 0{above}", printed(&project, index)))
        }
        Flattened::Split { removed, copied, muted } => {
            let landed = index + 1;
            let mut did = format!("Flattened {at} onto a new track, tracks/{landed} {}: {}, {} moved there off tracks/{index}", project.tracks[landed].name, printed(&project, landed), counted(removed.count_ones() as usize, "row"));
            if muted {
                did.push_str("; no Core played this row, so the new track is muted and holds a stock Core");
            }
            if copied {
                did.push_str("; the effects below it are copied there");
            }
            edits(&project, document, Scope::Whole, marks, marks, &did).map(|edits| shifting(edits, vec![Shift::Insert(landed)]))
        }
    }
}

fn named(places: &[String]) -> String {
    let more = if places.len() > MOST_MATCHED { format!(" and {} more", places.len() - MOST_MATCHED) } else { String::new() };
    format!("{}{more}", places[..places.len().min(MOST_MATCHED)].join(", "))
}

fn sorted_slot(document: &Json, track: usize, written: usize) -> Option<usize> {
    let entries = document.get("tracks")?.as_array()?.get(track)?.get("clips")?.as_array()?;
    let starts: Vec<Option<i32>> = entries.iter().map(|entry| clip_from_json(entry).map(|clip| clip.start)).collect();
    let start = (*starts.get(written)?)?;
    Some(starts.iter().flatten().filter(|other| **other < start).count() + starts[..written].iter().flatten().filter(|other| **other == start).count())
}

fn shifting(mut edits: Vec<Edit>, shifts: Vec<Shift>) -> Vec<Edit> {
    if let Some(first) = edits.first_mut() {
        first.shifts = shifts;
    }
    edits
}

fn sources_of(track: &Track) -> Vec<usize> {
    track.follows.into_iter().chain(wired(&track.rack)).collect()
}

fn wired(rack: &[Row]) -> impl Iterator<Item = usize> + '_ {
    rack.iter().filter_map(|row| match row {
        Row::Effect(slot) => slot.source().map(|(track, _)| track),
        Row::Midi(_) | Row::Core(_) | Row::Bus(_) => None,
    })
}

fn sources(project: &Project) -> Vec<usize> {
    project.tracks.iter().flat_map(sources_of).chain(project.returns().flat_map(|ret| wired(&ret.rack))).collect()
}

fn edits(project: &Project, document: &Json, scope: Scope, before: Marks, after: Marks, did: &str) -> Result<Vec<Edit>, String> {
    let json = project.to_json();
    let tracks = json.get("tracks").and_then(Json::as_array).ok_or("The project came back without its tracks")?;
    let mut out = Vec::new();
    match scope {
        Scope::Track(index) => out.push(edit(&["tracks", index.to_string().as_str()], tracks.get(index).cloned().ok_or_else(|| format!("The project came back without tracks/{index}"))?, did)),
        Scope::Whole => {
            out.push(edit(&["tracks"], Json::Array(tracks.to_vec()), did));
            for key in ["returns", "master"] {
                if document.get(key) != json.get(key) {
                    out.push(edit(&[key], json.get(key).cloned().unwrap_or(Json::Null), did));
                }
            }
        }
    }
    if after.playhead != before.playhead {
        out.push(edit(&["playheadBeat"], Json::Number(beats(after.playhead)), did));
    }
    match (before.looped, after.looped) {
        (Some(_), None) => out.extend([edit(&["loopStartBeat"], Json::Null, did), edit(&["loopEndBeat"], Json::Null, did)]),
        (Some((start, end)), Some((moved_start, moved_end))) => {
            if moved_start != start {
                out.push(edit(&["loopStartBeat"], Json::Number(beats(moved_start)), did));
            }
            if moved_end != end {
                out.push(edit(&["loopEndBeat"], Json::Number(beats(moved_end)), did));
            }
        }
        _ => {}
    }
    Ok(out)
}

fn edit(steps: &[&str], sent: Json, did: &str) -> Edit {
    Edit { steps: steps.iter().map(|step| String::from(*step)).collect(), sent, did: Some(String::from(did)), arranged: true, said: None, shifts: Vec::new() }
}

pub(crate) fn counted(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}

fn tally(count: usize, noun: &str, tail: &str) -> Option<String> {
    (count > 0).then(|| format!("{}{tail}", counted(count, noun)))
}

fn marked(before: Marks, after: Marks) -> [Option<String>; 2] {
    let playhead = (after.playhead != before.playhead).then(|| String::from("the playhead moved"));
    let looped = (after.looped != before.looped).then(|| String::from(if after.looped.is_some() { "the loop moved" } else { "the loop removed" }));
    [playhead, looped]
}

fn listed(parts: impl IntoIterator<Item = Option<String>>, none: &str) -> String {
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    if parts.is_empty() { String::from(none) } else { parts.join(", ") }
}

fn beats_said(length: f64) -> String {
    if length == 1.0 { String::from("1 beat") } else { format!("{length} beats") }
}

fn copies_said(copies: i32) -> String {
    if copies == 1 { String::from("1 copy") } else { format!("{copies} copies") }
}

pub(super) fn bars(spans: &[(i32, i32)]) -> String {
    let ranges: Vec<(i32, i32)> = spans.iter().map(|&(start, end)| (start / TICKS_PER_BAR + 1, (end - 1) / TICKS_PER_BAR + 1)).collect();
    let said: Vec<String> = ranges.iter().map(|&(first, last)| if first == last { first.to_string() } else { format!("{first}–{last}") }).collect();
    let noun = if matches!(ranges.as_slice(), [(first, last)] if first == last) { "bar" } else { "bars" };
    match said.split_last() {
        Some((last, rest)) if rest.len() >= MOST_BARS => format!("{noun} {}, … and {last}", rest[..MOST_BARS - 1].join(", ")),
        Some((last, [])) => format!("{noun} {last}"),
        Some((last, rest)) => format!("{noun} {} and {last}", rest.join(", ")),
        None => String::from(noun),
    }
}

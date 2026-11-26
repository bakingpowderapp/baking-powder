use super::counted;
use super::path::pitch_name;
use super::tools::Host;
use sdk::json::Json;
use crate::flatten::Sent;
use crate::midi::Part;
use crate::project::codec::clip_from_json;
use crate::project::{BEATS_PER_BAR, Clip, Note, Project, Row, SongNote, beats};
use crate::theory::{Root, ScaleName, keyed, pitch_class_name, prefer_flats};

const CLASSES: usize = 12;
const BEATS: usize = BEATS_PER_BAR as usize;
const UNIT: usize = 2;
const UNITS_PER_BAR: usize = BEATS / UNIT;
const FIFTH: usize = 7;
const LEAST_TONES: usize = 2;
const MOST_CHANGES: usize = 48;
const MOST_BARS: usize = 2048;
const MINOR_THIRD: usize = 3;
const MAJOR_THIRD: usize = 4;
const MOST_LOOSE: usize = 3;
const BASS_SHARE: f32 = 0.5;
const EDGE: f32 = 1.0 / 64.0;
const MISSING_COST: f32 = 0.2;
const SIZE_COST: f32 = 0.02;
const BASS_BONUS: f32 = 0.15;
const REST: &str = "rest";
const CHORDS: &str = "chords";
const SHAPES: [(&str, &[usize]); 20] = [
    ("", &[0, 4, 7]),
    ("m", &[0, 3, 7]),
    ("°", &[0, 3, 6]),
    ("aug", &[0, 4, 8]),
    ("sus2", &[0, 2, 7]),
    ("sus4", &[0, 5, 7]),
    ("5", &[0, 7]),
    ("7", &[0, 4, 7, 10]),
    ("maj7", &[0, 4, 7, 11]),
    ("m7", &[0, 3, 7, 10]),
    ("ø7", &[0, 3, 6, 10]),
    ("°7", &[0, 3, 6, 9]),
    ("6", &[0, 4, 7, 9]),
    ("m6", &[0, 3, 7, 9]),
    ("(add9)", &[0, 2, 4, 7]),
    ("m(add9)", &[0, 2, 3, 7]),
    ("7sus4", &[0, 5, 7, 10]),
    ("9", &[0, 2, 4, 7, 10]),
    ("m9", &[0, 2, 3, 7, 10]),
    ("maj9", &[0, 2, 4, 7, 11]),
];

#[derive(Clone)]
struct Unit {
    weights: [f32; CLASSES],
    basses: [usize; CLASSES],
}

struct Chord {
    name: String,
    root: Option<usize>,
    third: Option<usize>,
}

struct Segment {
    length: usize,
    sum: Unit,
    chord: Option<Chord>,
}

pub(super) fn clip_described(notes: &[Note], home: Option<(Root, ScaleName)>) -> Option<String> {
    let part = Part { name: String::new(), drums: false, notes: notes.to_vec(), length: 0.0 };
    described(std::slice::from_ref(&part), home)
}

pub(super) fn home_of(document: &Json) -> Option<(Root, ScaleName)> {
    let root = document.get("root").and_then(Json::as_str).and_then(Root::from_name)?;
    let scale = document.get("scale").and_then(Json::as_str).and_then(ScaleName::from_name)?;
    Some((root, scale))
}

fn moves(project: &Project, track: usize) -> bool {
    let Some(owner) = project.tracks.get(track) else {
        return false;
    };
    let key = project.key_bits();
    let laned = |at: usize| owner.automation.curves.iter().any(|curve| curve.aim.row() == Some(at)) || owner.automation.lfos.iter().any(|lfo| lfo.aim.row() == Some(at));
    owner.follows.is_some() || owner.rack.iter().enumerate().any(|(at, row)| matches!(row, Row::Midi(midi) if midi.heard().on || midi.shift() != 0 || midi.snap(key).is_some() || laned(at)))
}

pub(super) fn arp_output(project: &Project, track: usize, sent: &Sent) -> Option<Vec<SongNote>> {
    moves(project, track).then(|| sent.cored(project, track))
}

pub(super) fn arp_described(played: &[SongNote], clip: &Clip, home: Option<(Root, ScaleName)>) -> Option<String> {
    let (start, end) = (beats(clip.start), beats(clip.end()));
    let notes: Vec<Note> = played.iter().filter(|note| note.start >= start && note.start < end).map(|note| Note { pitch: note.pitch, start: (note.start - start) as f32, length: (note.end.min(end) - note.start) as f32, velocity: note.velocity, articulation: note.articulation }).collect();
    clip_described(&notes, home).map(|text| format!("the midi rows play {text}"))
}

fn list_mut<'a>(json: &'a mut Json, key: &str) -> &'a mut [Json] {
    match json {
        Json::Object(fields) => match fields.iter_mut().find(|(known, _)| known == key) {
            Some((_, Json::Array(items))) => items.as_mut_slice(),
            _ => &mut [],
        },
        _ => &mut [],
    }
}

pub(super) fn chorded(host: &dyn Host, document: &mut Json) {
    let home = home_of(document);
    let project = Project::from_json(document);
    let sent = project.as_ref().filter(|project| (0..project.tracks.len()).any(|track| moves(project, track))).map(|project| host.sent(project));
    for (index, track) in list_mut(document, "tracks").iter_mut().enumerate() {
        let played = project.as_ref().zip(sent.as_ref()).and_then(|(project, sent)| arp_output(project, index, sent));
        for clip in list_mut(track, "clips") {
            let Some(parsed) = clip.get("notes").and_then(|_| clip_from_json(clip)) else {
                continue;
            };
            let lines: Vec<String> = [clip_described(&parsed.notes, home), played.as_ref().and_then(|played| arp_described(played, &parsed, home))].into_iter().flatten().collect();
            if let (false, Json::Object(fields)) = (lines.is_empty(), clip) {
                let at = fields.iter().position(|(key, _)| key == "notes").map_or(fields.len(), |at| at + 1);
                fields.insert(at, (String::from(CHORDS), Json::String(lines.join("; "))));
            }
        }
    }
}

pub(super) fn unchorded(json: &mut Json) {
    match json {
        Json::Object(fields) => {
            if fields.iter().any(|(key, _)| key == "notes") {
                fields.retain(|(key, _)| key != CHORDS);
            }
            fields.iter_mut().for_each(|(_, value)| unchorded(value));
        }
        Json::Array(items) => items.iter_mut().for_each(unchorded),
        _ => {}
    }
}

pub(super) fn described(parts: &[Part], home: Option<(Root, ScaleName)>) -> Option<String> {
    let end = parts.iter().flat_map(|part| &part.notes).map(|note| note.start + note.length).fold(0.0, f32::max);
    if end <= 0.0 {
        return None;
    }
    let bars = ((end - EDGE) / BEATS as f32).ceil().max(1.0) as usize;
    if bars > MOST_BARS {
        return None;
    }
    let hits: usize = parts.iter().filter(|part| part.drums).map(|part| part.notes.len()).sum();
    let pitched: Vec<&Note> = parts.iter().filter(|part| !part.drums).flat_map(|part| &part.notes).collect();
    if pitched.is_empty() {
        return Some(format!("{} of drums, {}", counted(bars, "bar"), counted(hits, "hit")));
    }
    let units = heard(&pitched, bars * UNITS_PER_BAR);
    let (key, flats) = key(&pitched, &units, home);
    let names = merged(&units, flats);
    let period = period(&names);
    let mut text = format!("{} in {key}", counted(bars, "bar"));
    if let Some((low, high)) = line(&pitched) {
        text.push_str(&format!(", a line of {}, {} to {}", counted(pitched.len(), "note"), pitch_name(low), pitch_name(high)));
    }
    text.push_str(&format!(": {}", changes(&names[..period.unwrap_or(names.len())])));
    if let Some(period) = period {
        let (times, more) = (names.len() / period, names.len() % period / UNITS_PER_BAR);
        text.push_str(&format!(", repeating every {}, {times} times", counted(period / UNITS_PER_BAR, "bar")));
        if more > 0 {
            text.push_str(&format!(", then {}", counted(more, "more bar")));
        }
    }
    if hits > 0 {
        text.push_str(&format!(", with {}", counted(hits, "drum hit")));
    }
    Some(text)
}

fn line(notes: &[&Note]) -> Option<(i64, i64)> {
    let mut spans: Vec<(f32, f32)> = notes.iter().map(|note| (note.start, note.start + note.length)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let alone = spans.windows(2).all(|pair| pair[0].1 <= pair[1].0 + EDGE);
    let (low, high) = notes.iter().fold((i64::MAX, i64::MIN), |(low, high), note| (low.min(i64::from(note.pitch)), high.max(i64::from(note.pitch))));
    (alone && low <= high).then_some((low, high))
}

fn heard(notes: &[&Note], count: usize) -> Vec<Unit> {
    let span = UNIT as f32;
    let mut units = vec![Unit { weights: [0.0; CLASSES], basses: [0; CLASSES] }; count];
    let mut lowest: Vec<(Option<u8>, Option<u8>)> = vec![(None, None); count];
    for note in notes {
        let (from, until) = (note.start.max(0.0), note.start + note.length);
        let (first, last) = ((from / span).floor() as usize, ((until / span).ceil() as usize).min(count));
        for (index, (unit, (held, any))) in units.iter_mut().zip(lowest.iter_mut()).enumerate().take(last).skip(first) {
            let overlap = until.min((index + 1) as f32 * span) - from.max(index as f32 * span);
            if overlap <= 0.0 {
                continue;
            }
            unit.weights[usize::from(note.pitch) % CLASSES] += overlap;
            *any = Some(any.map_or(note.pitch, |low| low.min(note.pitch)));
            if overlap >= BASS_SHARE * span {
                *held = Some(held.map_or(note.pitch, |low| low.min(note.pitch)));
            }
        }
    }
    for (unit, (held, any)) in units.iter_mut().zip(lowest) {
        if let Some(low) = held.or(any) {
            unit.basses[usize::from(low) % CLASSES] += 1;
        }
    }
    units
}

fn key(notes: &[&Note], units: &[Unit], home: Option<(Root, ScaleName)>) -> (String, bool) {
    let (mut heard, mut bass) = ([0i32; CLASSES], [0i32; CLASSES]);
    for note in notes {
        heard[usize::from(note.pitch) % CLASSES] += 1;
    }
    for unit in units {
        for (class, count) in unit.basses.iter().enumerate() {
            bass[class] += *count as i32;
        }
    }
    let (root, scale, outside) = keyed(&heard, &bass, home).map_or_else(Default::default, |(fit, outside)| (fit.root, fit.scale, outside));
    let mut text = format!("{} {}", root.name(), scale.name());
    match outside {
        0 => {}
        1 => text.push_str(", 1 note outside"),
        outside => text.push_str(&format!(", {outside} notes outside")),
    }
    (text, prefer_flats(root, scale))
}

fn merged(units: &[Unit], flats: bool) -> Vec<Option<String>> {
    let mut segments: Vec<Segment> = Vec::new();
    for unit in units {
        let chord = named(unit, flats);
        match segments.last_mut() {
            Some(last) if alike(last.chord.as_ref(), chord.as_ref()) => {
                last.length += 1;
                for (weight, more) in last.sum.weights.iter_mut().zip(unit.weights) {
                    *weight += more;
                }
                for (count, more) in last.sum.basses.iter_mut().zip(unit.basses) {
                    *count += more;
                }
                last.chord = named(&last.sum, flats);
            }
            _ => segments.push(Segment { length: 1, sum: unit.clone(), chord }),
        }
    }
    let mut names = Vec::with_capacity(units.len());
    for segment in &segments {
        let name = segment.chord.as_ref().map(|chord| chord.name.clone());
        names.extend(std::iter::repeat_n(name, segment.length));
    }
    names
}

fn alike(a: Option<&Chord>, b: Option<&Chord>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.root.is_some() && a.root == b.root && (a.third.is_none() || b.third.is_none() || a.third == b.third),
        _ => false,
    }
}

fn named(unit: &Unit, flats: bool) -> Option<Chord> {
    let total: f32 = unit.weights.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let spell = |class: usize| pitch_class_name(class as i32, flats);
    let bass = (0..CLASSES).filter(|&class| unit.basses[class] > 0).max_by_key(|&class| unit.basses[class]);
    let mut best: Option<(f32, usize, &str, &[usize])> = None;
    for root in (0..CLASSES).filter(|&class| unit.weights[class] > 0.0) {
        for (suffix, shape) in SHAPES {
            let (mut covered, mut missing) = (0.0, 0);
            for interval in shape {
                let weight = unit.weights[(root + interval) % CLASSES];
                if weight > 0.0 {
                    covered += weight;
                } else {
                    missing += if *interval == FIFTH { 1 } else { LEAST_TONES };
                }
            }
            if missing > 1 || shape.len() - missing < LEAST_TONES {
                continue;
            }
            let mut score = 2.0 * covered - total - total * (MISSING_COST * missing as f32 + SIZE_COST * shape.len() as f32);
            if bass == Some(root) {
                score += total * BASS_BONUS;
            }
            if best.is_none_or(|(known, _, _, _)| score > known) {
                best = Some((score, root, suffix, shape));
            }
        }
    }
    let Some((_, root, suffix, shape)) = best else {
        let mut classes: Vec<usize> = (0..CLASSES).filter(|&class| unit.weights[class] > 0.0).collect();
        classes.sort_by(|a, b| unit.weights[*b].total_cmp(&unit.weights[*a]));
        let root = (classes.len() == 1).then(|| classes[0]);
        return Some(Chord { name: classes.iter().take(MOST_LOOSE).map(|&class| spell(class)).collect::<Vec<_>>().join(" "), root, third: None });
    };
    let mut name = format!("{}{suffix}", spell(root));
    if let Some(low) = bass.filter(|low| *low != root) {
        name.push('/');
        name.push_str(spell(low));
    }
    Some(Chord { name, root: Some(root), third: shape.iter().copied().find(|interval| matches!(*interval, MINOR_THIRD | MAJOR_THIRD)) })
}

fn period(names: &[Option<String>]) -> Option<usize> {
    let count = names.len();
    (UNITS_PER_BAR..=count / 2).step_by(UNITS_PER_BAR).find(|&period| (0..count - period).all(|unit| names[unit] == names[unit + period]))
}

fn changes(names: &[Option<String>]) -> String {
    let mut runs: Vec<(usize, usize, Option<&str>)> = Vec::new();
    for (unit, name) in names.iter().enumerate() {
        match runs.last_mut() {
            Some(run) if run.2 == name.as_deref() => run.1 += 1,
            _ => runs.push((unit, 1, name.as_deref())),
        }
    }
    let mut segments: Vec<(usize, usize, Option<&str>)> = Vec::new();
    for (start, length, name) in runs {
        match segments.last_mut() {
            Some(last) if name.is_none() && length < UNITS_PER_BAR && last.2.is_some() => last.1 += length,
            Some(last) if last.2 == name => last.1 += length,
            _ => segments.push((start, length, name)),
        }
    }
    let shown: Vec<String> = segments.iter().filter(|(_, length, name)| name.is_some() || *length >= UNITS_PER_BAR).map(|(start, _, name)| format!("{} {}", start * UNIT, name.unwrap_or(REST))).collect();
    let mut text = shown.iter().take(MOST_CHANGES).cloned().collect::<Vec<_>>().join(", ");
    if shown.len() > MOST_CHANGES {
        text.push_str(&format!(", … {}", counted(shown.len() - MOST_CHANGES, "more change")));
    }
    text
}

use super::arrange::bars;
use super::harmony::{arp_described, clip_described};
use super::path::pitch_name;
use sdk::json::{self, Json, tidy};
use crate::project::{Clip, File, Note, SongNote, VELOCITY, ticks};
use crate::theory::{Root, ScaleName};

type Object = Vec<(String, Json)>;

const LOWEST_PITCH: f64 = 0.0;
const HIGHEST_PITCH: f64 = 127.0;
const OCTAVE: f64 = 12.0;
const LEAST_VELOCITY: f64 = 1.0;
const MOST_VELOCITY: f64 = 127.0;

struct Clips<'a> {
    of_track: bool,
    song: bool,
    each: Vec<(String, &'a mut Object)>,
}

#[derive(Default)]
pub(super) struct Filter {
    from: Option<f64>,
    until: Option<f64>,
    above: Option<f64>,
    below: Option<f64>,
    picked: Option<Vec<usize>>,
}

impl Filter {
    pub(super) fn any(&self) -> bool {
        self.from.is_some() || self.until.is_some() || self.above.is_some() || self.below.is_some() || self.picked.is_some()
    }

    fn takes(&self, clip_start: f64, slot: usize, note: &Object) -> bool {
        self.holds(slot, read(note, "pitch").unwrap_or(0.0), clip_start + read(note, "startBeat").unwrap_or(0.0))
    }

    pub(super) fn holds(&self, slot: usize, pitch: f64, start: f64) -> bool {
        self.from.is_none_or(|from| start >= from) && self.until.is_none_or(|until| start < until) && self.above.is_none_or(|above| pitch > above) && self.below.is_none_or(|below| pitch < below) && self.picked.as_ref().is_none_or(|picked| picked.contains(&slot))
    }

    fn timed(&self) -> String {
        match (self.from, self.until) {
            (Some(from), Some(until)) => format!(" in beats {} to {}", json::number(from), json::number(until)),
            (Some(from), None) => format!(" from beat {}", json::number(from)),
            (None, Some(until)) => format!(" before beat {}", json::number(until)),
            (None, None) => String::new(),
        }
    }

    pub(super) fn said(&self) -> String {
        let mut said = String::new();
        if self.picked.is_some() {
            said.push_str(" picked in the app");
        }
        let pitched: Vec<String> = [self.above.map(|above| format!("above {}", pitch_name(above as i64))), self.below.map(|below| format!("below {}", pitch_name(below as i64)))].into_iter().flatten().collect();
        if !pitched.is_empty() {
            said.push(' ');
            said.push_str(&pitched.join(" and "));
        }
        said.push_str(&self.timed());
        said
    }
}

pub(super) fn filter(args: &Json, document: &Json, at: &str) -> Result<Filter, String> {
    let number = |key: &str| match args.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(value) => value.as_f64().filter(|value| value.is_finite()).map(Some).ok_or_else(|| format!("{key} is a number")),
    };
    let (from, until, above, below) = (number("fromBeat")?, number("untilBeat")?, number("above")?, number("below")?);
    if from.is_some_and(|from| from < 0.0) {
        return Err(String::from("fromBeat is a song beat from 0"));
    }
    if let (Some(from), Some(until)) = (from, until)
        && until <= from
    {
        return Err(String::from("untilBeat is after fromBeat"));
    }
    let picked = match args.get("picked").and_then(Json::as_bool) {
        Some(true) => {
            let open = document.get("openClip").and_then(Json::as_str).ok_or("picked takes the clip open in the app's piano roll, and none is open")?;
            if open != at {
                return Err(format!("picked takes the open clip, {open}; path is {at}"));
            }
            let slots: Vec<usize> = document.get("pickedNotes").and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(Json::as_f64).map(|slot| slot as usize).collect();
            if slots.is_empty() {
                return Err(String::from("No notes are picked in the app's piano roll"));
            }
            Some(slots)
        }
        _ => None,
    };
    Ok(Filter { from, until, above, below, picked })
}

#[derive(Default)]
struct Moved {
    count: usize,
    floored: usize,
    capped: usize,
}

impl Moved {
    fn held(&mut self, value: f64, floor: f64, ceiling: f64) -> f64 {
        let held = value.clamp(floor, ceiling);
        self.count += 1;
        self.floored += usize::from(held <= floor);
        self.capped += usize::from(held >= ceiling);
        held
    }

    fn limits(&self, floor: f64, ceiling: f64, unit: &str) -> String {
        let mut out = String::new();
        for (count, edge, value) in [(self.floored, "floor", floor), (self.capped, "ceiling", ceiling)] {
            if count == 0 {
                continue;
            }
            out.push_str(&if self.count == 1 { format!(", its {edge} {value}{unit}") } else { format!(", {count} at the {edge} {value}{unit}") });
        }
        out
    }
}

pub(super) fn scaled(found: &Json, scale: &Json, at: &str, filter: &Filter) -> Result<(Json, String), String> {
    let factor = scale.as_f64().filter(|factor| factor.is_finite()).ok_or("scale is a number multiplying node values, velocities and wave levels")?;
    if let Some((curve, nodes)) = curve_scaled(found, factor, filter, at)? {
        return Ok((curve, format!("Scaled {at}, {}{}, by {factor}", counted(nodes, "node", "nodes"), filter.timed())));
    }
    let mut next = found.clone();
    let Some(Clips { of_track, each, .. }) = clips(&mut next, at) else {
        return Err(format!("scale takes a curve, a clip, a track or the whole song; {at} is none of these"));
    };
    if factor <= 0.0 {
        return Err(String::from("scale on a clip or a track is above 0; to silence, mute the track"));
    }
    let decibels = 20.0 * factor.log10();
    let (mut velocities, mut waves, mut clips_with_notes, mut level) = (Moved::default(), Moved::default(), 0, 0.0);
    for (_, clip) in each {
        let clip_start = read(clip, "startBeat").unwrap_or(0.0);
        let mut heard = false;
        for (_, note) in notes_of(clip).enumerate().filter(|(slot, note)| filter.takes(clip_start, *slot, note)) {
            let velocity = velocities.held((read(note, "velocity").unwrap_or(f64::from(VELOCITY)) * factor).round(), LEAST_VELOCITY, MOST_VELOCITY);
            set(note, "velocity", velocity, f64::from(VELOCITY));
            heard = true;
        }
        clips_with_notes += usize::from(heard);
        if filter.any() {
            continue;
        }
        if let Some(wave) = wave_of(clip) {
            level = tidy(waves.held(read(wave, "levelDb").unwrap_or(0.0) + decibels, -File::DECIBELS, File::DECIBELS));
            set(wave, "levelDb", level, 0.0);
        }
    }
    if velocities.count == 0 && waves.count == 0 {
        return Err(format!("{at} has no notes{}{} to scale", filter.said(), if filter.any() { "" } else { " or waves" }));
    }
    let mut said = Vec::new();
    if velocities.count > 0 {
        let mut part = counted(velocities.count, "velocity", "velocities");
        part.push_str(&filter.said());
        if of_track {
            part.push_str(&format!(" in {}", counted(clips_with_notes, "clip", "clips")));
        }
        part.push_str(&velocities.limits(LEAST_VELOCITY, MOST_VELOCITY, ""));
        said.push(part);
    }
    if waves.count > 0 {
        let part = if waves.count == 1 { format!("1 wave to {} dB", hundredths(level)) } else { format!("{} waves by {} dB", waves.count, hundredths(decibels)) };
        said.push(part + &waves.limits(-File::DECIBELS, File::DECIBELS, " dB"));
    }
    Ok((next, format!("Scaled {at} by {factor}: {}", said.join(", "))))
}

fn curve_scaled(curve: &Json, factor: f64, filter: &Filter, at: &str) -> Result<Option<(Json, usize)>, String> {
    let mut curve = curve.clone();
    let Json::Object(fields) = &mut curve else {
        return Ok(None);
    };
    let Some((_, Json::Array(nodes))) = fields.iter_mut().find(|(key, _)| key == "nodes") else {
        return Ok(None);
    };
    if filter.above.is_some() || filter.below.is_some() || filter.picked.is_some() {
        return Err(format!("above, below and picked choose notes; {at} is a curve, whose nodes fromBeat and untilBeat choose"));
    }
    let mut count = 0;
    for node in nodes.iter_mut() {
        let Json::Object(node) = node else {
            continue;
        };
        let beat = read(node, "beat").unwrap_or(0.0);
        if !(filter.from.is_none_or(|from| beat >= from) && filter.until.is_none_or(|until| beat < until)) {
            continue;
        }
        count += 1;
        node.retain(|(key, _)| key != "travel");
        for (key, value) in node.iter_mut() {
            if let (true, Json::Number(number)) = (key == "value", value) {
                *number *= factor;
            }
        }
    }
    if count == 0 {
        return Err(format!("{at} has no nodes{}", filter.timed()));
    }
    Ok(Some((curve, count)))
}

pub(super) fn transposed(found: &Json, semitones: &Json, at: &str, filter: &Filter) -> Result<(Json, String), String> {
    let steps = semitones.as_f64().filter(|steps| steps.is_finite() && steps.fract() == 0.0 && *steps != 0.0).ok_or("transpose is a whole number of semitones, not 0")?;
    let mut next = found.clone();
    let Some(Clips { of_track, song, each }) = clips(&mut next, at) else {
        return Err(format!("transpose takes a clip, a track or the whole song; {at} is none of these"));
    };
    let (mut notes, mut clips_with_notes, mut lowest, mut highest, mut above, mut below) = (0, 0, f64::INFINITY, f64::NEG_INFINITY, 0, 0);
    let (mut waves, mut left, mut semitones, mut past) = (0, 0, 0.0, None);
    for (place, clip) in each {
        let clip_start = read(clip, "startBeat").unwrap_or(0.0);
        let mut heard = false;
        for (_, note) in notes_of(clip).enumerate().filter(|(slot, note)| filter.takes(clip_start, *slot, note)) {
            let Some((_, Json::Number(pitch))) = note.iter_mut().find(|(key, _)| key == "pitch") else {
                continue;
            };
            if pitch.fract() != 0.0 {
                continue;
            }
            notes += 1;
            heard = true;
            lowest = lowest.min(*pitch);
            highest = highest.max(*pitch);
            *pitch += steps;
            above += usize::from(*pitch > HIGHEST_PITCH);
            below += usize::from(*pitch < LOWEST_PITCH);
        }
        clips_with_notes += usize::from(heard);
        if filter.any() {
            continue;
        }
        let Some(wave) = wave_of(clip) else {
            continue;
        };
        if song {
            left += 1;
            continue;
        }
        waves += 1;
        semitones = read(wave, "semitones").unwrap_or(0.0) + steps;
        if semitones.abs() > File::SEMITONES && past.is_none() {
            past = Some((place, semitones));
        }
        set(wave, "semitones", semitones, 0.0);
    }
    if notes == 0 && waves == 0 {
        let or_waves = if song || filter.any() { "" } else { " or waves" };
        let hint = if left > 0 { "; a wave is transposed at its own clip or track" } else { "" };
        return Err(format!("{at} has no notes{}{or_waves} to transpose{hint}", filter.said()));
    }
    if above > 0 {
        return Err(format!("Transposing {at} by {steps} would take {} above {HIGHEST_PITCH}, {} the highest; {}", counted(above, "note", "notes"), pitch_name(highest as i64), room("rise", HIGHEST_PITCH - highest)));
    }
    if below > 0 {
        return Err(format!("Transposing {at} by {steps} would take {} below {LOWEST_PITCH}, {} the lowest; {}", counted(below, "note", "notes"), pitch_name(lowest as i64), room("fall", lowest - LOWEST_PITCH)));
    }
    if let Some((wave_at, semitones)) = past {
        return Err(format!("Transposing {at} by {steps} would take {wave_at}'s wave to {} semitones, past {}", json::number(semitones), File::SEMITONES.copysign(semitones)));
    }
    let mut said = Vec::new();
    if notes > 0 {
        let mut part = counted(notes, "note", "notes");
        part.push_str(&filter.said());
        if of_track {
            part.push_str(&format!(" in {}", counted(clips_with_notes, "clip", "clips")));
        }
        let (low, high) = (pitch_name((lowest + steps) as i64), pitch_name((highest + steps) as i64));
        part.push_str(&if low == high { format!(", now {low}") } else { format!(", now {low} to {high}") });
        said.push(part);
    }
    match waves {
        0 => {}
        1 => said.push(format!("1 wave to {} semitones", json::number(semitones))),
        many => said.push(format!("{many} waves by {steps} semitones")),
    }
    if left > 0 {
        said.push(format!("{} left alone", counted(left, "wave", "waves")));
    }
    if song && !filter.any() {
        let was = Root::from_name(next.get("root").and_then(Json::as_str).unwrap_or("C")).unwrap_or(Root::C);
        let now = Root::ALL[(was.index() as i64 + steps as i64).rem_euclid(Root::ALL.len() as i64) as usize];
        if let Json::Object(fields) = &mut next {
            let root = Json::String(String::from(now.name()));
            match fields.iter_mut().find(|(key, _)| key == "root") {
                Some((_, value)) => *value = root,
                None => fields.push((String::from("root"), root)),
            }
        }
        said.push(format!("root {} to {}", was.name(), now.name()));
    }
    Ok((next, format!("Transposed {at} by {steps}: {}", said.join(", "))))
}

pub(super) fn compared(before: &Clip, after: &Clip, home: Option<(Root, ScaleName)>, played: Option<&[SongNote]>) -> Option<String> {
    let mut gone = before.notes.clone();
    let mut fresh = Vec::new();
    for note in &after.notes {
        match gone.iter().position(|old| old == note) {
            Some(at) => {
                gone.swap_remove(at);
            }
            None => fresh.push(*note),
        }
    }
    if fresh.is_empty() && gone.is_empty() {
        return None;
    }
    let changed = fresh.len().min(gone.len());
    let mut parts = Vec::new();
    if changed > 0 {
        parts.push(format!("{changed} of {} changed", counted(after.notes.len(), "note", "notes")));
    }
    for (count, verb) in [(fresh.len() - changed, "added"), (gone.len() - changed, "removed")].into_iter().filter(|(count, _)| *count > 0) {
        parts.push(if parts.is_empty() { format!("{} {verb}", counted(count, "note", "notes")) } else { format!("{count} {verb}") });
    }
    if let Some((low, high)) = after.pitch_range() {
        parts.push(if low == high { pitch_name(i64::from(low)) } else { format!("{} to {}", pitch_name(i64::from(low)), pitch_name(i64::from(high))) });
    }
    let placed = |clip: &Clip, note: &Note| (clip.start + ticks(f64::from(note.start)), clip.start + ticks(f64::from(note.start + note.length)));
    let touched: Vec<(i32, i32)> = fresh.iter().map(|note| placed(after, note)).chain(gone.iter().map(|note| placed(before, note))).collect();
    if let (Some(first), Some(last)) = (touched.iter().map(|span| span.0).min(), touched.iter().map(|span| span.1).max()) {
        parts.push(bars(&[(first, last.max(first + 1))]));
    }
    if !before.notes.is_empty() && !after.notes.is_empty() {
        let classes = |notes: &[Note]| notes.iter().fold(0u16, |held, note| held | 1 << (note.pitch % 12));
        parts.push(String::from(if classes(&before.notes) == classes(&after.notes) { "pitch classes kept" } else { "pitch classes changed" }));
        let timing = |notes: &[Note]| {
            let mut spans: Vec<(i32, i32)> = notes.iter().map(|note| (ticks(f64::from(note.start)), ticks(f64::from(note.length)))).collect();
            spans.sort_unstable();
            spans
        };
        parts.push(String::from(if timing(&before.notes) == timing(&after.notes) { "timing kept" } else { "timing changed" }));
    }
    let mut line = parts.join(", ");
    for text in [clip_described(&after.notes, home), played.and_then(|played| arp_described(played, after, home))].into_iter().flatten() {
        line.push_str("; ");
        line.push_str(&text);
    }
    Some(line)
}

pub(super) fn fitted(found: &Json, range: &Json, at: &str, filter: &Filter) -> Result<(Json, String), String> {
    let shape = "fit is [lowest, highest], whole pitches 0 to 127 spanning twelve or more";
    let (lowest, highest) = match range.as_array().map(|pair| pair.iter().map(Json::as_f64).collect::<Option<Vec<f64>>>()) {
        Some(Some(pair)) if pair.len() == 2 && pair.iter().all(|pitch| pitch.fract() == 0.0 && (LOWEST_PITCH..=HIGHEST_PITCH).contains(pitch)) && pair[1] - pair[0] >= OCTAVE - 1.0 => (pair[0], pair[1]),
        _ => return Err(String::from(shape)),
    };
    let mut next = found.clone();
    let Some(Clips { of_track, each, .. }) = clips(&mut next, at) else {
        return Err(format!("fit takes a clip, a track or the whole song; {at} is none of these"));
    };
    let (mut matched, mut up, mut down, mut dropped, mut clips_with_notes) = (0, 0, 0, 0, 0);
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for (_, clip) in each {
        let clip_start = read(clip, "startBeat").unwrap_or(0.0);
        let (mut moved, mut heard) = (Vec::new(), false);
        for (slot, note) in notes_of(clip).enumerate().filter(|(slot, note)| filter.takes(clip_start, *slot, note)) {
            let Some((_, Json::Number(pitch))) = note.iter_mut().find(|(key, _)| key == "pitch") else {
                continue;
            };
            if pitch.fract() != 0.0 {
                continue;
            }
            matched += 1;
            heard = true;
            let landed = nearest(*pitch, lowest, highest);
            up += usize::from(landed > *pitch);
            down += usize::from(landed < *pitch);
            if landed != *pitch {
                moved.push(slot);
            }
            *pitch = landed;
            low = low.min(landed);
            high = high.max(landed);
        }
        clips_with_notes += usize::from(heard);
        dropped += undoubled(clip, &moved);
    }
    if matched == 0 {
        return Err(format!("{at} has no notes{} to fit", filter.said()));
    }
    let range = format!("{} to {}", pitch_name(lowest as i64), pitch_name(highest as i64));
    if up + down == 0 {
        return Ok((next, format!("Fitted {at} into {range}: every note{} is within it already", filter.said())));
    }
    let mut said = format!("{} of {}{} moved by octaves", up + down, counted(matched, "note", "notes"), filter.said());
    if of_track {
        said.push_str(&format!(" in {}", counted(clips_with_notes, "clip", "clips")));
    }
    said.push_str(&match (up, down) {
        (up, 0) => format!(", {up} up"),
        (0, down) => format!(", {down} down"),
        (up, down) => format!(", {up} up and {down} down"),
    });
    said.push_str(&format!(", now {} to {}", pitch_name(low as i64), pitch_name(high as i64)));
    if dropped > 0 {
        said.push_str(&format!("; {} dropped as doubles", counted(dropped, "note", "notes")));
    }
    Ok((next, format!("Fitted {at} into {range}: {said}")))
}

fn nearest(pitch: f64, lowest: f64, highest: f64) -> f64 {
    if (lowest..=highest).contains(&pitch) {
        return pitch;
    }
    let first = lowest + (pitch - lowest).rem_euclid(OCTAVE);
    if pitch < lowest { first } else { first + ((highest - first) / OCTAVE).floor() * OCTAVE }
}

fn undoubled(clip: &mut Object, moved: &[usize]) -> usize {
    let Some((_, Json::Array(notes))) = clip.iter_mut().find(|(key, _)| key == "notes") else {
        return 0;
    };
    let placed = |note: &Json| (note.get("pitch").and_then(Json::as_f64), note.get("startBeat").and_then(Json::as_f64).unwrap_or(0.0));
    let mut doubles: Vec<usize> = Vec::new();
    for &slot in moved {
        let own = placed(&notes[slot]);
        if notes.iter().enumerate().any(|(other, note)| other != slot && !doubles.contains(&other) && placed(note) == own) {
            doubles.push(slot);
        }
    }
    doubles.sort_unstable();
    for slot in doubles.iter().rev() {
        notes.remove(*slot);
    }
    doubles.len()
}

fn clips<'a>(json: &'a mut Json, at: &str) -> Option<Clips<'a>> {
    let song = json.get("tracks").is_some();
    let track = json.get("clips").is_some() || matches!(json.get("kind").and_then(Json::as_str), Some("midi" | "audio" | "group"));
    let clip = json.get("notes").is_some() || json.get("wave").is_some();
    let Json::Object(fields) = json else {
        return None;
    };
    if song {
        let each = match fields.iter_mut().find(|(key, _)| key == "tracks") {
            Some((_, Json::Array(tracks))) => tracks
                .iter_mut()
                .enumerate()
                .flat_map(|(index, track)| {
                    let clips = match track {
                        Json::Object(track) => clips_in(track),
                        _ => Vec::new(),
                    };
                    clips.into_iter().map(move |(slot, clip)| (format!("tracks/{index}/clips/{slot}"), clip))
                })
                .collect(),
            _ => Vec::new(),
        };
        return Some(Clips { of_track: true, song: true, each });
    }
    if track {
        let each = clips_in(fields).into_iter().map(|(slot, clip)| (format!("{at}/clips/{slot}"), clip)).collect();
        return Some(Clips { of_track: true, song: false, each });
    }
    clip.then(|| Clips { of_track: false, song: false, each: vec![(String::from(at), fields)] })
}

fn clips_in(fields: &mut Object) -> Vec<(usize, &mut Object)> {
    match fields.iter_mut().find(|(key, _)| key == "clips") {
        Some((_, Json::Array(items))) => items
            .iter_mut()
            .enumerate()
            .filter_map(|(index, clip)| match clip {
                Json::Object(clip) => Some((index, clip)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn notes_of(clip: &mut Object) -> impl Iterator<Item = &mut Object> {
    let notes = match clip.iter_mut().find(|(key, _)| key == "notes") {
        Some((_, Json::Array(notes))) => notes.as_mut_slice(),
        _ => &mut [],
    };
    notes.iter_mut().filter_map(|note| match note {
        Json::Object(note) => Some(note),
        _ => None,
    })
}

fn wave_of(clip: &mut Object) -> Option<&mut Object> {
    match clip.iter_mut().find(|(key, _)| key == "wave") {
        Some((_, Json::Object(wave))) => Some(wave),
        _ => None,
    }
}

fn read(object: &Object, key: &str) -> Option<f64> {
    object.iter().find(|(known, _)| known == key).and_then(|(_, value)| value.as_f64())
}

fn set(object: &mut Object, key: &str, value: f64, default: f64) {
    match object.iter_mut().find(|(known, _)| known == key) {
        Some((_, slot)) => *slot = Json::Number(value),
        None if value != default => object.push((String::from(key), Json::Number(value))),
        None => {}
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn room(verb: &str, by: f64) -> String {
    if by == 0.0 { format!("it cannot {verb}") } else { format!("it can {verb} by {by} at most") }
}

fn hundredths(value: f64) -> String {
    json::number((value * 100.0).round() / 100.0)
}

const MOST_STEPS: usize = 256;
const MOST_MADE: usize = 8192;
const STEP_BEATS: f64 = 0.25;
const CHORD_BEATS: f64 = 4.0;
pub(super) const CLIP_BEATS: f64 = 4.0;
const PATTERN_VELOCITY: f64 = 100.0;
const MOST_SWING: f64 = 0.5;
const NOTE_KEYS: [&str; 4] = ["pitch", "startBeat", "lengthBeats", "velocity"];

struct Hit {
    start: f64,
    length: f64,
    pitches: Vec<f64>,
    velocity: f64,
    slides: bool,
}

pub(super) fn patterned(json: &mut Json, steps: &[String], document: &Json, out: &mut Vec<String>) -> Result<(), String> {
    if steps.last().is_some_and(|step| step == "notes") {
        let clip = &steps[..steps.len() - 1];
        let length = super::path::locate(document, clip).and_then(|clip| clip.get("lengthBeats")).and_then(Json::as_f64).unwrap_or(CLIP_BEATS);
        return notes_made(json, length, &super::path::joined(clip), out);
    }
    if let [clip @ .., notes, index] = steps
        && notes == "notes"
        && index.parse::<usize>().is_ok()
        && json.as_array().is_some()
    {
        *json = tupled(json, &super::path::joined(clip))?;
        return Ok(());
    }
    let mut at = steps.to_vec();
    patterns_in(json, &mut at, out)
}

fn notes_made(notes: &mut Json, length: f64, at: &str, out: &mut Vec<String>) -> Result<(), String> {
    match notes {
        Json::Object(_) => {
            let (made, said) = expanded(notes, length, at)?;
            *notes = made;
            out.push(said);
        }
        Json::Array(items) => {
            for item in items.iter_mut().filter(|item| matches!(item, Json::Array(_))) {
                *item = tupled(item, at)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn tupled(note: &Json, at: &str) -> Result<Json, String> {
    match note.as_array().and_then(|items| items.iter().map(Json::as_f64).collect::<Option<Vec<f64>>>()) {
        Some(numbers) if (3..=NOTE_KEYS.len()).contains(&numbers.len()) => Ok(super::object(NOTE_KEYS.into_iter().zip(numbers).map(|(key, number)| (key, Json::Number(number))).collect())),
        _ => Err(format!("{at}: a note as a list is 3 or 4 numbers: pitch, startBeat, lengthBeats, velocity")),
    }
}

fn patterns_in(json: &mut Json, at: &mut Vec<String>, out: &mut Vec<String>) -> Result<(), String> {
    match json {
        Json::Object(fields) => {
            let length = fields.iter().find(|(key, _)| key == "lengthBeats").and_then(|(_, value)| value.as_f64()).unwrap_or(CLIP_BEATS);
            for (key, value) in fields.iter_mut() {
                if key.as_str() == "notes" {
                    notes_made(value, length, &super::path::joined(at), out)?;
                    continue;
                }
                at.push(key.clone());
                patterns_in(value, at, out)?;
                at.pop();
            }
            Ok(())
        }
        Json::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                at.push(index.to_string());
                patterns_in(item, at, out)?;
                at.pop();
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn expanded(pattern: &Json, length: f64, at: &str) -> Result<(Json, String), String> {
    let text = pattern.get("steps").and_then(Json::as_str).ok_or_else(|| format!("{at}: notes is a list or a pattern with steps; this object has none"))?;
    let (steps, slides) = cells(text, at)?;
    let number = |key: &str, default: f64| match pattern.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(value) => value.as_f64().filter(|value| value.is_finite() && *value > 0.0).ok_or_else(|| format!("{at}: {key} is a number of beats above 0")),
    };
    let step = number("stepBeats", STEP_BEATS)?;
    let gate = number("noteBeats", step)?;
    let chord_beats = number("chordBeats", CHORD_BEATS)?;
    let chords = chords_of(pattern, at)?;
    let arp = pattern.get("arp").and_then(Json::as_bool).unwrap_or(false);
    let (from, to) = velocities_of(pattern, at)?;
    let accent = bounded(pattern, "accent", MOST_VELOCITY, LEAST_VELOCITY..=MOST_VELOCITY).ok_or_else(|| format!("{at}: accent is the velocity of each X, 1 to 127, default 127"))?;
    let swing = bounded(pattern, "swing", 0.0, 0.0..=MOST_SWING).ok_or_else(|| format!("{at}: swing delays every second step by 0 to {MOST_SWING} of a step"))?;
    let (period, ramp) = (steps.len() as f64 * step, (length - step).max(step));
    let (mut hits, mut struck, mut turn, mut tile) = (Vec::<Hit>::new(), 0usize, 0usize, 0usize);
    while (tile as f64) * period < length {
        let base = tile as f64 * period;
        let mut index = 0;
        while index < steps.len() {
            if !matches!(steps[index], 'x' | 'X') {
                index += 1;
                continue;
            }
            let held = steps[index + 1..].iter().take_while(|step| **step == '-').count();
            let straight = base + index as f64 * step;
            let late = if (tile * steps.len() + index) % 2 == 1 { swing * step } else { 0.0 };
            let (start, accented, last) = (straight + late, steps[index] == 'X', index + held);
            index = last + 1;
            if start >= length {
                continue;
            }
            let velocity = if accented { accent } else { (from + (to - from) * (straight / ramp).min(1.0)).round().clamp(LEAST_VELOCITY, MOST_VELOCITY) };
            let chord = &chords[((straight / chord_beats).floor() as usize) % chords.len()];
            let pitches: Vec<f64> = if arp { vec![chord[turn % chord.len()]] } else { chord.clone() };
            turn += 1;
            struck += pitches.len();
            if struck > MOST_MADE {
                return Err(format!("{at}: over {MOST_MADE} notes; shorten the clip or raise stepBeats"));
            }
            let span = gate + held as f64 * step;
            let beats = if late > 0.0 { span.min((held + 1) as f64 * step - late) } else { span };
            hits.push(Hit { start, length: beats, pitches, velocity, slides: slides[last] });
        }
        tile += 1;
    }
    let starts: Vec<f64> = hits.iter().skip(1).map(|hit| hit.start).collect();
    for (hit, next) in hits.iter_mut().zip(starts).filter(|(hit, _)| hit.slides) {
        hit.length = next - hit.start;
    }
    let slid: Vec<bool> = std::iter::once(false).chain(hits.iter().map(|hit| hit.slides)).collect();
    let notes: Vec<Json> = hits
        .iter()
        .zip(slid)
        .flat_map(|(hit, slid)| {
            hit.pitches.iter().map(move |pitch| {
                let mut note = vec![("pitch", Json::Number(*pitch)), ("startBeat", Json::Number(tidy(hit.start))), ("lengthBeats", Json::Number(tidy(hit.length))), ("velocity", Json::Number(hit.velocity))];
                if slid {
                    note.push(("slide", Json::Bool(true)));
                }
                super::object(note)
            })
        })
        .collect();
    let made = format!("{at}: {} from the {}-step pattern", counted(notes.len(), "note", "notes"), steps.len());
    Ok((Json::Array(notes), made))
}

fn cells(text: &str, at: &str) -> Result<(Vec<char>, Vec<bool>), String> {
    let (mut steps, mut slides) = (Vec::new(), Vec::new());
    for cell in text.chars().filter(|cell| !cell.is_whitespace()) {
        match cell {
            'x' | 'X' | '.' | '-' => {
                steps.push(cell);
                slides.push(false);
            }
            '~' => match (steps.last(), slides.last_mut()) {
                (Some('x' | 'X' | '-'), Some(slide)) => *slide = true,
                _ => return Err(format!("{at}: ~ must follow x, X or -")),
            },
            stray => return Err(format!("{at}: {stray} is not a step: x note, X accent, . rest, - hold, ~ slide")),
        }
    }
    if steps.is_empty() || steps.len() > MOST_STEPS {
        return Err(format!("{at}: steps holds 1 to {MOST_STEPS} steps"));
    }
    if !steps.iter().any(|step| matches!(step, 'x' | 'X')) {
        return Err(format!("{at}: steps has no x, so no notes"));
    }
    Ok((steps, slides))
}

fn bounded(pattern: &Json, key: &str, default: f64, range: std::ops::RangeInclusive<f64>) -> Option<f64> {
    match pattern.get(key) {
        None | Some(Json::Null) => Some(default),
        Some(value) => value.as_f64().filter(|value| range.contains(value)),
    }
}

fn chords_of(pattern: &Json, at: &str) -> Result<Vec<Vec<f64>>, String> {
    let pitch = |value: &Json| value.as_f64().filter(|pitch| pitch.fract() == 0.0 && (LOWEST_PITCH..=HIGHEST_PITCH).contains(pitch)).ok_or_else(|| format!("{at}: a pitch is a whole number 0 to 127, 60 C4"));
    let chord = |value: &Json| -> Result<Vec<f64>, String> {
        let pitches = value.as_array().ok_or_else(|| format!("{at}: pitches is a chord, or a list of chords, one per chordBeats"))?;
        if pitches.is_empty() {
            return Err(format!("{at}: a chord in pitches holds at least one pitch"));
        }
        pitches.iter().map(pitch).collect()
    };
    match (pattern.get("pitch"), pattern.get("pitches")) {
        (Some(single), None) => Ok(vec![vec![pitch(single)?]]),
        (None, Some(Json::Array(items))) if !items.is_empty() && items.iter().all(|item| item.as_array().is_some()) => items.iter().map(chord).collect(),
        (None, Some(list @ Json::Array(_))) => Ok(vec![chord(list)?]),
        (Some(_), Some(_)) => Err(format!("{at}: a pattern takes pitch or pitches, not both")),
        _ => Err(format!("{at}: a pattern needs pitch or pitches")),
    }
}

fn velocities_of(pattern: &Json, at: &str) -> Result<(f64, f64), String> {
    let wrong = || format!("{at}: velocity is 1 to 127, or [from, to] across the clip; default {PATTERN_VELOCITY}");
    let one = |value: &Json| value.as_f64().filter(|velocity| (LEAST_VELOCITY..=MOST_VELOCITY).contains(velocity)).ok_or_else(wrong);
    match pattern.get("velocity") {
        None | Some(Json::Null) => Ok((PATTERN_VELOCITY, PATTERN_VELOCITY)),
        Some(Json::Array(pair)) => match pair.as_slice() {
            [from, to] => Ok((one(from)?, one(to)?)),
            _ => Err(wrong()),
        },
        Some(value) => one(value).map(|velocity| (velocity, velocity)),
    }
}

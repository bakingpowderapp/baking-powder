use crate::arp::Articulation;
use crate::inflate;
use crate::project::{BEATS_PER_BAR, Clip, Note};

pub const HEADER: &[u8] = b"MThd";
const TRACK: &[u8] = b"MTrk";
const SMPTE: u16 = 0x8000;
const DROP_FRAME: i8 = -29;
const DROP_FRAME_RATE: f64 = 29.97;
const CHANNELS: usize = 16;
const DRUMS: usize = 9;
const PITCHES: usize = 128;
const DATA: u8 = 0x7F;
const CHANNEL: u8 = 0x0F;
const NOTE_OFF: u8 = 0x80;
const NOTE_ON: u8 = 0x90;
const PROGRAM: u8 = 0xC0;
const PRESSURE: u8 = 0xD0;
const SYSEX: u8 = 0xF0;
const ESCAPE: u8 = 0xF7;
const META: u8 = 0xFF;
const TRACK_NAME: u8 = 0x03;
const END_OF_TRACK: u8 = 0x2F;
const TEMPO: u8 = 0x51;
const TIME_SIGNATURE: u8 = 0x58;
const SINGLE: u16 = 0;
const PARALLEL: u16 = 1;
const DIVISION: u16 = 480;
const QUARTER_NOTE: u8 = 2;
const CLOCKS_PER_CLICK: u8 = 24;
const THIRTY_SECONDS_PER_QUARTER: u8 = 8;
const OFF_VELOCITY: u8 = 64;
const MICROSECONDS_PER_MINUTE: f64 = 60_000_000.0;
const QUANTITY_BYTES: usize = 4;
const TAG_BYTES: usize = 4;
const CLIP: &str = "MidiClip";
const CLIP_END: &str = "/MidiClip";
const CLIP_START: &str = "CurrentStart";
const CLIP_UNTIL: &str = "CurrentEnd";
const CLIP_NAME: &str = "Name";
const KEY_TRACK: &str = "KeyTrack";
const KEY_TRACK_END: &str = "/KeyTrack";
const KEY: &str = "MidiKey";
const EVENT: &str = "MidiNoteEvent";
const VALUE: &str = "Value";
const TIME: &str = "Time";
const DURATION: &str = "Duration";
const VELOCITY: &str = "Velocity";
const ENABLED: &str = "IsEnabled";
const OFF: &str = "false";
const ENTITIES: [(&str, &str); 5] = [("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&apos;", "'"), ("&amp;", "&")];

pub struct Part {
    pub name: String,
    pub drums: bool,
    pub notes: Vec<Note>,
    pub length: f32,
}

impl Part {
    pub fn label(&self, stem: &str) -> String {
        if self.name.is_empty() { String::from(stem) } else { self.name.clone() }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.first().copied()
    }

    fn byte(&mut self) -> Option<u8> {
        let (&first, rest) = self.bytes.split_first()?;
        self.bytes = rest;
        Some(first)
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.bytes.split_at_checked(count)?;
        self.bytes = rest;
        Some(taken)
    }

    fn word(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes([self.byte()?, self.byte()?]))
    }

    fn long(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes([self.byte()?, self.byte()?, self.byte()?, self.byte()?]))
    }

    fn quantity(&mut self) -> Option<u32> {
        let mut value = 0;
        for _ in 0..QUANTITY_BYTES {
            let byte = self.byte()?;
            value = (value << 7) | u32::from(byte & DATA);
            if byte & !DATA == 0 {
                return Some(value);
            }
        }
        None
    }

    fn chunk(&mut self) -> Option<(&'a [u8], &'a [u8], bool)> {
        let tag = self.take(TAG_BYTES)?;
        let length = self.long()? as usize;
        let whole = length <= self.bytes.len();
        Some((tag, self.take(length.min(self.bytes.len()))?, whole))
    }
}

struct Voices {
    sounding: Vec<Option<(u64, u8)>>,
    notes: Vec<Note>,
    beats_per_tick: f64,
    pitched: bool,
}

impl Voices {
    fn new(beats_per_tick: f64) -> Voices {
        Voices { sounding: vec![None; CHANNELS * PITCHES], notes: Vec::new(), beats_per_tick, pitched: false }
    }

    fn beats(&self, ticks: u64) -> f32 {
        (ticks as f64 * self.beats_per_tick) as f32
    }

    fn off(&mut self, slot: usize, ticks: u64) {
        let Some((start, velocity)) = self.sounding[slot].take() else {
            return;
        };
        if ticks > start {
            self.pitched |= slot / PITCHES != DRUMS;
            self.notes.push(Note { pitch: (slot % PITCHES) as u8, start: self.beats(start), length: self.beats(ticks - start), velocity, articulation: Articulation::PLAIN });
        }
    }

    fn on(&mut self, slot: usize, velocity: u8, ticks: u64) {
        self.off(slot, ticks);
        self.sounding[slot] = Some((ticks, velocity));
    }

    fn finish(&mut self, ticks: u64) -> Vec<Note> {
        for slot in 0..self.sounding.len() {
            self.off(slot, ticks);
        }
        settle(std::mem::take(&mut self.notes))
    }
}

fn settle(mut notes: Vec<Note>) -> Vec<Note> {
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    let mut last = [None; PITCHES];
    for index in 0..notes.len() {
        let (pitch, start) = (usize::from(notes[index].pitch), notes[index].start);
        if let Some(previous) = last[pitch].replace(index) {
            let before: &mut Note = &mut notes[previous];
            before.length = before.length.min(start - before.start);
        }
    }
    notes.retain(|note| note.length > 0.0);
    notes
}

fn beats_per_tick(division: u16, bpm: f64) -> Option<f64> {
    if division & SMPTE == 0 {
        return (division > 0).then(|| 1.0 / f64::from(division));
    }
    let [high, low] = division.to_be_bytes();
    let frames = match i8::from_be_bytes([high]) {
        DROP_FRAME => DROP_FRAME_RATE,
        frames => -f64::from(frames),
    };
    let ticks = f64::from(low);
    (frames > 0.0 && ticks > 0.0).then(|| bpm / (60.0 * frames * ticks))
}

fn part(track: &[u8], whole: bool, beats_per_tick: f64) -> Option<Part> {
    let mut voices = Voices::new(beats_per_tick);
    let mut name = String::new();
    let mut ticks = 0;
    if events(Reader { bytes: track }, &mut voices, &mut name, &mut ticks).is_none() && whole {
        return None;
    }
    let notes = voices.finish(ticks);
    Some(Part { name, drums: !voices.pitched, notes, length: voices.beats(ticks) })
}

fn events(mut reader: Reader<'_>, voices: &mut Voices, name: &mut String, ticks: &mut u64) -> Option<()> {
    let mut status = 0u8;
    while !reader.bytes.is_empty() {
        *ticks += u64::from(reader.quantity()?);
        let next = reader.peek()?;
        let event = if next & !DATA == 0 { status } else { reader.byte()? };
        match event {
            META => {
                let kind = reader.byte()?;
                let length = reader.quantity()? as usize;
                let data = reader.take(length)?;
                if kind == TRACK_NAME && name.is_empty() {
                    *name = String::from(String::from_utf8_lossy(data).trim());
                }
                if kind == END_OF_TRACK {
                    break;
                }
            }
            SYSEX | ESCAPE => {
                let length = reader.quantity()? as usize;
                reader.take(length)?;
            }
            NOTE_OFF..SYSEX => {
                status = event;
                let kind = event & !CHANNEL;
                let first = reader.byte()? & DATA;
                let second = if matches!(kind, PROGRAM | PRESSURE) { 0 } else { reader.byte()? & DATA };
                let slot = usize::from(event & CHANNEL) * PITCHES + usize::from(first);
                match kind {
                    NOTE_ON if second > 0 => voices.on(slot, second, *ticks),
                    NOTE_ON | NOTE_OFF => voices.off(slot, *ticks),
                    _ => {}
                }
            }
            _ => return None,
        }
    }
    Some(())
}

struct Tags<'a> {
    text: &'a str,
}

impl<'a> Iterator for Tags<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let open = self.text.find('<')?;
        let inside = &self.text[open + 1..];
        let close = inside.find('>')?;
        self.text = &inside[close + 1..];
        Some(&inside[..close])
    }
}

fn attribute<'a>(attributes: &'a str, key: &str) -> Option<&'a str> {
    attributes.match_indices(key).find_map(|(at, _)| {
        let starts = at == 0 || attributes[..at].ends_with(' ');
        let value = starts.then(|| attributes[at + key.len()..].strip_prefix("=\""))??;
        value.find('"').map(|end| &value[..end])
    })
}

fn unescaped(text: &str) -> String {
    ENTITIES.iter().fold(String::from(text), |text, (entity, plain)| text.replace(entity, plain))
}

struct Scan {
    name: Option<String>,
    start: f32,
    end: f32,
    key: Option<u8>,
    events: Vec<(f32, f32, u8)>,
    notes: Vec<Note>,
}

impl Scan {
    fn new() -> Scan {
        Scan { name: None, start: 0.0, end: f32::INFINITY, key: None, events: Vec::new(), notes: Vec::new() }
    }

    fn flush(&mut self) {
        if let Some(pitch) = self.key.take() {
            self.notes.extend(self.events.drain(..).map(|(start, length, velocity)| Note { pitch, start, length, velocity, articulation: Articulation::PLAIN }));
        }
        self.events.clear();
    }

    fn part(mut self) -> Part {
        self.flush();
        let (start, end) = (self.start, self.end);
        let notes = self.notes.into_iter().filter(|note| note.start >= start && note.start < end).map(|note| Note { start: note.start - start, length: note.length.min(end - note.start), ..note }).collect();
        let length = if end.is_finite() { end - start } else { 0.0 };
        Part { name: self.name.unwrap_or_default(), drums: false, notes: settle(notes), length }
    }
}

fn clips(xml: &str) -> Option<Vec<Part>> {
    let mut parts = Vec::new();
    let mut scan: Option<Scan> = None;
    let mut seen = false;
    let tags = Tags { text: xml };
    for tag in tags {
        let (name, attributes) = tag.split_once(' ').unwrap_or((tag, ""));
        if name == CLIP_END {
            if let Some(part) = scan.take().map(Scan::part).filter(|part| !part.notes.is_empty()) {
                parts.push(part);
            }
            continue;
        }
        let Some(clip) = scan.as_mut() else {
            if name == CLIP {
                scan = Some(Scan::new());
                seen = true;
            }
            continue;
        };
        let number = |key: &str| attribute(attributes, key).and_then(|text| text.parse::<f32>().ok()).filter(|value| value.is_finite());
        match name {
            CLIP_START => clip.start = number(VALUE).unwrap_or(clip.start),
            CLIP_UNTIL => clip.end = number(VALUE).unwrap_or(clip.end),
            CLIP_NAME if clip.name.is_none() => clip.name = attribute(attributes, VALUE).map(unescaped),
            KEY => clip.key = number(VALUE).map(|key| key.clamp(0.0, 127.0) as u8),
            KEY_TRACK | KEY_TRACK_END => clip.flush(),
            EVENT if attribute(attributes, ENABLED) != Some(OFF) => {
                if let (Some(time), Some(duration), Some(velocity)) = (number(TIME), number(DURATION), number(VELOCITY)) {
                    clip.events.push((time, duration, velocity.round().clamp(1.0, 127.0) as u8));
                }
            }
            _ => {}
        }
    }
    seen.then_some(parts)
}

pub fn parse(bytes: &[u8], bpm: f64) -> Option<Vec<Part>> {
    if let Some(unpacked) = inflate::gunzip(bytes) {
        return clips(&String::from_utf8_lossy(&unpacked));
    }
    let mut reader = Reader { bytes };
    let (tag, header, _) = reader.chunk()?;
    if tag != HEADER {
        return None;
    }
    let mut header = Reader { bytes: header };
    let (_, _, division) = (header.word()?, header.word()?, header.word()?);
    let beats_per_tick = beats_per_tick(division, bpm)?;
    let mut parts = Vec::new();
    while let Some((tag, body, whole)) = reader.chunk() {
        if tag != TRACK {
            continue;
        }
        let part = part(body, whole, beats_per_tick)?;
        if !part.notes.is_empty() {
            parts.push(part);
        }
    }
    Some(parts)
}

fn quantity(bytes: &mut Vec<u8>, value: u32) {
    let mut out = vec![(value & u32::from(DATA)) as u8];
    let mut rest = value >> 7;
    while rest > 0 {
        out.push((rest & u32::from(DATA)) as u8 | !DATA);
        rest >>= 7;
    }
    out.reverse();
    bytes.extend(out);
}

fn meta(track: &mut Vec<u8>, delta: u32, kind: u8, data: &[u8]) {
    quantity(track, delta);
    track.extend([META, kind]);
    quantity(track, data.len() as u32);
    track.extend_from_slice(data);
}

fn chunk(bytes: &mut Vec<u8>, tag: &[u8], body: &[u8]) {
    bytes.extend_from_slice(tag);
    bytes.extend((body.len() as u32).to_be_bytes());
    bytes.extend_from_slice(body);
}

fn ticks(beat: f32) -> u32 {
    (f64::from(beat) * f64::from(DIVISION)).round() as u32
}

pub fn write(tracks: &[(String, Vec<Clip>)], bpm: f64) -> Vec<u8> {
    let tempo = (MICROSECONDS_PER_MINUTE / bpm).round() as u32;
    let format = if tracks.len() == 1 { SINGLE } else { PARALLEL };
    let mut bytes = Vec::new();
    chunk(&mut bytes, HEADER, &[format.to_be_bytes(), (tracks.len() as u16).to_be_bytes(), DIVISION.to_be_bytes()].concat());
    for (index, (name, clips)) in tracks.iter().enumerate() {
        let (mut events, mut end) = (Vec::new(), 0);
        for clip in clips {
            let (base, length) = (crate::project::beats(clip.start) as f32, clip.beats());
            end = end.max(ticks(base + length));
            for note in clip.notes.iter().filter(|note| note.start < length) {
                let on = ticks(base + note.start);
                let off = ticks(base + (note.start + note.length).min(length)).max(on + 1);
                events.push((on, NOTE_ON, note.pitch & DATA, note.velocity.clamp(1, DATA)));
                events.push((off, NOTE_OFF, note.pitch & DATA, OFF_VELOCITY));
            }
        }
        events.sort_unstable();
        let mut track = Vec::new();
        meta(&mut track, 0, TRACK_NAME, name.as_bytes());
        if index == 0 {
            meta(&mut track, 0, TEMPO, &tempo.to_be_bytes()[1..]);
            meta(&mut track, 0, TIME_SIGNATURE, &[BEATS_PER_BAR as u8, QUARTER_NOTE, CLOCKS_PER_CLICK, THIRTY_SECONDS_PER_QUARTER]);
        }
        let mut at = 0;
        for (tick, status, pitch, velocity) in events {
            quantity(&mut track, tick - at);
            track.extend([status, pitch, velocity]);
            at = tick;
        }
        meta(&mut track, end.saturating_sub(at), END_OF_TRACK, &[]);
        chunk(&mut bytes, TRACK, &track);
    }
    bytes
}

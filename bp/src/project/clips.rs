use std::ops::Range;
use super::{BEATS_PER_BAR, Curve, LONGEST_TICKS, TICKS_PER_BAR, TICKS_PER_BEAT, Track, VELOCITY, beats, bowed, ticks};
use crate::arp::Articulation;
use super::warp::{ENVELOPE_MAX, Loop};
use crate::theory::{self, Fit};

const PUSH: i32 = TICKS_PER_BEAT / 2;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SongNote {
    pub start: f64,
    pub end: f64,
    pub pitch: u8,
    pub velocity: u8,
    pub articulation: Articulation,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Note {
    pub pitch: u8,
    pub start: f32,
    pub length: f32,
    pub velocity: u8,
    pub articulation: Articulation,
}

impl Note {
    pub fn overlaps(&self, other: &Note) -> bool {
        self.pitch == other.pitch && self.start < other.start + other.length && other.start < self.start + self.length
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Sounding {
    pub pitches: Vec<u8>,
    pub travel: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ClipChord {
    pub start: f32,
    pub end: f32,
    pub pitches: Vec<u8>,
}

fn classes_of(pitches: &[u8]) -> Vec<u8> {
    let mut classes: Vec<u8> = pitches.iter().map(|pitch| pitch % 12).collect();
    classes.sort_unstable();
    classes.dedup();
    classes
}

#[derive(Clone, Copy)]
struct Strike {
    from: i32,
    to: i32,
    pitch: u8,
    velocity: u8,
}

impl Strike {
    fn sounds(&self, chord: &Range<i32>, bar: &Range<i32>) -> bool {
        chord.contains(&self.from) || self.from < chord.start && 2 * (self.to - bar.start) >= bar.end - bar.start
    }
}

fn struck_classes<'a>(strikes: impl Iterator<Item = &'a Strike>) -> Vec<u8> {
    classes_of(&strikes.map(|strike| strike.pitch).collect::<Vec<u8>>())
}

struct Held {
    span: Range<i32>,
    voices: Vec<(u8, u8)>,
}

impl Held {
    fn carries(&self, next: &Held) -> bool {
        self.span.end == next.span.start && self.voices.iter().map(|voice| voice.0).eq(next.voices.iter().map(|voice| voice.0))
    }
}

pub fn pitch_range(clips: &[Clip]) -> Option<(u8, u8)> {
    clips.iter().filter_map(Clip::pitch_range).reduce(|(lo, hi), (a, b)| (lo.min(a), hi.max(b)))
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Span {
    pub from: f64,
    pub to: f64,
    pub level: f64,
}

impl Span {
    pub const RAMP: f64 = 0.005;
    pub const LEAST: f64 = 2.0 * Span::RAMP;

    pub fn gain(self) -> f64 {
        sdk::dsp::db_to_gain(self.level)
    }

    pub fn holds(self, seconds: f64) -> bool {
        seconds >= self.from && seconds < self.to
    }

    fn shifted(self, by: f64) -> Option<Span> {
        let (from, to) = ((self.from + by).max(0.0), self.to + by);
        (to - from >= Span::LEAST).then_some(Span { from, to, ..self })
    }
}

pub fn level_among(spans: &[Span], seconds: f64) -> f64 {
    spans.iter().find(|span| span.holds(seconds)).map_or(1.0, |span| span.gain())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slope {
    Rise,
    Fall,
}

impl Slope {
    pub const BOTH: [Slope; 2] = [Slope::Rise, Slope::Fall];

    pub fn key(self) -> &'static str {
        match self {
            Slope::Rise => "fadeIn",
            Slope::Fall => "fadeOut",
        }
    }

    pub fn other(self) -> Slope {
        match self {
            Slope::Rise => Slope::Fall,
            Slope::Fall => Slope::Rise,
        }
    }

    pub fn levels(self) -> (f64, f64) {
        match self {
            Slope::Rise => (0.0, 1.0),
            Slope::Fall => (1.0, 0.0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Fade {
    pub from: f64,
    pub to: f64,
    pub bend: f32,
}

impl Fade {
    pub fn ends(self, slope: Slope) -> (f64, f64) {
        match slope {
            Slope::Rise => (self.from.min(self.to - Span::RAMP), self.to),
            Slope::Fall => (self.from, self.to.max(self.from + Span::RAMP)),
        }
    }

    pub fn shifted(self, by: f64) -> Fade {
        let from = (self.from + by).max(0.0);
        Fade { from, to: (self.to + by).max(from), ..self }
    }

    pub fn carried(self, slope: Slope, by_start: f64, by_end: f64, start: f64) -> Fade {
        match slope {
            Slope::Rise => self.shifted(by_start),
            Slope::Fall => self.shifted(by_end).no_sooner(self.shifted(by_start).from.min(start)),
        }
    }

    pub fn gain(self, slope: Slope, seconds: f64) -> f64 {
        let ((start, end), (before, after)) = (self.ends(slope), slope.levels());
        if seconds <= start {
            before
        } else if seconds >= end {
            after
        } else {
            bowed(before, after, self.bend, (seconds - start) / (end - start))
        }
    }

    fn no_later(self, than: f64) -> Fade {
        let to = self.to.min(than);
        Fade { from: self.from.min(to), to, ..self }
    }

    fn no_sooner(self, than: f64) -> Fade {
        let from = self.from.max(than);
        Fade { from, to: self.to.max(from), ..self }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Fades {
    pub rise: Option<Fade>,
    pub fall: Option<Fade>,
}

impl Fades {
    pub const NONE: Fades = Fades { rise: None, fall: None };

    pub fn of(self, slope: Slope) -> Option<Fade> {
        match slope {
            Slope::Rise => self.rise,
            Slope::Fall => self.fall,
        }
    }

    pub fn with(self, slope: Slope, fade: Option<Fade>) -> Fades {
        match slope {
            Slope::Rise => Fades { rise: fade.map(|rise| rise.no_later(self.fall.map_or(f64::INFINITY, |fall| fall.from))).filter(|rise| rise.to > 0.0), ..self },
            Slope::Fall => Fades { fall: fade.map(|fall| fall.no_sooner(self.rise.map_or(0.0, |rise| rise.to))), ..self },
        }
    }

    pub fn carried(self, by_start: f64, by_end: f64, start: f64) -> Fades {
        Fades::NONE.with(Slope::Rise, self.rise.map(|rise| rise.carried(Slope::Rise, by_start, by_end, start))).with(Slope::Fall, self.fall.map(|fall| fall.carried(Slope::Fall, by_start, by_end, start)))
    }

    pub fn gain(self, seconds: f64) -> f64 {
        self.rise.map_or(1.0, |rise| rise.gain(Slope::Rise, seconds)) * self.fall.map_or(1.0, |fall| fall.gain(Slope::Fall, seconds))
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct File {
    pub name: String,
    pub head: f64,
    pub offset: f64,
    pub tail: f64,
    pub pitch: f64,
    pub fine: f64,
    pub level: f64,
    pub spans: Vec<Span>,
    pub fades: Fades,
    pub tempo: f64,
    pub warp: bool,
    pub gap: Loop,
    pub envelope: f64,
}

impl File {
    pub const SEMITONES: f64 = 24.0;
    pub const CENTS: f64 = 100.0;
    pub const DECIBELS: f64 = 24.0;

    pub fn new(name: String) -> File {
        File { name, head: 0.0, offset: 0.0, tail: 0.0, pitch: 0.0, fine: 0.0, level: 0.0, spans: Vec::new(), fades: Fades::NONE, tempo: 0.0, warp: false, gap: Loop::Rest, envelope: f64::from(ENVELOPE_MAX) }
    }

    pub fn level_at(&self, seconds: f64) -> f64 {
        level_among(&self.spans, seconds) * self.fades.gain(seconds)
    }

    pub fn lay_span(&mut self, span: Span) -> usize {
        let mut kept = Vec::with_capacity(self.spans.len() + 2);
        for other in self.spans.drain(..) {
            if other.to <= span.from || other.from >= span.to {
                kept.push(other);
                continue;
            }
            if span.from - other.from >= Span::LEAST {
                kept.push(Span { to: span.from, ..other });
            }
            if other.to - span.to >= Span::LEAST {
                kept.push(Span { from: span.to, ..other });
            }
        }
        kept.push(span);
        kept.sort_by(|a, b| a.from.total_cmp(&b.from));
        self.spans = kept;
        self.spans.iter().position(|kept| *kept == span).unwrap_or(0)
    }

    pub fn stop(&self, frames: usize, rate: f64) -> f64 {
        (frames as f64 - self.tail * rate).max(0.0)
    }

    pub fn ratio(&self) -> f64 {
        ((self.pitch + self.fine / File::CENTS) / 12.0).exp2()
    }

    pub fn gain(&self) -> f64 {
        sdk::dsp::db_to_gain(self.level)
    }

    pub fn tempo_or(&self, bpm: f64) -> f64 {
        if self.tempo > 0.0 { self.tempo } else { bpm }
    }

    pub fn warp_ratio(&self, bpm: f64) -> f64 {
        self.tempo_or(bpm) / bpm
    }

    pub fn stretch(&self, bpm: f64) -> f64 {
        if self.warp { bpm / self.tempo_or(bpm) } else { self.ratio() }
    }

    pub fn entry(&self) -> f64 {
        self.head + self.offset
    }

    pub fn anchor(&self, start: f64, bpm: f64) -> f64 {
        start - self.entry() * bpm / (60.0 * self.stretch(bpm))
    }

    pub fn plays_whole(&self, length: i32, frames: usize, rate: f64, bpm: f64) -> bool {
        let played = beats(length) * rate * 60.0 / bpm * self.stretch(bpm);
        !self.warp && self.entry() == 0.0 && self.tail == 0.0 && self.pitch == 0.0 && self.fine == 0.0 && self.spans.is_empty() && self.fades == Fades::NONE && frames as f64 <= played + 1.0
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Clip {
    pub start: i32,
    pub length: i32,
    pub label: String,
    pub notes: Vec<Note>,
    pub file: Option<File>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Piece {
    pub clip: Clip,
    pub curves: Vec<Curve>,
    pub edges: Vec<Curve>,
}

impl Piece {
    pub fn shifted(self, by: i32) -> Piece {
        Piece { clip: Clip { start: self.clip.start + by, ..self.clip }, ..self }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Passage {
    pub clips: Vec<Clip>,
    pub curves: Vec<Curve>,
    pub edges: Vec<Curve>,
    pub length: i32,
}

impl Clip {
    pub fn chord(start: i32, label: String, pitches: &[u8]) -> Clip {
        let notes = pitches.iter().map(|&pitch| Note { pitch, start: 0.0, length: BEATS_PER_BAR as f32, velocity: VELOCITY, articulation: Articulation::PLAIN }).collect();
        Clip { start, length: TICKS_PER_BAR, label, notes, file: None }
    }

    pub fn audio(start: i32, length: i32, label: String, name: String) -> Clip {
        Clip { start, length, label, notes: Vec::new(), file: Some(File::new(name)) }
    }

    pub fn matched(&self, like: &Clip, heard_until: impl Fn(&Clip) -> Option<f64>) -> Option<File> {
        let (file, model) = (self.file.as_ref()?, like.file.as_ref()?);
        let tuned = Clip { file: Some(File { pitch: model.pitch, fine: model.fine, ..file.clone() }), ..self.clone() };
        let by_start = file.entry() - model.entry();
        let by_end = heard_until(&tuned).zip(heard_until(like)).map_or(by_start, |(theirs, ours)| theirs - ours);
        let spans = model.spans.iter().filter_map(|span| span.shifted(by_start)).collect();
        tuned.file.map(|tuned| File { fades: model.fades.carried(by_start, by_end, file.entry()), spans, ..tuned })
    }

    pub fn midi(start: i32, label: String, notes: Vec<Note>, until: f32) -> Clip {
        let room = (LONGEST_TICKS - start).max(0);
        let room_beats = beats(room) as f32;
        let notes: Vec<Note> = notes.into_iter().filter(|note| note.start < room_beats).map(|note| Note { length: note.length.min(room_beats - note.start), ..note }).collect();
        let end = notes.iter().map(|note| note.start + note.length).fold(0.0, f32::max);
        let bars = (end / BEATS_PER_BAR as f32).ceil().max(1.0) as i32;
        Clip { start, length: (bars * TICKS_PER_BAR).max(ticks(f64::from(until))).min(room), label, notes, file: None }
    }

    pub fn end(&self) -> i32 {
        self.start + self.length
    }

    pub fn between(&self, start: i32, end: i32, bpm: f64) -> Clip {
        let mut part = self.clone();
        if part.end() > end {
            part.resize(end - part.start);
        }
        if part.start < start {
            part.trim_head(start - part.start, bpm);
        }
        part.start -= start;
        part
    }

    pub fn beats(&self) -> f32 {
        beats(self.length) as f32
    }

    pub fn pitch_range(&self) -> Option<(u8, u8)> {
        let lo = self.notes.iter().map(|note| note.pitch).min()?;
        let hi = self.notes.iter().map(|note| note.pitch).max()?;
        Some((lo, hi))
    }

    fn bar(&self, line: i32) -> Range<i32> {
        line..(line + TICKS_PER_BAR).min(self.length)
    }

    fn strikes(&self) -> Vec<Strike> {
        let struck = |note: &Note| Strike { from: ticks(f64::from(note.start)), to: ticks(f64::from(note.start + note.length)).min(self.length), pitch: note.pitch, velocity: note.velocity };
        self.notes.iter().map(struck).filter(|strike| (0..self.length).contains(&strike.from)).collect()
    }

    fn pushed(&self, strikes: &[Strike], line: i32) -> i32 {
        let (bar, before) = (self.bar(line), self.bar(line - TICKS_PER_BAR));
        let next = struck_classes(strikes.iter().filter(|strike| strike.sounds(&bar, &bar)));
        let mut tail: Vec<i32> = strikes.iter().map(|strike| strike.from).filter(|from| (line - PUSH..line).contains(from)).collect();
        tail.sort_unstable_by(|a, b| b.cmp(a));
        tail.dedup();
        let mut edge = line;
        for at in tail {
            let early = struck_classes(strikes.iter().filter(|strike| strike.from == at));
            if !early.iter().all(|class| next.contains(class)) {
                break;
            }
            let earlier = struck_classes(strikes.iter().filter(|strike| strike.sounds(&(before.start..at), &before)));
            if !early.iter().all(|class| earlier.contains(class)) {
                edge = at;
            }
        }
        edge
    }

    pub fn bar_chords(&self) -> Vec<Note> {
        let strikes = self.strikes();
        let end = strikes.iter().map(|strike| strike.to).max().unwrap_or(0);
        let lines: Vec<i32> = (0..end).step_by(TICKS_PER_BAR as usize).collect();
        let edges: Vec<i32> = lines.iter().map(|&line| self.pushed(&strikes, line)).chain(lines.last().map(|&line| self.bar(line).end)).collect();
        let mut chords: Vec<Held> = Vec::new();
        for (index, &line) in lines.iter().enumerate() {
            let mut chord = Held { span: edges[index]..edges[index + 1], voices: Vec::new() };
            for strike in strikes.iter().filter(|strike| strike.sounds(&chord.span, &self.bar(line))) {
                match chord.voices.iter_mut().find(|(pitch, _)| *pitch == strike.pitch) {
                    Some((_, velocity)) => *velocity = (*velocity).max(strike.velocity),
                    None => chord.voices.push((strike.pitch, strike.velocity)),
                }
            }
            chord.voices.sort_unstable();
            match chords.last_mut() {
                Some(held) if held.carries(&chord) => {
                    held.span.end = chord.span.end;
                    for (voice, again) in held.voices.iter_mut().zip(&chord.voices) {
                        voice.1 = voice.1.max(again.1);
                    }
                }
                _ => chords.push(chord),
            }
        }
        let note = |held: &Held, &(pitch, velocity): &(u8, u8)| Note { pitch, start: beats(held.span.start) as f32, length: beats(held.span.end - held.span.start) as f32, velocity, articulation: Articulation::PLAIN };
        chords.iter().flat_map(|held| held.voices.iter().map(move |voice| note(held, voice))).collect()
    }

    pub fn chords(&self) -> Vec<ClipChord> {
        let length = self.beats();
        let mut edges: Vec<f32> = self.notes.iter().flat_map(|note| [note.start.clamp(0.0, length), (note.start + note.length).clamp(0.0, length)]).collect();
        edges.sort_by(f32::total_cmp);
        edges.dedup();
        let mut chords: Vec<ClipChord> = Vec::new();
        let mut open = false;
        for pair in edges.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            let mut pitches: Vec<u8> = self.notes.iter().filter(|note| note.start <= from && from < note.start + note.length).map(|note| note.pitch).collect();
            pitches.sort_unstable();
            pitches.dedup();
            if pitches.is_empty() {
                continue;
            }
            let classes = classes_of(&pitches);
            if classes.len() < 2 {
                open = false;
                continue;
            }
            match chords.last_mut() {
                Some(last) if open && classes_of(&last.pitches) == classes => {
                    last.end = to;
                    last.pitches.extend(pitches);
                    last.pitches.sort_unstable();
                    last.pitches.dedup();
                }
                _ => {
                    chords.push(ClipChord { start: from, end: to, pitches });
                    open = true;
                }
            }
        }
        chords
    }

    pub fn fits(&self) -> Vec<Fit> {
        let mut rising: Vec<&Note> = self.notes.iter().collect();
        rising.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
        let (mut heard, mut bass) = ([0; 12], [0; 12]);
        let mut last_start = None;
        for note in rising {
            let class = usize::from(note.pitch % 12);
            heard[class] += 1;
            if last_start != Some(note.start) {
                bass[class] += 1;
                last_start = Some(note.start);
            }
        }
        theory::fits(&heard, &bass)
    }

    pub fn resize(&mut self, length: i32) {
        let (old, new) = (self.beats(), beats(length) as f32);
        self.notes.retain_mut(|note| {
            let reaches_end = note.start + note.length >= old;
            note.length = if reaches_end { new - note.start } else { note.length.min(new - note.start) };
            note.length > 0.0
        });
        self.length = length;
    }

    pub fn trim_head(&mut self, cut: i32, bpm: f64) {
        self.start += cut;
        self.length -= cut;
        if let Some(file) = &mut self.file {
            let seconds = beats(cut) * 60.0 / bpm * file.stretch(bpm);
            file.head += seconds;
        }
        self.notes.retain_mut(|note| {
            note.start -= beats(cut) as f32;
            if note.start < 0.0 {
                note.length += note.start;
                note.start = 0.0;
            }
            note.length > 0.0
        });
    }

    pub fn place_note(&mut self, note: Note) {
        let (start, end) = (note.start, note.start + note.length);
        let mut kept = Vec::with_capacity(self.notes.len() + 2);
        for other in self.notes.drain(..) {
            if !other.overlaps(&note) {
                kept.push(other);
                continue;
            }
            let (other_start, other_end) = (other.start, other.start + other.length);
            if other_start < start {
                kept.push(Note { length: start - other_start, ..other });
            }
            if other_end > end {
                kept.push(Note { start: end, length: other_end - end, ..other });
            }
        }
        kept.push(note);
        kept.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
        self.notes = kept;
    }
}

impl Track {
    pub fn pitch_range(&self) -> Option<(u8, u8)> {
        pitch_range(&self.clips)
    }

    pub(super) fn placed(&self) -> impl Iterator<Item = SongNote> + '_ {
        let notes = self.clips.iter().flat_map(|clip| {
            clip.notes.iter().map(move |note| {
                let start = beats(clip.start) + f64::from(note.start);
                let end = (beats(clip.start) + f64::from(note.start + note.length)).min(beats(clip.end()));
                SongNote { start, end, pitch: note.pitch, velocity: note.velocity, articulation: note.articulation }
            })
        });
        notes.filter(|note| note.end > note.start)
    }

    pub fn song_notes(&self) -> Vec<SongNote> {
        let mut notes: Vec<SongNote> = self.placed().collect();
        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
        notes
    }

    pub fn sounding(&self, beat: f64) -> Option<Sounding> {
        let (mut pitches, mut from, mut to) = (Vec::new(), f64::NEG_INFINITY, f64::INFINITY);
        for note in self.placed() {
            if note.start <= beat && beat < note.end {
                pitches.push(note.pitch);
            }
            for edge in [note.start, note.end] {
                if edge <= beat {
                    from = from.max(edge);
                } else {
                    to = to.min(edge);
                }
            }
        }
        let chord = pitches.first().is_some_and(|first| pitches.iter().any(|pitch| pitch % 12 != first % 12));
        chord.then(|| Sounding { pitches, travel: ((beat - from) / (to - from)) as f32 })
    }

    pub fn piece(&self, clip: Clip, carry: bool) -> Piece {
        if !carry {
            return Piece { clip, curves: Vec::new(), edges: Vec::new() };
        }
        let (curves, edges) = self.automation.between(clip.start, self.reach(clip.end()));
        Piece { curves, edges, clip }
    }

    pub fn lift(&mut self, start: i32, carry: bool) -> Option<Piece> {
        let index = self.clips.iter().position(|clip| clip.start == start)?;
        let clip = self.clips.remove(index);
        let piece = self.piece(clip, carry);
        if carry {
            let reach = self.reach(piece.clip.end());
            self.automation.replace(start, reach, &[], &[]);
        }
        Some(piece)
    }

    pub fn lay(&mut self, piece: Piece, carry: bool, bpm: f64) {
        let Piece { clip, curves, edges } = piece;
        let (start, end) = (clip.start, clip.end());
        self.place(clip, bpm);
        if carry {
            let reach = self.reach(end);
            self.automation.replace(start, reach, &edges, &curves);
        }
    }

    pub fn passage(&self, start: i32, end: i32, carry: bool, bpm: f64) -> Passage {
        let clips = self.clips.iter().filter(|clip| clip.start < end && clip.end() > start).map(|clip| clip.between(start, end, bpm)).collect();
        let (curves, edges) = if carry { self.automation.between(start, end) } else { (Vec::new(), Vec::new()) };
        Passage { clips, curves, edges, length: end - start }
    }

    pub fn lay_passage(&mut self, passage: Passage, at: i32, carry: bool, bpm: f64) {
        let end = at + passage.length;
        self.take_between(at, end, bpm);
        for clip in passage.clips {
            self.place(Clip { start: clip.start + at, ..clip }, bpm);
        }
        if carry {
            self.automation.replace(at, end, &passage.edges, &passage.curves);
        }
    }

    pub fn clear_between(&mut self, start: i32, end: i32, carry: bool, bpm: f64) {
        self.take_between(start, end, bpm);
        if carry {
            self.automation.replace(start, end, &[], &[]);
        }
    }

    pub fn place(&mut self, clip: Clip, bpm: f64) {
        let (start, end) = (clip.start, clip.end());
        let mut kept = Vec::with_capacity(self.clips.len() + 2);
        for mut other in self.clips.drain(..) {
            if other.end() <= start || other.start >= end {
                kept.push(other);
                continue;
            }
            if other.start < start {
                let mut head = other.clone();
                head.resize(start - other.start);
                kept.push(head);
            }
            if other.end() > end {
                other.trim_head(end - other.start, bpm);
                kept.push(other);
            }
        }
        kept.push(clip);
        kept.sort_by_key(|clip| clip.start);
        self.clips = kept;
    }

    pub fn split(&mut self, start: i32, beat: i32, bpm: f64) -> bool {
        let Some(index) = self.clips.iter().position(|clip| clip.start == start && start < beat && beat < clip.end()) else {
            return false;
        };
        let mut tail = self.clips[index].clone();
        self.clips[index].resize(beat - start);
        tail.trim_head(beat - start, bpm);
        self.clips.insert(index + 1, tail);
        true
    }

    pub fn cut(&mut self, beat: i32, bpm: f64) -> bool {
        let under = self.clips.iter().find(|clip| clip.start < beat && beat < clip.end()).map(|clip| clip.start);
        under.is_some_and(|start| self.split(start, beat, bpm))
    }

    pub fn take_between(&mut self, start: i32, end: i32, bpm: f64) -> Vec<Clip> {
        self.cut(start, bpm);
        self.cut(end, bpm);
        let (within, kept): (Vec<Clip>, Vec<Clip>) = std::mem::take(&mut self.clips).into_iter().partition(|clip| start <= clip.start && clip.end() <= end);
        self.clips = kept;
        within
    }

    pub fn insert(&mut self, at: i32, by: i32, bpm: f64) {
        self.cut(at, bpm);
        for clip in &mut self.clips {
            if clip.start >= at {
                clip.start += by;
            }
        }
        self.automation.open(at, by);
    }
}

pub fn joined(parts: &[Clip], start: i32, end: i32, file: Option<File>) -> Clip {
    let label = parts.first().map(|clip| clip.label.clone()).unwrap_or_default();
    let notes = parts.iter().flat_map(|clip| {
        let shift = beats(clip.start - start) as f32;
        clip.notes.iter().map(move |note| Note { start: note.start + shift, ..*note })
    });
    Clip { start, length: end - start, label, notes: notes.collect(), file }
}

pub fn stacked(parts: &[Clip], start: i32, end: i32) -> Clip {
    let label = parts.first().map(|clip| clip.label.clone()).unwrap_or_default();
    let mut laid: Vec<(i32, i32, Note)> = Vec::new();
    for clip in parts {
        let (shift, room) = (clip.start - start, clip.beats());
        for note in clip.notes.iter().filter(|note| note.start < room) {
            let from = shift + ticks(f64::from(note.start));
            let to = shift + ticks(f64::from(note.start + note.length)).min(clip.length);
            laid.push((from, to, Note { start: note.start + beats(shift) as f32, length: note.length.min(room - note.start), ..*note }));
        }
    }
    laid.sort_by_key(|&(from, to, _)| from - to);
    let mut kept: Vec<(i32, i32, Note)> = Vec::with_capacity(laid.len());
    for (from, to, note) in laid {
        if !kept.iter().any(|&(longer_from, longer_to, longer)| longer.pitch == note.pitch && from < longer_to && longer_from < to) {
            kept.push((from, to, note));
        }
    }
    let mut notes: Vec<Note> = kept.into_iter().map(|(_, _, note)| note).collect();
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    Clip { start, length: end - start, label, notes, file: None }
}

const CONTINUES_EPSILON: f64 = 1e-9;

pub fn continues(prev: &Clip, next: &Clip, bpm: f64) -> bool {
    let (Some(before), Some(after)) = (&prev.file, &next.file) else {
        return false;
    };
    let played = beats(prev.length) * 60.0 / bpm * before.stretch(bpm);
    let warped_alike = before.warp == after.warp && before.tempo == after.tempo && before.gap == after.gap && before.envelope == after.envelope;
    next.start == prev.end() && before.name == after.name && before.pitch == after.pitch && before.fine == after.fine && before.level == after.level && before.tail == after.tail && before.spans == after.spans && before.fades == after.fades && warped_alike && (after.entry() - before.entry() - played).abs() < CONTINUES_EPSILON
}

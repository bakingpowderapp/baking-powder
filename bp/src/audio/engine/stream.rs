use super::*;

const EVENTS: usize = 256;
const KEYS: usize = u128::BITS as usize;
const ONSETS: usize = 32;

fn note(midi: u8) -> u128 {
    1u128.checked_shl(u32::from(midi)).unwrap_or(0)
}

#[derive(Clone, Copy)]
enum Event {
    On { midi: u8, velocity: u8 },
    Off { midi: u8 },
    Strike { midi: u8, velocity: u8, articulation: Articulation },
    Legato { from: u8, to: u8, velocity: u8, articulation: Articulation },
}

impl Event {
    fn play(self, sink: &mut impl Sink, deaf: bool) {
        match self {
            Event::On { .. } | Event::Strike { .. } if deaf => {}
            Event::Legato { from, .. } if deaf => sink.note_off(from),
            Event::On { midi, velocity } => sink.note_on(midi, velocity),
            Event::Off { midi } => sink.note_off(midi),
            Event::Strike { midi, velocity, articulation } => sink.strike(midi, velocity, articulation),
            Event::Legato { from, to, velocity, articulation } => sink.legato(from, to, velocity, articulation),
        }
    }

    fn cue(self, runner: &mut Runner, at: i64, sink: &mut impl Sink, struck: Option<&MidiRow>) {
        let scaled = |velocity: u8| struck.map_or(velocity, |row| row.struck(velocity));
        match self {
            Event::On { midi, velocity } => runner.cue_on(midi, scaled(velocity), Articulation::PLAIN, at, sink),
            Event::Off { midi } => runner.cue_off(midi, at, sink),
            Event::Strike { midi, velocity, articulation } => runner.cue_on(midi, scaled(velocity), articulation, at, sink),
            Event::Legato { from, to, velocity, articulation } => {
                runner.cue_off(from, at, sink);
                runner.cue_on(to, scaled(velocity), articulation, at, sink);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Onset {
    frame: i64,
    phrase: bool,
    drift: f64,
}

pub(crate) struct Onsets {
    log: [Onset; ONSETS],
    len: usize,
    heard: u128,
}

impl Onsets {
    const fn new() -> Onsets {
        Onsets { log: [Onset { frame: 0, phrase: false, drift: 0.0 }; ONSETS], len: 0, heard: 0 }
    }

    pub(in crate::audio) fn forget(&mut self) {
        self.len = 0;
    }

    pub(in crate::audio) fn retime(&mut self, now: i64, gained: f64) {
        for onset in self.log[..self.len].iter_mut().filter(|onset| onset.frame < now) {
            onset.drift += (now - onset.frame) as f64 * gained;
        }
    }

    pub(in crate::audio) fn drift(&self, frame: i64) -> f64 {
        self.log[..self.len].iter().find(|onset| onset.frame == frame).map_or(0.0, |onset| onset.drift)
    }

    fn note(&mut self, frame: i64, phrase: bool) {
        let at = self.log[..self.len].partition_point(|onset| onset.frame <= frame);
        if at > 0 && self.log[at - 1].frame == frame {
            self.log[at - 1].phrase |= phrase;
            return;
        }
        if self.len == ONSETS {
            if at == 0 {
                return;
            }
            self.log.copy_within(1..ONSETS, 0);
            self.len -= 1;
            self.log.copy_within(at - 1..self.len, at);
            self.log[at - 1] = Onset { frame, phrase, drift: 0.0 };
        } else {
            self.log.copy_within(at..self.len, at + 1);
            self.log[at] = Onset { frame, phrase, drift: 0.0 };
        }
        self.len += 1;
    }

    fn hear(&mut self, events: &[(i64, Event)]) {
        for &(at, event) in events {
            match event {
                Event::On { midi, .. } | Event::Strike { midi, .. } => {
                    self.note(at, self.heard == 0);
                    self.heard |= note(midi);
                }
                Event::Off { midi } => self.heard &= !note(midi),
                Event::Legato { from, to, .. } => self.heard = (self.heard & !note(from)) | note(to),
            }
        }
    }

    fn kept(&self, phrase: bool) -> impl DoubleEndedIterator<Item = i64> + '_ {
        self.log[..self.len].iter().filter(move |onset| onset.phrase || !phrase).map(|onset| onset.frame)
    }

    pub(in crate::audio) fn latest(&self, until: i64, phrase: bool) -> Option<i64> {
        self.kept(phrase).rev().find(|frame| *frame <= until)
    }

    pub(in crate::audio) fn within(&self, from: i64, to: i64, phrase: bool) -> impl Iterator<Item = i64> + '_ {
        self.kept(phrase).skip_while(move |frame| *frame < from).take_while(move |frame| *frame < to)
    }
}

pub(crate) struct Printer {
    sounding: Vec<(u8, f64, u8, Articulation)>,
    notes: Vec<SongNote>,
}

impl Printer {
    pub(in crate::audio) fn new() -> Printer {
        Printer { sounding: Vec::new(), notes: Vec::new() }
    }

    fn end(&mut self, midi: u8, beat: f64) {
        if let Some(at) = self.sounding.iter().position(|sounding| sounding.0 == midi) {
            let (pitch, start, velocity, articulation) = self.sounding.remove(at);
            if beat > start {
                self.notes.push(SongNote { start, end: beat, pitch, velocity, articulation });
            }
        }
    }

    fn end_all(&mut self, midi: u8, beat: f64) {
        while self.sounding.iter().any(|sounding| sounding.0 == midi) {
            self.end(midi, beat);
        }
    }

    fn start(&mut self, midi: u8, beat: f64, velocity: u8, articulation: Articulation) {
        self.end_all(midi, beat);
        self.sounding.push((midi, beat, velocity, articulation));
    }

    pub(in crate::audio) fn hear(&mut self, stream: &Stream, line: Segment) {
        for &(at, event) in &stream.events {
            let beat = line.beat_at(at);
            match event {
                Event::On { midi, velocity } => self.start(midi, beat, velocity, Articulation::PLAIN),
                Event::Strike { midi, velocity, articulation } => self.start(midi, beat, velocity, articulation),
                Event::Off { midi } => self.end_all(midi, beat),
                Event::Legato { from, to, velocity, articulation } => {
                    self.end(from, beat);
                    self.sounding.push((to, beat, velocity, articulation));
                }
            }
        }
    }

    pub(in crate::audio) fn printed(mut self, beat: f64) -> Vec<SongNote> {
        while let Some(&(midi, ..)) = self.sounding.last() {
            self.end(midi, beat);
        }
        self.notes.sort_by(|a, b| a.start.total_cmp(&b.start));
        self.notes
    }
}

pub(crate) struct Stream {
    pub(super) now: i64,
    clocked: usize,
    events: Vec<(i64, Event)>,
    sounding: u128,
    opened: usize,
    primed: usize,
    scanned: usize,
    onsets: Onsets,
}

impl Stream {
    pub(crate) fn new(max_frames: usize) -> Stream {
        Stream { now: 0, clocked: 0, events: Vec::with_capacity(max_frames.max(EVENTS) + KEYS), sounding: 0, opened: 0, primed: 0, scanned: 0, onsets: Onsets::new() }
    }

    pub(in crate::audio) fn onsets(&self) -> &Onsets {
        &self.onsets
    }

    pub(in crate::audio) fn onsets_mut(&mut self) -> &mut Onsets {
        &mut self.onsets
    }

    fn push(&mut self, event: Event) {
        let spare = self.events.capacity() - self.events.len();
        let event = match event {
            _ if spare > KEYS => event,
            Event::Off { midi } | Event::Legato { from: midi, .. } if self.sounding & note(midi) != 0 => Event::Off { midi },
            _ => return,
        };
        match event {
            Event::On { midi, .. } | Event::Strike { midi, .. } => self.sounding |= note(midi),
            Event::Off { midi } => self.sounding &= !note(midi),
            Event::Legato { from, to, .. } => self.sounding = (self.sounding & !note(from)) | note(to),
        }
        self.events.push((self.now, event));
    }

    pub(in crate::audio) fn sounding(&self) -> u128 {
        self.sounding
    }

    pub(in crate::audio) fn join(&mut self, sounding: u128) {
        self.sounding = sounding;
        self.onsets.heard = sounding;
    }

    pub(super) fn copy_from(&mut self, other: &Stream) {
        self.events.clear();
        self.events.extend_from_slice(&other.events[..other.events.len().min(self.events.capacity())]);
        self.clocked = other.clocked;
    }

    pub(super) fn clear(&mut self) {
        self.events.clear();
        self.clocked = 0;
        self.opened = 0;
        self.primed = 0;
        self.scanned = 0;
    }

    pub(super) fn lapse(&mut self) {
        for midi in 0..KEYS as u8 {
            if self.sounding & note(midi) != 0 {
                self.push(Event::Off { midi });
            }
        }
        self.opened = self.events.len();
        self.primed = self.opened;
    }

    pub(super) fn fold(&mut self) {
        let opened = self.opened.min(self.events.len());
        let mut latest = [usize::MAX; KEYS];
        for (index, (_, event)) in self.events.iter().enumerate().skip(opened) {
            let (gone, struck) = match *event {
                Event::On { midi, .. } | Event::Strike { midi, .. } => (None, Some(midi)),
                Event::Off { midi } => (Some(midi), None),
                Event::Legato { from, to, .. } => (Some(from), Some(to)),
            };
            for (midi, at) in [(gone, usize::MAX), (struck, index)] {
                if let Some(slot) = midi.and_then(|midi| latest.get_mut(usize::from(midi))) {
                    *slot = at;
                }
            }
        }
        let mut kept = opened;
        for index in opened..self.events.len() {
            let (at, event) = self.events[index];
            let held = match event {
                Event::On { midi, .. } => Some((midi, event)),
                Event::Strike { midi, velocity, articulation } => Some((midi, Event::Strike { midi, velocity, articulation: Articulation { join: Join::Play, ..articulation } })),
                Event::Legato { to, velocity, articulation, .. } => Some((to, Event::Strike { midi: to, velocity, articulation: Articulation { join: Join::Play, ..articulation } })),
                Event::Off { .. } => None,
            };
            if let Some((_, event)) = held.filter(|(midi, _)| latest.get(usize::from(*midi)) == Some(&index)) {
                self.events[kept] = (at, event);
                kept += 1;
            }
        }
        self.events.truncate(kept);
        self.primed = kept;
        self.scanned = kept;
    }

    pub(super) fn render(&mut self, runner: &mut Runner, fed: Option<&Stream>, struck: Option<&MidiRow>, d: &Dispatch) {
        self.now = d.now;
        let fed = fed.map_or(&[][..], |fed| &fed.events[if struck.is_some() { 0 } else { fed.primed.min(fed.events.len()) }..]);
        let mut cued = self.cue(runner, fed, 0, d.now, false, struck);
        runner.settle(self, d.now);
        self.clocked = self.events.len();
        let mut done = 0;
        loop {
            let until = fed.get(cued).map_or(d.frames, |(at, _)| ((at - d.now).max(0) as usize).clamp(done, d.frames));
            if until > done {
                let transport = d.transport.map(|transport| Transport { beat: transport.beat + done as f64 / d.frames_per_beat });
                runner.render(self, d.now + done as i64, until - done, d.frames_per_beat, d.clock.sample_rate, transport, |stream, _, to| stream.now = d.now + (done + to) as i64);
                done = until;
            }
            if cued >= fed.len() {
                break;
            }
            self.now = d.now + done as i64;
            cued = self.cue(runner, fed, cued, self.now, done >= d.frames, struck);
        }
        self.spare_released(d.now);
        let scanned = self.scanned.min(self.events.len());
        self.onsets.hear(&self.events[scanned..]);
        self.scanned = self.events.len();
    }

    fn spare_released(&mut self, now: i64) {
        let (opened, primed) = (self.opened, self.primed.min(self.events.len()));
        if primed <= opened {
            return;
        }
        let (mut touched, mut released) = (0u128, 0u128);
        for &(_, event) in self.events[primed..].iter().take_while(|(at, _)| *at == now) {
            let (midi, lets_go) = match event {
                Event::Off { midi } => (midi, true),
                Event::On { midi, .. } | Event::Strike { midi, .. } | Event::Legato { from: midi, .. } => (midi, false),
            };
            if touched & note(midi) == 0 && lets_go {
                released |= note(midi);
            }
            touched |= note(midi);
        }
        if released == 0 {
            return;
        }
        let (mut kept, mut unstruck) = (opened, 0u128);
        let (clocked, scanned) = (self.clocked, self.scanned);
        for index in opened..self.events.len() {
            let (at, event) = self.events[index];
            let spared = match event {
                Event::On { midi, .. } | Event::Strike { midi, .. } if index < primed && released & note(midi) != 0 => {
                    unstruck |= note(midi);
                    true
                }
                Event::Off { midi } if index >= primed && at == now && unstruck & note(midi) != 0 => {
                    unstruck &= !note(midi);
                    self.onsets.heard &= !note(midi);
                    true
                }
                _ => false,
            };
            if spared {
                self.primed -= usize::from(index < primed);
                self.clocked -= usize::from(index < clocked);
                self.scanned -= usize::from(index < scanned);
            } else {
                self.events[kept] = self.events[index];
                kept += 1;
            }
        }
        self.events.truncate(kept);
    }

    fn cue(&mut self, runner: &mut Runner, fed: &[(i64, Event)], mut cued: usize, now: i64, all: bool, struck: Option<&MidiRow>) -> usize {
        while let Some(&(at, event)) = fed.get(cued) {
            if !all && runner.room() == 0 && at > now {
                break;
            }
            event.cue(runner, at, self, struck);
            cued += 1;
        }
        cued
    }

    pub(super) fn play(&self, core: &mut Core, lane: u64, deaf: bool, l: &mut [f32], r: &mut [f32], d: &Dispatch) {
        let n = l.len();
        let (before, after) = self.events.split_at(self.clocked.min(self.events.len()));
        core.set_lane(lane);
        core.clock(d.frames_per_beat, d.clock.beat.map(|beat| (beat.at(0), (beat.anchor * beat.frames_per_beat).round() as i64 + beat.since)));
        for &(_, event) in before {
            event.play(core, deaf);
        }
        let mut done = 0;
        for &(at, event) in after {
            let to = ((at - d.now).max(0) as usize).clamp(done, n);
            if to > done {
                core.render(&mut l[done..to], &mut r[done..to]);
                done = to;
            }
            event.play(core, deaf);
        }
        if done < n {
            core.render(&mut l[done..n], &mut r[done..n]);
        }
    }
}

impl Sink for Stream {
    fn at(&mut self, at: i64) {
        self.now = at;
    }

    fn note_on(&mut self, midi: u8, velocity: u8) {
        self.push(Event::On { midi, velocity });
    }

    fn note_off(&mut self, midi: u8) {
        self.push(Event::Off { midi });
    }

    fn strike(&mut self, midi: u8, velocity: u8, articulation: Articulation) {
        self.push(Event::Strike { midi, velocity, articulation });
    }

    fn legato(&mut self, from: u8, to: u8, velocity: u8, articulation: Articulation) {
        self.push(Event::Legato { from, to, velocity, articulation });
    }
}

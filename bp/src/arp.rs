use sdk::division::{self, Division};
use sdk::notes::{CENTS_REACH, OCTAVE_REACH, SEMITONE_REACH, SEMITONES_PER_OCTAVE};
pub use sdk::notes::{Articulation, Join, Sink};
use sdk::schema::{Fields, bounded, choice, either, flag, items, name, number, one_of, params};
use sdk::json::{self, Json};

mod row;
mod snap;

pub use row::MidiRow;
pub use snap::{Snap, Ties};
use snap::Snapper;

const HELD: usize = 32;
const POOL: usize = 128;
const PENDING: usize = POOL;
const MIN_GATE_FRAMES: i64 = 48;
const LOW_NOTE: u8 = 36;
const STEP_SALT: u32 = 0x9e37_79b9;
const LINE_EPSILON: f64 = 1e-9;
const CUES: usize = 128;
pub const LANE_STEPS: usize = 64;
pub const SHORT_STEPS: usize = 16;
pub const RUN: i8 = 0;
const CHORD_NOTES: i8 = 16;
const FULL: i8 = 100;
const HALF: i8 = 50;
const TOP_VELOCITY: u8 = 127;
pub const STACK: usize = 3;
const HALF_STEP: f64 = 0.5;
const DRAWS_PER_BEAT: f64 = 960.0;
const UNENTERED: u8 = u8::MAX;
const SECONDS_PER_MINUTE: f64 = 60.0;
const STRUCK: usize = HELD * Arp::MAX_STEPS;

const CENTS_PER_SEMITONE: f64 = 100.0;
const LENGTH: &str = "length";
const STEPS: &str = "steps";
const STACKED: &str = "stackSemitones";

choice!(pub Style {
    Up = "Up" / "up",
    Down = "Down" / "down",
    UpDown = "UpDown" / "up-down",
    DownUp = "DownUp" / "down-up",
    UpAndDown = "Up & Down" / "up-and-down",
    DownAndUp = "Down & Up" / "down-and-up",
    Converge = "Converge" / "converge",
    Diverge = "Diverge" / "diverge",
    ConDiverge = "Con & Diverge" / "converge-diverge",
    PinkyUp = "Pinky Up" / "pinky-up",
    PinkyUpDown = "Pinky UpDown" / "pinky-up-down",
    ThumbUp = "Thumb Up" / "thumb-up",
    ThumbUpDown = "Thumb UpDown" / "thumb-up-down",
    PlayOrder = "Play Order" / "play-order",
    ChordTrigger = "Chord Trigger" / "chord-trigger",
    Random = "Random" / "random",
    RandomOther = "Random Other" / "random-other",
    RandomOnce = "Random Once" / "random-once",
});

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lane {
    Note,
    Octave,
    Semitone,
    Cents,
    Velocity,
    Gate,
    Mod,
}

impl Lane {
    pub const ALL: [Lane; 7] = [Lane::Note, Lane::Octave, Lane::Semitone, Lane::Cents, Lane::Velocity, Lane::Gate, Lane::Mod];
    pub const LABELS: [&str; 7] = ["NOTE", "OCT", "NOTE", "CENT", "VEL", "GATE", "MOD"];

    pub fn index(self) -> usize {
        self as usize
    }
}

impl Lane {
    pub fn range(self) -> (i8, i8) {
        match self {
            Lane::Note => (RUN, CHORD_NOTES),
            Lane::Octave => (-OCTAVE_REACH, OCTAVE_REACH),
            Lane::Semitone => (-SEMITONE_REACH, SEMITONE_REACH),
            Lane::Cents => (-CENTS_REACH, CENTS_REACH),
            Lane::Velocity | Lane::Gate => (1, FULL),
            Lane::Mod => (-FULL, FULL),
        }
    }

    pub const fn initial(self) -> i8 {
        match self {
            Lane::Note => RUN,
            Lane::Octave | Lane::Semitone | Lane::Cents | Lane::Mod => 0,
            Lane::Velocity => FULL,
            Lane::Gate => HALF,
        }
    }

    fn scale(self) -> f64 {
        match self {
            Lane::Velocity | Lane::Gate | Lane::Mod => f64::from(FULL),
            Lane::Note | Lane::Octave | Lane::Semitone | Lane::Cents => 1.0,
        }
    }

    pub fn spell(self, value: i8) -> String {
        match self {
            Lane::Note if value == RUN => String::from("RUN"),
            Lane::Note => format!("NOTE {value}"),
            Lane::Octave if value == 0 => String::from("0 OCT"),
            Lane::Octave => format!("{value:+} OCT"),
            Lane::Semitone if value == 0 => String::from("KEY"),
            Lane::Semitone => format!("{value:+} ST"),
            Lane::Cents if value == 0 => String::from("0 ¢"),
            Lane::Cents => format!("{value:+} ¢"),
            Lane::Velocity | Lane::Gate => format!("{value}%"),
            Lane::Mod if value == 0 => String::from("0%"),
            Lane::Mod => format!("{value:+}%"),
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Lane::Note => "Which chord note each step plays, counted up from the lowest and stopping at the highest; RUN follows STYLE. SHIFT-press clears the lane",
            Lane::Octave => "Octaves each step is moved up or down. SHIFT-press clears the lane",
            Lane::Semitone => "Semitones each step plays above or below the key you hold, OCT adding octaves. SHIFT-press clears the lane",
            Lane::Cents => "Cents each step plays above or below its note, up to half a semitone either way; a fractional stack offset adds its own. SHIFT-press clears the lane",
            Lane::Velocity => "How hard each step plays, as a share of the key's velocity, or of full velocity once the label reads FIX. SHIFT-press clears the lane",
            Lane::Gate => "How much of its step each note sounds before it lets go; a tie holds it into the next. SHIFT-press clears the lane",
            Lane::Mod => "A value a step's note keeps; the Core's MOD boxes aim it at levels, tunes, cutoff, pan and pulse width. SHIFT-press clears the lane",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Step {
    pub note: i8,
    pub octave: i8,
    pub rest: bool,
    pub semitone: i8,
    pub cents: i8,
    pub velocity: i8,
    pub gate: i8,
    pub modulation: i8,
    pub join: Join,
}

const TIE: &str = "tie";
const SLIDE: &str = "slide";
const REST: &str = "rest";

const SAVED: [(Lane, &str, &str); 7] = [
    (Lane::Note, "fromLowest", "with stepMode false, which held note the step plays, counted up from the lowest and stopping at the highest; 0 runs on in the style's order"),
    (Lane::Semitone, "semitones", "with stepMode true, semitones the step plays above or below the key you hold"),
    (Lane::Cents, "cents", "with stepMode true, cents the step plays above or below its note, a fractional stackSemitones adding its own"),
    (Lane::Octave, "octaveShift", "octaves the step is moved up or down"),
    (Lane::Velocity, "velocity", "with stepMode true, how hard the step plays, as a share of the key's velocity, or of full velocity under fixedVelocity; with stepMode false each held note plays as it was struck"),
    (Lane::Gate, "gate", "with stepMode true, how much of its step the note sounds before it lets go; with stepMode false the arp's gate is used"),
    (Lane::Mod, "mod", "a value the step hands its voice for life, which a Core's mod keys turn into oscillator levels, pitch offsets, filter cutoff, pan and pulse width"),
];

const MARKS: [(&str, &str); 3] = [(TIE, "with stepMode true, holds the note into the next step"), (SLIDE, "with stepMode true, glides into the next step's note"), (REST, "the step is silent")];

impl Step {
    fn lane(self, lane: Lane) -> i8 {
        match lane {
            Lane::Note => self.note,
            Lane::Octave => self.octave,
            Lane::Semitone => self.semitone,
            Lane::Cents => self.cents,
            Lane::Velocity => self.velocity,
            Lane::Gate => self.gate,
            Lane::Mod => self.modulation,
        }
    }

    fn to_json(self) -> Json {
        let mut fields = Fields::new();
        for (lane, key, _) in SAVED {
            fields.number(key, f64::from(self.lane(lane)) / lane.scale(), f64::from(lane.initial()) / lane.scale());
        }
        fields.flag(TIE, self.join == Join::Tie, false);
        fields.flag(SLIDE, self.join == Join::Slide, false);
        fields.flag(REST, self.rest, false);
        fields.into_json()
    }

    fn manual() -> Json {
        let mut fields = Fields::new();
        for (lane, key, about) in SAVED {
            let ((lo, hi), scale) = (lane.range(), lane.scale());
            fields.manual(key, bounded(f64::from(lane.initial()) / scale, f64::from(lo) / scale..=f64::from(hi) / scale, scale == 1.0), about);
        }
        for (key, about) in MARKS {
            fields.manual(key, either(false), about);
        }
        fields.into_json()
    }

    fn from_json(json: &Json) -> Step {
        let json = Some(json);
        let saved = |wanted: Lane| {
            let (lo, hi) = wanted.range();
            let key = SAVED.iter().find(|(lane, ..)| *lane == wanted).map_or("", |(_, key, _)| *key);
            (number(json, key, f64::from(wanted.initial()) / wanted.scale()) * wanted.scale()).round().clamp(f64::from(lo), f64::from(hi)) as i8
        };
        let join = match (flag(json, SLIDE, false), flag(json, TIE, false)) {
            (true, _) => Join::Slide,
            (false, true) => Join::Tie,
            (false, false) => Join::Play,
        };
        Step { note: saved(Lane::Note), octave: saved(Lane::Octave), rest: flag(json, REST, false), semitone: saved(Lane::Semitone), cents: saved(Lane::Cents), velocity: saved(Lane::Velocity), gate: saved(Lane::Gate), modulation: saved(Lane::Mod), join }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pattern {
    steps: [[i8; LANE_STEPS]; Lane::ALL.len()],
    rests: [bool; LANE_STEPS],
    joins: [Join; LANE_STEPS],
    length: u8,
}

impl Pattern {
    pub const DEFAULT: Pattern = Pattern {
        steps: [[Lane::Note.initial(); LANE_STEPS], [Lane::Octave.initial(); LANE_STEPS], [Lane::Semitone.initial(); LANE_STEPS], [Lane::Cents.initial(); LANE_STEPS], [Lane::Velocity.initial(); LANE_STEPS], [Lane::Gate.initial(); LANE_STEPS], [Lane::Mod.initial(); LANE_STEPS]],
        rests: [false; LANE_STEPS],
        joins: [Join::Play; LANE_STEPS],
        length: SHORT_STEPS as u8,
    };

    pub fn length(&self) -> usize {
        usize::from(self.length)
    }

    pub fn set_length(&mut self, length: usize) {
        self.length = length.clamp(1, LANE_STEPS) as u8;
    }

    pub fn get(&self, lane: Lane, index: usize) -> i8 {
        self.steps[lane.index()][index.min(LANE_STEPS - 1)]
    }

    pub fn set(&mut self, lane: Lane, index: usize, value: i8) {
        let (lo, hi) = lane.range();
        self.steps[lane.index()][index.min(LANE_STEPS - 1)] = value.clamp(lo, hi);
    }

    pub fn clear(&mut self, lane: Lane) {
        self.steps[lane.index()] = Pattern::DEFAULT.steps[lane.index()];
    }

    pub fn rested(&self, index: usize) -> bool {
        self.rests[index.min(LANE_STEPS - 1)]
    }

    pub fn set_rest(&mut self, index: usize, rested: bool) {
        self.rests[index.min(LANE_STEPS - 1)] = rested;
    }

    pub fn clear_rests(&mut self) {
        self.rests = [false; LANE_STEPS];
    }

    pub fn join(&self, index: usize) -> Join {
        self.joins[index.min(LANE_STEPS - 1)]
    }

    pub fn set_join(&mut self, index: usize, join: Join) {
        self.joins[index.min(LANE_STEPS - 1)] = join;
    }

    pub fn clear_joins(&mut self) {
        self.joins = [Join::Play; LANE_STEPS];
    }

    pub fn joins(&self) -> usize {
        self.joins[..self.length()].iter().filter(|join| **join != Join::Play).count()
    }

    pub fn step(&self, index: usize) -> Step {
        Step {
            note: self.get(Lane::Note, index),
            octave: self.get(Lane::Octave, index),
            rest: self.rested(index),
            semitone: self.get(Lane::Semitone, index),
            cents: self.get(Lane::Cents, index),
            velocity: self.get(Lane::Velocity, index),
            gate: self.get(Lane::Gate, index),
            modulation: self.get(Lane::Mod, index),
            join: self.join(index),
        }
    }

    pub fn put(&mut self, index: usize, step: Step) {
        for lane in Lane::ALL {
            self.set(lane, index, step.lane(lane));
        }
        self.set_rest(index, step.rest);
        self.set_join(index, step.join);
    }

    pub fn rests(&self) -> usize {
        self.rests[..self.length()].iter().filter(|rested| **rested).count()
    }

    pub fn slot(&self, step: i64) -> usize {
        step.rem_euclid(self.length() as i64) as usize
    }

    fn at(&self, lane: Lane, step: i64) -> i8 {
        self.steps[lane.index()][self.slot(step)]
    }

    fn rested_at(&self, step: i64) -> bool {
        self.rests[self.slot(step)]
    }

    fn join_at(&self, step: i64) -> Join {
        self.joins[self.slot(step)]
    }

    fn continues(&self, step: i64) -> bool {
        !self.rested_at(step) && self.join_at(step) != Join::Play
    }

    fn chain(&self) -> usize {
        let len = self.length();
        (0..len).map(|from| 1 + (1..len).take_while(|ahead| self.continues((from + ahead) as i64)).count()).max().unwrap_or(1)
    }

    fn runs_before(&self, step: i64) -> i64 {
        let len = self.length() as i64;
        let runs = |until: usize| (0..until).filter(|index| self.steps[Lane::Note.index()][*index] == RUN && !self.rests[*index]).count() as i64;
        step.div_euclid(len) * runs(self.length()) + runs(step.rem_euclid(len) as usize)
    }

    fn fields(&self, default: &Pattern, fields: &mut Fields) {
        if self == default {
            return;
        }
        let touched = (0..LANE_STEPS).rev().find(|index| self.step(*index) != default.step(*index)).map_or(0, |last| last + 1);
        fields.push(LENGTH, Json::Number(f64::from(self.length)));
        fields.push(STEPS, Json::Array((0..touched.max(self.length())).map(|index| self.step(index).to_json()).collect()));
    }

    fn manual(default: &Pattern, _: &str, _: &dyn Fn(&str) -> String, fields: &mut Fields) {
        fields.manual(LENGTH, bounded(f64::from(default.length), 1.0..=LANE_STEPS as f64, true), "steps before the pattern repeats; the list may run past it");
        fields.push(STEPS, Json::Array(vec![Json::from("one object per step from the first, in order; a key left out is the lane's default"), Step::manual()]));
    }

    fn from_json(json: Option<&Json>, default: Pattern) -> Pattern {
        let mut pattern = default;
        let steps = items(json, STEPS);
        for (index, item) in steps.iter().take(LANE_STEPS).enumerate() {
            pattern.put(index, Step::from_json(item));
        }
        let written = if steps.is_empty() { default.length() } else { steps.len().min(LANE_STEPS) };
        pattern.set_length(number(json, LENGTH, written as f64).round() as usize);
        pattern
    }
}

pub const RATE_LABELS: &[&str] = division::LABELS.split_at(division::LABELS.len() - RATES.len()).1;

pub const RATES: &[Division] = Division::ALL.split_at(Division::HalfDotted as usize).1;

params!(Arp (fields, read, document) {
    switch on = false, "on" => "the arp is on; off, the notes you hold sound as played";
    flag stepped = false, "stepMode" => "true: the pattern plays a line of its own from the one key you hold, each step sounding at that key plus its semitones, octaveShift and cents, at its own velocity and gate; false: the pattern plays the notes you hold in the style's order, a step picking fromLowest, or running on when it names none";
    flag step_chord = false, "stepChord" => "with stepMode true, every key you hold plays the line together, each step sounding all of them moved by its semitones, octaveShift and cents at its velocity and gate, a tie or slide carrying them all; false plays the line from the last key alone";
    flag finish_gate = false, "finishGate" => "with stepMode true, lifting the last key leaves the notes sounding their step's gate, a tie or slide carried to its end, and fires no further step; false stops them at once";
    flag fixed_velocity = false, "fixedVelocity" => "with stepMode true, a step's velocity is a share of full velocity, not of the key's";
    flag retrigger = true, "retrigger" => "the pattern restarts on a fresh chord; false runs on, locked to the song's grid";
    flag retrigger_each_note = false, "retriggerEachNote" => "with retrigger true, the pattern restarts on every key press, not only on a fresh chord";
    flag lock = false, "presetLock" => "keeps this arp and its pattern when a preset lands on the track";
    pick style: Style = Style::Up, "style" => "order the held notes play in, with stepMode false";
    switch sync = true, "sync" => "true steps at division, false at timeMs";
    own rate: Division = Division::Eighth, "division", |p, d, key, f| f.lead(key, Json::from(p.rate.name()), p.rate == d.rate), |json, d, key| name(json, key).and_then(Division::from_name).filter(|rate| RATES.contains(rate)).unwrap_or(d.rate), |d, key, about, f| f.manual(key, one_of(RATE_LABELS, d.rate.name()), about) => "how fast the steps fire, as a note value";
    lead time_ms = 250.0, "timeMs", Arp::MIN_TIME..=Arp::MAX_TIME => "how fast the steps fire, in milliseconds, when sync is false";
    lead gate = 0.5, "gate", Arp::MIN_GATE..=Arp::MAX_GATE => "how much of its step each note sounds, 1 the whole step; with stepMode true each step's own gate is used";
    number swing = 0.0, "swing", 0.0..=1.0 => "delays every second step; 1 is a step and a half then a half, 0.67 the triplet feel";
    whole steps: usize = 1, "copies", 1.0..=Arp::MAX_STEPS as f64 => "copies of the chord, or of each step, stacked by distanceSemitones";
    own stack: [f64; STACK] = [12.0, 24.0, 36.0], "distanceSemitones", Arp::write_stack, Arp::read_stack, Arp::stack_manual => "pitch gap between stacked copies";
    nested pattern: Pattern = Pattern::DEFAULT, "pattern";
});

impl Arp {
    pub const MIN_GATE: f64 = 0.01;
    pub const MAX_GATE: f64 = 2.0;
    pub const MAX_STEPS: usize = 4;
    pub const MAX_DISTANCE: f64 = 24.0;
    pub const MAX_OFFSET: f64 = 48.0;
    pub const MIN_TIME: f64 = 5.0;
    pub const MAX_TIME: f64 = 2000.0;

    pub fn distance(&self) -> f64 {
        self.stack[0]
    }

    pub fn set_distance(&mut self, distance: f64) {
        let distance = distance.round().clamp(-Arp::MAX_DISTANCE, Arp::MAX_DISTANCE);
        self.stack = std::array::from_fn(|copy| distance * (copy + 1) as f64);
    }

    fn uniform(&self) -> Option<f64> {
        let distance = self.stack[0];
        (distance == distance.round() && distance.abs() <= Arp::MAX_DISTANCE && self.stack.iter().enumerate().all(|(copy, offset)| *offset == distance * (copy + 1) as f64)).then_some(distance)
    }

    fn offset_of(&self, copy: usize) -> f64 {
        if copy == 0 { 0.0 } else { self.stack[(copy - 1).min(STACK - 1)] }
    }

    fn delay_frames(&self, step_frames: f64) -> f64 {
        self.swing * HALF_STEP * step_frames
    }

    fn span_frames(&self, step_frames: f64, even: bool) -> f64 {
        let delay = self.delay_frames(step_frames);
        if even { step_frames + delay } else { step_frames - delay }
    }

    fn write_stack(&self, default: &Arp, key: &str, fields: &mut Fields) {
        match self.uniform() {
            Some(distance) => fields.number(key, distance, default.distance()),
            None => fields.push(STACKED, Json::Array(self.stack.iter().map(|offset| Json::Number(*offset)).collect())),
        }
    }

    fn read_stack(json: Option<&Json>, default: &Arp, key: &str) -> [f64; STACK] {
        let distance = number(json, key, default.distance()).round().clamp(-Arp::MAX_DISTANCE, Arp::MAX_DISTANCE);
        let mut stack = std::array::from_fn(|copy| distance * (copy + 1) as f64);
        for (slot, item) in stack.iter_mut().zip(items(json, STACKED)) {
            *slot = item.as_f64().filter(|offset| offset.is_finite()).map_or(*slot, |offset| offset.clamp(-Arp::MAX_OFFSET, Arp::MAX_OFFSET));
        }
        stack
    }

    fn stack_manual(default: &Arp, key: &str, about: &str, fields: &mut Fields) {
        fields.manual(key, bounded(default.distance(), -Arp::MAX_DISTANCE..=Arp::MAX_DISTANCE, true), about);
        let (lo, hi) = (json::number(-Arp::MAX_OFFSET), json::number(Arp::MAX_OFFSET));
        fields.push(STACKED, Json::from(format!("{STACK} numbers {lo} to {hi}: each copy's own offset above the first, fractional for cents; written instead of {key} when the copies are not evenly spaced")));
    }

    pub(crate) fn from_json(json: Option<&Json>) -> Arp {
        Arp::read(json, Arp::DEFAULT)
    }

    pub fn tempo(&mut self, bpm: f64) {
        if self.sync {
            self.rate = Division::nearest(self.time_ms, bpm, RATES);
        } else {
            self.time_ms = self.rate.ms(bpm).clamp(Arp::MIN_TIME, Arp::MAX_TIME);
        }
    }

    pub fn step_frames(&self, frames_per_beat: f64, frames_per_second: f64) -> f64 {
        if self.sync { self.rate.beats() * frames_per_beat } else { self.time_ms * frames_per_second / 1000.0 }
    }

    pub fn step_beats(&self, bpm: f64) -> f64 {
        self.step_frames(1.0, bpm / SECONDS_PER_MINUTE)
    }

    pub fn memory(&self, bpm: f64) -> f64 {
        let held = if self.stepped { self.pattern.chain() as f64 } else { Arp::MAX_GATE };
        (held.max(Arp::MAX_GATE) * (1.0 + HALF_STEP) + 1.0) * self.step_beats(bpm)
    }

    pub fn recalls(&self) -> bool {
        !self.stepped && self.style == Style::RandomOther
    }

    pub fn restarts_each_note(&self) -> bool {
        self.retrigger && self.retrigger_each_note
    }

    pub fn unheard(&self) -> Vec<&'static str> {
        let (pattern, default) = (&self.pattern, Arp::DEFAULT);
        let lane = |lane: Lane| (0..pattern.length()).any(|index| pattern.get(lane, index) != lane.initial());
        let join = |join: Join| (0..pattern.length()).any(|index| pattern.join(index) == join);
        let unheard = if self.stepped {
            vec![("style", self.style != default.style), ("gate", self.gate != default.gate), ("steps' fromLowest", lane(Lane::Note))]
        } else {
            vec![("fixedVelocity", self.fixed_velocity), ("stepChord", self.step_chord), ("finishGate", self.finish_gate), ("steps' semitones", lane(Lane::Semitone)), ("steps' cents", lane(Lane::Cents)), ("steps' velocity", lane(Lane::Velocity)), ("steps' gate", lane(Lane::Gate)), ("steps' tie", join(Join::Tie)), ("steps' slide", join(Join::Slide))]
        };
        unheard.into_iter().filter(|(_, set)| *set).map(|(key, _)| key).collect()
    }
}

fn diverge(s: i64, len: i64) -> i64 {
    let mid = (len - 1) / 2;
    if s == 0 {
        return mid;
    }
    let distance = (s + 1) / 2;
    let signed = if s % 2 == 1 { mid + distance } else { mid - distance };
    signed.clamp(0, len - 1)
}

fn converge(s: i64, len: i64) -> i64 {
    if s % 2 == 0 { (s / 2).min(len - 1) } else { (len - 1 - s / 2).max(0) }
}

fn bounce(step: i64, len: i64) -> i64 {
    let period = 2 * (len - 1);
    let p = step.rem_euclid(period);
    if p < len { p } else { period - p }
}

fn anchored(step: i64, len: i64, thumb: bool) -> i64 {
    let inner = len - 1;
    if inner <= 0 {
        return 0;
    }
    let period = (2 * (inner - 1)).max(1);
    let p = step.rem_euclid(period * 2);
    if thumb {
        if p % 2 == 0 {
            return 0;
        }
        let x = ((p - 1) / 2).rem_euclid(period);
        if inner == 1 { 1 } else { (if x < inner { x } else { period - x }) + 1 }
    } else {
        if p % 2 == 1 {
            return len - 1;
        }
        let x = (p / 2).rem_euclid(period);
        if inner == 1 { 0 } else if x < inner { x } else { period - x }
    }
}

pub fn order(style: Style, step: i64, len: usize) -> usize {
    let len = len as i64;
    if len <= 1 {
        return 0;
    }
    let s = step.rem_euclid(len);
    let round = 2 * len;
    let index = match style {
        Style::Up | Style::PlayOrder | Style::ChordTrigger | Style::RandomOther | Style::RandomOnce => s,
        Style::Down => len - 1 - s,
        Style::UpDown => bounce(step, len),
        Style::DownUp => len - 1 - bounce(step, len),
        Style::UpAndDown => {
            let p = step.rem_euclid(round);
            if p < len { p } else { round - 1 - p }
        }
        Style::DownAndUp => {
            let p = step.rem_euclid(round);
            if p < len { len - 1 - p } else { p - len }
        }
        Style::Converge => converge(s, len),
        Style::Diverge => diverge(s, len),
        Style::ConDiverge => {
            let p = step.rem_euclid(round);
            if p < len { converge(p, len) } else { diverge(p - len, len) }
        }
        Style::PinkyUp => {
            let p = step.rem_euclid(2 * (len - 1));
            if p % 2 == 1 { len - 1 } else { (p / 2).min(len - 2) }
        }
        Style::PinkyUpDown => anchored(step, len, false),
        Style::ThumbUp => {
            let p = step.rem_euclid(2 * (len - 1));
            if p % 2 == 0 { 0 } else { ((p - 1) / 2 + 1).min(len - 1) }
        }
        Style::ThumbUpDown => anchored(step, len, true),
        Style::Random => i64::from(hash(step) % len as u32),
    };
    index.clamp(0, len - 1) as usize
}

pub fn hash(step: i64) -> u32 {
    let mut x = (step as i32) as u32 ^ STEP_SALT;
    x = (x ^ (x >> 16)).wrapping_mul(0x85eb_ca6b);
    x = (x ^ (x >> 13)).wrapping_mul(0xc2b2_ae35);
    x ^ (x >> 16)
}

fn lines_before(beat: f64, step: f64) -> i64 {
    (beat / step - LINE_EPSILON).ceil() as i64
}

fn swung_lines_before(beat: f64, step: f64, delay: f64) -> i64 {
    let line = lines_before(beat, step);
    if (line - 1).rem_euclid(2) == 1 && (line - 1) as f64 * step + delay >= beat - LINE_EPSILON * step { line - 1 } else { line }
}

#[derive(Clone, Copy)]
struct Shifted {
    midi: u8,
    cents: f64,
}

fn shifted(midi: u8, offset: f64) -> Option<Shifted> {
    let whole = offset.round();
    let pitch = u8::try_from(i32::from(midi) + whole as i32).ok().filter(|pitch| *pitch <= 127)?;
    Some(Shifted { midi: pitch, cents: (offset - whole) * CENTS_PER_SEMITONE })
}

fn transposed(midi: u8, semitones: i8) -> Option<u8> {
    shifted(midi, f64::from(semitones)).map(|moved| moved.midi)
}

#[derive(Clone, Copy)]
pub struct Transport {
    pub beat: f64,
}

#[derive(Clone, Copy)]
enum Cue {
    On { at: i64, midi: u8, velocity: u8, articulation: Articulation },
    Off { at: i64, midi: u8 },
}

impl Cue {
    fn at(self) -> i64 {
        match self {
            Cue::On { at, .. } | Cue::Off { at, .. } => at,
        }
    }

    fn midi(self) -> u8 {
        match self {
            Cue::On { midi, .. } | Cue::Off { midi, .. } => midi,
        }
    }
}

struct Grid {
    at: f64,
    stride: f64,
    delay: f64,
    line: i64,
    to: i64,
}

impl Grid {
    fn peek(&self) -> Option<i64> {
        (self.line < self.to).then(|| (self.at + if self.line.rem_euclid(2) == 1 { self.delay } else { 0.0 }).round() as i64)
    }

    fn take(&mut self) {
        self.at += self.stride;
        self.line += 1;
    }
}

pub struct Runner {
    arp: Arp,
    heard: bool,
    held: [(u8, u8); HELD],
    held_len: usize,
    velocity: [u8; 128],
    cents: [f64; 128],
    voicing: [u8; POOL],
    sorted: [u8; POOL],
    pool_len: usize,
    pending: [(u8, i64); PENDING],
    pending_len: usize,
    cues: [Cue; CUES],
    cues_len: usize,
    step: i64,
    last: i64,
    seed: u32,
    next: f64,
    playing: Option<i64>,
    sounding: [(u8, u8, u8); HELD],
    sounding_len: usize,
    placed: Option<(i64, f64, f64)>,
    snapper: Snapper,
    transpose: i8,
    entered: [u8; 128],
    passed: u128,
    leaving: bool,
}

fn key(midi: u8) -> u128 {
    1u128.checked_shl(u32::from(midi)).unwrap_or(0)
}

impl Runner {
    pub const fn new(arp: Arp) -> Runner {
        Runner {
            arp,
            heard: arp.on,
            held: [(0, 0); HELD],
            held_len: 0,
            velocity: [127; 128],
            cents: [0.0; 128],
            voicing: [0; POOL],
            sorted: [0; POOL],
            pool_len: 0,
            pending: [(0, 0); PENDING],
            pending_len: 0,
            cues: [Cue::Off { at: 0, midi: 0 }; CUES],
            cues_len: 0,
            step: 0,
            last: -1,
            seed: 0,
            next: 0.0,
            playing: None,
            sounding: [(0, 0, 0); HELD],
            sounding_len: 0,
            placed: None,
            snapper: Snapper::new(),
            transpose: 0,
            entered: [UNENTERED; 128],
            passed: 0,
            leaving: false,
        }
    }

    pub fn join(&mut self, sounding: u128) {
        self.passed = sounding;
    }

    pub fn retire(&mut self, leaving: bool) {
        self.leaving = leaving;
    }

    pub fn idle(&self, sounding: u128) -> bool {
        self.held_len == 0 && self.pending_len == 0 && self.cues_len == 0 && sounding & !self.passed == 0
    }

    fn passes(&self, midi: u8) -> bool {
        self.passed & key(midi) != 0
    }

    fn unpass(&mut self, midi: u8) -> bool {
        let passed = self.passes(midi);
        self.passed &= !key(midi);
        passed
    }

    pub fn playing(&self) -> Option<i64> {
        self.playing
    }

    pub fn stage(&mut self, arp: Arp) {
        self.arp = arp;
        self.repool();
    }

    pub fn snap_to(&mut self, snap: Option<Snap>) {
        self.snapper.set(snap);
    }

    pub fn transpose_by(&mut self, semitones: i8) {
        self.transpose = semitones;
    }

    fn enter(&mut self, midi: u8) -> (Option<u8>, Option<u8>) {
        let entered = transposed(midi, self.transpose);
        let before = std::mem::replace(&mut self.entered[usize::from(midi)], entered.unwrap_or(UNENTERED));
        ((before != UNENTERED && Some(before) != entered).then_some(before), entered)
    }

    fn leave(&mut self, midi: u8) -> Option<u8> {
        match std::mem::replace(&mut self.entered[usize::from(midi)], UNENTERED) {
            UNENTERED => transposed(midi, self.transpose),
            entered => Some(entered),
        }
    }

    pub fn expect(&mut self, arriving: Option<u8>) {
        self.snapper.expect(arriving.and_then(|midi| transposed(midi, self.transpose)));
    }

    pub fn place(&mut self, frame: i64, beat: f64, frames_per_beat: f64) {
        self.placed = Some((frame, beat, frames_per_beat));
        self.snapper.restart();
    }

    pub fn holding(&self) -> bool {
        self.held_len > 0
    }

    pub fn rewind(&mut self, zero: f64) {
        let (arp, placed, snapper, transpose, leaving) = (self.arp, self.placed, self.snapper.cleared(), self.transpose, self.leaving);
        *self = Runner { next: zero, placed, snapper, transpose, leaving, ..Runner::new(arp) };
    }

    fn drawn(&self, now: i64) -> i64 {
        self.placed.map_or(now, |(frame, beat, frames_per_beat)| ((beat + (now - frame) as f64 / frames_per_beat) * DRAWS_PER_BEAT).round() as i64)
    }

    pub fn retime(&mut self, now: i64, ratio: f64) {
        if self.arp.sync {
            self.next = now as f64 + (self.next - now as f64) * ratio;
        }
    }

    pub fn settle(&mut self, sink: &mut impl Sink, now: i64) {
        let on = self.arp.on;
        if on == self.heard {
            return;
        }
        self.heard = on;
        if on {
            for &(midi, _) in &self.held[..self.held_len] {
                self.snapper.note_off(sink, midi);
            }
            self.start(now);
        } else {
            self.silence(sink);
            for &(midi, velocity) in &self.held[..self.held_len] {
                self.snapper.note_on(sink, midi, velocity);
            }
        }
    }

    pub fn room(&self) -> usize {
        CUES - self.cues_len
    }

    pub fn release(&mut self, sink: &mut impl Sink) {
        if self.arp.on {
            self.silence(sink);
            self.sounding_len = 0;
        } else {
            for &(midi, _) in &self.held[..self.held_len] {
                self.snapper.note_off(sink, midi);
            }
        }
        self.snapper.release(sink);
        self.held_len = 0;
        self.cues_len = 0;
        self.entered = [UNENTERED; 128];
        self.repool();
    }

    pub fn note_on(&mut self, midi: u8, velocity: u8, articulation: Articulation, sink: &mut impl Sink, now: i64) {
        sink.at(now);
        if self.leaving {
            self.passed |= key(midi);
            sink.strike(midi, velocity, articulation);
            return;
        }
        if self.unpass(midi) {
            sink.note_off(midi);
        }
        let (stale, entered) = self.enter(midi);
        if let Some(stale) = stale {
            self.lift(stale, sink);
        }
        if let Some(midi) = entered {
            self.press(midi, velocity, articulation, sink, now);
        }
    }

    fn press(&mut self, midi: u8, velocity: u8, articulation: Articulation, sink: &mut impl Sink, now: i64) {
        let fresh = self.held_len == 0;
        self.hold(midi, velocity);
        self.repool();
        if !self.arp.on {
            self.snapper.strike(sink, midi, velocity, articulation);
        } else if self.arp.retrigger && (fresh || self.arp.retrigger_each_note) {
            self.start(now);
        }
    }

    pub fn note_off(&mut self, midi: u8, sink: &mut impl Sink) {
        if self.unpass(midi) {
            sink.note_off(midi);
        } else if let Some(midi) = self.leave(midi) {
            self.lift(midi, sink);
        }
    }

    fn lift(&mut self, midi: u8, sink: &mut impl Sink) {
        self.unhold(midi);
        self.repool();
        if !self.arp.on {
            self.snapper.note_off(sink, midi);
        } else if self.arp.stepped && self.held_len == 0 {
            if !self.arp.finish_gate {
                self.end_all(sink);
            }
            self.sounding_len = 0;
        }
    }

    pub fn due(&self) -> Option<i64> {
        self.pending[..self.pending_len].iter().map(|pending| pending.1).min()
    }

    pub fn cue_on(&mut self, midi: u8, velocity: u8, articulation: Articulation, at: i64, sink: &mut impl Sink) {
        self.cue(Cue::On { at, midi, velocity, articulation }, sink);
    }

    pub fn cue_off(&mut self, midi: u8, at: i64, sink: &mut impl Sink) {
        self.cue(Cue::Off { at, midi }, sink);
    }

    fn cue(&mut self, cue: Cue, sink: &mut impl Sink) {
        for now in [cue.at() - 1, cue.at()] {
            if self.cues_len == CUES {
                self.play_due(sink, now);
            }
        }
        if self.cues_len < CUES {
            self.cues[self.cues_len] = cue;
            self.cues_len += 1;
        } else {
            sink.at(cue.at());
            self.play(cue, sink);
        }
    }

    fn play(&mut self, cue: Cue, sink: &mut impl Sink) {
        match cue {
            Cue::On { at, midi, velocity, articulation } => self.note_on(midi, velocity, articulation, sink, at),
            Cue::Off { midi, .. } => self.note_off(midi, sink),
        }
    }

    fn play_due(&mut self, sink: &mut impl Sink, now: i64) {
        let mut due = [Cue::Off { at: 0, midi: 0 }; CUES];
        let (mut due_len, mut kept, mut index) = (0, 0, 0);
        while index < self.cues_len {
            let cue = self.cues[index];
            index += 1;
            if cue.at() > now {
                self.cues[kept] = cue;
                kept += 1;
            } else {
                due[due_len] = cue;
                due_len += 1;
            }
        }
        self.cues_len = kept;
        let due = &due[..due_len];
        self.expect(due.iter().filter(|cue| matches!(cue, Cue::On { .. })).map(|cue| cue.midi()).min());
        if self.arp.on {
            for cue in due {
                sink.at(cue.at());
                self.play(*cue, sink);
            }
        } else {
            let mut taken = [false; CUES];
            let (pairs, paired) = self.pair_due(due, &mut taken);
            for (cue, _) in due.iter().zip(taken).filter(|(cue, taken)| !taken && matches!(cue, Cue::Off { .. })) {
                sink.at(cue.at());
                self.play(*cue, sink);
            }
            for &(off, on) in &pairs[..paired] {
                let Cue::On { at, midi: to, velocity, articulation } = due[on] else {
                    continue;
                };
                let from = due[off].midi();
                sink.at(at);
                if self.leaving {
                    self.passed = (self.passed & !key(from)) | key(to);
                    sink.legato(from, to, velocity, articulation);
                } else {
                    self.legato(from, to, velocity, articulation, sink, at);
                }
            }
            for (cue, _) in due.iter().zip(taken).filter(|(cue, taken)| !taken && matches!(cue, Cue::On { .. })) {
                sink.at(cue.at());
                self.play(*cue, sink);
            }
        }
        self.expect(None);
        sink.at(now);
    }

    fn pair_due(&self, due: &[Cue], taken: &mut [bool; CUES]) -> ([(usize, usize); CUES], usize) {
        let (mut offs, mut ons) = ([0usize; CUES], [0usize; CUES]);
        let (mut offs_len, mut ons_len) = (0, 0);
        for (index, cue) in due.iter().enumerate() {
            match cue {
                Cue::Off { midi, .. } if self.passes(*midi) != self.leaving => {}
                Cue::Off { .. } => {
                    offs[offs_len] = index;
                    offs_len += 1;
                }
                Cue::On { articulation, .. } if articulation.join != Join::Play => {
                    ons[ons_len] = index;
                    ons_len += 1;
                }
                Cue::On { .. } => {}
            }
        }
        offs[..offs_len].sort_unstable_by_key(|index| due[*index].midi());
        ons[..ons_len].sort_unstable_by_key(|index| due[*index].midi());
        let mut pairs = [(0usize, 0usize); CUES];
        let mut paired = 0;
        for (&off, &on) in offs[..offs_len].iter().zip(&ons[..ons_len]) {
            taken[off] = true;
            taken[on] = true;
            pairs[paired] = (off, on);
            paired += 1;
        }
        (pairs, paired)
    }

    fn legato(&mut self, from: u8, to: u8, velocity: u8, articulation: Articulation, sink: &mut impl Sink, now: i64) {
        let left = self.leave(from);
        let (stale, entered) = self.enter(to);
        match (left, entered, stale) {
            (Some(from), Some(to), None) => {
                self.unhold(from);
                self.hold(to, velocity);
                self.repool();
                self.snapper.legato(sink, from, to, velocity, articulation);
            }
            _ => {
                for midi in left.into_iter().chain(stale) {
                    self.lift(midi, sink);
                }
                if let Some(to) = entered {
                    self.press(to, velocity, articulation, sink, now);
                }
            }
        }
    }

    fn grid(&self, transport: Transport, now: i64, frames: usize, frames_per_beat: f64) -> Grid {
        let step_beats = self.arp.rate.beats();
        let half_frame = 0.5 / frames_per_beat;
        let block_end = transport.beat + frames as f64 / frames_per_beat;
        let delay_beats = self.arp.swing * HALF_STEP * step_beats;
        let from = swung_lines_before(transport.beat - half_frame, step_beats, delay_beats);
        Grid {
            at: now as f64 + (from as f64 * step_beats - transport.beat) * frames_per_beat,
            stride: step_beats * frames_per_beat,
            delay: delay_beats * frames_per_beat,
            line: from,
            to: swung_lines_before(block_end - half_frame, step_beats, delay_beats),
        }
    }

    fn due_step(&self, step_frames: f64) -> i64 {
        (self.next + if self.step.rem_euclid(2) == 1 { self.arp.delay_frames(step_frames) } else { 0.0 }).round() as i64
    }

    pub fn render<S: Sink>(&mut self, sink: &mut S, now: i64, frames: usize, frames_per_beat: f64, frames_per_second: f64, transport: Option<Transport>, mut span: impl FnMut(&mut S, usize, usize)) {
        self.placed = transport.map(|transport| (now, transport.beat, frames_per_beat));
        let end = now + frames as i64;
        let step_frames = self.arp.step_frames(frames_per_beat, frames_per_second);
        let stride = step_frames.max(1.0);
        let mut grid = transport.filter(|_| self.arp.sync && !self.arp.retrigger).map(|transport| self.grid(transport, now, frames, frames_per_beat));
        if self.arp.on && grid.is_none() {
            self.catch_up(now, stride, step_frames);
        }
        let mut done = 0;
        loop {
            let step = if self.arp.on { grid.as_ref().map_or(Some(self.due_step(step_frames)), Grid::peek) } else { None };
            let cue = self.cues[..self.cues_len].iter().map(|cue| cue.at()).min();
            let at = [cue, self.due(), step].into_iter().flatten().min().map_or(end, |at| at.clamp(now + done as i64, end));
            let to = (at - now) as usize;
            if to > done {
                span(sink, done, to);
                done = to;
            }
            if at >= end {
                break;
            }
            self.play_due(sink, at);
            self.end_due(sink, at);
            if !self.arp.on {
                continue;
            }
            match &mut grid {
                Some(grid) => {
                    while grid.peek().is_some_and(|line| line <= at) {
                        self.step = grid.line;
                        let even = grid.line.rem_euclid(2) == 0;
                        grid.take();
                        self.fire(sink, at, step_frames, even);
                    }
                }
                None => {
                    self.catch_up(at, stride, step_frames);
                    while self.due_step(step_frames) <= at {
                        let even = self.step.rem_euclid(2) == 0;
                        self.fire(sink, at, step_frames, even);
                        self.next += stride;
                    }
                }
            }
        }
        if let Some(grid) = grid
            && self.arp.on
        {
            self.next = grid.at;
        }
    }

    fn catch_up(&mut self, now: i64, stride: f64, step_frames: f64) {
        if self.due_step(step_frames) < now {
            let skipped = ((now as f64 - 0.5 - self.next) / stride).ceil();
            self.next += skipped * stride;
            self.step += skipped as i64;
            if self.step.rem_euclid(2) == 0 && (self.next - stride + self.arp.delay_frames(step_frames)).round() as i64 >= now {
                self.next -= stride;
                self.step -= 1;
            }
        }
    }

    fn start(&mut self, now: i64) {
        self.step = 0;
        self.last = -1;
        self.seed = hash(self.drawn(now));
        self.next = now as f64;
        self.sounding_len = 0;
    }

    fn hold(&mut self, midi: u8, velocity: u8) {
        self.velocity[usize::from(midi)] = velocity;
        if let Some(slot) = self.held[..self.held_len].iter_mut().find(|held| held.0 == midi) {
            slot.1 = velocity;
        } else if self.held_len < HELD {
            self.held[self.held_len] = (midi, velocity);
            self.held_len += 1;
        }
    }

    fn unhold(&mut self, midi: u8) {
        let Some(index) = self.held[..self.held_len].iter().position(|held| held.0 == midi) else {
            return;
        };
        self.held.copy_within(index + 1..self.held_len, index);
        self.held_len -= 1;
    }

    fn repool(&mut self) {
        self.snapper.lean(self.held[..self.held_len].iter().map(|held| held.0).min());
        let mut base = [0u8; HELD];
        let mut base_len = 0;
        let mut seen = [false; 128];
        for floor in [LOW_NOTE, 0] {
            for &(midi, _) in &self.held[..self.held_len] {
                if midi >= floor && !seen[usize::from(midi)] {
                    seen[usize::from(midi)] = true;
                    base[base_len] = midi;
                    base_len += 1;
                }
            }
            if base_len > 0 {
                break;
            }
        }
        let copies = self.arp.steps.clamp(1, Arp::MAX_STEPS);
        let mut stacked = [false; 128];
        self.pool_len = 0;
        for copy in 0..copies {
            let offset = self.arp.offset_of(copy);
            for &midi in &base[..base_len] {
                let Some(Shifted { midi: pitch, cents }) = shifted(midi, offset) else {
                    continue;
                };
                if stacked[usize::from(pitch)] || self.pool_len >= POOL {
                    continue;
                }
                stacked[usize::from(pitch)] = true;
                self.voicing[self.pool_len] = pitch;
                self.pool_len += 1;
                self.velocity[usize::from(pitch)] = self.velocity[usize::from(midi)];
                self.cents[usize::from(pitch)] = cents;
            }
        }
        self.sorted[..self.pool_len].copy_from_slice(&self.voicing[..self.pool_len]);
        self.sorted[..self.pool_len].sort_unstable();
        if self.pool_len == 0 {
            self.playing = None;
            self.last = -1;
        }
    }

    fn fire(&mut self, sink: &mut impl Sink, now: i64, step_frames: f64, even: bool) {
        let step = self.step;
        self.step += 1;
        if self.arp.stepped {
            self.sequence(sink, now, step, step_frames, even);
            return;
        }
        let n = self.pool_len as i64;
        if n == 0 {
            self.playing = None;
            return;
        }
        self.playing = Some(step);
        let pattern = self.arp.pattern;
        if pattern.rested_at(step) {
            return;
        }
        let off = now + ((self.arp.span_frames(step_frames, even) * self.arp.gate).round() as i64).max(MIN_GATE_FRAMES);
        let shift = i32::from(pattern.at(Lane::Octave, step)) * SEMITONES_PER_OCTAVE;
        let modulation = f64::from(pattern.at(Lane::Mod, step)) / f64::from(FULL);
        let choice = i64::from(pattern.at(Lane::Note, step));
        if choice != i64::from(RUN) {
            let index = (choice - 1).min(n - 1);
            let midi = self.sorted[index as usize];
            self.sound(sink, midi, shift, off, modulation);
            self.last = index;
            return;
        }
        let run = pattern.runs_before(step);
        if self.arp.style == Style::ChordTrigger {
            let mut strikes = [(0u8, 0u8, Articulation::PLAIN); POOL];
            let mut struck = 0;
            for index in 0..self.pool_len {
                if let Some(strike) = self.sounded(self.sorted[index], shift, modulation) {
                    strikes[struck] = strike;
                    struck += 1;
                }
            }
            self.strike_all(sink, &mut strikes[..struck], off);
            self.last = -1;
            return;
        }
        let index = match self.arp.style {
            Style::RandomOther => {
                let candidate = i64::from(hash(run) % n as u32);
                if self.last >= 0 && n > 1 && candidate == self.last { (candidate + 1) % n } else { candidate }
            }
            Style::RandomOnce => i64::from((self.seed ^ hash(run)) % n as u32),
            style => order(style, run, self.pool_len) as i64,
        };
        let midi = if self.arp.style == Style::PlayOrder { self.voicing[index as usize] } else { self.sorted[index as usize] };
        self.sound(sink, midi, shift, off, modulation);
        self.last = index;
    }

    fn sequence(&mut self, sink: &mut impl Sink, now: i64, step: i64, step_frames: f64, even: bool) {
        if self.held_len == 0 {
            self.playing = None;
            return;
        }
        self.playing = Some(step);
        let pattern = self.arp.pattern;
        if pattern.rested_at(step) {
            return;
        }
        let shift = i32::from(pattern.at(Lane::Semitone, step)) + i32::from(pattern.at(Lane::Octave, step)) * SEMITONES_PER_OCTAVE;
        let share = f64::from(pattern.at(Lane::Velocity, step)) / f64::from(FULL);
        let modulation = f64::from(pattern.at(Lane::Mod, step)) / f64::from(FULL);
        let tune = f64::from(pattern.at(Lane::Cents, step));
        let off = now + self.held_frames(step, step_frames, even);
        let copies = self.arp.steps.clamp(1, Arp::MAX_STEPS);
        let join = pattern.join_at(step);
        let keys = if self.arp.step_chord { self.held_len } else { 1 };
        let (held, held_len) = (self.held, self.held_len);
        let (was, was_len) = (self.sounding, self.sounding_len);
        let pending = |runner: &Runner, midi: u8| runner.pending[..runner.pending_len].iter().position(|pending| pending.0 == midi);
        let carrying = join != Join::Play && was[..was_len].iter().any(|sounding| pending(self, sounding.1).is_some());
        if !carrying {
            self.end_all(sink);
        }
        self.sounding_len = 0;
        let mut glides = [(0u8, 0u8, 0u8, Articulation::PLAIN); STRUCK];
        let mut glided = 0;
        let mut strikes = [(0u8, 0u8, Articulation::PLAIN); STRUCK];
        let mut striking = 0;
        for &(key, key_velocity) in &held[held_len - keys..held_len] {
            let Some(pitch) = u8::try_from(i32::from(key) + shift).ok().filter(|pitch| *pitch <= 127) else {
                continue;
            };
            let struck = if self.arp.fixed_velocity { TOP_VELOCITY } else { key_velocity };
            let velocity = (f64::from(struck) * share).round().clamp(1.0, f64::from(TOP_VELOCITY)) as u8;
            let carried = was[..was_len].iter().find(|sounding| carrying && sounding.0 == key && pending(self, sounding.1).is_some()).map(|sounding| (sounding.1, sounding.2));
            if keys > 1 && join != Join::Play && carried.is_none() {
                continue;
            }
            for copy in 0..copies {
                let offset = self.arp.offset_of(copy);
                let Some(Shifted { midi, cents }) = shifted(pitch, offset) else {
                    continue;
                };
                let cents = cents + tune;
                let carried_copy = carried.and_then(|(from, struck)| shifted(from, offset).map(|from| (from.midi, struck))).filter(|(from, _)| pending(self, *from).is_some());
                match carried_copy {
                    Some((from, velocity)) if glided < STRUCK => {
                        glides[glided] = (from, midi, velocity, Articulation::stepped(cents, modulation, shift, join));
                        glided += 1;
                    }
                    None if striking < STRUCK => {
                        strikes[striking] = (midi, velocity, Articulation::stepped(cents, modulation, shift, Join::Play));
                        striking += 1;
                    }
                    Some(_) | None => {}
                }
            }
            if self.sounding_len < HELD {
                self.sounding[self.sounding_len] = (key, pitch, carried.map_or(velocity, |(_, struck)| struck));
                self.sounding_len += 1;
            }
        }
        let strikes = Runner::lowest_first(&mut strikes[..striking]);
        for &(midi, ..) in strikes.iter() {
            self.unpend(sink, midi);
        }
        glides[..glided].sort_by_key(|glide| glide.0);
        for &(from, to, velocity, articulation) in &glides[..glided] {
            if let Some(index) = pending(self, from) {
                self.snapper.legato(sink, from, to, velocity, articulation);
                self.pending[index] = (to, off);
            }
        }
        for &(midi, velocity, articulation) in strikes.iter() {
            self.pend(sink, midi, velocity, off, articulation);
        }
    }

    fn lowest_first(strikes: &mut [(u8, u8, Articulation)]) -> &[(u8, u8, Articulation)] {
        strikes.sort_by_key(|strike| strike.0);
        let mut kept = 0;
        for at in 0..strikes.len() {
            if kept > 0 && strikes[kept - 1].0 == strikes[at].0 {
                strikes[kept - 1] = strikes[at];
            } else {
                strikes[kept] = strikes[at];
                kept += 1;
            }
        }
        &strikes[..kept]
    }

    fn strike_all(&mut self, sink: &mut impl Sink, strikes: &mut [(u8, u8, Articulation)], off: i64) {
        let strikes = Runner::lowest_first(strikes);
        for &(midi, ..) in strikes.iter() {
            self.unpend(sink, midi);
        }
        for &(midi, velocity, articulation) in strikes.iter() {
            self.pend(sink, midi, velocity, off, articulation);
        }
    }

    fn held_frames(&self, step: i64, step_frames: f64, even: bool) -> i64 {
        let pattern = &self.arp.pattern;
        let reach = pattern.length() as i64 - 1;
        let mut last = step;
        let mut frames = 0.0;
        let mut parity = even;
        while last - step < reach && pattern.continues(last + 1) {
            frames += self.arp.span_frames(step_frames, parity);
            parity = !parity;
            last += 1;
        }
        let gate = f64::from(pattern.at(Lane::Gate, last)) / f64::from(FULL);
        ((frames + gate * self.arp.span_frames(step_frames, parity)).round() as i64).max(MIN_GATE_FRAMES)
    }

    fn sounded(&self, midi: u8, shift: i32, modulation: f64) -> Option<(u8, u8, Articulation)> {
        let pitch = u8::try_from(i32::from(midi) + shift).ok().filter(|pitch| *pitch <= 127)?;
        Some((pitch, self.velocity[usize::from(midi)], Articulation::stepped(self.cents[usize::from(midi)], modulation, shift, Join::Play)))
    }

    fn sound(&mut self, sink: &mut impl Sink, midi: u8, shift: i32, off: i64, modulation: f64) {
        if let Some((pitch, velocity, articulation)) = self.sounded(midi, shift, modulation) {
            self.unpend(sink, pitch);
            self.pend(sink, pitch, velocity, off, articulation);
        }
    }

    fn unpend(&mut self, sink: &mut impl Sink, midi: u8) {
        if let Some(index) = self.pending[..self.pending_len].iter().position(|pending| pending.0 == midi) {
            self.snapper.note_off(sink, midi);
            self.pending.copy_within(index + 1..self.pending_len, index);
            self.pending_len -= 1;
        }
    }

    fn pend(&mut self, sink: &mut impl Sink, midi: u8, velocity: u8, off: i64, articulation: Articulation) {
        if self.pending_len < PENDING {
            self.snapper.strike(sink, midi, velocity, articulation);
            self.pending[self.pending_len] = (midi, off);
            self.pending_len += 1;
        }
    }

    fn end_due(&mut self, sink: &mut impl Sink, now: i64) {
        let mut kept = 0;
        for index in 0..self.pending_len {
            let (midi, off) = self.pending[index];
            if off <= now {
                self.snapper.note_off(sink, midi);
            } else {
                self.pending[kept] = (midi, off);
                kept += 1;
            }
        }
        self.pending_len = kept;
    }

    fn end_all(&mut self, sink: &mut impl Sink) {
        for &(midi, _) in &self.pending[..self.pending_len] {
            self.snapper.note_off(sink, midi);
        }
        self.pending_len = 0;
    }

    fn silence(&mut self, sink: &mut impl Sink) {
        self.end_all(sink);
        self.playing = None;
    }
}

use crate::arp::{Articulation, Join};
use sdk::draw::{Rect, Tone};
use sdk::face::{CELL, FRAME, Format, PITCH};
use crate::grid;
use crate::project::{BEATS_PER_BAR, Clip, Note, VELOCITY, beats, ticks};

mod draw;
mod edit;

pub use draw::name;
pub use edit::{chordify, double, halve, quantize, reverse};

pub const ZOOM_MIN: i32 = 6;
pub const ZOOM_MAX: i32 = 40;
pub const ZOOM_DEFAULT: i32 = 12;
pub const TIME_MIN: f32 = 4.0;
pub const TIME_MAX: f32 = 240.0;
const LOW: u8 = 12;
const TOP: u8 = 107;
const OCTAVE: i32 = 12;
const PITCHES: i32 = (TOP - LOW) as i32 + 1;
const SNAP_DEFAULT: f32 = 1.0;
const STEP_PX: f32 = 6.0;
const ARM: i32 = 4;
const GRAB: i32 = 8;
const CHOP: i32 = GRAB;
const DASH: i32 = CHOP - FRAME;
const TOUCH: f32 = 1e-3;
const ZOOM_PX_PER_ROW: i32 = 6;
const KEYS: i32 = 2 * PITCH;
const READOUT_SCALE: i32 = 2;
const GUTTER: i32 = 20;
const KEY_X: i32 = 2 * FRAME + GUTTER;
const BLACK_W: i32 = 24;
const BIG_FRAME: i32 = 1;
const BLACK: [bool; 12] = [false, true, false, true, false, false, true, false, true, false, true, false];
const FIELD: [Tone; 4] = [Tone::Ground, Tone::Ink25, Tone::Ink50, Tone::Ink75];
const PULSE_SECONDS: f32 = 1.5;
const PULSE_STEPS: f32 = 16.0;
const NOTE_CELLS: i32 = 5;
const SHIFT_PX_PER_SEMITONE: i32 = 3;
const MOD_STEP: f64 = 0.01;

pub type NoteKey = (u8, f32);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dial {
    Velocity,
    Cents,
    Mod,
    Shift,
}

impl Dial {
    pub fn label(self) -> &'static str {
        match self {
            Dial::Velocity => "VEL",
            Dial::Cents => "CENT",
            Dial::Mod => "MOD",
            Dial::Shift => "SHIFT",
        }
    }

    pub fn format(self) -> Format {
        match self {
            Dial::Velocity => Format::Integer,
            Dial::Cents => Format::SignedCents,
            Dial::Mod => Format::SignedPercent,
            Dial::Shift => Format::Semitones,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Dial::Velocity => "How hard the picked notes strike; SHIFT-press or double-press for 127",
            Dial::Cents => "Tunes the picked notes in cents; SHIFT-press or double-press for 0",
            Dial::Mod => "Held MOD value of the picked notes; SHIFT-press or double-press clears it",
            Dial::Shift => "Arp step shift in semitones, scaled by STEP SHARE; SHIFT-press or double-press for 0",
        }
    }

    fn range(self) -> (f64, f64) {
        match self {
            Dial::Velocity => (1.0, f64::from(VELOCITY)),
            Dial::Cents => (-f64::from(Articulation::MAX_CENTS), f64::from(Articulation::MAX_CENTS)),
            Dial::Mod => (-1.0, 1.0),
            Dial::Shift => (-f64::from(Articulation::MAX_SHIFT), f64::from(Articulation::MAX_SHIFT)),
        }
    }

    fn per_px(self) -> f64 {
        match self {
            Dial::Velocity | Dial::Cents => 1.0,
            Dial::Mod => MOD_STEP,
            Dial::Shift => 1.0 / f64::from(SHIFT_PX_PER_SEMITONE),
        }
    }

    fn centred(self) -> bool {
        self != Dial::Velocity
    }

    fn travel(self, value: f64) -> f64 {
        let (lo, hi) = self.range();
        (value - lo) / (hi - lo)
    }

    pub fn of(self, note: &Note) -> Option<f64> {
        match self {
            Dial::Velocity => Some(f64::from(note.velocity)),
            Dial::Cents => Some(f64::from(note.articulation.cents)),
            Dial::Mod => note.articulation.modulation.map(f64::from),
            Dial::Shift => Some(f64::from(note.articulation.shift)),
        }
    }

    fn set(self, note: &mut Note, value: f64) {
        let (lo, hi) = self.range();
        let value = value.clamp(lo, hi);
        match self {
            Dial::Velocity => note.velocity = value.round() as u8,
            Dial::Cents => note.articulation.cents = value.round() as f32,
            Dial::Mod => note.articulation.modulation = Some(((value / MOD_STEP).round() * MOD_STEP) as f32),
            Dial::Shift => note.articulation.shift = value.round() as i8,
        }
    }

    fn reset(self, note: &mut Note) {
        match self {
            Dial::Velocity => note.velocity = VELOCITY,
            Dial::Cents => note.articulation.cents = 0.0,
            Dial::Mod => note.articulation.modulation = None,
            Dial::Shift => note.articulation.shift = 0,
        }
    }
}

pub enum Under {
    Keys(String),
    Zoom(String),
    Big(bool),
    Save,
    Preview(bool),
    Guide(bool),
    Fit,
    Chords,
    Double,
    Halve,
    Reverse,
    Up,
    Down,
    Tie(usize, Option<Join>),
    Dial(Dial, usize, Option<f64>),
    Note(Note),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Ghost {
    pub note: Note,
    pub source: usize,
    pub tick: i32,
}

impl Ghost {
    fn hold(&self) -> (usize, i32) {
        (self.source, self.tick)
    }
}

pub(super) fn key(note: &Note) -> NoteKey {
    (note.pitch, note.start)
}

fn step_of(snap: f32) -> i32 {
    ticks(f64::from(snap)).max(1)
}

fn tick_of(beat: f32) -> i32 {
    ticks(f64::from(beat)).max(0)
}

fn line_below(tick: i32, step: i32) -> i32 {
    tick - tick % step
}

pub(super) fn snap_round(beat: f32, snap: f32) -> f32 {
    let (tick, step) = (tick_of(beat), step_of(snap));
    let below = line_below(tick, step);
    beats(if 2 * (tick - below) <= step { below } else { below + step }) as f32
}

fn snap_floor(beat: f32, snap: f32) -> f32 {
    beats(line_below(tick_of(beat), step_of(snap))) as f32
}

pub(super) fn snap_after(beat: f32, snap: f32) -> f32 {
    let step = step_of(snap);
    beats(line_below(tick_of(beat), step) + step) as f32
}

pub(super) fn snap_before(beat: f32, snap: f32) -> f32 {
    beats(line_below((ticks(f64::from(beat)) - 1).max(0), step_of(snap))) as f32
}

pub(super) fn handle_w(w: i32) -> i32 {
    GRAB.min(w / 3)
}

pub fn shown(note: &Note, length: f32) -> bool {
    (LOW..=TOP).contains(&note.pitch) && note.start < length
}

pub(super) fn shifted(note: &Note, beats: f32, rows: i32) -> Note {
    Note { pitch: (i32::from(note.pitch) + rows) as u8, start: note.start + beats, ..*note }
}

#[derive(Clone, Copy)]
pub(super) enum Side {
    Start,
    End,
}

pub(super) fn least(note: &Note, side: Side, snap: f32) -> f32 {
    let end = note.start + note.length;
    let step = match side {
        Side::End => snap_after(note.start, snap) - note.start,
        Side::Start => end - snap_before(end, snap),
    };
    step.min(note.length)
}

pub(super) fn stretched(note: &Note, side: Side, delta: f32, length: f32, snap: f32) -> Note {
    let least = least(note, side, snap);
    match side {
        Side::End => {
            let room = (length - note.start).max(least);
            Note { length: (note.length + delta).clamp(least, room), ..*note }
        }
        Side::Start => {
            let end = note.start + note.length;
            let start = (note.start + delta).clamp(0.0, (end - least).max(0.0));
            Note { start, length: end - start, ..*note }
        }
    }
}

pub(super) fn quantized(note: &Note, length: f32, snap: f32) -> Note {
    let start = snap_round(note.start, snap).clamp(0.0, snap_before(length, snap));
    let end = snap_round(note.start + note.length, snap).clamp(snap_after(start, snap).min(length), length);
    Note { start, length: end - start, ..*note }
}

pub(super) fn reversed(note: &Note, join: Join, length: f32) -> Note {
    let end = length - note.start;
    let start = (end - note.length).max(0.0);
    Note { start, length: end - start, articulation: Articulation { join, ..note.articulation }, ..*note }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Nothing,
    Zoom,
    Time,
    Big,
    Save,
    Preview,
    Guide,
    Fit,
    Chords,
    Double,
    Halve,
    Reverse,
    Up,
    Down,
    Tie,
    Dial(Dial),
    Key(u8),
    Note(NoteKey),
    Start(NoteKey),
    End(NoteKey),
    Ghost(usize),
    Cell,
}

#[derive(Clone, Copy)]
struct Zoom {
    origin: (i32, i32),
    row: i32,
    focal: f64,
}

#[derive(Clone, Copy)]
struct TimeZoom {
    origin: (i32, i32),
    at_x: i32,
    time: f32,
    centre: f32,
}

#[derive(Clone, Copy)]
struct TimeAt {
    origin: (i32, i32),
    time: f32,
    beat: f32,
}

#[derive(Clone)]
struct Shift {
    anchor: Note,
    notes: Vec<Note>,
    rest: Vec<Note>,
    origin: (i32, i32),
    at: (i32, i32),
    armed: bool,
}

#[derive(Clone)]
struct Stretch {
    side: Side,
    anchor: Note,
    notes: Vec<Note>,
    rest: Vec<Note>,
    origin_x: i32,
    armed: bool,
}

#[derive(Clone)]
struct Marquee {
    origin: (i32, i32),
    at: (i32, i32),
    armed: bool,
    hit: Option<NoteKey>,
}

#[derive(Clone)]
enum Drag {
    Zoom(Zoom),
    Time(TimeZoom),
    TimeAt(TimeAt),
    Key,
    Draw { note: Note, rest: Vec<Note>, origin_x: i32, armed: bool },
    Move(Shift),
    Resize(Stretch),
    Marquee(Marquee),
    Dial { dial: Dial, origin_y: i32, from: Vec<(NoteKey, f64)> },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Save,
    Fit,
    Ghost(Ghost),
}

pub struct Roll {
    w: i32,
    h: i32,
    row: i32,
    big: bool,
    guide: bool,
    preview: bool,
    scroll: i32,
    time: f32,
    scroll_x: i32,
    length: f32,
    snap: f32,
    centre: Option<u8>,
    shown: Option<(usize, i32)>,
    picked: Vec<NoteKey>,
    hover: Hit,
    drag: Option<Drag>,
    sounding: Option<u8>,
    ghosts: Vec<Ghost>,
    held: Option<(usize, i32)>,
    pulse: f32,
}

impl Default for Roll {
    fn default() -> Roll {
        Roll::new()
    }
}

impl Roll {
    pub fn new() -> Roll {
        Roll { w: 0, h: 0, row: ZOOM_DEFAULT, big: false, guide: false, preview: true, scroll: 0, time: TIME_MIN, scroll_x: 0, length: 1.0, snap: SNAP_DEFAULT, centre: None, shown: None, picked: Vec::new(), hover: Hit::Nothing, drag: None, sounding: None, ghosts: Vec::new(), held: None, pulse: 0.0 }
    }

    pub fn set_snap(&mut self, beats: f32) {
        self.snap = beats;
    }

    pub fn set_ghosts(&mut self, ghosts: Vec<Ghost>, held: Option<(usize, i32)>) {
        self.ghosts = ghosts;
        self.held = held;
    }

    pub fn shown(&self) -> Option<(usize, i32)> {
        self.shown
    }

    pub fn open(&mut self, key: (usize, i32), clip: &Clip) {
        self.close();
        self.shown = Some(key);
        self.length = clip.beats();
        self.scroll_x = 0;
        let midpoint = clip.pitch_range().map_or(60, |(lo, hi)| ((i32::from(lo) + i32::from(hi)) / 2) as u8);
        self.centre = Some(midpoint.clamp(LOW, TOP));
    }

    pub fn close(&mut self) {
        self.shown = None;
        self.picked.clear();
        self.hover = Hit::Nothing;
        self.drag = None;
        self.sounding = None;
    }

    pub fn settle(&mut self, clip: &Clip) {
        self.picked.retain(|picked| clip.notes.iter().any(|note| key(note) == *picked));
        self.length = clip.beats();
        self.clamp_scroll_x();
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn sounding(&self) -> Option<u8> {
        self.sounding.filter(|_| self.preview)
    }

    pub fn silence(&mut self) {
        self.sounding = None;
    }

    pub fn zoom(&self) -> i32 {
        self.row
    }

    pub fn big(&self) -> bool {
        self.big
    }

    pub fn guide(&self) -> bool {
        self.guide
    }

    pub fn preview(&self) -> bool {
        self.preview
    }

    pub fn remember(&mut self, row: i32, big: bool, guide: bool, preview: bool, time: f32) {
        self.row = row.clamp(ZOOM_MIN, ZOOM_MAX);
        self.big = big;
        self.guide = guide;
        self.preview = preview;
        self.time = time.clamp(TIME_MIN, TIME_MAX);
    }

    pub fn time(&self) -> f32 {
        self.time
    }

    pub fn unpick(&mut self) -> bool {
        let had = !self.picked.is_empty();
        self.picked.clear();
        had
    }

    pub fn pick_all(&mut self, clip: &Clip) -> bool {
        self.picked = clip.notes.iter().filter(|note| shown(note, clip.beats())).map(key).collect();
        !self.picked.is_empty()
    }

    pub fn picking(&self) -> bool {
        !self.picked.is_empty()
    }

    pub fn layout(&mut self, w: i32, h: i32) {
        self.w = w;
        self.h = h;
        if let Some(pitch) = self.centre.take() {
            let index = i32::from(TOP - pitch);
            self.scroll = self.top(index) + (self.top(index + 1) - self.top(index)) / 2 - self.visible() / 2;
        }
        self.clamp_scroll();
        self.clamp_scroll_x();
    }

    pub fn scroll_by(&mut self, dx: i32, dy: i32) -> bool {
        let before = (self.scroll, self.scroll_x);
        self.scroll -= dy;
        self.scroll_x -= dx;
        self.clamp_scroll();
        self.clamp_scroll_x();
        (self.scroll, self.scroll_x) != before
    }

    pub fn tick(&mut self, seconds: f32) -> bool {
        let was = self.breath_step();
        self.pulse = (self.pulse + seconds).rem_euclid(PULSE_SECONDS);
        let breathed = was != self.breath_step() && self.ghosts.iter().any(|ghost| shown(&ghost.note, self.length));
        self.pan(seconds) | breathed
    }

    pub(super) fn breath(&self) -> f32 {
        0.5 - 0.5 * (self.pulse / PULSE_SECONDS * std::f32::consts::TAU).cos()
    }

    fn breath_step(&self) -> i32 {
        (self.breath() * PULSE_STEPS).round() as i32
    }

    fn pan(&mut self, seconds: f32) -> bool {
        let Some(Drag::Time(lens)) = &self.drag else {
            return false;
        };
        let mut lens = *lens;
        let dx = lens.at_x - lens.origin.0;
        if dx.abs() < ARM {
            return false;
        }
        let length = self.length;
        let beat_w = self.beat_w(length);
        let half = self.view_w() as f32 / 2.0;
        let (lo, hi) = (half / beat_w, (self.max_scroll_x() as f32 + half) / beat_w);
        lens.centre = (lens.centre + (dx * dx.abs()) as f32 / grid::PAN_DAMPING as f32 * seconds / beat_w).clamp(lo, hi.max(lo));
        let centre = lens.centre;
        self.drag = Some(Drag::Time(lens));
        self.set_time(self.time, centre, length)
    }

    fn visible(&self) -> i32 {
        self.h - PITCH
    }

    fn frame(&self) -> i32 {
        if self.big && self.visible() < PITCHES * ZOOM_MIN { BIG_FRAME } else { FRAME }
    }

    fn span(&self) -> i32 {
        if self.big { (self.visible() - self.frame()).max(PITCHES) } else { PITCHES * self.row }
    }

    fn top(&self, index: i32) -> i32 {
        (index * self.span() + PITCHES / 2) / PITCHES
    }

    fn max_scroll(&self) -> i32 {
        (self.span() + self.frame() - self.visible()).max(0)
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.clamp(0, self.max_scroll());
    }

    fn focal(&self) -> f64 {
        (f64::from(self.scroll) + f64::from(self.visible()) / 2.0) / f64::from(self.span())
    }

    fn zoom_to(&mut self, row: i32, focal: f64, dy: i32) -> bool {
        let before = (self.row, self.scroll);
        self.row = row.clamp(ZOOM_MIN, ZOOM_MAX);
        self.scroll = (focal * f64::from(self.span()) - f64::from(self.visible()) / 2.0).round() as i32 - dy;
        self.clamp_scroll();
        (self.row, self.scroll) != before
    }

    fn keys_label(&self) -> String {
        format!("{} KEYS", (self.visible() * PITCHES + self.span() / 2) / self.span())
    }

    fn header_cell(&self, from_right: i32) -> Rect {
        Rect::new(self.w - from_right * KEYS - FRAME, 0, KEYS + FRAME, CELL)
    }

    fn header(&self) -> [(Rect, Hit); 11] {
        [(self.header_cell(1), Hit::Big), (self.header_cell(2), Hit::Save), (self.header_cell(3), Hit::Preview), (self.header_cell(4), Hit::Guide), (self.header_cell(5), Hit::Fit), (self.header_cell(6), Hit::Down), (self.header_cell(7), Hit::Up), (self.header_cell(8), Hit::Reverse), (self.header_cell(9), Hit::Halve), (self.header_cell(10), Hit::Double), (self.header_cell(11), Hit::Chords)]
    }

    pub fn fit_cell(&self) -> Rect {
        self.header_cell(5)
    }

    fn left_cell(&self, index: i32) -> Rect {
        Rect::new(index * KEYS, 0, KEYS + FRAME, CELL)
    }

    fn note_cells(&self) -> [(Rect, Hit); NOTE_CELLS as usize] {
        [(self.left_cell(1), Hit::Dial(Dial::Velocity)), (self.left_cell(2), Hit::Dial(Dial::Cents)), (self.left_cell(3), Hit::Dial(Dial::Mod)), (self.left_cell(4), Hit::Dial(Dial::Shift)), (self.left_cell(5), Hit::Tie)]
    }

    pub fn under(&self, clip: &Clip) -> Option<Under> {
        let picked = self.picked.len();
        Some(match self.hover {
            Hit::Nothing | Hit::Cell | Hit::Key(_) | Hit::Ghost(_) => return None,
            Hit::Zoom => Under::Keys(self.keys_label()),
            Hit::Time => Under::Zoom(self.bars_label(clip.beats())),
            Hit::Big => Under::Big(self.big),
            Hit::Save => Under::Save,
            Hit::Preview => Under::Preview(self.preview),
            Hit::Guide => Under::Guide(self.guide),
            Hit::Fit => Under::Fit,
            Hit::Chords => Under::Chords,
            Hit::Double => Under::Double,
            Hit::Halve => Under::Halve,
            Hit::Reverse => Under::Reverse,
            Hit::Up => Under::Up,
            Hit::Down => Under::Down,
            Hit::Tie => Under::Tie(picked, self.picked_join(clip)),
            Hit::Dial(dial) => Under::Dial(dial, picked, self.picked_value(clip, dial)),
            Hit::Note(at) | Hit::Start(at) | Hit::End(at) => Under::Note(*clip.notes.iter().find(|note| key(note) == at)?),
        })
    }

    fn view_w(&self) -> i32 {
        self.w - FRAME - KEYS
    }

    fn fit_w(&self, length: f32) -> f32 {
        self.view_w() as f32 / length
    }

    fn beat_w(&self, length: f32) -> f32 {
        self.fit_w(length).max(self.time)
    }

    fn max_scroll_x(&self) -> i32 {
        ((self.length * self.beat_w(self.length)).round() as i32 - self.view_w()).max(0)
    }

    fn clamp_scroll_x(&mut self) {
        self.scroll_x = self.scroll_x.clamp(0, self.max_scroll_x());
    }

    fn centre_beat(&self, length: f32) -> f32 {
        (self.scroll_x as f32 + self.view_w() as f32 / 2.0) / self.beat_w(length)
    }

    fn set_time(&mut self, time: f32, centre: f32, length: f32) -> bool {
        self.set_time_under(time, centre, self.view_w() as f32 / 2.0, length)
    }

    fn set_time_under(&mut self, time: f32, beat: f32, at: f32, length: f32) -> bool {
        let before = (self.time, self.scroll_x);
        self.time = time.clamp(self.fit_w(length).min(TIME_MAX), TIME_MAX);
        self.scroll_x = (beat * self.beat_w(length) - at).round() as i32;
        self.clamp_scroll_x();
        (self.time, self.scroll_x) != before
    }

    fn time_travel(&self, length: f32) -> f64 {
        f64::from((self.beat_w(length) / TIME_MIN).log2() / (TIME_MAX / TIME_MIN).log2())
    }

    fn bars_label(&self, length: f32) -> String {
        let beats = self.view_w() as f32 / self.beat_w(length);
        match ((beats / BEATS_PER_BAR as f32).round() as i32, beats.round() as i32) {
            (0, 1) => String::from("1 BEAT"),
            (0, beats) => format!("{beats} BEATS"),
            (1, _) => String::from("1 BAR"),
            (bars, _) => format!("{bars} BARS"),
        }
    }

    fn x_of(&self, beat: f32, length: f32) -> i32 {
        KEYS + (beat * self.beat_w(length)).round() as i32 - self.scroll_x
    }

    fn beat_at(&self, x: i32, length: f32) -> f32 {
        (x - KEYS + self.scroll_x) as f32 / self.beat_w(length)
    }

    fn row_y(&self, pitch: u8) -> i32 {
        PITCH + self.top(i32::from(TOP) - i32::from(pitch)) - self.scroll
    }

    fn row_h(&self, pitch: u8) -> i32 {
        let index = i32::from(TOP) - i32::from(pitch);
        self.top(index + 1) - self.top(index)
    }

    fn row_index(&self, y: i32) -> i32 {
        ((y - PITCH + self.scroll + 1) * PITCHES - PITCHES / 2 - 1).div_euclid(self.span())
    }

    fn pitch_at(&self, y: i32) -> Option<u8> {
        let index = self.row_index(y);
        (0..PITCHES).contains(&index).then(|| (i32::from(TOP) - index) as u8)
    }

    pub fn shown_pitches(&self) -> (u8, u8) {
        (self.pitch_at(self.h - 1).unwrap_or(LOW), self.pitch_at(PITCH).unwrap_or(TOP))
    }

    pub fn shown_beats(&self, clip: &Clip) -> (f32, f32) {
        let length = clip.beats();
        (self.beat_at(KEYS, length).max(0.0), self.beat_at(self.w, length).min(length))
    }

    pub fn rows_from(&self, from_y: i32, to_y: i32) -> i32 {
        self.row_index(from_y) - self.row_index(to_y)
    }

    fn rows_rect(&self) -> Rect {
        Rect::new(0, PITCH, self.w, self.visible())
    }

    fn grid_rect(&self) -> Rect {
        Rect::new(KEYS, PITCH, self.w - KEYS, self.visible())
    }

    fn note_rect(&self, note: &Note, length: f32) -> Rect {
        let x0 = self.x_of(note.start, length) + FRAME;
        let x1 = self.x_of((note.start + note.length).min(length), length);
        let frame = self.frame();
        Rect::new(x0, self.row_y(note.pitch) + frame, (x1 - x0).max(1), self.row_h(note.pitch) - frame)
    }

    fn off(&self, at: (i32, i32)) -> bool {
        !Rect::new(0, 0, self.w, self.h).contains(at.0, at.1)
    }

    fn hit(&self, x: i32, y: i32, clip: &Clip) -> Hit {
        if self.left_cell(0).contains(x, y) {
            return Hit::Time;
        }
        if let Some((_, hit)) = self.header().into_iter().chain(self.note_cells()).find(|(cell, _)| cell.contains(x, y)) {
            return hit;
        }
        if y < PITCH {
            return Hit::Nothing;
        }
        if x < KEY_X {
            return if self.big { Hit::Nothing } else { Hit::Zoom };
        }
        let Some(pitch) = self.pitch_at(y) else {
            return Hit::Nothing;
        };
        if x < KEYS {
            return Hit::Key(pitch);
        }
        for note in clip.notes.iter().filter(|note| note.pitch == pitch && shown(note, clip.beats())) {
            let rect = self.note_rect(note, clip.beats());
            if x < rect.x - FRAME || x >= rect.x + rect.w {
                continue;
            }
            let (x, grab) = (x.max(rect.x), handle_w(rect.w));
            return if x >= rect.x + rect.w - grab {
                Hit::End(key(note))
            } else if x < rect.x + grab {
                Hit::Start(key(note))
            } else {
                Hit::Note(key(note))
            };
        }
        match self.ghost_at(x, pitch, clip.beats()) {
            Some(at) => Hit::Ghost(at),
            None => Hit::Cell,
        }
    }

    fn ghost_at(&self, x: i32, pitch: u8, length: f32) -> Option<usize> {
        self.ghosts.iter().position(|ghost| {
            if ghost.note.pitch != pitch || !shown(&ghost.note, length) {
                return false;
            }
            let rect = self.note_rect(&ghost.note, length);
            x >= rect.x - FRAME && x < rect.x + rect.w
        })
    }

    pub fn hover(&mut self, x: i32, y: i32, clip: &Clip) -> bool {
        let hit = self.hit(x, y, clip);
        let changed = hit != self.hover;
        self.hover = hit;
        changed
    }

    pub fn leave(&mut self) -> bool {
        let changed = self.hover != Hit::Nothing;
        self.hover = Hit::Nothing;
        changed
    }

    pub fn down(&mut self, x: i32, y: i32, clicks: i32, shift: bool, clip: &mut Clip, room: i32) -> (bool, Option<Action>) {
        let hit = self.hit(x, y, clip);
        self.hover = hit;
        match hit {
            Hit::Save => (false, Some(Action::Save)),
            Hit::Fit => (false, Some(Action::Fit)),
            Hit::Chords => (self.chordify(clip), None),
            Hit::Double => (self.double(clip, room), None),
            Hit::Halve => (self.halve(clip), None),
            Hit::Reverse => (self.reverse(clip), None),
            Hit::Ghost(at) if !shift => (false, Some(Action::Ghost(self.ghosts[at]))),
            Hit::Ghost(_) => (self.press(Hit::Cell, x, y, clicks, shift, clip), None),
            _ => (self.press(hit, x, y, clicks, shift, clip), None),
        }
    }

    fn press(&mut self, hit: Hit, x: i32, y: i32, clicks: i32, shift: bool, clip: &mut Clip) -> bool {
        match hit {
            Hit::Zoom => {
                if clicks >= 2 {
                    return self.zoom_to(ZOOM_DEFAULT, self.focal(), 0);
                }
                self.drag = Some(Drag::Zoom(Zoom { origin: (x, y), row: self.row, focal: self.focal() }));
                true
            }
            Hit::Time => {
                let centre = self.centre_beat(clip.beats());
                if clicks >= 2 {
                    return self.set_time(TIME_MIN, centre, clip.beats());
                }
                self.drag = Some(Drag::Time(TimeZoom { origin: (x, y), at_x: x, time: self.beat_w(clip.beats()), centre }));
                true
            }
            Hit::Big => {
                self.centre = Some(self.pitch_at(PITCH + self.visible() / 2).unwrap_or(60));
                self.big = !self.big;
                true
            }
            Hit::Guide => {
                self.guide = !self.guide;
                true
            }
            Hit::Preview => {
                self.preview = !self.preview;
                true
            }
            Hit::Up => self.shift_octave(clip, OCTAVE),
            Hit::Down => self.shift_octave(clip, -OCTAVE),
            Hit::Key(pitch) => {
                self.sounding = Some(pitch);
                self.drag = Some(Drag::Key);
                true
            }
            Hit::Note(pressed) | Hit::Start(pressed) | Hit::End(pressed) => {
                let Some(anchor) = clip.notes.iter().copied().find(|note| key(note) == pressed) else {
                    return false;
                };
                if clicks >= 2 {
                    clip.notes.retain(|note| key(note) != pressed);
                    let note = Note { length: (snap_after(anchor.start, self.snap) - anchor.start).min((clip.beats() - anchor.start).max(TOUCH)), ..anchor };
                    clip.place_note(note);
                    self.picked = vec![key(&note)];
                    return true;
                }
                if shift {
                    self.sweep_from(x, y, Some(pressed));
                    return false;
                }
                if !self.picked.contains(&pressed) {
                    self.picked = vec![pressed];
                }
                let (notes, rest): (Vec<Note>, Vec<Note>) = clip.notes.iter().partition(|note| self.picked.contains(&key(note)));
                self.sounding = Some(anchor.pitch);
                let side = match hit {
                    Hit::Start(_) => Some(Side::Start),
                    Hit::End(_) => Some(Side::End),
                    _ => None,
                };
                self.drag = Some(match side {
                    Some(side) => Drag::Resize(Stretch { side, anchor, notes, rest, origin_x: x, armed: false }),
                    None => Drag::Move(Shift { anchor, notes, rest, origin: (x, y), at: (x, y), armed: false }),
                });
                true
            }
            Hit::Tie => self.cycle_join(clip, shift),
            Hit::Dial(dial) => {
                if self.picked.is_empty() {
                    return false;
                }
                if shift || clicks >= 2 {
                    return self.set_picked(clip, |note| dial.reset(note));
                }
                let from = clip.notes.iter().filter(|note| self.picked.contains(&key(note))).map(|note| (key(note), dial.of(note).unwrap_or(0.0))).collect();
                self.drag = Some(Drag::Dial { dial, origin_y: y, from });
                true
            }
            Hit::Cell => {
                if shift {
                    self.sweep_from(x, y, None);
                    return false;
                }
                let Some(pitch) = self.pitch_at(y) else {
                    return false;
                };
                let start = snap_floor(self.beat_at(x, clip.beats()), self.snap).clamp(0.0, snap_before(clip.beats(), self.snap));
                let length = (snap_after(start, self.snap) - start).min(clip.beats() - start);
                let note = Note { pitch, start, length, velocity: VELOCITY, articulation: Articulation::PLAIN };
                let rest = clip.notes.clone();
                self.lay(clip, &rest, &[note]);
                self.sounding = Some(pitch);
                self.drag = Some(Drag::Draw { note, rest, origin_x: x, armed: false });
                true
            }
            Hit::Nothing | Hit::Save | Hit::Fit | Hit::Chords | Hit::Double | Hit::Halve | Hit::Reverse | Hit::Ghost(_) => false,
        }
    }

    pub fn zoom_at(&mut self, x: i32, y: i32, clip: &Clip) -> bool {
        if !self.grid_rect().contains(x, y) {
            return false;
        }
        self.drag = Some(Drag::TimeAt(TimeAt { origin: (x, y), time: self.beat_w(clip.beats()), beat: self.beat_at(x, clip.beats()) }));
        true
    }

    pub fn tap(&mut self, x: i32, y: i32, clip: &mut Clip) -> bool {
        self.press(Hit::Cell, x, y, 1, false, clip);
        self.up(x, y, clip)
    }

    fn sweep_from(&mut self, x: i32, y: i32, hit: Option<NoteKey>) {
        self.drag = Some(Drag::Marquee(Marquee { origin: (x, y), at: (x, y), armed: false, hit }));
    }

    fn lift_copies(&self, marquee: &Marquee, x: i32, y: i32, clip: &Clip) -> Option<Shift> {
        let pressed = marquee.hit.filter(|_| !marquee.armed)?;
        let reached = (x - marquee.origin.0).abs() >= ARM || (y - marquee.origin.1).abs() >= ARM;
        let anchor = clip.notes.iter().copied().find(|note| reached && key(note) == pressed)?;
        let notes = if self.picked.contains(&pressed) { self.copy_picked(clip) } else { vec![anchor] };
        Some(Shift { anchor, notes, rest: clip.notes.clone(), origin: marquee.origin, at: marquee.origin, armed: false })
    }

    pub fn moved(&mut self, x: i32, y: i32, clip: &mut Clip) -> bool {
        let Some(mut drag) = self.drag.take() else {
            return self.hover(x, y, clip);
        };
        if let Drag::Marquee(marquee) = &drag
            && let Some(copies) = self.lift_copies(marquee, x, y, clip)
        {
            drag = Drag::Move(copies);
        }
        let changed = match &mut drag {
            Drag::Zoom(zoom) => self.zoom_to(zoom.row + (x - zoom.origin.0) / ZOOM_PX_PER_ROW, zoom.focal, y - zoom.origin.1),
            Drag::Time(lens) => {
                lens.at_x = x;
                let time = lens.time * ((y - lens.origin.1) as f32 / grid::ZOOM_PX_PER_DOUBLE as f32).exp2();
                let centre = lens.centre;
                self.set_time(time, centre, clip.beats())
            }
            Drag::TimeAt(lens) => {
                let time = lens.time * ((y - lens.origin.1) as f32 / grid::ZOOM_PX_PER_DOUBLE as f32).exp2();
                let (beat, at) = (lens.beat, (lens.origin.0 - KEYS) as f32);
                self.set_time_under(time, beat, at, clip.beats())
            }
            Drag::Key => {
                let pitch = self.pitch_at(y);
                let changed = pitch != self.sounding;
                self.sounding = pitch;
                changed
            }
            Drag::Draw { note, rest, origin_x, armed } => {
                *armed |= (x - *origin_x).abs() >= ARM;
                if *armed {
                    let end = snap_round(self.beat_at(x, clip.beats()), self.snap).clamp(snap_after(note.start, self.snap).min(clip.beats()), clip.beats());
                    let changed = end - note.start != note.length;
                    note.length = end - note.start;
                    if changed {
                        self.lay(clip, rest, std::slice::from_ref(note));
                    }
                    changed
                } else {
                    false
                }
            }
            Drag::Move(shift) => {
                shift.at = (x, y);
                shift.armed |= (x - shift.origin.0).abs() >= ARM || (y - shift.origin.1).abs() >= ARM;
                if shift.armed {
                    let (beats, rows) = self.shift_of(shift, clip);
                    let laid: Vec<Note> = if self.off((x, y)) { Vec::new() } else { shift.notes.iter().map(|note| shifted(note, beats, rows)).collect() };
                    self.sounding = Some((i32::from(shift.anchor.pitch) + rows) as u8);
                    self.lay(clip, &shift.rest, &laid);
                }
                shift.armed
            }
            Drag::Resize(stretch) => {
                stretch.armed |= (x - stretch.origin_x).abs() >= ARM;
                if stretch.armed {
                    let delta = self.stretch_of(stretch, x, clip.beats());
                    let resized: Vec<Note> = stretch.notes.iter().map(|note| stretched(note, stretch.side, delta, clip.beats(), self.snap)).collect();
                    self.lay(clip, &stretch.rest, &resized);
                }
                stretch.armed
            }
            Drag::Dial { dial, origin_y, from } => {
                let delta = f64::from(*origin_y - y) * dial.per_px();
                let mut changed = false;
                for (picked, start) in from.iter() {
                    if let Some(note) = clip.notes.iter_mut().find(|note| key(note) == *picked) {
                        let before = *note;
                        dial.set(note, start + delta);
                        changed |= *note != before;
                    }
                }
                changed
            }
            Drag::Marquee(marquee) => {
                marquee.at = (x, y);
                marquee.armed |= (x - marquee.origin.0).abs() >= ARM || (y - marquee.origin.1).abs() >= ARM;
                if marquee.armed {
                    let sweep = grid::marquee(marquee.origin, marquee.at).intersect(self.grid_rect());
                    self.picked = clip.notes.iter().filter(|note| shown(note, clip.beats()) && self.note_rect(note, clip.beats()).overlaps(sweep)).map(key).collect();
                }
                marquee.armed
            }
        };
        self.drag = Some(drag);
        changed
    }

    pub fn up(&mut self, x: i32, y: i32, clip: &mut Clip) -> bool {
        self.sounding = None;
        let Some(drag) = self.drag.take() else {
            return false;
        };
        match drag {
            Drag::Move(shift) if !shift.armed => self.picked = vec![key(&shift.anchor)],
            Drag::Marquee(Marquee { armed: false, hit: Some(pressed), .. }) => match self.picked.iter().position(|picked| *picked == pressed) {
                Some(at) => {
                    self.picked.remove(at);
                }
                None => self.picked.push(pressed),
            },
            _ => {}
        }
        self.hover = self.hit(x, y, clip);
        true
    }
}

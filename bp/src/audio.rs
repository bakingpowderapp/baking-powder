use crate::arp::{Articulation, Join, MidiRow, Runner, Sink, Transport};
use coresynth::Core;
use sdk::dsp::ears::{Ear, Heard};
use sdk::dsp::finite;
use crate::project::warp::{self, Beats, Warped};
use fx::{Beat, Clock, Effect, params::FREAK_BANDS};
use crate::flatten::Sent;
use crate::metronome::Metronome;
use crate::project::{Aim, Automation, BUSES, Clip, Curve, DEFAULT_BPM, Fades, File, Host, LaneLfo, Layer, Node, Project, RACK_ROWS, RackTiming, Row, SEND_COUNT, SongNote, Span, TRACKS, Timing, Track, TrackTiming, beats, cap, copies, moved_index, lingering, places, places_with, placing, played, position_to_gain, source_of};
use sdk::spectrum::{Analyzer, SCOPES, Scope, Spectrum, Tap, Target};
use crate::wav::{self, Wav, Wave};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, Instant};

mod control;
pub mod duck;
mod engine;
pub mod vision;
mod render;

use engine::*;
use render::*;
pub use control::Audio;
pub use engine::{Demote, Workers};
pub use render::Renderer;
use vision::{Capture, Recipe, Rendered, Vision};
pub use vision::Seer;

const RECLAIM: usize = 8192;
const STRIP_PARTS: usize = 11;
const RETIRED_BY_ONE: usize = TRACKS * (RACK_ROWS + STRIP_PARTS) + (SEND_COUNT + 1) * RACK_ROWS + 1;
const STRAYS: usize = 3;
const BUDGET: usize = 2 * RECLAIM - STRAYS;
const PENDING: usize = 1024;
const BATCH: usize = PENDING - 2;
const _: () = assert!(RETIRED_BY_ONE <= BUDGET);
pub(crate) const EXPORT_SAMPLE_RATE: f64 = wav::SAMPLE_RATE as f64;
const EXPORT_BLOCK: usize = 4096;
const EXPORT_TAIL_SECONDS: f64 = 2.0;
const EXPORT_LONGEST_TAIL_SECONDS: f64 = 60.0;
const EXPORT_SILENCE: f32 = 1e-3;
const EXPORT_SPARED_CORES: usize = 2;
const PRINT_TAIL_SECONDS: f64 = 600.0;
const BEAT_EPSILON: f64 = 1e-6;
const FADE: f32 = 64.0;
const SPLICE: usize = FADE as usize;
const EVERY_CLASS: u16 = 0x0fff;
type Song = Vec<Vec<SongNote>>;

fn edge(k: usize) -> f32 {
    if k >= SPLICE { 0.0 } else { 0.5 + 0.5 * (std::f32::consts::PI * k as f32 / SPLICE as f32).cos() }
}

pub struct Rehearsal {
    pub tracks: Vec<usize>,
    pub notes: Vec<SongNote>,
    pub beats: f64,
}
type Curves = Vec<Vec<Curve>>;
type Lfos = Vec<Vec<LaneLfo>>;
type Lagged = Vec<Vec<Option<f64>>>;
const RETURNS: usize = SEND_COUNT + 1;

fn return_host(index: usize) -> Host {
    if index < SEND_COUNT { Host::Send(index) } else { Host::Master }
}
struct Gauges {
    rows: [AtomicU64; RACK_ROWS],
    held: [AtomicU32; RACK_ROWS],
    strips: [AtomicU64; RACK_ROWS],
    leaving: AtomicU64,
}

const BATCHES: u64 = u64::MAX >> RACK_ROWS;
const QUIET: f32 = 1e-6;

const BAND_METER_DB: f32 = 48.0;

pub(crate) struct Meter {
    left: AtomicU32,
    right: AtomicU32,
}

impl Meter {
    fn new() -> Meter {
        Meter { left: AtomicU32::new(0), right: AtomicU32::new(0) }
    }

    fn raise(&self, left: f32, right: f32) {
        self.left.fetch_max(left.to_bits(), Ordering::Relaxed);
        self.right.fetch_max(right.to_bits(), Ordering::Relaxed);
    }

    fn take(&self) -> (f32, f32) {
        (f32::from_bits(self.left.swap(0, Ordering::Relaxed)), f32::from_bits(self.right.swap(0, Ordering::Relaxed)))
    }
}

fn gauges() -> Gauges {
    Gauges { rows: std::array::from_fn(|_| AtomicU64::new(0)), held: std::array::from_fn(|_| AtomicU32::new(0)), strips: std::array::from_fn(|_| AtomicU64::new(0)), leaving: AtomicU64::new(0) }
}

impl Gauges {
    fn restart(&self, slot: usize, master: usize) {
        self.rows[slot].store(self.rows[master].load(Ordering::Relaxed), Ordering::Relaxed);
    }

    fn hold(&self, slot: usize, level: f32) {
        if level > 0.0 {
            self.held[slot].fetch_max(level.to_bits(), Ordering::Relaxed);
        }
    }

    fn take(&self, slot: usize) -> f32 {
        f32::from_bits(self.held[slot].swap(0, Ordering::Relaxed))
    }

    fn linger(&self, sounding: u64, batches: u64) {
        self.leaving.store(((batches & BATCHES) << RACK_ROWS) | sounding, Ordering::Relaxed);
    }

    fn lingering(&self, batches: u64) -> Option<u64> {
        let word = self.leaving.load(Ordering::Relaxed);
        (word >> RACK_ROWS == batches & BATCHES).then_some(word & !(BATCHES << RACK_ROWS))
    }
}

fn unpack_gauge(bits: u64) -> (f32, f32) {
    (f32::from_bits((bits >> 32) as u32), f32::from_bits(bits as u32))
}

type Strips = [[f32; 2]; FREAK_BANDS];

const NO_STRIPS: [Strips; RACK_ROWS] = [[[0.0; 2]; FREAK_BANDS]; RACK_ROWS];

fn unpack_strips(bits: u64) -> Strips {
    let db = |word: u64| f32::from(word as u8) / f32::from(u8::MAX) * BAND_METER_DB;
    std::array::from_fn(|band| {
        let word = bits >> (16 * (FREAK_BANDS - 1 - band));
        [db(word >> 8), db(word) - BAND_METER_DB]
    })
}

fn split_gauges(read: [(f32, f32); RACK_ROWS]) -> ([f32; RACK_ROWS], [f32; RACK_ROWS]) {
    (read.map(|(level, _)| level), read.map(|(_, signal)| signal))
}

struct Meters {
    tracks: Vec<Meter>,
    track_ears: Vec<Mutex<Option<Ear>>>,
    sends: [Meter; SEND_COUNT],
    send_ears: [Mutex<Option<Ear>>; SEND_COUNT],
    master: Meter,
    track_gauges: Vec<Gauges>,
    send_gauges: [Gauges; SEND_COUNT],
    master_gauges: Gauges,
    taps: [Tap; SCOPES],
    dropouts: AtomicU32,
    load: AtomicU32,
    lane: [AtomicU64; 2],
    sequenced: Vec<[[AtomicU64; 2]; RACK_ROWS]>,
    preview_progress: AtomicU64,
    rehearsal_beat: AtomicU64,
}

fn unpublished() -> AtomicU64 {
    AtomicU64::new(f64::NAN.to_bits())
}

fn publish(cell: &AtomicU64, value: f64) {
    cell.store(value.to_bits(), Ordering::Relaxed);
}

fn published(cell: &AtomicU64) -> Option<f64> {
    let value = f64::from_bits(cell.load(Ordering::Relaxed));
    value.is_finite().then_some(value)
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Lane {
    pub beats: f64,
    pub seconds: f64,
}

impl Lane {
    const UNCLOCKED: Lane = Lane { beats: f64::NAN, seconds: f64::NAN };

    fn cells(self) -> [AtomicU64; 2] {
        [AtomicU64::new(self.beats.to_bits()), AtomicU64::new(self.seconds.to_bits())]
    }

    fn store(self, cells: &[AtomicU64; 2]) {
        cells[0].store(self.beats.to_bits(), Ordering::Relaxed);
        cells[1].store(self.seconds.to_bits(), Ordering::Relaxed);
    }

    fn load(cells: &[AtomicU64; 2]) -> Lane {
        Lane { beats: f64::from_bits(cells[0].load(Ordering::Relaxed)), seconds: f64::from_bits(cells[1].load(Ordering::Relaxed)) }
    }

    fn clocked(self) -> Option<Lane> {
        (self.beats.is_finite() && self.seconds.is_finite()).then_some(self)
    }
}

impl Meters {
    fn gauges(&self, host: Host) -> &Gauges {
        match host {
            Host::Track(track) => &self.track_gauges[track.min(TRACKS - 1)],
            Host::Send(send) => &self.send_gauges[send.min(SEND_COUNT - 1)],
            Host::Master => &self.master_gauges,
        }
    }

    fn read(&self, host: Host, rack: &[Row]) -> [(f32, f32); RACK_ROWS] {
        let gauges = self.gauges(host);
        std::array::from_fn(|slot| {
            let (level, signal) = unpack_gauge(gauges.rows[slot].load(Ordering::Relaxed));
            let held = matches!(rack.get(slot), Some(Row::Effect(effect)) if effect.kind().meters_reduction());
            (if held { gauges.take(slot) } else { level }, signal)
        })
    }

    fn read_strips(&self, host: Host) -> [Strips; RACK_ROWS] {
        let gauges = self.gauges(host);
        std::array::from_fn(|slot| unpack_strips(gauges.strips[slot].load(Ordering::Relaxed)))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoreSolo {
    pub row: usize,
    pub effects: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Alone {
    rows: (usize, usize),
    effects: bool,
}

impl Alone {
    fn of(solo: CoreSolo, places: &[usize]) -> Alone {
        let rows = copies(places, solo.row);
        Alone { rows: (rows.start, rows.end), effects: solo.effects }
    }

    fn holds(self, row: usize) -> bool {
        (self.rows.0..self.rows.1).contains(&row)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Warm(usize);

impl Warm {
    const SETTLED: Warm = Warm(0);

    fn of(latency: usize) -> Warm {
        if latency > 0 { Warm(latency + SPLICE) } else { Warm::SETTLED }
    }

    fn apply(&mut self, l: &mut [f32], r: &mut [f32]) {
        let Warm(cold) = *self;
        if cold == 0 {
            return;
        }
        let n = l.len().min(r.len());
        for (i, (l, r)) in l.iter_mut().zip(r.iter_mut()).take(cold).enumerate() {
            let left = cold - i;
            let gain = if left > SPLICE { 0.0 } else { 1.0 - edge(SPLICE - left) };
            *l *= gain;
            *r *= gain;
        }
        *self = Warm(cold.saturating_sub(n));
    }
}

pub(crate) fn lane_of(layer: &Layer, place: usize) -> u64 {
    if layer.seed == 0 { place as u64 } else { layer.seed }
}

pub(crate) enum Insert {
    Midi { row: MidiRow, runner: Box<Runner>, stream: Box<Stream> },
    Core { layer: Box<Layer>, core: Box<Core>, stream: Box<Stream>, l: Vec<f32>, r: Vec<f32>, pass: u64 },
    Effect { slot: fx::Slot, unit: Effect, vision: Option<Box<Vision>>, capture: Option<Capture>, warm: Warm, twin: Option<Box<Twin>> },
    Bus { levels: [f32; BUSES], master: usize, l: Vec<f32>, r: Vec<f32> },
}

pub(crate) struct Twin {
    unit: Effect,
    warm: Warm,
    level: f32,
    vision: Option<Box<Vision>>,
}

impl Twin {
    fn new(slot: &fx::Slot, sample_rate: f64, max_frames: usize) -> Twin {
        Twin { unit: Effect::new(slot.kind(), sample_rate, max_frames), warm: Warm::SETTLED, level: 0.0, vision: None }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Region {
    from: f64,
    to: f64,
    gain: f32,
}

pub struct Cue {
    pub wave: Arc<Wave>,
    pub warp: Option<Warped>,
    pub once: bool,
}

#[derive(Clone)]
pub(crate) struct Play {
    start: f64,
    end: f64,
    anchor: f64,
    ratio: f64,
    stop: f64,
    gain: f32,
    wave: Arc<Wave>,
    regions: Vec<Region>,
    fades: Fades,
    warp: Option<Warped>,
}

impl PartialEq for Play {
    fn eq(&self, other: &Play) -> bool {
        self.start == other.start && self.end == other.end && self.anchor == other.anchor && self.ratio == other.ratio && self.stop == other.stop && self.gain == other.gain && Arc::ptr_eq(&self.wave, &other.wave) && self.regions == other.regions && self.fades == other.fades && self.warp == other.warp
    }
}

impl Insert {
    fn new(row: Row, sample_rate: f64, max_frames: usize, twinned: bool) -> Insert {
        match row {
            Row::Midi(midi) => Insert::Midi { row: midi, runner: Box::new(Runner::new(midi.heard())), stream: Box::new(Stream::new(max_frames)) },
            Row::Core(layer) => Insert::Core { layer: Box::new(layer), core: Box::new(Core::new(sample_rate, layer.patch)), stream: Box::new(Stream::new(max_frames)), l: vec![0.0; max_frames], r: vec![0.0; max_frames], pass: 0 },
            Row::Effect(slot) => Insert::Effect { slot, unit: Effect::new(slot.kind(), sample_rate, max_frames), vision: None, capture: None, warm: Warm::SETTLED, twin: twinned.then(|| Box::new(Twin::new(&slot, sample_rate, max_frames))) },
            Row::Bus(bus) => Insert::Bus { levels: bus.levels.map(|level| level as f32), master: bus.master, l: vec![0.0; max_frames], r: vec![0.0; max_frames] },
        }
    }

    fn wake(&mut self, sample_rate: f64) {
        if let Insert::Effect { slot, warm, twin, .. } = self {
            *warm = Warm::of(slot.latency(sample_rate));
            if let Some(twin) = twin {
                twin.warm = *warm;
            }
        }
    }

    fn set(&mut self, row: Row, sample_rate: f64) -> [Option<Box<Vision>>; 2] {
        match (self, row) {
            (Insert::Midi { row, runner, .. }, Row::Midi(next)) => {
                *row = next;
                runner.stage(next.heard());
                [None, None]
            }
            (Insert::Core { layer, core, .. }, Row::Core(next)) => {
                **layer = next;
                core.set_params(next.patch);
                [None, None]
            }
            (Insert::Effect { slot, vision, warm, twin, .. }, Row::Effect(next)) => {
                if slot.bypass && !next.bypass {
                    *warm = Warm::of(next.latency(sample_rate));
                    if let Some(twin) = twin.as_deref_mut() {
                        twin.warm = *warm;
                    }
                }
                *slot = next;
                if slot.reverse_band().is_some() { [None, None] } else { [vision.take(), twin.as_deref_mut().and_then(|twin| twin.vision.take())] }
            }
            (Insert::Bus { levels, master, .. }, Row::Bus(next)) => {
                *levels = next.levels.map(|level| level as f32);
                *master = next.master;
                [None, None]
            }
            _ => [None, None],
        }
    }

    fn renew(&mut self, fresh: &mut Insert, sample_rate: f64) -> [Option<Box<Vision>>; 2] {
        let (Insert::Effect { unit, twin, .. }, Insert::Effect { slot, unit: clean, twin: spare, .. }) = (&mut *self, fresh) else {
            return [None, None];
        };
        if !unit.holding() {
            std::mem::swap(unit, clean);
        }
        if let (Some(twin), Some(spare)) = (twin.as_deref_mut(), spare.as_deref_mut())
            && !twin.unit.holding()
        {
            std::mem::swap(&mut twin.unit, &mut spare.unit);
        }
        let stale = self.set(Row::Effect(*slot), sample_rate);
        self.wake(sample_rate);
        stale
    }
}

pub(crate) enum Message {
    Insert { at: usize, rack: Vec<Insert>, strip: Strip },
    Remove { at: usize },
    Move { from: usize, to: usize },
    Clear,
    Row { host: Host, index: usize, row: Row },
    RenewRow { host: Host, index: usize, insert: Insert },
    InsertRow { host: Host, index: usize, insert: Insert, joins: bool },
    RemoveRow { host: Host, index: usize },
    MoveRow { host: Host, from: usize, to: usize },
    Vision { host: Host, index: usize, vision: Box<Vision>, twin: bool },
    Forget { host: Host, index: usize },
    Strip { track: usize, gain: f32, heard: bool, pours: bool, unmuted: bool, group: Option<usize> },
    Twins { track: usize, lines: Option<Box<TwinLines>>, units: Vec<Option<Box<Twin>>> },
    Sends { track: usize, amounts: [f32; SEND_COUNT] },
    Master { gain: f32 },
    Return { send: usize, gain: f32 },
    SendSolo { send: Option<usize> },
    CoreSolo { track: usize, solo: Option<Alone> },
    Plays { track: usize, plays: Vec<Play> },
    Preview(Option<Box<Cue>>),
    PreviewGain { gain: f32 },
    Rehearse(Option<Box<Rehearsal>>),
    NoteOn { track: usize, midi: u8, velocity: u8 },
    NoteOff { track: usize, midi: u8 },
    Song(Box<Song>),
    Leaders { leaders: [u8; TRACKS], order: [u8; TRACKS] },
    Curves(Box<Curves>),
    Lfos(Box<Lfos>, Lagged),
    ReturnCurves(Box<Curves>),
    ReturnLfos(Box<Lfos>, Lagged),
    Tempo { bpm: f64 },
    Key { bits: u16 },
    Metronome { on: bool },
    Play { from: f64 },
    Export { from: f64, lead: usize, song: usize },
    Measure { song: usize },
    Place { beat: f64 },
    Stop,
    Loop { range: Option<(f64, f64)> },
    Timing(Box<Plan>),
    Begin,
    Split,
    Commit,
}

impl Message {
    fn retires(&self) -> usize {
        match self {
            Message::Clear => RETIRED_BY_ONE,
            Message::Insert { .. } => RACK_ROWS + STRIP_PARTS + 1,
            Message::Remove { .. } => RACK_ROWS + STRIP_PARTS,
            Message::Timing(_) => TRACKS + 1,
            Message::Twins { .. } => RACK_ROWS + 2,
            Message::InsertRow { .. } | Message::RemoveRow { .. } | Message::Vision { .. } | Message::Plays { .. } | Message::Preview(_) | Message::Rehearse(_) | Message::Song(_) | Message::Curves(_) | Message::ReturnCurves(_) => 1,
            Message::Row { .. } | Message::Forget { .. } | Message::Lfos(..) | Message::ReturnLfos(..) => 2,
            Message::RenewRow { .. } => 3,
            Message::Move { .. } | Message::MoveRow { .. } | Message::Strip { .. } | Message::Sends { .. } | Message::Master { .. } | Message::Return { .. } | Message::SendSolo { .. } | Message::CoreSolo { .. } | Message::PreviewGain { .. } | Message::NoteOn { .. } | Message::NoteOff { .. } | Message::Leaders { .. } | Message::Tempo { .. } | Message::Key { .. } | Message::Metronome { .. } | Message::Play { .. } | Message::Export { .. } | Message::Measure { .. } | Message::Place { .. } | Message::Stop | Message::Loop { .. } | Message::Begin | Message::Split | Message::Commit => 0,
        }
    }
}

enum Displaced {
    Song(Box<Song>),
    Curves(Box<Curves>),
    Lfos(Box<Lfos>),
    Lagged(Lagged),
    Rack(Vec<Insert>),
    Buffer(Vec<f32>),
    Insert(Insert),
    Plays(Vec<Play>),
    Preview(Box<Cue>),
    Rehearsal(Box<Rehearsal>),
    Vision(Box<Vision>),
    Plan(Box<Plan>),
    Track(Box<TrackPlan>),
    Line(Line),
    TwinLines(Box<TwinLines>),
    TwinUnit(Box<Twin>),
    TwinUnits(Vec<Option<Box<Twin>>>),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Position {
    pub beat: f64,
    pub wraps: u16,
}

fn pack(beat: f64, wraps: u16, plays: u16) -> u64 {
    (u64::from((beat as f32).to_bits()) << 32) | (u64::from(wraps) << 16) | u64::from(plays)
}

pub(crate) fn frames_per_beat(sample_rate: f64, bpm: f64) -> f64 {
    sample_rate * 60.0 / bpm.max(1.0)
}

pub(crate) fn heard(project: &Project) -> Vec<bool> {
    let tracks = &project.tracks;
    let soloed = |index: usize| tracks[index].solo;
    let any_solo = (0..tracks.len()).any(soloed);
    let member_soloed = |group: usize| (0..tracks.len()).any(|index| soloed(index) && project.group_of(index) == Some(group));
    tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            let group = project.group_of(index);
            let muted = track.mute || group.is_some_and(|group| tracks[group].mute);
            !muted && (!any_solo || soloed(index) || group.is_some_and(soloed) || (track.group && member_soloed(index)))
        })
        .collect()
}

pub(crate) fn signalled(project: &Project) -> u64 {
    let count = project.tracks.len().min(TRACKS);
    let racks = project.tracks.iter().enumerate().take(count).map(|(index, track)| (index, &track.rack)).chain(project.sends.iter().map(|send| (TRACKS, &send.rack))).chain(std::iter::once((TRACKS, &project.master.rack)));
    racks.fold(0, |named, (index, rack)| {
        rack.iter().fold(named, |named, row| match row {
            Row::Effect(slot) => named | source_of(slot, index, count),
            _ => named,
        })
    })
}

pub(crate) fn pours(project: &Project, heard: &[bool], track: usize) -> bool {
    match project.group_of(track) {
        Some(group) if !heard[group] => !project.tracks[track].mute,
        _ => heard[track],
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tail {
    Cut,
    Rings,
}

#[derive(Clone, Copy)]
pub(crate) struct Reach {
    song: usize,
    hold: usize,
    most: usize,
}

impl Reach {
    pub(crate) fn of(bpm: f64, span: (i32, i32), tail: Tail) -> Option<Reach> {
        let (start, end) = span;
        if start < 0 || end <= start {
            return None;
        }
        let seconds = |seconds: f64| (EXPORT_SAMPLE_RATE * seconds).round() as usize;
        let song = (frames_per_beat(EXPORT_SAMPLE_RATE, bpm) * beats(end - start)).round() as usize;
        let (hold, longest) = match tail {
            Tail::Cut => (0, 0),
            Tail::Rings => (seconds(EXPORT_TAIL_SECONDS), seconds(EXPORT_LONGEST_TAIL_SECONDS)),
        };
        let most = song.saturating_add(longest).min(wav::MAX_FRAMES);
        Some(Reach { song: song.min(most), hold, most })
    }

    pub(crate) fn expected(self) -> usize {
        self.song.saturating_add(self.hold).min(self.most)
    }
}

pub struct Job<T> {
    counted: Arc<AtomicUsize>,
    expected: usize,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<Option<T>>>,
}

pub type Export = Job<Vec<u8>>;

impl Export {
    pub fn start(project: &Project, waves: &[Arc<Wave>], span: (i32, i32), tail: Tail) -> Option<Export> {
        let expected = Reach::of(project.bpm, span, tail)?.expected();
        let (project, waves) = (project.clone(), waves.to_vec());
        Job::spawn("bp-export", expected, move |counted, stopped| {
            let mut file = Wav::new(expected);
            render_span(&project, &waves, span, tail, None, &[], counted, stopped, None, |left, right| file.append(left, right), |_, _| {}, |_| {})?;
            Some(file.finish())
        })
    }
}

impl<T: Send + 'static> Job<T> {
    pub(crate) fn spawn(name: &str, expected: usize, work: impl FnOnce(&AtomicUsize, &AtomicBool) -> Option<T> + Send + 'static) -> Option<Job<T>> {
        let counted = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        let (counting, stopping) = (Arc::clone(&counted), Arc::clone(&stopped));
        let thread = std::thread::Builder::new().name(String::from(name)).spawn(move || work(&counting, &stopping)).ok()?;
        Some(Job { counted, expected, stopped, thread: Some(thread) })
    }

    pub fn progress(&self) -> f32 {
        (self.counted.load(Ordering::Relaxed) as f32 / self.expected.max(1) as f32).min(1.0)
    }

    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn cancel(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    pub fn take(&mut self) -> Option<T> {
        self.thread.take()?.join().ok().flatten()
    }
}

pub(crate) fn render_span(project: &Project, waves: &[Arc<Wave>], span: (i32, i32), tail: Tail, solo: Option<usize>, solos: &[Option<CoreSolo>], counted: &AtomicUsize, stopped: &AtomicBool, measuring: Option<usize>, mut heard: impl FnMut(&[f32], &[f32]), mut gauged: impl FnMut(&Audio, usize), finished: impl FnOnce(&Audio)) -> Option<()> {
    let Reach { song, hold, most } = Reach::of(project.bpm, span, tail)?;
    let (mut audio, mut renderer) = Audio::open(EXPORT_SAMPLE_RATE, EXPORT_BLOCK, Seer::Inline);
    renderer.patient();
    renderer.start_workers(std::thread::available_parallelism().map_or(0, |cores| cores.get().saturating_sub(EXPORT_SPARED_CORES)), || None);
    audio.sync(project, solo, solos, waves);
    let timing = audio.timing();
    let shifts: Vec<(isize, isize)> = (0..timing.tracks.len()).map(|track| shift(timing, project, track)).collect();
    let early = shifts.iter().map(|&(_, latest)| latest).fold(0, isize::max).unsigned_abs();
    let output = timing.output;
    let lead = early + output;
    audio.link.send(Message::Metronome { on: false });
    if let Some(bar) = measuring {
        audio.link.send(Message::Measure { song });
        audio.hear(EXPORT_SAMPLE_RATE, bar, project.tracks.len());
    }
    if shifts.iter().any(|&shift| shift != (0, 0)) {
        audio.link.send(Message::Export { from: beats(span.0), lead: early, song });
    } else {
        audio.play(beats(span.0));
    }
    let (mut left, mut right) = (vec![0.0; EXPORT_BLOCK], vec![0.0; EXPORT_BLOCK]);
    let (mut rendered, mut done, mut quiet) = (0, 0, 0);
    while done < most && (done < song || quiet < hold) {
        if stopped.load(Ordering::Relaxed) {
            return None;
        }
        if rendered == early + song {
            audio.stop();
        }
        let n = EXPORT_BLOCK.min(match rendered.checked_sub(early) {
            None => early - rendered,
            Some(played) if played < song => song - played,
            Some(played) if played < most => most - played,
            Some(played) => most + output - played,
        });
        renderer.render(&mut left[..n], &mut right[..n]);
        rendered += n;
        let mut at = n.min(lead.saturating_sub(rendered - n));
        while at < n && done < most && (done < song || quiet < hold) {
            let take = (n - at).min(if done < song { song - done } else { most - done });
            let (l, r) = (&mut left[at..at + take], &mut right[at..at + take]);
            if tail == Tail::Cut {
                fade_out(l, r, song - done);
            }
            heard(l, r);
            done += take;
            counted.store(done, Ordering::Relaxed);
            quiet = if l.iter().chain(r.iter()).any(|sample| sample.abs() > EXPORT_SILENCE) { 0 } else { quiet + take };
            at += take;
        }
        gauged(&audio, done);
    }
    finished(&audio);
    Some(())
}

pub fn sent(project: &Project) -> Sent {
    let (mut audio, mut renderer) = Audio::open(EXPORT_SAMPLE_RATE, EXPORT_BLOCK, Seer::Blind);
    renderer.patient();
    renderer.print();
    audio.sync(project, None, &[], &[]);
    let timing = audio.timing();
    let shifts: Vec<(isize, isize)> = (0..timing.tracks.len()).map(|track| shift(timing, project, track)).collect();
    let early = shifts.iter().map(|&(_, latest)| latest).fold(0, isize::max).unsigned_abs();
    let song = (frames_per_beat(EXPORT_SAMPLE_RATE, project.bpm) * beats(project.end())).round() as usize;
    let most = song + (EXPORT_SAMPLE_RATE * PRINT_TAIL_SECONDS) as usize;
    audio.link.send(Message::Metronome { on: false });
    if shifts.iter().any(|&shift| shift != (0, 0)) {
        audio.link.send(Message::Export { from: 0.0, lead: early, song: most });
    } else {
        audio.play(0.0);
    }
    let (mut left, mut right) = (vec![0.0; EXPORT_BLOCK], vec![0.0; EXPORT_BLOCK]);
    let mut rendered = 0;
    while rendered < early + most && (rendered < early + song || !renderer.resting()) {
        renderer.render(&mut left, &mut right);
        rendered += EXPORT_BLOCK;
    }
    Sent::of(project, renderer.printed())
}

fn fade_out(left: &mut [f32], right: &mut [f32], to_end: usize) {
    for (i, (l, r)) in left.iter_mut().zip(right).enumerate() {
        let remaining = to_end - i;
        if remaining <= SPLICE {
            let gain = remaining as f32 / FADE;
            *l *= gain;
            *r *= gain;
        }
    }
}

pub fn bounce(name: String, clips: &[Clip], waves: &[Arc<Wave>], bpm: f64, start: i32, end: i32) -> Option<(Wave, f64)> {
    let track = Track { clips: clips.to_vec(), ..Track::audio() };
    let plays = control::plays(&track, waves, bpm, &mut Vec::new());
    let rate = plays.first()?.wave.rate;
    let channels = if plays.iter().any(|play| play.wave.channels == 2) { 2 } else { 1 };
    let frames_per_beat = frames_per_beat(rate, bpm);
    let frames = (beats(end - start) * frames_per_beat).round() as usize;
    if frames == 0 || frames > rate as usize * wav::LONGEST_SECONDS {
        return None;
    }
    let opening = plays.iter().any(|play| play.start == beats(start));
    let closing = plays.iter().any(|play| play.end == beats(end));
    let faded = |i: usize| {
        let mut fade = 1.0;
        if opening && i < SPLICE {
            fade *= (i + 1) as f32 / FADE;
        }
        if closing && frames - i <= SPLICE {
            fade *= (frames - i) as f32 / FADE;
        }
        fade
    };
    let (mut left, mut right) = (vec![0.0; EXPORT_BLOCK], vec![0.0; EXPORT_BLOCK]);
    let mut scale = 1.0f32;
    loop {
        let mut pcm = Vec::with_capacity(frames * channels);
        let mut peak = 0.0f32;
        let mut done = 0;
        while done < frames {
            let n = EXPORT_BLOCK.min(frames - done);
            left[..n].fill(0.0);
            right[..n].fill(0.0);
            let beat = beats(start) + done as f64 / frames_per_beat;
            pour(&plays, beat, Ramp::Up(f64::from(FADE)), frames_per_beat, rate, &mut left[..n], &mut right[..n]);
            for (at, (l, r)) in left[..n].iter().zip(&right[..n]).enumerate() {
                let fade = faded(done + at);
                let (l, r) = (l / fade * scale, r / fade * scale);
                peak = peak.max(l.abs()).max(r.abs());
                pcm.push(wav::quantized(l));
                if channels == 2 {
                    pcm.push(wav::quantized(r));
                }
            }
            done += n;
        }
        if peak <= 1.0 || scale < 1.0 {
            let level = if scale < 1.0 { (-20.0 * f64::from(scale).log10()).min(File::DECIBELS) } else { 0.0 };
            return Some((Wave::sound(name, rate, channels, pcm), level));
        }
        scale = 1.0 / peak;
    }
}
